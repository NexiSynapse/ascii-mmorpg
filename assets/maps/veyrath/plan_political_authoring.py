"""Build a reviewable political/name placement plan from the geography map.

The geography checkpoint is intentionally kept immutable.  This script turns
the manifest into an explicit editing brief: canonical records are matched to
generated Azgaar records, land cells receive a suggested faction/culture/
religion assignment, and settlements are either linked to a nearby generated
burg or marked for a new burg at a viable cell.
"""

from __future__ import annotations

import json
import math
from collections import Counter
from pathlib import Path


ROOT = Path(__file__).parent
DESIGN = json.loads((ROOT / "world-design.json").read_text(encoding="utf-8"))
FULL = json.loads((ROOT / "veyrath-full.json").read_text(encoding="utf-8"))
W, H = FULL["info"]["width"], FULL["info"]["height"]
CELLS = {c["i"]: c for c in FULL["pack"]["cells"]}
FEATURES = {f["i"]: f for f in FULL["pack"]["features"] if isinstance(f, dict)}
BIOMES = {b["i"]: b for b in FULL["pack"]["biomes"] if isinstance(b, dict)}
BURGS = [b for b in FULL["pack"]["burgs"] if isinstance(b, dict) and b.get("i", 0) > 0 and not b.get("removed")]

LAND_CELLS = [
    c
    for c in FULL["pack"]["cells"]
    if FEATURES.get(c.get("f"), {}).get("land", False) and c.get("h", 0) > 0
]
LAND_IDS = {c["i"] for c in LAND_CELLS}


def xy(anchor: list[float]) -> tuple[float, float]:
    return anchor[0] * W / 100.0, anchor[1] * H / 100.0


def distance(a: tuple[float, float], b: list[float] | tuple[float, float]) -> float:
    return math.hypot(a[0] - b[0], a[1] - b[1])


def cell_xy(cell: dict) -> tuple[float, float]:
    return tuple(cell["p"])


def generated_center(record: dict) -> tuple[float, float]:
    center = record.get("center")
    if center is None:
        return (0.0, 0.0)
    return cell_xy(CELLS[center])


def optimal_one_to_one(targets: list[tuple[str, tuple[float, float]]], records: list[dict]) -> dict[str, dict]:
    """Find the minimum-distance injective assignment with DP over bitmasks."""

    target_count = len(targets)
    record_count = len(records)
    if record_count < target_count:
        raise ValueError("not enough generated records for one-to-one assignment")

    costs = [[distance(anchor, generated_center(record)) for record in records] for _, anchor in targets]
    dp: dict[int, tuple[float, tuple[int, ...]]] = {0: (0.0, ())}
    for row in costs:
        next_dp: dict[int, tuple[float, tuple[int, ...]]] = {}
        for mask, (total, chosen) in dp.items():
            for index in range(record_count):
                if mask & (1 << index):
                    continue
                new_mask = mask | (1 << index)
                candidate = (total + row[index], chosen + (index,))
                previous = next_dp.get(new_mask)
                if previous is None or candidate[0] < previous[0]:
                    next_dp[new_mask] = candidate
        # The largest state space here is 14 choose 6 for cultures or 22 choose
        # 4 for religions, so retaining every mask keeps the assignment exact
        # without making this authoring utility depend on scipy.
        dp = next_dp

    total, chosen = min(dp.values(), key=lambda item: item[0])
    if len(chosen) != target_count:
        raise AssertionError(f"assignment has {len(chosen)} records for {target_count} targets")
    return {targets[row][0]: records[index] for row, index in enumerate(chosen)}


def nearest_land_cell(
    anchor: list[float], *, shoreline: bool = False, settlement_kind: str = "village"
) -> tuple[dict, float, dict]:
    target = xy(anchor)
    raw_candidates = LAND_CELLS
    candidates = [cell for cell in LAND_CELLS if cell.get("biome") not in {0, 11}]
    if not candidates:
        candidates = LAND_CELLS
    if shoreline:
        shoreline_cells = []
        for cell in candidates:
            neighbours = cell.get("c") or []
            if any(neighbour not in LAND_IDS for neighbour in neighbours):
                shoreline_cells.append(cell)
        if shoreline_cells:
            candidates = shoreline_cells
    cell = min(candidates, key=lambda candidate: distance(target, cell_xy(candidate)))
    raw_cell = min(raw_candidates, key=lambda candidate: distance(target, cell_xy(candidate)))
    flags = []
    if raw_cell["i"] != cell["i"]:
        flags.append(f"raw_anchor_biome_{BIOMES.get(raw_cell['biome'], {}).get('name', 'unknown').lower().replace(' ', '_')}")
    if raw_cell.get("f") not in {cell.get("f"), 2}:
        flags.append("anchor_outside_mainland")
    if distance(target, cell_xy(cell)) > 25:
        flags.append("large_anchor_adjustment")
    if shoreline and not any(neighbour not in LAND_IDS for neighbour in cell.get("c") or []):
        flags.append("shoreline_not_confirmed")
    return cell, distance(target, cell_xy(cell)), {
        "raw_cell": raw_cell,
        "raw_distance": distance(target, cell_xy(raw_cell)),
        "flags": flags,
    }


def settlement_kind_rank(kind: str) -> int:
    return {"capital": 0, "city": 1, "fortified_town": 2}.get(kind, 3)


def burg_priority(burg: dict, kind: str) -> tuple[int, float]:
    group = burg.get("group") or ""
    preferred = {
        "capital": {"capital"},
        "city": {"city"},
        "fortified_town": {"fort"},
        "village": {"village", "town"},
        "hamlet": {"village", "town"},
    }.get(kind, {"village", "town"})
    return (0 if group in preferred or (kind == "fortified_town" and burg.get("walls")) else 1, -burg.get("population", 0))


def choose_burgs() -> list[dict]:
    """Choose unique nearby burgs, without pretending distant burgs are viable."""

    used: set[int] = set()
    assignment: list[dict] = []
    ordered = sorted(DESIGN["settlements"], key=lambda item: settlement_kind_rank(item["kind"]))
    max_existing_distance = 28.0
    for settlement in ordered:
        coastal = settlement["id"] in {"blackwake", "saltgrave", "brinewatch", "gullscar", "crowhaven"}
        target_cell, cell_distance, raw_site = nearest_land_cell(
            settlement["at"], shoreline=coastal, settlement_kind=settlement["kind"]
        )
        target = cell_xy(target_cell)
        candidates = [burg for burg in BURGS if burg["i"] not in used]
        candidates.sort(key=lambda burg: (distance(target, [burg["x"], burg["y"]]), burg_priority(burg, settlement["kind"])))
        nearest = candidates[0] if candidates else None
        burg_distance = distance(target, [nearest["x"], nearest["y"]]) if nearest else float("inf")
        use_existing = nearest is not None and burg_distance <= max_existing_distance
        if use_existing:
            used.add(nearest["i"])

        assignment.append(
            {
                "settlement_id": settlement["id"],
                "name": settlement["name"],
                "kind": settlement["kind"],
                "region": settlement["region"],
                "faction": settlement["faction"],
                "population_target": settlement.get("population_target"),
                "site": settlement.get("site", ""),
                "anchor": settlement["at"],
                "target_cell": target_cell["i"],
                "target_xy": [round(value, 2) for value in target],
                "target_height": target_cell["h"],
                "target_biome": target_cell["biome"],
                "target_biome_name": BIOMES.get(target_cell["biome"], {}).get("name", "unknown"),
                "cell_distance": round(cell_distance, 2),
                "raw_anchor_cell": raw_site["raw_cell"]["i"],
                "raw_anchor_distance": round(raw_site["raw_distance"], 2),
                "site_flags": raw_site["flags"],
                "existing_burg_id": nearest["i"] if use_existing else None,
                "existing_burg_name": nearest.get("name", "") if use_existing else None,
                "existing_burg_group": nearest.get("group", "") if use_existing else None,
                "existing_burg_state": nearest.get("state", 0) if use_existing else None,
                "burg_distance": round(burg_distance, 2) if nearest else None,
                "new_burg_required": not use_existing,
                "placement_note": (
                    "Create a new burg on target_cell and set its canonical name."
                    if not use_existing
                    else "Rename and promote the existing burg; keep its generated cell."
                ),
            }
        )
    return assignment


def canonical_records() -> tuple[list[dict], list[dict], list[dict]]:
    states = [s for s in FULL["pack"]["states"] if isinstance(s, dict) and s.get("i", 0) > 0]
    cultures = [c for c in FULL["pack"]["cultures"] if isinstance(c, dict) and c.get("i", 0) > 0]
    religions = [r for r in FULL["pack"]["religions"] if isinstance(r, dict) and r.get("i", 0) > 0]

    factions = DESIGN["factions"]
    faction_targets = [(faction["id"], xy(faction["territorial_seed"])) for faction in factions]
    state_matches = optimal_one_to_one(faction_targets, states)

    culture_targets = [(culture["id"], xy(culture["centre"])) for culture in DESIGN["cultures"]]
    culture_matches = optimal_one_to_one(culture_targets, cultures)

    religion_targets = [(religion["id"], xy(religion["centre"])) for religion in DESIGN["religions"]]
    religion_matches = optimal_one_to_one(religion_targets, religions)

    faction_by_id = {faction["id"]: faction for faction in factions}
    region_by_faction = {region["faction"]: region for region in DESIGN["regions"]}
    faction_rows = []
    for faction in factions:
        state = state_matches[faction["id"]]
        region = region_by_faction[faction["id"]]
        culture = culture_matches[region["culture"]]
        religion_ids = [religion["id"] for religion in DESIGN["religions"] if region["id"] in religion["regions"]]
        faction_rows.append(
            {
                "faction_id": faction["id"],
                "name": faction["name"],
                "color": faction["colour"],
                "territorial_seed": faction["territorial_seed"],
                "capital_settlement": faction["capital"],
                "region_id": region["id"],
                "generated_state_id": state["i"],
                "generated_state_name": state["name"],
                "generated_culture_id": culture["i"],
                "generated_culture_name": culture["name"],
                "generated_religion_ids": [religion_matches[religion_id]["i"] for religion_id in religion_ids],
                "generated_religion_names": [religion_matches[religion_id]["name"] for religion_id in religion_ids],
            }
        )

    canonical_cultures = []
    for culture in DESIGN["cultures"]:
        generated = culture_matches[culture["id"]]
        canonical_cultures.append(
            {
                "id": culture["id"],
                "name": culture["name"],
                "identity": culture["identity"],
                "centre": culture["centre"],
                "generated_id": generated["i"],
                "generated_name": generated["name"],
            }
        )

    canonical_religions = []
    for religion in DESIGN["religions"]:
        generated = religion_matches[religion["id"]]
        canonical_religions.append(
            {
                "id": religion["id"],
                "name": religion["name"],
                "identity": religion["identity"],
                "regions": religion["regions"],
                "centre": religion["centre"],
                "generated_id": generated["i"],
                "generated_name": generated["name"],
            }
        )
    return faction_rows, canonical_cultures, canonical_religions


def build_cell_plan(faction_rows: list[dict], canonical_religions: list[dict]) -> dict:
    faction_by_id = {row["faction_id"]: row for row in faction_rows}
    religion_by_region = {}
    for religion in canonical_religions:
        for region in religion["regions"]:
            religion_by_region[region] = religion

    seeds = [(row["faction_id"], xy(row["territorial_seed"])) for row in faction_rows]
    neutral_anchors = [xy(anchor) for anchor in DESIGN["neutral_frontiers"][0]["anchors"]]
    assignments = []
    counts = Counter()
    for cell in LAND_CELLS:
        point = cell_xy(cell)
        nearest_faction, nearest_seed = min(seeds, key=lambda item: distance(point, item[1]))
        nearest_distance = distance(point, nearest_seed)
        neutral_distance = min(distance(point, anchor) for anchor in neutral_anchors)
        # The Unclaimed March is deliberately a set of pockets, not a seventh
        # state. A 3% canvas radius leaves the main capitals and settlements in
        # their intended powers while preserving contested frontier space.
        faction_id = "unclaimed-march" if neutral_distance <= min(48.0, nearest_distance * 0.42) else nearest_faction
        row = faction_by_id.get(faction_id)
        if row is None:
            assignments.append({"cell": cell["i"], "faction": "unclaimed-march", "state": 0, "culture": 0, "religion": 0})
            counts["unclaimed-march"] += 1
            continue
        region_id = row["region_id"]
        religion = religion_by_region.get(region_id)
        assignments.append(
            {
                "cell": cell["i"],
                "faction": faction_id,
                "state": row["generated_state_id"],
                "culture": row["generated_culture_id"],
                "religion": religion["generated_id"] if religion else 0,
            }
        )
        counts[faction_id] += 1

    return {
        "rule": "nearest territorial seed with Unclaimed March pockets around the three neutral frontier anchors",
        "neutral_frontier_radius_rule": "min(48px, nearest_faction_distance * 0.42)",
        "counts": dict(counts),
        "cells": assignments,
    }


def main() -> None:
    faction_rows, canonical_cultures, canonical_religions = canonical_records()
    cell_plan = build_cell_plan(faction_rows, canonical_religions)
    settlements = choose_burgs()
    output = {
        "schema": "veyrath-political-authoring-plan-v2",
        "source": "veyrath-full.json and world-design.json",
        "immutable_source_checkpoint": "veyrath-03-geography.map",
        "factions": faction_rows,
        "cultures": canonical_cultures,
        "religions": canonical_religions,
        "neutral_frontier": DESIGN["neutral_frontiers"][0],
        "cell_plan": {
            "rule": cell_plan["rule"],
            "neutral_frontier_radius_rule": cell_plan["neutral_frontier_radius_rule"],
            "counts": cell_plan["counts"],
            "land_cell_count": len(cell_plan["cells"]),
        },
        "settlements": settlements,
        "review_flags": {
            "new_burg_required": [item["settlement_id"] for item in settlements if item["new_burg_required"]],
            "coastal_targets": [item["settlement_id"] for item in settlements if item["settlement_id"] in {"blackwake", "saltgrave", "brinewatch", "gullscar", "crowhaven"}],
            "high_cell_snap_distance": [item["settlement_id"] for item in settlements if item["cell_distance"] > 25],
        },
        "notes": [
            "This is a placement plan only; it does not mutate the Azgaar checkpoint.",
            "Cell assignments are an explicit starting pass for political editing, not a substitute for visual border review.",
            "Create new burgs for flagged targets instead of renaming a distant generated burg.",
            "Apply canonical state/culture/religion names and colours only after checking the generated SVG in Azgaar.",
        ],
    }
    path = ROOT / "political-authoring-plan.json"
    path.write_text(json.dumps(output, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    print(f"wrote {path}")
    print(f"faction/state assignments: {len(faction_rows)}")
    print(f"unique culture assignments: {len({item['generated_id'] for item in canonical_cultures})}")
    print(f"religion assignments: {len(canonical_religions)}")
    print(f"land-cell assignments: {len(cell_plan['cells'])}")
    print(f"new burgs required: {len(output['review_flags']['new_burg_required'])}")
    print("new burg targets:", ", ".join(output["review_flags"]["new_burg_required"]))


if __name__ == "__main__":
    main()
