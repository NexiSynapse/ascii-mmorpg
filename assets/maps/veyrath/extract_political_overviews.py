"""Extract the political checkpoint SVG and its reduced atlas view."""

from __future__ import annotations

from pathlib import Path

from lxml import etree


ROOT = Path(__file__).parent
SOURCE = ROOT / "veyrath-04-political.map"
OVERVIEW = ROOT / "veyrath-04-political-overview.svg"
CLEAN = ROOT / "veyrath-04-political-clean.svg"
SVG_NS = "http://www.w3.org/2000/svg"
XLINK_NS = "http://www.w3.org/1999/xlink"


def element(tag: str, **attributes: str) -> etree._Element:
    node = etree.Element(f"{{{SVG_NS}}}{tag}")
    for key, value in attributes.items():
        node.set(key, value)
    return node


def normalize_for_standalone_render(root: etree._Element) -> None:
    """Inline the small CSS/definition dependencies used by Azgaar's SVG."""

    # Azgaar supplies these through its application stylesheet.  A standalone
    # SVG (and MuPDF) otherwise paints the full background as land and fills the
    # coastline use black.
    landmass = root.xpath('//*[@id="landmass"]')
    if landmass:
        landmass[0].set("mask", "url(#land)")
    states_body = root.xpath('//*[@id="statesBody"]')
    if states_body:
        states_body[0].set("mask", "url(#land)")
        states_body[0].set("fill-rule", "evenodd")
    coastline = root.xpath('//*[@id="coastline"]')
    if coastline:
        coastline[0].set("fill", "none")
        coastline[0].set("stroke-linejoin", "round")
    sea_island = root.xpath('//*[@id="sea_island"]')
    if sea_island:
        sea_island[0].set("fill", "none")
    for node in root.iter():
        href = node.get("href")
        if href and href.startswith("#"):
            node.set(f"{{{XLINK_NS}}}href", href)
    # The app's vignette filter is not portable to MuPDF and can become a
    # large opaque rectangle. The atlas already has a readable ocean edge.
    vignette = root.xpath('//*[@id="vignette"]')
    if vignette:
        vignette[0].getparent().remove(vignette[0])

    defs = root.find(f"{{{SVG_NS}}}defs")
    if defs is None:
        defs = element("defs")
        root.insert(0, defs)
    definitions = {
        "burgs-atlas-circle": element("symbol", overflow="visible", viewBox="-6 -6 12 12"),
        "burgs-atlas-square": element("symbol", overflow="visible", viewBox="-6 -6 12 12"),
        "ports-anchor": element("symbol", overflow="visible", viewBox="-5.4 -5.4 10.8 10.8"),
        "defs-compass-rose": element("g"),
    }
    definitions["burgs-atlas-circle"].append(element("circle", cx="0", cy="0", r="5"))
    definitions["burgs-atlas-square"].append(element("rect", x="-5", y="-5", width="10", height="10"))
    port = definitions["ports-anchor"]
    port_group = element("g", transform="scale(.3333)")
    port_group.append(element("path", d="M0,-11 C-2,-11 -4,-9 -4,-7 C-4,-5 -2,-3 0,-3 C2,-3 4,-5 4,-7 C4,-9 2,-11 0,-11 Z M-16,8 C-12,15 -5,18 0,18 C5,18 12,15 16,8 L11,8 C8,12 4,14 0,14 C-4,14 -8,12 -11,8 Z"))
    port.append(port_group)
    for name, node in definitions.items():
        if not root.xpath(f'//*[@id="{name}"]'):
            node.set("id", name)
            defs.append(node)

    # MuPDF does not render Azgaar's SVG2 `textPath href` labels reliably.
    # Keep the same six political labels as ordinary positioned text so the
    # standalone atlas and the browser view agree.
    labels_state = root.xpath('//*[@id="labels-state"]')
    if labels_state:
        group = labels_state[0]
        for child in list(group):
            group.remove(child)
        specs = [
            ("stateLabel2", 738, 707, ("The Greyfall", "Compact"), None),
            ("stateLabel5", 809, 220, ("The Ashen", "Synod"), None),
            ("stateLabel6", 426, 405, ("House", "Gallowmere"), "rotate(18 426 405)"),
            ("stateLabel8", 1062, 802, ("The Blackwake", "League"), "rotate(36 1062 802)"),
            ("stateLabel9", 835, 505, ("Crown of", "Veyrath"), None),
            ("stateLabel4", 1227, 398, ("The Mire", "Courts"), None),
        ]
        for label_id, x, y, lines, transform in specs:
            text = element(
                "text",
                id=label_id,
                x=str(x),
                y=str(y),
                fill="#3e3e4b",
                stroke="#3a3a3a",
                **{"stroke-width": "0", "font-family": "Almendra SC, serif", "font-size": "44", "text-anchor": "middle"},
            )
            if transform:
                text.set("transform", transform)
            first = element("tspan", x=str(x), dy="-0.25em")
            first.text = lines[0]
            second = element("tspan", x=str(x), dy="1.05em")
            second.text = lines[1]
            text.extend((first, second))
            group.append(text)


def main() -> None:
    records = SOURCE.read_bytes().split(b"\r\n")
    root = etree.fromstring(records[5])
    if root.tag.rsplit("}", 1)[-1] != "svg" or root.get("width") != "1600" or root.get("height") != "1100":
        raise ValueError("Unexpected political checkpoint SVG")
    normalize_for_standalone_render(root)

    OVERVIEW.write_bytes(etree.tostring(root, xml_declaration=True, encoding="utf-8", pretty_print=True))
    clean = etree.fromstring(etree.tostring(root))
    remove_ids = {
        "burgIcons", "burgEmblems", "markers", "military", "journeys", "emblems", "goods", "markets",
        "population", "tradeAnimation", "armies", "labels-hamlet", "labels-village", "labels-town",
        "labels-caravanserai", "labels-trading_post"
    }
    for element in list(clean.iter()):
        if element.get("id") in remove_ids:
            parent = element.getparent()
            if parent is not None:
                parent.remove(element)
    CLEAN.write_bytes(etree.tostring(clean, xml_declaration=True, encoding="utf-8", pretty_print=True))
    print(f"created {OVERVIEW} ({OVERVIEW.stat().st_size} bytes)")
    print(f"created {CLEAN} ({CLEAN.stat().st_size} bytes)")


if __name__ == "__main__":
    main()
