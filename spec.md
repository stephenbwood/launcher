# Protocol Handler Application Launcher — Spec

## 1. Overview

A lightweight, cross-platform (Windows/macOS/Linux) application that registers a custom
URL scheme (`launcher://`) and dispatches incoming URLs to one of two route types:

- **`run`** — launch a defined application with named/positional parameters.
- **`relay`** — download a file, launch an app to edit it, and upload the result to a
  destination URL once editing is finished.

Design goal: no embedded browser/webview runtime, no bundled Chromium — footprint should
be dominated by the UI toolkit, not by this feature set.

**Stack**: Rust + Tauri. Tauri uses the OS's native webview (WebView2 / WebKit /
WebKitGTK) rather than bundling Chromium, keeping binaries in the 3–15MB range. See §8 for
crate/API mapping against each section below.

---

## 2. URL Scheme

### 2.1 Run route

```
launcher://run/<app-id>?key1=value1&key2=value2&arg=positional1&arg=positional2
```

- **Named parameters**: standard query string keys, URL-decoded.
- **Positional parameters**: repeated `arg=` keys, preserved in order. Repeated keys are
  used instead of a delimited single value to avoid escaping ambiguity.

Example:

```
launcher://run/vscode?file=/path/to/file.txt&arg=--wait
```

### 2.2 Relay route

```
launcher://relay/<app-id>?src=<download-url>&dest=<upload-url>&filename=report.docx
```

- `src` — presigned GET URL to download the working file from.
- `dest` — presigned PUT/POST URL to upload the finished file to.
- `filename` — local filename to use in the session's temp directory (also used to infer
  file type/extension for the target app).

`src` and `dest` are treated as fully independent operations — different hosts, different
auth, no assumed relationship between them.

---

## 3. App Definitions

A local config file (JSON shown, TOML equally workable) maps `app-id` to an executable and
an argument template. One entry serves both route types: `run` uses the base `exec`/`args`,
and `relay` uses the same base but may override behavior via an optional `relay` block.

```json
{
  "vscode": {
    "exec": "code",
    "args": ["{file}", "{arg}"],
    "relay": {
      "args": ["{file}", "--wait"],
      "blocking": true
    }
  },
  "word": {
    "exec": "/Applications/Microsoft Word.app/Contents/MacOS/Microsoft Word",
    "args": ["{file}"]
  }
}
```

- `{file}` — substituted with the local file path (either the target of a `run` route's
  `file` param, or the relay's downloaded temp file path).
- `{arg}` — expands to all positional `arg=` values, in order, each as a separate argv
  entry.
- Named parameters other than `file` substitute by key: `{key1}`, `{key2}`, etc.
- `relay` block (optional): overrides `args`/`exec` and declares `blocking` for relay
  sessions specifically.
  - If **absent**, a relay session falls back to the base `args`/`exec` and assumes
    `blocking: false` — completion relies on idle-debounce + manual "Upload & Finish"
    (see §6.2) rather than process exit.
  - If **present**, its `args`/`blocking` apply only to sessions launched via `relay://`;
    `run://` always uses the base definition.
  - `exec` may also be overridden inside `relay` if the relay-appropriate app differs
    entirely from the `run` target (e.g. a lightweight editor for quick relay edits vs. a
    full IDE for normal launches) — falls back to the base `exec` if omitted.
- An app-id with no `relay` block and no plausible relay use case simply never gets
  targeted by a `relay://` URL in practice; no explicit allow/deny flag is required unless
  hard enforcement is wanted later (e.g. a `relay: false` sentinel to make the launcher
  reject relay requests against it outright).

---

## 4. OS Registration

### 4.1 Windows

Per-user registration (no admin elevation required) under
`HKEY_CURRENT_USER\Software\Classes`:

```
HKCU\Software\Classes\launcher
  (Default)        = "URL:Launcher Protocol"
  URL Protocol      = ""
  shell\open\command
    (Default)      = "C:\path\to\launcher.exe" "%1"
```

The full URL is delivered as `argv[1]` on process launch.

### 4.2 macOS

Declared in the app bundle's `Info.plist`:

```xml
<key>CFBundleURLTypes</key>
<array>
  <dict>
    <key>CFBundleURLSchemes</key>
    <array><string>launcher</string></array>
  </dict>
</array>
```

Launch Services registers the scheme when the `.app` bundle is present/run; can be forced
with `LSRegisterURL` at install time.

**Important**: delivery is via an Apple Event (`kInternetEventClass` /
`kAEGetURL`), not argv. The app must install an Apple Event handler — this affects both
initial launch and delivery to an already-running instance.

### 4.3 Linux

A `.desktop` file with a MIME association:

```ini
[Desktop Entry]
Type=Application
Name=Launcher
Exec=/path/to/launcher %u
MimeType=x-scheme-handler/launcher;
```

Registered via:

```
xdg-mime default launcher.desktop x-scheme-handler/launcher
```

URL is delivered as an argv entry, same as Windows.

---

## 5. Single-Instance & IPC

Behavior differs by platform when a second `launcher://` URL is triggered while the app is
already running:

| Platform | Second-invocation behavior |
|---|---|
| Windows | New process spawns |
| Linux | New process spawns |
| macOS | Existing process receives another Apple Event; no new process |

To handle Windows/Linux uniformly with macOS, the launcher needs a single-instance
mechanism independent of the OS delivery method:

1. On startup, attempt to acquire a lock (lock file or named pipe/Unix socket).
2. If lock acquisition fails, an instance is already running — forward the URL to it over
   the IPC channel (socket/pipe) and exit.
3. The running instance listens on that channel for forwarded URLs alongside its native
   OS event handling.

---

## 6. Relay Mode

### 6.1 Session lifecycle

```
1. Generate session ID (UUID).
2. Create session directory: <app-data>/relay-sessions/<session-id>/
3. GET src → save as <filename> inside session directory.
4. Persist session metadata to disk (session.json): app-id, src, dest, filename,
   local path, status, created_at.
5. Spawn app-id's executable against the local file path.
6. Start filesystem watcher on the file (mtime/hash) and, if the launched process is
   exit-blocking, watch the process handle too.
7. Update UI: show/append to the multi-session tray list with this session's status
   ("editing").
8. On completion trigger (see 6.2): upload file contents to dest (PUT/POST, no
   additional auth — dest is presigned).
9. On upload success: mark session "done", remove from active tray list, delete session
   directory (or retain briefly as a safety net, TBD).
10. On upload failure: mark session "error" in tray, retry with backoff, keep session
    directory and metadata intact.
```

### 6.2 Completion signal (multi-signal design)

No single OS signal reliably indicates "user is done editing." Combine three:

1. **Process exit** — authoritative when the launched app blocks until closed.
2. **Filesystem idle-debounce** — no writes to the file for N seconds (configurable;
   suggested default 30s) is treated as a candidate "idle" state, but does not
   auto-trigger upload by itself.
3. **Explicit user action** — tray entry per session has an "Upload & Finish" action.
   This is the only unambiguous signal and is always available regardless of the other
   two.

Default behavior: auto-upload on process exit for exit-blocking apps; otherwise rely on
the tray's manual trigger, with the idle-debounce state surfaced in the UI (e.g. status
changes from "editing" to "idle, ready to upload") but not auto-firing.

### 6.3 Concurrency

Multiple relay sessions run simultaneously. Each session is independent:

- Own session ID, temp directory, metadata file, filesystem watcher, and process handle.
- Tray/status UI is a list, one row per active session, each with its own status and
  manual "Upload & Finish" control.
- No shared state between sessions beyond the parent process managing them.

### 6.4 Conflict handling

None. Uploads always overwrite `dest` — no ETag/If-Match precondition, no
last-writer-wins detection. This can be added later as an optional feature without
breaking the URL scheme (an additional query param, e.g. `if-match=`).

### 6.5 Auth

`src` and `dest` are presigned URLs. The launcher performs a plain `GET` on `src` and a
plain `PUT`/`POST` on `dest` with no additional headers or stored credentials. The
launcher holds no long-lived secrets and requires no credential management, refresh, or
storage layer.

### 6.6 Crash recovery

On startup, the launcher scans `<app-data>/relay-sessions/` for session directories with
metadata but no corresponding live process/lock — these are orphaned sessions from a
prior crash.

For each orphaned session found, prompt the user (per-session) with three options:

- **Resume** — reopen the file in the associated app and resume watching.
- **Upload** — upload the file as-is to `dest`.
- **Discard** — delete the session directory without uploading.

No orphaned session is auto-uploaded or auto-discarded without user confirmation.

To make "resume" meaningful rather than just "re-launch the app," idle/edit state should
be persisted into `session.json` on every debounce-state change (not held only in
memory), so the prompt can show accurate context (e.g. last-modified time, whether the
file was mid-edit vs. already idle at crash time).

---

## 7. UI Screens

Two screens cover the user-facing surface: **Settings** (app registration) and **Relay
Queue** (live session monitoring). Both are Tauri windows/views within the same app, not
separate binaries.

### 7.1 Settings Screen

**Access**: gear icon button, persistent in the main window's title bar/corner, and also
reachable from the tray menu ("Settings…").

**Purpose**: register, edit, and remove application definitions (§3) — the `app-id` →
`exec`/`args`/`relay` records used by both `run` and `relay` routes.

**Layout**:

- A list view of all registered apps: `app-id`, `exec` path (truncated/tooltip for full
  path), and a badge indicating whether a `relay` override is defined.
- `+ Add Application` button opens the edit form (below) blank.
- Clicking a row opens the same edit form, pre-filled, for that app.
- Delete action per row (with confirmation, since deleting an app-id referenced by an
  in-flight relay session should be blocked or warned against).

**Edit form fields**:

- `app-id` — text, required, unique (validated against existing entries; immutable after
  creation to avoid silently breaking existing `launcher://run/<app-id>` links already in
  use elsewhere).
- `exec` — text + file-picker button, required. Basic existence check on save (non-fatal
  warning if the path doesn't currently resolve, since the app may be installed later).
- `args` — ordered list editor. Each entry is either a literal string or a placeholder
  (`{file}`, `{arg}`, or a named param like `{key1}`). Add/remove/reorder entries.
- `relay` override section — collapsed by default, expandable:
  - `exec` override (optional, falls back to base `exec` if left blank).
  - `args` override (same ordered-list editor as above).
  - `blocking` toggle — whether this app blocks until closed (drives the completion
    signal choice in §6.2).
- Save / Cancel. Save writes directly to the app definitions config file (§3); no
  separate "apply" step.

**Empty state**: "No applications registered yet" with the `+ Add Application` button
front and center.

### 7.2 Relay Queue Screen

**Access**: default view when opening the main window from the tray icon; also reachable
via a "Relay Queue" tray menu item. This is the multi-session list referenced in §6.3.

**Layout**: a list view, one row per relay session (active, idle, uploading, error, *and*
orphaned-pending-decision sessions from §6.6 — orphans surface here rather than in a
separate modal, using the same row/action pattern instead of a distinct dialog).

**Columns**:

| Column | Content |
|---|---|
| File | `filename` from the session, with a small status icon (editing / idle / uploading / error / orphaned) |
| Target application | The `app-id`'s display name (or `app-id` itself if no separate display name is defined) |
| Status | Text label: Editing, Idle (ready to upload), Uploading…, Done, Error, Orphaned |
| Actions | Right-aligned buttons, contents depend on status (below) |

**Action buttons by status**:

- **Editing**: `Upload & Finish` (manual trigger, §6.2), `Cancel` (discard session,
  confirmation required).
- **Idle**: `Upload & Finish` (primary), `Keep Editing` (dismiss the idle state and keep
  watching), `Cancel`.
- **Uploading**: buttons disabled, progress indicator in place of actions.
- **Error**: `Retry`, `Cancel`.
- **Orphaned** (post-crash, §6.6): `Resume`, `Upload`, `Discard` — the three options from
  §6.6, presented as row actions rather than a blocking prompt, so multiple orphaned
  sessions can be triaged from one screen instead of one modal per session.

**Empty state**: "No active relay sessions" — this is the default/idle state of the
screen for most of the app's lifetime, since sessions are usually short-lived.

**Row lifecycle**: rows appear the moment a `relay://` URL is handled (download starts)
and are removed automatically on successful upload (§6.1 step 9), or remain (as
Error/Orphaned) until the user resolves them via the action buttons.

---

## 8. Open Items for Future Iterations

- Optional `if-match`/ETag precondition support on `dest` uploads.
- Configurable retention period for completed session directories (currently: delete
  immediately on successful upload).
- Configurable idle-debounce window (currently hardcoded suggestion of 30s).

---

## 9. Implementation Notes (Rust + Tauri)

Maps each spec section to the crate/API expected to implement it. Not a lockfile — names
are the current best fit as of this writing; verify current versions/APIs before scaffolding.

| Spec section | Concern | Crate / API |
|---|---|---|
| §4.1 Windows registration | Write `HKCU\Software\Classes\<scheme>` keys | `winreg` |
| §4.2 macOS registration | Declare scheme in bundle | Tauri's `tauri.conf.json` bundle config generates `CFBundleURLTypes` in `Info.plist` |
| §4.2 macOS delivery | Receive Apple Event (`kAEGetURL`) | Tauri's deep-link plugin (`tauri-plugin-deep-link`) abstracts this instead of hand-rolling Apple Event handling |
| §4.3 Linux registration | Write `.desktop` file, run `xdg-mime` | `std::process::Command` shelling out to `xdg-mime`; `.desktop` file written directly or via Tauri's Linux bundler config |
| §4.1/§4.3 URL delivery (argv) | Parse `argv[1]`/`%u` | `std::env::args()` on startup |
| §5 Single-instance & IPC | Lock + forward URL to running instance | `tauri-plugin-single-instance` (handles the lock-and-forward pattern cross-platform; can replace most of the custom logic in §5) |
| §2 URL parsing | Query string / positional `arg=` parsing | `url` crate (`url::Url::parse`, `.query_pairs()`) |
| §3 App definitions | Config file load/parse | `serde` + `serde_json` (or `toml` if TOML is preferred) |
| §3 Process spawning | Launch app-id's `exec`/`args` | `std::process::Command` |
| §6.1 File download (`src`) | Presigned GET | `reqwest` |
| §6.1 File upload (`dest`) | Presigned PUT/POST | `reqwest` |
| §6.1 Session persistence | Write `session.json` | `serde_json` to a file under Tauri's `app_data_dir()` |
| §6.2 Filesystem idle-debounce | Watch file mtime/hash | `notify` crate, paired with a debounce timer (`notify-debouncer-mini` wraps this directly) |
| §6.2/§6.3 Process exit detection | Detect blocking app close | `std::process::Child::wait()` on a background thread/task per session |
| §6.3 Multi-session tray UI | Per-session status list, "Upload & Finish" action | Tauri's system tray API (`tauri::tray::TrayIconBuilder`) with a dynamic menu, or a small always-on-top Tauri window listing sessions if a menu list proves too limited |
| §6.6 Crash recovery scan | Detect orphaned session dirs on startup | Plain `std::fs::read_dir` over the sessions directory, cross-referenced against active locks from §5's single-instance mechanism |
| §7.1 Settings screen | App definitions CRUD UI | Tauri window (HTML/CSS/JS frontend) with Tauri commands (`#[tauri::command]`) bridging to the Rust-side config read/write from §3 |
| §7.2 Relay Queue screen | Live session list UI | Tauri window subscribed to session state via Tauri events (`app.emit`/`listen`) fired whenever a session's status changes (filesystem watcher tick, upload progress, process exit) |

**Note on §4.2 and §5**: Tauri's `tauri-plugin-deep-link` and `tauri-plugin-single-instance`
plugins cover a meaningful chunk of what §4.2 (Apple Event handling) and §5
(single-instance + IPC forwarding) describe from scratch — worth evaluating those plugins
first before hand-rolling the lock file/socket and Apple Event handler, since they're
maintained specifically for this cross-platform URL-scheme-delivery problem.
