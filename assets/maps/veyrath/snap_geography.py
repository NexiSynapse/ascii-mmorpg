"""Snap provisional Veyrath anchors to the generated geography checkpoint."""

from __future__ import annotations

import json
import math
from pathlib import Path


ROOT = Path(__file__).parent
DESIGN = json.loads((ROOT / "world-design.json").read_text(encoding="utf-8"))
FULL = json.loads((ROOT / "veyrath-full.json").read_text(encoding="utf-8"))
WIDTH = FULL["info"]["width"]
HEIGHT = FULL["info"]["height"]

CELLS = {cell["i"]: cell for cell in FULL["pack"]["cells"]}
LAND = [cell for cell in CELLS.values() if cell.get("h", 0) > 0]
BURGS = [burg for burg in FULL["pack"]["burgs"] if isinstance(burg, dict) and burg.get("i", 0) > 0]


def xy(anchor):
    return anchor[0] * WIDTH / 100.0, anchor[1] * HEIGHT / 100.0


def nearest(records, target, x_key, y_key):
    x, y = target
    return min(
        records,
        key=lambda record: (record.get(x_key, 0) - x) ** 2 + (record.get(y_key, 0) - y) ** 2,
    )


def distance(a, b):
    return math.hypot(a[0] - b[0], a[1] - b[1])


def snap_record(record):
    target = xy(record["at"])
    cell = nearest(LAND, target, "p", "p") if False else min(
        LAND,
        key=lambda c: (c["p"][0] - target[0]) ** 2 + (c["p"][1] - target[1]) ** 2,
    )
    burg = nearest(BURGS, target, "x", "y")
    return {
        "id": record["id"],
        "name": record["name"],
        "kind": record.get("kind"),
        "region": record.get("region"),
        "anchor_percent": record["at"],
        "anchor_xy": [round(target[0], 2), round(target[1], 2)],
        "cell": cell["i"],
        "cell_xy": [round(cell["p"][0], 2), round(cell["p"][1], 2)],
        "cell_height": cell.get("h", 0),
        "cell_state": cell.get("state", 0),
        "cell_biome": cell.get("biome", 0),
        "cell_distance": round(distance(target, cell["p"]), 2),
        "nearest_generated_burg": burg["i"],
        "nearest_generated_burg_name": burg.get("name", ""),
        "burg_distance": round(distance(target, [burg["x"], burg["y"]]), 2),
    }


def main():
    settlements = [snap_record(record) for record in DESIGN["settlements"]]
    landmarks = [snap_record(record) for record in DESIGN["landmarks"]]
    output = {
        "schema": "veyrath-geography-snap-v1",
        "source": "veyrath-full.json",
        "map": FULL["info"],
        "counts": {
            "pack_cells": len(CELLS),
            "land_cells": len(LAND),
            "generated_burgs": len(BURGS),
            "generated_states": len(FULL["pack"]["states"]),
            "generated_rivers": len(FULL["pack"]["rivers"]),
        },
        "settlements": settlements,
        "landmarks": landmarks,
    }
    path = ROOT / "geography-snap.json"
    path.write_text(json.dumps(output, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    print(f"wrote {path}")
    print(f"settlements: {len(settlements)}, landmarks: {len(landmarks)}")
    print(f"land cells: {len(LAND)}; generated burgs: {len(BURGS)}")
    print(f"max settlement cell distance: {max(item['cell_distance'] for item in settlements):.2f}")
    print(f"coastal/low-cell candidates: {sum(item['cell_height'] <= 2 for item in settlements)} settlements")


if __name__ == "__main__":
    main()
