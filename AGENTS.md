# AGENTS.md

## Source of truth
- `docs/SOT.md` is authoritative for what exists. `docs/SESSION_HANDOFF.md` is the resumable engineering record (tick semantics, last slice, next action). `docs/PROJECT_PLAN.md` is direction, not done work. `README.md` is the runnable overview.
- ADRs in `docs/adr/`: 0001 protocol, 0002 fixed-tick (current: 20 Hz idle target, not strict scheduler), 0003 ANSI fallback, 0004 alpha boundaries.
- If docs conflict with code/config, trust code.

## Workspace
- Cargo workspace (`edition 2021`): `crates/shared` (`ascii-mmorpg-shared`), `server`, `client`, `bot`, `tools`. Vendored `crates/libnotcurses-sys` via `[patch.crates-io]` — do not touch unless working on notcurses.
- Authority split: server owns positions/stats/combat/ticks/snapshots; client owns input/presentation/animation. Never let clients author state; never send rendered frames over the wire; renderer must not mutate snapshots (`render_frame(&world, frame)` only reads).

## Verify (CI order in `.github/workflows/ci.yml`, ubuntu + windows)
```text
cargo fmt --all -- --check
cargo test --workspace
cargo run -p ascii-mmorpg-tools -- content
cargo clippy --workspace --all-targets -- -D warnings
```
- Focused: `cargo test -p <package> <test_name>` (e.g. `cargo test -p ascii-mmorpg-shared combat`).
- `cargo run -p ascii-mmorpg-client -- --demo` renders 6 deterministic frames and exits — cheapest renderer smoke test.

## Protocol (`crates/shared/src/protocol.rs`, `codec.rs`)
- `PROTOCOL_VERSION: u16 = 2`. `ClientCommand.protocol_version` is first field; server rejects mismatch. Bump version on any wire change.
- Framing: 4-byte LE length + postcard payload, 1 MiB max (`MAX_FRAME_SIZE`), non-empty. Use `encode_frame/decode_frame/read_frame/write_frame`; do not invent framing.
- `WorldSnapshot.player` is the per-connection local view; `entities` holds remotes + mobs. `EntityKind` (`Player/Monster/Npc/Item`) drives client visuals, not gameplay.
- Combat is deterministic: `resolve_attack(attacker, defender, tick ^ target_id)`, Manhattan `MELEE_RANGE = 6` (`combat.rs`).
- Collision: deterministic AABB ground-footprint boundaries + diagonal sliding in `crates/shared/src/collision.rs` (`resolve_movement`, `overworld_obstacles`). Both `server::move_player` and `client::apply_local_command` use it; canopy/foliage never blocks. Highway corridor `|x| <= 4` is kept clear.

## Server (`server/src/main.rs`, `lib.rs`)
- `cargo run -p ascii-mmorpg-server` prints loopback demo; `cargo run -p ascii-mmorpg-server -- --tcp 127.0.0.1:4000` serves TCP (multi-client reader/writer threads + one sim thread, per-client `snapshot_for(id)` broadcast).
- `TICK_HZ = 20` is a strict scheduler: the TCP sim thread drains a bounded input queue (`MAX_PENDING = 512`) at tick boundaries — one tick per 50 ms idle or loaded, so burst input coalesces and can never exceed 20 Hz. Protocol mismatches are rejected immediately without ticking. See ADR 0002. `Simulation::advance_tick_with` is the batched entry point; `fixed_tick_for` remains for loopback/tests.

## Client (`client/src/main.rs`, `lib.rs`, `renderer.rs`, `gpu/`, `ui.rs`, `term.rs`)
- Modes: default realtime (`WASD`/arrows move, `F` attack, `R` rest, `I` inventory, `C` character, `M` map, `P` pause, `V`/`T` cycle weather, `Q`/`Esc` quit); `--line` legacy stdin mode; `--demo` offline demo (6 frames); `--connect ADDR` TCP mode. Pause (`ActiveModal::Paused`) freezes local input and shows a large-font menu; `Q` quits directly from pause. Client-side only — the server keeps ticking online.
- `--fps`/`--weather` use `rposition` so CLI overrides `.bat` defaults: `--fps <N|uncapped|max>` (default 60, `0` = uncapped), `--weather <rain|snow|storm>` (default rain). `--notcurses` requires `--features notcurses`.
- GPU is the default feature (`gpu`, alias `wgpu-scene`): offscreen 3D scene → color/normal-depth/entity-metadata targets → WGSL compute ASCII pass (`client/src/gpu/`). Always renders the native terminal grid (never downscaled); large grids use adaptive SSAA (4x ≤4096 cells, else 2x). The formatter quantizes channels to 32 levels for ANSI run-merging. Terrain/grass/fallback ramps stay quiet at bright bands (strokes, then `░▒▓█` light pools — never alphabet texture); structural vertical edges emit `|`; letter detail lives on entity materials so actors pop. Falls back to CPU renderer automatically on init failure; `--cpu` forces fallback. `--wgpu-demo [N] [WxH]` is the headless benchmark (optional size simulates zoomed-out grids). There is no `--wgpu` realtime flag.
- `PlainTerminal` is a differential dirty-line rasterizer (`\x1b[{row};1H` + `\x1b[K`, full repaint only on line-count change or resize) to avoid Windows ConPTY tearing. Preserve that behavior. `client/src/term.rs` (`CellMatrix`, span-diff encoder) and Ratatui HUD in `client/src/ui.rs` feed through the same ANSI path.
- Terminal backends stay behind `Terminal` trait in `client/src/lib.rs`. Keep bindings out of gameplay/renderer code. Frames compose in one Ratatui buffer (`RatatuiHud::render_composed_frame` + `ComposedScene` spec in `client/src/ui.rs`): HUD top bar, blitted scene (`partitions::SceneCell`/`blit_scene`), side minimap/threat panel (terminals ≥64 cols), log strip, legend, modal overlay with `Clear` occlusion. Output is always exactly terminal-sized. CPU inks lower via `Ink::rgb`; GPU cells map directly (quantized). Minimap stays inside the HUD (no extra GPU pass).
- GPU monster dispatch keys off label text (`warrior` → champion, `skeleton` → skeleton, else mossling) in `gpu/renderer.rs`; CPU asset selection mirrors it in `assets.rs`.

## Assets (`assets/ascii/`, `assets/ASSET_SOURCES.md`)
- Frames are `include_str!`-embedded, `---` on its own line separates animation frames; runtime never reads asset files from disk. Item icons key off label text (`sword`/`shield`/generic). Separator matching is line-based, so art may safely contain runs like `|---|`.
- `assets/ascii/MANIFEST.md` lists the real visual inputs; `assets/source/kenney/` (CC0) are reference-only, never loaded. Do not bulk-copy outside art without recording exact license/attribution in `ASSET_SOURCES.md`.

## Content (`content/*.ron`, `tools/`)
- `cargo run -p ascii-mmorpg-tools -- content` validates `content/` (default arg is `content` dir). Enforces: exactly 3 spells, non-empty items, ≥2 zones, monster `loot` refs exist, loot weights > 0, `min <= max` with `min > 0`.
- Bot smoke: start server with `--tcp`, then `cargo run -p ascii-mmorpg-bot -- 127.0.0.1:4000 10`.

## Launchers (`play_*.bat`)
- Path-independent via `%~dp0`; never hardcode install paths. Exe lookup prefers `target\release`, falls back to `target\debug`; all default `--fps uncapped`.
- `play_notcurses.bat` probes `target\x86_64-pc-windows-gnu\{release,debug}` first, resolves MSYS2 UCRT64 via `%MSYS2_ROOT%` (default `C:\msys64`, fallback `C:\tools\msys64`), warns instead of failing if absent, keeps `chcp 65001`. `play_wezterm*.bat` must keep targeting `wezterm-gui.exe start --always-new-process` (wrapper `wezterm.exe` detaches early → error 10054).
