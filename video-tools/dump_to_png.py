#!/usr/bin/env python3
"""Render an ANSI SGR dump (from --wgpu-demo) to a PNG our study can measure.

Cell aspect mirrors a terminal font (8 px wide x 16 px tall). Foreground ink
is painted as a filled cell so structural metrics (blobs, contrast, backing)
work identically on both sides.
"""
import re
import sys

import numpy as np
import cv2

SGR = re.compile(r"\x1b\[([0-9;]*)m")
FG = re.compile(r"38;2;(\d+);(\d+);(\d+)")
BG = re.compile(r"48;2;(\d+);(\d+);(\d+)")
CW, CH = 8, 16


def parse(path):
    rows = open(path, "rb").read().decode("utf-8", errors="replace").split("\n")[:-1]
    grid = []
    for line in rows:
        cells = []
        pos, fg, bg = 0, None, None
        for m in SGR.finditer(line):
            for ch in line[pos : m.start()]:
                # A real terminal inks non-space glyphs with fg; space glyphs
                # show their background (nameplate backing reads correctly).
                if ch == " " and bg is not None:
                    cells.append(("bg", bg))
                else:
                    cells.append(fg)
            params = m.group(1)
            mm = FG.search(params)
            if mm:
                fg = tuple(int(x) for x in mm.groups())
            mm = BG.search(params)
            if mm:
                bg = tuple(int(x) for x in mm.groups())
            if params in ("0", "00", "", "49"):
                bg = None
            pos = m.end()
        for ch in line[pos:]:
            if ch == " " and bg is not None:
                cells.append(("bg", bg))
            else:
                cells.append(fg)
        grid.append(cells)
    return grid


def render(path, out):
    grid = parse(path)
    width = max(len(r) for r in grid)
    height = len(grid)
    img = np.full((height * CH, width * CW, 3), (4, 3, 2), np.uint8)
    for r, cells in enumerate(grid):
        for c, fg in enumerate(cells):
            if fg is None:
                continue
            y0, x0 = r * CH, c * CW
            img[y0 : y0 + CH, x0 : x0 + CW] = fg[1] if isinstance(fg, tuple) and fg and fg[0] == "bg" else fg
    cv2.imwrite(out, cv2.cvtColor(img, cv2.COLOR_RGB2BGR))
    print(f"wrote {out} ({width}x{height} cells)")


if __name__ == "__main__":
    render(sys.argv[1], sys.argv[2])
