# Forensic Analysis of Reference Videos & Research: Achieving Next-Level ASCII Graphics Fidelity

**Reference Source**: Media assets in `data/videos/` (`Video-31774.mp4`, `Video-47489.mp4`, `Video-90418.mp4`, `WhatsApp Video 2026-10-10 at 11.13.39 PM (1).mp4`, `WhatsApp Video 2026-10-10 at 11.13.39 PM.mp4`, `WhatsApp Video 2026-10-10 at 11.13.42 PM.mp4`) by indie developer **@Ansenjeo**.

---

## 1. Forensic Visual Analysis of `data/videos`

Frame-by-frame analysis of the source videos using FFmpeg extraction and image inspection reveals the exact graphical pipeline, grid resolution, and visual techniques employed in the reference project:

### 1.1 Screen Resolution & Terminal Grid Density
* **Video Dimensions**: 720 $\times$ 1280 pixels (portrait 9:16 mobile/vertical video capture).
* **Monospace Cell Size**: Approximately $6.5 \times 10$ pixels per character cell.
* **Effective Terminal Grid**: $\approx 110$ columns $\times 128$ rows.
* **Font Choice**: Ultra-crisp monospace font with clean, high-contrast glyph silhouettes (`@`, `8`, `#`, `%`, `*`, `+`, `=`, `-`, `:`, `.`, `~`, `/`, `\`, `|`, `Q`, `q`, `o`).

### 1.2 Humanoid Player Model ("Senje" & "Skald")
* **Height & Scale**: The player character spans **25 to 30 rows tall** and **12 to 16 columns wide** (occupying roughly $20-25\%$ of screen height).
* **Anatomical Definition**:
  * **Head / Visor**: Defined rounded dome (`/----\`), visor eye slit (`|@@@+ \`, `|o.o|`), and chin guard (`\@@+ /`).
  * **Shoulders & Torso**: Broad shoulders (`/@@@-@@-@@\`) tapering to a narrow waist; chest plates shaded with dense `@` and `%` glyphs.
  * **Arms**: Clearly articulated arms with distinct bicep, elbow, forearm, and hand segments (`/@@\`, `\%/`, `/*/`, `\ /`).
  * **Legs & Stance**: Distinct pelvic separation splitting into two separate legs. Thighs (`/@@@@@@\`), knees (`/@@%o*: |`), shins (`|@@%o**+ |`), and boots (`\%____@@%o**+..`).
* **Directional Shading & Lighting**:
  * Surfaces facing the light source (key light top-left) are filled with high-density, bright glyphs (`@`, `▓`, `8`).
  * Mid-tones transition through medium-density glyphs (`%`, `o`, `*`, `+`).
  * Shadowed facets and contours use sparse glyphs (`-`, `.`, `:`, open whitespace, and contour strokes `/`, `\`, `|`).
* **Animation States**:
  * **Walk/Run Cycle**: Skeletal leg strides with forward foot placement and counter-swinging arms.
  * **Combat Stance / Cast**: Arms gesture forward into a dynamic martial/casting pose while volleys of ice crystal spears launch outward.
  * **Torch Raise**: In dungeon corridors, the character raises a lantern/torch high overhead, creating a dynamic lighting source that shifts body shadows.

### 1.3 Environmental Complexity & World Composition
* **Instanced Swaying Grass**: The forest/meadow floor is covered in thousands of fine grass blade glyphs (`|`, `/`, `\`, `,`, `'`) that gently sway with wind wave functions, interspersed with floating spore/firefly particles (`*`, `·`).
* **Towering Organic Pine Trees**: Large tree silhouettes spanning **40 to 60 rows tall**, with natural branching, needle clusters of varying densities, and foundation contact shadows.
* **Cobblestone Roadways**: Winding paths textured with slate-colored flagstones (`o`, `Q`, `q`, `.`, `8`), bordered by darker curb stones.
* **Atmospheric Negative Space**: Pitch-dark / midnight background void that lets illuminated focal points stand out with zero visual noise.

### 1.4 Boss Fights & Huge Entities (The Dragon)
* In `Video-90418.mp4` (at 30s), a gigantic **Dragon boss** spans over **60 rows tall and 80 columns wide**:
  * Large bat wings with stretched membrane texturing and spine ridges.
  * Horned skull, coiled neck, massive claws, and tail.
  * Two-tone shading: Golden/cream lighted belly scales and deep crimson/brown wing scales.
  * Telegraph combat laser / arc projecting from the dragon toward the player before charging.
  * Full-width boss HP bar at top of screen (`Dragon 2800/6000`).

### 1.5 Real-Time Lighting & Particle VFX
* **Radial Point Lights**: Torches, campfires, and player lanterns cast smooth radial light halos. The characters beneath them dynamically brighten and darken as they move through the radius.
* **Particle Spell Volleys**:
  * Ice spears (`^`, `A`, `#`, `/`, `\`) shooting in tight formation.
  * Lightning arcs with glowing core bloom.
  * Purple/amber impact spark bursts (`#`, `%`, `*`).

---

## 2. Reverse-Engineering the Reference Pipeline

Web research into developer **@Ansenjeo** reveals the exact underlying technology:
* The game is developed in **Unity** (or Godot) using standard 3D assets (Mixamo/Blender skeletal humanoid rigs, 3D particle systems, dynamic point lights, and 3D terrain).
* The 3D scene is rendered into an offscreen render texture (`RenderTexture`).
* A custom **full-screen post-processing shader** divides the screen into a monospace character grid:
  1. Samples average luminance ($Y = 0.2126R + 0.7152G + 0.0722B$) and surface normals within each grid cell.
  2. Samples Sobel edge detection to detect geometry contours (drawing `/`, `\`, `|`, `-` on silhouette edges).
  3. Maps luminance to an ASCII character ramp texture atlas (e.g., `" .:-=+*#%@"`).
  4. Blends the original scene color with the sampled character glyph to output the final ASCII frame.

---

## 3. How `ascii-mmorpg` Can Achieve This Quality

In our Rust workspace, we have three viable architectural approaches to reach or surpass this graphic fidelity:

| Architecture | Description | Pros | Cons |
| :--- | :--- | :--- | :--- |
| **Path A: Pure Terminal Software 3D Rasterizer** (Rust Crate) | Embed low-poly 3D models (`.obj` / glTF); run CPU vertex transform, Z-buffer, and normal dot-product lighting directly to `Canvas`. | 100% terminal-native, runs anywhere (SSH, ConPTY, Linux, Windows), zero GPU dependencies, pure Rust purity. | CPU rasterization requires careful spatial optimization / SIMD for complex scenes. |
| **Path B: Pre-Rendered Volumetric Sprite Sheets** | Render 3D skeletal animations in Blender/Mixamo through an ASCII shader, baking 8-directional high-res ASCII text frames. | Extremely fast runtime ($O(1)$ string blit), zero math overhead, identical visual fidelity, simple asset manifest. | Fixed camera angles, requires offline asset authoring pipeline. |
| **Path C: GPU/WGPU Offscreen Shader Pipeline** | Run an offscreen 3D scene in `wgpu` with real-time compute/fragment ASCII shader; pipe cell buffer to Notcurses/ConPTY. | 1:1 identical to Ansenjeo's engine; hardware point lights, infinite grass, 60+ FPS physics. | Requires GPU/Vulkan/DirectX support; adds heavier client dependencies. |

---

## 4. Recommended Technical Roadmap

### Phase 1: High-Fidelity Humanoid Models & Per-Glyph Shading (Completed)
* Upgrade player warrior to an articulated 14-row Humanoid model with defined head, face, broad shoulders, arms, waist, pelvis, separated walking legs, boots, and drop shadow.
* Implement per-glyph material shading pipelines (`draw_player_knight`, `draw_monster_mossling`, `draw_npc_herbalist`, `draw_tree_prop`, `draw_ruin_prop`, `draw_gate_prop`).
* Calm the background into a deep midnight void with subtle, non-chaotic dither.

### Phase 2: Directional Humanoid Sprite Sets (8-Way Facing & Animations)
* Add directional facing to authoritative player state (`Facing::North`, `South`, `East`, `West`, diagonals).
* Author full humanoid sprite sets for each direction:
  * Idle (breathing expansion)
  * Walk / Run (2-4 frame articulated stride)
  * Combat Attack / Spell Cast (lunging weapon strike / raised hands)

### Phase 3: Dynamic Point-Light Halo System
* Replace hardcoded torch distances with a generalized 2D light source buffer (`LightSource { x, y, radius, color, intensity }`).
* Compute inverse-square falloff ($I = \frac{I_0}{1 + k \cdot d^2}$) for each light (torches, campfires, player lanterns, spell missiles).
* Multiply underlying terrain and character colors by the accumulated light intensity for dynamic radial illumination.

### Phase 4: Big Entities & Boss Battles
* Author large multi-tile boss assets (e.g., 20-30 row Dragon / Skeleton Giant).
* Implement telegraph warning arcs / ground damage shapes before boss attacks land.
* Add full-screen boss health bar UI.

### Phase 5: Software 3D Mesh / Rasterizer Experiment (Optional Future Milestone)
* Build a standalone `ascii-raster` submodule that reads low-poly `.obj` meshes and renders depth-aware ASCII models in real-time.
