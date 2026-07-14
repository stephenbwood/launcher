//! Relay mode (§6): download a working file, launch an editor against it, and
//! upload the result on a completion signal. Multiple sessions run concurrently
//! and independently (§6.3).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use notify::RecursiveMode;
use notify_debouncer_mini::{new_debouncer, DebounceEventResult, Debouncer};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use crate::config;
use crate::error::{AppError, AppResult};
use crate::process::SpawnCommand;
use crate::state::AppState;
use crate::substitute;

/// Frontend event emitted whenever the session list changes.
pub const EVENT_UPDATE: &str = "relay:update";

const UPLOAD_MAX_ATTEMPTS: u32 = 4;
const UPLOAD_BASE_BACKOFF_MS: u64 = 500;

/// Per-session status (§6.2, §7.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionStatus {
    Downloading,
    Editing,
    Idle,
    Uploading,
    Done,
    Error,
    Orphaned,
}

/// Resolved upload configuration for a session, snapshotted from the app's
/// relay block at start time so it survives config edits and crash recovery.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UploadSpec {
    #[serde(default, skip_serializing_if = "config::UploadMethod::is_put")]
    pub method: config::UploadMethod,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field_name: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub form_fields: Vec<config::FormField>,
}

/// Persisted session metadata (`session.json`, §6.1 step 4).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub app_id: String,
    /// Display name for the "Target application" column (§7.2).
    pub app_name: String,
    pub src: String,
    pub dest: String,
    pub filename: String,
    pub local_path: String,
    pub status: SessionStatus,
    /// Whether the launched app blocks until closed (§6.2).
    pub blocking: bool,
    /// How the finished file is uploaded to `dest` (§6.1 step 8).
    #[serde(default)]
    pub upload: UploadSpec,
    pub created_at: String,
    pub updated_at: String,
    /// Last observed file modification time — persisted on every debounce-state
    /// change so a post-crash "resume" prompt has accurate context (§6.6).
    #[serde(default)]
    pub last_modified: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
}

/// Runtime handles for a session, not persisted. Dropping the debouncer stops
/// the watcher; aborting the tasks stops the process-wait loop.
pub struct SessionRuntime {
    pub session: Session,
    debouncer: Option<Debouncer<notify::RecommendedWatcher>>,
    tasks: Vec<tokio::task::JoinHandle<()>>,
}

impl SessionRuntime {
    fn bare(session: Session) -> Self {
        Self {
            session,
            debouncer: None,
            tasks: Vec::new(),
        }
    }

    /// Stop the watcher and background tasks for this session.
    fn shutdown(&mut self) {
        self.debouncer = None;
        for t in self.tasks.drain(..) {
            t.abort();
        }
    }
}

// --------------------------------------------------------------------------
// timestamps / paths
// --------------------------------------------------------------------------

fn now() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_default()
}

fn system_time_rfc3339(t: std::time::SystemTime) -> Option<String> {
    OffsetDateTime::from(t).format(&Rfc3339).ok()
}

fn session_dir(state: &AppState, id: &str) -> PathBuf {
    state.sessions_dir.join(id)
}

fn meta_path(dir: &Path) -> PathBuf {
    dir.join("session.json")
}

// --------------------------------------------------------------------------
// snapshot / emit
// --------------------------------------------------------------------------

/// Snapshot all sessions (cloned), newest first, for the UI and tray.
pub fn snapshot(state: &AppState) -> Vec<Session> {
    let map = state.sessions.lock().expect("sessions lock poisoned");
    let mut list: Vec<Session> = map.values().map(|r| r.session.clone()).collect();
    list.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    list
}

/// Emit the current session list to the frontend and refresh the tray (§7.2).
pub fn emit_update(app: &AppHandle, state: &AppState) {
    let list = snapshot(state);
    if let Err(e) = app.emit(EVENT_UPDATE, &list) {
        log::warn!("failed to emit {EVENT_UPDATE}: {e}");
    }
    crate::tray::refresh(app, &list);
}

/// Write `session.json` for a session (§6.1 step 4).
fn persist(state: &AppState, session: &Session) -> AppResult<()> {
    let dir = session_dir(state, &session.id);
    std::fs::create_dir_all(&dir)?;
    let text = serde_json::to_string_pretty(session)?;
    std::fs::write(meta_path(&dir), text)?;
    Ok(())
}

/// Apply a mutation to a session under lock, returning the updated clone.
/// The `updated_at` stamp is refreshed automatically. Never awaits under lock.
fn mutate<F>(state: &AppState, id: &str, f: F) -> Option<Session>
where
    F: FnOnce(&mut Session),
{
    let mut map = state.sessions.lock().expect("sessions lock poisoned");
    let rt = map.get_mut(id)?;
    f(&mut rt.session);
    rt.session.updated_at = now();
    Some(rt.session.clone())
}

/// Persist + emit after a mutation.
fn commit(app: &AppHandle, state: &AppState, session: &Session) {
    if let Err(e) = persist(state, session) {
        log::warn!("failed to persist session {}: {e}", session.id);
    }
    emit_update(app, state);
}

// --------------------------------------------------------------------------
// session start (§6.1)
// --------------------------------------------------------------------------

/// Kick off a new relay session for a `relay://` route (§2.2, §6.1).
/// Returns the new session id.
pub async fn start_session(
    app: AppHandle,
    state: Arc<AppState>,
    app_id: String,
    src: String,
    dest: String,
    filename: String,
    log_id: Option<String>,
) -> AppResult<String> {
    let cfg = config::load(&state.config_path)?;
    let def = config::get(&cfg, &app_id)?.clone();

    // §3 hard-deny sentinel.
    if !def.relay_allowed() {
        return Err(AppError::RelayNotAllowed(app_id));
    }

    let id = uuid::Uuid::new_v4().to_string();
    let dir = session_dir(&state, &id);
    std::fs::create_dir_all(&dir)?;
    let local_path = dir.join(&filename);

    // Snapshot the upload config from the relay block (defaults to a raw PUT).
    let upload = def
        .relay
        .as_ref()
        .map(|r| UploadSpec {
            method: r.method,
            field_name: r.file_field.clone(),
            form_fields: r.form_fields.clone(),
        })
        .unwrap_or_default();

    let session = Session {
        id: id.clone(),
        app_id: app_id.clone(),
        app_name: def.display(&app_id),
        src: src.clone(),
        dest,
        filename: filename.clone(),
        local_path: local_path.to_string_lossy().to_string(),
        status: SessionStatus::Downloading,
        blocking: def.relay_blocking(),
        upload,
        created_at: now(),
        updated_at: now(),
        last_modified: None,
        error: None,
    };

    // Row appears the moment the download starts (§7.2 row lifecycle).
    {
        let mut map = state.sessions.lock().expect("sessions lock poisoned");
        map.insert(id.clone(), SessionRuntime::bare(session.clone()));
    }
    commit(&app, &state, &session);

    // 1. GET src -> local file (§6.1 step 3).
    if let Err(e) = download(&state.http, &src, &local_path).await {
        let msg = e.to_string();
        if let Some(s) = mutate(&state, &id, |s| {
            s.status = SessionStatus::Error;
            s.error = Some(format!("download failed: {msg}"));
        }) {
            commit(&app, &state, &s);
        }
        return Err(e);
    }

    // 2. Spawn the editor and start watching (§6.1 steps 5-6).
    let last_modified = std::fs::metadata(&local_path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(system_time_rfc3339);

    let updated = mutate(&state, &id, |s| {
        s.status = SessionStatus::Editing;
        s.last_modified = last_modified;
    });
    if let Some(s) = &updated {
        commit(&app, &state, s);
    }

    spawn_editor_and_watch(&app, &state, &def, &id, &local_path, log_id.as_deref())?;

    Ok(id)
}

/// Download `src` into `dest_path` (§6.1 step 3, §6.5 plain GET, no auth).
async fn download(http: &reqwest::Client, src: &str, dest_path: &Path) -> AppResult<()> {
    let resp = http.get(src).send().await?.error_for_status()?;
    let bytes = resp.bytes().await?;
    tokio::fs::write(dest_path, &bytes).await?;
    Ok(())
}

/// Launch the app against the local file and attach a watcher (+ process-wait
/// task when the app is exit-blocking). Stores the runtime handles.
fn spawn_editor_and_watch(
    app: &AppHandle,
    state: &Arc<AppState>,
    def: &config::AppDefinition,
    id: &str,
    local_path: &Path,
    log_id: Option<&str>,
) -> AppResult<()> {
    let exec = def.relay_exec().to_string();
    let template = def.relay_args().to_vec();
    let blocking = def.relay_blocking();

    let file = local_path.to_string_lossy().to_string();
    let argv = substitute::build_argv(&template, Some(&file), &HashMap::new(), &[]);
    let command = if blocking {
        SpawnCommand::blocking(&exec, &argv)
    } else {
        SpawnCommand::new(&exec, &argv)
    };
    if let Some(log_id) = log_id {
        if let Err(e) = state.logs.lock().expect("logs lock poisoned").mark_cli(
            log_id,
            &command.program,
            &command.args,
        ) {
            log::warn!("failed to update launch log {log_id}: {e}");
        } else {
            crate::logs::emit_update(app, state);
        }
    }

    log::info!(
        "relay spawn: {} {:?} (blocking={blocking})",
        command.program,
        command.args
    );

    let mut tasks: Vec<tokio::task::JoinHandle<()>> = Vec::new();

    if blocking {
        // Keep the child so we can await its exit (§6.2 signal 1).
        let child = tokio::process::Command::new(&command.program)
            .args(&command.args)
            .spawn()
            .map_err(|e| AppError::Other(format!("failed to launch '{exec}': {e}")))?;

        let app2 = app.clone();
        let state2 = Arc::clone(state);
        let id2 = id.to_string();
        tasks.push(tokio::spawn(async move {
            let mut child = child;
            let _ = child.wait().await;
            log::info!("relay: blocking app for session {id2} exited -> auto-upload");
            // Auto-upload on process exit for exit-blocking apps (§6.2 default).
            complete_and_upload(app2, state2, id2).await;
        }));
    } else {
        // Fire-and-forget; completion relies on idle-debounce + manual (§6.2).
        std::process::Command::new(&command.program)
            .args(&command.args)
            .spawn()
            .map_err(|e| AppError::Other(format!("failed to launch '{exec}': {e}")))?;
    }

    // Filesystem idle-debounce watcher (§6.2 signal 2). We watch the session
    // directory and filter to the working file so our own `session.json` writes
    // don't feed back into the watcher.
    let debouncer = build_watcher(app, state, id, local_path)?;

    // Store runtime handles.
    let mut map = state.sessions.lock().expect("sessions lock poisoned");
    if let Some(rt) = map.get_mut(id) {
        rt.debouncer = Some(debouncer);
        rt.tasks = tasks;
    }
    Ok(())
}

/// Build a debounced filesystem watcher for a session's working file (§6.2).
fn build_watcher(
    app: &AppHandle,
    state: &Arc<AppState>,
    id: &str,
    local_path: &Path,
) -> AppResult<Debouncer<notify::RecommendedWatcher>> {
    let app = app.clone();
    let state = Arc::clone(state);
    let id = id.to_string();
    let watch_name = local_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let local_path_buf = local_path.to_path_buf();

    let mut debouncer = new_debouncer(
        Duration::from_secs(state.idle_secs.max(1)),
        move |res: DebounceEventResult| {
            let events = match res {
                Ok(ev) => ev,
                Err(errs) => {
                    log::warn!("watcher error(s): {errs:?}");
                    return;
                }
            };

            // Only react to events touching the working file, not session.json.
            let touched = events.iter().any(|e| {
                e.path
                    .file_name()
                    .map(|n| n == watch_name.as_str())
                    .unwrap_or(false)
            });
            if !touched {
                return;
            }

            let last_modified = std::fs::metadata(&local_path_buf)
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(system_time_rfc3339);

            // The debouncer fired => writes occurred then settled for the idle
            // window. Surface "idle, ready to upload" for non-blocking apps, but
            // do NOT auto-fire the upload (§6.2). Never override a terminal state.
            let updated = mutate(&state, &id, |s| {
                s.last_modified = last_modified;
                if !s.blocking && matches!(s.status, SessionStatus::Editing | SessionStatus::Idle) {
                    s.status = SessionStatus::Idle;
                }
            });

            if let Some(s) = updated {
                commit(&app, &state, &s);
            }
        },
    )
    .map_err(|e| AppError::Other(format!("failed to start file watcher: {e}")))?;

    // Watch the session directory (non-recursive) to survive editors that
    // replace the file via temp+rename.
    let dir = local_path.parent().unwrap_or(local_path);
    debouncer
        .watcher()
        .watch(dir, RecursiveMode::NonRecursive)
        .map_err(|e| AppError::Other(format!("failed to watch {}: {e}", dir.display())))?;

    Ok(debouncer)
}

// --------------------------------------------------------------------------
// completion / upload (§6.1 steps 8-10)
// --------------------------------------------------------------------------

/// Upload the working file to `dest` and finalize the session. Used by the
/// process-exit trigger and the manual "Upload & Finish" action (§6.2).
pub async fn complete_and_upload(app: AppHandle, state: Arc<AppState>, id: String) {
    // Snapshot the session; bail if it's gone or already finished/uploading.
    let session = {
        let map = state.sessions.lock().expect("sessions lock poisoned");
        match map.get(&id) {
            Some(rt) => rt.session.clone(),
            None => return,
        }
    };
    if matches!(
        session.status,
        SessionStatus::Uploading | SessionStatus::Done
    ) {
        return;
    }

    if let Some(s) = mutate(&state, &id, |s| {
        s.status = SessionStatus::Uploading;
        s.error = None;
    }) {
        commit(&app, &state, &s);
    }

    // Upload with backoff retries (§6.1 step 10).
    let mut attempt = 0;
    loop {
        attempt += 1;
        match upload(
            &state.http,
            &session.dest,
            Path::new(&session.local_path),
            &session.upload,
        )
        .await
        {
            Ok(()) => {
                finish_success(&app, &state, &id);
                return;
            }
            Err(e) => {
                if attempt >= UPLOAD_MAX_ATTEMPTS {
                    let msg = e.to_string();
                    if let Some(s) = mutate(&state, &id, |s| {
                        s.status = SessionStatus::Error;
                        s.error = Some(format!("upload failed: {msg}"));
                    }) {
                        // Keep session dir + metadata intact (§6.1 step 10).
                        commit(&app, &state, &s);
                    }
                    return;
                }
                let backoff = UPLOAD_BASE_BACKOFF_MS * 2u64.pow(attempt - 1);
                log::warn!("upload attempt {attempt} failed ({e}); retrying in {backoff}ms");
                tokio::time::sleep(Duration::from_millis(backoff)).await;
            }
        }
    }
}

/// Upload the file contents to the presigned `dest` per the session's upload
/// spec (§6.1 step 8, §6.5). No additional auth — `dest` is presigned.
async fn upload(
    http: &reqwest::Client,
    dest: &str,
    path: &Path,
    spec: &UploadSpec,
) -> AppResult<()> {
    let bytes = tokio::fs::read(path).await?;

    let request = match spec.method {
        config::UploadMethod::Put => http.put(dest).body(bytes),
        config::UploadMethod::Post => http.post(dest).body(bytes),
        config::UploadMethod::Multipart => {
            let filename = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "file".into());
            let field = spec
                .field_name
                .clone()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "file".into());

            let part = reqwest::multipart::Part::bytes(bytes).file_name(filename);
            let mut form = reqwest::multipart::Form::new();
            // Extra fields first; the file part goes last, which some endpoints
            // (e.g. an S3 presigned POST policy) require.
            for f in &spec.form_fields {
                form = form.text(f.name.clone(), f.value.clone());
            }
            form = form.part(field, part);
            http.post(dest).multipart(form)
        }
    };

    request.send().await?.error_for_status()?;
    Ok(())
}

/// Mark done, remove from active list, delete the session directory (§6.1 step 9).
fn finish_success(app: &AppHandle, state: &AppState, id: &str) {
    // Remove under lock, then shut the runtime down *after* releasing the lock.
    // `shutdown` drops the debouncer, which joins the watcher thread — and that
    // thread may be blocked trying to acquire this same lock. Joining it while
    // holding the lock would deadlock.
    let removed = {
        let mut map = state.sessions.lock().expect("sessions lock poisoned");
        map.remove(id)
    };
    if let Some(mut rt) = removed {
        rt.shutdown();
    }

    let dir = session_dir(state, id);
    if let Err(e) = std::fs::remove_dir_all(&dir) {
        if e.kind() != std::io::ErrorKind::NotFound {
            log::warn!("failed to remove session dir {}: {e}", dir.display());
        }
    }
    log::info!("relay session {id} done");
    emit_update(app, state);
}

// --------------------------------------------------------------------------
// user / tray actions (§7.2)
// --------------------------------------------------------------------------

/// Manual "Upload & Finish" (§6.2 signal 3).
pub async fn upload_finish(app: AppHandle, state: Arc<AppState>, id: String) -> AppResult<()> {
    ensure_exists(&state, &id)?;
    complete_and_upload(app, state, id).await;
    Ok(())
}

/// "Retry" from the Error state (§7.2).
pub async fn retry(app: AppHandle, state: Arc<AppState>, id: String) -> AppResult<()> {
    ensure_exists(&state, &id)?;
    complete_and_upload(app, state, id).await;
    Ok(())
}

/// "Keep Editing" — dismiss the idle state, keep watching (§7.2).
pub fn keep_editing(app: &AppHandle, state: &AppState, id: &str) -> AppResult<()> {
    ensure_exists(state, id)?;
    if let Some(s) = mutate(state, id, |s| {
        if s.status == SessionStatus::Idle {
            s.status = SessionStatus::Editing;
        }
    }) {
        commit(app, state, &s);
    }
    Ok(())
}

/// "Cancel" / "Discard" — stop everything and delete the session dir (§7.2, §6.6).
pub fn discard(app: &AppHandle, state: &AppState, id: &str) -> AppResult<()> {
    // Remove under lock; shut down (joins watcher thread) after releasing it to
    // avoid deadlocking against a watcher callback waiting on the same lock.
    let removed = {
        let mut map = state.sessions.lock().expect("sessions lock poisoned");
        map.remove(id)
    };
    let mut rt = removed.ok_or_else(|| AppError::UnknownSession(id.to_string()))?;
    rt.shutdown();

    let dir = session_dir(state, id);
    if let Err(e) = std::fs::remove_dir_all(&dir) {
        if e.kind() != std::io::ErrorKind::NotFound {
            log::warn!("failed to remove session dir {}: {e}", dir.display());
        }
    }
    emit_update(app, state);
    Ok(())
}

fn ensure_exists(state: &AppState, id: &str) -> AppResult<()> {
    let map = state.sessions.lock().expect("sessions lock poisoned");
    if map.contains_key(id) {
        Ok(())
    } else {
        Err(AppError::UnknownSession(id.to_string()))
    }
}

// --------------------------------------------------------------------------
// crash recovery (§6.6)
// --------------------------------------------------------------------------

/// Scan the sessions directory on startup for orphaned sessions (metadata but
/// no live process, since we just started). Load them as `Orphaned` for triage
/// in the Relay Queue (§6.6, §7.2). No auto-upload/auto-discard.
pub fn recover_orphans(app: &AppHandle, state: &AppState) {
    let read = match std::fs::read_dir(&state.sessions_dir) {
        Ok(r) => r,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
        Err(e) => {
            log::warn!("failed to scan sessions dir: {e}");
            return;
        }
    };

    let mut found = 0;
    for entry in read.flatten() {
        if !entry.path().is_dir() {
            continue;
        }
        let meta = meta_path(&entry.path());
        let text = match std::fs::read_to_string(&meta) {
            Ok(t) => t,
            Err(_) => continue,
        };
        let mut session: Session = match serde_json::from_str(&text) {
            Ok(s) => s,
            Err(e) => {
                log::warn!(
                    "skipping unreadable session {}: {e}",
                    entry.path().display()
                );
                continue;
            }
        };

        // A session that had already finished uploading needn't be resurrected.
        if session.status == SessionStatus::Done {
            let _ = std::fs::remove_dir_all(entry.path());
            continue;
        }

        session.status = SessionStatus::Orphaned;
        session.updated_at = now();
        let _ = persist(state, &session);

        let mut map = state.sessions.lock().expect("sessions lock poisoned");
        map.insert(session.id.clone(), SessionRuntime::bare(session));
        found += 1;
    }

    if found > 0 {
        log::info!("recovered {found} orphaned relay session(s)");
        emit_update(app, state);
    }
}

/// "Resume" an orphaned session: relaunch the app and resume watching (§6.6).
pub fn resume(app: &AppHandle, state: &Arc<AppState>, id: &str) -> AppResult<()> {
    let session = {
        let map = state.sessions.lock().expect("sessions lock poisoned");
        map.get(id)
            .map(|r| r.session.clone())
            .ok_or_else(|| AppError::UnknownSession(id.to_string()))?
    };

    let cfg = config::load(&state.config_path)?;
    let def = config::get(&cfg, &session.app_id)?.clone();
    let local_path = PathBuf::from(&session.local_path);

    if let Some(s) = mutate(state, id, |s| {
        s.status = SessionStatus::Editing;
        s.error = None;
    }) {
        commit(app, state, &s);
    }

    spawn_editor_and_watch(app, state, &def, id, &local_path, None)
}
