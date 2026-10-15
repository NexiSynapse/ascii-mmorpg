use std::f32::consts::TAU;

/// Which grid axis a [`Wave`] displaces along.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    X,
    Y,
}

/// Easing curve shape for [`Track::Ease`]: how a 0..=1 loop position maps to
/// a 0..=1 blend factor. All variants start at 0 and end at 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EaseKind {
    /// Steady ramp: `u`.
    Linear,
    /// Slow start, fast end: `u * u`.
    In,
    /// Fast start, slow end: `1 - (1 - u) * (1 - u)`.
    Out,
    /// Slow ends, fast middle (smoothstep): `u * u * (3 - 2 * u)`.
    InOut,
}

impl EaseKind {
    pub fn apply(self, u: f32) -> f32 {
        let u = u.clamp(0.0, 1.0);
        match self {
            EaseKind::Linear => u,
            EaseKind::In => u * u,
            EaseKind::Out => 1.0 - (1.0 - u) * (1.0 - u),
            EaseKind::InOut => u * u * (3.0 - 2.0 * u),
        }
    }
}

/// One animation channel on a cell — the "keyframe track" of this engine
/// (three.js analogy: `KeyframeTrack`; [`Anim`] below is the `AnimationClip`,
/// [`crate::compose`] is the mixer that evaluates everything at time `t`).
#[derive(Clone, Debug, PartialEq)]
pub enum Track {
    /// Traveling sine wave: `amp * sin(2PI * (pos / wavelength - t / period) + phase)`.
    /// Crest moves toward +axis as `t` grows (left-to-right for `Axis::Y` on a row).
    /// `brighten` adds a brightness shimmer in sync with the displacement.
    Wave {
        axis: Axis,
        /// Peak displacement in cells.
        amp: f32,
        /// Crest-to-crest distance in cells.
        wavelength: f32,
        /// Seconds per full traveling cycle.
        period: f32,
        /// Phase offset in radians; stagger by position for traveling motion.
        phase: f32,
        /// Brightness shimmer amplitude (0 = none, 0.3 = gentle glow).
        brighten: f32,
    },
    /// Glyph flip-book: `frames[(t * fps) % len]`. For water sparkle, torch
    /// flicker shapes, walk cycles.
    Cycle { frames: Vec<char>, fps: f32 },
    /// Smooth brightness breathing between `low` and `high` (cosine loop).
    /// For torch glow, magic pulse, selection highlight.
    Pulse {
        low: f32,
        high: f32,
        period: f32,
        phase: f32,
    },
    /// Eased one-way ramp from `low` to `high` over `period` seconds, looping
    /// (wraps to `low` at the loop seam). For HP-bar drains, fade-ins,
    /// menu slides: pick [`EaseKind`] for the feel. `phase` is in radians
    /// (added as `phase / TAU` loops, matching [`Track::Pulse`]).
    Ease {
        low: f32,
        high: f32,
        period: f32,
        phase: f32,
        kind: EaseKind,
    },
    /// Stepped positional jitter: every `period` seconds each cell jumps to a
    /// deterministic pseudo-random offset in `-amp..=amp` (integer hash of the
    /// cell position, time step, and `seed` — stable across platforms and
    /// runs). For glitch shakes, drunk wobble, impact rattle. Brightness is
    /// untouched; combine with `Pulse`/`Ease` for flashing. `period <= 0` or
    /// `amp == 0` holds still.
    Noise { amp: f32, period: f32, seed: u32 },
}

/// Ordered channel list evaluated top-to-bottom; later tracks win conflicts
/// (e.g. two `Cycle`s: the last one picks the glyph).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Anim {
    pub tracks: Vec<Track>,
}

impl Anim {
    pub fn still() -> Self {
        Self::default()
    }

    pub fn with(mut self, track: Track) -> Self {
        self.tracks.push(track);
        self
    }
}

/// Mixer output for one cell at time `t`: optional glyph override, integer
/// grid offset, brightness multiplier.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    pub glyph: Option<char>,
    pub dx: i32,
    pub dy: i32,
    pub bright: f32,
}

/// Evaluate every track of `anim` at time `t` seconds. `x`/`y` is the cell's
/// layer-local position: a [`Track::Wave`] reads the coordinate along its own
/// axis, so one shared track ripples a whole field, while per-cell `phase`
/// staggers neighbors into traveling motion.
pub fn sample(anim: &Anim, x: f32, y: f32, t: f32) -> Sample {
    let mut out = Sample {
        glyph: None,
        dx: 0,
        dy: 0,
        bright: 1.0,
    };
    for track in &anim.tracks {
        match track {
            Track::Wave {
                axis,
                amp,
                wavelength,
                period,
                phase,
                brighten,
            } => {
                if *period > 0.0 && *wavelength != 0.0 {
                    let pos = match axis {
                        Axis::X => x,
                        Axis::Y => y,
                    };
                    let sway = TAU * (pos / wavelength - t / period) + phase;
                    let shift = (amp * sway.sin()).round() as i32;
                    match axis {
                        Axis::X => out.dx += shift,
                        Axis::Y => out.dy += shift,
                    }
                    out.bright *= 1.0 + brighten * sway.sin();
                }
            }
            Track::Cycle { frames, fps } => {
                if !frames.is_empty() && *fps > 0.0 {
                    let index = (t * fps).max(0.0) as usize % frames.len();
                    out.glyph = Some(frames[index]);
                }
            }
            Track::Pulse {
                low,
                high,
                period,
                phase,
            } => {
                if *period > 0.0 {
                    let wave = 0.5 - 0.5 * (TAU * t / period + phase).cos();
                    out.bright *= low + (high - low) * wave;
                }
            }
            Track::Ease {
                low,
                high,
                period,
                phase,
                kind,
            } => {
                if *period > 0.0 {
                    let u = (t / period + phase / TAU) % 1.0;
                    let u = if u < 0.0 { u + 1.0 } else { u };
                    out.bright *= low + (high - low) * kind.apply(u);
                }
            }
            Track::Noise { amp, period, seed } => {
                if *period > 0.0 && *amp != 0.0 {
                    let step = (t / period).floor() as i32;
                    let xi = x.floor() as i32;
                    let yi = y.floor() as i32;
                    let nx = hash01(xi, yi, step, *seed);
                    let ny = hash01(yi, xi.wrapping_add(step), step, seed.wrapping_add(1));
                    out.dx += (amp * (nx * 2.0 - 1.0)).round() as i32;
                    out.dy += (amp * (ny * 2.0 - 1.0)).round() as i32;
                }
            }
        }
    }
    out
}

/// Deterministic 0..1 hash of grid coords, a time step, and a seed.
/// Integer-only (splitmix-style) so every platform and run agrees.
fn hash01(x: i32, y: i32, step: i32, seed: u32) -> f32 {
    let mut h = seed
        .wrapping_add(x as u32)
        .wrapping_mul(374761393)
        .wrapping_add((y as u32).wrapping_mul(668265263))
        .wrapping_add((step as u32).wrapping_mul(2246822519));
    h = (h ^ (h >> 13)).wrapping_mul(1274126177);
    h ^= h >> 16;
    (h >> 8) as f32 / 16777216.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wave_crest_travels_with_time() {
        let anim = Anim::still().with(Track::Wave {
            axis: Axis::Y,
            amp: 1.0,
            wavelength: 8.0,
            period: 2.0,
            phase: 0.0,
            brighten: 0.0,
        });
        // Same cell, quarter period apart: displacement flips sign.
        let a = sample(&anim, 5.0, 0.0, 0.0);
        let b = sample(&anim, 5.0, 0.0, 0.5);
        assert_eq!((a.dy, b.dy), (0, -1));
        // Same time, one wavelength apart along the axis: identical displacement.
        let c = sample(&anim, 5.0, 8.0, 0.5);
        assert_eq!(b.dy, c.dy);
    }

    #[test]
    fn wave_degenerate_params_hold_still() {
        let anim = Anim::still()
            .with(Track::Wave {
                axis: Axis::Y,
                amp: 2.0,
                wavelength: 0.0,
                period: 1.0,
                phase: 0.0,
                brighten: 0.5,
            })
            .with(Track::Wave {
                axis: Axis::X,
                amp: 2.0,
                wavelength: 4.0,
                period: 0.0,
                phase: 0.0,
                brighten: 0.0,
            });
        assert_eq!(
            sample(&anim, 3.0, 7.0, 1.0),
            Sample {
                glyph: None,
                dx: 0,
                dy: 0,
                bright: 1.0,
            }
        );
    }

    #[test]
    fn cycle_loops_frames_and_last_track_wins() {
        let anim = Anim::still()
            .with(Track::Cycle {
                frames: vec!['a', 'b'],
                fps: 2.0,
            })
            .with(Track::Cycle {
                frames: vec!['x', 'y', 'z'],
                fps: 2.0,
            });
        assert_eq!(sample(&anim, 0.0, 0.0, 0.0).glyph, Some('x'));
        assert_eq!(sample(&anim, 0.0, 0.0, 0.5).glyph, Some('y'));
        assert_eq!(sample(&anim, 0.0, 0.0, 1.5).glyph, Some('x'));
        assert_eq!(sample(&anim, 0.0, 0.0, 2.5).glyph, Some('z'));
    }

    #[test]
    fn pulse_breathes_between_bounds() {
        let anim = Anim::still().with(Track::Pulse {
            low: 0.6,
            high: 1.4,
            period: 2.0,
            phase: 0.0,
        });
        let mid = sample(&anim, 0.0, 0.0, 0.0).bright;
        let peak = sample(&anim, 0.0, 0.0, 1.0).bright;
        assert!((mid - 0.6).abs() < 1e-5, "t=0 starts at low");
        assert!((peak - 1.4).abs() < 1e-5, "half period hits high");
    }

    #[test]
    fn ease_ramps_and_loops() {
        let anim = Anim::still().with(Track::Ease {
            low: 0.0,
            high: 2.0,
            period: 4.0,
            phase: 0.0,
            kind: EaseKind::Linear,
        });
        assert!((sample(&anim, 0.0, 0.0, 0.0).bright - 0.0).abs() < 1e-5);
        assert!((sample(&anim, 0.0, 0.0, 2.0).bright - 1.0).abs() < 1e-5);
        assert!((sample(&anim, 0.0, 0.0, 4.0).bright - 0.0).abs() < 1e-5);
        // Curve shapes: In starts slow, Out starts fast.
        let ease_in = Anim::still().with(Track::Ease {
            low: 0.0,
            high: 1.0,
            period: 1.0,
            phase: 0.0,
            kind: EaseKind::In,
        });
        let ease_out = Anim::still().with(Track::Ease {
            low: 0.0,
            high: 1.0,
            period: 1.0,
            phase: 0.0,
            kind: EaseKind::Out,
        });
        let quarter_in = sample(&ease_in, 0.0, 0.0, 0.25).bright;
        let quarter_out = sample(&ease_out, 0.0, 0.0, 0.25).bright;
        assert!(quarter_in < 0.25, "ease-in lags linear early");
        assert!(quarter_out > 0.25, "ease-out leads linear early");
        // Degenerate period holds still.
        let dead = Anim::still().with(Track::Ease {
            low: 0.0,
            high: 9.0,
            period: 0.0,
            phase: 0.0,
            kind: EaseKind::Linear,
        });
        assert!((sample(&dead, 0.0, 0.0, 5.0).bright - 1.0).abs() < 1e-5);
    }

    #[test]
    fn noise_is_deterministic_bounded_and_stepped() {
        let anim = Anim::still().with(Track::Noise {
            amp: 2.0,
            period: 0.25,
            seed: 7,
        });
        let a = sample(&anim, 3.0, 5.0, 1.0);
        let b = sample(&anim, 3.0, 5.0, 1.0);
        assert_eq!((a.dx, a.dy), (b.dx, b.dy), "same inputs agree");
        assert!(a.dx.abs() <= 2 && a.dy.abs() <= 2, "offsets stay in amp");
        // Same step, different cells usually differ; different steps differ
        // for at least one of a spread of cells (hash coverage, not luck:
        // 16 cells x 2 steps would all have to collide at p ~ 1/25).
        let same_step_same: bool =
            (0..4).all(|x| sample(&anim, x as f32, 0.0, 1.0) == sample(&anim, 0.0, 0.0, 1.0));
        assert!(!same_step_same, "noise varies across cells");
        let changed: bool =
            (0..4).any(|x| sample(&anim, x as f32, 0.0, 1.0) != sample(&anim, x as f32, 0.0, 1.3));
        assert!(changed, "noise steps forward with time");
        // Degenerate params hold still.
        let dead = Anim::still()
            .with(Track::Noise {
                amp: 0.0,
                period: 0.25,
                seed: 7,
            })
            .with(Track::Noise {
                amp: 3.0,
                period: 0.0,
                seed: 7,
            });
        assert_eq!(
            sample(&dead, 3.0, 5.0, 1.0),
            Sample {
                glyph: None,
                dx: 0,
                dy: 0,
                bright: 1.0,
            }
        );
    }
}
