//! 命令层：参数搬运 + App 操作，业务零残留。

use std::time::Instant;

use serde::{Deserialize, Serialize};
use tauri::State;
use yohaku_app::coordinator::{LiveDeskEvent, MediaLookup, PresenceSources};
use yohaku_app::error::ApiError;
use yohaku_app::s3::S3Config;
use yohaku_app::state::LiveDeskStatus;
use yohaku_protocol::CLIENT_VERSION;
use yohaku_protocol::error::{ResponseError, parse_capabilities};
use yohaku_protocol::negotiator::negotiate;
use yohaku_protocol::pairing::{
    PairingError, SCOPE_PRESENCE_WRITE, build_claim_body, parse_claim_response,
};
use yohaku_store::ConnectionMetadata;
use yohaku_store::history::{SyncEvent, SyncState};
use yohaku_store::privacy::PrivacyRules;
use yohaku_store::settings::{Settings, SettingsPatch};

use crate::wiring::App;

fn api_err(code: &str, message: impl Into<String>) -> ApiError {
    ApiError::new(code, message)
}

fn pairing_error(e: PairingError) -> ApiError {
    let code = match &e {
        PairingError::InvalidPairingCode | PairingError::InvalidDeviceName => "VALIDATION_FAILED",
        PairingError::MissingScope => "MISSING_SCOPE",
        _ => "MALFORMED",
    };
    ApiError::new(code, e.to_string())
}

// ---------- 设置 ----------

#[tauri::command]
#[specta::specta]
pub fn get_settings(app: State<'_, App>) -> Result<Settings, ApiError> {
    Ok(app.pipeline.settings())
}

#[tauri::command]
#[specta::specta]
pub fn update_settings(app: State<'_, App>, patch: SettingsPatch) -> Result<Settings, ApiError> {
    let settings = app.pipeline.update_settings(&patch);
    settings
        .save(&app.data_dir)
        .map_err(|e| api_err("STORE", e.to_string()))?;
    app.apply_settings_side_effects(&settings);
    Ok(settings)
}

#[tauri::command]
#[specta::specta]
pub fn set_paused(app: State<'_, App>, paused: bool) -> Result<Settings, ApiError> {
    let settings = app.pipeline.update_settings(&SettingsPatch {
        pause_sharing: Some(paused),
        ..Default::default()
    });
    settings
        .save(&app.data_dir)
        .map_err(|e| api_err("STORE", e.to_string()))?;
    app.send_event(LiveDeskEvent::SettingsChanged);
    Ok(settings)
}

// ---------- 隐私规则 ----------

#[tauri::command]
#[specta::specta]
pub fn get_privacy_rules(app: State<'_, App>) -> Result<PrivacyRules, ApiError> {
    Ok(app.pipeline.rules())
}

#[tauri::command]
#[specta::specta]
pub fn update_privacy_rules(
    app: State<'_, App>,
    rules: PrivacyRules,
) -> Result<PrivacyRules, ApiError> {
    let mut normalized = rules;
    normalized.schema_version = yohaku_store::privacy::PRIVACY_SCHEMA_VERSION;
    normalized
        .save(&app.data_dir)
        .map_err(|e| api_err("STORE", e.to_string()))?;
    app.pipeline.replace_rules(normalized.clone());
    // 策略变更：作废预览同意 + 协调器重发
    *app.preview_seen_at.lock().unwrap() = None;
    app.send_event(LiveDeskEvent::PolicyChanged);
    Ok(normalized)
}

// ---------- 连接 ----------

#[derive(Serialize, specta::Type, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionStatusView {
    pub paired: bool,
    pub base_url: Option<String>,
    pub device_id: Option<String>,
    pub live_desk_enabled: bool,
    /// 已开启但策略指纹与当前不符 → 需要重新确认
    pub consent_stale: bool,
    pub coordinator: LiveDeskStatus,
    pub app_version: String,
}

#[tauri::command]
#[specta::specta]
pub fn get_connection_status(app: State<'_, App>) -> Result<ConnectionStatusView, ApiError> {
    let metadata = app
        .connection
        .load_metadata()
        .map_err(|e| api_err("STORE", e.to_string()))?;
    let coordinator = app.status.lock().unwrap().clone();
    let consent_stale = match (
        &metadata,
        &metadata
            .as_ref()
            .and_then(|m| m.consent_fingerprint.clone()),
    ) {
        (Some(m), Some(fp)) => m.is_live_desk_enabled && *fp != app.pipeline.policy_fingerprint(),
        _ => false,
    };
    Ok(ConnectionStatusView {
        paired: metadata.is_some(),
        base_url: metadata.as_ref().map(|m| m.base_url.clone()),
        device_id: metadata.as_ref().map(|m| m.device_id.clone()),
        live_desk_enabled: metadata
            .as_ref()
            .map(|m| m.is_live_desk_enabled)
            .unwrap_or(false),
        consent_stale,
        coordinator,
        app_version: CLIENT_VERSION.to_string(),
    })
}

#[derive(Serialize, specta::Type, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PairingResultView {
    pub device_id: String,
}

#[tauri::command]
#[specta::specta]
pub fn start_pairing(
    app: State<'_, App>,
    server_url: String,
    pairing_code: String,
    device_name: String,
) -> Result<PairingResultView, ApiError> {
    let (code, name) =
        yohaku_protocol::pairing::validate_pairing_input(&pairing_code, &device_name)
            .map_err(|e| pairing_error(e))?;
    let base = server_url.trim_end_matches('/').to_string();
    // 1. 预检能力（无认证）
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
    let response = app
        .http
        .send(caps_request)
        .map_err(|e| api_err("TRANSPORT", e.0))?;
    let (_meta, caps) = parse_capabilities(&response.body).map_err(|e| match e {
        ResponseError::IncompatibleSchema | ResponseError::IncompatibleSchemaVersion => {
            api_err("SCHEMA_UNSUPPORTED", e.to_string())
        }
        other => api_err("INVALID_CAPABILITIES", other.to_string()),
    })?;
    match negotiate(&caps, CLIENT_VERSION) {
        yohaku_protocol::negotiator::Negotiation::Available(_) => {}
        yohaku_protocol::negotiator::Negotiation::ClientUpdateRequired => {
            return Err(api_err("UPDATE_REQUIRED", "客户端版本低于服务端最低要求"));
        }
        yohaku_protocol::negotiator::Negotiation::SchemaUnsupported => {
            return Err(api_err(
                "SCHEMA_UNSUPPORTED",
                "服务端不支持 Presence schema v2",
            ));
        }
        yohaku_protocol::negotiator::Negotiation::FeatureUnavailable => {
            return Err(api_err("FEATURE_UNAVAILABLE", "服务端未开启 Live Desk"));
        }
        yohaku_protocol::negotiator::Negotiation::InvalidCapabilities => {
            return Err(api_err("INVALID_CAPABILITIES", "服务端能力数据无效"));
        }
    }
    // 2. 消费一次性配对码
    let body = build_claim_body(&code, &name).map_err(|e| pairing_error(e))?;
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
    let response = app
        .http
        .send(claim_request)
        .map_err(|e| api_err("TRANSPORT", e.0))?;
    if !(200..300).contains(&response.status) {
        if let Some(server_error) = yohaku_protocol::error::parse_error(&response.body) {
            return Err(api_err(&server_error.code, server_error.message));
        }
        return Err(api_err("HTTP_ERROR", format!("status {}", response.status)));
    }
    let claim = parse_claim_response(&response.body).map_err(|e| pairing_error(e))?;
    if !claim.scopes.iter().any(|s| s == SCOPE_PRESENCE_WRITE) {
        return Err(pairing_error(PairingError::MissingScope));
    }
    // 3. 安装（token 入受保护存储；metadata 提交；Live Desk 默认关闭）
    let metadata: ConnectionMetadata = app
        .connection
        .install_pairing_claim(
            &claim.device_id,
            &claim.device_token,
            &claim.scopes,
            claim.next_sequence,
            &base,
        )
        .map_err(|e| api_err("STORE", e.to_string()))?;
    app.send_event(LiveDeskEvent::SettingsChanged);
    Ok(PairingResultView {
        device_id: metadata.device_id,
    })
}

#[tauri::command]
#[specta::specta]
pub fn remove_pairing(app: State<'_, App>) -> Result<(), ApiError> {
    app.connection
        .clear()
        .map_err(|e| api_err("STORE", e.to_string()))?;
    app.send_event(LiveDeskEvent::SettingsChanged);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn enable_live_desk(app: State<'_, App>) -> Result<(), ApiError> {
    // 同意门：必须先看过净化预览
    if !app.preview_fresh() {
        return Err(api_err("CONSENT_REQUIRED", "请先查看当前净化预览"));
    }
    let fingerprint = app.pipeline.policy_fingerprint();
    app.connection
        .update_metadata(|m| {
            m.is_live_desk_enabled = true;
            m.consent_fingerprint = Some(fingerprint.clone());
        })
        .map_err(|e| api_err("STORE", e.to_string()))?;
    app.send_event(LiveDeskEvent::SettingsChanged);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn disable_live_desk(app: State<'_, App>) -> Result<(), ApiError> {
    app.connection
        .update_metadata(|m| {
            m.is_live_desk_enabled = false;
            m.consent_fingerprint = None;
        })
        .map_err(|e| api_err("STORE", e.to_string()))?;
    app.send_event(LiveDeskEvent::SettingsChanged);
    Ok(())
}

// ---------- 预览 ----------

#[derive(Serialize, specta::Type, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PreviewApplication {
    pub display_name: String,
    pub window_title: Option<String>,
}

#[derive(Serialize, specta::Type, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PreviewMedia {
    pub kind: String,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub player: Option<String>,
    pub playing: bool,
    pub position_seconds: Option<f64>,
    pub duration_seconds: Option<f64>,
}

#[derive(Serialize, specta::Type, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PreviewView {
    pub fingerprint: String,
    pub availability: String,
    pub application: Option<PreviewApplication>,
    pub media: Option<PreviewMedia>,
}

#[tauri::command]
#[specta::specta]
pub fn get_preview(app: State<'_, App>) -> Result<PreviewView, ApiError> {
    let raw_app = app.sources.current_application();
    let raw_media = match app.sources.current_media() {
        yohaku_app::coordinator::MediaLookup::Session(state) => Some(*state),
        _ => None,
    };
    let mut tracker = yohaku_app::session::MediaSessionTracker::new();
    let snapshot = app
        .pipeline
        .capture(raw_app.as_ref(), raw_media.as_ref(), &mut tracker);
    *app.preview_seen_at.lock().unwrap() = Some(Instant::now());
    Ok(PreviewView {
        fingerprint: app.pipeline.policy_fingerprint(),
        availability: match snapshot.availability {
            yohaku_protocol::presence::Availability::Idle => "idle".into(),
            yohaku_protocol::presence::Availability::Active => "active".into(),
        },
        application: snapshot.application.map(|a| PreviewApplication {
            display_name: a.display_name,
            window_title: a.window_title,
        }),
        media: snapshot.media.map(|m| PreviewMedia {
            kind: match m.kind {
                yohaku_protocol::presence::MediaKind::Music => "music".into(),
                yohaku_protocol::presence::MediaKind::Podcast => "podcast".into(),
                yohaku_protocol::presence::MediaKind::Video => "video".into(),
                yohaku_protocol::presence::MediaKind::Unknown => "unknown".into(),
            },
            title: m.title,
            artist: m.artist,
            album: m.album,
            player: m.player_display_name,
            playing: m.playing,
            position_seconds: m.position_seconds,
            duration_seconds: m.duration_seconds,
        }),
    })
}

// ---------- 历史 ----------

#[tauri::command]
#[specta::specta]
pub fn list_history(app: State<'_, App>) -> Result<Vec<SyncEvent>, ApiError> {
    let mut events = app
        .history
        .list(&app.data_dir)
        .map_err(|e| api_err("STORE", e.to_string()))?;
    events.reverse(); // 新的在前
    Ok(events)
}

#[tauri::command]
#[specta::specta]
pub fn clear_history(app: State<'_, App>) -> Result<(), ApiError> {
    app.history
        .clear(&app.data_dir)
        .map_err(|e| api_err("STORE", e.to_string()))
}

// ---------- S3 ----------

#[derive(Serialize, specta::Type, Clone)]
#[serde(rename_all = "camelCase")]
pub struct S3ConfigView {
    pub endpoint: Option<String>,
    pub bucket: String,
    pub region: String,
    pub custom_domain: Option<String>,
    pub base_path: String,
    pub access_key: String,
    pub has_credentials: bool,
}

#[derive(Deserialize, specta::Type, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct S3ConfigPatch {
    pub endpoint: Option<String>,
    pub bucket: Option<String>,
    pub region: Option<String>,
    pub custom_domain: Option<String>,
    pub base_path: Option<String>,
    pub access_key: Option<String>,
    /// 明文仅在内存中；空字符串/None 表示保持不变
    pub secret_key: Option<String>,
}

#[tauri::command]
#[specta::specta]
pub fn get_s3_config(app: State<'_, App>) -> Result<S3ConfigView, ApiError> {
    let cfg = app.load_s3_config();
    Ok(S3ConfigView {
        endpoint: cfg.endpoint,
        bucket: cfg.bucket,
        region: cfg.region,
        custom_domain: cfg.custom_domain,
        base_path: cfg.base_path,
        access_key: cfg.access_key,
        has_credentials: !cfg.secret_key.is_empty(),
    })
}

#[tauri::command]
#[specta::specta]
pub fn update_s3_config(
    app: State<'_, App>,
    patch: S3ConfigPatch,
) -> Result<S3ConfigView, ApiError> {
    let mut cfg = app.load_s3_config();
    if let Some(v) = patch.endpoint {
        cfg.endpoint = (!v.trim().is_empty()).then_some(v.trim().to_string());
    }
    if let Some(v) = patch.bucket {
        cfg.bucket = v.trim().to_string();
    }
    if let Some(v) = patch.region {
        cfg.region = v.trim().to_string();
    }
    if let Some(v) = patch.custom_domain {
        cfg.custom_domain =
            (!v.trim().is_empty()).then_some(v.trim().trim_end_matches('/').to_string());
    }
    if let Some(v) = patch.base_path {
        cfg.base_path = v.trim().to_string();
    }
    if let Some(v) = patch.access_key {
        cfg.access_key = v.trim().to_string();
    }
    if let Some(v) = patch.secret_key {
        if !v.is_empty() {
            cfg.secret_key = v;
        }
    }
    app.save_s3_config(&cfg)
        .map_err(|e| api_err("STORE", e.to_string()))?;
    app.assets.set_config(Some(cfg.clone()));
    // 资产 host 白名单变化 → 协调器重协商
    app.send_event(LiveDeskEvent::SettingsChanged);
    Ok(S3ConfigView {
        endpoint: cfg.endpoint,
        bucket: cfg.bucket,
        region: cfg.region,
        custom_domain: cfg.custom_domain,
        base_path: cfg.base_path,
        access_key: cfg.access_key,
        has_credentials: !cfg.secret_key.is_empty(),
    })
}

// ---------- 退出 ----------

#[tauri::command]
#[specta::specta]
pub fn quit_app(app: tauri::AppHandle, state: State<'_, App>) -> Result<(), ApiError> {
    let _ = state.events.send(LiveDeskEvent::Shutdown);
    // 给 best-effort 清除留出时间（上限 500ms + 余量）
    std::thread::sleep(std::time::Duration::from_millis(700));
    app.exit(0);
    Ok(())
}
