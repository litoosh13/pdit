#!/bin/sh
# Fetches what the desktop app's OCR needs (D-055) into desktop/resources/ocr/, at pinned versions with SHA-256
# checks; files already there with the right checksum are kept. Run by Tauri's beforeBuildCommand.
#  - lib/: libtesseract.5.dylib (universal, macOS 11+) + its licences, from leafmind's v0.2.0 release.
#  - tessdata/: English, German, Persian, Arabic (tessdata_best) and osd (tessdata), as leafmind's
#    scripts/fetch-tessdata.sh pins them (sources and licences: leafmind THIRD_PARTY.md).
# Nothing here is downloaded at run time; the app reads these files from its bundle.
set -eu
here=$(cd "$(dirname "$0")/.." && pwd)
out="$here/resources/ocr"
mkdir -p "$out/lib" "$out/tessdata"

sha256() { shasum -a 256 "$1" | cut -d' ' -f1; }
fetch() { # url file sha256
    if [ -f "$2" ] && [ "$(sha256 "$2")" = "$3" ]; then return; fi
    echo "downloading $1"
    curl -fL --retry 3 -o "$2.part" "$1"
    if [ "$(sha256 "$2.part")" != "$3" ]; then echo "checksum mismatch: $2" >&2; rm -f "$2.part"; exit 1; fi
    mv "$2.part" "$2"
}

zip="$out/tesseract-macos-v0.2.0.zip"
fetch https://github.com/litoosh13/leafmind/releases/download/v0.2.0/tesseract-macos-v0.2.0.zip "$zip" \
    0b2c25e5af2f45ca5437d6430c332733e7419410522968f378e7ab54b6b79d8b
if [ ! -f "$out/lib/libtesseract.5.dylib" ]; then
    unzip -o -q -j "$zip" -d "$out/lib"
fi

while read -r repo commit file sum; do
    fetch "https://github.com/tesseract-ocr/$repo/raw/$commit/$file" "$out/tessdata/$file" "$sum"
done <<'LIST'
tessdata_best e12c65a915945e4c28e237a9b52bc4a8f39a0cec eng.traineddata 8280aed0782fe27257a68ea10fe7ef324ca0f8d85bd2fd145d1c2b560bcb66ba
tessdata_best e12c65a915945e4c28e237a9b52bc4a8f39a0cec deu.traineddata 8407331d6aa0229dc927685c01a7938fc5a641d1a9524f74838cdac599f0d06e
tessdata_best e12c65a915945e4c28e237a9b52bc4a8f39a0cec fas.traineddata 99e420969b5ddd2cb135b416316a7ed417c59c4faf9e0d28941348f6448114df
tessdata_best e12c65a915945e4c28e237a9b52bc4a8f39a0cec ara.traineddata ab9d157d8e38ca00e7e39c7d5363a5239e053f5b0dbdb3167dde9d8124335896
tessdata ced78752cc61322fb554c280d13360b35b8684e4 osd.traineddata e19f2ae860792fdf372cf48d8ce70ae5da3c4052962fe22e9de1f680c374bb0e
LIST
echo "ocr files ready: $out"
