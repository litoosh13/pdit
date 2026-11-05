#!/usr/bin/env python3
"""fixtures/synthetic-redact.pdf: a made-up PDF with the places a redacted word could hide. Planted words start with SECRET."""
import sys, zlib
W, H = 40, 40
# A 40x40 image: left half dark red (to be redacted), right half green. Shared by both pages.
pixels = bytes(v for y in range(H) for x in range(W) for v in ((200, 0, 0) if x < 20 else (0, 200, 0)))
page1 = b"""BT /F1 14 Tf 72 740 Td (KEEP-LINE-ONE stays) Tj ET
BT /F1 14 Tf 72 700 Td (Name: SECRET-MIDLINE and keep-after) Tj ET
BT /F1 14 Tf 72 660 Td [(SECRET)-100(-KERNED)] TJ ET
BT 3 Tr /F1 14 Tf 72 640 Td (SECRET-INVISIBLE) Tj 0 Tr ET
BT /F1 14 Tf 72 620 Td (SECRET-COVERED) Tj ET
1 1 1 rg 70 615 200 18 re f 0 0 0 rg
/Span <</ActualText (SECRET-ACTUALTEXT)>> BDC BT /F1 14 Tf 72 590 Td (x-word) Tj ET EMC
/Span <</Alt (SECRET-ALT-TEXT)>> BDC BT /F1 14 Tf 72 570 Td (y-word) Tj ET EMC
q 1 0 0 1 72 540 cm /Fm1 Do Q
q 200 0 0 200 72 300 cm /Figure <</Alt (SECRET-FIGURE-ALT)>> BDC /Im1 Do EMC Q
/Span /MC0 BDC BT /F1 14 Tf 332 470 Td (z-word) Tj ET EMC
q 1 0 0 1 72 200 cm /Fm2 Do Q
/OC /L1 BDC BT /F1 14 Tf 72 520 Td (SECRET-LAYER) Tj ET EMC
/Pattern cs /P1 scn 320 300 200 30 re f 0 g
0 0 1 RG 2 w 320 400 m 500 400 l S
0.8 0.8 1 rg 320 340 100 40 re f
0 0 0 rg BT /F1 14 Tf 72 100 Td (KEEP-LINE-LAST stays) Tj ET
"""
form = b"BT /F1 14 Tf 0 0 Td (SECRET-FORMXOBJECT) Tj ET\n"
page2 = b"BT /F1 14 Tf 72 740 Td (KEEP-PAGE-TWO stays) Tj ET\nBT /F1 14 Tf 72 700 Td (SECRET-OTHERPAGE) Tj ET\nq 100 0 0 100 72 500 cm /Im1 Do Q\n"
page3 = b"BT /F1 14 Tf 72 500 Td (KEEP-ROTATED stays) Tj ET\nBT /F1 14 Tf 72 460 Td (SECRET-ROTATED) Tj ET\n"
tile = b"BT /F1 9 Tf 2 12 Td (SECRET-PATTERN) Tj ET\n"
oldrev = b"BT /F1 14 Tf 72 740 Td (SECRET-OLDREVISION) Tj ET\n"

def stream(data, extra=b"", compress=True):
    if compress:
        data = zlib.compress(data); extra += b" /Filter /FlateDecode"
    return b"<< /Length %d%s >>\nstream\n" % (len(data), extra) + data + b"\nendstream"

res = b"<< /Font << /F1 5 0 R >> /XObject << /Im1 6 0 R /Fm1 7 0 R /Fm2 33 0 R >> /Properties << /L1 28 0 R /MC0 34 0 R >> /Pattern << /P1 29 0 R >> >>"
objs = {
 1: b"<< /Type /Catalog /Pages 2 0 R /Outlines 15 0 R /StructTreeRoot 21 0 R /MarkInfo << /Marked true >> /OCProperties << /OCGs [28 0 R] /D << /OFF [28 0 R] >> >> /AcroForm << /XFA 27 0 R /Fields [14 0 R 23 0 R] /DA (/F1 12 Tf 0 g) /DR << /Font << /F1 5 0 R >> >> >> /Names << /EmbeddedFiles << /Names [(SECRET-ATTACH.txt) 19 0 R] >> >> /Metadata 18 0 R >>",
 2: b"<< /Type /Pages /Kids [3 0 R 4 0 R 30 0 R] /Count 3 >>",
 3: b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources " + res + b" /Contents 8 0 R /Annots [11 0 R 12 0 R 13 0 R 14 0 R 24 0 R 25 0 R 26 0 R] /Thumb 32 0 R >>",
 4: b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources " + res + b" /Contents 9 0 R >>",
 5: b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>",
 6: stream(pixels, b" /Type /XObject /Subtype /Image /Width 40 /Height 40 /ColorSpace /DeviceRGB /BitsPerComponent 8 /Alt (SECRET-IMAGE-ALT)"),
 7: stream(form, b" /Type /XObject /Subtype /Form /BBox [0 0 300 30] /Resources << /Font << /F1 5 0 R >> >>"),
 8: stream(page1),
 9: stream(page2),
 10: b"<< /Title (SECRET-INFO-TITLE) /Author (Made Up Person) >>",
 11: b"<< /Type /Annot /Subtype /FreeText /Rect [330 700 560 730] /Contents (SECRET-ANNOT-INSIDE) /DA (/F1 12 Tf 0 g) >>",
 12: b"<< /Type /Annot /Subtype /Link /Rect [330 660 560 690] /A << /S /URI /URI (https://example.com/SECRET-LINK) >> >>",
 13: b"<< /Type /Annot /Subtype /Text /Rect [20 20 40 40] /Contents (SECRET-ANNOT-OUTSIDE) >>",
 14: b"<< /Type /Annot /Subtype /Widget /FT /Tx /T (SECRET-FIELDNAME) /V (SECRET-FIELDVALUE) /Rect [330 620 560 650] /DA (/F1 12 Tf 0 g) >>",
 15: b"<< /Type /Outlines /First 16 0 R /Last 16 0 R /Count 1 >>",
 16: b"<< /Title (SECRET-BOOKMARK) /Parent 15 0 R /Dest [3 0 R /Fit] >>",
 17: stream(b"SECRET-ORPHAN-STREAM unreferenced\n"),
 18: stream(b"<x:xmpmeta xmlns:x='adobe:ns:meta/'><dc:title>SECRET-XMP</dc:title></x:xmpmeta>", b" /Type /Metadata /Subtype /XML", compress=False),
 19: b"<< /Type /Filespec /F (SECRET-ATTACH.txt) /EF << /F 20 0 R >> >>",
 20: stream(b"SECRET-ATTACHED-CONTENT\n", b" /Type /EmbeddedFile"),
 21: b"<< /Type /StructTreeRoot /K 22 0 R >>",
 22: b"<< /Type /StructElem /S /P /P 21 0 R /ActualText (SECRET-STRUCT) >>",
 23: b"<< /FT /Tx /T (SECRET-PARENTFIELD) /V (SECRET-KIDVALUE) /Kids [24 0 R] /DA (/F1 12 Tf 0 g) >>",
 24: b"<< /Type /Annot /Subtype /Widget /Parent 23 0 R /Rect [330 580 560 610] >>",
 25: b"<< /Type /Annot /Subtype /Highlight /Rect [330 540 560 570] /QuadPoints [330 570 560 570 330 540 560 540] /C [1 1 0] /Contents (SECRET-HIGHLIGHT) /Popup 26 0 R >>",
 26: b"<< /Type /Annot /Subtype /Popup /Parent 25 0 R /Rect [20 60 120 120] /Contents (SECRET-POPUP) >>",
 27: stream(b"<xfa><v>SECRET-XFA</v></xfa>"),
 28: b"<< /Type /OCG /Name (Hidden) >>",
 29: stream(tile, b" /Type /Pattern /PatternType 1 /PaintType 1 /TilingType 1 /BBox [0 0 200 30] /XStep 200 /YStep 30 /Resources << /Font << /F1 5 0 R >> >>"),
 30: b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Rotate 90 /Resources " + res + b" /Contents 31 0 R >>",
 31: stream(page3),
 33: stream(b"/Span <</Alt (SECRET-FORM-ALT)>> BDC BT /F1 14 Tf 0 0 Td (KEEP-FORM stays) Tj ET EMC\n", b" /Type /XObject /Subtype /Form /BBox [0 0 300 30] /Resources << /Font << /F1 5 0 R >> >>"),
 34: b"<< /ActualText (SECRET-NAMED) >>",
 32: stream(b"SECRET-THUMB", b" /Width 12 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8"),
}
# First revision: page 2's content is the old text; the appended revision replaces it (a classic leak).
first = dict(objs); first[9] = stream(oldrev)
out = bytearray(b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n")
offs = {}
for n in sorted(first):
    offs[n] = len(out); out += b"%d 0 obj\n" % n + first[n] + b"\nendobj\n"
size = max(first) + 1
xref = len(out)
out += b"xref\n0 %d\n0000000000 65535 f \n" % size
for n in range(1, size): out += b"%010d 00000 n \n" % offs[n]
out += b"trailer\n<< /Size %d /Root 1 0 R /Info 10 0 R >>\nstartxref\n%d\n%%%%EOF\n" % (size, xref)
off9 = len(out); out += b"9 0 obj\n" + objs[9] + b"\nendobj\n"
xref2 = len(out)
out += b"xref\n9 1\n%010d 00000 n \n" % off9
out += b"trailer\n<< /Size %d /Root 1 0 R /Info 10 0 R /Prev %d >>\nstartxref\n%d\n%%%%EOF\n" % (size, xref, xref2)
open(sys.argv[1], "wb").write(out)
