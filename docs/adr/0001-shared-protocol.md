# ADR 0001: Shared postcard protocol

## Status

Accepted for the alpha.

## Decision

All executables exchange versioned Rust structs from `crates/shared`. Frames use a
four-byte little-endian payload length followed by a postcard payload. The frame
limit is one MiB and the first field in client commands is the protocol version.

## Rationale

This keeps the first playable slice dependency-light while retaining a compact,
typed wire format. A later protocol revision can add capability negotiation without
making the simulation depend on terminal or transport code.
