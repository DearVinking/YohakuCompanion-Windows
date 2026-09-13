use chrono::{DateTime, TimeZone, Utc};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};
use yohaku_app::capture::{RawApplicationFocus, RawMediaState};
use yohaku_app::coordinator::{
    Coordinator, CoordinatorDeps, LiveDeskEvent, MediaLookup, PresenceSources, StepOutcome,
    StoreSequencePersistence, run,
};
use yohaku_app::ports::{
    Clock, HttpRequest, HttpResponse, HttpTransport, MonotonicClock, TransportError,
};
use yohaku_app::privacy::PrivacyPipeline;
use yohaku_app::state::{CoordinatorState, LiveDeskStatus};
use yohaku_protocol::sequencer::SequencePersistence;
use yohaku_store::settings::SettingsPatch;
use yohaku_store::{ConnectionStore, SecretStore, StoreResult};

const DEVICE: &str = "11111111-1111-4111-8111-111111111111";
const BASE: &str = "https://core.example.com";

struct FixedClock(DateTime<Utc>);
impl Clock for FixedClock {
    fn now(&self) -> DateTime<Utc> {
        self.0
    }
}

struct MemorySecrets(Mutex<BTreeMap<String, Vec<u8>>>);
impl SecretStore for MemorySecrets {
    fn set(&self, key: &str, value: &[u8]) -> StoreResult<()> {
        self.0.lock().unwrap().insert(key.into(), value.to_vec());
        Ok(())
    }
    fn get(&self, key: &str) -> StoreResult<Option<Vec<u8>>> {
        Ok(self.0.lock().unwrap().get(key).cloned())
    }
    fn remove(&self, key: &str) -> StoreResult<()> {
        self.0.lock().unwrap().remove(key);
        Ok(())
    }
}

struct FakeMonotonic(AtomicU64);

impl FakeMonotonic {
    fn advance(&self, ms: u64) {
        self.0.fetch_add(ms, Ordering::SeqCst);
    }
}

impl MonotonicClock for FakeMonotonic {
    fn now_millis(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}

struct MemoryPersistence(Mutex<BTreeMap<String, i64>>);
impl SequencePersistence for MemoryPersistence {
    fn load_next(&self, device_id: &str) -> Option<i64> {
        self.0.lock().unwrap().get(device_id).copied()
    }
    fn save_next(&self, device_id: &str, next: i64) {
        self.0.lock().unwrap().insert(device_id.into(), next);
    }
}

fn capabilities_body() -> Vec<u8> {
    r#"{"meta":{"schema":"yohaku.companion.presence","schemaVersion":2,"requestId":"22222222-2222-4222-8222-222222222222","serverTime":"2026-01-02T03:04:05.000Z"},
"data":{"minimumClientVersion":"1.7.3","presenceSchemaVersions":[2],"momentSchemaVersions":[1],
"features":{"liveDesk":true,"mediaTimeline":true,"moments":false,"readingSessions":true,"mediaArtwork":true},
"limits":{"presencePayloadBytes":32768,"presenceRequestsPerMinute":10,"presenceLeaseMinSeconds":30,"presenceLeaseMaxSeconds":300,"recommendedHeartbeatSeconds":90,"maximumClockSkewSeconds":60}}}"#
        .as_bytes()
        .to_vec()
}

fn mutation_ok(request_id: &str) -> HttpResponse {
    HttpResponse {
        status: 200,
        body: format!(
            r#"{{"meta":{{"schema":"yohaku.companion.presence","schemaVersion":2,"requestId":"{request_id}","serverTime":"2026-01-02T03:04:05.000Z"}},"data":{{"acceptedSequence":100,"receivedAt":"2026-01-02T03:04:05.000Z","state":{{}}}}}}"#
        )
        .into_bytes(),
    }
}

struct ScriptedHttp {
    get_override: Mutex<Option<Result<HttpResponse, TransportError>>>,
    put_override: Mutex<Option<HttpResponse>>,
    request_started: Mutex<Option<mpsc::Sender<&'static str>>>,
    pub requests: Mutex<Vec<HttpRequest>>,
}

impl HttpTransport for ScriptedHttp {
    fn send(&self, request: HttpRequest) -> Result<HttpResponse, TransportError> {
        self.requests.lock().unwrap().push(request.clone());
        if let Some(notify) = self.request_started.lock().unwrap().as_ref() {
            let _ = notify.send(request.method);
        }
        if request.method == "GET" {
            if let Some(response) = self.get_override.lock().unwrap().take() {
                return response;
            }
            return Ok(HttpResponse {
                status: 200,
                body: capabilities_body(),
            });
        }
        if let Some(over) = self.put_override.lock().unwrap().take() {
            return Ok(over);
        }
        let body = String::from_utf8(request.body.unwrap()).unwrap();
        let start = body.find("\"requestId\":\"").unwrap() + 13;
        Ok(mutation_ok(&body[start..start + 36]))
    }
}

struct FakeSources {
    app: Mutex<Option<RawApplicationFocus>>,
    media: Mutex<MediaLookup>,
    calls: Mutex<Vec<&'static str>>,
}

impl PresenceSources for FakeSources {
    fn current_application(&self) -> Option<RawApplicationFocus> {
        self.calls.lock().unwrap().push("application");
        self.app.lock().unwrap().clone()
    }
    fn current_media(&self) -> MediaLookup {
        self.calls.lock().unwrap().push("media");
        match &mut *self.media.lock().unwrap() {
            MediaLookup::Session(state) => MediaLookup::Session(state.clone()),
            MediaLookup::NoSession => MediaLookup::NoSession,
            MediaLookup::Unavailable => MediaLookup::Unavailable,
        }
    }
}

struct Harness {
    monotonic: Arc<FakeMonotonic>,
    _dir: tempfile::TempDir,
    coordinator: Coordinator,
    status: Arc<Mutex<LiveDeskStatus>>,
    http: Arc<ScriptedHttp>,
    sources: Arc<FakeSources>,
    connection: Arc<ConnectionStore>,
    pipeline: Arc<PrivacyPipeline>,
}

fn app_sample() -> RawApplicationFocus {
    RawApplicationFocus {
        application_key: "msedge.exe".into(),
        display_name: "Microsoft Edge".into(),
        window_title: Some("文档".into()),
    }
}

fn media_sample() -> RawMediaState {
    RawMediaState::new(
        Some("歌名".into()),
        Some("歌手".into()),
        None,
        "cloudmusic.exe".into(),
        "网易云音乐".into(),
        true,
        Some(200.0),
        Some(30.0),
        Some(Utc.with_ymd_and_hms(2026, 1, 2, 3, 4, 0).unwrap()),
    )
}

fn harness() -> Harness {
    let dir = tempfile::tempdir().unwrap();
    let connection = Arc::new(ConnectionStore::new(
        dir.path(),
        Arc::new(MemorySecrets(Mutex::new(BTreeMap::new()))),
    ));
    connection
        .install_pairing_claim(
            DEVICE,
            "token-1",
            &["companion:presence:write".into()],
            5,
            BASE,
        )
        .unwrap();
    connection
        .update_metadata(|m| m.is_live_desk_enabled = true)
        .unwrap();

    let status = Arc::new(Mutex::new(LiveDeskStatus::default()));
    let http = Arc::new(ScriptedHttp {
        get_override: Mutex::new(None),
        put_override: Mutex::new(None),
        request_started: Mutex::new(None),
        requests: Mutex::new(Vec::new()),
    });
    let sources = Arc::new(FakeSources {
        app: Mutex::new(Some(app_sample())),
        media: Mutex::new(MediaLookup::Session(Box::new(media_sample()))),
        calls: Mutex::new(Vec::new()),
    });
    let pipeline = Arc::new(PrivacyPipeline::new(Default::default(), Default::default()));
    let monotonic = Arc::new(FakeMonotonic(AtomicU64::new(0)));
    let deps = CoordinatorDeps {
        connection: connection.clone(),
        pipeline: pipeline.clone(),
        sources: sources.clone(),
        http: http.clone(),
        clock: Arc::new(FixedClock(
            Utc.with_ymd_and_hms(2026, 1, 2, 3, 4, 5).unwrap(),
        )),
        monotonic: monotonic.clone(),
        sequence_persistence: Arc::new(MemoryPersistence(Mutex::new(BTreeMap::new()))),
    };
    Harness {
        monotonic,
        _dir: dir,
        coordinator: Coordinator::new(deps, status.clone()),
        status,
        http,
        sources,
        connection,
        pipeline,
    }
}

impl Harness {
    fn bring_online(&mut self) {
        assert_eq!(
            self.coordinator.step(Some(LiveDeskEvent::SettingsChanged)),
            StepOutcome::Continue
        );
        assert_eq!(
            *self.status.lock().unwrap(),
            LiveDeskStatus {
                state: CoordinatorState::Connecting,
                ..LiveDeskStatus::default()
            }
        );
        self.coordinator.step(None);
        assert_eq!(
            *self.status.lock().unwrap(),
            LiveDeskStatus {
                last_sent_at: None,
                ..online_status(CoordinatorState::Active, None)
            }
        );
        let sent_before = self.http.requests.lock().unwrap().len();
        self.tick_after_interval();
        assert!(self.http.requests.lock().unwrap().len() > sent_before);
        assert_eq!(
            *self.status.lock().unwrap(),
            online_status(CoordinatorState::Active, None)
        );
    }

    fn tick_after_interval(&mut self) {
        self.monotonic.advance(6_000);
        self.coordinator.step(None);
    }

    fn put_bodies(&self) -> Vec<String> {
        self.http
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.method == "PUT")
            .filter_map(|r| r.body.clone())
            .map(|b| String::from_utf8(b).unwrap())
            .collect()
    }
}

fn run_and_observe(
    coordinator: Coordinator,
    http: &ScriptedHttp,
    event: LiveDeskEvent,
    request_count: usize,
) -> Vec<&'static str> {
    let (notify, requests) = mpsc::channel();
    *http.request_started.lock().unwrap() = Some(notify);
    let (sender, events) = mpsc::channel();
    let worker = std::thread::spawn(move || run(coordinator, events));
    sender.send(event).unwrap();
    let observed = (0..request_count)
        .map_while(|_| requests.recv_timeout(Duration::from_secs(2)).ok())
        .collect();
    let _ = sender.send(LiveDeskEvent::Shutdown);
    worker.join().unwrap();
    observed
}

fn online_status(state: CoordinatorState, error_code: Option<&str>) -> LiveDeskStatus {
    LiveDeskStatus {
        state,
        last_error_code: error_code.map(str::to_string),
        last_sent_at: Some("2026-01-02T03:04:05.000Z".into()),
        media_capable: true,
        server_base_url: Some(BASE.into()),
        device_id: Some(DEVICE.into()),
    }
}

#[test]
fn full_lifecycle_online_send() {
    let mut h = harness();
    assert_eq!(*h.status.lock().unwrap(), LiveDeskStatus::default());
    h.bring_online();
    let bodies = h.put_bodies();
    assert!(bodies[0].contains(r#""displayName":"Microsoft Edge""#));
    assert!(bodies[0].contains(r#""title":"歌名""#));
    let payload: serde_json::Value = serde_json::from_str(&bodies[0]).unwrap();
    assert_eq!(
        payload["data"]["application"].get("icon"),
        Some(&serde_json::Value::Null)
    );
    assert_eq!(
        payload["data"]["media"].get("artwork"),
        Some(&serde_json::Value::Null)
    );
    let requests = h.http.requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    assert_eq!(
        requests[0].url,
        "https://core.example.com/companion/capabilities"
    );
    assert_eq!(
        requests[1].url,
        "https://core.example.com/companion/presence"
    );
    assert!(bodies[0].contains(r#""sampledAt":"2026-01-02T03:04:00.000Z""#));
    assert_eq!(*h.sources.calls.lock().unwrap(), ["application", "media"]);
}

#[test]
fn sequence_persistence_never_lowers_the_next_sequence_for_the_same_device() {
    let h = harness();
    let persistence = StoreSequencePersistence {
        store: h.connection.clone(),
    };
    persistence.save_next(DEVICE, 30);
    persistence.save_next(DEVICE, 10);
    assert_eq!(persistence.load_next(DEVICE), Some(30));
    persistence.save_next("22222222-2222-4222-8222-222222222222", 80);
    assert_eq!(persistence.load_next(DEVICE), Some(30));
    persistence.save_next(DEVICE, 35);
    assert_eq!(persistence.load_next(DEVICE), Some(35));
}

#[test]
fn coalesces_multiple_semantic_events_into_one_send() {
    let mut h = harness();
    h.bring_online();
    let before = h.http.requests.lock().unwrap().len();
    h.coordinator.step(Some(LiveDeskEvent::AppChanged));
    h.coordinator
        .step(Some(LiveDeskEvent::MediaSemanticChanged));
    h.tick_after_interval();
    assert_eq!(h.http.requests.lock().unwrap().len(), before + 1);
}

#[test]
fn sleep_clears_then_wake_renegotiates() {
    let mut h = harness();
    h.bring_online();
    let before = h.http.requests.lock().unwrap().len();
    h.monotonic.advance(30_000);
    h.coordinator.step(Some(LiveDeskEvent::SleepOrLock));
    assert_eq!(
        *h.status.lock().unwrap(),
        online_status(CoordinatorState::Suspended, None)
    );
    assert_eq!(*h.sources.calls.lock().unwrap(), ["application", "media"]);
    let bodies = h.put_bodies();
    assert!(bodies.iter().any(|b| b.contains(r#""reason":"sleep""#)));
    assert_eq!(h.http.requests.lock().unwrap().len(), before + 1);
    h.coordinator.step(Some(LiveDeskEvent::Wake));
    assert_eq!(
        *h.status.lock().unwrap(),
        online_status(CoordinatorState::Connecting, None)
    );
    h.coordinator.step(None);
    assert_eq!(
        *h.status.lock().unwrap(),
        online_status(CoordinatorState::Active, None)
    );
}

#[test]
fn schema_rejection_triggers_renegotiation() {
    let mut h = harness();
    h.bring_online();
    *h.http.put_override.lock().unwrap() = Some(HttpResponse {
        status: 426,
        body: br#"{"meta":{},"error":{"code":"OTHER","message":"m","retryable":false,"retryAfterMs":null,"acceptedSequence":null,"fields":[]}}"#.to_vec(),
    });
    h.coordinator.step(Some(LiveDeskEvent::AppChanged));
    h.tick_after_interval();
    assert_eq!(
        *h.status.lock().unwrap(),
        online_status(CoordinatorState::Connecting, Some("SCHEMA_REJECTED"))
    );
    h.coordinator.step(None);
    assert_eq!(
        *h.status.lock().unwrap(),
        online_status(CoordinatorState::Active, None)
    );
    let before = h.http.requests.lock().unwrap().len();
    h.coordinator.step(Some(LiveDeskEvent::AppChanged));
    h.tick_after_interval();
    assert!(h.http.requests.lock().unwrap().len() > before);
}

#[test]
fn pause_suspends_with_clear_and_resume_renegotiates() {
    let mut h = harness();
    h.bring_online();
    h.pipeline.update_settings(&SettingsPatch {
        pause_sharing: Some(true),
        ..Default::default()
    });
    h.coordinator.step(Some(LiveDeskEvent::SettingsChanged));
    assert_eq!(
        *h.status.lock().unwrap(),
        online_status(CoordinatorState::Suspended, None)
    );
    let bodies = h.put_bodies();
    assert!(bodies.iter().any(|b| b.contains(r#""reason":"paused""#)));
    h.pipeline.update_settings(&SettingsPatch {
        pause_sharing: Some(false),
        ..Default::default()
    });
    h.coordinator.step(Some(LiveDeskEvent::SettingsChanged));
    assert_eq!(
        *h.status.lock().unwrap(),
        online_status(CoordinatorState::Connecting, None)
    );
    h.coordinator.step(None);
    assert_eq!(
        *h.status.lock().unwrap(),
        online_status(CoordinatorState::Active, None)
    );
}

#[test]
fn heartbeat_waits_until_due_and_captures_fresh_sources() {
    let mut h = harness();
    h.bring_online();
    h.monotonic.advance(29_999);
    h.coordinator.step(None);
    assert_eq!(h.put_bodies().len(), 1);
    assert_eq!(h.coordinator.current_wait(), Duration::from_millis(1));
    h.sources.app.lock().unwrap().as_mut().unwrap().display_name = "New application".into();
    let mut media = media_sample();
    media.title = Some("New song".into());
    *h.sources.media.lock().unwrap() = MediaLookup::Session(Box::new(media));
    h.monotonic.advance(1);
    h.coordinator.step(None);
    let bodies = h.put_bodies();
    assert_eq!(bodies.len(), 2);
    let payload: serde_json::Value = serde_json::from_str(&bodies[1]).unwrap();
    assert_eq!(
        payload["data"]["application"]["displayName"],
        "New application"
    );
    assert_eq!(payload["data"]["media"]["title"], "New song");
    assert_eq!(
        *h.sources.calls.lock().unwrap(),
        ["application", "media", "application", "media"]
    );
    assert_eq!(h.coordinator.current_wait(), Duration::from_secs(30));
}

#[test]
fn failed_send_consumes_interval_and_next_send_captures_fresh_sources() {
    let mut h = harness();
    h.bring_online();
    *h.http.put_override.lock().unwrap() = Some(HttpResponse {
        status: 400,
        body: br#"{"meta":{},"error":{"code":"POLICY_REJECTED","message":"m","retryable":false,"retryAfterMs":null,"acceptedSequence":null,"fields":[]}}"#.to_vec(),
    });
    h.monotonic.advance(6_000);
    h.coordinator.step(Some(LiveDeskEvent::AppChanged));
    assert_eq!(h.put_bodies().len(), 2);
    assert_eq!(
        *h.status.lock().unwrap(),
        online_status(CoordinatorState::Degraded, Some("SERVER"))
    );

    h.sources.app.lock().unwrap().as_mut().unwrap().display_name = "After failure".into();
    h.coordinator.step(Some(LiveDeskEvent::AppChanged));
    assert_eq!(h.coordinator.current_wait(), Duration::from_secs(6));
    h.monotonic.advance(5_999);
    h.coordinator.step(None);
    assert_eq!(h.put_bodies().len(), 2);
    h.monotonic.advance(1);
    h.coordinator.step(None);
    let bodies = h.put_bodies();
    assert_eq!(bodies.len(), 3);
    let payload: serde_json::Value = serde_json::from_str(&bodies[2]).unwrap();
    assert_eq!(
        payload["data"]["application"]["displayName"],
        "After failure"
    );
    assert_eq!(
        *h.sources.calls.lock().unwrap(),
        [
            "application",
            "media",
            "application",
            "media",
            "application",
            "media"
        ]
    );
    assert_eq!(
        *h.status.lock().unwrap(),
        online_status(CoordinatorState::Active, None)
    );
}

#[test]
fn waiting_retries_at_deadline_or_on_network_and_policy_events() {
    for event in [
        None,
        Some(LiveDeskEvent::NetworkUp),
        Some(LiveDeskEvent::PolicyChanged),
    ] {
        let mut h = harness();
        *h.http.get_override.lock().unwrap() = Some(Err(TransportError("offline".into())));
        h.coordinator.step(Some(LiveDeskEvent::SettingsChanged));
        h.coordinator.step(None);
        assert_eq!(
            *h.status.lock().unwrap(),
            LiveDeskStatus {
                state: CoordinatorState::Degraded,
                last_error_code: Some("CAPABILITIES_UNAVAILABLE".into()),
                ..LiveDeskStatus::default()
            }
        );
        assert_eq!(h.coordinator.current_wait(), Duration::from_secs(30));
        h.monotonic.advance(29_000);
        h.coordinator.step(Some(LiveDeskEvent::AppChanged));
        h.coordinator
            .step(Some(LiveDeskEvent::MediaSemanticChanged));
        h.coordinator.step(None);
        assert_eq!(h.http.requests.lock().unwrap().len(), 1);
        assert_eq!(h.coordinator.current_wait(), Duration::from_secs(1));
        if event.is_none() {
            h.monotonic.advance(1_000);
        }
        h.coordinator.step(event);
        assert_eq!(h.http.requests.lock().unwrap().len(), 2);
        assert_eq!(
            *h.status.lock().unwrap(),
            LiveDeskStatus {
                last_sent_at: None,
                ..online_status(CoordinatorState::Active, None)
            }
        );
        assert_eq!(h.coordinator.current_wait(), Duration::ZERO);
        assert!(h.sources.calls.lock().unwrap().is_empty());
    }
}

#[test]
fn negotiation_failures_preserve_unrelated_status_fields() {
    let caps = String::from_utf8(capabilities_body()).unwrap();
    for (body, state) in [
        (
            caps.replace("\"1.7.3\"", "\"9.0.0\""),
            CoordinatorState::UpdateRequired,
        ),
        (
            caps.replace("\"liveDesk\":true", "\"liveDesk\":false"),
            CoordinatorState::ServerFeatureUnavailable,
        ),
        (
            caps.replace(
                "\"presenceSchemaVersions\":[2]",
                "\"presenceSchemaVersions\":[1]",
            ),
            CoordinatorState::ServerFeatureUnavailable,
        ),
        ("{}".into(), CoordinatorState::Degraded),
    ] {
        let mut h = harness();
        h.bring_online();
        *h.http.put_override.lock().unwrap() = Some(HttpResponse {
            status: 426,
            body: Vec::new(),
        });
        h.coordinator.step(Some(LiveDeskEvent::AppChanged));
        h.tick_after_interval();
        *h.http.get_override.lock().unwrap() = Some(Ok(HttpResponse {
            status: 200,
            body: body.into_bytes(),
        }));
        h.coordinator.step(None);
        assert_eq!(
            *h.status.lock().unwrap(),
            online_status(state, Some("SCHEMA_REJECTED"))
        );
        assert_eq!(h.coordinator.current_wait(), Duration::from_secs(300));
    }
}

#[test]
fn unavailable_media_uses_cache_with_stripped_timeline() {
    let mut h = harness();
    h.bring_online();
    assert!(h.put_bodies()[0].contains(r#""positionMs":30000"#));
    *h.sources.media.lock().unwrap() = MediaLookup::Unavailable;
    let first_sid = extract(&h.put_bodies()[0], "sessionId");
    for _ in 0..2 {
        h.coordinator
            .step(Some(LiveDeskEvent::MediaSemanticChanged));
        h.tick_after_interval();
        let bodies = h.put_bodies();
        let last = bodies.last().unwrap();
        let payload: serde_json::Value = serde_json::from_str(last).unwrap();
        let media = &payload["data"]["media"];
        assert_eq!(media["title"], "歌名");
        assert_eq!(media["artist"], "歌手");
        assert_eq!(media["playback"]["durationMs"], 200_000);
        assert_eq!(media["playback"]["positionMs"], serde_json::Value::Null);
        assert_ne!(media["playback"]["sampledAt"], "2026-01-02T03:04:00.000Z");
        assert_eq!(extract(last, "sessionId"), first_sid);
    }
}

#[test]
fn no_session_clears_cached_media_for_later_unavailable_samples() {
    let mut h = harness();
    h.bring_online();
    for lookup in [MediaLookup::NoSession, MediaLookup::Unavailable] {
        *h.sources.media.lock().unwrap() = lookup;
        h.coordinator
            .step(Some(LiveDeskEvent::MediaSemanticChanged));
        h.tick_after_interval();
        let bodies = h.put_bodies();
        let payload: serde_json::Value = serde_json::from_str(bodies.last().unwrap()).unwrap();
        assert_eq!(payload["data"]["media"], serde_json::Value::Null);
        assert_eq!(payload["data"]["availability"], "active");
    }
}

#[test]
fn settings_changes_refresh_the_latest_privacy_result_at_the_next_send_slot() {
    let mut h = harness();
    h.bring_online();
    h.pipeline.update_settings(&SettingsPatch {
        share_applications: Some(false),
        ..Default::default()
    });
    h.coordinator.step(Some(LiveDeskEvent::SettingsChanged));
    assert_eq!(h.put_bodies().len(), 1);
    assert_eq!(h.coordinator.current_wait(), Duration::from_secs(6));
    h.tick_after_interval();
    let bodies = h.put_bodies();
    assert_eq!(bodies.len(), 2);
    let payload: serde_json::Value = serde_json::from_str(&bodies[1]).unwrap();
    assert_eq!(payload["data"]["application"], serde_json::Value::Null);
    assert_eq!(payload["data"]["media"]["title"], "歌名");

    h.pipeline.update_settings(&SettingsPatch {
        share_media: Some(false),
        ..Default::default()
    });
    h.coordinator.step(Some(LiveDeskEvent::SettingsChanged));
    h.tick_after_interval();
    let bodies = h.put_bodies();
    assert_eq!(bodies.len(), 3);
    let payload: serde_json::Value = serde_json::from_str(&bodies[2]).unwrap();
    assert_eq!(payload["data"]["application"], serde_json::Value::Null);
    assert_eq!(payload["data"]["media"], serde_json::Value::Null);
    assert_eq!(payload["data"]["availability"], "idle");
    assert_eq!(
        h.http
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.method == "GET")
            .count(),
        1
    );
}

#[test]
fn capabilities_from_non_success_status_are_rejected() {
    for status in [199, 300, 401, 409, 426, 429, 500, 503] {
        let mut h = harness();
        *h.http.get_override.lock().unwrap() = Some(Ok(HttpResponse {
            status,
            body: capabilities_body(),
        }));
        h.coordinator.step(Some(LiveDeskEvent::SettingsChanged));
        h.coordinator.step(None);
        assert_eq!(
            *h.status.lock().unwrap(),
            LiveDeskStatus {
                state: CoordinatorState::Degraded,
                last_error_code: Some("CAPABILITIES_UNAVAILABLE".into()),
                ..LiveDeskStatus::default()
            },
            "HTTP {status} must not grant presence authority"
        );
        assert_eq!(h.coordinator.current_wait(), Duration::from_secs(30));
        h.tick_after_interval();
        assert!(h.put_bodies().is_empty());
    }
}

#[test]
fn run_connects_after_settings_without_another_event() {
    let h = harness();
    assert_eq!(
        run_and_observe(h.coordinator, &h.http, LiveDeskEvent::SettingsChanged, 2),
        ["GET", "PUT"]
    );
}

#[test]
fn run_reconnects_after_wake_without_another_event() {
    let mut h = harness();
    h.bring_online();
    h.coordinator.step(Some(LiveDeskEvent::SleepOrLock));
    assert_eq!(
        run_and_observe(h.coordinator, &h.http, LiveDeskEvent::Wake, 2),
        ["GET", "PUT"]
    );
}

#[test]
fn run_renegotiates_rejected_schema_without_another_event() {
    let mut h = harness();
    h.bring_online();
    *h.http.put_override.lock().unwrap() = Some(HttpResponse {
        status: 426,
        body: Vec::new(),
    });
    h.monotonic.advance(6_000);
    assert_eq!(
        run_and_observe(h.coordinator, &h.http, LiveDeskEvent::AppChanged, 3),
        ["PUT", "GET", "PUT"]
    );
}

#[test]
fn run_exits_when_event_channel_disconnects() {
    const CHILD: &str = "YOHAKU_TEST_DISCONNECTED_RUN";
    if std::env::var_os(CHILD).is_some() {
        let h = harness();
        let (sender, events) = mpsc::channel();
        drop(sender);
        run(h.coordinator, events);
        return;
    }

    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "run_exits_when_event_channel_disconnects"])
        .env(CHILD, "1")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success());
            return;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("run did not exit after the event channel disconnected");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn privacy_hidden_app_yields_null_application() {
    let mut h = harness();
    let mut rules = yohaku_store::privacy::PrivacyRules::default();
    rules.apps.insert(
        "msedge.exe".into(),
        yohaku_store::privacy::AppRule {
            application: yohaku_store::privacy::Level::Hide,
            window_title: yohaku_store::privacy::Level::Hide,
            media: yohaku_store::privacy::Level::Inherit,
            display_alias: None,
        },
    );
    h.pipeline.replace_rules(rules);
    assert!(h.sources.app.lock().unwrap().is_some());
    h.bring_online();
    let bodies = h.put_bodies();
    assert!(bodies[0].contains(r#""application":null"#));
    assert!(bodies[0].contains(r#""title":"歌名""#));
    assert!(bodies[0].contains(r#""availability":"active""#));
}

#[test]
fn shutdown_clears_and_stops() {
    let mut h = harness();
    h.bring_online();
    h.coordinator.step(Some(LiveDeskEvent::AppChanged));
    h.monotonic.advance(30_000);
    assert_eq!(
        h.coordinator.step(Some(LiveDeskEvent::Shutdown)),
        StepOutcome::Shutdown
    );
    let bodies = h.put_bodies();
    assert!(bodies.iter().any(|b| b.contains(r#""reason":"shutdown""#)));
    assert_eq!(*h.status.lock().unwrap(), LiveDeskStatus::default());
    assert_eq!(*h.sources.calls.lock().unwrap(), ["application", "media"]);
}

#[test]
fn disable_stops_sending() {
    let mut h = harness();
    h.bring_online();
    h.connection
        .update_metadata(|m| m.is_live_desk_enabled = false)
        .unwrap();
    h.coordinator.step(Some(LiveDeskEvent::SettingsChanged));
    assert_eq!(
        *h.status.lock().unwrap(),
        LiveDeskStatus {
            last_sent_at: Some("2026-01-02T03:04:05.000Z".into()),
            ..LiveDeskStatus::default()
        }
    );
    let before = h.http.requests.lock().unwrap().len();
    h.coordinator.step(Some(LiveDeskEvent::AppChanged));
    h.coordinator.step(None);
    assert_eq!(h.http.requests.lock().unwrap().len(), before);
}

fn extract(body: &str, key: &str) -> String {
    let needle = format!("\"{key}\":\"");
    let start = body.find(&needle).unwrap() + needle.len();
    body[start..start + 36].to_string()
}
