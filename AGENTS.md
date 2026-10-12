# AGENTS.md

## Source of truth
- `docs/SOT.md` is authoritative for what exists. `docs/EXTERNAL_AGENT_EXPLAINER.md` is the comprehensive architecture, pipeline, quality, and animation explainer for external agents. `docs/PROJECT_PLAN.md` is direction, not done work. `README.md` is the runnable overview.
- ADRs in `docs/adr/`: 0001 protocol, 0002 fixed-tick (stale: says 10 Hz single-client), 0003 ANSI fallback, 0004 alpha boundaries.
- If docs conflict with code/config, trust code.

## Workspace
- Cargo workspace (`edition 2021`, stable Rust): `crates/shared` (`ascii-mmorpg-shared`), `server` (`ascii-mmorpg-server`), `client` (`ascii-mmorpg-client`), `bot` (`ascii-mmorpg-bot`), `tools` (`ascii-mmorpg-tools`). Plus vendored `crates/libnotcurses-sys` via `[patch.crates-io]` — do not touch unless working on notcurses.
- Authority split: server owns positions/stats/combat/ticks/snapshots; client owns input/presentation/animation. Never let clients author state; never send rendered frames over the wire; renderer must not mutate snapshots (`render_frame(&world, frame)` only reads).

## Verify (CI order in `.github/workflows/ci.yml`, runs ubuntu + windows)
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
- Collision: deterministic AABB spatial boundaries and diagonal sliding physics in `crates/shared/src/collision.rs` (`resolve_movement`, `overworld_obstacles`). Both `server::move_player` and `client::apply_local_command` use it.

## Server (`server/src/main.rs`, `lib.rs`)
- `cargo run -p ascii-mmorpg-server` prints loopback demo; `cargo run -p ascii-mmorpg-server -- --tcp 127.0.0.1:4000` serves TCP (multi-client reader/writer threads + one sim thread, per-client `snapshot_for(id)` broadcast).
- Timing semantics: `TICK_HZ` is the 20 Hz simulation target. The TCP simulation thread advances once after each 50 ms receive timeout while idle, but each accepted input also calls `fixed_tick_for()` immediately; sustained input can therefore advance ticks faster than 20 Hz and prevent timeout-driven ticks. This is not a rigorously fixed-rate scheduler. Verify `server/src/main.rs` and `docs/SESSION_HANDOFF.md` before changing tick semantics.

## Client (`client/src/main.rs`, `lib.rs`, `renderer.rs`, `assets.rs`, `network.rs`)
- Modes: default realtime (`WASD`/arrows move, `F` attack, `R` rest, `V`/`T` cycle weather, `Q`/`Esc` quit); `--line` legacy stdin mode; `--demo` offline demo; `--connect ADDR` TCP mode.
- Flags use `rposition` so CLI overrides `.bat` defaults: `--fps <N|uncapped|max>` (default 60, `0` = uncapped), `--weather <rain|snow|storm>` (default rain), `--notcurses` requires `--features notcurses`.
- `PlainTerminal` is a differential dirty-line rasterizer (`\x1b[{row};1H` + `\x1b[K`, full repaint only when line count changes or on resize) to avoid Windows ConPTY tearing. Preserve that behavior.
- Terminal backends stay behind `Terminal` trait in `client/src/lib.rs`. Notcurses parsing handles only `0m` / `38;5;Nm`; keep bindings out of gameplay/renderer code.
- Client presentation: Ratatui-powered dark fantasy HUD (`Gauge`, `Block`, `Paragraph`, `Layout`) in `client/src/ui.rs`, composited seamlessly across both GPU and CPU renderers with dirty-state ANSI serialization.

## Assets (`assets/ascii/`, `assets/ASSET_SOURCES.md`)
- Frames are `include_str!`-embedded, `---` on its own line separates animation frames; runtime never reads asset files from disk. Item icons key off label text (`sword`/`shield`/generic).
- Monster art keys off label text too (`skeleton warrior` → warrior, `skeleton` → skeleton, else mossling). Separator matching is line-based, so art may safely contain runs like `|---|`.
- `assets/ascii/MANIFEST.md` lists the real visual inputs; `assets/source/kenney/` (CC0) are reference-only, never loaded. Do not bulk-copy outside art without recording exact license/attribution in `ASSET_SOURCES.md`.

## Content (`content/*.ron`, `tools/`)
- `cargo run -p ascii-mmorpg-tools -- content` validates `content/` (default arg is `content` dir). Enforces: exactly 3 spells, non-empty items, ≥2 zones, monster `loot` refs exist, loot weights > 0, `min <= max`.
- Bot smoke: start server with `--tcp`, then `cargo run -p ascii-mmorpg-bot -- 127.0.0.1:4000 10`.

## Notcurses / launchers
- Launchers are path-independent: they resolve everything at runtime via `%~dp0`, so a fresh clone works without editing. Never hardcode install paths back in.
- Exe lookup: `play_ansi.bat` prefers `target\release`, falls back to `target\debug`; `play_notcurses.bat` also probes `target\x86_64-pc-windows-gnu\{release,debug}` first. Both print `Using <exe>` and error with the `cargo build` command if nothing is found.
- `play_notcurses.bat` locates MSYS2 UCRT64 via `%MSYS2_ROOT%` (default `C:\msys64`, fallback `C:\tools\msys64`), prepends `ucrt64\bin` + `usr\bin` to `PATH`, warns instead of failing if absent, and keeps `chcp 65001`.
- `play_wezterm*.bat` locate WezTerm via `%WEZTERM_EXE%`, then `where wezterm-gui.exe`, then `%ProgramFiles%` / `%LOCALAPPDATA%` install dirs; they must keep targeting `wezterm-gui.exe start --always-new-process` (wrapper `wezterm.exe` detaches early → error 10054).
- All `.bat` launchers default `--fps uncapped`.

## GPU rendering (Path C, `gpu` default, `wgpu-scene` alias)
- Flags: `--wgpu` (explicit realtime 3D hardware offscreen pipeline, combines with `--connect`/`--fps`/`--notcurses`), `--cpu` (forces CPU rasterizer fallback), `--wgpu-demo [N]` (headless benchmark reporting adapter info, frametimes, and FPS).
- Pipeline: Offscreen 3D scene + real-time compute ASCII shader (`client/src/gpu/`). Default feature: `gpu` (`wgpu`, `bytemuck`, `pollster`, `glam`). Hardware instancing, 8 dynamic point lights, 4x SSAA (4x4 / 16 samples per cell) over `Rgba8UnormSrgb` color, `Rgba32Float` normal/depth, and `Rgba8Unorm` entity-metadata targets, Gribb-Hartmann frustum culling, O(1) depth unprojection, and asynchronous staging rings (`RING_SIZE = 2`) for high-throughput batch rendering.
- Graceful fallback: Automatically falls back to CPU procedural renderer if GPU initialization fails, or can be forced with `--cpu`: `cargo run -p ascii-mmorpg-client -- --cpu`.
- Verification: keep code warning-free under `cargo clippy --workspace --all-targets -- -D warnings`.

