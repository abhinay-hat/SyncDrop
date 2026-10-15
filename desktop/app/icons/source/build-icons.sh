#!/bin/bash
# Rasterize source/app-icon.svg and source/tray-template.svg into ../
set -euo pipefail
cd "$(dirname "$0")"
OUT="$(cd .. && pwd)"
RSVG=/opt/homebrew/bin/rsvg-convert
APP="$PWD/app-icon.svg"
TRAY="$PWD/tray-template.svg"

render() {
  "$RSVG" -w "$1" -h "$1" "$2" -o "$3"
}

render 32 "$APP" "$OUT/32x32.png"
render 128 "$APP" "$OUT/128x128.png"
render 256 "$APP" "$OUT/128x128@2x.png"
render 512 "$APP" "$OUT/icon.png"
render 44 "$TRAY" "$OUT/tray.png"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
SET="$WORK/SyncDrop.iconset"
mkdir -p "$SET"
render 16 "$APP" "$SET/icon_16x16.png"
render 32 "$APP" "$SET/icon_16x16@2x.png"
render 32 "$APP" "$SET/icon_32x32.png"
render 64 "$APP" "$SET/icon_32x32@2x.png"
render 128 "$APP" "$SET/icon_128x128.png"
render 256 "$APP" "$SET/icon_128x128@2x.png"
render 256 "$APP" "$SET/icon_256x256.png"
render 512 "$APP" "$SET/icon_256x256@2x.png"
render 512 "$APP" "$SET/icon_512x512.png"
render 1024 "$APP" "$SET/icon_512x512@2x.png"
/usr/bin/iconutil -c icns "$SET" -o "$OUT/icon.icns"

ICO="$WORK/ico"
mkdir -p "$ICO"
for size in 16 24 32 48 64 256; do
  render "$size" "$APP" "$ICO/$size.png"
done
python3 - "$OUT/icon.ico" "$ICO" << 'PY'
import struct
import sys
from pathlib import Path

out, folder = Path(sys.argv[1]), Path(sys.argv[2])
sizes = [16, 24, 32, 48, 64, 256]
blobs = [(size, (folder / f"{size}.png").read_bytes()) for size in sizes]
offset = 6 + 16 * len(blobs)
entries = b""
body = b""
for size, data in blobs:
    dim = 0 if size >= 256 else size
    entries += struct.pack("<BBBBHHII", dim, dim, 0, 0, 1, 32, len(data), offset)
    body += data
    offset += len(data)
out.write_bytes(struct.pack("<HHH", 0, 1, len(blobs)) + entries + body)
PY
