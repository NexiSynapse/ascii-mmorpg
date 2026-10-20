"""Validate authoring inputs; this does not validate an actual Azgaar export."""

from collections import Counter, deque
import json
from pathlib import Path


def require(condition, message):
    if not condition:
        raise ValueError(message)


def indexed(records, label):
    result = {record["id"]: record for record in records}
    require(len(result) == len(records), f"Duplicate {label} IDs")
    return result


def point(position, label):
    require(
        len(position) == 2
        and all(isinstance(value, (int, float)) and 0 <= value <= 100 for value in position),
        f"Invalid provisional coordinate for {label}: {position}",
    )


def main():
    path = Path(__file__).with_name("world-design.json")
    design = json.loads(path.read_text(encoding="utf-8"))
    require(
        design["status"] in {
            "design_only_pending_azgaar_generation",
            "base_generated_pending_lore_geography_and_exports",
            "base_generated_pending_geography",
            "geography_generated_pending_political_edits",
            "political_checkpoint_pending_visual_review",
            "political_checkpoint_export_reviewed_placement_pending",
        },
        "Unexpected authoring status",
    )
    regions = indexed(design["regions"], "region")
    factions = indexed(design["factions"], "faction")
    cultures = indexed(design["cultures"], "culture")
    religions = indexed(design["religions"], "religion")
    settlements = indexed(design["settlements"], "settlement")
    landmarks = indexed(design["landmarks"], "landmark")
    passes = indexed(design["passes"], "pass")
    rivers = indexed(design["hydrology_intent"]["rivers"], "river")
    lakes = indexed(design["hydrology_intent"]["lakes"], "lake")

    require(len(regions) == len(factions) == len(cultures) == 6, "Expected six regions, factions, cultures")
    require(len(religions) == 4, "Expected four religious traditions")
    require(len(settlements) == 40, "Expected forty settlements")
    settlement_counts = Counter(record["kind"] for record in settlements.values())
    require(settlement_counts["capital"] == 1, "Expected one major capital")
    require(settlement_counts["city"] == 3, "Expected three major cities")
    require(settlement_counts["fortified_town"] == 6, "Expected six fortified towns")
    require(settlement_counts["village"] + settlement_counts["hamlet"] == 30, "Expected thirty villages/hamlets")
    require(len(landmarks) == 45, "Expected forty-five landmarks")
    landmark_counts = Counter(record["kind"] for record in landmarks.values())
    require(landmark_counts == {"fortification": 12, "ruin": 21, "special": 12}, "Landmark hierarchy mismatch")
    require(sum(record["principal"] for record in landmarks.values()) == 24, "Expected twenty-four principal landmarks")
    require(len(passes) == 4 and len(lakes) == 4, "Expected four principal passes and four notable lakes")

    for identifier, region in regions.items():
        require(region["faction"] in factions, f"Unknown faction for {identifier}")
        require(region["culture"] in cultures, f"Unknown culture for {identifier}")
        point(region["anchor"], identifier)

    for identifier, faction in factions.items():
        capital = settlements.get(faction["capital"])
        require(capital is not None and capital["faction"] == identifier, f"Invalid faction capital for {identifier}")

    for religion in religions.values():
        require(all(region in regions for region in religion["regions"]), "Unknown religious region")

    for identifier, settlement in settlements.items():
        require(settlement["region"] in regions, f"Unknown region for {identifier}")
        require(settlement["faction"] in factions, f"Unknown faction for {identifier}")
        point(settlement["at"], identifier)

    for identifier, landmark in landmarks.items():
        require(landmark["region"] in regions, f"Unknown landmark region for {identifier}")
        point(landmark["at"], identifier)
        if "settlement" in landmark:
            require(landmark["settlement"] in settlements, f"Unknown linked settlement for {identifier}")

    for identifier, river in rivers.items():
        require(len(river["path"]) >= 2, f"Too few river anchors for {identifier}")
        for anchor in river["path"]:
            point(anchor, identifier)
        if "joins" in river:
            require(river["joins"] in rivers, f"Unknown receiving river for {identifier}")
            require(river["path"][-1] in rivers[river["joins"]]["path"], f"Unshared confluence anchor for {identifier}")
            seen = {identifier}
            downstream = river
            while "joins" in downstream:
                receiving = downstream["joins"]
                require(receiving not in seen, f"Cyclic proposed drainage at {identifier}")
                seen.add(receiving)
                downstream = rivers[receiving]

    for identifier, lake in lakes.items():
        point(lake["anchor"], identifier)
        require(lake["outflow"] in rivers, f"Unknown intended lake outlet for {identifier}")

    nodes = set(settlements) | {f"pass:{identifier}" for identifier in passes}
    adjacency = {node: set() for node in nodes}
    transport = design["transport_intent"]
    road_sequences = [road["nodes"] for road in transport["royal_roads"]] + transport["regional_links"]
    for sequence in road_sequences:
        require(len(sequence) >= 2 and all(node in nodes for node in sequence), f"Invalid proposed route: {sequence}")
        for origin, destination in zip(sequence, sequence[1:]):
            adjacency[origin].add(destination)
            adjacency[destination].add(origin)

    queue = deque(["veyrath"])
    visited = {"veyrath"}
    while queue:
        origin = queue.popleft()
        for destination in adjacency[origin] - visited:
            visited.add(destination)
            queue.append(destination)
    require(visited == nodes, f"Unconnected proposed overland nodes: {sorted(nodes - visited)}")

    for trail in transport["wilderness_trails"]:
        require(trail["from"] in settlements and trail["to_landmark"] in landmarks, "Invalid wilderness trail reference")
    for origin, destination in transport["maritime_links"]:
        require(origin in settlements and destination in settlements, "Invalid maritime route reference")
    for crossing in transport["principal_crossings"]:
        require(crossing["river"] in rivers, "Unknown river at proposed crossing")
        point(crossing["anchor"], crossing["name"])
    for corruption in design["localised_corruption"]:
        require(corruption["landmark"] in landmarks, "Unknown corruption anchor")

    starter = design["starter_slice"]
    require(settlements[starter["spawn_settlement"]].get("starter") is True, "Starter settlement mismatch")
    require(starter["social_hub"] in settlements and starter["bandit_stronghold"] in landmarks, "Invalid starter hub/stronghold")
    require(all(identifier in landmarks for identifier in starter["nearby_objectives"]), "Invalid starter objective")
    for origin, destination in zip(starter["progression_route"], starter["progression_route"][1:]):
        require(destination in adjacency[origin], f"Starter route lacks proposed road: {origin} -> {destination}")

    print("Authoring manifest OK: 6 regions, 6 factions, 6 cultures, 4 religions")
    print("40 settlements: 1 capital, 3 cities, 6 fortified towns, 30 villages/hamlets")
    print("45 landmarks: 12 fortifications, 21 ruins, 12 special locations; 24 principal")
    print("44 planned overland nodes connected; 4 passes, 4 lakes; references valid")
    print("This validates design inputs only. See geography-review.md for the generated terrain checkpoint; final political placement acceptance remains pending.")


if __name__ == "__main__":
    main()
