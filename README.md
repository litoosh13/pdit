# pdit

A private PDF editor. Your PDFs stay on your computer: nothing is uploaded.

Edit text and images, fill and create forms, comment, sign, add links and bookmarks, rearrange pages,
find and replace, print — and, with [leafmind](https://github.com/litoosh13/leafmind), find form fields,
read scanned pages (OCR) and answer questions about a document, all on your device.

pdit is written in Rust (PDFium for PDFs, Dioxus for the interface). It runs as a desktop app (macOS) and
in the browser. It is in early development.

## Download

macOS (Apple silicon and Intel): get the `.dmg` from [Releases](../../releases). The app isn't signed by Apple
yet, so the first time macOS asks: System Settings → Privacy & Security → **Open Anyway**.

Asking questions needs an Apple-silicon Mac and downloads its models (about 760 MB) the first time you ask.

## Build

```bash
scripts/fetch-pdfium.sh            # once: the pinned PDFium engine
docker compose run --rm islands    # the React parts of the interface
docker compose up serve            # browser version at http://localhost:8080
```

Desktop app: see [desktop/README.md](desktop/README.md).

## License

AGPL-3.0 — see [LICENSE](LICENSE). Third-party parts and their licences: [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
