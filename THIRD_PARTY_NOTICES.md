# Third-party notices

pdit is AGPL-3.0 ([LICENSE](LICENSE)). It includes or downloads the following, each under its own licence.
Full licence texts are in [licenses/](licenses).

## Built into pdit

| Part | Licence | Text |
|---|---|---|
| PDFium (via pdfium-lib 8046d) and the libraries it builds with: Anti-Grain Geometry, BigInt, FreeType, Little CMS, OpenJPEG, LibTIFF, zlib, libpng, libjpeg-turbo, Abseil, ICU, fast_float, Dragonbox | BSD-3-Clause and others | [licenses/pdfium.md](licenses/pdfium.md) |
| Rust crates (Dioxus, pdfium-render, lopdf, allsorts, tract, image, …), browser build | MIT, Apache-2.0 and others | [licenses/rust-crates.md](licenses/rust-crates.md) |
| Rust crates of the desktop app (Tauri, leafmind-ocr, leafmind-qa, ort, tokenizers, …) | MIT, Apache-2.0 and others | [licenses/rust-crates-desktop.md](licenses/rust-crates-desktop.md) |
| MuPDF and the libraries it builds with (desktop app: text editing, D-065): mupdf / mupdf-sys crates, jbig2dec, URW base-14 fonts; FreeType, HarfBuzz, Gumbo, Little CMS, libjpeg, OpenJPEG, zlib | AGPL-3.0; FreeType License, MIT, Apache-2.0, IJG, BSD-2-Clause, zlib | [licenses/mupdf.md](licenses/mupdf.md) |
| [leafmind](https://github.com/litoosh13/leafmind) (fields, OCR, questions) | AGPL-3.0 | [LICENSE](LICENSE) |
| React, React DOM, Scheduler; liquid-gooey; Devigner UI; Devigner Icons (artwork: Solar Icon Set by 480 Design, CC BY 4.0, and Iconsax by Vuesax; modified); Motion; cn; thinking-orbs | MIT; CC BY 4.0; Iconsax free use | [licenses/other.md](licenses/other.md) |
| Beautiful UI components (ported) | MIT | [licenses/other.md](licenses/other.md) |
| Tesseract OCR and its language files; Leptonica (macOS app) | Apache-2.0; BSD-2-Clause | [licenses/other.md](licenses/other.md) |
| Form-field models `form-field-v1-nano`, `form-field-v1-state` by Nutrient | Apache-2.0 | [crates/pdit-app/assets/models/SOURCES.md](crates/pdit-app/assets/models/SOURCES.md), [licenses/apache-2.0.txt](licenses/apache-2.0.txt) |
| Fonts: Inter, Noto Sans, Liberation, Carlito, Caladea; Vazirmatn (desktop app) | SIL Open Font License 1.1 | [licenses/fonts.md](licenses/fonts.md) |
| Animations from [Transitions.dev](https://transitions.dev) | Transitions.dev's terms | noted in each CSS file that uses one |

The CSS taken from Transitions.dev is used under its authors' own terms. It is **not** covered by pdit's AGPL-3.0
licence; reuse it only as those terms allow.

## Downloaded later, only if you ask questions (macOS app)

| Part | Licence | Source |
|---|---|---|
| ONNX Runtime 1.30 | MIT (its own third-party notices come in the same archive) | https://github.com/microsoft/onnxruntime |
| gte-multilingual-base and gte-multilingual-reranker-base (int8 ONNX) | Apache-2.0 | https://huggingface.co/onnx-community |
