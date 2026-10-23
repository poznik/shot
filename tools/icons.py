"""Makes the app and tray icon PNGs (docs/design/icons.md) from their two sources.

    uv run --no-project --with pillow --with resvg-py python tools/icons.py

Needs Pillow and resvg-py.

- The volumetric icon is a picture: assets/app/app.png, cropped to the glyph on transparent.
  It is scaled down for 32 px and up.
- The flat icon is the geometry below. Each size gets its own SVG on its pixel grid, so the
  frame's straight edges stay sharp, and resvg renders it. assets/app/app-flat.svg is the same
  geometry on a 1024 grid, for design tools.
"""

import io
import math
from pathlib import Path

import resvg_py
from PIL import Image

ROOT = Path(__file__).resolve().parent.parent
APP = ROOT / "assets" / "app"
TRAY = ROOT / "assets" / "tray"

VIOLET = "#7A3FE7"
# On a dark taskbar the base violet is under 3:1; this one is 5:1 on #202020.
VIOLET_LIGHT = "#A07BFF"

# The frame, in units of its outer side: corner radius, bar width, and the width of the cut
# along the diagonal from bottom-left to top-right.
RADIUS = 0.25
WIDTH = 0.132
CUT = 0.05

# The bolt, traced from the flat source, in units of the frame's outer box.
BOLT = [
    (0.4143, 0.8095), (0.4052, 0.8083), (0.3967, 0.8022), (0.3928, 0.7928), (0.3937, 0.7814),
    (0.4868, 0.5966), (0.2977, 0.5957), (0.2875, 0.5907), (0.2800, 0.5800), (0.2785, 0.5685),
    (0.2814, 0.5582), (0.4859, 0.2653), (0.5104, 0.2310), (0.5177, 0.2236), (0.5289, 0.2173),
    (0.5421, 0.2147), (0.6902, 0.2147), (0.7003, 0.2173), (0.7077, 0.2227), (0.7128, 0.2321),
    (0.7136, 0.2425), (0.7089, 0.2560), (0.5614, 0.4564), (0.7338, 0.4578), (0.7439, 0.4622),
    (0.7504, 0.4699), (0.7531, 0.4803), (0.7506, 0.4927), (0.7429, 0.5028), (0.6910, 0.5550),
    (0.5624, 0.6798), (0.4285, 0.8024), (0.4214, 0.8075),
]

# Pixel grid of the flat icon at each size: margin and bar width, px.
GRID = {16: (1, 2), 18: (1, 2), 20: (1, 2), 24: (1, 3), 32: (2, 4), 36: (2, 4), 40: (2, 5),
        48: (3, 6), 1024: (32, 127)}

# The volumetric glyph's longer side, as a share of the icon: Windows leaves 4% around it,
# macOS fits it into the 824 / 1024 body of Apple's grid.
FILL_WINDOWS = 0.92
FILL_MACOS = 824 / 1024


def rounded_rect(x0, y0, x1, y1, r):
    return (f"M{x0 + r:g} {y0:g}H{x1 - r:g}A{r:g} {r:g} 0 0 1 {x1:g} {y0 + r:g}"
            f"V{y1 - r:g}A{r:g} {r:g} 0 0 1 {x1 - r:g} {y1:g}H{x0 + r:g}"
            f"A{r:g} {r:g} 0 0 1 {x0:g} {y1 - r:g}V{y0 + r:g}A{r:g} {r:g} 0 0 1 {x0 + r:g} {y0:g}Z")


def flat_svg(size, color, alert=False):
    """The flat icon at `size` px; `alert` puts `!` in place of the bolt."""
    m, w = GRID[size]
    side = size - 2 * m
    r = round(RADIUS * side, 2)
    frame = (rounded_rect(m, m, size - m, size - m, r) +
             rounded_rect(m + w, m + w, size - m - w, size - m - w, max(r - w, 0)))
    # The cut: a band across the whole icon along the line x + y = size.
    d = round(max(CUT * side, 1) / math.sqrt(2), 3)
    cut = (f"M{2 * size - d:g} {-size}L{2 * size + d:g} {-size}"
           f"L{-size} {2 * size + d:g}L{-size} {2 * size - d:g}Z")
    if alert:
        # Bar and dot on whole pixels, as wide as the frame's bar rounded up to even.
        bw = w + w % 2
        gap = max(bw // 2, 1)
        x = (size - bw) // 2
        pad = round(0.1 * side)
        top, bottom = m + w + pad, size - m - w - pad
        mark = (f'<rect x="{x}" y="{top}" width="{bw}" height="{bottom - top - bw - gap}"/>'
                f'<rect x="{x}" y="{bottom - bw}" width="{bw}" height="{bw}"/>')
    else:
        points = " ".join(f"{m + u * side:.2f} {m + v * side:.2f}" for u, v in BOLT)
        mark = f'<path d="M{points}Z"/>'
    return (f'<svg xmlns="http://www.w3.org/2000/svg" width="{size}" height="{size}" '
            f'viewBox="0 0 {size} {size}" fill="{color}">'
            f'<mask id="cut"><rect width="{size}" height="{size}" fill="#fff"/>'
            f'<path d="{cut}" fill="#000"/></mask>'
            f'<path fill-rule="evenodd" d="{frame}" mask="url(#cut)"/>{mark}</svg>\n')


def flat(size, color, alert=False):
    png = resvg_py.svg_to_bytes(svg_string=flat_svg(size, color, alert))
    return Image.open(io.BytesIO(bytes(png))).convert("RGBA")


def volumetric(source, size, fill):
    """`source` scaled so its longer side is `fill` of `size`, centered on a transparent square."""
    scale = fill * size / max(source.size)
    w, h = round(source.width * scale), round(source.height * scale)
    # Premultiplied, so the transparent pixels' color does not bleed into the edges.
    glyph = source.convert("RGBa").resize((w, h), Image.LANCZOS).convert("RGBA")
    icon = Image.new("RGBA", (size, size))
    icon.paste(glyph, ((size - w) // 2, (size - h) // 2))
    return icon


def save(image, path):
    path.parent.mkdir(parents=True, exist_ok=True)
    image.save(path, optimize=True)
    print(path.relative_to(ROOT))


def main():
    source = Image.open(APP / "app.png").convert("RGBA")
    (APP / "app-flat.svg").write_text(flat_svg(1024, VIOLET), encoding="utf-8", newline="\n")

    # Windows: flat for the title bar and the small icons, volumetric for the taskbar and up.
    for size in (16, 20, 24, 32, 40, 48):
        save(flat(size, VIOLET), APP / "windows" / f"app-flat-{size}.png")
    for size in (32, 40, 48, 64, 256):
        save(volumetric(source, size, FILL_WINDOWS), APP / "windows" / f"app-{size}.png")

    # macOS: the iconset `iconutil -c icns` takes; 32 px and down are flat.
    for points in (16, 32, 128, 256, 512):
        for scale, suffix in ((1, ""), (2, "@2x")):
            px = points * scale
            image = flat(px, VIOLET) if px <= 32 else volumetric(source, px, FILL_MACOS)
            save(image, APP / "macos" / "Shot.iconset" / f"icon_{points}x{points}{suffix}.png")

    # Windows tray: one set per taskbar theme.
    for theme, color in (("light", VIOLET), ("dark", VIOLET_LIGHT)):
        for size in (16, 20, 24, 32, 40, 48):
            for alert, name in ((False, "tray"), (True, "tray-alert")):
                save(flat(size, color, alert), TRAY / "windows" / theme / f"{name}-{size}.png")

    # macOS menu bar: template images, the system tints them.
    for size, suffix in ((18, ""), (36, "@2x")):
        for alert, name in ((False, "trayTemplate"), (True, "trayAlertTemplate")):
            save(flat(size, "#000000", alert), TRAY / "macos" / f"{name}{suffix}.png")


if __name__ == "__main__":
    main()
