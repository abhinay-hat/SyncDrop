<div align="center">

<img src="Resources/AppIcon.png" width="128" height="128" alt="SyncDrop icon">

# SyncDrop

**OneDrive for your own hardware — auto-sync the folders you choose to any drive you plug in, the moment it connects.**

[![Download](https://img.shields.io/badge/Download-SyncDrop.zip-2E7DF6?style=for-the-badge)](https://github.com/abhinay-hat/SyncDrop/releases/latest/download/SyncDrop.zip)
[![License: MIT](https://img.shields.io/badge/License-MIT-1FB6C9.svg?style=for-the-badge)](LICENSE)
[![Platform](https://img.shields.io/badge/macOS-13%2B-555.svg?style=for-the-badge)](#requirements)

</div>

SyncDrop is a lightweight macOS **menu-bar** app. No dock icon, no window clutter.

Think of it like OneDrive or iCloud Drive — but instead of syncing to a cloud you don't control, it syncs the folders **you** pick to **your own** storage device. Configure your folders once; then whenever you connect the configured drive — an **external SSD, USB pen drive, or hard disk** — SyncDrop automatically mirrors your selected files onto it. Plug in, it syncs, you're backed up. No subscriptions, no cloud, your data stays yours.

---

## Install

### Option A — Download the app (recommended)

1. **[Download `SyncDrop.zip`](https://github.com/abhinay-hat/SyncDrop/releases/latest/download/SyncDrop.zip)** from the latest release.
2. **Unzip** it (double-click).
3. **Drag `SyncDrop.app`** into your `/Applications` folder.
4. **First launch:** right-click `SyncDrop.app` → **Open** → **Open** in the dialog.
   > The build is *ad-hoc signed* (not notarized), so macOS Gatekeeper asks for confirmation the first time. You only do this once.

   If macOS still blocks it, remove the quarantine flag from Terminal:
   ```bash
   xattr -dr com.apple.quarantine /Applications/SyncDrop.app
   ```
5. The **SyncDrop icon appears in your menu bar**. Click it → **Settings** to set up.

### Option B — Build from source

Requires the Swift toolchain (Xcode or Command Line Tools).

```bash
git clone https://github.com/abhinay-hat/SyncDrop.git
cd SyncDrop

make app        # builds a signed SyncDrop.app in the repo root
make install    # builds and copies it to ~/Applications
```

Other targets:

```bash
swift build     # debug build
swift test      # run unit tests (needs full Xcode, not just Command Line Tools)
make clean      # remove build artifacts
```

---

## First-time setup

Open the menu-bar icon → **Settings**, then in the **Folders** tab:

1. **Source (Mac)** — choose the folder you want backed up (e.g. `~/Desktop/Projects`).
2. **SSD Volume Name** — type the exact name of your SSD as it appears in Finder (e.g. `Extreme Pro`).
3. **Destination (on SSD)** — plug in the SSD, then choose the target folder on it.

In the **Behavior** tab, turn on **Auto-sync when SSD connected** if you want it to run automatically. That's it — plug in the drive and SyncDrop syncs.

---

## Features

| Feature | What it does |
|---------|--------------|
| **Auto-sync on connect** | Plug in your SSD and the sync starts (opt-in per profile). |
| **Multiple profiles** | Different source/destination/exclude sets, switchable from the menu. |
| **Dry-run preview** | See exactly what will be added, updated, or deleted before committing. |
| **Mirror mode** | Adds `--delete` so the SSD exactly matches the Mac. |
| **Versioned backups** | Overwritten/deleted files are kept under `.syncdrop_archive/<date>` instead of discarded. |
| **Exclude patterns** | rsync glob patterns, with sane macOS defaults (`.DS_Store`, `node_modules`, …). |
| **Auto-eject** | Safely ejects the SSD after a successful sync. |
| **Sync history & notifications** | Recent runs (files, size, duration) and a completion notification. |
| **exFAT-safe** | Avoids permission/ownership flags that break on exFAT; uses `--modify-window=1` for its 2-second timestamp granularity. |

---

## Requirements

- macOS **13 (Ventura)** or later
- `rsync` — ships with macOS at `/usr/bin/rsync`
- (Source builds only) Swift toolchain

---

## How it works

`VolumeMonitor` watches `NSWorkspace` mount/unmount notifications and matches your configured SSD volume name. On connect — with auto-sync enabled — `SyncEngine` launches `rsync` with a per-profile argument list, streams progress from stdout, and reports state through Combine to the menu-bar UI. Profiles, settings, and history persist in `UserDefaults` via `ConfigStore`.

---

## Project layout

```
Sources/
  SyncDropCore/    # rsync engine, volume monitor, config, models (no UI)
  SyncDrop/        # AppKit + SwiftUI menu-bar app
Tests/             # XCTest unit tests for the core
android/           # Android port (work in progress)
Resources/         # app icon (.svg source + .icns)
```

---

## Roadmap

SyncDrop today is macOS → local drive. The goal is **sync the folders you choose to any storage you own, from any device.** Planned:

- **NAS / network drives** — sync to a NAS or any mounted network share, not just USB-attached drives.
- **Android app** — sync from your phone to a USB-OTG SSD/pen drive (in progress under [`android/`](android/)).
- **Windows app** — the same plug-in-and-sync experience on Windows.
- **More targets** — any drive type macOS/Windows can mount (SSD, pen drive, HDD, SD card).

Have a use case or want to help build one of these? Open an issue.

## Android (in progress)

An Android port (Kotlin + Jetpack Compose) targeting USB-OTG drive sync is in early development under [`android/`](android/).

---

## Contributing

Issues and pull requests welcome. Build with `make app`, run `swift test` before submitting.

## License

[MIT](LICENSE) © 2026 Abhinay Reddy
