//! Shared application state, managed by Tauri as `Arc<AppState>` so background
//! tasks (downloads, watchers, process waiters) and commands share it.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::error::AppResult;
use crate::logs::LogStore;
use crate::relay::SessionRuntime;
use crate::window_state::MainWindowStateStore;

/// Default filesystem idle-debounce window (§6.2). Configurable later (§8).
pub const DEFAULT_IDLE_SECS: u64 = 30;

/// Duplicate protocol deliveries inside this window are treated as one inbound request.
pub const INBOUND_URI_DEBOUNCE: Duration = Duration::from_secs(2);

pub struct AppState {
    /// Live relay sessions keyed by session id.
    ///
    /// A `std::sync::Mutex` (not tokio) is used deliberately: the `notify`
    /// watcher callback runs on a synchronous thread and must touch this map.
    /// Invariant: never hold this lock across an `.await`.
    pub sessions: Mutex<HashMap<String, SessionRuntime>>,

    /// Logged URI handling records shown in the Logs tab.
    pub logs: Mutex<LogStore>,

    /// Persisted visibility state for the main window.
    pub main_window_state: Mutex<MainWindowStateStore>,

    /// Recently accepted inbound protocol URIs used to suppress duplicate OS/plugin deliveries.
    recent_inbound_uris: Mutex<HashMap<String, Instant>>,

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
    pub fn new(
        sessions_dir: PathBuf,
        config_path: PathBuf,
        logs_path: PathBuf,
        main_window_state_path: PathBuf,
    ) -> AppResult<Self> {
        Ok(Self {
            sessions: Mutex::new(HashMap::new()),
            logs: Mutex::new(LogStore::load(logs_path)?),
            main_window_state: Mutex::new(MainWindowStateStore::load(main_window_state_path)?),
            recent_inbound_uris: Mutex::new(HashMap::new()),
            sessions_dir,
            config_path,
            http: reqwest::Client::new(),
            idle_secs: DEFAULT_IDLE_SECS,
        })
    }

    pub fn should_handle_inbound_uri(&self, raw: &str) -> bool {
        self.should_handle_inbound_uri_at(raw, Instant::now())
    }

    fn should_handle_inbound_uri_at(&self, raw: &str, now: Instant) -> bool {
        let raw = raw.trim();
        let mut recent = self
            .recent_inbound_uris
            .lock()
            .expect("inbound URI debounce lock poisoned");

        recent.retain(|_, seen_at| match now.checked_duration_since(*seen_at) {
            Some(age) => age < INBOUND_URI_DEBOUNCE,
            None => true,
        });

        if recent.contains_key(raw) {
            return false;
        }

        recent.insert(raw.to_string(), now);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{Duration, Instant};

    fn temp_path(name: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!("launcher-state-test-{name}-{}", std::process::id()));
        let _ = fs::remove_file(&path);
        path
    }

    #[test]
    fn suppresses_immediate_duplicate_inbound_uri() {
        let state = AppState::new(
            temp_path("sessions"),
            temp_path("apps.json"),
            temp_path("logs.json"),
            temp_path("main-window-state.json"),
        )
        .unwrap();
        let now = Instant::now();
        let raw = "launcher://run/clp?src=https%3A%2F%2Fexample.test%2Fhandoff.clphop";

        assert!(state.should_handle_inbound_uri_at(raw, now));
        assert!(!state.should_handle_inbound_uri_at(raw, now + Duration::from_millis(670)));
        assert!(state.should_handle_inbound_uri_at(raw, now + Duration::from_secs(3)));
    }

    #[test]
    fn allows_different_inbound_uri_inside_debounce_window() {
        let state = AppState::new(
            temp_path("sessions"),
            temp_path("apps.json"),
            temp_path("logs.json"),
            temp_path("main-window-state.json"),
        )
        .unwrap();
        let now = Instant::now();

        assert!(state.should_handle_inbound_uri_at("launcher://run/a", now));
        assert!(state
            .should_handle_inbound_uri_at("launcher://run/b", now + Duration::from_millis(100)));
    }
}

