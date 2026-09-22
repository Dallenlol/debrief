#!/usr/bin/env python3
"""Draw the Hark app icon (1024x1024 PNG), the source every platform icon is built from.

    python scripts/make-icons.py            # writes assets/icon.png (the 1024 master)
    python scripts/make-icons.py --tauri    # ... and regenerates every platform icon

The mark: two level-meter bars standing as the stems of an H, crossed in ember.
`assets/logo.svg` draws the same geometry with the same constants - keep them in
step if you change one. Needs only Pillow.
"""

from __future__ import annotations

import math
import subprocess
import sys
from pathlib import Path

from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parent.parent
SIZE = 1024
SS = 4  # supersampling factor; drawn at SIZE*SS then resized for clean edges

# --- geometry on the 1024 grid (mirrored in assets/logo.svg) ---
TILE_INSET = 88
TILE_RADIUS = 190
STEM_W = 78
STEM_H = 440
GAP = 116          # stem centre to tile centre
CROSSBAR_H = 56
CROSSBAR_RISE = 14  # crossbars sit a touch above the true centre or they look low

# --- colours: the app's own tokens, converted from oklch to sRGB ---
def oklch_to_rgb(l: float, c: float, h_deg: float) -> tuple[int, int, int]:
    h = math.radians(h_deg)
    a, b = c * math.cos(h), c * math.sin(h)
    l_ = (l + 0.3963377774 * a + 0.2158037573 * b) ** 3
    m_ = (l - 0.1055613458 * a - 0.0638541728 * b) ** 3
    s_ = (l - 0.0894841775 * a - 1.2914855480 * b) ** 3
    r = 4.0767416621 * l_ - 3.3077115913 * m_ + 0.2309699292 * s_
    g = -1.2684380046 * l_ + 2.6097574011 * m_ - 0.3413193965 * s_
    bl = -0.0041960863 * l_ - 0.7034186147 * m_ + 1.7076147010 * s_

    def enc(u: float) -> int:
        u = max(0.0, min(1.0, u))
        u = 12.92 * u if u <= 0.0031308 else 1.055 * u ** (1 / 2.4) - 0.055
        return round(u * 255)

    return enc(r), enc(g), enc(bl)


INK = oklch_to_rgb(0.20, 0.010, 60)    # --ink, the tile
PAPER = oklch_to_rgb(0.985, 0.006, 80)  # --canvas, the bars
EMBER = oklch_to_rgb(0.62, 0.200, 32)   # --ember, the crossbar


def draw(size: int = SIZE) -> Image.Image:
    s = size * SS
    k = s / SIZE  # grid -> pixel scale
    img = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)

    d.rounded_rectangle(
        [TILE_INSET * k, TILE_INSET * k, (SIZE - TILE_INSET) * k, (SIZE - TILE_INSET) * k],
        radius=TILE_RADIUS * k,
        fill=INK + (255,),
    )

    cx = cy = SIZE / 2
    half = STEM_W / 2

    # Two level-meter bars standing as the stems of an H.
    for x in (cx - GAP, cx + GAP):
        d.rounded_rectangle(
            [(x - half) * k, (cy - STEM_H / 2) * k, (x + half) * k, (cy + STEM_H / 2) * k],
            radius=half * k,
            fill=PAPER + (255,),
        )

    # The crossbar butts into both stems (square ends, so the colour change lands
    # exactly on the stem edge) and rides slightly high, as crossbars do.
    y = cy - CROSSBAR_RISE
    d.rectangle(
        [(cx - GAP + half - 1) * k, (y - CROSSBAR_H / 2) * k, (cx + GAP - half + 1) * k, (y + CROSSBAR_H / 2) * k],
        fill=EMBER + (255,),
    )

    return img.resize((size, size), Image.LANCZOS)


def main() -> int:
    out = ROOT / "assets" / "icon.png"
    out.parent.mkdir(exist_ok=True)
    icon = draw()
    icon.save(out)
    print(f"wrote {out.relative_to(ROOT)} ({icon.size[0]}x{icon.size[1]}, ink={INK} ember={EMBER})")

    # A contact sheet at the sizes that actually matter, to eyeball legibility.
    sizes = [16, 24, 32, 48, 64, 128, 256]
    sheet = Image.new("RGBA", (sum(sizes) + 16 * len(sizes), 256 + 32), (245, 243, 239, 255))
    x = 8
    for n in sizes:
        sheet.alpha_composite(icon.resize((n, n), Image.LANCZOS), (x, 16 + (256 - n) // 2))
        x += n + 16
    preview = ROOT / "docs" / "img" / "icon-sizes.png"
    preview.parent.mkdir(parents=True, exist_ok=True)
    sheet.save(preview)
    print(f"wrote {preview.relative_to(ROOT)}")

    if "--tauri" in sys.argv:
        subprocess.run(["pnpm", "tauri", "icon", str(out)], cwd=ROOT, check=True, shell=sys.platform == "win32")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
