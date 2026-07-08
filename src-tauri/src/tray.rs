//! System tray with a dynamic per-session menu (§6.3, §7.2). Rebuilt whenever
//! the session list changes so each active session gets an "Upload & Finish"
//! entry alongside the global navigation items.

use tauri::menu::MenuBuilder;
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager};

use crate::relay::{Session, SessionStatus};
use crate::window_state::MainWindowState;

const TRAY_ID: &str = "main";

/// Build the tray icon once, at startup.
pub fn build(app: &AppHandle) -> tauri::Result<()> {
    let menu = build_menu(app, &[])?;
    let icon = app
        .default_window_icon()
        .cloned()
        .expect("app should have a default window icon");

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .tooltip("Launcher")
        .menu(&menu)
        .on_menu_event(|app, event| on_menu(app, event.id().as_ref()))
        .on_tray_icon_event(|tray, event| {
            // Left click opens the app without changing the selected tab.
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                open_main_current_tab(tray.app_handle());
            }
        })
        .build(app)?;

    Ok(())
}

/// Rebuild the tray menu from the current session list.
pub fn refresh(app: &AppHandle, sessions: &[Session]) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    match build_menu(app, sessions) {
        Ok(menu) => {
            if let Err(e) = tray.set_menu(Some(menu)) {
                log::warn!("failed to update tray menu: {e}");
            }
        }
        Err(e) => log::warn!("failed to build tray menu: {e}"),
    }
}

fn build_menu(
    app: &AppHandle,
    sessions: &[Session],
) -> tauri::Result<tauri::menu::Menu<tauri::Wry>> {
    let mut b = MenuBuilder::new(app)
        .text("open-queue", "Relay Queue")
        .text("open-settings", "Settings…");

    let active: Vec<&Session> = sessions
        .iter()
        .filter(|s| !matches!(s.status, SessionStatus::Done))
        .collect();

    if !active.is_empty() {
        b = b.separator();
        for s in &active {
            let label = format!("{}  —  {}", s.filename, status_label(s.status));
            b = b.text(format!("open-queue-for:{}", s.id), label);
            // Offer the quick "Upload & Finish" for sessions where it applies.
            if matches!(s.status, SessionStatus::Editing | SessionStatus::Idle) {
                b = b.text(format!("upload:{}", s.id), "    ↳ Upload & Finish");
            }
        }
    }

    b = b.separator().text("quit", "Quit");
    b.build()
}

fn status_label(status: SessionStatus) -> &'static str {
    match status {
        SessionStatus::Downloading => "Downloading…",
        SessionStatus::Editing => "Editing",
        SessionStatus::Idle => "Idle, ready to upload",
        SessionStatus::Uploading => "Uploading…",
        SessionStatus::Done => "Done",
        SessionStatus::Error => "Error",
        SessionStatus::Orphaned => "Orphaned",
    }
}

fn on_menu(app: &AppHandle, id: &str) {
    match id {
        "quit" => app.exit(0),
        "open-queue" => open_main(app, "queue"),
        "open-settings" => open_main(app, "settings"),
        other if other.starts_with("upload:") => {
            let sid = other.trim_start_matches("upload:").to_string();
            let app = app.clone();
            let state = app
                .state::<std::sync::Arc<crate::state::AppState>>()
                .inner()
                .clone();
            tauri::async_runtime::spawn(async move {
                let _ = crate::relay::upload_finish(app, state, sid).await;
            });
        }
        other if other.starts_with("open-queue-for:") => open_main(app, "queue"),
        _ => {}
    }
}

/// Show + focus the main window without changing the frontend's selected tab.
pub fn open_main_current_tab(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        if let Some(state) = app.try_state::<std::sync::Arc<crate::state::AppState>>() {
            if let Err(e) = state
                .main_window_state
                .lock()
                .expect("main window state lock poisoned")
                .set(MainWindowState::Visible)
            {
                log::warn!("failed to persist main window visible state: {e}");
            }
        }
        let _ = win.unminimize();
        let _ = win.show();
        let _ = win.set_focus();
    }
}

/// Show + focus the main window and ask the frontend to switch to `view`.
pub fn open_main(app: &AppHandle, view: &str) {
    open_main_current_tab(app);
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.emit("navigate", view);
    }
}
