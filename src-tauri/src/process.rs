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
            if let Some(app_bundle) = app_bundle_path(exec) {
                let mut args = Vec::new();
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
fn app_bundle_path(exec: &str) -> Option<PathBuf> {
    let path = Path::new(exec);
    for ancestor in path.ancestors() {
        let is_app = ancestor
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("app"));
        if is_app {
            return Some(ancestor.to_path_buf());
        }
    }
    None
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
    fn mac_app_bundle_uses_open_with_args() {
        let cmd = SpawnCommand::new(
            "/Applications/Visual Studio Code.app",
            &["--new-window".into(), "/tmp/file.txt".into()],
        );

        assert_eq!(
            cmd,
            SpawnCommand {
                program: "/usr/bin/open".into(),
                args: vec![
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
    fn mac_app_bundle_executable_uses_open_with_bundle() {
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
                program: "/usr/bin/open".into(),
                args: vec![
                    "-a".into(),
                    "/Applications/ColorLabPro.app".into(),
                    "--args".into(),
                    "handoff".into(),
                    "open".into(),
                    "--url".into(),
                    "https://example.test".into()
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
                    "-W".into(),
                    "-a".into(),
                    "/Applications/TextEdit.app".into()
                ]
            }
        );
    }
}
