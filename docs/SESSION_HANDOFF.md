# ASCII MMORPG — session handoff

Updated: 2026-10-12. This is the resumable engineering record, not a full transcript.

## Start here

Workspace: `ASCII-Game/`; application repository: `ascii-mmorpg/`. Read the parent and repository `AGENTS.md` files.
Repository status: All recent work has been committed and pushed to `origin/main` in commit `5652ab6` per user request. The working tree is clean.

Current slices completed:
- Pine tree trunk visibility: elevated lowest canopy cone to Y = 3.60, assigned trunk and root flares dedicated Wood/Timber material (7.0) with vertical bark striations, cleared highway collision corridor (|x| <= 4), and synchronized procedural tree rendering on GPU.
- Synchronized meadow grass wind sway: low-altitude vertex sway in `vs_main` and wind-leaning ASCII blade characters in `ASCII_POST_WGSL` traveling in sync with wind current phase.
- Smooth transitions: exponential spring damping for camera and player coordinates, plus ease-out deceleration for humanoid walk phase.
- GPU animation/damping is frame-rate independent, stationary terrain is separated from foliage wind, while deterministic batch/demo rendering remains intact.
- Server tick documentation now matches the implementation, and the TCP loop has a regression test covering idle timeout and immediate input advancement.

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
- [ ] Defer reflections and a separate upscaler until a benchmark shows the existing 4× SSAA path is the bottleneck and a minimal design is chosen.

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
