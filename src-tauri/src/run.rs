//! `run` route handling (§2.1): resolve the app definition, build the argv from
//! the base template, and spawn the process (fire-and-forget).

use std::collections::HashMap;

use crate::config;
use crate::error::{AppError, AppResult};
use crate::process::SpawnCommand;
use crate::state::AppState;
use crate::substitute;

/// Launch an app for a `run` route.
///
/// `file` is taken from the `file` named param (if present); the remaining
/// named params and positional `arg=` values feed the template (§3).
pub fn launch(
    state: &AppState,
    app_id: &str,
    named: &HashMap<String, String>,
    positional: &[String],
    log_id: Option<&str>,
) -> AppResult<()> {
    let cfg = config::load(&state.config_path)?;
    let def = config::get(&cfg, app_id)?;

    // `run://` always uses the base exec/args (§3).
    let file = named.get("file").map(|s| s.as_str());
    let argv = substitute::build_argv(&def.args, file, named, positional);
    let command = SpawnCommand::new(&def.exec, &argv);

    if let Some(log_id) = log_id {
        if let Err(e) = state.logs.lock().expect("logs lock poisoned").mark_cli(
            log_id,
            &command.program,
            &command.args,
        ) {
            log::warn!("failed to update launch log {log_id}: {e}");
        }
    }

    log::info!("run: {} {:?}", command.program, command.args);

    std::process::Command::new(&command.program)
        .args(&command.args)
        .spawn()
        .map_err(|e| AppError::Other(format!("failed to launch '{}': {e}", def.exec)))?;

    Ok(())
}
