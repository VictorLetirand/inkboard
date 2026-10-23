#!/usr/bin/env python3
"""Draws the bitmaps of the Windows installer (NSIS welcome / finish sidebar and page header) from the application icon.

Usage: python scripts/make_installer_images.py   (needs Pillow; writes packaging/installer/*.bmp)
"""
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / 'packaging' / 'installer'
ICON = ROOT / 'crates' / 'inkboard-app' / 'assets' / 'icon.png'
ACCENT = (0x4F, 0x6D, 0xF5)


def font(size, bold=False):
    for name in ('segoeuib.ttf' if bold else 'segoeui.ttf', 'arialbd.ttf' if bold else 'arial.ttf', 'DejaVuSans-Bold.ttf' if bold else 'DejaVuSans.ttf'):
        try:
            return ImageFont.truetype(name, size)
        except OSError:
            continue
    return ImageFont.load_default()


def sidebar():
    w, h = 164, 314
    img = Image.new('RGB', (w, h), ACCENT)
    px = img.load()
    for y in range(h):
        k = y / (h - 1)
        c = tuple(round(a * (1 - 0.35 * k)) for a in ACCENT)
        for x in range(w):
            px[x, y] = c
    icon = Image.open(ICON).convert('RGBA').resize((92, 92), Image.LANCZOS)
    img.paste(icon, ((w - 92) // 2, 54), icon)
    d = ImageDraw.Draw(img)
    f = font(25, True)
    tw = d.textlength('Inkboard', font=f)
    d.text(((w - tw) / 2, 160), 'Inkboard', font=f, fill=(255, 255, 255))
    f2 = font(12)
    for i, line in enumerate(('Infinite whiteboard', 'Offline and free')):
        tw = d.textlength(line, font=f2)
        d.text(((w - tw) / 2, 198 + i * 18), line, font=f2, fill=(226, 232, 255))
    return img


def header():
    w, h = 150, 57
    img = Image.new('RGB', (w, h), (255, 255, 255))
    icon = Image.open(ICON).convert('RGBA').resize((44, 44), Image.LANCZOS)
    img.paste(icon, (w - 44 - 8, 6), icon)
    return img


if __name__ == '__main__':
    OUT.mkdir(parents=True, exist_ok=True)
    sidebar().save(OUT / 'sidebar.bmp', format='BMP')
    header().save(OUT / 'header.bmp', format='BMP')
    print('written', OUT)
