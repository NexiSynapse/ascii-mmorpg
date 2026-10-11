# ASCII MMORPG — Source of Truth

**Project status:** playable technical alpha / renderer spike  
**Repository workspace:** `C:\Games\ASCII-Game`  
**Language:** Rust stable  
**Target:** desktop terminals with a client-side animated ASCII/glyph scene renderer

This document is the single source of truth for the current implementation. It records what exists, what is authoritative, how to run it, what was verified, and what remains planned. The reference videos and analysis documents describe the visual target; they do not prove the original game's internal implementation.

## 1. Product direction

The game is a real-time fantasy MMORPG presented as a dark, spatial, perspective ASCII/glyph world. It is not intended to be a flat one-character-per-tile roguelike.

The visual target includes:

- Elevated oblique/isometric-like camera
- Dense glyph surfaces forming terrain, trees, paths, ruins, dungeons and gates
- Near-black/navy negative space
- Cyan/gray environmental glyphs
- Bright lime/yellow-green player entities
- Orange torch/fire lighting
- Clean white nameplates and status bars
- Rain-like diagonal streaks
- Animated player and monster silhouettes
- Animated walking, attacking, casting, hit and effect sequences
- Conventional UI layered cleanly above the stylized world

Reference evidence is recorded in the attached analysis documents:

- `ascii_mmorpg_reference_brief.md`
- `ascii_mmorpg_reconstruction.md`

The original rendering pipeline is unknown. This project uses a client-side hybrid approach: authored ASCII frames and procedural glyph materials now, with a future depth-aware scene/raster pipeline and optional Notcurses backend (evaluated in `NOTCURSES_RESEARCH_FINDINGS.md`).

## 2. Current architecture

```text
Keyboard / client input
        |
        v
ClientCommand intent
        |
        | TCP, length-prefixed postcard frame
        v
Per-client server reader thread
        |
        v
One authoritative simulation thread
        |
        +--> per-player WorldSnapshot views
        |
        v
Per-client writer threads
        |
        v
Client network reader
        |
        v
Client-side animated renderer
        |
        v
ANSI terminal output
```

### Server authority

The server owns:

- Player entities and IDs
- Player positions and stats
- Monster state
- Movement and combat resolution
- Tick progression
- Protocol validation
- Snapshot generation
- Multi-client broadcast

Clients send intents, not authoritative state. A client cannot directly claim a position, damage value or inventory change.

### Client responsibility

The client owns:

- Keyboard input
- ASCII asset selection
- Animation playback
- Scene presentation
- Glyph colours and materials
- Rain, spell trails and atmospheric effects
- Nameplate and HP-bar rendering
- Terminal output

Rendering does not mutate authoritative snapshots.

## 3. Workspace layout

```text
Cargo.toml                 Cargo workspace
crates/shared/             Protocol, content, combat, loot, framing
server/                    Authoritative simulation and TCP server
client/                    Input, TCP client, ASCII assets, renderer
bot/                       Headless TCP client and smoke client
tools/                     RON content validator
content/                   RON game definitions
docs/                      ADRs and this source of truth
assets/ascii/              Workspace-authored ASCII frames
assets/source/kenney/      Optional CC0 source reference packs
.github/                   CI workflow
```

## 4. Protocol and entity model

Shared protocol types live in `crates/shared/src/protocol.rs`.

`ActorSnapshot` currently carries:

- Entity ID
- Name
- `EntityKind`
- Position
- Stats/HP
- Glyph fallback

Current entity kinds:

```rust
Player
Monster
Npc
Item
```

Each connected client receives a personalized view:

- `snapshot.player` is that connection's local player
- `snapshot.entities` contains other players and visible monsters/items/NPCs

The current server gives every connection an independent player entity, while the demo world still has one Mossling monster.

## 5. Networking

Start the TCP server:

```text
cargo run -p ascii-mmorpg-server -- --tcp 127.0.0.1:4000
```

Connect the real-time client:

```text
cargo run -p ascii-mmorpg-client -- --connect 127.0.0.1:4000
```

The client supports:

- Welcome validation
- Protocol-version validation
- Immediate command sending without Enter
- Authoritative snapshot rendering
- Server errors
- EOF/disconnect handling
- Clean network-reader shutdown

The server supports:

- Multiple simultaneous TCP clients
- One simulation thread
- Per-client reader/writer threads
- Per-client personalized snapshots
- Snapshot broadcasting

## 6. Controls and modes

Default client mode is real-time:

```text
cargo run -p ascii-mmorpg-client
```

Controls:

```text
W/A/S/D or arrow keys     Move immediately
F                         Attack
R                         Rest
Q or Escape               Quit
```

Line-input compatibility mode:

```text
cargo run -p ascii-mmorpg-client -- --line
```

Finite animated renderer demo:

```text
cargo run -p ascii-mmorpg-client -- --demo
```

The current `--demo` mode renders six deterministic animated frames and exits.

## 7. ASCII asset system

The current visible art is ASCII-native and workspace-authored. It is embedded at compile time with `include_str!`, so the executable does not depend on its runtime working directory.

Current assets:

```text
assets/ascii/player_warrior.txt       Animated player frames
assets/ascii/monster_mossling.txt     Animated Mossling frames
assets/ascii/npc_herbalist.txt        Animated NPC fallback frames
assets/ascii/tree.txt                 Animated tree prop frames
assets/ascii/ruin.txt                 Animated ruin prop frames
assets/ascii/item_icons.txt           Sword, shield and generic item icons
assets/ascii/spell_effect.txt         Spell/effect glyph frames
assets/ascii/MANIFEST.md              Asset inventory and provenance
```

Frames are separated by a line containing `---` and selected by client render frame.

Item selection currently uses the item label:

- Label containing `sword` → sword icon
- Label containing `shield` → shield icon
- Otherwise → generic item icon

External ASCII collections were researched but not copied indiscriminately because user-contributed art often has unclear or per-work licensing. Useful references include:

- https://www.asciiart.eu/
- https://textart.sh/
- https://www.figlet.org/

Future imported art must have its exact licence and attribution recorded before inclusion.

## 8. Optional source assets

The following Kenney packs are staged as optional source references, not as final visible art:

```text
assets/source/kenney/mini-forest/
assets/source/kenney/mini-dungeon/
```

Archives:

```text
assets/source/kenney/kenney_mini-forest_1.0.zip
assets/source/kenney/kenney_mini-dungeon.zip
```

Both source pages identify the packs as Creative Commons CC0:

- https://kenney.nl/assets/mini-forest
- https://kenney.nl/assets/mini-dungeon

These may later help with silhouette studies, scene layout or a converter, but the current renderer does not load FBX/GLTF models.

## 9. Current renderer

The client renderer lives in `client/src/renderer.rs`.

It currently provides:

- Dark forest/dungeon-inspired backdrop
- Moss and stone floor treatment
- Procedural foliage and ruin props
- Rain-like diagonal streaks
- `EntityKind`-specific palettes
- Embedded authored ASCII character frames
- Nameplates
- HP bars
- Spell/attack trail
- Deterministic frame rendering
- ANSI output
- Snapshot immutability

The renderer is deliberately a bounded spike. It demonstrates the presentation architecture and animation system but is not yet a full 3D rasterizer or Notcurses renderer.

## 10. Animation model

Animation is client-side. The server remains authoritative for gameplay state.

Current frame selection is based on the render frame and does not alter snapshots.

Supported visual concepts:

- Idle frame variation
- Walking/pose variation
- Monster animation variation
- Prop animation variation
- Spell glyph cycling
- Rain movement
- Attack/effect trail changes

The next protocol refinement should add explicit server animation state and sequence IDs, for example:

```rust
AnimationState::Idle
AnimationState::Walking
AnimationState::Attacking
AnimationState::Casting
AnimationState::Hit
AnimationState::Dead
```

The client should interpolate/play frames locally rather than receive animation frames over the network.

## 11. Content and gameplay currently present

RON content exists for:

- Zones
- Monsters
- Loot
- Items
- Spells

The current alpha includes:

- Whispering Forest
- Ruined Gate
- Rootbound Dungeon
- Mossling demo enemy
- Warrior spell definitions
- Starter equipment definitions
- Deterministic combat and weighted loot rules
- 20 Hz fixed-timestep clock
- TCP framing and loopback smoke path

The following full MMORPG systems are not yet implemented:

- Accounts and authentication
- SQLite persistence
- Inventory/equipment server actions
- XP and level progression
- Multiple enemy AI states
- Boss phases and telegraphs
- Full spell resolution
- Zone streaming
- Pathfinding
- ECS migration
- Interest management
- Prediction/reconciliation/interpolation
- Notcurses backend
- Full scene geometry/rasterizer

## 12. Verification commands

Run formatting:

```text
cargo fmt --all
```

Run all tests:

```text
cargo test --workspace
```

Run lint with warnings denied:

```text
cargo clippy --workspace --all-targets -- -D warnings
```

Validate RON content:

```text
cargo run -p ascii-mmorpg-tools -- content
```

Run the offline client:

```text
cargo run -p ascii-mmorpg-client
```

Run the animated renderer demo:

```text
cargo run -p ascii-mmorpg-client -- --demo
```

Run two network bots against one server:

```text
cargo run -p ascii-mmorpg-server -- --tcp 127.0.0.1:4000
cargo run -p ascii-mmorpg-bot -- 127.0.0.1:4000 10
```

The current project has repeatedly passed formatting, workspace tests, clippy with warnings denied, content validation and renderer demo execution.

## 13. Next implementation priorities

1. Add explicit `AnimationState`, facing and sequence metadata to snapshots.
2. Add Skeleton and Skeleton Warrior authored ASCII frame sets.
3. Add a real animation clock and state transition playback.
4. Add target selection and a clean target-health panel.
5. Add inventory/equipment UI with ASCII item icons.
6. Add client-side interpolation and local prediction/reconciliation.
7. Replace the current simple backdrop with a camera-aware scene/grid representation.
8. Add frame dumps and snapshot tests for reference scenes.
9. Build the asset conversion/authoring tools for larger ASCII scenes.
10. Add Notcurses behind a safe terminal backend abstraction.
11. Add persistence and account boundaries.
12. Add ECS and richer multiplayer world state.

## 14. Non-goals and guardrails

- Do not move rendering work to the server.
- Do not send rendered frames over the network.
- Do not let clients author authoritative positions, HP, damage or inventory.
- Do not bulk-copy online ASCII artwork without licence verification.
- Do not treat the reference videos as proof of the original engine or mechanics.
- Keep authored ASCII assets editable, reviewable and attribution-safe.
