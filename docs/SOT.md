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
crates/libnotcurses-sys/   Vendored and patched Notcurses FFI bindings
server/                    Authoritative simulation and TCP server
client/                    Input, TCP client, ASCII assets, ANSI/Notcurses renderer
bot/                       Headless TCP client and smoke client
tools/                     RON content validator
content/                   RON game definitions
docs/                      ADRs, research findings, and this source of truth
assets/ascii/              Workspace-authored ASCII frames
assets/source/kenney/      Optional CC0 source reference packs
play_ansi.bat              Windows launcher for ANSI client (uncapped by default)
play_notcurses.bat         Windows launcher for Notcurses client (uncapped by default)
play_wezterm.bat           WezTerm GUI launcher for ANSI client
play_wezterm_notcurses.bat WezTerm GUI launcher for Notcurses client
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
assets/ascii/skeleton.txt             Animated skeletal undead frames
assets/ascii/skeleton_warrior.txt     Animated shield-and-blade skeleton frames
assets/ascii/npc_herbalist.txt        Animated NPC fallback frames
assets/ascii/tree.txt                 Animated tree prop frames
assets/ascii/ruin.txt                 Animated ruin prop frames
assets/ascii/item_icons.txt           Sword, shield and generic item icons
assets/ascii/spell_effect.txt         Spell/effect glyph frames
assets/ascii/MANIFEST.md              Asset inventory and provenance
```

Frames are separated by a line containing exactly `---` (line-based split, so art itself may contain runs like `|---|`) and selected by client render frame.

Monster art is selected by label (`Skeleton Warrior` before `Skeleton`, case-insensitive; otherwise Mossling), mirroring item-icon selection:

- Label containing `skeleton warrior` → skeleton warrior frames
- Label containing `skeleton` → skeleton frames
- Otherwise → Mossling frames

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

## 15. Recent updates and technical changelog

### 15.1 Notcurses Windows integration & research findings
- **Research & documentation**: Analyzed Notcurses C engine architecture on Windows/MSYS2 (`windows.c`, ConPTY IPC, cell rasters, direct mode). Documented root-cause findings and architecture in `docs/NOTCURSES_RESEARCH_FINDINGS.md`.
- **Vendored FFI bindings**: Staged and patched `crates/libnotcurses-sys` via `[patch.crates-io]` in root `Cargo.toml`.
- **Target support**: Enabled build support for `x86_64-pc-windows-gnu` linking MSYS2 ucrt64 `notcurses.dll` / `libnotcurses-core.dll` via `--features notcurses`.
- **Terminal backend abstraction**: Implemented `NotcursesTerminal` in `client/src/lib.rs` implementing the common `Terminal` trait, supporting cell rendering and native resize queries.

### 15.2 Windows ConPTY IPC & terminal fixes
- **ConPTY IPC tearing & line wrap fix**: Resolved cursor positioning races and line wrap artifacts under Windows ConPTY by building a differential dirty-line terminal rasterizer in `PlainTerminal`. Strips CRLF escapes, uses absolute row positioning (`\x1b[{row};1H`), appends clear-to-EOL (`\x1b[K`), and triggers clean full repaints on terminal resize.
- **WezTerm error 10054 fix**: Fixed the `wezterm_mux_server_impl::local > encoding PDU to client: writing pdu data buffer: An existing connection was forcibly closed by the remote host. (os error 10054)` error. Identified that the CLI wrapper `wezterm.exe` detached its client socket early; resolved by directly targeting `wezterm-gui.exe start --always-new-process --cwd "%~dp0"`.

### 15.3 Framerate decoupling & uncapped performance engine
- **Decoupled game loop**: Decoupled client visual rendering and animation from the server's fixed 20 Hz simulation clock.
- **CLI flag `--fps`**: Added `--fps <TARGET | uncapped | max>` flag. Configured argument parsing with `rposition` so command-line overrides take precedence over script defaults.
- **Sub-millisecond pacer**: Implemented high-precision frame pacing combining coarse `event::poll` sleeping with tight yield/spin on Windows. Passing `0` or `uncapped` disables sleep entirely for benchmarking maximum rendering throughput.
- **Performance HUD overlay**: Added in-game real-time metrics display: `[<FPS> FPS (<target>) | draw: <ms>]`.
- **Uncapped default for testing**: Configured all launcher batch scripts (`play_ansi.bat`, `play_notcurses.bat`, `play_wezterm.bat`, `play_wezterm_notcurses.bat`) to launch uncapped by default.

### 15.4 Dark fantasy & 3D aesthetic overhaul (Skyrim / Elden Ring style)
- **Articulated knight character model (`assets/ascii/player_warrior.txt`)**: Upgraded player model from a flat 4-line sprite to an articulated 6-row knight featuring a horned greathelm (`/▲\`), polished plate pauldrons (`/▓▓▓\`), breastplate emblem (`<|()|>`), chainmail fauld (`|888|\`), greaves, and a 3D ground drop shadow (`( ___ )`).
- **Layered 3D environmental props**:
  - `assets/ascii/tree.txt`: Layered pine needle canopies with ground drop shadows.
  - `assets/ascii/ruin.txt`: Ancient stone pillars with foundation drop shadows (`( _____ )`).
- **24-bit TrueColor palette**: Added 18 curated color tokens in `client/src/renderer.rs`: `KnightSteel`, `KnightGold`, `KnightCape`, `DropShadow`, `CobbleRoad`, `CobbleGlow`, `CobbleBorder`, `GrassMeadow`, `GrassTorch`, `PineTree`, `TreeShadow`, `TorchFlame`, `TorchGlow`, `Lightning`, `LightningWhite`, `BannerRed`, `WoodPost`, `FloatingDamage`.
- **Dynamic 3D radial lighting**: Paved winding cobblestone highways with stone curbs; torches and lanterns cast warm radial halos that blend smoothly into deep midnight negative space. Added hanging crimson battle banners and timber posts.
- **Combat visual effects**: Vertical electric cyan lightning bolts striking down with white cores, radial fiery ember bursts around targets, and popping floating damage numbers (`91`, `CRIT`).

### 15.5 Launch runner automation
- `play_ansi.bat`: Launches MSVC optimized release build with `--fps uncapped`.
- `play_notcurses.bat`: Sets MSYS2 ucrt64 runtime paths and launches GNU Notcurses release binary with `--notcurses --fps uncapped`.
- `play_wezterm.bat`: Launches WezTerm GUI directly with new process isolation running `play_ansi.bat`.
- `play_wezterm_notcurses.bat`: Launches WezTerm GUI directly running `play_notcurses.bat`.

### 15.6 CI matrix & verification status
- **Unit test suite**: All 31 unit tests across workspace crates passing (`cargo test --workspace`), including weather shader overlay and distinct mode tests.
- **RON content validator**: Clean validation of all 11 definitions (`cargo run -p ascii-mmorpg-tools -- content`).
- **Formatting & lints**: Formatted according to standard style (`cargo fmt --all -- --check`) and clean Clippy build with all warnings denied (`cargo clippy --workspace --all-targets -- -D warnings`).
- **GitHub Actions CI**: Matrix workflow passing green on both `ubuntu-latest` and `windows-latest`.

### 15.7 Atmospheric weather shader overlay layer (Rain, Snowfall, and Storm)
- **Top-level shader overlay pass**: Elevated atmospheric weather from baked ground backdrop cells into a dedicated post-processing overlay layer (`apply_weather_shader`). Running above the scene backdrop, combat effects, and character models creates realistic 3D depth and parallax.
- **Multiple weather shader modes (`WeatherMode`)**:
  - `WeatherMode::Rain`: Calm, multi-layer angled streaks (`/`, `·`, `|`) falling at a serene, slow atmospheric speed (`frame / 24` distant drizzle, `frame / 18` midground, `frame / 12` foreground streaks) calibrated specifically for high-framerate/uncapped smoothness without strobing, subtle wind drift, and smoothed ground impact splash ripples (`~`, `.`).
  - `WeatherMode::Snow`: Volumetric snowflakes (`*`, `•`, `·`, `°`) with fluttering horizontal sinusoidal sway (`sin(y * freq + (frame / 6) * 0.1) * amp`) and slow multi-speed descent layers (`frame / 24 .. frame / 16`).
  - `WeatherMode::Storm`: High-velocity tempest streaks (`frame / 8 .. frame / 4`) with violent wind drift and periodic electrical lightning flash cycles illuminating the atmosphere.
- **Translucent UI / anchor masking (`is_ui_or_anchor_cell`)**: Precipitation composites over empty space, terrain, props, and player plate armor while automatically protecting nameplates, health bars (`[########]`), and actor anchors (`@`, `m`).
- **Interactive controls & CLI**:
  - In-game keybinding `V` / `T`: Instantly cycles weather modes (Rain → Snowfall → Storm) during real-time gameplay with immediate HUD notification.
  - CLI flag `--weather <rain|snow|storm>`: Configures initial weather mode on startup.
### 15.8 Volumetric depth-aware ASCII models, animation & calm scene display
- **Volumetric ASCII character & monster models**:
  - **Player knight (`assets/ascii/player_warrior.txt`)**: Upgraded to an 11-row, 13-column volumetric knight with 4 authored animation frames. Features a golden crested helm (`KnightGold`), steel visor eye slit (`KnightVisor`), gleaming plate breastplate with emblem `<|()|>` / `>|<>|<` (`KnightSteel`), crimson cape trims (`KnightCape`), and ground drop foundation shadow (`DropShadow`).
  - **Monster mossling (`assets/ascii/monster_mossling.txt`)**: Upgraded to a 10-row volumetric organic beast with 4 animation frames. Features glowing amber eyes (`MossEye`), textured organic moss mass (`MossBody`), shadowed core hide (`MossHide`), and living tendrils (`Moss`).
  - **NPC herbalist (`assets/ascii/npc_herbalist.txt`)**: Upgraded to a 7-row model with 4 animation frames (idle breathing, blinking eyes, linen robe shift).
  - **Environmental props**:
    - `tree.txt`: 9-row layered pine trees with 2-frame wind canopy sway (`PineTree`, `PineCanopy`, `WoodPost`).
    - `ruin.txt`: 7-row ancient stone pillars and archways with weathered highlights (`StoneLight`), deep shadow crevices (`StoneDark`), stone walls (`RuinWall`), and window arch openings (`RuinWindow`).
    - `gate.txt`: 10-row fortified gatehouse with arched portcullis and timber supports.
- **Per-glyph depth-aware shading pipelines**:
  - Replaced monolithic entity colors with per-glyph material shading: `draw_player_knight`, `draw_monster_mossling`, `draw_npc_herbalist`, `draw_tree_prop`, `draw_ruin_prop`, and `draw_gate_prop`.
  - Preserved actor identity anchors (`@`, `m`, `h`) with their ANSI color codes for HUD and camera tracking compatibility while rendering surrounding body mass with full 24-bit TrueColor depth gradients.
- **Calm, less chaotic, atmospheric scene composition**:
  - **Serene negative space**: Eliminated high-contrast character noise (`*`, `#`, `%`, `&`) from the open terrain. Replaced with deep midnight void (`\x1b[38;2;12;15;22m`) and subtle, sparse ground dither (`·`, `.`, ` `) using `GrassDark` and `Moss`.
  - **Winding cobblestone highway**: Softened highway curbs and road tiles (`o`, `·`, `Q`, `.`) illuminated by warm amber radiant halos from timber lantern posts (`TorchFlame`, `TorchGlow`).
  - **Gentle weather overlay pacing**: Calibrated rain and snow descent rates (`frame / 24` to `frame / 12`) to maintain smooth, tranquil ambiance even under high-framerate/uncapped rendering.

### 15.9 Reference-parity graphics pass (light field, camera, sampler, effects)
- **Skeletal undead assets**: workspace-authored `skeleton.txt` / `skeleton_warrior.txt` (4 frames each, bone palette `BoneLight`/`Bone`/`BoneDark`/`BoneEye`, steel shield and blade), selected by monster label; online ASCII collections researched but not copied (per-work licensing unclear).
- **Line-based frame splitting**: `client/src/assets.rs` splits on separator *lines*, so art may contain runs like `|---|`; also fixes miscounted `frame_count` for such art.
- **Light field**: player-carried + lantern point lights with radial falloff replace Manhattan tint bands (`light_level`, directional bias); backdrop bands and ground texture thin with darkness.
- **Oblique camera + scattered props**: one shared projection for all layers; trees/ruins placed on a deterministic world grid instead of fixed coords, with a world-lock test.
- **Depth veil**: far terrain/prop inks sink toward the void (`dim_ink`, two tiers) while actor, light, weather, and UI inks stay bright — foreground/background channels.
- **Effects**: persistent spell beam removed (event-only strike flash + embers + impact `+`); damage numbers parsed from the real message figure instead of the `"91"` placeholder.
- **Nameplates**: greedy nearest-free (label, bar) row slots so overlapping actors keep readable stacked labels.
- **Metrics**: near-black fraction test (≥70%, measured 3.7% at baseline), still-air shimmer test (only weather/actor cells may change), `ASCII_DUMP=1` snapshot dumps for visual review against `data/videos/`.

## 16. Depth awareness & animation techniques in terminal ASCII engines

### 16.1 Depth awareness mechanisms
1. **Luminance & normal-based glyph density ramps**:
   - In 3D graphics, lighting is determined by the Lambertian cosine law ($I = \vec{N} \cdot \vec{L}$). In ASCII, brightness levels are mapped to visual character density:
     `"  .,:;+*#%@"` or `·-=≡8#@▓`.
   - Characters facing the light source (specular/diffuse) use high-density, sharp glyphs (`▓`, `8`, `@`, `Q`), while surfaces angled away or in shadow use low-density, open glyphs (`:`, `·`, `.`).
2. **Aerial perspective & atmospheric depth (Z-distance falloff)**:
   - Contrast and saturation decrease with distance from the camera. Distant terrain and props use darker, lower-contrast inks (`StoneDark`, `GrassDark`, `Background`), while foreground actors and illuminated focal points use high-contrast, radiant TrueColor tones (`KnightSteel`, `TorchFlame`).
3. **Drop shadows & contact occlusion**:
   - Anchoring characters to the ground plane requires an elliptical contact shadow (`( _____ )`) directly beneath their base using dark drop-shadow ink (`\x1b[38;2;16;20;28m`). This immediately establishes spatial grounding and depth separation from the backdrop.
4. **Z-order layer compositing**:
   - The scene is rendered in strict depth order:
     `Backdrop terrain` → `Drop shadows` → `Static props (trees, ruins)` → `Entities (monsters, NPCs, player)` → `Combat VFX & lightning` → `Weather shader overlay (rain, snow)` → `UI HUD & status labels`.

### 16.2 Terminal ASCII animation techniques
1. **Multi-frame keyframe cycling**:
   - Splitting authored frames with `---` enables deterministic tick/frame selection (`frame / phase % frame_count`). Keyframes capture subtle secondary motion: chest expansion during breathing, leg weight shifts, eye blinks (`(^^)` → `(..)`), and fabric swaying.
2. **Sinusoidal procedural vertex/column deformation**:
   - For fluid natural phenomena (wind swaying trees, banners fluttering, snow drifting), procedural trigonometric offsets are applied:
     $$x_{\text{offset}} = \sin\left(y \cdot \omega + \frac{\text{frame}}{T}\right) \cdot A$$
3. **Sub-glyph phase shifting**:
   - Rather than displacing whole character coordinates (which can look jarring on a discrete terminal grid), motion can be conveyed by cycling glyph shapes in place: alternating between directional strokes (`/` → `|` → `\` → `|`), creating the visual impression of continuous rotation or waving without jitter.
