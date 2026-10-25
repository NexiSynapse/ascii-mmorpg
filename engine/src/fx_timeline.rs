//! Shared combat-effect timeline (review 2026-10-25 P1).
//!
//! Every combat effect follows the same recognizable phase model —
//! telegraph → impact → decay — with eased progress and one retention
//! window, instead of each effect re-deriving its own age math. Effects
//! describe WHEN the phases happen; the timeline answers WHICH phase a
//! presentation time is in and how far through it is. Presentation-only.

/// Phase schedule for one effect occurrence.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EffectTimeline {
    /// Presentation time the effect starts (the cast/hit instant).
    pub start: f32,
    /// Wind-up end / burst start (`start + telegraph_s`).
    pub telegraph_until: f32,
    /// Burst end (`telegraph_until + impact_s`).
    pub impact_at: f32,
    /// Full retention end (`impact_at + decay_s`); effects are dropped
    /// after this.
    pub end: f32,
}

/// Which phase a presentation time falls in. `progress` is 0..=1 through
/// the phase (raw linear — callers apply easing per effect family).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EffectPhase {
    Telegraph { progress: f32 },
    Impact { progress: f32 },
    Decay { progress: f32 },
    Done,
}

fn finite_or(value: f32, fallback: f32) -> f32 {
    if value.is_finite() {
        value
    } else {
        fallback
    }
}

impl EffectTimeline {
    /// Build a schedule from durations. Non-finite or negative segments
    /// collapse to zero so a malformed effect cannot stall the timeline.
    pub fn new(start: f32, telegraph_s: f32, impact_s: f32, decay_s: f32) -> Self {
        let start = finite_or(start, 0.0);
        let telegraph_s = finite_or(telegraph_s, 0.0).max(0.0);
        let impact_s = finite_or(impact_s, 0.0).max(0.0);
        let decay_s = finite_or(decay_s, 0.0).max(0.0);
        Self {
            start,
            telegraph_until: start + telegraph_s,
            impact_at: start + telegraph_s + impact_s,
            end: start + telegraph_s + impact_s + decay_s,
        }
    }

    /// Phase at presentation time `t` (negative/NaN times read as `start`).
    pub fn phase(&self, t: f32) -> EffectPhase {
        let t = if t.is_finite() { t } else { self.start };
        if t < self.telegraph_until {
            let span = (self.telegraph_until - self.start).max(f32::EPSILON);
            EffectPhase::Telegraph {
                progress: ((t - self.start) / span).clamp(0.0, 1.0),
            }
        } else if t < self.impact_at {
            let span = (self.impact_at - self.telegraph_until).max(f32::EPSILON);
            EffectPhase::Impact {
                progress: ((t - self.telegraph_until) / span).clamp(0.0, 1.0),
            }
        } else if t < self.end {
            let span = (self.end - self.impact_at).max(f32::EPSILON);
            EffectPhase::Decay {
                progress: ((t - self.impact_at) / span).clamp(0.0, 1.0),
            }
        } else {
            EffectPhase::Done
        }
    }

    /// Whether the effect should still be retained at `t`.
    pub fn is_live(&self, t: f32) -> bool {
        self.phase(t) != EffectPhase::Done
    }
}

/// Ease-out (quadratic): fast start, soft landing — the reference's
/// decay feel for rings and particles.
pub fn ease_out(p: f32) -> f32 {
    let p = p.clamp(0.0, 1.0);
    1.0 - (1.0 - p) * (1.0 - p)
}

/// Ease-in (quadratic): slow start, fast finish — rising embers.
pub fn ease_in(p: f32) -> f32 {
    let p = p.clamp(0.0, 1.0);
    p * p
}

#[cfg(test)]
mod tests {
    use super::*;

    fn timeline() -> EffectTimeline {
        // 0.5s telegraph, 0.45s impact, 0.3s decay, starting at t=2.0.
        EffectTimeline::new(2.0, 0.5, 0.45, 0.3)
    }

    #[test]
    fn phases_advance_through_the_schedule() {
        let tl = timeline();
        assert_eq!(tl.phase(1.5), EffectPhase::Telegraph { progress: 0.0 });
        assert_eq!(tl.phase(2.0), EffectPhase::Telegraph { progress: 0.0 });
        assert!(matches!(tl.phase(2.25), EffectPhase::Telegraph { progress }
            if (progress - 0.5).abs() < 1e-4));
        assert_eq!(tl.phase(2.5), EffectPhase::Impact { progress: 0.0 });
        assert!(matches!(tl.phase(2.725), EffectPhase::Impact { progress }
            if (progress - 0.5).abs() < 1e-4));
        assert_eq!(tl.phase(2.95), EffectPhase::Decay { progress: 0.0 });
        assert!(matches!(tl.phase(3.10), EffectPhase::Decay { progress }
            if (progress - 0.5).abs() < 1e-4));
        assert_eq!(tl.phase(3.25), EffectPhase::Done);
        assert_eq!(tl.phase(10.0), EffectPhase::Done);
    }

    #[test]
    fn retention_matches_the_phase_model() {
        let tl = timeline();
        assert!(tl.is_live(2.0));
        assert!(tl.is_live(3.24));
        assert!(!tl.is_live(3.26));
    }

    #[test]
    fn malformed_segments_collapse_instead_of_stalling() {
        let tl = EffectTimeline::new(f32::NAN, -1.0, f32::INFINITY, f32::NAN);
        // start=0, telegraph=0, impact=0 (infinite clamped away), decay=0:
        // everything is immediately Done — never a stuck phase.
        assert_eq!(tl.phase(0.0), EffectPhase::Done);
        assert_eq!(tl.phase(100.0), EffectPhase::Done);
        let zero = EffectTimeline::new(1.0, 0.0, 0.0, 0.0);
        assert_eq!(zero.phase(1.0), EffectPhase::Done);
        // A zero-length telegraph jumps straight to impact progress.
        let instant = EffectTimeline::new(1.0, 0.0, 1.0, 0.0);
        assert_eq!(instant.phase(1.5), EffectPhase::Impact { progress: 0.5 });
    }

    #[test]
    fn easings_are_bounded_and_monotonic() {
        assert_eq!(ease_out(0.0), 0.0);
        assert_eq!(ease_out(1.0), 1.0);
        assert_eq!(ease_in(0.0), 0.0);
        assert_eq!(ease_in(1.0), 1.0);
        assert_eq!(ease_out(2.0), 1.0);
        assert_eq!(ease_in(-1.0), 0.0);
        assert!(ease_out(0.5) > 0.5);
        assert!(ease_in(0.5) < 0.5);
    }
}
