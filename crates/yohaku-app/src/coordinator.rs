use crate::capture::{
    RawApplicationFocus, RawMediaState, protocol_application_part, protocol_media_part,
};
use crate::ports::{Clock, HttpRequest, HttpTransport, MonotonicClock, TransportError};
use crate::presence_client::{PresenceClient, PresenceError};
use crate::privacy::PrivacyPipeline;
use crate::state::{CoordinatorState, LiveDeskStatus};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use yohaku_protocol::CLIENT_VERSION;
use yohaku_protocol::error::{ResponseError, parse_capabilities};
use yohaku_protocol::negotiator::{NegotiatedConfig, Negotiation, negotiate};
use yohaku_protocol::presence::{ClearReason, Mapper, PresenceSnapshotInput};
use yohaku_protocol::sequencer::{SequencePersistence, Sequencer};
use yohaku_store::ConnectionStore;

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

pub enum MediaLookup {
    Session(Box<RawMediaState>),
    NoSession,
    Unavailable,
}

pub trait PresenceSources: Send + Sync {
    fn current_application(&self) -> Option<RawApplicationFocus>;
    fn current_media(&self) -> MediaLookup;
}

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
                m.next_sequence = Some(m.next_sequence.map_or(next, |stored| stored.max(next)));
            }
        });
    }
}

pub struct CoordinatorDeps {
    pub connection: Arc<ConnectionStore>,
    pub pipeline: Arc<PrivacyPipeline>,
    pub sources: Arc<dyn PresenceSources>,
    pub http: Arc<dyn HttpTransport>,
    pub clock: Arc<dyn Clock>,
    pub monotonic: Arc<dyn MonotonicClock>,
    pub sequence_persistence: Arc<dyn SequencePersistence>,
}

enum Runtime {
    Disabled,
    Connecting,
    Waiting {
        retry_at_ms: u64,
    },
    Running {
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

    pub fn current_wait(&self) -> Duration {
        match &self.runtime {
            Runtime::Disabled | Runtime::Suspended => Duration::from_secs(3600),
            Runtime::Connecting => Duration::ZERO,
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
                    if matches!(event, LiveDeskEvent::SettingsChanged)
                        && matches!(self.runtime, Runtime::Running { .. })
                    {
                        self.refresh_requested = true;
                    }
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
            Runtime::Running { .. } => self.step_running(event.as_ref()),
            Runtime::Suspended => {}
        }
        StepOutcome::Continue
    }

    fn step_running(&mut self, event: Option<&LiveDeskEvent>) {
        if let Some(event) = event
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
            let (client, config) = match std::mem::replace(&mut self.runtime, Runtime::Connecting) {
                Runtime::Running { client, config } => (client, config),
                other => {
                    self.runtime = other;
                    return;
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

    fn reevaluate(&mut self) {
        if !self.is_enabled() {
            self.clear_best_effort(ClearReason::ConnectionRemoved);
            self.runtime = Runtime::Disabled;
            self.reset_send_flags();
            self.set_status(|s| {
                s.state = CoordinatorState::Disabled;
                s.media_capable = false;
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
            s.server_base_url = Some(base_url);
            s.device_id = Some(device_id);
        });
        self.refresh_requested = true;
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
        if !(200..300).contains(&response.status) {
            return Err(NegotiationFailure::Network(format!(
                "HTTP status {}",
                response.status
            )));
        }
        let (_meta, data) = parse_capabilities(&response.body).map_err(|e| match e {
            ResponseError::IncompatibleSchema | ResponseError::IncompatibleSchemaVersion => {
                NegotiationFailure::ServerRejection
            }
            _other => NegotiationFailure::Invalid,
        })?;
        match negotiate(&data, CLIENT_VERSION) {
            Negotiation::Available(config) => {
                let mapper = Mapper::new(data.limits, Default::default());
                Ok((config, mapper))
            }
            Negotiation::ClientUpdateRequired => Err(NegotiationFailure::UpdateRequired),
            Negotiation::SchemaUnsupported | Negotiation::FeatureUnavailable => {
                Err(NegotiationFailure::ServerRejection)
            }
            Negotiation::InvalidCapabilities => Err(NegotiationFailure::Invalid),
        }
    }

    fn send_snapshot(
        &mut self,
        client: &PresenceClient,
        config: &NegotiatedConfig,
    ) -> Result<(), PresenceError> {
        let raw_app = self.deps.sources.current_application();
        let raw_media = match self.deps.sources.current_media() {
            MediaLookup::Session(state) => {
                let state = *state;
                self.media_cache = Some(state.clone());
                Some(state)
            }
            MediaLookup::NoSession => {
                self.media_cache = None;
                None
            }
            MediaLookup::Unavailable => self.media_cache.as_mut().map(|cached| {
                cached.position_seconds = None;
                cached.position_sampled_at = None;
                cached.clone()
            }),
        };

        let snapshot = self.deps.pipeline.capture(
            raw_app.as_ref(),
            raw_media.as_ref(),
            &mut self.session_tracker,
        );

        let input = PresenceSnapshotInput {
            request_id: String::new(),
            device_id: String::new(),
            sequence: 0,
            observed_at: self.deps.clock.now(),
            lease_ttl_seconds: DEFAULT_LEASE_REQUEST,
            availability: snapshot.availability,
            application: snapshot.application.as_ref().map(protocol_application_part),
            media: snapshot
                .media
                .as_ref()
                .map(|media| protocol_media_part(media, config.supports_media_artwork)),
        };

        client
            .replace_presence(input, self.deps.clock.as_ref())
            .map(|_| ())
    }

    fn apply_send_outcome(
        &mut self,
        outcome: Result<(), PresenceError>,
        client: Box<PresenceClient>,
        config: NegotiatedConfig,
    ) -> Runtime {
        match outcome {
            Ok(()) => {
                let now = self.deps.clock.now();
                self.set_status(|s| {
                    s.state = CoordinatorState::Active;
                    s.last_error_code = None;
                    s.last_sent_at = Some(yohaku_protocol::time::format_rfc3339_millis(now));
                });
                Runtime::Running { client, config }
            }
            Err(PresenceError::SchemaRejected) => {
                self.reset_send_flags();
                self.set_status(|s| {
                    s.state = CoordinatorState::Connecting;
                    s.last_error_code = Some("SCHEMA_REJECTED".into());
                });
                Runtime::Connecting
            }
            Err(e) => {
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

pub fn run(mut coordinator: Coordinator, events: Receiver<LiveDeskEvent>) {
    loop {
        let wait = coordinator.current_wait().min(Duration::from_secs(3600));
        let event = match events.recv_timeout(wait) {
            Ok(event) => Some(event),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => break,
        };
        match coordinator.step(event) {
            StepOutcome::Continue => {}
            StepOutcome::Shutdown => break,
        }
    }
}
