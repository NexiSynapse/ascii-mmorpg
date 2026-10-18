# Reference-capture analysis → visual-gap playbook (2026-10-17)

Produced by `video_analysis.py` (see README). Every number below is a
measurement, not an impression. Limitations at the bottom.

## The captures

| Video | Content | Signature moments |
|---|---|---|
| Video-90418 (89 s) | Dragon boss fight | breath cone 16.5–18.5 s, particle bursts 10.7/26.7 s, dragon **silhouette-swap death cloud** 34.4–35.6 s (ease-out), orange dashed AoE rings throughout, cake-flame spray 24.3–26.8 s |
| Video-47489 (55 s) | Spell testing | lightning columns 2.5–6.5 s, ground-burst explosion 8.5–14.5 s, **icicle spike waves** sweeping rows 14.5–24.5 s, bridge red-rune AoE 22.5 s, torch flame jets 32.5–34.5 s |
| Video-31774 (49 s) | Village building + inventory | purple glyph mob 16.5–22.5 s, full inventory UI frame 18.5–20.5 s, banners/lamps, battle log ribbon at top |
| WhatsApp …42 PM (51 s) | Multiplayer test | drifting white-blue **fog canopy** over everything, rain streaks, spear effect 28.3 s, green glowing mob swarm, temple ruins 32.5–46.5 s |
| WhatsApp …39 PM + (1) | More village building | **huge organic trees** (multi-glyph canopies), red banners, standing torches with warm pools, stone gate, rain |

## What the reference look actually is (cross-video)

1. **Quiet dark ground, loud actors.** Median luma is ~20–33/255 across all
   captures, but Laplacian variance is 1300–2400 (extremely detailed). That
   combination = a *dim, dense carpet* of low-contrast ground glyphs with
   sparse brighter highlights, under *bright, saturated, individually
   shaded* actors/props/trees. Our game does the inverse on grass: the
   meadow letter ramp (`+ - t o 0 …`) is comparatively loud, so nothing pops.
2. **Whole-prop silhouettes matter.** The dragon, trees, and buildings are
   shapes first, texture second — each prop is one connected silhouette
   filled with per-glyph shading, not a random glyph haze.
3. **Color carries state, glyphs carry form.** Combat FX are solid single
   hues (orange burns, white lightning, blue ice) with expanding rings +
   scatter particles; cities use warm pools (torch) vs cool ground.
4. **Telegraphs before damage.** Orange dashed ellipses on the ground mark
   incoming AoEs (90418 throughout, 47489 22.5 s) — readable 1+ s ahead.
5. **Impact framing.** Bursts begin as a solid bright core, then rings +
   ember scatter ease-out over ~1–2 s; death is a silhouette-swap cloud.
6. **Persistent atmosphere.** Rain streaks + fog canopy banks in motion
   almost always; lightning flashes brighten the *whole* screen at once
   (47489 4.5–6.5 s).
7. **Labels everywhere.** Name plates with dark backing over actors; thin
   red boss bar top-center with the name in the bar; combat log ribbon.

## Prioritized port list (mapped to our engine surfaces)

| # | Item | Reference timestamps | Engine surface | Slice size |
|---|---|---|---|---|
| 1 | Ground-quiet/actor-bright contrast hierarchy (re-tune terrain/meadow+beach ramps dim; actors keep letters) | all | `shaders.rs` ramp tables + band thresholds | M |
| 2 | Impact/ember FX pack: solid-core burst → ring → scatter, single hue per damage type | 90418 10.7/24.5/26.7, 47489 8.5–14.5 | new compute-PASS-2 particles or mesh billboards + flame material | M |
| 3 | AoE telegraph: dashed ground ellipse, color-coded, drawn pre-hit | 90418 ~0:05/0:28/3:34, 47489 22.5 | new `MeshBuffer::add_ground_ring` + material + shader dash pattern | M |
| 4 | Lightning column spell + whole-flash | 47489 2.5–6.5 | spell FX in world cells (vertical column ramp) + flash overlay | S |
| 5 | Icicle/spike wave sweep | 47489 14.5–24.5 | rows of spike billboards advancing, ease-linear | S |
| 6 | Fog canopy banks drifting | 42 PM throughout | weather layer: slow drifting glyph bands (existing fog terraces as base) | S |
| 7 | Big organic trees + banners + standing torches | 39 PM throughout | `mesh.rs` add_* + materials | M |
| 8 | Boss HP bar (thin red, name inside) + dark-backed labels | 90418 top bar | `ui.rs` HUD | S |
| 9 | Rain streak orientation | 39/42 PM | weather particle angle in GPU pass | S |

Recommended next slice: **#1 (quiet ground / bright actors)** — it is the
single change that makes everything else read correctly, and it compounds
with all later FX. Then #2 + #3 together as the "combat readability" pack.

## Honest limitations / misses

- Video "39 PM" (31.8 s) reports **zero events**: it is one continual slow
  scroll whose per-frame flow (0.7–1.6 px) sits *below its own median×1.5*
  threshold — sustained-motion content can hide below the video's own
  baseline. The overview sheet + `zoom` are the answer there.
- Audio is RMS-only (no whisper): dialogue ("I'm on X now too – @Ansenjeo",
  "Spell test") came from burned-in captions on the frames, not transcripts.
- Flow/diffs computed ≤360 px wide: pin-prick twinkles (loot sparks?) may
  be under-flagged between dense tiles; zoom any suspicious timestamp.
- The dragon-swap death (34.4–35.6 s) reads as one blur event, not an
  intentional dissolve — that interpretation is mine, not measured.

## Character-clarity study (2026-10-17, `clearness_study.py`)

Question: how do the reference's characters read as clear and distinct at
a resolution no better than ours? Method: 8 frames across all content types
(spell test idle/wind-up, boss fight, village, multiplayer combat, dungeon)
measured with `clearness_study.py`; our renderer's `--wgpu-demo` frame
rendered to PNG (`dump_to_png.py`, scene-only crop) and pushed through the
exact same metrics. Numbers are per-frame measurements.

Measured pattern (reference):

| metric | reference (8 frames) | ours (spawn scene) |
|---|---|---|
| frame p50 luma | 4–22 (median ~11) | 41.5 |
| pure-black pixels | 1.6–51.9% (one frame >half!) | 0.5% |
| carpet saturation (S, 0–255) | 121–156 | 85 |
| actor blob dL vs local ring | +60…+145 (readable ones) | +80 |
| actor/prop hue dominance (hue1) | 0.55–0.96 | player 0.48 (torch pool 1.00) |
| white-text pixels on black backing | 0.45–0.62 in world views | none in world |

How the game makes characters clear — five mechanisms, one principle:

1. **Local quiet beat, not global brightness.** The principle underneath
   everything: *inside the actor's local neighborhood, the background is
   uniform and quiet, and the actor has a strong delta against it.* Two
   modes both measured: dungeon/arena frames are near-black with a DARK
   actor silhouette carried by a bright white label (nothing on screen
   reached luma 140 except text); village/carpet frames use bright,
   saturated actors over a dim carpet. Ours already matches the dL bound
   (+80) — the pop is fine. What differs is the *surround*.
2. **The stage is empty (negative space).** Reference floors around
   characters are pure black or a single uniform value — up to 52% pure
   black in arena frames; even the carpetiest village leaves a locally
   smooth pool under the figure. Our carpet fills 100% of cells with
   punctuation (black 0.5%) right up to sprite edges, so the sprite
   boundary competes with glyph neighborhood noise.
3. **Every character wears a dark-backed float label.** Name on a solid
   near-black plate (2 rows), often + HP digits/bar under it; the current
   target gets a dashed ellipse marker on the ground; boss adds a thin
   red top bar with the name inside. In the densest frames (multiplayer
   rain + canopy wash) the plates are what carries readability — 45–62%
   of white text in world views sits on black backing. We render no
   in-world labels at all (names live in the side panel only).
4. **The carpet is dark AND saturated; the actor is bright (or white)** —
   two radically different palettes, measured S 121–156 deep navy/teal
   carpet vs S≤40-ish desaturated-white or single-hue actors. Ours sweep
   the carpet grey-green (S 85) and the player splits .48 across
   metal+cloth hues. The contrast axis is brightness for the floor and
   saturation/hue for the identity.
5. **In-world state glyphs**: HP digits next to monsters, possession-like
   subtitle lines under names, target marker under the current enemy.

Concrete ports ranked by evidence (our surfaces):
| # | Mechanism | Engine surface | Size |
|---|---|---|---|
| A | Dark-backed floating nameplates + HP (all actors; white-on-black, world-projected) | `ui.rs` blit pass + camera projection (already have `camera_state`) | M |
| B | Ground quiet-radius: suppress/dim ground glyph detail within ~2.5 cells of each actor so silhouettes stand on a near-black stage | compute pass entity mask | S–M |
| C | Target dashed ellipse marker + HP digits under current target, boss-style top bar | item #3 telegraph ring surface (same dash language) | M |
| D | Carpet hue identity: shift ground palette from grey-green (S≈85) toward the deep saturated darks (S≈120–150) at same luma | material colors in `mesh.rs` | S |

Tools this study added: `clearness_study.py` (metrics on any frame), 
`dump_to_png.py` (ANSI dump → PNG for identical measurement; note: fixed 
2026-10-17 to track background + paint space cells with bg, and to write 
RGB so `cv2.imwrite`'s BGR no longer swaps red/blue — measurement PNGs 
before this date have hue-mirrored color, saturation/luma unaffected). 
Honest bounds: 8 representative frames, blob detection on luma≥140 ink 
(very dim actors are invisible to it — the dungeon frame's dark silhouette 
was found via its label only); plate% is a backing heuristic (ring 
erode/dilate), not OCR; hues use OpenCV HSV range.

Implementation status (2026-10-17, same day): A, B, C, D all shipped as
one verification slice.

- **A + C — nameplates + target ring.** `SceneLabel` (`ui.rs`) carries the
  projected feet/head anchor, name, `Option<(hp, max)>`, player/target
  flags; `gpu/renderer.rs compute_scene_labels` projects with the frame's
  `view_proj` (player tracks the smoothed render position), the target
  rule mirrors `Command::Attack`'s first-alive-monster-in-melee-range.
  Plates: black bg, white name (player bold), `[██████] 12/95` red
  mini-bar + digits for combatants; dashed orange `=` ring at the target's
  feet; player plate drawn last. A boss-style top bar is deferred until a
  boss exists to put in it.
- **B — stage clearance.** Compute pass scans the metadata entity channel
  at 1 texel/cell over ±3 cells; ground cells near any actor emit pure
  space, the outer shell dithers by `hash2(x,y)` for a ragged arena edge;
  rain/props/terrain/actor surfaces untouched. Measured: scene black%
  0.5% → 4.1% (reference band 1.6–52%).
- **D — carpet hue identity.** Meadow/trunks/pines/road/border/blade
  albedo pushed to raised-chroma same-luma versions (pinned meadow
  [0.012,0.072,0.028]; contract `g <= 0.15` still test-enforced).
  Measured: carpet-saturation p90 128, p75 102→128, mean 74→87, share of
  S≥100 pixels 29%→47% (reference median 121–156; partially reached —
  the 32-level channel quantizer keeps the punctuations' floor at S≈85).
- Proxy plate% on our new frame: 0.29 (was none), gaps vs reference
  0.45–0.62 remain mostly because metric counts scene-wide sparse ink.

Implementation status (2026-10-17, second slice): port items #2 and #3,
the combat-readability pack, shipped as one FX slice.

- **#2 — impact/ember bursts.** `MeshBuffer::add_impact_burst` plays the
  reference timeline: white-hot solid core (progress 0), expanding ground
  ring, then six ember shards rising and fading over 0.45 s. `gpu/
  renderer.rs` watches the snapshot combat log presentation-side: `You hit
  {name} for {n} damage` spawns a single-hue amber-ember burst at the
  named target; `You cast {spell}` spawns the telegraph-then-impact
  sequence at the caster (Cleave amber arc / Iron Will cyan spirit /
  Thunder Mark blue lightning; unknown spells neutral gray). FX quads are
  emissive material 4 stamped with reserved entity id 254 and class 0 so
  they survive stage clearance and keep their authored hue (class 0 on
  purpose — the 60% class tint on edge cells would swamp small quads).
  Deduped by message, capped at 16 concurrent, expired by
  `telegraph + 0.45 s`.
- **#3 — AoE telegraphs (honest scoping).** The reference draws dashes
  pre-hit for mob AoE; our mobs have no AoE, so the client's only
  client-known pre-hit event is the player's own queued cast — the
  `MeshBuffer::add_ground_ring` dashed ellipse carries that (dashes sweep
  inward + rotate during the wind-up, then resolve into the burst). A
  true mob-AoE telegraph needs a server gameplay intent to observe and is
  deferred. Dashes already landed at the target's feet with the clarity
  pack (item C) cover the "marked spot" language for melee.
- FX is GPU-path only; the CPU fallback (and `--cpu`) renders no bursts
  yet — consistent with the CPU path also having no camera projection.
