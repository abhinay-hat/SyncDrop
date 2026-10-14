# SyncDrop → Rust migration plan

Goal: one Rust codebase for the desktop app on **macOS, Windows, Linux**, replacing the Swift app
(`Sources/`, `Tests/`, `Package.swift`). Feature parity with the Swift app first, then delete Swift.

Android (`android/`) stays Kotlin: its sync reads sources through SAF `DocumentFile` URIs, which a
Rust core can't open directly, so sharing code there buys little. Revisit only if the diff logic
grows complex.

## Layout

```
desktop/
  Cargo.toml                 # workspace: members = ["core", "app"]
  core/                      # crate `syncdrop-core` — no UI, unit-tested
    src/lib.rs
    src/profile.rs           # SyncProfile (serde, #[serde(default)] for optional/newer fields)
    src/config.rs            # ConfigStore: profiles, active id, settings, history → JSON file
    src/engine.rs            # native copy engine (replaces rsync) + dry-run
    src/progress.rs          # SyncProgress / SyncRecord
    src/volumes.rs           # drive detection (polling) + eject
    src/nas.rs               # SMB mount per OS + password in OS keyring
    src/gdrive.rs            # Google OAuth (loopback + PKCE) + Drive v3 upload
  app/                       # crate `syncdrop` — Tauri 2 tray app
    src/main.rs
    tauri.conf.json
    ui/                      # plain HTML/CSS/JS, no bundler, no Node
      index.html             # settings window
      popup.html             # sync progress popup
```

## Rules for every phase

- Port behavior from the Swift file named in the phase; read it first.
- Stable, widely used crates only: `serde`, `serde_json`, `uuid`, `dirs`, `globset`, `walkdir`,
  `filetime`, `chrono`, `thiserror`, `sysinfo`, `keyring`, `reqwest` (rustls), `tauri` 2 + official plugins.
  Nothing else without a reason in a comment.
- No `unwrap()` on I/O in library code. Return `Result`.
- Every core module gets `#[cfg(test)]` tests using `tempfile` dirs. `cargo test` must pass.
- OS-specific code behind `#[cfg(target_os = "...")]`; every OS must at least compile.
- Do not touch `android/`, `Sources/`, `Tests/` until Phase 6.

## Phase 1 — core: profile + config + progress

Port `SyncProfile.swift`, `ConfigStore.swift`, `SyncProgress.swift`, `SyncRecord.swift`.
- Config file: `dirs::config_dir()/SyncDrop/config.json`. Atomic write (temp file + rename).
- Missing/newer fields must decode (serde defaults), same as the Swift custom `Decodable`.
- Corrupt config → back it up to `config.json.bak`, start fresh, don't crash.
- Default excludes per OS: add `Thumbs.db`, `desktop.ini`, `$RECYCLE.BIN` on Windows.
- `expanded_source_path` / `expanded_dest_path` expand `~`.
- Keep `is_google_drive`, `is_cloud_storage`, `is_google_drive_cloud` helpers.

## Phase 2 — core: copy engine + dry-run

Port `SyncEngine.swift` + `DryRunEngine.swift` semantics, without rsync:
- Walk source, skip `excludes` (globset, matched against file name and relative path).
- Copy when dest missing, size differs, or mtime differs by > 1 s (exFAT 2 s granularity → treat
  `modify_window = 1s` like rsync `--modify-window=1`; FAT/exFAT may need 2 s — make it a const).
- Preserve mtime (`filetime`). Never copy permissions/owner (exFAT).
- Copy to `name.syncdrop-tmp` then rename → no half-written files on unplug.
- `mirror_mode`: delete dest files/dirs not in source (respecting excludes — never delete excluded).
- `keep_versions`: before overwrite/delete, move old dest file into
  `<dest>/.syncdrop-versions/<YYYY-MM-DD_HHMMSS>/<rel path>`; exclude that dir from walks.
- Progress callback: `(files_done, files_total, bytes_done, bytes_total, current_path)`.
- Cancellation via `Arc<AtomicBool>`.
- Dry-run returns `Plan { copies, updates, deletes }` with no writes. The real sync executes the same plan.
- Result → `SyncRecord` appended to history (cap history at 100).

## Phase 3 — core: volumes, eject, NAS

Port `VolumeMonitor.swift` + `NASMounter.swift`:
- Detection: poll `sysinfo::Disks` every 2 s on a background thread, emit `Connected(mount_point)` /
  `Disconnected` when a disk whose name or mount-point basename equals `ssd_name` appears/disappears.
  (ponytail: polling, not DiskArbitration/WM_DEVICECHANGE/udev — upgrade if 2 s latency matters.)
- Re-check right after wake: if the poll gap is > 10 s, rescan immediately.
- Eject: macOS `diskutil eject <mount>`, Linux `udisksctl unmount -b` (fallback `umount`),
  Windows PowerShell `(New-Object -comObject Shell.Application).Namespace(17).ParseName('E:').InvokeVerb('Eject')`.
- NAS mount (`auto_mount_nas`): macOS `mount_smbfs //user@host/share <dir>` (or `open smb://` fallback),
  Windows `net use \\host\share /user:<u> <p>`, Linux `gio mount smb://host/share`.
  Password via `keyring` crate (service `SyncDrop`, account = profile id). Never in config.json.
  Never put the password on a process command line visible in logs; on macOS/Linux pass it via
  URL only to the mount call, never log it.
- Background loop retries NAS mount every 60 s while `auto_mount_nas` and not mounted.

## Phase 4 — core: Google Drive

Port `GoogleOAuthManager.swift`, `GoogleDriveClient.swift`, `GoogleDriveSyncEngine.swift`:
- OAuth 2.0 installed-app flow: loopback `http://127.0.0.1:<random port>` + PKCE, open browser with
  `open`/`xdg-open`/`start` (the `opener` Tauri plugin in the app layer is fine too).
- Client id/secret are user-entered in Settings (Swift kept them in UserDefaults). Store client id in
  config.json, client secret + refresh token in `keyring` (service `SyncDrop`).
- Drive v3: find-or-create folder path, upload new/changed files (compare size + modifiedTime),
  resumable upload for > 5 MB. Same progress callback as Phase 2.
- Tests: unit-test the request building / diff logic with no network.

## Phase 5 — Tauri 2 tray app

Port `AppDelegate.swift`, `MenuBarController.swift`, `SettingsView.swift`, `SyncPopup*`, `DryRunSheet.swift`,
`LoginItemManager.swift`:
- Tray icon + menu: profile list (active ✓), Sync Now, Dry Run, Eject, Settings, Launch at Login, Quit.
  Tray status text: idle / syncing NN% / last sync time.
- No dock/taskbar window on start (macOS `ActivationPolicy::Accessory`).
- Settings window (`ui/index.html`): CRUD profiles, all fields, folder pickers (`tauri-plugin-dialog`),
  NAS password field (sent to core → keyring), Google sign-in button, history table.
- Popup window (`ui/popup.html`): progress bar, current file, cancel.
- Dry-run view: list of copies/updates/deletes before confirming.
- Wiring like AppDelegate: on `Connected` + `auto_sync` → sync; on finish + `auto_eject` → eject;
  notification on finish/error (`tauri-plugin-notification`).
- Launch at login: `tauri-plugin-autostart`.
- Tauri commands are thin wrappers over `syncdrop-core`; no sync logic in the app crate.
- `cargo build` in `desktop/` must succeed on macOS.

## Phase 6 — packaging, CI, Swift removal

- `.github/workflows/desktop.yml`: matrix macos/windows/ubuntu → `cargo test` + `cargo tauri build`.
- Makefile targets: `make desktop` (tauri build), `make desktop-test`.
- macOS signing/notarization: port `Scripts/sign-and-notarize.sh` to use the Tauri bundle.
- One-time import: on first launch on macOS, if `config.json` is missing, read old profiles from
  `defaults export <bundle id> -` (Swift app's UserDefaults) and convert.
- Delete `Sources/`, `Tests/`, `Package.swift`, Swift Makefile targets.
  Update `CLAUDE.md` + `.claude/rules/` (replace macos-architecture.md with desktop-architecture.md).
