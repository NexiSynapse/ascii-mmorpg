use super::{Layer, Partition};

/// Everything needed to render one moment: the layer stack plus the
/// partitions that slice the frame (three.js analogy: `Scene`).
/// Layers are composited bottom-up by `z`; partitions pick which layers they
/// show, so one scene feeds the world view, sidebar, and overlays at once.
#[derive(Clone, Debug, Default)]
pub struct Scene {
    pub layers: Vec<Layer>,
    pub partitions: Vec<Partition>,
}

impl Scene {
    /// Layer index of `name`, for wiring partitions to layers.
    pub fn layer(&self, name: &str) -> Option<usize> {
        self.layers.iter().position(|l| l.name == name)
    }
}

/// World-space camera: cell offset subtracted inside camera partitions.
#[derive(Clone, Copy, Debug, Default)]
pub struct Camera {
    pub x: i32,
    pub y: i32,
}
