# ASCII MMORPG alpha

This repository contains a compileable first slice of a Rust terminal MMORPG:

- `crates/shared`: versioned postcard protocol, length-prefixed framing, stats,
  deterministic combat, and weighted loot.
- `server`: fixed-tick simulation plus a loopback smoke path and blocking TCP
  skeleton (`cargo run -p ascii-mmorpg-server -- --tcp 127.0.0.1:4000`).
- `client`: plain ANSI/ASCII fallback renderer and command parser. The default
  renderer draws a deterministic forest/dungeon scene with compile-time
  embedded, workspace-authored ASCII frames for warriors, Mosslings, NPCs,
  props, items, and spell effects. Entity labels, status bars, rain, and the
  spell trail remain layered over the animated assets. The optional `notcurses`
  feature is a deliberately dependency-free placeholder until native terminal
  integration is chosen.
- `bot`: small TCP load client that sends deterministic commands.
- `tools`: RON content validator for monsters, loot, items, spells, and zones.

With stable Rust installed:

```text
cargo test --workspace
cargo run -p ascii-mmorpg-tools -- content
cargo run -p ascii-mmorpg-server
cargo run -p ascii-mmorpg-client
cargo run -p ascii-mmorpg-client -- --demo
```

The client starts in real-time mode: movement and actions respond immediately
to key presses without Enter. Use `w/a/s/d` or the arrow keys to move, `f` to
attack, `r` to rest, and `q` or Escape to quit. The older line-input mode is
still available with `cargo run -p ascii-mmorpg-client -- --line`.

To connect the real-time client to the TCP server, start the server and pass
its address with `--connect`:

```text
cargo run -p ascii-mmorpg-server -- --tcp 127.0.0.1:4000
cargo run -p ascii-mmorpg-client -- --connect 127.0.0.1:4000
```

Connected mode sends each key command immediately using the shared postcard
framing and renders authoritative `Snapshot` events from the server. If the
server closes the connection, the client reports it and exits cleanly.

The TCP server accepts multiple clients concurrently. A dedicated simulation
thread owns the authoritative world, while per-client reader and writer threads
handle sockets. Each authoritative snapshot is broadcast to all connected
clients with a personalized local-player view. Each connection now owns a
server-side player entity, while other connected players are included as
remote entities in the snapshot.

Snapshots also carry explicit `EntityKind` metadata (`Player`, `Monster`,
`Npc`, or `Item`) so the client scene renderer can choose visuals independently
from server gameplay logic.

Render frames are selected from the snapshot tick (or a demo/replay frame) and
never mutate or replace authoritative snapshot data. The current ASCII library
is documented in `assets/ascii/MANIFEST.md`. These plain-text frames are
original workspace-authored assets embedded with `include_str!`, so rendering
does not depend on the process working directory.

The Kenney CC0 source packs are staged under `assets/source/kenney/`. They are
optional source references for future tooling; they are not loaded by the ASCII
renderer. The client does not load, convert, or transmit glTF/FBX or other
source models.

The checked-in slice is deliberately honest about the alpha boundary: it has
the shared protocol, deterministic combat/loot rules, fixed 20 Hz simulation,
TCP framing, an ASCII renderer, bot smoke coverage, and validated RON content.
Authentication, SQLite persistence, map streaming, ECS, and native notcurses
output are the next implementation slices (see
`docs/adr/0004-alpha-boundaries.md`).
