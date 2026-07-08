import React, { useCallback, useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import { readText as readClipboard } from "@tauri-apps/plugin-clipboard-manager";

const TAB_BASE = "rounded-md px-3 py-1.5 text-sm text-app-muted transition-colors hover:bg-app-panel-2 hover:text-app-text";
const BUTTON_BASE = "inline-flex items-center justify-center rounded-md border border-app-border bg-app-panel-2 px-3 py-1.5 text-[13px] text-app-text transition hover:bg-[#40434a] disabled:cursor-not-allowed disabled:opacity-60";
const BUTTON_SMALL = "px-2 py-0.5 text-xs";
const BUTTON_PRIMARY = "border-app-accent bg-app-accent text-white hover:brightness-110";
const BUTTON_DANGER = "border-[#5a3134] text-[#f4b6b6] hover:bg-app-danger hover:text-white";
const INPUT_CLASS = "mt-1 w-full rounded-md border border-app-border bg-app-bg px-2.5 py-1.5 text-[13px] text-app-text disabled:opacity-60";
const INLINE_INPUT_CLASS = "min-w-0 flex-1 rounded-md border border-app-border bg-app-bg px-2.5 py-1.5 text-[13px] text-app-text";
const HINT_CLASS = "mt-1 block text-[11px] text-app-muted";
const HINT_WARN_CLASS = "mt-1 block text-[11px] text-app-warn";
const TH_CLASS = "border-b border-app-border px-2.5 py-2 text-left text-xs font-semibold uppercase tracking-wide text-app-muted";
const TD_CLASS = "p-2.5 align-middle";
const MONO_TRUNCATE = "truncate font-mono text-xs";

const STATUS_LABELS = {
  downloading: "Downloading...",
  editing: "Editing",
  idle: "Idle (ready to upload)",
  uploading: "Uploading...",
  done: "Done",
  error: "Error",
  orphaned: "Orphaned",
};

const STATUS_ICONS = {
  downloading: "⬇",
  editing: "✎",
  idle: "◔",
  uploading: "⬆",
  done: "✓",
  error: "⚠",
  orphaned: "⟳",
};

const STATUS_COLORS = {
  downloading: "text-app-accent",
  editing: "text-app-accent",
  idle: "text-app-warn",
  uploading: "text-app-accent",
  done: "text-app-ok",
  error: "text-app-danger",
  orphaned: "text-app-warn",
};

const LOG_STATUS_LABELS = {
  handled: "Handled",
  launched: "Launched",
  error: "Error",
};

const LOG_STATUS_COLORS = {
  handled: "text-app-muted",
  launched: "text-app-ok",
  error: "text-app-danger",
};

function Button({ children, className = "", size = "normal", variant = "default", ...props }) {
  const variantClass =
    variant === "primary" ? BUTTON_PRIMARY : variant === "danger" ? BUTTON_DANGER : "";
  const sizeClass = size === "small" ? BUTTON_SMALL : "";
  return (
    <button className={`${BUTTON_BASE} ${sizeClass} ${variantClass} ${className}`.trim()} {...props}>
      {children}
    </button>
  );
}

function App() {
  const [view, setView] = useState("queue");
  const [sessions, setSessions] = useState([]);
  const [logs, setLogs] = useState([]);
  const [apps, setApps] = useState({});
  const [appRows, setAppRows] = useState([]);
  const [editingId, setEditingId] = useState(null);
  const [showAppModal, setShowAppModal] = useState(false);
  const [showUrlModal, setShowUrlModal] = useState(false);
  const [urlImportValue, setUrlImportValue] = useState("");

  const refreshSessions = useCallback(async () => {
    setSessions(await invoke("list_sessions"));
  }, []);

  const refreshLogs = useCallback(async () => {
    setLogs(await invoke("list_logs"));
  }, []);

  const refreshApps = useCallback(async () => {
    const config = await invoke("list_apps");
    setApps(config);
    const rows = await Promise.all(
      Object.entries(config).map(async ([id, def]) => ({
        id,
        def,
        uriExample: exampleUri(id, def),
        commandExample: await cliPreview(def),
      })),
    );
    setAppRows(rows);
  }, []);

  useEffect(() => {
    refreshSessions().catch(console.error);
    refreshApps().catch(console.error);
    refreshLogs().catch(console.error);
  }, [refreshApps, refreshLogs, refreshSessions]);

  useEffect(() => {
    let unlistenWindow = null;
    getCurrentWindow()
      .listen("navigate", (event) => setView(event.payload))
      .then((unlisten) => {
        unlistenWindow = unlisten;
      })
      .catch(console.error);
    return () => unlistenWindow?.();
  }, []);

  useEffect(() => {
    let unlistenRelay = null;
    listen("relay:update", (event) => setSessions(event.payload))
      .then((unlisten) => {
        unlistenRelay = unlisten;
      })
      .catch(console.error);
    return () => unlistenRelay?.();
  }, []);

  useEffect(() => {
    if (view === "logs") refreshLogs().catch(console.error);
  }, [refreshLogs, view]);

  const openForm = (id = null) => {
    setEditingId(id);
    setShowAppModal(true);
  };

  const closeForm = () => {
    setEditingId(null);
    setShowAppModal(false);
  };

  const saveApp = async (appId, definition) => {
    const updated = await invoke("save_app", { appId, definition });
    setApps(updated);
    await refreshApps();
    closeForm();
  };

  const deleteApp = async (appId) => {
    if (!confirm(`Delete application "${appId}"?`)) return;
    try {
      const updated = await invoke("delete_app", { appId });
      setApps(updated);
      await refreshApps();
    } catch (e) {
      alert(`${e}`);
    }
  };

  const exportConfig = async () => {
    try {
      const path = await saveDialog({
        defaultPath: "launcher-apps.json",
        filters: [{ name: "JSON", extensions: ["json"] }],
      });
      if (path) await invoke("export_config", { path });
    } catch (e) {
      alert(`Export failed: ${e}`);
    }
  };

  const importConfig = async () => {
    try {
      const selected = await openDialog({
        multiple: false,
        directory: false,
        filters: [{ name: "JSON", extensions: ["json"] }],
      });
      if (typeof selected !== "string") return;
      await resolveAndCommit(await invoke("import_config", { path: selected }), refreshApps);
    } catch (e) {
      alert(`Import failed: ${e}`);
    }
  };

  const openUrlImport = async () => {
    setUrlImportValue("");
    setShowUrlModal(true);
    try {
      const clip = (await readClipboard())?.trim();
      if (clip && isHttpsUrl(clip)) setUrlImportValue(clip);
    } catch {
      // Clipboard empty or unavailable; leave the field blank.
    }
  };

  const submitUrlImport = async (event) => {
    event.preventDefault();
    const url = urlImportValue.trim();
    if (!url) return;
    if (!/^https:\/\//i.test(url)) {
      alert("URL must start with https://");
      return;
    }
    try {
      await resolveAndCommit(await invoke("import_config_from_url", { url }), refreshApps);
      setShowUrlModal(false);
    } catch (e) {
      alert(`Import failed: ${e}`);
    }
  };

  const clearLogs = async () => {
    if (!confirm("Clear all log entries?")) return;
    try {
      await invoke("clear_logs");
      await refreshLogs();
    } catch (e) {
      alert(`Clear failed: ${e}`);
    }
  };

  return (
    <div className="min-h-screen bg-app-bg text-sm text-app-text [color-scheme:dark]">
      <TopBar view={view} setView={setView} />
      <main className="p-4">
        {view === "queue" && <RelayQueue sessions={sessions} />}
        {view === "settings" && (
          <SettingsView
            appRows={appRows}
            deleteApp={deleteApp}
            exportConfig={exportConfig}
            importConfig={importConfig}
            openForm={openForm}
            openUrlImport={openUrlImport}
          />
        )}
        {view === "logs" && <LogsView logs={logs} clearLogs={clearLogs} refreshLogs={refreshLogs} />}
      </main>
      {showAppModal && (
        <AppFormModal
          apps={apps}
          editingId={editingId}
          onClose={closeForm}
          onSave={saveApp}
        />
      )}
      {showUrlModal && (
        <UrlImportModal
          value={urlImportValue}
          onChange={setUrlImportValue}
          onClose={() => setShowUrlModal(false)}
          onSubmit={submitUrlImport}
        />
      )}
    </div>
  );
}

function TopBar({ view, setView }) {
  return (
    <header className="flex items-center gap-4 border-b border-app-border bg-app-panel px-4 py-2.5">
      <div className="font-bold tracking-wide">Launcher</div>
      <nav className="ml-2 flex gap-1">
        {[
          ["queue", "Relay Queue"],
          ["settings", "Settings"],
          ["logs", "Logs"],
        ].map(([id, label]) => (
          <button
            className={`${TAB_BASE} ${view === id ? "bg-app-panel-2 text-app-text" : "text-app-muted"}`}
            key={id}
            onClick={() => setView(id)}
          >
            {label}
          </button>
        ))}
      </nav>
    </header>
  );
}

function RelayQueue({ sessions }) {
  const visible = sessions.filter((session) => session.status !== "done");
  if (!visible.length) {
    return <div className="px-4 py-12 text-center text-app-muted">No active relay sessions</div>;
  }
  return (
    <table className="w-full border-collapse">
      <thead>
        <tr>
          <th className={TH_CLASS}>File</th>
          <th className={TH_CLASS}>Target application</th>
          <th className={TH_CLASS}>Status</th>
          <th className="w-px border-b border-app-border px-2.5 py-2" />
        </tr>
      </thead>
      <tbody>
        {visible.map((session) => (
          <tr className="border-b border-app-border" key={session.id}>
            <td className={TD_CLASS}>
              <span className={`mr-1 inline-block w-5 text-center ${STATUS_COLORS[session.status] || ""}`}>
                {STATUS_ICONS[session.status] || "•"}
              </span>
              {session.filename}
            </td>
            <td className={TD_CLASS}>{session.app_name || session.app_id}</td>
            <td className={TD_CLASS} title={session.error || ""}>
              {STATUS_LABELS[session.status] || session.status}
            </td>
            <td className="space-x-1.5 whitespace-nowrap p-2.5 text-right align-middle">
              <SessionActions session={session} />
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

function SessionActions({ session }) {
  const run = (cmd) => sessionCmd(cmd, session.id);
  switch (session.status) {
    case "editing":
      return (
        <>
          <Button size="small" variant="primary" onClick={() => run("session_upload_finish")}>
            Upload & Finish
          </Button>
          <Button size="small" variant="danger" onClick={() => confirmCancel(session)}>
            Cancel
          </Button>
        </>
      );
    case "idle":
      return (
        <>
          <Button size="small" variant="primary" onClick={() => run("session_upload_finish")}>
            Upload & Finish
          </Button>
          <Button size="small" onClick={() => run("session_keep_editing")}>
            Keep Editing
          </Button>
          <Button size="small" variant="danger" onClick={() => confirmCancel(session)}>
            Cancel
          </Button>
        </>
      );
    case "uploading":
    case "downloading":
      return <span className="inline-block size-4 animate-spin rounded-full border-2 border-app-border border-t-app-accent" />;
    case "error":
      return (
        <>
          <Button size="small" variant="primary" onClick={() => run("session_retry")}>
            Retry
          </Button>
          <Button size="small" variant="danger" onClick={() => confirmCancel(session)}>
            Cancel
          </Button>
        </>
      );
    case "orphaned":
      return (
        <>
          <Button size="small" variant="primary" onClick={() => run("session_resume")}>
            Resume
          </Button>
          <Button size="small" onClick={() => run("session_upload_orphan")}>
            Upload
          </Button>
          <Button size="small" variant="danger" onClick={() => run("session_discard")}>
            Discard
          </Button>
        </>
      );
    default:
      return null;
  }
}

function SettingsView({ appRows, deleteApp, exportConfig, importConfig, openForm, openUrlImport }) {
  return (
    <>
      <div className="mb-3 flex items-center justify-between">
        <h2 className="m-0 text-base font-semibold">Registered applications</h2>
        <div className="flex gap-2">
          <Button onClick={importConfig}>Import File...</Button>
          <Button onClick={openUrlImport}>Import URL...</Button>
          <Button onClick={exportConfig}>Export...</Button>
          <Button variant="primary" onClick={() => openForm(null)}>
            + Add Application
          </Button>
        </div>
      </div>
      {appRows.length ? (
        <ul className="m-0 list-none p-0">
          {appRows.map(({ id, def, uriExample, commandExample }) => (
            <li className="mb-2 flex items-center justify-between gap-3 rounded-lg border border-app-border bg-app-panel px-3 py-2.5" key={id}>
              <button className="min-w-0 flex-1 cursor-pointer overflow-hidden text-left" onClick={() => openForm(id)}>
                <div className="flex min-w-0 items-center gap-2.5">
                  <span className="shrink-0 font-semibold">
                    {def.display_name ? `${def.display_name} [${id}]` : id}
                  </span>
                  <span className="min-w-0 flex-1 truncate text-xs text-app-muted" title={def.exec}>
                    {def.exec}
                  </span>
                  {def.relay && (
                    <span className="rounded-full bg-app-accent px-1.5 py-px text-[10px] uppercase tracking-wide text-white">
                      relay
                    </span>
                  )}
                </div>
                <div className={`mt-1 ${MONO_TRUNCATE} text-app-muted`} title={uriExample}>
                  {uriExample}
                </div>
                <div className={`mt-1 ${MONO_TRUNCATE} text-app-text`} title={commandExample}>
                  {commandExample}
                </div>
              </button>
              <Button size="small" variant="danger" onClick={() => deleteApp(id)}>
                Delete
              </Button>
            </li>
          ))}
        </ul>
      ) : (
        <div className="px-4 py-12 text-center text-app-muted">
          <p>No applications registered yet</p>
          <Button className="mt-3" variant="primary" onClick={() => openForm(null)}>
            + Add Application
          </Button>
        </div>
      )}
    </>
  );
}

function LogsView({ logs, clearLogs, refreshLogs }) {
  const rerunLogEntry = async (id) => {
    try {
      await invoke("rerun_log_entry", { id });
      await refreshLogs();
    } catch (e) {
      alert(`Re-run failed: ${e}`);
    }
  };

  return (
    <>
      <div className="mb-3 flex items-center justify-between">
        <h2 className="m-0 text-base font-semibold">Logs</h2>
        <div className="flex gap-2">
          <Button disabled={!logs.length} variant="danger" onClick={clearLogs}>
            Clear
          </Button>
        </div>
      </div>
      {logs.length ? (
        <table className="w-full table-fixed border-collapse">
          <thead>
            <tr>
              <th className={`${TH_CLASS} w-[180px]`}>Time</th>
              <th className={TH_CLASS}>URI</th>
              <th className={`${TH_CLASS} w-[110px]`}>Status</th>
              <th className={TH_CLASS}>CLI call</th>
              <th className="w-[92px] border-b border-app-border px-2.5 py-2" />
            </tr>
          </thead>
          <tbody>
            {logs.map((entry) => (
              <tr className="border-b border-app-border" key={entry.id}>
                <td className={TD_CLASS}>{formatLogTime(entry.created_at)}</td>
                <td className="break-words p-2.5 align-middle font-mono text-xs" title={entry.raw_uri}>
                  {entry.raw_uri}
                </td>
                <td className={`p-2.5 align-middle font-semibold ${LOG_STATUS_COLORS[entry.status] || ""}`} title={entry.error || ""}>
                  {LOG_STATUS_LABELS[entry.status] || entry.status}
                </td>
                <td className="break-words p-2.5 align-middle font-mono text-xs" title={entry.cli_call || entry.error || ""}>
                  {entry.cli_call || ""}
                </td>
                <td className="space-x-1.5 whitespace-nowrap p-2.5 text-right align-middle">
                  <Button size="small" variant="primary" onClick={() => rerunLogEntry(entry.id)}>
                    Re-run
                  </Button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      ) : (
        <div className="px-4 py-12 text-center text-app-muted">No handled URIs logged yet</div>
      )}
    </>
  );
}

function AppFormModal({ apps, editingId, onClose, onSave }) {
  const isEdit = editingId !== null;
  const initial = isEdit ? apps[editingId] : { exec: "", args: [] };
  const [appId, setAppId] = useState(isEdit ? editingId : "");
  const [exec, setExec] = useState(initial?.exec || "");
  const [displayName, setDisplayName] = useState(initial?.display_name || "");
  const [args, setArgs] = useState(initial?.args || []);
  const [relayEnabled, setRelayEnabled] = useState(!!initial?.relay);
  const [relayExec, setRelayExec] = useState(initial?.relay?.exec || "");
  const [relayArgs, setRelayArgs] = useState(initial?.relay?.args || []);
  const [relayBlocking, setRelayBlocking] = useState(!!initial?.relay?.blocking);
  const [uploadMethod, setUploadMethod] = useState(initial?.relay?.method || "put");
  const [fileField, setFileField] = useState(initial?.relay?.file_field || "");
  const [formFields, setFormFields] = useState(initial?.relay?.form_fields || []);
  const [execHint, setExecHint] = useState("");
  const [execWarn, setExecWarn] = useState(false);

  const appIdHint = isEdit ? "app-id is immutable after creation" : "must be unique";

  const submit = async (event) => {
    event.preventDefault();
    const trimmedId = appId.trim();
    const trimmedExec = exec.trim();
    if (!trimmedId || !trimmedExec) return;
    if (!isEdit && Object.prototype.hasOwnProperty.call(apps, trimmedId)) {
      alert(`An application with id "${trimmedId}" already exists.`);
      return;
    }

    const definition = { exec: trimmedExec, args: args.filter(Boolean) };
    if (displayName.trim()) definition.display_name = displayName.trim();
    if (relayEnabled) {
      const relay = {};
      if (relayExec.trim()) relay.exec = relayExec.trim();
      const filteredRelayArgs = relayArgs.filter(Boolean);
      if (filteredRelayArgs.length) relay.args = filteredRelayArgs;
      if (relayBlocking) relay.blocking = true;
      if (uploadMethod !== "put") relay.method = uploadMethod;
      if (uploadMethod === "multipart") {
        if (!fileField.trim()) {
          alert("Multipart uploads need a file field name.");
          return;
        }
        relay.file_field = fileField.trim();
        const fields = formFields.filter((field) => field.name.trim().length > 0);
        if (fields.length) relay.form_fields = fields;
      }
      definition.relay = relay;
    }

    try {
      await onSave(trimmedId, definition);
    } catch (e) {
      alert(`Save failed: ${e}`);
    }
  };

  const checkExec = async () => {
    const path = exec.trim();
    if (!path) {
      setExecHint("");
      setExecWarn(false);
      return;
    }
    const exists = await invoke("exec_exists", { path });
    setExecHint(exists ? "" : "⚠ path not found — you can still save (app may be installed later)");
    setExecWarn(!exists);
  };

  return (
    <div className="fixed inset-0 z-10 flex items-center justify-center bg-black/60">
      <div className="max-h-[88vh] w-[480px] overflow-y-auto rounded-[10px] border border-app-border bg-app-panel p-5">
        <h3 className="mb-4 mt-0 text-base font-semibold">{isEdit ? `Edit "${editingId}"` : "Add Application"}</h3>
        <form onSubmit={submit}>
          <label className="mb-3 block">
            app-id
            <input className={INPUT_CLASS} disabled={isEdit} value={appId} onChange={(event) => setAppId(event.target.value)} required autoComplete="off" />
            <small className={HINT_CLASS}>{appIdHint}</small>
          </label>
          <label className="mb-3 block">
            exec
            <span className="mt-1 flex items-center gap-2">
              <input className={INLINE_INPUT_CLASS} value={exec} onBlur={checkExec} onChange={(event) => setExec(event.target.value)} required autoComplete="off" />
              <Button type="button" onClick={async () => {
                const selected = await openDialog({ multiple: false, directory: false });
                if (typeof selected === "string") setExec(selected);
              }}>
                Browse...
              </Button>
            </span>
            <small className={execWarn ? HINT_WARN_CLASS : HINT_CLASS}>{execHint}</small>
          </label>
          <label className="mb-3 block">
            display name (optional)
            <input className={INPUT_CLASS} value={displayName} onChange={(event) => setDisplayName(event.target.value)} autoComplete="off" />
          </label>
          <ArgEditor title="args" args={args} setArgs={setArgs}>
            Placeholders: <Code>{"{file}"}</Code>, <Code>{"{arg}"}</Code>, or named like <Code>{"{key}"}</Code>.
          </ArgEditor>
          <div className="mb-3 rounded-lg border border-app-border px-3 py-2.5">
            <label className="flex items-center gap-2">
              <input checked={relayEnabled} onChange={(event) => setRelayEnabled(event.target.checked)} type="checkbox" />
              Enable relay for this application
            </label>
            {relayEnabled && (
              <div className="mt-3">
                <label className="mb-3 block">
                  relay exec (optional, falls back to exec)
                  <input className={INPUT_CLASS} value={relayExec} onChange={(event) => setRelayExec(event.target.value)} autoComplete="off" />
                </label>
                <ArgEditor title="relay args (optional)" args={relayArgs} setArgs={setRelayArgs}>
                  <Code>{"{file}"}</Code> is the downloaded file. <Code>{"{src}"}</Code>, <Code>{"{dest}"}</Code>, and <Code>{"{filename}"}</Code> are supplied by the <Code>relay://</Code> URL and handled automatically — don't add them here.
                </ArgEditor>
                <label className="mb-3 flex items-center gap-2">
                  <input checked={relayBlocking} onChange={(event) => setRelayBlocking(event.target.checked)} type="checkbox" />
                  blocking (app blocks until closed)
                </label>
                <label className="mb-3 block">
                  Upload method (on completion)
                  <select className={INPUT_CLASS} value={uploadMethod} onChange={(event) => setUploadMethod(event.target.value)}>
                    <option value="put">PUT — raw file body</option>
                    <option value="post">POST — raw file body</option>
                    <option value="multipart">POST — multipart/form-data</option>
                  </select>
                </label>
                {uploadMethod === "multipart" && (
                  <>
                    <label className="mb-3 block">
                      File field name
                      <input className={INPUT_CLASS} value={fileField} onChange={(event) => setFileField(event.target.value)} placeholder="file" autoComplete="off" />
                    </label>
                    <FieldEditor fields={formFields} setFields={setFormFields} />
                  </>
                )}
              </div>
            )}
          </div>
          <div className="mt-2 flex justify-end gap-2">
            <Button type="button" onClick={onClose}>
              Cancel
            </Button>
            <Button type="submit" variant="primary">
              Save
            </Button>
          </div>
        </form>
      </div>
    </div>
  );
}

function ArgEditor({ args, children, setArgs, title }) {
  return (
    <fieldset className="mb-3 rounded-lg border border-app-border px-3 py-2.5">
      <legend className="px-1 text-xs text-app-muted">{title}</legend>
      <div className="mb-2 flex flex-col gap-1.5">
        {args.map((arg, index) => (
          <div className="flex gap-1.5" key={index}>
            <input
              className={INLINE_INPUT_CLASS}
              value={arg}
              onChange={(event) => updateArray(setArgs, index, event.target.value)}
              placeholder="literal or {placeholder}"
            />
            <Button size="small" type="button" variant="danger" onClick={() => removeArrayItem(setArgs, index)}>
              ✕
            </Button>
          </div>
        ))}
      </div>
      <Button size="small" type="button" onClick={() => setArgs((current) => [...current, ""])}>
        + Add arg
      </Button>
      <small className={HINT_CLASS}>{children}</small>
    </fieldset>
  );
}

function FieldEditor({ fields, setFields }) {
  return (
    <fieldset className="mb-3 rounded-lg border border-app-border px-3 py-2.5">
      <legend className="px-1 text-xs text-app-muted">Additional form fields (optional)</legend>
      <div className="mb-2 flex flex-col gap-1.5">
        {fields.map((field, index) => (
          <div className="flex gap-1.5" key={index}>
            <input
              className="w-[38%] rounded-md border border-app-border bg-app-bg px-2.5 py-1.5 text-[13px] text-app-text"
              value={field.name}
              onChange={(event) => updateField(setFields, index, "name", event.target.value)}
              placeholder="name"
            />
            <input
              className={INLINE_INPUT_CLASS}
              value={field.value}
              onChange={(event) => updateField(setFields, index, "value", event.target.value)}
              placeholder="value"
            />
            <Button size="small" type="button" variant="danger" onClick={() => removeArrayItem(setFields, index)}>
              ✕
            </Button>
          </div>
        ))}
      </div>
      <Button size="small" type="button" onClick={() => setFields((current) => [...current, { name: "", value: "" }])}>
        + Add field
      </Button>
    </fieldset>
  );
}

function UrlImportModal({ value, onChange, onClose, onSubmit }) {
  return (
    <div className="fixed inset-0 z-10 flex items-center justify-center bg-black/60">
      <div className="w-[380px] rounded-[10px] border border-app-border bg-app-panel p-5">
        <h3 className="mb-4 mt-0 text-base font-semibold">Import from URL</h3>
        <form onSubmit={onSubmit}>
          <label className="mb-3 block">
            Configuration URL (https)
            <input
              className={INPUT_CLASS}
              type="url"
              value={value}
              onChange={(event) => onChange(event.target.value)}
              placeholder="https://example.com/apps.json"
              autoComplete="off"
              required
            />
          </label>
          <p className={HINT_CLASS}>Imported apps are merged into your current definitions; you'll be asked how to resolve any conflicts.</p>
          <div className="mt-2 flex justify-end gap-2">
            <Button type="button" onClick={onClose}>
              Cancel
            </Button>
            <Button type="submit" variant="primary">
              Import
            </Button>
          </div>
        </form>
      </div>
    </div>
  );
}

function Code({ children }) {
  return <code className="rounded bg-app-bg px-1 py-px text-[11px]">{children}</code>;
}

async function sessionCmd(cmd, id) {
  try {
    await invoke(cmd, { id });
  } catch (e) {
    alert(`Action failed: ${e}`);
  }
}

function confirmCancel(session) {
  if (confirm(`Discard relay session for "${session.filename}"? The edited file will not be uploaded.`)) {
    sessionCmd("session_cancel", session.id);
  }
}

function exampleUri(id, def) {
  if (def.relay) return `launcher://relay/${id}?src={src}&dest={dest}&filename={filename}`;
  const params = [];
  const seen = new Set();
  for (const tok of def.args || []) {
    for (const match of String(tok).match(/\{(\w+)\}/g) || []) {
      const name = match.slice(1, -1);
      if (seen.has(name)) continue;
      seen.add(name);
      if (name === "file") params.push("file=/path/to/file");
      else if (name === "arg") params.push("arg=--example");
      else params.push(`${name}=value`);
    }
  }
  return `launcher://run/${id}${params.length ? `?${params.join("&")}` : ""}`;
}

async function cliPreview(def) {
  try {
    return await invoke("preview_cli_call", { definition: def });
  } catch (e) {
    console.error(e);
    return exampleUri("", def);
  }
}

async function resolveAndCommit(preview, refreshApps) {
  const replaceIds = [];
  for (const id of preview.conflicts) {
    const replace = confirm(
      `An application "${id}" already exists.\n\n` +
        `OK — Replace it with the imported definition\n` +
        `Cancel — Keep your existing definition`,
    );
    if (replace) replaceIds.push(id);
  }
  await invoke("commit_import", { imported: preview.imported, replaceIds });
  await refreshApps();
}

function isHttpsUrl(str) {
  try {
    return new URL(str).protocol === "https:";
  } catch {
    return false;
  }
}

function formatLogTime(value) {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value || "";
  return date.toLocaleString();
}

function updateArray(setter, index, value) {
  setter((current) => current.map((item, itemIndex) => (itemIndex === index ? value : item)));
}

function updateField(setter, index, key, value) {
  setter((current) =>
    current.map((item, itemIndex) => (itemIndex === index ? { ...item, [key]: value } : item)),
  );
}

function removeArrayItem(setter, index) {
  setter((current) => current.filter((_, itemIndex) => itemIndex !== index));
}

createRoot(document.getElementById("root")).render(<App />);
