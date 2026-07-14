//! `run` route handling (§2.1): resolve the app definition, build the argv from
//! the base template, and spawn the process (fire-and-forget).

use std::collections::HashMap;

use tauri::AppHandle;

use crate::config;
use crate::error::AppResult;
use crate::process::SpawnCommand;
use crate::state::AppState;
use crate::substitute;

/// Launch an app for a `run` route.
///
/// Resolves the command, records the CLI in the log, then spawns on a detached
/// thread so Launch Services / target-app startup cannot stall URL dispatch or
/// leave a log row stuck at `Handled` with no CLI.
pub fn launch(
    app: &AppHandle,
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
        // Emit after persist so the UI can show the CLI even if spawn stalls.
        crate::logs::emit_update(app, state);
    }

    log::info!("run: {} {:?}", command.program, command.args);

    let program = command.program.clone();
    let args = command.args.clone();
    let exec = def.exec.clone();
    // Detach spawn from the dispatch thread. macOS Launch Services (and some
    // GUI targets) can block the caller; that must not freeze deep-link handling.
    std::thread::spawn(move || {
        if let Err(e) = std::process::Command::new(&program).args(&args).spawn() {
            log::warn!("failed to launch '{exec}': {e}");
        }
    });

    Ok(())
}
