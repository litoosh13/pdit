# pdit's patched copy of pdfium-render 0.9.4

This folder is pdfium-render 0.9.4 exactly as published on crates.io
(https://crates.io/crates/pdfium-render/0.9.4, upstream https://github.com/ajrcarey/pdfium-render),
MIT OR Apache-2.0 (see `LICENSE.md`), used through `[patch.crates-io]` in the workspace `Cargo.toml`.

## Why

pdit fills PDF form fields the way a person typing does, through PDFium's form-fill calls
(`FORM_SetFocusedAnnot`, `FORM_ReplaceSelection`, `FORM_OnChar`, ...), so that the filled values get a
fresh appearance and are visible in every viewer. pdfium-render's bindings implement those calls, but in
0.9.4 the handles and the bindings accessor they need are crate-private.

## Visibility changes

- `src/pdfium.rs`: `PdfiumLibraryBindingsAccessor` is `pub` instead of `pub(crate)` (both the
  `thread_safe` and the plain variant). The upstream author called the `pub(crate)` a mistake in
  ajrcarey/pdfium-render#254.
- `src/pdf/document/form.rs`: `PdfForm::handle()` is `pub`.
- `src/pdf/document.rs`: `PdfDocument::handle()` is `pub`.
- `src/pdf/document/page.rs`: `PdfPage::page_handle()` is `pub`.

## Behaviour fix (2026-10-08)

- `src/bindings/wasm_bindings.rs`, `FPDFFont_GetFontData`: the first, length-only call (`buffer` null,
  `buflen` 0) copied the font into the null buffer and panicked (`ptr::copy` precondition) in the browser
  build. Now it copies only when a buffer is given, and at most `buflen` bytes. Needed by `PdfFont::data()`,
  which pdit uses to check a subset font has glyphs for new text.

## Behaviour fix (2026-10-09, P-043)

- `src/pdf/document/page/annotation/private.rs`, the fallbacks of `fill_color_impl`, `set_fill_color_impl`,
  `stroke_color_impl` and `set_stroke_color_impl` (taken when `FPDFAnnot_GetColor` / `SetColor` fail, e.g. an
  annotation with an appearance stream): upstream passed the annotation handle itself to
  `FPDFPageObj_GetStrokeColor` and friends (`self.handle() as FPDF_PAGEOBJECT`), so PDFium read — or, on set,
  wrote — an annotation as if it were a page object. In the browser build this gave "memory access out of bounds"
  in `FPDFPageObj_GetStrokeColor` on some runs (P-043) and could corrupt memory on set. Now they use the
  annotation's first appearance object (`FPDFAnnot_GetObject(annot, 0)`), or null when it has none (PDFium then
  returns false: no colour). New helper `first_object_or_null` on `PdfPageAnnotationPrivate`.

- `src/pdf/document/page/text.rs`, new `chars_for_objects` (2026-10-16, Find speed, P-050 #9): the characters of
  several text objects in one pass over the page's characters. Upstream's `chars_for_object` calls
  `FPDFText_GetTextObject` for every character of the page per object, so Find on an 80-page document with matches
  in most lines took 37 s.

## Removed from the copy (not needed to build pdit)

- `src/bindgen/pdfium_*.rs` except `pdfium_7881.rs` (pdit uses the `pdfium_latest` feature = 7881; the
  others are only included under their own feature flags).
- `include/` (C headers, used only by the `bindings` feature to regenerate the bindgen files), `test/`,
  `.github/`, `Cargo.lock`, and the crates.io packaging markers.

## Updating

Copy the new release from crates.io, apply the four visibility changes above, remove the same files, and
update the version pin in the workspace `Cargo.toml`.

## Behaviour fix (2026-10-28, D-081)

- `src/bindings/wasm_bindings.rs`, `FPDFAnnot_GetInkListPath`: the browser build passed three arguments to PDFium's
  four-argument function (the buffer `length` was left out), so PDFium only returned the point count and never filled
  the buffer; pdit then read uninitialised memory as stroke points. Now `length` is passed. Needed to move a pen
  drawing's `/InkList` along with it.
