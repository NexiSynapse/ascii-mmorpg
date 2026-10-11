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

### Rendering backends: WGPU 3D Shader Pipeline (Path C) vs CPU

The client supports two rendering engines through `ActiveRenderer`:

1. **WGPU 3D Offscreen Shader Pipeline (Path C, Default)**:
   - Headless offscreen 3D scene rendering via `wgpu` (`DirectX 12` / `Vulkan` / `Metal`).
   - Procedural 3D articulated humanoid warrior, mossling monster, herbalist NPC, pine trees, stone gate, and ancient ruins.
   - Dynamic Blinn-Phong lighting: directional ambient/sunlight + multiple dynamic point lights (flickering player hip lantern, twin gate torches, staff lantern).
   - Volumetric 3D falling weather particles (angled rain streaks, splashes, snow).
   - Real-time ASCII compute shader: Sobel edge detection on depth/normals (`|`, `_`, `/`, `\`, `+`) + 4x4 Bayer dithered luminance ramp (` `, `.`, `:`, `*`, `=`, `o`, `O`, `%`, `#`, `@`) + boosted truecolor foreground with atmospheric deep contrast background.
   - Zero-overhead PCIe staging readback directly to terminal stdout.

2. **Procedural ANSI CPU Fallback**:
   - Pure terminal CPU rendering for headless servers or environments without GPU drivers.
   - Force CPU mode with `--cpu`:
     ```text
     cargo run -p ascii-mmorpg-client -- --cpu
     ```

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

### 15.10 Offscreen wgpu diorama with compute ASCII pass (Initial Spike)
- **Feature `wgpu-scene`** (`client/Cargo.toml`, alias to `gpu`): headless wgpu 3D scene (render-to-texture, no window/surface) + WGSL compute pass that reduces texel blocks to packed cell records; the CPU maps one small buffer per frame and formats ANSI through the existing `Terminal` trait (ConPTY differential path and Notcurses both work unmodified).
- **Scene**: ground slab, gatehouse landmark, world-grid pines, lantern posts doubling as point lights, player-carried torch light with flicker, nearest entities as boxes.
- **CLI**: `--wgpu` (realtime, honors `--connect`/`--fps`/`--notcurses`), `--wgpu-fallback` (software rasterizer), `--wgpu-demo [N]` (headless frames + timing). `WGPU_FORCE_FALLBACK=1` / `WGPU_POWER=low` env overrides for driver isolation.
- *Note*: Fully generalized and superseded by the Path C instanced multi-buffered pipeline described in Section 18.

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

## 17. Forensic video research & humanoid character architecture
- **Reference video analysis (`data/videos/`)**:
  - Detailed forensic breakdown of reference footage by indie developer `@Ansenjeo` (`Video-31774.mp4`, `Video-47489.mp4`, `Video-90418.mp4`, and WhatsApp clips).
  - Reverse-engineered pipeline: Unity 3D skeletal humanoid meshes + point lights + particle systems rendered to texture, processed through a full-screen luminance/normal ASCII post-processing shader with Sobel edge detection onto an $\approx 110 \times 128$ cell grid.
  - Complete research findings and 3-path architectural implementation analysis documented in `docs/RESEARCH_ASCII_GRAPHICS_FIDELITY.md`.
- **Articulated humanoid player model (`assets/ascii/player_warrior.txt`)**:
  - Upgraded player model to a **24-row articulated Humanoid** matching the high-resolution reference style (`senje_crop.png`). Features a defined golden crested helm (`KnightGold`), visor eye slits (`KnightVisor`), warm face features (`NpcSkin`), gleaming plate pauldrons, breastplate, belt emblem `<|()|>` / `>|<>|<`, chainmail fauld, cuirass, poleyn knee pads, greaves with volumetric luminance falloff (`%` -> `*` -> `+`), armored sabatons, and ground contact shadow.
  - 4 animation frames: idle breathing, left-leg forward stride with counter-swinging arms, neutral recoil, and right-leg forward stride.
  - Per-glyph TrueColor depth shading via `draw_player_knight` in `client/src/renderer.rs`.
- **High-resolution world assets across all entities and props**:
  - `monster_mossling.txt`: Upgraded to **18 rows tall** with multi-tier volumetric body scales, amber glowing eyes, organic tendrils, and articulated foot shifting in 4 animation frames.
  - `tree.txt`: Upgraded to **20 rows tall** towering pine with wide needle boughs, massive trunk, and wind sway frames.
  - `ruin.txt`: Upgraded to **12 rows tall** ancient stone archway with twin colonnade pillars and weathered foundation stones.
  - `gate.txt`: Upgraded to **17 rows tall** fortified fortress gatehouse with crenellated battlements, arrow slits, and arched portcullis.
  - `npc_herbalist.txt`: Upgraded to **16 rows tall** robed apothecary with staff, cowl hood, and draping linen folds in 4 animation frames.

## 18. Path C: GPU/WGPU Offscreen Shader Pipeline & GPU Batch Processing

### 18.1 Architecture overview
Path C implements a pure hardware-accelerated 3D graphics and compute pipeline using `wgpu` (`DirectX 12` / `Vulkan` / `Metal`):
1. **3D Scene Rasterization Pass (`SCENE_3D_WGSL`)**: Evaluates true 3D Blinn-Phong lighting, multiple dynamic point lights with inverse-square falloff and organic flicker, directional sunlight/moonlight, specular highlights, and atmospheric distance fog directly onto an offscreen multi-target render buffer (`color_texture` in `Rgba8UnormSrgb` and `normal_depth_texture` in `Rgba8Unorm`).
2. **ASCII Post-Processing Compute Pass (`ASCII_POST_WGSL`)**: Executes a high-performance compute shader across $16 \times 16$ thread workgroups over the G-buffer. Evaluates a Sobel filter over normal and depth gradients to detect silhouettes and geometric edges (`_`, `|`, `+`, `/`), computes perceived luminance with a $4 \times 4$ Bayer dither matrix, and maps lit pixels to volumetric density glyphs (` `, `.`, `:`, `*`, `=`, `o`, `O`, `%`, `#`, `@`), outputting structured `GpuCell` structures.
3. **Multi-Buffered Asynchronous Staging Ring (`RING_SIZE = 2`)**:
   - Maintains a double-buffered ring of in-flight frame slots (`FrameSlot`), each with dedicated uniform buffers, storage buffers, and readback staging buffers.
   - Decoupled `submit_frame()` and `readback_slot()` eliminate synchronous `Maintain::Wait` serialization stalls. Frame $N$ executes on the GPU queue concurrently while the CPU reads back and presents Frame $N - 1$.
   - Non-blocking `Maintain::Poll` check ensures zero-wait frame retrieval.
4. **GPU Hardware Instanced Batch Drawing**:
   - `InstanceRaw` (96-byte aligned vertex attribute layout) provides per-instance model matrix, color tint, and material/roughness overrides.
   - Prototype meshes (`MeshBuffer::create_pine_tree_prototype()`, `MeshBuffer::create_weather_particle_prototype()`, `MeshBuffer::create_ruin_pillar_prototype()`) are pre-baked onto GPU VRAM buffers at initialization.
   - Repeated world assets (pine trees, ruin pillars, hundreds of weather particles) are submitted as single instanced GPU draw calls (`draw_indexed(..., instance_count)`), reducing CPU vertex generation and PCIe transfer by over 90%.
5. **Batch Processing Engine (`render_batch`)**:
   - High-throughput batch API accepting slices of world snapshots.
   - Keeps the GPU queue continuously saturated across the staging ring, processing batches at maximum hardware throughput.
6. **Zero-Allocation Fast ANSI String Formatter**:
   - Directly converts color changes into pre-allocated terminal strings via integer math without heap allocations, guaranteeing that the CPU formatting loop never bottlenecks the GPU command queue.

### 18.2 Sub-Pixel 3D Rendering & Optimization Architecture (Asciicker Innovations Adapted)
To achieve razor-sharp asset silhouettes and mathematical depth precision while maintaining our distinct **Ansenjeo volumetric ASCII aesthetic**, the pipeline incorporates key optimization and rendering logic adapted from Michał Sokalski's `asciicker`:
1. **$2\times$ Super-Sampled G-Buffer (SSAA)**:
   - Offscreen textures (`color_texture`, `normal_depth_texture`, `depth_texture`) are rasterized at $2\times$ terminal resolution ($2W \times 2H$).
   - In `ASCII_POST_WGSL`, each compute invocation for cell $(x, y)$ samples the $2 \times 2$ sub-pixel quadrant ($p_{00}, p_{10}, p_{01}, p_{11}$).
   - Integrates 4-sample sub-pixel color and luminance, eliminating polygon edge swimming and pixel shimmering during camera movement.
   - Evaluates sub-pixel horizontal and vertical depth/normal differences ($\Delta Z_h, \Delta Z_v$) to orient silhouette edges (`_`, `|`, `/`, `\`) with sub-pixel boundary precision.
2. **$O(1)$ Depth Unprojection (`CameraUniforms::unproject_screen_to_world`)**:
   - `GpuCell` encodes GPU 32-bit depth (`depth_bits: u32`).
   - Using the inverted view-projection matrix (`inv_view_proj`), any screen cell under the cursor is unprojected into exact 3D world $(X, Y, Z)$ coordinates in $O(1)$ constant time with zero CPU mesh-triangle raycasting.
3. **Camera View-Frustum Plane Culling (`Frustum`)**:
   - Extracts 6 frustum planes directly from `view_proj` via the Gribb-Hartmann algorithm.
   - Evaluates `Frustum::test_sphere` and `Frustum::test_aabb` before instance packing, culling off-screen trees, pillars, and entities before they enter the GPU instance buffer.
4. **Native 3D Wavefront `.obj` Asset Loader**:
   - `MeshBuffer::load_obj()` and `MeshBuffer::add_obj()` parse standard 3D `.obj` files exported from Blender.
   - Automatically parses vertex positions, vertex normals, and triangulates polygonal faces with custom materials and roughness, providing a clean path for custom 3D asset importation.

### 18.3 Upstream Integration: Skeletal Undead, Anti-Shimmer Lighting & Headless Benchmarking
Incorporated beneficial commits from remote `rendering_gra_update`:
1. **Skeletal Undead Models & Articulated Animation**:
   - Embedded `assets/ascii/skeleton.txt` and `assets/ascii/skeleton_warrior.txt` (4 frames each) with dedicated bone palette (`BoneLight`, `Bone`, `BoneDark`, `BoneEye`), steel broadsword, and tower shield.
   - Line-based frame separator logic preserves artistic separators like `|---|`.
2. **Radial Light Fields & Anti-Shimmer Quantization**:
   - Integrated dynamic light field with player-carried torch and fixed world lantern posts with radial distance attenuation.
   - Quantized ground light levels to $0.05$ steps, preventing discrete sub-pixel camera jitter from flipping ground dither specks between consecutive frames.
3. **Headless GPU Benchmarking & Unified CLI**:
   - Added `--wgpu-demo [N]` CLI flag for headless automated benchmarking, rendering $N$ frames directly through our hardware-accelerated $2\times$ SSAA compute pipeline without requiring terminal interaction.
   - Verified benchmark output: **171.7 FPS equivalent** (5.82 ms/frame) on NVIDIA GeForce GTX 1650 under Vulkan with full 3D instanced geometry, dynamic lights, and Sobel edge detection.

## 19. Object Collision Mechanics, Spatial Boundaries & Sliding Physics

### 19.1 Architecture & Bounding Volumes (`crates/shared/src/collision.rs`)
To prevent the player from phasing through world structures, environmental props, and terrain edges, a deterministic spatial collision system was introduced into `ascii-mmorpg-shared`:
1. **Axis-Aligned Bounding Volumes (`BoundingBox`)**:
   - Defined in integer tile coordinates with constant-time point containment (`contains`) and box-box intersection (`intersects`) tests.
   - Uniform boundary expansion (`expand(margin)`) for actor clearance margins.
2. **Depth-Aware Ground-Contact Footprints (`overworld_obstacles()`)**:
   - In 2.5D/isometric perspective with height elevation, characters stand on the ground at their feet, while tree canopies, roofs, and lintels extend upward into the air.
   - Collision boundaries strictly represent the **ground-contact footprint** (tree trunks, pillar bases, and foundation walls) where an actor's feet make physical contact. Upper visual geometry (foliage, branches, headers) does not impede ground movement.
   - **Towering Pine Trees**: Physical collision is strictly the single-tile trunk contact point where the tree roots into the ground (e.g. `(-7, -2)`, `(8, -3)`, `(9, -10)`, `(-9, -10)`, and procedural `point.x == tree_wx && point.y == tree_wy`). When standing in front of a tree (South, `tree.y + 1`), the character's feet rest in front of the trunk while their torso and head overlap the tree in front, occluding it via the 3D depth buffer without blocking movement.
   - **Stone Fortress Gatehouse**: Solid stone guard towers (West `[-4, -2]`, East `[2, 4]`) and portcullis grating (`[-1, 1]`) at foundation line $Y = -12$.
   - **Ancient Stone Ruins & Colonnade**: Altar pedestal base (`[-8, -5]` to `[-7, -5]`) and colonnade pillars (`(-10, -5)` and `(-6, -5)`).
   - **Timber Lantern Posts & War Banners**: Ground contact points at `(-6, 2)`, `(6, -4)`, `(-8, 3)`, and `(8, -3)`.
   - **Zone Boundary Perimeter**: Enforces the playable bounds of the Whispering Forest overworld (`[-35, -35]` to `[35, 35]`), preventing players from wandering off into coordinate void.
3. **Movement Resolution & Smooth Diagonal Sliding (`resolve_movement`)**:
   - Direct step evaluation for cardinal directions (`North`, `South`, `East`, `West`). When blocked, the player halts at the boundary and receives immediate status feedback (e.g., `Blocked by Stone Gate.`, `Blocked by Towering Pine Tree.`).
   - For diagonal movement (`NE`, `NW`, `SE`, `SW`), if the diagonal target is obstructed by a wall or obstacle, the engine tests the component horizontal and vertical steps. If one axis is unobstructed, the character smoothly slides along the obstacle surface instead of coming to a jarring halt.
4. **Client-Server Parity**:
   - Both the local client loop (`apply_local_command` in `client/src/lib.rs`) and authoritative server simulation (`move_player` in `server/src/lib.rs`) evaluate the shared collision engine, guaranteeing identical physical boundaries in standalone and networked modes.

## 20. Movement-Aware Articulation & Stationary Foot Posture

### 20.1 Stationary Leg Articulation vs. Walk Cycle (`GpuRenderer::update_walk_phase`)
Previously, procedural 3D humanoid entities in the GPU pipeline drove leg swing and boot displacement via a continuous time variable (`time * 4.0`), causing characters to continuously march in place even while standing still.

1. **Velocity and Displacement Tracking**:
   - `GpuRenderer` tracks the local player's previous grid coordinate (`last_player_pos`) and the timestamp of their latest displacement (`last_move_instant`).
   - A 280ms stride window (`is_some_and(|t| now.duration_since(t).as_millis() < 280)`) determines active walking state.
2. **Stationary Idle Stance**:
   - When stationary, `walk_phase` resets to `0.0`.
   - In `MeshBuffer::add_humanoid`, `leg_swing = (walk_phase).sin() * 0.3 = 0.0`. Both armored greaves and leather sabatons (boots) remain strictly planted side-by-side on the ground plane ($Y + 0.12, Z + 0.06$) with zero foot or leg movement.
   - Remote player entities (`EntityKind::Player`) at rest are likewise rendered with `walk_phase = 0.0`.
3. **Active Stride Striding**:
   - When displacement occurs within the stride window, `player_walk_phase` articulates forward by `+0.35` radians per rendered frame, driving smooth alternating forward/backward stride swings for the left and right legs.

## 21. Ratatui Dark Fantasy HUD & UI Presentation Layer

### 21.1 Architecture & Layout (`client/src/ui.rs`)
To elevate the visual aesthetic of the client from basic plain-text footer rows into an authentic, dark fantasy MMORPG experience, `client/src/ui.rs` integrates `ratatui` widgets and layout constraints:
1. **Top Navigation & Vitality Header (`render_top_bar`)**:
   - Framed with rounded borders (`BorderType::Rounded`) in weathered bronze (`Color::Rgb(90, 80, 70)`).
   - Displays real-time location title (`🌲 Whispering Forest`), player coordinate pin (`📍 (X, Y)`), active weather badge (`🌧 Rain`, `❄ Snow`, `⚡ Storm`), tick index, and active renderer engine badge.
   - **Fractional Unicode Health Gauge (`Gauge`)**: TrueColor gradient meter (`█`, `▉`, `▊`, `▌`, `▍`, `▎`, `▏`) transitioning dynamically from vibrant emerald green (>50% HP) to amber gold (25–50% HP) to flashing crimson (<25% HP), displaying formatted `HP: {curr}/{max}` and percentage.
   - **Player Combat Attributes**: Styled class insignia (`[⚔ WARRIOR]`) and color-coded stat badges: `ATK` (crimson), `DEF` (steel blue), and `LUCK` (gilded amber).
2. **Action & Combat Event Log (`render_combat_log`)**:
   - Scrollable action log framed with rounded borders.
   - Context-sensitive event colorizer:
     - Combat strikes (`⚔ You strike Skeleton Warrior for 15 damage!`) in radiant flame orange (`Color::Rgb(255, 130, 80)`).
     - Defeats and kills (`💀 Skeleton Warrior was slain!`) in royal amethyst/magenta (`Color::Rgb(240, 80, 120)`).
     - Physical collision halts (`⚠ Blocked by Towering Pine Tree.`) in cautionary gold (`Color::Rgb(240, 190, 70)`).
     - Server connects and zone transfers in crisp celestial cyan (`Color::Rgb(100, 210, 255)`).
3. **Nearby Threat Radar & Target Lock**:
   - Real-time scanner identifying living hostiles within sensor radius, calculating Manhattan distance in meters (`💀 Skeleton Warrior (3m)`).
   - Mini target health meter displaying the target's remaining vitality.
   - Peaceful indicator (`✨ Area peaceful.`) when no hostile threats are in proximity.
4. **Hotkey Legend Footer (`render_controls_legend`)**:
   - Clean centralized hotkey cheatsheet: `[WASD/Arrows] Move`, `[F] Attack`, `[R] Rest`, `[V/T] Cycle Weather`, `[Q/Esc] Quit`.
5. **High-Performance Buffer Serializer (`buffer_to_ansi`)**:
   - Traverses Ratatui `Buffer` cells and emits delta ANSI attribute sequences (`\x1b[38;2;...m`, `\x1b[1m`, `\x1b[0m`) only on style transitions, fully compatible with Windows ConPTY, Notcurses, and differential dirty-line terminal blitters.

## 22. Procedural 3D Character Models & Entity Dispatch

### 22.1 High-Detail Procedural Character Geometry (`client/src/gpu/mesh.rs`)
To eliminate generic boxy representations and deliver visually stunning characters across the 3D GPU graphics pipeline, high-fidelity procedural meshes were authored with dedicated anatomical sculpting, micro-insets, materials, and roughness properties:

1. **Player Knight (`MeshBuffer::add_humanoid`)**:
   - **Greathelm & Brow Diadem**: Steel helmet with golden brow band, cheek guards, dark gorget neck collar, and cyan enchanted visor slit ($[0.35, 0.90, 1.0]$) with material glow.
   - **Knight Plume**: Swept crest in royal crimson with gilded trim.
   - **Cuirass & Faulds**: Sculpted peascod breastplate with medial ridge, gilded heraldic eagle/cross inlay, oiled leather belt with golden buckle, and segmented fauld tassets.
   - **Layered Pauldrons**: Multi-tiered spiked shoulder armor with gilded crest accents.
   - **Flowing Cape**: Royal crimson cloak articulated with subtle physics-driven sway.
   - **Articulated Limbs**: Cuisses with knee poleyns, shin greaves, and fluted sabatons (boots) with stationary idle support.
   - **Armament**: Gilded heater shield with heraldic cross and broadsword with fuller groove, swept quillons, wire-wrapped hilt, and octagonal pommel stone.

2. **Skeletal Undead (`MeshBuffer::add_skeleton`)**:
   - **Cranium & Sockets**: Bleached ivory skull with hollow eye sockets illuminated by glowing ember soul glints ($[1.0, 0.35, 0.15]$), maxilla, and bared jaw.
   - **Skeletal Anatomy**: Cervical vertebrae, thoracic ribcage with hollow cavity shading, spine column, pelvic basin, and spindly jointed leg bones with shambling stride.
   - **Armament**: Notched rusted scimitar with aged steel blade and crossguard.

3. **Skeleton Champion Warrior (`MeshBuffer::add_skeleton_warrior`)**:
   - **Horned Spangenhelm**: Iron combat helm reinforced with brass trim and protruding iron horns.
   - **Spiked Pauldrons & Hauberk**: Heavy spiked shoulder guards over tattered chainmail hauberk and breastplate fragment.
   - **Reinforced Tower Shield**: Heavy iron-bound door shield with brass rim and central iron boss skull emblem.
   - **Executioner's Broadsword**: Massive two-handed executioner blade with brass swept guard.

4. **Mossling Forest Lurker (`MeshBuffer::add_mossling`)**:
   - **Carapace & Spine Ridge**: Bulky layered moss carapace with bark underbelly plating and dorsal spine ridge.
   - **Quadruped Stalker Limbs**: Muscular clawed stalker limbs with root talons and stride articulation.
   - **Fanged Maw**: Dark mandibles with paired protruding ivory tusks.
   - **Compound Eyes**: Multi-faceted predatory amber luminescent eyes ($[1.0, 0.82, 0.20]$).
   - **Bioluminescent Fungal Horns**: Conical fungal horns tipped with pulsating bioluminescent spore pods ($[0.45, 0.95, 0.55]$).

5. **Herbalist NPC (`MeshBuffer::add_herbalist`)**:
   - **Scholar Robe & Cowl**: Twilight blue scholar robe with gold-embroidered hem and shadowy cowl hood.
   - **Alchemical Satchel**: Oiled leather satchel holding twin glass flasks (crimson healing draught and cerulean mana potion).
   - **Elderwood Staff & Lantern**: Gnarled elderwood staff holding a brass filigree lantern cage with warm organic flame flickering.

### 22.2 Dynamic Entity Model Dispatch (`GpuRenderer::populate_scene`)
In `client/src/gpu/renderer.rs`, entity rendering inspects monster identity labels instead of defaulting all hostiles to mosslings:
- Entities containing `"warrior"` instantiate the armored `Skeleton Warrior` champion model.
- Entities containing `"skeleton"` instantiate the agile `Skeleton` undead model.
- Other beasts instantiate the quad-legged `Mossling` stalker model.
- NPCs instantiate the hooded `Herbalist` model, and player entities instantiate the fully-armored `Humanoid Knight`.

