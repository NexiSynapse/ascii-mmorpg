"""Extract Azgaar's embedded SVG and a reduced atlas view from a .map file."""

from __future__ import annotations

from pathlib import Path

from lxml import etree


ROOT = Path(__file__).parent
SOURCE = ROOT / "veyrath-03-geography.map"
OVERVIEW = ROOT / "veyrath-overview.svg"
CLEAN = ROOT / "veyrath-clean.svg"


def read_svg() -> etree._Element:
    data = SOURCE.read_bytes()
    separator = b"\r\n" if b"\r\n" in data else b"\n"
    records = data.split(separator)
    svg = records[5]
    root = etree.fromstring(svg)
    if root.tag.rsplit("}", 1)[-1] != "svg" or root.get("width") != "1600" or root.get("height") != "1100":
        raise ValueError("Unexpected embedded Azgaar SVG")
    return root


def write_svg(root: etree._Element, path: Path) -> None:
    path.write_bytes(
        etree.tostring(
            root,
            xml_declaration=True,
            encoding="utf-8",
            pretty_print=True,
        )
    )


def main() -> None:
    root = read_svg()
    write_svg(root, OVERVIEW)

    clean = etree.fromstring(etree.tostring(root))
    remove_ids = {
        "burgIcons",
        "burgEmblems",
        "markers",
        "military",
        "journeys",
        "emblems",
        "goods",
        "markets",
        "population",
        "tradeAnimation",
        "armies",
        "labels-hamlet",
        "labels-village",
        "labels-town",
        "labels-caravanserai",
        "labels-trading_post",
    }
    for element in list(clean.iter()):
        if element.get("id") in remove_ids:
            parent = element.getparent()
            if parent is not None:
                parent.remove(element)
    write_svg(clean, CLEAN)

    print(f"created {OVERVIEW} ({OVERVIEW.stat().st_size} bytes)")
    print(f"created {CLEAN} ({CLEAN.stat().st_size} bytes)")
    print("clean view removes secondary POI clutter while retaining terrain, water, borders, routes, rivers, and principal labels")


if __name__ == "__main__":
    main()
