#!/usr/bin/env python3
"""Generate fixtures/synthetic-split-lines.pdf: a synthetic page whose lines are
split into several text objects, some in the middle of a word, with a shape
between two pieces and a space-only piece, as many PDF writers do. Courier 10
(every character 6 pt wide), so the pieces sit exactly end to end.
Synthetic text only — never a look-alike of the user's documents."""
import sys

LEFT, SIZE, CHAR = 72, 10, 6.0

# Each line: baseline and its pieces (one text object each). None = a shape.
LINES = [
    # A wrapped sentence over two lines, split mid-word on both.
    (740, ["The garden club keeps a shared shed where members bor", None, "row tools,"]),
    (727, ["provided each tool is cleaned and hung on its hook be", "fore dusk."]),
    # A standalone line in three pieces, the middle one only a space.
    (690, ["Visitors", " ", "sign the book."]),
]


def esc(s):
    return s.replace("\\", "\\\\").replace("(", "\\(").replace(")", "\\)")


parts = []
for y, pieces in LINES:
    x = LEFT
    for piece in pieces:
        if piece is None:
            parts.append(f"0.8 g {LEFT} 760 40 4 re f 0 g\n")
            continue
        parts.append(f"BT /F1 {SIZE} Tf {x:.1f} {y} Td ({esc(piece)}) Tj ET\n")
        x += CHAR * len(piece)
content = "".join(parts).encode("latin-1")

objs = [
    b"<</Type/Catalog/Pages 2 0 R>>",
    b"<</Type/Pages/Kids[3 0 R]/Count 1>>",
    b"<</Type/Page/Parent 2 0 R/MediaBox[0 0 595 842]/Resources<</Font<</F1 5 0 R>>>>/Contents 4 0 R>>",
    b"<</Length %d>>\nstream\n" % len(content) + content + b"endstream",
    b"<</Type/Font/Subtype/Type1/BaseFont/Courier>>",
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

path = sys.argv[1] if len(sys.argv) > 1 else "fixtures/synthetic-split-lines.pdf"
with open(path, "wb") as f:
    f.write(out)
print("wrote", path, len(out), "bytes")
