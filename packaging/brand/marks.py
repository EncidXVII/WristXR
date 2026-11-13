"""The DesktopVR / FitnessVR / Parallax marks: one geometry, every icon file.

The same script lives in all three repositories. It draws the marks from their construction (upright bars on a
32-unit grid, leaned 1:4, sharp corners cut at 45 degrees) and writes, for one app:

  <app>-mark.svg        the mark in the lime accent, transparent (README, docs)
  <app>-mark-ink.svg    the mark in dark ink, for light backgrounds
  <app>-mark-white.png  the mark in white at 256 px, for an app to tint with its theme accent
  <app>-icon.svg        the app icon: the mark on a graphite tile
  <app>-<n>.png         the app icon at 16-1024 px (Linux, SteamVR, window icons)
  <app>.ico             the app icon, 16-256 px (Windows exe, taskbar, Explorer)
  <app>-tray.ico        the tray icon for dark taskbars: lime mark only, 16-64 px
  <app>-tray-ink.ico    the tray icon for light taskbars: ink mark only, 16-64 px

Usage: python marks.py desktopvr|fitnessvr|parallax OUT_DIR
Needs Pillow.
"""

import io
import struct
import sys
from pathlib import Path

from PIL import Image, ImageDraw

LIME = (0xC6, 0xFF, 0x1A)
INK = (0x1B, 0x1B, 0x1D)
TILE = (0x14, 0x14, 0x16)
K = 0.25  # lean: one across for every four up, about the centre line (y = 16)

# Each mark as polygons on the upright 32-unit grid, cuts already applied. Holes are listed after the shape they
# sit in and drawn with the even-odd rule. Flat bars are 4 units, upright bars 5; joints are cut 3, bar ends 2.
MARKS = {
    "desktopvr": [
        [(4, 4), (24.25, 4), (28, 7), (28, 20), (7.75, 20), (4, 17)],  # screen
        [(9, 8), (23, 8), (23, 16), (9, 16)],  # its opening
        [(10, 24), (19.5, 24), (22, 26), (22, 28), (12.5, 28), (10, 26)],  # stand
    ],
    "fitnessvr": [
        [(5, 4), (15.25, 4), (19, 7), (19, 24), (23, 24), (23, 16), (28.5, 16), (31, 18), (31, 20), (28, 20),
         (28, 28), (17.75, 28), (14, 25), (14, 8), (10, 8), (10, 20), (4.5, 20), (2, 18), (2, 16), (5, 16)],
    ],
    "parallax": [
        [(3, 12), (15.25, 12), (19, 15), (19, 28), (6.75, 28), (3, 25)],  # front frame
        [(8, 16), (14, 16), (14, 24), (8, 24)],  # its opening
        [(11, 4), (24.25, 4), (28, 7), (28, 20), (25.5, 20), (23, 18), (23, 8), (13.5, 8), (11, 6)],  # frame behind
    ],
}
HOLES = {"desktopvr": {1}, "fitnessvr": set(), "parallax": {1}}


def leaned(poly):
    return [(x + K * (16 - y), y) for x, y in poly]


def svg_path(app):
    out = []
    for poly in MARKS[app]:
        pts = leaned(poly)
        out.append("M" + "L".join(f"{x:g} {y:g}" for x, y in pts) + "Z")
    return " ".join(out)


def mark_svg(app, rgb):
    hexc = "#%02X%02X%02X" % rgb
    return (f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32">'
            f'<path fill="{hexc}" fill-rule="evenodd" d="{svg_path(app)}"/></svg>\n')


def icon_svg(app):
    # The 256 px layout: a 256 tile, radius 32, the mark at 6 px per unit, 32 px in from the edge.
    return (f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 256 256">'
            f'<rect width="256" height="256" rx="32" fill="#141416"/>'
            f'<path transform="translate(32 32) scale(6)" fill="#C6FF1A" fill-rule="evenodd" d="{svg_path(app)}"/>'
            f'</svg>\n')


SS = 16  # supersampling: each output pixel is the average of 16 x 16 samples


def draw_mark(app, size, offset, scale, rgb, base=None):
    """The mark at `scale` px per unit, `offset` px in from the top left, over `base` (or transparent)."""
    big = size * SS
    mask = Image.new("L", (big, big), 0)
    d = ImageDraw.Draw(mask)
    for i, poly in enumerate(MARKS[app]):
        pts = [((offset + x * scale) * SS, (offset + y * scale) * SS) for x, y in leaned(poly)]
        d.polygon(pts, fill=0 if i in HOLES[app] else 255)
    mask = mask.reduce(SS)
    img = base.copy() if base else Image.new("RGBA", (size, size), (0, 0, 0, 0))
    img.paste(Image.new("RGBA", (size, size), rgb + (255,)), (0, 0), mask)
    return img


def tile(size):
    big = size * SS
    m = Image.new("L", (big, big), 0)
    ImageDraw.Draw(m).rounded_rectangle((0, 0, big - 1, big - 1), radius=max(1, round(size * 0.125)) * SS, fill=255)
    m = m.reduce(SS)
    img = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    img.paste(Image.new("RGBA", (size, size), TILE + (255,)), (0, 0), m)
    return img


# The mark inside the app icon, per size: (offset px, px per unit). Units land on whole pixels at every size, so the
# flat edges stay sharp. At 16 px the mark fills the tile; from 24 px it sits at three quarters.
APP_LAYOUT = {16: (0, 0.5), 20: (2, 0.5), 24: (4, 0.5), 32: (4, 0.75), 40: (4, 1.0), 48: (8, 1.0),
              64: (8, 1.5), 128: (16, 3.0), 256: (32, 6.0), 512: (64, 12.0), 1024: (128, 24.0)}
TRAY_SIZES = [16, 20, 24, 32, 40, 48, 64]


def app_icon(app, size):
    off, sc = APP_LAYOUT[size]
    return draw_mark(app, size, off, sc, LIME, tile(size))


def tray_icon(app, size, rgb):
    return draw_mark(app, size, 0, size / 32, rgb)


def dib(img):
    """A 32-bit icon image in BMP (DIB) form: header, bottom-up BGRA, then the 1-bit AND mask."""
    w, h = img.size
    px = img.load()
    rows = []
    for y in range(h - 1, -1, -1):
        rows.append(b"".join(struct.pack("BBBB", px[x, y][2], px[x, y][1], px[x, y][0], px[x, y][3]) for x in range(w)))
    stride = ((w + 31) // 32) * 4
    mask = []
    for y in range(h - 1, -1, -1):
        bits = bytearray(stride)
        for x in range(w):
            if px[x, y][3] == 0:
                bits[x // 8] |= 0x80 >> (x % 8)
        mask.append(bytes(bits))
    xor = b"".join(rows)
    andm = b"".join(mask)
    header = struct.pack("<IiiHHIIiiII", 40, w, h * 2, 1, 32, 0, len(xor) + len(andm), 0, 0, 0, 0)
    return header + xor + andm


def ico(images):
    """An .ico with BMP entries below 128 px and PNG entries from 128 px, as Windows expects."""
    blobs = []
    for img in images:
        if img.size[0] >= 128:
            b = io.BytesIO()
            img.save(b, "PNG", optimize=True)
            blobs.append(b.getvalue())
        else:
            blobs.append(dib(img))
    out = struct.pack("<HHH", 0, 1, len(images))
    offset = 6 + 16 * len(images)
    for img, blob in zip(images, blobs):
        w, h = img.size
        out += struct.pack("<BBBBHHII", w % 256, h % 256, 0, 0, 1, 32, len(blob), offset)
        offset += len(blob)
    return out + b"".join(blobs)


def main():
    if len(sys.argv) != 3 or sys.argv[1] not in MARKS:
        sys.exit(__doc__)
    app, out = sys.argv[1], Path(sys.argv[2])
    out.mkdir(parents=True, exist_ok=True)
    (out / f"{app}-mark.svg").write_text(mark_svg(app, LIME), encoding="utf-8")
    (out / f"{app}-mark-ink.svg").write_text(mark_svg(app, INK), encoding="utf-8")
    (out / f"{app}-icon.svg").write_text(icon_svg(app), encoding="utf-8")
    draw_mark(app, 256, 0, 8, (255, 255, 255)).save(out / f"{app}-mark-white.png", optimize=True)
    for n in (16, 32, 48, 64, 128, 256, 512, 1024):
        app_icon(app, n).save(out / f"{app}-{n}.png", optimize=True)
    (out / f"{app}.ico").write_bytes(ico([app_icon(app, n) for n in (16, 20, 24, 32, 40, 48, 64, 128, 256)]))
    (out / f"{app}-tray.ico").write_bytes(ico([tray_icon(app, n, LIME) for n in TRAY_SIZES]))
    (out / f"{app}-tray-ink.ico").write_bytes(ico([tray_icon(app, n, INK) for n in TRAY_SIZES]))
    print(f"{app}: wrote {out}")


if __name__ == "__main__":
    main()
