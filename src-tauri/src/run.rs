//! `run` route handling (§2.1): resolve the app definition, build the argv from
//! the base template, and spawn the process (fire-and-forget).

use std::collections::HashMap;

use crate::config;
use crate::error::{AppError, AppResult};
use crate::substitute;

/// Launch an app for a `run` route.
///
/// `file` is taken from the `file` named param (if present); the remaining
/// named params and positional `arg=` values feed the template (§3).
pub fn launch(
    config_path: &std::path::Path,
    app_id: &str,
    named: &HashMap<String, String>,
    positional: &[String],
) -> AppResult<()> {
    let cfg = config::load(config_path)?;
    let def = config::get(&cfg, app_id)?;

    // `run://` always uses the base exec/args (§3).
    let file = named.get("file").map(|s| s.as_str());
    let argv = substitute::build_argv(&def.args, file, named, positional);

    log::info!("run: {} {:?}", def.exec, argv);

    std::process::Command::new(&def.exec)
        .args(&argv)
        .spawn()
        .map_err(|e| AppError::Other(format!("failed to launch '{}': {e}", def.exec)))?;

    Ok(())
}
