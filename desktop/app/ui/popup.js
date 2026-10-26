const invoke = (cmd, args) => window.__TAURI__.core.invoke(cmd, args || {});

let closeTimer = 0;
let closeArmed = false;
let announcedStep = -1;
let hoverHold = false;
let lastDry = null;
let latest = null;
let ejected = false;
let focusedStage = "";

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

function armClose() {
  clearClose();
  if (!closeArmed || hoverHold) return;
  closeTimer = setTimeout(() => invoke("close_window"), 6000);
}

function profileFor(state) {
  const id = state.syncingProfileId
    || state.dryRun?.profileId
    || state.activeProfileId;
  return (state.profiles || []).find((profile) => profile.id === id) || null;
}

function liveFor(state, id) {
  return (state.profilesLive || []).find((row) => row.id === id) || null;
}

function driveTarget(profile) {
  if (!profile) return false;
  const dest = profile.destPath || "";
  if (
    profile.ssdName === "Google Drive (Cloud)"
    || dest.startsWith("gdrive://")
    || /Google Drive \(Cloud\)|Google Drive Cloud:/i.test(dest)
    || (profile.nasHost || "").trim()
  ) {
    return false;
  }
  return true;
}

function destinationName(profile) {
  if (!profile) return "";
  const dest = profile.destPath || "";
  if (
    profile.ssdName === "Google Drive (Cloud)"
    || dest.startsWith("gdrive://")
    || /Google Drive/i.test(dest)
  ) {
    return "Google Drive";
  }
  if ((profile.nasHost || "").trim()) return profile.nasShare || profile.ssdName || "Network share";
  return profile.ssdName || dest || "External drive";
}

function focusOnce(key, el) {
  if (focusedStage === key || !el || el.hidden || el.disabled) return;
  focusedStage = key;
  el.focus();
}

function announce(percent) {
  const step = Math.floor(Math.max(0, Math.min(100, percent)) / 10);
  if (step === announcedStep) return;
  announcedStep = step;
  document.getElementById("live").textContent = `${(step * 10).toLocaleString()} percent`;
}

function showDry(dry) {
  lastDry = dry;
  ejected = false;
  clearClose();
  closeArmed = false;
  document.getElementById("progress-view").hidden = true;
  document.getElementById("dry-view").hidden = false;
  const profile = latest
    ? (latest.profiles || []).find((item) => item.id === dry.profileId) || null
    : null;
  const heading = document.getElementById("dry-heading");
  heading.textContent = profile
    ? `${profile.name} → ${destinationName(profile)}`
    : "Preview";
  const groups = [
    { key: "copies", title: "Copy", danger: false },
    { key: "updates", title: "Update", danger: false },
    { key: "deletes", title: "Delete", danger: true },
  ];
  const root = document.getElementById("groups");
  const counts = { copies: 0, updates: 0, deletes: 0 };
  for (const group of groups) counts[group.key] = (dry[group.key] || []).length;
  const snapshot = JSON.stringify({
    copies: dry.copies || [],
    updates: dry.updates || [],
    deletes: dry.deletes || [],
    error: dry.error || "",
  });
  const same = root.dataset.snapshot === snapshot;
  const work = counts.copies + counts.updates + counts.deletes;
  const err = document.getElementById("dry-error");
  const empty = document.getElementById("dry-empty");
  err.hidden = !dry.error;
  err.textContent = dry.error || "";
  const noop = !dry.error && work === 0;
  empty.hidden = !noop;
  if (!same) {
    root.dataset.snapshot = snapshot;
    root.replaceChildren();
    if (!noop && !dry.error) {
      for (const group of groups) {
        const paths = dry[group.key] || [];
        if (!paths.length) continue;
        const section = document.createElement("section");
        section.className = group.danger ? "group delete" : "group";
        const title = document.createElement("h2");
        title.textContent = `${group.title} · ${paths.length.toLocaleString()}`;
        const list = document.createElement("ul");
        for (const path of paths) {
          const li = document.createElement("li");
          li.textContent = path;
          list.append(li);
        }
        section.append(title, list);
        root.append(section);
      }
    }
  }
  const button = document.getElementById("dry-sync");
  const cancel = document.getElementById("dry-cancel");
  if (noop) {
    button.hidden = true;
    cancel.textContent = "Close";
    focusOnce("preview-empty", cancel);
  } else if (dry.error) {
    button.hidden = true;
    cancel.textContent = "Close";
    focusOnce("preview-error", cancel);
  } else if (counts.deletes > 0) {
    button.hidden = false;
    button.disabled = false;
    button.textContent = `Delete ${counts.deletes.toLocaleString()} and copy ${(counts.copies + counts.updates).toLocaleString()}`;
    cancel.textContent = "Cancel";
    focusOnce("preview-delete", button);
  } else {
    button.hidden = false;
    button.disabled = false;
    button.textContent = `Copy ${counts.copies.toLocaleString()} and update ${counts.updates.toLocaleString()}`;
    cancel.textContent = "Cancel";
    focusOnce("preview-copy", button);
  }
}

function showProgress(state) {
  const dry = state.dryRun;
  if (dry && !state.syncing) {
    showDry(dry);
    return;
  }
  document.getElementById("dry-view").hidden = true;
  document.getElementById("progress-view").hidden = false;
  const profile = profileFor(state);
  const heading = document.getElementById("heading");
  const progress = state.progress || {};
  const pct = Math.round(progress.percent || 0);
  const running = progress.state === "running" || (state.syncing && !state.previewing);
  const done = progress.state === "done";
  const failed = progress.state === "error" || progress.state === "interrupted";
  if (running) ejected = false;
  const live = profile ? liveFor(state, profile.id) : null;
  const ejectable = !!(profile && driveTarget(profile) && !ejected && (live ? live.ejectable : state.mount && !String(state.mount).startsWith("gdrive:")));
  heading.className = "display";
  if (failed) heading.textContent = "Sync failed";
  else if (done && ejected) heading.textContent = "Safe to unplug";
  else if (done) heading.textContent = "Sync complete";
  else if (state.previewing) heading.textContent = "Preparing preview";
  else heading.textContent = profile ? profile.name : "SyncDrop";
  const sub = document.getElementById("sub");
  sub.textContent = profile ? destinationName(profile) : "";
  document.getElementById("meter").hidden = failed;
  document.getElementById("stats").hidden = failed;
  document.getElementById("stat-speed").hidden = !running;
  document.getElementById("stat-eta").hidden = !running;
  document.getElementById("bar").value = pct;
  document.getElementById("percent").textContent = `${pct.toLocaleString()}%`;
  if (running) announce(pct);
  const current = document.getElementById("current");
  if (state.previewing) {
    document.getElementById("files").textContent = "Preview";
    current.textContent = "";
    current.hidden = true;
  } else if (failed) {
    current.hidden = true;
  } else {
    document.getElementById("files").textContent = `${(progress.filesDone || 0).toLocaleString()} / ${(progress.filesTotal || 0).toLocaleString()}`;
    current.hidden = false;
    current.textContent = middle(progress.currentFile || "", 72);
  }
  document.getElementById("bytes").textContent = `${bytes(progress.bytesDone)} / ${bytes(progress.bytesTotal)}`;
  document.getElementById("speed").textContent = running && progress.speedBps > 0 ? `${bytes(progress.speedBps)}/s` : "—";
  document.getElementById("eta").textContent = running ? eta(progress.etaSecs) : "—";
  const message = document.getElementById("message");
  const summary = document.getElementById("summary");
  message.hidden = true;
  summary.hidden = true;
  if (done) {
    const files = (progress.filesDone || 0).toLocaleString();
    const took = progress.durationSecs != null ? ` in ${eta(progress.durationSecs)}` : "";
    summary.hidden = false;
    summary.textContent = `Synced ${files} files · ${bytes(progress.bytesDone)}${took}`;
  } else if (failed) {
    message.hidden = false;
    message.textContent = progress.detail || (progress.state === "interrupted" ? "The sync was interrupted before it finished." : "The sync did not finish.");
  }
  const cancel = document.getElementById("cancel");
  const dismiss = document.getElementById("dismiss");
  const retry = document.getElementById("retry");
  cancel.hidden = !running && !state.previewing;
  dismiss.hidden = running || state.previewing;
  retry.hidden = !failed;
  retry.classList.toggle("primary", failed);
  document.getElementById("open-logs").hidden = !failed;
  const eject = document.getElementById("eject");
  eject.hidden = !(done && ejectable);
  if (done && !ejectable && !ejected && !closeArmed) {
    closeArmed = true;
    announcedStep = -1;
    armClose();
  }
  if (!done) {
    closeArmed = false;
    clearClose();
  }
  if (running || state.previewing) focusOnce("running", cancel);
  else if (failed) focusOnce("failed", retry);
  else if (done && ejectable) focusOnce("done-eject", eject);
  else if (done) focusOnce(ejected ? "safe" : "done", dismiss);
}

async function refresh() {
  latest = await invoke("get_state");
  showProgress(latest);
}

document.getElementById("cancel").onclick = () => invoke("cancel_sync").then(refresh);
document.getElementById("dismiss").onclick = () => invoke("close_window");
document.getElementById("retry").onclick = () => {
  clearClose();
  closeArmed = false;
  focusedStage = "";
  invoke("get_state").then((state) => {
    const profile = profileFor(state);
    if (!profile) return;
    invoke("sync_now", { profileId: profile.id, confirmed: !profile.mirrorMode });
  });
};
document.getElementById("open-logs").onclick = () => {
  invoke("open_logs_folder").catch(() => {});
};
document.getElementById("eject").onclick = () => {
  invoke("get_state").then((state) => {
    const profile = profileFor(state);
    if (!profile) return Promise.reject(new Error("no profile"));
    return invoke("eject", { profileId: profile.id });
  }).then(() => {
    ejected = true;
    focusedStage = "";
    document.getElementById("heading").textContent = "Safe to unplug";
    document.getElementById("eject").hidden = true;
    const dismiss = document.getElementById("dismiss");
    dismiss.hidden = false;
    dismiss.focus();
  }).catch(() => {});
};
document.getElementById("dry-cancel").onclick = () => invoke("close_window");
document.getElementById("dry-sync").onclick = () => {
  if (!lastDry || document.getElementById("dry-sync").disabled) return;
  clearClose();
  document.getElementById("dry-view").hidden = true;
  document.getElementById("progress-view").hidden = false;
  invoke("sync_now", { profileId: lastDry.profileId, confirmed: true });
};

document.body.addEventListener("mouseenter", () => {
  hoverHold = true;
  clearClose();
});
document.body.addEventListener("mouseleave", () => {
  hoverHold = false;
  armClose();
});
document.body.addEventListener("focusin", () => {
  hoverHold = true;
  clearClose();
});
document.body.addEventListener("focusout", () => {
  setTimeout(() => {
    if (!document.body.contains(document.activeElement)) {
      hoverHold = false;
      armClose();
    }
  }, 0);
});

document.addEventListener("keydown", (event) => {
  const apple = /Mac|iPhone|iPad/.test(navigator.platform || navigator.userAgent || "");
  const mod = apple ? event.metaKey : event.ctrlKey;
  if (mod && event.key === ".") {
    event.preventDefault();
    if (!document.getElementById("cancel").hidden) document.getElementById("cancel").click();
  }
});

window.__TAURI__.event.listen("sync-progress", () => refresh());
window.__TAURI__.event.listen("dry-run", (event) => {
  invoke("get_state").then((state) => {
    latest = state;
    showDry(event.payload);
  });
});

refresh();
