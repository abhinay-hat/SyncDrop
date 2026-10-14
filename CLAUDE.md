# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

SyncDrop: auto-sync chosen folders to an external drive (or NAS share) when it connects.
- **Desktop**: Tauri 2 tray app (`desktop/`) for macOS, Windows, and Linux. Core logic is Rust (`syncdrop-core`); the shell is a thin Tauri wrapper.
- **Android**: Jetpack Compose app (`android/`) with USB OTG detection, Room persistence, WorkManager background sync, and Hilt DI.

## Commands

`cargo` on this machine is `~/.cargo/bin/cargo`.

### Desktop
```bash
make desktop-test        # cargo test in desktop/
make desktop             # cargo tauri build in desktop/app (needs tauri-cli ^2)
make desktop-run         # cargo run -p syncdrop
make clean               # cargo clean plus leftover Swift bundle outputs

cd desktop && ~/.cargo/bin/cargo test
cd desktop && ~/.cargo/bin/cargo build
cd desktop/app && ~/.cargo/bin/cargo tauri build
```

macOS signing: set `APPLE_SIGNING_IDENTITY` before `make desktop`. Notarize with `Scripts/sign-and-notarize.sh` (App Store Connect API key env vars; `APP_IDENTITY` is accepted as an alias of the signing identity).

### Android
```bash
cd android
./gradlew test                   # run all unit tests
./gradlew :app:assembleDebug     # build debug APK (outputs to app/build/outputs/apk/debug/)
./gradlew :app:installDebug      # install debug APK on connected device/emulator
```

## Architecture

- **Desktop**: workspace crates `syncdrop-core` (profiles, config, copy engine, volumes, NAS, Google Drive) and `syncdrop` (Tauri 2 tray + `desktop/app/ui`). Details → `.claude/rules/desktop-architecture.md` (loads on `desktop/**`).
- **Android**: single-activity Compose app (Room, Hilt, WorkManager, USB OTG, SAF) → `.claude/rules/android-architecture.md` (loads on `android/**`).
