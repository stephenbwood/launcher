//! App definitions config (§3). Maps `app-id` -> executable + argument template,
//! with an optional `relay` override block. Persisted as JSON under the app's
//! config directory (`apps.json`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

/// How the finished relay file is sent to `dest` on completion (§6.1 step 8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UploadMethod {
    /// PUT with the raw file bytes as the body (the historical default).
    #[default]
    Put,
    /// POST with the raw file bytes as the body.
    Post,
    /// POST as `multipart/form-data`, with the file under `file_field` (plus any
    /// extra `form_fields`).
    Multipart,
}

impl UploadMethod {
    /// Serde helper so a default (PUT) method is omitted from the on-disk config.
    pub fn is_put(&self) -> bool {
        matches!(self, UploadMethod::Put)
    }
}

/// A static multipart form field (name → value), used alongside the file part
/// for `multipart/form-data` uploads (e.g. an S3 presigned POST's policy fields).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FormField {
    pub name: String,
    pub value: String,
}

/// Optional relay-specific override for an app definition (§3).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RelayOverride {
    /// Override executable; falls back to the base `exec` when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exec: Option<String>,

    /// Override argument template; falls back to the base `args` when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<Vec<String>>,

    /// Whether the launched app blocks until closed. Drives the completion
    /// signal choice in §6.2. Absent/false => rely on idle-debounce + manual.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocking: Option<bool>,

    /// HTTP method/encoding used to upload to `dest` (§6.1 step 8).
    #[serde(default, skip_serializing_if = "UploadMethod::is_put")]
    pub method: UploadMethod,

    /// Multipart file-part field name (required when `method` is `multipart`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_field: Option<String>,

    /// Extra multipart form fields sent before the file part.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub form_fields: Vec<FormField>,
}

/// A single application definition (§3).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppDefinition {
    pub exec: String,

    #[serde(default)]
    pub args: Vec<String>,

    /// Optional friendly name shown in the Relay Queue's "Target application"
    /// column; falls back to the app-id itself (§7.2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relay: Option<RelayOverride>,

    /// Optional hard-deny sentinel (§3): when `false`, relay requests against
    /// this app-id are rejected outright.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relay_allowed: Option<bool>,
}

impl AppDefinition {
    /// Resolve the executable to use for a relay session (relay override wins).
    pub fn relay_exec(&self) -> &str {
        self.relay
            .as_ref()
            .and_then(|r| r.exec.as_deref())
            .unwrap_or(&self.exec)
    }

    /// Resolve the argument template for a relay session (relay override wins).
    pub fn relay_args(&self) -> &[String] {
        self.relay
            .as_ref()
            .and_then(|r| r.args.as_deref())
            .unwrap_or(&self.args)
    }

    /// Whether a relay session using this app blocks until the app closes (§6.2).
    pub fn relay_blocking(&self) -> bool {
        self.relay
            .as_ref()
            .and_then(|r| r.blocking)
            .unwrap_or(false)
    }

    /// Whether relay is permitted against this app-id (§3 `relay: false` sentinel).
    pub fn relay_allowed(&self) -> bool {
        self.relay_allowed.unwrap_or(true)
    }

    /// Display name for UI, falling back to a caller-supplied id.
    pub fn display(&self, app_id: &str) -> String {
        self.display_name
            .clone()
            .unwrap_or_else(|| app_id.to_string())
    }
}

/// The whole config: app-id -> definition. `BTreeMap` keeps a stable order in
/// the on-disk JSON and in the Settings list.
pub type AppConfig = BTreeMap<String, AppDefinition>;

/// Load the config from disk. A missing file yields an empty config (fresh install).
pub fn load(path: &Path) -> AppResult<AppConfig> {
    match std::fs::read_to_string(path) {
        Ok(text) => {
            if text.trim().is_empty() {
                return Ok(AppConfig::new());
            }
            serde_json::from_str(&text).map_err(|e| AppError::Config(e.to_string()))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(AppConfig::new()),
        Err(e) => Err(AppError::Io(e)),
    }
}

/// Persist the config to disk, creating parent directories as needed.
pub fn save(path: &Path, config: &AppConfig) -> AppResult<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(config)?;
    std::fs::write(path, text)?;
    Ok(())
}

/// Look up a single app definition, erroring if the id is unknown.
pub fn get<'a>(config: &'a AppConfig, app_id: &str) -> AppResult<&'a AppDefinition> {
    config
        .get(app_id)
        .ok_or_else(|| AppError::UnknownApp(app_id.to_string()))
}

/// Standard on-disk config path given the app config directory.
pub fn config_path(app_config_dir: &Path) -> PathBuf {
    app_config_dir.join("apps.json")
}
