//! Combat burst vocabulary (video mimicry S4, 2026-10-26).
//!
//! The reference captures render spell/impact effects as PARTICLE
//! vocabulary, not solid geometry: concentric golden rings of `*`/`o`/`·`
//! expanding with ease-out, spiral arms of dots winding outward, a thin
//! ground shockwave arc that flashes out in the first half-second, and
//! ember speckle that lingers after the heavy phase — roughly 1.4s of
//! heavy churn then ~1s of ease-out decay (video-tools/out findings).
//! This module is the ONE burst builder so that read cannot drift.
//! Presentation-only: the anchor arrives already projected; authoritative
//! combat state stays in the sim.
//!
//! Phase model: `push_burst` takes `impact_age` = seconds since the
//! telegraph ended (the caller's `fx_timeline` telegraph still runs the
//! warning ring). Heavy phase [`HEAVY_S`] expands rings/spiral/shockwave;
//! decay phase [`DECAY_S`] fades everything while embers linger. Ring
//! glyphs carry a small warm emission so the bloom receivers tint nearby
//! terrain — the reference's "effect lights the ground" falloff.

use crate::continuous::{
    ContinuousScene, Emission, Material as ContinuousMaterial, Point2, Primitive, PrimitiveKind,
    Transform2,
};
use crate::fx_timeline::ease_out;
use crate::Rgb;

/// Heavy phase: rings expand, spiral winds, shockwave flashes out.
pub const HEAVY_S: f32 = 1.4;
/// Decay phase: ease-out fade; embers linger longest.
pub const DECAY_S: f32 = 1.0;
/// Hard per-burst primitive budget (shockwave 10 + rings 30 + spiral 14
/// + embers 8 = 62 peak; the cap also bounds pathological scales).
pub const BURST_BUDGET: usize = 64;

/// Total lifetime of the impact vocabulary (excludes the telegraph).
pub fn alive(impact_age: f32) -> bool {
    (0.0..HEAVY_S + DECAY_S).contains(&impact_age)
}

/// Scene palette for one burst, derived by the caller from the effect's
/// colour (hot = whitened core tone, ring = effect colour, ember = warm
/// spark tone).
#[derive(Clone, Copy, Debug)]
pub struct BurstPalette {
    pub hot: Rgb,
    pub ring: Rgb,
    pub ember: Rgb,
}

fn hash01(seed: u32, salt: u32) -> f32 {
    let mut h = seed.wrapping_mul(374_761_393) ^ salt.wrapping_mul(668_265_263);
    h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
    ((h ^ (h >> 16)) & 0x00ff_ffff) as f32 / 16_777_215.0
}

fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let ch = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t) as u8;
    Rgb::new(ch(a.r, b.r), ch(a.g, b.g), ch(a.b, b.b))
}

/// Ground-ellipse squash: a world circle projects to a screen ellipse
/// under the oblique camera; every ground ring uses this y squash.
const GROUND_SQUASH: f32 = 0.5;

/// Compose the burst at `anchor` (projected ground point in continuous
/// grid space) for `impact_age` seconds after the telegraph. `radius` is
/// the effect's world radius in cells, `scale` the caller's size factor
/// (clamped). Deterministic in (seed, impact_age); pushes at most
/// [`BURST_BUDGET`] primitives; nothing after heavy+decay.
pub fn push_burst(
    scene: &mut ContinuousScene,
    anchor: Point2,
    impact_age: f32,
    radius: f32,
    seed: u32,
    scale: f32,
    palette: &BurstPalette,
) {
    if !alive(impact_age) {
        return;
    }
    let tau = impact_age.max(0.0);
    let scale = scale.clamp(0.4, 1.6);
    let radius = radius.max(0.5) * scale;
    let decay_p = ((tau - HEAVY_S) / DECAY_S).clamp(0.0, 1.0);
    let fade = 1.0 - decay_p;
    let expand = ease_out((tau / HEAVY_S).min(1.0));
    let mut pushed = 0usize;
    let mut push =
        |glyph: char, x: f32, y: f32, color: Rgb, opacity: f32, size: f32, emit: bool| {
            if pushed >= BURST_BUDGET {
                return;
            }
            let mut material = ContinuousMaterial::new(color).with_opacity(opacity);
            if emit {
                material = material.with_emission(Emission::new(
                    mix(color, Rgb::new(255, 240, 200), 0.25),
                    0.30 * opacity,
                    2.2,
                ));
            }
            scene.push(Primitive::new(
                PrimitiveKind::glyph(glyph),
                Transform2::new(Point2::new(x, y), 0.015).with_scale(Point2::new(size, size)),
                material,
            ));
            pushed += 1;
        };

    // Ground shockwave arc: one fast flash — out and gone inside 0.5s.
    if tau < 0.5 {
        let wave_p = tau / 0.5;
        let r = radius * (0.3 + 2.0 * ease_out(wave_p));
        let opacity = 0.8 * (1.0 - wave_p);
        for i in 0..10 {
            let a = i as f32 * std::f32::consts::TAU / 10.0 + hash01(seed, 1) * 0.6;
            push(
                '_',
                anchor.x + a.cos() * r,
                anchor.y + a.sin() * r * GROUND_SQUASH,
                palette.hot,
                opacity,
                0.8,
                false,
            );
        }
    }

    // Concentric rings: three staggered ease-out expansions; inner rings
    // are hotter and carry the terrain-tinting emission.
    let ring_specs: [(usize, char, f32); 3] = [(12, '*', 0.0), (10, 'o', 0.18), (8, '·', 0.36)];
    for (ring, (count, glyph, stagger)) in ring_specs.into_iter().enumerate() {
        let local = (tau - stagger).max(0.0);
        if local <= 0.0 {
            continue;
        }
        let p = (local / HEAVY_S).min(1.0);
        let r = radius * (0.25 + (0.75 + 0.35 * ring as f32) * ease_out(p));
        let opacity = (if tau <= HEAVY_S {
            0.45 + 0.55 * ease_out(p)
        } else {
            fade
        }) * (1.0 - 0.18 * ring as f32);
        if opacity <= 0.02 {
            continue;
        }
        for i in 0..count {
            let a = i as f32 * std::f32::consts::TAU / count as f32
                + hash01(seed, 5 + ring as u32) * 0.8;
            let color = if ring == 0 {
                mix(palette.hot, palette.ring, 0.3)
            } else {
                palette.ring
            };
            push(
                glyph,
                anchor.x + a.cos() * r,
                anchor.y + a.sin() * r * GROUND_SQUASH,
                color,
                opacity,
                0.95 - 0.12 * ring as f32,
                ring == 0,
            );
        }
    }

    // Spiral arms: two counter-phase arms of dots winding outward during
    // the heavy phase, gone early in decay.
    if tau < HEAVY_S * 0.9 {
        let spin = tau * 2.2;
        for arm in 0..2u32 {
            let base = arm as f32 * std::f32::consts::PI + spin * if arm == 0 { 1.0 } else { -0.8 };
            for i in 0..7 {
                let frac = (i + 1) as f32 / 7.0;
                let r = radius * (0.2 + 0.6 * expand) * frac;
                let a = base + i as f32 * 0.35;
                let opacity = 0.75 * fade * (1.0 - frac * 0.4);
                if opacity <= 0.02 {
                    continue;
                }
                push(
                    if i < 2 { '*' } else { '·' },
                    anchor.x + a.cos() * r,
                    anchor.y + a.sin() * r * GROUND_SQUASH,
                    palette.ring,
                    opacity,
                    0.8 - 0.08 * i as f32,
                    false,
                );
            }
        }
    }

    // Ember speckle: rises on the ease-in curve, drifts outward, lingers
    // through most of the decay — the reference's trailing sparks.
    let life = (tau / (HEAVY_S + DECAY_S)).min(1.0);
    let ember_opacity = (1.0 - life).powf(0.8);
    if ember_opacity > 0.03 {
        let rise = crate::fx_timeline::ease_in((tau / (HEAVY_S + 0.6)).min(1.0)) * 2.4;
        for i in 0..8 {
            let a = hash01(seed, 9 + i as u32) * std::f32::consts::TAU;
            let drift = radius * (0.25 + 0.85 * expand * hash01(seed, 20 + i as u32));
            let jitter = (hash01(seed, 30 + i as u32) - 0.5) * 0.3;
            push(
                if i % 3 == 0 { '\'' } else { '.' },
                anchor.x + a.cos() * drift + jitter,
                anchor.y + a.sin() * drift * GROUND_SQUASH - rise - i as f32 * 0.08,
                mix(palette.ember, palette.hot, 0.2),
                ember_opacity,
                0.6,
                false,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn palette() -> BurstPalette {
        BurstPalette {
            hot: Rgb::new(255, 250, 235),
            ring: Rgb::new(255, 190, 70),
            ember: Rgb::new(255, 150, 50),
        }
    }

    fn burst(age: f32) -> ContinuousScene {
        let mut scene = ContinuousScene::new();
        push_burst(
            &mut scene,
            Point2::new(20.0, 20.0),
            age,
            1.4,
            7,
            1.0,
            &palette(),
        );
        scene
    }

    fn glyphs(scene: &ContinuousScene) -> Vec<(char, f32, f32, f32)> {
        scene
            .primitives()
            .iter()
            .filter_map(|p| match p.kind {
                PrimitiveKind::Glyph { glyph } => Some((
                    glyph,
                    p.transform.position.x,
                    p.transform.position.y,
                    p.material.opacity,
                )),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn budget_vocabulary_and_lifetime() {
        // Peak budget inside the heavy phase; only the vocabulary glyphs.
        for age in [0.1, 0.3, 0.7, 1.2, 1.8, 2.2] {
            let scene = burst(age);
            assert!(scene.primitives().len() <= BURST_BUDGET, "age {age}");
            for (glyph, ..) in glyphs(&scene) {
                assert!("*o·_'.".contains(glyph), "unexpected glyph {glyph}");
            }
        }
        // Nothing before impact or after heavy+decay.
        assert_eq!(burst(-0.1).primitives().len(), 0);
        assert_eq!(burst(HEAVY_S + DECAY_S + 0.1).primitives().len(), 0);
    }

    #[test]
    fn phase_shape_shockwave_rings_embers() {
        // Shockwave only in the first 0.5s.
        assert!(glyphs(&burst(0.2)).iter().any(|g| g.0 == '_'));
        assert!(!glyphs(&burst(1.0)).iter().any(|g| g.0 == '_'));
        // Rings expand outward: the outermost '*' is farther at 0.9s than
        // at 0.15s.
        let spread = |age: f32| {
            glyphs(&burst(age))
                .iter()
                .filter(|g| g.0 == '*')
                .map(|g| ((g.1 - 20.0).powi(2) + (g.2 - 20.0).powi(2)).sqrt())
                .fold(0.0f32, f32::max)
        };
        assert!(spread(0.9) > spread(0.15) * 1.3, "rings must expand");
        // Mid-decay embers linger with falling opacity, rings thinned.
        let late = glyphs(&burst(HEAVY_S + 0.6));
        assert!(late.iter().any(|g| g.0 == '.' || g.0 == '\''));
        let mid = glyphs(&burst(0.6));
        let mid_max = mid.iter().map(|g| g.3).fold(0.0f32, f32::max);
        let late_max = late.iter().map(|g| g.3).fold(0.0f32, f32::max);
        assert!(late_max < mid_max, "decay must fade opacity");
    }

    #[test]
    fn deterministic_in_seed_and_age() {
        let a = burst(0.8);
        let b = burst(0.8);
        let ga = glyphs(&a);
        let gb = glyphs(&b);
        assert_eq!(ga.len(), gb.len());
        assert!(ga.iter().zip(&gb).all(|(x, y)| x.0 == y.0 && x.1 == y.1));
        let c = burst(1.1);
        assert!(
            ga.len() != glyphs(&c).len() || ga[0].1 != glyphs(&c)[0].1,
            "age must change the burst"
        );
    }
}
