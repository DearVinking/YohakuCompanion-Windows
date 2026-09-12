//! S3 兼容存储：SigV4 签名 PUT、公网 URL 构造、对象键规则。
//! 签名实现以 AWS 官方测试向量校验（见 tests）。

use crate::ports::HttpRequest;
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct S3Config {
    /// 自定义上传端点 host（如 "file.example.com"）；None 用 AWS 虚拟主机式
    pub endpoint: Option<String>,
    pub bucket: String,
    pub region: String,
    /// 公网访问域（优先于 endpoint 构造公网 URL）
    pub custom_domain: Option<String>,
    pub base_path: String,
    pub access_key: String,
    /// 仅运行期内存；持久化时经 SecretStore 单独存放（serde 跳过）
    #[serde(skip)]
    pub secret_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum S3Error {
    #[error("s3 config incomplete")]
    Incomplete,
    #[error("upload failed with status {0}")]
    UploadFailed(u16),
    #[error("transport error: {0}")]
    Transport(String),
}

impl S3Config {
    /// 具备发起上传的完整凭据。
    pub fn is_configured(&self) -> bool {
        !self.bucket.trim().is_empty()
            && !self.access_key.trim().is_empty()
            && !self.secret_key.trim().is_empty()
    }
}

const EMPTY_BODY_HASH: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// SigV4 的 URI 路径编码：保留 [A-Za-z0-9-._~] 与 '/'，其余 %XX 大写。
fn uri_encode_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for &b in path.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    use hmac::{Hmac, KeyInit, Mac};
    let mut mac =
        <Hmac<Sha256> as KeyInit>::new_from_slice(key).expect("hmac accepts any key length");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

/// 上传目标：PUT 到 `upload_url`，对外发布 `public_url`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UploadTarget {
    pub upload_url: String,
    pub public_url: String,
    pub object_key: String,
}

/// 公网基址：customDomain 优先，其次 AWS 虚拟主机式，再次 path-style endpoint。
pub fn public_base_url(cfg: &S3Config) -> String {
    if let Some(domain) = cfg
        .custom_domain
        .as_deref()
        .filter(|d| !d.trim().is_empty())
    {
        return format!("https://{}", domain.trim().trim_end_matches('/'));
    }
    match cfg.endpoint.as_deref().filter(|e| !e.trim().is_empty()) {
        Some(endpoint) => format!(
            "https://{}/{}/",
            endpoint.trim().trim_end_matches('/'),
            cfg.bucket
        ),
        None => format!("https://{}.s3.{}.amazonaws.com/", cfg.bucket, cfg.region),
    }
}

fn upload_host(cfg: &S3Config) -> String {
    match cfg.endpoint.as_deref().filter(|e| !e.trim().is_empty()) {
        Some(endpoint) => endpoint.trim().trim_end_matches('/').to_string(),
        None => format!("{}.s3.{}.amazonaws.com", cfg.bucket, cfg.region),
    }
}

fn normalized_base_path(cfg: &S3Config) -> String {
    let base = cfg.base_path.trim().trim_matches('/');
    if base.is_empty() {
        "app-icons".to_string()
    } else {
        base.to_string()
    }
}

/// 上传路径：自定义 endpoint 用 path-style（含 bucket 段）；
/// AWS 默认虚拟主机式的 bucket 已在 host 里。
fn upload_path(cfg: &S3Config, key: &str) -> String {
    let has_endpoint = cfg
        .endpoint
        .as_deref()
        .is_some_and(|e| !e.trim().is_empty());
    if has_endpoint {
        uri_encode_path(&format!("/{}/{}/", cfg.bucket, key.trim_start_matches('/')))
    } else {
        uri_encode_path(&format!("/{key}"))
    }
}

/// 应用图标对象键：`<base>/icons/<png_sha256>.png`。
pub fn application_icon_target(cfg: &S3Config, png_sha256: &str) -> UploadTarget {
    let base = normalized_base_path(cfg);
    let key = format!("{base}/icons/{png_sha256}.png");
    let public_base = public_base_url(cfg);
    UploadTarget {
        object_key: key.clone(),
        upload_url: format!("https://{}{}", upload_host(cfg), upload_path(cfg, &key)),
        public_url: format!(
            "{}{}",
            public_base.trim_end_matches('/'),
            uri_encode_path(&format!("/{key}"))
        ),
    }
}

/// 媒体封面对象键：`<base>/media-artwork/<sha256(deviceId)>/current.png`，
/// 公网 URL 带 `?v=<内容sha256>`（服务端/CDN 的缓存破坏参数）。
pub fn media_artwork_target(cfg: &S3Config, device_id: &str, content_sha256: &str) -> UploadTarget {
    let base = normalized_base_path(cfg);
    let device_hash = sha256_hex(device_id.as_bytes());
    let key = format!("{base}/media-artwork/{device_hash}/current.png");
    let public_base = public_base_url(cfg);
    UploadTarget {
        object_key: key.clone(),
        upload_url: format!("https://{}{}", upload_host(cfg), upload_path(cfg, &key)),
        public_url: format!(
            "{}{}?v={content_sha256}",
            public_base.trim_end_matches('/'),
            uri_encode_path(&format!("/{key}"))
        ),
    }
}

/// 构造 SigV4 签名的 PUT 请求（S3 单次 PUT，签名头为
/// content-type/host/x-amz-content-sha256/x-amz-date + extra）。
pub fn sign_put(
    cfg: &S3Config,
    target: &UploadTarget,
    body: &[u8],
    content_type: &str,
    extra_headers: &[(String, String)],
    now: DateTime<Utc>,
) -> Result<HttpRequest, S3Error> {
    sign_request("PUT", cfg, target, body, content_type, extra_headers, now)
}

/// SigV4 通用签名：canonical request =
/// `method / 上传路径 / 空 query / 排序规范头 / signed headers / payload hash`。
pub fn sign_request(
    method: &'static str,
    cfg: &S3Config,
    target: &UploadTarget,
    body: &[u8],
    content_type: &str,
    extra_headers: &[(String, String)],
    now: DateTime<Utc>,
) -> Result<HttpRequest, S3Error> {
    if !cfg.is_configured() {
        return Err(S3Error::Incomplete);
    }
    let secret_key = &cfg.secret_key;
    let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let date_stamp = now.format("%Y%m%d").to_string();
    let payload_hash = if body.is_empty() {
        EMPTY_BODY_HASH.to_string()
    } else {
        sha256_hex(body)
    };

    // 规范头（须按名称排序）
    let mut headers: Vec<(String, String)> = vec![
        ("host".to_string(), upload_host(cfg)),
        ("x-amz-content-sha256".to_string(), payload_hash.clone()),
        ("x-amz-date".to_string(), amz_date.clone()),
    ];
    if !content_type.is_empty() {
        headers.push(("content-type".to_string(), content_type.to_string()));
    }
    for (k, v) in extra_headers {
        headers.push((k.trim().to_lowercase(), v.trim().to_string()));
    }
    headers.sort_by(|a, b| a.0.cmp(&b.0));

    let signed_headers = headers
        .iter()
        .map(|(k, _)| k.as_str())
        .collect::<Vec<_>>()
        .join(";");
    let canonical_headers = headers
        .iter()
        .map(|(k, v)| format!("{k}:{v}\n"))
        .collect::<String>();

    let path = match target.upload_url.split_once("://") {
        Some((_, rest)) => match rest.find('/') {
            Some(idx) => &rest[idx..],
            None => "/",
        },
        None => "/",
    };
    let canonical_request =
        format!("{method}\n{path}\n\n{canonical_headers}\n{signed_headers}\n{payload_hash}");

    let scope = format!("{date_stamp}/{}/s3/aws4_request", cfg.region);
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{}",
        sha256_hex(canonical_request.as_bytes())
    );

    let k_date = hmac_sha256(
        format!("AWS4{secret_key}").as_bytes(),
        date_stamp.as_bytes(),
    );
    let k_region = hmac_sha256(&k_date, cfg.region.as_bytes());
    let k_service = hmac_sha256(&k_region, b"s3");
    let k_signing = hmac_sha256(&k_service, b"aws4_request");
    let signature = hex::encode(hmac_sha256(&k_signing, string_to_sign.as_bytes()));

    let authorization = format!(
        "AWS4-HMAC-SHA256 Credential={}/{}, SignedHeaders={}, Signature={}",
        cfg.access_key, scope, signed_headers, signature
    );

    let mut request_headers: Vec<(String, String)> = headers
        .iter()
        .map(|(k, v)| (title_case(k), v.clone()))
        .collect();
    request_headers.push(("Authorization".to_string(), authorization));
    request_headers.push(("x-amz-content-sha256".to_string(), payload_hash));

    Ok(HttpRequest {
        method,
        url: target.upload_url.clone(),
        headers: request_headers,
        body: Some(body.to_vec()),
        timeout_ms: 10_000,
    })
}

fn title_case(header: &str) -> String {
    let mut chars = header.chars();
    match chars.next() {
        Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
        None => String::new(),
    }
}

/// 对象键前缀清洗（base_path）供外部诊断使用。
pub fn effective_base_path(cfg: &S3Config) -> String {
    normalized_base_path(cfg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    // AWS SigV4 官方文档「GET Object」示例（含 x-amz-content-sha256 头的版本）：
    // https://docs.aws.amazon.com/AmazonS3/latest/API/sig-v4-header-based-auth.html
    #[test]
    fn aws_official_get_object_vector() {
        let cfg = S3Config {
            // 文档示例的 host 为 us-east-1 传统虚拟主机式（无 region 段）
            endpoint: Some("examplebucket.s3.amazonaws.com".into()),
            bucket: "examplebucket".into(),
            region: "us-east-1".into(),
            custom_domain: None,
            base_path: String::new(),
            access_key: "AKIAIOSFODNN7EXAMPLE".into(),
            secret_key: "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY".into(),
        };
        let target = UploadTarget {
            upload_url: "https://examplebucket.s3.amazonaws.com/test.txt".into(),
            public_url: "https://examplebucket.s3.amazonaws.com/test.txt".into(),
            object_key: "test.txt".into(),
        };
        let now = Utc.with_ymd_and_hms(2013, 5, 24, 0, 0, 0).unwrap();
        let request = sign_request(
            "GET",
            &cfg,
            &target,
            b"",
            "", // 官方向量无 content-type
            &[("range".into(), "bytes=0-9".into())],
            now,
        )
        .unwrap();
        assert_eq!(request.method, "GET");
        let auth = request
            .headers
            .iter()
            .find(|(k, _)| k == "Authorization")
            .unwrap();
        assert_eq!(
            auth.1,
            "AWS4-HMAC-SHA256 Credential=AKIAIOSFODNN7EXAMPLE/20130524/us-east-1/s3/aws4_request, \
SignedHeaders=host;range;x-amz-content-sha256;x-amz-date, \
Signature=f0e8bdb87c964420e857bd35b5d6ed310bd44f0170aba48dd91039c6036bdb41"
        );
    }

    fn cfg() -> S3Config {
        S3Config {
            endpoint: None,
            bucket: "mybucket".into(),
            region: "ap-east-1".into(),
            custom_domain: None,
            base_path: "app-icons".into(),
            access_key: "AK".into(),
            secret_key: "SK".into(),
        }
    }

    #[test]
    fn url_construction_branches() {
        // AWS 虚拟主机式
        let t = media_artwork_target(&cfg(), "device-1", &"ab".repeat(32));
        assert!(
            t.upload_url.starts_with(
                "https://mybucket.s3.ap-east-1.amazonaws.com/app-icons/media-artwork/"
            )
        );
        assert!(t.public_url.ends_with(&format!("?v={}", "ab".repeat(32))));

        // 自定义 endpoint → path-style 上传
        let mut custom = cfg();
        custom.endpoint = Some("file.example.com".into());
        let t = application_icon_target(&custom, "abc123");
        assert!(
            t.upload_url
                .starts_with("https://file.example.com/mybucket/app-icons/icons/abc123.png")
        );
        assert!(
            t.public_url
                .starts_with("https://file.example.com/mybucket/app-icons/icons/abc123.png")
        );

        // customDomain 优先于 endpoint 构造公网 URL
        custom.custom_domain = Some("cdn.example.com/".into());
        let t = application_icon_target(&custom, "abc123");
        assert!(
            t.public_url
                .starts_with("https://cdn.example.com/app-icons/icons/abc123.png")
        );
        assert!(
            t.upload_url
                .starts_with("https://file.example.com/mybucket/")
        );
    }

    #[test]
    fn key_uri_encoding() {
        assert_eq!(uri_encode_path("/test$file.text"), "/test%24file.text");
        assert_eq!(uri_encode_path("/a b/c.png"), "/a%20b/c.png");
        assert_eq!(uri_encode_path("/safe-._~x/y~z.png"), "/safe-._~x/y~z.png");
    }

    #[test]
    fn incomplete_config_rejected() {
        let mut c = cfg();
        c.bucket = String::new();
        let target = application_icon_target(&cfg(), "x");
        assert_eq!(
            sign_put(&c, &target, b"png", "image/png", &[], Utc::now()).unwrap_err(),
            S3Error::Incomplete
        );
    }
}
