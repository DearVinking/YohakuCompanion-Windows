//! 壳层接线：组装 store/app/platform，为协调器桥接平台捕获源，
//! 并把状态暴露给命令层。

use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::Manager;

use yohaku_app::capture::{RawApplicationFocus, RawMediaState};
use yohaku_app::coordinator::{
    AssetHosting, Coordinator, CoordinatorDeps, LiveDeskEvent, MediaLookup, PresenceSources,
    S3AssetHosting, run,
};
use yohaku_app::ports::{Clock, HttpRequest, HttpResponse, HttpTransport, TransportError};
use yohaku_app::privacy::PrivacyPipeline;
use yohaku_app::state::LiveDeskStatus;
use yohaku_platform::foreground::{FocusSample, ForegroundMonitor, SharedSample};
use yohaku_platform::icons::extract_png;
use yohaku_platform::media::MediaMonitor;
use yohaku_platform::system::{SystemEvent, SystemEvents};
use yohaku_store::history::HistoryStore;
use yohaku_store::privacy::PrivacyRules;
use yohaku_store::settings::Settings;
use yohaku_store::{ConnectionStore, SecretStore, StoreResult};

/// ureq 阻塞传输（超时按请求覆盖）。
pub struct UreqTransport {
    agent: ureq::Agent,
}

impl UreqTransport {
    pub fn new() -> Self {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(10)))
            .build()
            .new_agent();
        UreqTransport { agent }
    }
}

impl Default for UreqTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpTransport for UreqTransport {
    fn send(&self, request: HttpRequest) -> Result<HttpResponse, TransportError> {
        let response = match request.method {
            "GET" => {
                let mut b = self.agent.get(&request.url);
                for (key, value) in &request.headers {
                    b = b.header(key, value);
                }
                b.call()
            }
            "PUT" | "POST" => {
                let builder = if request.method == "PUT" {
                    self.agent.put(&request.url)
                } else {
                    self.agent.post(&request.url)
                };
                let mut b = builder;
                for (key, value) in &request.headers {
                    b = b.header(key, value);
                }
                let body = request.body.clone().unwrap_or_default();
                b.send(&body[..])
            }
            other => return Err(TransportError(format!("unsupported method {other}"))),
        }
        .map_err(|e| TransportError(e.to_string()))?;
        let status = response.status().as_u16();
        let body = response
            .into_body()
            .read_to_vec()
            .map_err(|e| TransportError(e.to_string()))?;
        Ok(HttpResponse { status, body })
    }
}

struct FixedClock;

impl Clock for FixedClock {
    fn now(&self) -> chrono::DateTime<chrono::Utc> {
        chrono::Utc::now()
    }
}

/// 平台捕获源 → app 层 PresenceSources 适配器。
pub struct ShellSources {
    focus: SharedSample<FocusSample>,
    media_monitor: Mutex<Option<Arc<MediaMonitor>>>,
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
        let guard = self.media_monitor.lock().unwrap();
        let Some(monitor) = guard.as_ref() else {
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
                    sample.artwork_bytes,
                )))
            }
            None => MediaLookup::NoSession,
        }
    }
}

impl ShellSources {
    fn media_monitor(&self) -> Option<Arc<MediaMonitor>> {
        self.media_monitor.lock().unwrap().clone()
    }
}

/// 图标 PNG 提取（applicationKey → exe 路径 → PNG）。
struct ShellIconProvider {
    focus: SharedSample<FocusSample>,
    cache: Mutex<std::collections::HashMap<String, Option<Vec<u8>>>>,
}

impl yohaku_app::coordinator::IconByteProvider for ShellIconProvider {
    fn icon_png(&self, application_key: &str) -> Option<Vec<u8>> {
        if let Some(cached) = self.cache.lock().unwrap().get(application_key) {
            return cached.clone();
        }
        // 前台样本里存的是 key；需要 exe 完整路径 → 由 monitor 样本提供不足，
        // 退化为 key（exe 文件名）在常见安装目录探测不可行——改为：
        // foreground monitor 记录的 sample 扩展了 exe 路径（见 FocusSample）。
        let path = self
            .focus
            .lock()
            .unwrap()
            .as_ref()
            .filter(|s| s.application_key == application_key)
            .map(|s| s.exe_path.clone());
        let png = path.and_then(|p| extract_png(&p));
        self.cache
            .lock()
            .unwrap()
            .insert(application_key.to_string(), png.clone());
        png
    }
}

/// 管理的结构体（Tauri State）。
pub struct App {
    pub data_dir: PathBuf,
    pub connection: Arc<ConnectionStore>,
    pub pipeline: Arc<PrivacyPipeline>,
    pub assets: Arc<S3AssetHosting>,
    pub http: Arc<dyn HttpTransport>,
    pub history: Arc<HistoryStore>,
    pub events: Sender<LiveDeskEvent>,
    pub status: Arc<Mutex<LiveDeskStatus>>,
    pub sources: Arc<ShellSources>,
    pub secrets: Arc<dyn SecretStore>,
    /// 预览可见性（开启 Live Desk 的同意门）：get_preview 时刷新
    pub preview_seen_at: Mutex<Option<Instant>>,
}

impl App {
    pub fn preview_fresh(&self) -> bool {
        self.preview_seen_at
            .lock()
            .unwrap()
            .map(|t| t.elapsed() < Duration::from_secs(600))
            .unwrap_or(false)
    }

    pub fn send_event(&self, event: LiveDeskEvent) {
        let _ = self.events.send(event);
    }

    pub fn load_s3_config(&self) -> yohaku_app::s3::S3Config {
        let mut cfg: yohaku_app::s3::S3Config =
            yohaku_store::json_io::read_json_opt(&self.data_dir.join("s3.json"))
                .ok()
                .flatten()
                .unwrap_or_default();
        if let Ok(Some(secret)) = self.secrets.get(yohaku_store::S3_SECRET_KEY) {
            cfg.secret_key = String::from_utf8_lossy(&secret).to_string();
        }
        cfg
    }

    pub fn save_s3_config(&self, cfg: &yohaku_app::s3::S3Config) -> StoreResult<()> {
        self.secrets
            .set(yohaku_store::S3_SECRET_KEY, cfg.secret_key.as_bytes())?;
        yohaku_store::json_io::write_json(&self.data_dir.join("s3.json"), cfg)
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
    let connection = Arc::new(ConnectionStore::new(&data_dir, secrets.clone()));
    let settings = Settings::load(&data_dir)?;
    let rules = PrivacyRules::load(&data_dir)?;
    let pipeline = Arc::new(PrivacyPipeline::new(settings, rules));

    let http: Arc<dyn HttpTransport> = Arc::new(UreqTransport::new());
    let clock: Arc<dyn Clock> = Arc::new(FixedClock);
    let assets = Arc::new(S3AssetHosting::new(http.clone(), clock.clone()));
    // 启动即装载 S3 配置（若有）
    let s3_dir = data_dir.clone();
    let s3_cfg: Option<yohaku_app::s3::S3Config> = {
        let mut cfg: yohaku_app::s3::S3Config =
            yohaku_store::json_io::read_json_opt(&s3_dir.join("s3.json"))
                .ok()
                .flatten()
                .unwrap_or_default();
        if let Ok(Some(secret)) = secrets.get(yohaku_store::S3_SECRET_KEY) {
            cfg.secret_key = String::from_utf8_lossy(&secret).to_string();
        }
        cfg.is_configured().then_some(cfg)
    };
    assets.set_config(s3_cfg);

    let (event_tx, event_rx) = std::sync::mpsc::channel::<LiveDeskEvent>();

    // 前台监控
    let focus: SharedSample<FocusSample> = Arc::new(Mutex::new(None));
    let (focus_tx, focus_rx) = std::sync::mpsc::channel::<()>();
    ForegroundMonitor::spawn(focus_tx, focus.clone());

    // 媒体监控（GSMTC 不可用时保持 None → Unavailable）
    let (media_tx, media_rx) = std::sync::mpsc::channel::<()>();
    let media_monitor: Option<Arc<MediaMonitor>> = MediaMonitor::spawn(media_tx).ok().map(Arc::new);

    // 系统事件
    let (system_tx, system_rx) = std::sync::mpsc::channel::<SystemEvent>();
    SystemEvents::spawn(system_tx);

    // 平台通知 → 协调器事件桥接（三条阻塞转发线程）
    {
        let tx = event_tx.clone();
        std::thread::Builder::new()
            .name("focus-forward".into())
            .spawn(move || {
                while let Ok(()) = focus_rx.recv() {
                    if tx.send(LiveDeskEvent::AppChanged).is_err() {
                        break;
                    }
                }
            })
            .map_err(|e| e.to_string())?;
    }
    {
        let tx = event_tx.clone();
        std::thread::Builder::new()
            .name("media-forward".into())
            .spawn(move || {
                while let Ok(()) = media_rx.recv() {
                    if tx.send(LiveDeskEvent::MediaSemanticChanged).is_err() {
                        break;
                    }
                }
            })
            .map_err(|e| e.to_string())?;
    }
    {
        let tx = event_tx.clone();
        std::thread::Builder::new()
            .name("system-forward".into())
            .spawn(move || {
                while let Ok(event) = system_rx.recv() {
                    let mapped = match event {
                        SystemEvent::SleepOrLock => LiveDeskEvent::SleepOrLock,
                        SystemEvent::Wake => LiveDeskEvent::Wake,
                        SystemEvent::NetworkUp => LiveDeskEvent::NetworkUp,
                    };
                    if tx.send(mapped).is_err() {
                        break;
                    }
                }
            })
            .map_err(|e| e.to_string())?;
    }
    if let Some(monitor) = &media_monitor {
        monitor.set_preferred_players(pipeline.settings().preferred_players.clone());
    }

    // 系统事件（睡眠/锁屏/网络）
    let sources = Arc::new(ShellSources {
        focus: focus.clone(),
        media_monitor: Mutex::new(media_monitor),
    });

    // 图标提供器：需要 exe 完整路径——扩展 FocusSample 的字段见 foreground 模块
    let icon_provider = Arc::new(ShellIconProvider {
        focus: focus.clone(),
        cache: Mutex::new(std::collections::HashMap::new()),
    });
    assets.set_icon_provider(Some(icon_provider));

    let history = Arc::new(HistoryStore::default());
    let status = Arc::new(Mutex::new(LiveDeskStatus::default()));
    let coordinator = Coordinator::new(
        CoordinatorDeps {
            connection: connection.clone(),
            pipeline: pipeline.clone(),
            sources: sources.clone(),
            assets: assets.clone(),
            http: http.clone(),
            clock: clock.clone(),
            monotonic: Arc::new(yohaku_app::ports::RealMonotonic::new()),
            history: history.clone(),
            history_dir: data_dir.clone(),
            sequence_persistence: Arc::new(yohaku_app::coordinator::StoreSequencePersistence {
                store: connection.clone(),
            }),
        },
        status.clone(),
    );
    std::thread::Builder::new()
        .name("livedesk-coordinator".into())
        .spawn(move || run(coordinator, event_rx))
        .map_err(|e| format!("spawn coordinator: {e}"))?;

    // 开机自启同步
    let launch_at_login = pipeline.settings().launch_at_login;
    sync_autostart(app.handle(), launch_at_login);

    app.manage(App {
        data_dir,
        connection,
        pipeline,
        assets,
        http,
        history,
        events: event_tx,
        status,
        sources,
        secrets,
        preview_seen_at: Mutex::new(None),
    });
    Ok(())
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
