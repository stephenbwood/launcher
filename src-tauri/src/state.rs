//! Shared application state, managed by Tauri as `Arc<AppState>` so background
//! tasks (downloads, watchers, process waiters) and commands share it.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use crate::relay::SessionRuntime;

/// Default filesystem idle-debounce window (§6.2). Configurable later (§8).
pub const DEFAULT_IDLE_SECS: u64 = 30;

pub struct AppState {
    /// Live relay sessions keyed by session id.
    ///
    /// A `std::sync::Mutex` (not tokio) is used deliberately: the `notify`
    /// watcher callback runs on a synchronous thread and must touch this map.
    /// Invariant: never hold this lock across an `.await`.
    pub sessions: Mutex<HashMap<String, SessionRuntime>>,

    /// `<app-data>/relay-sessions/` (§6.1).
    pub sessions_dir: PathBuf,

    /// `<app-config>/apps.json` (§3).
    pub config_path: PathBuf,

    /// Shared HTTP client for src/dest transfers (§6.1).
    pub http: reqwest::Client,

    /// Idle-debounce window in seconds (§6.2).
    pub idle_secs: u64,
}

impl AppState {
    pub fn new(sessions_dir: PathBuf, config_path: PathBuf) -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
            sessions_dir,
            config_path,
            http: reqwest::Client::new(),
            idle_secs: DEFAULT_IDLE_SECS,
        }
    }
}
