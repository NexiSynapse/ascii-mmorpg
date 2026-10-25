# Video mimicry plan — frame-by-frame reference analysis

Date: 2026-10-25. Source: `data/videos/` (all 720×1280 @ 30fps portrait captures).
Method: re-extracted every frame (`video_analysis.py zoom`) around each previously detected
motion band / cut, quantified per-frame motion with FFT phase correlation
(`video-tools/motion_fft.py`, self-test validated) and connected-component glyph-pitch
statistics, and vision-analyzed 10 representative frames (idle, walk mid-stride, channel-FX,
boss, boss-death peak, village, two inventory-modal frames, two cut boundaries).

## Measured facts

### View & grid
- World views sit on a **uniform lattice ≈ 6.5px × 9.5px per cell (~110 cols × 134 rows)**
  (modal NN-centroid gaps: x=6–7px, y=9–10px across all world frames). Cells are taller
  than wide, terminal-style. No region of a world view switches to a different pitch.
- Camera: **oblique top-down** (head/shoulders visible). V90418 adds depth: row pitch
  compresses from **14–16px near the camera to 9–10px at the horizon** (same column
  lattice — perspective projection, not a font change). Waterline wave rows at the horizon.
- Black void dominates: 55–95% empty. Dark-forest floor is **pure black — no ground glyphs
  at all**; the village scene carries ground texture/rain (25–27% ink).
- Sprites are **multi-glyph and letter-built**: trees 3–6 rows / 20–40 glyphs with per-tree
  canopy variety (bushy `qQO0b` clusters vs columnar `s|_` trunks); player 5–6 glyph glowing
  cluster (blob-like while channelling/aura) reading as 8–12 glyphs tall in combat poses;
  bosses ~20+ glyph clusters, 2.5–3× the player. Nameplates render above figures in-world.
- Lighting: player/torches are focal colored blooms that **tint nearby glyphs with falloff**
  (village torch glow discs ~20 cells wide; death rings amber-tint the field). V90418 biome
  has a pale-cyan palette with a horizon brightness gradient.

### Movement (phase correlation, per frame at 30fps)
- V47489 channel-walk segment: **ease-in ramp ≈ 0.4s → constant cruise ≈ 8px/frame
  diagonal (≈1.2 cells/frame, ~240px/s) held ~2s → ease-out stop ≈ 0.5s.** Scroll is
  quantized per frame; no motion blur, no sub-pixel smear.
- V31774 forest walk: same follow-camera with **turns** (direction changes, brief stops)
  — steady cruise with step-like direction edits rather than a single straight pan.
- Walking figures get **motion-trail glyph streaks** behind them.

### Animations & FX
- Ambient churn is everywhere, not only on actors: boss-fight frames change 30–39% of ink
  per frame vs ~0.5–5% for calm world — water rows, rain streaks, motes, boss/player
  idle animators all run simultaneously.
- FX vocabulary at burst (V90418 34.43–36.87s, V90418 death 76.37–79.5s): **telegraph
  ground ring (dashed arc) → concentric golden particle rings + spiral arcs** built from
  `*`/`.` glyphs, peak coverage ~35–40% of screen, **terrain tint with distance falloff**,
  thin ground shockwave arc, lingering ember speckle; heavy phase ~1.4s then ~1s decay
  (ease-out). Boss death keeps a readable silhouette + health-bar sliver inside the effect.
- Channelled spells (V47489): vertical **twisting particle column**, bright dense core
  thinning upward, no terrain bloom — effect-local density is higher than the background's.

### Transitions
- **No fades, wipes, or letterboxing anywhere.** Scene changes are **1-frame hard swaps**
  (exactly one frame at ~56% absdiff, neighbours <1%): world → inventory modal
  (V31774 t≈16.5, V47489 t≈47.2), and hard cuts to still segments.
- The inventory modal shows the **world dimly behind**, a **Matrix-style ASCII-rain
  backdrop at a smaller pitch than the world**, a **larger 8×12px UI font**, equipment
  slots with **ASCII-art item sprites** (sword, helmet, shield), stat bars, and a bag grid
  with scrollbar.

## Hypothesis verdicts (user's assumptions)

1. *"Some places use more dots for higher resolution"* — **refuted within world views,
   confirmed across screens.** The world lattice is uniform; the density impression comes
   from (a) dense multi-glyph sprites over a 2–9%-ink floor, (b) V90418's perspective row
   compression (projection, same lattice), (c) **different pitches per context**: world
   6.5×9.5px vs inventory UI 8×12px vs ASCII-rain backdrop smaller still.
2. *"Remove restrictions on how close two characters can be"* — **no such restriction
   exists in the continuous path.** Native primitives are positioned in continuous grid
   space, may overlap freely (paint order = push order), and already use sub-cell spacing
   (flame rows sit at 0.55-cell pitch). The only grid-locked path is cell layers
   (one glyph per integer cell). What is genuinely missing is (a) per-partition cell pitch,
   (b) optional perspective row scaling, (c) sprite authoring that exploits sub-cell pitch.

## Engine gaps → bounded slices

Each slice is independently verifiable; run in CI order after each
(`fmt`, `test --workspace`, `clippy -D warnings`, present-feature lib tests, `--check`s).

- **S1 · Per-partition pitch (de-restriction, engine seam).** Add a `pitch_scale` (or
  explicit cell size) to partitions; apply it when that partition's cell quads are emitted
  in `frame_continuous_instances`. World stays 1.0; modal UI uses ~1.25 (8×12 feel);
  ASCII-rain backdrop can go <1. Acceptance: unit test asserting per-partition quad sizes;
  play `--check` contract; no change to world rendering at scale 1.0.
- **S2 · Letter-built multi-glyph props.** Upgrade `veyrath_world` sprites: trees as 3–6
  row letter canopies (2–3 canopy variants by hash) at 0.55 sub-cell pitch via native
  primitives; keep `truncate` budget; sparse pure-black forest floor mode. Acceptance:
  world `--check` still passes; per-tree glyph count/canopy-variant tests.
- **S3 · Camera-follow easing + walk trails.** Follow-camera profile ease-in 0.4s /
  constant cruise / ease-out 0.5s (reuse `fx_timeline` eases); scroll quantized per
  frame; short motion-trail glyph streaks behind moving actors (weather-style budget).
  Acceptance: camera position unit test pins the ramp shape; trail budget test.
- **S4 · FX vocabulary pack.** Extend the play FX integration: concentric particle ring
  stack + spiral arcs at peak, ground shockwave arc, ember speckle, ~1.4s heavy + ~1s
  ease-out decay, terrain tint through existing bloom receivers. Acceptance: fx
  phase/budget `--check` contracts.
- **S5 · Inventory-style modal.** World dimmed behind (existing stage/dither clearance),
  ASCII-rain backdrop (tiny letter motes at S1 small pitch), larger-pitch UI font,
  ASCII-art item sprites, bag grid + stat bars. Hard 1-frame open/close (no fade) —
  matches current M-map modal behavior. Acceptance: modal `--check` partition/pitch test.
- **S6 · Perspective row compression (optional depth).** Far rows at reduced y-pitch for
  depth scenes (V90-style waterline/horizon): either per-row `Transform2` y-scale in
  scene authoring (no engine change) or a partition-level row-pitch ramp. Decide after
  S2 lands; only if the flat oblique view reads as insufficiently deep.

Not planned: fades/wipes (reference has none), variable within-world dot density
(refuted), removing non-existent spacing limits. Portrait-vs-landscape: our canvas is
landscape; adopt the *ratios* (cell aspect ≈ 0.68, ink density, sprite sizes), not the
phone aspect.
