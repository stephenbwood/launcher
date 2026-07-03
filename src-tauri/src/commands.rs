//! Tauri commands bridging the frontend UI to the Rust core (§7.1, §7.2).

use std::sync::Arc;

use tauri::{AppHandle, State};

use crate::config::{self, AppConfig, AppDefinition};
use crate::error::{AppError, AppResult};
use crate::relay::{self, Session};
use crate::state::AppState;

// ---- Settings screen: app definitions CRUD (§7.1) ----

/// List all registered app definitions.
#[tauri::command]
pub fn list_apps(state: State<'_, Arc<AppState>>) -> AppResult<AppConfig> {
    config::load(&state.config_path)
}

/// Create or update an app definition. `app_id` is immutable after creation
/// (§7.1); to enforce this, callers editing an existing entry must pass the same
/// id. Returns the updated config.
#[tauri::command]
pub fn save_app(
    state: State<'_, Arc<AppState>>,
    app_id: String,
    definition: AppDefinition,
) -> AppResult<AppConfig> {
    if app_id.trim().is_empty() {
        return Err(AppError::Config("app-id is required".into()));
    }
    if definition.exec.trim().is_empty() {
        return Err(AppError::Config("exec is required".into()));
    }
    let mut cfg = config::load(&state.config_path)?;
    cfg.insert(app_id, definition);
    config::save(&state.config_path, &cfg)?;
    Ok(cfg)
}

/// Delete an app definition. Blocked if an in-flight relay session references it
/// (§7.1 — deleting a referenced app-id should be blocked/warned).
#[tauri::command]
pub fn delete_app(state: State<'_, Arc<AppState>>, app_id: String) -> AppResult<AppConfig> {
    let referenced = {
        let map = state.sessions.lock().expect("sessions lock poisoned");
        map.values().any(|r| r.session.app_id == app_id)
    };
    if referenced {
        return Err(AppError::Config(format!(
            "cannot delete '{app_id}': an active relay session is using it"
        )));
    }
    let mut cfg = config::load(&state.config_path)?;
    cfg.remove(&app_id);
    config::save(&state.config_path, &cfg)?;
    Ok(cfg)
}

/// Export the current app definitions to a user-chosen file (pretty JSON).
#[tauri::command]
pub fn export_config(state: State<'_, Arc<AppState>>, path: String) -> AppResult<()> {
    let cfg = config::load(&state.config_path)?;
    let text = serde_json::to_string_pretty(&cfg)?;
    std::fs::write(&path, text)
        .map_err(|e| AppError::Config(format!("could not write {path}: {e}")))?;
    Ok(())
}

/// A validated import that hasn't been applied yet. `conflicts` lists the
/// app-ids present in both the imported config and the current one, so the UI
/// can ask the user whether to keep or replace each before committing.
#[derive(serde::Serialize)]
pub struct ImportPreview {
    imported: AppConfig,
    conflicts: Vec<String>,
}

/// Import app definitions from a file. Parses and validates, but does not save —
/// returns a preview for the frontend to resolve conflicts against (see
/// `commit_import`).
#[tauri::command]
pub fn import_config(state: State<'_, Arc<AppState>>, path: String) -> AppResult<ImportPreview> {
    let text = std::fs::read_to_string(&path)
        .map_err(|e| AppError::Config(format!("could not read {path}: {e}")))?;
    preview_import(&state, &text)
}

/// Import app definitions from an HTTPS URL. Parses and validates without saving.
#[tauri::command]
pub async fn import_config_from_url(
    state: State<'_, Arc<AppState>>,
    url: String,
) -> AppResult<ImportPreview> {
    let url = url.trim();
    // Restrict to HTTPS: config drives which executables get launched, so we
    // don't fetch it over plaintext transports.
    if !url.to_ascii_lowercase().starts_with("https://") {
        return Err(AppError::Config("URL must start with https://".into()));
    }
    let text = state
        .http
        .get(url)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    preview_import(&state, &text)
}

/// Parse + validate an imported config and compute which app-ids conflict with
/// the current config.
fn preview_import(state: &AppState, text: &str) -> AppResult<ImportPreview> {
    let imported = parse_and_validate(text)?;
    let current = config::load(&state.config_path)?;
    let conflicts = imported
        .keys()
        .filter(|id| current.contains_key(*id))
        .cloned()
        .collect();
    Ok(ImportPreview {
        imported,
        conflicts,
    })
}

/// Merge a previewed import into the current config and persist it. Imported
/// app-ids that don't already exist are always added; conflicting ids are only
/// overwritten when listed in `replace_ids` (otherwise the existing definition
/// is kept). No existing app is ever removed, so a merge can't orphan an active
/// relay session.
#[tauri::command]
pub fn commit_import(
    state: State<'_, Arc<AppState>>,
    imported: AppConfig,
    replace_ids: Vec<String>,
) -> AppResult<AppConfig> {
    // Re-validate defensively (the payload made a round-trip through the UI).
    validate(&imported)?;

    let replace: std::collections::HashSet<String> = replace_ids.into_iter().collect();
    let mut current = config::load(&state.config_path)?;

    for (id, def) in imported {
        let exists = current.contains_key(&id);
        if !exists || replace.contains(&id) {
            current.insert(id, def);
        }
        // else: conflicting id the user chose to keep — leave current as-is.
    }

    config::save(&state.config_path, &current)?;
    Ok(current)
}

/// Deserialize an imported config from text and validate it.
fn parse_and_validate(text: &str) -> AppResult<AppConfig> {
    let cfg: AppConfig = serde_json::from_str(text)
        .map_err(|e| AppError::Config(format!("invalid config: {e}")))?;
    validate(&cfg)?;
    Ok(cfg)
}

/// Every entry needs a non-empty exec (serde already enforces the field's
/// presence; guard against blank values here).
fn validate(cfg: &AppConfig) -> AppResult<()> {
    for (id, def) in cfg {
        if def.exec.trim().is_empty() {
            return Err(AppError::Config(format!("app '{id}' has an empty exec")));
        }
    }
    Ok(())
}

/// Non-fatal existence check for the Settings edit form (§7.1).
#[tauri::command]
pub fn exec_exists(path: String) -> bool {
    if path.trim().is_empty() {
        return false;
    }
    // Absolute path that resolves, or a bare command name found on PATH.
    if std::path::Path::new(&path).exists() {
        return true;
    }
    which_on_path(&path).is_some()
}

fn which_on_path(cmd: &str) -> Option<std::path::PathBuf> {
    let paths = std::env::var_os("PATH")?;
    let exts: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".EXE;.BAT;.CMD".into())
            .split(';')
            .map(|s| s.to_string())
            .collect()
    } else {
        vec![String::new()]
    };
    for dir in std::env::split_paths(&paths) {
        for ext in &exts {
            let candidate = dir.join(format!("{cmd}{ext}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

// ---- Relay Queue screen: live session list + actions (§7.2) ----

/// Current session list (the frontend also subscribes to `relay:update` events).
#[tauri::command]
pub fn list_sessions(state: State<'_, Arc<AppState>>) -> Vec<Session> {
    relay::snapshot(&state)
}

#[tauri::command]
pub async fn session_upload_finish(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    id: String,
) -> AppResult<()> {
    relay::upload_finish(app, state.inner().clone(), id).await
}

#[tauri::command]
pub async fn session_retry(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    id: String,
) -> AppResult<()> {
    relay::retry(app, state.inner().clone(), id).await
}

#[tauri::command]
pub fn session_keep_editing(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    id: String,
) -> AppResult<()> {
    relay::keep_editing(&app, &state, &id)
}

#[tauri::command]
pub fn session_cancel(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    id: String,
) -> AppResult<()> {
    relay::discard(&app, &state, &id)
}

// ---- Orphan triage (§6.6) ----

#[tauri::command]
pub fn session_resume(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    id: String,
) -> AppResult<()> {
    relay::resume(&app, &state.inner().clone(), &id)
}

#[tauri::command]
pub async fn session_upload_orphan(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    id: String,
) -> AppResult<()> {
    relay::upload_finish(app, state.inner().clone(), id).await
}

#[tauri::command]
pub fn session_discard(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    id: String,
) -> AppResult<()> {
    relay::discard(&app, &state, &id)
}
