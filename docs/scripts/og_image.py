#!/usr/bin/env python3
"""Write docs/source/_static/og.png, 1200 by 630, from the repo logo.

The card is the wordmark in docs/source/_static/logo.svg, with mark.svg
inlined, centered on the logo's own ground. rsvg-convert paints it.
"""

from __future__ import annotations

import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
STATIC = ROOT / "docs" / "source" / "_static"
OUT = STATIC / "og.png"
WIDTH = 1200
HEIGHT = 630
# The wordmark's own ground, from logo.svg.
GROUND = "#0B0B0A"


def inner_svg(path: Path) -> str:
    text = path.read_text(encoding="utf-8")
    start = text.find("<svg")
    if start < 0:
        raise SystemExit(f"{path}: no svg root")
    open_end = text.find(">", start)
    close = text.rfind("</svg>")
    if open_end < 0 or close < 0:
        raise SystemExit(f"{path}: svg root does not close")
    return text[open_end + 1 : close].strip()


def wordmark() -> str:
    mark = inner_svg(STATIC / "mark.svg")
    logo = inner_svg(STATIC / "logo.svg")
    image = '<image href="mark.svg"'
    at = logo.find(image)
    if at < 0:
        raise SystemExit("logo.svg: no mark.svg image")
    end = logo.find("/>", at)
    if end < 0:
        raise SystemExit("logo.svg: mark image does not close")
    embedded = (
        '<svg x="18" y="10" width="200" height="200" viewBox="0 0 200 200">'
        f"{mark}</svg>"
    )
    return logo[:at] + embedded + logo[end + 2 :]


def canvas() -> str:
    # 820 by 220 is logo.svg's viewBox. Center it on the card.
    x = (WIDTH - 820) / 2
    y = (HEIGHT - 220) / 2
    return (
        '<?xml version="1.0" encoding="UTF-8"?>\n'
        '<svg xmlns="http://www.w3.org/2000/svg" '
        f'width="{WIDTH}" height="{HEIGHT}" viewBox="0 0 {WIDTH} {HEIGHT}">\n'
        f'  <rect width="{WIDTH}" height="{HEIGHT}" fill="{GROUND}"/>\n'
        f'  <svg x="{x:g}" y="{y:g}" width="820" height="220" viewBox="0 0 820 220">\n'
        f"{wordmark()}\n"
        "  </svg>\n"
        "</svg>\n"
    )


def main() -> int:
    svg = canvas()
    tmp = OUT.with_suffix(".svg")
    tmp.write_text(svg, encoding="utf-8")
    try:
        subprocess.run(
            ["rsvg-convert", "-w", str(WIDTH), "-h", str(HEIGHT), "-o", str(OUT), str(tmp)],
            check=True,
        )
    except FileNotFoundError:
        print("og_image: rsvg-convert is not installed", file=sys.stderr)
        return 1
    finally:
        tmp.unlink(missing_ok=True)
    data = OUT.read_bytes()
    if data[1:4] != b"PNG":
        print("og_image: output is not a PNG", file=sys.stderr)
        return 1
    # IHDR width and height are the next eight bytes after the length and type.
    ihdr = data.index(b"IHDR")
    width = int.from_bytes(data[ihdr + 4 : ihdr + 8], "big")
    height = int.from_bytes(data[ihdr + 8 : ihdr + 12], "big")
    if (width, height) != (WIDTH, HEIGHT):
        print(f"og_image: got {width}x{height}, wanted {WIDTH}x{HEIGHT}", file=sys.stderr)
        return 1
    print(f"og_image: wrote {OUT.relative_to(ROOT)} ({WIDTH}x{HEIGHT})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
