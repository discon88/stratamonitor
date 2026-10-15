"""Rasterise the StrataMonitor magnifying-glass icon.

Tool used: Python 3.13 + Pillow 12.3.0. There is no resvg/inkscape/magick/rsvg-convert
on this machine, so the geometry described by icons/icon.svg is drawn directly with
Pillow at the same proportions instead of being rasterised from the SVG.

Run:  python icons/make_icons.py
Writes: icons/icon-32.png, icons/icon-128.png, icons/icon-128@2x.png, icons/icon.ico

Geometry (icons/icon.svg uses a 32-unit viewBox):
  background  rounded square, fill #0e1113, corner radius 7/32 of the side
  lens        circle centre (13.5, 13.5) r 8, stroke #34d399 width 3.67
  handle      (19.2, 19.2) -> (27.5, 27.5), stroke #34d399 width 3.67, round caps
"""

import math
from pathlib import Path
from PIL import Image, ImageDraw

ACCENT = (0x34, 0xD3, 0x99, 255)  # upstream --accent
BG = (0x0E, 0x11, 0x13, 255)      # upstream --bg

# 32-unit viewBox geometry, normalised to 0..1
LENS_C = (13.5 / 32, 13.5 / 32)
LENS_R = 8.0 / 32
STROKE = 3.67 / 32
HANDLE = ((19.2 / 32, 19.2 / 32), (27.5 / 32, 27.5 / 32))
RADIUS = 7.0 / 32

SS = 8  # supersample factor, so the 32px icon stays crisp


def card(size: int) -> Image.Image:
    """One icon card at `size` device px, drawn at SS*size then downscaled."""
    big = size * SS
    img = Image.new("RGBA", (big, big), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)

    r = RADIUS * big
    d.rounded_rectangle([0, 0, big - 1, big - 1], radius=r, fill=BG)

    w = STROKE * big
    cx, cy = LENS_C[0] * big, LENS_C[1] * big
    rr = LENS_R * big
    d.ellipse([cx - rr, cy - rr, cx + rr, cy + rr], outline=ACCENT, width=max(1, round(w)))

    (x0, y0), (x1, y1) = (HANDLE[0][0] * big, HANDLE[0][1] * big), (HANDLE[1][0] * big, HANDLE[1][1] * big)
    d.line([x0, y0, x1, y1], fill=ACCENT, width=max(1, round(w)))
    cap = w / 2
    for (px, py) in ((x1, y1),):
        d.ellipse([px - cap, py - cap, px + cap, py + cap], fill=ACCENT)

    return img.resize((size, size), Image.LANCZOS)


def main() -> None:
    out = Path(__file__).resolve().parent
    out.mkdir(parents=True, exist_ok=True)

    p32 = card(32)
    p128 = card(128)
    p256 = card(256)  # 128 @2x

    p32.save(out / "icon-32.png")
    p128.save(out / "icon-128.png")
    p256.save(out / "icon-128@2x.png")

    p256.save(
        out / "icon.ico",
        sizes=((256, 256), (48, 48), (32, 32)),
    )

    for name in ("icon-32.png", "icon-128.png", "icon-128@2x.png", "icon.ico"):
        print(f"{out / name}: {(out / name).stat().st_size} bytes")


if __name__ == "__main__":
    main()
