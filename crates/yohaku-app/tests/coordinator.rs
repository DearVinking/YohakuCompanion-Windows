//! 协调器状态机集成测试：假源/假传输/假时钟/假资产，直接驱动 step()。

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use yohaku_app::coordinator::{
    Coordinator, CoordinatorDeps, LiveDeskEvent, MediaLookup, PresenceSources, StepOutcome,
};
use yohaku_app::ports::{Clock, HttpTransport, HttpRequest, HttpResponse, MonotonicClock, TransportError};
use yohaku_app::capture::{RawApplicationFocus, RawMediaState};
use yohaku_app::privacy::PrivacyPipeline;
use yohaku_app::state::{CoordinatorState, LiveDeskStatus};
use yohaku_protocol::sequencer::SequencePersistence;
use yohaku_store::history::HistoryStore;
use yohaku_store::settings::SettingsPatch;
use yohaku_store::{ConnectionStore, SecretStore, StoreResult};
use chrono::{DateTime, TimeZone, Utc};

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

/// 可推进的假单调时钟（毫秒）。
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
    format!(
        r#"{{"meta":{{"schema":"yohaku.companion.presence","schemaVersion":2,"requestId":"22222222-2222-4222-8222-222222222222","serverTime":"2026-01-02T03:04:05.000Z"}},
"data":{{"minimumClientVersion":"1.7.3","presenceSchemaVersions":[2],"momentSchemaVersions":[1],
"features":{{"liveDesk":true,"mediaTimeline":true,"moments":false,"readingSessions":true,"mediaArtwork":true}},
"limits":{{"presencePayloadBytes":32768,"presenceRequestsPerMinute":10,"presenceLeaseMinSeconds":30,"presenceLeaseMaxSeconds":300,"recommendedHeartbeatSeconds":90,"maximumClockSkewSeconds":60}}}}}}"#
    )
    .into_bytes()
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

/// 按方法分发：GET → capabilities；PUT → 200 mutation（回显 requestId）。
/// 可注入一个覆盖脚本（如 426）。
struct ScriptedHttp {
    put_override: Mutex<Option<HttpResponse>>,
    pub requests: Mutex<Vec<HttpRequest>>,
}

impl HttpTransport for ScriptedHttp {
    fn send(&self, request: HttpRequest) -> Result<HttpResponse, TransportError> {
        self.requests.lock().unwrap().push(request.clone());
        if request.method == "GET" {
            return Ok(HttpResponse { status: 200, body: capabilities_body() });
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
}

impl PresenceSources for FakeSources {
    fn current_application(&self) -> Option<RawApplicationFocus> {
        self.app.lock().unwrap().clone()
    }
    fn current_media(&self) -> MediaLookup {
        match &mut *self.media.lock().unwrap() {
            MediaLookup::Session(state) => MediaLookup::Session(state.clone()),
            MediaLookup::NoSession => MediaLookup::NoSession,
            MediaLookup::Unavailable => MediaLookup::Unavailable,
        }
    }
}

struct FakeAssets;
impl yohaku_app::coordinator::AssetHosting for FakeAssets {
    fn host_media_artwork(&self, _bytes: &[u8], _device_id: &str) -> Option<String> {
        Some("https://assets.example.com/m/current.png?v=ab".into())
    }
    fn host_application_icon(&self, _key: &str) -> Option<String> {
        Some("https://assets.example.com/icons/x.png".into())
    }
    fn allowed_hosts(&self) -> Vec<String> {
        vec!["assets.example.com".into()]
    }
}

struct Harness {
    monotonic: Arc<FakeMonotonic>,
    _dir: tempfile::TempDir,
    coordinator: Coordinator,
    status: Arc<Mutex<LiveDeskStatus>>,
    http: Arc<ScriptedHttp>,
    sources: Arc<FakeSources>,
    history_dir: PathBuf,
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
        None,
    )
}

fn harness() -> Harness {
    let dir = tempfile::tempdir().unwrap();
    let connection = Arc::new(ConnectionStore::new(
        dir.path(),
        Arc::new(MemorySecrets(Mutex::new(BTreeMap::new()))),
    ));
    connection
        .install_pairing_claim(DEVICE, "token-1", &["companion:presence:write".into()], 5, BASE)
        .unwrap();
    connection
        .update_metadata(|m| m.is_live_desk_enabled = true)
        .unwrap();

    let status = Arc::new(Mutex::new(LiveDeskStatus::default()));
    let http = Arc::new(ScriptedHttp {
        put_override: Mutex::new(None),
        requests: Mutex::new(Vec::new()),
    });
    let sources = Arc::new(FakeSources {
        app: Mutex::new(Some(app_sample())),
        media: Mutex::new(MediaLookup::Session(Box::new(media_sample()))),
    });
    let pipeline = Arc::new(PrivacyPipeline::new(Default::default(), Default::default()));
    let monotonic = Arc::new(FakeMonotonic(AtomicU64::new(0)));
    let history_dir = dir.path().to_path_buf();
    let deps = CoordinatorDeps {
        connection: connection.clone(),
        pipeline: pipeline.clone(),
        sources: sources.clone(),
        assets: Arc::new(FakeAssets),
        http: http.clone(),
        clock: Arc::new(FixedClock(Utc.with_ymd_and_hms(2026, 1, 2, 3, 4, 5).unwrap())),
        monotonic: monotonic.clone(),
        history: Arc::new(HistoryStore::default()),
        history_dir: history_dir.clone(),
        sequence_persistence: Arc::new(MemoryPersistence(Mutex::new(BTreeMap::new()))),
    };
    Harness {
        monotonic,
        _dir: dir,
        coordinator: Coordinator::new(deps, status.clone()),
        status,
        http,
        sources,
        history_dir,
        connection,
        pipeline,
    }
}

impl Harness {
    /// 标准上线序列：SettingsChanged → Connecting → tick 协商 → Active → tick 首发快照。
    fn bring_online(&mut self) {
        assert_eq!(self.coordinator.step(Some(LiveDeskEvent::SettingsChanged)), StepOutcome::Continue);
        assert_eq!(self.status.lock().unwrap().state, CoordinatorState::Connecting);
        self.coordinator.step(None);
        assert_eq!(self.status.lock().unwrap().state, CoordinatorState::Active);
        let sent_before = self.http.requests.lock().unwrap().len();
        self.tick_after_interval();
        assert!(self.http.requests.lock().unwrap().len() > sent_before);
    }

    /// 推进单调时钟越过限速间隔并 tick 一次（触发待发快照）。
    fn tick_after_interval(&mut self) {
        self.monotonic.advance(6_000); // caps rpm=10 → 间隔 6s
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

#[test]
fn full_lifecycle_online_send_and_history() {
    let mut h = harness();
    assert_eq!(h.status.lock().unwrap().state, CoordinatorState::Disabled);
    h.bring_online();
    let bodies = h.put_bodies();
    assert!(bodies[0].contains(r#""displayName":"Microsoft Edge""#));
    assert!(bodies[0].contains(r#""title":"歌名""#));
    assert!(bodies[0].contains(r#""icon":{"url":"https://assets.example.com/icons/x.png"}"#));
    // 位置采样时间为已知值
    assert!(bodies[0].contains(r#""sampledAt":"2026-01-02T03:04:00.000Z""#));
    let history = HistoryStore::default().list(&h.history_dir).unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].state, yohaku_store::history::SyncState::Succeeded);
    assert_eq!(history[0].output_summary.as_deref(), Some("accepted seq 100"));
}

#[test]
fn coalesces_multiple_semantic_events_into_one_send() {
    let mut h = harness();
    h.bring_online();
    let before = h.http.requests.lock().unwrap().len();
    h.coordinator.step(Some(LiveDeskEvent::AppChanged));
    h.coordinator.step(Some(LiveDeskEvent::MediaSemanticChanged));
    h.tick_after_interval();
    assert_eq!(h.http.requests.lock().unwrap().len(), before + 1);
}

#[test]
fn sleep_clears_then_wake_renegotiates() {
    let mut h = harness();
    h.bring_online();
    let before = h.http.requests.lock().unwrap().len();
    h.coordinator.step(Some(LiveDeskEvent::SleepOrLock));
    assert_eq!(h.status.lock().unwrap().state, CoordinatorState::Suspended);
    // 清除请求已发出（PUT，reason=sleep）
    let bodies = h.put_bodies();
    assert!(bodies.iter().any(|b| b.contains(r#""reason":"sleep""#)));
    assert_eq!(h.http.requests.lock().unwrap().len(), before + 1);
    // 唤醒：重协商 → Active → 首发快照
    h.coordinator.step(Some(LiveDeskEvent::Wake));
    assert_eq!(h.status.lock().unwrap().state, CoordinatorState::Connecting);
    h.coordinator.step(None);
    assert_eq!(h.status.lock().unwrap().state, CoordinatorState::Active);
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
    // 发送失败 → 立即进入重协商
    assert_eq!(h.status.lock().unwrap().state, CoordinatorState::Connecting);
    assert_eq!(h.status.lock().unwrap().last_error_code.as_deref(), Some("SCHEMA_REJECTED"));
    // 下一个 tick 完成重协商 → Active
    h.coordinator.step(None);
    assert_eq!(h.status.lock().unwrap().state, CoordinatorState::Active);
    // 再来一次事件，且 override 已清空 → 正常发送
    let before = h.http.requests.lock().unwrap().len();
    h.coordinator.step(Some(LiveDeskEvent::AppChanged));
    h.tick_after_interval();
    assert!(h.http.requests.lock().unwrap().len() > before);
}

#[test]
fn pause_suspends_with_clear_and_resume_renegotiates() {
    let mut h = harness();
    h.bring_online();
    h.pipeline.update_settings(&SettingsPatch { pause_sharing: Some(true), ..Default::default() });
    h.coordinator.step(Some(LiveDeskEvent::SettingsChanged));
    assert_eq!(h.status.lock().unwrap().state, CoordinatorState::Suspended);
    let bodies = h.put_bodies();
    assert!(bodies.iter().any(|b| b.contains(r#""reason":"paused""#)));
    // 恢复 → 重协商 → Active
    h.pipeline.update_settings(&SettingsPatch { pause_sharing: Some(false), ..Default::default() });
    h.coordinator.step(Some(LiveDeskEvent::SettingsChanged));
    assert_eq!(h.status.lock().unwrap().state, CoordinatorState::Connecting);
    h.coordinator.step(None);
    assert_eq!(h.status.lock().unwrap().state, CoordinatorState::Active);
}

#[test]
fn unavailable_media_uses_cache_with_stripped_timeline() {
    let mut h = harness();
    h.bring_online();
    // 首次发送：位置存在
    assert!(h.put_bodies()[0].contains(r#""positionMs":30000"#));
    // 媒体查询失败 → 缓存沿用但位置不上报
    *h.sources.media.lock().unwrap() = MediaLookup::Unavailable;
    h.coordinator.step(Some(LiveDeskEvent::MediaSemanticChanged));
    h.tick_after_interval();
    let bodies = h.put_bodies();
    let last = bodies.last().unwrap();
    assert!(last.contains(r#""positionMs":null"#));
    // 会话身份不变（同一缓存身份 → 同 sessionId）
    let first_sid = extract(&bodies[0], "sessionId");
    let last_sid = extract(last, "sessionId");
    assert_eq!(first_sid, last_sid);
}

#[test]
fn privacy_hidden_app_yields_null_application() {
    let mut h = harness();
    // 隐藏该应用
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
    // 无应用源：application 显式 null，媒体仍在 → availability 仍 active
    h.sources.app.lock().unwrap().take();
    h.bring_online();
    let bodies = h.put_bodies();
    assert!(bodies[0].contains(r#""application":null"#));
    assert!(bodies[0].contains(r#""availability":"active""#));
}

#[test]
fn shutdown_clears_and_stops() {
    let mut h = harness();
    h.bring_online();
    h.coordinator.step(Some(LiveDeskEvent::Shutdown));
    let bodies = h.put_bodies();
    assert!(bodies.iter().any(|b| b.contains(r#""reason":"shutdown""#)));
    assert_eq!(h.status.lock().unwrap().state, CoordinatorState::Disabled);
}

#[test]
fn disable_stops_sending() {
    let mut h = harness();
    h.bring_online();
    h.connection
        .update_metadata(|m| m.is_live_desk_enabled = false)
        .unwrap();
    h.coordinator.step(Some(LiveDeskEvent::SettingsChanged));
    assert_eq!(h.status.lock().unwrap().state, CoordinatorState::Disabled);
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


