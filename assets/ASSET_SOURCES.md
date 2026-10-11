# Asset sources

The first visual reference packs are downloaded under `assets/source/kenney/`.
They are optional source references only; the server never loads or transmits
these files. The current renderer uses the original workspace-authored,
plain-text ASCII frames listed in `assets/ascii/MANIFEST.md`.

## Kenney Mini Forest

- Source: https://kenney.nl/assets/mini-forest
- Download: `source/kenney/kenney_mini-forest_1.0.zip`
- Local extracted source: `source/kenney/mini-forest/`
- Licence: Creative Commons CC0
- Useful content: trees, grass/ground patches, rocks, bridges, flags, platforms,
  animated character source and forest props.

## Kenney Mini Dungeon

- Source: https://kenney.nl/assets/mini-dungeon
- Download: `source/kenney/kenney_mini-dungeon.zip`
- Local extracted source: `source/kenney/mini-dungeon/`
- Licence: Creative Commons CC0
- Useful content: gates, columns, floors, stones, banners, chests, shields,
  characters, props and dungeon structures.

## Conversion policy

These packs are visual source assets, not the final ASCII art in the current
slice. The checked-in ASCII library is authored in the workspace and embedded
at compile time; it does not load or convert Kenney models. A future client
asset pipeline may convert selected models into terminal-friendly scene
geometry, material palettes and animation clips. Keep each source pack's
`License.txt` with the downloaded files. Before adding any future pack, record
its exact licence here and verify redistribution terms.

## Imported medieval gate slice

- Source model: `source/kenney/mini-dungeon/Models/OBJ format/gate.obj`
- Source licence: Creative Commons CC0, retained in the adjacent pack
- Runtime conversion: `assets/ascii/gate.txt`
- Status: first static terminal adaptation; geometry/depth conversion remains a
  planned follow-up and the OBJ remains the provenance reference.

## Workspace-authored skeletal undead (reference-look, no third-party art)

- Files: `assets/ascii/skeleton.txt`, `assets/ascii/skeleton_warrior.txt`
- Licence: original workspace-authored art, same as the repository (MIT).
  No pixels or glyphs copied from any external collection.
- Provenance: visual idiom (pale spiky humanoid, rib striations, bone-white
  palette) studied from the project's own reference clips in `data/videos/`
  (multiplayer forest combat). Online ASCII collections (e.g. asciiart.eu's
  mythology/skeletons gallery) were deliberately NOT sourced: per-work
  licensing there is unclear and most ASCII art is copyrighted by default.
- Selection: client picks them by monster label (`Skeleton Warrior` before
  `Skeleton`, case-insensitive; see `client/src/assets.rs`), mirroring the
  existing item-icon label convention.
