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
            if let Some(program) = app_bundle_executable_path(exec) {
                return Self {
                    program: program.to_string_lossy().to_string(),
                    args: argv,
                };
            }

            if let Some(app_bundle) = exact_app_bundle_path(exec) {
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
fn exact_app_bundle_path(exec: &str) -> Option<PathBuf> {
    let path = Path::new(exec);
    if is_app_bundle(path) {
        return Some(path.to_path_buf());
    }
    None
}

#[cfg(target_os = "macos")]
fn app_bundle_executable_path(exec: &str) -> Option<PathBuf> {
    let app_bundle = exact_app_bundle_path(exec)?;
    let executable = bundle_executable_name(&app_bundle)
        .and_then(|name| existing_bundle_executable(&app_bundle, &name))
        .or_else(|| single_bundle_executable(&app_bundle))?;
    Some(executable)
}

#[cfg(target_os = "macos")]
fn is_app_bundle(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("app"))
}

#[cfg(target_os = "macos")]
fn bundle_executable_name(app_bundle: &Path) -> Option<String> {
    let info = plist::Value::from_file(app_bundle.join("Contents/Info.plist")).ok()?;
    info.as_dictionary()?
        .get("CFBundleExecutable")?
        .as_string()
        .map(ToString::to_string)
}

#[cfg(target_os = "macos")]
fn existing_bundle_executable(app_bundle: &Path, name: &str) -> Option<PathBuf> {
    let executable = app_bundle.join("Contents/MacOS").join(name);
    executable.is_file().then_some(executable)
}

#[cfg(target_os = "macos")]
fn single_bundle_executable(app_bundle: &Path) -> Option<PathBuf> {
    let macos_dir = app_bundle.join("Contents/MacOS");
    let mut executables = Vec::new();
    for entry in std::fs::read_dir(macos_dir).ok()? {
        let path = entry.ok()?.path();
        if path.is_file() {
            executables.push(path);
        }
    }
    if executables.len() == 1 {
        executables.pop()
    } else {
        None
    }
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
    fn mac_app_bundle_uses_bundle_executable_with_args() {
        let app = fixture_app_bundle("bundle-args", "clp");
        let exec = app.path.to_string_lossy().to_string();
        let program = app.executable.to_string_lossy().to_string();
        let cmd = SpawnCommand::new(&exec, &["--new-window".into(), "/tmp/file.txt".into()]);

        assert_eq!(
            cmd,
            SpawnCommand {
                program,
                args: vec!["--new-window".into(), "/tmp/file.txt".into()]
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
    fn blocking_mac_app_bundle_uses_bundle_executable() {
        let app = fixture_app_bundle("blocking-bundle", "TextEdit");
        let exec = app.path.to_string_lossy().to_string();
        let program = app.executable.to_string_lossy().to_string();
        let cmd = SpawnCommand::blocking(&exec, &[]);

        assert_eq!(
            cmd,
            SpawnCommand {
                program,
                args: vec![]
            }
        );
    }

    #[cfg(target_os = "macos")]
    struct FixtureApp {
        path: PathBuf,
        executable: PathBuf,
    }

    #[cfg(target_os = "macos")]
    impl Drop for FixtureApp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    #[cfg(target_os = "macos")]
    fn fixture_app_bundle(name: &str, executable_name: &str) -> FixtureApp {
        let path = std::env::temp_dir().join(format!(
            "launcher-process-test-{name}-{}.app",
            std::process::id()
        ));
        let contents = path.join("Contents");
        let macos = contents.join("MacOS");
        std::fs::create_dir_all(&macos).unwrap();
        std::fs::write(
            contents.join("Info.plist"),
            format!(
                r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleExecutable</key>
  <string>{executable_name}</string>
</dict>
</plist>
"#
            ),
        )
        .unwrap();
        let executable = macos.join(executable_name);
        std::fs::write(&executable, "").unwrap();

        FixtureApp { path, executable }
    }
}
