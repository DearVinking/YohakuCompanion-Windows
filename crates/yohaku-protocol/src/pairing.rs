//! 一次性配对码流程的请求/响应（macOS 版 CompanionPairingClient 语义）。

use crate::MAX_SAFE_INTEGER;
use crate::ids::is_valid_identifier;
use crate::json::{take_required, to_sorted_json};

pub const SCOPE_PRESENCE_WRITE: &str = "companion:presence:write";
pub const ERR_PAIRING_EXPIRED: &str = "COMPANION_PAIRING_EXPIRED";
pub const ERR_VALIDATION_FAILED: &str = "VALIDATION_FAILED";
pub const ERR_RATE_LIMITED: &str = "RATE_LIMITED";
pub const ERR_INTERNAL: &str = "INTERNAL_ERROR";
pub const ERR_HTTP: &str = "HTTP_ERROR";

const MAX_PAIRING_CODE: usize = 32;
const MAX_DEVICE_NAME: usize = 120;
const MAX_RESPONSE_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PairingError {
    #[error("pairing code must be 1-{MAX_PAIRING_CODE} characters")]
    InvalidPairingCode,
    #[error("device name must be 1-{MAX_DEVICE_NAME} characters")]
    InvalidDeviceName,
    #[error("response too large")]
    ResponseTooLarge,
    #[error("missing scope {SCOPE_PRESENCE_WRITE}")]
    MissingScope,
    #[error("malformed pairing response: {0}")]
    Malformed(String),
    #[error("invalid identifier in {0}")]
    InvalidIdentifier(&'static str),
    #[error("invalid safe integer in {0}")]
    InvalidSafeInteger(&'static str),
}

/// 校验并返回 trim 后的 (pairingCode, deviceName)。
/// macOS 版会对设备名做 NFC 规范化；Windows 端依赖服务端规范化，此处保持原样。
pub fn validate_pairing_input(
    code: &str,
    device_name: &str,
) -> Result<(String, String), PairingError> {
    let code = code.trim();
    if code.is_empty() || code.chars().count() > MAX_PAIRING_CODE {
        return Err(PairingError::InvalidPairingCode);
    }
    let name = device_name.trim();
    if name.is_empty() {
        return Err(PairingError::InvalidDeviceName);
    }
    if name.chars().count() > MAX_DEVICE_NAME {
        return Err(PairingError::InvalidDeviceName);
    }
    Ok((code.to_string(), name.to_string()))
}

pub fn build_claim_body(code: &str, device_name: &str) -> Result<String, PairingError> {
    let (code, name) = validate_pairing_input(code, device_name)?;
    #[derive(serde::Serialize)]
    struct Body<'a> {
        #[serde(rename = "deviceName")]
        device_name: &'a str,
        #[serde(rename = "pairingCode")]
        pairing_code: &'a str,
    }
    to_sorted_json(&Body {
        device_name: &name,
        pairing_code: &code,
    })
    .map_err(|e| PairingError::Malformed(e.to_string()))
}

#[derive(Debug, Clone)]
pub struct PairingClaim {
    pub device_id: String,
    pub device_token: String,
    pub scopes: Vec<String>,
    pub next_sequence: i64,
}

/// 解析 claim 响应（≤64 KiB）。明文 token 只在内存中传递，调用方
/// 必须直接交给受保护存储，不得回落到日志或普通文件。
pub fn parse_claim_response(body: &[u8]) -> Result<PairingClaim, PairingError> {
    if body.len() > MAX_RESPONSE_BYTES {
        return Err(PairingError::ResponseTooLarge);
    }
    let root: serde_json::Value =
        serde_json::from_slice(body).map_err(|e| PairingError::Malformed(e.to_string()))?;
    let data = root
        .as_object()
        .and_then(|o| o.get("data"))
        .and_then(|v| v.as_object())
        .ok_or_else(|| PairingError::Malformed("missing data".into()))?;
    let device_id: String = take_required(data, "deviceId").map_err(PairingError::Malformed)?;
    if !is_valid_identifier(&device_id) {
        return Err(PairingError::InvalidIdentifier("data.deviceId"));
    }
    let device_token: String =
        take_required(data, "deviceToken").map_err(PairingError::Malformed)?;
    if device_token.is_empty() {
        return Err(PairingError::Malformed("empty deviceToken".into()));
    }
    let scopes: Vec<String> = take_required(data, "scopes").map_err(PairingError::Malformed)?;
    if !scopes.iter().any(|s| s == SCOPE_PRESENCE_WRITE) {
        return Err(PairingError::MissingScope);
    }
    let next_sequence: i64 =
        take_required(data, "nextSequence").map_err(PairingError::Malformed)?;
    if !(0..=MAX_SAFE_INTEGER).contains(&next_sequence) {
        return Err(PairingError::InvalidSafeInteger("data.nextSequence"));
    }
    Ok(PairingClaim {
        device_id,
        device_token,
        scopes,
        next_sequence,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claim_body_is_sorted_and_trimmed() {
        let body = build_claim_body("  ABC123  ", "  我的设备 ").unwrap();
        assert_eq!(body, r#"{"deviceName":"我的设备","pairingCode":"ABC123"}"#);
    }

    #[test]
    fn claim_input_validation() {
        assert_eq!(
            build_claim_body("", "dev").unwrap_err(),
            PairingError::InvalidPairingCode
        );
        assert_eq!(
            build_claim_body(&"x".repeat(33), "dev").unwrap_err(),
            PairingError::InvalidPairingCode
        );
        assert_eq!(
            build_claim_body("code", "  ").unwrap_err(),
            PairingError::InvalidDeviceName
        );
        assert_eq!(
            build_claim_body("code", &"名".repeat(121)).unwrap_err(),
            PairingError::InvalidDeviceName
        );
    }

    #[test]
    fn claim_response_roundtrip() {
        let body = br#"{"meta":{},"data":{"deviceId":"11111111-1111-4111-8111-111111111111","deviceToken":"tok-1","scopes":["companion:presence:write"],"nextSequence":42}}"#;
        let claim = parse_claim_response(body).unwrap();
        assert_eq!(claim.device_id, "11111111-1111-4111-8111-111111111111");
        assert_eq!(claim.device_token, "tok-1");
        assert_eq!(claim.next_sequence, 42);
    }

    #[test]
    fn claim_response_rejections() {
        // 缺少 presence scope
        let no_scope = br#"{"data":{"deviceId":"11111111-1111-4111-8111-111111111111","deviceToken":"t","scopes":["companion:moment:write"],"nextSequence":1}}"#;
        assert_eq!(
            parse_claim_response(no_scope).unwrap_err(),
            PairingError::MissingScope
        );
        // nextSequence 越界
        let bad_seq = br#"{"data":{"deviceId":"11111111-1111-4111-8111-111111111111","deviceToken":"t","scopes":["companion:presence:write"],"nextSequence":-1}}"#;
        assert_eq!(
            parse_claim_response(bad_seq).unwrap_err(),
            PairingError::InvalidSafeInteger("data.nextSequence")
        );
        // 响应超限
        let big = vec![b'x'; MAX_RESPONSE_BYTES + 1];
        assert_eq!(
            parse_claim_response(&big).unwrap_err(),
            PairingError::ResponseTooLarge
        );
    }
}
