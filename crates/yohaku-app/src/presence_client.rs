use crate::ports::{Clock, HttpRequest, HttpResponse, HttpTransport, TransportError};
use std::sync::{Arc, Mutex, MutexGuard, TryLockError};
use std::time::{Duration, Instant};
use yohaku_protocol::CLIENT_VERSION;
use yohaku_protocol::error::{MutationResponse, parse_error, parse_mutation};
use yohaku_protocol::presence::{ClearReason, Mapper, MapperError, PresenceSnapshotInput};
use yohaku_protocol::sequencer::Sequencer;

pub const ERR_SCHEMA_UNSUPPORTED: &str = "COMPANION_SCHEMA_UNSUPPORTED";
pub const ERR_FEATURE_UNAVAILABLE: &str = "COMPANION_FEATURE_UNAVAILABLE";

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum PresenceError {
    #[error("transport failure: {0}")]
    Transport(String),
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

pub struct PresenceClient {
    http: Arc<dyn HttpTransport>,
    mapper: Mapper,
    sequencer: Sequencer,
    base_url: String,
    device_id: String,
    token: String,
    send_slot: Mutex<()>,
}

struct RequestPlan {
    request_id: String,
    body: String,
}

struct SendBudget {
    started_at: Instant,
    timeout: Duration,
}

impl SendBudget {
    fn remaining(&self) -> Result<Duration, PresenceError> {
        self.timeout
            .checked_sub(self.started_at.elapsed())
            .filter(|remaining| remaining.as_millis() > 0)
            .ok_or_else(|| PresenceError::Transport("presence request timed out".into()))
    }
}

impl PresenceClient {
    pub fn new(
        http: Arc<dyn HttpTransport>,
        mapper: Mapper,
        sequencer: Sequencer,
        base_url: String,
        device_id: String,
        token: String,
    ) -> Self {
        PresenceClient {
            http,
            mapper,
            sequencer,
            base_url: base_url.trim_end_matches('/').to_string(),
            device_id,
            token,
            send_slot: Mutex::new(()),
        }
    }

    pub fn reconcile(&self, accepted: i64) {
        self.sequencer.reconcile(accepted);
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
        self.perform(plan, None)
    }

    pub fn clear_presence(
        &self,
        reason: ClearReason,
        clock: &dyn Clock,
    ) -> Result<MutationResponse, PresenceError> {
        let plan = self.build_clear_plan(reason, clock)?;
        self.perform(plan, None)
    }

    pub fn clear_presence_bounded(
        &self,
        reason: ClearReason,
        clock: &dyn Clock,
        timeout_ms: u64,
    ) -> Result<MutationResponse, PresenceError> {
        let budget = SendBudget {
            started_at: Instant::now(),
            timeout: Duration::from_millis(timeout_ms),
        };
        budget.remaining()?;
        let plan = self.build_clear_plan(reason, clock)?;
        self.perform(plan, Some(budget))
    }

    fn build_clear_plan(
        &self,
        reason: ClearReason,
        clock: &dyn Clock,
    ) -> Result<RequestPlan, PresenceError> {
        let request_id = uuid::Uuid::new_v4().to_string();
        let sequence = self.sequencer.reserve();
        let body =
            self.mapper
                .build_clear(&request_id, &self.device_id, sequence, reason, clock.now())?;
        Ok(RequestPlan { request_id, body })
    }

    fn request_for(
        &self,
        plan: &RequestPlan,
        budget: Option<&SendBudget>,
    ) -> Result<HttpRequest, PresenceError> {
        let timeout_ms = match budget {
            Some(budget) => budget.remaining()?.as_millis() as u64,
            None => 10_000,
        };
        let headers = vec![
            ("Accept".to_string(), "application/json".to_string()),
            (
                "Authorization".to_string(),
                format!("Bearer {}", self.token),
            ),
            ("Content-Type".to_string(), "application/json".to_string()),
            (
                "X-Yohaku-Companion-Version".to_string(),
                CLIENT_VERSION.to_string(),
            ),
        ];
        Ok(HttpRequest {
            method: "PUT",
            url: format!("{}/companion/presence", self.base_url),
            headers,
            body: Some(plan.body.clone().into_bytes()),
            timeout_ms,
        })
    }

    fn perform(
        &self,
        plan: RequestPlan,
        budget: Option<SendBudget>,
    ) -> Result<MutationResponse, PresenceError> {
        let budget = budget.as_ref();
        let _slot = self.acquire_send_slot(budget)?;
        let request = self.request_for(&plan, budget)?;
        let result = self.attempt(request, &plan.request_id);
        match result {
            Ok(response) => Ok(response),
            Err(PresenceError::Transport(_) | PresenceError::Decode(_)) => {
                let retry = self.request_for(&plan, budget)?;
                self.attempt(retry, &plan.request_id)
            }
            Err(other) => Err(other),
        }
    }

    fn acquire_send_slot(
        &self,
        budget: Option<&SendBudget>,
    ) -> Result<MutexGuard<'_, ()>, PresenceError> {
        let Some(budget) = budget else {
            return Ok(self.send_slot.lock().unwrap());
        };
        loop {
            let remaining = budget.remaining()?;
            match self.send_slot.try_lock() {
                Ok(slot) => return Ok(slot),
                Err(TryLockError::WouldBlock) => {
                    std::thread::sleep(remaining.min(Duration::from_millis(1)));
                }
                Err(TryLockError::Poisoned(error)) => panic!("send slot poisoned: {error}"),
            }
        }
    }

    fn attempt(
        &self,
        request: HttpRequest,
        request_id: &str,
    ) -> Result<MutationResponse, PresenceError> {
        let response = self
            .http
            .send(request)
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
        if let Some(error) = &server_error
            && let Some(accepted) = error.accepted_sequence
        {
            self.sequencer.reconcile(accepted);
        }
        if response.status == 426 {
            return Err(PresenceError::SchemaRejected);
        }
        if let Some(error) = &server_error
            && (error.code == ERR_SCHEMA_UNSUPPORTED || error.code == ERR_FEATURE_UNAVAILABLE)
        {
            return Err(PresenceError::SchemaRejected);
        }
        let retryable = server_error
            .as_ref()
            .map(|e| e.retryable)
            .unwrap_or(response.status >= 500);
        if retryable && response.status >= 500 {
            return Err(PresenceError::Transport(format!(
                "retryable {}",
                response.status
            )));
        }
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
    use super::*;
    use chrono::{DateTime, TimeZone, Utc};
    use std::collections::VecDeque;
    use std::sync::mpsc;
    use std::time::Duration;
    use yohaku_protocol::presence::Availability;

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
        FailAfter(Duration),
        Timeout,
        Json(u16, String),
    }

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
            let timeout_ms = request.timeout_ms;
            self.requests.lock().unwrap().push(request);
            let step = self
                .steps
                .lock()
                .unwrap()
                .pop_front()
                .expect("script exhausted");
            match step {
                Step::Fail => Err(TransportError("connection reset".into())),
                Step::FailAfter(delay) => {
                    std::thread::sleep(delay);
                    Err(TransportError("connection reset".into()))
                }
                Step::Timeout => {
                    std::thread::sleep(Duration::from_millis(timeout_ms));
                    Err(TransportError("timed out".into()))
                }
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
        assert!(
            r.headers
                .iter()
                .any(|(k, v)| k == "Authorization" && v == "Bearer token-1")
        );
        assert!(
            r.headers
                .iter()
                .any(|(k, v)| k == "X-Yohaku-Companion-Version" && v == "1.7.3")
        );
        assert!(
            r.headers
                .iter()
                .any(|(k, v)| k == "Content-Type" && v == "application/json")
        );
    }

    #[test]
    fn transport_failure_retries_same_body_once() {
        let http = scripted(vec![Step::Fail, Step::Json(200, mutation_ok())]);
        let c = client(http.clone());
        let mutation = c.replace_presence(input(), &fixed_clock()).unwrap();
        assert_eq!(mutation.accepted_sequence, 100);
        let requests = http.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].method, requests[1].method);
        assert_eq!(requests[0].url, requests[1].url);
        assert_eq!(requests[0].headers, requests[1].headers);
        assert_eq!(requests[0].body, requests[1].body);
        assert_eq!(request_id_of(&requests[0]), request_id_of(&requests[1]));
        assert_eq!(requests[0].timeout_ms, 10_000);
        assert_eq!(requests[1].timeout_ms, 10_000);
    }

    #[test]
    fn transport_failure_twice_is_final() {
        let http = scripted(vec![Step::Fail, Step::Fail]);
        let c = client(http);
        assert!(matches!(
            c.replace_presence(input(), &fixed_clock()),
            Err(PresenceError::Transport(_))
        ));
        assert_eq!(c.sequencer.peek(), 11, "retry must not reserve again");
    }

    #[test]
    fn decode_failure_is_retried_like_transport() {
        let http = scripted(vec![
            Step::Json(200, r#"{"meta":{},"data":{}}"#.into()),
            Step::Json(200, mutation_ok()),
        ]);
        let c = client(http.clone());
        assert_eq!(
            c.replace_presence(input(), &fixed_clock())
                .unwrap()
                .accepted_sequence,
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
        assert_eq!(c.sequencer.peek(), 16);
    }

    #[test]
    fn non_retryable_4xx_is_fatal() {
        let http = scripted(vec![Step::Json(
            400,
            error_json("VALIDATION_FAILED", false, None),
        )]);
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
        let mutation = c
            .clear_presence(ClearReason::Sleep, &fixed_clock())
            .unwrap();
        assert_eq!(mutation.accepted_sequence, 100);
        let body = String::from_utf8(http.requests.lock().unwrap()[0].body.clone().expect("body"))
            .unwrap();
        assert!(body.contains(r#""reason":"sleep""#));
    }

    #[test]
    fn ordinary_clear_retries_same_request_with_full_per_attempt_timeout() {
        let http = scripted(vec![Step::Fail, Step::Fail]);
        let c = client(http.clone());
        assert!(matches!(
            c.clear_presence(ClearReason::Sleep, &fixed_clock()),
            Err(PresenceError::Transport(_))
        ));
        let requests = http.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].method, requests[1].method);
        assert_eq!(requests[0].url, requests[1].url);
        assert_eq!(requests[0].headers, requests[1].headers);
        assert_eq!(requests[0].body, requests[1].body);
        assert_eq!(request_id_of(&requests[0]), request_id_of(&requests[1]));
        assert_eq!(requests[0].timeout_ms, 10_000);
        assert_eq!(requests[1].timeout_ms, 10_000);
        assert_eq!(c.sequencer.peek(), 11, "retry must not reserve again");
    }

    #[test]
    fn mapper_failure_reserves_sequence_without_sending() {
        let http = scripted(vec![]);
        let c = client(http.clone());
        let mut invalid = input();
        invalid.application.as_mut().unwrap().display_name.clear();
        assert!(matches!(
            c.replace_presence(invalid, &fixed_clock()),
            Err(PresenceError::Decode(_))
        ));
        assert!(http.requests.lock().unwrap().is_empty());
        assert_eq!(c.sequencer.peek(), 11);
    }

    #[test]
    fn bounded_clear_retry_uses_remaining_budget() {
        let http = scripted(vec![
            Step::FailAfter(Duration::from_millis(50)),
            Step::Json(200, mutation_ok()),
        ]);
        let c = client(http.clone());
        c.clear_presence_bounded(ClearReason::Sleep, &fixed_clock(), 500)
            .unwrap();
        let requests = http.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert!((1..=500).contains(&requests[0].timeout_ms));
        assert!((1..=450).contains(&requests[1].timeout_ms));
        assert!(requests[1].timeout_ms < requests[0].timeout_ms);
        assert_eq!(requests[0].method, requests[1].method);
        assert_eq!(requests[0].url, requests[1].url);
        assert_eq!(requests[0].headers, requests[1].headers);
        assert_eq!(requests[0].body, requests[1].body);
    }

    #[test]
    fn bounded_clear_does_not_retry_after_budget_expires() {
        let http = scripted(vec![Step::Timeout, Step::Json(200, mutation_ok())]);
        let c = client(http.clone());
        assert!(matches!(
            c.clear_presence_bounded(ClearReason::Shutdown, &fixed_clock(), 50),
            Err(PresenceError::Transport(_))
        ));
        assert_eq!(http.requests.lock().unwrap().len(), 1);
        assert_eq!(c.sequencer.peek(), 11);
    }

    #[test]
    fn bounded_clear_times_out_while_send_slot_is_occupied() {
        let http = scripted(vec![Step::Json(200, mutation_ok())]);
        let c = Arc::new(client(http.clone()));
        let slot = c.send_slot.lock().unwrap();
        let (completed, result) = mpsc::channel();
        let sending_client = c.clone();
        let worker = std::thread::spawn(move || {
            let outcome =
                sending_client.clear_presence_bounded(ClearReason::Shutdown, &fixed_clock(), 50);
            completed.send(outcome).unwrap();
        });
        let before_release = result.recv_timeout(Duration::from_secs(1));
        drop(slot);
        worker.join().unwrap();
        assert!(matches!(
            before_release,
            Ok(Err(PresenceError::Transport(_)))
        ));
        assert!(http.requests.lock().unwrap().is_empty());
    }

    #[test]
    fn bounded_clear_zero_budget_does_not_send() {
        let http = scripted(vec![Step::Json(200, mutation_ok())]);
        let c = client(http.clone());
        assert!(matches!(
            c.clear_presence_bounded(ClearReason::Shutdown, &fixed_clock(), 0),
            Err(PresenceError::Transport(_))
        ));
        assert!(http.requests.lock().unwrap().is_empty());
    }
}

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
