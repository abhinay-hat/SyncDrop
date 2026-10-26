const invoke = (cmd, args) => window.__TAURI__.core.invoke(cmd, args || {});

let state = null;
let selectedId = null;
let excludes = [];
let formDirty = false;
let logTimer = 0;
let toastTimer = 0;
let dialogWait = null;

const apple = /Mac|iPhone|iPad/.test(navigator.platform || navigator.userAgent || "");

const FIELD_TAB = {
  source: "sync",
  dest: "sync",
  ssd: "sync",
  nasHost: "sync",
  nasShare: "sync",
  excludes: "rules",
};

const GLYPH = {
  ok: '<circle cx="8" cy="8" r="5.2"/><path d="M5.4 8.2 7.1 9.9 10.7 6.2"/>',
  off: '<circle cx="8" cy="8" r="5.2"/>',
  sync: '<path d="M3.2 8a4.8 4.8 0 0 1 8.2-3.4M12.8 8a4.8 4.8 0 0 1-8.2 3.4"/>',
  fail: '<circle cx="8" cy="8" r="5.2"/><path d="M8 5.1v3.3M8 11.1h.01"/>',
};

function glyph(kind) {
  const holder = document.createElement("span");
  holder.innerHTML = `<svg viewBox="0 0 16 16" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">${GLYPH[kind] || GLYPH.off}</svg>`;
  return holder.firstChild;
}

function selected() {
  if (!state) return null;
  return state.profiles.find((profile) => profile.id === selectedId) || state.profiles[0] || null;
}

function liveOf(id) {
  return (state.profilesLive || []).find((row) => row.id === id) || null;
}

function lastOf(profile) {
  return (state.history || []).find((row) => row.profileId && row.profileId === profile.id) || null;
}

function currentTarget() {
  return document.querySelector('input[name="target"]:checked')?.value || "drive";
}

function targetOf(profile) {
  if (!profile) return "drive";
  const dest = profile.destPath || "";
  if (profile.ssdName === "Google Drive (Cloud)" || dest.startsWith("gdrive://") || /Google Drive \(Cloud\)|Google Drive Cloud:/i.test(dest)) {
    return "google";
  }
  if ((profile.nasHost || "").trim()) return "nas";
  return "drive";
}

function targetTitle(profile) {
  const target = targetOf(profile);
  if (target === "google") return "Google Drive";
  if (target === "nas") return profile.nasShare || profile.ssdName || profile.name || "Network share";
  return profile.ssdName || profile.name || "External drive";
}

function emptyToNull(value) {
  const trimmed = value.trim();
  return trimmed ? trimmed : null;
}

function checked(id) {
  return document.getElementById(id).getAttribute("aria-checked") === "true";
}

function setChecked(id, value) {
  document.getElementById(id).setAttribute("aria-checked", value ? "true" : "false");
}

function toast(message, bad) {
  const el = document.getElementById("toast");
  el.textContent = message;
  el.classList.toggle("bad", !!bad);
  el.hidden = false;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => { el.hidden = true; }, 3200);
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
  const fmt = (n, unit) => `${n.toLocaleString()} ${unit}${n === 1 ? "" : "s"} ago`;
  if (delta < 45) return "Last sync just now";
  if (delta < 3600) return `Last sync ${fmt(Math.floor(delta / 60), "minute")}`;
  if (delta < 86400) return `Last sync ${fmt(Math.floor(delta / 3600), "hour")}`;
  return `Last sync ${fmt(Math.floor(delta / 86400), "day")}`;
}

function bytes(n) {
  const value = Math.max(0, Number(n) || 0);
  const units = ["bytes", "KB", "MB", "GB", "TB"];
  let size = value;
  let unit = 0;
  while (size >= 1000 && unit < units.length - 1) {
    size /= 1000;
    unit += 1;
  }
  const shown = unit === 0 ? Math.round(size) : (size < 10 ? Number(size.toFixed(1)) : Math.round(size));
  return `${shown.toLocaleString()} ${units[unit]}`;
}

function eta(seconds) {
  if (seconds == null || !Number.isFinite(seconds) || seconds < 0) return "—";
  const s = Math.round(seconds);
  if (s < 60) return `${s.toLocaleString()}s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m.toLocaleString()}m ${(s % 60).toLocaleString()}s`;
  return `${Math.floor(m / 60).toLocaleString()}h ${(m % 60).toLocaleString()}m`;
}

function statusText(profile) {
  const running = state.syncing && state.syncingProfileId === profile.id;
  if (running && state.previewing) return "Preparing preview";
  if (running) return "Syncing";
  const live = liveOf(profile.id);
  return live && live.connected ? "Connected" : "Not connected";
}

function statusKind(text) {
  if (text === "Syncing" || text === "Preparing preview") return "sync";
  if (text === "Connected") return "ok";
  return "off";
}

function markDirty() {
  formDirty = true;
  showSavebar();
  if (state) paintHeader();
}

function showSavebar() {
  const prefs = !document.getElementById("prefs-main").hidden;
  document.getElementById("savebar").hidden = !formDirty || prefs;
}

function applyTarget(target) {
  document.querySelectorAll("[data-target]").forEach((el) => {
    el.hidden = !el.dataset.target.split(/\s+/).includes(target);
  });
  const destLabel = document.getElementById("dest-label");
  const destHelp = document.getElementById("dest-help");
  const ssdLabel = document.getElementById("ssd-label");
  const ssdHelp = document.getElementById("ssd-help");
  if (target === "nas") {
    destLabel.textContent = "Folder on the share";
    destHelp.textContent = "Where files land after the share is mounted.";
    ssdLabel.textContent = "Volume name";
    ssdHelp.textContent = "The name this computer gives the share. SyncDrop matches it when the share connects.";
  } else {
    destLabel.textContent = "Destination folder";
    destHelp.textContent = "Folder on the drive.";
    ssdLabel.textContent = "Drive name";
    ssdHelp.textContent = "The name of the physical disk SyncDrop watches for.";
  }
  const warn = document.getElementById("mirror-warn");
  warn.hidden = document.querySelector('input[name="mode"]:checked')?.value !== "mirror";
}

function selectTab(id) {
  document.querySelectorAll(".tabs [role=tab]").forEach((button) => {
    const on = button.dataset.tab === id;
    button.setAttribute("aria-selected", on ? "true" : "false");
    button.tabIndex = on ? 0 : -1;
  });
  for (const panel of ["sync", "rules", "activity", "about"]) {
    document.getElementById(panel).hidden = panel !== id;
  }
  if (id === "activity" && document.getElementById("logs-pane") && !document.getElementById("logs-pane").hidden) {
    refreshLogs();
  }
}

function selectActivity(id) {
  document.querySelectorAll("#activity [role=tab]").forEach((button) => {
    const on = button.dataset.activity === id;
    button.setAttribute("aria-selected", on ? "true" : "false");
    button.tabIndex = on ? 0 : -1;
  });
  document.getElementById("history-pane").hidden = id !== "history";
  document.getElementById("logs-pane").hidden = id !== "logs";
  if (id === "logs") refreshLogs();
}

function moveTabs(event, selector, read, choose) {
  const tabs = [...document.querySelectorAll(selector)];
  const index = tabs.indexOf(document.activeElement);
  if (index < 0) return;
  let next = null;
  if (event.key === "ArrowRight") next = tabs[(index + 1) % tabs.length];
  if (event.key === "ArrowLeft") next = tabs[(index - 1 + tabs.length) % tabs.length];
  if (event.key === "Home") next = tabs[0];
  if (event.key === "End") next = tabs[tabs.length - 1];
  if (!next) return;
  event.preventDefault();
  choose(read(next));
  next.focus();
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
      markDirty();
      paintChips();
    };
    chip.append(label, remove);
    box.append(chip);
  }
}

function selectionInside(root) {
  const sel = document.getSelection();
  if (!sel || sel.isCollapsed || !sel.anchorNode) return false;
  return root.contains(sel.anchorNode);
}

function paintSidebar() {
  const list = document.getElementById("profile-list");
  const ids = state.profiles.map((item) => item.id).join("\n");
  const same = list.dataset.ids === ids && list.childElementCount === state.profiles.length;
  if (!same) {
    const focusId = document.activeElement?.dataset?.profileId || "";
    list.replaceChildren();
    state.profiles.forEach((item, index) => list.append(profileOption(item, index)));
    list.dataset.ids = ids;
    if (focusId) {
      const again = list.querySelector(`[data-profile-id="${focusId}"]`);
      if (again) again.focus();
    }
    return;
  }
  state.profiles.forEach((item, index) => fillProfileOption(list.children[index], item, index));
}

function profileOption(item, index) {
  const row = document.createElement("button");
  row.type = "button";
  row.className = "profile-row";
  row.setAttribute("role", "option");
  row.dataset.profileId = item.id;
  const name = document.createElement("span");
  name.className = "name";
  const status = document.createElement("span");
  status.className = "status";
  row.append(name, status);
  row.onclick = () => selectProfile(item.id);
  fillProfileOption(row, item, index);
  return row;
}

function fillProfileOption(row, item, index) {
  const on = item.id === (selected()?.id);
  row.setAttribute("aria-selected", on ? "true" : "false");
  row.tabIndex = on || (index === 0 && !selected()) ? 0 : -1;
  row.querySelector(".name").textContent = item.name;
  const text = statusText(item);
  const active = item.id === state.activeProfileId;
  const label = active ? `${text} · Active` : text;
  const status = row.querySelector(".status");
  status.title = active ? "Tray menu actions use this profile" : "";
  if (status.dataset.label !== label) {
    status.replaceChildren(glyph(statusKind(text)), document.createTextNode(label));
    status.dataset.label = label;
  }
}

function blockReason(profile) {
  if (formDirty) return "Save changes first";
  const source = (profile.sourcePath || "").trim();
  const target = targetOf(profile);
  const dest = target === "google"
    ? ((state.googleFolder || "").trim() || "SyncDrop")
    : (profile.destPath || "").trim();
  if (!source) return "Choose a source folder first.";
  if (!dest) return "Choose a destination first.";
  if (target === "google" && !state.googleSignedIn) return "Sign in to Google Drive first.";
  if (target !== "google" && !(liveOf(profile.id) && liveOf(profile.id).connected)) {
    return target === "nas" ? "This share is not connected." : "This drive is not connected.";
  }
  if (state.syncing && state.syncingProfileId && state.syncingProfileId !== profile.id) {
    return "Another profile is syncing.";
  }
  return "";
}

function paintHeader() {
  const profile = selected();
  const heading = document.getElementById("profile-heading");
  const stateLine = document.getElementById("drive-state");
  if (!profile) {
    heading.textContent = "No profile";
    stateLine.textContent = "Not connected";
    document.getElementById("fail-reason").hidden = true;
    document.getElementById("sync-reason").hidden = true;
    document.getElementById("eject").hidden = true;
    document.getElementById("active-help").hidden = true;
    return;
  }
  heading.textContent = targetTitle(profile);
  const text = statusText(profile);
  stateLine.replaceChildren(glyph(statusKind(text)), document.createTextNode(text));
  const last = lastOf(profile);
  document.getElementById("last-sync").textContent = last ? relative(last.at) : "No sync yet";
  const live = liveOf(profile.id);
  const free = document.getElementById("free-space");
  free.textContent = live && live.freeBytes != null && targetOf(profile) === "drive"
    ? ` · ${bytes(live.freeBytes)} free`
    : "";
  const mine = state.syncing && state.syncingProfileId === profile.id && state.progress?.state === "running";
  const failed = !!(last && !last.succeeded && !mine);
  const failEl = document.getElementById("fail-reason");
  failEl.hidden = !failed;
  failEl.textContent = failed ? (last.error || "The last sync did not finish.") : "";
  const reason = blockReason(profile);
  const reasonEl = document.getElementById("sync-reason");
  reasonEl.hidden = !reason;
  reasonEl.textContent = reason;
  const progress = document.getElementById("hero-progress");
  if (mine) {
    const view = state.progress || {};
    const pct = Math.round(view.percent || 0).toLocaleString();
    const files = `${(view.filesDone || 0).toLocaleString()} / ${(view.filesTotal || 0).toLocaleString()} files`;
    progress.hidden = false;
    progress.textContent = `${pct}% · ${files} · ETA ${eta(view.etaSecs)}`;
  } else {
    progress.hidden = true;
  }
  const syncNow = document.getElementById("sync-now");
  syncNow.hidden = !!mine;
  syncNow.textContent = failed ? "Retry" : "Sync now";
  syncNow.disabled = !!reason || state.syncing;
  document.getElementById("cancel-sync").hidden = !mine;
  document.getElementById("dry-run").disabled = !!reason || state.syncing;
  const drive = targetOf(profile) === "drive";
  const ejectable = !!(drive && live && live.ejectable);
  const eject = document.getElementById("eject");
  eject.hidden = !drive;
  eject.disabled = !ejectable || state.syncing;
  eject.title = ejectable ? "Eject the drive" : "Nothing to eject";
  const make = document.getElementById("make-active");
  const isActive = profile.id === state.activeProfileId;
  make.hidden = isActive;
  document.getElementById("active-help").hidden = !isActive;
  document.getElementById("first-run").hidden = !!(profile.destPath || "").trim();
}

function paintForm() {
  const profile = selected();
  document.getElementById("loading").hidden = true;
  if (!profile) return;
  document.getElementById("profile-name").value = profile.name || "";
  document.getElementById("source").value = profile.sourcePath || "";
  document.getElementById("dest").value = profile.destPath || "";
  document.getElementById("ssd").value = profile.ssdName || "";
  document.getElementById("nas-host").value = profile.nasHost || "";
  document.getElementById("nas-share").value = profile.nasShare || "";
  document.getElementById("nas-user").value = profile.nasUsername || "";
  document.getElementById("nas-password").value = "";
  setChecked("auto-mount", !!profile.autoMountNas);
  setChecked("auto-sync", !!profile.autoSync);
  setChecked("auto-eject", !!profile.autoEject);
  setChecked("keep-versions", !!profile.keepVersions);
  setChecked("respect-gitignore", profile.respectGitignore !== false);
  setChecked("skip-deps", profile.skipDependencyCaches !== false);
  setChecked("keep-git", profile.keepGit !== false);
  const cleanupStatus = document.getElementById("cleanup-status");
  const cleanupApply = document.getElementById("cleanup-apply");
  if (cleanupStatus) {
    cleanupStatus.hidden = true;
    cleanupStatus.textContent = "";
  }
  if (cleanupApply) cleanupApply.hidden = true;
  const target = targetOf(profile);
  document.querySelectorAll('input[name="target"]').forEach((input) => {
    input.checked = input.value === target;
  });
  document.querySelectorAll('input[name="mode"]').forEach((input) => {
    input.checked = profile.mirrorMode ? input.value === "mirror" : input.value === "copy";
  });
  document.getElementById("client-id").value = state.googleClientId || "";
  document.getElementById("gfolder").value = state.googleFolder || "SyncDrop";
  document.getElementById("google-state").textContent = state.googleSignedIn
    ? `Signed in${state.googleEmail ? ` · ${state.googleEmail}` : ""}`
    : "Not signed in";
  document.getElementById("secret-state").textContent = state.googleHasSecret
    ? "A client secret is in the keychain."
    : "No client secret saved.";
  excludes = (profile.excludes || []).slice();
  applyTarget(target);
  paintChips();
  paintHistory();
  paintPrefs();
}

function paintPrefs() {
  setChecked("notify", !!state.notifyOnComplete);
  setChecked("launch", !!state.launchAtLogin);
}

function paintHistory() {
  const body = document.getElementById("history-body");
  if (selectionInside(body)) return;
  const profile = selected();
  const rows = (state.history || []).filter((row) => profile && row.profileId === profile.id);
  const signature = rows.map((row) => row.id).join("\n");
  if (body.dataset.signature === signature && body.childElementCount === rows.length * 2) {
    rows.forEach((row, index) => fillHistoryRow(body.children[index * 2], row));
    document.getElementById("history-empty").hidden = rows.length > 0;
    return;
  }
  const focusId = document.activeElement?.dataset?.recordId || "";
  body.replaceChildren();
  body.dataset.signature = signature;
  for (const row of rows) {
    const built = historyRow(row);
    body.append(built.tr, built.extra);
  }
  document.getElementById("history-empty").hidden = rows.length > 0;
  if (focusId) {
    const again = body.querySelector(`[data-record-id="${focusId}"]`);
    if (again) again.focus();
  }
}

function historyRow(row) {
  const tr = document.createElement("tr");
  const when = document.createElement("td");
  when.className = "num";
  const profileCell = document.createElement("td");
  const files = document.createElement("td");
  files.className = "num";
  const size = document.createElement("td");
  size.className = "num";
  const duration = document.createElement("td");
  duration.className = "num";
  const status = document.createElement("td");
  const badge = document.createElement("span");
  const toggle = document.createElement("button");
  toggle.type = "button";
  toggle.className = "disclosure";
  toggle.dataset.recordId = row.id;
  toggle.setAttribute("aria-expanded", "false");
  toggle.textContent = "Details";
  status.append(badge, toggle);
  tr.append(when, profileCell, files, size, duration, status);
  const extra = historyDetail(row);
  toggle.onclick = () => {
    extra.hidden = !extra.hidden;
    toggle.setAttribute("aria-expanded", extra.hidden ? "false" : "true");
  };
  fillHistoryRow(tr, row);
  return { tr, extra };
}

function historyDetail(row) {
  const extra = document.createElement("tr");
  extra.className = "details";
  extra.hidden = true;
  const td = document.createElement("td");
  td.colSpan = 6;
  td.textContent = row.succeeded ? "Completed." : (row.error || "The sync failed.");
  extra.append(td);
  return extra;
}

function fillHistoryRow(tr, row) {
  const cells = tr.children;
  cells[0].textContent = row.date;
  cells[1].textContent = row.profileName || "—";
  cells[2].textContent = (row.fileCount || 0).toLocaleString();
  cells[3].textContent = row.size;
  cells[4].textContent = row.duration;
  const badge = cells[5].querySelector(".badge") || cells[5].querySelector("span");
  badge.className = row.succeeded ? "badge ok" : "badge bad";
  badge.textContent = row.succeeded ? "Succeeded" : "Failed";
  badge.title = row.error || "";
  const toggle = cells[5].querySelector(".disclosure");
  toggle.setAttribute("aria-label", row.succeeded ? "Show sync details" : "Show failure reason");
  const extra = tr.nextElementSibling;
  if (extra && extra.classList.contains("details")) {
    extra.querySelector("td").textContent = row.succeeded ? "Completed." : (row.error || "The sync failed.");
  }
}

function profileFromForm() {
  const profile = { ...selected() };
  const target = currentTarget();
  profile.name = document.getElementById("profile-name").value.trim() || profile.name;
  profile.sourcePath = document.getElementById("source").value.trim();
  profile.mirrorMode = document.querySelector('input[name="mode"]:checked')?.value === "mirror";
  profile.autoSync = checked("auto-sync");
  profile.autoEject = target === "drive" && checked("auto-eject");
  profile.keepVersions = target !== "google" && checked("keep-versions");
  profile.respectGitignore = checked("respect-gitignore");
  profile.skipDependencyCaches = checked("skip-deps");
  profile.keepGit = checked("keep-git");
  profile.excludes = excludes.slice();
  if (target === "google") {
    const folder = document.getElementById("gfolder").value.trim() || "SyncDrop";
    profile.destPath = `Google Drive Cloud: /${folder}`;
    profile.ssdName = "Google Drive (Cloud)";
    profile.nasHost = null;
    profile.nasShare = null;
    profile.nasUsername = null;
    profile.autoMountNas = false;
  } else if (target === "nas") {
    profile.destPath = document.getElementById("dest").value.trim();
    profile.ssdName = document.getElementById("ssd").value.trim();
    profile.nasHost = emptyToNull(document.getElementById("nas-host").value);
    profile.nasShare = emptyToNull(document.getElementById("nas-share").value);
    profile.nasUsername = emptyToNull(document.getElementById("nas-user").value);
    profile.autoMountNas = checked("auto-mount");
  } else {
    profile.destPath = document.getElementById("dest").value.trim();
    profile.ssdName = document.getElementById("ssd").value.trim();
    profile.nasHost = null;
    profile.nasShare = null;
    profile.nasUsername = null;
    profile.autoMountNas = false;
  }
  return profile;
}

async function reload(full) {
  state = await invoke("get_state");
  if (!state.profiles.some((profile) => profile.id === selectedId)) {
    selectedId = state.activeProfileId || state.profiles[0]?.id || null;
  }
  if (full) paintForm();
  else {
    paintHistory();
    paintGoogleStatus();
  }
  paintSidebar();
  if (document.getElementById("prefs-main").hidden) paintHeader();
  else paintPrefs();
}

function paintGoogleStatus() {
  if (!state) return;
  document.getElementById("google-state").textContent = state.googleSignedIn
    ? `Signed in${state.googleEmail ? ` · ${state.googleEmail}` : ""}`
    : "Not signed in";
  document.getElementById("secret-state").textContent = state.googleHasSecret
    ? "A client secret is in the keychain."
    : "No client secret saved.";
}

async function saveGoogleFields() {
  await invoke("set_google_client_id", { value: document.getElementById("client-id").value });
  const secret = document.getElementById("client-secret").value;
  if (secret) await invoke("set_google_client_secret", { value: secret });
  await invoke("set_google_folder", { value: document.getElementById("gfolder").value });
  document.getElementById("client-secret").value = "";
}

async function saveProfile() {
  clearErrors();
  const profile = profileFromForm();
  if (currentTarget() === "google") await saveGoogleFields();
  await invoke("save_profile", { profile });
  const password = document.getElementById("nas-password").value;
  if (currentTarget() === "nas" && password.trim()) {
    await invoke("set_nas_password", { profileId: profile.id, password });
    document.getElementById("nas-password").value = "";
  }
  formDirty = false;
  showSavebar();
  await reload(true);
  toast("Saved");
}

async function confirmLeave() {
  if (!formDirty) return true;
  const choice = await openDialog({
    title: "Unsaved changes",
    body: "Save this profile before leaving it?",
    ok: "Save",
    discard: "Discard",
  });
  if (choice === "cancel") return false;
  if (choice === "save") {
    try {
      await saveProfile();
    } catch (err) {
      showIssues(err);
      return false;
    }
  } else {
    formDirty = false;
    paintForm();
    showSavebar();
  }
  return true;
}

async function selectProfile(id) {
  if (id === selected()?.id && document.getElementById("prefs-main").hidden) return;
  if (!(await confirmLeave())) return;
  selectedId = id;
  document.getElementById("profile-main").hidden = false;
  document.getElementById("prefs-main").hidden = true;
  document.getElementById("open-prefs").removeAttribute("aria-current");
  formDirty = false;
  paintForm();
  paintSidebar();
  paintHeader();
  showSavebar();
}

function openDialog({ title, body, ok, discard, danger }) {
  return new Promise((resolve) => {
    document.getElementById("dialog-title").textContent = title;
    document.getElementById("dialog-body").textContent = body;
    const okBtn = document.getElementById("dialog-ok");
    const discardBtn = document.getElementById("dialog-discard");
    okBtn.textContent = ok;
    okBtn.classList.toggle("danger", !!danger);
    okBtn.classList.toggle("primary", !danger);
    discardBtn.hidden = !discard;
    if (discard) discardBtn.textContent = discard;
    const finish = (value) => {
      dialogWait = null;
      document.getElementById("dialog").close();
      resolve(value);
    };
    dialogWait = finish;
    okBtn.onclick = () => finish("ok");
    discardBtn.onclick = () => finish("discard");
    document.getElementById("dialog-cancel").onclick = () => finish("cancel");
    document.getElementById("dialog").showModal();
  });
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
  markDirty();
  paintChips();
}

function levelRank(line) {
  if (/\bERROR\b/.test(line)) return "error";
  if (/\bWARN\b/.test(line)) return "warn";
  return "info";
}

function visibleLogs(lines) {
  const min = document.getElementById("log-level").value;
  const query = document.getElementById("log-search").value.trim().toLowerCase();
  const rank = { info: 0, warn: 1, error: 2 };
  return lines.filter((line) => {
    if (rank[levelRank(line)] < rank[min]) return false;
    if (query && !line.toLowerCase().includes(query)) return false;
    return true;
  });
}

async function refreshLogs() {
  const pane = document.getElementById("logs-pane");
  const view = document.getElementById("log-view");
  if (!pane || pane.hidden) return;
  let lines = [];
  try {
    lines = await invoke("get_recent_logs", { limit: 2000 });
  } catch (err) {
    document.getElementById("log-empty").hidden = false;
    document.getElementById("log-empty").textContent = String(err);
    return;
  }
  const shown = visibleLogs(lines);
  const snapshot = shown.join("\n");
  if (selectionInside(view)) return;
  if (view.dataset.snapshot === snapshot) {
    if (checked("auto-scroll")) view.scrollTop = view.scrollHeight;
    document.getElementById("log-empty").hidden = shown.length > 0;
    return;
  }
  view.dataset.snapshot = snapshot;
  view.replaceChildren();
  for (const line of shown) {
    const row = document.createElement("div");
    row.className = `log-line log-${levelRank(line)}`;
    row.textContent = line;
    view.append(row);
  }
  document.getElementById("log-empty").hidden = shown.length > 0;
  document.getElementById("log-empty").textContent = "No log lines yet.";
  if (checked("auto-scroll")) view.scrollTop = view.scrollHeight;
}

function wireSwitch(id, onChange) {
  document.getElementById(id).onclick = () => {
    const next = !checked(id);
    setChecked(id, next);
    if (onChange) onChange(next);
    else markDirty();
  };
}

document.getElementById("dialog").addEventListener("cancel", (event) => {
  event.preventDefault();
  if (dialogWait) dialogWait("cancel");
});

document.querySelector(".tabs").addEventListener("keydown", (event) => {
  moveTabs(event, ".tabs [role=tab]", (button) => button.dataset.tab, selectTab);
});
document.querySelector("#activity .segment").addEventListener("keydown", (event) => {
  moveTabs(event, "#activity [role=tab]", (button) => button.dataset.activity, selectActivity);
});
document.querySelectorAll(".tabs [role=tab]").forEach((button) => {
  button.onclick = () => selectTab(button.dataset.tab);
});
document.querySelectorAll("#activity [role=tab]").forEach((button) => {
  button.onclick = () => selectActivity(button.dataset.activity);
});

document.getElementById("profile-list").addEventListener("keydown", (event) => {
  const options = [...document.querySelectorAll("#profile-list [role=option]")];
  const index = options.indexOf(document.activeElement);
  if (index < 0) return;
  let next = index;
  if (event.key === "ArrowDown") next = Math.min(options.length - 1, index + 1);
  else if (event.key === "ArrowUp") next = Math.max(0, index - 1);
  else if (event.key === "Home") next = 0;
  else if (event.key === "End") next = options.length - 1;
  else return;
  event.preventDefault();
  options[next].focus();
  const profile = state.profiles[next];
  if (profile) selectProfile(profile.id);
});

document.querySelectorAll("input, select").forEach((el) => {
  if (["log-search", "log-level", "new-name", "exclude-input"].includes(el.id)) return;
  const touch = () => {
    markDirty();
    if (el.name === "target") {
      if (el.value !== "google") {
        const dest = document.getElementById("dest");
        if (/Google Drive/i.test(dest.value)) dest.value = "";
        const ssd = document.getElementById("ssd");
        if (ssd.value === "Google Drive (Cloud)") ssd.value = "";
      }
      applyTarget(el.value);
    }
    if (el.name === "mode") applyTarget(currentTarget());
    paintHeader();
  };
  el.addEventListener("input", touch);
  el.addEventListener("change", touch);
});

["auto-sync", "auto-eject", "keep-versions", "auto-mount", "respect-gitignore", "skip-deps", "keep-git"].forEach((id) => wireSwitch(id));

let pendingCleanup = null;
document.getElementById("cleanup-scan").onclick = async () => {
  const profile = selected();
  if (!profile) return;
  const status = document.getElementById("cleanup-status");
  const apply = document.getElementById("cleanup-apply");
  status.hidden = false;
  status.textContent = "Scanning…";
  apply.hidden = true;
  pendingCleanup = null;
  try {
    const plan = await invoke("cleanup_plan", { profileId: profile.id });
    pendingCleanup = plan;
    const n = (plan.targets || []).length;
    const size = bytes(plan.totalBytes || 0);
    status.textContent = n
      ? `Found ${n.toLocaleString()} cache folder${n === 1 ? "" : "s"} · ${size}`
      : "No dependency or cache folders found on the destination.";
    apply.hidden = n === 0;
  } catch (err) {
    status.textContent = String(err);
    toast(String(err), true);
  }
};
document.getElementById("cleanup-apply").onclick = async () => {
  const profile = selected();
  if (!profile || !pendingCleanup || !(pendingCleanup.targets || []).length) return;
  const n = pendingCleanup.targets.length;
  const size = bytes(pendingCleanup.totalBytes || 0);
  const choice = await openDialog({
    title: "Clean up drive",
    body: `Delete ${n.toLocaleString()} dependency/cache folder${n === 1 ? "" : "s"} (${size}) from this profile's destination? This cannot be undone.`,
    ok: "Delete caches",
    danger: true,
  });
  if (choice !== "ok") return;
  try {
    await invoke("cleanup_apply", { profileId: profile.id });
    pendingCleanup = null;
    document.getElementById("cleanup-apply").hidden = true;
    document.getElementById("cleanup-status").textContent = "Caches deleted.";
    toast("Destination cleaned");
  } catch (err) {
    toast(String(err), true);
  }
};
wireSwitch("notify", (value) => {
  invoke("set_notify_on_complete", { value }).catch((err) => toast(String(err), true));
});
wireSwitch("launch", (value) => {
  invoke("set_launch_at_login", { value }).catch((err) => toast(String(err), true));
});
wireSwitch("auto-scroll", () => {});

document.getElementById("save-hint").textContent = apple ? "⌘S" : "Ctrl+S";

document.getElementById("add-form").onsubmit = async (event) => {
  event.preventDefault();
  if (!(await confirmLeave())) return;
  const name = document.getElementById("new-name").value.trim() || "Profile";
  try {
    selectedId = await invoke("add_profile", { name });
    document.getElementById("new-name").value = "";
    formDirty = false;
    await reload(true);
    toast("Profile added");
  } catch (err) {
    toast(String(err), true);
  }
};

document.getElementById("open-prefs").onclick = async () => {
  if (!(await confirmLeave())) return;
  document.getElementById("profile-main").hidden = true;
  document.getElementById("prefs-main").hidden = false;
  document.getElementById("open-prefs").setAttribute("aria-current", "page");
  document.getElementById("savebar").hidden = true;
  paintPrefs();
};

document.getElementById("pick-source").onclick = async () => {
  const path = await pickFolder("Choose source folder");
  if (!path) return;
  document.getElementById("source").value = path;
  markDirty();
  paintHeader();
};

document.getElementById("pick-dest").onclick = async () => {
  const path = await pickFolder("Choose destination folder");
  if (!path) return;
  document.getElementById("dest").value = path;
  markDirty();
  paintHeader();
};

document.getElementById("save").onclick = () => saveProfile().catch(showIssues);
document.getElementById("revert").onclick = () => {
  formDirty = false;
  paintForm();
  paintHeader();
  showSavebar();
};

document.getElementById("make-active").onclick = async () => {
  const profile = selected();
  if (!profile) return;
  try {
    await invoke("activate_profile", { id: profile.id });
    await reload(false);
    toast("This profile is active");
  } catch (err) {
    toast(String(err), true);
  }
};

document.getElementById("sync-now").onclick = () => {
  const profile = selected();
  if (!profile) return;
  invoke("sync_now", { profileId: profile.id, confirmed: false }).catch(showIssues);
};

document.getElementById("dry-run").onclick = () => {
  const profile = selected();
  if (!profile) return;
  invoke("dry_run", { profileId: profile.id }).catch(showIssues);
};

document.getElementById("cancel-sync").onclick = () => {
  invoke("cancel_sync").then(() => reload(false)).catch((err) => toast(String(err), true));
};

document.getElementById("eject").onclick = () => {
  const profile = selected();
  if (!profile) return;
  invoke("eject", { profileId: profile.id }).then(() => reload(false)).catch((err) => toast(String(err), true));
};

document.getElementById("delete-profile").onclick = async () => {
  const profile = selected();
  if (!profile) return;
  const choice = await openDialog({
    title: "Delete profile",
    body: `Delete “${profile.name}”? Files already copied stay where they are.`,
    ok: "Delete profile",
    danger: true,
  });
  if (choice !== "ok") return;
  try {
    await invoke("delete_profile", { id: profile.id });
    formDirty = false;
    selectedId = null;
    await reload(true);
    toast("Profile deleted");
  } catch (err) {
    toast(String(err), true);
  }
};

document.getElementById("set-password").onclick = async () => {
  const profile = selected();
  if (!profile) return;
  try {
    await invoke("set_nas_password", {
      profileId: profile.id,
      password: document.getElementById("nas-password").value,
    });
    document.getElementById("nas-password").value = "";
    toast("Password saved in the keychain");
  } catch (err) {
    toast(String(err), true);
  }
};

document.getElementById("clear-password").onclick = async () => {
  const profile = selected();
  if (!profile) return;
  const choice = await openDialog({
    title: "Remove password",
    body: "Remove the saved password for this share?",
    ok: "Remove password",
    danger: true,
  });
  if (choice !== "ok") return;
  try {
    await invoke("delete_nas_password", { profileId: profile.id });
    document.getElementById("nas-password").value = "";
    toast("Password removed");
  } catch (err) {
    toast(String(err), true);
  }
};

document.getElementById("open-console").onclick = () => {
  invoke("open_external", { url: "https://console.cloud.google.com/apis/credentials" })
    .catch((err) => toast(String(err), true));
};

document.getElementById("sign-in").onclick = async () => {
  try {
    await saveGoogleFields();
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

document.getElementById("clear-history").onclick = async () => {
  const choice = await openDialog({
    title: "Clear history",
    body: "Remove every recorded sync from this computer?",
    ok: "Clear history",
    danger: true,
  });
  if (choice !== "ok") return;
  try {
    await invoke("clear_history");
    await reload(false);
    toast("History cleared");
  } catch (err) {
    toast(String(err), true);
  }
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
document.getElementById("copy-logs").onclick = async () => {
  try {
    const lines = visibleLogs(await invoke("get_recent_logs", { limit: 2000 }));
    await navigator.clipboard.writeText(lines.join("\n"));
    toast(lines.length ? "Logs copied" : "Nothing to copy");
  } catch (err) {
    toast(String(err), true);
  }
};
document.getElementById("open-logs").onclick = () => {
  invoke("open_logs_folder").catch((err) => toast(String(err), true));
};

document.addEventListener("keydown", (event) => {
  if (document.getElementById("dialog").open) return;
  const mod = apple ? event.metaKey : event.ctrlKey;
  if (!mod) return;
  const key = event.key.toLowerCase();
  if (key === "s") {
    event.preventDefault();
    if (formDirty) document.getElementById("save").click();
  } else if (key === "r") {
    event.preventDefault();
    if (!document.getElementById("sync-now").hidden) document.getElementById("sync-now").click();
  } else if (event.key === ".") {
    event.preventDefault();
    if (!document.getElementById("cancel-sync").hidden) document.getElementById("cancel-sync").click();
  } else if (event.key === ",") {
    event.preventDefault();
    document.getElementById("open-prefs").click();
  }
});

window.__TAURI__.event.listen("google-status", (event) => {
  const payload = event.payload || {};
  toast(payload.ok ? "Signed in to Google Drive" : (payload.error || "Sign-in failed"), !payload.ok);
  reload(false).catch((err) => toast(String(err), true));
});
window.__TAURI__.event.listen("sync-progress", () => {
  reload(false).catch(() => {});
});
window.__TAURI__.event.listen("ui-state", () => {
  reload(false).catch(() => {});
});

document.getElementById("save-hint").textContent = apple ? "⌘S" : "Ctrl+S";
invoke("app_version").then((version) => {
  document.getElementById("app-version").textContent = version;
}).catch(() => {});
logTimer = setInterval(refreshLogs, 2000);

selectTab("sync");
selectActivity("history");
reload(true).catch((err) => {
  document.getElementById("loading").textContent = String(err);
  toast(String(err), true);
});
