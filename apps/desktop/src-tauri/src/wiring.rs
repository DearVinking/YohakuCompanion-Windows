use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use tauri::Manager;

use yohaku_app::capture::{RawApplicationFocus, RawMediaState};
use yohaku_app::coordinator::{
    Coordinator, CoordinatorDeps, LiveDeskEvent, MediaLookup, PresenceSources, run,
};
use yohaku_app::error::ApiError;
use yohaku_app::ports::{Clock, HttpTransport, SystemClock};
use yohaku_app::privacy::PrivacyPipeline;
use yohaku_app::state::LiveDeskStatus;
use yohaku_platform::foreground::{FocusSample, ForegroundMonitor, SharedSample};
use yohaku_platform::media::MediaMonitor;
use yohaku_platform::system::{SystemEvent, SystemEvents};
use yohaku_store::privacy::PrivacyRules;
use yohaku_store::settings::{Settings, SettingsPatch};
use yohaku_store::{ConnectionStore, SecretStore};

mod transport;
use transport::{DurablePresenceTransport, UreqTransport};

pub struct ShellSources {
    focus: SharedSample<FocusSample>,
    media_monitor: Option<Arc<MediaMonitor>>,
}

impl PresenceSources for ShellSources {
    fn current_application(&self) -> Option<RawApplicationFocus> {
        let sample = self.focus.lock().unwrap().clone()?;
        Some(RawApplicationFocus {
            application_key: sample.application_key,
            display_name: sample.display_name,
            window_title: sample.window_title,
        })
    }

    fn current_media(&self) -> MediaLookup {
        let Some(monitor) = self.media_monitor.as_ref() else {
            return MediaLookup::Unavailable;
        };
        match monitor.current_media() {
            Some(sample) => {
                let now = chrono::Utc::now();
                let position = sample.current_position(now).or(sample.position_seconds);
                MediaLookup::Session(Box::new(RawMediaState::new(
                    sample.title,
                    sample.artist,
                    sample.album,
                    sample.player_key,
                    sample.player_display_name,
                    sample.playing,
                    sample.duration_seconds,
                    position,
                    Some(now),
                )))
            }
            None => MediaLookup::NoSession,
        }
    }
}

impl ShellSources {
    fn media_monitor(&self) -> Option<Arc<MediaMonitor>> {
        self.media_monitor.clone()
    }
}

pub struct App {
    pub data_dir: PathBuf,
    pub connection: Arc<ConnectionStore>,
    pub pipeline: Arc<PrivacyPipeline>,
    pub http: Arc<dyn HttpTransport>,
    pub events: Sender<LiveDeskEvent>,
    pub status: Arc<Mutex<LiveDeskStatus>>,
    pub sources: Arc<ShellSources>,
    settings_write: Mutex<()>,
    coordinator_done: Mutex<Option<Receiver<()>>>,
    _foreground_monitor: ForegroundMonitor,
    _system_events: SystemEvents,
}

impl App {
    pub fn send_event(&self, event: LiveDeskEvent) {
        let _ = self.events.send(event);
    }

    pub fn set_paused(&self, paused: bool) -> Result<Settings, ApiError> {
        self.update_settings(&SettingsPatch {
            pause_sharing: Some(paused),
            ..Default::default()
        })
    }

    pub(super) fn update_settings(&self, patch: &SettingsPatch) -> Result<Settings, ApiError> {
        let _guard = self.settings_write.lock().unwrap();
        let settings = persist_settings(&self.pipeline, &self.data_dir, patch)?;
        self.apply_settings_side_effects(&settings);
        Ok(settings)
    }

    pub fn apply_settings_side_effects(&self, settings: &Settings) {
        if let Some(monitor) = self.sources.media_monitor() {
            monitor.set_preferred_players(settings.preferred_players.clone());
        }
        self.send_event(LiveDeskEvent::SettingsChanged);
    }
}

pub fn setup(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let data_dir = yohaku_store::paths::data_dir();
    yohaku_platform::logger::init(&data_dir);

    let secrets: Arc<dyn SecretStore> = Arc::new(
        yohaku_platform::credentials::DpapiSecretStore::new(&data_dir.join("secrets")),
    );
    let connection = Arc::new(ConnectionStore::new(&data_dir, secrets));
    let settings = Settings::load(&data_dir)?;
    let rules = PrivacyRules::load(&data_dir)?;
    let pipeline = Arc::new(PrivacyPipeline::new(settings, rules));

    let http: Arc<dyn HttpTransport> = Arc::new(UreqTransport::new());
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);

    let (event_tx, event_rx) = std::sync::mpsc::channel::<LiveDeskEvent>();

    let focus: SharedSample<FocusSample> = Arc::new(Mutex::new(None));
    let (focus_tx, focus_rx) = std::sync::mpsc::channel::<()>();
    let foreground_monitor = ForegroundMonitor::spawn(focus_tx, focus.clone());

    let (media_tx, media_rx) = std::sync::mpsc::channel::<()>();
    let media_monitor: Option<Arc<MediaMonitor>> = MediaMonitor::spawn(media_tx).ok().map(Arc::new);

    let (system_tx, system_rx) = std::sync::mpsc::channel::<SystemEvent>();
    let system_events = SystemEvents::spawn(system_tx);

    spawn_forwarder("focus-forward", focus_rx, event_tx.clone(), |_| {
        LiveDeskEvent::AppChanged
    })?;
    spawn_forwarder("media-forward", media_rx, event_tx.clone(), |_| {
        LiveDeskEvent::MediaSemanticChanged
    })?;
    spawn_forwarder(
        "system-forward",
        system_rx,
        event_tx.clone(),
        |event| match event {
            SystemEvent::SleepOrLock => LiveDeskEvent::SleepOrLock,
            SystemEvent::Wake => LiveDeskEvent::Wake,
            SystemEvent::NetworkUp => LiveDeskEvent::NetworkUp,
        },
    )?;
    if let Some(monitor) = &media_monitor {
        monitor.set_preferred_players(pipeline.settings().preferred_players.clone());
    }

    let sources = Arc::new(ShellSources {
        focus,
        media_monitor,
    });

    let status = Arc::new(Mutex::new(LiveDeskStatus::default()));
    let coordinator = Coordinator::new(
        CoordinatorDeps {
            connection: connection.clone(),
            pipeline: pipeline.clone(),
            sources: sources.clone(),
            http: Arc::new(DurablePresenceTransport::new(
                http.clone(),
                connection.clone(),
            )),
            clock,
            monotonic: Arc::new(yohaku_app::ports::RealMonotonic::new()),
            sequence_persistence: Arc::new(yohaku_app::coordinator::StoreSequencePersistence {
                store: connection.clone(),
            }),
        },
        status.clone(),
    );
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("livedesk-coordinator".into())
        .spawn(move || {
            run(coordinator, event_rx);
            let _ = done_tx.send(());
        })
        .map_err(|e| format!("spawn coordinator: {e}"))?;

    let launch_at_login = pipeline.settings().launch_at_login;
    sync_autostart(app.handle(), launch_at_login);

    app.manage(App {
        data_dir,
        connection,
        pipeline,
        http,
        events: event_tx,
        status,
        sources,
        settings_write: Mutex::new(()),
        coordinator_done: Mutex::new(Some(done_rx)),
        _foreground_monitor: foreground_monitor,
        _system_events: system_events,
    });
    Ok(())
}

fn persist_settings(
    pipeline: &PrivacyPipeline,
    dir: &std::path::Path,
    patch: &SettingsPatch,
) -> Result<Settings, ApiError> {
    let mut settings = pipeline.settings();
    settings.apply(patch);
    settings
        .save(dir)
        .map_err(|e| ApiError::new("STORE", e.to_string()))?;
    pipeline.replace_settings(settings.clone());
    Ok(settings)
}

fn spawn_forwarder<T: Send + 'static>(
    name: &str,
    rx: Receiver<T>,
    tx: Sender<LiveDeskEvent>,
    map: impl Fn(T) -> LiveDeskEvent + Send + 'static,
) -> Result<(), String> {
    std::thread::Builder::new()
        .name(name.into())
        .spawn(move || {
            while let Ok(event) = rx.recv() {
                if tx.send(map(event)).is_err() {
                    break;
                }
            }
        })
        .map(|_| ())
        .map_err(|e| e.to_string())
}

pub fn request_quit(app: &tauri::AppHandle, state: &App) {
    let app = app.clone();
    schedule_quit(&state.events, &state.coordinator_done, move || app.exit(0));
}

fn schedule_quit(
    events: &Sender<LiveDeskEvent>,
    coordinator_done: &Mutex<Option<Receiver<()>>>,
    exit: impl FnOnce() + Send + 'static,
) {
    let Some(done) = coordinator_done.lock().unwrap().take() else {
        return;
    };
    let _ = events.send(LiveDeskEvent::Shutdown);
    tauri::async_runtime::spawn_blocking(move || {
        let _ = done.recv_timeout(std::time::Duration::from_millis(700));
        exit();
    });
}

pub fn sync_autostart(app: &tauri::AppHandle, enable: bool) {
    use tauri_plugin_autostart::ManagerExt;
    let autostart = app.autolaunch();
    let _ = if enable {
        autostart.enable()
    } else {
        autostart.disable()
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quitting_returns_promptly_and_exits_after_coordinator_finishes() {
        let (events, received) = std::sync::mpsc::channel();
        let (done, completion) = std::sync::mpsc::channel();
        let completion = Mutex::new(Some(completion));
        let (exited, exit) = std::sync::mpsc::channel();
        let started = std::time::Instant::now();
        schedule_quit(&events, &completion, move || exited.send(()).unwrap());
        assert!(started.elapsed() < std::time::Duration::from_millis(100));
        assert!(matches!(received.try_recv(), Ok(LiveDeskEvent::Shutdown)));
        assert!(exit.try_recv().is_err());
        done.send(()).unwrap();
        exit.recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
    }

    #[test]
    fn repeated_quit_requests_schedule_one_shutdown_and_exit() {
        let (events, received) = std::sync::mpsc::channel();
        let (done, completion) = std::sync::mpsc::channel();
        let completion = Mutex::new(Some(completion));
        let (exited, exit) = std::sync::mpsc::channel();
        for _ in 0..2 {
            let exited = exited.clone();
            schedule_quit(&events, &completion, move || exited.send(()).unwrap());
        }
        assert!(matches!(received.try_recv(), Ok(LiveDeskEvent::Shutdown)));
        assert!(received.try_recv().is_err());
        done.send(()).unwrap();
        exit.recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        assert!(exit.try_recv().is_err());
    }

    #[test]
    fn quitting_remains_bounded_when_coordinator_stalls() {
        let (events, _received) = std::sync::mpsc::channel();
        let (_done, completion) = std::sync::mpsc::channel();
        let completion = Mutex::new(Some(completion));
        let (exited, exit) = std::sync::mpsc::channel();
        schedule_quit(&events, &completion, move || exited.send(()).unwrap());
        assert!(
            exit.recv_timeout(std::time::Duration::from_millis(20))
                .is_err()
        );
        exit.recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
    }

    #[test]
    fn failed_settings_save_does_not_change_running_privacy() {
        let dir = tempfile::tempdir().unwrap();
        let pipeline = PrivacyPipeline::new(Settings::default(), PrivacyRules::default());
        let before = pipeline.settings();
        let patch = SettingsPatch {
            share_applications: Some(false),
            pause_sharing: Some(true),
            ..Default::default()
        };
        assert!(persist_settings(&pipeline, &dir.path().join("missing"), &patch).is_err());
        assert_eq!(pipeline.settings(), before);
    }

    #[test]
    fn saved_settings_match_memory_and_disk() {
        let dir = tempfile::tempdir().unwrap();
        let pipeline = PrivacyPipeline::new(Settings::default(), PrivacyRules::default());
        let patch = SettingsPatch {
            pause_sharing: Some(true),
            ..Default::default()
        };
        let saved = persist_settings(&pipeline, dir.path(), &patch).unwrap();
        assert!(saved.pause_sharing);
        assert_eq!(pipeline.settings(), saved);
        assert_eq!(Settings::load(dir.path()).unwrap(), saved);
    }
}
