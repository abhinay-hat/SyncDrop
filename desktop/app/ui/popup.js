const invoke = (cmd, args) => window.__TAURI__.core.invoke(cmd, args || {});

let closeTimer = 0;

function bytes(n) {
  const value = Math.max(0, Number(n) || 0);
  const units = ["bytes", "KB", "MB", "GB", "TB"];
  let size = value;
  let unit = 0;
  while (size >= 1000 && unit < units.length - 1) {
    size /= 1000;
    unit += 1;
  }
  if (unit === 0) return `${Math.round(size)} ${units[unit]}`;
  return `${size < 10 ? size.toFixed(1) : Math.round(size)} ${units[unit]}`;
}

function eta(seconds) {
  if (seconds == null || !Number.isFinite(seconds) || seconds < 0) return "—";
  const s = Math.round(seconds);
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m ${s % 60}s`;
  return `${Math.floor(m / 60)}h ${m % 60}m`;
}

function middle(text, max) {
  const value = text || "";
  if (value.length <= max) return value;
  const keep = Math.floor((max - 1) / 2);
  return `${value.slice(0, keep)}…${value.slice(value.length - keep)}`;
}

function clearClose() {
  clearTimeout(closeTimer);
  closeTimer = 0;
}

function showDry(dry) {
  clearClose();
  document.getElementById("progress-view").hidden = true;
  document.getElementById("dry-view").hidden = false;
  const groups = [
    { key: "copies", title: "Copy", cls: "" },
    { key: "updates", title: "Update", cls: "" },
    { key: "deletes", title: "Delete", cls: "delete" },
  ];
  const root = document.getElementById("groups");
  root.replaceChildren();
  let total = 0;
  for (const group of groups) {
    const paths = dry[group.key] || [];
    total += paths.length;
    const section = document.createElement("section");
    section.className = `group ${group.cls}`.trim();
    const heading = document.createElement("h3");
    heading.textContent = `${group.title} · ${paths.length}`;
    const list = document.createElement("ul");
    for (const path of paths) {
      const li = document.createElement("li");
      li.textContent = path;
      list.append(li);
    }
    section.append(heading, list);
    root.append(section);
  }
  const err = document.getElementById("dry-error");
  err.textContent = dry.error || (total === 0 ? "Nothing to copy." : "");
  document.getElementById("dry-sync").disabled = !!dry.error || total === 0;
}

function showProgress(state) {
  const dry = state.dryRun;
  if (dry && !state.syncing) {
    showDry(dry);
    return;
  }
  document.getElementById("dry-view").hidden = true;
  document.getElementById("progress-view").hidden = false;
  const profile = (state.profiles || []).find((p) => p.id === state.activeProfileId);
  document.getElementById("heading").textContent = profile ? profile.name : "SyncDrop";
  document.getElementById("sub").textContent = state.connected
    ? (state.mount && !String(state.mount).startsWith("gdrive:") ? state.mount : "Destination connected")
    : "Not connected";
  const progress = state.progress || {};
  const pct = Math.round(progress.percent || 0);
  document.getElementById("bar").value = pct;
  document.getElementById("percent").textContent = `${pct}%`;
  const running = progress.state === "running" || state.syncing || state.previewing;
  const done = progress.state === "done";
  const failed = progress.state === "error" || progress.state === "interrupted";
  if (state.previewing) {
    document.getElementById("files").textContent = "Preview";
    document.getElementById("current").textContent = "";
  } else {
    document.getElementById("files").textContent = `${progress.filesDone || 0} / ${progress.filesTotal || 0}`;
    document.getElementById("current").textContent = middle(progress.currentFile || "", 64);
  }
  document.getElementById("bytes").textContent = `${bytes(progress.bytesDone)} / ${bytes(progress.bytesTotal)}`;
  document.getElementById("speed").textContent = running && progress.speedBps > 0 ? `${bytes(progress.speedBps)}/s` : "—";
  document.getElementById("eta").textContent = running ? eta(progress.etaSecs) : "—";
  document.getElementById("message").textContent = progress.detail || "";
  const summary = document.getElementById("summary");
  if (done) {
    const took = progress.durationSecs != null ? ` in ${eta(progress.durationSecs)}` : "";
    summary.hidden = false;
    summary.textContent = `Synced ${progress.filesDone || 0} files, ${bytes(progress.bytesDone)}${took}.`;
  } else if (failed) {
    summary.hidden = false;
    summary.textContent = progress.detail || "Sync did not finish.";
  } else {
    summary.hidden = true;
    summary.textContent = "";
  }
  document.getElementById("cancel").hidden = !running;
  document.getElementById("dismiss").hidden = running;
  document.getElementById("retry").hidden = !failed;
  if (done && !closeTimer) {
    closeTimer = setTimeout(() => invoke("close_window"), 2500);
  }
  if (!done) clearClose();
}

async function refresh() {
  showProgress(await invoke("get_state"));
}

document.getElementById("cancel").onclick = () => invoke("cancel_sync").then(refresh);
document.getElementById("dismiss").onclick = () => invoke("close_window");
document.getElementById("retry").onclick = () => {
  clearClose();
  invoke("sync_now");
};
document.getElementById("dry-cancel").onclick = () => invoke("close_window");
document.getElementById("dry-sync").onclick = () => {
  clearClose();
  document.getElementById("dry-view").hidden = true;
  document.getElementById("progress-view").hidden = false;
  invoke("sync_now");
};

window.__TAURI__.event.listen("sync-progress", (event) => {
  invoke("get_state").then((state) => {
    state.progress = event.payload;
    state.syncing = event.payload.state === "running";
    state.dryRun = null;
    showProgress(state);
  });
});

window.__TAURI__.event.listen("dry-run", (event) => showDry(event.payload));

refresh();
