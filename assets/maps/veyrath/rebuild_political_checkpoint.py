"""Rebuild an editable Azgaar .map record stream from the political export.

Azgaar's Full JSON is the convenient validation/export form, while the .map
file is the editable project form.  The base geography checkpoint already
contains the unchanged style/font/relief records, so this utility replaces the
SVG and political/data records with the live political pass while preserving
the remaining project records byte-for-byte in meaning.
"""

from __future__ import annotations

import json
import re
from datetime import date
from pathlib import Path


ROOT = Path(__file__).parent
BASE = ROOT / "veyrath-03-geography.map"
FULL = ROOT / "veyrath-04-political-full.json"
SVG = ROOT / "veyrath-04-political.svg"
OUTPUT = ROOT / "veyrath-04-political.map"


def compact(value: object) -> str:
    # Some generated Azgaar marker notes contain UTF-16 surrogate pairs. JSON
    # escaping keeps those notes valid UTF-8 in the .map record stream.
    return json.dumps(value, ensure_ascii=True, separators=(",", ":"))


def cell_array(cells: list[dict], key: str) -> str:
    ordered = sorted(cells, key=lambda cell: cell["i"])
    return ",".join(str(cell.get(key, 0) if cell.get(key) is not None else 0) for cell in ordered)


def cell_routes(cells: list[dict]) -> str:
    routes = {}
    for cell in cells:
        value = cell.get("routes")
        if value:
            routes[str(cell["i"])] = value
    return compact(routes)


def main() -> None:
    full = json.loads(FULL.read_text(encoding="utf-8"))
    pack = full["pack"]
    grid = full["grid"]
    base = BASE.read_bytes().split(b"\r\n")
    if len(base) != 53:
        raise ValueError(f"expected 53 Azgaar records, found {len(base)}")

    base_info = base[0].decode("utf-8").split("|")
    base_info[2] = str(date.today())
    base_info[6] = str(full["info"].get("mapId", base_info[6]))
    records = [record.decode("utf-8") for record in base]
    records[0] = "|".join(base_info)
    records[1] = compact(full["settings"]["options"]["map"])
    records[3] = compact(pack["biomes"])
    svg = SVG.read_text(encoding="utf-8")
    svg = re.sub(r'(<svg\b[^>]*\bwidth=")[^"]*(")', r'\g<1>1600\2', svg, count=1)
    svg = re.sub(r'(<svg\b[^>]*\bheight=")[^"]*(")', r'\g<1>1100\2', svg, count=1)
    records[5] = svg

    grid_cells = sorted(grid["cells"], key=lambda cell: cell["i"])
    grid_general = {
        key: grid[key]
        for key in ("spacing", "cellsX", "cellsY", "boundary", "points", "features")
        if key in grid
    }
    records[6] = compact(grid_general)
    records[7] = ",".join(str(cell.get("h", 0)) for cell in grid_cells)
    records[8] = ",".join(str(cell.get("prec", 0)) for cell in grid_cells)
    records[9] = ",".join(str(cell.get("f", 0)) for cell in grid_cells)
    records[10] = ",".join(str(cell.get("t", 0)) for cell in grid_cells)
    records[11] = ",".join(str(cell.get("temp", 0)) for cell in grid_cells)

    records[12] = compact(pack["features"])
    records[13] = compact(pack["cultures"])
    records[14] = compact(pack["states"])
    records[15] = compact(pack["burgs"])

    pack_cells = pack["cells"]
    records[16] = cell_array(pack_cells, "biome")
    records[17] = cell_array(pack_cells, "burg")
    records[18] = cell_array(pack_cells, "conf")
    records[19] = cell_array(pack_cells, "culture")
    records[20] = cell_array(pack_cells, "fl")
    records[21] = cell_array(pack_cells, "pop")
    records[22] = cell_array(pack_cells, "r")
    records[24] = cell_array(pack_cells, "s")
    records[25] = cell_array(pack_cells, "state")
    records[26] = cell_array(pack_cells, "religion")
    records[27] = cell_array(pack_cells, "province")

    records[29] = compact(pack["religions"])
    records[30] = compact(pack["provinces"])
    records[32] = compact(pack["rivers"])
    records[35] = compact(pack["markers"])
    records[36] = cell_routes(pack_cells)
    records[37] = compact(pack["routes"])
    records[38] = compact(pack["zones"])
    records[41] = compact(pack["goods"])
    records[42] = compact(pack["markets"])
    records[43] = compact(pack["deals"])
    records[46] = compact(pack["measurers"])
    records[52] = compact(pack["journeys"])

    OUTPUT.write_bytes(b"\r\n".join(record.encode("utf-8") for record in records))
    print(f"wrote {OUTPUT}")
    print(f"records: {len(records)}; bytes: {OUTPUT.stat().st_size}")


if __name__ == "__main__":
    main()
