# ASCII MMORPG — Reference-Aware Project Plan

**Status:** Active planning baseline
**Audience:** Project contributors and coding agents
**Last reviewed:** 2026-10-11

This document is the implementation guide for the ASCII MMORPG project. It combines
the current repository architecture with the supplied visual-reference analyses. It
is intentionally more authoritative for implementation direction than screenshots,
social-media captions, or guesses about the original game's internals.

## 1. Product definition

Build a dark, atmospheric online fantasy RPG whose spatial world is rendered as
dense coloured ASCII-like glyphs in a desktop terminal.

The target is a **perspective 3D or pseudo-3D spatial RPG**, not a flat
one-character-per-tile roguelike. The game should feel like a small online action
RPG presented through a terminal-native visual language:

- Elevated oblique or pseudo-isometric camera
- Forests, paths, dungeons, walls, gates, bridges and castles
- Dense glyph surfaces that communicate shape, depth and material
- Near-black/navy negative space
- Cyan/gray environmental glyphs
- Bright lime/yellow-green player entities
- Orange torch, fire and spell lighting
- Crisp conventional labels, bars and menus above the world
- Rain, particles, spell trails and boss telegraphs
- Multiplayer scenes containing multiple named entities
- Warrior progression, equipment, inventory, loot, spells and bosses

The exact original game engine and rendering pipeline are unknown. This project
recreates the observable visual and gameplay identity; it does not claim to recover
the original implementation.

## 2. Evidence policy

The visual reference documents are evidence, not source code specifications.

### Directly supported by the supplied footage

- The world has depth and perspective-like composition.
- Environmental forms are made from dense, coloured glyph-like marks.
- Forest, dungeon, fortress, gate and path-like environments appear.
- White conventional labels appear over entities.
- Enemy labels such as `Skeleton` and `Skeleton Warrior` appear.
- Bar-like status elements appear near or under entities.
- Rain-like diagonal streaks appear in a multiplayer scene.
- The scene changes continuously as the viewpoint or player moves.
- Boss footage shows a large boss bar, phase text, area effects and combat feedback.
- Inventory footage shows equipment slots, a bag grid, item tooltips and RPG stats.

The supplied videos were checked with frequent frame snapshots and contact sheets,
not just a single representative still. Across the three clips, the repeatable
visual signals are:

- Near-black/navy negative space with sparse green/teal vegetation and irregular
  gray corridors or structures; there is no persistent square tile grid.
- Small bright humanoids with floating white names, white `+` target markers and
  stable screen scale while the world scrolls around them.
- Orange/yellow structures and torches, red/orange danger, and blue-white magic;
  spell feedback is angular lightning, an impact halo and short-lived particles,
  not a continuous pink beam.
- A larger boss silhouette and a long top-centered red/orange boss bar in combat
  scenes; exploration remains visually sparse and minimally overlaid.
- Inventory scenes are separate crisp terminal panels with borders, grids,
  tooltips and stats; social-video captions are not game UI requirements.

### Plausible but unconfirmed

- The world is rendered from ordinary 3D geometry with an ASCII post-process.
- The camera follows the player.
- The entity bars are health bars.
- Named entities are networked players rather than scripted test actors.
- Orange points are torches and bright particles are spells.
- The original game uses server authority and persistent accounts.

### Do not fabricate as facts

- The original engine, language, renderer or networking stack
- Exact camera projection, cell pitch, glyph font or sampling algorithm
- Exact controls, movement speed, collision or pathfinding rules
- The meaning of every observed bar, number or visual effect
- Original server architecture, persistence or anti-cheat behavior

When uncertain, implement the smallest testable version and label the decision as a
project choice rather than an observation about the reference.

## 3. Current repository baseline

The repository is a compileable technical alpha and renderer spike.

### Already implemented

- Rust workspace with `shared`, `server`, `client`, `bot` and `tools` crates
- Versioned postcard protocol with length-prefixed frames
- Deterministic combat and weighted loot
- RON content schemas and validation
- Authoritative simulation model
- TCP client/server skeleton with multiple client connections
- Personalized snapshots containing local and remote entities
- ANSI/ASCII fallback renderer
- Compile-time embedded authored ASCII frames
- Procedural forest/dungeon-inspired backdrop
- Labels, HP bars, rain-like effects and spell trails
- Loopback smoke path and deterministic bot
- Workspace tests, formatting, clippy and content validation

### Current content

- Whispering Forest
- Ruined Gate
- Rootbound Dungeon
- Mossling monster
- Warrior spell definitions
- Starter items and weighted loot

### Known baseline gaps

- TCP simulation is not yet continuously driven by a real 20 Hz clock; the fixed
  tick abstraction exists, but the network loop is effectively input-driven.
- `docs/adr/0002-fixed-tick.md` still describes the older 10 Hz single-client design.
- Animation state, facing and sequence IDs are not explicit protocol state.
- The backdrop is not yet a camera-aware spatial scene or depth-aware rasterizer.
- Inventory, equipment actions, XP progression, enemy AI and boss phases are absent.
- Accounts, authentication and SQLite persistence are absent.
- Interest management, prediction, reconciliation and interpolation are absent.
- Notcurses is only a planned backend; ANSI remains the working backend.

## 4. Target architecture

```text
Client input
  -> intent messages
  -> network transport
  -> server IO tasks
  -> authoritative 20 Hz simulation
  -> snapshot/event stream
  -> client scene projection
  -> glyph renderer and effects
  -> crisp UI composition
  -> terminal backend
```

### Server

The server owns all authoritative gameplay state:

- Accounts and sessions
- Player identity, position, facing and stats
- Enemy AI and combat
- Abilities, cooldowns and effects
- Inventory, equipment and loot
- XP, levels and progression
- Zone state and interest management
- Tick progression and snapshot generation

Clients send intents. The server validates movement, range, cooldowns, line of
sight, inventory legality and message rates before applying them.

The simulation thread is the only writer of world state. Network IO and database
persistence communicate with it through channels. Solo/test mode must use the same
simulation path through an in-process transport.

### Client

The client owns presentation:

- Input polling and capability detection
- Snapshot interpolation and local prediction
- Camera and screen projection
- Scene instance construction
- Glyph rasterization and cell conversion
- Effects, rain and animation playback
- Nameplates, bars, HUD and inventory UI
- Terminal output

Rendering must never mutate authoritative snapshots. The client should receive
state transitions and events, not pre-rendered frames.

### Terminal backend

All terminal-specific code belongs behind a small `term` boundary. The public API
should cover initialization, shutdown, resize, capability reporting, input polling,
cell writes and frame presentation.

The ANSI backend remains the fallback for CI, unsupported systems and frame dumps.
The Notcurses backend is optional and must not leak bindings into gameplay or
renderer code.

## 5. Rendering plan

The implementation must keep the renderer swappable because the references do not
prove whether the original uses post-processing, glyph materials or custom glyph
geometry.

### Preferred first experiment

```text
simple lit geometry
  -> depth/normal/material buffers
  -> 2x4 subpixel cell sampling
  -> brightness and edge glyph selection
  -> colour-tinted ANSI frame dump
```

This is the preferred hypothesis because it can reproduce depth, lighting,
silhouette and material variation without committing the entire game to a specific
asset representation.

### Per-frame pipeline

1. Build scene instances from the latest snapshot.
2. Project world coordinates through a camera.
3. Rasterize terrain, props, entities and effects into depth/material data.
4. Shade directional and point lights, fog and ambient darkness.
5. Convert sample regions into glyph cells.
6. Apply brightness quantization and anti-shimmer hysteresis.
7. Compose world, effects and crisp UI planes.
8. Present through ANSI or Notcurses.

### Visual requirements

- Preserve large dark regions and strong silhouettes.
- Use category-specific glyph density and palettes.
- Keep player entities bright and readable.
- Keep labels, bars and panels crisp rather than forcing them through the world
  glyph conversion.
- Use localized orange light and restrained glow.
- Add rain as a separate effect layer.
- Support frame dumps for visual regression tests.
- Add quality settings for sampling resolution, glow, rain and target FPS.

### First imported medieval slice (current)

The existing Kenney Mini Dungeon pack is the first licensed source used for the
medieval-dark direction. Its CC0 `gate.obj` is retained as provenance and has a
small compile-time terminal adaptation in `assets/ascii/gate.txt`. The gate is
now placed as a stable world-space landmark by the client renderer. This is an
intentional thin slice: it proves source tracking, static placement and visual
integration without pretending that hand-authored glyphs are a 3D converter.

Next conversion step: render the retained OBJ through an offline depth/material
sampler, compare its silhouette with the hand-authored gate, then promote only
the useful generated representation into the embedded asset library.

## 6. Gameplay model

The intended alpha includes:

- One Warrior class
- Three spells
- Forest/castle/gate outdoor content
- One dungeon
- Skeleton, Skeleton Warrior and one additional enemy type
- One multi-phase boss
- Combat, loot, inventory, equipment, XP and levels
- Real-time multiplayer target of roughly 20–50 players per server
- Accounts and persistence

The following remain post-alpha backlog:

- Crafting
- Auction house
- Guilds
- Quests
- Additional classes
- Monetization

## 7. Protocol direction

Retain TCP and length-prefixed postcard frames initially. Move to UDP or QUIC only
if profiling demonstrates that TCP head-of-line blocking is a real problem.

The protocol should evolve toward these message categories:

### Client to server

- `Hello`
- `Login`
- `Move`
- `Target`
- `Attack`
- `Cast`
- `Equip`
- `Unequip`
- `Drop`
- `Pickup`
- `Chat`
- `Ping`

### Server to client

- `Welcome`
- Snapshot or snapshot delta
- Damage/heal/death/loot events
- Spell and boss-phase events
- Inventory updates
- Stats updates
- Chat
- Pong

The server should send animation state, facing and sequence metadata. The client
should play animation frames locally rather than receive rendered animation frames.

## 8. Delivery milestones

### M0 — Backend and capability spike

**Goal:** prove terminal access without destabilizing the existing ANSI path.

- Add the terminal backend boundary.
- Test Notcurses initialization separately.
- Report colour, mouse, keyboard, resize and synchronized-update capabilities.
- Keep ANSI and headless paths working.

**Exit criteria:** Linux and Windows builds can run the capability check; CI stays
green; no gameplay code imports Notcurses bindings.

### M1 — Fixed tick and spatial renderer spike

**Goal:** connect a real server-driven entity to a spatial glyph renderer.

- Drive TCP simulation continuously at 20 Hz.
- Queue commands and apply them at tick boundaries.
- Add world units, camera coordinates and screen projection.
- Render a lit plane/cube or equivalent minimal scene.
- Add glyph conversion and frame dumps.

**Exit criteria:** one server-driven entity moves in a spatial scene; tick cadence,
frame time and raster stages are measured.

### M2 — First visual environment

**Goal:** reproduce the reference identity before adding broad gameplay.

- Forest path and vegetation
- Dungeon corridor
- Gate or fortress structure
- Material-specific glyph palettes
- Rain and torch lighting
- Crisp labels and target bars
- Anti-shimmer tuning

The first renderer slice should therefore prioritize stable layered composition:
dark negative space, sparse material glyphs, centered camera framing, target
markers and short event-shaped spell effects. Static terrain must not change when
the presentation frame advances; only rain, actor pose and active effects may do so.

**Exit criteria:** a short captured scene is recognisably the intended dark,
depth-aware glyph RPG rather than a flat text map.

### M3 — Fun combat loop

**Goal:** establish whether the game is enjoyable at terminal resolution.

- Skeleton and Skeleton Warrior
- Target selection
- Melee attacks and cooldowns
- Damage feedback
- Loot drops
- XP and level-up
- Initial boss placeholder

**Exit criteria:** a player can find, fight, defeat, loot and progress from an enemy.

### M4 — Inventory and equipment

**Goal:** implement the RPG data loop visible in the references.

- Equipment slots
- 120-slot bag model or a smaller tested equivalent initially
- Item rarity and damage ranges
- Equip, unequip, drop and pickup
- Tooltip and derived stat display
- Server-side legality checks

**Exit criteria:** items can be obtained, inspected, equipped, unequipped and
persisted through the authoritative server state.

### M5 — Multiplayer depth

**Goal:** make multiple players reliable in one world.

- Interest management
- Snapshot deltas
- Remote interpolation
- Local prediction and reconciliation
- Chat and login flow
- Account boundary
- SQLite save/load

**Exit criteria:** two or more clients can fight in the same area and restart the
server without losing committed player data.

### M6 — Spells and boss encounter

**Goal:** reproduce the most distinctive combat presentation.

- Three server-resolved spells
- Spell visual archetypes
- Area telegraphs
- Boss health bar
- Phase transitions
- Distinct boss attacks and effects

**Exit criteria:** a group can defeat a multi-phase boss with readable warnings and
feedback.

### M7 — Content and balance

**Goal:** provide a 30–45 minute playable slice.

- Second zone or expanded dungeon
- Enemy placement
- Loot tuning
- Bot-driven balance runs
- Settings and quality modes

### M8 — Public alpha readiness

**Goal:** support a small real player session.

- Windows/Linux compatibility pass
- Onboarding and recovery behavior
- Load testing
- Deployment and backups
- Metrics and crash reporting

**Exit criteria:** approximately 20–50 real players can share a session within the
measured server and bandwidth budgets.

## 9. Performance and quality budgets

Initial targets:

- Client: 30 FPS at full-screen 1080p terminal on a mid-range laptop
- Server: under 20 ms per tick at 50 players
- Network: under 20 KB/s per player during typical play

Measure each stage independently:

- Scene build
- Rasterization
- Shading
- Cell conversion
- UI composition
- Terminal presentation

Never add `rayon`, a custom cache, or a more complex transport solely because the
plan mentions it. Add it when profiling shows the simpler implementation misses a
budget.

## 10. Testing strategy

Required checks should remain small and executable:

- Unit tests for shared protocol round trips, combat, stats and loot
- Fixed-tick simulation tests
- Server/bot integration fights
- Content validator tests
- Frame-dump snapshot tests for fixed scenes
- Terminal capability tests where the platform permits them
- Bot swarm load tests
- Manual matrix: Windows Terminal, Kitty or WezTerm on Linux, and ANSI fallback

Every non-trivial renderer or simulation change should leave one runnable check
behind. Prefer deterministic tests over timing-sensitive tests.

## 11. Architectural guardrails

- Do not move rendering work to the server.
- Do not send rendered frames over the network.
- Do not let clients author positions, HP, damage or inventory.
- Keep terminal bindings inside the terminal backend module.
- Keep the ANSI fallback usable without native libraries.
- Do not choose a rendering hypothesis as historical fact.
- Do not copy third-party ASCII artwork without licence verification.
- Keep authored assets editable and reviewable.
- Keep protocol changes explicit and versioned.
- Prefer the smallest implementation that can validate the next visual or gameplay
  hypothesis.

## 12. Decisions to record as ADRs

Create or update short ADRs when these choices become real:

1. Continuous 20 Hz TCP simulation semantics
2. World units, camera projection and coordinate conventions
3. Glyph sampling resolution and cell conversion
4. Renderer layering and UI composition
5. Notcurses version and platform support
6. ECS adoption boundary, if profiling justifies it
7. Persistence schema and account lifecycle
8. Interest-management and snapshot-delta semantics

Until then, treat the plan as direction rather than permission to build every listed
system immediately.

## 13. Immediate next actions

1. Correct the TCP loop so it advances independently of incoming commands.
2. Update ADR 0002 from the obsolete 10 Hz/single-client description.
3. Add explicit animation state, facing and sequence IDs to snapshots.
4. Replace the current horizon/sky composition with a top-down world-space
   viewport. Terrain, props, actors and effects must use the same player-following
   camera transform so stationary mobs never appear to move under input.
 5. Build the first reference-shaped scene: irregular forest ground, open dark
    space, paths, clustered trees/ruins, larger readable actors, floating labels,
    target markers and localized blue-white impact effects. Keep debug text out of
    the playfield; retain it only in a small footer while the prototype is tested.
 6. Add a minimal depth-aware glyph conversion experiment and frame dump after the
    layered prototype has a stable visual baseline.
7. Recreate one forest path, one dungeon corridor and one gate scene.
8. Add target selection and a clean target-health panel.
9. Decide whether Notcurses, `hecs`, `glam`, `rayon` or model loading are justified
   only after measured prototype results.

### M2A — Reference-shaped viewport (active build slice)

This is the immediate implementation slice, not a claim that the original engine
has been recovered. It deliberately uses the existing ANSI canvas and authored
assets before introducing a 3D dependency.

- Use world coordinates for deterministic terrain and prop placement.
- Follow the local player with one shared camera transform for every world layer.
- Remove the artificial horizon and sky/ground split.
- Prefer dark negative space, irregular green/teal glyph clusters and sparse
  landmarks over repeated horizontal patterns.
- Keep actor silhouettes readable and animate only their authored pose frames.
- Treat attack/cast visuals as localized, short-lived event feedback rather than a
  persistent beam.

**Exit criteria:** walking moves the world consistently; the mob remains stationary
in world space; the scene reads as a top-down forest/ruin viewport; no pink stream
or side-on horizon remains; renderer tests cover camera consistency and stable
terrain across presentation frames.

## 14. Asset plan: models through glyph conversion

The visual plan assumes that the world is built from low-poly 3D models and then
rendered through the glyph converter. True hand-authored ASCII is reserved mainly
for item icons, title screens and UI mockups. This distinction prevents us from
trying to draw every environment directly as static text.

### 14.1 Candidate sources

| Source | Use | License check |
| --- | --- | --- |
| Quaternius | Skeletons, dragons, characters, RPG props and environments | CC0 collection; verify the specific download page |
| KayKit | Dungeon pieces, RPG props and animated fantasy characters | Verify the pack's included license |
| Kenney | Buildings, towers, walls, siege and general props | CC0 |
| Poly Pizza | Occasional one-off props | Usually CC-BY; record author and attribution |

Specific first candidates are Quaternius' Animated Monster Pack for skeleton and
dragon silhouettes, a Quaternius Low Poly RPG Pack for general characters and
props, and KayKit or Kenney dungeon/gate pieces. These are candidates, not yet
imported assets.

### 14.2 Conversion and runtime strategy

1. Download or receive one legally usable candidate pack.
2. Record the exact source URL, pack name, license, author and local file hashes
   in `assets/ASSET_SOURCES.md`.
3. Convert FBX, OBJ or Blend assets in Blender, or initially load OBJ from the
   `tools` crate if that is enough for the experiment. Do not add a model loader
   to the gameplay client before the converter prototype proves it is needed.
4. Convert a static pose first. Do not build skeletal animation for the alpha;
   split rigid parts or use a static silhouette until the renderer is validated.
5. Pass the model through the glyph pipeline: geometry -> depth/normal/material
   buffers -> cell sampling -> glyph and colour selection.
6. Compare silhouette, lighting and readability in Windows Terminal and one Linux
   terminal before accepting the asset.

### 14.3 ASCII and terminal tools

- **REXPaint:** author and polish item icons, title screens, inventory panels and
  paper-doll UI; use its `.xp` format where practical.
- **`rexpaint` Rust crate:** evaluate for reading `.xp` UI assets in the tools or
  client pipeline.
- **Chafa:** use only as an offline image-to-cell draft/conversion tool; its
  LGPLv3-or-later license must remain isolated from shipped gameplay code.
- **Notcurses blitters:** evaluate before adding Chafa or another converter to the
  runtime, because Notcurses may already cover image-to-cell conversion.

### 14.4 First asset import slice

1. Quaternius animated monster pack: one skeleton and one dragon silhouette.
2. KayKit or Quaternius dungeon/RPG pack: gate, wall, torch and ruin pieces.
3. Kenney: supplementary castle/building pieces only if the first pack lacks a
   coherent structure set.
4. REXPaint: player/enemy item icons, title treatment and inventory mockup.

**Asset milestone exit criteria:** one imported low-poly player or enemy, one
environment prop and one torch/gate scene render through the glyph converter with
recorded licensing, stable silhouettes, readable material colours and a frame dump.
Do not copy a whole project. Reuse small, well-understood converter, camera,
terminal and UI components from compatible open-source projects only after checking
their licenses and adapting them to this architecture.

## 15. Reference documents

Repository architecture and decisions:

- `README.md`
- `docs/SOT.md`
- `docs/adr/0001-shared-protocol.md`
- `docs/adr/0002-fixed-tick.md`
- `docs/adr/0003-terminal-fallback.md`
- `docs/adr/0004-alpha-boundaries.md`
- `assets/ASSET_SOURCES.md`
- `assets/ascii/MANIFEST.md`

Visual evidence supplied outside the repository:

- `ascii_mmorpg_reconstruction.md` — detailed observations from Videos 1–3,
  including boss combat, inventory and spell evidence.
- `ascii_mmorpg_reference_brief.md` — visual brief for Videos 4–6, including
  forest, dungeon, fortress and multiplayer scenes.

The visual documents are reference evidence. This plan is the actionable project
interpretation of that evidence and the current codebase.
