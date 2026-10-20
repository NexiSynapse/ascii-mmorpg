# Veyrath — The Dying Crown

## Authoring status

**Base, lore metadata, geography, political checkpoint, and standalone atlas exports generated; settlement placement and final canon acceptance remain.**

`world-design.json` is an authoring manifest, not a playable game map. The
current political checkpoint is `veyrath-04-political.map`; its live Full JSON
data is `veyrath-04-political-full.json`. The raw geography checkpoint remains
`veyrath-03-geography.map`. Manifest coordinates are provisional composition anchors.
They must be relocated to suitable generated land, water, and elevation before
settlements, landmarks, roads, or borders are accepted.

The official Azgaar editor was opened at
<https://azgaar.github.io/Fantasy-Map-Generator/> (v1.154.2). A base candidate
was generated and saved as `veyrath-01-base.map` with seed `731204`, extent
`1600 × 1100`, 10K points, Pangea heightmap, six Dark Fantasy cultures, six
states, forty burgs, and four religions. The geography checkpoint was then
replaced with the deterministic terrain pass, and the political checkpoint
applies the first Veyrath canon names and assignments. The political atlas views
are normalized for standalone SVG/MuPDF rendering: Azgaar's CSS land mask,
coastline fill rule, referenced burg symbols, and political labels are embedded
in the derived views.

## Authoring manifest

- One connected mainland, open southern/southeastern sea, northern expansion
  beyond a broken mountain crescent.
- Six geographical regions and six political powers; the Unclaimed March is
  neutral territory, not a seventh state.
- Forty settlement targets: one capital, three cities, six fortified towns,
  and thirty villages/hamlets.
- Forty-five landmark targets: twelve fortifications, twenty-one ruins or
  abandoned structures, and twelve special/working locations.
- Six cultures, four religious traditions, four principal passes, four notable
  lakes, and a north-to-south main river with tributaries.
- The twenty-four principal landmarks are eligible for the low-zoom overview.
  Suppress collisions and duplicate settlement labels; capital-district details
  belong in a regional inset. Secondary locations belong at higher zoom or in
  the POI view.
- Corruption is localised around a few sites. Settled agricultural and trading
  landscapes remain the dominant inhabited environment.

## Coordinate and scale convention

All manifest positions use `[x, y]` percentages: west/east = 0/100,
north/south = 0/100. For a 1600 × 1100 generation extent, multiply x by 16
and y by 11. This is an authoring canvas, not a game-grid resolution.

The initial distance target is 0.25 miles per canvas unit (roughly 400 miles
across the canvas). Verify the actual land extent and travel expectations
before adopting this as canon. Azgaar cells describe regional geography;
local game terrain requires a separate conversion/detail-generation workflow.

## Resume in Azgaar

1. In Options, use approximately 10K points, six states, six cultures, four
   religions, and forty settlements. Use a mainland/continental candidate.
   Candidate seeds in the manifest are suggestions, not selected results.
2. Generate at least three candidates and record their actual generation
   settings and versions. Compare connected land, inland space, southern sea,
   and usable estuaries. Save the chosen base as `veyrath-01-base.map`.
3. In Tools → Heightmap, shape the northern crescent, western uplands, central
    basin, eastern floodplain, and southern valleys. Use Erase mode for major
    reconstruction before naming settlements. The deterministic input is
    `veyrath-heightmap.png`; the completed editor checkpoint is
    `veyrath-03-geography.map`.
4. Establish cold-to-temperate climate and biomes, then generate and inspect
   drainage. The planned river polylines are catchment intentions: actual
    routes must follow downhill cells. The current pass has one connected land
    component and generated drainage; the political checkpoint now applies the
    six Veyrath powers, four traditions, and neutral frontier rule.
5. Place the capital first, then other settlements on suitable land. Adjust
   manifest anchors to actual cells. Set states/cultures/religions, leave
   disputed frontier neutral, and create internal provinces.
6. Build terrain-aware roads and document the actual bridge/pass/causeway
   crossings. Add the forty-five landmark records and location notes.
7. Apply the manifest palette and readable label hierarchy. Save the finished
   editable map as `veyrath.map`, then export Full JSON and an overview image.

## Political checkpoint files — placement review pending

| File | Purpose |
| --- | --- |
| `veyrath-04-political.map` | Editable political Azgaar source checkpoint |
| `veyrath-04-political-full.json` | Political checkpoint Full JSON-shaped export |
| `veyrath-04-political-overview.png` | Political high-resolution atlas overview; 4800 × 3300 |
| `veyrath-04-political-clean.svg` | Scalable clean political atlas view |
| `geography-review.md` | Recorded checks against actual exported map data |

Current geography file: `veyrath-03-geography.map`; political file:
`veyrath-04-political.map` (both present; generated/rebuilt for Azgaar v1.154.2).
`veyrath-02-lore.map` is also present; its settings metadata names the world
“Veyrath — The Dying Crown”, and its generated geometry/data records are
preserved from the base checkpoint. The metadata checkpoint was produced by
`patch_lore_checkpoint.py` because browser takeover remained paused.

`veyrath-full.json` contains the live geography data model. `veyrath-04-political-full.json`
contains the political checkpoint data model, while `veyrath-04-political.map`
is the editable project form. `veyrath-02-lore-full.json` preserves the earlier
official export. The `veyrath-04-political-*` SVG/PNG files are extracted and
rasterized political atlas views at `4800 × 3300`; the older overview files
remain the geography views.

`geography-snap.json` records the nearest viable land cell and generated burg
for every provisional settlement and landmark anchor. It is the input for the
political/name placement pass. The five canonical ports now resolve to working
coastal burgs in the political checkpoint; the adjusted northern, fen, and
southern anchors still require final manual acceptance against the live map.

`political-authoring-plan.json` assigns six Veyrath factions to distinct
generated state records, six cultures, four religions, a neutral frontier rule,
and all forty manifest settlements to unique generated or newly-created burgs.
`apply_political_authoring.js` applies that plan to a new checkpoint; it leaves
`veyrath-03-geography.map` untouched. `validate_political_checkpoint.py` checks
the editable map stream, canonical record names/counts, ports, standalone SVG
references/labels, and PNGs.

Useful additional exports: physical, political, biome, and POI views; a starter
region crop; the pre-major-edit `.map` checkpoints. Record actual export sizes
and generator version. A seed alone does not preserve manual edits.

## Validation before acceptance

The authoring-input check passed:

```text
python assets/maps/veyrath/validate_design.py
```

The base `.map` header/file checks also passed. These checks do not establish
actual terrain, hydrology, political borders, or visual acceptance.

The authoring-input check passed:

```text
python assets/maps/veyrath/validate_design.py
```

It checks counts, unique IDs, region/faction references, planned confluences,
and the proposed road graph: all forty settlement nodes and four pass nodes
are connected. It does not establish actual geographical or route validity.

- All major regions share one connected mainland; any offshore islands have
  a geographical or gameplay purpose.
- The main river and tributaries drain toward their actual outlets. Inspect
  lake outlets, confluences, floodplain branches, and river elevations.
- Capital and settlements occupy viable cells; ports occupy real sheltered
  coast/harbour cells. Marsh settlements occupy dry rises or maintained banks.
- Principal roads form a connected network. Ridge and river crossings have
  named passes, bridges, fords, ferries, or causeways.
- State borders use defensible/economic geography and allow neutral frontiers.
- Names, counts, six-region placement, and capital prominence match the brief.
- At overview and regional zoom, roads/rivers/labels remain distinguishable.
- Reopen `veyrath-04-political.map` in Azgaar and compare it with the exported images and
  Full JSON before marking the project complete.

## First playable slice — design only

Start in Millhaven in Greyfall Vale. The route through Greyfall and King's
Bridge reaches Veyrath, then follows the Old Westroad through Harthstead and
Wychford toward Gallowmere. Nearby objectives include Greyfall Mill,
Blackford Bridge, the Broken Banner Battlefield, Gallows Manor, Red Ditch
Keep (bandit stronghold), and the western forest's Widow's Chapel.

These are future environment/quest anchors. The political atlas is now exposed
through the client's ASCII World Map modal; the locations are not yet used by
the server simulation, collision, or gameplay zone logic.

## Sources and rights

- Generator: <https://github.com/Azgaar/Fantasy-Map-Generator/>
- Tutorial: <https://github.com/Azgaar/Fantasy-Map-Generator/wiki/Quick-Start-Tutorial>
- JSON implementation: `src/services/io/export-json.ts` in the generator.
- Azgaar's MIT licence explicitly permits use of generated maps, including
  commercially. Retain its copyright/licence when redistributing its code.
- Veyrath's requested world identity and principal names come from the user's
  world brief; secondary names and the provisional manifest are project-authored.
