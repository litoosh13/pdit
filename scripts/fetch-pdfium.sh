#!/bin/sh
# Downloads the pinned PDFium WebAssembly build, verifies its checksum, and puts
# pdfium.js + pdfium.wasm into the app's assets. Set PDFIUM_ZIP to use an
# already-downloaded wasm.zip instead of downloading.
set -eu

RELEASE=8046d
SHA256=062d10055de90a01120acf307ce16e0022021a32203f4aeae4877750f9a08427
URL="https://github.com/paulocoutinhox/pdfium-lib/releases/download/$RELEASE/wasm.zip"
DEST=crates/pdit-app/assets/pdfium

cd "$(dirname "$0")/.."

zip="${PDFIUM_ZIP:-}"
if [ -z "$zip" ]; then
    zip="$(mktemp)"
    trap 'rm -f "$zip"' EXIT
    echo "Downloading PDFium WASM $RELEASE"
    curl -fsSL -o "$zip" "$URL"
fi

if command -v sha256sum >/dev/null; then
    echo "$SHA256  $zip" | sha256sum -c -
else
    echo "$SHA256  $zip" | shasum -a 256 -c -
fi

mkdir -p "$DEST"
unzip -o -j -q "$zip" release/node/pdfium.js release/node/pdfium.wasm -d "$DEST"
echo "PDFium $RELEASE installed in $DEST"
