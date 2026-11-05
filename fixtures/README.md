# Test fixtures

Synthetic files for pdit's self-checks. None contain personal data.

| File | What it is |
|---|---|
| `synthetic-rental.pdf` | A made-up rental agreement, 1 page, Helvetica (not embedded). |
| `synthetic-subset-font.pdf` | A made-up lease in an embedded Noto Sans subset (typing new characters needs a fallback font). |
| `synthetic-wrapped.pdf` | Wrapped paragraphs for the reflow editor (`scripts/make_wrapped_fixture.py`). |
| `synthetic-split-lines.pdf` | Lines split into several text objects, even mid-word, with a shape between pieces (`scripts/make_split_fixture.py`). |
| `synthetic-form.pdf` | A small fillable form: two text fields, a checkbox, a dropdown (`scripts/make_form_fixture.py`). |
| `synthetic-rows.pdf` | A made-up garden club registration as "LABEL: value" rows in capitals, a flat text form for the question checks (`scripts/make_rows_fixture.py`). |
| `synthetic-redact.pdf` | Three made-up pages with words starting SECRET planted in the places redaction can miss: invisible and covered text, marked content, form XObjects, layers, patterns, fields, annotations, tags, thumbnails, attachments, metadata, an old revision (`scripts/make_redact_fixture.py`). |
| `synthetic-persian.pdf` | Three made-up Persian lines in Vazirmatn (`cargo run -p pdit-mupdf --example persian_fixture`). |
| `synthetic-image.png` | A small image for the image tools. |
| `synthetic-scan.pdf` | Two made-up pages as images for Clean scan: a flat scan with tint, uneven light and a shadow; a bent page photographed on a dark table (`scripts/make_scan_fixture.py`). |
