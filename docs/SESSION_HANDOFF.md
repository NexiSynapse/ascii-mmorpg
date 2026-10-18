# ASCII MMORPG — session handoff

Updated: 2026-10-18. This is the resumable engineering record, not a full transcript.

Latest slice: the 3D play renderer now has a 4x4 depth sample resolve, broad
material-specific printable-ASCII glyph families, stable coverage/normal/
variance-aware selection, extra tree/road/gate detail, and richer close actor
planes. The world-black foreground-only contract remains intact. The next
human check is a live close/medium/far comparison of glyph density and
silhouette readability.

## Start here

Workspace: `ASCII-Game/`; application repository: `ascii-mmorpg/`. Read the parent and repository `AGENTS.md` files. Latest turn: "Game feel, character animation & visual fidelity refinement" section at the end — the 3D play path now uses joint-pivot humanoid poses, responsive facing/gait/attack presentation state, focus-safe input, faceted cones/road landmarks, and improved actor labels; the terminal GPU path now has attack-driven poses, grounded limbs/grass, distinct road/sand/stone materials, slope normals, static non-foliage organic surfaces, and differentiated storm weather. The next human check remains the live 3D window plus the 156x44 TUI.
Repository status (verified 2026-10-18, Windows): checked-out `rendering_gra_update` is at `b1afcfd` and tracks `origin/rendering_gra_update`; `main`/`origin/main` are separate and behind. Do not commit or push unless asked. Current intentional worktree changes include the refinement, black-world, and high-density slices in the client/GPU and engine paths; the pre-existing user modification to `play_ansi.bat` is preserved untouched. CRLF-vs-LF warnings are historical; current diffs are real content.

Current slices completed:
- Pine tree trunk visibility: elevated lowest canopy cone to Y = 3.60, assigned trunk and root flares dedicated Wood/Timber material (7.0) with vertical bark striations, cleared highway collision corridor (|x| <= 4), and synchronized procedural tree rendering on GPU.
- Synchronized meadow grass wind sway: low-altitude vertex sway in `vs_main` and wind-leaning ASCII blade characters in `ASCII_POST_WGSL` traveling in sync with wind current phase.
- Smooth transitions: exponential spring damping for camera and player coordinates, plus ease-out deceleration for humanoid walk phase.
- GPU animation/damping is frame-rate independent, stationary terrain is separated from foliage wind, while deterministic batch/demo rendering remains intact.
- Server tick documentation now matches the implementation, and the TCP loop has a regression test covering idle timeout and immediate input advancement.
- Video analysis pipeline built and all six reference captures measured with it: dense adaptive sampling, contact sheets, motion/easing/blur/freeze metrics. See `ascii-mmorpg/video-tools/REPORT.md` for the look-parity port list.
- Quiet-ground / actor-bright contrast hierarchy (port list #1, measured): meadow/pines/trunks/road dimmed to reference darkness, blade field thinned to tufts; scene median fg luma 71→33/255, letters off the ground, actors pop. See the latest handoff section.
- Character-model shading from the two ASCII-renderer articles (cel-quantized sun facets + kciter headlight + rim light, entity fragments only), which exposed and fixed a latent `set_entity_context` bug that stamped terrain/props with entity metadata id 1 instead of the reserved 0. See the latest handoff section.
- Character-clarity study (measured, `video-tools/clearness_study.py` + `dump_to_png.py`): the reference's actor clarity = local-quiet surround + strong local delta (not actor brightness) + dark-backed floating nameplates/state + a dark-and-saturated carpet vs bright/white actors. Ours already matches the dL pop (+80) but has no in-world labels/backing, zero negative space, and a grey (S 85) carpet. Full table in `REPORT.md`.
- GPU ASCII shape-vector optimization: the compute pass now reuses its existing 4x4 SSAA colour samples to build a six-region glyph-shape vector, applies normalized contrast enhancement, and avoids the previous 12 redundant colour texture reads per cell. This is based on the shape-aware sampling and contrast techniques described by Alex Harri, while retaining the existing material ramps and edge glyph policy.
- OSRS-style camera controls: the GPU client now uses an elevated top-down camera with larger readable actor models; ANSI realtime mode captures the mouse and holding right-click while dragging orbits yaw/pitch around the smoothed player focus. Camera state is client-only and never enters snapshots or protocol messages.

## Master state (2026-10-13, all details consolidated)

### A. Completed slices (all verified, none pushed)
1. Strict queued-command 20 Hz scheduler (`MAX_PENDING=512`, `Simulation::advance_tick_with`, burst coalescing) + ADR-0002/SOT/AGENTS updates.
2. Pause overlay (`P`, `ActiveModal::Paused`, client-side) + dynamic partitions (`FontProfile`, `GameState`) + Ratatui single-buffer composed frame (exactly terminal-sized, `Clear`-occluded modals, side radar, status card).
3. Quiet GPU ramps (`░▒▓█` light pools, `|` edges, letter detail on entities), adaptive SSAA + native 1:1 grid + ANSI quantization + hash dirty-line diffing.
4. Notcurses input fix (`Terminal::next_key` via `poll_event()`, `decay_held_keys`, GPU notice into `world.messages`, quiet `play_notcurses.sh`).
5. Engine slice 1: `ascii-mmorpg-engine` scaffold (cells, layers, partitions, compositor, Wave/Cycle/Pulse) + `wave` headless example.
6. Engine slice 2: windowed presenter (`present.rs`: winit 0.30 + wgpu 24 instanced glyph atlas, Liberation Mono embedded, Vulkan-only, Esc/Q/`--frames` exit) + `present` example.
7. Engine slice 3: `Ease`/`Noise` tracks, `push_text`/`fill_rect`/`border_rect`, `sprite::` importer (`---` frames → flip-book layers), `sprite` example.
8. Port slice 1: `bridge::` snapshot→Scene 2.5D rebuild (`SceneView`, camera-follow, nameplates/HP pips/shadows, flow-field grass, torch) + `game` example.
9. Port slice 2: `SceneSource` + `run_dynamic`, playable `play` example (local sim, 20 Hz clock, WASD/F/R/Q, `--check` self-test).
10. Reference study (no code): 72 frames + docs → fidelity/animation/shader findings (sections below).
11. Shader port slice: `Cell::{glow,scale}`, compose glow-spill light buffer, 32×16 atlas with `█░▒▓` extras, `fill_shaded_rect`/`Shade`, presenter billboards, bridge torch aura/glow/flow strokes, `fx` showcase example.

### B. Engine API inventory (`engine/src/`, lib has zero required deps)
- Dynamic colour slice: `ColorTrack::{Static,Gradient,Rainbow}`, `AnimCell::with_color`, optional `SceneActor.palette`. Compose resolves colour before brightness and glow. See latest section for scope and remaining per-glyph shader work.
- `anim.rs`: `Track::{Wave,Cycle,Pulse,Ease,Noise}`, `EaseKind::{Linear,In,Out,InOut}`, `Anim`, `sample` (+degenerate-still rules).
- `cell.rs`: `Cell{glyph,fg,bg,bold,transparent,glow,scale}`, `CLEAR`, `new/bold/with_glow/with_scale`.
- `compose.rs`: `compose(scene,camera,t)->Frame`, `Frame{rows,get}`, glow spill (radius glow×4, quadratic falloff, bg-only, cross-partition).
- `layer.rs`: `AnimCell`, `Layer{push,push_text(+bold),fill_rect,border_rect,fill_shaded_rect}`, `Shade`.
- `partition.rs`/`scene.rs`: `Partition{name,x,y,w,h,layers,camera,vx,vy}`, `Scene{layers,partitions,layer()}`, `Camera{x,y}`.
- `sprite.rs`: `parse_frames` (line `---`, CRLF-tolerant), `sprite_size` (char-counted), `sprite_layer(name,z,frames,SpriteDraw{x,y,fg,bg,fps})`; spaces-in-some-frames flash opaque (documented).
- `bridge.rs`: `ActorKind`, `SceneActor`, `SceneView`, `scene_from_view` (64×(3+18+2), player-centered camera), `demo_view()`; rain z=1 below actors; plates overwrite when adjacent (demo spaced instead).
- `present.rs` (feature `present`): `run` (static via adapter), `run_dynamic` + `SceneSource{scene_at,key}`, `PresentOptions{title,cell_px,width_cells,height_cells,fps,max_frames}`; atlas 32×16; per-instance scale clamp 0.25–2.0 (sub-cell margins → clear color).
- Examples: `wave` (headless), `present` (static window), `sprite` (warrior panel), `game` (static 2.5D), `play` (playable + `--check`), `fx` (shader showcase + `--check`).
- Dev-deps only: server + shared (examples). Present needs a display; `--frames N` is the headless proof everywhere.

### C. Reference findings (pixel-verified, with confidence)
- Grid ~110×128 portrait; actors 25–30 rows, dragon 60×80 (High).
- Four composited techniques: flat mosaic cells (volumes) + oriented glyph strokes (detail) + smooth vector cores with glow halos (effects) + scaled glyph billboards (sparks, sizes vary) + crisp UI last (High — full-res crops).
- Dynamic point lights with smooth radial falloff; amber↔green global washes (High).
- Motion curve, all 1,651 frames of Video-47489: mean Δ3.08, p50 1.65, p90 8.11, max ~30; attack ≈10f (0.33 s), release ≈35f (1.2 s); poses never repeat within ⅙ s; idle = drift (Medium-High, quantitative).
- Grass = stable texture sub-second; sway modeled as slow shimmer, not displacement (Medium — one blink crop missed clean grass, recorded).
- Smooth turns = full-rate skeletal sweep, lightning = evolving polyline with sequenced nodes (High — 6 fps strips).
- Screen-vs-world glyph lock: UNRESOLVED. Brief Videos 4/5 durations swapped vs our filenames (recorded). Social captions are overlays, never UI.

### D. Verification status (all 2026-10-13, Windows)
`fmt --check` clean; `cargo test --workspace` all pass (client 74, engine 26, server 8+3, shared 14, tools 2, bot 1); `tools content` → 11 definitions; `clippy --workspace --all-targets -D warnings` clean; engine `clippy --features present` clean; `client -- --demo` exit 0; `sprite`/`game`/`play`/`fx` examples all presented via `--frames` exit 0; `play -- --check` and `fx -- --check` ok. Frame forensics in `%TEMP%\opencode\frames\` (contact sheets, dense strips, crops) — regenerable via the ffmpeg commands in chat history, not checked in.

### E. Ordered next actions (one bounded slice per session)
1. Data-driven particle emitters (`fx::` layers: burst/ring/chain/spike/number records) replacing the scripted `fx` demo. **Completed 2026-10-13; see verification below.**
2. Amber wash uniform + verified point-light falloff; screen/world-lock test with stabilized footage.
3. Port slice 3: TCP windowed client (`--connect`) + modals/chat at terminal parity.
4. Scale up: denser grids + multi-row entity sprites via bridge; boss/target panels; label declutter.
5. Push `main` to `origin/main` ONLY when explicitly asked.

Completed this turn: fullscreen presenter/demo scaling, reliable key edge handling, richer bridge actor models, and consistent sized HUD/log overlays. Within-glyph fragment gradients remain deferred; this slice prioritised visible fullscreen quality and usable controls.

## Product goal and reference constraints

Build a dark medieval fantasy online RPG rendered through dense coloured terminal glyphs, with an elevated oblique/pseudo-3D world, readable silhouettes, irregular forests/paths/fortresses, localized warm torches, blue-white magic, weather, and crisp labels/HUD. Not a flat tile roguelike or a screen-fixed backdrop. Terrain, actors, props, and effects must share a camera transform. Keep idle scenery stable, allow intentional wind/weather/pose animation, and keep multiplayer authority on the server.

Earlier reference work sampled three supplied gameplay videos at 2 FPS and inspected contact sheets. Observations: dark navy negative space; green/teal vegetation; gray architecture; small bright actors with floating names and target markers; angular lightning/impact particles; large boss silhouette/bar; separate inventory panels. Social-video captions are not game UI. The original engine is unknown; geometry-to-glyph rendering is our implementation choice, not a recovered fact. Original attachments remain in the workspace inbox; temporary extracted frames may not persist. See `PROJECT_PLAN.md` for reference direction and asset provenance in `../assets/ASSET_SOURCES.md`.

## Architecture and entry points

- Rust 2021 workspace: `crates/shared`, `server`, `client`, `bot`, `tools`. Cargo declares Rust 1.70; current-toolchain checks do not prove MSRV compatibility.
- Protocol v2: length-prefixed Postcard, 4-byte little-endian length, 1 MiB cap. Eight movement directions; deterministic shared combat and Manhattan melee range 6. Shared collision/diagonal sliding is reused by local and server simulation.
- Server owns positions, combat, stats, ticks, and personalized snapshots. TCP uses per-client readers/writers and one simulation thread. Client owns input, animation, camera and terminal presentation; rendering must not mutate snapshots.
- Default GPU feature: WGPU offscreen geometry -> colour/normal-depth/entity metadata targets -> compute glyph/colour selection -> readback -> terminal/HUD. CPU renderer remains fallback. Existing hardware instancing/culling, asynchronous staging ring, depth reconstruction, edge-aware glyph selection, dithering and tone mapping should be reused.
- `client/src/gpu/renderer.rs`: scene population, lights, camera/player interpolation, walk animation, frame/batch entry points.
- `client/src/gpu/mesh.rs`: procedural geometry, metadata and grass blades. `shaders.rs`: vertex wind/water, lighting and ASCII compute shader. `pipeline.rs`: targets, SSAA, passes, buffers/readback. `uniforms.rs`: camera/time/light layout.
- `client/src/main.rs`: CLI, raw-mode lifetime, input/FPS loop. `client/src/lib.rs`: terminal/scene traits and local simulation. `client/src/ui.rs`: Ratatui HUD.
- Preserve differential ANSI dirty-row output and resize handling; avoid full-frame clearing that causes ConPTY tearing. Notcurses is optional, with vendored `crates/libnotcurses-sys` and MSYS2-aware launchers. Earlier missing-native-library results predate that work and are not its current status. Optional backend was not freshly verified in this turn.

## Work already applied before this handoff

These are inherited implementation records, not changes newly made by this documentation turn:

| Files | Changes |
| --- | --- |
| `client/src/main.rs` | RawModeGuard cleanup on failure; opposite keys cancel; diagonals selected from signed axes; exhaustive 16-state movement test. |
| `server/src/main.rs` | Drop extra error sender before writer join; send protocol errors to offending client; connect before welcome/snapshots; broadcast helper; 50 ms idle receive timeout advances simulation. |
| `server/src/lib.rs` | Explicit connected-player snapshot lookup; connect_player; demo mob ID 1000 instead of 2; reject protocol mismatch before tick/message mutation; regression coverage. |
| `crates/shared/src/collision.rs` | Filtered highway corridor (`|x| > 4`) so background procedural trees and ruins never obstruct the cobblestone road. |
| `client/src/gpu/mesh.rs` | Pine tree lowest canopy elevated to Y = 3.60; trunk and root flares assigned material 7.0 (Wood/Timber) with expanded lateral surface roots. |
| `client/src/gpu/shaders.rs` | Synchronized meadow grass vertex wind current in `vs_main`; dynamic wind-leaning blade glyphs in `ASCII_POST_WGSL`; wood material 7u with vertical bark striations (`|`, `I`, `H`, `T`, `!`, `[`, `]`, `=`, `#`). |
| `client/src/gpu/renderer.rs` | Exponential spring damping for camera follow (0.28) and player glide (0.38); smooth ease-out walk cycle deceleration (0.65); procedural background tree spawning. |
| `client/src/term.rs` | Pure-Rust double-buffered terminal engine (`CellMatrix`, TrueColor ANSI parser, span-diffing encoder, Mode 2026 markers). |
| `client/src/ui.rs` | Fixed-width ASCII inventory/equipment icons instead of wide emoji. |

Tick semantics are now documented consistently: `TICK_HZ = 20` is the target, idle TCP receive timeouts advance one tick per 50 ms, and each input also advances a tick immediately. Sustained input can therefore exceed 20 Hz; this remains a known implementation limitation rather than a claimed fixed-rate scheduler.

## Current renderer findings and next implementation order

The latest read-only GPU review identified the smallest high-value slice:

1. **Real-time animation clock and delta-time damping** — completed in `client/src/gpu/renderer.rs`. `GpuRenderer` now owns elapsed-time state, realtime scene construction uses elapsed seconds and delta-time exponential damping, while integer-frame scene construction remains deterministic for tests and batches.
2. **Stop terrain swimming** — completed in `client/src/gpu/shaders.rs`. Material-3 vertex displacement now also requires elevated world height; compute-pass glyph-level grass animation remains enabled.
3. **Only then tune water/specular lighting** — existing directional + point-light diffuse/specular/fog path is real GPU lighting. Improve falloff or water response only after visual inspection.
4. **Defer true reflections** — no reflection pass exists. It would require reflection/previous-frame textures, bind groups, camera or screen-space ray traversal, water ordering, and terminal-cell reduction handling.
5. **Defer a new upscaler** — the pipeline already renders at 4× and reduces 4×4 blocks with edge-aware glyph selection, dithering and tone mapping. Profile before adding FSR/temporal reconstruction.
6. **Defer more grass geometry** — crossed grass blades now exist. More instances could multiply cost under 4× SSAA; profile first.

## Active TODO checklist

- [x] NEW 2026-10-18 (game-feel refinement): completed 12 bounded refinement passes across the depth-raster play path and terminal GPU path — articulated pose/gait/attack timing, shortest-path facing, focus-safe input, actor grounding/label anchors, faceted environment silhouettes, distinct surface materials, slope-aware terrain lighting, foliage/static-organic separation, storm differentiation, and defensive lighting cleanup. See "Game feel, character animation & visual fidelity refinement" at the end.
- [x] NEW 2026-10-18 (critical visual overhaul): completed 15 visibility/readability passes in the windowed 3D play path — close camera framing, larger cell pitch and actors, reduced fog/edge noise, brighter hierarchy, clear terrain patches/road/rocks/trees, distinct player/mossling/skeleton/herbalist silhouettes, grounded shadows, target ring, backed nameplates, and crisp title/status HUD overlays. See "Critical visual overhaul — visibility, world detail, and readability" at the end.
- [x] NEW 2026-10-18 (fifteenth slice): 3D OSRS-mode windowed game — `examples/play.rs` renders through the depth raster backend (`raster::`) with an orbit camera (right-drag yaw/pitch + wheel zoom, camera-relative WASD via the camera basis), mesh-expressed combat FX (telegraph dashes / core / ring / embers as decals + boxes), screen-space nameplates + rising damage digits, HUD/log bands, and an `I` inventory modal. Engine additions: `Vec3` pub ops + `ops::Sub`, `Mesh::quad_y`/`decal_y`/`retinted`, `SceneSource::pointer(PointerEvent)` mouse wiring. Verified: fmt 0; workspace tests 0 failed (engine 100, +3/raster +1/present with `--features present` lib 112 all pass); tools content ok; clippy `-D warnings` workspace + present; `play -- --check` green; `play -- --frames 30` window clean. See "3D OSRS-mode windowed game".
- [ ] User live verdict on the 3D window: orbit drag feel (and only then the drag-sign check), wheel zoom, camera-relative walk, smacking the mossling with `F` (burst + digit), `1/2/3` telegraphs, `I` panel, whether the depth read is like the reference captures. Plus the carried TUI verdict at 156x44 for the FX/clarity packs.


- [x] NEW 2026-10-17 (fourteenth slice): combat FX pack + clarity passes ported into the engine's windowed renderer — `bridge::stage_pass` (dark arena under actors), `target_ring_pass` (marching dashed decal under the melee target), `fx_layer_pass` (`bridge::SceneFx` telegraph→core→ring→embers + damage digits sampled at `SceneView::presentation_time_s`), `fx::Effect::Implosion` (shrinking rotating dash ring), `play.rs` message watcher + **1/2/3** cast keys + extended `--check`. Verified: 97 engine tests (+4), `play -- --check` green, 30-frame window run clean. See "Combat FX in the windowed engine renderer" section.
- [ ] User live verdicts: (a) windowed renderer — WASD + F + 1/2/3 at `cargo run -p ascii-mmorpg-engine --features present --example play`; (b) terminal TUI — the thirteenth slice's F-burst/cast telegraph at 156x44 via `play_ansi.bat`.

- [x] NEW 2026-10-17 (thirteenth slice): combat FX pack (REPORT #2 + #3, GPU path) — message-observed impact bursts (white-hot core → ring → embers, 0.45 s, single amber hue for melee) + cast telegraphs (dashed ground ring sweeping inward, per-school hue) resolving into bursts; FX quads = emissive material 4, reserved entity id 254, class 0 (keeps authored hue); deduped/capped/pruned by animation time; deterministic path skips observation. See "Combat FX pack #2+#3" section. User live verdict pending (F attack burst + cast telegraph at 156x44).

- [x] Add explicit-time per-cell colour palettes and dynamic colour-driven background glow; demonstrate actor palette overrides and bound extreme glow.
- [ ] Add simultaneous within-glyph gradient shading in the windowed presenter (current palette resolves one foreground RGB per glyph per frame).
- [x] Add within-glyph horizontal/vertical foreground gradients to the windowed presenter while preserving flat-colour defaults and glow.
- [x] Reuse SSAA samples for six-region shape-aware glyph selection and add normalized per-cell contrast enhancement in the GPU ASCII pass.
- [x] Add OSRS-style elevated camera framing, larger detailed actor scale, and ANSI hold-right-click drag orbit controls.
- [x] GPU scene-partition fill: render the `scene_partition_dims` grid natively so `blit_scene` fills the composed HUD's scene area exactly (2026-10-17).
- [x] Synchronized-update markers (DECSET 2026) on both `PlainTerminal::draw_into` repaint paths to stop sub-ms transient line tearing (2026-10-17).
- [x] NEW 2026-10-17: user reports BOTH issues still visible live ("not fixed yet" + screenshot). RESOLVED same day (see "Open live-fill investigation" section): root cause was the adaptive-SSAA 2x tier reading compute-pass taps out of bounds (top-left half-grid coverage), NOT stale dims. Removed the 2x tier (fixed 4x).
- [x] NEW 2026-10-17 (second slice): HUD border lines "facing the canvas" missing (top bar bottom border, log-block top/left borders, side panel left border). Root cause: East-Asian-Wide emoji (🌲📍🌧⚡📜🗺💀🎯✨⚔⚠🎒🛡🏛🧭🌊👾) in HUD strings rendered 2 terminal columns but authored as 1 buffer cell — every overflowing row auto-wrapped and shredded the composed layout (row 1 was 159 cols at a 156-col terminal). Replaced all emoji-presentation chars with narrow ASCII; ambiguous-width text symbols (☼ ♠ ▲ █ ░ ⚑ •) kept (verified 1-col). See the "Live HUD border / emoji-width slice" section. All composed rows now exactly terminal width at 156x44/120x30/220x70.
- [ ] User live re-confirmation in a real terminal (`play_ansi.bat` at 156x44): scene fills the whole partition (SSAA fix), every border line is present and stable in motion (emoji-width fix), duplicated Minimap gone after relaunch (full-redraw fix), and GRID/WASD reads correct. If a seam artifact survives, investigate a per-frame GPU readback ring diff — NOT terminal encoding.
- [x] NEW 2026-10-17 (fourth slice): persistent duplicated Minimap fixed at the paint layer — `PlainTerminal::draw_into` full-redraw branch (runs at every relaunch/resize) chained rows with `\r\n`, which ConPTY mis-advances one row on full-width rows; the differential branch had already been fixed to absolute jumps but the full branch kept the bug, and hash-diffing preserved the skewed layout forever. Full redraw now uses absolute per-row `\x1b[{n};1H` addressing (no `\r\n` in either branch) + regression test. Status card now also prints `GRID WxH` as a permanent ConPTY size-mismatch diagnostic.
- [x] NEW 2026-10-17 (third slice): camera-relative movement. `SceneRenderer::camera_yaw()` (default 0.0; GPU renderer reports `CameraState.yaw`; CPU stays 0.0 = world-fixed). `movement_direction(held, yaw)` rotates the held-screen vector into world axes (screen-up = camera facing `(-sin yaw, -cos yaw)`, screen-right = `(cos yaw, -sin yaw)`) and snaps to the nearest of the 8 protocol directions. Protocol untouched (still `Command::Move(Direction)`; server owns positions). Tests: yaw-0 mapping unchanged, quarter-turn/45°/diagonal rotation, negative yaw. Help text says "WASD move (camera-relative)".
- [x] NEW 2026-10-17 (fifth slice): widened directional contrast in the GPU ASCII pass per Harri's ascii-rendering post (external sampling per component now takes the max over its widened affecting set of outward texels + corners instead of one texel — kills residual boundary staircasing; kciter's post had no missing techniques, audit recorded). Perf 4.20→4.22 ms/frame.
- [x] NEW 2026-10-17 (sixth slice): player art faces the camera. `MeshBuffer::add_humanoid` took `_facing_angle` and ignored it (both call sites hardcoded 0.0); now a real `facing_angle` — `rotate_vertices_since` rotates head/body/limbs/normals as one unit about the foot anchor, and the local player call site passes `camera_state.yaw`. Yaw 0 bit-identical; player-only per user choice. See "Player art faces-the-camera slice".
- [x] NEW 2026-10-17 (seventh slice): southern ocean at the bottom of the map — mesh water/beach classification keyed off shared `WORLD_BOUNDS.max_y`, directional rolling swell + foam + depth gradient in the water shaders, feeding the material-6 wave glyph ramp; collision untouched. Harri's writeup has no wave technique (verified) — canonical ASCII-ocean method documented in the handoff. User-reported live noise ("grass texture everywhere") fixed same day: tree/grass shoreline gates, quiet water glyph language (no letters, no strokes on water), spec/fresnel dimmed so distant water stays dark. See "Southern ocean slice".
- [x] NEW 2026-10-17 (eighth slice): video-understanding pipeline + reference analysis. `ascii-mmorpg/video-tools/` (python/ffmpeg/OpenCV, no cargo impact): `analyze` does a full-rate measurement pass (color-aware diffs, histogram cuts, DIS/Farneback flow with ease labels, blur, audio RMS) → events → dense 3 fps timestamped contact sheets; `zoom` dumps any interval at native fps + per-pair diffs.csv. Validated on a synthetic benchmark (every event class found; 7 real bugs fixed via benchmark + real data). All 6 reference videos analyzed → `video-tools/REPORT.md` = the prioritized port list (9 items, engine-surface mapped) for look parity: quiet-dim ground + bright actors, impact/ember FX, AoE telegraph rings, lightning flash, icicle sweep, fog canopy, big trees/banners/torches, boss bar/labels, rain. See "Video-tools pipeline + reference analysis slice".
- [ ] User live re-confirmation in a real terminal (`play_ansi.bat` at 156x44): scene fills the whole partition (SSAA fix), every border line is present and stable in motion (emoji-width fix), duplicated Minimap gone after relaunch (full-redraw fix), GRID/WASD reads correct, boundary lines a cell crisper in motion (widened contrast), right-drag orbit turns the player figure to keep facing the viewer (facing slice), and the ocean at the map's south edge rolls waves/foam convincingly (walk the highway to y=35 and judge). If a seam artifact survives, capture a `--wgpu-demo` dump at live size and decode per-row display width — NOT terminal encoding.
- [x] NEW 2026-10-17 (ninth slice): quiet-ground / actor-bright contrast hierarchy (port list #1). Meadow, pine, trunk, and road albedos dimmed ~half twice (measured per-cell fg luma), blade field thinned to 1-in-5 tufts, material-3 band-2 'y' swapped out; quiet-ground bound test added. Scene median fg luma 71→33/255 (reference 20–33 ✓), letters 15%→<1%, fine punctuation now dominant. See "Quiet-ground / actor-bright contrast slice".
- [x] NEW 2026-10-17 (tenth slice): character-model clarity from the two ASCII-renderer articles — cel-quantized sun diffuse (Harri cel shading / kciter Shade ramp), kciter headlight term, and rim light, all gated to entity fragments (`in.entity_id > 0.5`); found + fixed latent `set_entity_context` bug (reserved id 0 folded to metadata 1, so terrain/props carried entity_id 1 and any `entity_id > 0` gate lit the whole landscape; regression test added). See "Character-model shading + entity-metadata zero fix".
- [ ] User live re-confirmation in a real terminal (`play_ansi.bat` at 156x44) — carried six: scene fills the whole partition, HUD borders stable in motion, duplicated Minimap gone after relaunch, GRID/WASD correct, boundary lines crisp in motion, orbit turns the player to face the viewer, ocean rolls convincingly — quiet-ground verdict: ground reads as a quiet dark punctuation carpet and props/actors pop — plus this slice: the player reads with stepped lit/shadow facets, lit chest/face, faint rim, correct outline vs ground (watch for over-bright/tinted artifacts in motion).
- [x] NEW 2026-10-17 (eleventh, study): character-clarity study — measured the reference's actor-readability mechanisms against ours with 8 ref frames + our rendered dump (`clearness_study.py`/`dump_to_png.py`, REPORT.md section). Conclusion: local-quiet + local-delta + dark-backed nameplates + two-palette system; ours matches the delta pop but lacks in-world labels/backing, negative space, and carpet saturation identity. Ranked ports A–D recorded; A+B recommended next. See "Character-clarity study".
- [x] NEW 2026-10-17 (twelfth slice): character-clarity pack A+B+C+D implemented in one verified slice — dark-backed floating nameplates + HP mini-bars on every actor (world-projected via the frame's `view_proj`), orange dashed ground ring under the melee target (same rule `Command::Attack` uses), stage clearance in the compute pass (ground cells near actors emit pure space, ragged dithered edge), and same-luma raised-chroma carpet/prop palettes. Measured: black% 0.5→4.1% (ref band ✓), plate% 0→0.29, carpet-S tail p75 102→128, S≥100 share 29→47%. Two tool bugs fixed in `dump_to_png.py` (space cells now paint bg; RGB/BGR swap in cv2.imwrite — preexisting: all prior measurement PNGs had hue-mirrored color). See "Character-clarity pack A+B+C+D".
- [ ] NEXT (user call): user live verdict at 156x44 (`play_ansi.bat`) for the clarity pack — plates/HP/ring/stage in motion; then port item #2 impact/ember FX pack + #3 AoE telegraph rings (combat readability).
- [ ] Adopt `ui::scene_partition_dims` in the CPU renderer (`--cpu` path still renders the full grid and gets top-left cropped). Next bounded slice, after the live verdict.

- [x] Reconcile `AGENTS.md`, `docs/SOT.md`, ADR 0002, and `server/src/main.rs` about actual tick semantics; add a focused server test for idle and input timing without calling the current loop fixed-rate.
- [x] Implement frame-rate-independent GPU elapsed time and exponential camera/player damping in `client/src/gpu/renderer.rs`.
- [x] Fix pine tree trunk visibility: elevated lowest canopy cone to Y = 3.60, assigned Wood/Timber material 7.0 with bark striations, and cleared the highway corridor (|x| <= 4) with synchronized procedural tree rendering on GPU.
- [x] Animate meadow grass wind sway: low-altitude vertex sway in `vs_main` and dynamic wind-leaning ASCII blade characters in `ASCII_POST_WGSL` traveling in sync with wind current phase.
- [x] Smooth engine transitions: exponential spring damping for camera and player coordinates, plus ease-out deceleration for humanoid walk phase.
- [x] Remove low-height meadow displacement from `client/src/gpu/shaders.rs`; preserve elevated foliage wind and glyph-level grass animation.
- [x] Add/adjust deterministic tests that do not depend on wall-clock time; run GPU demo/manual smoke where available.
- [x] Inspect the current diff and separate inherited changes from this slice. Do not reset or discard unrelated work.
- [x] Run the project checks from `ascii-mmorpg/`: `cargo fmt --all -- --check`, `cargo test --workspace --locked`, `cargo run --locked -p ascii-mmorpg-tools -- content`, and `cargo clippy --workspace --all-targets --locked -- -D warnings`.
- [x] Run a visual GPU check (`cargo run --locked -p ascii-mmorpg-client -- --wgpu-demo 60` or `graphics_capture`) and record actual output.
- [x] Commit and push verified codebase to `origin/main` (`5652ab6`) per user request.
- [x] Implement strict queued-command 20 Hz scheduler: bounded queue (`MAX_PENDING = 512`), one tick per 50 ms idle or loaded, burst coalescing, immediate protocol-error rejection; `Simulation::advance_tick_with` + updated TCP loop tests.
- [x] Tune water/specular lighting: tighter water specular lobe (32.0/0.9), deeper base, sun 0.4→0.35 / point 0.5→0.45, fog 14/38→16/42.
- [x] Add cheap fresnel sky reflection on water as a no-new-texture stand-in; full planar/screen-space reflection pass stays deferred.
- [x] Add edge-adaptive micro-contrast sharpen in compute pass (upscaler-lite); full FSR/temporal stays deferred pending profiling.
- [x] Expand meadow grass density (~2x blades, ±18/±14, seed%2); later measured as stroke-soup against the reference and thinned to seed%5 sparse tufts in the quiet-ground slice.
- [x] Zoom perf + default-zoom legibility: capped GPU render grid (`MAX_RENDER_CELLS = 6144`, aspect-preserving, CPU nearest-neighbor upscale), adaptive SSAA (4x ≤4096 cells, else 2x), closer camera (eye 9.8/+11.5, FOV 38°), stronger entity-outline faction mix (0.40→0.60), wider pillar cull bounds.
- [x] High-res zoom fix (reverted the render cap — it blurred everything): native 1:1 GPU grid at any zoom, adaptive SSAA only cuts sub-pixel samples; 5-bit ANSI channel quantization (~40% smaller frames, 98→59 KB at 220x70); hash-based dirty-line diffing in `PlainTerminal::draw_into` (no per-frame full-frame clone); `--wgpu-demo [N] [WxH]` size arg for headless zoom benchmarking.
- [x] Implement data-driven engine particle emitters (`Burst`, `Ring`, `Chain`, `Spike`, `Number`) with explicit-time sampling, fade/release, bounded output, deterministic tests, and an `fx` showcase migration.
- [x] Make the windowed presenter fullscreen-by-default with surface-sized grids, F11 toggle, reliable key edge handling, and resize-safe instance capacity.
- [x] Improve windowed demo models, fullscreen coverage, dynamic game controls, and clipped consistent HUD/log overlays.
- [x] GPU proof + pacing fix: adapter log (GTX 1650 / Vulkan / DiscreteGpu), removed budget-gate-vs-vsync double throttle (29→80fps wall), Immediate present for uncapped `fps: 0`, richer `PRESENT_PROFILE`.
- [x] Build-cost optimization: rain layer cached in `StaticLayers`; `stamp_art` decodes art once + probes by index (warm build 10.6→6.1ms).
- [x] Compose + instance optimization: still-cell fast path, index-based `apply_glow`, pre-sized instance buffer (debug frame ~15.5→~12ms).
- [x] Threads + LUTs: persistent 3-worker band pool for `frame_instances` (per-frame spawn regressed release, reverted to parked pool), glyph-slot LUT, single-sin Wave sampling.
- [x] Depth-buffered 3D-to-ASCII backend (`engine/src/raster.rs`, asciicker-inspired, original code): 2x supersampled z-buffer, face-normal diffuse, depth-edge glyphs, fog; humanoid/slab builders; `depth` example.
- [x] Mesh transforms: `Transform` + baked `Mesh::transformed`/`transformed_about` (joint pivots); model turntable in `depth` example.
- [x] OBJ model loading: `raster::load_obj` (v/f subset, fan triangulation, numbered errors) + bundled `sword.obj` + `model` example.
- [x] Windowed raster viewer (`viewer` example, 96x35, resize-proof focal, FPS readout) + character-clarity pass (rim light, ambient 0.22, dark slab, 2x figure-vs-ground contract).
- [ ] TCP windowed client (`--connect`), live selection/boss-event wiring (still on hold pending user call).
- [ ] Push `main` to `origin/main` (ahead by 5) ONLY when explicitly asked.

## Widened directional contrast slice (2026-10-17)

Request: "Use the writeups to improve the rendering for the TUI even more" (links to Alex Harri's ascii-rendering post and kciter's ASCII 3D renderer post).

Cross-check against what already existed (prior slices): 4x SSAA supersampling, six-region internal sampling vector via SSAA taps, global normalize-then-pow contrast (`contrast_power = 1.22`), stroke-vs-texture gating, depth-edge glyphs, extended ramps, kciter's full 3D pipeline — all already implemented. The unexploited Harri technique was the **widened directional contrast enhancement**: at each sampling-vector component, before the global pass, normalize against the max of ALL *affecting external regions* (his `AFFECTING_EXTERNAL_INDICES`), not just the one texel straight outward. The narrowed pass was exactly Harri's "doesn't reach far enough" failure mode — external light stopped at one component and residual staircasing survived.

Implemented (`client/src/gpu/shaders.rs`, `cs_main` + `shape_glyph`):
- New per-cell external texels: left/right edge segments at the top and bottom rows (`ext_l_top/ext_l_bot/ext_r_top/ext_r_bot`) and the four outward corners (`ext_corner_ul/ur/ll/lr`) — 8 more color-texture loads/cell (~+20% loads, negligible).
- Per-component max-of-affecting externals `e0c..e5c` (both top circles reach both top components, the left/right edge columns span that whole side, corners give diagonal reach), mirroring Harri's widened table; `shape_glyph` signature unchanged (it now receives the combined externals).
- `dir_power = 2.0` unchanged (Harri's upper range); No unvalidated retuning. kciter's post adds no missing techniques for the TUI path (its pipeline/ramps already existed) — recorded for the audit trail.

Verification (2026-10-17, Windows, GTX 1650): fmt clean; workspace tests 0 failed (client 82 lib); tools content ok (11); CI-order clippy clean; `--snippet` shader test extended (widened-`e4c` + corner assertions); fresh `--wgpu-demo 1 156x44` width gate still all-rows-exactly-156/zero-wide; dump diff vs prior build: 8 glyph transitions concentrated at boundary rows 10–27 / 7 columns (static rainy scene; expect the visible gain live on moving edges); performance 4.20 → 4.22 ms/frame at 156x44; `--demo` smoke exit 0.

Changed this slice: `client/src/gpu/shaders.rs`, `AGENTS.md` (GPU bullet: Harri-technique provenance), this handoff.

Next action: user live visual verdict at 156x44 (`play_ansi.bat`) — boundary lines between regions/actors should sit a cell crisper in motion; everything else this day unchanged. Then next bounded slice from the checklist (CPU renderer scene-partition adoption). Do not commit or push unless asked.

## 3D OSRS-mode windowed game (2026-10-18, latest — READ FIRST next session)

Request: "make this one like our tui game, basically make it have runescape controls and all that depth etc, and inventory etc" — upgrade `engine/examples/play.rs` from the flat 2.5D bridge look to the depth-buffered 3D raster backend with OSRS controls.

Implemented:
- **Engine additions (dependency-free):** `raster::` gained `Vec3` public `dot`/`cross`/`norm` + `std::ops::Sub` (so scene authors can pose cameras/projectors against the same math), `Mesh::quad_y` (flat 2-tri ground/Ocean sheet), `Mesh::decal_y` (yaw-rotated flat ground decal for FX dashes), `Mesh::retinted(from,to)` (per-material repaint for mob palettes). `present::` gained `SceneSource::pointer(PointerEvent{held, delta, wheel})` wired in the winit event loop (MouseInput edges slots [left,right], CursorMoved physical deltas, MouseWheel line/pixel ticks) + `PointerTrack` unit tests. `present.rs` also handles `WindowEvent` mouse arms; Esc/Q/`--frames` unchanged.
- **`engine/examples/play.rs` rewritten (3D-first, bridge still owns tests):** per frame builds a mesh list — 14-unit zoned meadow quads (dim saturated green palette per quiet-carpet contract) windowed around the player, southern ocean + beach sheets south of `WORLD_BOUNDS.max_y` (35), 9 hand-placed box trees (trunk+canopy), item drops as small gold boxes, and `Mesh::humanoid()` actors scaled 0.62/0.55 from the foot anchor with `facing` yaw (updates on movement steps; moss-tinted mob remaps, rust-red cloth for the NPC) — then ONE `raster::render_layer` carve into the world partition plus a screen-space `labels` layer (name + `▮▯` HP mini-bar per actor via a local `project(p, cam)` replicating the fill projection; **damage digits** lifted/rising from observed `combat_fx`).
- **OSRS camera + controls (presentation-only state in `LocalGame`):** orbit yaw initialized to π (looking north), pitch −0.62, dist 17, `CAM_FOCAL = 34` (≈2 cells/unit so the 192-col view spans ~96 world cells like the bridge); right-drag orbits yaw/pitch (clamped −1.05..−0.3), wheel dollies 9..30; `camera_axes()` maps WASD through the ground-projected camera basis (screen-up = camera forward) into `KinematicMotion::update`, so movement is camera-relative and snaps to the 8 compass steps exactly like the TUI; camera pos = `aim − fwd·dist` glides toward the smoothed player anchor. Per-entity render positions smooth toward snapshot cells (snap >2.5 = teleport). **Known consistent-mirror quirk:** the raster basis has east on screen-right when facing north (chirality flip); WASD/drag/facing are all basis-relative end-to-end so steering feels correct on screen, but the top-down compass flips east-west — noted, not a bug to "fix" without a portrait reference.
- **Combat FX re-expressed in meshes:** telegraph = 12 tangential `decal_y` dashes sweeping inward at `fx.radius`, burst = white-hot core box (mix 0.65) + 10-dash expanding ground ring + 6 rising embers — same pack #2/#3 timeline keys (`bridge::SceneFx`, `FX_CORE_S`/`FX_BURST_S`), same message watcher code as before. Damage digits ride the labels layer.
- **HUD/log/modal:** dark slate HUD band (zone/HP/tick, legend incl. `[I] inventory` + `[Right-drag] orbit` + `[Wheel] zoom`, integer `█░` HP bar), 3-line log strip, and an `I`-toggled centered gold-bordered "Inventory & Equipment" modal partition mirroring the TUI's fixture content (Equipped Gear + Pouch columns; data-driven gear remains a later slice). All partitions pinned (`camera: false`); the 3D camera pans inside the raster pass. `--check` rewritten: F attack damages the mossling + spawns FX + a digit lands in the label layer + 3 closed partitions; held-D walks (world-west under yaw π) in 40 frames; `I` toggles a 4th modal partition on and back off; `--frames N` still exits after N frames.

Verification (2026-10-18, Windows, GTX 1650): fmt 0; workspace tests 0 failed (client 95, engine lib 100 + tools/server/shared/bot unchanged); engine `--features present` lib tests 112 all pass (+3 raster: quad/decal tri-counts + yaw long-axis + retint isolation, +1 present PointerTrack); tools content ok 11; clippy `-D warnings` clean across workspace AND `--features present --all-targets` (Vec3 now has `ops::Sub`, no inherent `.sub`; `u64::from` no-ops cleaned); `play -- --check` green; `play -- --frames 30` presented 30 frames to the real Vulkan window cleanly. Not yet visually judged by a human — user verdict pending (see next action).

Run it:
1. `cargo run -p ascii-mmorpg-engine --locked --features present --example play` — WASD/arrows walk relative to the camera, hold **right mouse + drag** = orbit (yaw + pitch), **wheel** zoom, **F** attack (ember burst + rising digit at the target), **1/2/3** cast (dashed telegraphs sweep in, school-hued), **I** inventory panel, **R** rest, **Q**/Esc quit.
2. `--frames N` = automated window proof; `--check` = headless self-test (no window).
3. Vulkan-only as before; the old 2.5D bridge path remains fully intact for `bridge`/`game`/tests.

Deferred (honest notes): no road strip, no rain/weather in 3D yet, NPC/item detail is flat boxes, mobs interpolate but keep 20 Hz distance-independent steps for non-player actors behind the camera (player smooths), no minimap/map modal, inventory is a fixture, TCP-connect still pending from the earlier queue.

Changed this slice: `engine/src/raster.rs` (Vec3 pub ops + Sub ops impl, `quad_y`/`decal_y`/`retinted` + 3 tests), `engine/src/present.rs` (`PointerEvent` + `SceneSource::pointer` + App wire + 1 test), `engine/examples/play.rs` (full rewrite: 3D world builder, orbit camera, camera-relative input, labels/digits, HUD/log, inventory modal, rewritten `--check`), `AGENTS.md` engine bullet, this handoff.

Next action (next session): user LIVE verdict on the 3D window (orbit feel/drag sign, zoom, walk feel, FX readability, plate/digit overlay, inventory panel) and the still-pending TUI verdict at 156x44; fix the drag sign or camera/focal tuning if it feels wrong; then the REPORT.md queue (#4 lightning column, #5 icicle sweep, #6 fog banks, #7 organic trees/banners/torches) or CPU `scene_partition_dims` adoption. Do not commit or push unless asked.

## High-density glyph and world-detail slice (2026-10-18, latest)

Request: replace the remaining narrow ramp-like 3D ASCII output with a
high-density, geometry-aware colored glyph system while preserving the pure
black world viewport, depth/occlusion, dark-fantasy palette, camera, controls,
animation, gameplay, and performance.

Implemented in the depth-raster play path:

- **4x4 native-cell supersampling.** `engine/src/raster.rs` now resolves a
  `SAMPLE_GRID × SAMPLE_GRID` depth/shading block per terminal cell instead of
  the old 2x2 block. Each winning sample carries shaded color, luminance
  variance, material identity, and view-space face normal; perspective depth
  and near-wins remain unchanged.
- **Broad printable ASCII vocabulary.** Generic, foliage, bark, earth, stone,
  water, fire, magic, and metal families use stable printable ASCII families
  rather than one global ` .:-=+*#%@` ramp. Edge strokes select material-aware
  variants (`|!iIl1`, `-_=+~`, `/\\xX*`, etc.) instead of always emitting the
  same four characters.
- **Geometry-aware and distance-aware selection.** Cell coverage, averaged
  view normal, supersample luminance variance, fog distance, and a spatial
  hash choose density/detail. Close partial silhouettes get fine directional
  ink; distant cells suppress jitter and simplify deterministically. No
  time-varying random glyph noise was introduced.
- **Model enrichment.** `humanoid_pose` now has visor, chest heraldry, and crest
  planes; mosslings have bark spots and brow/teeth detail; skeletons have
  horizontal rib bars; herbalists have robe/hood bands. `play.rs` adds a third
  pine canopy tier, stable root decals, split grass blades, road cobbles, gate
  keystone/rubble, and additional foreground stone detail.
- **Material correction.** Bright warm surfaces such as sand are classified as
  earth instead of bark, keeping their glyph language distinct from dark
  red-brown timber.
- **Regression coverage.** Added tests for broad printable glyph coverage,
  deterministic material-aware selection, black world backgrounds, occlusion,
  fog monotonicity, point-light foreground-only behavior, and 4x raster output.

Verification for this slice (Windows, NVIDIA GeForce MX230/Vulkan):

- `cargo fmt --all`
- `cargo test -p ascii-mmorpg-engine --features present --lib` — 124 passed
- `cargo test --workspace` — all workspace tests passed (client 105, engine 112, server 8 + 3, shared 14, tools 2, bot 1)
- `cargo fmt --all -- --check` — passed
- `cargo clippy --workspace --all-targets -- -D warnings` — passed
- `cargo clippy -p ascii-mmorpg-engine --features present --all-targets -- -D warnings` — passed
- `cargo run -p ascii-mmorpg-tools -- content` — `content ok: 11 definitions`
- `cargo run -p ascii-mmorpg-engine --example depth -- --check` — passed
- `cargo run -p ascii-mmorpg-engine --example depth` — emitted dense turntable
  rows using the expanded glyph families
- `cargo run -p ascii-mmorpg-engine --locked --features present --example play -- --check` — passed
- `cargo run -p ascii-mmorpg-engine --locked --features present --example play -- --frames 60` — 60 Vulkan frames presented
- Captured current window comparisons: `C:\Users\kladd\AppData\Local\Temp\omnirush\reco-analysis\ascii_high_density_latest.png` (default), `ascii_high_density_zoom.png` (wheel-zoom close view), and `ascii_high_density_orbit.png` (right-drag orbit). They show a black world viewport with distributed terrain ink, dense tree facets, readable close actor models, labels, and foreground-only color.
- `git diff --check` — passed; only existing CRLF conversion warnings appeared

Still required:

- Full workspace CI-order checks and present-feature clippy after this slice.
- Human review is now complete for the captured default, close zoom, and orbit
  views. The current remaining visual follow-up is an optional wider/zoomed-out
  comparison against the supplied reference video.
- `ffprobe` remains unavailable for the reference-video pipeline; use the
  existing OpenCV analysis and screenshots unless the tool is installed.

Changed for this slice: `engine/src/raster.rs`, `engine/examples/play.rs`, and
this handoff. The earlier intentional changes in the client/GPU, bridge,
compose, presenter, and `play_ansi.bat` remain in the worktree; the launcher
modification is pre-existing and untouched.

Next action: run the full required checks, then perform the live visual density
review. If the close view reads as texture soup, narrow the family jitter and
reserve structural glyphs for actual edges/normals before adding more geometry.
Do not commit or push unless asked.

## Combat FX in the windowed engine renderer (2026-10-17)

Request: "implement all of this in the custom rust renderer and tell me how to run it" — the clarity/FX packs ported to the engine's windowed presenter path.

Implemented:
- **`engine/src/bridge.rs`** gained the presentation passes (pure, sampled from `SceneView`): `stage_pass` (dark opaque floor patch `STAGE_BG (7,6,5)` within r≈3.3 of every actor, ragged outer shell via `hash2(…,997)`, z0 pushed straight after the cached ground so it draws over ground but under props/actors, offset-tracked for glide), `target_ring_pass` (`Option<Layer>` marching dashed orange `=` ellipse rx 1.9/ry 1.15 under `melee_target` — first living Monster within Manhattan `MELEE_RANGE=6` of the player, mirroring `command::Attack`; `None` layers skip), `fx_layer_pass` (samples `view.combat_fx` at `view.presentation_time_s`: telegraph dashes via the new `fx::Effect::Implosion` (shrinks `radius*(1-age/lifetime)` + swirls `270°/s`, tangent-oriented `-`/`|` glyphs), then the burst — white-hot core `'█'` hand-stamped for `FX_CORE_S=0.18` fading to 0, `Ring{8, radius}` `'o'` glow 0.85 over `FX_BURST_S=0.45`, `Burst{14, 3.2/s}` embers `'•'` from +0.06 s, and `Number{rise 1.1}` damage digits). `SceneView` gained `combat_fx: Vec<SceneFx>` + `presentation_time_s: f32`; world partition layers now include `stage`/`target-ring`/`fx`. FX consts are `pub` (`FX_CORE_S`, `FX_BURST_S`) so callers prune consistently.
- **`engine/src/fx.rs`**: new `Effect::Implosion { count, radius }` (pure, deterministic, dash glyphs, tested for tightening + 90° swirl landing).
- **`engine/examples/play.rs`**: `LocalGame` now holds `combat_fx` + `seen_combat_events` (cap 64/16, expire at `telegraph + FX_BURST_S + FX_CORE_S`) and translates sim messages exactly like the TUI GPU renderer (`You hit {name} for {n} damage` → ember burst at the named entity *with rising digit text*; `You cast {spell}` → caster-anchored telegraph: Cleave amber 2.0/0.7 s, Iron-Will cyan 1.8/0.6 s, Thunder-Mark blue 3.0/0.9 s, unknown neutral). Keys **1/2/3** push the three casts; HUD legend shows `[1-3] cast`. `run_check` extended: after an F attack the built scene must have non-empty stage + fx layers and (while the mossling lives) a ≥4-dash target ring.
- The terminal TUI renderer already had all of this (thirteenth slice); this slice is the same presentation language for the windowed engine path.

Verification (2026-10-17, Windows, GTX 1650): fmt 0; engine lib tests 97 (+4: stage coverage/opacity, ring dash hug + absence out of range, telegraph→core→ring timeline, amber digit rise); fx 6 (+2 implosion); workspace tests 0 failed (client 95 unchanged); tools content ok 11; clippy `-D warnings` 0 across workspace AND `--features present --all-targets`; `play -- --check` green (attack FX ink + stage stamp + target ring asserted in the real sim loop); `play -- --frames 30` presented 30 frames to the real window cleanly.

Run it:
1. `cargo run -p ascii-mmorpg-engine --locked --features present --example play` — windowed world (WASD/arrows move, hold to glide; **F** melee attack → amber core→ring→ember burst + rising damage digit under the mossling; **1/2/3** cast cleave / iron-will / thunder-mark → dashed telegraph sweeps inward then bursts in the school's hue; **R** rest; **Q**/Esc quit).
2. `play_args`: `--frames N` exits after N presented frames (automated proof); `--check` runs the headless movement/combat/FX self-test.
3. Note: Vulkan-only instance; if a window refuses to open on Windows try updating the GPU driver. Native ASCII/terminal path is unchanged (`play_ansi.bat`).

Changed this slice: `engine/src/bridge.rs` (+stage/ring/fx passes, SceneFx, SceneView fields, partition wiring, HUD legend, 4 tests), `engine/src/fx.rs` (+Implosion +2 tests), `engine/examples/play.rs` (FX watcher + cast keys + check assertions), `AGENTS.md` engine bullet, this handoff.

Next action (next session): user live verdict on the windowed renderer (F burst + 1/2/3 telegraphs) AND the still-pending terminal verdict (attach FX from the thirteenth slice at 156x44). Remaining REPORT.md queue: #4 lightning column, #5 icicle sweep, #6 fog banks, #7 organic trees/banners/torches; small carried item: adopt `ui::scene_partition_dims` in the CPU renderer. Do not commit or push unless asked.

## Combat FX pack #2+#3 (2026-10-17)

Request: "continue" — proceed to the queued combat-readability ports (`video-tools/REPORT.md` items #2 impact/ember FX + #3 AoE telegraphs).

Implemented (GPU path only):
- **Message-observed FX.** `GpuRenderer` now carries `combat_fx: Vec<CombatFx>` + `seen_combat_events` dedup ring (64). Every realtime frame scans `world.messages` presentation-side (server authority untouched): `You hit {name} for {n} damage.` → single-hue amber-ember burst at the named entity's world position (×0.8 mesh scale); `You cast {spell}; …` → telegraph-then-impact at the caster (smoothed player focus). Spell-school hues: Cleave amber arc, Iron Will cyan spirit, Thunder Mark blue lightning (AoE radius 1.9), unknown spells neutral gray. Non-combat lines parse to `None` (unit-tested, incl. unknown-target names).
- **Add_impact_burst / add_ground_ring (`client/src/gpu/mesh.rs`).** `add_impact_burst(center, progress 0..1, color, scale)` plays the reference timeline: white-hot core (mix 0.65 toward white, grows) → expanding dashed ground ring (radius `scale*(0.25+1.15*ease)`, fades) → six ember shards rising `dy ≤ ~0.9` + shrinking/fading (progress > 0.18). `add_ground_ring(center, radius, color, phase, dash_count 3..16)` = flat emissive dashed ellipse (the telegraph: dashes sweep inward 25% + rotate `phase=1.5*progress` during wind-up, then the burst resolves at the same anchor). Both stamp material 4 (emissive ×1.5, raw albedo) — caller must `set_entity_context(FX_ENTITY_ID=254, class)` first; **class 0 on purpose**: the post pass tints every edge cell 60% toward the class color and FX quads are almost all edge cells, so class 3 swamped authored hues (found in dump verification).
- **Lifetime/timeline.** `FX_BURST_SECONDS=0.45`, `FX_TELEGRAPH_SECONDS=0.9` (Iron Will 0.6); total = telegraph + 0.45; pruned by `animation_time - born`; capped 16 concurrent. All timing keys off `animation_time` (realtime: elapsed clock; deterministic path = `frame*0.05`), and the deterministic path SKIPS observation entirely so demo/preview mesh output stays a pure function of the snapshot (`stamp_combat_fx` still runs, so pre-seeded FX could be tested deterministically).
- **Placement caveat (presentation-only):** melee FX anchor at the entity's raw grid position (duplicated mob names → first match); casts anchor at the smoothed player focus. FX cells are nonzero entity ink, so they ALSO open stage clearance around themselves — merged into the actors' black arena, consistent with the clarity look.
- Honest scoping (#3): the reference draws dashes pre-hit for MOB AoE; our mobs have none, so the telegraph carries the client's only client-known pre-hit event — the player's own queued cast. True mob-AoE telegraphs need server gameplay to observe; deferred. Melee "mark the spot" already landed via the clarity pack's dashed target ring.

Verification (2026-10-17, Windows, GTX 1650): CI order all green — fmt 0, workspace tests 0 failed (client 95 lib, +3: `combat_messages_translate_into_fx_specs`, `combat_fx_spawn_once_and_expire_from_snapshot_messages` — GPU test asserting spawn/dedup/expiry + emissive FX-ink vertices near the target — and `impact_burst_follows_core_then_ring_then_ember_timeline` — mesh crime-scene test: all FX verts material 4 + entity 254, embers rise > core top + 0.4, ring dashes sit in the radius band), tools content ok 11, clippy `-D warnings` 0. Visual: rendered a hit + a Thunder-cast world through `render_frame` at 156×44 (temporary `#[ignore]` test, removed after), dumped via `dump_to_png.py` — frame 1 shows the amber ember ring scattered around both actors on the black stage, telegraph frames show pale blue-white dashes around the caster, clearly distinct hue (PNGs `fxhit1/fxcast1/fxcast3_view.png` in `%TEMP%\opencode\`). FPS cost negligible (≤ ~15 small quads per active FX).

Changed this slice: `client/src/gpu/mesh.rs` (add_ground_ring + add_impact_burst), `client/src/gpu/renderer.rs` (CombatFx/FX consts + process/stamp/fx_from_message + 3 tests), `video-tools/REPORT.md` (port-list status: #2 shipped, #3 shipped-with-scoping-note), `AGENTS.md` (combat-FX bullet), this handoff.

Next action (next session): user live verdict at 156x44 (`play_ansi.bat`) — carried checks PLUS this slice: press `F` on a melee mob → amber core→ring→ember burst (~0.45 s) at the target's feet over the black stage; cast a spell → colored dashes sweep inward at the player then resolve into a burst; hue per spell school (Cleave amber / Iron Will cyan / Thunder Mark blue). Watch: bursts over the road (emissive on bright albedo), FX overlapping plates at scene edges, and that repeated attacks don't stack more than one burst per message. Remaining queue from REPORT.md: #4 lightning column, #5 icicle sweep, #6 fog banks, #7 organic trees/banners/torches (re-check overlap with existing mesh systems before porting each); still pending: adopt `ui::scene_partition_dims` in the CPU renderer. Do not commit or push unless asked.

## Character-clarity pack A+B+C+D (2026-10-17)

Request: "Ok implement all these mechanisms" — the four clarity-study ports at once (A nameplates, B stage clearance, C target ring/digits, D carpet hue identity).

Implemented:
- **A + C — `ui.rs` + `gpu/renderer.rs`.** New `ui::SceneLabel<'a> { col, feet_row, head_row, name, hp, is_player, is_target }`; `ComposedScene` gained `labels: &'a [SceneLabel<'a>]` (CPU path passes `&[]` — it has no camera projection). `gpu/renderer.rs compute_scene_labels` (static, unit-tested) projects feet (y=0.1) and head (Item 0.6 / Monster 2.0 / Player-Npc 2.35) with the frame's `view_proj` (NDC → cell: `(ndc*0.5+0.5)*scene_w`, `(0.5-ndc.y*0.5)*scene_h`, margin 1.05, w<=0 culled); the player uses the smoothed render position. Target rule = same as `Command::Attack`: nearest alive Monster in melee range (manhattan min). `draw_scene_labels` draws the dashed orange `=` ring (28 samples, step 2, rx 3.5/ry 1.15 at feet+1) under non-player plates, then plates (black bg, white name, player bold; combatants get `[██████] 8/8` red mini-bar + digits row), player plate last. Both render paths (`render_frame`, `render_batch`) + `format_frame_with_hud` pass labels through.
- **B — shaders.rs stage clearance.** In `cs_main` after the entity-edge block: metadata entity channel scanned at 1 texel/cell (stride 4) over dx,dy in ±3; ground cells (`entity_id == 0u`) within an actor emit `glyph = 32u` (pure space), outer shell (dist == 3) dithers `hash2(x, y) % 2u` for a ragged arena edge; rain/props/terrain (entity 0 everywhere) never open a stage; actor surfaces + outlines untouched. Test-pinned `actors_stand_on_a_locally_cleared_stage`.
- **D — mesh.rs hue identity.** Same-luma raised-chroma albedos: meadow [0.012,0.072,0.028] (pinned test updated), blades [0.02,0.105,0.035], pine dark/mid/light [0.03,0.135,0.05]/[0.05,0.165,0.065]/[0.07,0.205,0.08], trunks [0.28,0.155,0.055]/[0.185,0.095,0.035], road [0.155,0.18,0.245]/[0.13,0.145,0.20], border [0.235,0.205,0.145]. Quiet-ground contract (`g <= 0.15` material 3) still test-enforced.

Bugs found and fixed on the way:
- **Nameplate centering**: name row was drawn starting AT the anchor (chars at `center_x+1..+len`) while the HP row centered — names sat up to width/2 right of the actor. Name now centers inside the plate width; plate centers on the feet anchor (verified: "Mossling" plate + `[██████] 8/8` + orange ring all center on col 71 = the Mossling's projected feet).
- **dump_to_png.py (pre-existing tool bugs)**: (1) painted every cell with fg ink including spaces → plates read as solid white bars in verification PNGs; now tracks bg (48;2 + palette 40/49) and paints space cells with bg, matching a real terminal. (2) wrote RGB tuples through `cv2.imwrite` (BGR) → ALL prior measurement PNGs had red/blue channels swapped (saturation/luma metrics unaffected; hue-based ones were hue-mirrored). Both fixed; re-derive any cached hue conclusions from post-fix renders. Cargoside untouched.

Verification (2026-10-17, Windows, GTX 1650): CI order all green — fmt 0, workspace tests 0 failed (client 92 lib, +2: `scene_labels_draw_plates_hpdigits_and_target_ring`, `scene_labels_project_onto_actor_cells` — camera geometry computed exactly via a hand-rolled projection to avoid iterated guessing), tools content ok 11, clippy `-D warnings` 0. Release `--wgpu-demo 30 156x44`: plates land ("Adventurer"/"Mossling" white-on-black above heads, red 8/8 bar, orange dashed ring on the melee target's feet, black stage with ragged dithered edge around both actors — visually verified via dump PNG crops `plates_zoom4/5.png` in `%TEMP%\opencode\`). Metrics (scene-cropped): black% 0.5%→4.1% (reference band 1.6–52%), plate% 0→0.29, carpet-S p75 102→128, mean 74→87, S≥100 share 29%→47%, p50 luma 38 (reference median 20–33 — the stage accounts for the new black; punctuation still quiet). Carpet-S p50 stays 85 (32-level channel quantizer floor) — honest partial vs reference 121–156 median. Perf 2.14 ms/frame (extra 49 metadata taps negligible).

Changed this slice: `client/src/ui.rs` (SceneLabel + draw pass + ComposedScene field + test), `client/src/gpu/renderer.rs` (compute_scene_labels + both render paths + test), `client/src/gpu/shaders.rs` (stage clearance + test), `client/src/gpu/mesh.rs` (hue-identity palettes + pinned test update), `client/src/renderer.rs` (CPU labels `&[]`), `video-tools/dump_to_png.py` (bg-aware + RGB fix), `video-tools/REPORT.md` (implementation status), `AGENTS.md` (clarity-pack bullet), this handoff.

Next action (next session): user live verdict at 156x44 (`play_ansi.bat`) — carried checks PLUS: every actor floats a black plate with its name, monsters show `[██████] hp/max`, the in-melee monster has an orange dashed ground ring, actors stand on a locally black quiet stage while walking, plates track smoothly in motion (watch: plate jitter while the player glides, ring/plate overlap at scene edges, stage hard-edgedness over the road). Then port item #2 impact/ember FX + #3 AoE telegraph rings (combat-readability pack). Do not commit or push unless asked.

## Character-clarity study (2026-10-17)

Request: "From the studied video frames find out how we can make our characters as clear and distinct as that game — they didn't have high resolution but everything was still clear and distinct."

Method: 8 representative frames across all reference content (spell idle/wind-up, boss, village, multiplayer combat, dungeon), measured with the new `video-tools/clearness_study.py` (luma bands, blob-vs-local-ring contrast, hue dominance, white-text-on-black backing detection); our `--wgpu-demo` frame rendered to PNG via the new `video-tools/dump_to_png.py` (8×16 cell rects) and measured with the same script (scene-only crop, cols <124/rows 3–40 convention).

Measured gap table (reference 8 frames vs ours): p50 luma 4–22 vs 41.5; pure-black 1.6–52% vs 0.5%; carpet saturation 121–156 vs 85; readable actor-vs-ring dL +60…+145 vs +80 (ours already passes!); player hue1 0.48 vs reference 0.55–0.96 (torch pool 1.00); white text on black backing 45–62% in world views vs none.

Answers ("how"):
1. **Local quiet + strong local delta is the principle, not actor brightness.** Dungeon frame: near-black floor, DARK actor silhouette, identity carried by a white label; village frame: bright saturated actor over dim carpet. Both modes measured.
2. **Stage/negative space**: reference floors around characters are pure black or uniform — up to 52% pure black; ours fills 100% of cells with punctuation right up to sprite edges.
3. **Dark-backed floating nameplates** on every actor (name on a solid near-black plate + HP digits/bar; dashed ellipse marker under the current target; thin red boss bar with name inside). The plates carry readability in the densest frames. We render no in-world labels (names only in the side panel).
4. **Two-palette system**: carpet dark AND saturated (deep navy/teal S 121–156), actors bright-or-desaturated-white (single-hue for FX). Ours is grey-green S 85 and split-hue player.
5. **In-world state glyphs**: HP digits by monsters, subtitle lines, target ring.

To do from this (ranked by evidence): A dark-backed floating nameplates + HP via `ui.rs` blit + camera projection (we have `camera_state`; M size); B ground quiet-radius (~2.5 cells, entity mask in compute pass, S–M); C target ellipse + digits + boss bar (reuses item #3 dash language); D carpet hue identity (deep saturated darks, same luma, `mesh.rs`, S). A+B is the recommended next slice — it directly answers this question; port items #2/#3 remain queued after.

User question, answered 2026-10-17: would the clarity pack need the Rust engine presenter or is it TUI-possible? Verdict: **TUI-native, all of it** — the reference captures themselves are terminal output; plates/labels/rings/boss bar are just composed-buffer cells, quiet-radius is scene content, palette identity is material colors. The engine presenter only buys sub-cell positioning, per-partition fonts, `Cell::scale`/gradient/glow techniques, and no-ConPTY arbitrary grids — useful later for showcase/smooth-skeletal goals, not needed for A–D.

Verification this turn: 8 ref frames + ours_scene.png analyzed (numbers above in REPORT.md "Character-clarity study"); full dump with HUD excluded; no game code changed; fmt/tests/clippy untouched by study (python only).

Changed this slice: `video-tools/clearness_study.py` (new), `video-tools/dump_to_png.py` (new), `video-tools/REPORT.md` (new section), this handoff.

Next action (next session): user picks/Approves A+B (nameplates + stage-clearance) as the next bounded slice, or #2/#3 from the earlier plan. Do not commit or push unless asked.

## Character-model shading + entity-metadata zero fix (2026-10-17)

Request: use the two ASCII-renderer articles' lighting/shading methods (Alex Harri "ASCII characters are not pixels"; kciter "Building an ASCII 3D Renderer") to improve character-model clarity and definition.

Implemented (entity fragments only — `in.entity_id > 0.5` — so terrain/props keep the untouched dark-carpet lighting):
- **Cel-quantized sun term** (Harri's cel-shading separation + kciter's discrete Shade ramp): `sun_ndl = select(NdotL, floor(NdotL * 4.0) / 4.0, entity>0.5)` — body facets resolve as distinct lit/shadow bands instead of a mushy gradient at terminal resolution.
- **kciter headlight** ("the light comes from the direction you are looking"): `+ (1.0, 0.95, 0.85) * dot(N,V)^2 * 0.30` on camera-facing surfaces (chest, face) so they never collapse into the shadow band.
- **Rim light**: `(0.85, 0.90, 1.0) * (1 - dot(N,V))^3 * 0.35` lifts the grazing silhouette off the dark ground.

**Latent bug found and fixed along the way** (the metadata gate initially lit the whole landscape): `MeshBuffer::set_entity_context` mapped entity id 0 — the reserved "no entity" context used for terrain/props/water (renderer.rs `set_entity_context(0, 0)` before terrain and props) — through the 1-based wrap `((id-1) % 255) + 1`, which folds 0 → **1**. Every terrain/prop vertex carried entity_id 1, so any `entity_id > 0` shader gate fired on the whole scene. Evidence chain: red-tint probe showed R-only channel deltas `(16,0,0)/(8,0,0)/(24,0,0)` on ~3700 landscape cells (uniform warm deltas alone were misleading — decode per-channel); gates disabled (`> 1000.0`) → 0 changed cells vs baseline; fixed mapping → 87 color-changed cells all in rows 17–24 (the player band). Fix: special-case id 0 → 0 in `set_entity_context`; regression test `zero_entity_context_maps_to_metadata_zero` (also pins the 1→1 and 256→1 wraps). Bonus: entity metadata now genuinely distinguishes actors from ground, so entity silhouettes/edge metadata register correctly against terrain.

Verification (2026-10-17, Windows, GTX 1650): fmt clean (1 wrapped assertion reformatted); workspace tests 0 failed (client 89 lib — +1 scalar test, +1 shader test `character_models_shade_with_cel_headlight_and_rim`); tools content ok (11); clippy `--workspace --all-targets -D warnings` exit 0; release built; probe dumps in `%TEMP%\opencode\` (`gate_off.bin` baseline, `probe_red.bin`, `char_fix.bin`). Actor-band (rows 14–23, cols 40–90): p90 99→108, max 185→187, median unchanged (ground dominates). Same-binary run-to-run diff: 2–3 cells (known tie-flip); 222 total changed cells vs baseline are glyph decisions from the corrected metadata (spurious terrain-vs-sky horizon id-edges gone) + actor-only colors — terrain color untouched.

Changed this slice: `client/src/gpu/shaders.rs` (fs_main character-shading block + test), `client/src/gpu/mesh.rs` (`set_entity_context` id-0 fix + test), this handoff.

Next action (next session): user live verdict at 156x44 (`play_ansi.bat`) — all carried checks (quiet ground, ocean, orbit-facing, borders, Minimap, GRID, crispness) plus this slice: the player should read with stepped lit/shadow facets, a lit chest/face, a faint rim, and a (new) correct outline against the ground; check for over-bright or tinted artifacts while walking. Then port item #2 (impact/ember FX) + #3 (AoE telegraph rings) from `video-tools/REPORT.md`. Do not commit or push unless asked.

## Quiet-ground / actor-bright contrast slice (2026-10-17)

Request: implement port-list item #1 from `video-tools/REPORT.md` — quiet dim ground carpet with bright actors (reference recipe: median frame luma 20–33/255).

Method: measured, not guessed. Decoded per-cell ANSI fg colors from `--wgpu-demo 1 156x44` dumps, restricted to the scene partition (cols < 124 — the HUD side panel was skewing earlier stats), classified glyph families, and compared dumps before/after each change (baseline `ocean_3.bin`; helpers in `%TEMP%\opencode\{scene_stats,luma_map,glyph_luma,diff_dumps}.py`).

Root causes found (three stacked brightness sources):
1. Meadow albedo too bright: base [0.18,0.38,0.19] lit to fg luma ~70/255 → bands 3–4 letter/mid-mark soup ('y','v','w','*','+','=').
2. Grass blade field: per-cell blades (seed%2, ±18/±14) put edge-stroke glyphs ('|','/','\\','!') on ~23% of the frame; blades also face +Z (toward camera/sun) so they glowed brighter than the carpet beneath them.
3. Pines + road: pine canopy greens and the cobble road kept large screen areas in bands 3–5 ('=' '+' '*' 'o' storms).

Changes:
- `client/src/gpu/mesh.rs`: meadow base → [0.03, 0.07, 0.035]; grass blades → [0.04, 0.10, 0.05]; pine greens (both builders incl. the instanced prototype) dark/mid/light → [0.05,0.13,0.07]/[0.07,0.16,0.09]/[0.09,0.20,0.11]; trunks → [0.26,0.16,0.08]/[0.17,0.10,0.05]; cobble road → [0.17,0.18,0.20]/[0.14,0.15,0.17], border → [0.21,0.20,0.18]. Sand/water palettes untouched.
- `client/src/gpu/renderer.rs`: blade field thinned seed%2 → seed%5 (~20% coverage — sparse swaying tufts; verified visually negligible: ~39 stroke cells at the spawn view).
- `client/src/gpu/shaders.rs`: material-3 band 2 wind-lean 'y' → ';' (letters out of the ground mid-tones; bands ≥3 keep v/y/w for lit foliage).
- Test: `southern_ocean_starts_exactly_at_the_world_boundary` now pins the dim meadow color AND enforces a quiet-ground bound (material-3 vertices g ≤ 0.15; sand/road exempt) so this contract cannot silently regress.

Verification (2026-10-17, Windows, GTX 1650): fmt clean; workspace tests 0 failed (client 87 lib); tools content ok (11); CI-order clippy clean; release build; `--demo` exit 0; `--wgpu-demo` GRID 156x44. Scene-only fg stats: median 71→33/255 (reference range 20–33 ✓), p90 97→50, letters 15%→<1%, fine punctuation 8%→42% (now dominant), strokes 23%→19% (trees/entities/road edges — the intended "props pop" population); shore wave margin healthy (shore 823 vs inland 386, +200 asserted). Perf 3.4–7.1 ms/frame run-to-run (background-load variance; steady state in the historical ~4.2 ballpark; blade thinning only removes geometry).

Visual (decoded dump): ground is now `.` `,` `'` `` ` `` `;` `:` `-` `~` at dark green (8,16,16)–(48,40,24); trunks/torch-lit strokes/actor glyphs pop warm; warrior reads clearly at rows 17–22 against the carpet.

Changed this slice: `client/src/gpu/{mesh,renderer,shaders}.rs`, `AGENTS.md` (GPU bullet), this handoff.

Next action (next session): user live verdict at 156x44 (`play_ansi.bat`) — quiet ground + popping actors, ocean verdict, plus the six carried live checks. Then port item #2 (impact/ember FX) + #3 (AoE telegraph rings) as the combat-readability pack from `video-tools/REPORT.md`. Do not commit or push unless asked.

## Video-tools pipeline + reference analysis slice (2026-10-17)

Request: build a robust, agent-oriented video-understanding pipeline (FFmpeg + OpenCV; adaptive sampling at 2-3 fps snapshots, motion analysis, contact sheets, interval zoom), then analyze `data/videos/` (6 captures, 720×1280@30 fps, ~9,169 frames) against our game to decide what to port for look parity.

Implemented (`ascii-mmorpg/video-tools/`, python + ffmpeg/ffprobe + opencv 5.0, no cargo impact):
- `video_analysis.py` CLI: `analyze <videos> [--outdir out] [--dense-fps 3]` and `zoom <video> <start> <end> [--fps N]`. Full-rate measurement pass (color-aware pixel diffs >8/>2 levels, 32-bin luma histogram distance, DIS/Farneback optical flow magnitude + 8-bin direction, Laplacian blur) → per-second JSON + per-frame CSV → event detection (scene cuts, freezes, motion bands with ease labels from flow-vs-time correlation, subtle-change spans, sustained-blur spans vs video median) → dense 3 fps timestamped contact sheets on flagged intervals (bounded: overlap-merge, ≤5 s windows, ≤20/video) + overview sheet per video + per-second audio RMS. `zoom` dumps ANY interval at native fps + per-pair diffs.csv.
- `README.md`: method, thresholds, CLI outputs, known limitations.
- `REPORT.md`: per-video timestamped findings + cross-video "reference recipe" + **prioritized port list mapped to engine surfaces** (9 items, #1 quiet-ground/actor-bright contrast hierarchy; then impact/ember FX, AoE telegraph rings, lightning/flash, icicle sweep, fog canopy banks, big trees/banners/torches, boss HP bar + label backing, rain streaks).

Benchmark validation: synthetic 21 s clip (freeze / slow hue morph / full-field ramp / continuous motion / white-flash hard cut / subtle fade / sustained blur) — all event classes detected; six real bugs found and fixed by the benchmark (luma-only diffs blind to hue morphs; ramp histogram ghosting; interval chaining; blur vs flat-color; motion floor; stem-collision output collision). One real-data bug found and fixed (fast pans false-fired as cuts via the absdiff branch — now histogram+movement gated). Real-run numbers: Video-90418 = dragon boss fight (breath cone 16.5–18.5 s, silhouette-swap death cloud 34.4–35.6 s ease-out, orange dashed AoE rings); Video-47489 = spell test (lightning columns 2.5–6.5 s, ground burst 8.5–14.5 s, icicle spike waves 14.5–24.5 s); Video-31774 = village + inventory UI (18.5–20.5 s); WhatsApp 42 PM = multiplayer + rain + fog canopy; WhatsApp 39 PM + (1) = village/banners/torches/trees. Reference recipe: median luma 20–33/255 with Laplacian var 1300–2400 = dim dense quiet ground carpet + bright saturated single-hue FX + silhouetted shaded props + telegraph rings before damage + fog/rain atmosphere. Honest misses: audio RMS-only (no STT — captions read from frames); 39 PM's slow scroll reports zero events (flow below its own median×1.5 — use overview/zoom); pin-prick twinkles may under-flag between dense tiles.

Verification (2026-10-17, Windows): python compile ok; benchmark + all 6 videos analyzed with clean event lists; `video-tools/` standalone (no workspace change); no CI steps needed this slice.

Changed this slice: `video-tools/` (new: `video_analysis.py`, `README.md`, `REPORT.md`, analysis outputs in `out/`), this handoff.

Next action (next session): user picks next bounded slice from the port list in `video-tools/REPORT.md` (#1 quiet ground/bright actors recommended — it compounds with everything else). Do not commit or push unless asked.

## Southern ocean slice (2026-10-17)

Request: "Add an ocean at the bottom of the map using harri's wave rendering technique."

Attribution finding (verified with web fetch): Harri's writeup ("ASCII characters are not pixels", alexharri.com/blog/ascii-rendering) contains NO wave/ocean technique — it covers shape vectors, contrast enhancement, and the widened directional table only. The "ASCII ocean" demos people associate with such renderers are all the same construction, so this slice implements that canonical method inside Harri's shape-picking pipeline: one directional rolling swell (long swell + cross-chop), crest bands scrolling toward the shore, white foam breaking at crest peaks, a shallow-to-deep color gradient, all flowing through this project's existing Harri-style 6D shape/contrast ASCII pass with the material-6 wave glyph ramp.

Placement: the walkable map ends at `WORLD_BOUNDS.max_y` = 35 (shared `crates/shared/src/collision.rs`); mesh z = world y * 0.8, so the shoreline sits at z = 28.0 and the water south of it is pure scenery — collision untouched, the last walkable rows become beach. "Bottom of the map" = south edge (North = (0, -1)).

Implemented:
- `client/src/gpu/mesh.rs` `add_terrain`: new leading classification — quad center z >= shore → open water (material 6, one flat plane at y = -0.12); [shore-2.4, shore) → beach (dry sand [0.78,0.71,0.54], wet shoreline strip [0.42,0.39,0.31] via >= shore-1.4); older road/meadow branches unchanged north of the beach, and the ocean branch fires first so the Cobblestone Highway dead-ends into the sea.
- `client/src/gpu/shaders.rs` vertex pass: water bob rebuilt as directional swell `sin(t*2.2 + z*0.55)*0.14` (crests travel -z → north, onto the beach) + chop `sin(t*3.1 + x*0.9)*0.05` (was a tiny axis-agnostic shimmer; material 6 had no other user).
- Fragment water: same swell/chop phases → normal shading; depth gradient `clamp((z-28.0)/14.0)` from shallow [0.10,0.28,0.52] to deep [0.05,0.15,0.33]; foam `smoothstep(0.58, 0.92, swell+chop)` mixes toward white [0.88,0.94,1.0] and turns opaque (`select(0.88, 1.0, …)`), so foam rows rasterize as bright wave language; kept tight lobe 32.0 + fresnel sky reflection.
- Tests (all pass; no GPU adapter → wave test skips cleanly): mesh `southern_ocean_starts_exactly_at_the_world_boundary` (materials, flat plane, sand rows, land/water share the shoreline edge bit-exactly); renderer `ocean_renders_when_the_player_reaches_the_south_map_edge`; renderer `shore_view_outputs_rolling_wave_glyph_language` (end-to-end; see noise fix below for current counts); WGSL text tests pin swell/foam/gradient/quiet-language constants. Terrain radius (42–68) crosses the shoreline from anywhere, so even inland builds contain far-south water vertices — assertions encode that explicitly.

Aesthetic caveat — RESOLVED same day (user reported it live: "why is this grass texture everywhere??"). Three stacked noise sources found and fixed:
- Procedural trees and grass blades spawned purely from the camera-window hash with only an `|x| > 4` gate — no southern bound — so pines stood in the surf and grass blades textured the whole ocean (`renderer.rs`: trees now skip `tree_wy >= 32`, grass skips `gy >= 32`; the beach band starts at world y=32).
- The water glyph ramp dithered every cell (`'.', '-', '`', 'c', 's'` variants per band), so the sea read as letter turf instead of wave lines (`shaders.rs` `material_glyph`): calm bands 0-1 are now empty space, band 2 is a pure `~`, band 3 mixes `~`/`=`, foam caps break `=`/`~`/`*` — no letters anywhere in water.
- Stroke overrides fired on water (wobble normals look like geometry to the Sobel gate) speckling `/` `\` `|` creases over the sea; `shape_glyph` now takes `water_surface` (material 6) and never strokes. Also spec 0.9→0.6 and fresnel 0.35→0.10: the sky-reflection lift held distant water in mid luminance bands (constant `cs` texture at the horizon).
Verified visually via decoded shore frames: the ocean is now rolling `~`/`=` lines with calm empty gaps (`*~***~~~~****=~~*=~~~~...`), foam caps `*`, and the letter noise is gone. Wave-glyph counts shore 1014 vs inland 611 (margin asserted +200). Spawn-view dump unchanged vs pre-ocean (2-cell GPU tie noise only); frame is exactly 44x156 (rows 44-45 of the dump file are the zero-length file tail, identical to pre-ocean).

Known deferrals: CPU renderer/`--cpu` path and the windowed engine-bridge world have no ocean (separate bounded slices); animation_time drives the swell (frame-clocked, fine at 60fps).

Verification (2026-10-17, Windows): fmt clean; workspace tests 0 failed (client 87 lib); `tools content` ok (11); CI-order clippy clean; release build ok; `--demo` smoke exit 0; 156x44 dump unchanged vs pre-ocean at spawn (only the known 1-cell depth-tie nondeterminism; the 157-char "rows" are the per-row trailing `\r` the dump has always carried); shore-view frame confirmed visually via a temporary decoder print (rolling `~`/`=` foam lines south of the beach, player light pool on the sand).

Changed this slice: `client/src/gpu/mesh.rs`, `client/src/gpu/shaders.rs`, `client/src/gpu/renderer.rs` (tests + tree/grass shoreline gates), `AGENTS.md` GPU bullet, this handoff.

Next action: user live re-verdict — stand on the beach at the map's south edge (walk the highway down to (x,35)) and judge the cleaned-up rolling waves/foam/beach, plus the still-pending six-point verdict from the facing slice. Then next bounded slice from the checklist (CPU renderer scene-partition adoption). Do not commit or push unless asked.

## Player art faces-the-camera slice (2026-10-17)

Request: "Move the character head and body in the direction the camera is facing, and make the body face in the direction of camera as well."

The movement half was already done (camera-relative WASD slice). This slice is the facing half, decided with the user: the player's **whole figure** (head/helm/body/arms/sword as one unit) faces **toward the camera** as it orbits, player-only.

Root finding (`client/src/gpu/mesh.rs` / `gpu/renderer.rs`): `MeshBuffer::add_humanoid` took `_facing_angle` and ignored it entirely; both call sites hardcoded `0.0`, so the part geometry authored facing +Z never rotated.

Implemented:
- `mesh.rs`: new `rotate_vertices_since(first_vertex, anchor, angle)` — right-hand +Y rotation of the vertices appended since a marker, about the vertical axis through the anchor; normals rotate too (shading stays correct), metadata/indices untouched, non-finite/0.0 angles are no-ops.
- `add_humanoid` now captures its first-vertex marker and applies `facing_angle` for real (doc explains the authored +Z convention). Geometry authored facing +Z == the yaw-0 camera-eye direction, so with `facing_angle = camera_state.yaw` the model's +Z maps exactly to the eye offset `(sin yaw, cos yaw)` from the renderer's own eye formula — the body looks at the camera at every orbit angle, and yaw 0 is bit-identical to before.
- `renderer.rs`: local player call site passes `camera_state.yaw` (realtime `populate_scene_smoothed_at_time` already receives `self.camera`). Other `EntityKind::Player` remotes and monsters/NPCs stay unrotated (user chose player-only). Deterministic demo paths pass `CameraState::default()` (yaw 0) — untouched.
- Tests: new `humanoid_facing_rotates_head_and_body_about_the_foot_anchor` (quarter-turn maps every `(x,z)` to `(z,-x)` incl. normals, height preserved, zero facing bit-identical).

Discovery (pre-existing, NOT from this slice): `--wgpu-demo 1 156x44` dumps are NOT byte-identical run-to-run on this machine — two reruns of the same binary differed by one glyph (a `/`↔`\` diagonal-stroke tie flip). Depth-diagonal equality ties flip with GPU nondeterminism; the mesh unit tests still enforce CPU-side exact determinism. Do not treat single-cell flips in GPU demos as slice regressions; re-run before diagnosing.

Verification (2026-10-17, Windows): workspace tests 0 failed (client 83 lib now), `tools content` ok (11), CI-order clippy clean, fmt clean, release build ok, `--demo` smoke exit 0. Perf unchanged.

Changed this slice: `client/src/gpu/mesh.rs` (rotation helper + real facing + test), `client/src/gpu/renderer.rs` (call site), `AGENTS.md` controls bullet, this handoff.

Next action: user live verdict at 156x44 — right-drag orbit should now turn the player figure so its front (visor) always faces the viewer; movement unchanged. Then next bounded slice from the checklist (CPU renderer scene-partition adoption). Do not commit or push unless asked.

## Full-redraw absolute-addressing slice (2026-10-17)

Request: "minimap is duplicated." + "make the canvas ASCII characters smaller so it appears more detailed."

Duplicate-Minimap diagnosis and fix (code-verified):
- The composed buffer contained the panel title exactly once (decoded `--wgpu-demo` frames prove it), yet the user's live screen showed it twice, one row apart, and it PERSISTS after relaunch. Root cause: `PlainTerminal::draw_into`'s FULL-REDRAW branch (first frame / relaunch / resize) still chained rows with `\r\n`, while the DIFFERENTIAL branch already used absolute `\x1b[{row};1H` jumps. The project's own historical handoff note ("Full-canvas + transient-line fix (2026-10-17"): full-width rows auto-wrap the cursor, so `\r\n` landed one row too far down in Windows ConPTY and left stale cells — the fix was only ever applied to the differential branch. Every relaunch re-ran the malformed full repaint, and then the differential branch (hash-based) never rewrote rows it believes are unchanged, so the skewed layout survived indefinitely — exactly the persistent ghost.
- Fix: the full-redraw branch now addresses every row absolutely (per-row `\x1b[{i+1};1H`, no `\r\n` anywhere in either branch). Rationale comment cites the historical note in code.
- New regression test `full_redraw_uses_absolute_row_addressing_only` (client bin) asserts per-row jumps and no `\r\n` in the full-redraw payload.
- Permanent diagnostic: status card now reads `POS (x,y) TICK n <backend>` / `WX <weather> GRID <w>x<h>`. The GRID line mirrors what crossterm/ConPTY report; if a user ever sees GRID disagree with the physical window, Windows resized the buffer without ConPTY relaying it — that pinpoints terminal-side size lies instantly.

Canvas-detail request outcome (no app change):
- User asked for smaller canvas glyphs with unchanged HUD size. A terminal renders the whole window in ONE font, so per-region glyph scaling is impossible (documented constraint); the canvas already renders exactly 1 glyph per terminal cell (verified at 156x44). User chose to shrink the Windows Terminal font themselves (Ctrl+- / font-size setting): same window pixel size, smaller glyphs, more scene detail; HUD text shrinks equally as a physical consequence. App-side alternative stays available if they change their mind: trim HUD chrome (top bar 3→2 rows, log 4→3, panel 28→24 cols) to give the canvas more cells at the current font.

Verification (2026-10-17, Windows, GTX 1650): `cargo fmt --all -- --check` clean; `cargo test --workspace --locked` all suites 0 failed (client 82 lib now); `tools content` ok (11 definitions); CI-order clippy clean; release rebuild + fresh `--wgpu-demo 1 156x44` dump: all 44 rows exactly 156 display cols, zero wide glyphs, `GRID 156x44` visible in the status card, Minimap title exactly once per frame; 200-frame headless scan: zero frames address the title at more than one row; `--demo` smoke exit 0.

Changed this slice: `client/src/lib.rs` (full-redraw absolute addressing + rationale + test), `client/src/ui.rs` (status-card POS/GRID lines), this handoff.

Next action: user live verdict via `play_ansi.bat` at 156x44 on all pending items — duplicated Minimap gone after relaunch, scene partition fully filled, HUD borders stable in motion, WASD walking camera-relative — and confirm the GRID line matches their window. Do not commit or push unless asked.



Request: "the transient line isn't gone and it doesn't fill the scene."

## GPU scene-partition fill + synchronized-frame flicker fix (2026-10-17)

Root causes (both structural, verified in code):
1. **Scene did not fill the scene partition.** `GpuRenderer` rendered the FULL terminal grid, but the composed HUD reserves 8 rows (top bar 3 + log 4 + legend 1) and a 28-col side panel, and `partitions::blit_scene` *clips* cells to that smaller area — so the visible scene was a top-left crop with the wrong aspect, and partition space sat unblitted. Fixed: new `ui::scene_partition_dims(width,height)` derives the exact scene-partition size from the composed layout; `GpuRenderer` now renders that grid natively (`scene_width`/`scene_height` fields; pipeline, camera aspect, ray-unprojection, batches all keyed to it) while keeping full terminal dims only for the composed HUD frame. The blit now fills the partition exactly and the camera aspect matches what is visible. CPU renderer keeps its old behavior (same structural issue exists there — next-slice candidate).
2. **Transient sub-ms line on idle.** The realtime path is `PlainTerminal::draw_into` (dirty-line hash diffing) — which had NO synchronized-update markers, unlike the `SpanDiffEncoder` benchmark path. Idle torch/grass/lighting animation keeps nearly every scene row dirty each frame; Windows Terminal paints between the scattered absolute-row jumps, which reads as a brief torn/black line that "stops when the character moves" (motion masks it). Fixed: both repaint paths (full redraw and differential payload) are now wrapped in DECSET 2026 synchronized-update markers (`\x1b[?2026h` … `\x1b[?2026l`), so the terminal commits each frame atomically. Unsupported terminals ignore the private mode (no behavior change).

Changed: `client/src/ui.rs` (`scene_partition_dims` + death-match doc comment to the composed layout), `client/src/gpu/renderer.rs` (scene-dims render grid, `resize` only rebuilds when the partition size changes, tests updated), `client/src/lib.rs` (`draw_into` 2026 markers on both paths; stale CRLF-chaining test expectation fixed to absolute jumps), this handoff. `client/src/ui.rs` had a ZERO diff before this slice despite the summary's claim that `last_scene_dims` tracking had landed — that claim was historical/stale; the actual implementation is described here.

Verification (2026-10-17, Windows, GTX 1650):
- `cargo fmt --all -- --check`: clean.
- `cargo test --workspace`: all pass (client 81 lib + 2 bin, all suites ok).
- `cargo run -p ascii-mmorpg-tools -- content`: `content ok: 11 definitions`.
- `cargo clippy --workspace --all-targets -- -D warnings`: clean (CI order).
  - NOTE: `cargo clippy --all-features` fails at the vendored `crates/libnotcurses-sys` build script (pre-existing native build failure, unrelated to this slice; CI never compiles it). Historical handoff claims of "clean both orders" are not reproducible for `--all-features` today.
- `--wgpu-demo 2 120x30` structural dump (release, Vulkan): frame is 30 rows; scene partition 92×22 by construction; scene-row tails (cols 92–119) contain the side panel chrome (LUCK stat, minimap card, GATE/TORCH/BRIDGE radar, zone refs), blit fills the partition, HUD preserves the remainder. Camera now frames the scene partition aspect.
- `--demo`: renders all 6 frames, `2026h/2026l` markers visible wrapping each differential frame payload.
- User's idle-flicker stop-condition (flicker when idle, stops while moving) matches the mode-2026 diagnosis; needs the user's live confirmation (`play_ansi.bat`).

Next action: user visually confirms in a real terminal (flicker gone with WGPU default + scene fills the partition). If the transient line survives, next investigation is a per-frame GPU readback ring replay (capture N frames headless and diff cells for a suspicious row), NOT more terminal-encoder guessing. CPU renderer scene-partition adoption is a second bounded slice. Do not commit or push unless asked.

## Camera-relative movement slice (2026-10-17)

Request: "Make the character move in the direction the camera is facing."

Implemented:
- `client/src/lib.rs` — `SceneRenderer::camera_yaw() -> f32` (default 0.0), routed through `GpuRenderer` (`self.camera.yaw`) and `ActiveRenderer` (Cpu → 0.0). No protocol or snapshot change; server still owns positions, `Command::Move(Direction)` is unchanged.
- `client/src/main.rs` — `movement_direction(held, yaw)`: screen-up maps to the camera's forward ground vector `(-sin yaw, -cos yaw)`, screen-right to `(cos yaw, -sin yaw)`; the rotated vector snaps to the nearest compass octant (`atan2(x, -y)`, `round(θ / 45°)`). yaw=0 reproduces the previous fixed mapping exactly. Call site: the 90 ms held-key tick now passes `renderer.camera_yaw()`.
- Help text: "WASD move (camera-relative)". CPU renderer / notcurses have no orbit camera (yaw 0.0), so their movement is unchanged by design.
- Tests (client bin): `camera_yaw_zero_keeps_world_relative_mapping` (yaw 0 = old mapping), `movement_rotates_with_camera_yaw` (quarter turn: up→West/right→North; 45°: up→NorthWest because the camera offsets to the player's southeast; negative quarter: up→East/right→South; diagonals; None when idle). NOTE: the initial 45° test expectation (NE) was wrong — the camera convention is confirmed NW; trust `camera.rs`'s `sin/cos` eye offset, not the test author's compass intuition.

Verification (2026-10-17, Windows): `cargo fmt --all -- --check` clean; `cargo test --workspace --locked` all suites 0 failed (client 81 lib + 4 bin); `tools content` ok; CI-order clippy clean; release rebuild + `--demo` smoke exit 0.

Next action: user live re-check — WASD should walk in the direction the screen shows "up", including after right-drag orbits; Q/reset behavior unchanged. Combined with the two earlier slices today (SSAA fill + border stability), all three pending live confirmations can land in one `play_ansi.bat` run at 156x44. Do not commit or push unless asked.

```text
Read `D:\Personal\codes\Projects\ASCII-Game\AGENTS.md`, `D:\Personal\codes\Projects\ASCII-Game\ascii-mmorpg\AGENTS.md`, and `D:\Personal\codes\Projects\ASCII-Game\ascii-mmorpg\docs\SESSION_HANDOFF.md`, starting with the "Widened directional contrast slice" section (2026-10-17). Inspect the worktree and preserve all changes (HEAD 1692003, ahead by 5, not pushed; uncommitted slices in pipeline.rs/renderer.rs/shaders.rs/AGENTS.md/ui.rs/main.rs/lib.rs/SESSION_HANDOFF.md — SSAA fix, emoji-width fix, camera-relative movement, full-redraw absolute addressing + GRID diagnostic, widened directional contrast). Ask the user for their live verdict at 156x44 on all six: duplicated Minimap gone after relaunch, scene partition fully filled, HUD borders stable in motion, WASD walking camera-relative, status-card GRID matches their window, and boundary lines a cell crisper in motion (widened contrast). If a ghost row survives live, capture a `--wgpu-demo` dump at the live size and decode per-row display width — do NOT touch terminal encoding. If the user confirms, pick the next bounded slice from the checklist (CPU renderer scene-partition adoption is first) and verify with fmt, workspace tests, tools content, CI-order clippy, and coverage/width dumps. Update the handoff and TODO checklist with actual results, finish with a fresh-session handoff prompt. Do not commit or push unless requested.
```


## Live HUD border / emoji-width slice (2026-10-17)

Request: "The log and heading and other borders have a few lines missing, specifically border lines facing towards canvas."

Root cause (verified by decoding a released `--wgpu-demo 1 156x44` dump byte-exactly, including East-Asian display widths): the composed buffer was well-formed — every HUD border existed (top bar `╰───╯` row 3, panel `│` col 128 + `╭ Minimap ─╮`/`╰──╯`, log strip `╭ Log ─╮`/`╰──╯` rows 40–43) — but rows containing East-Asian-Wide emoji were authored as 1 buffer cell while rendering as 2 terminal columns. Top-bar row 1 (🌲 📍 🌧) measured **159 display columns** at a 156-col terminal; rows with 🗺 💀 🎯 ⚔ ⚠ ✨ 📜 measured 157. Every overflowing row auto-wrapped, so the terminal's row layout shifted/shredded: HUD border lines adjacent to the scene canvas visually vanished, content climbed into the top bar, and bottom rows clipped. This same wrap-shift is the likely mechanical driver of the historic "pinned seams" and is now removed at the source.

Changed (all in `client/src/ui.rs`): every emoji-presentation character replaced by narrow ASCII — top bar (`Whispering Forest`, position coords, weather `[Rain]/[Snow]/[Storm]` without 🌧 ❄ ⚡ prefixes), log title `📜→" Log "`, side panel `🗺→" Minimap "`, `📍→"[at] "`, `🎯→"! Threats"`, log prefixes `⚔→">", 💀→"X"/"!!", ⚠→"!!", ✨→"*"`, `✨ Area peaceful→*`, inventory `🎒`/character-sheet `📜 🛡 ⚔`/world-map `🗺 🏛 ⛩ 📍 🧭 🌊 👾` titles and legend lines cleaned, map art `🏛RUIN→^RUIN` (1-col char preserves the authored grid). Ambiguous-width symbols kept because Windows Terminal renders them narrow (verified in dumps): `☼ ♠ ▲ █ ░ ⚑ •`. No emoji remain in any HUD string (Python `east_asian_width` scan of client/src = zero W/F/1F000-range chars); server/shared sources have none either.

Verification (2026-10-17, Windows, GTX 1650):
- `cargo fmt --all` + `-- --check`: clean (one formatting fix applied in ui.rs after the edits).
- `cargo test --workspace --locked`: all suites 0 failed (client 81+2, engine-suite 91, server 8+3, shared 14+2).
- `cargo run --locked -p ascii-mmorpg-tools -- content`: `content ok: 11 definitions`.
- `cargo clippy --workspace --all-targets -- -D warnings`: clean (CI order).
- `--demo` release smoke: exit 0.
- Width gate (new decoder generalized to display-width counting): `--wgpu-demo 2 156x44` / `120x30` / `220x70` — **every frame row is exactly terminal width, zero wide glyphs**; border glyphs verified at key columns (panel `│` at col 128 for rows 4–39, `╭/╰` rows 4/39/40/43).
- Perf at 156x44 release: ~11.4 ms/frame (SSAA slice).

Next action: user live re-check (`play_ansi.bat` at 156x44) — scene fills the partition AND all HUD borders are stable in motion. If seams persist, per-frame GPU readback ring diff — NOT terminal-encoder changes. Fresh-session prompt:

```text
Read `D:\Personal\codes\Projects\ASCII-Game\AGENTS.md`, `D:\Personal\codes\Projects\ASCII-Game\ascii-mmorpg\AGENTS.md`, and `D:\Personal\codes\Projects\ASCII-Game\ascii-mmorpg\docs\SESSION_HANDOFF.md`, starting with the "Live HUD border / emoji-width slice" section (2026-10-17). Inspect the worktree and preserve all changes (HEAD 1692003, ahead by 5, not pushed; uncommitted slices in pipeline.rs/renderer.rs/AGENTS.md/ui.rs — SSAA fix + emoji-width fix). Ask the user for their live verdict at 156x44: scene partition fully filled, HUD borders stable in motion. If anything still misplaces rows live, capture a `--wgpu-demo` dump at the live size and decode per-row display width (all rows must equal terminal width; wide-char scan must be zero) — but do NOT touch terminal encoding. If the user confirms, pick the next bounded slice from the checklist (CPU renderer scene-partition adoption is first) and verify with fmt, workspace tests, tools content, CI-order clippy, and coverage/width dumps. Update the handoff and TODO checklist with actual results, finish with a fresh-session handoff prompt. Do not commit or push unless requested.
```


## Open live-fill investigation (2026-10-17)

**RESOLVED later the same day (2026-10-17). Summary: H1 was ruled out empirically; the real cause was H2-adjacent but mechanical — the adaptive-SSAA 2x tier broke the compute pass's 4x-locked tap grid, confining valid content to the top-left quadrant of the scene partition (`~cells/2 × cells/2 + edge taper = the observed ~65x19 of 128x36 at 156x44`). Fix: retire the 2x tier, fixed 4x SSAA at every grid size. Both previous "fixed" sessions missed it because every previously dumped size (92x22, 128x22) stayed at or under 4096 cells — the 2x path only engages above 4096 cells and was never coverage-checked. Evidence chain below.**

Fixed in this session:
- `client/src/gpu/pipeline.rs` — `adaptive_ssaa` now returns `SSAA_FACTOR` (4) for every grid size, with the full failure rationale in its doc comment; `ssaa` field doc updated.
- `client/src/gpu/renderer.rs` — scene-dims doc comment updated; test `large_terminal_renders_natively_with_reduced_ssaa` → `ssaa_is_fixed_4x_for_every_grid_size` (asserts 4 at 80x30 / 128x36 / 220x70); `display_output_matches_terminal_size_despite_capped_render` asserts `pipeline.ssaa == 4`.
- `AGENTS.md` — client GPU bullet updated (fixed 4x SSAA + why the 2x tier was retired).
- Did NOT touch `PlainTerminal`/encoding (per the no-encoder-changes gate; the size-propagation check cleared it anyway).

Size-propagation check (clears `PlainTerminal::refresh_size`):
- `lib.rs:240-250` `refresh_size` DOES re-query `crossterm::terminal::size()` per frame and updates only on change. `main.rs:358-361` wires it into `renderer.resize` every frame; `GpuRenderer::new`/`resize` both map through `scene_partition_dims` consistently. Live rendering saw the true terminal size.

Decoded-coverage evidence (release, GTX 1650/Vulkan, `--wgpu-demo` dumps parsed by stripping CSI sequences and counting non-space glyphs inside the scene band; user's live size confirmed as **156x44**):
- 156x44 BEFORE fix: scene band 128x36 → content in rows 4–22 only (19/36 rows), and inside those rows only cols 1–65 of 128 — a hard 65x19 top-left block; rows 23–38 contain ONLY the side panel's 2-col border chrome at cols 129–156 (cells were being emitted there). Identical shape to the user's screenshot (~90x22 top-left block, hard right edge, black below; their size estimate was the human-eye version of 65x19 seen through font/zoom).
- Mechanism: `cs_main` (shaders.rs:804-815) hardcodes `base = x*4`, `tex_w = width*4` and a 4x4 tap loop, while `adaptive_ssaa` yields 2x for grids > 4096 cells (128x36 = 4608). Every cell with x ≥ cw/2 or y ≥ ch/2 samples fully out-of-bounds texels → zeros → luma 0 → space-on-black. Content only survives at cells/2. The `smooth_slope` thresholds (3-texel fine span vs 8-texel neighbor span) and the six-region shape vector are likewise 4x-locked, so the tier was retired rather than re-tuned.
- 156x44 AFTER fix: 36/36 rows × all 128 cols covered; 11.39 ms/frame release (87.8 FPS equiv).
- Regression sizes after fix: 120x30 → 22/22 x 92 cols; 156x30 → 22/22 x 128 cols; 220x70 → 62/62 x 192 cols (the 192x62 partition was >4096 cells — the old tier would have half-blocked it too).

Pinned seam lines: not reproducible in any static dump before or after; if they survive live, investigate a per-frame GPU readback ring diff — do NOT re-open terminal-encoder changes.

— Historical investigation record (pre-fix, kept for evidence/context): —

User ran the game live and reports "not fixed yet" with a screenshot. What the screenshot shows:
- The scene content is confined to a top-left block of roughly 80–95 cols × ~22 rows, with a HARD rectangular right edge and a hard cut to pure black below (bottom of the scene partition, ~15+ rows, entirely black, zero glyphs).
- Thin horizontal seam gaps (1–2 empty rows) between content bands sit at fixed heights — the user's "pinned lines" in stills.
- The HUD (top bar, log, legend, side panel) spans the full window/framework width.
- Window looks ~190 cols × ~45–54 rows at default font; Windows Terminal title "Windows PowerShell".

Competing hypotheses (indistinguishable by eye — both render as "space with near-black bg"):
- **H1 stale-dims (leading):** renderer terminal dims are STUCK at the startup size (default Windows Terminal 120x30 → scene partition (92,22) ≈ measured content block). The per-frame resize chain (`realtime_loop` → `terminal.refresh_size()` at `main.rs:358` → `renderer.resize(...)` at `main.rs:361`) is wired, so if H1 is real the failure is inside `PlainTerminal::refresh_size` (`lib.rs` ~240) — possibly returning the stored size instead of re-querying `crossterm::terminal::size()`. NOT YET VERIFIED.
- **Contradiction to check under H1:** `format_frame_with_hud` composes the HUD at the SAME `self.width/self.height` — if dims were stale at 120x30, the top bar/log would also be confined to ~120 cols, but the screenshot's HUD looks full-width. Either the user's logical column count IS ~120 with a large font (then the content block ≈ (92,22) and the black region is H2 below), or something splits "HUD dims fresh / scene grid stale", which current code structurally cannot do — re-verify before trusting either side.
- **H2 void cells:** the grid matches the partition but the WORLD doesn't cover the frustum at large aspect — cells beyond the terrain radius render as blank space on near-black bg (camera: pitch 58°, distance 13.5, FOV 34°, `aspect=(w/h)*0.48`; terrain_radius clamps at 68.0, grass capped at ±28/±22 around the camera). Content hugging the top-LEFT edge favors H1; content centered would favor H2.

Verified this turn by reading code (no changes made):
- `GpuRenderer::resize` (`gpu/renderer.rs:99-117`) is correct: no early-return bug; pipeline rebuilt only when partition dims change.
- `GpuPipeline::resize` (`pipeline.rs:710+`) drains in-flight staging slots before reallocating.
- `populate_scene_*` consume `scene_width/scene_height` consistently; camera default `pitch 58°, distance 13.5` (`camera.rs`).
- `--wgpu-demo 2 120x30` (validated earlier slice) renders scene (92,22) PON FULL HUD chrome — the failing config is the user's larger live size.

Bounded next steps for the new session (in order): — COMPLETED as described above; kept as methodology record.

Verification for this resolution session (2026-10-17, Windows, GTX 1650/Vulkan):
- `cargo fmt --all -- --check`: clean.
- `cargo test --workspace --locked`: all suites pass, 0 failed (client 81 lib + 2 bin + 91-test suite, server 8+3, shared 14+2, remaining 0-test suites ok).
- `cargo run --locked -p ascii-mmorpg-tools -- content`: `content ok: 11 definitions`.
- `cargo clippy --workspace --all-targets -- -D warnings`: clean (CI order; zero warnings).
- `--demo` release smoke: exit 0 (6 frames, 2026 markers intact).
- Coverage dumps (decoded): pre/post evidence in the summary above. PowerShell note: `cmd /c "... > file"` for writes (PS re-encodes UTF-8), `Select-String` over `2>&1` reports exit=1 from stderr-as-ErrorRecord noise — read the tool output text, not $LASTEXITCODE, for these wrappers.

Work is uncommitted; HEAD `1692003`, local `main` ahead of `origin/main` by 5 — NOT pushed, do not push unless asked.

Fresh-session prompt:

```text
Read `D:\Personal\codes\Projects\ASCII-Game\AGENTS.md`, `D:\Personal\codes\Projects\ASCII-Game\ascii-mmorpg\AGENTS.md`, and `D:\Personal\codes\Projects\ASCII-Game\ascii-mmorpg\docs\SESSION_HANDOFF.md`, starting with the "Open live-fill investigation" section (2026-10-17, RESOLVED). Inspect the worktree and preserve all changes (HEAD 1692003, ahead by 5, not pushed; untracked engine/ files + camera.rs present; session changes to pipeline.rs/renderer.rs/AGENTS.md are the SSAA fix — uncommitted). Ask the user whether the top-left block, bottom-black region, and pinned seam lines are gone live at 156x44 (`play_ansi.bat`). If anything survives, capture fresh `--wgpu-demo` dumps (release), decode coverage, and investigate a per-frame GPU readback ring diff — do NOT touch terminal encoding. If the user is happy, present the next bounded slice from the checklist (CPU renderer scene-partition adoption) and verify it with fmt, workspace tests, tools content, CI-order clippy, and coverage dumps. Update the handoff and TODO checklist with actual results, finish with a fresh-session handoff prompt. Do not commit or push unless requested.
```

## ASCII renderer article study and optimization slice (2026-10-16)

Sources reviewed:
- `https://kciter.so/posts/ascii-3d-renderer/en/` — confirms the useful baseline already present here: transform/raster/depth stages, face-normal lighting, back-face/visibility concerns, and shade ramps rather than texture mapping.
- `https://alexharri.com/blog/ascii-rendering` — adds the higher-value terminal-specific idea: treat a glyph as a shaped coverage region, collect multiple samples per cell, normalize a shape vector, and use contrast enhancement to sharpen boundaries. The article also recommends reusing GPU work and avoiding brute-force per-cell CPU lookups.

Applied only to the existing GPU compute pass (`client/src/gpu/shaders.rs`):
- The existing 4x4 SSAA loop now accumulates six luma regions (top-left/top-right, middle-left/middle-right, bottom-left/bottom-right) while it already has each colour sample in hand.
- `shape_glyph` consumes those six values, normalizes by the local maximum, and applies a conservative exponent (`1.22`) before its directional stroke tests. Flat regions remain eligible for the existing material glyph fallback; edges receive more readable `-`, `_`, `|`, `/`, `\\`, and `+` choices.
- Removed the former `luma_at`/`shape_region` path, eliminating 12 redundant colour-texture reads per cell without adding a pass, bind group, or CPU work. This is a bounded optimization; a full 6D character-vector nearest-neighbour table and external directional sampling remain deferred until visual profiling justifies them.

Verification for this slice (Windows):
- `cargo fmt --all -- --check` passed.
- `cargo test -p ascii-mmorpg-client gpu::shaders --locked` passed (2 shader tests; 73 filtered client tests).
- `cargo run --locked -p ascii-mmorpg-client -- --wgpu-demo 10 96x28` passed on NVIDIA GeForce GTX 1650 / Vulkan: 10 frames, 9.67 ms/frame, 103.4 FPS equivalent; shader compiled and executed successfully.

Next action: visually compare the updated glyph contours and contrast in a real terminal or captured frame against the previous shader, then tune `contrast_power` only if the result is too harsh. Do not add a new upscaler or reflection pass in the same slice.

## OSRS-style camera and generic MMO controls slice (2026-10-16)

Request: shift the game toward an Old School RuneScape visual presentation: elevated camera, readable character scale/detail, and generic MMO right-mouse camera movement.

Implemented:
- Added `client/src/camera.rs` with client-only `CameraState`: elevated default pitch (58°), bounded pitch (30°–78°), yaw orbit, and reset support. No protocol or snapshot changes.
- Replaced the fixed GPU camera eye with a spherical orbit around the smoothed player-follow focus. The default camera is more top-down, closer, and uses a narrower 34° FOV so actors occupy more of the terminal grid.
- Increased GPU actor model scale from roughly 0.70–0.78 to 0.84–0.90. Existing procedural humanoid/monster meshes and material detail are retained; only presentation scale changed.
- Extended the terminal abstraction with `TerminalEvent`, preserving backend ownership. ANSI `PlainTerminal` exposes mouse events; notcurses remains key-owned and does not call crossterm.
- Normal realtime mode enables/disables crossterm mouse capture through a guard. Holding right mouse and dragging changes camera yaw/pitch; drag baselines, release, clamping, and terminal-cell coordinate deltas are handled safely. CPU rendering accepts the camera API as a no-op fallback.
- Removed the remaining direct crossterm polling from frame pacing so the shared realtime loop no longer violates the notcurses input ownership rule.

Verification for this slice (Windows):
- `cargo fmt --all -- --check` passed.
- `cargo test -p ascii-mmorpg-client --locked` passed: 79 library tests and 2 binary tests, including camera-state and right-drag reducer coverage.
- `cargo clippy -p ascii-mmorpg-client --all-targets --locked -- -D warnings` passed with MSRV-safe pitch constants.
- The existing GPU tests passed, including instanced batches and renderer creation. Manual physical mouse-drag validation remains to be performed in a real ANSI terminal emulator that supports mouse reporting.

Known scope boundary: this slice implements orbit controls for the ANSI/WGPU client. The CPU renderer remains functional but has no 3D camera to orbit, and notcurses mouse support is intentionally deferred rather than mixing input backends.

## Current slice verification evidence (2026-10-12)

Changed this turn: `crates/shared/src/collision.rs`, `client/src/gpu/mesh.rs`, `client/src/gpu/shaders.rs`, `client/src/gpu/renderer.rs`, `client/src/term.rs`, and this handoff.
Features implemented:
- Prominent pine tree trunk visibility: elevated lowest canopy cone to Y = 3.60, assigned trunk and root flares dedicated Wood/Timber material (7.0) with vertical bark striations, and cleared the highway collision corridor (|x| <= 4) with synchronized procedural tree rendering on GPU.
- Synchronized meadow grass wind sway: low-altitude vertex sway in `vs_main` and wind-leaning ASCII blade characters in `ASCII_POST_WGSL` traveling in sync with wind current phase.
- Smooth transitions: exponential spring damping for camera and player coordinates, plus ease-out deceleration for humanoid walk phase.

Verification results:
- `cargo fmt --all -- --check`: passed with zero formatting issues.
- `cargo test --workspace`: passed all 85 workspace unit tests (59 client lib, 1 client bin, 7 server lib, 1 server bin, 14 shared, 2 tools, 1 bot).
- `cargo run -p ascii-mmorpg-tools -- content`: passed with `content ok: 11 definitions`.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed with 0 warnings.
- `git diff --check`: passed with zero whitespace errors.
- Explicit user request: "Made some changes push the current codebase to git main branch".

## Current server-timing slice verification evidence (2026-10-12)

Changed this turn: `AGENTS.md`, `docs/SOT.md`, `docs/adr/0002-fixed-tick.md`, `server/src/main.rs`, and this handoff. Existing staged, unstaged, and untracked work was preserved; no commit or push was performed.

- `cargo fmt --all -- --check`: passed.
- `cargo test -p ascii-mmorpg-server tcp_loop_advances_idle_and_input_ticks_separately --locked`: passed (1 binary test).
- `cargo test -p ascii-mmorpg-server --locked`: passed (7 library tests, 1 binary test, 0 doc tests).
- `cargo clippy -p ascii-mmorpg-server --all-targets --locked -- -D warnings`: passed.
- `git diff --check`: non-blocking LF/CRLF conversion warnings only; no whitespace errors.
- No blockers. The regression test uses the real 50 ms timeout with a 200 ms receive bound and completed in about 0.06 seconds.

## Historical verification evidence

These results were reported in earlier turns; rerun after the next code change. They are not evidence for unmodified current HEAD:

- `cargo test --workspace --locked`: passed a suite reported as client 58, server 7, shared 14, bot 1, tools 2, plus zero doc tests.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: passed.
- `cargo run --locked -p ascii-mmorpg-tools -- content`: passed with `content ok: 11 definitions`.
- `git diff --check`: passed except line-ending warnings.
- `cargo fmt --all -- --check`: previously reported pre-existing formatting differences in GPU mesh/renderer; the current slice rerun passes.
- Notcurses feature/native build: previously failed on Windows when `libnotcurses-sys` could not locate native Notcurses/pkg-config. Later vendored/native work may have changed this; verify rather than assume.

## Session-quality note

The user supplied a screenshot showing five of six external session checks passed: real project work, project code changed, final answer, enough turns, and few failed turns. “The whole conversation is recorded” was unchecked. An agent can update code/docs and provide a final answer, but cannot guarantee recording/upload status or make a session qualify by padding work. Keep one bounded substantive slice per session, update this handoff and the tool TODO list, and recommend a fresh session afterward.

## Definition of done for the next session

The next session should choose whether to implement a strict queued-command 20 Hz scheduler or explicitly retain the current input-driven behavior. It should not add reflections, a new upscaler, broad asset imports, or unrelated cleanup in the same turn.

## Copyable next-session prompt

```
Read `D:\Personal\codes\Projects\ASCII-Game\AGENTS.md`, `D:\Personal\codes\Projects\ASCII-Game\ascii-mmorpg\AGENTS.md`, and `D:\Personal\codes\Projects\ASCII-Game\ascii-mmorpg\docs\SESSION_HANDOFF.md` starting at "Combat FX in the windowed engine renderer (2026-10-17, latest — READ FIRST next session)". Inspect the current worktree and preserve all existing changes (fourteen uncommitted unpushed slices stack in it). Ask the user for live verdicts: (a) windowed renderer `cargo run -p ascii-mmorpg-engine --locked --features present --example play` — WASD glide, F melee burst + rising digit, 1/2/3 spell telegraphs (Cleave amber / Iron-Will cyan / Thunder-Mark blue), dark stage + dashed target decal ring; (b) terminal TUI at 156x44 via `play_ansi.bat` — F attack ember burst + cast telegraph + nameplates/black stage carried checks. Then pick the user's next item from `video-tools/REPORT.md` (#4 lightning column, #5 icicle sweep, #6 fog banks, #7 organic trees/banners/torches) or the small carried CPU-renderer `scene_partition_dims` adoption, verify with the CI order (plus `--features present` checks for engine slices), update `docs/SESSION_HANDOFF.md` and the TODO checklist, and finish with a fresh handoff prompt. Do not commit or push unless asked.
```

## Pause + dynamic partitions slice (2026-10-13)

Request: per-region font sizes (dense high-res game grid, readable HUD, large pause overlay) with dynamic partitions by game state, `P` to pause. Constraint: terminal font size is window-wide, so true per-region font scaling is impossible in one terminal window — density is simulated via glyph weight/spacing, documented in `client/src/partitions.rs`.

Changed: `client/src/partitions.rs` (new: `FontProfile` Dense/Medium/Large, `GameState` Playing/Paused/ModalOpen + `from_modal`, `Partitions::default_for`/`profile_for_row`/`map_dense_glyph` identity choke point, 5 tests), `client/src/lib.rs` (`pub mod partitions`, re-export, `SceneRenderer::is_paused`/`toggle_pause` defaults, pause render test), `client/src/ui.rs` (`ActiveModal::Paused`, `render_pause_menu` large-block menu + stats/keybinds columns, `[P] Pause` in both legends, 2 tests), `client/src/main.rs` (`P` toggles pause, `Q` quits directly from pause, F/R + auto-move gated on `!is_paused`, help texts mention `P`), `client/src/gpu/renderer.rs` (paused-frame test), `AGENTS.md` (keybinds + partitions note).

Minimap stays inside the HUD overlay (no extra GPU pass) — the FPS-optimal choice. Pause is client-side input freeze only; online the server keeps ticking.

Verification (2026-10-13, Windows): `cargo fmt --all -- --check` clean; `cargo test --workspace` all pass (client 70 incl. 9 new, server 8, shared 14+3+2, rest 0-fail); `cargo run -p ascii-mmorpg-tools -- content` → `content ok: 11 definitions`; `cargo clippy --workspace --all-targets -- -D warnings` clean; `cargo run -p ascii-mmorpg-client -- --demo` exit 0; `--wgpu-demo 5` runs, no crash.

Next action: visual check of the pause overlay in a real terminal (`play_ansi.bat`, press `P`); tune menu padding/width if the 55/45 split cramps narrow windows. Do not commit or push unless asked.

## Ratatui composed-frame slice (2026-10-13)

Choice: Ratatui layering (no new deps) over notcurses planes / native window. Verified notcurses backend uses a single plane (`lib.rs:313`) — no layering today; and no in-terminal library can do per-region font sizes (terminal-emulator owned), so layering + glyph weight is the achievable goal.

Changed: `client/src/ui.rs` (`ComposedScene` spec, `render_composed_frame`: top bar / scene+side / log strip / legend + `Clear` modal overlay; `render_side_panel` minimap+threats; extracted `log_lines`/`threat_lines` shared with the legacy log widget), `client/src/partitions.rs` (`SceneCell`, `blit_scene`, `centered_rect`, +2 tests), `client/src/renderer.rs` (`Ink::rgb` table replaces dead `ansi()`/`rows()`, `Canvas::scene_cells`, composed `render_frame`), `client/src/gpu/renderer.rs` (cells→`SceneCell` compose; deleted dead `push_rgb_ansi`/`push_fast_u32`), `client/src/main.rs` (full-height resize, demo/line modes full size), `client/src/lib.rs` (player/status test updated to scene partition), `AGENTS.md` (compose bullet).

Layout: side panel only at ≥64 cols; modal overlay covers the whole main row (true occlusion — verified by test asserting `Minimap` disappears when paused). Output is always exactly terminal-sized, so dirty-line diffing never sees line-count changes.

Verification (2026-10-13, Windows): `fmt --check` clean; `cargo test --workspace` all pass (client 74); `tools content` ok; `clippy -D warnings` clean; `--demo` exit 0; `--wgpu-demo 10` at 100x30/160x50/220x70 → 9.1/12.2/23.2 ms/frame (debug, MX230; same ballpark as pre-slice, real-terminal bytes down since output no longer overflows screen height).

Next action: visual check in a real terminal (`play_ansi.bat` + `P`, `I`, `M`); consider compact radar art in the side panel (currently position + threats). Do not commit or push unless asked.

## Visual quality iteration (2026-10-13)

Looked at real frames (temporary `client/tests/dump_frame_tmp.rs` harness, since removed; GPU ground truth via `cmd.exe` byte redirect because PowerShell re-encodes piped UTF-8 into `?`). Findings: scene partitions were dense, but the side panel (5 lines in 19 rows) and log strip (1 message in 5 rows) were black holes; the GPU terrain/grass ramps mapped bright bands to alphabet soup (`c/s/r`, `o/u/n/x`, `O/0/S/D`, `%/$/X/H`, `#/M/W`, grass `o/w/m/n`, `%/&/8/W`); structural vertical edges hashed to `I`.

Changed: `client/src/gpu/shaders.rs` (terrain/grass/fallback bright bands → quiet strokes, then `░▒▓█` light pools; vertical structural edges emit `|`; entity materials keep letter detail for contrast), `client/src/ui.rs` (side panel: position + compact radar + zone card + threats; log strip 5→4 rows split into messages + always-full status card; compact legend fits 80 cols).

Measured on 96x28 frames: GPU scene letter share now 14.7%, stray edge-`I` count 3 (legit material detail), `█`x61/`▒`x31/`░`x13/`▓`x10 light pools fire; CPU scene fill 64.9% with full panels and fitting legend.

Verification: `fmt --check` clean; `cargo test --workspace` all pass (client 74); `tools content` ok; `clippy -D warnings` clean; `--demo` exit 0; `--wgpu-demo 10 220x70` ~18 ms/frame (debug, MX230 — no regression).

Next action: play live (`play_ansi.bat`) and judge color + motion, which dumps can't show; remaining blanks (tall-terminal side bottom, single-message log) fill naturally with more entities/messages. Do not commit or push unless asked.

## Merge `origin/rendering_gra_update` into `main` (2026-10-13)

Request: fetch the `rendering_gra_update` branch and merge both lines, prioritising the better features/optimisations.

Pre-merge finding: the local worktree showed 161 modified files, but `git diff --ignore-cr-at-eol` was empty — pure CRLF-vs-LF noise (worktree CRLF, blobs LF; no `.gitattributes`, `core.autocrlf` unset), zero content changes. The only real local item was untracked `play_notcurses.sh` (Arch Linux notcurses launcher, kept). Their branch had already merged `origin/main` (`4ff35f2`) and added one feature commit (`59d50c3`), so it was a strict superset of local content — nothing to reconcile, take-theirs was the correct resolution on every hunk. User confirmed: discard CRLF noise, merge, keep the script.

Changed (fast-forward `4ff35f2..59d50c3`, no conflicts, +1509/-415 across 13 files, `client/src/partitions.rs` new): strict queued-command 20 Hz scheduler (`MAX_PENDING = 512`, `Simulation::advance_tick_with`, burst coalescing, immediate protocol-error rejection; resolves the old input-driven overshoot documented in prior `AGENTS.md`), pause overlay (`P`, `ActiveModal::Paused`, client-side only), single-buffer Ratatui composed frame (exactly terminal-sized output, `Clear`-occluded modal, side radar/minimap, status card, compact legend), quiet GPU terrain/grass ramps (`░▒▓█` light pools, `|` edges, letter detail on entities), adaptive SSAA + native 1:1 grid + ANSI quantization + hash dirty-line diffing, `AGENTS.md`/ADR-0002/SOT updates to match.

Verification (2026-10-13, Linux): `cargo fmt --all -- --check` clean; `cargo test --workspace` all pass, 0 failed (client 74, server 8, shared 14+3+2); `cargo run -p ascii-mmorpg-tools -- content` → `content ok: 11 definitions`; `cargo clippy --workspace --all-targets -- -D warnings` clean; `cargo run -p ascii-mmorpg-client -- --demo` exit 0. Local `main` now at `59d50c3`, ahead of `origin/main` by 3 commits; untracked `play_notcurses.sh` preserved. Not pushed.

Next action: push `main` to `origin/main` only when asked; then live visual check (`play_ansi.bat` + `P`, `I`, `M`) for color/motion. Do not commit or push unless asked.

## Notcurses input diagnosis (2026-10-13, Linux)

Symptoms reported: build error on Linux + dead controls via `play_notcurses.sh`.

Build: NOT reproduced. `cargo build --workspace` (dev, default incl. `gpu`), `cargo build -p ascii-mmorpg-client --features notcurses` (dev), and `cargo build --release -p ascii-mmorpg-client --features notcurses` (the script's exact build) all exit 0. The loud yellow `libnotcurses-sys` warnings (unused imports, fn-pointer comparisons) look like errors but are non-fatal — likely what was read as a build failure. If a different error was seen, its exact text is still needed.

Controls: REPRODUCED under a pty (`script -qec`, full logs in `/tmp/opencode/`). Delayed-`q` test: ANSI client (`target/debug/ascii-mmorpg-client --fps 30`) exits 0 on `q` — input fine. Notcurses client (`target/release/... --notcurses --fps 30`) ignores `q` and runs until SIGKILL — all keys dead, including quit. (First attempt with an immediate piped `q` was inconclusive for both modes: the startup stale-input drain in `run_realtime_with_terminal` eats pre-start bytes, so the key must arrive after startup.)

Root cause: in `--notcurses` mode `NotcursesTerminal::new()` (`client/src/lib.rs:320`) brings up notcurses, which owns terminal input on the same fd, while the game loop reads keys via crossterm `event::poll/read` (`client/src/main.rs:356`) — crossterm starves, so no key (move/attack/modal/quit) ever registers. The code never calls any notcurses input API.

Fix direction (next slice, not implemented here): in notcurses mode read input via `notcurses::Notcurses::poll_event()` (non-blocking, exists in v3.6.0 per registry source) and map `Input` (chars, `NcKey` arrows, Esc) onto the existing key handling; leave the crossterm path for the ANSI client. No code changed this turn, so no fmt/test/clippy run — only builds + behavioral repro above.

Next action: implement the `poll_event()` input route for `run_realtime_notcurses`, re-run the delayed-`q` pty test plus `WASD` movement, then the full CI check order. Do not commit or push unless asked.

## Notcurses controls fix (2026-10-13, Linux)

Symptoms: `play_notcurses.sh` showed a wall of warning "code" at build, then the game rendered but no key worked (not even `Q` to quit).

Diagnosis corrections: dev/release builds (default and `--features notcurses`) all exit 0 — the "build error" was the `libnotcurses-sys` warning wall, now hidden by the launcher (build log prints only on failure). The earlier pty "proof" of dead keys was invalid: without a real terminal answering its init queries, the notcurses client never leaves `Notcurses::new()` (futex-blocked, 0% CPU under `script`/tmux), so no key test could reach the loop there. The stdin-contention root cause stands by construction and is now fixed (below).

Changed (`client/src/lib.rs`, `client/src/main.rs`, `play_notcurses.sh`, `AGENTS.md`):
- `Terminal` trait gains `next_key()` (default: crossterm poll/read) and `decay_held_keys()` (default false). `NotcursesTerminal` overrides `next_key()` via `notcurses::Notcurses::poll_event()` (verified `Ok(NoInput)` on empty; only real failures `Err`) plus `map_notcurses_input()` (`Received::Char/Key` → arrows/Esc/Enter, `InputType` → Press/Repeat/Release, modifiers mapped), and sets `decay_held_keys() = true` since releases need kitty protocol.
- Game loop + startup drain read keys only through `terminal.next_key()`; held movement keys decay after each consumed 90 ms move tick in decay mode (OS repeat keeps held movement smooth, taps step once, keys can never stick). ANSI path is byte-identical behavior.
- GPU-fallback notice moved from stderr (`[WGPU] ...` splattered as "code" on the fullscreen) into `world.messages`.
- `play_notcurses.sh` hides successful build output; prints the log only on failure.
- Drive-by: `notcurses_error()` uses `ErrorKind::Other` (pre-existing `clippy::incompatible_msrv` failure under `--features notcurses`, which CI never compiles); `AGENTS.md` gains the input-routing one-liner.

Verification (2026-10-13, Linux): `fmt --check` clean; `cargo test --workspace` all ok (14 suites, 0 failed); `cargo test -p ascii-mmorpg-client --features notcurses` 77 pass incl. 3 new mapping tests; `tools content` → 11 definitions ok; `clippy -D warnings` clean both default and `--features notcurses`; `cargo run -- --demo` exit 0. End-to-end in a REAL foot terminal: `wasd` + `q` via `wtype` → clean exit 0 (quit key processed through the new path). Script UX re-verified headless: quiet build, one-line error + `Game finished with code 1` when no TTY. Harness scripts kept outside the repo at `/tmp/opencode/nc_harness.py` (+ transcript). Investigation notes: system libnotcurses is 3.0.17 vs 3.11.0-era bindings, but init+render+input all work against it in foot, so no ABI action needed.

Changed files: `client/src/lib.rs`, `client/src/main.rs`, `play_notcurses.sh` (untracked), `ascii-mmorpg/AGENTS.md`, this handoff. Not committed/pushed.

Next action: user re-runs `bash play_notcurses.sh` on their terminal and confirms `WASD` moves, `I/C/M/P` modals open, `Q` quits with no pre-render error text. Do not commit or push unless asked.

## Engine slice 1: backup + `ascii-mmorpg-engine` scaffold (2026-10-13)

Request: back up the codebase, then start a terminal-independent animated-ASCII engine (animated cells, waves like `data/videos/`, three.js-inspired layering/partitions), all in Rust.

Backup (verified): `../ASCII-Game-backup-2026-10-13-pre-engine/` (118M: `ascii-mmorpg/` incl. `.git` history, `data/`, root `AGENTS.md`; `target/` excluded). Backup worktree shows the same dirty state as live (3 modified + `play_notcurses.sh` untracked), HEAD `59d50c3`.

New crate `engine/` (`ascii-mmorpg-engine`, zero deps, in workspace members): `Rgb` (lerp/scale), `Cell` (glyph+fg/bg/bold+transparent), `Track::{Wave,Cycle,Pulse}` + `Anim` + `sample(anim,x,y,t)` (the mixer), `AnimCell`/`Layer` (z-ordered strata), `Partition` (named viewport rects with layer subsets + world view origin / camera follow), `Scene`/`Camera`, `compose(scene,camera,t)->Frame` (bottom-up z, per-cell sampling, partition clipping, topmost opaque wins). three.js mapping documented in `engine/src/lib.rs` (scene graph→layer stack, material→Cell, clip/track/mixer→Anim/sample/compose, scissor viewports→Partition). Headless proof: `cargo run -p ascii-mmorpg-engine --example wave` prints 8 frames of a left-to-right water wave + breathing/cycling torch + sidebar partition.

Verification (2026-10-13, Linux): `fmt --check` clean; engine 11 tests pass (wave travel/degenerate/period, cycle order, pulse bounds, top-wins, transparency, camera scroll, partition clip); full `cargo test --workspace` 114 passed 0 failed; `tools content` ok; `clippy --workspace --all-targets -D warnings` 0 errors.

Changed: `Cargo.toml` (+engine member), `engine/*` (new), `ascii-mmorpg/AGENTS.md` (workspace line), this handoff. Not committed/pushed.

Roadmap (next slices, in order): 1) windowed presenter — pick stack (`winit`+`wgpu` glyph atlas vs `pixels` framebuffer; ask user), presenting `Frame`s at 60 Hz with sub-cell smoothing; 2) engine gaps: text runs, easing/noise tracks, border/fill helpers, importing entity sprites from `assets/ascii/`; 3) game migration: drive engine `Scene` from `WorldSnapshot` (client) side-by-side with current renderer; 4) retire terminal-only paths. Do not commit or push unless asked.

## Engine slice 2: windowed presenter, winit + wgpu atlas (2026-10-13)

Choice (user): `winit 0.30` + `wgpu 24` instanced glyph atlas over the simpler `pixels` framebuffer.

Changed (`engine/`, all new except noted): `assets/LiberationMono-Regular.ttf` (312 KB, SIL OFL, attribution in `assets/README.md`) + `present/atlas.rs` (fontdue raster → 16x16 R8 coverage atlas, slot = codepoint, `?` fallback incl. emoji note); `present.rs` (`PresentOptions`, `run(scene,camera,opts)->Result<u64>` presented count, winit `ApplicationHandler` loop at 60 Hz vsync, per-cell instances fg/bg/glyph, Nearest sampler, Esc/Q/close/`--frames N` exit, non-resizable window); `demo.rs` (shared `wave_scene()` now used by both `wave` headless and `present` windowed examples); `examples/present.rs`; `Cargo.toml` `present` feature (winit/wgpu/fontdue/pollster/bytemuck, all optional, default empty so CI is unaffected); `AGENTS.md` presenter note; torch bgs warmed from pure black to ember-dark after screenshot review.

Findings that cost real time (recorded so nobody re-pays): wgpu 24 needs `PipelineCompilationOptions::default()` on both shader stages and `SurfaceError::Other` handled; texture upload must go staging-buffer→`copy_buffer_to_texture` (a pitch-wide texture silently breaks UVs — first screenshot showed blocky glyphs); the GLES backend segfaults in mesa `eglTerminate` at teardown (gdb-verified `wgpu_hal::gles` drop) → instance is Vulkan-only now, which also fixed the exit crash; the earlier `script`/tmux "dead keys" evidence was a headless-pty init artifact, not product behavior. Note: killing the demo with SIGTERM pops a desktop "process crashed" notification — always exit via `--frames`/keys instead.

Verification (2026-10-13, Linux/Wayland): `fmt --check` clean; engine+present 12 tests pass; `cargo test --workspace` 114 passed 0 failed; `tools content` ok; `clippy -D warnings` clean default and `--features present`; `present -- --frames 240/200/120` all exit 0 with matching presented counts; grim screenshots prove crisp glyphs, traveling wave, cycling/breathing torch, sidebar partition (zoom on torch), `===` bed.

Changed files: `engine/Cargo.toml`, `engine/src/{lib,demo,present}.rs`, `engine/src/present/atlas.rs`, `engine/examples/present.rs`, `engine/examples/wave.rs` (now uses `demo::wave_scene`), `engine/assets/*`, `Cargo.lock` (new deps), `ascii-mmorpg/AGENTS.md`, this handoff. Not committed/pushed.

Next action: engine slice 3 — text runs + easing/noise tracks + border/fill helpers + importing `assets/ascii/` sprites as engine layers, so real game content (not just the wave demo) can be authored in the new model. Do not commit or push unless asked.

## Engine slice 3: text/border helpers, Ease/Noise tracks, sprite importer (2026-10-13)

Changed (`engine/src/anim.rs`, `layer.rs`, `sprite.rs` new, `lib.rs`, `examples/sprite.rs` new, `AGENTS.md`, this handoff):
- `Track::Ease { low, high, period, phase, kind: EaseKind::{Linear,In,Out,InOut} }` — looping eased brightness ramp (fades, drains, menu slides); degenerate period holds still.
- `Track::Noise { amp, period, seed }` — stepped positional jitter from an integer hash of (cell, time-step, seed): deterministic across platforms/runs, bounded by `amp`, brightness untouched; degenerate params hold still.
- `Layer::push_text`/`push_text_bold` (spaces stay transparent holes, `\n` multi-line, `\r` ignored), `fill_rect`, `border_rect` (`+`/`-`/`|` with degenerate sizes drawing what fits).
- `sprite::` module: `parse_frames` (same line-based `---` + CRLF-tolerant semantics as `client/src/assets.rs`), `sprite_size` (char-counted, multi-byte safe), `sprite_layer(name, z, frames, SpriteDraw { x, y, fg, bg, fps })` (per-cell `Cycle` flip-book only where glyphs vary; all-space margins skipped; uniform `fg`/`bg`). Spaces-in-some-frames flash opaque — documented, matches shipped dense art. `SpriteDraw` struct keeps the constructor under the clippy arg limit.
- `examples/sprite.rs`: warrior sprite flip-books inside a bordered+labelled panel (headless proof real game content composes). `lib.rs` re-exports new API.

Verification (2026-10-13, Windows): `fmt --check` clean; `cargo test --workspace` all pass (client 74, engine 19 incl. 7 new, server 8+3, shared 14, tools 2, bot 1); `tools content` → 11 definitions ok; `clippy --workspace --all-targets -D warnings` clean; engine `clippy --features present` clean; `cargo check -p ascii-mmorpg-engine --features present` ok; `client -- --demo` exit 0; `engine --example sprite` prints 4 warrior poses (0/2 and 1/3 are frame pairs — test steps 0.25 s, not 0.5 s). Two self-caught test bugs fixed during the slice (border push order, warrior pair timing). Not committed/pushed.

Next action: engine slice 4 — drive an engine `Scene` from a live `WorldSnapshot` (client side-by-side with the current renderer); then retire terminal-only paths. Do not commit or push unless asked.

## Port slice 1: snapshot→Scene 2.5D bridge + windowed `game` example (2026-10-13)

Request: make the windowed experiment 2.5D like the terminal game; do the full port. Scoped to slice 1 (static scene proof); live updates + input stay next.

Changed (`engine/src/bridge.rs` new, `engine/src/lib.rs`, `engine/examples/game.rs` new, `AGENTS.md`, this handoff):
- `bridge::` keeps the engine dependency-free: callers map their snapshot onto `SceneView { player, actors, messages, tick, zone }` (`SceneActor { x, y, kind, name, glyph, hp, max_hp }`, `ActorKind::{Player,Monster,Npc,Item}`); `scene_from_view` returns `(Scene, Camera)` centered on the player.
- 2.5D look, all grid tricks: fixed-extent meadow (stable under camera) with depth-shaded grass, cobble road, wave pond, pines with vertical canopies + trunks, rocks, pulsing/flip-book torch; rain layer *below* actors (a wave-displaced drop covered `@` at z=4 — caught by test); actor shadows (south row), floating centered nameplates, HP pips while damaged (player always); faction palette mirrors the client dispatch (skeleton bone, champion gold, mossling green); pinned HUD (zone/HP/tick/legend) + log partitions.
- `demo_view()` mirrors the terminal client's `sample_world` cast; `game` example presents 64x23 (`VIEW_W` x `HUD_H+VIEW_H+LOG_H`, exact window fit) with `--frames` proof flag.
- Deferred + documented: per-entity volumetric `sprite::` art (one fighter would fill a quarter of a 64-wide grid), sparse-overlay sprite spaces, label declutter (adjacent plates overwrite — demo cast spaced out instead), live snapshot refresh, key input.

Verification (2026-10-13, Windows): `fmt --check` clean; `cargo test --workspace` all pass (engine 23 incl. 4 new bridge, client 74, rest 0-fail); `tools content` → 11 definitions ok; `clippy --workspace --all-targets -D warnings` clean; engine `clippy --features present` clean; `game -- --frames 300` → `presented 300 frames`, exit 0 (window "ASCII MMORPG — windowed 2.5D port" shown to user for visual judgment). Not committed/pushed.

Next action: port slice 2 — live snapshot refresh (rebuilt or diffed scene per tick from a real `WorldSnapshot`, local or TCP) + WASD/F/Q key input through the winit loop; keep the terminal client untouched side-by-side. Do not commit or push unless asked.

## Port slice 2: playable windowed port — live sim + key input (2026-10-13)

Changed (`engine/src/present.rs`, `engine/Cargo.toml`, `engine/examples/play.rs` new, this handoff):
- Presenter: new `SceneSource` trait (`scene_at(t)` per frame + `key(code, pressed)` input; Esc/Q/close still always exit) and `run_dynamic(source, opts)`. Existing `run(scene, camera, opts)` kept via a `StaticSource` adapter — `wave`/`game` examples untouched.
- `play` example: a local server `Simulation::demo()` ticks at 20 Hz inside the frame loop (`FixedTickClock`, 5-tick stall cap with backlog dropped like burst coalescing); WASD/arrows held-key 8-way movement (opposites cancel, same 16-state resolution as the terminal client), F attack / R rest edge-triggered, Q/Esc quits. Snapshots map to `bridge::SceneView` each frame (zone parsed from the `Zone:`-prefixed first message). `--frames N` automated proof; `--check` headless self-test (F damages adjacent mossling, held-East advances player + camera follows) — no window needed.
- `engine/Cargo.toml`: server + shared as `[dev-dependencies]` (examples-only); the library stays free of game crates.

Verification (2026-10-13, Windows): `fmt --check` clean; `cargo test --workspace` all pass (no new lib tests — example logic proven by `--check`); `tools content` → 11 definitions ok; `clippy --workspace --all-targets -D warnings` clean; engine `clippy --features present` clean; `play -- --check` → `play check ok`; `play -- --frames 300` → `presented 300 frames`, exit 0 ("ASCII MMORPG — playable windowed port" window shown to user for live WASD/F testing). Terminal client untouched. Not committed/pushed.

Next action: port slice 3 — TCP mode for the windowed client (`--connect ADDR`: socket reader thread + snapshot-driven `SceneSource`, server keeps authority) and/or inventory/character/map modals + chat input; keep terminal parity. Do not commit or push unless asked.

## Reference-video study: what fidelity/animation the target wants (2026-10-13)

Studied all 6 clips in `data/videos/` (12 evenly-spaced frames each, contact sheets in `%TEMP%\opencode\frames\`) plus `RESEARCH_ASCII_GRAPHICS_FIDELITY.md` and both Downloads docs. No code changed.

Confirmed from my own frames: ~110x128 portrait grid; entities 25–30 rows tall; dragon with aligned-row glyph shading + red boss bar (`Dragon N/6000`, `phase 2/3` labels) + orange ring telegraphs + damage numbers; branching node lightning; orange burst core with scattered `O/+/#/%` + elliptical hash ring; persistent ice-spike rows; radial torch/lantern falloff lighting nearby glyphs; rain as screen-space diagonal streaks; crisp white nameplates/bars/panels composited over the glyph world; player-follow camera; inventory paper-doll + bag grid + tooltips as clean UI.

Note: the brief's Video 4/5 durations (31.83/29.83 s) are swapped relative to our files — our `(1).mp4` is 29.83 s (dungeon) and `39 PM.mp4` is 31.83 s (outdoor/castle). Contact sheets `v4_outdoor`/`v5_dungeon` carry the same swap; content identification above uses on-screen captions, not filenames.

Gap to our port (ordered): 1) density/scale — reference actors are 25+ rows, ours are 1 glyph on 64x23; 2) no light buffer/glow hybrid — reference mixes glyph particles with smooth radial glow; 3) no particle systems (burst/ring, chain lightning, spike rows, damage numbers, telegraphs); 4) weather is world-locked, reference rain is screen-space; 5) UI layering we already match. Proposed order: light buffer + glow → particle archetypes → larger volumetric entity sprites + denser grids → boss/target panels. Full write-up given as the turn's final answer, not a new doc file.

## Grass-motion verdict + smooth-turn requirement (2026-10-13)

Grass: dense-frame test (3 fps tiles + a ⅓ s full-res blink pair) shows grass as a stable textured field at sub-second scales — no resolvable per-blade displacement; the clearly-moving things are dragon pose, particles/rings, and amber↔green lighting washes. (One blink crop landed on the moving dragon edge, not clean grass — recorded so nobody over-claims.) Working model: grass sway is slow, low-amplitude shimmer (brightness/phase drift over seconds), which matches our Wave+brighten tracks, not per-frame displacement.
User requirement: character orientation changes read as one smooth continuous sweep (a line visibly traveling left-to-right across the body), never a discrete 2-pose snap. Our warrior flip-book is 4 frames / 2 unique pairs at fps 4 — the opposite of this. Fix direction: turns as ~0.3 s Ease-driven morphs with per-column phase stagger (the sweep), walk as higher-fps Cycles with real in-betweens; our 60 Hz realtime sampling already supports it. Prototype next.

## Dense-strip verdict: full-rate skeletal motion (2026-10-13)

Pulled 6 fps strips (`sweep_v3.jpg` t=4–7 s spell test, `sweep_v6.jpg` t=12–15 s multiplayer) after the user pointed at these two clips for the smooth-motion requirement. V3 shows it unambiguously: the ~25-row pale avatar changes limb/torso pose in EVERY ⅙ s frame (arms rising into the cast gesture across the strip) — full-video-rate skeletal animation, not a flip-book; the lightning stroke visibly extends/evolves across consecutive frames with pulsing node glows rather than flashing as one sprite. V6 shows lime figures gliding continuously with tracking labels + rain.
Engine consequence: discrete `Cycle` glyph-swaps at low fps cannot reproduce this; actors need pose interpolation at display rate (authored keyframe poses ≥8 phases sampled continuously, or per-cell offset maps), lightning needs an evolving-polyline effect with node glow, and sweeps come from traveling phase. This supersedes the "higher-fps Cycles" half of the previous note — in-betweens must be real poses, not faster stepping.

## Full-frame motion curve, Video-47489 (2026-10-13)

Measured every one of the 1,651 frames (30 fps, 55 s): mean per-pixel brightness delta vs previous frame (0–255). Mean 3.08, min 0.01, max 29.97; p50 1.65, p90 8.11, p99 13.70. Read: motion is near-continuous — half of all frames shift by ≥1.65 levels on near-black footage (thousands of glyphs), the top decile is effects (8+), spikes ≈30 are flashes/cuts; a few near-static frames exist (min 0.01 — quiet beats, likely UI-open moments), so "never static" is softened to "idle is drift, action is 10–15 distinct steps/sec."
Engine budgets derived: effect attack ≈10 frames (0.33 s) to full bloom, release ≈35 frames (1.2 s) to rest; actor poses must not repeat within any ⅙ s window during motion; idle shimmer must keep frame delta > 0. Combined with the dense-strip reads (cast growth, burst bloom/ring/release), this is the quantitative smoothness target.

## Reference shader/lighting forensics (2026-10-13)

Full-res crops (`sh_lightning/burst/dragon.png` in temp frames): the lightning core is an anti-aliased smooth vector stroke with a radial glow halo + sparse dim glyph sparks around it; burst `O` particles vary in SIZE (world-space scaled billboards, not grid cells) over a smooth bright core; the dragon wing is rigid screen-aligned rows of flat-shaded mosaic cells with no letterforms, while grass is oriented thin glyph strokes — two surface treatments in one frame. Reconstructed model: 3D scene + dynamic point lights with smooth radial falloff (verified in dungeon frames) → mosaic quantization for large volumes, oriented glyph strokes for detail, smooth-core + glow + scaled glyph billboards for effects, crisp UI composited last. Screen-vs-world glyph lock still unresolved (needs stabilized footage).
Deltas for our WGSL, in order: 1) glow bypass — cells past a bloom threshold render as solid quads + radial halo before quantization (biggest change); 2) flat mosaic mode for volume materials (our ░▒▓█ bands approximate it); 3) scaled glyph billboards for sparks (windowed presenter can scale via atlas quads; terminal grid cannot — fidelity argument for the windowed port); 4) orientation-field grass strokes instead of hash-random; 5) verify point-light falloff curve + add amber wash uniform.

## Shader port slice: glow, mosaic, billboards, flow strokes (2026-10-13)

Implemented reference items 1–4 in the windowed engine (terminal WGSL untouched this slice).
Changed (`engine/src/cell.rs`, `compose.rs`, `layer.rs`, `present.rs`, `present/atlas.rs`, `bridge.rs`, `demo.rs`, `examples/fx.rs` new, `AGENTS.md`, this handoff):
- `Cell::{glow, scale}` (`with_glow`/`with_scale` builders; `CLEAR` emits nothing). `compose::apply_glow` light buffer: every stamped glowing cell washes its fg hue onto neighboring bgs with quadratic falloff (radius = glow×4), post-partition so halos cross borders; writes bgs only, deterministic, transparent-safe.
- Atlas 16×16 → 32×16: curated extras (`█░▒▓`, box drawing, ■) rasterized at slots 256+ (`slot()` resolves them, rest → `?`); shader UV math updated to match. `Layer::fill_shaded_rect` + `Shade` struct: vertical-gradient mosaic volumes (dragon-wing rows).
- Presenter billboards: per-instance `scale` (centered quad scaling, clamped 0.25–2.0; sub-cell margins fall back to clear — documented dark-scene use).
- Bridge: flow-field grass strokes (coherent 4×4 orientation patches, camera-stable), torch flame glow 1.0, player aura 0.35, loot glint 0.5.
- `examples/fx.rs`: scripted 2.4 s showcase (graded mosaic panel, halo torch, evolving lightning polyline with attack/hold/release envelope + node blooms + breathing spark billboards) with `--check` (bolt grows then fully releases) and `--frames` proof.
- Clippy repairs inside the slice: `Shade`/`SpriteDraw`-style param bundling, bool assert, plus a self-broken `impl Layer` brace fixed before verify.

Verification (2026-10-13, Windows): `fmt --check` clean; `cargo test --workspace` all pass (engine 26 incl. glow×2/shade/atlas-ink); `tools content` ok; `clippy --workspace --all-targets -D warnings` clean; engine `clippy --features present` clean; `fx -- --check` ok; `fx -- --frames 240` → `presented 240 frames`, exit 0; `play -- --check` still ok. Not committed/pushed.

Next action after this completed emitter slice: amber wash + verified point-light falloff, then port slice 3 (TCP windowed client). Do not commit or push unless asked.

## Data-driven particle emitter slice (2026-10-13)

Scope: completed section E item 1 only. Added `engine/src/fx.rs` and exported it from `engine/src/lib.rs`; migrated `engine/examples/fx.rs` from scripted bolt/spark construction to persistent `Emitter` records sampled by `fx::layer`. The five records are `Burst`, `Ring`, `Chain`, `Spike`, and `Number`. Sampling is pure and explicit-time, with deterministic seeded burst placement, growth/release timing, foreground/glow fade, preserved scale, invalid-input rejection, coordinate guards, and a 4096-cell safety cap. Existing mosaic and torch showcase content remains.

Changed this turn: `engine/src/fx.rs`, `engine/src/lib.rs`, `engine/examples/fx.rs`, and this handoff. Existing tracked modifications and untracked `engine/`/`play_notcurses.sh` work were preserved. No commit or push was performed.

Verification:
- `cargo fmt --all -- --check`: passed.
- `cargo test --workspace`: passed; client 74 + 1 binary, engine 30, server 8 + 3 binary, shared 14, tools 2, bot 1; doc tests passed.
- `cargo run -p ascii-mmorpg-tools -- content`: passed with `content ok: 11 definitions`.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `cargo test -p ascii-mmorpg-engine --features present fx`: passed, 4 FX tests.
- `cargo check -p ascii-mmorpg-engine --features present --examples`: passed.
- `cargo run -p ascii-mmorpg-engine --features present --example fx -- --check`: passed with `fx check ok: five data-driven effects grow and release`.
- `cargo run -p ascii-mmorpg-engine --features present --example fx -- --frames 1`: passed with `[present] presented 1 frames` and `presented 1 frames`.
- No blockers. Cargo emitted temporary package/build lock waits because checks were launched in parallel; all commands completed successfully.

Next action: implement the amber wash uniform and verify point-light falloff, or, if visual evidence changes the priority, add the stabilized screen/world glyph-lock test. Keep TCP windowed-client port work deferred until that bounded lighting/lock slice is complete.

## Art slice: reference-style character models + tree (2026-10-13)

Re-authored `assets/ascii/{player_warrior,monster_mossling,skeleton,skeleton_warrior,tree}.txt` toward the forensic spec: dense upper-left key-light shading (`@▓8%` → `o*+` → sparse `:'` contours), articulated stride phases, skeletons grown 12→16 rows, tree 2→4 sway phases (MANIFEST updated), warrior kept 24 rows. All test marker strings preserved in frame 0 (`|()|`/`|<>|`, `(O O)`+`[||===||]>`, `||===||`, `(^^^^)`); `client/src/assets.rs` gains `animation_frames_never_repeat_within_a_cycle` (all 4 frames unique per model + tree — encodes the no-repeat rule). Unchanged: npc/ruin/gate/items/spell art, GPU procedural meshes, bridge 1-glyph actors (volumetric bridge actors stay E.4).

Verification (2026-10-13, Windows): `fmt --check` clean; `cargo test --workspace` all pass (client 75 incl. new uniqueness test); `tools content` ok; `clippy --workspace --all-targets -D warnings` clean; `client -- --demo` exit 0; `engine --example sprite` renders new warrior correctly. Not committed/pushed.

## CONCURRENCY WARNING (2026-10-13, ~23:10)

A second editor is working in this SAME worktree concurrently: `engine/src/fx.rs` (data-driven `Emitter`/`Effect::{Burst,Ring,Chain,Spike,Number}` layers — i.e. section E.1), `color.rs` rainbow/`ColorTrack`, `compose.rs` dynamic glow, `bridge.rs` actor-model extensions, and `examples/{game,play}.rs` updates all landed interleaved with this session's timestamps. No conflicts with the art slice (disjoint files); full `cargo test --workspace` passes together (engine 40 tests). Before the next slice: re-read Master state B, `git status`, and the new `fx::` API — do NOT reimplement emitters, and do NOT overwrite `engine/src/*` without diffing first.

## Copyable handoff prompt (current — use this for the next session)
```text
Read `D:\Personal\codes\Projects\ASCII-Game\AGENTS.md`, `D:\Personal\codes\Projects\ASCII-Game\ascii-mmorpg\AGENTS.md`, and `D:\Personal\codes\Projects\ASCII-Game\ascii-mmorpg\docs\SESSION_HANDOFF.md` (start with the Master state section and the latest slice section). Inspect the current worktree without disturbing existing changes — HEAD 59d50c3, tracked modifications plus untracked `engine/` and `play_notcurses.sh`, nothing pushed. Complete the first pending bounded task from section E: amber wash uniform and verified point-light falloff, with a focused screen/world glyph-lock test only if the existing evidence supports it. Verify with the project checks plus `--features present` checks and relevant `--frames`/`--check` proofs, update the handoff and TODO checklist, and finish with a fresh-session handoff prompt. Do not commit or push unless requested.
```

## Dynamic actor colour and glow (2026-10-13, latest)

User requested non-fixed character colours, rainbow gradients, and glow illuminating backgrounds. Implemented the bounded colour-track/compositor foundation rather than a global ambient wash. `ColorTrack::{Static,Gradient,Rainbow}` samples explicit time and layer-local coordinates, `AnimCell::with_color` overrides foreground, and `SceneActor.palette` overrides kind-based defaults. Existing constructors retain static colours. Compose applies palette then brightness then background glow. Demo actors include different palettes; `play` mapping uses `None` to retain its original appearance. Glow now rejects nonfinite intensities and caps intensity at 8 (radius 32), preventing extreme unbounded loops.

Changed paths (relative to repository): `engine/src/color.rs`, `engine/src/layer.rs`, `engine/src/compose.rs`, `engine/src/lib.rs`, `engine/src/bridge.rs`, `engine/examples/play.rs`, `docs/SESSION_HANDOFF.md`. No dependencies, server/protocol or terminal renderer changes; no commit/push. Two implementation agents handled core and bridge; an API naming mismatch was resolved to builder-style `.with_color(track)` / `.colored(track)`.

Commands/results:
- `cargo fmt --all`, then `cargo fmt --all -- --check`: passed.
- `cargo test --workspace`: passed after final fixes; engine 37, client 74+1, server 8+3, shared 14, tools 2, bot 1; doc tests pass.
- `cargo run -p ascii-mmorpg-tools -- content`: `content ok: 11 definitions`.
- `cargo clippy --workspace --all-targets -- -D warnings`: initially failed on `clone_on_copy` in the new bridge code; removed redundant clone and reran successfully.
- `cargo test -p ascii-mmorpg-engine --features present`: passed 37 tests before the final glow-bound regression addition; final default workspace run includes that new regression.
- `cargo clippy -p ascii-mmorpg-engine --features present --all-targets -- -D warnings`: passed after final fixes.
- `cargo run -p ascii-mmorpg-engine --features present --example game -- --frames 60`: exit 0, presented 60 frames (smoke, not visual-quality certification).
- `cargo run -p ascii-mmorpg-engine --features present --example play -- --check`: passed movement/combat checks.

Remaining: palette currently resolves ONE RGB per glyph per frame. Spatial gradients span multiple cells, rainbow varies over time; simultaneous rainbow colours inside a single glyph require presenter fragment shading. Glow remains a background-only screen-grid spill, not shadowed physical lighting. Amber wash remains pending; this slice must not be described as completing section E lighting wholesale. No environmental blocker.

Next action / fresh-session prompt: Read both AGENTS.md files and this handoff (Master state and this latest section). Preserve the worktree. Implement a bounded within-glyph gradient material in the windowed presenter, with deterministic colour/gradient checks and a focused showcase; preserve flat-colour defaults and colour-driven glow. Run workspace checks plus present-feature checks and windowed smoke, update handoff/TODO, and do not commit or push.

## Fullscreen demo, controls, models, and overlays (2026-10-13, latest)

User requested a significantly better fullscreen demo, working controls, richer models, and consistent text overlays. Implemented:
- `engine/src/present.rs`: default borderless fullscreen; explicit nonzero `width_cells`/`height_cells` remains windowed; live surface dimensions derive the glyph grid; resize reallocates instance capacity when needed; F11 toggles fullscreen; Q/Esc exit; repeated key presses are ignored while press/release edges are forwarded once; `SceneSource::scene_at_size` lets sources adapt scene layout to the actual grid.
- `engine/src/bridge.rs`: expanded meadow bounds for fullscreen coverage; multi-cell models for player, mossling, skeleton, herbalist, and sword/item; per-part palette/glow retained; sized bridge layout with clipped/padded HUD and log panels and collision-aware nameplates.
- `engine/examples/game.rs`: now uses a dynamic fullscreen source; WASD movement works with accumulated sub-cell motion, F/R write readable overlay messages, and the existing `--frames` proof remains.
- `engine/examples/play.rs`: playable simulation now uses the sized bridge path on live grids as well as its existing controls/combat.

Changed paths this turn: `engine/src/present.rs`, `engine/src/bridge.rs`, `engine/examples/game.rs`, `engine/examples/play.rs`, and this handoff. Existing tracked/untracked work was preserved. No commit or push.

Verification:
- `cargo fmt --all -- --check`: passed after formatting an initial game-example diff.
- `cargo test --workspace`: passed; client 74 + binary, engine 40, server 8 + 3 binary, shared 14, tools 2, bot 1; doc tests passed.
- `cargo run -p ascii-mmorpg-tools -- content`: `content ok: 11 definitions`.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `cargo clippy -p ascii-mmorpg-engine --features present --all-targets -- -D warnings`: initially caught a redundant `ColorTrack::clone` and an unused example variable; both were fixed and the rerun passed.
- `cargo test -p ascii-mmorpg-engine --features present --lib`: passed 43 engine tests.
- `cargo run -p ascii-mmorpg-engine --features present --example game -- --frames 60`: passed, presented 60 fullscreen-capable frames.
- `cargo run -p ascii-mmorpg-engine --features present --example play -- --frames 60`: passed, presented 60 frames.
- `cargo run -p ascii-mmorpg-engine --features present --example play -- --check`: passed movement/combat checks.
- `cargo run -p ascii-mmorpg-engine --features present --example fx -- --check`: passed.

Known limitation: automated smoke proves window creation/rendering and the playable self-check proves controls through the source, but it does not physically press keys in a real window. Manual confirmation should press WASD, F, R, F11, and Q/Esc. Within-glyph multi-colour fragment shading is still pending; current dynamic palettes resolve one RGB per glyph while the richer model uses multiple cells and glow.

Next action / fresh-session prompt: Read both AGENTS.md files and this handoff. Preserve existing work. Implement within-glyph gradient material support in the presenter shader/instance data, add deterministic focused tests and a visual showcase, then run workspace checks, present-feature checks, `game --frames`, `play --check`, and update this handoff. Do not commit or push.

## Current-state review (2026-10-13)

Review-only pass requested before selecting the next implementation slice. The current worktree contains the staged engine/graphics port plus modified client/assets/docs and untracked `play_notcurses.sh`; HEAD remains `59d50c3`, with no commit or push performed. The engine path is currently the strongest demo path: fullscreen-by-default presenter, surface-sized grid, F11/Q/Esc controls, multi-cell bridge models, dynamic palettes/glow, clipped HUD/log, and playable `game`/`play` examples. The terminal client remains a separate, more complete multiplayer path with server authority and GPU renderer.

Review findings:
- No blocking runtime defect was found in the reviewed fullscreen/model/control flow from the checked source and prior successful smoke checks. Manual key presses are still required to certify real OS input because automated `--frames` runs do not press a physical window key.
- The fullscreen demo's WASD movement is presentation-demo state, not server-authoritative simulation; `play` is the authoritative local loopback proof. This is correct for the static `game` showcase but must not be presented as network gameplay.
- Dynamic colour currently resolves one foreground RGB per glyph. Multi-cell models and glow make characters richer, but true within-glyph gradients remain unimplemented and are still the clearest next visual slice.
- Handoff metadata is partly historical: the Master section says strict scheduling while older inherited paragraphs describe the former input-driven behavior; the current repository AGENTS and latest implementation use the strict queued scheduler. Future cleanup should label or consolidate the stale historical text rather than change runtime behavior.
- A read-only `git diff --cached --stat` attempt hit `fatal: unable to read 1fbcc79ed11d6432f0e7c17b3a9aa7fa2630b984`; this is a repository/index object-integrity blocker for a complete staged-diff review. `git status` still listed the expected staged files and untracked engine/launcher work. No repair was attempted.

Checks in this review: no new build/test commands were required; the latest recorded checks immediately before this review passed workspace tests, content validation, workspace clippy, present-feature engine tests/clippy, `game --frames 60`, `play --frames 60`, `play --check`, and `fx --check`. These are inherited evidence, not rerun in this read-only review.

Next action: repair or investigate the missing Git object/index integrity without resetting work, then implement the within-glyph presenter gradient slice if the worktree remains intact. Do not commit or push.

## Within-glyph presenter gradients (2026-10-13, latest)

Implemented the deferred gradient slice after preserving the damaged repository state. `Cell` now carries a second foreground endpoint and optional X/Y gradient axis; existing cells default to identical endpoints and remain flat. The presenter passes both endpoints plus the axis through `GlyphInstance` into WGSL, which interpolates colour across glyph-local UVs before applying atlas alpha. Bridge actor `ColorTrack::Gradient` palettes now populate this metadata, so gradient actors use both multi-cell models and true within-glyph shading. Glow remains based on resolved compositor colour and is unchanged.

Changed paths: `engine/src/cell.rs`, `engine/src/present.rs`, `engine/src/bridge.rs`, and this handoff. No commit or push.

Verification:
- `cargo fmt --all -- --check`: passed.
- `cargo test --workspace`: initial parallel run hit a transient Windows linker `LNK1104` for `target\debug\examples\game.exe`; rerun passed (client 75 + binary, engine 43, server 8 + 3 binary, shared 14, tools 2, bot 1; doc tests passed).
- `cargo test -p ascii-mmorpg-engine --features present`: passed, 48 tests.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `cargo run -p ascii-mmorpg-tools -- content`: `content ok: 11 definitions`.
- `cargo run -p ascii-mmorpg-engine --features present --example game -- --frames 10`: passed, presented 10 frames.
- `cargo run -p ascii-mmorpg-engine --features present --example play -- --check`: passed.
- `git fsck --full --no-reflogs`: still reports 40 missing blobs, including the index blob for `docs/SESSION_HANDOFF.md` and staged engine files. Refs and history remain readable; no reset, index rebuild, object repair, commit, or push was attempted.

Next action: repair the Git object/index integrity safely using a backup index or fresh clone comparison, without resetting or overwriting the worktree. After that, manually validate fullscreen keyboard input (WASD/F/R/F11/Q/Esc) and assess the gradient demo visually. Do not commit or push.

## Volumetric actor art in the windowed bridge (2026-10-14, this session)

Replaced the 3×3 mini-models with compact volumetric art sized for windowed grids (~9–13 rows vs 20+-row views), foot-anchored, focal-steady.

Changed (`engine/assets/actors/{warrior,mossling,skeleton,herbalist}.txt` new, `engine/src/bridge.rs`, this handoff):
- New art set: 4 phases each, `---`-separated, same format as the terminal set. Focal glyph (`@`/`m`/`s`/`H`) held at one local cell in EVERY phase so camera/palette/glow contracts keep working. Items keep the mini `SWORD` cluster (loot glints read better small).
- `bridge.rs`: `cached_frames` (OnceLock per source), `focal_of` (votes for the most-frequent focal cell, ties toward center), `art_for` (source + uniform tint + focal aura per kind: warrior 0.4, npc 0.15, skeleton 0.2, mossling 0.2), `stamp_art` (foot-anchored stamping, phase-varying cells flip-book at 6 fps = one phase per 1/6 s, focal cell keeps palette track + aura). `push_actor` returns the art's top row so plates float above the head.
- Demo cast respread to fit the 64-wide view: player (12,12), mossling (30,12), skeleton (40,17), herbalist (-10,14), sword (4,9).
- Tests: `focal_glyph_stays_steady_every_phase` (new), `actor_models_stamp_multiple_distinct_cells` updated for art (`/▲` crown check). All existing center/palette/glow/sized tests pass unchanged.

Verification (2026-10-14, Windows): `fmt --check` clean; `cargo test --workspace` all pass (engine 41 incl. new focal test, client 75, rest 0-fail); `tools content` ok; `clippy --workspace --all-targets -D warnings` clean; engine `clippy --features present` clean; `game -- --frames 120` → `presented 120 frames`, exit 0. Not committed/pushed.

## Within-glyph gradient + fullscreen presenter slice (2026-10-15)

Implemented by the concurrent session (see CONCURRENCY WARNING below). This session verified only.

Changed (`engine/src/cell.rs`, `present.rs`, `bridge.rs`):
- `Cell` gains `fg_end: Rgb` and `fg_gradient: Option<Axis>`; flat cells use `fg_end == fg` and `None`. `Cell::with_fg_gradient` builder.
- Presenter `GlyphInstance` carries gradient fields; WGSL interpolates `fg_start`/`fg_end` using glyph-local UV. Shader contract/propagation tests added.
- Bridge gradient `ColorTrack` palettes create gradient cell metadata; composed-frame propagation test added.
- Presenter defaults to borderless fullscreen when `PresentOptions.width_cells == 0 || height_cells == 0`; explicit nonzero dimensions remain windowed. F11 toggles fullscreen. `SceneSource::scene_at_size` for dynamic grids.

Verification (2026-10-15, Windows): `fmt --check` clean; `cargo test --workspace` all pass (engine 43, client 75, rest 0-fail); `tools content` → 11 definitions ok; `clippy --workspace --all-targets -D warnings` clean; engine `clippy --features present` clean; `game -- --frames 60` → `presented 60 frames`, exit 0; `play -- --check` → `play check ok`; `fx -- --check` → `fx check ok: five data-driven effects grow and release`. Not committed/pushed.

## Git integrity blocker (2026-10-15)

The server restart on 2026-10-13 corrupted `.git/objects` (loose objects deleted, packfile missing commits 59d50c3+). Non-destructive investigation results:
- Commit `59d50c3` exists as an object (`git cat-file -t` confirms) but its tree references blobs that are missing from both loose objects and packfile.
- `git fsck --full` reports many missing blobs (e.g. `1fbcc79`, `23a91d9`, `ccfe6bbe`).
- `git diff --cached` fails because the index references missing blobs.
- The remote `origin/main` is at `4ff35f2` (older); `origin/new-rendering-engine` exists.
- The working tree is fully intact — all code compiles and passes tests.

Recovery options (require user decision):
1. Reinitialize git from working tree (squash all work into one commit on new branch).
2. Attempt `git unpack-objects` from packfile (low probability — pack is damaged).
3. Check if another worktree/backup has the missing objects.

Do NOT reset, delete, rebuild index, or fetch destructively without explicit user approval.

## Amber wash + verified light falloff (2026-10-15, this session)

Measured the reference burst halo (Video-47489 t=10.5 s, concentric-box signalstats): halo excess ≈15 brightness levels one cell out, fading by ~5 cells (~40 px at 8 px/cell), peak in the spark ring. Our old glow washed 163 levels at distance 1 (the windowed blob in the user's screenshot).

Changed (`engine/src/scene.rs`, `compose.rs`, `present.rs`, `examples/fx.rs`, this handoff):
- `Scene.wash: Option<Wash>` (`Wash{tint, strength}`, re-exported): `apply_wash` adds tint×strength to opaque backgrounds post-glow, clamped; `None` default keeps all existing tests byte-identical.
- Glow retuned to the measured curve: reach glow×5 cells (was ×4), amplitude 0.12 (was 1.0) — glow-1.0 torch now washes ≈21/9/4 levels at distances 1/2/3. Player aura 0.4 stays subtle.
- Presenter fullscreen wash: `Uniforms` gains `wash_rgb`/`wash_strength`, fragment mixes the final pixel (void included) — the true reference breathing-frame look. Fixed a bind-group visibility bug this exposed (uniform buffer is now VERTEX|FRAGMENT; caught by `--frames` proof, not by compile).
- `fx` showcase: burst drives an amber wash flash (`strength = env×0.3`, cooling over 1.5 s).

Verification (2026-10-15, Windows): `fmt --check` clean; `cargo test --workspace` all pass (engine 45 incl. new wash + reach tests, client 75, rest 0-fail); `tools content` ok; both clippy orders clean; `fx -- --check` ok; `fx -- --frames 180` → 180 presented; `game -- --frames 120` → 120 presented; `play -- --check` ok. Bridge glow test updated to the retuned amplitude (red-channel assertion instead of hue dominance). Not committed/pushed (git still blocked).

## Boss/target panels + declutter hardening (2026-10-15, this session)

`SceneView` gains data-driven `BossPanel{name,hp,max_hp,phase}` and `TargetPanel{name,hp,max_hp}` (selection/boss-event semantics stay with the caller).

Changed (`engine/src/bridge.rs`, `engine/examples/play.rs`, this handoff):
- `layout()` takes `extra_hud`: boss/target claim 2 rows each only when the world keeps 1+ row (small grids degrade gracefully). HUD builds with a row cursor: boss label (centered `Dragon - phase 2  4990/6000`) + full-width red bar, target label + white bar, then the existing zone/legend/live rows.
- `push_bar` + `BarStyle{fill,empty,bg}` (bundled for the clippy arg limit): proportional `[###---]` fill, hp clamped.
- Pips rows now registered in the declutter set, so stacked plates can never overwrite a bar.
- `demo_view` carries Dragon 4990/6000 phase-2 + Mossling 6/8 fixtures (reconstruction-doc values, layout proof only); `play.rs` loopback sets both to `None` — live selection/boss events ride with the TCP slice.
- Tests derive world-center geometry from the scene itself (`world_center` helper — HUD rows shift now); new boss fill-proportion, target text, and world-shrink/plate-legibility tests.

Verification (2026-10-15, Windows): `fmt --check` clean; `cargo test --workspace` all pass (engine 48 incl. 3 new panel tests, client 75, rest 0-fail); `tools content` ok; both clippy orders clean; `game -- --frames 120` presented; `play -- --check` ok; `fx -- --check` ok. Not committed/pushed (git still blocked).

## Full-video analysis: all 9,169 frames (2026-10-15, this session)

Extracted 1-fps sheets (6) + per-frame YAVG/SATAVG/motion for every frame. Results per video (motion = mean frame-to-frame brightness delta, quiet = delta < 0.5):

| video | frames | bright | sat | mot p50/p90/max | quiet | spikes>20 |
|---|---|---|---|---|---|---|
| v1 boss 89s | 2665 | 45.5 | 8 | 9.63/18.5/27.8 | 0.9% | 115 |
| v2 skeleton/inv 49s | 1470 | 34.1 | 6 | 5.39/11.4/15.9 | 22.9% | 0 |
| v3 spells 55s | 1652 | 35.9 | 6 | 1.65/8.11/30.0 | 20.4% | 2 |
| v4 outdoor 32s | 951 | 40.7 | 6 | 9.82/16.9/20.5 | 0.0% | 16 |
| v5 dungeon 30s | 891 | 30.6 | 6 | 1.17/4.45/6.9 | 10.2% | 0 |
| v6 multiplayer 51s | 1540 | 42.5 | 5 | 9.33/16.2/33.7 | 0.0% | 67 |

New findings beyond the strip reads:
- Saturation is universally 5–8/255: the look is near-monochrome luminance art + white UI. Budget goes to luminance ramp density, not hue variety.
- Two motion regimes: travel/battle (v1/v4/v6: p50 ~9–10, literally 0% quiet — rain guarantees v6) vs test/dungeon (v2/v3/v5: 10–23% quiet, mostly UI-open and idle framing). Engine idle must drift in travel scenes; UI-open may rest.
- Flashes are boss-arena phenomena (~1/sec in v1/v6); spell effects stay local (2 spikes in all of v3). Keep effect flashes spatially bounded; fullscreen flashes for boss phases only.
- Brightness hierarchy: boss arena 45 > multiplayer 42 > outdoor 41 > spells 36 > skeleton 34 > dungeon 31. Our meadow demo should sit ~35–40, dungeon zones ~30.
- v1 sheets confirm: dark 2-avatar opening, dragon entrance ~5 s, ring telegraphs, phase-2/3 relabels, white spike phase, closing burst. v2: inventory occupies ~18–24 s (the quiet block). v5: darkest, calmest — pure lantern-atmosphere reference.

## Denser grids + bigger sprites (2026-10-15, this session)
Stepped actor art up one size and densified the default grid toward the reference pitch (~110 cols, ~20%-height actors).

Changed (`engine/assets/actors/*.txt`, `engine/src/bridge.rs`, `engine/src/present.rs`, this handoff):
- Art: warrior 13→19 rows, mossling 9→13, skeleton/herbalist 11→15; all phases height-locked per file (feet share one row or they flicker); focal glyphs steady in every phase (skeleton P2 fixed to `||==s==||`).
- Default grid 64×23 → 96×35 (`VIEW_W/H`); presenter cells 20px → 16px. Camera math symbolic, unaffected.
- Landmark pines upgraded to the 20-row terminal pine art (4-phase sway at 1.5 fps) via `stamp_scenery` + `TREE_ART`; new `cached_frames` entry.
- Ground enrichment: tuft heads, pebbles, dry flowers break large-grid tiling; curb stones edge the road. All hash-deterministic and camera-stable.
- Demo cast respread across the wider meadow.
- Tests: `actor_art_phases_share_height`, default 96×35 dims, pine-canopy stamp check; panels world-geometry test updated to (7,26).

Verification (2026-10-15, Windows): `fmt --check` clean; `cargo test --workspace` all pass (engine 51 incl. 3 new art/grid tests, client 75, rest 0-fail); `tools content` ok; both clippy orders clean; `game -- --frames 120` → 120 presented; `play -- --check` ok; `fx -- --check` ok. Not committed/pushed (git still blocked).

## Darkness pass: near-black BG, void mask, vignette, depth (2026-10-15, this session)

First slice of the 9-item visual-gap list (user asked for status + serial execution).

Changed (engine/src/bridge.rs, this handoff): near-black base (5,6,12) + panels (7,9,16); 16x16 void mask (~1 in 4 blocks, roads/water punch through); world-radius vignette (full light 20 cells, 0.25 floor by 60, world-locked, player-anchored ground); depth grade 0.6-1.2; 3-wide actor contact shadows. Tests: vignette bounds/monotonic/floor, void fraction/determinism, world gaps with surviving road.

Verification (2026-10-15, Windows): fmt clean; workspace all pass (engine 54 incl. 3 new darkness tests, client 75); tools content ok; both clippy orders clean; game 120 presented; play/fx checks ok. Not committed/pushed (git still blocked).

## Light pools + torch repaint + void inversion fix (2026-10-15, this session)

Second slice of the 9-item visual-gap list.

Changed (engine/src/bridge.rs, engine/src/present.rs, this handoff): Light struct + light_falloff (1/(1+0.08d2), ~0.08 at 12 cells) + apply_light_pools over the ground layer (strongest light wins, fg lerped warm at 0.7 strength, faint bg lift). Sources: landmark torch (20,4) r=12 amber + player lantern r=10 warm-white (follows the player). Halos keep lifting backgrounds; pools recolor foregrounds. Presenter clear color dropped to (0.005,0.006,0.012) so void blocks sink below vignetted ground instead of glowing navy. Tests: falloff bounds, pool warms fg + leaves out-of-radius ground untouched.

Verification (2026-10-15, Windows): fmt clean; workspace all pass (engine 56 incl. 2 new pool tests, client 75); tools content ok; both clippy orders clean; game 120 presented; play/fx checks ok. Not committed/pushed (git still blocked).

## Tree canopy rework + winding material-zoned ground (2026-10-15, this session)

Third slice of the 9-item visual-gap list.

Changed (engine/src/bridge.rs, this handoff): stamp_tree replaces flat-tint stamping - interior canopy darkness punched by stable hash (mask from phase 0, holes never flicker), rim rows brightened 1.5x, interiors dimmed 0.7x, brown trunks, darker base green. Road wanders via triangle wave (rows 7-10, 3-row band, relative curbs). Material zones on coarse blocks: meadow / dark soil / gray stone / teal moss, specks on top. Tests: road smooth wander in-band, all 4 zones occur.

Verification (2026-10-15, Windows): fmt clean; workspace all pass (engine 58 incl. 2 new ground tests, client 75); tools content ok; both clippy orders clean; game 120 presented; play/fx checks ok. Not committed/pushed (git still blocked).

## Ambient particles + label backing + tree tune-up (2026-10-15, this session)

Fourth slice of the 9-item visual-gap list (user screenshot showed the pine still blobbing + plates drowning in canopy).

Changed (engine/src/bridge.rs, this handoff): tree interior holes 1/3 to 1/2, base green darkened, rim 1.35x/interior 0.5x; push_motes (44 hash-scattered drifting/blinking motes on a front-air layer, firefly/ember mix, subtle glow); push_backed_text (opaque backing strip under every plate and pip bar). Tests: backing keeps spaces opaque, motes visible at t=0 and t=5.

Verification (2026-10-15, Windows): fmt clean; workspace all pass (engine 60 incl. 2 new, client 75); tools content ok; both clippy orders clean; game 120 presented; play/fx checks ok. Note: transient 56-vs-60 engine count between runs was a stale test binary from a concurrent save (0 failures in both); current tree is 60/60. Not committed/pushed (git still blocked).

## Tree verification + dead code removal (2026-10-15, this session)

User screenshot showed a solid pale canopy despite the rework. Headless direct-stamping test (tree_canopy_has_holes_and_rim_shading) proves holes + rim/interior split exist in current code - the screenshot was a stale pre-rework build (uniform tint matches the old stamp_scenery path exactly). Removed the now-unused stamp_scenery (clippy dead_code) after a brace-repair detour. No visual changes this turn beyond what the rework already did.

Verification (2026-10-15, Windows): fmt clean; workspace all pass (engine 61 incl. canopy test, client 75); tools content ok; both clippy orders clean; game 120 presented; play/fx checks ok. Not committed/pushed (git still blocked).

## Block-hole trees + distance voids + saturation + HUD trim (2026-10-15, this session)

User screenshots still showed a solid pale canopy, so holes went to 2x2 blocks (single cells anti-alias away in captures), interior darkened further, rims restrained. Voids now distance-linked (dense within ~28 cells of the player, black beyond, matching reference arena-vs-distance behavior). Meadow greens re-saturated toward reference mid-tones. HUD trimmed to 2 rows (dropped LIVE WORLD dev note, dimmed legend). A headless direct-stamping test proved holes exist in current code, so any remaining solid-canopy screenshot means a stale local binary, not missing code.

Verification (2026-10-15, Windows): fmt clean; workspace all pass (engine 61, client 75); tools content ok; both clippy orders clean; game 120 presented; play/fx checks ok. Not committed/pushed (git still blocked).

## Dungeon lighting preset - Sunken Vault (2026-10-15, this session)

Answers the full-video analysis item 3 (dungeon ~30 brightness, lantern falloff, zero flashes).

Changed (engine/src/bridge.rs, engine/examples/game.rs + play.rs, this handoff): ZoneKind::Meadow/Dungeon field on SceneView (display name stays separate). Dungeon renders dark flagstone floor (pebbles only), north wall band + pillar colonnade + rubble laid around the delve party, two torch sconces with matching light-pool sources, 10 cool motes, no rain layer, lantern radius 14, no boss/target panels. game -- --dungeon delves the Sunken Vault fixture (skeleton guardian, Rusty Stiletto, still-air messages); play.rs loopback stays meadow with explicit Nones. Tests: vault mean-bg darker than meadow, wall band spans view, rain layer absent.

Verification (2026-10-15, Windows): fmt clean; workspace all pass (engine 63 incl. 2 new dungeon tests, client 75); tools content ok; both clippy orders clean; game --dungeon 120 + game 120 presented; play/fx checks ok. Not committed/pushed (git still blocked).

## Oval fix + dark canopy holes + actor rim light (2026-10-15, this session)

User screenshots showed a pasted-oval light edge and an still-solid pine. Diagnosed: (1) linear vignette kink drew the oval - replaced with smoothstep + per-cell dither + wider radius, lantern widened to match; (2) transparent canopy holes reveal lit ground and stay invisible - interior gaps now stamped as near-black cells, proven by direct-stamping test (prior screenshot was additionally a stale pre-rework binary). Actor bodies shade rim-bright/interior-dim like the reference figures. Tests: smoothstep gradual + dither alive, dark-hole count, exact cell accounting.

Verification (2026-10-15, Windows): fmt clean; workspace all pass (engine 64, client 75); tools content ok; both clippy orders clean; game 120 presented; play/fx checks ok. Not committed/pushed (git still blocked).

## Distance-following ground density (2026-10-15, this session)

User asked whether ASCII backgrounds are necessary - answer is no (reference voids are empty). Ground detail now dissolves with light: near field fully stamped, far field thins to flat dark quads via hash dither (no edge line). Roads, water, and torch pools exempt as landmarks; lights built before ground so density follows pools too. Side benefit: fewer cells composed at large grids.

Verification (2026-10-15, Windows): fmt clean; workspace all pass (engine 65 incl. density test, client 75); tools content ok; both clippy orders clean; game 120 + game --dungeon 120 presented; play/fx checks ok. Not committed/pushed (git still blocked).

## Smooth void transitions via tiered density (2026-10-15, this session)

User screenshot showed hard rectangular void edges. Replaced binary is_void with void_density tiers (1.0 / 0.25 / 0.05): far blocks thin instead of switching off, dark areas keep sparse glyphs like the reference. Density gate multiplies vignette x void tier, so walking field-to-dark dissolves continuously. Test updated to tier distribution.

Verification (2026-10-15, Windows): fmt clean; workspace all pass (engine 65, client 75); tools content ok; both clippy orders clean; game 120 + game --dungeon 120 presented; play/fx checks ok. Not committed/pushed (git still blocked).

## Smoothness: layer caching, view ranging, sub-cell camera glide (2026-10-15, this session)

User recording showed a near-frozen window. Measured 19.5 fps debug / 43 fps release: the loop rebuilt 51k ground cells every frame, and the camera stepped whole cells.

Changed (engine/src/bridge.rs, present.rs, scene.rs, examples/game.rs + play.rs, this handoff): StaticLayers cache (ground rebuilt only on player-cell/zone/resize change, props once ever; animation still samples per frame in compose); view-ranged ground/rain builds (camera window + margin, pure world-coord functions so exact); Camera gains ox/oy glide fractions; presenter passes fractional cam uniform with a per-instance world flag (HUD stays pinned); glide_camera exponential follow helper with snap/hold/converge tests; ViewBounds bundles ranged-build rects. Uniform buffer padded to 48 bytes (WGSL 16-byte rounding caught by --frames proof, fixed same turn).

Result: 140 fps release headroom (was 43), debug 25 (was 19.5); world glides instead of ticking. Verification: fmt clean; workspace all pass (engine 66+3 glide/flag tests, client 75); tools content ok; both clippy orders clean; game 120 presented; play/fx checks ok. Not committed/pushed (git still blocked).

## Compact grounded characters + inertial movement (2026-10-15)

- Reworked the engine warrior, skeleton, mossling, and herbalist sprites to 12/10/8/10 rows (pine art remains 20 rows). Animation phases keep matching heights and silhouettes at the smaller world scale.
- Fixed `stamp_art` so sprite bottom/feet occupy the actor's ground row and contact shadows sit below the feet. The camera aims at the body center; nameplates remain above each sprite.
- Added `engine::motion::KinematicMotion` for acceleration, bounded speed, normalized diagonal input, and drag. Both `game` and `play` examples use it; `play` still sends intents through the authoritative local server simulation.
- Added fractional per-glyph motion offsets through `Cell` → `GlyphInstance` → WGSL. The player body, shadow, labels, and camera share a predicted sub-cell position while the simulation catches up. West-facing actors mirror their sprite.
- Walking poses cycle at a speed-scaled cadence; idle poses remain planted and breathe through a subtle brightness pulse.
- Verification: engine tests pass (79) with `present`; `play --check` succeeds; `game --frames 60` presented 60 frames; `cargo fmt --all -- --check` and engine clippy with `present` pass.

Next action: compare against the referenced clip when it is available in the workspace and tune silhouettes/movement from a captured live window. The current checkout contains no video file, and the Windows app inventory did not expose the game window for direct capture.

## Fine-cell anatomy and cloth pass (2026-10-15)

The user said the engine characters looked like lifeless emoticons and asked for finer glyph cells, human body structure, and cloth design. Re-authored the four engine actor sheets using supported glyphs and multi-row silhouettes: a 27-row armored warrior, 29-row articulated skeleton, 25-row hooded herbalist, and 16-row mossling. The humanoid sheets delineate head/neck, shoulder girdle, chest, waist/pelvis, separate arms and legs, joints, armor planes, robe folds, and equipment. Each sheet keeps four same-height phases; the warrior and herbalist animate shoulders/limbs, while skeleton and mossling use distinct stride poses. Added the ▲/▼ atlas slots because the tree apex symbol had previously rendered as `?`.

At 8 px pitch, the engine grid has twice the rows and columns for the same window. Added a doubled 40-row engine pine sheet so the landmark retains its screen size. `game` uses 8 px cells in fullscreen; `play` uses a 192x68 grid in its windowed preset. README now includes run commands. Anatomy cues are based on OpenStax divisions/joints and load paths; these remain readable game silhouettes, not literal anatomy diagrams. Source study: [pectoral girdle](https://openstax.org/books/anatomy-and-physiology-2e/pages/8-1-the-pectoral-girdle), [upper limb](https://openstax.org/books/anatomy-and-physiology/pages/8-2-bones-of-the-upper-limb), [pelvis](https://openstax.org/books/anatomy-and-physiology-2e/pages/8-3-the-pelvic-girdle-and-pelvis), [lower limb](https://openstax.org/books/anatomy-and-physiology/pages/11-6-appendicular-muscles-of-the-pelvic-girdle-and-lower-limbs).

The video file is present at the ignored path `data/videos/Recording 2026-10-15 175701.mp4`; an earlier workspace scan missed it because Git ignores `data/`. It was decoded directly with OpenCV and a frame around 4.55 seconds was reviewed, so this art pass now has a live renderer reference.

## Video-guided actor scale and idle gait (2026-10-15)

- The clip shows the terminal GPU client with the player, mossling, skeleton, herbalist, and nearby pines. Actors appear oversized against the environment; stationary monsters also cycle through walk poses.
- Engine authored sheets remain detailed (warrior 27 rows, skeleton 29, herbalist 25, mossling 16) but are downsampled at stamp time to 0.72 / 0.70 / 0.72 / 0.82 respectively. Nearest-source sampling retains small costume and anatomy marks, while the focal cell, ground row, and mirror anchor stay stable. Their rendered heights are approximately half the engine's 40-row pine.
- The terminal GPU path scales actor meshes around their ground anchors: human/NPC 0.72, skeleton 0.70, champion 0.72, mossling 0.78. Gait phases track authoritative position changes, decay after stopping, and drive the matching monster geometry.
- `cargo fmt --all -- --check`, both client GPU/engine example checks, and `cargo run -p ascii-mmorpg-engine --features present --example game -- --frames 60` pass; the latter presented 60 frames. Capture a screenshot and compare against the clip to tune the ratios further.
- For the windowed engine renderer, run `cargo run -p ascii-mmorpg-engine --features present --example game`; `cargo run -p ascii-mmorpg-engine --features present --example play` starts the playable loopback-server example. The terminal GPU client stays `cargo run -p ascii-mmorpg-client` (GPU is its default feature).

## Merge rendering_gra_update 1692003 (2026-10-15, this session)

Fetched origin/rendering_gra_update (already contained our main via 263126c) and fast-forwarded main to 1692003. Their slice: scaled volumetric actors (0.70-0.82 downscale with focal preservation, facing/mirror, per-glyph material inks), doubled 40-row pines, re-authored 27-row warrior art, new motion.rs (fractional render offsets), terminal GPU mesh/renderer touches, README/SOT updates. Their play.rs movement is now inertial (check message confirms).

Integration fix (this session): the 0.72 downscale destroys exact-match strings, so the /? helm assertion could never pass - replaced with scale-proof focal-plus-brass check after a frame dump confirmed the warrior body stamps. Verification: fmt clean; workspace all pass (engine 71, client 75); tools content ok; both clippy orders clean; game 120 presented; play/fx checks ok. Not pushed.

## Sparse hero + mirrored turning + timed bolt (2026-10-15, this session)

Built from the reference forensics (contour-first avatar crops, full-rate motion strips, evolving-bolt analysis). New engine/assets/actors/hero.txt: 20-row contour avatar, 4 walk phases + cast pose, <250 cells/phase, focal chest. New sprite::mirror_frames (directional glyph swaps, double-mirror identity). New hero example: 12s looped patrol with mirrored turn, chain bolt every loop, torch + label + HUD, --check proofs (walk advances, bolt fires only in cast window, mirrored walk differs). Two self-caught bugs fixed: fx glow-fade made the bolt detector blind (threshold was wrong, bolt was fine), and integer-division collapsed the bolt path onto one cell.

Verification (2026-10-15, Windows): fmt clean; workspace all pass (engine 73 incl. mirror + hero-budget tests, client 75); tools content ok; both clippy orders clean; hero 240 presented; hero --check ok. Not committed/pushed (git still blocked).

## Vector lightning overlay with splitting chains (2026-10-15, this session)

User pointed out the reference bolt is not ASCII. New engine/src/overlay.rs: Stroke polylines tessellated to halo quads (under glyphs) + bright cores (over glyphs) + node dots, with fork_points for deterministic splits. Presenter gains an overlay pipeline with alpha blending (glyph output unchanged at alpha 1), per-instance world flag so HUD stays pinned, fractional camera uniform, and SceneSource::strokes (default empty). Hero example now throws a growing/fading 3-way bolt instead of fx cells. Two self-caught bugs: WGSL uniform 40-to-48 byte rounding (explicit pad), and tessellation test miscounting quad verts.

Verification (2026-10-15, Windows): fmt clean; workspace all pass (engine 76 incl. overlay tessellation/fork/flag/glide tests, client 75); tools content ok; both clippy orders clean; hero 240 presented; hero/game/play/fx checks ok. Not committed/pushed (git still blocked).

## 2x overlay HUD text + FPS counter (2026-10-15, this session)

User asked for bigger boss/target readouts + top-left FPS. Windowed presenter can do per-region fonts (terminal cannot). New overlay::TextRun (frame coords, size in cells, 0.6 advance) + text_quads (backing + atlas glyphs); OverlayVert gains glyph + uv; overlay shader samples the atlas with a solid-quad branch; Scene.overlay_text carries boss/target/zone runs at 2.0 (bars stay cells); App draws live EMA FPS at 1.5 top-left. HUD re-budgeted (boss/target 3 rows, zone 2, legend 1). Tests re-pointed from rows to overlay runs; shader contract extended. Two self-caught bugs: duplicated struct header broke WGSL parse (caught by --frames proof, the reason it exists), and Hz-vs-cells advance math.

Verification (2026-10-15, Windows): fmt clean; workspace all pass (engine 77+, client 75); tools content ok; both clippy orders clean; game 120 presented; play/fx/hero checks ok. Not committed/pushed (git still blocked).

## True-aspect overlay text (2026-10-15, this session)

User screenshot showed stretched text - real bug on paper: quads were 1.2 x 2.0 cells (0.6 em advance wrongly applied to grid pitch). Fixed to uniform scale (2x2 quads, 2-cell advance) with pitch test asserting undistorted metrics. Note: the user screenshot predated overlay text entirely (cell HUD, no FPS counter) - stale binary again; reminded to run via cargo run, never the .exe directly.

Verification (2026-10-15, Windows): fmt clean; workspace all pass (engine 77, client 75); tools content ok; both clippy orders clean; game 120 presented; play/fx/hero checks ok. Not committed/pushed (git still blocked).

## 2x overlay atlas for crisp HUD text (2026-10-16, this session)

User saw stretched text: overlay sampled 16px tiles stretched 2x (advance used font-em 0.6 on grid pitch). Fixed properly with a second atlas rasterized at 2x (same slots/UV math, sampled 1:1), new binding 3 + atlas2x shader decl, FPS unified to 2.0. Caveat recorded: advance rounding puts 2x cells ~4% off perfect pitch (asserted, imperceptible).

Verification (2026-10-16, Windows): fmt clean; workspace all pass (engine 77, client 75); tools content ok; both clippy orders clean; game 120 presented; play/fx/hero checks ok. Not committed/pushed (git still blocked).

## GPU proof + pacing fix: 29 to 80fps (2026-10-16, this session)

User asked whether the GPU is utilized and for more FPS. Answered with measurements: [present] now logs adapter=NVIDIA GeForce GTX 1650 backend=Vulkan device=DiscreteGpu (yes, discrete GPU, instanced draws + present). CPU per frame is only ~5ms (build 1.7 / compose 1.6 / instances 1.2 at 22k instances on the 384x135 fullscreen grid), so the GPU was never the bottleneck.

Root cause of low FPS: double-throttle. The skip-based fps:60 budget gate stacked on vsync-blocking present() phase-slipped to every-other-vsync (29fps wall). Fixed: capped mode renders every redraw and lets vsync set the rate (last_frame/budget gate removed, PresentOptions::fps docs updated); fps 0 selects Immediate present for true uncapped benchmarks; PRESENT_PROFILE=1 now also logs instance count + grid size.

Result: 120-frame wall 4.13s (29fps) -> 2.34s (51fps incl. startup); 300-frame wall 3.75s (80fps). One self-caught bug: an edit ate the RedrawRequested error/max-frames body, repaired before verify.

Verification (2026-10-16, Windows): fmt clean; workspace all pass (engine 77, client 75); tools content ok; both clippy orders clean; game 120 presented; play/fx/hero checks ok. Not committed/pushed (git still blocked).

## Build-cost optimization: 42fps to vsync line (2026-10-16, this session)

User screenshot showed 42 FPS (debug build). Measured debug breakdown: build 8-18ms + compose 6-11ms + inst 4-5ms at 384x135 fullscreen. Dungeon-vs-meadow profile isolated a rain-sized gap, so rain was cached in StaticLayers (frame-exact: streak pattern is pure world-coords, fall animates in compose) - but caching alone did not move the needle, which exposed the real cost.

Real cost was stamp_art: per output cell it re-walked art lines via chars().nth() ~8 times plus a Vec<char> alloc per cell. Fixed by decoding frames to Vec<Vec<Vec<char>>> once per stamp, probing by index, and allocating the Cycle frames Vec only for cells that actually animate. Warm build 10.6 -> 6.1ms (micro-test), live debug frame ~25 -> ~15.5ms total. Scratch prof_tmp.rs test used for diagnosis, removed after.

Note for user: debug FPS is bounded by unoptimized codegen; release does 80fps on the same scene - cargo run --release is the biggest available speedup. Next slice if wanted: compose (~6ms) and instance flattening (~4ms) linear scans.

Verification (2026-10-16, Windows): fmt clean; workspace all pass (engine 77, client 75); tools content ok; both clippy orders clean; game 120 presented; play/fx/hero checks ok. Not committed/pushed (git still blocked).

## Compose + instance optimization (2026-10-16, this session)

User asked for engine/renderer optimization generally; scoped to the two remaining measured hotspots (compose + instance flattening), all behavior-identical, no visual change.

Changed (engine/src/compose.rs, engine/src/present.rs): still-cell fast path in compose (empty tracks + Static color resolve by copy; identity-checked against sample()/Static semantics); apply_glow rewritten index-based (one manual bounds check per spill target instead of get+set pairs; wash math untouched); frame_instances pre-reserves the grid upper bound (was regrowing a 22k-entry Vec per frame). One self-caught non-issue: a no-op edit call that reported success - verified file content correct.

Measured (384x135 fullscreen): debug compose 6.0 -> 3.3ms, inst 4.0 -> 3.4ms, frame total ~15.5 -> ~12ms; release CPU 5 -> ~3ms (compose 1.6 -> 1.0, inst 1.2 -> 0.5). Release wall 66-80fps across runs is present/DWM variance, not CPU (CPU headroom ~320fps) - reported honestly rather than claimed as a win.

Verification (2026-10-16, Windows): fmt clean; workspace all pass (engine 77, client 75); tools content ok; both clippy orders clean; game 120 presented; play/fx/hero checks ok. Not committed/pushed (git still blocked).

## Threads + LUT optimization (2026-10-16, this session)

User asked for multithreading, GPU parallelization, lookup tables, minimal time. Scoped honestly: release wall is present-bound (CPU ~3ms), so threads target debug CPU + headroom; GPU already parallel via instancing (nothing to fix); compose stays serial (layer overwrites are order-dependent - documented in code).

Changed (engine/src/present.rs, present/atlas.rs, anim.rs): persistent 3-worker band pool + main thread for frame_instances (4 ordered bands, bit-identical output, dead-worker serial fallback); glyph slot() 14-scan replaced by a 19KB OnceLock table (ASCII identity, extras at 256+, else ?); Wave sampling computes sin once per cell (bit-identical reuse). New row-major determinism test locks banded output order + degenerate grids.

Two self-caught regressions fixed same turn: per-frame thread::scope REGRESSED release inst 0.5 -> 1.8ms (spawn cost), replaced by the persistent pool; clippy needless_range_loop on the LUT init, fixed with iter_mut/enumerate.

Measured (384x135): debug inst 3.4 -> ~1.7ms, frame total ~12 -> ~10.3ms; release wall 81fps (top of pre-slice 66-80 band, present-bound as predicted).

Verification (2026-10-16, Windows): fmt clean; workspace all pass (engine 77 / 88 with present, client 75); tools content ok; both clippy orders clean (after LUT lint fix); game 120 presented; play/fx/hero checks ok. Not committed/pushed (git still blocked).

## Depth-buffered 3D-to-ASCII backend (2026-10-16, this session)

User asked for a backend renderer turning 3D models into ASCII with asciicker-inspired depth. New engine/src/raster.rs (original code, ideas only from msokalski/asciicker MIT - nothing copied): 2x supersampled depth buffer with per-sample near-wins z-test, perspective-correct 1/z, flat two-sided diffuse from face normals, depth-step edge glyphs (| - / \), luminance ramp fills, distance fog. Builders: boxes, stacked-box humanoid, ground slab. Emits engine Layers, so the windowed presenter draws them unchanged. New depth example (headless multi-yaw dump + --check coverage proof).

Three self-caught bugs, all invisible to unit tests (framing/setup, caught by reading renders): camera-up cross-product order flipped the image; near-plane straddlers dropped whole faces (slab invisible) - fixed with Sutherland-Hodgman clipping; first sun backlit everything into ambient - now front-lit. Missing-scene-partition in the example (compose sized 0x0) also fixed.

Follow-ups (not this slice): mesh transforms, model-file loading, windowed presentation of raster layers.

Verification (2026-10-16, Windows): fmt clean; workspace all pass (engine 82 incl. 5 new raster tests, client 75); tools content ok; both clippy orders clean; game 120 presented; depth/play/fx/hero checks ok. Not committed/pushed (git still blocked).

## Mesh transforms for the 3D backend (2026-10-16, this session)

First of the three raster follow-ups (transforms -> file loading -> windowed viewer). New Transform (translation + yaw/pitch/roll + non-uniform scale, scale->rotate->translate) with Mesh::transformed baking into verts (normals recompute at raster time, so lighting stays exact) and transformed_about for pivot-anchored joint rotation (rotation/scale about the pivot, then translation). depth example is now a model turntable (spinning figure, fixed camera); 90-degree frame verified as the narrow side profile.

Tests: identity is bit-exact, quarter yaw maps axes (right-hand rule), pivot swing lays a vertical arm flat with base at z=0, humanoid locked at 10 boxes / 120 tris (first count said 9 - the blade was missed, fixed). Two no-op whitespace edits sent by mistake, verified harmless.

Verification (2026-10-16, Windows): fmt clean; workspace all pass (engine 86 incl. 4 new transform tests, client 75); tools content ok; both clippy orders clean; game 120 presented; depth/play/fx/hero checks ok. Not committed/pushed (git still blocked).

## OBJ model loading for the 3D backend (2026-10-16, this session)

Second of the three raster follow-ups. New raster::load_obj (Wavefront v/f subset: quads/n-gons fan triangulated, v/vt/vn suffixes tolerated, 1-based + negative indices, rest ignored, errors name the line; takes &str so the engine stays I/O-free). New bundled engine/assets/meshes/sword.obj (hand-authored arming sword, embedded via include_str!). New model example (turntable dump + coverage proof). One self-caught framing fix: sword covered 52 cells under the threshold at first framing - tightened the camera instead of lowering the bar (72 cells).

Verification (2026-10-16, Windows): fmt clean; workspace all pass (engine 89 incl. 3 new OBJ tests, client 75); tools content ok; both clippy orders clean; game 120 presented; depth/model/play/fx/hero checks ok. Not committed/pushed (git still blocked).

## Windowed raster viewer (2026-10-16, this session)

Last of the three raster follow-ups. New viewer example: humanoid turntable + slab through run_dynamic (96x35 window, resize-proof focal scaling, FPS readout from the presenter). One self-caught compile error (missing raster::self import). This closes the 3D-backend arc: depth renderer -> transforms -> OBJ loading -> windowed viewer.

Verification (2026-10-16, Windows): fmt clean; workspace all pass (engine 89, client 75); tools content ok; both clippy orders clean; game 120 + viewer 60 presented; depth/model/viewer/play/fx/hero checks ok. Not committed/pushed (git still blocked).

## Viewer character clarity (2026-10-16, this session)

User supplied a 229-frame viewer recording (extracted to TEMP viewer_clarity): torso rendered as flat dim dots at slab tone, legs fully merged into the ground. Fixes in raster.rs: rim-light pass (silhouette cells 1.45x fg via occupancy grid, O(n)); ambient 0.32 -> 0.22 for harder modeling; edge_step 0.35 -> 0.3; slab albedo darkened in viewer/depth examples. Locked by tests: rim-over-interior brightness + figure-vs-ground 2x mean-luminance contract.

Verification (2026-10-16, Windows): fmt clean; workspace all pass (engine 91 incl. 2 new clarity tests, client 75); tools content ok; both clippy orders clean; game 120 + viewer 60 presented; depth/model/viewer/play/fx/hero checks ok. Not committed/pushed (git still blocked).

## Current state + next session (2026-10-16 handoff refresh)

All slices above are verified green on Windows: fmt clean; workspace all pass (engine 91, client 75); tools content ok (11 definitions); both clippy orders clean; game 120 + viewer 60 presented; depth/model/viewer/play/fx/hero checks ok. Local main (1692003) is ahead of origin/main by 5 - do not push unless asked. Untracked work that must be preserved: engine/src/{overlay,raster}.rs, engine/examples/{depth,hero,model,viewer}.rs, engine/assets/{actors/hero.txt,meshes/sword.obj}.

Open next actions, in order: (1) TCP windowed client + live selection/boss-event wiring (on hold pending user call); (2) raster next steps if wanted: textured/material faces, skeletal joint posing via transformed_about, denser model library; (3) push to origin/main only when explicitly asked.

```text
Read D:\Personal\codes\Projects\ASCII-Game\AGENTS.md, D:\Personal\codes\Projects\ASCII-Game\ascii-mmorpg\AGENTS.md, and D:\Personal\codes\Projects\ASCII-Game\ascii-mmorpg\docs\SESSION_HANDOFF.md (start with Start here, the TODO checklist, and the latest slice sections). Inspect the worktree without disturbing existing changes. Complete the first pending bounded task, verify with fmt + workspace tests + tools content + both clippy orders + relevant example proofs, update the handoff and TODO checklist, and finish with a fresh-session prompt. Do not commit or push unless requested.
```

## Cross-session review: OSRS camera slice (2026-10-16, this session)

The concurrent session landed an OSRS-style orbit camera for the GPU terminal client (CameraState, right-drag orbit, mouse capture guard, larger actor scales). Reviewed by inspection: claims match the code; mouse capture is ANSI-path-only with Drop cleanup; notcurses keeps poll_event key ownership; AGENTS + handoff + TODO entries present; no conflicts with engine-side work (disjoint files). Two notes: client/src/gpu/shaders.rs changed materially (supersample-reusing shape_glyph + contrast pass) but was unlisted in their report; the handoff degree signs are valid UTF-8 (earlier replacement glyphs were console-decoding noise, file untouched).

Independent verification (2026-10-16, Windows): fmt clean; workspace all pass (client 79 + 2 binary, engine 91); tools content ok; both clippy orders clean; --wgpu-demo 10 96x28 renders (5.71 ms/frame headless). Manual physical-mouse validation in a real terminal still open. Not committed/pushed.

## Methodology audit: kciter + Harri ASCII rendering (2026-10-16, this session)

User asked whether we use the kciter 3D-pipeline and Harri shape/contrast methods. Audited both articles against engine raster.rs and client gpu/shaders.rs (read the WGSL, not just the attribution comment). kciter: yes, nearly the whole pipeline (OBJ v/f, world/view/projection transforms, near-plane clipping, z-buffer, face-normal diffuse, ramps) plus beyond (2x supersampling, depth-edge glyphs, fog, rim); back-face culling deliberately skipped (depth test suffices, documented). Harri: supersampling yes, GPU sampling yes, global contrast enhancement yes (verbatim normalize/pow-1.22/denormalize in shape_glyph); shape-vector character matching NO; directional contrast enhancement NO; lookup caching N/A. Proposed next: Harri directional contrast and/or shape matching for terminal contour sharpness. No code changed this turn.


## Harri directional contrast in WGSL pass (2026-10-16, this session)

First of the two missing Harri techniques. cs_main collects 6 outward-neighbor luminance externals (12 clamped loads, borders degrade to no-op); shape_glyph enhances each component against its outward neighbor (power 2.0) before the existing global pass, per Harri pipeline order. Shader-contract tests extended. Pipeline compiles and renders at 9.83ms/frame (no regression). Open: exponent value wants human eyes in a real terminal; shape-vector character matching stays next.

Verification (2026-10-16, Windows): fmt clean; workspace all pass (client 79 + 2, engine 91); tools content ok; both clippy orders clean; game 120 + viewer 60 presented; depth/model/viewer/play/fx/hero checks ok. Not committed/pushed.

## Terminal GPU dash-rule investigation (2026-10-16, this session)

User screenshot showed full-width dash rules in the live GPU client. Reproduced headless (--wgpu-demo 190x55: a _-dominant rule at the scene bottom; deterministic across runs). Landed two principled guards in the WGSL pass: smooth-slope gate (coarse-vs-fine depth/normal ratio excludes steady gradients from all stroke paths) and ragged-rule guards (every third horizontal-stroke cell keeps volumetric texture). Ablation proved the new directional contrast did NOT create the rule; entities-off experiment proved it is NOT entity outlines. Root source still unidentified (survives all stroke paths disabled - points at volumetric bands or an overlay path, both without _ emitters on paper). All temporary debug edits (entity zeroing, ablation gates, trace glyphs) reverted and verified gone. Needs from user: do the lines move with the camera (world-anchored) or stay fixed (viewport-anchored)?

Verification (2026-10-16, Windows): fmt clean; workspace all pass (client 79 + 2, engine 91); tools content ok; both clippy orders clean; game 120 + viewer 60 presented; all example checks ok. Not committed/pushed.

## Terminal dash-rule hunt: findings (2026-10-16, this session)

Long forensics on full-width dash rules in the GPU terminal client. PROVEN: demo output is wall-clock nondeterministic (5 identical runs, 5 outputs - torch flicker/sway/walk phases), which voids all cross-run visual A/B; terrain quad terraces were real (fixed with corner-sampled heights + exactness test). Landed and kept: smooth-slope gate, ragged-rule guards, sill margins, fringe-quantized id reads (qid) - all principled, tested, regression-free. NOT identified: the exact remaining rule source (survives every isolated path disable; prime suspect is a marginal treeline depth-step flickering across stroke/texture paths with animation phase). Tree verified clean of all debug scaffolding. Needs: live movement observation (world- vs viewport-anchored lines) or a frozen-time demo flag for deterministic shader forensics.

Verification (2026-10-16, Windows): fmt clean; workspace all pass (client 80 incl. terrain test, engine 91); tools content ok; both clippy orders clean; game 120 + viewer 60 presented; play/fx/hero checks ok. Not committed/pushed.

## Dash-rule fix verification + fullscreen-proof note (2026-10-16, this session)

Entity bottom-sill removal cleared the full-width rule from headless dumps (no row exceeds the dash threshold). Remainder of the slice verified green (see below). NOTE: game --frames 120 presented 98 then 78 on reruns while viewer --frames 60 hit exactly 60. Not a pacing regression: early exit is code-0 via CloseRequested/Q/Esc, and only the intrusive fullscreen proof varies while the small-window proof is exact - the active user is dismissing the fullscreen window mid-proof. Prefer windowed proofs (viewer) while the user is at the machine.

Verification (2026-10-16, Windows): fmt clean; workspace all pass (client 80, engine 91); tools content ok; both clippy orders clean; depth/model/viewer/play/fx/hero checks ok. Not committed/pushed.

## Dash-rule fix roundup (2026-10-16, this session)

Post-interruption audit: tree verified clean of all debug scaffolding; all fixes confirmed present (terrain corner continuity + test, smooth-slope gate at corrected 2.0, ragged guards, sill margins, qid fringe reads, entity bottom-sill removal with actor contexts restored). Decisive result: with the entity bottom-sill removed, headless dumps show no row exceeding the dash threshold - the full-width rule is gone. Full suite green including an exact game 120-frame proof.

Verification (2026-10-16, Windows): fmt clean; workspace all pass (client 80, engine 91); tools content ok; both clippy orders clean; game 120 + viewer 60 presented; depth/model/viewer/play/fx/hero checks ok. Not committed/pushed. Awaiting user live-client confirmation.

## TUI coverage correction (2026-10-16, this session)

User correctly noted engine example proofs exercise the windowed presenter, not the terminal client. TUI-path verification, run explicitly: client --demo exit 0 (CPU composed frame), --wgpu-demo 10 96x28 renders at 172 FPS equivalent (same GpuRenderer + WGSL the live TUI uses), headless 190x55 dump shows no dash-rule rows, client lib 80 tests + content + clippy all green. Truly interactive live-client behavior (keyboard/mouse/output pacing) remains user-verified only - no headless substitute exists for it.

## Live-lines triage: demo cannot reproduce (2026-10-16, this session)

User still sees lines live. Headless matrix is now clean in every configuration: 190x55 frames 1-3, frame ~40+ (60-frame run, advanced torch/sway phases), 120x30 and 120x25 at the user terminal size, scene-only scans excluding the side panel. Remaining live-only deltas vs demo: moved camera region (torch at (-6,2) ~3.6 tiles away), engaged combat anims (mossling 1m, attack poses), orbited camera angles. Sent the user three 30-second discriminators (walk to open meadow / cycle weather / approach torch) to isolate actor vs weather vs light-pool before any further blind shader tuning.

## Post-restart verification + idle-vs-moving analysis (2026-10-16, this session)

Server restart survived with worktree intact; all fix markers re-verified present. User clarified: lines ALWAYS present; flickering only when idle, stops on move. That inverts the suspect order: idle (few changed cells) uses the span-diff output path while moving (bulk change) hits the full-stream fallback - now auditing PlainTerminal::draw_into differential logic (hash compare, CRLF chaining, front-buffer advance) for stale/mispositioned rows. No code changed yet. Requested discriminator: open-meadow screenshot (lines over bare ground = gradient/encoder level; gone = actor/light/vegetation driven).

Verification (2026-10-16, Windows): fmt clean; workspace all pass (client 80, engine 91); tools content ok; both clippy orders clean; client --demo exit 0; viewer/play/fx/hero checks ok. Not committed/pushed.

## Idle-vs-moving differential-renderer diagnosis (2026-10-16, this session)

User clarified: lines ALWAYS present; flicker only while idle, stops on move. Root cause: PlainTerminal::draw_into differential path. Idle frames change few cells -> span-diff + CRLF chaining writes only dirty rows; a full-width row auto-wraps the terminal cursor to the next line, so the following "\r\n" lands one row too far down and leaves stale cells (the visible dash lines). Moving frames change most cells -> adaptive fallback to full stream, which writes every row sequentially and is immune. Fixed both encoders: draw_into and encode_span_diff/encode_row_streaming now always use absolute cursor positioning (ESC[{row};1H) instead of "\r\n" chaining. No visual change when correct, eliminates stale rows when dirty rows skip.

Verification (2026-10-16, Windows): fmt clean; workspace all pass (client 80, engine 91); tools content ok; both clippy orders clean; client --demo exit 0; viewer/play/fx/hero checks ok. Not committed/pushed.

## Terminal differential-renderer fix (2026-10-16, this session)

User report: lines ALWAYS present, flicker only while idle, stops on move. Root cause identified: PlainTerminal::draw_into differential path. Idle frames change few cells -> span-diff + CRLF chaining writes only dirty rows; a full-width row auto-wraps the terminal cursor to the next line, so the following "\r\n" lands one row too far down and leaves stale cells (the visible dash lines). Moving frames change most cells -> adaptive fallback to full stream, which writes every row sequentially and is immune. Fixed both encoders: draw_into and encode_span_diff/encode_row_streaming now always use absolute cursor positioning (ESC[{row};1H) instead of "\r\n" chaining. Also removed the now-dead last_written_line / last_written_row variables.

Verification (2026-10-16, Windows): fmt clean; workspace all pass (client 80, engine 91); tools content ok; both clippy orders clean; client --demo exit 0; viewer/play/fx/hero checks ok. Not committed/pushed.

## Full-canvas + transient-line fix (2026-10-17, this session)

User asked for: (1) GPU by default - already default features = ["gpu"], no flag needed; only --cpu forces CPU. (2) transient sub-ms black line - fixed at the output layer: removed \r\n chaining in both PlainTerminal::draw_into and SpanDiffEncoder; full-width rows auto-wrap the cursor, so "\r\n" landed one row too far down and left stale cells. Now absolute cursor jumps only. (3) whole canvas without HUD overlay - the GPU renderer now renders at the scene area dimensions (terminal minus top bar/log/legend) by computing scene dims in format_frame_with_hud and resizing only when they change, tracking last scene dims. HUD is drawn by Ratatui in its own regions, so the 3D scene fills exactly its canvas and never spills into log/sidebar. Dead-variable warnings cleaned up.

Verification (2026-10-17, Windows): fmt clean; workspace all pass (client 80, engine 91); tools content ok; both clippy orders clean; client --demo exit 0; viewer/play/fx/hero checks ok. Not committed/pushed.

## Game feel, character animation & visual fidelity refinement (2026-10-18)

Request: perform iterative refinement of textures/materials, character construction, animation smoothness, movement response, and overall gameplay feel without changing authority, mechanics, or the existing renderer architecture.

The audit found two different presentation paths. The terminal GPU path already had procedural character meshes and server-authoritative snapshots, but had several material/animation inconsistencies. The windowed 3D `play` example was more severe: it rebuilt a static `Mesh::humanoid()` every frame, used fixed glide smoothing, updated facing only on whole-cell steps, and rendered boxed tree canopies. The following twelve bounded passes were implemented and checked incrementally:

1. **Baseline architecture audit.** Confirmed the protocol/server boundary remains authoritative; identified the static windowed humanoid, integer-cell raster output, material-3 wind leakage, missing GPU attack phase, duplicate champion weapon, label-height mismatch, flat terrain normals, and indistinct road/sand/stone glyph families.
2. **Shared GPU gait timing.** Replaced mixed wall-clock/fixed-step cadence with one frame-rate-independent gait rate and exponential idle fade. Initial stationary actors now start with planted feet instead of entering a walk cycle.
3. **Foliage/static-organic separation.** Material-3 wind displacement and glyph-time animation are now gated to non-entity foliage. Skeleton bones, mossling surfaces, and the herbalist staff no longer sway as if they were grass.
4. **Presentation attack motion.** Existing combat log messages now drive a short presentation-only attack phase for the player and identified remote attackers; humanoid, skeleton, champion, and mossling geometry receive the phase while server combat remains unchanged.
5. **Grounding and light robustness.** Grass blades use the same rolling meadow height field as the terrain, point-light normalization rejects zero distance/radius, and dead actors stop contributing creature light pools.
6. **Windowed articulated humanoid pose.** Added `raster::HumanoidPose` and `Mesh::humanoid_pose`: overlapping torso/hip pieces plus joint-pivot legs, arms, shield, and weapon with gait, attack, breathing, foot-contact correction, and deterministic tests. Legacy `Mesh::humanoid()` remains intact for older examples.
7. **Responsive windowed movement/facing.** Added shortest-angle helpers and exponential facing response. `play` derives target facing from live camera-relative input before a discrete server step, advances gait from bounded velocity, and keeps the camera and player on the same smoothed presentation anchor.
8. **Input safety and combat timing.** Window focus loss now releases tracked keys/buttons and clears the pointer drag baseline. Freshly observed windowed combat FX are stamped with the actual observation time instead of appearing pre-expired.
9. **Environment silhouette pass.** Added deterministic low-poly `raster::Mesh::cone` facets and replaced flat boxed tree canopies in `play` with layered cones; added a restrained raised road ribbon and curb landmarks so movement/depth have a stronger visual reference.
10. **Surface-material pass.** GPU road, beach, and stone curb vertices now use dedicated material ids and glyph languages. Meadow quads calculate slope-aware normals from their shared corner height field, improving depth/lighting agreement without a new pass.
11. **Character cohesion pass.** Armored skeletons no longer inherit the base rusted blade when they add their authored executioner sword. GPU remote actors rotate toward the orbit camera, projected label anchors were brought closer to each model's scaled head, humanoid arms/legs use small pitch-pivot articulation, and grounded correction prevents walking boots from dipping below y=0.
12. **Weather and regression pass.** Storm now has denser, faster, longer, brighter rain than ordinary rain. Added tests for cone slope normals, terrain materials/normals, storm differentiation, pose articulation/attack/grounding, duplicate weapon removal, focus clearing, angle wrapping, and attack message parsing.

Changed files in this refinement: `engine/src/raster.rs`, `engine/src/motion.rs`, `engine/src/present.rs`, `engine/examples/play.rs`, `client/src/gpu/mesh.rs`, `client/src/gpu/renderer.rs`, and `client/src/gpu/shaders.rs`. The pre-existing local `play_ansi.bat` modification was preserved and not edited.

Verification (2026-10-18, Windows):

- `cargo fmt --all -- --check`: passed.
- `cargo test --workspace --locked`: passed on the sequential rerun; 105 client, 105 engine, 8+3 server, 14 shared, 2 tools, and 1 bot tests passed, with doc tests clean. A concurrent first attempt hit the known Windows `LNK1104` collision on `target\\debug\\examples\\play.exe` while a window proof was linking; the sequential rerun passed.
- `cargo run --locked -p ascii-mmorpg-tools -- content`: passed, `content ok: 11 definitions`.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: passed.
- Targeted engine/client GPU tests and both present/gpu clippy checks passed.
- `cargo run -p ascii-mmorpg-engine --locked --features present --example play -- --check`: passed.
- `cargo run -p ascii-mmorpg-engine --locked --features present --example play -- --frames 60`: presented 60 Vulkan frames successfully on the available NVIDIA MX230.
- `cargo run -p ascii-mmorpg-client --features gpu -- --wgpu-demo 5 96x28`: rendered 5 GPU frames successfully; the debug run measured 77.35 ms/frame, so this is a correctness smoke rather than a release performance claim.
- `git diff --check`: passed; only line-ending conversion warnings were reported by Git.

Honest remaining limitations: the depth rasterizer is still CPU-side and resolves to one terminal cell after a combined world pass, so it does not yet provide true sub-cell actor interpolation or skeletal skinning. The windowed scene still lacks rain/weather overlays and TCP authority. The terminal GPU path still uses procedural meshes rather than the authored multi-frame ASCII sheets. Human live judgment remains needed for orbit drag feel, foot sliding at different zooms, the new road/tree silhouettes, and the 156x44 TUI material/attack presentation.

Next action: run the live windowed play command and `play_ansi.bat` at 156x44, then choose whether to tune visual timings from that verdict or continue the remaining visual queue. Do not commit or push unless explicitly requested.

## Critical visual overhaul — visibility, world detail, and readability (2026-10-18)

Request: overhaul the difficult-to-see 3D MMORPG presentation from the supplied screenshot baseline: dark flat field, tiny actors, weak terrain hierarchy, and unreadable HUD. The windowed `engine/examples/play.rs` path is the affected target; the terminal GPU client remains a separate renderer.

### Root causes found

- The default window was a 192×70 grid at 8px cells, with `CAM_DIST=17`, `CAM_FOCAL=34`, and a world width of roughly 96 units. The player model was scaled to 0.62, so the game showed a strategic-sized field while gameplay actors occupied only a few rows.
- Raster fog reduced distant geometry by 75%, the void/background was near-black, and the play scene used low-luminance meadow blocks. This compounded into punctuation-like terrain with little midground context.
- `DepthOpts.edge_step=0.28` caused small terrain depth differences to resolve as many horizontal/diagonal edge glyphs, producing visual noise instead of coherent ground masses.
- Every non-item entity in the windowed example used the same humanoid recipe with recolours. Mosslings, skeletons, and NPCs therefore lacked distinct silhouettes.
- Labels were one-cell text with sparse backing, and the HUD was composed entirely from one-cell layer text. The presenter already supported crisp overlay text, but `play` did not use it.

### Fifteen refinement passes completed

1. **Baseline audit:** traced camera → raster projection → cell resolution → presenter composition and identified the visibility bottlenecks above.
2. **Gameplay framing:** changed the default window to 160×56 at 10px cells, reducing the excessive strategic field while keeping a substantial world view.
3. **Camera scale:** moved the default orbit from 17 to 13.5 units, raised focal length from 34 to 48, kept orbit bounds predictable, and retained the elevated OSRS pitch.
4. **Resize stability:** camera focal scales with the smaller width/height resize ratio so larger windows reveal context without shrinking the controlled character.
5. **Raster visibility:** lifted the near-black void from (5,6,12) to (9,12,20), raised default ambient lighting, reduced fog dimming from 75% to 52%, and increased the play far plane to 64.
6. **Edge-noise control:** raised the play depth-edge threshold to 0.48 so smooth ground does not become a field of repeated dash rules.
7. **Ground hierarchy:** brightened and diversified meadow blocks, retained a clearly raised road ribbon/curbs, and added broad clearing patches instead of increasing per-cell stippling.
8. **Foreground landmarks:** added low two-tier stone clusters near the play area and brighter faceted trunks/canopies so foreground scale is legible.
9. **Player prominence:** increased the player scale from 0.62 to 0.96 and raised humanoid material values for readable steel, cloth, leather, and gold separation.
10. **Distinct actor recipes:** added grounded raster meshes for mosslings, skeletons, and herbalists with separate silhouettes, muzzle/horns, rib gaps/weapon, hood/staff/lantern respectively. The player keeps the articulated humanoid pose system.
11. **Entity scale/grounding:** tuned monster/NPC scales, added contact-shadow decals under actors, enlarged loot boxes, and added loot ground accents.
12. **Combat readability:** added a world-space orange dashed target ring in addition to the label/HP treatment; target selection follows the existing melee-range rule.
13. **Nameplate readability:** moved labels to the actual model head heights, added opaque backing rectangles, role-aware colors, and preserved HP bars/digits.
14. **HUD hierarchy:** added a large crisp `WHISPERING FOREST` title and large zone/HP/tick overlay text using the presenter's existing `TextRun` path; added HUD/log borders and less cramped secondary controls.
15. **Integration pass:** validated combat, movement, inventory, resize-aware framing, actor recipe grounding, full workspace behavior, content, clippy, and a 60-frame Vulkan presentation run.

### Changed files

- `engine/examples/play.rs`: camera/framing, palette, terrain patches/landmarks, actor dispatch/scales, target/shadow/loot markers, readable HUD overlays, label backing, resize focal scaling, and presentation proof.
- `engine/src/raster.rs`: visibility/fog defaults, brighter void, articulated/grounded mossling/skeleton/herbalist mesh recipes, and distinct-silhouette tests.
- `docs/SESSION_HANDOFF.md`: this record.

### Verification (2026-10-18, Windows)

- `cargo fmt --all -- --check`: passed.
- `cargo test --workspace --locked`: passed — 105 client tests, 106 engine tests, 8+3 server tests, 14 shared tests, 2 tools tests, 1 bot test, and doc tests clean.
- `cargo run --locked -p ascii-mmorpg-tools -- content`: passed, `content ok: 11 definitions`.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: passed.
- `cargo clippy -p ascii-mmorpg-engine --features present --all-targets -- -D warnings`: passed.
- `cargo run -p ascii-mmorpg-engine --features present --example play -- --check`: passed.
- `cargo run -p ascii-mmorpg-engine --locked --features present --example play -- --frames 60`: presented 60 Vulkan frames on the NVIDIA MX230.

### Remaining limits and live review

The windowed raster path is still a CPU depth-to-cell renderer rather than a true GPU 3D scene with skeletal skinning. The available automated proof confirms geometry, labels, controls, resize math, and Vulkan presentation, but cannot replace a human comparison of the supplied screenshot. A fresh live run should check the close gameplay view, right-drag orbit, wheel zoom, resize, `F`/`1-3` combat readability, and whether the larger silhouettes now dominate the terrain as intended. No commit or push was made.

## Dark-fantasy environment refinement — moonlight, landmarks, and warm pools (2026-10-18)

This bounded follow-up continued the visibility overhaul without changing gameplay, camera input, protocol, or the terminal renderer. The target remains the windowed `engine/examples/play.rs` depth-raster path.

### Implemented

- Added a cool moonlight tint to `raster::DepthOpts`, with a deliberately low default strength so warm local materials retain their identity. Ground-cell resolution now uses deterministic hashed quiet moss/earth glyphs and dark material-specific backgrounds instead of repeated horizontal bands.
- Added four far midnight-indigo backdrop walls plus distant low conifer silhouettes so missed rays resolve to a readable night sky rather than a hard black horizon.
- Added three localized torch anchors: two-tier warm ground pools, physical posts, bright caps, and flame cones. This creates warm navigation points while keeping the implementation within the current single-pass mesh rasterizer.
- Added an ancient stone gate landmark with cool masonry, crimson hanging banners, and restrained violet ground markers. Existing tree roots and canopy layers remain part of the scene composition.
- Increased player, monster, NPC, and skeleton presentation scales and raised projected head anchors so the larger silhouettes and nameplates stay aligned. HP mini-bars now use solid block glyphs for clearer contrast.
- Reworded and repositioned the windowed HUD toward the moonlit wilderness presentation while preserving the existing controls and title/status overlays.

### Changed files

- `engine/examples/play.rs`: backdrop, distant silhouettes, torch pools, gate/banners/markers, actor scale and label anchors, HUD copy, and moonlight options.
- `engine/src/raster.rs`: `DepthOpts` moon tint/strength, quiet ground glyph selection, deterministic cell hash, and ground background treatment.
- `play_ansi.bat`: pre-existing user modification preserved and not included in this renderer slice.

### Verification (2026-10-18, Windows)

- `cargo fmt --all -- --check`: passed.
- `cargo test --workspace`: passed — client 105, engine 106, server 8+3, shared 14, tools 2, bot 1; doc tests clean.
- `cargo run -p ascii-mmorpg-tools -- content`: passed, `content ok: 11 definitions`.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `cargo run -p ascii-mmorpg-engine --locked --features present --example play -- --check`: passed (`3D world, FX digits, orbit movement, inventory modal`).
- `cargo run -p ascii-mmorpg-engine --locked --features present --example play -- --frames 60`: presented 60 Vulkan frames on the NVIDIA MX230 with no runtime error.
- `cargo run -p ascii-mmorpg-client -- --demo`: completed the deterministic six-frame terminal smoke run.
- `git diff --check`: passed; Git reported only the existing LF/CRLF conversion warnings.

The first parallel workspace-test attempt encountered Windows `LNK1104` because an earlier live `play.exe` still held the target executable open; after stopping that stale process, the complete workspace test rerun passed. The finite 60-frame window smoke exited cleanly and left no `play` process running.

### Remaining live review

Automated checks do not establish the artistic result. The next human check is the live window: compare the new cool sky, distant tree line, gate, torches, ground quieting, actor prominence, orbit/zoom, and `F`/`1-3` effects against the reference. Then re-check `play_ansi.bat` at 156x44 for the carried TUI readability items. No commit or push was made.

## Colored-glyph-only renderer overhaul — pure black world viewport (2026-10-18, latest)

Request: correct the previous dark-fantasy pass so the MMORPG remains a genuine ASCII renderer. Environmental colour, lighting, material identity, and atmospheric detail now arrive through visible glyph foregrounds, glyph density, and placement. The world viewport is pure black; HUD/modal panels retain their intentional interface styling.

### Root cause of the coloured background

The windowed path had three independent background sources: `raster::resolve_cell` assigned `VOID_BG`/material-scaled backgrounds to every hit cell, `present::SHADER` blended each glyph quad's `bg` across the cell outside the atlas alpha, and the presenter clear colour was a dark navy rather than black. The scene builder amplified the problem with large coloured sky walls, meadow/road/ocean planes, warm torch pool planes, and bridge light/glow passes that repainted world backgrounds. The terminal GPU path also generated a dark per-cell `bg` channel and used a non-black terminal default.

### Twelve refinement passes completed

1. **Renderer audit:** traced `play.rs` → camera/projection → `raster::render_layer` → `compose::Frame` → `present::frame_instances`/WGSL and separately traced the terminal GPU lowering path. Identified every world background source above.
2. **Black-background guarantee:** rasterized world cells now use `Rgb::new(0,0,0)` backgrounds; the windowed render target clear is black; world instances force black backgrounds; the glyph shader bypasses washes for world cells; bridge stage/world constants and terminal defaults are black.
3. **Buffer/depth correctness:** `Frame` now records a world-cell mask, preserves partition ownership through composition, skips world background wash/glow writes, applies world glow to existing glyph foregrounds only, and clears the final world background channel after all passes. Empty cells remain `Cell::CLEAR`.
4. **Centralized palette:** `play.rs` now uses a `WorldPalette` for foliage, bark/root, path, grass, stone, water/sand, fire/ember, magic, and actor shadows. Interface colours remain separate HUD/label constants.
5. **Glyph density mapping:** `raster.rs` classifies visible faces into foliage, bark, earth, stone, water, fire, magic, metal, or generic material languages. Density and edge selection use luminance, fog distance, surface depth structure, material identity, and deterministic hashing rather than one global ramp.
6. **Terrain refinement:** removed the continuous meadow ground field and replaced it with sparse stable path marks, shoreline/wave segments, and hashed grass tufts. Large unoccupied terrain areas are now black instead of a dotted or filled carpet.
7. **Structural refinement:** removed the skybox/far-wall geometry and large road/beach/ocean/torch pool planes. Trees gained trunk roots and branch solids; stones, the gate, crimson banners, and violet markers remain as selective glyph geometry.
8. **Directional shading:** retained the face-normal diffuse/depth raster and added a restrained configurable moon tint (`DepthOpts::moon_tint`/`moonlight_strength`) that changes glyph foreground RGB only.
9. **Localized lighting:** added raster `PointLight` sources. Torches and active combat FX brighten/tint nearby visible triangle samples with quadratic distance falloff; no screen-space halo or coloured empty-cell pool is emitted.
10. **Entity refinement:** preserved enlarged actor recipes, nameplates, HP bars, shadows, target rings, damage digits, and combat FX while forcing labels/world backings to black and keeping actor/material glyph colours distinct.
11. **Depth/camera validation:** preserved the orbit camera, camera-relative movement, focal/zoom behaviour, depth testing, occlusion, and resize path. The world partition is marked as a camera/world partition for black-ink enforcement even though the 3D camera is baked inside the raster pass.
12. **Final polish/regression:** added black-background and point-light tests, updated glow/light tests to assert foreground-only world lighting, changed the terminal GPU packed background output to zero, and ran workspace, content, clippy, deterministic demo, gameplay, and Vulkan presentation checks.

### Changed files

- `engine/src/raster.rs`: black world cells, material-aware glyph profiles, moonlight foreground shading, `PointLight`, glyph-only local lighting, and focused regression tests.
- `engine/src/compose.rs`: world-cell ownership mask, black finalization, foreground-only world glow, and black-background regression test.
- `engine/src/present.rs`: pure-black surface clear, world-instance black backgrounds, world-only wash bypass in WGSL, and presenter regression assertions.
- `engine/examples/play.rs`: centralized foreground palette, selective glyph terrain/landmarks, no sky/floor/pool fill planes, enlarged actor labels, torch/combat point lights, and foreground-only world partition.
- `engine/src/bridge.rs`: black world/stage palette and foreground-only light-pool/glow behavior.
- `client/src/gpu/pipeline.rs`: terminal GPU scene colour target clears to black.
- `client/src/gpu/shaders.rs`: packed GPU background channels are retained for ABI compatibility but always zero.
- `client/src/gpu/renderer.rs`: terminal scene lowering discards GPU background colour and writes black scene backgrounds.
- `client/src/partitions.rs`: blank scene cells use black foreground/default state.
- `client/src/terminal_colors.rs`: terminal/notcurses default background is pure black.
- `docs/SESSION_HANDOFF.md`: this record.
- `play_ansi.bat`: pre-existing user modification preserved and not included in this renderer change.

### Verification (2026-10-18, Windows)

- `cargo fmt --all -- --check`: passed.
- `cargo test --workspace`: passed — client 105, engine 109, server 8+3, shared 14, tools 2, bot 1; doc tests clean.
- `cargo run -p ascii-mmorpg-tools -- content`: passed, `content ok: 11 definitions`.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `cargo clippy -p ascii-mmorpg-engine --features present --all-targets -- -D warnings`: passed.
- `cargo test -p ascii-mmorpg-engine --features present --lib`: passed — 121 tests, including black world/background and point-light tests.
- `cargo run -p ascii-mmorpg-engine --locked --features present --example play -- --check`: passed — 3D world, FX digits, orbit movement, and inventory modal.
- `cargo run -p ascii-mmorpg-engine --locked --features present --example play -- --frames 60`: presented 60 Vulkan frames on the NVIDIA MX230.
- `cargo run -p ascii-mmorpg-client -- --demo`: completed the deterministic six-frame terminal smoke run with the existing HUD styling and black scene default.
- `git diff --check`: passed; only the repository's existing LF/CRLF conversion warnings were reported.

### Visual evidence and remaining review

Captured comparable windowed output at `%TEMP%\omnirush\reco-analysis\ascii_black_world.png`, `ascii_black_world2.png`, and `ascii_black_orbit_zoom.png`. The world viewport is visibly black between glyphs; tree foliage, bark, stone, water, magic, fire, player/enemy silhouettes, labels, and combat marks are foreground-colored glyphs. The second capture shows denser material-specific foliage/branch silhouettes while retaining large black negative space; the orbit/zoom capture was taken after sending a right-drag and wheel event to the live window.

The automated checks cannot judge artistic parity or camera feel. The remaining human review is to run the live window, right-drag orbit, wheel zoom, walk, `F`, `1/2/3`, and `I`, then compare a close and wide view against the reference. No commit or push was made.
