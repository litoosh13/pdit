# Test fixtures

Synthetic files for pdit's self-checks. None contain personal data.

| File | What it is |
|---|---|
| `synthetic-rental.pdf` | A made-up rental agreement, 1 page, Helvetica (not embedded). |
| `synthetic-subset-font.pdf` | A made-up lease in an embedded Noto Sans subset (typing new characters needs a fallback font). |
| `synthetic-wrapped.pdf` | Wrapped paragraphs for the reflow editor (`scripts/make_wrapped_fixture.py`). |
| `synthetic-split-lines.pdf` | Lines split into several text objects, even mid-word, with a shape between pieces (`scripts/make_split_fixture.py`). |
| `synthetic-form.pdf` | A small fillable form: two text fields, a checkbox, a dropdown (`scripts/make_form_fixture.py`). |
| `synthetic-image.png` | A small image for the image tools. |
