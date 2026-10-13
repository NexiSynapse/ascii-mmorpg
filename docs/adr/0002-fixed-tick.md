# ADR 0002: Fixed-tick simulation

## Status

Accepted for the alpha; the original 10 Hz/single-client decision is superseded by
the current 20 Hz-target implementation described below.

## Decision

The server owns simulation time and exposes a strict 20 Hz scheduler
(`TICK_HZ = 20`). The loopback runner is deterministic. The TCP runner drains a
bounded input queue (`MAX_PENDING = 512`) at tick boundaries: one tick per
50 ms, idle or loaded, so burst input coalesces into the next tick and can no
longer exceed 20 Hz. Protocol-version mismatches are rejected immediately
without ticking or queueing. Snapshots include the authoritative tick. The TCP
runner supports multiple connected clients through one simulation thread.

## Consequences

Combat and loot receive tick-derived seeds, which makes unit tests reproducible and
keeps clients from choosing outcomes. A future scheduler change must preserve
single-owner simulation ordering while applying queued commands at tick boundaries.
