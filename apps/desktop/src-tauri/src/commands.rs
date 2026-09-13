use serde::Serialize;
use tauri::State;
use yohaku_app::coordinator::{LiveDeskEvent, MediaLookup, PresenceSources};
use yohaku_app::error::ApiError;
use yohaku_app::state::LiveDeskStatus;
use yohaku_protocol::CLIENT_VERSION;
use yohaku_store::privacy::PrivacyRules;
use yohaku_store::settings::{Settings, SettingsPatch};

use crate::wiring::App;

mod pairing;

#[tauri::command]
#[specta::specta]
pub fn get_settings(app: State<'_, App>) -> Result<Settings, ApiError> {
    Ok(app.pipeline.settings())
}

#[tauri::command]
#[specta::specta]
pub fn update_settings(
    handle: tauri::AppHandle,
    app: State<'_, App>,
    patch: SettingsPatch,
) -> Result<Settings, ApiError> {
    let settings = app.update_settings(&patch)?;
    if let Some(enabled) = patch.launch_at_login {
        crate::wiring::sync_autostart(&handle, enabled);
    }
    crate::tray::sync_menu(&handle);
    Ok(settings)
}

#[tauri::command]
#[specta::specta]
pub fn set_paused(
    handle: tauri::AppHandle,
    app: State<'_, App>,
    paused: bool,
) -> Result<Settings, ApiError> {
    let settings = app.set_paused(paused)?;
    crate::tray::sync_menu(&handle);
    Ok(settings)
}

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
        .map_err(|e| ApiError::new("STORE", e.to_string()))?;
    app.pipeline.replace_rules(normalized.clone());
    app.send_event(LiveDeskEvent::PolicyChanged);
    Ok(normalized)
}

#[derive(Serialize, specta::Type, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionStatusView {
    pub paired: bool,
    pub base_url: Option<String>,
    pub device_id: Option<String>,
    pub live_desk_enabled: bool,
    pub coordinator: LiveDeskStatus,
    pub app_version: String,
}

#[tauri::command]
#[specta::specta]
pub fn get_connection_status(
    handle: tauri::AppHandle,
    app: State<'_, App>,
) -> Result<ConnectionStatusView, ApiError> {
    crate::tray::sync_menu(&handle);
    let metadata = app
        .connection
        .load_metadata()
        .map_err(|e| ApiError::new("STORE", e.to_string()))?;
    let coordinator = app.status.lock().unwrap().clone();
    Ok(ConnectionStatusView {
        paired: metadata.is_some(),
        base_url: metadata.as_ref().map(|m| m.base_url.clone()),
        device_id: metadata.as_ref().map(|m| m.device_id.clone()),
        live_desk_enabled: metadata
            .as_ref()
            .map(|m| m.is_live_desk_enabled)
            .unwrap_or(false),
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
pub async fn start_pairing(
    app: State<'_, App>,
    server_url: String,
    pairing_code: String,
    device_name: String,
) -> Result<PairingResultView, ApiError> {
    let http = app.http.clone();
    let connection = app.connection.clone();
    let metadata = tauri::async_runtime::spawn_blocking(move || {
        pairing::pair(
            http.as_ref(),
            connection.as_ref(),
            &server_url,
            &pairing_code,
            &device_name,
        )
    })
    .await
    .map_err(|_| ApiError::new("INTERNAL", "配对任务未能完成。"))??;
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
        .map_err(|e| ApiError::new("STORE", e.to_string()))?;
    app.send_event(LiveDeskEvent::SettingsChanged);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn enable_live_desk(app: State<'_, App>) -> Result<(), ApiError> {
    app.connection
        .update_metadata(|m| {
            m.is_live_desk_enabled = true;
        })
        .map_err(|e| ApiError::new("STORE", e.to_string()))?;
    app.send_event(LiveDeskEvent::SettingsChanged);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn disable_live_desk(app: State<'_, App>) -> Result<(), ApiError> {
    app.connection
        .update_metadata(|m| {
            m.is_live_desk_enabled = false;
        })
        .map_err(|e| ApiError::new("STORE", e.to_string()))?;
    app.send_event(LiveDeskEvent::SettingsChanged);
    Ok(())
}

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
    pub availability: String,
    pub application: Option<PreviewApplication>,
    pub media: Option<PreviewMedia>,
}

#[tauri::command]
#[specta::specta]
pub fn get_preview(app: State<'_, App>) -> Result<PreviewView, ApiError> {
    let raw_app = app.sources.current_application();
    let raw_media = match app.sources.current_media() {
        MediaLookup::Session(state) => Some(*state),
        _ => None,
    };
    let mut tracker = yohaku_app::session::MediaSessionTracker::new();
    let snapshot = app
        .pipeline
        .capture(raw_app.as_ref(), raw_media.as_ref(), &mut tracker);
    Ok(PreviewView {
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

#[tauri::command]
#[specta::specta]
pub fn quit_app(app: tauri::AppHandle, state: State<'_, App>) -> Result<(), ApiError> {
    crate::wiring::request_quit(&app, &state);
    Ok(())
}
