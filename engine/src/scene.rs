use super::{overlay::TextRun, Layer, Partition, Rgb};

/// Everything needed to render one moment: the layer stack plus the
/// partitions that slice the frame (three.js analogy: `Scene`).
/// Layers are composited bottom-up by `z`; partitions pick which layers they
/// show, so one scene feeds the world view, sidebar, and overlays at once.
#[derive(Clone, Debug, Default)]
pub struct Scene {
    pub layers: Vec<Layer>,
    pub partitions: Vec<Partition>,
    /// Global tint wash, applied after glow (reference amber washes during
    /// bursts/boss phases). `None` (default) leaves the frame untouched.
    pub wash: Option<Wash>,
    /// Crisp overlay text (boss/target readouts) drawn big over the frame.
    /// Headless composition ignores it; the presenter tessellates it.
    pub overlay_text: Vec<TextRun>,
}

/// Scene-wide background tint: every opaque background gains
/// `tint * strength` (clamped). Foregrounds are untouched, so glyphs keep
/// their hue while the air between them warms — the reference wash look.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Wash {
    pub tint: Rgb,
    /// 0.0 = no-op, 1.0 = full tint added. Callers clamp; compose clamps too.
    pub strength: f32,
}

impl Scene {
    /// Layer index of `name`, for wiring partitions to layers.
    pub fn layer(&self, name: &str) -> Option<usize> {
        self.layers.iter().position(|l| l.name == name)
    }
}

/// World-space camera: integer cell offset subtracted inside camera
/// partitions, plus a sub-cell glide fraction (`ox`/`oy` in cells) applied
/// by the presenter. Integer placement keeps composition exact; the fraction
/// makes follow motion continuous instead of stepping cell-to-cell.
#[derive(Clone, Copy, Debug, Default)]
pub struct Camera {
    pub x: i32,
    pub y: i32,
    pub ox: f32,
    pub oy: f32,
}
