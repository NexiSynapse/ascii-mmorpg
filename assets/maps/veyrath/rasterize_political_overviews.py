"""Rasterize the political checkpoint atlas SVGs at 3x."""

from __future__ import annotations

from pathlib import Path

import fitz


ROOT = Path(__file__).parent


def rasterize(source: str, destination: str) -> None:
    svg = ROOT / source
    png = ROOT / destination
    document = fitz.open(stream=svg.read_bytes(), filetype="svg")
    pixmap = document[0].get_pixmap(matrix=fitz.Matrix(3, 3), alpha=False)
    pixmap.save(str(png))
    document.close()
    print(f"created {png} ({png.stat().st_size} bytes)")


if __name__ == "__main__":
    rasterize("veyrath-04-political-overview.svg", "veyrath-04-political-overview.png")
    rasterize("veyrath-04-political-clean.svg", "veyrath-04-political-clean.png")
