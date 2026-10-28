#!/bin/sh
# Downloads leafmind-fill's browser package (D-080: filling a form from a sentence online), verifies its checksum and
# unpacks it next to the bridge (assets/ai-fill/leafmind-fill: a folder asset, which dx copies unchanged). The models
# are not in it: the online version downloads them only when the user asks (assets/ai-fill/ai-fill.js).
set -eu

TAG=v0.9.0
SHA256=05e609c265745c4f29a4e021e24e38d1a2856a403401147cc5067cb536125bad
URL="https://github.com/litoosh13/leafmind/releases/download/$TAG/leafmind-fill-wasm-$TAG.zip"
DEST=crates/pdit-app/assets/ai-fill/leafmind-fill

cd "$(dirname "$0")/.."

zip="$(mktemp)"
trap 'rm -f "$zip"' EXIT
echo "Downloading leafmind-fill $TAG (browser)"
curl -fsSL -o "$zip" "$URL"

if command -v sha256sum >/dev/null; then
    echo "$SHA256  $zip" | sha256sum -c -
else
    echo "$SHA256  $zip" | shasum -a 256 -c -
fi

rm -rf "$DEST"
mkdir -p "$DEST"
unzip -o -j -q "$zip" 'leafmind-fill-wasm/*' -d "$DEST"
rm -f "$DEST/.gitignore" "$DEST/package.json" "$DEST"/*.d.ts
echo "leafmind-fill $TAG installed in $DEST"
