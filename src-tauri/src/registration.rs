//! OS-level `launcher://` scheme registration (§4).
//!
//! - Windows: per-user `HKCU\Software\Classes\launcher` keys via `winreg` (§4.1).
//! - Linux: a `.desktop` file + `xdg-mime default` (§4.3).
//! - macOS: handled by the app bundle's `Info.plist` (`CFBundleURLTypes`, §4.2),
//!   generated from `tauri.conf.json` — nothing to do at runtime.
//!
//! Tauri's deep-link plugin can also register the scheme, but the spec calls for
//! an explicit `winreg`/`xdg-mime` path, so we own it here and keep the behaviour
//! predictable across dev and installed builds.

const SCHEME: &str = "launcher";

/// Register the current executable as the handler for `launcher://`.
pub fn register() -> std::io::Result<()> {
    let exe = std::env::current_exe()?;
    register_with_exe(&exe)
}

#[cfg(windows)]
fn register_with_exe(exe: &std::path::Path) -> std::io::Result<()> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let base = format!("Software\\Classes\\{SCHEME}");

    // HKCU\Software\Classes\launcher
    let (scheme_key, _) = hkcu.create_subkey(&base)?;
    scheme_key.set_value("", &"URL:Launcher Protocol")?;
    scheme_key.set_value("URL Protocol", &"")?;

    // ...\shell\open\command  (Default) = "exe" "%1"
    let (cmd_key, _) = hkcu.create_subkey(format!("{base}\\shell\\open\\command"))?;
    let command = format!("\"{}\" \"%1\"", exe.display());
    cmd_key.set_value("", &command)?;

    log::info!("registered {SCHEME}:// -> {command}");
    Ok(())
}

#[cfg(target_os = "linux")]
fn register_with_exe(exe: &std::path::Path) -> std::io::Result<()> {
    use std::io::Write;

    let desktop_name = "launcher.desktop";
    let apps_dir = dirs_local_applications();
    std::fs::create_dir_all(&apps_dir)?;
    let desktop_path = apps_dir.join(desktop_name);

    let contents = format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Launcher\n\
         Exec={} %u\n\
         NoDisplay=true\n\
         StartupNotify=false\n\
         MimeType=x-scheme-handler/{SCHEME};\n",
        exe.display()
    );

    let mut f = std::fs::File::create(&desktop_path)?;
    f.write_all(contents.as_bytes())?;

    // xdg-mime default launcher.desktop x-scheme-handler/launcher
    let status = std::process::Command::new("xdg-mime")
        .args([
            "default",
            desktop_name,
            &format!("x-scheme-handler/{SCHEME}"),
        ])
        .status();

    match status {
        Ok(s) if s.success() => log::info!("registered {SCHEME}:// via xdg-mime"),
        Ok(s) => log::warn!("xdg-mime exited with status {s}"),
        Err(e) => log::warn!("could not run xdg-mime (is xdg-utils installed?): {e}"),
    }

    // Best-effort refresh of the desktop database.
    let _ = std::process::Command::new("update-desktop-database")
        .arg(&apps_dir)
        .status();

    Ok(())
}

#[cfg(target_os = "linux")]
fn dirs_local_applications() -> std::path::PathBuf {
    if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
        if !xdg.is_empty() {
            return std::path::PathBuf::from(xdg).join("applications");
        }
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    std::path::PathBuf::from(home).join(".local/share/applications")
}

#[cfg(target_os = "macos")]
fn register_with_exe(_exe: &std::path::Path) -> std::io::Result<()> {
    // Scheme is declared in the bundle Info.plist (CFBundleURLTypes) and picked
    // up by Launch Services when the .app is present/run (§4.2). Nothing to do
    // at runtime.
    log::info!("macOS: {SCHEME}:// registration handled by app bundle Info.plist");
    Ok(())
}
