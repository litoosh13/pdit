#!/usr/bin/env python3
"""Generate fixtures/synthetic-rows.pdf: a made-up garden club registration printed as "LABEL: value" rows in
capitals (a flat text form, no fields), for the self-check's questions on forms (leafmind 0.8: rows stay lines,
capital labels score). Synthetic text only — never a look-alike of the user's documents."""
import sys

ROWS = [
    "GARDEN CLUB REGISTRATION - not personal data",
    "MEMBER NAME: Robin Example",
    "CLUB NAME: Mossbury Allotment Society",
    "CLUB ADDRESS: 3 Orchard Road, Mossbury",
    "PLOT SIZE: Half plot",
    "START OF SEASON: 1 March 2027",
    "YEARLY FEE: 45 Euro",
    "WATER ACCESS: Shared tap",
]


def esc(s):
    return s.replace("\\", "\\\\").replace("(", "\\(").replace(")", "\\)")


content = "".join(
    f"BT /F1 11 Tf 72 {760 - 22 * i} Td ({esc(row)}) Tj ET\n" for i, row in enumerate(ROWS)
).encode("latin-1")

objs = [
    b"<</Type/Catalog/Pages 2 0 R>>",
    b"<</Type/Pages/Kids[3 0 R]/Count 1>>",
    b"<</Type/Page/Parent 2 0 R/MediaBox[0 0 595 842]/Resources<</Font<</F1 5 0 R>>>>/Contents 4 0 R>>",
    b"<</Length %d>>\nstream\n" % len(content) + content + b"endstream",
    b"<</Type/Font/Subtype/Type1/BaseFont/Helvetica>>",
]

out = bytearray(b"%PDF-1.4\n")
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

path = sys.argv[1] if len(sys.argv) > 1 else "fixtures/synthetic-rows.pdf"
with open(path, "wb") as f:
    f.write(out)
print("wrote", path, len(out), "bytes")
