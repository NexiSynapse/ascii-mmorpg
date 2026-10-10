# ADR 0002: Fixed-tick simulation

## Status

Accepted for the alpha.

## Decision

The server owns simulation time and advances it at 10 ticks per second. Commands
are applied at the next fixed tick; snapshots include the authoritative tick. The
loopback runner is a deterministic smoke path and the TCP runner is intentionally
single-client and blocking until a later session introduces scheduling and
authentication.

## Consequences

Combat and loot receive tick-derived seeds, which makes unit tests reproducible and
keeps clients from choosing outcomes.
