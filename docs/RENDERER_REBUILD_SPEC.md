# Continuous Veyrath Renderer Rebuild

## Problem Statement

The current windowed play renderer treats the world as a terminal-like logical
grid. The submitted screenshots show torch effects as ordinary coloured glyphs
without a readable warm light field. Grass, rain, characters, and effects are
constrained by cell ownership, integer placement, uniform glyph sizing, and
8-bit colour composition.

This prevents the renderer from matching the reference's animated, glowing,
layered ASCII appearance and prevents the new Veyrath map from becoming a rich
visual world. The repository now has a Veyrath authoring checkpoint and World
Map modal, but Veyrath is not yet authoritative gameplay terrain. The rebuilt
renderer must complement that update without mutating server-owned state or
treating provisional map anchors as final collision coordinates.

## Solution

Rebuild the world renderer around a continuous GPU canvas with a black
background and independent visual primitives. Keep the existing terminal/grid
path as a compatibility fallback and keep the HUD cell-aligned during the
first milestone.

The new path will render Veyrath-backed world visuals, layered animated glyph
characters, grass, rain, torches, sun/rays, terrain details, and effects with
continuous positions, depth, per-primitive transforms, gradients, opacity,
emission, and bloom.

The first vertical slice is a standalone torch/sun showcase rendered by the
new path. It must then be integrated into the in-world torches before terrain
and characters are migrated.

## User Stories

1. As a player, I want the world to render on a black continuous canvas so
   empty space remains dark and intentional.
2. As a player, I want glyphs to move between logical rows and columns so
   animation does not snap like a terminal.
3. As a player, I want individual glyphs to scale, rotate, skew, squiggle,
   fade, and change opacity so characters and effects feel alive.
4. As a player, I want one glyph to contain multiple colours and gradients so
   fire, sunlight, water, and magic have internal visual structure.
5. As a player, I want torch light to illuminate nearby world visuals rather
   than only recolour the flame glyph.
6. As a player, I want torch rays and embers to originate at the flame and
   drift along seeded curved paths.
7. As a player, I want particles to use different glyph sizes so a torch has
   a bright core, small sparks, and fading dust.
8. As a player, I want glow contributions to accumulate in a floating-point
   buffer so overlapping lights remain visible before final conversion.
9. As a player, I want bloom to spread softly while unlit void remains mostly
   black.
10. As a player, I want the sun to be representable by a glyph cluster with
    animated rays and light spill.
11. As a player, I want grass to wave coherently with weather while remaining
    grounded at its base.
12. As a player, I want rain to fall diagonally with wind, depth variation,
    fractional motion, and non-uniform paths.
13. As a player, I want characters to be composed of independently animated
    glyph layers rather than one rigid terminal block.
14. As a player, I want character layers to have independent depth, scale,
    tilt, colour, emission, and animation phases.
15. As a player, I want terrain to use sparse continuous primitives and
    negative space instead of a glyph in every cell.
16. As a player, I want Veyrath regions, roads, rivers, settlements, and
    landmarks to influence the renderer's visual identity.
17. As a player, I want the existing Veyrath World Map modal and the future
    continuous world view to share one map data model.
18. As a player, I want the fixed elevated oblique 2.5D camera to remain
    readable while the representation becomes continuous.
19. As a player, I want fullscreen to preserve aspect ratio and upscale the
    authored scene without horizontal distortion.
20. As a player, I want fullscreen to increase visual scale or effect density
    modestly while preserving a 60 FPS target.
21. As a player, I want weaker hardware to reduce effect density or internal
    render scale rather than causing unusable frame times.
22. As a player, I want the HUD and inventory to remain readable while the
    world renderer is rebuilt.
23. As a developer, I want effects to be presentation-only so server authority
    and protocol version 2 remain unchanged.
24. As a developer, I want deterministic seeds and fixed-time inputs so visual
    effects can be tested and replayed.
25. As a developer, I want a continuous renderer seam tested through
    observable primitive, depth, emission, and animation output.
26. As a developer, I want the old grid-first world path retained as a fallback
    until the continuous path is visually accepted.
27. As a developer, I want map authoring status and provisional coordinates to
    remain explicit so unaccepted Veyrath geography is not final collision.
28. As a developer, I want accepted Veyrath data reusable for world overview,
    regional insets, landmarks, terrain themes, and future playable zones.

## Implementation Decisions

- The continuous renderer is the primary new world representation; the existing
  logical-grid presenter remains a fallback and compatibility path.
- The first implementation preserves the fixed elevated oblique 2.5D camera
  and GPU depth ordering while replacing grid-rasterized world output.
- The world canvas clears to black. Empty void is not painted with a coloured
  floor.
- A unified primitive scene model supports glyph sprites, glyph clusters,
  ribbons, streaks, rays, particles, light emitters, and depth-tested meshes.
- Each glyph primitive supports continuous position, depth, scale, rotation,
  skew/tilt, opacity, foreground/background/emission colour, colour gradients,
  glow strength, and animation phase.
- Animation is deterministic and presentation-only. Effect descriptions
  provide emitter origin, lifetime, seeded direction, acceleration, curve/noise,
  glyph sequence, colour/scale/opacity/glow curves, depth layer, and blend
  mode.
- Rendering uses floating-point intermediate colour/emission data. Emissive
  primitives write to an emission buffer; a bounded blur/downsample pass creates
  bloom; final composite adds bloom before display conversion.
- Lighting is stylized rather than physically accurate: compact warm torch
  pools, dark unlit terrain, exaggerated emissive cores, readable coloured
  spill, and no flat pasted circular halo.
- Fullscreen uses aspect-preserving scale-to-fit with black letterboxing when
  required. Authored scene scale may increase in fullscreen, but geometry must
  not stretch.
- HUD and inventory remain cell-aligned during the first milestone. World
  effects, terrain, characters, and combat visuals migrate first.
- Veyrath authoring data is first consumed as a visual/data-driven world layer.
  Political checkpoint data, atlas, regions, faction palette, rivers, roads,
  settlements, and landmarks are not authoritative collision or simulation
  until separately accepted and converted.
- Existing World Map UI and the new renderer share Veyrath semantic data. The
  printable ASCII atlas remains available for terminal fallback.
- The first vertical slice is an isolated black-background torch/sun showcase.
  It must be visually accepted before replacing the in-world torch path, then
  terrain and characters migrate incrementally.
- No protocol, server simulation, or client-authored gameplay position change
  is introduced.
- The target is 60 FPS at the authored scene on the NVIDIA GTX 1650. Adaptive
  internal render scale and effect-density degradation are allowed on weaker
  hardware.

## Testing Decisions

- Tests exercise public seams and observable behaviour, not private details.
- The primary seam is a deterministic continuous-scene builder receiving a
  Veyrath-backed scene and fixed time, returning primitives, depth data, and
  emission/light contributions.
- Primitive tests verify independent position, scale, rotation/skew, opacity,
  gradient, and glow behaviour.
- Effect tests verify deterministic seeded trajectories, curved/randomized
  motion, fade/shrink curves, source anchoring, and time variation.
- Bloom tests verify that emission affects nearby existing world visuals while
  empty void remains dark.
- Weather tests verify grounded grass bases, coherent tip motion, directional
  rain, depth variation, and storm density changes.
- Map tests verify Veyrath regions, landmarks, roads, rivers, and faction
  colours project consistently at fixed camera/time inputs.
- Authority tests verify snapshots, server positions, collision data, and
  protocol messages remain unchanged by rendering.
- Fullscreen tests verify aspect-preserving scale-to-fit and stable authored
  scene dimensions.
- Performance smoke tests use the existing Vulkan presenter on the GTX 1650
  and record frame counts for showcase and play scenes.
- Existing client map tests, engine raster tests, GPU parity tests,
  `play --check`, workspace tests, clippy, content validation, and
  `git diff --check` remain required.

## Out of Scope

- Making Veyrath geography authoritative for server collision, movement, zone
  simulation, or quests.
- Changing protocol version 2.
- Replacing HUD/inventory with continuous UI in the first milestone.
- Physically accurate global illumination, reflections, or path tracing.
- Deleting the old grid-first renderer before live visual acceptance.
- Final settlement placement or canon acceptance in Azgaar.
- Adding remote multiplayer rendering authority or sending rendered frames over
  the wire.

## Further Notes

The attached screenshots are a failed visual acceptance signal for the old
torch path: they show local coloured torch glyphs but no readable surrounding
luminance. The new architecture must make this impossible to hide behind a
source-layer or integer-colour test; acceptance needs a real emission field and
bloom composite.

The Veyrath checkpoint begins at Millhaven in Greyfall Vale as the intended
starter area. Local terrain generation should use map semantic region and
landmark data while retaining a separate conversion step for accepted playable
geometry.
