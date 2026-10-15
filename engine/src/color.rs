use super::Axis;

/// 8-bit RGB color. Copy-small on purpose: frames hold thousands of these.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// Linear interpolation toward `other` by `t` in 0..=1.
    pub fn lerp(self, other: Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        let mix = |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * t) as u8;
        Self::new(
            mix(self.r, other.r),
            mix(self.g, other.g),
            mix(self.b, other.b),
        )
    }

    /// Scale brightness by `f` (e.g. from a [`crate::Pulse`](crate::anim) track).
    pub fn scaled(self, f: f32) -> Self {
        let scale = |c: u8| (f32::from(c) * f).clamp(0.0, 255.0) as u8;
        Self::new(scale(self.r), scale(self.g), scale(self.b))
    }
}

/// A deterministic per-cell foreground color source.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ColorTrack {
    /// Keep one color at every time and position.
    Static(Rgb),
    /// Blend from `start` to `end` along the selected cell axis.
    Gradient {
        start: Rgb,
        end: Rgb,
        axis: Axis,
        span: f32,
    },
    /// Cycle hue over time, with an optional phase offset in radians.
    Rainbow {
        period: f32,
        phase: f32,
        saturation: f32,
        value: f32,
    },
}

impl ColorTrack {
    /// Resolve this track at time `t` and layer-local position `(x, y)`.
    /// Invalid floating-point inputs and degenerate spans/periods fall back to
    /// a stable color rather than producing NaNs or panicking.
    pub fn sample(self, t: f32, position: (f32, f32)) -> Rgb {
        match self {
            Self::Static(color) => color,
            Self::Gradient {
                start,
                end,
                axis,
                span,
            } => {
                if !span.is_finite() || span.abs() <= f32::EPSILON {
                    return start;
                }
                let coordinate = match axis {
                    Axis::X => position.0,
                    Axis::Y => position.1,
                };
                if !coordinate.is_finite() {
                    return start;
                }
                start.lerp(end, coordinate / span)
            }
            Self::Rainbow {
                period,
                phase,
                saturation,
                value,
            } => {
                if !period.is_finite() || period <= 0.0 {
                    return hsv_to_rgb(0.0, saturation, value);
                }
                let time = if t.is_finite() { t } else { 0.0 };
                let phase = if phase.is_finite() { phase } else { 0.0 };
                let hue = (time / period + phase / std::f32::consts::TAU).rem_euclid(1.0);
                hsv_to_rgb(hue, saturation, value)
            }
        }
    }
}

fn hsv_to_rgb(hue: f32, saturation: f32, value: f32) -> Rgb {
    let h = hue.rem_euclid(1.0) * 6.0;
    let s = if saturation.is_finite() {
        saturation.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let v = if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let chroma = v * s;
    let x = chroma * (1.0 - ((h % 2.0) - 1.0).abs());
    let m = v - chroma;
    let (r, g, b) = match h as u8 {
        0 => (chroma, x, 0.0),
        1 => (x, chroma, 0.0),
        2 => (0.0, chroma, x),
        3 => (0.0, x, chroma),
        4 => (x, 0.0, chroma),
        _ => (chroma, 0.0, x),
    };
    Rgb::new(to_channel(r + m), to_channel(g + m), to_channel(b + m))
}

fn to_channel(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lerp_midpoint_and_clamp() {
        let a = Rgb::new(0, 0, 0);
        let b = Rgb::new(100, 200, 50);
        assert_eq!(a.lerp(b, 0.5), Rgb::new(50, 100, 25));
        assert_eq!(a.lerp(b, 9.0), b);
    }

    #[test]
    fn scaled_clamps_channels() {
        assert_eq!(Rgb::new(200, 100, 50).scaled(2.0), Rgb::new(255, 200, 100));
        assert_eq!(Rgb::new(200, 100, 50).scaled(0.0), Rgb::new(0, 0, 0));
    }

    #[test]
    fn gradient_varies_across_cells() {
        let track = ColorTrack::Gradient {
            start: Rgb::new(0, 0, 0),
            end: Rgb::new(255, 100, 20),
            axis: Axis::X,
            span: 4.0,
        };
        assert_ne!(track.sample(0.0, (0.0, 0.0)), track.sample(0.0, (4.0, 0.0)));
        assert_eq!(track.sample(0.0, (0.0, 0.0)), Rgb::new(0, 0, 0));
        assert_eq!(track.sample(0.0, (4.0, 0.0)), Rgb::new(255, 100, 20));
    }

    #[test]
    fn rainbow_is_deterministic_and_changes_over_time() {
        let track = ColorTrack::Rainbow {
            period: 2.0,
            phase: 0.0,
            saturation: 1.0,
            value: 1.0,
        };
        assert_eq!(
            track.sample(0.25, (3.0, 2.0)),
            track.sample(0.25, (3.0, 2.0))
        );
        assert_ne!(track.sample(0.0, (3.0, 2.0)), track.sample(0.5, (3.0, 2.0)));
    }

    #[test]
    fn invalid_rainbow_period_is_safe() {
        let track = ColorTrack::Rainbow {
            period: 0.0,
            phase: f32::NAN,
            saturation: f32::INFINITY,
            value: f32::NEG_INFINITY,
        };
        assert_eq!(track.sample(f32::NAN, (f32::NAN, 0.0)), Rgb::new(0, 0, 0));
    }
}
