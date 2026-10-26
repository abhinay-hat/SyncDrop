# SyncDrop UI redesign brief (pass 2)

Source critique: `.impeccable/critique/2026-10-14T23-20-32Z__desktop-app-ui-index-html.md` (17/40).
Mode: **Operate** (task tool). Tone: **native utility, sharper**. Scope: **all issues P0–P3**.
Priority order: safety + status first, then structure, then accessibility, then polish.

## Visual direction

- Feels like macOS System Settings / a well-made menu-bar utility, not a web dashboard. Windows/Linux
  get the same layout; no OS-specific skins.
- Brand color = teal from the logo (`#1FB6C9` family) as the single accent, tuned per theme for AA
  (button text ≥ 4.5:1). Neutrals slightly cool. Danger red reserved for deletes only.
- Type scale with real hierarchy (fixes detector `flat-type-hierarchy`): 11 / 13 / 15 / 20 / 28 px,
  weights 400/500/600 only (no 560). Tabular numerals for counts, sizes, times.
- Input borders ≥ 3:1 against their background (WCAG 1.4.11). Disabled controls still legible.
- Sidebar brand mark uses the new logo glyph (inline SVG, `desktop/app/icons/source/app-icon.svg`),
  not a document icon.

## Structure

- **Hero drive card** at the top of each profile: drive/target name large, state as text + icon
  (Connected / Not connected / Syncing 42% / Last sync failed), last sync relative time, free space if
  available. Primary action = Sync now; becomes **Cancel** with inline progress (percent, files x/y,
  ETA) while syncing. Secondary = Preview (dry run), Eject.
- **Target type first**: segmented control External drive / Network share / Google Drive. Only that
  target's fields render. No NAS/Cloud fields for a drive profile and vice versa.
- Sections (≤ 5): **Sync** (source, destination, target fields), **Rules** (mode, excludes),
  **Activity** (history + logs merged, segmented History | Logs), **About**.
  App-wide prefs (notify on complete, launch at login) move to a **Preferences** item at the bottom of
  the sidebar, not inside a profile.
- **One sticky Save / Revert bar** that appears only when the form is dirty ("Unsaved changes").
  ⌘S / Ctrl+S saves. Switching profile with unsaved changes asks: Save / Discard / Cancel.
- **Selecting ≠ activating**: clicking a profile only opens it for editing. Each profile row shows its
  own status text (not just a dot). "Make active" is explicit if the core still needs an active profile;
  ideally every profile with auto-sync watches its own drive.

## Safety (P0/P1)

- P0: add `[hidden]{display:none!important}`; popup views must never stack.
- **Mode** is a 2-option control, not a toggle: "Copy new & changed" (default) vs
  "Exact mirror — deletes files not in source" with a danger-colored inline warning when chosen.
- Mirror profiles: Sync now always shows the preview first. Preview groups Copy / Update / Delete with
  counts; Delete group in a danger-tinted container; confirm button names the consequence
  ("Delete 4 and copy 120"). No-op plans say "Already up to date".
- Delete profile, Clear history, Remove password: confirm dialog (in-page, not native `confirm()`)
  or 5 s undo toast.
- Sync now disabled with a reason when source/destination missing.

## Popup

- 440×460 card: profile + destination, progress bar, percent, files x/y, bytes, speed, ETA, current
  file (middle-truncated), Cancel.
- Finish = the peak-end moment: "Safe to unplug" / "Synced 1,284 files · 4.2 GB in 3m 12s",
  **Eject** button (if drive target) + Close. No auto-close when the drive can be ejected; otherwise
  auto-close after 6 s, paused on hover/focus.
- Error: one message (no duplicate), cause in plain words, Retry + Open logs.
- `role="status"` live region announcing 10% steps, not every tick.

## Accessibility & keyboard

- Tabs: proper ARIA tabs (roving tabindex, ←/→, Home/End, `aria-controls`/`aria-labelledby`).
- Profile list: listbox with ↑/↓ or plain buttons list; pick one pattern and do it fully.
- Toggles `role="switch"` with `aria-checked`. Status never color-only.
- Logs: not `aria-live`; add Copy and Auto-scroll toggle; raise cap to 2,000 lines.
- Shortcuts: ⌘S save, ⌘R sync now, ⌘. cancel, ⌘, preferences (Ctrl on Win/Linux). Esc closes dialogs.
- Landmarks: `<main>` on both pages; nav outside the tablist role.

## Copy

- Say "drive" only for the physical disk; Google Drive is always "Google Drive".
- Explain globs inline with 2 examples (`*.tmp`, `node_modules`). Placeholders are clearly examples
  ("e.g. Extreme Pro").
- Google setup: 3 numbered steps with the console link; Desktop app OAuth (no redirect URI to configure).
- Empty first-run state: "Plug in a drive or choose a folder to start" with the 3 steps.
- Numbers formatted with locale separators; "Succeeded" / "Failed" badges with failure reason on hover
  and in a details row.

## Verify

- cargo build zero warnings, cargo test green, app launches 10 s with no panic.
