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
    /// Cell-pitch multiplier applied at CONTINUOUS emission only (video
    /// mimicry S1): 1.0 keeps the frame grid; larger values render this
    /// partition's composed cells bigger, smaller values pack more, smaller
    /// glyphs in. The rect is authored in unscaled cells, so a modal that
    /// covers the screen at 1.3× pitch is authored on a `w/1.3 × h/1.3`
    /// rect and scaled back up at emission — the reference game's inventory
    /// runs ~8×12px glyphs against the world's ~6.5×9.5. Camera-following
    /// world partitions should stay at 1.0 so world ink keeps its lattice.
    pub pitch_scale: f32,
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
            pitch_scale: 1.0,
        }
    }

    /// Look at the world window starting at (`vx`, `vy`) instead of the origin.
    pub fn with_view(mut self, vx: i32, vy: i32) -> Self {
        self.vx = vx;
        self.vy = vy;
        self
    }

    /// Render this partition's composed cells at a multiple of the frame
    /// pitch, clamped to `0.25..=4.0` so a bad authoring value cannot
    /// collapse or explode the grid. Compose, the CPU raster, and the grid
    /// fallback all ignore this — it only changes continuous emission.
    pub fn with_pitch(mut self, pitch: f32) -> Self {
        self.pitch_scale = if pitch.is_finite() {
            pitch.clamp(0.25, 4.0)
        } else {
            1.0
        };
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

    #[test]
    fn pitch_defaults_to_one_and_clamps() {
        assert_eq!(
            Partition::new("hud", 0, 0, 10, 1, vec![], false).pitch_scale,
            1.0
        );
        assert_eq!(
            Partition::new("modal", 0, 0, 10, 1, vec![], false)
                .with_pitch(1.3)
                .pitch_scale,
            1.3
        );
        assert_eq!(
            Partition::new("rain", 0, 0, 10, 1, vec![], false)
                .with_pitch(9.0)
                .pitch_scale,
            4.0
        );
        assert_eq!(
            Partition::new("tiny", 0, 0, 10, 1, vec![], false)
                .with_pitch(0.05)
                .pitch_scale,
            0.25
        );
        assert_eq!(
            Partition::new("nan", 0, 0, 10, 1, vec![], false)
                .with_pitch(f32::NAN)
                .pitch_scale,
            1.0
        );
    }
}
