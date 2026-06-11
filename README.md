# SyncDrop

A lightweight macOS menu-bar app that mirrors a folder on your Mac to an external SSD with `rsync` — automatically, the moment the drive is plugged in.

No dock icon, no window clutter. It lives in the menu bar, watches for your SSD, and keeps a backup in sync.

## Features

- **Auto-sync on connect** — plug in your SSD and SyncDrop starts the sync (opt-in per profile).
- **Multiple profiles** — different source/destination/exclude sets, switchable from the menu.
- **Dry-run preview** — see exactly what will be added, updated, or deleted before committing.
- **Mirror mode** — `--delete` so the SSD exactly matches the Mac (files removed on Mac are removed on SSD).
- **Versioned backups** — overwritten/deleted files are kept under `.syncdrop_archive/<date>` instead of being discarded.
- **Exclude patterns** — rsync glob patterns, with sensible macOS defaults (`.DS_Store`, `.Spotlight-V100`, `node_modules`, …).
- **Auto-eject** — safely eject the SSD when a sync finishes.
- **Sync history & notifications** — recent runs (files, size, duration) and a completion notification.
- **exFAT-safe** — avoids permission/ownership flags that break on exFAT and uses `--modify-window=1` for its 2-second timestamp granularity.

## Requirements

- macOS 13 (Ventura) or later
- `rsync` (ships with macOS at `/usr/bin/rsync`)
- Xcode or Swift toolchain to build

## Build & install

```bash
# Build a signed .app bundle
make app

# Build and install to ~/Applications
make install
```

Then launch SyncDrop from `~/Applications`. The icon appears in your menu bar; open **Settings** to configure a profile (source folder, SSD volume name, destination).

For development:

```bash
swift build     # debug build
swift test      # run tests (requires full Xcode, not just Command Line Tools)
```

## How it works

`VolumeMonitor` watches `NSWorkspace` mount/unmount notifications and matches the configured SSD volume name. On connect (with auto-sync on), `SyncEngine` launches `rsync` with a per-profile argument list, streams progress from stdout, and reports state through Combine to the menu-bar UI. Settings, profiles, and history persist in `UserDefaults` via `ConfigStore`.

## Project layout

```
Sources/
  SyncDropCore/    # rsync engine, volume monitor, config, models (no UI)
  SyncDrop/        # AppKit/SwiftUI menu-bar app
Tests/             # XCTest unit tests for the core
android/           # Android port (work in progress)
Resources/         # app icon
```

## Android

An Android port (Kotlin + Jetpack Compose) is in early development under `android/`, targeting USB-OTG SSD sync.

## License

[MIT](LICENSE) © 2026 Abhinay Reddy
