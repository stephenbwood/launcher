# Logs Tab Design

## Goal

Add a read-only Logs tab that shows each handled `launcher://` URI and the exact executable plus arguments Launcher spawned from it.

## Behavior

- Every handled URI creates a log entry with a timestamp, raw URI, route type, app id, status, and optional CLI call.
- `run` entries receive their CLI call immediately before the process is spawned.
- `relay` entries are created when the URI is accepted, then updated when the editor process is spawned after download and relay command resolution.
- Failed URI handling is visible as an error entry even when no CLI call exists.
- Logs persist across app restarts in a small JSON file under the app data directory.

## Architecture

Add a focused Rust `logs` module responsible for log entry types, persistence, append/update operations, and CLI string formatting. Extend `AppState` with a log store path and mutex-protected log state. Backend dispatch creates entries and run/relay launch paths update them with the resolved command.

Expose `list_logs` as a Tauri command. The frontend adds a third top-level tab named `Logs` with a newest-first table for time, URI, status, and CLI call.

## Testing

Use Rust unit tests for log serialization/update behavior and command formatting. Verify the frontend compiles with `npm run build`.
