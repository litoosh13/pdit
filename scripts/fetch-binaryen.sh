#!/bin/sh
# Puts binaryen where dx 0.7.10 looks for it first, pinned and checksum-verified. Left to itself dx downloads it without
# checking the answer: a 504 from GitHub leaves no wasm-opt and fails the build later with "Failed to run wasm-opt: No
# such file or directory". dx 0.7.10 fetches version 127 into a folder named binaryen-129: both go here on an upgrade.
set -eu

VERSION=127
FOLDER=binaryen-129
case "$(uname -s)-$(uname -m)" in
    Linux-x86_64) PLATFORM=x86_64-linux SHA256=c8ebe5d00c978601086bcad33b2c80fcfe33d6a8b87b754ba4ea86a9a16cc316 ;;
    Darwin-arm64) PLATFORM=arm64-macos SHA256=5b53afbf6b71b3f41e5c896855b41f9f0bcda8d3666e8703d8bd1f65a9b4f767 ;;
    Darwin-x86_64) PLATFORM=x86_64-macos SHA256=c814a6aa734f652867de080cf092830de476f2ee2714b3998b14dea7859f3bd7 ;;
    MINGW*-x86_64 | MSYS*-x86_64) PLATFORM=x86_64-windows SHA256=1f771649683901951531a2753ee95220939841c81bf4331a306c1b3298c02544 ;;
    *) echo "No binaryen pinned for $(uname -s)-$(uname -m)" >&2; exit 1 ;;
esac
DEST="$HOME/.dx/tools/$FOLDER"
if [ -e "$DEST/bin/wasm-opt" ] || [ -e "$DEST/bin/wasm-opt.exe" ]; then
    echo "binaryen $VERSION already in $DEST"
    exit 0
fi

archive="$(mktemp)"
trap 'rm -f "$archive"' EXIT
echo "Downloading binaryen $VERSION ($PLATFORM)"
curl -fsSL --retry 8 --retry-delay 15 --retry-all-errors --connect-timeout 30 -o "$archive" \
    "https://github.com/WebAssembly/binaryen/releases/download/version_$VERSION/binaryen-version_$VERSION-$PLATFORM.tar.gz"

if command -v sha256sum >/dev/null; then
    echo "$SHA256  $archive" | sha256sum -c -
else
    echo "$SHA256  $archive" | shasum -a 256 -c -
fi

mkdir -p "$DEST"
tar -xzf "$archive" -C "$DEST" --strip-components=1 "binaryen-version_$VERSION/bin" "binaryen-version_$VERSION/lib"
echo "binaryen $VERSION installed in $DEST"
