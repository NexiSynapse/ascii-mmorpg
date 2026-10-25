//! Camera-follow easing profile (video mimicry S3, 2026-10-25).
//!
//! The reference captures (Video-47489, phase-correlation measurements in
//! `video-tools/out`) scroll the world under a follow camera with a
//! measurable shape: an EASE-IN ramp of ~0.4s to constant cruise speed, a
//! cruise that tracks the walker exactly, and an EASE-OUT settle of ~0.5s
//! when the walker stops — no overshoot, no oscillation. A first-order
//! exponential glide toward the focus produces exactly that family; the
//! response rate sets the window (8/s ⇒ 90% of cruise in ~0.29s, 97% by
//! 0.45s; remaining lag decays 20x inside 0.5s). This module pins that
//! shape with tests so presentation code cannot silently regress it.
//!
//! The engine draws on a continuous canvas, so camera positions stay
//! fractional — the reference's frame-quantized scroll is a terminal
//! artifact, not a goal (sub-cell motion is a spec'd presentation win).

/// First-order follow camera over a 2D focus point (world XZ).
#[derive(Clone, Copy, Debug)]
pub struct FollowCam {
    pos: (f32, f32),
    seeded: bool,
    response: f32,
    snap: f32,
}

/// Default response (1/s): matches the measured ~0.4s ease-in window.
pub const DEFAULT_RESPONSE: f32 = 8.0;
/// Default snap distance: teleports (respawns, scene jumps) larger than
/// this are followed instantly instead of glided.
pub const DEFAULT_SNAP: f32 = 4.0;

impl Default for FollowCam {
    fn default() -> Self {
        Self::new()
    }
}

impl FollowCam {
    pub fn new() -> Self {
        Self {
            pos: (0.0, 0.0),
            seeded: false,
            response: DEFAULT_RESPONSE,
            snap: DEFAULT_SNAP,
        }
    }

    /// Seed the starting focus (skip the initial glide-in from the origin).
    pub fn at(mut self, x: f32, y: f32) -> Self {
        self.pos = (x, y);
        self.seeded = true;
        self
    }

    /// Set the response rate (1/s), clamped to 1..=30 — outside that range
    /// the ramp no longer resembles the measured reference window.
    pub fn with_response(mut self, response: f32) -> Self {
        self.response = if response.is_finite() {
            response.clamp(1.0, 30.0)
        } else {
            DEFAULT_RESPONSE
        };
        self
    }

    /// Set the teleport-snap distance, clamped to at least half a cell.
    pub fn with_snap(mut self, snap: f32) -> Self {
        self.snap = if snap.is_finite() {
            snap.max(0.5)
        } else {
            DEFAULT_SNAP
        };
        self
    }

    /// Current focus position.
    pub fn position(&self) -> (f32, f32) {
        self.pos
    }

    /// Advance one frame toward `target` by `dt` seconds. The first call
    /// and any jump beyond the snap distance snap directly; everything
    /// else glides exponentially (ease-in to cruise, ease-out settle,
    /// never overshooting).
    pub fn advance(&mut self, target: (f32, f32), dt: f32) -> (f32, f32) {
        if !self.seeded {
            self.pos = target;
            self.seeded = true;
            return target;
        }
        let dist = (target.0 - self.pos.0).hypot(target.1 - self.pos.1);
        if dist > self.snap {
            self.pos = target;
            return target;
        }
        let glide = 1.0 - (-dt.max(0.0) * self.response).exp();
        self.pos = (
            self.pos.0 + (target.0 - self.pos.0) * glide,
            self.pos.1 + (target.1 - self.pos.1) * glide,
        );
        self.pos
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 30.0;

    #[test]
    fn first_frame_and_teleports_snap() {
        let mut cam = FollowCam::new();
        assert_eq!(cam.advance((10.0, -4.0), DT), (10.0, -4.0));
        // Small drift glides; a jump beyond the snap distance teleports.
        let near = cam.advance((10.2, -4.1), DT);
        assert!((near.0 - 10.2).abs() < 0.2, "small drift must glide");
        assert_eq!(cam.advance((30.0, 9.0), DT), (30.0, 9.0));
        assert_eq!(cam.position(), (30.0, 9.0));
    }

    /// Reference ease-in window (Video-47489): a walker at constant speed
    /// drags the camera to ≥90% of cruise within ~0.4s, and the camera
    /// never runs FASTER than the walker (no overshoot in the ramp).
    #[test]
    fn ramp_reaches_cruise_inside_the_reference_window() {
        let mut cam = FollowCam::new().at(0.0, 0.0);
        let mut prev = cam.position();
        let mut reach_90: Option<f32> = None;
        let mut max_speed = 0.0f32;
        for frame in 0..240 {
            let t = frame as f32 * DT;
            let p = cam.advance((t, 0.0), DT); // target walks +x at 1 cell/s
            let speed = (p.0 - prev.0).hypot(p.1 - prev.1) / DT;
            prev = p;
            max_speed = max_speed.max(speed);
            if reach_90.is_none() && speed >= 0.90 {
                reach_90 = Some(t);
            }
        }
        assert!(
            reach_90.is_some_and(|t| t <= 0.4),
            "90% cruise by 0.4s, got {:?}",
            reach_90
        );
        assert!(
            max_speed <= 1.0 + 1e-4,
            "camera must never outrun the walker (max {max_speed})"
        );
    }

    /// Reference ease-out window: after the walker stops, the camera
    /// closes to within 0.05 cells by 0.5s and settles without passing
    /// the target.
    #[test]
    fn settles_inside_the_reference_window_after_stop() {
        let mut cam = FollowCam::new().at(0.0, 0.0);
        for frame in 0..90 {
            let t = frame as f32 * DT;
            cam.advance((t, 0.0), DT);
        }
        for frame in 0..30 {
            cam.advance((3.0, 0.0), DT); // walker stopped at x=3
            if frame == 14 {
                let lag = (3.0 - cam.position().0).abs();
                assert!(lag <= 0.05, "settle by 0.5s, lag {lag}");
            }
        }
        assert!(
            cam.position().0 <= 3.0 + 1e-4,
            "camera must not overshoot the stop target"
        );
    }
}
