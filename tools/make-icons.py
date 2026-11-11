#!/usr/bin/env python3
"""Generates the app icons from packaging/icons/source.webp.

The source is the artwork on a black background. This crops it to the rounded square, replaces
the black corners with a transparent, anti-aliased rounded mask and writes:

  packaging/icons/hicolor/<N>x<N>/apps/io.github.blub3.Huehuecoyotl.png   (Linux, Flatpak)
  packaging/icons/coyotl.ico                                             (Windows .exe)

Usage: tools/make-icons.py   (needs Pillow)
"""
from pathlib import Path

from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "packaging" / "icons"
APP_ID = "io.github.blub3.Huehuecoyotl"
SIZES = [16, 24, 32, 48, 64, 128, 256, 512]
MASTER = 1024
MARGIN = 0.03  # transparent border, as a fraction of the canvas
RADIUS = 0.22  # corner radius, as a fraction of the square


def content_box(img):
    """Bounding box of the non-black square, scanned through the middle row and column."""
    rgb = img.convert("RGB")
    w, h = rgb.size

    def first(coords):
        return next(i for i, xy in coords if sum(rgb.getpixel(xy)) > 60)

    left = first((x, (x, h // 2)) for x in range(w))
    right = first((x, (x, h // 2)) for x in reversed(range(w)))
    top = first((y, (w // 2, y)) for y in range(h))
    bottom = first((y, (w // 2, y)) for y in reversed(range(h)))
    # A few pixels inwards, so no dark fringe survives the anti-aliased edge.
    inset = 4
    return left + inset, top + inset, right + 1 - inset, bottom + 1 - inset


def master():
    src = Image.open(OUT / "source.webp").convert("RGB")
    side = round(MASTER * (1 - 2 * MARGIN))
    art = src.crop(content_box(src)).resize((side, side), Image.LANCZOS)
    # The mask is drawn at 4x and scaled down for a smooth edge.
    big = Image.new("L", (side * 4, side * 4), 0)
    ImageDraw.Draw(big).rounded_rectangle(
        (0, 0, side * 4 - 1, side * 4 - 1), radius=round(side * 4 * RADIUS), fill=255
    )
    art.putalpha(big.resize((side, side), Image.LANCZOS))
    canvas = Image.new("RGBA", (MASTER, MASTER), (0, 0, 0, 0))
    off = (MASTER - side) // 2
    canvas.paste(art, (off, off), art)
    return canvas


def main():
    icon = master()
    for n in SIZES:
        d = OUT / "hicolor" / f"{n}x{n}" / "apps"
        d.mkdir(parents=True, exist_ok=True)
        icon.resize((n, n), Image.LANCZOS).save(d / f"{APP_ID}.png", optimize=True)
    icon.resize((256, 256), Image.LANCZOS).save(
        OUT / "coyotl.ico", sizes=[(n, n) for n in (16, 24, 32, 48, 64, 128, 256)]
    )


if __name__ == "__main__":
    main()
