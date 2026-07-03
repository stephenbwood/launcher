# Launcher

A lightweight cross-platform desktop app that registers the `launcher://` URL
scheme and dispatches incoming URLs to a defined application. Built with
**Rust + Tauri 2** (native OS webview — no bundled Chromium). See [`spec.md`](spec.md)
for the full design.

## Routes

```
launcher://run/<app-id>?file=/path/to/file.txt&arg=--wait
launcher://relay/<app-id>?src=<get-url>&dest=<put-url>&filename=report.docx
```

- **run** — launch a configured app with named/positional params (§2.1).
- **relay** — download a file, launch an editor, then upload the result back to
  `dest` on a completion signal (process exit, filesystem idle-debounce, or the
  manual "Upload & Finish" tray/queue action) (§2.2, §6).

## Project layout

```
index.html            Frontend shell (Relay Queue + Settings tabs)
src/                  Frontend (vanilla JS + CSS, bundled by Vite)
  main.js             UI logic + Tauri command calls
  styles.css
src-tauri/            Rust backend
  src/
    lib.rs            Tauri builder: plugins, setup, command registry
    main.rs           Binary entry point
    urlparse.rs       launcher:// parsing (§2)
    config.rs         App definitions (apps.json) (§3)
    substitute.rs     {file}/{arg}/{key} argument templating (§3)
    registration.rs   OS scheme registration: winreg / xdg-mime / Info.plist (§4)
    dispatch.rs       Route an incoming URL to run/relay
    run.rs            run route (§2.1)
    relay.rs          Relay sessions: download/spawn/watch/upload/recovery (§6)
    state.rs          Shared AppState
    commands.rs       Tauri commands bridging the UI (§7)
    tray.rs           Dynamic per-session system tray (§6.3, §7.2)
  tauri.conf.json     Tauri + bundle config (declares the launcher scheme)
  capabilities/       Frontend permission grants
apps.example.json     Example app-definitions config
```

## Prerequisites

- **Node.js** 18+ and npm
- **Rust** (stable) — https://rustup.rs
- Platform webview / build deps per the
  [Tauri v2 prerequisites](https://v2.tauri.app/start/prerequisites/):
  - Windows: Microsoft C++ Build Tools + WebView2 (preinstalled on Win 11)
  - macOS: Xcode Command Line Tools
  - Linux: `webkit2gtk`, `libappindicator`, etc.

## Develop

```bash
npm install
npm run tauri dev
```

## Build

```bash
npm run tauri build
```

### Icons

Placeholder icons live in `src-tauri/icons/` (PNG + ICO). To generate a full,
proper icon set (including macOS `.icns`) from a source image and wire it into
`tauri.conf.json`:

```bash
npm run tauri icon path/to/source.png
```

## Configuration

App definitions are stored as JSON at the OS app-config path
(`apps.json`), editable through the **Settings** screen in-app. See
[`apps.example.json`](apps.example.json) for the schema:

- `exec` / `args` — base command used by `run://` (and `relay://` when no
  override is present).
- `args` placeholders: `{file}` (the file path), `{arg}` (expands to each
  positional `arg=` value), and `{key}` for any other named param.
- optional `relay` block — overrides `exec`/`args` and sets `blocking` for
  relay sessions only.
- optional `display_name` — friendly name shown in the Relay Queue.
- optional `relay_allowed: false` — hard-deny relay against this app-id (§3).

## Notes / deviations from spec

- The `relay: false` hard-deny sentinel (§3) is modeled as a separate
  `relay_allowed` boolean field, since `relay` itself carries the override
  object (a value can't be both an object and `false` in the same key).
- Idle-debounce window is the spec's suggested 30s default (§6.2); making it
  configurable is an open item (§8).
- Completed session directories are deleted immediately on successful upload
  (§6.1 step 9); a retention window is an open item (§8).
