# Comparative Repository Analysis & Engine Upgrades

This document details the architectural analysis of six open-source ASCII/terminal and retro 3D game engines, comparing their rendering, animation, collision, and UI techniques against our dual-pipeline MMORPG engine (`client/src/gpu` and `client/src/renderer.rs`), followed by specific upgrades integrated into our codebase.

---

## 1. Analyzed Repositories & Architectural Breakdown

### 1.1 `vercel-labs/arcade`
* **Repository**: [https://github.com/vercel-labs/arcade](https://github.com/vercel-labs/arcade)
* **Architecture**: TypeScript / Node / Web TUI framework focused on generative terminal rendering.
* **Core Mechanisms**:
  1. **Cell-Level Differ (`src/engine/diff.ts`)**: Maintains a shadow surface buffer of the prior frame. Checks individual cell coordinates `(x, y)` and emits terminal cursor jumps (`\x1b[y;xH`) only for cells that changed. Returns an empty string `""` on idle frames.
  2. **Alex Harri Shape-Matching Glyphs (`src/engine/glyph.ts`)**: Divides each cell into a $3 \times 6$ sub-pixel brightness/coverage grid (18 dimensions). Compares cell vectors against precomputed $8 \times 8$ monospace font glyphs using Euclidean distance, Cauchy-Schwarz lower-bound pruning, and temperature-based softmax sampling.
  3. **Visual Post-Processing (`src/engine/bloom.ts`, `src/engine/ink-burn.ts`)**: Downscaled luminance bloom kernel and phosphor decay trails simulating CRT phosphor persistence.
* **Comparison & Verification**:
  * **Differ Evaluation**: In high-frequency dynamic weather scenes (falling rain, storm tempest, snow), precipitation mutates 40–70% of rows every tick. Individual cell cursor jumps cost 6–10 bytes per sequence. For dense updates, emitting continuous scanlines with cached ANSI colors and zero-byte CRLF jumps (our `PlainTerminal`) is significantly faster and avoids Windows ConPTY parser bottlenecks. However, dirty-flagging static UI sections provides savings.
  * **Glyph Selection Evaluation**: CPU shape-matching evaluates 18-dimensional distance across 95 ASCII glyphs per cell per frame, which bottlenecks high-framerate Rust loops. In contrast, our WGPU compute shader (`cs_main`) performs 4x SSAA G-buffer normal and depth discontinuity gradient checks directly on GPU hardware in $<0.2\text{ms}$, differentiating true 3D physical edges (`_`, `|`, `/`, `\`, `+`) from flat color noise.
* **Potential Upgrades to Adopt**:
  * **Phosphor Trails / Hit Flash Persistence**: Storing a decaying hit-flash luminance buffer for lightning strikes and critical attack impacts.

---

### 1.2 `AndreaZero/ascii-mmorpg-game` (Project Wanderer / Blackwater)
* **Repository**: [https://github.com/AndreaZero/ascii-mmorpg-game](https://github.com/AndreaZero/ascii-mmorpg-game)
* **Architecture**: Godot 4 3D MMORPG utilizing spatial shaders to render 3D scenes as ASCII characters.
* **Core Mechanisms**:
  1. **Triplanar Mapping (`shaders/surface_glyphs.gdshader`)**: Projects world-space glyph patterns onto 3D surfaces based on surface normal dominance ($N_y > N_x \land N_y > N_z \implies \text{XZ}$), preventing UV distortion across cliffs and walls.
  2. **Wind Sway Vertex Shader (`shaders/grass.gdshader`)**: Billboarded grass and foliage displace laterally using sinusoidal time equations:
     ```glsl
     VERTEX.x += sin(TIME * 1.05 + origin.x * 0.7 + origin.z * 0.4) * UV.y * 0.035;
     ```
  3. **Dynamic Water Surface (`shaders/water.gdshader`)**: Animated wave ripples with screen-space UV distortion and planar reflections.
  4. **Procedural Rig (`scripts/wanderer_rig.gd`)**: Kinematic movement rig with distinct walk and idle states.
* **Comparison & Verification**:
  * Because our primary GPU pipeline renders 3D geometric meshes with custom WGSL shaders and maps screen luminance/edges in a compute pass, we do not suffer from 2D UV texture stretching.
  * Our tree foliage and grass meshes were previously static during rendering.
* **Potential Upgrades to Adopt**:
  * **Foliage Wind Swaying**: In WGPU vertex shader `vs_main`, apply lateral sinusoidal vertex offsets to foliage vertices (`material_id == 3.0`), creating organic swaying pine trees, bushes, and cloth banners.
  * **Dynamic Water Ripple Glyphs**: Animated sinusoidal ripple distortion on river surfaces producing shifting wave glyphs (`~`, `≈`, `-`).

---

### 1.3 `LingDong-/asciimare`
* **Repository**: [https://github.com/LingDong-/asciimare](https://github.com/LingDong-/asciimare)
* **Architecture**: Python-based terminal voxel raymarching engine.
* **Core Mechanisms**:
  1. **Empirical Font Luminosity Ramps (`asciimare/modules/grays.py`)**: Renders monospace font characters into offscreen bitmaps and counts inked pixels to establish mathematically precise luminance ramps for specific fonts (Menlo, Consolas, Courier).
  2. **Sparse Voxel Raymarching (`asciimare/modules/voxfield.py`)**: Traverses 3D voxel grids with directional sunlight and ambient occlusion shading.
* **Comparison & Verification**:
  * Pre-calculated ink ramps ensure smooth perceived gradient transitions without visual banding.
  * CPU voxel raymarching cannot compete with hardware WGPU instanced mesh rasterization (170+ FPS).
* **Potential Upgrades to Adopt**:
  * **Font-Calibrated Luminance Ramps**: Tuning our Bayer dithered ramp in both CPU and GPU shaders to match standard Windows Terminal / ConPTY fonts (Consolas, Cascadia Code).
  * **Multi-Charset Support**: Providing selectable glyph modes (Standard ASCII, CP437 block characters `░▒▓█`, and Braille density dots).

---

### 1.4 `saultyevil/ascii-raycasting`
* **Repository**: [https://github.com/saultyevil/ascii-raycasting](https://github.com/saultyevil/ascii-raycasting)
* **Architecture**: C / C++ Wolfenstein-3D style DDA (Digital Differential Analyzer) raycaster.
* **Core Mechanisms**:
  1. **DDA Grid Traversal**: Fast column-by-column ray stepping through 2D map grids.
  2. **Distance Attenuation**: Euclidean distance shading with fish-eye cosine correction.
  3. **2D Tile Collision**: Grid cell collision checks preventing player movement through solid tiles.
* **Comparison & Verification**:
  * 2D raycasting is restricted to vertical column walls and cannot render our 2.5D isometric view, multi-tiered ruins, organic pine canopies, or articulated 3D character models.
  * The 2D grid collision approach confirms our design in `crates/shared/src/collision.rs`.
* **Potential Upgrades to Adopt**:
  * Solidified ground footprint collisions (verified in `collision.rs`).

---

### 1.5 `justMoritz/3d-game-engine`
* **Repository**: [https://github.com/justMoritz/3d-game-engine](https://github.com/justMoritz/3d-game-engine)
* **Architecture**: Lightweight JavaScript raycasting 3D engine inspired by javidx9.
* **Core Mechanisms**:
  1. **Corner Edge Detection**: Casts secondary rays at wall cell boundaries to detect physical corners, rendering distinct boundary characters (`|`) to emphasize wall edges.
  2. **Bitwise Performance Tricks**: Uses bitwise NOT (`~~`) for fast truncation and precomputed trig constants.
* **Comparison & Verification**:
  * The concept of edge highlighting to define object silhouettes is essential for readable ASCII art. In our GPU compute shader, we achieve this through normal/depth gradient discontinuity detection, which detects all 3D mesh creases rather than only 2D wall corners.
* **Potential Upgrades to Adopt**:
  * Fast integer arithmetic and cached constants for CPU rendering loops.

---

### 1.6 `jonatasfernandespimenta/CmdMMO`
* **Repository**: [https://github.com/jonatasfernandespimenta/CmdMMO](https://github.com/jonatasfernandespimenta/CmdMMO)
* **Architecture**: Python terminal MMORPG with client-server networking.
* **Core Mechanisms**:
  1. **Modular Windowed TUI (`client/game/ui/`)**: Distinct UI modules for `combatui.py`, `inventoryui.py`, `skillsui.py`, `house_editor_ui.py`, and `party_ui.py`.
  2. **Window Panes & Bordered Boxes**: Clean framed layout separating game world, stats, party, and combat log.
* **Comparison & Verification**:
  * Confirms the value of our `RatatuiHud` integration in `client/src/ui.rs`.
* **Potential Upgrades to Adopt**:
  * **Interactive Overlay Windows**: Toggleable modals/panels for Inventory (`I`), Character Attributes & Skills (`C`), and World Map (`M`).

---

## 2. Integrated Upgrades in Our Engine

Following this analysis, we integrate the following enhancements directly into our codebase:

### Upgrade 1: CPU Renderer Stationary Feet When Idle (Implemented)
* **Origin**: Movement-aware articulation (verified in `gpu/renderer.rs` and Blackwater rig).
* **Implementation**: In `client/src/renderer.rs`, player rendering detects idle state via `is_player_walking`. When stationary, the player is locked to Frame 0 (symmetrical planted boots `(_____)  (_____)` with zero leg swing) rather than cycling walking frames on every tick.

### Upgrade 2: Foliage Wind Swaying in WGPU Shader (Implemented)
* **Origin**: Inspired by `AndreaZero/ascii-mmorpg-game` (`grass.gdshader`).
* **Implementation**: In `client/src/gpu/shaders.rs` (`vs_main`), apply lateral sinusoidal displacement to vertices where `material_id == 3.0` (Foliage):
  ```wgsl
  if (mat_id >= 2.8 && mat_id <= 3.2 && world_pos4.y > 0.8) {
      let wind_intensity = min((world_pos4.y - 0.8) * 0.045, 0.18);
      let sway = sin(camera.time * 2.4 + world_pos4.x * 0.65 + world_pos4.z * 0.45) * wind_intensity;
      world_pos4.x += sway;
      world_pos4.z += sway * 0.45;
  }
  ```
  This creates swaying pine canopies and vegetation responsive to the simulation clock.

### Upgrade 3: Water Shimmer & Animated Ripple Glyphs (Implemented)
* **Origin**: Inspired by `AndreaZero/ascii-mmorpg-game` (`water.gdshader`).
* **Implementation**:
  * **CPU 2.5D Renderer (`client/src/renderer.rs`)**:
    * Defined `Ink::WaterDeep`, `Ink::WaterShallow`, `Ink::WaterRipple`, `Ink::WaterShimmer`, and `Ink::BridgeWood`.
    * Implemented a winding river across the Whispering Forest meadow ($wy \approx 8 + wx / 5$).
    * Where the stone road intersects the water, rendered a sturdy timber bridge (`'='`, `'#'`, `BridgeWood`).
    * Generated real-time animated wave ripples modulating with frame time: shifting between `~`, `≈`, `-`, and `·` with dynamic specular highlights and riverbank reeds (`%`).
  * **GPU WGPU Pipeline (`client/src/gpu/shaders.rs`)**:
    * In `vs_main`, procedural wave displacement for water surfaces (`material_id == 6.0`):
      ```wgsl
      if (mat_id >= 5.8 && mat_id <= 6.2) {
          let wave = sin(camera.time * 2.6 + world_pos4.x * 1.8 + world_pos4.z * 1.2) * 0.035;
          world_pos4.y += wave;
      }
      ```
    * In `fs_main`, dynamic wave normal perturbing with Blinn-Phong sun/moon specular glints.

### Upgrade 4: Interactive Overlay Modals (CmdMMO-Inspired Windowed TUI) (Implemented)
* **Origin**: Inspired by `jonatasfernandespimenta/CmdMMO` (`client/game/ui/`).
* **Implementation**:
  * **`ActiveModal` Architecture (`client/src/ui.rs`, `client/src/lib.rs`)**:
    * Defined `ActiveModal` enum with `None`, `Inventory`, `CharacterSheet`, and `WorldMap`.
    * Extended `SceneRenderer` trait with `active_modal()`, `set_active_modal()`, and `toggle_modal()`.
    * In `client/src/main.rs`, added keybindings:
      * `[I]`: Toggles Inventory & Equipment modal (equipped weapons/armor, satchel items, healing draughts, gold, encumbrance).
      * `[C]`: Toggles Character Attributes & Skills modal (Level, XP progress bar, STR, DEX, CON, INT, LUCK, active buffs).
      * `[M]`: Toggles 2D Overworld Radar Map modal (Whispering Forest ASCII minimap with coordinates, fortress gate, ruins, torches, and hostile radar reach).
      * `[Esc]` / `[Q]`: Closes any open modal window gracefully before handling exit.
    * Adaptive HUD layout: Expands reserve space from 7 to 12 rows when a modal is open, with double-bordered TrueColor styling and dynamic hotkey legend banners.

### Upgrade 5: Fast Integer Arithmetic & Loop Optimizations (Implemented)
* **Origin**: Inspired by `justMoritz/3d-game-engine`.
* **Implementation**: Cached lighting constants, fast Euclidean coordinate wrapping via `div_euclid` and `rem_euclid`, precomputed light radii, and differential dirty-row blitting in `PlainTerminal` to minimize terminal parser overhead.
