---
paths:
  - "desktop/**"
---

### Desktop architecture

Rust workspace (`desktop/Cargo.toml`): `syncdrop-core` (no UI) and `syncdrop` (Tauri 2 tray). Android stays in `android/`.

**Core** (`desktop/core/src/`)

- **`profile`** — `SyncProfile`. `#[serde(default)]` so older JSON still loads. `autoMountNAS` is accepted as an alias of `autoMountNas` (Swift CodingKey). `expanded_source_path` / `expanded_dest_path` expand `~`. Google Drive helpers: `is_google_drive`, `is_cloud_storage`, `is_google_drive_cloud`.
- **`config`** — `ConfigStore` reads and writes `dirs::config_dir()/SyncDrop/config.json` atomically. Corrupt files are renamed to `config.json.bak` and replaced. History is capped at 20. On macOS, if that file is missing, a one-time `defaults export com.syncdrop.app -` pulls the Swift UserDefaults plist (profiles and history are JSON `<data>` blobs; `Date` is seconds since 2001-01-01). The Google client id is copied; the client secret is not written into the file.
- **`progress`** — `SyncProgress` / `SyncRecord`.
- **`engine`** — walks the source, skips excludes, copies when size or mtime (modify window 1s) differs, writes `*.syncdrop-tmp` then renames, can mirror and keep versions under `.syncdrop-versions/`. Dry-run and sync share one plan. Cancel via `AtomicBool`.
- **`volumes`** — polls `sysinfo` disks every 2s, emits connect/disconnect for `ssd_name`, rescans after a >10s gap, ejects per OS.
- **`nas`** — SMB mount per OS. Password is in the OS keyring (service `SyncDrop`, account = profile id), never in `config.json` and never logged. Retries every 60s while `auto_mount_nas` is set.
- **`gdrive`** — loopback OAuth + PKCE. Client id is in config; client secret and refresh token are in the keyring. Drive v3 folder create + upload; resumable above 5 MB. Tests cover request building, not the network.

**App** (`desktop/app/`)

- Tray menu (profiles, Sync Now, Dry Run, Eject, Settings, Launch at Login, Quit) and status text. macOS starts as an accessory (no Dock icon).
- `ui/index.html` settings, `ui/popup.html` progress. Tauri commands only call core.
- On `Connected` + `auto_sync`, sync. On success + `auto_eject`, eject. Notifications via `tauri-plugin-notification`. Login item via `tauri-plugin-autostart`. Folder pickers via `tauri-plugin-dialog`.
