//! LiveDesk 协调器（macOS 版 CompanionLiveDeskCoordinator 语义）：
//! 单线程事件驱动状态机。语义变化与心跳触发全新捕获（不重放旧报告）；
//! 睡眠/锁屏/暂停/关机执行 best-effort 清除；schema 拒绝即重协商。
//! 通过 [`Coordinator::step`] 单步驱动，便于用假端口测试。

use crate::capture::{
    RawApplicationFocus, RawMediaState, protocol_application_part, protocol_media_part,
};
use crate::ports::{Clock, HttpRequest, HttpTransport, MonotonicClock, TransportError};
use crate::presence_client::{PresenceClient, PresenceError};
use crate::privacy::PrivacyPipeline;
use crate::s3::{S3Config, effective_base_path, media_artwork_target, public_base_url, sign_put};
use crate::state::{CoordinatorState, LiveDeskStatus};
use std::collections::HashMap;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;
use yohaku_protocol::CLIENT_VERSION;
use yohaku_protocol::capabilities::CapabilityLimits;
use yohaku_protocol::error::{ResponseError, parse_capabilities};
use yohaku_protocol::negotiator::{NegotiatedConfig, Negotiation, negotiate};
use yohaku_protocol::presence::{ClearReason, Mapper, PresenceSnapshotInput};
use yohaku_protocol::sequencer::{SequencePersistence, Sequencer};
use yohaku_store::ConnectionStore;
use yohaku_store::history::{HistoryStore, SyncEvent, SyncState, SyncTrigger};

pub const DEFAULT_LEASE_REQUEST: i64 = 90;
const RETRY_DELAY_SERVER_REJECTION: Duration = Duration::from_secs(300);
const RETRY_DELAY_GENERIC: Duration = Duration::from_secs(30);
const CLEAR_BEST_EFFORT_TIMEOUT_MS: u64 = 500;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LiveDeskEvent {
    AppChanged,
    MediaSemanticChanged,
    NetworkUp,
    SleepOrLock,
    Wake,
    PolicyChanged,
    SettingsChanged,
    Shutdown,
}

/// 平台捕获源。媒体查询区分「无会话」与「查询失败」：
/// 失败时沿用缓存但剥离时间线（位置不上报，macOS hasFreshTimeline 语义）。
pub enum MediaLookup {
    Session(Box<RawMediaState>),
    NoSession,
    Unavailable,
}

pub trait PresenceSources: Send + Sync {
    fn current_application(&self) -> Option<RawApplicationFocus>;
    fn current_media(&self) -> MediaLookup;
}

/// 应用图标 PNG 提取器（平台层实现；测试注入固定字节）。
pub trait IconByteProvider: Send + Sync {
    fn icon_png(&self, application_key: &str) -> Option<Vec<u8>>;
}

/// 资产托管端口（生产实现为 S3；测试可替换）。
pub trait AssetHosting: Send + Sync {
    /// 归一化 + 上传媒体封面；失败返回 None（降级纯文本，不缓存失败）。
    fn host_media_artwork(&self, artwork_bytes: &[u8], device_id: &str) -> Option<String>;
    /// 解析应用图标公网 URL（会话内去重上传）；失败返回 None。
    fn host_application_icon(&self, application_key: &str) -> Option<String>;
    /// 资产公网 host 集合（协议 icon URL 白名单）。
    fn allowed_hosts(&self) -> Vec<String>;
}

/// S3 资产托管实现。
pub struct S3AssetHosting {
    http: Arc<dyn HttpTransport>,
    clock: Arc<dyn Clock>,
    config: RwLock<Option<S3Config>>,
    icon_provider: RwLock<Option<Arc<dyn IconByteProvider>>>,
    artwork_cache: Mutex<Option<((String, String), String)>>,
    icon_cache: Mutex<HashMap<String, String>>,
}

impl S3AssetHosting {
    pub fn new(http: Arc<dyn HttpTransport>, clock: Arc<dyn Clock>) -> Self {
        S3AssetHosting {
            http,
            clock,
            config: RwLock::new(None),
            icon_provider: RwLock::new(None),
            artwork_cache: Mutex::new(None),
            icon_cache: Mutex::new(HashMap::new()),
        }
    }

    pub fn set_config(&self, config: Option<S3Config>) {
        *self.config.write().unwrap() = config;
        // 配置变更即作废会话内缓存（指纹不同 → URL 不同）
        self.icon_cache.lock().unwrap().clear();
        *self.artwork_cache.lock().unwrap() = None;
    }

    pub fn set_icon_provider(&self, provider: Option<Arc<dyn IconByteProvider>>) {
        *self.icon_provider.write().unwrap() = provider;
    }

    fn config(&self) -> Option<S3Config> {
        let cfg = self.config.read().unwrap().clone()?;
        cfg.is_configured().then_some(cfg)
    }
}

impl AssetHosting for S3AssetHosting {
    fn host_media_artwork(&self, artwork_bytes: &[u8], device_id: &str) -> Option<String> {
        let cfg = self.config()?;
        let normalized = crate::artwork::normalize_artwork(artwork_bytes).ok()?;
        let fingerprint = format!(
            "{}|{}|{}",
            cfg.bucket,
            public_base_url(&cfg),
            effective_base_path(&cfg)
        );
        {
            let cache = self.artwork_cache.lock().unwrap();
            if let Some(((hash, fp), url)) = cache.as_ref()
                && *hash == normalized.content_hash
                && *fp == fingerprint
            {
                return Some(url.clone());
            }
        }
        let target = media_artwork_target(&cfg, device_id, &normalized.content_hash);
        let request = sign_put(
            &cfg,
            &target,
            &normalized.png,
            "image/png",
            &[],
            self.clock.now(),
        )
        .ok()?;
        let response = self.http.send(request).ok()?;
        if !(200..300).contains(&response.status) {
            return None;
        }
        let mut cache = self.artwork_cache.lock().unwrap();
        *cache = Some((
            (normalized.content_hash, fingerprint),
            target.public_url.clone(),
        ));
        Some(target.public_url)
    }

    fn host_application_icon(&self, application_key: &str) -> Option<String> {
        let cfg = self.config()?;
        if let Some(url) = self.icon_cache.lock().unwrap().get(application_key) {
            return Some(url.clone());
        }
        let provider = self.icon_provider.read().unwrap().clone()?;
        let png = provider.icon_png(application_key)?;
        let normalized = crate::artwork::normalize_artwork(&png).ok()?;
        let target = crate::s3::application_icon_target(&cfg, &normalized.content_hash);
        let request = sign_put(
            &cfg,
            &target,
            &normalized.png,
            "image/png",
            &[],
            self.clock.now(),
        )
        .ok()?;
        let response = self.http.send(request).ok()?;
        if !(200..300).contains(&response.status) {
            return None;
        }
        let mut cache = self.icon_cache.lock().unwrap();
        cache.insert(application_key.to_string(), target.public_url.clone());
        Some(target.public_url)
    }

    fn allowed_hosts(&self) -> Vec<String> {
        self.config()
            .map(|cfg| {
                let base = public_base_url(&cfg);
                base.split("//")
                    .nth(1)
                    .and_then(|rest| rest.split('/').next())
                    .map(|host| vec![host.to_string()])
                    .unwrap_or_default()
            })
            .unwrap_or_default()
    }
}

/// 序列号持久化（落 ConnectionStore 元数据）。
pub struct StoreSequencePersistence {
    pub store: Arc<ConnectionStore>,
}

impl SequencePersistence for StoreSequencePersistence {
    fn load_next(&self, device_id: &str) -> Option<i64> {
        let metadata = self.store.load_metadata().ok()??;
        (metadata.device_id == device_id).then_some(())?;
        metadata.next_sequence
    }
    fn save_next(&self, device_id: &str, next: i64) {
        let _ = self.store.update_metadata(|m| {
            if m.device_id == device_id {
                m.next_sequence = Some(next);
            }
        });
    }
}

pub struct CoordinatorDeps {
    pub connection: Arc<ConnectionStore>,
    pub pipeline: Arc<PrivacyPipeline>,
    pub sources: Arc<dyn PresenceSources>,
    pub assets: Arc<dyn AssetHosting>,
    pub http: Arc<dyn HttpTransport>,
    pub clock: Arc<dyn Clock>,
    pub monotonic: Arc<dyn MonotonicClock>,
    pub history: Arc<HistoryStore>,
    pub history_dir: std::path::PathBuf,
    pub sequence_persistence: Arc<dyn SequencePersistence>,
}

enum Runtime {
    Disabled,
    Connecting,
    Waiting {
        retry_at_ms: u64,
    },
    Running {
        // Box 压平变体体积差（client 约 280 字节，其余变体 ≤ 8 字节）
        client: Box<PresenceClient>,
        config: NegotiatedConfig,
    },
    Suspended,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepOutcome {
    Continue,
    Shutdown,
}

enum SendOutcome {
    Sent,
    Failed(PresenceError),
}

enum NegotiationFailure {
    UpdateRequired,
    ServerRejection,
    Invalid,
    Network(String),
}

pub struct Coordinator {
    deps: CoordinatorDeps,
    status: Arc<Mutex<LiveDeskStatus>>,
    runtime: Runtime,
    refresh_requested: bool,
    last_send_started_ms: Option<u64>,
    heartbeat_at_ms: Option<u64>,
    media_cache: Option<RawMediaState>,
    session_tracker: crate::session::MediaSessionTracker,
}

impl Coordinator {
    pub fn new(deps: CoordinatorDeps, status: Arc<Mutex<LiveDeskStatus>>) -> Self {
        Coordinator {
            deps,
            status,
            runtime: Runtime::Disabled,
            refresh_requested: false,
            last_send_started_ms: None,
            heartbeat_at_ms: None,
            media_cache: None,
            session_tracker: crate::session::MediaSessionTracker::new(),
        }
    }

    fn set_status(&self, update: impl FnOnce(&mut LiveDeskStatus)) {
        let mut status = self.status.lock().unwrap();
        update(&mut status);
    }

    fn is_enabled(&self) -> bool {
        self.deps
            .connection
            .load_metadata()
            .ok()
            .flatten()
            .map(|m| m.is_live_desk_enabled)
            .unwrap_or(false)
    }

    fn is_paused(&self) -> bool {
        self.deps.pipeline.settings().pause_sharing
    }

    /// 下一次事件等待时长。
    pub fn current_wait(&self) -> Duration {
        match &self.runtime {
            Runtime::Disabled | Runtime::Suspended | Runtime::Connecting => {
                Duration::from_secs(3600)
            }
            Runtime::Waiting { retry_at_ms, .. } => {
                Duration::from_millis(retry_at_ms.saturating_sub(self.deps.monotonic.now_millis()))
            }
            Runtime::Running { config, .. } => {
                if self.refresh_requested {
                    Duration::from_millis(self.rate_limit_wait_ms(config))
                } else {
                    let at_ms = self.heartbeat_at_ms.unwrap_or_else(|| {
                        self.deps.monotonic.now_millis()
                            + config.heartbeat_seconds(DEFAULT_LEASE_REQUEST) as u64 * 1000
                    });
                    Duration::from_millis(at_ms.saturating_sub(self.deps.monotonic.now_millis()))
                }
            }
        }
    }

    fn rate_limit_wait_ms(&self, config: &NegotiatedConfig) -> u64 {
        let min_interval_ms = config.minimum_send_interval_ms() as u64;
        self.last_send_started_ms
            .map(|t| {
                min_interval_ms.saturating_sub(self.deps.monotonic.now_millis().saturating_sub(t))
            })
            .unwrap_or(0)
    }

    /// 单步：处理事件（None = 超时 tick）并执行相应动作。
    pub fn step(&mut self, event: Option<LiveDeskEvent>) -> StepOutcome {
        if let Some(event) = &event {
            match event {
                LiveDeskEvent::Shutdown => {
                    self.clear_best_effort(ClearReason::Shutdown);
                    self.set_status(|s| *s = LiveDeskStatus::default());
                    return StepOutcome::Shutdown;
                }
                LiveDeskEvent::SleepOrLock => {
                    self.clear_best_effort(ClearReason::Sleep);
                    self.runtime = Runtime::Suspended;
                    self.reset_send_flags();
                    self.set_status(|s| s.state = CoordinatorState::Suspended);
                    return StepOutcome::Continue;
                }
                LiveDeskEvent::SettingsChanged | LiveDeskEvent::Wake => {
                    self.reevaluate();
                    return StepOutcome::Continue;
                }
                _ => {}
            }
        }

        match &mut self.runtime {
            Runtime::Disabled => {
                if event.is_some() {
                    self.reevaluate();
                }
            }
            Runtime::Connecting => self.try_connect(),
            Runtime::Waiting { retry_at_ms, .. } => {
                let due = event
                    .as_ref()
                    .map(|e| matches!(e, LiveDeskEvent::NetworkUp | LiveDeskEvent::PolicyChanged))
                    .unwrap_or_else(|| self.deps.monotonic.now_millis() >= *retry_at_ms);
                if due {
                    self.runtime = Runtime::Connecting;
                    self.try_connect();
                }
            }
            Runtime::Running { .. } => {
                if let Some(event) = &event
                    && matches!(
                        event,
                        LiveDeskEvent::AppChanged
                            | LiveDeskEvent::MediaSemanticChanged
                            | LiveDeskEvent::PolicyChanged
                            | LiveDeskEvent::NetworkUp
                    )
                {
                    self.refresh_requested = true;
                }
                let heartbeat_due = self
                    .heartbeat_at_ms
                    .map(|at| self.deps.monotonic.now_millis() >= at)
                    .unwrap_or(false);
                if heartbeat_due {
                    self.refresh_requested = true;
                }
                let should_send = {
                    let Runtime::Running { config, .. } = &self.runtime else {
                        unreachable!("guarded by match arm");
                    };
                    self.refresh_requested && self.rate_limit_wait_ms(config) == 0
                };
                if should_send {
                    // 取回所有权以便后续 &mut self 调用
                    let (client, config) =
                        match std::mem::replace(&mut self.runtime, Runtime::Connecting) {
                            Runtime::Running { client, config } => (client, config),
                            other => {
                                self.runtime = other;
                                return StepOutcome::Continue;
                            }
                        };
                    self.refresh_requested = false;
                    self.last_send_started_ms = Some(self.deps.monotonic.now_millis());
                    self.heartbeat_at_ms = Some(
                        self.deps.monotonic.now_millis()
                            + config.heartbeat_seconds(DEFAULT_LEASE_REQUEST) as u64 * 1000,
                    );
                    let outcome = self.send_snapshot(&client, &config);
                    self.runtime = self.apply_send_outcome(outcome, client, config);
                }
            }
            Runtime::Suspended => {}
        }
        StepOutcome::Continue
    }

    /// 重新评估启用/暂停状态。
    fn reevaluate(&mut self) {
        if !self.is_enabled() {
            self.clear_best_effort(ClearReason::ConnectionRemoved);
            self.runtime = Runtime::Disabled;
            self.reset_send_flags();
            self.set_status(|s| {
                s.state = CoordinatorState::Disabled;
                s.media_capable = false;
                s.artwork_capable = false;
                s.server_base_url = None;
                s.device_id = None;
            });
            return;
        }
        if self.is_paused() {
            self.clear_best_effort(ClearReason::Paused);
            self.runtime = Runtime::Suspended;
            self.reset_send_flags();
            self.set_status(|s| s.state = CoordinatorState::Suspended);
            return;
        }
        if !matches!(self.runtime, Runtime::Running { .. }) {
            self.runtime = Runtime::Connecting;
            self.set_status(|s| s.state = CoordinatorState::Connecting);
        }
    }

    fn reset_send_flags(&mut self) {
        self.refresh_requested = false;
        self.heartbeat_at_ms = None;
        self.last_send_started_ms = None;
    }

    /// 能力协商：成功 → Running；失败 → Waiting。
    fn try_connect(&mut self) {
        let Some((base_url, device_id, token)) = self
            .deps
            .connection
            .load_enabled_connection()
            .ok()
            .flatten()
        else {
            self.runtime = Runtime::Disabled;
            self.set_status(|s| s.state = CoordinatorState::Disabled);
            return;
        };
        if self.is_paused() {
            self.reevaluate();
            return;
        }
        let (config, mapper) = match self.fetch_and_negotiate(&base_url) {
            Ok(pair) => pair,
            Err(NegotiationFailure::UpdateRequired) => {
                self.wait_with(
                    CoordinatorState::UpdateRequired,
                    RETRY_DELAY_SERVER_REJECTION,
                );
                return;
            }
            Err(NegotiationFailure::ServerRejection) => {
                self.wait_with(
                    CoordinatorState::ServerFeatureUnavailable,
                    RETRY_DELAY_SERVER_REJECTION,
                );
                return;
            }
            Err(NegotiationFailure::Invalid) => {
                self.wait_with(CoordinatorState::Degraded, RETRY_DELAY_SERVER_REJECTION);
                return;
            }
            Err(NegotiationFailure::Network(e)) => {
                log::warn!("capability fetch failed: {e}");
                self.set_status(|s| s.last_error_code = Some("CAPABILITIES_UNAVAILABLE".into()));
                self.wait_with(CoordinatorState::Degraded, RETRY_DELAY_GENERIC);
                return;
            }
        };
        let pairing_next = self
            .deps
            .connection
            .load_metadata()
            .ok()
            .flatten()
            .map(|m| m.pairing_next_sequence)
            .unwrap_or(0);
        let sequencer = Sequencer::new(
            self.deps.sequence_persistence.clone(),
            &device_id,
            pairing_next,
        );
        let client = PresenceClient::new(
            self.deps.http.clone(),
            mapper,
            sequencer,
            base_url.clone(),
            device_id.clone(),
            token,
        );
        self.runtime = Runtime::Running {
            client: Box::new(client),
            config,
        };
        self.set_status(|s| {
            s.state = CoordinatorState::Active;
            s.last_error_code = None;
            s.media_capable = config.supports_media_timeline;
            s.artwork_capable = config.supports_media_artwork;
            s.server_base_url = Some(base_url);
            s.device_id = Some(device_id);
        });
        self.refresh_requested = true; // 上线后立即发送首个快照
        self.last_send_started_ms = None;
    }

    fn wait_with(&mut self, state: CoordinatorState, delay: Duration) {
        self.runtime = Runtime::Waiting {
            retry_at_ms: self.deps.monotonic.now_millis() + delay.as_millis() as u64,
        };
        self.set_status(|s| s.state = state);
    }

    fn fetch_and_negotiate(
        &self,
        base_url: &str,
    ) -> Result<(NegotiatedConfig, Mapper), NegotiationFailure> {
        let request = HttpRequest {
            method: "GET",
            url: format!("{}/companion/capabilities", base_url.trim_end_matches('/')),
            headers: vec![
                ("Accept".to_string(), "application/json".to_string()),
                (
                    "X-Yohaku-Companion-Version".to_string(),
                    CLIENT_VERSION.to_string(),
                ),
            ],
            body: None,
            timeout_ms: 10_000,
        };
        let response = self
            .deps
            .http
            .send(request)
            .map_err(|TransportError(e)| NegotiationFailure::Network(e))?;
        let (_meta, data) = parse_capabilities(&response.body).map_err(|e| match e {
            ResponseError::IncompatibleSchema | ResponseError::IncompatibleSchemaVersion => {
                NegotiationFailure::ServerRejection
            }
            _other => NegotiationFailure::Invalid,
        })?;
        match negotiate(&data, CLIENT_VERSION) {
            Negotiation::Available(config) => {
                let mapper = Mapper::new(
                    CapabilityLimits {
                        presence_payload_bytes: data.limits.presence_payload_bytes,
                        presence_requests_per_minute: data.limits.presence_requests_per_minute,
                        presence_lease_min_seconds: data.limits.presence_lease_min_seconds,
                        presence_lease_max_seconds: data.limits.presence_lease_max_seconds,
                        recommended_heartbeat_seconds: data.limits.recommended_heartbeat_seconds,
                        maximum_clock_skew_seconds: data.limits.maximum_clock_skew_seconds,
                    },
                    self.deps.assets.allowed_hosts().into_iter().collect(),
                );
                Ok((config, mapper))
            }
            Negotiation::ClientUpdateRequired => Err(NegotiationFailure::UpdateRequired),
            Negotiation::SchemaUnsupported | Negotiation::FeatureUnavailable => {
                Err(NegotiationFailure::ServerRejection)
            }
            Negotiation::InvalidCapabilities => Err(NegotiationFailure::Invalid),
        }
    }

    /// 捕获 → 资产 → PUT。
    fn send_snapshot(&mut self, client: &PresenceClient, config: &NegotiatedConfig) -> SendOutcome {
        let raw_app = self.deps.sources.current_application();
        let raw_media = match self.deps.sources.current_media() {
            MediaLookup::Session(state) => {
                self.media_cache = Some(*state.clone());
                Some(*state)
            }
            MediaLookup::NoSession => {
                self.media_cache = None;
                None
            }
            MediaLookup::Unavailable => self.media_cache.take().map(|mut cached| {
                cached.position_seconds = None;
                cached.position_sampled_at = None;
                cached
            }),
        };

        let snapshot = self.deps.pipeline.capture(
            raw_app.as_ref(),
            raw_media.as_ref(),
            &mut self.session_tracker,
        );

        let device_id = client.device_id().to_string();
        let icon_url = match (snapshot.application.as_ref(), raw_app.as_ref()) {
            (Some(_), Some(raw)) => self.deps.assets.host_application_icon(&raw.application_key),
            _ => None,
        };
        let artwork_url = if config.supports_media_artwork {
            raw_media
                .as_ref()
                .and_then(|m| m.artwork_bytes.as_deref())
                .and_then(|bytes| self.deps.assets.host_media_artwork(bytes, &device_id))
        } else {
            None
        };

        let input = PresenceSnapshotInput {
            request_id: String::new(),
            device_id: String::new(),
            sequence: 0,
            observed_at: self.deps.clock.now(),
            lease_ttl_seconds: DEFAULT_LEASE_REQUEST,
            availability: snapshot.availability,
            application: snapshot
                .application
                .as_ref()
                .map(|app| protocol_application_part(app, icon_url)),
            media: snapshot.media.as_ref().map(|media| {
                protocol_media_part(media, artwork_url, config.supports_media_artwork)
            }),
        };

        let started_at = self.deps.clock.now();
        let result = client.replace_presence(input, self.deps.clock.as_ref());
        let finished_at = self.deps.clock.now();
        self.record_history(result.as_ref(), started_at, finished_at);
        match result {
            Ok(_) => SendOutcome::Sent,
            Err(e) => SendOutcome::Failed(e),
        }
    }

    fn record_history(
        &self,
        result: Result<&yohaku_protocol::error::MutationResponse, &PresenceError>,
        started_at: chrono::DateTime<chrono::Utc>,
        finished_at: chrono::DateTime<chrono::Utc>,
    ) {
        let (state, error_code, summary) = match result {
            Ok(mutation) => (
                SyncState::Succeeded,
                None,
                Some(format!("accepted seq {}", mutation.accepted_sequence)),
            ),
            Err(e) => {
                let code = match e {
                    PresenceError::Server { code, .. } => code.clone(),
                    other => fixed_error_code(other).to_string(),
                };
                (SyncState::Failed, Some(code), None)
            }
        };
        let event = SyncEvent {
            id: uuid::Uuid::new_v4().to_string(),
            destination: "liveDesk".into(),
            trigger: SyncTrigger::SemanticChange,
            state,
            error_code,
            output_summary: summary,
            started_at,
            finished_at,
        };
        let _ = self.deps.history.append(&self.deps.history_dir, event);
    }

    fn apply_send_outcome(
        &mut self,
        outcome: SendOutcome,
        client: Box<PresenceClient>,
        config: NegotiatedConfig,
    ) -> Runtime {
        match outcome {
            SendOutcome::Sent => {
                let now = self.deps.clock.now();
                self.set_status(|s| {
                    s.state = CoordinatorState::Active;
                    s.last_error_code = None;
                    s.last_sent_at = Some(yohaku_protocol::time::format_rfc3339_millis(now));
                });
                Runtime::Running { client, config }
            }
            SendOutcome::Failed(PresenceError::SchemaRejected) => {
                // 丢弃 authority，立即重协商
                self.reset_send_flags();
                self.set_status(|s| {
                    s.state = CoordinatorState::Connecting;
                    s.last_error_code = Some("SCHEMA_REJECTED".into());
                });
                Runtime::Connecting
            }
            SendOutcome::Failed(e) => {
                self.set_status(|s| {
                    s.state = CoordinatorState::Degraded;
                    s.last_error_code = Some(fixed_error_code(&e).to_string());
                });
                Runtime::Running { client, config }
            }
        }
    }

    fn clear_best_effort(&mut self, reason: ClearReason) {
        let Runtime::Running { client, .. } = &self.runtime else {
            return;
        };
        // best-effort：受限超时，先到者赢；失败静默（租约过期兜底）
        let _ = client.clear_presence_bounded(
            reason,
            self.deps.clock.as_ref(),
            CLEAR_BEST_EFFORT_TIMEOUT_MS,
        );
    }
}

fn fixed_error_code(e: &PresenceError) -> &'static str {
    match e {
        PresenceError::Transport(_) => "TRANSPORT",
        PresenceError::SchemaRejected => "SCHEMA_REJECTED",
        PresenceError::PayloadTooLarge => "PAYLOAD_TOO_LARGE",
        PresenceError::Server { .. } => "SERVER",
        PresenceError::Decode(_) => "DECODE",
    }
}

/// 常驻线程入口。
pub fn run(mut coordinator: Coordinator, events: Receiver<LiveDeskEvent>) {
    loop {
        let wait = coordinator.current_wait().min(Duration::from_secs(3600));
        let event = events.recv_timeout(wait).ok();
        match coordinator.step(event) {
            StepOutcome::Continue => {}
            StepOutcome::Shutdown => break,
        }
    }
}
