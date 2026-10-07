# pdit desktop app

The pdit web build in a [Tauri 2](https://v2.tauri.app/) window.
PDFs stay on this computer; nothing is uploaded. Saving opens a native Save dialog.

## Build the macOS .dmg

Prerequisites (once):

```sh
cargo install tauri-cli --version "^2" --locked   # cargo tauri
cargo install dioxus-cli                           # dx, in ~/.cargo/bin
rustup component add llvm-tools                    # dx strips the wasm with rust-objcopy
```

Then, from this folder:

```sh
rustup target add x86_64-apple-darwin                          # once, for Intel Macs
cargo tauri build --target universal-apple-darwin --bundles dmg   # Intel + Apple Silicon
```

Tauri first fetches the OCR files (`scripts/fetch-ocr.sh`: Tesseract + language files, pinned and checksum-verified), then runs the release web build (`dx build --release --platform web` in `crates/pdit-app`,
using the `dx` from `~/.cargo/bin` — another `dx`, e.g. Deno's, may come first on `PATH`), then
builds the app. The disk image lands in `target/universal-apple-darwin/release/bundle/dmg/`.

The app is ad-hoc signed only: on another Mac, macOS asks once to allow it
(System Settings → Privacy & Security → Open Anyway) until it is signed with an Apple Developer ID
and notarised.

The app icon's source is `icons/app-icon.svg`; regenerate the sizes with `cargo tauri icon icons/app-icon.svg -o icons`
(then delete the Android/iOS/Store files it also writes).
