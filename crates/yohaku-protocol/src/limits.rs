//! 字段限长与 URL 校验（协议安全边界）。

use std::collections::BTreeSet;
use std::fmt;

pub const MAX_URL_BYTES: usize = 2048;
pub const MAX_ACTIVITY_KEY: usize = 64;

pub fn unicode_scalar_len(s: &str) -> usize {
    s.chars().count()
}

/// `^[a-z][a-z0-9.-]{0,63}$`
pub fn valid_activity_key(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() => {}
        _ => return false,
    }
    s.chars().skip(1).all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '-')
        && s.len() <= MAX_ACTIVITY_KEY
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UrlRejection {
    NotHttps,
    HasUserinfo,
    HostNotAllowed,
    InvalidQuery,
    TooLong,
}

impl fmt::Display for UrlRejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            UrlRejection::NotHttps => "not https",
            UrlRejection::HasUserinfo => "userinfo not allowed",
            UrlRejection::HostNotAllowed => "host not allowed",
            UrlRejection::InvalidQuery => "query not allowed",
            UrlRejection::TooLong => "url too long",
        };
        f.write_str(text)
    }
}

/// https、无 userinfo、host 精确命中白名单、≤2048 字节、无控制字符与空白。
pub fn valid_public_https_url(
    url: &str,
    allowed_hosts: &BTreeSet<String>,
) -> Result<(), UrlRejection> {
    if url.len() > MAX_URL_BYTES {
        return Err(UrlRejection::TooLong);
    }
    if url.bytes().any(|b| b <= 0x20 || b == 0x7f || b == b'\\') {
        return Err(UrlRejection::NotHttps);
    }
    let rest = url.strip_prefix("https://").ok_or(UrlRejection::NotHttps)?;
    let authority_end = rest
        .find(['/', '?', '#'])
        .unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    if authority.contains('@') {
        return Err(UrlRejection::HasUserinfo);
    }
    let host = match authority.rsplit_once(':') {
        Some((host, port)) if !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) => host,
        _ => authority,
    };
    if !allowed_hosts.contains(host) {
        return Err(UrlRejection::HostNotAllowed);
    }
    Ok(())
}

/// 媒体封面前 URL：https、无 fragment、无 userinfo、≤2048 字节，
/// 且查询串必须恰为一个参数 `v=<64 位小写 hex>`（归一化 PNG 内容 sha256）。
pub fn valid_artwork_url(url: &str) -> Result<(), UrlRejection> {
    if url.len() > MAX_URL_BYTES {
        return Err(UrlRejection::TooLong);
    }
    if url.bytes().any(|b| b <= 0x20 || b == 0x7f || b == b'\\') {
        return Err(UrlRejection::NotHttps);
    }
    let rest = url.strip_prefix("https://").ok_or(UrlRejection::NotHttps)?;
    if rest.contains('#') {
        return Err(UrlRejection::NotHttps);
    }
    let authority_end = rest.find(['/', '?']).unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    if authority.contains('@') {
        return Err(UrlRejection::HasUserinfo);
    }
    let path_and_query = &rest[authority_end..];
    let Some((_path, query)) = path_and_query.split_once('?') else {
        return Err(UrlRejection::InvalidQuery);
    };
    if query.is_empty() || query.contains('&') {
        return Err(UrlRejection::InvalidQuery);
    }
    let Some((key, value)) = query.split_once('=') else {
        return Err(UrlRejection::InvalidQuery);
    };
    let value_is_content_hash = value.len() == 64
        && value.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    if key != "v" || !value_is_content_hash {
        return Err(UrlRejection::InvalidQuery);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hosts() -> BTreeSet<String> {
        BTreeSet::from(["assets.example.com".to_string()])
    }

    #[test]
    fn scalar_len_counts_chars_not_bytes() {
        assert_eq!(unicode_scalar_len("编辑中"), 3);
        assert_eq!(unicode_scalar_len("ab"), 2);
    }

    #[test]
    fn activity_keys() {
        assert!(valid_activity_key("com.apple.music"));
        assert!(valid_activity_key("a"));
        assert!(!valid_activity_key("1abc")); // 首字符必须小写字母
        assert!(!valid_activity_key("A-b"));
        assert!(!valid_activity_key("")); // 首字符必须存在
        assert!(!valid_activity_key(&"a".repeat(65)));
        assert!(valid_activity_key(&"a".repeat(64)));
    }

    #[test]
    fn url_accepts_whitelisted_https() {
        let h = hosts();
        assert!(valid_public_https_url("https://assets.example.com/icons/ab.png?v=deadbeef", &h).is_ok());
        assert!(valid_public_https_url("https://assets.example.com/", &h).is_ok());
        assert!(valid_public_https_url("https://assets.example.com:443/a.png", &h).is_ok());
    }

    #[test]
    fn url_rejections() {
        let h = hosts();
        assert_eq!(
            valid_public_https_url("http://assets.example.com/a.png", &h),
            Err(UrlRejection::NotHttps)
        );
        assert_eq!(
            valid_public_https_url("https://user:pass@assets.example.com/a.png", &h),
            Err(UrlRejection::HasUserinfo)
        );
        assert_eq!(
            valid_public_https_url("https://evil.example.com/a.png", &h),
            Err(UrlRejection::HostNotAllowed)
        );
        assert_eq!(
            valid_public_https_url(
                &format!("https://assets.example.com/{}", "a".repeat(2100)),
                &h
            ),
            Err(UrlRejection::TooLong)
        );
        // 反斜杠在浏览器里等价于斜杠，可被用来绕过 authority 解析
        assert!(valid_public_https_url("https://evil.example.com\\@assets.example.com/", &h).is_err());
        assert!(valid_public_https_url("https://assets.example.com/a b.png", &h).is_err());
    }

    #[test]
    fn artwork_urls() {
        let hash = "ab".repeat(32);
        let ok = format!("https://assets.example.com/m/current.png?v={hash}");
        assert!(valid_artwork_url(&ok).is_ok());
        assert!(valid_artwork_url("https://assets.example.com/m.png").is_err()); // 无 v
        assert!(valid_artwork_url(&format!("https://a.com/m.png?v={hash}&x=1")).is_err()); // 多参数
        assert!(valid_artwork_url("https://a.com/m.png?v=AB".to_string().as_str()).is_err()); // 非 64 hex
        let upper = format!("https://a.com/m.png?v={}", hash.to_uppercase());
        assert!(valid_artwork_url(&upper).is_err()); // 必须小写
        assert!(valid_artwork_url(&format!("https://a.com/m.png#x?v={hash}")).is_err()); // fragment
    }
}
