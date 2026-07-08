import React, { useCallback, useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import {
  Dialog,
  DialogPanel,
  DialogTitle,
  Field,
  Fieldset,
  Input,
  Label,
  Legend,
  Listbox,
  ListboxButton,
  ListboxOption,
  ListboxOptions,
  Select,
} from "@headlessui/react";
import {
  ArrowDownTrayIcon,
  ArrowPathIcon,
  ArrowUpTrayIcon,
  CheckCircleIcon,
  ChevronUpDownIcon,
  CloudArrowDownIcon,
  CloudArrowUpIcon,
  ComputerDesktopIcon,
  DocumentIcon,
  ExclamationTriangleIcon,
  FolderOpenIcon,
  MoonIcon,
  PencilSquareIcon,
  PlusIcon,
  SunIcon,
  TrashIcon,
  XMarkIcon,
} from "@heroicons/react/24/outline";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  open as openDialog,
  save as saveDialog,
} from "@tauri-apps/plugin-dialog";
import { readText as readClipboard } from "@tauri-apps/plugin-clipboard-manager";

const TAB_BASE =
  "rounded-md px-3 py-1.5 text-sm text-app-muted transition-colors hover:bg-app-panel-2 hover:text-app-text";
const BUTTON_BASE =
  "inline-flex items-center justify-center rounded-md border px-3 py-1.5 text-[13px] transition disabled:cursor-not-allowed disabled:opacity-60";
const BUTTON_DEFAULT =
  "border-app-border bg-app-panel-2 text-app-text hover:bg-app-button-hover";
const BUTTON_SMALL = "px-2 py-0.5 text-xs";
const BUTTON_PRIMARY =
  "border-app-accent bg-app-accent text-app-on-accent hover:brightness-110";
const BUTTON_DANGER =
  "border-app-danger bg-app-danger text-app-on-accent hover:brightness-110";
const INPUT_CLASS =
  "mt-1 w-full rounded-md border border-app-border bg-app-bg px-2.5 py-1.5 text-[13px] text-app-text disabled:opacity-60";
const INLINE_INPUT_CLASS =
  "min-w-0 flex-1 rounded-md border border-app-border bg-app-bg px-2.5 py-1.5 text-[13px] text-app-text";
const HINT_CLASS = "mt-1 block text-[11px] text-app-muted";
const HINT_WARN_CLASS = "mt-1 block text-[11px] text-app-warn";
const TH_CLASS =
  "border-b border-app-border px-2.5 py-2 text-left text-xs font-semibold uppercase tracking-wide text-app-muted";
const TD_CLASS = "p-2.5 align-middle";
const MONO_TRUNCATE = "truncate font-mono text-xs";
const THEME_STORAGE_KEY = "launcher.themeMode";

const THEME_OPTIONS = [
  { id: "system", label: "System", icon: ComputerDesktopIcon },
  { id: "light", label: "Light", icon: SunIcon },
  { id: "dark", label: "Dark", icon: MoonIcon },
];

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
  downloading: CloudArrowDownIcon,
  editing: PencilSquareIcon,
  idle: ArrowPathIcon,
  uploading: CloudArrowUpIcon,
  done: CheckCircleIcon,
  error: ExclamationTriangleIcon,
  orphaned: ArrowPathIcon,
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

function Button({
  children,
  className = "",
  icon: Icon = null,
  size = "normal",
  variant = "default",
  ...props
}) {
  const variantClass =
    variant === "primary"
      ? BUTTON_PRIMARY
      : variant === "danger"
        ? BUTTON_DANGER
        : BUTTON_DEFAULT;
  const sizeClass = size === "small" ? BUTTON_SMALL : "";
  return (
    <button
      className={`${BUTTON_BASE} ${sizeClass} ${variantClass} ${className}`.trim()}
      {...props}
    >
      {Icon && <Icon aria-hidden="true" className="mr-1.5 size-4 shrink-0" />}
      {children}
    </button>
  );
}

function App() {
  const [view, setView] = useState("queue");
  const [themeMode, setThemeMode] = useState(readThemeMode);
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
      .listen("navigate", (event) => setView(normalizeView(event.payload)))
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

  useEffect(() => {
    applyThemeMode(themeMode);
    try {
      localStorage.setItem(THEME_STORAGE_KEY, themeMode);
    } catch {
      // Theme selection can still apply for this session.
    }
    if (themeMode !== "system") return undefined;
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const onChange = () => applyThemeMode("system");
    media.addEventListener("change", onChange);
    return () => media.removeEventListener("change", onChange);
  }, [themeMode]);

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
    if (!confirm(`Delete handler "${appId}"?`)) return;
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
      await resolveAndCommit(
        await invoke("import_config", { path: selected }),
        refreshApps,
      );
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
      await resolveAndCommit(
        await invoke("import_config_from_url", { url }),
        refreshApps,
      );
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
    <div className="min-h-screen bg-app-bg text-sm text-app-text">
      <TopBar
        setThemeMode={setThemeMode}
        setView={setView}
        themeMode={themeMode}
        view={view}
      />
      <main className="p-4">
        {view === "queue" && <RelayQueue sessions={sessions} />}
        {view === "handlers" && (
          <HandlersView
            appRows={appRows}
            deleteApp={deleteApp}
            exportConfig={exportConfig}
            importConfig={importConfig}
            openForm={openForm}
            openUrlImport={openUrlImport}
          />
        )}
        {view === "logs" && (
          <LogsView
            logs={logs}
            clearLogs={clearLogs}
            refreshLogs={refreshLogs}
          />
        )}
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

function TopBar({ setThemeMode, setView, themeMode, view }) {
  return (
    <header className="flex items-center gap-4 border-b border-app-border bg-app-panel px-4 py-2.5">
      <div className="font-bold tracking-wide">Launcher</div>
      <nav className="ml-2 flex gap-1">
        {[
          ["queue", "Relay Queue"],
          ["handlers", "Handlers"],
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
      <div className="ml-auto">
        <ThemeModeSelect mode={themeMode} setMode={setThemeMode} />
      </div>
    </header>
  );
}

function ThemeModeSelect({ mode, setMode }) {
  const selected =
    THEME_OPTIONS.find((option) => option.id === mode) ?? THEME_OPTIONS[0];
  const SelectedIcon = selected.icon;
  return (
    <Listbox onChange={setMode} value={mode}>
      <div className="relative">
        <ListboxButton className={`${BUTTON_BASE} ${BUTTON_DEFAULT}`}>
          <SelectedIcon aria-hidden="true" className="mr-1.5 size-4 shrink-0" />
          {selected.label}
          <ChevronUpDownIcon
            aria-hidden="true"
            className="ml-1.5 size-4 shrink-0 text-app-muted"
          />
        </ListboxButton>
        <ListboxOptions className="absolute right-0 z-20 mt-1 w-36 rounded-md border border-app-border bg-app-panel p-1 shadow-lg">
          {THEME_OPTIONS.map((option) => {
            const Icon = option.icon;
            return (
              <ListboxOption
                className="flex cursor-pointer items-center rounded px-2 py-1.5 text-sm text-app-text data-focus:bg-app-panel-2"
                key={option.id}
                value={option.id}
              >
                <Icon aria-hidden="true" className="mr-2 size-4 shrink-0" />
                {option.label}
              </ListboxOption>
            );
          })}
        </ListboxOptions>
      </div>
    </Listbox>
  );
}

function RelayQueue({ sessions }) {
  const visible = sessions.filter((session) => session.status !== "done");
  if (!visible.length) {
    return (
      <div className="px-4 py-12 text-center text-app-muted">
        No active relay sessions
      </div>
    );
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
              <StatusIcon status={session.status} />
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

function StatusIcon({ status }) {
  const Icon = STATUS_ICONS[status] || DocumentIcon;
  return (
    <Icon
      aria-hidden="true"
      className={`mr-1 inline-block size-5 align-text-bottom ${STATUS_COLORS[status] || ""}`}
    />
  );
}

function SessionActions({ session }) {
  const run = (cmd) => sessionCmd(cmd, session.id);
  switch (session.status) {
    case "editing":
      return (
        <>
          <Button
            icon={CloudArrowUpIcon}
            size="small"
            variant="primary"
            onClick={() => run("session_upload_finish")}
          >
            Upload & Finish
          </Button>
          <Button
            icon={XMarkIcon}
            size="small"
            variant="danger"
            onClick={() => confirmCancel(session)}
          >
            Cancel
          </Button>
        </>
      );
    case "idle":
      return (
        <>
          <Button
            icon={CloudArrowUpIcon}
            size="small"
            variant="primary"
            onClick={() => run("session_upload_finish")}
          >
            Upload & Finish
          </Button>
          <Button
            icon={PencilSquareIcon}
            size="small"
            onClick={() => run("session_keep_editing")}
          >
            Keep Editing
          </Button>
          <Button
            icon={XMarkIcon}
            size="small"
            variant="danger"
            onClick={() => confirmCancel(session)}
          >
            Cancel
          </Button>
        </>
      );
    case "uploading":
    case "downloading":
      return (
        <span className="inline-block size-4 animate-spin rounded-full border-2 border-app-border border-t-app-accent" />
      );
    case "error":
      return (
        <>
          <Button
            icon={ArrowPathIcon}
            size="small"
            variant="primary"
            onClick={() => run("session_retry")}
          >
            Retry
          </Button>
          <Button
            icon={XMarkIcon}
            size="small"
            variant="danger"
            onClick={() => confirmCancel(session)}
          >
            Cancel
          </Button>
        </>
      );
    case "orphaned":
      return (
        <>
          <Button
            icon={ArrowPathIcon}
            size="small"
            variant="primary"
            onClick={() => run("session_resume")}
          >
            Resume
          </Button>
          <Button
            icon={CloudArrowUpIcon}
            size="small"
            onClick={() => run("session_upload_orphan")}
          >
            Upload
          </Button>
          <Button
            icon={TrashIcon}
            size="small"
            variant="danger"
            onClick={() => run("session_discard")}
          >
            Discard
          </Button>
        </>
      );
    default:
      return null;
  }
}

function HandlersView({
  appRows,
  deleteApp,
  exportConfig,
  importConfig,
  openForm,
  openUrlImport,
}) {
  return (
    <>
      <div className="mb-3 flex items-center justify-between">
        <h2 className="m-0 text-base font-semibold">Registered handlers</h2>
        <div className="flex gap-2">
          <Button icon={ArrowDownTrayIcon} onClick={importConfig}>
            Import File...
          </Button>
          <Button icon={CloudArrowDownIcon} onClick={openUrlImport}>
            Import URL...
          </Button>
          <Button icon={ArrowUpTrayIcon} onClick={exportConfig}>
            Export...
          </Button>
          <Button
            icon={PlusIcon}
            variant="primary"
            onClick={() => openForm(null)}
          >
            Add Handler
          </Button>
        </div>
      </div>
      {appRows.length ? (
        <ul className="m-0 list-none p-0">
          {appRows.map(({ id, def, uriExample, commandExample }) => (
            <li
              className="mb-2 flex items-center justify-between gap-3 rounded-lg border border-app-border bg-app-panel px-3 py-2.5"
              key={id}
            >
              <button
                className="min-w-0 flex-1 cursor-pointer overflow-hidden text-left"
                onClick={() => openForm(id)}
              >
                <div className="flex min-w-0 items-center gap-2.5">
                  <span className="shrink-0 font-semibold">
                    {def.display_name ? `${def.display_name} [${id}]` : id}
                  </span>
                  <span
                    className="min-w-0 flex-1 truncate text-xs text-app-muted"
                    title={def.exec}
                  >
                    {def.exec}
                  </span>
                  {def.relay && (
                    <span className="rounded-full bg-app-accent px-1.5 py-px text-[10px] uppercase tracking-wide text-app-on-accent">
                      relay
                    </span>
                  )}
                </div>
                <div
                  className={`mt-1 ${MONO_TRUNCATE} text-app-muted`}
                  title={uriExample}
                >
                  {uriExample}
                </div>
                <div
                  className={`mt-1 ${MONO_TRUNCATE} text-app-text`}
                  title={commandExample}
                >
                  {commandExample}
                </div>
              </button>
              <Button
                icon={TrashIcon}
                size="small"
                variant="danger"
                onClick={() => deleteApp(id)}
              >
                Delete
              </Button>
            </li>
          ))}
        </ul>
      ) : (
        <div className="px-4 py-12 text-center text-app-muted">
          <p>No handlers registered yet</p>
          <Button
            className="mt-3"
            icon={PlusIcon}
            variant="primary"
            onClick={() => openForm(null)}
          >
            Add Handler
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
          <Button
            disabled={!logs.length}
            icon={TrashIcon}
            variant="danger"
            onClick={clearLogs}
          >
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
                <td
                  className="break-words p-2.5 align-middle font-mono text-xs"
                  title={entry.raw_uri}
                >
                  {entry.raw_uri}
                </td>
                <td
                  className={`p-2.5 align-middle font-semibold ${LOG_STATUS_COLORS[entry.status] || ""}`}
                  title={entry.error || ""}
                >
                  {LOG_STATUS_LABELS[entry.status] || entry.status}
                </td>
                <td
                  className="break-words p-2.5 align-middle font-mono text-xs"
                  title={entry.cli_call || entry.error || ""}
                >
                  {entry.cli_call || ""}
                </td>
                <td className="space-x-1.5 whitespace-nowrap p-2.5 text-right align-middle">
                  <Button
                    icon={ArrowPathIcon}
                    size="small"
                    variant="primary"
                    onClick={() => rerunLogEntry(entry.id)}
                  >
                    Re-run
                  </Button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      ) : (
        <div className="px-4 py-12 text-center text-app-muted">
          No handled URIs logged yet
        </div>
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
  const [relayBlocking, setRelayBlocking] = useState(
    !!initial?.relay?.blocking,
  );
  const [uploadMethod, setUploadMethod] = useState(
    initial?.relay?.method || "put",
  );
  const [fileField, setFileField] = useState(initial?.relay?.file_field || "");
  const [formFields, setFormFields] = useState(
    initial?.relay?.form_fields || [],
  );
  const [execHint, setExecHint] = useState("The application to be run.");
  const [execWarn, setExecWarn] = useState(false);

  const appIdHint = isEdit
    ? "App ID cannot be changed after creation."
    : "Must be unique. Used to route requests to the selected application.";

  const submit = async (event) => {
    event.preventDefault();
    const trimmedId = appId.trim();
    const trimmedExec = exec.trim();
    if (!trimmedId || !trimmedExec) return;
    if (!isEdit && Object.prototype.hasOwnProperty.call(apps, trimmedId)) {
      alert(`A handler with id "${trimmedId}" already exists.`);
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
        const fields = formFields.filter(
          (field) => field.name.trim().length > 0,
        );
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
    setExecHint(
      exists
        ? ""
        : "⚠ path not found — you can still save (app may be installed later)",
    );
    setExecWarn(!exists);
  };

  return (
    <Dialog className="relative z-10" onClose={onClose} open>
      <div className="fixed inset-0 bg-app-overlay/60" aria-hidden="true" />
      <div className="fixed inset-0 flex items-center justify-center">
        <DialogPanel className="max-h-[88vh] w-[480px] overflow-y-auto rounded-[10px] border border-app-border bg-app-panel p-5">
          <DialogTitle className="mb-4 mt-0 text-base font-semibold">
            {isEdit ? `Edit "${editingId}"` : "Add Handler"}
          </DialogTitle>
          <form onSubmit={submit}>
            <Fieldset className="space-y-3">
              <Legend className="sr-only">Handler details</Legend>
              <Field>
                <Label className="block">Handler ID</Label>
                <Input
                  className={INPUT_CLASS}
                  disabled={isEdit}
                  value={appId}
                  onChange={(event) => setAppId(event.target.value)}
                  required
                  autoComplete="off"
                />
                <small className={HINT_CLASS}>{appIdHint}</small>
              </Field>
              <Field>
                <Label className="block">Application</Label>
                <span className="mt-1 flex items-center gap-2">
                  <Input
                    className={INLINE_INPUT_CLASS}
                    value={exec}
                    onBlur={checkExec}
                    onChange={(event) => setExec(event.target.value)}
                    required
                    autoComplete="off"
                  />
                  <Button
                    icon={FolderOpenIcon}
                    type="button"
                    onClick={async () => {
                      const selected = await openDialog({
                        multiple: false,
                        directory: false,
                      });
                      if (typeof selected === "string") setExec(selected);
                    }}
                  >
                    Browse...
                  </Button>
                </span>
                <small className={execWarn ? HINT_WARN_CLASS : HINT_CLASS}>
                  {execHint}
                </small>
              </Field>
              <Field>
                <Label className="block">Display Name (optional)</Label>
                <Input
                  className={INPUT_CLASS}
                  value={displayName}
                  onChange={(event) => setDisplayName(event.target.value)}
                  autoComplete="off"
                />
              </Field>
              <ArgEditor
                title="Application Arguments"
                args={args}
                setArgs={setArgs}
              >
                Placeholders: <Code>{"{file}"}</Code>, <Code>{"{arg}"}</Code>,
                or named like <Code>{"{key}"}</Code>.
              </ArgEditor>
              <Fieldset className="rounded-lg border border-app-border px-3 py-2.5">
                <Legend className="sr-only">Relay options</Legend>
                <Field className="flex items-center gap-2">
                  <Input
                    checked={relayEnabled}
                    onChange={(event) => setRelayEnabled(event.target.checked)}
                    type="checkbox"
                  />
                  <Label>Enable relay for this handler</Label>
                </Field>
                {relayEnabled && (
                  <div className="mt-3 space-y-3">
                    <Field>
                      <Label className="block">
                        Relay Application (optional, falls back to Application)
                      </Label>
                      <Input
                        className={INPUT_CLASS}
                        value={relayExec}
                        onChange={(event) => setRelayExec(event.target.value)}
                        autoComplete="off"
                      />
                    </Field>
                    <ArgEditor
                      title="Application Arguments (optional)"
                      args={relayArgs}
                      setArgs={setRelayArgs}
                    >
                      <Code>{"{file}"}</Code> is the downloaded file.{" "}
                      <Code>{"{src}"}</Code>, <Code>{"{dest}"}</Code>, and{" "}
                      <Code>{"{filename}"}</Code> are supplied by the{" "}
                      <Code>relay://</Code> URL and handled automatically —
                      don't add them here.
                    </ArgEditor>
                    <Field className="flex items-center gap-2">
                      <Input
                        checked={relayBlocking}
                        onChange={(event) =>
                          setRelayBlocking(event.target.checked)
                        }
                        type="checkbox"
                      />
                      <Label>blocking (app blocks until closed)</Label>
                    </Field>
                    <Field>
                      <Label className="block">
                        Upload method (on completion)
                      </Label>
                      <Select
                        className={INPUT_CLASS}
                        value={uploadMethod}
                        onChange={(event) =>
                          setUploadMethod(event.target.value)
                        }
                      >
                        <option value="put">PUT — raw file body</option>
                        <option value="post">POST — raw file body</option>
                        <option value="multipart">
                          POST — multipart/form-data
                        </option>
                      </Select>
                    </Field>
                    {uploadMethod === "multipart" && (
                      <>
                        <Field>
                          <Label className="block">File field name</Label>
                          <Input
                            className={INPUT_CLASS}
                            value={fileField}
                            onChange={(event) =>
                              setFileField(event.target.value)
                            }
                            placeholder="file"
                            autoComplete="off"
                          />
                        </Field>
                        <FieldEditor
                          fields={formFields}
                          setFields={setFormFields}
                        />
                      </>
                    )}
                  </div>
                )}
              </Fieldset>
            </Fieldset>
            <div className="mt-3 flex justify-end gap-2">
              <Button icon={XMarkIcon} type="button" onClick={onClose}>
                Cancel
              </Button>
              <Button icon={CheckCircleIcon} type="submit" variant="primary">
                Save
              </Button>
            </div>
          </form>
        </DialogPanel>
      </div>
    </Dialog>
  );
}

function ArgEditor({ args, children, setArgs, title }) {
  return (
    <Fieldset className="rounded-lg border border-app-border px-3 py-2.5">
      <Legend className="px-1 text-xs text-app-muted">{title}</Legend>
      <div className="mb-2 flex flex-col gap-1.5">
        {args.map((arg, index) => (
          <div className="flex gap-1.5" key={index}>
            <Field className="min-w-0 flex-1">
              <Label className="sr-only">
                {title} {index + 1}
              </Label>
              <Input
                className={INLINE_INPUT_CLASS}
                value={arg}
                onChange={(event) =>
                  updateArray(setArgs, index, event.target.value)
                }
                placeholder="literal or {placeholder}"
              />
            </Field>
            <Button
              size="small"
              type="button"
              variant="danger"
              onClick={() => removeArrayItem(setArgs, index)}
            >
              ✕
            </Button>
          </div>
        ))}
      </div>
      <Button
        icon={PlusIcon}
        size="small"
        type="button"
        onClick={() => setArgs((current) => [...current, ""])}
      >
        Add arg
      </Button>
      <small className={HINT_CLASS}>{children}</small>
    </Fieldset>
  );
}

function FieldEditor({ fields, setFields }) {
  return (
    <Fieldset className="rounded-lg border border-app-border px-3 py-2.5">
      <Legend className="px-1 text-xs text-app-muted">
        Additional form fields (optional)
      </Legend>
      <div className="mb-2 flex flex-col gap-1.5">
        {fields.map((field, index) => (
          <div className="flex gap-1.5" key={index}>
            <Field className="w-[38%]">
              <Label className="sr-only">Field name {index + 1}</Label>
              <Input
                className="w-full rounded-md border border-app-border bg-app-bg px-2.5 py-1.5 text-[13px] text-app-text"
                value={field.name}
                onChange={(event) =>
                  updateField(setFields, index, "name", event.target.value)
                }
                placeholder="name"
              />
            </Field>
            <Field className="min-w-0 flex-1">
              <Label className="sr-only">Field value {index + 1}</Label>
              <Input
                className={INLINE_INPUT_CLASS}
                value={field.value}
                onChange={(event) =>
                  updateField(setFields, index, "value", event.target.value)
                }
                placeholder="value"
              />
            </Field>
            <Button
              size="small"
              type="button"
              variant="danger"
              onClick={() => removeArrayItem(setFields, index)}
            >
              ✕
            </Button>
          </div>
        ))}
      </div>
      <Button
        icon={PlusIcon}
        size="small"
        type="button"
        onClick={() =>
          setFields((current) => [...current, { name: "", value: "" }])
        }
      >
        + Add field
      </Button>
    </Fieldset>
  );
}

function UrlImportModal({ value, onChange, onClose, onSubmit }) {
  return (
    <Dialog className="relative z-10" onClose={onClose} open>
      <div className="fixed inset-0 bg-app-overlay/60" aria-hidden="true" />
      <div className="fixed inset-0 flex items-center justify-center">
        <DialogPanel className="w-[380px] rounded-[10px] border border-app-border bg-app-panel p-5">
          <DialogTitle className="mb-4 mt-0 text-base font-semibold">
            Import from URL
          </DialogTitle>
          <form onSubmit={onSubmit}>
            <Fieldset>
              <Legend className="sr-only">Import settings</Legend>
              <Field className="mb-3">
                <Label className="block">Configuration URL (https)</Label>
                <Input
                  className={INPUT_CLASS}
                  type="url"
                  value={value}
                  onChange={(event) => onChange(event.target.value)}
                  placeholder="https://example.com/apps.json"
                  autoComplete="off"
                  required
                />
              </Field>
            </Fieldset>
            <p className={HINT_CLASS}>
              Imported handlers are merged into your current definitions; you'll
              be asked how to resolve any conflicts.
            </p>
            <div className="mt-2 flex justify-end gap-2">
              <Button icon={XMarkIcon} type="button" onClick={onClose}>
                Cancel
              </Button>
              <Button icon={ArrowDownTrayIcon} type="submit" variant="primary">
                Import
              </Button>
            </div>
          </form>
        </DialogPanel>
      </div>
    </Dialog>
  );
}
function Code({ children }) {
  return (
    <code className="rounded bg-app-bg px-1 py-px text-[11px]">{children}</code>
  );
}

async function sessionCmd(cmd, id) {
  try {
    await invoke(cmd, { id });
  } catch (e) {
    alert(`Action failed: ${e}`);
  }
}

function confirmCancel(session) {
  if (
    confirm(
      `Discard relay session for "${session.filename}"? The edited file will not be uploaded.`,
    )
  ) {
    sessionCmd("session_cancel", session.id);
  }
}

function exampleUri(id, def) {
  if (def.relay)
    return `launcher://relay/${id}?src={src}&dest={dest}&filename={filename}`;
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
      `A handler "${id}" already exists.\n\n` +
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

function normalizeView(view) {
  return view === "settings" ? "handlers" : view;
}

function readThemeMode() {
  if (typeof localStorage === "undefined") return "system";
  try {
    const stored = localStorage.getItem(THEME_STORAGE_KEY);
    return THEME_OPTIONS.some((option) => option.id === stored)
      ? stored
      : "system";
  } catch {
    return "system";
  }
}

function applyThemeMode(mode) {
  if (typeof window === "undefined") return;
  const resolved =
    mode === "system"
      ? window.matchMedia("(prefers-color-scheme: dark)").matches
        ? "dark"
        : "light"
      : mode;
  document.documentElement.dataset.theme = resolved;
  document.documentElement.dataset.themeMode = mode;
}

function updateArray(setter, index, value) {
  setter((current) =>
    current.map((item, itemIndex) => (itemIndex === index ? value : item)),
  );
}

function updateField(setter, index, key, value) {
  setter((current) =>
    current.map((item, itemIndex) =>
      itemIndex === index ? { ...item, [key]: value } : item,
    ),
  );
}

function removeArrayItem(setter, index) {
  setter((current) => current.filter((_, itemIndex) => itemIndex !== index));
}

applyThemeMode(readThemeMode());

createRoot(document.getElementById("root")).render(<App />);
