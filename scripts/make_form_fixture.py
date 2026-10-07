#!/usr/bin/env python3
"""Generate fixtures/synthetic-form.pdf: a synthetic AcroForm with a few real
form fields (text, checkbox, combo) to test fill-form detection and filling.
Synthetic text only — never a look-alike of the user's documents."""
import sys

# label text drawn on the page (Helvetica 11), and the field widgets.
LABELS = [
    (708, "Synthetic membership form - not personal data"),
    (704 - 704, ""),  # spacer (ignored)
]
CONTENT = (
    "BT /Helv 13 Tf 72 792 Td (Synthetic membership form - not personal data) Tj ET\n"
    "BT /Helv 11 Tf 72 705 Td (Full name) Tj ET\n"
    "BT /Helv 11 Tf 72 665 Td (Email) Tj ET\n"
    "BT /Helv 11 Tf 72 625 Td (I agree to the terms) Tj ET\n"
    "BT /Helv 11 Tf 72 585 Td (Country) Tj ET\n"
).encode("latin-1")

objs = [
    # 1 Catalog + AcroForm
    b"<</Type/Catalog/Pages 2 0 R/AcroForm<</Fields[4 0 R 5 0 R 6 0 R 7 0 R]"
    b"/NeedAppearances true/DA(/Helv 0 Tf 0 g)/DR<</Font<</Helv 8 0 R>>>>>>>>",
    # 2 Pages
    b"<</Type/Pages/Kids[3 0 R]/Count 1>>",
    # 3 Page
    b"<</Type/Page/Parent 2 0 R/MediaBox[0 0 595 842]"
    b"/Resources<</Font<</Helv 8 0 R>>>>/Annots[4 0 R 5 0 R 6 0 R 7 0 R]/Contents 9 0 R>>",
    # 4 text: full_name (empty)
    b"<</Type/Annot/Subtype/Widget/FT/Tx/T(full_name)/Rect[150 700 450 720]"
    b"/P 3 0 R/F 4/DA(/Helv 12 Tf 0 g)/MK<</BC[0.6 0.6 0.6]>>>>",
    # 5 text: email (has a value)
    b"<</Type/Annot/Subtype/Widget/FT/Tx/T(email)/V(a.member@example.org)"
    b"/Rect[150 660 450 680]/P 3 0 R/F 4/DA(/Helv 12 Tf 0 g)/MK<</BC[0.6 0.6 0.6]>>>>",
    # 6 checkbox: agree (off)
    b"<</Type/Annot/Subtype/Widget/FT/Btn/T(agree)/V/Off/AS/Off"
    b"/Rect[150 620 168 638]/P 3 0 R/F 4/MK<</BC[0.6 0.6 0.6]>>>>",
    # 7 combo: country
    b"<</Type/Annot/Subtype/Widget/FT/Ch/Ff 131072/T(country)"
    b"/Opt[(Germany)(France)(Spain)(Italy)]/Rect[150 580 350 600]"
    b"/P 3 0 R/F 4/DA(/Helv 12 Tf 0 g)/MK<</BC[0.6 0.6 0.6]>>>>",
    # 8 Font
    b"<</Type/Font/Subtype/Type1/BaseFont/Helvetica/Name/Helv>>",
    # 9 Contents
    b"<</Length %d>>\nstream\n" % len(CONTENT) + CONTENT + b"endstream",
]

out = bytearray(b"%PDF-1.7\n")
offsets = []
for i, body in enumerate(objs, start=1):
    offsets.append(len(out))
    out += b"%d 0 obj\n" % i + body + b"\nendobj\n"
xref = len(out)
out += b"xref\n0 %d\n" % (len(objs) + 1)
out += b"0000000000 65535 f \n"
for off in offsets:
    out += b"%010d 00000 n \n" % off
out += b"trailer\n<</Size %d/Root 1 0 R>>\nstartxref\n%d\n%%%%EOF\n" % (len(objs) + 1, xref)

path = sys.argv[1] if len(sys.argv) > 1 else "fixtures/synthetic-form.pdf"
with open(path, "wb") as f:
    f.write(out)
print("wrote", path, len(out), "bytes")
