"""Rasterize extracted Veyrath SVG overviews to 3x PNG atlas images."""

from __future__ import annotations

from pathlib import Path

import fitz


ROOT = Path(__file__).parent
SCALE = 3


def rasterize(svg_name: str, png_name: str) -> None:
    svg = ROOT / svg_name
    png = ROOT / png_name
    # MuPDF has a direct SVG loader on this host; using it avoids a browser or
    # native GTK/Pango dependency and keeps the rasterization deterministic.
    document = fitz.open(stream=svg.read_bytes(), filetype="svg")
    page = document[0]
    pixmap = page.get_pixmap(matrix=fitz.Matrix(SCALE, SCALE), alpha=False)
    pixmap.save(str(png))
    document.close()
    print(f"created {png} ({png.stat().st_size} bytes)")


def main() -> None:
    rasterize("veyrath-overview.svg", "veyrath-overview.png")
    rasterize("veyrath-clean.svg", "veyrath-clean.png")


if __name__ == "__main__":
    main()
