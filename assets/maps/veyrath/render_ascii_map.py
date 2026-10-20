"""Render the political Full JSON checkpoint as a compact terminal atlas.

The game includes the checked-in text output so the client has no runtime JSON
dependency. Run this script with ``--check`` after changing the checkpoint to
verify that the checked-in ASCII atlas is still derived from it.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path


ROOT = Path(__file__).parent
SOURCE = ROOT / "veyrath-04-political-full.json"
OUTPUT = ROOT / "veyrath-04-ascii.txt"
WIDTH = 36
HEIGHT = 16
STATE_GLYPHS = {2: "v", 4: "m", 5: "a", 6: "g", 8: "b", 9: "c"}
LANDMARKS = {
    "Veyrath": "@",
    "Greyfall": "G",
    "Millhaven": "*",
    "Crownspire Fortress": "A",
    "Dunhallow": "D",
    "Mirewatch": "M",
    "Blackwake Port": "B",
}


def render(data: dict) -> str:
    cells = [cell for cell in data["pack"]["cells"] if isinstance(cell, dict) and cell.get("i") is not None]
    features = data["pack"]["features"]
    rows: list[list[str]] = []
    for grid_y in range(HEIGHT):
        row: list[str] = []
        y = (grid_y + 0.5) / HEIGHT * 1100
        for grid_x in range(WIDTH):
            x = (grid_x + 0.5) / WIDTH * 1600
            cell = min(cells, key=lambda value: (value["p"][0] - x) ** 2 + (value["p"][1] - y) ** 2)
            feature = features[cell["f"]]
            land = bool(feature.get("land")) if isinstance(feature, dict) else cell.get("h", 0) >= 20
            if not land or cell.get("h", 0) < 20:
                glyph = "~"
            else:
                glyph = STATE_GLYPHS.get(cell.get("state", 0), ".")
                if cell.get("h", 0) >= 75:
                    glyph = "^"
                elif cell.get("r", 0):
                    glyph = ":"
                elif cell.get("routes"):
                    glyph = "="
            row.append(glyph)
        rows.append(row)

    burgs = {
        burg.get("name"): burg
        for burg in data["pack"]["burgs"]
        if isinstance(burg, dict) and burg.get("i") and not burg.get("removed")
    }
    for name, glyph in LANDMARKS.items():
        burg = burgs[name]
        x = min(WIDTH - 1, max(0, int(burg["x"] / 1600 * WIDTH)))
        y = min(HEIGHT - 1, max(0, int(burg["y"] / 1100 * HEIGHT)))
        rows[y][x] = glyph

    border = "+" + "-" * WIDTH + "+"
    return "\n".join([border, *("|" + "".join(row) + "|" for row in rows), border]) + "\n"


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true", help="verify the checked-in atlas matches the checkpoint")
    args = parser.parse_args()
    rendered = render(json.loads(SOURCE.read_text(encoding="utf-8")))
    if args.check:
        actual = OUTPUT.read_text(encoding="utf-8")
        if actual != rendered:
            raise SystemExit(f"{OUTPUT} is stale; regenerate it from {SOURCE}")
        print(f"ASCII atlas OK: {OUTPUT}")
    else:
        print(rendered, end="")


if __name__ == "__main__":
    main()
