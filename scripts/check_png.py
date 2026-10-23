#!/usr/bin/env python3
"""Sanity check of a screenshot: at least 800x600 and not uniform. Standard library only.

Usage: python scripts/check_png.py <file.png> [--min-width 800] [--min-height 600] [--min-colors 50]
Exit code 0 when the image passes, 1 otherwise (missing file, unreadable PNG, too small, too few colors).
"""
import struct
import sys
import zlib

SIGNATURE = b"\x89PNG\r\n\x1a\n"
CHANNELS = {0: 1, 2: 3, 3: 1, 4: 2, 6: 4}  # color type -> samples per pixel (8-bit only)


def fail(msg):
    print(f"check_png: FAIL: {msg}")
    sys.exit(1)


def read_png(path):
    """Returns (width, height, bytes per pixel, unfiltered rows)."""
    try:
        data = open(path, "rb").read()
    except OSError as e:
        fail(f"cannot read {path}: {e}")
    if not data.startswith(SIGNATURE):
        fail("not a PNG file")
    pos, header, idat = len(SIGNATURE), None, []
    while pos + 8 <= len(data):
        length, kind = struct.unpack(">I4s", data[pos : pos + 8])
        body = data[pos + 8 : pos + 8 + length]
        pos += 12 + length
        if kind == b"IHDR":
            header = struct.unpack(">IIBBBBB", body)
        elif kind == b"IDAT":
            idat.append(body)
        elif kind == b"IEND":
            break
    if header is None or not idat:
        fail("truncated PNG (no IHDR or IDAT)")
    width, height, depth, ctype, _, _, interlace = header
    if depth != 8 or ctype not in CHANNELS or interlace != 0:
        fail(f"unsupported PNG (bit depth {depth}, color type {ctype}, interlace {interlace})")
    bpp = CHANNELS[ctype]
    stride = width * bpp
    try:
        raw = zlib.decompress(b"".join(idat))
    except zlib.error as e:
        fail(f"corrupt image data: {e}")
    if len(raw) < height * (stride + 1):
        fail("truncated image data")
    return width, height, bpp, unfilter(raw, height, stride, bpp)


def unfilter(raw, height, stride, bpp):
    rows, prev = [], bytearray(stride)
    for y in range(height):
        start = y * (stride + 1)
        ftype = raw[start]
        cur = bytearray(raw[start + 1 : start + 1 + stride])
        if ftype == 1:  # Sub
            for i in range(bpp, stride):
                cur[i] = (cur[i] + cur[i - bpp]) & 255
        elif ftype == 2:  # Up
            for i in range(stride):
                cur[i] = (cur[i] + prev[i]) & 255
        elif ftype == 3:  # Average
            for i in range(stride):
                left = cur[i - bpp] if i >= bpp else 0
                cur[i] = (cur[i] + ((left + prev[i]) >> 1)) & 255
        elif ftype == 4:  # Paeth
            for i in range(stride):
                a = cur[i - bpp] if i >= bpp else 0
                b = prev[i]
                c = prev[i - bpp] if i >= bpp else 0
                p = a + b - c
                pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                pred = a if pa <= pb and pa <= pc else (b if pb <= pc else c)
                cur[i] = (cur[i] + pred) & 255
        elif ftype != 0:
            fail(f"unknown filter type {ftype}")
        rows.append(cur)
        prev = cur
    return rows


def main(argv):
    args = [a for a in argv if not a.startswith("--")]
    opts = {a.split("=")[0]: a.split("=")[1] for a in argv if a.startswith("--") and "=" in a}
    if len(args) != 1:
        print(__doc__)
        return 1
    min_w = int(opts.get("--min-width", 800))
    min_h = int(opts.get("--min-height", 600))
    min_colors = int(opts.get("--min-colors", 50))
    width, height, bpp, rows = read_png(args[0])
    if width < min_w or height < min_h:
        fail(f"{width}x{height} is smaller than {min_w}x{min_h}")
    colors = set()
    for y in range(0, height, 5):
        row = rows[y]
        for x in range(0, width, 3):
            colors.add(bytes(row[x * bpp : x * bpp + bpp]))
        if len(colors) >= min_colors:
            break
    if len(colors) < min_colors:
        fail(f"only {len(colors)} distinct colors in the sample (need {min_colors}): the image looks blank")
    print(f"check_png: OK {args[0]} {width}x{height}, >= {len(colors)} colors")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
