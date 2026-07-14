//! Cross-platform process launch helpers.

use std::path::{Path, PathBuf};

/// Resolved command to spawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpawnCommand {
    pub program: String,
    pub args: Vec<String>,
}

impl SpawnCommand {
    pub fn new(exec: &str, argv: &[String]) -> Self {
        Self::for_launch(exec, argv, false)
    }

    pub fn blocking(exec: &str, argv: &[String]) -> Self {
        Self::for_launch(exec, argv, true)
    }

    fn for_launch(exec: &str, argv: &[String], wait: bool) -> Self {
        let argv = normalize_argv(argv);

        #[cfg(target_os = "macos")]
        {
            if let Some(app_bundle) = exact_app_bundle_path(exec) {
                // Launch via `open` so we go through Launch Services instead of
                // exec'ing the bundle binary from this process. Direct
                // Contents/MacOS launches can hang the deep-link / Apple Event
                // thread (UI stuck on Handled, no CLI update) — especially under
                // Accessory activation policy.
                //
                // `-n` forces a new process so `--args` is honored when the app
                // is already running (required for single-instance forwarders
                // like ColorLabPro).
                let mut args = Vec::new();
                args.push("-n".to_string());
                if wait {
                    args.push("-W".to_string());
                }
                args.push("-a".to_string());
                args.push(app_bundle.to_string_lossy().to_string());
                if !argv.is_empty() {
                    args.push("--args".to_string());
                    args.extend(argv);
                }
                return Self {
                    program: "/usr/bin/open".to_string(),
                    args,
                };
            }
        }

        Self {
            program: exec.to_string(),
            args: argv,
        }
    }
}

/// Resolve a launch command, rewriting macOS bundle binaries back through `open`.
///
/// Log re-runs may still store a prior `Contents/MacOS/...` path; sending that
/// through Launch Services avoids the same main-thread hang as a live `.app` launch.
pub fn resolve_spawn(exec: &str, argv: &[String]) -> SpawnCommand {
    #[cfg(target_os = "macos")]
    {
        if let Some(app_bundle) = app_bundle_from_macos_executable(exec) {
            return SpawnCommand::new(&app_bundle.to_string_lossy(), argv);
        }
    }
    SpawnCommand::new(exec, argv)
}

#[cfg(target_os = "macos")]
fn app_bundle_from_macos_executable(exec: &str) -> Option<PathBuf> {
    let path = Path::new(exec);
    let macos_dir = path.parent()?;
    if macos_dir.file_name()?.to_str()? != "MacOS" {
        return None;
    }
    let contents = macos_dir.parent()?;
    if contents.file_name()?.to_str()? != "Contents" {
        return None;
    }
    let app_bundle = contents.parent()?;
    is_app_bundle(app_bundle).then(|| app_bundle.to_path_buf())
}

/// Non-fatal existence check for user-configured executables.
pub fn executable_exists(path: &str) -> bool {
    if path.trim().is_empty() {
        return false;
    }
    // Absolute/relative paths that resolve, including macOS `.app` bundles.
    if Path::new(path).exists() {
        return true;
    }
    which_on_path(path).is_some()
}

fn which_on_path(cmd: &str) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    let exts: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".EXE;.BAT;.CMD".into())
            .split(';')
            .map(|s| s.to_string())
            .collect()
    } else {
        vec![String::new()]
    };
    for dir in std::env::split_paths(&paths) {
        for ext in &exts {
            let candidate = dir.join(format!("{cmd}{ext}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

fn normalize_argv(argv: &[String]) -> Vec<String> {
    argv.iter().map(|arg| normalize_cli_arg(arg)).collect()
}

fn normalize_cli_arg(arg: &str) -> String {
    let Some(first) = arg.chars().next() else {
        return String::new();
    };
    let replacement = match first {
        // Common smart punctuation substitutions for double-hyphen CLI options.
        '\u{2013}' | '\u{2014}' | '\u{2015}' => "--",
        // Common single-hyphen variants.
        '\u{2010}' | '\u{2011}' | '\u{2012}' | '\u{2212}' => "-",
        _ => return arg.to_string(),
    };
    let rest = &arg[first.len_utf8()..];
    format!("{replacement}{rest}")
}

#[cfg(target_os = "macos")]
fn exact_app_bundle_path(exec: &str) -> Option<PathBuf> {
    let path = Path::new(exec);
    if is_app_bundle(path) {
        return Some(path.to_path_buf());
    }
    None
}

#[cfg(target_os = "macos")]
fn is_app_bundle(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("app"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_executable_spawns_directly() {
        let cmd = SpawnCommand::new("code", &["--new-window".into()]);

        assert_eq!(
            cmd,
            SpawnCommand {
                program: "code".into(),
                args: vec!["--new-window".into()]
            }
        );
    }

    #[test]
    fn leading_smart_dash_cli_args_are_normalized() {
        let cmd = SpawnCommand::new("clp", &["—url".into(), "–verbose".into(), "value".into()]);

        assert_eq!(
            cmd,
            SpawnCommand {
                program: "clp".into(),
                args: vec!["--url".into(), "--verbose".into(), "value".into()]
            }
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn mac_app_bundle_uses_open_n_with_args() {
        let cmd = SpawnCommand::new(
            "/Applications/Visual Studio Code.app",
            &["--new-window".into(), "/tmp/file.txt".into()],
        );

        assert_eq!(
            cmd,
            SpawnCommand {
                program: "/usr/bin/open".into(),
                args: vec![
                    "-n".into(),
                    "-a".into(),
                    "/Applications/Visual Studio Code.app".into(),
                    "--args".into(),
                    "--new-window".into(),
                    "/tmp/file.txt".into()
                ]
            }
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn mac_app_bundle_executable_spawns_directly() {
        let cmd = SpawnCommand::new(
            "/Applications/ColorLabPro.app/Contents/MacOS/clp",
            &[
                "handoff".into(),
                "open".into(),
                "--url".into(),
                "https://example.test".into(),
            ],
        );

        assert_eq!(
            cmd,
            SpawnCommand {
                program: "/Applications/ColorLabPro.app/Contents/MacOS/clp".into(),
                args: vec![
                    "handoff".into(),
                    "open".into(),
                    "--url".into(),
                    "https://example.test".into(),
                ]
            }
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn blocking_mac_app_bundle_waits_for_app() {
        let cmd = SpawnCommand::blocking("/Applications/TextEdit.app", &[]);

        assert_eq!(
            cmd,
            SpawnCommand {
                program: "/usr/bin/open".into(),
                args: vec![
                    "-n".into(),
                    "-W".into(),
                    "-a".into(),
                    "/Applications/TextEdit.app".into()
                ]
            }
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn resolve_spawn_rewrites_bundle_macos_executable_through_open() {
        let cmd = resolve_spawn(
            "/Applications/ColorLabPro.app/Contents/MacOS/clp",
            &["handoff".into(), "open".into(), "--url".into(), "https://example.test".into()],
        );

        assert_eq!(
            cmd,
            SpawnCommand {
                program: "/usr/bin/open".into(),
                args: vec![
                    "-n".into(),
                    "-a".into(),
                    "/Applications/ColorLabPro.app".into(),
                    "--args".into(),
                    "handoff".into(),
                    "open".into(),
                    "--url".into(),
                    "https://example.test".into(),
                ]
            }
        );
    }
}
