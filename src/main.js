// Frontend for the Launcher app: Relay Queue (§7.2) + Settings (§7.1).
// Talks to the Rust core over Tauri commands and subscribes to `relay:update`.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import { readText as readClipboard } from "@tauri-apps/plugin-clipboard-manager";

// ---------------------------------------------------------------------------
// View switching
// ---------------------------------------------------------------------------

function showView(view) {
  document.querySelectorAll(".view").forEach((el) => el.classList.remove("is-active"));
  document.querySelectorAll(".tab").forEach((el) => el.classList.remove("is-active"));
  document.getElementById(`view-${view}`)?.classList.add("is-active");
  document.querySelector(`.tab[data-view="${view}"]`)?.classList.add("is-active");
  if (view === "logs") refreshLogs().catch((e) => console.error(e));
}

document.querySelectorAll("[data-view]").forEach((el) => {
  el.addEventListener("click", () => showView(el.dataset.view));
});

// Tray navigation (§7.2): the Rust side emits "navigate" with a view name.
getCurrentWindow().listen("navigate", (e) => showView(e.payload));

// ---------------------------------------------------------------------------
// Relay Queue (§7.2)
// ---------------------------------------------------------------------------

const STATUS_LABELS = {
  downloading: "Downloading…",
  editing: "Editing",
  idle: "Idle (ready to upload)",
  uploading: "Uploading…",
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

function actionButton(label, cls, handler) {
  const b = document.createElement("button");
  b.className = `btn small ${cls || ""}`.trim();
  b.textContent = label;
  b.addEventListener("click", handler);
  return b;
}

// Action buttons by status (§7.2).
function renderActions(td, s) {
  td.innerHTML = "";
  const add = (...els) => els.forEach((e) => td.appendChild(e));

  switch (s.status) {
    case "editing":
      add(
        actionButton("Upload & Finish", "primary", () => sessionCmd("session_upload_finish", s.id)),
        actionButton("Cancel", "danger", () => confirmCancel(s)),
      );
      break;
    case "idle":
      add(
        actionButton("Upload & Finish", "primary", () => sessionCmd("session_upload_finish", s.id)),
        actionButton("Keep Editing", "", () => sessionCmd("session_keep_editing", s.id)),
        actionButton("Cancel", "danger", () => confirmCancel(s)),
      );
      break;
    case "uploading":
    case "downloading": {
      const spin = document.createElement("span");
      spin.className = "spinner";
      add(spin);
      break;
    }
    case "error":
      add(
        actionButton("Retry", "primary", () => sessionCmd("session_retry", s.id)),
        actionButton("Cancel", "danger", () => confirmCancel(s)),
      );
      break;
    case "orphaned":
      add(
        actionButton("Resume", "primary", () => sessionCmd("session_resume", s.id)),
        actionButton("Upload", "", () => sessionCmd("session_upload_orphan", s.id)),
        actionButton("Discard", "danger", () => sessionCmd("session_discard", s.id)),
      );
      break;
    default:
      break;
  }
}

function confirmCancel(s) {
  if (confirm(`Discard relay session for "${s.filename}"? The edited file will not be uploaded.`)) {
    sessionCmd("session_cancel", s.id);
  }
}

async function sessionCmd(cmd, id) {
  try {
    await invoke(cmd, { id });
  } catch (e) {
    alert(`Action failed: ${e}`);
  }
}

function renderSessions(sessions) {
  const body = document.getElementById("sessions-body");
  const table = document.getElementById("sessions-table");
  const empty = document.getElementById("queue-empty");
  body.innerHTML = "";

  const visible = sessions.filter((s) => s.status !== "done");
  table.style.display = visible.length ? "" : "none";
  empty.style.display = visible.length ? "none" : "";

  for (const s of visible) {
    const tr = document.createElement("tr");

    const file = document.createElement("td");
    file.innerHTML = `<span class="status-icon status-${s.status}">${STATUS_ICONS[s.status] || "•"}</span> ${escapeHtml(s.filename)}`;
    tr.appendChild(file);

    const app = document.createElement("td");
    app.textContent = s.app_name || s.app_id;
    tr.appendChild(app);

    const status = document.createElement("td");
    status.textContent = STATUS_LABELS[s.status] || s.status;
    if (s.error) status.title = s.error;
    tr.appendChild(status);

    const actions = document.createElement("td");
    actions.className = "actions";
    renderActions(actions, s);
    tr.appendChild(actions);

    body.appendChild(tr);
  }
}

async function refreshSessions() {
  const sessions = await invoke("list_sessions");
  renderSessions(sessions);
}

// Live updates pushed from the Rust core (§7.2).
listen("relay:update", (e) => renderSessions(e.payload));


// ---------------------------------------------------------------------------
// Logs
// ---------------------------------------------------------------------------

const LOG_STATUS_LABELS = {
  handled: "Handled",
  launched: "Launched",
  error: "Error",
};

function renderLogs(logs) {
  const body = document.getElementById("logs-body");
  const table = document.getElementById("logs-table");
  const empty = document.getElementById("logs-empty");
  body.innerHTML = "";

  table.style.display = logs.length ? "" : "none";
  empty.style.display = logs.length ? "none" : "";

  for (const entry of logs) {
    const tr = document.createElement("tr");

    const time = document.createElement("td");
    time.textContent = formatLogTime(entry.created_at);
    tr.appendChild(time);

    const uri = document.createElement("td");
    uri.className = "log-mono";
    uri.textContent = entry.raw_uri;
    uri.title = entry.raw_uri;
    tr.appendChild(uri);

    const status = document.createElement("td");
    status.className = `log-status log-status-${entry.status}`;
    status.textContent = LOG_STATUS_LABELS[entry.status] || entry.status;
    if (entry.error) status.title = entry.error;
    tr.appendChild(status);

    const cli = document.createElement("td");
    cli.className = "log-mono";
    cli.textContent = entry.cli_call || "";
    cli.title = entry.cli_call || entry.error || "";
    tr.appendChild(cli);

    body.appendChild(tr);
  }
}

async function refreshLogs() {
  const logs = await invoke("list_logs");
  renderLogs(logs);
}

function formatLogTime(value) {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value || "";
  return date.toLocaleString();
}

// ---------------------------------------------------------------------------
// Settings — app definitions CRUD (§7.1)
// ---------------------------------------------------------------------------

let currentConfig = {};
let editingId = null; // null => creating a new app

// Build a representative full URI for an app. Relay-enabled apps show a
// `launcher://relay` URI with the special {src}/{dest}/{filename} placeholders
// the caller fills in (§2.2); otherwise a `launcher://run` URI derived from the
// args template's placeholders (§2.1).
function exampleUri(id, def) {
  if (def.relay) {
    return `launcher://relay/${id}?src={src}&dest={dest}&filename={filename}`;
  }
  const params = [];
  const seen = new Set();
  for (const tok of def.args || []) {
    for (const m of String(tok).match(/\{(\w+)\}/g) || []) {
      const name = m.slice(1, -1);
      if (seen.has(name)) continue;
      seen.add(name);
      if (name === "file") params.push("file=/path/to/file");
      else if (name === "arg") params.push("arg=--example");
      else params.push(`${name}=value`);
    }
  }
  const query = params.length ? `?${params.join("&")}` : "";
  return `launcher://run/${id}${query}`;
}

function renderAppList(config) {
  currentConfig = config;
  const list = document.getElementById("app-list");
  const empty = document.getElementById("settings-empty");
  const ids = Object.keys(config);
  list.innerHTML = "";

  list.style.display = ids.length ? "" : "none";
  empty.style.display = ids.length ? "none" : "";

  for (const id of ids) {
    const def = config[id];
    const li = document.createElement("li");
    li.className = "app-row";

    const info = document.createElement("div");
    info.className = "app-info";
    const hasRelay = def.relay ? '<span class="badge">relay</span>' : "";
    const example = exampleUri(id, def);
    const title = def.display_name
      ? `${escapeHtml(def.display_name)} [${escapeHtml(id)}]`
      : escapeHtml(id);
    info.innerHTML =
      `<div class="app-line">` +
        `<span class="app-id">${title}</span>` +
        `<span class="app-exec" title="${escapeHtml(def.exec)}">${escapeHtml(def.exec)}</span>` +
        hasRelay +
      `</div>` +
      `<div class="app-uri" title="${escapeHtml(example)}">${escapeHtml(example)}</div>`;
    info.addEventListener("click", () => openForm(id));
    li.appendChild(info);

    const del = actionButton("Delete", "danger", async (ev) => {
      ev.stopPropagation();
      if (!confirm(`Delete application "${id}"?`)) return;
      try {
        const updated = await invoke("delete_app", { appId: id });
        renderAppList(updated);
      } catch (e) {
        alert(`${e}`);
      }
    });
    li.appendChild(del);

    list.appendChild(li);
  }
}

async function refreshApps() {
  const config = await invoke("list_apps");
  renderAppList(config);
}

// ---- arg list editors ----

function addArgRow(container, value = "") {
  const row = document.createElement("div");
  row.className = "arg-row";
  const input = document.createElement("input");
  input.type = "text";
  input.value = value;
  input.placeholder = "literal or {placeholder}";
  input.className = "arg-input";
  const rm = actionButton("✕", "danger", () => row.remove());
  row.append(input, rm);
  container.appendChild(row);
}

function readArgs(container) {
  return [...container.querySelectorAll(".arg-input")]
    .map((i) => i.value)
    .filter((v) => v.length > 0);
}

// ---- multipart form-field editor (name/value pairs) ----

function addFieldRow(container, name = "", value = "") {
  const row = document.createElement("div");
  row.className = "field-row";
  const nameInput = document.createElement("input");
  nameInput.type = "text";
  nameInput.value = name;
  nameInput.placeholder = "name";
  nameInput.className = "field-name";
  const valueInput = document.createElement("input");
  valueInput.type = "text";
  valueInput.value = value;
  valueInput.placeholder = "value";
  valueInput.className = "field-value";
  const rm = actionButton("✕", "danger", () => row.remove());
  row.append(nameInput, valueInput, rm);
  container.appendChild(row);
}

function readFields(container) {
  return [...container.querySelectorAll(".field-row")]
    .map((r) => ({
      name: r.querySelector(".field-name").value.trim(),
      value: r.querySelector(".field-value").value,
    }))
    .filter((f) => f.name.length > 0);
}

function setMultipartVisibility(method) {
  document.getElementById("multipart-fields").hidden = method !== "multipart";
}

// ---- form open/close ----

function openForm(id) {
  editingId = id;
  const isEdit = id !== null;
  document.getElementById("modal-title").textContent = isEdit ? `Edit "${id}"` : "Add Application";

  const appIdInput = document.getElementById("f-app-id");
  appIdInput.value = isEdit ? id : "";
  // app-id is immutable after creation (§7.1).
  appIdInput.disabled = isEdit;
  document.getElementById("app-id-hint").textContent = isEdit
    ? "app-id is immutable after creation"
    : "must be unique";

  const def = isEdit ? currentConfig[id] : { exec: "", args: [] };
  document.getElementById("f-exec").value = def.exec || "";
  document.getElementById("f-display").value = def.display_name || "";
  document.getElementById("exec-hint").textContent = "";

  const argsList = document.getElementById("args-list");
  argsList.innerHTML = "";
  (def.args || []).forEach((a) => addArgRow(argsList, a));

  // Relay is explicitly on/off via the enable checkbox.
  const relay = def.relay || null;
  document.getElementById("f-relay-enabled").checked = !!relay;
  document.getElementById("relay-fields").hidden = !relay;
  document.getElementById("f-relay-exec").value = relay?.exec || "";
  document.getElementById("f-relay-blocking").checked = !!relay?.blocking;
  const relayArgsList = document.getElementById("relay-args-list");
  relayArgsList.innerHTML = "";
  (relay?.args || []).forEach((a) => addArgRow(relayArgsList, a));

  const method = relay?.method || "put";
  document.getElementById("f-upload-method").value = method;
  setMultipartVisibility(method);
  document.getElementById("f-file-field").value = relay?.file_field || "";
  const fieldsList = document.getElementById("form-fields-list");
  fieldsList.innerHTML = "";
  (relay?.form_fields || []).forEach((f) => addFieldRow(fieldsList, f.name, f.value));

  document.getElementById("modal").hidden = false;
}

function closeForm() {
  document.getElementById("modal").hidden = true;
  editingId = null;
}

async function submitForm(ev) {
  ev.preventDefault();
  const appId = document.getElementById("f-app-id").value.trim();
  const exec = document.getElementById("f-exec").value.trim();
  if (!appId || !exec) return;

  // Uniqueness check for new entries (§7.1).
  if (editingId === null && Object.prototype.hasOwnProperty.call(currentConfig, appId)) {
    alert(`An application with id "${appId}" already exists.`);
    return;
  }

  const args = readArgs(document.getElementById("args-list"));
  const display = document.getElementById("f-display").value.trim();

  const definition = { exec, args };
  if (display) definition.display_name = display;

  // Persist a relay block only when relay is explicitly enabled.
  if (document.getElementById("f-relay-enabled").checked) {
    const relay = {};
    const relayExec = document.getElementById("f-relay-exec").value.trim();
    const relayArgs = readArgs(document.getElementById("relay-args-list"));
    if (relayExec) relay.exec = relayExec;
    if (relayArgs.length) relay.args = relayArgs;
    if (document.getElementById("f-relay-blocking").checked) relay.blocking = true;

    const method = document.getElementById("f-upload-method").value;
    if (method !== "put") relay.method = method;

    if (method === "multipart") {
      const fileField = document.getElementById("f-file-field").value.trim();
      if (!fileField) {
        alert("Multipart uploads need a file field name.");
        return;
      }
      relay.file_field = fileField;
      const formFields = readFields(document.getElementById("form-fields-list"));
      if (formFields.length) relay.form_fields = formFields;
    }

    definition.relay = relay;
  }

  try {
    const updated = await invoke("save_app", { appId, definition });
    renderAppList(updated);
    closeForm();
  } catch (e) {
    alert(`Save failed: ${e}`);
  }
}

// ---- wiring ----

document.getElementById("add-app").addEventListener("click", () => openForm(null));
document.getElementById("add-app-empty").addEventListener("click", () => openForm(null));
document.getElementById("export-apps").addEventListener("click", exportConfig);
document.getElementById("import-apps").addEventListener("click", importConfig);
document.getElementById("import-url").addEventListener("click", openUrlImport);
document.getElementById("cancel-url-import").addEventListener("click", closeUrlImport);
document.getElementById("url-form").addEventListener("submit", submitUrlImport);

async function openUrlImport() {
  const input = document.getElementById("f-import-url");
  input.value = "";
  document.getElementById("url-modal").hidden = false;
  input.focus();

  // Auto-fill from the clipboard when it holds a valid https URL.
  try {
    const clip = (await readClipboard())?.trim();
    if (clip && isHttpsUrl(clip)) {
      input.value = clip;
      input.select();
    }
  } catch {
    // Clipboard empty or unavailable — leave the field blank.
  }
}

function isHttpsUrl(str) {
  try {
    return new URL(str).protocol === "https:";
  } catch {
    return false;
  }
}

function closeUrlImport() {
  document.getElementById("url-modal").hidden = true;
}

// Import app definitions from an HTTPS URL, replacing the current config.
async function submitUrlImport(ev) {
  ev.preventDefault();
  const url = document.getElementById("f-import-url").value.trim();
  if (!url) return;
  if (!/^https:\/\//i.test(url)) {
    alert("URL must start with https://");
    return;
  }
  try {
    const preview = await invoke("import_config_from_url", { url });
    await resolveAndCommit(preview);
    closeUrlImport();
  } catch (e) {
    alert(`Import failed: ${e}`);
  }
}

// Export the app definitions to a user-chosen JSON file.
async function exportConfig() {
  try {
    const path = await saveDialog({
      defaultPath: "launcher-apps.json",
      filters: [{ name: "JSON", extensions: ["json"] }],
    });
    if (!path) return;
    await invoke("export_config", { path });
  } catch (e) {
    alert(`Export failed: ${e}`);
  }
}

// Import app definitions from a JSON file, merging into the current config.
async function importConfig() {
  try {
    const selected = await openDialog({
      multiple: false,
      directory: false,
      filters: [{ name: "JSON", extensions: ["json"] }],
    });
    if (typeof selected !== "string") return;
    const preview = await invoke("import_config", { path: selected });
    await resolveAndCommit(preview);
  } catch (e) {
    alert(`Import failed: ${e}`);
  }
}

// Given an import preview, ask the user how to resolve each conflicting app-id,
// then merge the import into the current config.
async function resolveAndCommit(preview) {
  const replaceIds = [];
  for (const id of preview.conflicts) {
    const replace = confirm(
      `An application "${id}" already exists.\n\n` +
        `OK — Replace it with the imported definition\n` +
        `Cancel — Keep your existing definition`,
    );
    if (replace) replaceIds.push(id);
  }
  const merged = await invoke("commit_import", {
    imported: preview.imported,
    replaceIds,
  });
  renderAppList(merged);
}
document.getElementById("cancel-edit").addEventListener("click", closeForm);
document.getElementById("app-form").addEventListener("submit", submitForm);
document.getElementById("add-arg").addEventListener("click", () =>
  addArgRow(document.getElementById("args-list")),
);
document.getElementById("add-relay-arg").addEventListener("click", () =>
  addArgRow(document.getElementById("relay-args-list")),
);
document.getElementById("add-form-field").addEventListener("click", () =>
  addFieldRow(document.getElementById("form-fields-list")),
);
document.getElementById("f-relay-enabled").addEventListener("change", (e) => {
  document.getElementById("relay-fields").hidden = !e.target.checked;
});
document.getElementById("f-upload-method").addEventListener("change", (e) => {
  setMultipartVisibility(e.target.value);
});

document.getElementById("pick-exec").addEventListener("click", async () => {
  const selected = await openDialog({ multiple: false, directory: false });
  if (typeof selected === "string") {
    document.getElementById("f-exec").value = selected;
  }
});

// Non-fatal exec existence check on blur (§7.1).
document.getElementById("f-exec").addEventListener("blur", async (e) => {
  const path = e.target.value.trim();
  const hint = document.getElementById("exec-hint");
  if (!path) {
    hint.textContent = "";
    return;
  }
  const exists = await invoke("exec_exists", { path });
  hint.textContent = exists ? "" : "⚠ path not found — you can still save (app may be installed later)";
  hint.className = exists ? "hint" : "hint warn";
});

function escapeHtml(str) {
  return String(str).replace(/[&<>"']/g, (c) => ({
    "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;",
  })[c]);
}

// ---------------------------------------------------------------------------
// Boot
// ---------------------------------------------------------------------------

refreshSessions().catch((e) => console.error(e));
refreshApps().catch((e) => console.error(e));
refreshLogs().catch((e) => console.error(e));
