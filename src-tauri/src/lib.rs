//! Protocol handler application launcher — Tauri app entry point.
//! See `spec.md` for the full design; module docs map to spec sections.

mod commands;
mod config;
mod dispatch;
mod error;
mod logs;
mod registration;
mod relay;
mod run;
mod state;
mod substitute;
mod tray;
mod urlparse;

use std::sync::Arc;

use tauri::Manager;
use tauri_plugin_deep_link::DeepLinkExt;

use state::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // §5 single-instance + IPC: the plugin acquires a cross-platform lock
        // and forwards a second invocation's argv to the running instance. The
        // "deep-link" feature makes it cooperate with the deep-link plugin.
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            log::info!("second instance argv: {argv:?}");
            if let Some(url) = dispatch::url_from_args(argv) {
                dispatch::handle_url(app, &url);
            }
            // Surface the running instance regardless.
            tray::open_main(app, "queue");
        }))
        // §4.2 macOS Apple Event delivery + runtime deep-link handling.
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_log::Builder::new().build())
        .setup(|app| {
            let handle = app.handle().clone();

            // ---- paths & shared state ----
            let data_dir = handle.path().app_data_dir()?;
            let sessions_dir = data_dir.join("relay-sessions");
            std::fs::create_dir_all(&sessions_dir)?;

            let config_dir = handle.path().app_config_dir()?;
            std::fs::create_dir_all(&config_dir)?;
            let config_path = config::config_path(&config_dir);

            let state = Arc::new(AppState::new(
                sessions_dir,
                config_path,
                data_dir.join("logs.json"),
            )?);
            app.manage(state.clone());

            // ---- §4 OS scheme registration (per-user, no elevation) ----
            if let Err(e) = registration::register() {
                log::warn!("scheme registration failed: {e}");
            }

            // ---- deep-link delivery (§4.2 + already-running instances) ----
            let h = handle.clone();
            app.deep_link().on_open_url(move |event| {
                for url in event.urls() {
                    dispatch::handle_url(&h, url.as_str());
                }
            });

            // ---- §7.2 system tray ----
            tray::build(&handle)?;

            // ---- §6.6 crash recovery: surface orphaned sessions ----
            relay::recover_orphans(&handle, &state);

            // ---- cold-start URL from argv (§4.1/§4.3) ----
            if let Some(url) = dispatch::url_from_args(std::env::args()) {
                dispatch::handle_url(&handle, &url);
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_apps,
            commands::save_app,
            commands::delete_app,
            commands::export_config,
            commands::import_config,
            commands::import_config_from_url,
            commands::commit_import,
            commands::exec_exists,
            commands::list_sessions,
            commands::list_logs,
            commands::session_upload_finish,
            commands::session_retry,
            commands::session_keep_editing,
            commands::session_cancel,
            commands::session_resume,
            commands::session_upload_orphan,
            commands::session_discard,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
