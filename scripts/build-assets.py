#!/usr/bin/env python3
"""Build Elocutio brand, app, and platform icons from the source artwork.

Run from the repository root:

    python3 scripts/build-assets.py

Requires Pillow. On macOS, also generates AppIcon.icns via iconutil.
"""

from __future__ import annotations

import subprocess
import sys
from pathlib import Path

from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parent.parent
BRAND = ROOT / "assets" / "brand"
SOURCE = BRAND / "source"
ICONS = ROOT / "assets" / "icons"

# Brand palette (docs/BRAND.md)
INK_INDIGO = (25, 31, 112, 255)  # #191F70
SOFT_INDIGO = (184, 190, 255, 255)  # #B8BEFF
NEAR_BLACK = (23, 23, 23, 255)  # #171717
WARM_PAPER = (248, 247, 243, 255)  # #F8F7F3
WHITE = (255, 255, 255, 255)


def recolor(path: Path, color: tuple[int, int, int, int]) -> Image.Image:
    """Use the source alpha channel as a mask and fill with a brand color."""
    source = Image.open(path).convert("RGBA")
    alpha = source.getchannel("A")
    output = Image.new("RGBA", source.size, color)
    output.putalpha(alpha)
    return output


def trim(image: Image.Image, pad_ratio: float = 0.05) -> Image.Image:
    """Crop transparent margins and add a small transparent pad."""
    bbox = image.getchannel("A").getbbox()
    if bbox:
        image = image.crop(bbox)
    pad = max(1, int(max(image.size) * pad_ratio))
    canvas = Image.new("RGBA", (image.width + 2 * pad, image.height + 2 * pad), (0, 0, 0, 0))
    canvas.alpha_composite(image, (pad, pad))
    return canvas


def square(image: Image.Image, pad_ratio: float = 0.04) -> Image.Image:
    """Center an image on a transparent square canvas."""
    size = int(max(image.size) * (1 + pad_ratio * 2))
    canvas = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    canvas.alpha_composite(image, ((size - image.width) // 2, (size - image.height) // 2))
    return canvas


def save(image: Image.Image, path: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    image.save(path, optimize=True)
    print(f"wrote {path.relative_to(ROOT)} ({image.width}x{image.height})")


def build_lockups() -> None:
    save(trim(recolor(SOURCE / "lockup-indigo.png", INK_INDIGO)), BRAND / "lockup-indigo.png")
    save(trim(recolor(SOURCE / "lockup-black.png", NEAR_BLACK)), BRAND / "lockup-black.png")
    save(trim(recolor(SOURCE / "lockup-white.png", WHITE)), BRAND / "lockup-white.png")
    save(trim(recolor(SOURCE / "lockup-white.png", SOFT_INDIGO)), BRAND / "lockup-soft-indigo.png")
    save(trim(recolor(SOURCE / "wordmark-white-transparent.png", WHITE)), BRAND / "wordmark-white.png")


def build_marks() -> Image.Image:
    indigo = square(trim(recolor(SOURCE / "mark-indigo.png", INK_INDIGO)))
    black = square(trim(recolor(SOURCE / "mark-black.png", NEAR_BLACK)))
    soft = square(trim(recolor(SOURCE / "mark-white-transparent.png", SOFT_INDIGO)))
    white = square(trim(recolor(SOURCE / "mark-white-transparent.png", WHITE)))
    save(indigo, BRAND / "mark-indigo.png")
    save(soft, BRAND / "mark-soft-indigo.png")
    save(black, BRAND / "mark-black.png")
    save(white, BRAND / "mark-white.png")
    return white


def app_icon(mark: Image.Image, size: int = 1024) -> Image.Image:
    """A macOS-style rounded-square icon: indigo field, white quote mark."""
    canvas = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    draw = ImageDraw.Draw(canvas)
    inset = int(size * 0.06)
    box = (inset, inset, size - inset, size - inset)
    radius = int((size - 2 * inset) * 0.225)
    draw.rounded_rectangle(box, radius=radius, fill=INK_INDIGO)

    target = int((size - 2 * inset) * 0.68)
    glyph = mark.copy()
    glyph.thumbnail((target, target), Image.LANCZOS)
    canvas.alpha_composite(glyph, ((size - glyph.width) // 2, (size - glyph.height) // 2))
    return canvas


def full_bleed_icon(mark: Image.Image, size: int, background: tuple[int, int, int, int]) -> Image.Image:
    canvas = Image.new("RGBA", (size, size), background)
    target = int(size * 0.66)
    glyph = mark.copy()
    glyph.thumbnail((target, target), Image.LANCZOS)
    canvas.alpha_composite(glyph, ((size - glyph.width) // 2, (size - glyph.height) // 2))
    return canvas.convert("RGB")


def menu_bar_template(mark: Image.Image, size: int = 44) -> Image.Image:
    canvas = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    glyph = recolor(SOURCE / "mark-black.png", (0, 0, 0, 255))
    glyph = trim(glyph, 0)
    target = int(size * 0.88)
    glyph.thumbnail((target, target), Image.LANCZOS)
    canvas.alpha_composite(glyph, ((size - glyph.width) // 2, (size - glyph.height) // 2))
    return canvas


def build_platform_icons(mark: Image.Image) -> None:
    icon = app_icon(mark)
    save(icon, ICONS / "app-icon-1024.png")
    for size in (512, 256, 128, 64, 32, 16):
        save(icon.resize((size, size), Image.LANCZOS), ICONS / f"app-icon-{size}.png")
        save(icon.resize((size, size), Image.LANCZOS), ICONS / f"browser-{size}.png")

    save(full_bleed_icon(mark, 1024, INK_INDIGO), ICONS / "ios-1024.png")
    save(full_bleed_icon(mark, 512, INK_INDIGO), ICONS / "android-512.png")
    save(menu_bar_template(mark), ICONS / "menu-bar-template.png")

    favicon = icon.resize((64, 64), Image.LANCZOS)
    favicon.save(ICONS / "favicon.ico", sizes=[(16, 16), (32, 32), (48, 48), (64, 64)])
    print(f"wrote {ICONS.relative_to(ROOT)}/favicon.ico")

    icon.save(
        ICONS / "windows.ico",
        sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)],
    )
    print(f"wrote {ICONS.relative_to(ROOT)}/windows.ico")

    social = Image.new("RGB", (1280, 640), WARM_PAPER[:3])
    lockup = Image.open(BRAND / "lockup-indigo.png").convert("RGBA")
    lockup.thumbnail((860, 360), Image.LANCZOS)
    social.paste(lockup, ((1280 - lockup.width) // 2, (640 - lockup.height) // 2), lockup)
    social.save(ICONS / "social-preview.png", optimize=True)
    print(f"wrote {ICONS.relative_to(ROOT)}/social-preview.png")

    # macOS .icns
    iconset = ICONS / "AppIcon.iconset"
    iconset.mkdir(exist_ok=True)
    for name, size in {
        "icon_16x16.png": 16,
        "icon_16x16@2x.png": 32,
        "icon_32x32.png": 32,
        "icon_32x32@2x.png": 64,
        "icon_128x128.png": 128,
        "icon_128x128@2x.png": 256,
        "icon_256x256.png": 256,
        "icon_256x256@2x.png": 512,
        "icon_512x512.png": 512,
        "icon_512x512@2x.png": 1024,
    }.items():
        icon.resize((size, size), Image.LANCZOS).save(iconset / name, optimize=True)
    try:
        subprocess.run(
            ["iconutil", "-c", "icns", str(iconset), "-o", str(ICONS / "AppIcon.icns")],
            check=True,
        )
        print(f"wrote {ICONS.relative_to(ROOT)}/AppIcon.icns")
    except (FileNotFoundError, subprocess.CalledProcessError) as error:
        print(f"iconutil unavailable ({error}); skipping .icns", file=sys.stderr)


def main() -> int:
    if not (SOURCE / "mark-indigo.png").exists():
        print("Missing assets/brand/source/mark-indigo.png", file=sys.stderr)
        return 1
    build_lockups()
    build_marks()
    tight_mark = trim(recolor(SOURCE / "mark-white-transparent.png", WHITE), 0)
    build_platform_icons(tight_mark)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
