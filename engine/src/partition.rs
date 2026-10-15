/// A named viewport rectangle composited from a subset of layers.
///
/// Partitions carve the frame into regions with different jobs: the world view
/// (camera-following), sidebar, inventory, log strip, modal overlay —
/// three.js analogy: scissor viewports sharing one renderer. A partition
/// draws only its `layers` (by index into [`crate::Scene::layers`]) clipped
/// to its rect. Layer cells live in shared world coordinates; the partition
/// shows the window starting at its view origin (`vx`, `vy`), or at the
/// camera when `camera` is set, so the world scrolls under a fixed window
/// while HUD partitions stay pinned.
#[derive(Clone, Debug)]
pub struct Partition {
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub layers: Vec<usize>,
    pub camera: bool,
    pub vx: i32,
    pub vy: i32,
}

impl Partition {
    pub fn new(
        name: impl Into<String>,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        layers: Vec<usize>,
        camera: bool,
    ) -> Self {
        Self {
            name: name.into(),
            x,
            y,
            w,
            h,
            layers,
            camera,
            vx: 0,
            vy: 0,
        }
    }

    /// Look at the world window starting at (`vx`, `vy`) instead of the origin.
    pub fn with_view(mut self, vx: i32, vy: i32) -> Self {
        self.vx = vx;
        self.vy = vy;
        self
    }

    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contains_clips_edges() {
        let p = Partition::new("side", 60, 0, 20, 24, vec![], false);
        assert!(p.contains(60, 0));
        assert!(p.contains(79, 23));
        assert!(!p.contains(80, 0));
        assert!(!p.contains(59, 23));
    }
}
