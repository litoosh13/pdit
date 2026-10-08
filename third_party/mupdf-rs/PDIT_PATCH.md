# pdit's patched copy of the `mupdf` crate 0.8.0

This folder is the `mupdf` crate 0.8.0 (safe Rust bindings to MuPDF) as published on crates.io
(https://crates.io/crates/mupdf/0.8.0, upstream https://github.com/messense/mupdf-rs), AGPL-3.0 (see
`LICENSE`), used through `[patch.crates-io]`. MuPDF itself (C) comes from the unmodified `mupdf-sys` 0.8.0 crate.

## Why

pdit edits text with MuPDF's Story layout engine (HTML/CSS layout with HarfBuzz shaping: re-wrapped paragraphs,
Persian/Arabic). The safe API has no Story binding, so pdit calls MuPDF through its own small C helper
(`crates/pdit-mupdf/src/story.c`), which needs the context and the raw document/page pointers.

## Changes (only visibility; no behaviour changed)

- `src/context.rs`: `context()` is `pub` instead of `pub(crate)`.
- `src/pdf/document.rs`: `PdfDocument::as_raw()` is `pub` instead of `pub(crate)`.
- `src/pdf/page.rs`: `PdfPage::as_raw()` added (returns the private `inner` pointer).

## Removed from the copy

- `tests/` (10 MB of fixtures, used only by the crate's own tests), `examples/`, and the `[[test]]`,
  `[[example]]` and `[dev-dependencies]` sections of `Cargo.toml`. The crate's `#[cfg(test)]` modules still name
  `tests/` files; pdit never runs them.

## Updating

Copy the new published crate over this folder, delete `tests/` and `examples/` and their manifest sections again,
and re-apply the three changes above.
