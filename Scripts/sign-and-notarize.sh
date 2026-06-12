#!/usr/bin/env bash
#
# Sign with Developer ID, notarize via App Store Connect API, and staple.
# Adapted from the codexbar release flow (https://github.com/steipete/codexbar).
#
# Requires a PAID Apple Developer account and these env vars:
#   APP_IDENTITY                 "Developer ID Application: Your Name (TEAMID)"
#   APP_STORE_CONNECT_API_KEY_P8 contents of the .p8 key (or set ..._P8_PATH)
#   APP_STORE_CONNECT_KEY_ID     the key ID
#   APP_STORE_CONNECT_ISSUER_ID  the issuer ID
#
# Result: SyncDrop.app (stapled) + SyncDrop.zip ready to upload to a release.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

APP_NAME="SyncDrop"
APP_BUNDLE="$APP_NAME.app"
ZIP_NAME="$APP_NAME.zip"

: "${APP_IDENTITY:?Set APP_IDENTITY to your Developer ID Application identity}"
if [[ -z "${APP_STORE_CONNECT_KEY_ID:-}" || -z "${APP_STORE_CONNECT_ISSUER_ID:-}" ]]; then
  echo "Missing APP_STORE_CONNECT_KEY_ID / APP_STORE_CONNECT_ISSUER_ID." >&2
  exit 1
fi

# Build + assemble the bundle, signed with the real Developer ID + hardened runtime.
make app SIGN_ID="$APP_IDENTITY"

# Materialize the API key.
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

# Notarize the bundle.
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

# Clean packaging hygiene, then the distributable zip.
xattr -cr "$APP_BUNDLE"
find "$APP_BUNDLE" -name '._*' -delete
rm -f "$ZIP_NAME"
/usr/bin/ditto --norsrc -c -k --keepParent "$APP_BUNDLE" "$ZIP_NAME"

echo "Verifying…"
spctl -a -t exec -vv "$APP_BUNDLE"
xcrun stapler validate "$APP_BUNDLE"

echo "✓ Notarized + stapled: $ZIP_NAME"
