//! 协议响应解析（客户端所需的最小投影）。

use crate::MAX_SAFE_INTEGER;
use crate::ids::is_valid_identifier;
use crate::json::take_nullable;
use crate::json::take_required;
use crate::presence::{PRESENCE_SCHEMA, PRESENCE_SCHEMA_VERSION};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ResponseError {
    #[error("incompatible schema")]
    IncompatibleSchema,
    #[error("incompatible schema version")]
    IncompatibleSchemaVersion,
    #[error("invalid identifier in {0}")]
    InvalidIdentifier(&'static str),
    #[error("invalid safe integer in {0}")]
    InvalidSafeInteger(&'static str),
    #[error("malformed response: {0}")]
    Malformed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResponseMeta {
    pub request_id: String,
    pub server_time: String,
}

fn parse_meta(
    meta: &serde_json::Map<String, serde_json::Value>,
) -> Result<ResponseMeta, ResponseError> {
    let schema: String = take_required(meta, "schema").map_err(ResponseError::Malformed)?;
    let schema_version: i64 =
        take_required(meta, "schemaVersion").map_err(ResponseError::Malformed)?;
    let request_id: String = take_required(meta, "requestId").map_err(ResponseError::Malformed)?;
    let server_time: String =
        take_required(meta, "serverTime").map_err(ResponseError::Malformed)?;
    if schema != PRESENCE_SCHEMA {
        return Err(ResponseError::IncompatibleSchema);
    }
    if schema_version != PRESENCE_SCHEMA_VERSION {
        return Err(ResponseError::IncompatibleSchemaVersion);
    }
    if !is_valid_identifier(&request_id) {
        return Err(ResponseError::InvalidIdentifier("meta.requestId"));
    }
    Ok(ResponseMeta {
        request_id,
        server_time,
    })
}

fn envelope(body: &[u8]) -> Result<serde_json::Map<String, serde_json::Value>, ResponseError> {
    let value: serde_json::Value =
        serde_json::from_slice(body).map_err(|e| ResponseError::Malformed(e.to_string()))?;
    value
        .as_object()
        .cloned()
        .ok_or_else(|| ResponseError::Malformed("not an object".into()))
}

// ---------- Capabilities ----------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureSet {
    pub live_desk: bool,
    pub media_timeline: bool,
    pub moments: bool,
    pub reading_sessions: bool,
    pub media_artwork: Option<bool>,
    pub media_playback_links: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilitiesData {
    pub minimum_client_version: String,
    pub presence_schema_versions: Vec<i64>,
    pub moment_schema_versions: Vec<i64>,
    pub features: FeatureSet,
    pub limits: crate::capabilities::CapabilityLimits,
}

pub fn parse_capabilities(body: &[u8]) -> Result<(ResponseMeta, CapabilitiesData), ResponseError> {
    let root = envelope(body)?;
    let meta_value = take_required::<serde_json::Map<String, serde_json::Value>>(&root, "meta")
        .map_err(ResponseError::Malformed)?;
    let meta = parse_meta(&meta_value)?;
    let data = take_required::<serde_json::Map<String, serde_json::Value>>(&root, "data")
        .map_err(ResponseError::Malformed)?;
    let features_map =
        take_required::<serde_json::Map<String, serde_json::Value>>(&data, "features")
            .map_err(ResponseError::Malformed)?;
    let features = FeatureSet {
        live_desk: take_required(&features_map, "liveDesk").map_err(ResponseError::Malformed)?,
        media_timeline: take_required(&features_map, "mediaTimeline")
            .map_err(ResponseError::Malformed)?,
        moments: take_required(&features_map, "moments").map_err(ResponseError::Malformed)?,
        reading_sessions: take_required(&features_map, "readingSessions")
            .map_err(ResponseError::Malformed)?,
        media_artwork: take_optional(&features_map, "mediaArtwork")
            .map_err(ResponseError::Malformed)?,
        media_playback_links: take_optional(&features_map, "mediaPlaybackLinks")
            .map_err(ResponseError::Malformed)?,
    };
    let limits_map = take_required::<serde_json::Map<String, serde_json::Value>>(&data, "limits")
        .map_err(ResponseError::Malformed)?;
    let limits = crate::capabilities::CapabilityLimits::from_map(&limits_map)
        .ok_or(ResponseError::Malformed("limits".into()))?;
    Ok((
        meta,
        CapabilitiesData {
            minimum_client_version: take_required(&data, "minimumClientVersion")
                .map_err(ResponseError::Malformed)?,
            presence_schema_versions: take_required(&data, "presenceSchemaVersions")
                .map_err(ResponseError::Malformed)?,
            moment_schema_versions: take_required(&data, "momentSchemaVersions")
                .map_err(ResponseError::Malformed)?,
            features,
            limits,
        },
    ))
}

use crate::json::take_optional;

// ---------- Mutation ----------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MutationResponse {
    pub meta: ResponseMeta,
    pub accepted_sequence: i64,
}

/// 客户端最小投影：meta + acceptedSequence。服务端 `state` 投影由
/// Live Desk 前端另行渲染，客户端不做解码。
pub fn parse_mutation(
    body: &[u8],
    expected_request_id: &str,
) -> Result<MutationResponse, ResponseError> {
    let root = envelope(body)?;
    let meta_value = take_required::<serde_json::Map<String, serde_json::Value>>(&root, "meta")
        .map_err(ResponseError::Malformed)?;
    let meta = parse_meta(&meta_value)?;
    if meta.request_id != expected_request_id {
        return Err(ResponseError::InvalidIdentifier("meta.requestId echo"));
    }
    let data = take_required::<serde_json::Map<String, serde_json::Value>>(&root, "data")
        .map_err(ResponseError::Malformed)?;
    let accepted_sequence: i64 =
        take_required(&data, "acceptedSequence").map_err(ResponseError::Malformed)?;
    if !(0..=MAX_SAFE_INTEGER).contains(&accepted_sequence) {
        return Err(ResponseError::InvalidSafeInteger("data.acceptedSequence"));
    }
    Ok(MutationResponse {
        meta,
        accepted_sequence,
    })
}

// ---------- 错误信封 ----------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
    pub retry_after_ms: Option<i64>,
    pub accepted_sequence: Option<i64>,
    pub fields: Vec<String>,
}

/// 宽松解码：无法解析为错误信封时返回 None（调用方按状态码兜底）。
pub fn parse_error(body: &[u8]) -> Option<ServerError> {
    let root = envelope(body).ok()?;
    let error = take_required::<serde_json::Map<String, serde_json::Value>>(&root, "error").ok()?;
    Some(ServerError {
        code: take_required(&error, "code").ok()?,
        message: take_required(&error, "message").ok()?,
        retryable: take_required(&error, "retryable").ok()?,
        retry_after_ms: take_nullable(&error, "retryAfterMs").ok()?,
        accepted_sequence: take_nullable(&error, "acceptedSequence").ok()?,
        fields: take_required(&error, "fields").ok()?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const REQUEST_ID: &str = "11111111-1111-4111-8111-111111111111";

    fn capabilities_body() -> Vec<u8> {
        format!(
            r#"{{"meta":{{"schema":"{PRESENCE_SCHEMA}","schemaVersion":2,"requestId":"{REQUEST_ID}","serverTime":"2026-01-02T03:04:05.000Z"}},
"data":{{"minimumClientVersion":"1.7.3","presenceSchemaVersions":[2],"momentSchemaVersions":[1],
"features":{{"liveDesk":true,"mediaTimeline":true,"moments":false,"readingSessions":true,"mediaArtwork":true}},
"limits":{{"presencePayloadBytes":32768,"presenceRequestsPerMinute":10,"presenceLeaseMinSeconds":30,"presenceLeaseMaxSeconds":300,"recommendedHeartbeatSeconds":90,"maximumClockSkewSeconds":60}}}}}}"#
        )
        .into_bytes()
    }

    #[test]
    fn capabilities_roundtrip() {
        let (meta, data) = parse_capabilities(&capabilities_body()).unwrap();
        assert_eq!(meta.request_id, REQUEST_ID);
        assert_eq!(data.minimum_client_version, "1.7.3");
        assert!(data.features.live_desk);
        assert_eq!(data.features.media_artwork, Some(true));
        assert_eq!(data.features.media_playback_links, None);
        assert_eq!(data.limits.presence_payload_bytes, 32768);
    }

    #[test]
    fn capabilities_rejects_wrong_schema_or_version() {
        let body = String::from_utf8(capabilities_body()).unwrap();
        let text = body.replace("\"schemaVersion\":2", "\"schemaVersion\":3");
        assert_eq!(
            parse_capabilities(text.as_bytes()).unwrap_err(),
            ResponseError::IncompatibleSchemaVersion
        );
        let text = body.replace("yohaku.companion.presence", "yohaku.companion.other");
        assert_eq!(
            parse_capabilities(text.as_bytes()).unwrap_err(),
            ResponseError::IncompatibleSchema
        );
    }

    #[test]
    fn mutation_requires_request_id_echo() {
        let body = format!(
            r#"{{"meta":{{"schema":"{PRESENCE_SCHEMA}","schemaVersion":2,"requestId":"{REQUEST_ID}","serverTime":"2026-01-02T03:04:05.000Z"}},"data":{{"acceptedSequence":9,"receivedAt":"2026-01-02T03:04:05.000Z","state":{{}}}}}}"#
        )
        .into_bytes();
        let ok = parse_mutation(&body, REQUEST_ID).unwrap();
        assert_eq!(ok.accepted_sequence, 9);
        assert_eq!(
            parse_mutation(&body, "22222222-2222-4222-8222-222222222222").unwrap_err(),
            ResponseError::InvalidIdentifier("meta.requestId echo")
        );
    }

    #[test]
    fn mutation_rejects_out_of_range_sequence() {
        let body = format!(
            r#"{{"meta":{{"schema":"{PRESENCE_SCHEMA}","schemaVersion":2,"requestId":"{REQUEST_ID}","serverTime":"2026-01-02T03:04:05.000Z"}},"data":{{"acceptedSequence":-1,"receivedAt":"2026-01-02T03:04:05.000Z","state":{{}}}}}}"#
        )
        .into_bytes();
        assert_eq!(
            parse_mutation(&body, REQUEST_ID).unwrap_err(),
            ResponseError::InvalidSafeInteger("data.acceptedSequence")
        );
    }

    #[test]
    fn error_envelope() {
        let body = br#"{"meta":{},"error":{"code":"RATE_LIMITED","message":"slow down","retryable":true,"retryAfterMs":1500,"acceptedSequence":null,"fields":[]}}"#;
        let err = parse_error(body).unwrap();
        assert_eq!(err.code, "RATE_LIMITED");
        assert!(err.retryable);
        assert_eq!(err.retry_after_ms, Some(1500));
        assert_eq!(err.accepted_sequence, None);
        assert!(parse_error(b"not json").is_none());
    }
}
