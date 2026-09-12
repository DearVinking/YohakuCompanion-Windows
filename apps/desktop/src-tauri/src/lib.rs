#![deny(clippy::undocumented_unsafe_blocks)]

mod commands;
mod tray;
mod wiring;

use tauri::Manager;

pub fn run() {
    let builder = command_builder();
    let handler_builder = builder.clone();
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main_window(app);
        }))
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .setup(move |app| {
            wiring::setup(app)?;
            tray::create(app)?;
            builder.mount_events(app);
            Ok(())
        })
        .on_window_event(|window, event| {
            // 关窗仅隐藏：托盘常驻
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(handler_builder.invoke_handler())
        .run(tauri::generate_context!())
        .expect("error while running Yohaku Companion");
}

pub fn show_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

pub fn command_builder() -> tauri_specta::Builder<tauri::Wry> {
    tauri_specta::Builder::<tauri::Wry>::new()
        .commands(tauri_specta::collect_commands![
            commands::get_settings,
            commands::update_settings,
            commands::get_privacy_rules,
            commands::update_privacy_rules,
            commands::get_connection_status,
            commands::start_pairing,
            commands::remove_pairing,
            commands::enable_live_desk,
            commands::disable_live_desk,
            commands::get_preview,
            commands::set_paused,
            commands::list_history,
            commands::clear_history,
            commands::get_s3_config,
            commands::update_s3_config,
            commands::quit_app,
        ])
        .error_handling(tauri_specta::ErrorHandlingMode::Throw)
}
