//! Presence 客户端（macOS 版 YohakuPresenceClient 语义）：
//! 序列号预留 → mapper 组包 → 单发送槽串行 → 歧义失败以同一
//! sequence/requestId/body 立即重试一次；服务端 acceptedSequence 单调 reconcile。

use crate::ports::{Clock, HttpTransport, HttpRequest, HttpResponse, TransportError};
use std::sync::{Arc, Mutex};
use yohaku_protocol::error::{parse_error, parse_mutation, MutationResponse};
use yohaku_protocol::presence::{
    ClearReason, Mapper, MapperError, PresenceSnapshotInput,
};
use yohaku_protocol::sequencer::Sequencer;
use yohaku_protocol::CLIENT_VERSION;

pub const ERR_SCHEMA_UNSUPPORTED: &str = "COMPANION_SCHEMA_UNSUPPORTED";
pub const ERR_FEATURE_UNAVAILABLE: &str = "COMPANION_FEATURE_UNAVAILABLE";

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum PresenceError {
    /// 歧义传输失败（含重试后仍失败）
    #[error("transport failure: {0}")]
    Transport(String),
    /// schema/特性被服务端拒绝 → 丢弃 authority 重新协商
    #[error("schema rejected by server")]
    SchemaRejected,
    #[error("payload too large")]
    PayloadTooLarge,
    #[error("server error {code}: {message}")]
    Server { code: String, message: String },
    #[error("response decode failed: {0}")]
    Decode(String),
}

impl From<MapperError> for PresenceError {
    fn from(e: MapperError) -> Self {
        match e {
            MapperError::PayloadTooLarge => PresenceError::PayloadTooLarge,
            other => PresenceError::Decode(other.to_string()),
        }
    }
}

/// 协商完成后的能力快照（决定 artwork/link 键是否编码）。
#[derive(Debug, Clone, Copy)]
pub struct CapabilityFlags {
    pub supports_media_artwork: bool,
    pub supports_media_playback_links: bool,
}

pub struct PresenceClient {
    http: Arc<dyn HttpTransport>,
    mapper: Mapper,
    sequencer: Sequencer,
    base_url: String,
    device_id: String,
    token: String,
    #[allow(dead_code)] // link 能力启用前仅留存
    flags: CapabilityFlags,
    #[allow(dead_code)] // 能力标志留存在客户端上，供后续 link 能力启用
    send_slot: Mutex<()>,
}

struct RequestPlan {
    request_id: String,
    body: String,
}

impl PresenceClient {
    pub fn new(
        http: Arc<dyn HttpTransport>,
        mapper: Mapper,
        sequencer: Sequencer,
        base_url: String,
        device_id: String,
        token: String,
        flags: CapabilityFlags,
    ) -> Self {
        PresenceClient {
            http,
            mapper,
            sequencer,
            base_url: base_url.trim_end_matches('/').to_string(),
            device_id,
            token,
            flags,
            send_slot: Mutex::new(()),
        }
    }

    pub fn reconcile(&self, accepted: i64) {
        self.sequencer.reconcile(accepted);
    }

    pub fn device_id(&self) -> &str {
        &self.device_id
    }

    fn build_plan(
        &self,
        input: PresenceSnapshotInput,
        clock: &dyn Clock,
    ) -> Result<RequestPlan, PresenceError> {
        let input = PresenceSnapshotInput {
            request_id: uuid::Uuid::new_v4().to_string(),
            device_id: self.device_id.clone(),
            sequence: self.sequencer.reserve(),
            observed_at: clock.now(),
            ..input
        };
        Ok(RequestPlan {
            request_id: input.request_id.clone(),
            body: self.mapper.build_presence(&input)?,
        })
    }

    pub fn replace_presence(
        &self,
        input: PresenceSnapshotInput,
        clock: &dyn Clock,
    ) -> Result<MutationResponse, PresenceError> {
        let plan = self.build_plan(input, clock)?;
        self.perform(plan, clock)
    }

    pub fn clear_presence(
        &self,
        reason: ClearReason,
        clock: &dyn Clock,
    ) -> Result<MutationResponse, PresenceError> {
        let request_id = uuid::Uuid::new_v4().to_string();
        let sequence = self.sequencer.reserve();
        let body = self.mapper.build_clear(
            &request_id,
            &self.device_id,
            sequence,
            reason,
            clock.now(),
        )?;
        self.perform(RequestPlan { request_id, body }, clock)
    }

    /// best-effort 清除：受限超时（睡眠/锁屏/关机路径，先到者赢）。
    pub fn clear_presence_bounded(
        &self,
        reason: ClearReason,
        clock: &dyn Clock,
        timeout_ms: u64,
    ) -> Result<MutationResponse, PresenceError> {
        let request_id = uuid::Uuid::new_v4().to_string();
        let sequence = self.sequencer.reserve();
        let body = self.mapper.build_clear(
            &request_id,
            &self.device_id,
            sequence,
            reason,
            clock.now(),
        )?;
        self.perform_bounded(RequestPlan { request_id, body }, clock, timeout_ms)
    }

    fn request_for(&self, plan: &RequestPlan) -> HttpRequest {
        self.request_for_with_timeout(plan, 10_000)
    }

    fn request_for_with_timeout(&self, plan: &RequestPlan, timeout_ms: u64) -> HttpRequest {
        let headers = vec![
            ("Accept".to_string(), "application/json".to_string()),
            (
                "Authorization".to_string(),
                format!("Bearer {}", self.token),
            ),
            (
                "Content-Type".to_string(),
                "application/json".to_string(),
            ),
            (
                "X-Yohaku-Companion-Version".to_string(),
                CLIENT_VERSION.to_string(),
            ),
        ];
        HttpRequest {
            method: "PUT",
            url: format!("{}/companion/presence", self.base_url),
            headers,
            body: Some(plan.body.clone().into_bytes()),
            timeout_ms,
        }
    }

    /// 单发送槽 + 单次幂等重试（macOS 版 performWithSingleRetry 语义矩阵）。
    fn perform(&self, plan: RequestPlan, _clock: &dyn Clock) -> Result<MutationResponse, PresenceError> {
        self.perform_bounded(plan, _clock, 10_000)
    }

    fn perform_bounded(
        &self,
        plan: RequestPlan,
        _clock: &dyn Clock,
        timeout_ms: u64,
    ) -> Result<MutationResponse, PresenceError> {
        let _slot = self.send_slot.lock().unwrap();
        let request = self.request_for_with_timeout(&plan, timeout_ms);
        let result = self.attempt(&request, &plan.request_id);
        match result {
            Ok(response) => Ok(response),
            Err(PresenceError::Transport(_) | PresenceError::Decode(_)) => {
                // 歧义失败：服务端可能已提交 → 同 sequence/requestId/body 重试一次
                let retry = self.request_for(&plan);
                self.attempt(&retry, &plan.request_id)
            }
            Err(other) => Err(other),
        }
    }

    fn attempt(
        &self,
        request: &HttpRequest,
        request_id: &str,
    ) -> Result<MutationResponse, PresenceError> {
        let response = self
            .http
            .send(request.clone())
            .map_err(|TransportError(e)| PresenceError::Transport(e))?;
        let mutation = self.decode(&response, request_id)?;
        self.sequencer.reconcile(mutation.accepted_sequence);
        Ok(mutation)
    }

    fn decode(
        &self,
        response: &HttpResponse,
        request_id: &str,
    ) -> Result<MutationResponse, PresenceError> {
        if (200..300).contains(&response.status) {
            return parse_mutation(&response.body, request_id)
                .map_err(|e| PresenceError::Decode(e.to_string()));
        }
        let server_error = parse_error(&response.body);
        // 任何带 acceptedSequence 的服务端错误先 reconcile（同槽内已串行）
        if let Some(error) = &server_error
            && let Some(accepted) = error.accepted_sequence {
                self.sequencer.reconcile(accepted);
            }
        if response.status == 426 {
            return Err(PresenceError::SchemaRejected);
        }
        if let Some(error) = &server_error
            && (error.code == ERR_SCHEMA_UNSUPPORTED || error.code == ERR_FEATURE_UNAVAILABLE) {
                return Err(PresenceError::SchemaRejected);
            }
        let retryable = server_error
            .as_ref()
            .map(|e| e.retryable)
            .unwrap_or(response.status >= 500);
        if retryable && response.status >= 500 {
            return Err(PresenceError::Transport(format!("retryable {}", response.status)));
        }
        // 不可重试的服务端错误/不可解析的非 2xx
        if let Some(error) = &server_error {
            return Err(PresenceError::Server {
                code: error.code.clone(),
                message: error.message.clone(),
            });
        }
        Err(PresenceError::Decode(format!("status {}", response.status)))
    }
}

#[cfg(test)]
mod tests {
    use yohaku_protocol::presence::Availability;
    use super::*;
    use chrono::{DateTime, TimeZone, Utc};
    use std::collections::VecDeque;

    const DEVICE: &str = "11111111-1111-4111-8111-111111111111";

    struct FixedClock(DateTime<Utc>);

    impl Clock for FixedClock {
        fn now(&self) -> DateTime<Utc> {
            self.0
        }
    }

    fn fixed_clock() -> FixedClock {
        FixedClock(Utc.with_ymd_and_hms(2026, 1, 2, 3, 4, 5).unwrap())
    }

    enum Step {
        Fail,
        Json(u16, String),
    }

    /// 脚本化传输：按序返回步骤；`Json` 步骤 body 中的 `{REQUEST_ID}`
    /// 被请求体里的实际 requestId 替换（模拟服务端回显）。
    struct Scripted {
        steps: Mutex<VecDeque<Step>>,
        pub requests: Mutex<Vec<HttpRequest>>,
    }

    fn request_id_of(request: &HttpRequest) -> String {
        let body = String::from_utf8(request.body.clone().expect("body")).unwrap();
        let start = body.find("\"requestId\":\"").expect("requestId") + 13;
        body[start..start + 36].to_string()
    }

    impl HttpTransport for Scripted {
        fn send(&self, request: HttpRequest) -> Result<HttpResponse, TransportError> {
            self.requests.lock().unwrap().push(request);
            let step = self.steps.lock().unwrap().pop_front().expect("script exhausted");
            match step {
                Step::Fail => Err(TransportError("connection reset".into())),
                Step::Json(status, body) => {
                    let rid = request_id_of(self.requests.lock().unwrap().last().unwrap());
                    Ok(HttpResponse {
                        status,
                        body: body.replace("{REQUEST_ID}", &rid).into_bytes(),
                    })
                }
            }
        }
    }

    fn scripted(steps: Vec<Step>) -> Arc<Scripted> {
        Arc::new(Scripted {
            steps: Mutex::new(steps.into()),
            requests: Mutex::new(Vec::new()),
        })
    }

    fn mutation_ok() -> String {
        format!(
            r#"{{"meta":{{"schema":"{0}","schemaVersion":2,"requestId":"{{REQUEST_ID}}","serverTime":"2026-01-02T03:04:05.000Z"}},"data":{{"acceptedSequence":100,"receivedAt":"2026-01-02T03:04:05.000Z","state":{{}}}}}}"#,
            "yohaku.companion.presence"
        )
    }

    fn error_json(code: &str, retryable: bool, accepted: Option<i64>) -> String {
        let accepted = accepted
            .map(|v| v.to_string())
            .unwrap_or_else(|| "null".into());
        format!(
            r#"{{"meta":{{}},"error":{{"code":"{code}","message":"m","retryable":{retryable},"retryAfterMs":null,"acceptedSequence":{accepted},"fields":[]}}}}"#
        )
    }

    fn input() -> PresenceSnapshotInput {
        PresenceSnapshotInput {
            request_id: String::new(),
            device_id: String::new(),
            sequence: 0,
            observed_at: Utc::now(),
            lease_ttl_seconds: 90,
            availability: Availability::Active,
            application: Some(yohaku_protocol::presence::ApplicationPart {
                display_name: "Edge".into(),
                activity_key: None,
                activity_custom_label: None,
                window_title: None,
                icon_url: None,
            }),
            media: None,
        }
    }

    fn client(http: Arc<dyn HttpTransport>) -> PresenceClient {
        let sequencer = Sequencer::new(
            Arc::new(tests_support::MemoryPersistence::default()),
            DEVICE,
            10,
        );
        PresenceClient::new(
            http,
            Mapper::new(
                yohaku_protocol::capabilities::CapabilityLimits {
                    presence_payload_bytes: 32_768,
                    presence_requests_per_minute: 10,
                    presence_lease_min_seconds: 30,
                    presence_lease_max_seconds: 300,
                    recommended_heartbeat_seconds: 30,
                    maximum_clock_skew_seconds: 60,
                },
                Default::default(),
            ),
            sequencer,
            "https://core.example.com".into(),
            DEVICE.into(),
            "token-1".into(),
            CapabilityFlags {
                supports_media_artwork: false,
                supports_media_playback_links: false,
            },
        )
    }

    #[test]
    fn success_sends_required_headers_and_reconciles() {
        let http = scripted(vec![Step::Json(200, mutation_ok())]);
        let c = client(http.clone());
        let mutation = c.replace_presence(input(), &fixed_clock()).unwrap();
        assert_eq!(mutation.accepted_sequence, 100);
        assert_eq!(c.sequencer.peek(), 101);
        let requests = http.requests.lock().unwrap();
        let r = &requests[0];
        assert_eq!(r.method, "PUT");
        assert_eq!(r.url, "https://core.example.com/companion/presence");
        assert!(r.headers.iter().any(|(k, v)| k == "Authorization" && v == "Bearer token-1"));
        assert!(r.headers.iter().any(|(k, v)| k == "X-Yohaku-Companion-Version" && v == "1.7.3"));
        assert!(r.headers.iter().any(|(k, v)| k == "Content-Type" && v == "application/json"));
    }

    #[test]
    fn transport_failure_retries_same_body_once() {
        let http = scripted(vec![Step::Fail, Step::Json(200, mutation_ok())]);
        let c = client(http.clone());
        let mutation = c.replace_presence(input(), &fixed_clock()).unwrap();
        assert_eq!(mutation.accepted_sequence, 100);
        let requests = http.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        // 幂等重试：同 sequence/requestId/body
        assert_eq!(requests[0].body, requests[1].body);
        assert_eq!(request_id_of(&requests[0]), request_id_of(&requests[1]));
    }

    #[test]
    fn transport_failure_twice_is_final() {
        let http = scripted(vec![Step::Fail, Step::Fail]);
        let c = client(http);
        assert!(matches!(
            c.replace_presence(input(), &fixed_clock()),
            Err(PresenceError::Transport(_))
        ));
    }

    #[test]
    fn decode_failure_is_retried_like_transport() {
        // 200 但响应不可解析（缺 requestId 回显）→ 歧义 → 重试一次成功
        let http = scripted(vec![
            Step::Json(200, r#"{"meta":{},"data":{}}"#.into()),
            Step::Json(200, mutation_ok()),
        ]);
        let c = client(http.clone());
        assert_eq!(
            c.replace_presence(input(), &fixed_clock()).unwrap().accepted_sequence,
            100
        );
        assert_eq!(http.requests.lock().unwrap().len(), 2);
    }

    #[test]
    fn retryable_5xx_reconciles_accepted_and_retries() {
        let http = scripted(vec![
            Step::Json(500, error_json("INTERNAL", true, Some(15))),
            Step::Fail,
        ]);
        let c = client(http);
        let err = c.replace_presence(input(), &fixed_clock()).unwrap_err();
        assert!(matches!(err, PresenceError::Transport(_)));
        assert_eq!(c.sequencer.peek(), 16); // acceptedSequence=15 已 reconcile
    }

    #[test]
    fn non_retryable_4xx_is_fatal() {
        let http = scripted(vec![Step::Json(400, error_json("VALIDATION_FAILED", false, None))]);
        let c = client(http);
        assert!(matches!(
            c.replace_presence(input(), &fixed_clock()),
            Err(PresenceError::Server { .. })
        ));
    }

    #[test]
    fn http_426_and_schema_codes_reject_schema() {
        for (status, body) in [
            (426u16, error_json("OTHER", true, None)),
            (400, error_json(ERR_SCHEMA_UNSUPPORTED, false, None)),
            (400, error_json(ERR_FEATURE_UNAVAILABLE, false, None)),
        ] {
            let http = scripted(vec![Step::Json(status, body)]);
            let c = client(http);
            assert_eq!(
                c.replace_presence(input(), &fixed_clock()).unwrap_err(),
                PresenceError::SchemaRejected
            );
        }
    }

    #[test]
    fn clear_presence_sends_clear_shape() {
        let http = scripted(vec![Step::Json(200, mutation_ok())]);
        let c = client(http.clone());
        let mutation = c.clear_presence(ClearReason::Sleep, &fixed_clock()).unwrap();
        assert_eq!(mutation.accepted_sequence, 100);
        let body =
            String::from_utf8(http.requests.lock().unwrap()[0].body.clone().expect("body"))
                .unwrap();
        assert!(body.contains(r#""reason":"sleep""#));
    }
}

/// 测试支持件。
#[cfg(test)]
pub(crate) mod tests_support {
    use std::collections::BTreeMap;
    use std::sync::Mutex;
    use yohaku_protocol::sequencer::SequencePersistence;

    #[derive(Default)]
    pub struct MemoryPersistence(pub Mutex<BTreeMap<String, i64>>);

    impl SequencePersistence for MemoryPersistence {
        fn load_next(&self, device_id: &str) -> Option<i64> {
            self.0.lock().unwrap().get(device_id).copied()
        }
        fn save_next(&self, device_id: &str, next: i64) {
            self.0.lock().unwrap().insert(device_id.to_string(), next);
        }
    }
}
