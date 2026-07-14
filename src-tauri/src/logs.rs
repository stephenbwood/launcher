use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use crate::error::AppResult;
use crate::state::AppState;

/// Frontend event emitted whenever the log list changes.
pub const EVENT_UPDATE: &str = "logs:update";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogStatus {
    Handled,
    Launched,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub id: String,
    pub created_at: String,
    pub raw_uri: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_id: Option<String>,
    pub status: LogStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cli_call: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exec: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub argv: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug)]
pub struct LogStore {
    path: PathBuf,
    entries: Vec<LogEntry>,
}

impl LogStore {
    pub fn load(path: PathBuf) -> AppResult<Self> {
        let entries = match std::fs::read_to_string(&path) {
            Ok(text) if text.trim().is_empty() => Vec::new(),
            Ok(text) => serde_json::from_str(&text)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(e.into()),
        };
        Ok(Self { path, entries })
    }

    pub fn append_handled_uri(
        &mut self,
        raw_uri: &str,
        route_type: &str,
        app_id: Option<&str>,
    ) -> AppResult<String> {
        let id = uuid::Uuid::new_v4().to_string();
        self.entries.push(LogEntry {
            id: id.clone(),
            created_at: now(),
            raw_uri: raw_uri.to_string(),
            route_type: Some(route_type.to_string()),
            app_id: app_id.map(|s| s.to_string()),
            status: LogStatus::Handled,
            cli_call: None,
            exec: None,
            argv: None,
            error: None,
        });
        self.persist()?;
        Ok(id)
    }

    pub fn append_error(
        &mut self,
        raw_uri: &str,
        route_type: Option<&str>,
        app_id: Option<&str>,
        error: &str,
    ) -> AppResult<String> {
        let id = uuid::Uuid::new_v4().to_string();
        self.entries.push(LogEntry {
            id: id.clone(),
            created_at: now(),
            raw_uri: raw_uri.to_string(),
            route_type: route_type.map(|s| s.to_string()),
            app_id: app_id.map(|s| s.to_string()),
            status: LogStatus::Error,
            cli_call: None,
            exec: None,
            argv: None,
            error: Some(error.to_string()),
        });
        self.persist()?;
        Ok(id)
    }

    pub fn mark_cli(&mut self, id: &str, exec: &str, argv: &[String]) -> AppResult<()> {
        if let Some(entry) = self.entries.iter_mut().find(|entry| entry.id == id) {
            entry.status = LogStatus::Launched;
            entry.cli_call = Some(format_cli_call(exec, argv));
            entry.exec = Some(exec.to_string());
            entry.argv = Some(argv.to_vec());
            entry.error = None;
            self.persist()?;
        }
        Ok(())
    }

    pub fn mark_error(&mut self, id: &str, error: &str) -> AppResult<()> {
        if let Some(entry) = self.entries.iter_mut().find(|entry| entry.id == id) {
            entry.status = LogStatus::Error;
            entry.error = Some(error.to_string());
            self.persist()?;
        }
        Ok(())
    }

    pub fn list_newest_first(&self) -> Vec<LogEntry> {
        let mut entries = self.entries.clone();
        entries.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        entries
    }

    pub fn get(&self, id: &str) -> Option<LogEntry> {
        self.entries.iter().find(|entry| entry.id == id).cloned()
    }

    pub fn clear(&mut self) -> AppResult<()> {
        self.entries.clear();
        self.persist()
    }

    fn persist(&self) -> AppResult<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(&self.entries)?;
        std::fs::write(&self.path, text)?;
        Ok(())
    }
}

pub fn format_cli_call(exec: &str, argv: &[String]) -> String {
    std::iter::once(exec)
        .chain(argv.iter().map(String::as_str))
        .map(quote_arg)
        .collect::<Vec<_>>()
        .join(" ")
}

fn quote_arg(value: &str) -> String {
    if value.is_empty() || value.chars().any(char::is_whitespace) || value.contains('"') {
        format!("\"{}\"", value.replace('"', "\\\""))
    } else {
        value.to_string()
    }
}

fn now() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_default()
}

/// Snapshot all log entries, newest first, for the UI.
pub fn snapshot(state: &AppState) -> Vec<LogEntry> {
    state
        .logs
        .lock()
        .expect("logs lock poisoned")
        .list_newest_first()
}

/// Emit the current log list to the frontend.
pub fn emit_update(app: &AppHandle, state: &AppState) {
    let list = snapshot(state);
    if let Err(e) = app.emit(EVENT_UPDATE, &list) {
        log::warn!("failed to emit {EVENT_UPDATE}: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_path(name: &str) -> std::path::PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "launcher-logs-test-{name}-{}.json",
            std::process::id()
        ));
        let _ = fs::remove_file(&path);
        path
    }

    #[test]
    fn formats_cli_call_with_escaped_arguments() {
        let cli = format_cli_call(
            "C:\\Program Files\\App\\app.exe",
            &["plain".into(), "two words".into()],
        );

        assert_eq!(
            cli,
            "\"C:\\Program Files\\App\\app.exe\" plain \"two words\""
        );
    }

    #[test]
    fn appends_and_updates_log_entries_on_disk() {
        let path = temp_path("append-update");
        let mut store = LogStore::load(path.clone()).unwrap();

        let id = store
            .append_handled_uri(
                "launcher://run/editor?file=C%3A%5Ctmp%5Cfile.txt",
                "run",
                Some("editor"),
            )
            .unwrap();
        store
            .mark_cli(&id, "editor.exe", &["C:\\tmp\\file.txt".into()])
            .unwrap();

        let reloaded = LogStore::load(path.clone()).unwrap();
        let entries = reloaded.list_newest_first();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, id);
        assert_eq!(
            entries[0].raw_uri,
            "launcher://run/editor?file=C%3A%5Ctmp%5Cfile.txt"
        );
        assert_eq!(entries[0].route_type.as_deref(), Some("run"));
        assert_eq!(entries[0].app_id.as_deref(), Some("editor"));
        assert_eq!(entries[0].status, LogStatus::Launched);
        assert_eq!(
            entries[0].cli_call.as_deref(),
            Some("editor.exe C:\\tmp\\file.txt")
        );
        assert_eq!(entries[0].exec.as_deref(), Some("editor.exe"));
        assert_eq!(entries[0].argv, Some(vec!["C:\\tmp\\file.txt".into()]));

        let _ = fs::remove_file(path);
    }

    #[test]
    fn clears_log_entries_on_disk() {
        let path = temp_path("clear");
        let mut store = LogStore::load(path.clone()).unwrap();
        store
            .append_handled_uri("launcher://run/editor", "run", Some("editor"))
            .unwrap();

        store.clear().unwrap();

        let reloaded = LogStore::load(path.clone()).unwrap();
        assert!(reloaded.list_newest_first().is_empty());

        let _ = fs::remove_file(path);
    }
}
