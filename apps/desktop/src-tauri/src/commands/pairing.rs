use yohaku_app::error::ApiError;
use yohaku_app::ports::{HttpResponse, HttpTransport};
use yohaku_protocol::CLIENT_VERSION;
use yohaku_protocol::error::{ResponseError, parse_capabilities};
use yohaku_protocol::negotiator::negotiate;
use yohaku_protocol::pairing::{
    PairingClaim, PairingError, build_claim_body, parse_claim_response, validate_pairing_input,
};
use yohaku_store::{ConnectionMetadata, ConnectionStore};

fn pairing_error(e: PairingError) -> ApiError {
    let (code, message) = match e {
        PairingError::InvalidPairingCode => ("VALIDATION_FAILED", "请输入 1–32 个字符的配对码。"),
        PairingError::InvalidDeviceName => ("VALIDATION_FAILED", "请输入 1–120 个字符的设备名称。"),
        PairingError::MissingScope => (
            "MISSING_SCOPE",
            "配对信息缺少同步权限，请检查服务器设置后重新配对。",
        ),
        PairingError::ResponseTooLarge => (
            "MALFORMED",
            "服务器返回的配对信息过大，请联系服务器管理员。",
        ),
        PairingError::Malformed(_)
        | PairingError::InvalidIdentifier(_)
        | PairingError::InvalidSafeInteger(_) => (
            "MALFORMED",
            "服务器返回的配对信息有误，请联系服务器管理员。",
        ),
    };
    ApiError::new(code, message)
}

pub(super) fn pair(
    http: &dyn HttpTransport,
    connection: &ConnectionStore,
    server_url: &str,
    pairing_code: &str,
    device_name: &str,
) -> Result<ConnectionMetadata, ApiError> {
    let (code, name) = validate_pairing_input(pairing_code, device_name).map_err(pairing_error)?;
    let base = server_url.trim_end_matches('/');
    preflight(http, base)?;
    let claim = claim(http, base, &code, &name)?;
    let metadata: ConnectionMetadata = connection
        .install_pairing_claim(
            &claim.device_id,
            &claim.device_token,
            &claim.scopes,
            claim.next_sequence,
            base,
        )
        .map_err(|e| ApiError::new("STORE", e.to_string()))?;
    Ok(metadata)
}

fn preflight(http: &dyn HttpTransport, base: &str) -> Result<(), ApiError> {
    let caps_request = yohaku_app::ports::HttpRequest {
        method: "GET",
        url: format!("{base}/companion/capabilities"),
        headers: vec![
            ("Accept".into(), "application/json".into()),
            ("X-Yohaku-Companion-Version".into(), CLIENT_VERSION.into()),
        ],
        body: None,
        timeout_ms: 10_000,
    };
    let response = http
        .send(caps_request)
        .map_err(|e| ApiError::new("TRANSPORT", e.0))?;
    let body = successful_body(response)?;
    let (_meta, caps) = parse_capabilities(&body).map_err(|e| match e {
        ResponseError::IncompatibleSchema | ResponseError::IncompatibleSchemaVersion => {
            ApiError::new(
                "SCHEMA_UNSUPPORTED",
                "Yohaku Companion 与服务器版本不兼容，请检查双方是否已更新。",
            )
        }
        _ => ApiError::new(
            "INVALID_CAPABILITIES",
            "无法读取服务器信息，请检查服务器地址和设置。",
        ),
    })?;
    match negotiate(&caps, CLIENT_VERSION) {
        yohaku_protocol::negotiator::Negotiation::Available(_) => {}
        yohaku_protocol::negotiator::Negotiation::ClientUpdateRequired => {
            return Err(ApiError::new(
                "UPDATE_REQUIRED",
                "请更新 Yohaku Companion 后重新连接。",
            ));
        }
        yohaku_protocol::negotiator::Negotiation::SchemaUnsupported => {
            return Err(ApiError::new(
                "SCHEMA_UNSUPPORTED",
                "Yohaku Companion 与服务器版本不兼容，请检查双方是否已更新。",
            ));
        }
        yohaku_protocol::negotiator::Negotiation::FeatureUnavailable => {
            return Err(ApiError::new(
                "FEATURE_UNAVAILABLE",
                "服务器暂不支持 Live Desk，请检查服务器设置。",
            ));
        }
        yohaku_protocol::negotiator::Negotiation::InvalidCapabilities => {
            return Err(ApiError::new(
                "INVALID_CAPABILITIES",
                "服务器信息有误，请检查服务器设置。",
            ));
        }
    }
    Ok(())
}

fn claim(
    http: &dyn HttpTransport,
    base: &str,
    code: &str,
    name: &str,
) -> Result<PairingClaim, ApiError> {
    let body = build_claim_body(code, name).map_err(pairing_error)?;
    let claim_request = yohaku_app::ports::HttpRequest {
        method: "POST",
        url: format!("{base}/companion/pairings/claim"),
        headers: vec![
            ("Accept".into(), "application/json".into()),
            ("Content-Type".into(), "application/json".into()),
            ("X-Yohaku-Companion-Version".into(), CLIENT_VERSION.into()),
        ],
        body: Some(body.into_bytes()),
        timeout_ms: 10_000,
    };
    let response = http
        .send(claim_request)
        .map_err(|e| ApiError::new("TRANSPORT", e.0))?;
    let body = successful_body(response)?;
    let claim = parse_claim_response(&body).map_err(pairing_error)?;
    Ok(claim)
}

fn successful_body(response: HttpResponse) -> Result<Vec<u8>, ApiError> {
    if !(200..300).contains(&response.status) {
        if let Some(server_error) = yohaku_protocol::error::parse_error(&response.body) {
            return Err(ApiError::new(&server_error.code, server_error.message));
        }
        return Err(ApiError::new(
            "HTTP_ERROR",
            format!("配对失败，请稍后重试（HTTP {}）。", response.status),
        ));
    }
    Ok(response.body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};
    use yohaku_app::ports::{HttpRequest, HttpResponse, TransportError};
    use yohaku_store::{SecretStore, StoreResult};

    struct MemorySecret(Mutex<Option<Vec<u8>>>);
    impl SecretStore for MemorySecret {
        fn set(&self, _: &str, value: &[u8]) -> StoreResult<()> {
            *self.0.lock().unwrap() = Some(value.to_vec());
            Ok(())
        }
        fn get(&self, _: &str) -> StoreResult<Option<Vec<u8>>> {
            Ok(self.0.lock().unwrap().clone())
        }
        fn remove(&self, _: &str) -> StoreResult<()> {
            *self.0.lock().unwrap() = None;
            Ok(())
        }
    }

    struct ScriptedHttp {
        responses: Mutex<VecDeque<HttpResponse>>,
        requests: Mutex<Vec<HttpRequest>>,
    }
    impl HttpTransport for ScriptedHttp {
        fn send(&self, request: HttpRequest) -> Result<HttpResponse, TransportError> {
            self.requests.lock().unwrap().push(request);
            self.responses
                .lock()
                .unwrap()
                .pop_front()
                .ok_or_else(|| TransportError("unexpected request".into()))
        }
    }
    fn capabilities() -> Vec<u8> {
        br#"{"meta":{"schema":"yohaku.companion.presence","schemaVersion":2,"requestId":"11111111-1111-4111-8111-111111111111","serverTime":"2026-01-02T03:04:05.000Z"},"data":{"minimumClientVersion":"1.7.3","presenceSchemaVersions":[2],"momentSchemaVersions":[1],"features":{"liveDesk":true,"mediaTimeline":true,"moments":false,"readingSessions":true,"mediaArtwork":true},"limits":{"presencePayloadBytes":32768,"presenceRequestsPerMinute":10,"presenceLeaseMinSeconds":30,"presenceLeaseMaxSeconds":300,"recommendedHeartbeatSeconds":90,"maximumClockSkewSeconds":60}}}"#.to_vec()
    }
    fn http(status: u16) -> ScriptedHttp {
        ScriptedHttp {
            responses: Mutex::new(VecDeque::from([
                HttpResponse { status, body: capabilities() },
                HttpResponse { status: 200, body: br#"{"data":{"deviceId":"11111111-1111-4111-8111-111111111111","deviceToken":"test-token","scopes":["companion:presence:write"],"nextSequence":7}}"#.to_vec() },
            ])),
            requests: Mutex::new(Vec::new()),
        }
    }
    fn connection(dir: &std::path::Path) -> ConnectionStore {
        ConnectionStore::new(dir, Arc::new(MemorySecret(Mutex::new(None))))
    }

    #[test]
    fn invalid_input_never_consumes_pairing_code() {
        let dir = tempfile::tempdir().unwrap();
        let store = connection(dir.path());
        let http = http(200);
        let error = pair(&http, &store, "https://core", " ", "device").unwrap_err();
        assert_eq!(error.code, "VALIDATION_FAILED");
        assert!(http.requests.lock().unwrap().is_empty());
        assert!(store.load_metadata().unwrap().is_none());
    }

    #[test]
    fn failed_capabilities_status_never_claims() {
        let dir = tempfile::tempdir().unwrap();
        let store = connection(dir.path());
        let http = http(503);
        let result = pair(&http, &store, "https://core", "code", "device");
        assert!(
            result.is_err(),
            "non-success capabilities must not authorize a claim"
        );
        assert_eq!(http.requests.lock().unwrap().len(), 1);
        assert!(store.device_token().unwrap().is_none());
    }

    #[test]
    fn pairing_preserves_request_order_and_installs_disabled_connection() {
        let dir = tempfile::tempdir().unwrap();
        let store = connection(dir.path());
        let http = http(200);
        let metadata = pair(&http, &store, "https://core///", " code ", " Laptop ").unwrap();
        let requests = http.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(
            (requests[0].method, requests[0].url.as_str()),
            ("GET", "https://core/companion/capabilities")
        );
        assert_eq!(
            (requests[1].method, requests[1].url.as_str()),
            ("POST", "https://core/companion/pairings/claim")
        );
        assert_eq!(
            requests[1].body.as_deref(),
            Some(br#"{"deviceName":"Laptop","pairingCode":"code"}"#.as_slice())
        );
        assert!(!metadata.is_live_desk_enabled);
        assert_eq!(metadata.next_sequence, Some(7));
        assert_eq!(store.device_token().unwrap().as_deref(), Some("test-token"));
    }

    #[test]
    fn missing_scope_is_rejected_before_installing_credentials() {
        let dir = tempfile::tempdir().unwrap();
        let store = connection(dir.path());
        let http = http(200);
        http.responses.lock().unwrap()[1].body = br#"{"data":{"deviceId":"11111111-1111-4111-8111-111111111111","deviceToken":"test-token","scopes":[],"nextSequence":7}}"#.to_vec();
        assert_eq!(
            pair(&http, &store, "https://core", "code", "device")
                .unwrap_err()
                .code,
            "MISSING_SCOPE"
        );
        assert!(store.device_token().unwrap().is_none());
        assert!(store.load_metadata().unwrap().is_none());
    }
}
