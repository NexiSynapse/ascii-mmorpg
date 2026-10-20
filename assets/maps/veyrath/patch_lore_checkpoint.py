"""Create a lore-named Azgaar .map checkpoint without changing its geometry.

Azgaar stores the map settings as the second CRLF-delimited record and the
serialized SVG/data records after it. This utility changes only settings.lore,
preserving every byte after the settings record so the generated base remains
an intact, reloadable checkpoint.
"""

from __future__ import annotations

import json
from pathlib import Path


SOURCE = Path(__file__).with_name("veyrath-01-base.map")
DESTINATION = Path(__file__).with_name("veyrath-02-lore.map")
NAME = "Veyrath — The Dying Crown"
DESCRIPTION = (
    "A dying medieval kingdom fractured by civil war, religious persecution, "
    "political collapse, and the failed campaign against an ancient supernatural "
    "force. The Crown still holds the heartland, rival powers control the forests, "
    "mountains, fen, frontier, and coast, while corruption remains concentrated "
    "around forgotten sites."
)


def main() -> None:
    data = SOURCE.read_bytes()
    separator = b"\r\n" if b"\r\n" in data else b"\n"
    records = data.split(separator)
    if len(records) < 7:
        raise ValueError("Not an Azgaar map record stream")

    header = records[0].decode("utf-8")
    fields = header.split("|")
    if len(fields) < 7 or fields[3] != "731204" or fields[4:6] != ["1600", "1100"]:
        raise ValueError(f"Unexpected base header: {header[:180]}")

    settings = json.loads(records[1].decode("utf-8"))
    settings.setdefault("lore", {})
    settings["lore"]["name"] = NAME
    settings["lore"]["description"] = DESCRIPTION
    records[1] = json.dumps(settings, ensure_ascii=False, separators=(",", ":")).encode("utf-8")

    DESTINATION.write_bytes(separator.join(records))
    restored = DESTINATION.read_bytes().split(separator)
    restored_settings = json.loads(restored[1].decode("utf-8"))
    if restored_settings["lore"]["name"] != NAME:
        raise ValueError("Lore checkpoint did not round-trip")
    if restored[5] != records[5] or restored[6:] != records[6:]:
        raise ValueError("Lore checkpoint changed generated geometry/data records")
    print(f"created {DESTINATION} ({DESTINATION.stat().st_size} bytes)")
    print(f"name={NAME}")
    print("geometry/data records preserved")


if __name__ == "__main__":
    main()
