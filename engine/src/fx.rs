//! Pure, bounded particle/effect layers.
//!
//! [`layer`] samples explicit seconds; it does not retain state or consult a
//! clock.  Emitter origins are world cell coordinates (fractional values are
//! rounded when stamped), and `grow` values are seconds per revealed point.
//! Each emitter is capped at [`MAX_CELLS`] output cells, including long text,
//! paths, and deliberately huge counts.  Invalid, non-finite, or out-of-range
//! inputs simply produce no cells.

use crate::{AnimCell, Cell, Layer};

/// Hard safety limit for one emitter's contribution to a layer.
pub const MAX_CELLS: usize = 4096;
const COORD_LIMIT: f32 = 1_000_000.0;

/// A time-bounded effect source. `start` and `lifetime` are seconds in the
/// caller's explicit timeline; the effect is visible for `start <= time <
/// start + lifetime`.
#[derive(Clone, Debug)]
pub struct Emitter {
    pub origin: (f32, f32),
    pub start: f32,
    pub lifetime: f32,
    pub cell: Cell,
    pub kind: Effect,
}

/// Data-driven particle archetypes. All sampling is deterministic for a given
/// emitter and time, including [`Effect::Burst`] (whose seed is not global
/// state).
#[derive(Clone, Debug)]
pub enum Effect {
    Burst {
        count: usize,
        speed: f32,
        seed: u32,
    },
    Ring {
        count: usize,
        radius: f32,
    },
    Chain {
        points: Vec<(i32, i32)>,
        grow: f32,
    },
    Spike {
        length: usize,
        direction: (f32, f32),
        grow: f32,
    },
    /// A vertical spell column that reveals upward, with deterministic jagged
    /// side branches.  The caller can pair it with a [`crate::Wash`] for the
    /// reference's whole-screen lightning flash.
    LightningColumn {
        height: usize,
        width: usize,
        grow: f32,
        seed: u32,
    },
    /// Parallel icicles advancing as a wave.  `direction` is the direction
    /// each spike points; rows are laid out on its perpendicular axis.
    IcicleWave {
        rows: usize,
        length: usize,
        direction: (f32, f32),
        spacing: f32,
        grow: f32,
    },
    /// Telegraph: a dashed ring that starts at `radius` and sweeps inward
    /// (plus a slow rotation) reaching the origin exactly when the emitter
    /// expires — the reference's "wind-up closes before the hit" language.
    /// Glyph is `-`, rotated toward the ring tangent, so it reads as dashes
    /// rather than a dotted circle.
    Implosion {
        count: usize,
        radius: f32,
    },
    /// A loose, rising smoke ring.  Unlike [`Effect::Ring`], puffs use a
    /// small deterministic glyph family and drift upward as the ring opens.
    SmokeRing {
        count: usize,
        radius: f32,
        rise: f32,
        seed: u32,
    },
    Number {
        text: String,
        rise: f32,
    },
}

/// Build an effect layer at `time`. Sampling is order-independent and has no
/// I/O, game state, allocation beyond the returned layer, or wall-clock use.
pub fn layer(name: impl Into<String>, z: i32, emitters: &[Emitter], time: f32) -> Layer {
    let mut out = Layer::new(name, z);
    if !time.is_finite() {
        return out;
    }
    for emitter in emitters {
        let Some(age) = active_age(emitter, time) else {
            continue;
        };
        if !valid_origin(emitter.origin) {
            continue;
        }
        match &emitter.kind {
            Effect::Burst { count, speed, seed } => {
                if *count == 0 || !speed.is_finite() || *speed < 0.0 {
                    continue;
                }
                let n = (*count).min(MAX_CELLS);
                for i in 0..n {
                    let angle = hash_unit(seed.wrapping_add(i as u32)).to_radians();
                    push(
                        &mut out,
                        emitter,
                        emitter.origin.0 + angle.cos() * *speed * age,
                        emitter.origin.1 + angle.sin() * *speed * age,
                        '•',
                        age,
                    );
                }
            }
            Effect::Ring { count, radius } => {
                if *count == 0 || !radius.is_finite() || *radius < 0.0 {
                    continue;
                }
                let n = (*count).min(MAX_CELLS);
                let r = *radius * (age / emitter.lifetime);
                if !r.is_finite() {
                    continue;
                }
                for i in 0..n {
                    let angle = i as f32 * 360.0 / *count as f32;
                    let radians = angle.to_radians();
                    push(
                        &mut out,
                        emitter,
                        emitter.origin.0 + radians.cos() * r,
                        emitter.origin.1 + radians.sin() * r,
                        'o',
                        age,
                    );
                }
            }
            Effect::Chain { points, grow } => {
                if points.is_empty() || !valid_grow(*grow) {
                    continue;
                }
                let revealed = ((age / *grow).floor() as usize)
                    .min(points.len())
                    .min(MAX_CELLS);
                for (i, &(x, y)) in points.iter().take(revealed).enumerate() {
                    let glyph = if (i + 1) % 5 == 0 {
                        'O'
                    } else {
                        chain_glyph(points, i)
                    };
                    push(
                        &mut out,
                        emitter,
                        emitter.origin.0 + x as f32,
                        emitter.origin.1 + y as f32,
                        glyph,
                        age,
                    );
                }
            }
            Effect::Spike {
                length,
                direction,
                grow,
            } => {
                if *length == 0 || !valid_grow(*grow) || !valid_direction(*direction) {
                    continue;
                }
                let revealed = ((age / *grow).floor() as usize).min(*length).min(MAX_CELLS);
                let glyph = ray_glyph(*direction);
                for i in 0..revealed {
                    let distance = i as f32 + 1.0;
                    push(
                        &mut out,
                        emitter,
                        emitter.origin.0 + direction.0 * distance,
                        emitter.origin.1 + direction.1 * distance,
                        glyph,
                        age,
                    );
                }
            }
            Effect::LightningColumn {
                height,
                width,
                grow,
                seed,
            } => {
                if *height == 0 || *width == 0 || !valid_grow(*grow) {
                    continue;
                }
                let revealed = ((age / *grow).floor() as usize).min(*height).min(MAX_CELLS);
                let half = (*width / 2) as i32;
                for row in 0..revealed {
                    let jitter = ((hash_unit(seed.wrapping_add(row as u32)) / 45.0).round() as i32
                        % (*width as i32))
                        - half;
                    let y = emitter.origin.1 - row as f32;
                    let glyph = if row % 4 == 0 { '+' } else { '|' };
                    push(
                        &mut out,
                        emitter,
                        emitter.origin.0 + jitter as f32,
                        y,
                        glyph,
                        age,
                    );
                    if row > 1 && row % 3 == 0 {
                        let side = if row % 2 == 0 { 1.0 } else { -1.0 };
                        push(
                            &mut out,
                            emitter,
                            emitter.origin.0 + jitter as f32 + side,
                            y + 1.0,
                            if side > 0.0 { '/' } else { '\\' },
                            age,
                        );
                    }
                }
            }
            Effect::IcicleWave {
                rows,
                length,
                direction,
                spacing,
                grow,
            } => {
                if *rows == 0
                    || *length == 0
                    || !valid_grow(*grow)
                    || !spacing.is_finite()
                    || *spacing < 0.0
                    || !valid_direction(*direction)
                {
                    continue;
                }
                let revealed_rows = ((age / *grow).floor() as usize).min(*rows).min(MAX_CELLS);
                let norm = (direction.0 * direction.0 + direction.1 * direction.1)
                    .sqrt()
                    .max(1.0e-6);
                let (dx, dy) = (direction.0 / norm, direction.1 / norm);
                let (px, py) = (-dy, dx);
                for row in 0..revealed_rows {
                    let base_x = emitter.origin.0 + px * row as f32 * *spacing;
                    let base_y = emitter.origin.1 + py * row as f32 * *spacing;
                    for i in 0..(*length).min(MAX_CELLS) {
                        let distance = i as f32 + 1.0;
                        push(
                            &mut out,
                            emitter,
                            base_x + dx * distance,
                            base_y + dy * distance,
                            if i == 0 { '^' } else { ray_glyph(*direction) },
                            age,
                        );
                    }
                }
            }
            Effect::Implosion { count, radius } => {
                if *count == 0 || !radius.is_finite() || *radius < 0.0 {
                    continue;
                }
                let n = (*count).min(MAX_CELLS);
                let progress = (age / emitter.lifetime).clamp(0.0, 1.0);
                let r = radius * (1.0 - progress);
                if !r.is_finite() {
                    continue;
                }
                for i in 0..n {
                    // Slow swirl as the dashes tighten; each one orients
                    // along the local ring tangent for a dash look.
                    let degrees = i as f32 * 360.0 / n as f32 + age * 270.0;
                    let radians = degrees.to_radians();
                    let tangent = radians + std::f32::consts::FRAC_PI_2;
                    let tx = tangent.cos();
                    let ty = tangent.sin();
                    let glyph = if tx.abs() > ty.abs() * 1.5 { '-' } else { '|' };
                    push(
                        &mut out,
                        emitter,
                        emitter.origin.0 + radians.cos() * r,
                        emitter.origin.1 + radians.sin() * r,
                        glyph,
                        age,
                    );
                }
            }
            Effect::SmokeRing {
                count,
                radius,
                rise,
                seed,
            } => {
                if *count == 0
                    || !radius.is_finite()
                    || *radius < 0.0
                    || !rise.is_finite()
                    || *rise < 0.0
                {
                    continue;
                }
                let n = (*count).min(MAX_CELLS);
                let progress = (age / emitter.lifetime).clamp(0.0, 1.0);
                let r = *radius * (0.35 + progress * 0.65);
                for i in 0..n {
                    let angle = (i as f32 * 360.0 / n as f32
                        + hash_unit(seed.wrapping_add(i as u32)) * 0.18
                        + age * 18.0)
                        .to_radians();
                    let wobble = 1.0 + 0.12 * (age * 3.0 + i as f32).sin();
                    let glyph = match (hash_unit(seed.wrapping_add(i as u32 + 17)) / 90.0) as usize
                    {
                        0 => 'o',
                        1 => 'O',
                        2 => '@',
                        _ => '*',
                    };
                    push(
                        &mut out,
                        emitter,
                        emitter.origin.0 + angle.cos() * r * wobble,
                        emitter.origin.1 + angle.sin() * r * wobble - *rise * age,
                        glyph,
                        age,
                    );
                }
            }
            Effect::Number { text, rise } => {
                if text.is_empty() || !rise.is_finite() || *rise < 0.0 {
                    continue;
                }
                let y = emitter.origin.1 - *rise * age;
                for (i, glyph) in text.chars().take(MAX_CELLS).enumerate() {
                    push(
                        &mut out,
                        emitter,
                        emitter.origin.0 + i as f32,
                        y,
                        glyph,
                        age,
                    );
                }
            }
        }
    }
    out
}

fn active_age(emitter: &Emitter, time: f32) -> Option<f32> {
    if !emitter.start.is_finite() || !emitter.lifetime.is_finite() || emitter.lifetime <= 0.0 {
        return None;
    }
    let age = time - emitter.start;
    (age.is_finite() && age >= 0.0 && age < emitter.lifetime).then_some(age)
}

fn valid_origin((x, y): (f32, f32)) -> bool {
    x.is_finite() && y.is_finite() && x.abs() <= COORD_LIMIT && y.abs() <= COORD_LIMIT
}
fn valid_grow(grow: f32) -> bool {
    grow.is_finite() && grow > 0.0
}
fn valid_direction((x, y): (f32, f32)) -> bool {
    x.is_finite() && y.is_finite() && (x != 0.0 || y != 0.0) && x.abs() <= 1.0e4 && y.abs() <= 1.0e4
}

fn push(layer: &mut Layer, emitter: &Emitter, x: f32, y: f32, glyph: char, age: f32) {
    if layer.cells.len() >= MAX_CELLS
        || !x.is_finite()
        || !y.is_finite()
        || x.abs() > COORD_LIMIT
        || y.abs() > COORD_LIMIT
    {
        return;
    }
    let fade = (1.0 - age / emitter.lifetime).clamp(0.0, 1.0);
    let mut cell = emitter.cell;
    cell.glyph = glyph;
    cell.fg = cell.fg.scaled(fade);
    cell.glow *= fade;
    layer.push(AnimCell::still(x.round() as i32, y.round() as i32, cell));
}

fn hash_unit(seed: u32) -> f32 {
    let mut x = seed.wrapping_mul(0x9e37_79b9).wrapping_add(0x7f4a_7c15);
    x ^= x >> 16;
    x = x.wrapping_mul(0x85eb_ca6b);
    x ^= x >> 13;
    (x as f32 / u32::MAX as f32) * 360.0
}

fn chain_glyph(points: &[(i32, i32)], i: usize) -> char {
    if i + 1 >= points.len() {
        return '+';
    }
    let (dx, dy) = (points[i + 1].0 - points[i].0, points[i + 1].1 - points[i].1);
    if dx == 0 {
        '|'
    } else if dy == 0 {
        '-'
    } else if dx.signum() == dy.signum() {
        '\\'
    } else {
        '/'
    }
}

fn ray_glyph((x, y): (f32, f32)) -> char {
    if x.abs() > y.abs() * 1.5 {
        '-'
    } else if y.abs() > x.abs() * 1.5 {
        '|'
    } else if x.signum() == y.signum() {
        '\\'
    } else {
        '/'
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Rgb;

    fn e(kind: Effect) -> Emitter {
        Emitter {
            origin: (2.0, 3.0),
            start: 1.0,
            lifetime: 4.0,
            cell: Cell::new('*', Rgb::new(100, 80, 60), Rgb::new(0, 0, 0))
                .with_glow(1.0)
                .with_scale(0.5),
            kind,
        }
    }

    #[test]
    fn all_kinds_sample_and_decay() {
        let es = [
            e(Effect::Burst {
                count: 3,
                speed: 1.0,
                seed: 4,
            }),
            e(Effect::Ring {
                count: 4,
                radius: 2.0,
            }),
            e(Effect::Chain {
                points: vec![(0, 0), (1, 0), (1, 1)],
                grow: 1.0,
            }),
            e(Effect::Spike {
                length: 3,
                direction: (1.0, 0.0),
                grow: 1.0,
            }),
            e(Effect::Number {
                text: "42".into(),
                rise: 1.0,
            }),
        ];
        let a = layer("fx", 2, &es, 2.5);
        assert!(!a.cells.is_empty());
        assert!(a
            .cells
            .iter()
            .all(|c| c.cell.scale == 0.5 && c.cell.glow < 1.0));
    }

    #[test]
    fn deterministic_and_time_bounded() {
        let es = [e(Effect::Burst {
            count: 8,
            speed: 2.0,
            seed: 9,
        })];
        let a = layer("x", 0, &es, 2.0);
        let b = layer("x", 0, &es, 2.0);
        assert_eq!(
            a.cells
                .iter()
                .map(|c| (c.x, c.y, c.cell.glyph))
                .collect::<Vec<_>>(),
            b.cells
                .iter()
                .map(|c| (c.x, c.y, c.cell.glyph))
                .collect::<Vec<_>>()
        );
        assert!(layer("x", 0, &es, 0.9).cells.is_empty());
        assert!(layer("x", 0, &es, 5.0).cells.is_empty());
    }

    #[test]
    fn invalid_and_huge_inputs_are_safe() {
        let es = [
            e(Effect::Burst {
                count: usize::MAX,
                speed: f32::MAX,
                seed: 1,
            }),
            e(Effect::Number {
                text: "x".repeat(20_000),
                rise: 1.0,
            }),
            Emitter {
                origin: (f32::NAN, 0.0),
                ..e(Effect::Ring {
                    count: 2,
                    radius: 1.0,
                })
            },
        ];
        assert!(layer("x", 0, &es, 2.0).cells.len() <= MAX_CELLS);
        assert!(layer(
            "x",
            0,
            &[e(Effect::Chain {
                points: vec![(0, 0), (1, 0)],
                grow: 0.0
            })],
            2.0
        )
        .cells
        .is_empty());
    }

    #[test]
    fn growth_reveals_chain_and_spike() {
        let chain = [e(Effect::Chain {
            points: vec![(0, 0), (1, 0), (1, 1)],
            grow: 1.0,
        })];
        let spike = [e(Effect::Spike {
            length: 3,
            direction: (0.0, 1.0),
            grow: 1.0,
        })];
        assert_eq!(layer("x", 0, &chain, 2.5).cells.len(), 1);
        assert_eq!(layer("x", 0, &chain, 3.1).cells.len(), 2);
        assert_eq!(layer("x", 0, &spike, 3.9).cells.len(), 2);
    }

    #[test]
    fn lightning_column_reveals_branches_upward() {
        let column = [e(Effect::LightningColumn {
            height: 6,
            width: 5,
            grow: 0.5,
            seed: 12,
        })];
        let early = layer("x", 0, &column, 1.5);
        let late = layer("x", 0, &column, 4.0);
        assert!(late.cells.len() > early.cells.len());
        assert!(late.cells.iter().any(|c| c.cell.glyph == '+'));
        assert!(late.cells.iter().any(|c| c.y < 3));
    }

    #[test]
    fn icicle_wave_reveals_parallel_rows() {
        let wave = [e(Effect::IcicleWave {
            rows: 3,
            length: 4,
            direction: (1.0, 0.0),
            spacing: 2.0,
            grow: 1.0,
        })];
        assert_eq!(layer("x", 0, &wave, 3.9).cells.len(), 8);
        let full = layer("x", 0, &wave, 4.0);
        assert_eq!(full.cells.len(), 12);
        assert!(full.cells.iter().any(|c| c.cell.glyph == '^'));
        assert!(full.cells.iter().any(|c| c.y != 3));
    }

    #[test]
    fn smoke_ring_rises_and_uses_cloud_glyphs() {
        let smoke = [e(Effect::SmokeRing {
            count: 12,
            radius: 3.0,
            rise: 0.5,
            seed: 4,
        })];
        let low = layer("x", 0, &smoke, 1.2);
        let high = layer("x", 0, &smoke, 3.0);
        assert_eq!(low.cells.len(), 12);
        assert_eq!(high.cells.len(), 12);
        assert!(high.cells.iter().any(|c| c.y < low.cells[0].y));
        assert!(high
            .cells
            .iter()
            .all(|c| matches!(c.cell.glyph, 'o' | 'O' | '@' | '*')));
    }

    #[test]
    fn implosion_sweeps_inward_and_spins() {
        let im = [e(Effect::Implosion {
            count: 8,
            radius: 4.0,
        })];
        // Lifetime 4: at age 0.0 radius 4.0, at age 3.0 radius 1.0.
        let early = layer("x", 0, &im, 1.0); // emitter start 1.0 → age 0
        let late = layer("x", 0, &im, 4.0); // age 3.0
        assert_eq!(early.cells.len(), 8);
        assert_eq!(late.cells.len(), 8);
        let span = |cells: &[AnimCell]| {
            cells
                .iter()
                .map(|c| {
                    let (dx, dy) = (c.x - 2, c.y - 3); // emitter origin (2,3)
                    (dx * dx + dy * dy) as f32
                })
                .fold(0.0f32, f32::max)
                .sqrt()
        };
        assert!(
            span(&late.cells) < span(&early.cells) * 0.4,
            "rings tighten"
        );
        // Spin + tighten: dash 0 starts east of the origin (age 0, angle 0°,
        // radius 4.0) and ends north of it (age 3.0, angle 90°, radius 1.0;
        // origin (2,3) - dash lands on (2,4)).
        assert!(
            late.cells.iter().any(|c| c.x == 2 && c.y == 4),
            "dash lands north of origin after the swirl"
        );
    }

    #[test]
    fn implosion_glyphs_read_as_dashes() {
        let im = [e(Effect::Implosion {
            count: 8,
            radius: 3.0,
        })];
        let mid = layer("x", 0, &im, 3.0);
        assert!(mid
            .cells
            .iter()
            .all(|c| c.cell.glyph == '-' || c.cell.glyph == '|'));
    }
}
