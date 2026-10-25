//! Layered animated glyph characters for the continuous canvas (issue #8).
//!
//! A character is a stack of independently animated primitive layers —
//! shadow, aura, legs, torso, arms, head, equipment, and idle motes — built
//! from the same [`ContinuousScene`] seam the torch/sun showcase proved.
//! Every layer family samples its own motion channel (gait, action, idle, or
//! presentation time), so a swing never disturbs the walk cycle and the aura
//! never disturbs the silhouette. This module is presentation-only: it never
//! sees snapshots, authoritative positions, or gameplay state; callers pass a
//! foot anchor in continuous grid space plus client-owned motion state.

use crate::continuous::{
    ColorGradient, ContinuousScene, Emission, GradientAxis, Material, Point2, Primitive,
    PrimitiveKind, Transform2,
};
use crate::Rgb;

/// Upper bound on primitives per composed character. Keeps the presenter's
/// per-character instance/emission cost constant regardless of animation.
pub const CHARACTER_PRIMITIVE_BUDGET: usize = 32;

/// Per-role colours for the layered silhouette.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CharacterPalette {
    pub head: Rgb,
    pub torso: Rgb,
    pub limb: Rgb,
    pub gear: Rgb,
    pub aura: Rgb,
}

/// What to compose: palette, silhouette scale (1.0 ≈ four cell rows), and an
/// optional aura (`None` suppresses the aura layer; idle motes stay).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CharacterSpec {
    pub palette: CharacterPalette,
    pub scale: f32,
    pub aura: Option<Rgb>,
}

/// Client-owned animation state. Each field drives exactly one layer family:
/// `gait` the legs/arms/torso bob, `action` the weapon swing, `idle` the
/// breathing drift, and `t` the aura orbit plus mote cycles. `facing`
/// orients the ground shadow and the equipment side.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CharacterMotion {
    pub gait: f32,
    /// Action phase in `0.0..=1.0`; `0.0` is idle. Maps to a weapon arc.
    pub action: f32,
    pub idle: f32,
    pub facing: f32,
    pub t: f32,
}

impl CharacterMotion {
    pub const fn idle_at(t: f32) -> Self {
        Self {
            gait: 0.0,
            action: 0.0,
            idle: t,
            facing: 0.0,
            t,
        }
    }
}

/// Deterministic seeded unit noise in [0, 1) — the same hash construction as
/// `fx::hash_unit`, normalized to a unit range like the torch showcase.
fn unit(seed: u32) -> f32 {
    let mut x = seed.wrapping_mul(0x9e37_79b9).wrapping_add(0x7f4a_7c15);
    x ^= x >> 16;
    x = x.wrapping_mul(0x85eb_ca6b);
    x ^= x >> 13;
    (x >> 8) as f32 / (1u32 << 24) as f32
}

fn finite_or(value: f32, fallback: f32) -> f32 {
    if value.is_finite() {
        value
    } else {
        fallback
    }
}

/// Compose one layered character at `anchor` (the foot position, continuous
/// grid space, up is -y) into `scene`. Deterministic in its inputs; layer
/// depths sort shadow < aura < legs/arms/torso/head < equipment < motes, so
/// the silhouette always reads above its shadow and under its gear.
pub fn push_character(
    scene: &mut ContinuousScene,
    spec: &CharacterSpec,
    motion: CharacterMotion,
    anchor: Point2,
) {
    let s = finite_or(spec.scale, 1.0).clamp(0.5, 3.0);
    let gait = finite_or(motion.gait, 0.0);
    let action = finite_or(motion.action, 0.0).clamp(0.0, 1.0);
    let idle = finite_or(motion.idle, 0.0);
    let facing = finite_or(motion.facing, 0.0);
    let t = finite_or(motion.t, 0.0).max(0.0);
    let bob = (gait * 2.0).sin() * 0.06 * s;
    let stride = 0.5 + 0.5 * gait.cos();
    let swing = gait.sin();
    // Un-bobbed shoulder line: the anchor height gait-independent layers
    // (head, weapon) key from, so the walk cycle cannot leak into them.
    let shoulder_y = anchor.y - 1.27 * s;

    // Ground shadow: wide, low, dark blocks hinting an ellipse, oriented to
    // the facing yaw and breathing with the idle clock.
    for (i, ox) in [0.0f32, 0.62].iter().enumerate() {
        let width = if i == 0 { 1.5 } else { 0.7 };
        scene.push(Primitive::new(
            PrimitiveKind::glyph('▀'),
            Transform2::new(
                Point2::new(anchor.x + (ox - 0.31) * s, anchor.y + 0.08),
                -2.0,
            )
            .with_scale(Point2::new(width * s, 0.34 * s))
            .with_rotation(facing)
            .with_skew(Point2::new((idle * 0.5).sin() * 0.04, 0.0)),
            Material::new(Rgb::new(6, 9, 12)).with_opacity(0.62 - 0.18 * i as f32),
        ));
    }

    // Aura: orbiting motes behind the body, keyed to presentation time only.
    if let Some(aura) = spec.aura {
        for i in 0..6u32 {
            let phase = t * 1.4 + i as f32 * std::f32::consts::TAU / 6.0;
            let radius = (1.25 + 0.22 * (t * 2.1 + i as f32).sin()) * s;
            let x = anchor.x + phase.cos() * radius;
            let y = anchor.y - 1.5 * s + phase.sin() * radius * 0.62;
            let pulse = 0.5 + 0.5 * (t * 3.0 + i as f32 * 1.7).sin();
            let mote = (0.32 + 0.20 * pulse) * s;
            scene.push(Primitive::new(
                PrimitiveKind::particle('·'),
                Transform2::new(Point2::new(x, y), -1.0).with_scale(Point2::new(mote, mote)),
                Material::new(aura)
                    .with_gradient(ColorGradient::new(
                        aura,
                        Rgb::new(aura.r / 3, aura.g / 3, aura.b / 3),
                        GradientAxis::Radial,
                    ))
                    .with_opacity(0.30 + 0.30 * pulse)
                    .with_emission(Emission::new(aura, 0.12 + 0.08 * pulse, 1.5 * s)),
            ));
        }
    }

    // Legs: two striders offset by half a gait cycle. Swing and length carry
    // the walk in sub-cell steps; idle (gait 0) settles to a neutral stance.
    for (side, dir) in [(0usize, 1.0f32), (1, -1.0)] {
        let x = anchor.x + dir * (0.16 + swing * dir * 0.20 * stride) * s;
        let len = (0.72 + dir * swing * 0.16) * s;
        scene.push(Primitive::new(
            PrimitiveKind::glyph('│'),
            Transform2::new(
                Point2::new(x, anchor.y - len * 0.5),
                0.1 + 0.05 * side as f32,
            )
            .with_scale(Point2::new(0.20 * s, len))
            .with_rotation(swing * dir * 0.18 * stride),
            Material::new(spec.palette.limb)
                .with_gradient(ColorGradient::new(
                    spec.palette.limb,
                    Rgb::new(
                        (spec.palette.limb.r / 2).max(8),
                        (spec.palette.limb.g / 2).max(8),
                        (spec.palette.limb.b / 2).max(8),
                    ),
                    GradientAxis::Vertical,
                ))
                .with_opacity(0.97),
        ));
    }

    // Torso: bobs at twice the stride rate, breathes on the idle clock, and
    // leans into the stride with a small skew.
    let torso_y = anchor.y - 1.27 * s - bob - (idle * 0.9).sin() * 0.03;
    scene.push(Primitive::new(
        PrimitiveKind::glyph('┼'),
        Transform2::new(Point2::new(anchor.x, torso_y), 0.5)
            .with_scale(Point2::new(0.62 * s, 1.05 * s))
            .with_skew(Point2::new(swing * 0.10 * stride, 0.0)),
        Material::new(spec.palette.torso)
            .with_gradient(ColorGradient::new(
                spec.palette.torso,
                spec.palette.limb,
                GradientAxis::Vertical,
            ))
            .with_opacity(0.98),
    ));

    // Arms: counter-swing against the legs, hanging from the torso sides.
    for dir in [1.0f32, -1.0] {
        let glyph = if dir > 0.0 { '/' } else { '\\' };
        let x = anchor.x + dir * 0.52 * s;
        let y = torso_y + 0.08 - dir * swing * 0.16 * s * stride;
        scene.push(Primitive::new(
            PrimitiveKind::particle(glyph),
            Transform2::new(Point2::new(x, y), 0.4)
                .with_scale(Point2::new(0.30 * s, 0.72 * s))
                .with_rotation(-swing * dir * 0.35 * stride),
            Material::new(spec.palette.limb).with_opacity(0.95),
        ));
    }

    // Head: independent of the gait bob (keyed to the un-bobbed shoulder
    // line plus the idle drift) so it reads as a stable figure; a radial
    // gradient keeps it the brightest landmark.
    let head_x = anchor.x + (idle * 0.7).sin() * 0.05 * s;
    let head_y = shoulder_y - 0.78 * s;
    scene.push(Primitive::new(
        PrimitiveKind::glyph('o'),
        Transform2::new(Point2::new(head_x, head_y), 0.9)
            .with_scale(Point2::new(0.58 * s, 0.58 * s)),
        Material::new(spec.palette.head).with_gradient(ColorGradient::new(
            spec.palette.head,
            spec.palette.torso,
            GradientAxis::Radial,
        )),
    ));

    // Equipment. The weapon sweeps an arc driven by the action phase alone —
    // it hangs from the un-bobbed shoulder line so gait/idle cannot leak
    // into the swing, and idle rests it on the shoulder; the shield counters
    // with a small lean. Both stay clear of the head so nameplates above
    // read intact.
    let swing_angle = -0.55 + action * 2.1;
    let weapon_x = anchor.x + (0.62 + 0.34 * swing_angle.cos()) * s;
    let weapon_y = shoulder_y - 0.10 - 0.62 * swing_angle.sin() * s;
    scene.push(Primitive::new(
        PrimitiveKind::glyph('|'),
        Transform2::new(Point2::new(weapon_x, weapon_y), 1.2)
            .with_scale(Point2::new(0.16 * s, 1.30 * s))
            .with_rotation(swing_angle * 0.55),
        Material::new(spec.palette.gear)
            .with_gradient(ColorGradient::new(
                spec.palette.gear,
                Rgb::new(
                    (spec.palette.gear.r / 2).max(10),
                    (spec.palette.gear.g / 2).max(10),
                    (spec.palette.gear.b / 2).max(10),
                ),
                GradientAxis::Vertical,
            ))
            .with_opacity(0.96),
    ));
    scene.push(Primitive::new(
        PrimitiveKind::glyph('('),
        Transform2::new(Point2::new(anchor.x - 0.58 * s, torso_y + 0.06), 1.1)
            .with_scale(Point2::new(0.30 * s, 0.72 * s))
            .with_rotation(-swing * 0.12 * stride + action * 0.10),
        Material::new(spec.palette.gear).with_opacity(0.92),
    ));

    // Idle motes: two seeded rising sparks above the head, independent of
    // every other layer, fading before they reach nameplate height.
    for i in 0..2u32 {
        let h1 = unit(0xC1A + i);
        let h2 = unit(0xC1A + i + 41);
        let lifetime = 1.6 + h2;
        let age = (t + h1 * lifetime).rem_euclid(lifetime);
        let p = age / lifetime;
        let fade = (p * std::f32::consts::PI).sin();
        let x =
            anchor.x + (p * std::f32::consts::PI).sin() * 0.28 * if i == 0 { 1.0 } else { -1.0 };
        let y = head_y - 0.7 - p * 1.1;
        let mote = (0.22 + 0.12 * h2) * (1.0 - 0.5 * p) * s;
        scene.push(Primitive::new(
            PrimitiveKind::particle('·'),
            Transform2::new(Point2::new(x, y), 1.4).with_scale(Point2::new(mote, mote)),
            Material::new(spec.palette.head)
                .with_opacity(0.45 * fade)
                .with_emission(Emission::new(spec.palette.head, 0.06 * fade, 1.0)),
        ));
    }
}

/// Compose a standalone character scene (one character, centred). Useful for
/// tests and previews; games normally call [`push_character`] into a shared
/// scene.
pub fn compose_character(
    spec: &CharacterSpec,
    motion: CharacterMotion,
    anchor: Point2,
) -> ContinuousScene {
    let mut scene = ContinuousScene::new();
    push_character(&mut scene, spec, motion, anchor);
    scene
}

#[cfg(test)]
mod tests {
    use super::*;

    fn palette() -> CharacterPalette {
        CharacterPalette {
            head: Rgb::new(210, 255, 170),
            torso: Rgb::new(120, 220, 90),
            limb: Rgb::new(90, 190, 70),
            gear: Rgb::new(235, 215, 145),
            aura: Rgb::new(255, 220, 120),
        }
    }

    fn spec(aura: bool) -> CharacterSpec {
        CharacterSpec {
            palette: palette(),
            scale: 1.0,
            aura: aura.then(|| Rgb::new(255, 220, 120)),
        }
    }

    #[test]
    fn budget_is_bounded_and_aura_off_is_smaller() {
        let full = compose_character(&spec(true), CharacterMotion::idle_at(0.0), Point2::ZERO);
        let bare = compose_character(&spec(false), CharacterMotion::idle_at(0.0), Point2::ZERO);
        assert!(full.primitives().len() <= CHARACTER_PRIMITIVE_BUDGET);
        assert_eq!(bare.primitives().len() + 6, full.primitives().len());
    }

    #[test]
    fn composition_is_deterministic() {
        let motion = CharacterMotion {
            gait: 2.2,
            action: 0.4,
            idle: 0.7,
            facing: 0.3,
            t: 1.9,
        };
        let a = compose_character(&spec(true), motion, Point2::new(3.5, 7.25));
        let b = compose_character(&spec(true), motion, Point2::new(3.5, 7.25));
        assert_eq!(a.primitives(), b.primitives());
        assert_eq!(a.sample(1.9), b.sample(1.9));
    }

    /// Gait moves legs/arms/torso but leaves the weapon, aura, and motes
    /// untouched: each layer family animates on its own channel.
    #[test]
    fn gait_moves_body_but_not_equipment_or_aura() {
        let still = compose_character(
            &spec(true),
            CharacterMotion {
                gait: 0.0,
                action: 0.0,
                idle: 0.0,
                facing: 0.0,
                t: 1.0,
            },
            Point2::ZERO,
        );
        let walking = compose_character(
            &spec(true),
            CharacterMotion {
                gait: 1.3,
                action: 0.0,
                idle: 0.0,
                facing: 0.0,
                t: 1.0,
            },
            Point2::ZERO,
        );
        let glyph_positions = |scene: &ContinuousScene, want: char| {
            scene
                .primitives()
                .iter()
                .filter(|p| match p.kind {
                    PrimitiveKind::Glyph { glyph } | PrimitiveKind::Particle { glyph } => {
                        glyph == want
                    }
                    _ => false,
                })
                .map(|p| p.transform.position)
                .collect::<Vec<_>>()
        };
        let legs_still = glyph_positions(&still, '│');
        let legs_walk = glyph_positions(&walking, '│');
        let moved = legs_still
            .iter()
            .zip(legs_walk.iter())
            .any(|(a, b)| (a.x - b.x).abs() >= 0.1);
        assert!(moved, "gait must move the legs in sub-cell steps");

        let weapon_still = glyph_positions(&still, '|');
        let weapon_walk = glyph_positions(&walking, '|');
        assert_eq!(weapon_still, weapon_walk, "weapon is action-keyed only");

        // '·' is shared by the aura (depth -1) and the idle motes (depth
        // 1.4); the aura contract is about the orbiting ring only.
        let aura = |scene: &ContinuousScene| {
            scene
                .primitives()
                .iter()
                .filter(|p| match p.kind {
                    PrimitiveKind::Glyph { glyph } | PrimitiveKind::Particle { glyph } => {
                        glyph == '·' && p.transform.depth < 0.0
                    }
                    _ => false,
                })
                .map(|p| p.transform.position)
                .collect::<Vec<_>>()
        };
        assert_eq!(aura(&still), aura(&walking), "aura is time-keyed only");
    }

    /// Action swings the weapon without disturbing the walk silhouette.
    #[test]
    fn action_swings_weapon_but_not_legs() {
        let idle = compose_character(
            &spec(false),
            CharacterMotion {
                gait: 0.9,
                action: 0.0,
                idle: 0.0,
                facing: 0.0,
                t: 1.0,
            },
            Point2::ZERO,
        );
        let swinging = compose_character(
            &spec(false),
            CharacterMotion {
                gait: 0.9,
                action: 0.85,
                idle: 0.0,
                facing: 0.0,
                t: 1.0,
            },
            Point2::ZERO,
        );
        let find = |scene: &ContinuousScene, want: char| {
            scene
                .primitives()
                .iter()
                .filter_map(|p| match p.kind {
                    PrimitiveKind::Glyph { glyph } | PrimitiveKind::Particle { glyph } => {
                        (glyph == want).then_some((p.transform.position, p.transform.rotation))
                    }
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        let weapon_idle = find(&idle, '|');
        let weapon_swing = find(&swinging, '|');
        assert!(
            weapon_idle
                .iter()
                .zip(weapon_swing.iter())
                .any(|(a, b)| (a.1 - b.1).abs() >= 0.3 || (a.0.y - b.0.y).abs() >= 0.3),
            "action phase must swing the weapon"
        );
        let legs_idle = find(&idle, '│');
        let legs_swing = find(&swinging, '│');
        assert_eq!(legs_idle, legs_swing, "legs are gait-keyed only");
    }

    /// The layer stack uses every appearance channel the seam offers:
    /// sub-cell offsets, non-trivial scale/rotation, gradients (vertical and
    /// radial), opacity below 1, and real emissions.
    #[test]
    fn layers_use_subcell_and_appearance_channels() {
        let motion = CharacterMotion {
            gait: 1.1,
            action: 0.5,
            idle: 0.4,
            facing: 0.2,
            t: 1.0,
        };
        let sampled = compose_character(&spec(true), motion, Point2::new(10.0, 10.0)).sample(1.0);
        let fractional = sampled
            .iter()
            .any(|p| p.transform.position.x.fract().abs() > 1e-3);
        assert!(fractional, "layers must hold sub-cell positions");
        assert!(sampled.iter().any(|p| p.transform.rotation != 0.0));
        assert!(sampled.iter().any(|p| p.material.opacity < 0.9));
        assert!(sampled.iter().any(|p| p
            .material
            .gradient
            .is_some_and(|g| g.axis == GradientAxis::Vertical)));
        assert!(sampled.iter().any(|p| p
            .material
            .gradient
            .is_some_and(|g| g.axis == GradientAxis::Radial)));
        assert!(sampled
            .iter()
            .any(|p| p.material.emission.is_some_and(|e| e.strength > 0.0)));
        let scales: std::collections::BTreeSet<i64> = sampled
            .iter()
            .map(|p| (p.transform.scale.y * 100.0).round() as i64)
            .collect();
        assert!(scales.len() >= 4, "silhouette needs varied glyph sizes");
    }

    /// Readability contract: sampled order keeps the shadow under the body,
    /// the body under the equipment, and the equipment under the motes, so
    /// the composed silhouette can never draw gear behind its own shadow.
    #[test]
    fn sampled_depth_order_keeps_shadow_under_body_under_gear() {
        let sampled = compose_character(
            &spec(true),
            CharacterMotion {
                gait: 0.8,
                action: 0.3,
                idle: 0.2,
                facing: 0.1,
                t: 1.0,
            },
            Point2::new(5.0, 5.0),
        )
        .sample(1.0);
        let depth_of = |want: char| {
            sampled
                .iter()
                .find(|p| match p.kind {
                    PrimitiveKind::Glyph { glyph } | PrimitiveKind::Particle { glyph } => {
                        glyph == want
                    }
                    _ => false,
                })
                .map(|p| p.transform.depth)
        };
        let shadow = depth_of('▀').expect("shadow layer");
        let torso = depth_of('┼').expect("torso layer");
        let weapon = depth_of('|').expect("weapon layer");
        assert!(
            shadow < torso,
            "shadow ({shadow}) must sort under torso ({torso})"
        );
        assert!(
            torso < weapon,
            "torso ({torso}) must sort under weapon ({weapon})"
        );
    }
}
