"""Writes fixtures/synthetic-scan.pdf (D-068 Clean scan self-check): two made-up pages as full-page images —
page 1 a flat scan (tinted paper, uneven light, a shadow, noise), page 2 a bent page photographed at an angle on a
dark table. Made-up text only. Needs numpy, opencv-python-headless and Pillow:
    uv run --with numpy --with opencv-python-headless --with pillow scripts/make_scan_fixture.py
"""
import numpy as np, cv2
from PIL import Image, ImageDraw, ImageFont

W, H = 1240, 1754  # A4 at 150 dpi
FONT = "/System/Library/Fonts/Supplemental/Georgia.ttf"
rng = np.random.default_rng(7)

def clean_page():
    im = Image.new("RGB", (W, H), "white")
    d = ImageDraw.Draw(im)
    h1, body, small = ImageFont.truetype(FONT, 46), ImageFont.truetype(FONT, 26), ImageFont.truetype(FONT, 20)
    d.text((110, 120), "Riverside Bicycle Workshop", font=h1, fill="black")
    d.text((110, 190), "Opening hours and house rules", font=body, fill=(60, 60, 60))
    y = 270
    para = ("Members may use the repair stands on weekday evenings and on Saturday mornings. "
            "Each tool goes back on its hook after use, and borrowed pumps are returned before closing. "
            "Spare parts in the green drawers are free for members; parts in the locked cabinet are sold at cost. "
            "Please write any broken tool on the board by the door so it can be fixed the same week.")
    for _ in range(3):
        words, line = para.split(), ""
        for w in words:
            if d.textlength(line + " " + w, font=body) > W - 220:
                d.text((110, y), line.strip(), font=body, fill="black"); y += 40; line = ""
            line += " " + w
        d.text((110, y), line.strip(), font=body, fill="black"); y += 70
    # a small table
    d.text((110, y), "Day", font=body, fill="black"); d.text((500, y), "Hours", font=body, fill="black")
    for i, (a, b) in enumerate([("Monday", "17:00 - 20:00"), ("Wednesday", "17:00 - 20:00"), ("Saturday", "09:00 - 13:00")]):
        yy = y + 50 + i * 44
        d.line((110, yy - 6, W - 110, yy - 6), fill=(90, 90, 90), width=2)
        d.text((110, yy), a, font=body, fill="black"); d.text((500, yy), b, font=body, fill="black")
    # blue ink note and a faint grey footer
    d.text((110, H - 330), "Approved by the workshop committee", font=body, fill=(30, 50, 160))
    d.line((110, H - 270, 520, H - 280), fill=(30, 50, 160), width=3)
    d.text((110, H - 160), "Synthetic test page - not a real document", font=small, fill=(130, 130, 130))
    return np.asarray(im).astype(np.float32) / 255

def flat(page):
    yy, xx = np.mgrid[0:H, 0:W].astype(np.float32)
    tint = np.array([0.93, 0.89, 0.80], np.float32)                     # yellowish paper
    light = 0.62 + 0.38 * (1 - (xx / W * 0.5 + yy / H * 0.7) / 1.2)     # darker towards the lower right
    shadow = np.exp(-(((xx - 0.85 * W) / (0.32 * W)) ** 2 + ((yy - 0.25 * H) / (0.30 * H)) ** 2))
    light = light * (1 - 0.45 * cv2.GaussianBlur(shadow, (0, 0), 25))  # a soft phone/hand shadow
    img = page * tint * light[..., None]
    img += rng.normal(0, 0.025, img.shape).astype(np.float32)           # sensor noise
    return np.clip(img, 0, 1)

def hard(page):
    img = flat(page)
    # page curl: rows bend down towards the right edge, columns bulge
    yy, xx = np.mgrid[0:H, 0:W].astype(np.float32)
    mx = xx + 18 * np.sin(yy / H * np.pi)
    my = yy - 40 * np.sin(xx / W * np.pi) * (0.4 + 0.6 * yy / H) + 25 * (xx / W) ** 2
    img = cv2.remap(img, mx, my, cv2.INTER_LINEAR, borderValue=(0, 0, 0))
    img = cv2.GaussianBlur(img, (0, 0), 1.3)                            # slight focus blur
    # on a dark table, photographed at an angle
    CW, CH = 1500, 2000
    table = np.zeros((CH, CW, 3), np.float32) + np.array([0.28, 0.20, 0.14], np.float32)
    table += rng.normal(0, 0.03, table.shape).astype(np.float32)
    src = np.float32([[0, 0], [W, 0], [W, H], [0, H]])
    dst = np.float32([[170, 150], [1330, 230], [1390, 1880], [120, 1800]])
    M = cv2.getPerspectiveTransform(src, dst)
    warped = cv2.warpPerspective(img, M, (CW, CH), borderValue=(-1, -1, -1))
    mask = warped[..., 0] >= 0
    table[mask] = warped[mask]
    return np.clip(table, 0, 1)

page = clean_page()
pages = []
for img in (flat(page), hard(page)):
    im = Image.fromarray((img * 255).astype(np.uint8))
    pages.append(im.resize((im.width * 2 // 3, im.height * 2 // 3), Image.LANCZOS))
# 100 dpi: page 1 is A4.
pages[0].save("fixtures/synthetic-scan.pdf", save_all=True, append_images=pages[1:], resolution=100.0, quality=70)
print("fixtures/synthetic-scan.pdf")
