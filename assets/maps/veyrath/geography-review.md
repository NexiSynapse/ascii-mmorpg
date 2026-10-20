# Veyrath geography and political review — terrain checkpoint plus first canon pass

Review date: 2026-10-20

## Source

- Azgaar Fantasy Map Generator v1.154.2
- Base checkpoint: `veyrath-01-base.map`
- Lore checkpoint: `veyrath-02-lore.map`
- Geography checkpoint: `veyrath-03-geography.map`
- Heightmap input: `veyrath-heightmap.png`
- Seed: `731204`
- Extent: `1600 × 1100`
- Points: `10,000`
- Initial heightmap template: Pangea
- Geography pass: deterministic grayscale Image Converter input, then Azgaar
  regeneration of the graph, drainage, biomes, burgs, routes, and political cells

All three `.map` checkpoints parse as UTF-8 Azgaar record streams. The lore
checkpoint preserves the base geometry; the geography checkpoint intentionally
rebuilds the generated data after the heightmap conversion. The geography Full
JSON contains 5,648 pack cells, 10,043 grid cells, 689 burgs, 10 generated
states, 15 generated cultures, 206 rivers, and 497 routes.

## Current visual and graph assessment

The terrain pass produces one connected mainland with an open southern and
eastern sea, a broken high northern crescent, western uplands, a broad central
heartland basin, an eastern lowland/fen belt, and southern coastal valleys. The
actual pack-cell graph has one connected land component of 5,127 land cells;
521 pack cells are water. The generated drainage contains 206 rivers and the
overview SVG shows the new coastline and river network.

The climate settings remain cold-to-temperate and damp enough for the brief:
map latitude 45, equatorial temperature 22°C, north-pole temperature -33°C,
south-pole temperature -6°C, and precipitation 105. The regenerated map is
still an authoring geography, not the final Veyrath political or settlement
layout.

## Evidence produced

- `veyrath-01-base.map`: editable Azgaar base.
- `veyrath-02-lore.map`: editable lore checkpoint, geometry preserved.
- `veyrath-heightmap.png`: deterministic 1600 × 1100 grayscale terrain input.
- `veyrath-03-geography.map`: editable geography checkpoint after conversion.
- `veyrath-04-political.map`: editable political checkpoint after the first canon pass.
- `veyrath-02-lore-full.json`: official Full JSON export of the lore checkpoint.
- `veyrath-full.json`: live geography Full JSON data model with pack/grid data.
- `geography-snap.json`: nearest-cell and nearest-burg analysis for all 40
  settlement and 45 landmark anchors.
- `political-authoring-plan.json`: distinct generated state and burg assignments
  for the six factions and forty settlement names.
- `veyrath-04-political-full.json`: political checkpoint Full JSON-shaped export.
- `veyrath-04-political-overview.svg` and `veyrath-04-political-clean.svg`:
  political atlas views.
- `veyrath-04-political-overview.png` and `veyrath-04-political-clean.png`:
  4800 × 3300 political atlas rasterizations.
- `veyrath-overview.svg`: extracted embedded Azgaar geography overview.
- `veyrath-clean.svg`: overview with secondary POI clutter removed.
- `veyrath-overview.png`: 4800 × 3300 rasterized overview.
- `veyrath-clean.png`: 4800 × 3300 rasterized reduced overview.

Both JSON checkpoints parse as UTF-8 JSON and contain the Azgaar `info`,
`settings`, `mapCoordinates`, `pack`, `grid`, and `nameBases` sections. The
geography SVG is a valid XML document and provides a faithful overview of the
saved map state. The PNGs are local rasterizations of that embedded SVG, not
separate direct PNG downloads.

## Acceptance status

| Check | Status | Evidence or gap |
| --- | --- | --- |
| Base `.map` opens as an Azgaar record stream | Passed | v1.154.2 header, settings, SVG, and trailing records present |
| Design manifest counts and references | Passed | `python validate_design.py` |
| Embedded SVG dimensions | Passed | `1600 × 1100`, XML parse succeeds |
| One connected mainland | Passed | Pack-cell graph: 5,127 land cells in one component |
| Northern Ashen Crown | Partial | Broken northern high crescent is present; Crownfell/Synod settlement anchors were shifted to viable highland cells and still need live-map acceptance |
| Main river and tributaries | Partial | 206 regenerated rivers drain the terrain; five generated rivers now carry the Veyrath names; target confluences remain a visual/manual check |
| Eastern Hollow Fen and southern bays | Partial | Lowland/coast structure is present; fen and southern settlement anchors remain provisional |
| Provisional anchor snap | Partial | 40 settlements and 45 landmarks mapped; the five canonical ports resolve to coastal burgs, while adjusted northern/fen/southern sites remain flagged for manual review |
| Six Veyrath state borders | Passed for first checkpoint | Six canonical state records own the explicit political cell assignment; visual border review remains |
| Veyrath settlement names | Passed for first checkpoint | All forty manifest settlement names are present on unique burgs |
| Veyrath landmark names | Passed for first checkpoint | All forty-five canonical landmarks are Azgaar marker records with unique `landmarkId` values; icon/label collision review remains |
| Terrain-aware roads and crossings | Partial | 497 generated routes exist; four generated routes now carry the Veyrath road names; crossing geometry remains a visual/manual check |
| Dark atlas style | Partial | Political atlas SVG/PNG is standalone-renderable and visually coherent; final palette/label acceptance remains |
| Geography Full JSON | Passed | `veyrath-full.json` parses and matches the geography checkpoint counts |
| Political checkpoint Full JSON | Passed with reconstruction note | `veyrath-04-political-full.json` parses; six states, six cultures, four religions, 40 named burgs; it is a live-model Full JSON-shaped reconstruction, not a confirmed official Azgaar Full download |
| PNG overview | Passed with local rasterization | 4800 × 3300 PNGs validated with Pillow; standalone SVG masks/definitions/labels were normalized before rasterization |

## Next bounded slice

Use `veyrath-04-political.map` as the working source for visual review. The
first political pass has reduced the active records to six powers, six cultures,
four religions, a 121-cell Unclaimed March pocket set, forty named settlements,
forty-five landmark markers, four renamed routes, five renamed rivers, and five
canonical working ports. The next bounded slice is to accept or correct the
flagged northern/fen/southern settlement sites in the live map, make one final
controlled attempt at an official Azgaar Full export, and then begin the runtime
map-import design without mutating the editable checkpoint.
