# ADR 0002: Fixed-tick simulation

## Status

Accepted for the alpha; the original 10 Hz/single-client decision is superseded by
the current 20 Hz-target implementation described below.

## Decision

The server owns simulation time and exposes a 20 Hz target (`TICK_HZ = 20`). The
loopback runner is deterministic. In the current TCP runner, the simulation thread
waits up to 50 ms for commands and advances one tick on an idle timeout; an input
command is applied immediately and also advances one tick. Consequently, sustained
input can advance faster than 20 Hz, so this is a 20 Hz idle target rather than a
strict fixed-rate scheduler. Snapshots include the authoritative tick. The TCP
runner supports multiple connected clients through one simulation thread.

## Consequences

Combat and loot receive tick-derived seeds, which makes unit tests reproducible and
keeps clients from choosing outcomes. A future scheduler change must preserve
single-owner simulation ordering while applying queued commands at tick boundaries.
