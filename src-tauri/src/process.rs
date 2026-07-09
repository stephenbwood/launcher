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
        #[cfg(target_os = "macos")]
        {
            if is_app_bundle_path(exec) {
                let mut args = Vec::new();
                if wait {
                    args.push("-W".to_string());
                }
                args.push("-a".to_string());
                args.push(exec.to_string());
                if !argv.is_empty() {
                    args.push("--args".to_string());
                    args.extend(argv.iter().cloned());
                }
                return Self {
                    program: "/usr/bin/open".to_string(),
                    args,
                };
            }
        }

        Self {
            program: exec.to_string(),
            args: argv.to_vec(),
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

#[cfg(target_os = "macos")]
fn is_app_bundle_path(exec: &str) -> bool {
    Path::new(exec)
        .extension()
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
