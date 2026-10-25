//! Letter-built scenery props (video mimicry S2, 2026-10-25).
//!
//! The reference captures render trees as 3–6-ROW multi-glyph shapes
//! built from LETTERS — bushy `qQ0Ob` canopies against columnar `s|'`
//! habits, every tree different, standing over near-black ground — not
//! as single glyphs or shaded meshes (video-tools/out/refstudy). This
//! module is the ONE tree builder for the in-world play scenery and any
//! showcase, so that read cannot drift. Presentation-only: positions
//! arrive already projected; authoritative world state never changes.
//!
//! Sprites sit at 0.55-cell pitch — deliberately SUB-cell so canopy mass
//! reads denser than the ground lattice (the reference's "more dots"
//! impression comes from dense sprites over a 2–9%-ink floor, not from a
//! finer world grid). Sway is one coherent travelling wave per tree: a
//! row shares its phase and upper rows lag the trunk, so neighbours
//! wave together the way the reference canopies do. Trees carry no
//! emission — light pools come from the torch/flame system and receiver
//! fill lands on world cells; props, like actors, read as lit ink by
//! their palette alone.

use crate::continuous::{
    ContinuousScene, Material as ContinuousMaterial, Point2, Primitive, PrimitiveKind, Transform2,
};
use crate::Rgb;

/// Scene palette for one tree species (bark + three canopy tones: deep
/// shadow, body, lit/highlight).
#[derive(Clone, Copy, Debug)]
pub struct TreePalette {
    pub bark: Rgb,
    pub canopy_deep: Rgb,
    pub canopy: Rgb,
    pub canopy_lit: Rgb,
}

/// Canopy habits keyed off the per-tree seed hash — the reference's
/// per-tree variety (bushy vs rounded vs columnar) with fixed authoring
/// so glyphs stay deterministic in seed + time.
const HABITS: [&[&str]; 3] = [
    // Bushy: widest, the 20–40-glyph reference mass.
    &[".q.", "qQq", "Q0OQ", "bQ0Qb"],
    // Rounded: compact lollipop.
    &[".o.", "oOo", "O@O", ".o."],
    // Columnar: tall sparse conifer (cypress habit).
    &["'", "s|s", "s|s", "s|s", ".s."],
];

/// Hard per-tree budget (habit mass + trunk + root); the bushy habit is
/// the largest at 19 glyphs. Trees are scenery — one builder can never
/// blow the caller's native budget on its own.
pub const TREE_BUDGET: usize = 24;

/// Horizontal sub-cell pitch between canopy glyphs (matches the flame
/// row stack's spacing so sprite density reads consistently).
const PITCH: f32 = 0.55;
/// Vertical spacing between sprite rows in cell units.
const ROW_H: f32 = 0.8;
/// Trunk rows below the canopy before the root tick.
const TRUNK_ROWS: usize = 2;

fn hash01(seed: u32, salt: u32) -> f32 {
    let mut h = seed.wrapping_mul(374_761_393) ^ salt.wrapping_mul(668_265_263);
    h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
    ((h ^ (h >> 16)) & 0x00ff_ffff) as f32 / 16_777_215.0
}

fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let ch = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t) as u8;
    Rgb::new(ch(a.r, b.r), ch(a.g, b.g), ch(a.b, b.b))
}

/// Compose one letter-built tree at `anchor` (the projected ground point
/// in continuous grid space) into `scene`. `seed` picks the habit and
/// the per-glyph colour jitter (stable per tree), `scale` is the
/// caller's distance/size factor (clamped, glyph quads and spacing
/// scale together), and `t` drives the coherent sway. Deterministic in
/// its inputs; pushes at most [`TREE_BUDGET`] primitives.
pub fn push_tree(
    scene: &mut ContinuousScene,
    anchor: Point2,
    seed: u32,
    scale: f32,
    t: f32,
    palette: &TreePalette,
) {
    let scale = scale.clamp(0.3, 1.6);
    let habit = HABITS[(hash01(seed, 0) * HABITS.len() as f32) as usize % HABITS.len()];
    let phase = hash01(seed, 7) * std::f32::consts::TAU;
    let total_rows = TRUNK_ROWS + habit.len();
    let mut pushed = 0usize;

    let mut row_glyph = |glyph: char, x: f32, y: f32, color: Rgb, glyph_scale: f32| {
        if pushed < TREE_BUDGET {
            scene.push(Primitive::new(
                PrimitiveKind::glyph(glyph),
                Transform2::new(Point2::new(x, y), 0.0)
                    .with_scale(Point2::new(glyph_scale, glyph_scale)),
                ContinuousMaterial::new(color),
            ));
            pushed += 1;
        }
    };

    // Trunk + root first (bottom rows, no sway — the canopy waves, the
    // stem holds). Pushing bottom-up means upper rows overlap lower in
    // paint order: canopy ink sits over trunk ink at the join.
    for k in 0..TRUNK_ROWS {
        let y = anchor.y - k as f32 * ROW_H * scale;
        let jitter = (hash01(seed, 3 + k as u32) - 0.5) * 0.08;
        row_glyph(
            '|',
            anchor.x + jitter * scale,
            y,
            mix(palette.bark, palette.canopy_deep, 0.10 * k as f32),
            0.9 * scale,
        );
    }
    row_glyph(
        '_',
        anchor.x + 0.28 * scale,
        anchor.y - TRUNK_ROWS as f32 * ROW_H * scale + 0.1,
        mix(palette.bark, palette.canopy_deep, 0.35),
        0.8 * scale,
    );

    // Canopy rows above the trunk: each row sways with a phase lag down
    // its height (frac 0 at the trunk join → 1 at the crown) so the
    // crown whips slightly ahead — one coherent wave, never per-glyph
    // noise (the grass-gust lesson).
    for (r, row) in habit.iter().enumerate() {
        let k = TRUNK_ROWS + r;
        let frac = k as f32 / total_rows as f32;
        let sway = (t * 0.8 + phase + r as f32 * 0.35).sin() * 0.10 * frac * frac;
        let y = anchor.y - k as f32 * ROW_H * scale;
        let count = row.chars().count();
        for (i, glyph) in row.chars().enumerate() {
            let lx = (i as f32 - (count as f32 - 1.0) / 2.0) * PITCH * scale;
            // Per-glyph tone: hash jitter + height lift; the bottom
            // canopy row dips toward the deep tone, holes read deepest.
            let jit = hash01(seed, 11 + (k * 8 + i) as u32);
            let hole = glyph == '0' || glyph == '@';
            let mut color = mix(
                palette.canopy,
                palette.canopy_lit,
                (jit * 0.35 + frac * 0.45).min(1.0),
            );
            if r == 0 {
                color = mix(color, palette.canopy_deep, 0.30);
            }
            if hole {
                color = mix(color, palette.canopy_deep, 0.45);
            }
            row_glyph(glyph, anchor.x + lx + sway * scale, y, color, 0.95 * scale);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn palette() -> TreePalette {
        TreePalette {
            bark: Rgb::new(96, 74, 48),
            canopy_deep: Rgb::new(24, 58, 34),
            canopy: Rgb::new(48, 108, 58),
            canopy_lit: Rgb::new(120, 190, 96),
        }
    }

    fn glyphs(scene: &ContinuousScene) -> Vec<(char, f32, f32)> {
        scene
            .primitives()
            .iter()
            .filter_map(|p| match p.kind {
                PrimitiveKind::Glyph { glyph } => {
                    Some((glyph, p.transform.position.x, p.transform.position.y))
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn tree_stays_within_budget_and_grows_upward() {
        for seed in 0..24u32 {
            let mut scene = ContinuousScene::new();
            push_tree(
                &mut scene,
                Point2::new(20.0, 18.0),
                seed,
                1.0,
                0.0,
                &palette(),
            );
            assert!(
                scene.primitives().len() <= TREE_BUDGET,
                "seed {seed}: {} prims",
                scene.primitives().len()
            );
            assert!(
                scene.primitives().len() >= 12,
                "seed {seed}: habit too thin ({} prims)",
                scene.primitives().len()
            );
            let ys: Vec<f32> = glyphs(&scene).iter().map(|g| g.2).collect();
            assert!(
                ys.iter().all(|y| *y < 18.01),
                "tree must grow upward from the anchor"
            );
        }
    }

    #[test]
    fn canopy_is_letter_built_with_variety() {
        // Every tree's canopy is built from letters (qQ0Ob / oO@ / s|'),
        // and different seeds pick visibly different habits: the widest
        // row of the bushy habit is 5 glyphs, rounded tops out at 3.
        let mut widths = Vec::new();
        for seed in 0..30u32 {
            let mut scene = ContinuousScene::new();
            push_tree(
                &mut scene,
                Point2::new(10.0, 10.0),
                seed,
                1.0,
                0.0,
                &palette(),
            );
            let g = glyphs(&scene);
            assert!(g.iter().any(|c| "qQ0Oob@s'|_".contains(c.0)));
            // Widest row = most glyphs sharing one y (rounded to row pitch).
            let mut best = 0;
            for y in g.iter().map(|c| c.2) {
                best = best.max(g.iter().filter(|c| (c.2 - y).abs() < 0.1).count());
            }
            widths.push(best);
        }
        let max_width = *widths.iter().max().unwrap();
        let min_width = *widths.iter().min().unwrap();
        assert!(max_width >= 5, "bushy habit must reach 5-glyph rows");
        assert!(min_width <= 3, "compact habits must stay narrow");
        assert!(max_width > min_width, "seeds must pick different habits");
    }

    #[test]
    fn sway_is_coherent_and_deterministic() {
        // Same inputs → same tree; time moves canopy glyphs but never the
        // trunk.
        let mut a = ContinuousScene::new();
        push_tree(&mut a, Point2::new(5.0, 5.0), 42, 1.0, 1.25, &palette());
        let mut b = ContinuousScene::new();
        push_tree(&mut b, Point2::new(5.0, 5.0), 42, 1.0, 1.25, &palette());
        let ga = glyphs(&a);
        let gb = glyphs(&b);
        assert_eq!(ga.len(), gb.len());
        assert!(ga.iter().zip(&gb).all(|(x, y)| x.0 == y.0 && x.1 == y.1));

        let mut c = ContinuousScene::new();
        push_tree(&mut c, Point2::new(5.0, 5.0), 42, 1.0, 2.0, &palette());
        // Trunk = the two rows above the anchor (y > 3.9 at scale 1); the
        // columnar habit's canopy ALSO uses '|', so glyph identity alone
        // cannot select the trunk.
        let trunk_a: Vec<f32> = ga.iter().filter(|g| g.2 > 3.9).map(|g| g.1).collect();
        let trunk_c: Vec<f32> = glyphs(&c)
            .iter()
            .filter(|g| g.2 > 3.9)
            .map(|g| g.1)
            .collect();
        assert_eq!(trunk_a, trunk_c, "trunk must never sway");
        let canopy_a: Vec<f32> = ga
            .iter()
            .filter(|g| "qQ0Oob@s'".contains(g.0))
            .map(|g| g.1)
            .collect();
        let canopy_c: Vec<f32> = glyphs(&c)
            .iter()
            .filter(|g| "qQ0Oob@s'".contains(g.0))
            .map(|g| g.1)
            .collect();
        assert_ne!(canopy_a, canopy_c, "canopy must sway over time");
    }
}
