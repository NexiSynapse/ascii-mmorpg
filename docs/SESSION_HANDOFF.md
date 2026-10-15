# ASCII MMORPG — session handoff

Updated: 2026-10-15. This is the resumable engineering record, not a full transcript.

## Start here

Workspace: `ASCII-Game/`; application repository: `ascii-mmorpg/`. Read the parent and repository `AGENTS.md` files.
Repository status (verified 2026-10-13, Windows): HEAD `59d50c3` (local `main`, 3 commits ahead of `origin/main` — NOT pushed, do not push unless asked). Modified (tracked): `AGENTS.md`, `Cargo.lock`, `Cargo.toml`, `client/src/lib.rs`, `client/src/main.rs`, `docs/SESSION_HANDOFF.md`. New (untracked, do not delete): `engine/` (full crate, see below), `play_notcurses.sh` (Linux notcurses launcher). Pre-existing notcurses-input fix + strict 20 Hz scheduler work is in those modifications; all engine/port work is new files plus the two `AGENTS.md`s and this handoff. CRLF-vs-LF noise is historical (resolved take-theirs at merge); current diffs are real content.

Current slices completed:
- Pine tree trunk visibility: elevated lowest canopy cone to Y = 3.60, assigned trunk and root flares dedicated Wood/Timber material (7.0) with vertical bark striations, cleared highway collision corridor (|x| <= 4), and synchronized procedural tree rendering on GPU.
- Synchronized meadow grass wind sway: low-altitude vertex sway in `vs_main` and wind-leaning ASCII blade characters in `ASCII_POST_WGSL` traveling in sync with wind current phase.
- Smooth transitions: exponential spring damping for camera and player coordinates, plus ease-out deceleration for humanoid walk phase.
- GPU animation/damping is frame-rate independent, stationary terrain is separated from foliage wind, while deterministic batch/demo rendering remains intact.
- Server tick documentation now matches the implementation, and the TCP loop has a regression test covering idle timeout and immediate input advancement.

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

- [x] Add explicit-time per-cell colour palettes and dynamic colour-driven background glow; demonstrate actor palette overrides and bound extreme glow.
- [ ] Add simultaneous within-glyph gradient shading in the windowed presenter (current palette resolves one foreground RGB per glyph per frame).
- [x] Add within-glyph horizontal/vertical foreground gradients to the windowed presenter while preserving flat-colour defaults and glow.

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
- [x] Expand meadow grass density (~2x blades, ±18/±14, seed%2); reduce if batch timing regresses.
- [x] Zoom perf + default-zoom legibility: capped GPU render grid (`MAX_RENDER_CELLS = 6144`, aspect-preserving, CPU nearest-neighbor upscale), adaptive SSAA (4x ≤4096 cells, else 2x), closer camera (eye 9.8/+11.5, FOV 38°), stronger entity-outline faction mix (0.40→0.60), wider pillar cull bounds.
- [x] High-res zoom fix (reverted the render cap — it blurred everything): native 1:1 GPU grid at any zoom, adaptive SSAA only cuts sub-pixel samples; 5-bit ANSI channel quantization (~40% smaller frames, 98→59 KB at 220x70); hash-based dirty-line diffing in `PlainTerminal::draw_into` (no per-frame full-frame clone); `--wgpu-demo [N] [WxH]` size arg for headless zoom benchmarking.
- [x] Implement data-driven engine particle emitters (`Burst`, `Ring`, `Chain`, `Spike`, `Number`) with explicit-time sampling, fade/release, bounded output, deterministic tests, and an `fx` showcase migration.
- [x] Make the windowed presenter fullscreen-by-default with surface-sized grids, F11 toggle, reliable key edge handling, and resize-safe instance capacity.
- [x] Improve windowed demo models, fullscreen coverage, dynamic game controls, and clipped consistent HUD/log overlays.

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

```text
Read `D:\Personal\codes\Projects\ASCII-Game\AGENTS.md`, `D:\Personal\codes\Projects\ASCII-Game\ascii-mmorpg\AGENTS.md`, and `D:\Personal\codes\Projects\ASCII-Game\ascii-mmorpg\docs\SESSION_HANDOFF.md`. Inspect the current worktree and preserve all existing changes. Decide whether the server should gain a strict queued-command 20 Hz scheduler or retain its documented input-driven timing; do not implement unrelated rendering work. Record exact files, commands, results, blockers, and one next action in `docs/SESSION_HANDOFF.md` and update the TODO list. Do not commit or push.
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
