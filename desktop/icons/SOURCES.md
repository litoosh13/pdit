# Desktop app icon sources

| File | Source | Used for |
|---|---|---|
| `app-icon.svg` | Designed for pdit (option D "Highlight": lines of text, one highlighted, with a blue text caret, on paper), chosen by the user 2026-10-05 (D-048). Drawn as plain SVG shapes, no fonts or third-party art. AGPL-3.0-only with the rest of pdit. | The app icon's source. |
| `32x32.png`, `128x128.png`, `128x128@2x.png`, `icon.icns`, `icon.ico` | Generated from `app-icon.svg` with `cargo tauri icon` | Window, Dock and installer icons. |
| `crates/pdit-app/public/pdit-icon.svg`, `pdit-192.png`, `pdit-512.png` | Copy of `app-icon.svg`; PNGs rendered from it with headless Chrome (2026-10-10) | The online version's tab and install icons (M-007). |
