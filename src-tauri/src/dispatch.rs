//! Entry point for an incoming `launcher://` URL, from any delivery path:
//! argv on cold start (§4.1/§4.3), the deep-link plugin (macOS Apple Event,
//! §4.2), or a forwarded URL from a second instance (§5).

use std::sync::Arc;

use tauri::{AppHandle, Manager};

use crate::error::AppResult;
use crate::state::AppState;
use crate::urlparse::{self, Route};

/// Parse and act on a single `launcher://` URL.
pub fn handle_url(app: &AppHandle, raw: &str) {
    log::info!("handling url: {raw}");
    if let Err(e) = dispatch(app, raw) {
        log::warn!("failed to handle url '{raw}': {e}");
    }
}

fn dispatch(app: &AppHandle, raw: &str) -> AppResult<()> {
    let state = app.state::<Arc<AppState>>().inner().clone();
    let route = match urlparse::parse(raw) {
        Ok(route) => route,
        Err(e) => {
            append_error_log(&state, raw, None, None, &e.to_string());
            return Err(e);
        }
    };

    match route {
        Route::Run {
            app_id,
            named,
            positional,
        } => {
            let log_id = append_handled_log(&state, raw, "run", Some(&app_id));
            if let Err(e) =
                crate::run::launch(&state, &app_id, &named, &positional, log_id.as_deref())
            {
                if let Some(id) = &log_id {
                    mark_error_log(&state, id, &e.to_string());
                }
                return Err(e);
            }
        }
        Route::Relay {
            app_id,
            src,
            dest,
            filename,
        } => {
            let log_id = append_handled_log(&state, raw, "relay", Some(&app_id));
            // Relay is async (download → spawn → watch); run it off the caller.
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = crate::relay::start_session(
                    app.clone(),
                    state.clone(),
                    app_id,
                    src,
                    dest,
                    filename,
                    log_id.clone(),
                )
                .await
                {
                    if let Some(id) = &log_id {
                        mark_error_log(&state, id, &e.to_string());
                    }
                    log::warn!("relay session failed to start: {e}");
                }
            });
        }
    }
    Ok(())
}

fn append_handled_log(
    state: &AppState,
    raw: &str,
    route_type: &str,
    app_id: Option<&str>,
) -> Option<String> {
    match state
        .logs
        .lock()
        .expect("logs lock poisoned")
        .append_handled_uri(raw, route_type, app_id)
    {
        Ok(id) => Some(id),
        Err(e) => {
            log::warn!("failed to append launch log: {e}");
            None
        }
    }
}

fn append_error_log(
    state: &AppState,
    raw: &str,
    route_type: Option<&str>,
    app_id: Option<&str>,
    error: &str,
) {
    if let Err(e) = state
        .logs
        .lock()
        .expect("logs lock poisoned")
        .append_error(raw, route_type, app_id, error)
    {
        log::warn!("failed to append error launch log: {e}");
    }
}

fn mark_error_log(state: &AppState, id: &str, error: &str) {
    if let Err(e) = state
        .logs
        .lock()
        .expect("logs lock poisoned")
        .mark_error(id, error)
    {
        log::warn!("failed to update launch log {id}: {e}");
    }
}

/// Scan a process's argv for the first `launcher://` argument (§4.1/§4.3).
pub fn url_from_args<I: IntoIterator<Item = String>>(args: I) -> Option<String> {
    args.into_iter().find(|a| a.starts_with("launcher://"))
}
