# UI fix list (re-critique 27/40)

Fix all items in one batch. Keep everything else from docs/ui-redesign-brief.md intact.

## P1
1. Refresh loop wipes UI state: `setInterval(reload, 1500)` (settings.js ~968) rebuilds sidebar/history/logs. Replace with event-driven updates (listen to Rust events) or diff-patch only changed text nodes; never re-render focused/expanded/selected elements. Profile listbox must keep focus on ArrowDown/Up/Home/End. Log view must not clear text selection.
2. Popup preview cuts off confirm button at 440x460: make the preview list scroll inside a fixed-height region with header + sticky footer buttons always visible.
3. Inline labels break forms (Name, Drive name, Host, Share, Username, Client ID/secret): stacked label-above-input everywhere, consistent grid; NAS: Host+Share row, Username+Password row, Auto-mount switch below.
4. Popup Retry calls sync_now confirmed:true: mirror profiles must go through preview again.

## P2
5. Log WARN lines reuse `.warn` banner class: rename log level classes (`.log-warn`, etc.).
6. Sync now while form dirty: disable with reason "Save changes first" (or save then sync); blockReason must use saved profile.
7. Preferences below fold at 640px: sidebar full-height sticky, list scrolls, Preferences pinned at bottom.
8. "Safe to unplug" before eject: say "Sync complete" + Eject; only after eject success say "Safe to unplug".
9. Failure states: popup heading "Sync failed", hide stale bar/stats, plain-language cause, Retry primary, Open logs. Main hero: show reason + Retry, and still show drive connected/disconnected.
10. No-op preview: single "Already up to date" state, no empty groups, one Close button.
11. Preview header shows profile name -> destination.
12. History: filter to current profile (or label as "All profiles" with a profile column); match by profile id not name (add id to SyncRecord in core with serde default + test); no wrapping (nowrap numeric columns, tabular-nums).
13. Hide Eject for Google Drive/cloud targets; disabled outline buttons clearly distinct in dark mode (lower opacity + no border emphasis + cursor).
14. New profiles: core defaults for source/ssd_name must be empty (placeholders only); keep config migration tests green; update tests.

## a11y (axe)
15. `#redirect-uri` needs a label.
16. `aria-expanded` on `<tr>`: use a disclosure `<button aria-expanded>` inside the row.
17. Save bar inside a landmark (e.g. within `<main>` or `role="region" aria-label="Unsaved changes"`).
18. Popup heading order: h1 -> h2 for groups.
19. Switch help text linked via `aria-describedby`; popup sets initial focus (Cancel during sync, primary in done/error/preview).

## P3
20. Switches get a visible knob. Remove duplicate percent in syncing hero; label ETA. Mirror warning text not a copy of the radio label. Spacing between Name input and target control. Explain "Active" with help text ("Tray menu actions use this profile"). Finished popup hides empty Speed/ETA.

## Verify (one round)
cargo build zero warnings, cargo test green; detector at scratchpad detrun/scripts/detect.mjs on index.html + popup.html clean; launch app 10s no panic.
