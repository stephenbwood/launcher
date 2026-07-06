use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::AppResult;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MainWindowState {
    Visible,
    MinimizedToTray,
}

#[derive(Debug)]
pub struct MainWindowStateStore {
    path: PathBuf,
    state: MainWindowState,
}

impl MainWindowStateStore {
    pub fn load(path: PathBuf) -> AppResult<Self> {
        let state = match std::fs::read_to_string(&path) {
            Ok(text) if text.trim().is_empty() => MainWindowState::Visible,
            Ok(text) => serde_json::from_str(&text)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => MainWindowState::Visible,
            Err(e) => return Err(e.into()),
        };
        Ok(Self { path, state })
    }

    pub fn state(&self) -> MainWindowState {
        self.state
    }

    pub fn set(&mut self, state: MainWindowState) -> AppResult<()> {
        self.state = state;
        self.persist()
    }

    fn persist(&self) -> AppResult<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(&self.state)?;
        std::fs::write(&self.path, text)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_path(name: &str) -> std::path::PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "launcher-window-state-test-{name}-{}.json",
            std::process::id()
        ));
        let _ = fs::remove_file(&path);
        path
    }

    #[test]
    fn defaults_to_visible_when_state_file_is_missing() {
        let path = temp_path("missing");

        let store = MainWindowStateStore::load(path.clone()).unwrap();

        assert_eq!(store.state(), MainWindowState::Visible);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn persists_minimized_to_tray_state() {
        let path = temp_path("persist");
        let mut store = MainWindowStateStore::load(path.clone()).unwrap();

        store.set(MainWindowState::MinimizedToTray).unwrap();

        let reloaded = MainWindowStateStore::load(path.clone()).unwrap();
        assert_eq!(reloaded.state(), MainWindowState::MinimizedToTray);
        let _ = fs::remove_file(path);
    }
}
