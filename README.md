<div align="center">

<img src="desktop/app/icons/128x128.png" width="128" height="128" alt="SyncDrop icon">

# SyncDrop

**OneDrive for your own hardware — auto-sync the folders you choose to any drive you plug in, the moment it connects.**

[![Download](https://img.shields.io/badge/Download-latest%20release-2E7DF6?style=for-the-badge)](https://github.com/abhinay-hat/SyncDrop/releases/latest)
[![License: MIT](https://img.shields.io/badge/License-MIT-1FB6C9.svg?style=for-the-badge)](LICENSE)
[![Platform](https://img.shields.io/badge/macOS-13%2B-555.svg?style=for-the-badge)](#requirements)

<br>

![SyncDrop](docs/screenshots/hero.png)

</div>

SyncDrop is a lightweight macOS **menu-bar** app. No dock icon, no window clutter.

Think of it like OneDrive or iCloud Drive — but instead of syncing to a cloud you don't control, it syncs the folders **you** pick to **your own** storage device. Configure your folders once; then whenever you connect the configured drive — an **external SSD, USB pen drive, or hard disk** — SyncDrop automatically mirrors your selected files onto it. Plug in, it syncs, you're backed up. No subscriptions, no cloud, your data stays yours.

<div align="center">

![SyncDrop demo](docs/screenshots/demo.gif)

*Plug in your drive → SyncDrop detects it → mirrors your folders → done.*

</div>

---

## Download

Installers for the current release are on **[GitHub Releases](https://github.com/abhinay-hat/SyncDrop/releases/latest)**.

| Platform | File |
|----------|------|
| macOS Apple silicon | [`SyncDrop_0.1.0_aarch64.dmg`](https://github.com/abhinay-hat/SyncDrop/releases/latest/download/SyncDrop_0.1.0_aarch64.dmg) |
| macOS Intel | [`SyncDrop_0.1.0_x64.dmg`](https://github.com/abhinay-hat/SyncDrop/releases/latest/download/SyncDrop_0.1.0_x64.dmg) |
| Windows | [`SyncDrop_0.1.0_x64-setup.exe`](https://github.com/abhinay-hat/SyncDrop/releases/latest/download/SyncDrop_0.1.0_x64-setup.exe) |
| Linux AppImage | [`SyncDrop_0.1.0_amd64.AppImage`](https://github.com/abhinay-hat/SyncDrop/releases/latest/download/SyncDrop_0.1.0_amd64.AppImage) |
| Linux deb | [`SyncDrop_0.1.0_amd64.deb`](https://github.com/abhinay-hat/SyncDrop/releases/latest/download/SyncDrop_0.1.0_amd64.deb) |
| Android | [`SyncDrop-android-v0.1.0.apk`](https://github.com/abhinay-hat/SyncDrop/releases/latest/download/SyncDrop-android-v0.1.0.apk) |

macOS also includes `SyncDrop_aarch64.app.tar.gz` and `SyncDrop_x64.app.tar.gz` (the `.app` bundle).

**Install notes**

- **macOS unsigned:** right-click the app → **Open**, or run `xattr -dr com.apple.quarantine` on it.
- **Windows SmartScreen:** **More info > Run anyway**.
- **Linux AppImage:** `chmod +x` the file before launching it.

## Install

### Build from source

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

<div align="center">

![SyncDrop settings](docs/screenshots/settings.png)

</div>

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

- macOS, Windows, or Linux for the desktop app (WebView2 on Windows, webkit2gtk 4.1 on Linux)
- Rust and `tauri-cli` ^2 for source builds (`cargo` is `~/.cargo/bin/cargo`)

---

## How it works

The desktop app polls for the configured drive. On connect — with auto-sync enabled — the Rust copy engine mirrors the selected folders and reports progress to the tray. Profiles, settings, and history persist in `config.json`.

---

## Project layout

```
desktop/
  core/            # syncdrop-core: profiles, config, copy engine, volumes, NAS, Drive
  app/             # Tauri 2 tray app + ui/
android/           # Android port (work in progress)
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

Issues and pull requests welcome. Run `make desktop-test` before submitting. `make desktop` produces the Tauri bundles (`cargo tauri build`).

### Releasing (maintainers)

```bash
make desktop    # Tauri bundles under desktop/target/release/bundle/
```

To ship a **notarized** macOS build, set `APPLE_SIGNING_IDENTITY` so Tauri signs during the build, then notarize:

```bash
export APPLE_SIGNING_IDENTITY="Developer ID Application: Your Name (TEAMID)"
export APP_STORE_CONNECT_KEY_ID=...        # App Store Connect API key id
export APP_STORE_CONNECT_ISSUER_ID=...     # issuer id
export APP_STORE_CONNECT_API_KEY_P8="$(cat AuthKey_XXXX.p8)"
./Scripts/sign-and-notarize.sh   # builds, notarizes, staples, zips
```

See [`Scripts/sign-and-notarize.sh`](Scripts/sign-and-notarize.sh).

#### Automated releases (GitHub Actions)

`make release` prints the tag and push commands. Pushing a `v*` tag triggers [`.github/workflows/release.yml`](.github/workflows/release.yml), which builds desktop installers and the Android debug APK and attaches them to a GitHub Release. `0.x` tags are marked prerelease.

```bash
make release
git tag v0.1.0 && git push origin v0.1.0
```

If these repo **secrets** are set, macOS builds are signed (and notarized when the Apple ID secrets are present). When they are absent, the build is unsigned:

| Secret | What |
|--------|------|
| `APPLE_CERTIFICATE` | Developer ID certificate, `.p12` base64-encoded |
| `APPLE_CERTIFICATE_PASSWORD` | password for that `.p12` |
| `APPLE_SIGNING_IDENTITY` | `Developer ID Application: Your Name (TEAMID)` |
| `APPLE_ID` | Apple ID used for notarization |
| `APPLE_PASSWORD` | app-specific password |
| `APPLE_TEAM_ID` | Apple Developer Team ID |

## License

[MIT](LICENSE) © 2026 Abhinay Reddy
