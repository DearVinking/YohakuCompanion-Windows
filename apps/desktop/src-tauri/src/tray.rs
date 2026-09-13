use std::sync::Mutex;
use tauri::Manager;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{TrayIcon, TrayIconBuilder, TrayIconEvent};
use yohaku_app::state::CoordinatorState;

use crate::wiring::App;

struct TrayHandle {
    _tray: TrayIcon,
    status: MenuItem<tauri::Wry>,
    pause: MenuItem<tauri::Wry>,
    text: Mutex<MenuText>,
}

struct MenuText {
    status: &'static str,
    pause: &'static str,
}

impl MenuText {
    fn current(app: &App) -> Self {
        let state = app.status.lock().unwrap().state;
        let paused = app.pipeline.settings().pause_sharing;
        Self {
            status: match state {
                CoordinatorState::Active => "已连接",
                CoordinatorState::Connecting => "正在连接…",
                CoordinatorState::Degraded => "同步失败，等待重试",
                CoordinatorState::UpdateRequired => "请更新 Yohaku Companion",
                CoordinatorState::ServerFeatureUnavailable => "服务器暂不支持 Live Desk",
                CoordinatorState::Suspended => "已暂停同步",
                CoordinatorState::Disabled => "未开启同步",
            },
            pause: if paused {
                "恢复同步"
            } else {
                "暂停同步"
            },
        }
    }
}

pub fn create(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let text = MenuText::current(&app.state::<App>());
    let status = MenuItem::with_id(app, "status", text.status, false, None::<&str>)?;
    let open = MenuItem::with_id(app, "open", "打开设置", true, None::<&str>)?;
    let pause = MenuItem::with_id(app, "pause", text.pause, true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "退出 Yohaku Companion", true, None::<&str>)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&status, &sep, &open, &pause, &sep2, &quit])?;
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
                let _ = state.set_paused(paused);
                sync_menu(app);
            }
            "quit" => {
                let state = app.state::<App>();
                crate::wiring::request_quit(app, &state);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            sync_menu(tray.app_handle());
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
        _tray: tray,
        status,
        pause,
        text: Mutex::new(text),
    });
    Ok(())
}

pub fn sync_menu(app: &tauri::AppHandle) {
    let Some(handle) = app.try_state::<TrayHandle>() else {
        return;
    };
    let current = MenuText::current(&app.state::<App>());
    let mut previous = handle.text.lock().unwrap();
    if current.status != previous.status && handle.status.set_text(current.status).is_ok() {
        previous.status = current.status;
    }
    if current.pause != previous.pause && handle.pause.set_text(current.pause).is_ok() {
        previous.pause = current.pause;
    }
}
