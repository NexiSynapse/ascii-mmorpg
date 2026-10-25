//! Shared ASCII torch flame (review 2026-10-25 P1).
//!
//! ONE flame builder for the in-world play torches and the standalone
//! showcase, so the accepted read — plain ASCII punctuation rows, tapered
//! teardrop, vivid white→yellow→orange→burnt ramp that breathes over
//! time, hottest base rows feeding the bloom field — cannot drift between
//! the two call sites. Presentation-only.

use crate::continuous::{
    ContinuousScene, Emission, Material as ContinuousMaterial, Point2, Primitive, PrimitiveKind,
    Transform2,
};
use crate::Rgb;

/// Per-torch flame pulse state (from the caller's deterministic pulse
/// function); `sway` is the horizontal lean factor.
#[derive(Clone, Copy, Debug)]
pub struct FlamePulse {
    pub light_gain: f32,
    pub flame_scale: f32,
    pub radius_scale: f32,
    pub sway: f32,
}

fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let ch = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t) as u8;
    Rgb::new(ch(a.r, b.r), ch(a.g, b.g), ch(a.b, b.b))
}

/// Animated smooth colour ramp up an ASCII flame row stack: VIVID fire
/// colours — near-white base → yellow → orange → burnt tip — with the hot
/// zone breathing (wobble) so the ramp changes over time. `u` is 0 at the
/// base and 1 at the tip.
pub fn flame_row_color(u: f32, wobble: f32, flicker: f32) -> Rgb {
    let u = (u + 0.18 * wobble).clamp(0.0, 1.0);
    let base = if u < 0.34 {
        mix(Rgb::new(255, 250, 228), Rgb::new(255, 212, 84), u / 0.34)
    } else if u < 0.70 {
        mix(
            Rgb::new(255, 212, 84),
            Rgb::new(255, 148, 36),
            (u - 0.34) / 0.36,
        )
    } else {
        mix(
            Rgb::new(255, 148, 36),
            Rgb::new(208, 96, 28),
            (u - 0.70) / 0.30,
        )
    };
    // Flicker brightness rides on top of the ramp (small signed mix).
    mix(base, Rgb::new(255, 255, 240), (flicker - 0.85) * 0.3)
}

/// Row stack: base (widest, hottest) to tip.
const FLAME_ROWS: [(&str, f32); 4] = [(".:*:.", 0.0), (":*:", 0.36), ("^:", 0.70), ("'", 1.0)];
/// LOCAL per-glyph glow (small radius) for the three lower rows: the flame
/// body keeps its halo without acting like a row of independent lamps.
const LOCAL_EMISSION: f32 = 0.10;
const LOCAL_RADIUS: f32 = 1.4;
/// The ONE torch light: carried by the base row's centre glyph (the flame
/// anchor) so the pool reads as a single coherent warm source with a wide
/// radius — not five overlapping point lights (review 2026-10-25).
const TORCH_LIGHT_STRENGTH: f32 = 1.05;
const TORCH_LIGHT_RADIUS: f32 = 5.2;

/// Compose the layered ASCII flame at `anchor` (the projected stick-top
/// position in continuous grid space) into `scene`. `phase` seeds the
/// per-flame wobble offset (e.g. the torch index). Deterministic in its
/// inputs. Emission layout: one dedicated torch light at the anchor
/// (coherent pool) + small local glows on the hot glyphs (flame halo).
pub fn push_ascii_flame(
    scene: &mut ContinuousScene,
    anchor: Point2,
    pulse: FlamePulse,
    phase: f32,
    t: f32,
) {
    for (row_index, (row, u)) in FLAME_ROWS.iter().enumerate() {
        let sway_row = pulse.sway * (1.5 + u * 4.0);
        let row_y = anchor.y - row_index as f32 * 0.95 + pulse.light_gain * 0.10;
        let wobble = (t * 5.0 + row_index as f32 * 1.3 + phase * 2.1).sin();
        let row_color = flame_row_color(*u, wobble, pulse.light_gain);
        let count = row.chars().count();
        for (i, glyph) in row.chars().enumerate() {
            let x = anchor.x + (i as f32 - (count as f32 - 1.0) / 2.0) * 0.55 + sway_row;
            let hot = glyph == '*' || (glyph == ':' && count <= 3);
            // The centre glyph of the base row IS the torch light source.
            let torch_light = row_index == 0 && glyph == '*' && i == count / 2;
            // Side glyphs dim only slightly — a heavy dim washed every row
            // toward brown and collapsed the ramp.
            let color = if hot {
                row_color
            } else {
                mix(row_color, Rgb::new(172, 112, 42), 0.15)
            };
            let mut material = ContinuousMaterial::new(color)
                .with_opacity(1.0 - *u * 0.25 - if hot { 0.0 } else { 0.05 });
            if torch_light {
                material = material.with_emission(Emission::new(
                    mix(row_color, Rgb::new(255, 205, 120), 0.45),
                    TORCH_LIGHT_STRENGTH + 0.25 * pulse.light_gain,
                    TORCH_LIGHT_RADIUS * pulse.radius_scale,
                ));
            } else if row_index < 3 && hot {
                material = material.with_emission(Emission::new(
                    mix(row_color, Rgb::new(255, 255, 240), 0.15),
                    LOCAL_EMISSION + 0.04 * pulse.light_gain,
                    LOCAL_RADIUS * pulse.radius_scale,
                ));
            }
            scene.push(Primitive::new(
                PrimitiveKind::glyph(glyph),
                Transform2::new(Point2::new(x, row_y), row_index as f32 * 0.05).with_scale(
                    Point2::new(0.95 * pulse.flame_scale, 0.95 * pulse.flame_scale),
                ),
                material,
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pulse() -> FlamePulse {
        FlamePulse {
            light_gain: 0.9,
            flame_scale: 1.0,
            radius_scale: 1.0,
            sway: 0.02,
        }
    }

    #[test]
    fn flame_is_ascii_tapered_and_centered_on_the_anchor() {
        let mut scene = ContinuousScene::new();
        push_ascii_flame(&mut scene, Point2::new(10.0, 20.0), pulse(), 0.0, 1.0);
        let glyphs: Vec<_> = scene
            .primitives()
            .iter()
            .filter_map(|p| match p.kind {
                PrimitiveKind::Glyph { glyph } => Some((glyph, p.transform.position)),
                _ => None,
            })
            .collect();
        assert!(!glyphs.is_empty());
        assert!(
            glyphs.iter().all(|(g, _)| g.is_ascii_punctuation()),
            "flame rows must be plain ASCII punctuation"
        );
        // Taper: the stack spans ≥2 rows above the anchor and the base row
        // is wider than the tip row.
        let topmost = glyphs
            .iter()
            .map(|(_, p)| 20.0 - p.y)
            .fold(0.0_f32, f32::max);
        assert!(topmost >= 2.0, "flame must be a real stack (top {topmost})");
        let base_width = glyphs.iter().filter(|(_, p)| p.y > 19.9).count();
        let tip_rows = glyphs.iter().filter(|(_, p)| p.y < 18.2).count();
        assert!(base_width > tip_rows, "flame must taper to the tip");
        // The base row is centred on the anchor x (hot glyph within 0.6).
        assert!(glyphs
            .iter()
            .any(|(g, p)| *g == '*' && (p.x - 10.0).abs() <= 0.6 && p.y > 19.9));
    }

    /// Emission layout (review 2026-10-25): exactly ONE wide torch light
    /// (the coherent pool) plus small local glows — never a row of
    /// same-strength lamps.
    #[test]
    fn emission_is_one_torch_light_plus_local_glows() {
        let mut scene = ContinuousScene::new();
        push_ascii_flame(&mut scene, Point2::new(10.0, 20.0), pulse(), 0.0, 1.0);
        let emissions: Vec<_> = scene
            .primitives()
            .iter()
            .filter_map(|p| p.material.emission)
            .collect();
        let wide: Vec<_> = emissions.iter().filter(|e| e.radius >= 4.0).collect();
        assert_eq!(wide.len(), 1, "exactly one coherent torch light expected");
        assert!(
            wide[0].strength >= 1.0,
            "torch light must dominate the pool"
        );
        let locals: Vec<_> = emissions.iter().filter(|e| e.radius < 4.0).collect();
        assert!(
            locals.len() >= 3,
            "the flame body keeps its local halo glows"
        );
        assert!(locals.iter().all(|e| e.strength <= 0.2));
    }

    #[test]
    fn flame_ramp_is_vivid_dynamic_and_emissive_at_the_base() {
        let early = {
            let mut s = ContinuousScene::new();
            push_ascii_flame(&mut s, Point2::new(0.0, 0.0), pulse(), 0.0, 0.2);
            s
        };
        let late = {
            let mut s = ContinuousScene::new();
            push_ascii_flame(&mut s, Point2::new(0.0, 0.0), pulse(), 0.0, 1.5);
            s
        };
        let color_at = |scene: &ContinuousScene, y_min: f32, y_max: f32| {
            scene
                .primitives()
                .iter()
                .find(|p| {
                    matches!(p.kind, PrimitiveKind::Glyph { .. })
                        && p.transform.position.y >= y_min
                        && p.transform.position.y <= y_max
                })
                .map(|p| p.material.foreground)
        };
        let base = color_at(&early, -0.4, 0.4).expect("base row");
        let tip = color_at(&early, -3.2, -2.4).expect("tip row");
        let spread = (i16::from(base.r) - i16::from(tip.r))
            .abs()
            .max((i16::from(base.g) - i16::from(tip.g)).abs())
            .max((i16::from(base.b) - i16::from(tip.b)).abs());
        assert!(spread >= 80, "ramp spread {spread} is not vivid");
        let base_late = color_at(&late, -0.4, 0.4).expect("late base row");
        assert_ne!(base, base_late, "ramp must animate over time");
        let emissive = early
            .primitives()
            .iter()
            .filter(|p| p.material.emission.is_some())
            .count();
        assert!(emissive >= 3, "hot rows must feed the bloom field");
    }
}
