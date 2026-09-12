//! 托盘：状态、打开设置、暂停/恢复、退出。状态文本由前端轮询同步。

use std::sync::Mutex;
use tauri::Manager;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{TrayIcon, TrayIconBuilder, TrayIconEvent};

use crate::wiring::App;

pub fn create(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let menu = build_menu(app.handle(), None, false)?;
    let tray = TrayIconBuilder::with_id("main")
        .icon(app.default_window_icon().unwrap().clone())
        .tooltip("Yohaku Companion")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => crate::show_main_window(app),
            "pause" => {
                let state = app.state::<App>();
                let paused = !state.pipeline.settings().pause_sharing;
                let settings =
                    state
                        .pipeline
                        .update_settings(&yohaku_store::settings::SettingsPatch {
                            pause_sharing: Some(paused),
                            ..Default::default()
                        });
                let _ = settings.save(&state.data_dir);
                state.send_event(LiveDeskEvent::SettingsChanged);
                sync_menu(app);
            }
            "quit" => {
                let state = app.state::<App>();
                let _ = state.events.send(LiveDeskEvent::Shutdown);
                std::thread::sleep(std::time::Duration::from_millis(700));
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: tauri::tray::MouseButton::Left,
                    button_state: tauri::tray::MouseButtonState::Up,
                    ..
                }
            ) {
                crate::show_main_window(tray.app_handle());
            }
        })
        .build(app)?;
    app.manage(TrayHandle {
        tray: Mutex::new(tray),
    });
    Ok(())
}

pub struct TrayHandle {
    pub tray: Mutex<TrayIcon>,
}

use yohaku_app::coordinator::LiveDeskEvent;

fn build_menu(
    app: &tauri::AppHandle,
    status_text: Option<&str>,
    paused: bool,
) -> Result<Menu<tauri::Wry>, tauri::Error> {
    let status = MenuItem::with_id(
        app,
        "status",
        status_text.unwrap_or("未启用"),
        false,
        None::<&str>,
    )?;
    let open = MenuItem::with_id(app, "open", "打开设置", true, None::<&str>)?;
    let pause = MenuItem::with_id(
        app,
        "pause",
        if paused {
            "恢复上报"
        } else {
            "暂停上报"
        },
        true,
        None::<&str>,
    )?;
    let sep = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    Menu::with_items(app, &[&status, &sep, &open, &pause, &sep2, &quit])
}

/// 前端状态轮询时同步托盘菜单（文本 + 暂停态）。
pub fn sync_menu(app: &tauri::AppHandle) {
    let Some(handle) = app.try_state::<TrayHandle>() else {
        return;
    };
    let state = app.state::<App>();
    let status = state.status.lock().unwrap().clone();
    let paused = state.pipeline.settings().pause_sharing;
    let text = match status.state {
        yohaku_app::state::CoordinatorState::Active => "Live Desk · 已连接",
        yohaku_app::state::CoordinatorState::Connecting => "连接中…",
        yohaku_app::state::CoordinatorState::Degraded => "降级（最近发送失败）",
        yohaku_app::state::CoordinatorState::UpdateRequired => "需要更新客户端",
        yohaku_app::state::CoordinatorState::ServerFeatureUnavailable => "服务端功能不可用",
        yohaku_app::state::CoordinatorState::Suspended => "已暂停",
        yohaku_app::state::CoordinatorState::Disabled => "未启用",
    };
    if let Ok(menu) = build_menu(app, Some(text), paused) {
        if let Ok(mut tray) = handle.tray.lock() {
            let _ = tray.set_menu(Some(menu));
        }
    }
}
