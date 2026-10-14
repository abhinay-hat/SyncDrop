#!/usr/bin/env bash
#
# Notarize the Tauri macOS bundle and staple the ticket.
#
# Tauri signs the .app while building when APPLE_SIGNING_IDENTITY is set
# (https://v2.tauri.app/distribute/sign/macos/):
#
#   export APPLE_SIGNING_IDENTITY="Developer ID Application: Your Name (TEAMID)"
#   make desktop
#   ./Scripts/sign-and-notarize.sh
#
# APP_IDENTITY is accepted as another name for that same identity string.
# This script builds if you have not already, then submits the .app.
#
# Notarization uses an App Store Connect API key:
#   APP_STORE_CONNECT_KEY_ID
#   APP_STORE_CONNECT_ISSUER_ID
#   APP_STORE_CONNECT_API_KEY_P8   contents of the .p8 (or set ..._P8_PATH)
#
# Result: stapled SyncDrop.app under desktop/target/release/bundle/macos/
# and SyncDrop.zip in the repo root.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

export APPLE_SIGNING_IDENTITY="${APPLE_SIGNING_IDENTITY:-${APP_IDENTITY:-}}"
: "${APPLE_SIGNING_IDENTITY:?Set APPLE_SIGNING_IDENTITY to your Developer ID Application identity}"

if [[ -z "${APP_STORE_CONNECT_KEY_ID:-}" || -z "${APP_STORE_CONNECT_ISSUER_ID:-}" ]]; then
  echo "Missing APP_STORE_CONNECT_KEY_ID / APP_STORE_CONNECT_ISSUER_ID." >&2
  exit 1
fi

make desktop

APP_BUNDLE="desktop/target/release/bundle/macos/SyncDrop.app"
if [[ ! -d "$APP_BUNDLE" ]]; then
  echo "Tauri did not produce $APP_BUNDLE" >&2
  exit 1
fi

TMP="$(mktemp -d "${TMPDIR:-/tmp}/syncdrop-notarize.XXXXXX")"
chmod 700 "$TMP"
trap 'rm -rf "$TMP"' EXIT
KEY_PATH="$TMP/key.p8"
if [[ -n "${APP_STORE_CONNECT_API_KEY_P8_PATH:-}" ]]; then
  cp "$APP_STORE_CONNECT_API_KEY_P8_PATH" "$KEY_PATH"
else
  : "${APP_STORE_CONNECT_API_KEY_P8:?Set APP_STORE_CONNECT_API_KEY_P8 or ..._PATH}"
  ( umask 077; printf '%s' "$APP_STORE_CONNECT_API_KEY_P8" | sed 's/\\n/\n/g' > "$KEY_PATH" )
fi
chmod 600 "$KEY_PATH"

NOTARIZE_ZIP="$TMP/notarize.zip"
/usr/bin/ditto --norsrc -c -k --keepParent "$APP_BUNDLE" "$NOTARIZE_ZIP"

echo "Submitting for notarization…"
xcrun notarytool submit "$NOTARIZE_ZIP" \
  --key "$KEY_PATH" \
  --key-id "$APP_STORE_CONNECT_KEY_ID" \
  --issuer "$APP_STORE_CONNECT_ISSUER_ID" \
  --wait

echo "Stapling ticket…"
xcrun stapler staple "$APP_BUNDLE"

xattr -cr "$APP_BUNDLE"
find "$APP_BUNDLE" -name '._*' -delete
rm -f SyncDrop.zip
/usr/bin/ditto --norsrc -c -k --keepParent "$APP_BUNDLE" SyncDrop.zip

echo "Verifying…"
spctl -a -t exec -vv "$APP_BUNDLE"
xcrun stapler validate "$APP_BUNDLE"

echo "✓ Notarized + stapled: SyncDrop.zip"
