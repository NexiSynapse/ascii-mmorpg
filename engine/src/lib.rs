//! Portable animated-ASCII scene engine.
//!
//! The terminal client can only run where a terminal exists. This crate is
//! the first slice of the replacement: a renderer-agnostic scene model where
//! **every cell can animate** — a single glyph ripples, pulses, or flip-books
//! through frames, like the reference captures in `data/videos/`.
//!
//! Borrowed from three.js (conceptually, not code):
//!
//! - `Scene` + [`Layer`] — three.js scene graph, flattened: ASCII compositing
//!   is a z-ordered layer stack (background / world / effects / hud), not a
//!   transform tree.
//! - [`Cell`] — three.js material: HOW one addressable glyph looks.
//! - [`Track`] / [`Anim`] — three.js `KeyframeTrack` / `AnimationClip`: per-cell
//!   channels (`Wave`, `Cycle`, `Pulse`) evaluated at time `t`.
//! - [`compose`] — three.js `Renderer.render` + `AnimationMixer`: samples every
//!   track at `t` and flattens layers into a [`Frame`].
//! - [`Partition`] — three.js scissor viewports: named rects (world view,
//!   sidebar, inventory, log) sharing one scene, each with its own layer set
//!   and optional camera follow.
//! - [`Camera`] — three.js camera, reduced to a world-space cell offset.
//!
//! The engine never touches I/O: `compose` returns plain [`Frame`]s. A future
//! presenter (windowed GUI via winit/wgpu, or the existing terminal client)
//! draws them. See `examples/wave.rs` for a headless animated preview.

mod anim;
pub mod bridge;
mod cell;
mod color;
mod compose;
pub mod demo;
pub mod fx;
mod layer;
mod partition;
mod scene;
pub mod sprite;

pub use anim::{sample, Anim, Axis, EaseKind, Sample, Track};
pub use cell::Cell;
pub use color::{ColorTrack, Rgb};
pub use compose::{compose, Frame};
pub use demo::{wave_scene, VIEW_H, VIEW_W};
pub use layer::{AnimCell, Layer, Shade};
pub use partition::Partition;
pub use scene::{Camera, Scene};
pub use sprite::{parse_frames, sprite_layer, sprite_size, SpriteDraw, FRAME_SEPARATOR};

/// Windowed presenter (real OS window, GPU glyph atlas). Behind the
/// `present` feature: needs a display at runtime, so CI never compiles it —
/// verify locally with `--features present`.
#[cfg(feature = "present")]
pub mod present;
