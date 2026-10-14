# SyncDrop v2 Design

## Scope
Two parallel deliverables:
1. **macOS app** — add 5 new features to existing Swift/SwiftUI codebase
2. **Android app** — new Kotlin/Compose app with USB OTG SSD sync

---

## macOS App — New Features

### 1. Auto-eject after sync
Toggle in Settings → Behavior: "Eject SSD after successful sync."
On `syncDidComplete`, if toggle is on: call `NSWorkspace.shared.unmountAndEjectDevice(atPath: "/Volumes/<ssdName>")`. Show notification: "Sync complete — safe to unplug." If eject fails, show error notification but don't mark sync as failed.
New `ConfigStore` key: `autoEject: Bool` (default `false`).

### 2. Exclude rules
Array of glob patterns stored in `ConfigStore` as `[String]` (default: `.DS_Store`, `.Spotlight-V100`, `.fseventsd`, `.Trashes`, `node_modules`).
Each pattern passed as `--exclude=<pattern>` arg to rsync (before source/dest).
Settings → Folders tab: editable list with add/remove. User can type any pattern.

### 3. Dry-run preview
"Preview Changes" button in `SyncPopupContentView` (idle state, alongside "Start Sync").
Runs rsync with `--dry-run --itemize-changes` + existing args (minus `--progress --stats`).
`DryRunParser` reads itemize output: first char `>` = send (new/updated), `*deleting` = delete.
Result shown as sheet with three counts: N files to copy, N files to update, N files to delete.
"Sync Now" confirms and starts real sync. Only shown in idle state. Disabled during real sync.

### 4. Versioned backups
Toggle in Settings → Behavior: "Keep versions of overwritten files."
When on, adds `--backup --backup-dir=".syncdrop_archive/$(date +%Y-%m-%d)"` to rsync args.
Files overwritten or deleted (in mirror mode) move into archive folder on SSD rather than being destroyed.
New `ConfigStore` key: `keepVersions: Bool` (default `false`).

### 5. Multiple sync profiles
`ConfigStore` refactored: single source/dest/ssdName → array of `SyncProfile` structs.
`SyncProfile`: `id: UUID`, `name: String`, `sourcePath: String`, `destPath: String`, `ssdName: String`, `mirrorMode: Bool`, `autoSync: Bool`, `autoEject: Bool`, `keepVersions: Bool`, `excludes: [String]`.
One profile is `activeProfileId`. `SyncEngine` and `VolumeMonitor` read from active profile.
Settings: profile list with add/duplicate/delete. Switch profile from menu bar.
Migration: on first launch with v2, convert existing single-config to a profile named "Default".

---

## Android App

### Stack
- Language: Kotlin
- UI: Jetpack Compose
- Async: Coroutines + Flow
- Background: WorkManager
- Min SDK: 26 (Android 8)

### Core Flow
1. `UsbReceiver` (BroadcastReceiver) listens for `ACTION_USB_DEVICE_ATTACHED`.
2. `UsbDeviceChecker` filters for USB mass storage class (class 0x08).
3. On detection: show `SyncPromptNotification` + bring app to foreground if open.
4. User taps "Sync" → `FolderPickerScreen` (if no profiles configured) or goes straight to sync.
5. `SyncEngine` copies files: source (SAF `DocumentFile` tree) → destination (USB volume via `UsbManager`).
6. `SyncProgressScreen` shows file count, current filename, cancel button.
7. On complete: notification + optional auto-eject.

### Components

**Data layer**
- `SyncProfile`: id, name, sourceUri (SAF URI string), destPath, mirrorMode, autoSync, excludePatterns
- `SyncRecord`: date, fileCount, durationMs, succeeded
- `AppPreferences`: Room DB or DataStore for profiles + history

**Sync logic**
- `FileComparer`: compare source vs dest by name + lastModified + size; returns (toCopy, toDelete) lists
- `FileCopier`: copies `DocumentFile` → USB volume using `UsbDeviceConnection` + bulk transfer, or falls back to `FileOutputStream` if volume is mounted as MTP/filesystem
- `SyncWorker` (WorkManager): runs sync in background, posts progress via `Flow`

**UI screens**
- `HomeScreen`: list of profiles, SSD connection status, "Sync Now" per profile
- `ProfileEditScreen`: source folder picker (`ACTION_OPEN_DOCUMENT_TREE`), dest path, toggles
- `SyncProgressScreen`: progress bar, current file, cancel
- `HistoryScreen`: list of past syncs

**Settings**
- Auto-sync on connect (per profile)
- Mirror mode — delete files removed from phone
- Exclude patterns
- Notification preferences

### Android permissions required
- `android.permission.MANAGE_USB` (system — use `UsbManager` request instead)
- `android.permission.FOREGROUND_SERVICE`
- `android.permission.POST_NOTIFICATIONS` (API 33+)
- SAF URI permissions (persisted via `takePersistableUriPermission`)

### USB OTG strategy
Primary: request `UsbDevice` permission via `UsbManager.requestPermission`, mount as mass storage, use `UsbDeviceConnection` for raw file transfer.
Fallback: if device mounts as external storage volume (some Android ROMs auto-mount USB drives), use standard `File` API via `Environment.getExternalStorageState`.

---

## What is NOT in scope
- Windows app (deferred — can be added as a third deliverable later)
- Cloud sync / network sync
- Bi-directional sync
- Encryption at rest
- Bandwidth throttling
