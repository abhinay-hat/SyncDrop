const invoke = (cmd, args) => window.__TAURI__.core.invoke(cmd, args || {});

let state = null;
let excludes = [];
let formDirty = false;
let logTimer = 0;
let toastTimer = 0;

const FIELD_TAB = {
  source: "general",
  dest: "general",
  ssd: "general",
  nasHost: "network",
  nasShare: "network",
  excludes: "excludes",
};

function active() {
  if (!state) return null;
  return state.profiles.find((p) => p.id === state.activeProfileId) || state.profiles[0] || null;
}

function emptyToNull(value) {
  const trimmed = value.trim();
  return trimmed ? trimmed : null;
}

function toast(message, bad) {
  const el = document.getElementById("toast");
  el.textContent = message;
  el.classList.toggle("bad", !!bad);
  el.hidden = false;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => { el.hidden = true; }, 2800);
}

function clearErrors() {
  document.querySelectorAll(".field-error").forEach((el) => { el.textContent = ""; });
}

function showIssues(err) {
  const text = String(err);
  let issues = null;
  try { issues = JSON.parse(text); } catch (_) { issues = null; }
  if (!Array.isArray(issues)) {
    toast(text, true);
    return;
  }
  clearErrors();
  let tab = null;
  for (const issue of issues) {
    const slot = document.querySelector(`[data-for="${issue.field}"]`);
    if (slot) slot.textContent = issue.message;
    if (!tab && FIELD_TAB[issue.field]) tab = FIELD_TAB[issue.field];
  }
  if (tab) selectTab(tab);
  toast(issues[0] ? issues[0].message : "Could not save", true);
}

function relative(unix) {
  if (!unix) return "No sync yet";
  const delta = Math.max(0, Date.now() / 1000 - unix);
  if (delta < 45) return "Last sync just now";
  if (delta < 3600) return `Last sync ${Math.floor(delta / 60)} min ago`;
  if (delta < 86400) return `Last sync ${Math.floor(delta / 3600)} hr ago`;
  return `Last sync ${Math.floor(delta / 86400)} d ago`;
}

function statusOf(profile) {
  const on = profile.id === state.activeProfileId;
  if (!on) return "idle";
  if (state.syncing || (state.progress && state.progress.state === "running")) return "syncing";
  if (state.progress && state.progress.state === "error") return "error";
  if (state.connected) return "connected";
  return "idle";
}

function selectTab(id) {
  document.querySelectorAll(".tabs button").forEach((button) => {
    const on = button.dataset.tab === id;
    button.setAttribute("aria-selected", on ? "true" : "false");
  });
  document.querySelectorAll(".panel").forEach((panel) => {
    panel.classList.toggle("active", panel.id === id);
  });
  if (id === "logs") refreshLogs();
}

function paintChips() {
  const box = document.getElementById("chips");
  box.replaceChildren();
  for (const pattern of excludes) {
    const chip = document.createElement("span");
    chip.className = "chip";
    const label = document.createElement("span");
    label.textContent = pattern;
    const remove = document.createElement("button");
    remove.type = "button";
    remove.textContent = "Remove";
    remove.setAttribute("aria-label", `Remove ${pattern}`);
    remove.onclick = () => {
      excludes = excludes.filter((item) => item !== pattern);
      formDirty = true;
      paintChips();
    };
    chip.append(label, remove);
    box.append(chip);
  }
}

function paintSidebar() {
  const list = document.getElementById("profile-list");
  list.replaceChildren();
  for (const item of state.profiles) {
    const row = document.createElement("button");
    row.type = "button";
    row.className = "profile-row";
    row.setAttribute("role", "option");
    row.setAttribute("aria-selected", item.id === state.activeProfileId ? "true" : "false");
    const dot = document.createElement("span");
    dot.className = `dot ${statusOf(item)}`;
    const name = document.createElement("span");
    name.className = "name";
    name.textContent = item.name;
    row.append(dot, name);
    row.onclick = async () => {
      if (item.id === state.activeProfileId) return;
      try {
        await invoke("activate_profile", { id: item.id });
        formDirty = false;
        await reload(true);
      } catch (err) {
        toast(String(err), true);
      }
    };
    list.append(row);
  }
}

function paintHeader() {
  const profile = active();
  document.getElementById("profile-heading").textContent = profile ? profile.name : "No profile";
  const drive = document.getElementById("drive-state");
  if (state.connected) {
    const mount = state.mount && !state.mount.startsWith("gdrive:") ? state.mount : "connected";
    drive.textContent = `Drive connected · ${mount}`;
  } else {
    drive.textContent = "Drive not connected";
  }
  const profileName = profile ? profile.name : "";
  const last = (state.history || []).find((row) => !row.profileName || row.profileName === profileName);
  document.getElementById("last-sync").textContent = last ? relative(last.at) : "No sync yet";
  document.getElementById("eject").disabled = !state.connected || !state.mount || state.mount.startsWith("gdrive:");
  const busy = !!(state.syncing || state.previewing);
  document.getElementById("sync-now").disabled = busy;
  document.getElementById("dry-run").disabled = busy;
}

function paintForm() {
  const profile = active();
  document.getElementById("loading").hidden = true;
  if (!profile) return;
  document.getElementById("profile-name").value = profile.name || "";
  document.getElementById("source").value = profile.sourcePath || "";
  document.getElementById("dest").value = profile.destPath || "";
  document.getElementById("ssd").value = profile.ssdName || "";
  document.getElementById("nas-host").value = profile.nasHost || "";
  document.getElementById("nas-share").value = profile.nasShare || "";
  document.getElementById("nas-user").value = profile.nasUsername || "";
  document.getElementById("auto-mount").checked = !!profile.autoMountNas;
  document.getElementById("auto-sync").checked = !!profile.autoSync;
  document.getElementById("mirror").checked = !!profile.mirrorMode;
  document.getElementById("auto-eject").checked = !!profile.autoEject;
  document.getElementById("keep-versions").checked = !!profile.keepVersions;
  document.getElementById("notify").checked = !!state.notifyOnComplete;
  document.getElementById("launch").checked = !!state.launchAtLogin;
  document.getElementById("client-id").value = state.googleClientId || "";
  document.getElementById("gfolder").value = state.googleFolder || "SyncDrop";
  document.getElementById("google-state").textContent = state.googleSignedIn
    ? `Signed in${state.googleEmail ? ` · ${state.googleEmail}` : ""}`
    : "Not signed in";
  document.getElementById("secret-state").textContent = state.googleHasSecret
    ? "A client secret is in the keychain."
    : "No client secret saved.";
  excludes = (profile.excludes || []).slice();
  paintChips();
  paintHistory();
}

function paintHistory() {
  const body = document.getElementById("history-body");
  body.replaceChildren();
  for (const row of state.history || []) {
    const tr = document.createElement("tr");
    const cells = [
      row.date,
      row.profileName || "—",
      String(row.fileCount),
      row.size,
      row.duration,
    ];
    for (const text of cells) {
      const td = document.createElement("td");
      td.textContent = text;
      tr.append(td);
    }
    const status = document.createElement("td");
    const badge = document.createElement("span");
    badge.className = row.succeeded ? "badge ok" : "badge bad";
    badge.textContent = row.succeeded ? "OK" : "Failed";
    status.append(badge);
    tr.append(status);
    body.append(tr);
  }
  document.getElementById("history-empty").hidden = (state.history || []).length > 0;
}

function profileFromForm() {
  const profile = { ...active() };
  profile.name = document.getElementById("profile-name").value.trim() || profile.name;
  profile.sourcePath = document.getElementById("source").value.trim();
  profile.destPath = document.getElementById("dest").value.trim();
  profile.ssdName = document.getElementById("ssd").value.trim();
  profile.nasHost = emptyToNull(document.getElementById("nas-host").value);
  profile.nasShare = emptyToNull(document.getElementById("nas-share").value);
  profile.nasUsername = emptyToNull(document.getElementById("nas-user").value);
  profile.autoMountNas = document.getElementById("auto-mount").checked;
  profile.excludes = excludes.slice();
  profile.autoSync = document.getElementById("auto-sync").checked;
  profile.mirrorMode = document.getElementById("mirror").checked;
  profile.autoEject = document.getElementById("auto-eject").checked;
  profile.keepVersions = document.getElementById("keep-versions").checked;
  return profile;
}

async function reload(full) {
  state = await invoke("get_state");
  paintSidebar();
  paintHeader();
  if (full || !formDirty) paintForm();
  else paintHistory();
}

async function saveProfile() {
  clearErrors();
  await invoke("save_profile", { profile: profileFromForm() });
  formDirty = false;
  await reload(true);
  toast("Saved");
}

async function pickFolder(title) {
  const picked = await invoke("plugin:dialog|open", {
    options: { directory: true, multiple: false, title },
  });
  if (!picked) return null;
  const path = typeof picked === "string" ? picked : picked.path || picked[0];
  if (!path) return null;
  return invoke("prefer_home_path", { path });
}

function addExclude() {
  const input = document.getElementById("exclude-input");
  const pattern = input.value.trim();
  if (!pattern || excludes.includes(pattern)) {
    input.value = "";
    return;
  }
  excludes.push(pattern);
  input.value = "";
  formDirty = true;
  paintChips();
}

function levelRank(line) {
  if (/\bERROR\b/.test(line)) return "error";
  if (/\bWARN\b/.test(line)) return "warn";
  return "info";
}

async function refreshLogs() {
  const view = document.getElementById("log-view");
  if (!view || !document.getElementById("logs").classList.contains("active")) return;
  let lines = [];
  try {
    lines = await invoke("get_recent_logs", { limit: 400 });
  } catch (err) {
    document.getElementById("log-empty").hidden = false;
    document.getElementById("log-empty").textContent = String(err);
    return;
  }
  const min = document.getElementById("log-level").value;
  const query = document.getElementById("log-search").value.trim().toLowerCase();
  const rank = { info: 0, warn: 1, error: 2 };
  const shown = lines.filter((line) => {
    if (rank[levelRank(line)] < rank[min]) return false;
    if (query && !line.toLowerCase().includes(query)) return false;
    return true;
  });
  view.replaceChildren();
  for (const line of shown) {
    const row = document.createElement("div");
    row.className = `log-line ${levelRank(line)}`;
    row.textContent = line;
    view.append(row);
  }
  document.getElementById("log-empty").hidden = shown.length > 0;
  document.getElementById("log-empty").textContent = "No log lines yet.";
}

document.querySelectorAll(".tabs button").forEach((button) => {
  button.onclick = () => selectTab(button.dataset.tab);
});

document.querySelectorAll("input, select").forEach((el) => {
  if (el.id === "log-search" || el.id === "log-level" || el.id === "new-name" || el.id === "exclude-input") return;
  el.addEventListener("input", () => { formDirty = true; });
  el.addEventListener("change", () => { formDirty = true; });
});

document.getElementById("add-form").onsubmit = async (event) => {
  event.preventDefault();
  const name = document.getElementById("new-name").value.trim() || "Profile";
  try {
    await invoke("add_profile", { name });
    document.getElementById("new-name").value = "";
    formDirty = false;
    await reload(true);
  } catch (err) {
    toast(String(err), true);
  }
};

document.getElementById("pick-source").onclick = async () => {
  const path = await pickFolder("Choose source folder");
  if (path) {
    document.getElementById("source").value = path;
    formDirty = true;
  }
};

document.getElementById("pick-dest").onclick = async () => {
  const path = await pickFolder("Choose destination folder");
  if (path) {
    document.getElementById("dest").value = path;
    formDirty = true;
  }
};

document.getElementById("pick-gdrive").onclick = async () => {
  const path = await pickFolder("Choose a folder inside Google Drive");
  if (!path) return;
  document.getElementById("dest").value = path;
  const ssd = document.getElementById("ssd");
  if (!ssd.value || ssd.value === "Extreme Pro") ssd.value = "Google Drive";
  formDirty = true;
};

document.getElementById("use-cloud").onclick = () => {
  const folder = document.getElementById("gfolder").value.trim() || "SyncDrop";
  document.getElementById("dest").value = `Google Drive Cloud: /${folder}`;
  document.getElementById("ssd").value = "Google Drive (Cloud)";
  formDirty = true;
};

document.getElementById("save-general").onclick = () => saveProfile().catch(showIssues);
document.getElementById("save-excludes").onclick = () => saveProfile().catch(showIssues);
document.getElementById("save-network").onclick = () => saveProfile().catch(showIssues);

document.getElementById("save-behavior").onclick = async () => {
  try {
    await invoke("set_notify_on_complete", { value: document.getElementById("notify").checked });
    await invoke("set_launch_at_login", { value: document.getElementById("launch").checked });
    await saveProfile();
  } catch (err) {
    showIssues(err);
  }
};

document.getElementById("delete-profile").onclick = async () => {
  const profile = active();
  if (!profile) return;
  const before = state.profiles.length;
  try {
    await invoke("delete_profile", { id: profile.id });
    formDirty = false;
    await reload(true);
    toast(state.profiles.length < before ? "Profile deleted" : "The last profile stays");
  } catch (err) {
    toast(String(err), true);
  }
};

document.getElementById("set-password").onclick = async () => {
  try {
    await saveProfile();
    await invoke("set_nas_password", { password: document.getElementById("nas-password").value });
    document.getElementById("nas-password").value = "";
    toast("Password saved in the keychain");
  } catch (err) {
    showIssues(err);
  }
};

document.getElementById("clear-password").onclick = async () => {
  try {
    await invoke("delete_nas_password");
    document.getElementById("nas-password").value = "";
    toast("Password removed");
  } catch (err) {
    toast(String(err), true);
  }
};

document.getElementById("save-google").onclick = async () => {
  try {
    await invoke("set_google_client_id", { value: document.getElementById("client-id").value });
    const secret = document.getElementById("client-secret").value;
    if (secret) await invoke("set_google_client_secret", { value: secret });
    await invoke("set_google_folder", { value: document.getElementById("gfolder").value });
    document.getElementById("client-secret").value = "";
    await reload(false);
    toast("Google credentials saved");
  } catch (err) {
    toast(String(err), true);
  }
};

document.getElementById("use-folder").onclick = async () => {
  try {
    await invoke("set_google_folder", { value: document.getElementById("gfolder").value });
    document.getElementById("use-cloud").click();
    await saveProfile();
    toast("This profile syncs to Google Drive");
  } catch (err) {
    showIssues(err);
  }
};

document.getElementById("sign-in").onclick = async () => {
  try {
    await invoke("set_google_client_id", { value: document.getElementById("client-id").value });
    const secret = document.getElementById("client-secret").value;
    if (secret) await invoke("set_google_client_secret", { value: secret });
    toast("Waiting for the browser…");
    await invoke("google_sign_in");
  } catch (err) {
    toast(String(err), true);
  }
};

document.getElementById("sign-out").onclick = async () => {
  try {
    await invoke("google_sign_out");
    await reload(false);
    toast("Signed out");
  } catch (err) {
    toast(String(err), true);
  }
};

document.getElementById("open-console").onclick = () => {
  invoke("open_external", { url: "https://console.cloud.google.com/apis/credentials" })
    .catch((err) => toast(String(err), true));
};

document.getElementById("clear-history").onclick = async () => {
  try {
    await invoke("clear_history");
    await reload(false);
  } catch (err) {
    toast(String(err), true);
  }
};

document.getElementById("sync-now").onclick = () => {
  invoke("sync_now").catch(showIssues);
};

document.getElementById("dry-run").onclick = () => {
  invoke("dry_run").catch(showIssues);
};

document.getElementById("eject").onclick = () => {
  invoke("eject").then(() => reload(false)).catch((err) => toast(String(err), true));
};

document.getElementById("add-exclude").onclick = addExclude;
document.getElementById("exclude-input").addEventListener("keydown", (event) => {
  if (event.key === "Enter") {
    event.preventDefault();
    addExclude();
  }
});

document.getElementById("log-level").onchange = refreshLogs;
document.getElementById("log-search").oninput = refreshLogs;
document.getElementById("open-logs").onclick = () => {
  invoke("open_logs_folder").catch((err) => toast(String(err), true));
};

window.__TAURI__.event.listen("google-status", (event) => {
  const payload = event.payload || {};
  toast(payload.ok ? "Signed in to Google Drive" : (payload.error || "Sign-in failed"), !payload.ok);
  reload(false).catch((err) => toast(String(err), true));
});

logTimer = setInterval(refreshLogs, 2000);
setInterval(() => {
  if (!state) return;
  reload(false).catch(() => {});
}, 3000);

reload(true).catch((err) => {
  document.getElementById("loading").textContent = String(err);
  toast(String(err), true);
});
