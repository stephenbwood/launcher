//! Central error type. Commands return `Result<T, String>` to the frontend, so
//! `AppError` implements `Into<String>` via `Serialize`/`Display`.

use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("invalid launcher:// URL: {0}")]
    InvalidUrl(String),

    #[error("unknown app-id: {0}")]
    UnknownApp(String),

    #[error("relay session not found: {0}")]
    UnknownSession(String),

    #[error("app '{0}' is not allowed to be used for relay (relay: false)")]
    RelayNotAllowed(String),

    #[error("config error: {0}")]
    Config(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("{0}")]
    Other(String),
}

pub type AppResult<T> = Result<T, AppError>;

// Tauri commands need the error to be Serialize so it crosses the IPC boundary.
impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}
