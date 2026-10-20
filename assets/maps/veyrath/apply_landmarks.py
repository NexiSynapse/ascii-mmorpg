"""Add the canonical Veyrath landmark records to the political export.

Azgaar stores map POIs as marker records. This pass preserves generated
markers and appends one named marker for each manifest landmark at its snapped
geography cell. It also gives the five principal river records canonical names
and names the four principal road routes.
"""

from __future__ import annotations

import json
from pathlib import Path


ROOT = Path(__file__).parent
FULL = ROOT / "veyrath-04-political-full.json"
DESIGN = ROOT / "world-design.json"
SNAP = ROOT / "geography-snap.json"


ICON_BY_TYPE = {
    "fortification": ("fortifications", "glyph-1f3fa"),
    "ruin": ("ruins", "glyph-1f3fa"),
    "special": ("landmarks", "glyph-1f5ff"),
}


def marker_icon(landmark: dict) -> tuple[str, str]:
    name = landmark["name"].lower()
    if "bridge" in name or "cross" in name:
        return "bridges", "glyph-1f309"
    if "battlefield" in name or "banner" in name:
        return "battlefields", "glyph-2694-fe0f"
    if "mine" in name:
        return "mines", "glyph-26cf-fe0f"
    if "glacier" in name or "ice" in name:
        return "rifts", "glyph-1f300"
    if "oak" in name or "root" in name or "hunt" in name or "orchard" in name:
        return "sacred-forests", "glyph-1f333"
    if "abbey" in name or "chapel" in name or "priory" in name or "cloister" in name or "hospice" in name:
        return "dungeons", "glyph-1f5dd-fe0f"
    if "fleet" in name or "lighthouse" in name:
        return "pirates", "glyph-1f3f4-200d-2620-fe0f"
    return ICON_BY_TYPE[landmark["kind"]]


def main() -> None:
    full = json.loads(FULL.read_text(encoding="utf-8"))
    design = json.loads(DESIGN.read_text(encoding="utf-8"))
    snap = json.loads(SNAP.read_text(encoding="utf-8"))
    snap_by_id = {item["id"]: item for item in snap["landmarks"]}
    markers = [item for item in full["pack"]["markers"] if isinstance(item, dict)]
    used_names = {item.get("name") for item in markers}
    next_id = max((item.get("i", -1) for item in markers), default=-1) + 1
    created = []
    for landmark in design["landmarks"]:
        if landmark["name"] in used_names:
            continue
        site = snap_by_id[landmark["id"]]
        marker_type, icon = marker_icon(landmark)
        record = {
            "icon": icon,
            "type": marker_type,
            "x": site["cell_xy"][0],
            "y": site["cell_xy"][1],
            "cell": site["cell"],
            "i": next_id,
            "name": landmark["name"],
            "note": (
                f"Veyrath landmark; region={landmark['region']}; kind={landmark['kind']}; "
                f"principal={str(landmark.get('principal', False)).lower()}. {landmark.get('purpose', '')}"
            ),
            "landmarkId": landmark["id"],
            "region": landmark["region"],
            "principal": bool(landmark.get("principal", False)),
        }
        markers.append(record)
        used_names.add(landmark["name"])
        created.append(record)
        next_id += 1

    full["pack"]["markers"] = markers
    # Longest generated rivers are the most stable candidates for the five
    # named hydrology intents; retain their geometry and only replace labels.
    river_names = ["The Crownwater", "Blackroot River", "Frostvein River", "The Greywash", "Lornwater"]
    rivers = sorted(full["pack"]["rivers"], key=lambda river: river.get("length", 0), reverse=True)
    for river, name in zip(rivers, river_names):
        river["name"] = name
    route_names = ["The Crown Road", "The Old Westroad", "The Fen Causeway", "The Salt Road"]
    roads = [route for route in full["pack"]["routes"] if route.get("group") == "roads"]
    for route, name in zip(sorted(roads, key=lambda route: len(route.get("points", [])), reverse=True), route_names):
        route["name"] = name

    FULL.write_text(json.dumps(full, ensure_ascii=True, separators=(",", ":")) + "\n", encoding="utf-8")
    print(f"added landmarks: {len(created)}")
    print(f"total markers: {len(markers)}")
    print("renamed rivers:", ", ".join(river_names))
    print("renamed roads:", ", ".join(route_names))


if __name__ == "__main__":
    main()
