use super::MediaSample;
use crate::com::ComApartment;
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession, GlobalSystemMediaTransportControlsSessionManager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus,
};
use windows_future::{AsyncStatus, IAsyncOperation};

const POLL_INTERVAL: Duration = Duration::from_millis(300);
const ASYNC_TIMEOUT: Duration = Duration::from_millis(2_000);

#[derive(Debug, Clone)]
struct TrackedSession {
    source: String,
    title: Option<String>,
    artist: Option<String>,
    album: Option<String>,
    playing: bool,
    rate: f64,
    duration_seconds: Option<f64>,
    position_seconds: Option<f64>,
    updated_at: DateTime<Utc>,
    sampled_ms: u64,
}

fn wait_async<T: windows::core::RuntimeType + Clone>(op: &IAsyncOperation<T>) -> Option<T> {
    let deadline = std::time::Instant::now() + ASYNC_TIMEOUT;
    loop {
        if std::time::Instant::now() >= deadline {
            return None;
        }
        match op.Status() {
            Ok(AsyncStatus::Completed) => return op.GetResults().ok(),
            Ok(AsyncStatus::Started) => std::thread::sleep(Duration::from_millis(8)),
            _ => return None,
        }
    }
}

fn filetime_to_datetime(v: i64) -> Option<DateTime<Utc>> {
    let unix_nanos = (v as i128).saturating_mul(100) - 11_644_473_600_000_000_000i128;
    DateTime::from_timestamp_nanos(unix_nanos as i64).into()
}

fn monotonic_ms() -> u64 {
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    START
        .get_or_init(std::time::Instant::now)
        .elapsed()
        .as_millis() as u64
}

fn non_empty(s: String) -> Option<String> {
    let t = s.trim();
    (!t.is_empty()).then(|| t.to_string())
}

fn display_name_from_source(source: &str) -> String {
    let head = source.split('!').next().unwrap_or(source);
    let tail = head.rsplit(['\\', '/']).next().unwrap_or(head);
    let stem = tail.strip_suffix(".exe").unwrap_or(tail);
    if !stem.is_empty() {
        stem.to_string()
    } else {
        source.to_string()
    }
}

fn is_preferred(source: &str, preferred: &[String]) -> bool {
    let lower = source.to_lowercase();
    preferred.iter().any(|p| !p.is_empty() && lower.contains(p))
}

fn read_session(session: &GlobalSystemMediaTransportControlsSession) -> Option<TrackedSession> {
    let source = session
        .SourceAppUserModelId()
        .ok()?
        .to_string_lossy()
        .to_string();
    let props = wait_async(&session.TryGetMediaPropertiesAsync().ok()?)?;
    let title = props
        .Title()
        .ok()
        .and_then(|t| non_empty(t.to_string_lossy()));
    let artist = props
        .Artist()
        .ok()
        .and_then(|t| non_empty(t.to_string_lossy()));
    let album = props
        .AlbumTitle()
        .ok()
        .and_then(|t| non_empty(t.to_string_lossy()));

    let playback = session.GetPlaybackInfo().ok()?;
    let playing = playback.PlaybackStatus().ok()?
        == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing;
    let rate = playback
        .PlaybackRate()
        .ok()
        .and_then(|r| r.Value().ok())
        .unwrap_or(if playing { 1.0 } else { 0.0 });

    let timeline = session.GetTimelineProperties().ok();
    let (position, updated_at) = match &timeline {
        Some(t) => {
            let position = t.Position().ok().map(|p| p.Duration as f64 / 10_000_000.0);
            let updated = t
                .LastUpdatedTime()
                .ok()
                .and_then(|dt| filetime_to_datetime(dt.UniversalTime));
            (position, updated.unwrap_or_else(Utc::now))
        }
        None => (None, Utc::now()),
    };
    let duration = timeline.as_ref().and_then(|t| {
        t.EndTime()
            .ok()
            .zip(t.StartTime().ok())
            .map(|(end, start)| (end.Duration.saturating_sub(start.Duration)) as f64 / 10_000_000.0)
    });

    Some(TrackedSession {
        source,
        title,
        artist,
        album,
        playing,
        rate,
        duration_seconds: duration,
        position_seconds: position,
        updated_at,
        sampled_ms: monotonic_ms(),
    })
}

struct Inner {
    sessions: Mutex<HashMap<String, TrackedSession>>,
    preferred: Mutex<Vec<String>>,
}

impl Inner {
    fn refresh_all(&self, manager: &GlobalSystemMediaTransportControlsSessionManager) {
        let Ok(sessions) = manager.GetSessions() else {
            return;
        };
        let mut live_keys = Vec::new();
        let size = sessions.Size().unwrap_or(0);
        for i in 0..size {
            let Ok(session) = sessions.GetAt(i) else {
                continue;
            };
            if let Some(tracked) = read_session(&session) {
                live_keys.push(tracked.source.clone());
                self.sessions
                    .lock()
                    .unwrap()
                    .insert(tracked.source.clone(), tracked);
            }
        }
        self.sessions
            .lock()
            .unwrap()
            .retain(|k, _| live_keys.contains(k));
    }

    fn arbitrate(&self) -> Option<MediaSample> {
        let sessions = self.sessions.lock().unwrap();
        let preferred = self.preferred.lock().unwrap();
        let rank =
            |s: &TrackedSession| (is_preferred(&s.source, &preferred), s.playing, s.sampled_ms);
        sessions
            .values()
            .max_by(|a, b| rank(a).cmp(&rank(b)))
            .map(|s| MediaSample {
                title: s.title.clone(),
                artist: s.artist.clone(),
                album: s.album.clone(),
                player_key: s.source.to_lowercase(),
                player_display_name: display_name_from_source(&s.source),
                playing: s.playing,
                duration_seconds: s.duration_seconds,
                position_seconds: s.position_seconds,
                position_rate: if s.playing { s.rate.max(0.0) } else { 0.0 },
                position_updated_at: s.updated_at,
            })
    }
}

pub struct MediaMonitor {
    inner: Arc<Inner>,
    stop: Arc<std::sync::atomic::AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl MediaMonitor {
    pub fn spawn(notify: Sender<()>) -> Result<Self, String> {
        Self::spawn_with_initializer(notify, || {
            let request = GlobalSystemMediaTransportControlsSessionManager::RequestAsync()
                .map_err(|error| format!("request GSMTC session manager: {error}"))?;
            wait_async(&request)
                .ok_or_else(|| "GSMTC session manager initialization failed or timed out".into())
        })
    }

    fn spawn_with_initializer(
        notify: Sender<()>,
        initialize: impl FnOnce() -> Result<GlobalSystemMediaTransportControlsSessionManager, String>
        + Send
        + 'static,
    ) -> Result<Self, String> {
        let inner = Arc::new(Inner {
            sessions: Mutex::new(HashMap::new()),
            preferred: Mutex::new(Vec::new()),
        });
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stop_clone = stop.clone();
        let inner_clone = inner.clone();
        let (startup_tx, startup_rx) = mpsc::sync_channel(1);
        let thread = std::thread::Builder::new()
            .name("media-monitor".into())
            .spawn(move || {
                let _apartment = match ComApartment::initialize_mta() {
                    Ok(apartment) => apartment,
                    Err(error) => {
                        let _ = startup_tx.send(Err(error));
                        return;
                    }
                };
                let manager = match initialize() {
                    Ok(manager) => manager,
                    Err(error) => {
                        let _ = startup_tx.send(Err(error));
                        return;
                    }
                };
                if startup_tx.send(Ok(())).is_err() {
                    return;
                }
                drop(startup_tx);
                let mut last_semantic = String::new();
                loop {
                    if stop_clone.load(Ordering::Relaxed) {
                        break;
                    }
                    inner_clone.refresh_all(&manager);
                    if let Some(sample) = inner_clone.arbitrate() {
                        let semantic = format!(
                            "{}|{:?}|{:?}|{:?}|{}|{:?}",
                            sample.player_key,
                            sample.title,
                            sample.artist,
                            sample.album,
                            sample.playing,
                            sample.duration_seconds
                        );
                        if semantic != last_semantic {
                            last_semantic = semantic;
                            let _ = notify.send(());
                        }
                    } else if !last_semantic.is_empty() {
                        last_semantic.clear();
                        let _ = notify.send(());
                    }
                    std::thread::sleep(POLL_INTERVAL);
                }
            })
            .map_err(|e| e.to_string())?;
        let initialized = startup_rx
            .recv()
            .unwrap_or_else(|_| Err("media monitor stopped during initialization".into()));
        if let Err(error) = initialized {
            let _ = thread.join();
            return Err(error);
        }
        Ok(MediaMonitor {
            inner,
            stop,
            thread: Some(thread),
        })
    }

    pub fn current_media(&self) -> Option<MediaSample> {
        self.inner.arbitrate().map(|mut sample| {
            let now = Utc::now();
            if let Some(position) = sample.current_position(now) {
                sample.position_seconds = Some(position);
                sample.position_updated_at = now;
                sample.position_rate = 0.0;
            }
            sample
        })
    }

    pub fn set_preferred_players(&self, keys: Vec<String>) {
        *self.inner.preferred.lock().unwrap() = keys;
    }

    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for MediaMonitor {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn worker_initialization_failure_is_returned_by_spawn() {
        let (notify, events) = mpsc::channel();
        let result =
            MediaMonitor::spawn_with_initializer(
                notify,
                || Err("GSMTC unavailable fixture".into()),
            );
        assert_eq!(result.err().as_deref(), Some("GSMTC unavailable fixture"));
        assert_eq!(events.try_recv(), Err(mpsc::TryRecvError::Disconnected));
    }

    #[test]
    fn spawn_waits_for_worker_initialization() {
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let (result_tx, result_rx) = mpsc::channel();
        let caller = std::thread::spawn(move || {
            let (notify, _events) = mpsc::channel();
            let result = MediaMonitor::spawn_with_initializer(notify, move || {
                entered_tx.send(()).unwrap();
                release_rx.recv().unwrap();
                Err("GSMTC unavailable fixture".into())
            });
            result_tx.send(result).ok();
        });
        entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let premature_result = result_rx.recv_timeout(Duration::from_millis(50));
        release_tx.send(()).unwrap();
        caller.join().unwrap();
        assert!(matches!(
            premature_result,
            Err(mpsc::RecvTimeoutError::Timeout)
        ));
        assert_eq!(
            result_rx.recv().unwrap().err().as_deref(),
            Some("GSMTC unavailable fixture")
        );
    }
}
