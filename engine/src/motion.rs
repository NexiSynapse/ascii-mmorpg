//! Inertial movement model used by the interactive engine examples.
//!
//! It produces movement intents only. World position and collision remain
//! owned by the server simulation.

/// Shortest signed angular distance from `from` to `to` in radians.
pub fn shortest_angle_delta(from: f32, to: f32) -> f32 {
    let tau = std::f32::consts::TAU;
    (to - from + std::f32::consts::PI).rem_euclid(tau) - std::f32::consts::PI
}

/// Frame-rate independent shortest-path angle smoothing.
pub fn smooth_angle(current: f32, target: f32, rate: f32, dt: f32) -> f32 {
    let blend = 1.0 - (-rate.max(0.0) * dt.max(0.0)).exp();
    current + shortest_angle_delta(current, target) * blend
}

/// Ground-plane velocity with acceleration, diagonal normalization, and drag.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct KinematicMotion {
    velocity: (f32, f32),
    carry: (f32, f32),
}

impl KinematicMotion {
    /// Approach `input` (each axis in -1..=1) with acceleration and friction.
    /// Returns a discrete cell-step direction when enough distance accumulates.
    /// `dt` is capped so a stalled window cannot generate a large movement.
    pub fn update(&mut self, input: (f32, f32), dt: f32) -> Option<(i8, i8)> {
        const MAX_SPEED: f32 = 4.0;
        const ACCEL: f32 = 18.0;
        const DRAG: f32 = 22.0;

        let dt = dt.clamp(0.0, 0.05);
        let (mut ix, mut iy) = (input.0.clamp(-1.0, 1.0), input.1.clamp(-1.0, 1.0));
        let input_len = ix.hypot(iy);
        if input_len > 1.0 {
            ix /= input_len;
            iy /= input_len;
        }
        let desired = (ix * MAX_SPEED, iy * MAX_SPEED);
        let delta = (desired.0 - self.velocity.0, desired.1 - self.velocity.1);
        let delta_len = delta.0.hypot(delta.1);
        let max_delta = if input_len > 0.0 { ACCEL } else { DRAG } * dt;
        let scale = if delta_len > max_delta && delta_len > 0.0 {
            max_delta / delta_len
        } else {
            1.0
        };
        self.velocity.0 += delta.0 * scale;
        self.velocity.1 += delta.1 * scale;

        self.carry.0 += self.velocity.0 * dt;
        self.carry.1 += self.velocity.1 * dt;
        if self.carry.0.abs().max(self.carry.1.abs()) < 1.0 {
            return None;
        }
        let step = (
            if self.carry.0.abs() >= 1.0 {
                self.carry.0.signum() as i8
            } else {
                0
            },
            if self.carry.1.abs() >= 1.0 {
                self.carry.1.signum() as i8
            } else {
                0
            },
        );
        self.carry.0 -= f32::from(step.0);
        self.carry.1 -= f32::from(step.1);
        Some(step)
    }

    pub fn velocity(&self) -> (f32, f32) {
        self.velocity
    }

    /// Fractional distance already integrated but not yet represented by a
    /// server-owned cell position.
    pub fn remainder(&self) -> (f32, f32) {
        self.carry
    }
}

#[cfg(test)]
mod tests {
    use super::{shortest_angle_delta, smooth_angle, KinematicMotion};

    #[test]
    fn starts_slowly_and_accelerates_to_a_bounded_speed() {
        let mut motion = KinematicMotion::default();
        assert_eq!(motion.update((1.0, 0.0), 0.05), None);
        assert!((motion.velocity().0 - 0.9).abs() < 0.001);
        for _ in 0..100 {
            motion.update((1.0, 0.0), 0.05);
        }
        assert!((motion.velocity().0 - 4.0).abs() < 0.001);
        assert!(motion.velocity().1.abs() < 0.001);
    }

    #[test]
    fn diagonal_speed_is_normalized_and_release_applies_drag() {
        let mut motion = KinematicMotion::default();
        for _ in 0..100 {
            motion.update((1.0, 1.0), 0.05);
        }
        let v = motion.velocity();
        assert!((v.0.hypot(v.1) - 4.0).abs() < 0.001);
        assert!((v.0 - v.1).abs() < 0.001);
        for _ in 0..20 {
            motion.update((0.0, 0.0), 0.05);
        }
        assert!(motion.velocity().0 < v.0);
        assert!(motion.velocity().1 < v.1);
    }

    #[test]
    fn a_long_frame_cannot_generate_an_unbounded_step() {
        let mut motion = KinematicMotion::default();
        assert_eq!(motion.update((1.0, 0.0), 5.0), None);
        assert!(motion.velocity().0 <= 0.901);
    }

    #[test]
    fn angle_smoothing_takes_the_short_way_across_pi() {
        let current = std::f32::consts::PI - 0.08;
        let target = -std::f32::consts::PI + 0.08;
        let delta = shortest_angle_delta(current, target);
        assert!(
            delta > 0.0 && delta < 0.2,
            "unexpected wrapped delta {delta}"
        );
        let next = smooth_angle(current, target, 20.0, 0.05);
        assert!(next > current, "wrapped turn should advance through +pi");
    }

    #[test]
    fn angle_smoothing_is_stable_for_zero_delta_and_bad_time() {
        let angle = 0.7;
        assert_eq!(smooth_angle(angle, angle, 20.0, 0.5), angle);
        assert_eq!(smooth_angle(angle, 1.2, 20.0, -1.0), angle);
    }
}
