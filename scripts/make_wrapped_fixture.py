#!/usr/bin/env python3
"""Generate fixtures/synthetic-wrapped.pdf: a synthetic page with wrapped
paragraphs (one text object per visual line, Helvetica) to test reflow grouping.
Synthetic text only — never a look-alike of the user's documents."""
import sys

# (baseline_y, text). Left margin 72. One BT/Tj/ET per line = one text object.
LINES = [
    # Paragraph A (3 lines, wraps to a closing period)
    (740, "This wrapped clause continues across multiple lines purely to exercise the"),
    (724, "reflow editor, so selecting any single line should capture the whole block"),
    (708, "down to this closing period."),
    # Standalone short sentence (blank-line gap above)
    (672, "A separate short note stands alone."),
    # Paragraph C (2 lines)
    (636, "Another wrapped block appears lower on the page with two long lines that the"),
    (620, "grouping should also keep together as one paragraph."),
]

def esc(s):
    return s.replace("\\", "\\\\").replace("(", "\\(").replace(")", "\\)")

content = "".join(
    f"BT /F1 11 Tf 72 {y} Td ({esc(t)}) Tj ET\n" for y, t in LINES
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

path = sys.argv[1] if len(sys.argv) > 1 else "fixtures/synthetic-wrapped.pdf"
with open(path, "wb") as f:
    f.write(out)
print("wrote", path, len(out), "bytes")
