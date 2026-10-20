"""Validate the Veyrath political checkpoint and its derived atlas exports."""

from __future__ import annotations

import json
from pathlib import Path
from xml.etree import ElementTree

from PIL import Image


ROOT = Path(__file__).parent
DESIGN = json.loads((ROOT / "world-design.json").read_text(encoding="utf-8"))
FULL = json.loads((ROOT / "veyrath-04-political-full.json").read_text(encoding="utf-8"))


def living(records: list[dict]) -> list[dict]:
    return [record for record in records if record.get("i", 0) and not record.get("removed")]


def main() -> None:
    map_path = ROOT / "veyrath-04-political.map"
    records = map_path.read_bytes().split(b"\r\n")
    assert len(records) == 53, len(records)
    assert records[0].startswith(b"1.154.2|")
    settings = json.loads(records[1])
    assert settings["graph"]["width"] == 1600 and settings["graph"]["height"] == 1100
    assert b'width="1600"' in records[5] and b'height="1100"' in records[5]

    assert FULL["info"]["width"] == 1600 and FULL["info"]["height"] == 1100
    assert len(FULL["pack"]["cells"]) == 5648
    states = living(FULL["pack"]["states"])
    cultures = living(FULL["pack"]["cultures"])
    religions = living(FULL["pack"]["religions"])
    assert len(states) == 6, [state["name"] for state in states]
    assert len(cultures) == 6, [culture["name"] for culture in cultures]
    assert len(religions) == 4, [religion["name"] for religion in religions]

    expected_state_names = {faction["name"] for faction in DESIGN["factions"]}
    assert {state["name"] for state in states} == expected_state_names
    expected_culture_names = {culture["name"] for culture in DESIGN["cultures"]}
    assert {culture["name"] for culture in cultures} == expected_culture_names
    expected_religion_names = {religion["name"] for religion in DESIGN["religions"]}
    assert {religion["name"] for religion in religions} == expected_religion_names

    burg_names = {
        burg.get("name")
        for burg in FULL["pack"]["burgs"]
        if isinstance(burg, dict) and burg.get("i") and not burg.get("removed")
    }
    expected_settlement_names = {settlement["name"] for settlement in DESIGN["settlements"]}
    assert expected_settlement_names <= burg_names
    ports = {
        burg.get("name")
        for burg in FULL["pack"]["burgs"]
        if isinstance(burg, dict) and burg.get("port")
    }
    assert {"Blackwake Port", "Saltgrave", "Brinewatch", "Gullscar", "Crowhaven"} <= ports

    for name in ("veyrath-04-political-overview.svg", "veyrath-04-political-clean.svg"):
        root = ElementTree.parse(ROOT / name).getroot()
        assert root.get("width") == "1600" and root.get("height") == "1100"
        ids = {element.get("id") for element in root.iter() if element.get("id")}
        for element in root.iter():
            href = element.get("href") or element.get("{http://www.w3.org/1999/xlink}href")
            if href and href.startswith("#"):
                assert href[1:] in ids, (name, href)
        assert "vignette" not in ids
        assert "mask" in {attribute for element in root.iter() for attribute in element.attrib}
        state_labels = next(element for element in root.iter() if element.get("id") == "labels-state")
        assert {"The Ashen", "Crown of", "The Greyfall"} <= {
            text.strip()
            for element in state_labels.iter()
            for text in [element.text]
            if text and text.strip()
        }
    for name in ("veyrath-04-political-overview.png", "veyrath-04-political-clean.png"):
        with Image.open(ROOT / name) as image:
            assert image.size == (4800, 3300), (name, image.size)
    print("Political checkpoint OK: 6 states, 6 cultures, 4 religions, 40 named settlements")
    print("Editable map, live Full JSON-shaped export, standalone SVG views, and PNG atlas validated")


if __name__ == "__main__":
    main()
