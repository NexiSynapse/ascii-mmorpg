//! Weather and environmental particles for the continuous canvas (issue #9).
//!
//! Composes world-anchored weather glyphs — grounded grass tufts whose tips
//! wave on one coherent gust field, directional rain streaks on seeded
//! non-linear fall paths, and clear-air dust motes — as placement records
//! in world space. Callers project each [`WeatherFx`] anchor through their
//! camera into a [`crate::ContinuousScene`] (see the `play` example's
//! `--continuous-weather` gate). Everything is a pure function of the mode,
//! density scalar, coverage window, and presentation time: positions are
//! world-locked (stable while the camera moves), motion is fractional, and
//! no gameplay or simulation state is read or written.

use crate::continuous::Point2;
use crate::raster::Vec3;
use crate::Rgb;

/// Upper bound on composed weather glyphs per call. Keeps presenter
/// instance work constant regardless of mode or window size.
pub const WEATHER_PRIMITIVE_BUDGET: usize = 512;

/// Weather identity: each mode has a distinct deterministic density and
/// motion profile (clear = motes + calm grass; rain = moderate streaks;
/// storm = dense fast streaks and strong gusts).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WeatherMode {
    Clear,
    Rain,
    Storm,
}

/// Composition inputs. `density` in `0.0..=1.0` scales effect counts
/// adaptively (presentation load control) without touching any gameplay
/// state; `road_half_width` excludes the kept-clear highway corridor from
/// grass placement (world-authoring concern, passed by the caller).
#[derive(Clone, Copy, Debug)]
pub struct WeatherParams {
    pub mode: WeatherMode,
    pub density: f32,
    pub road_half_width: f32,
}

impl WeatherParams {
    pub fn new(mode: WeatherMode) -> Self {
        Self {
            mode,
            density: 1.0,
            road_half_width: 0.0,
        }
    }

    pub const fn with_density(mut self, density: f32) -> Self {
        self.density = density;
        self
    }

    pub const fn with_road_half_width(mut self, width: f32) -> Self {
        self.road_half_width = width;
        self
    }
}

/// World-space coverage window for the weather lattice (inclusive cell
/// bounds on the even-locked grid).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CoverageWindow {
    pub x0: i32,
    pub x1: i32,
    pub z0: i32,
    pub z1: i32,
}

/// One composed weather glyph in world space. `seed` is the stable lattice
/// identity, so callers and tests can track the same tuft or drop across
/// presentation times.
#[derive(Clone, Copy, Debug)]
pub struct WeatherFx {
    pub anchor: Vec3,
    pub seed: u32,
    pub glyph: char,
    pub scale: Point2,
    pub rotation: f32,
    pub skew: Point2,
    pub color: Rgb,
    pub opacity: f32,
    pub kind: FxKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FxKind {
    /// Grounded tuft base — never moves.
    GrassBase,
    /// Waving tuft blade — leans with the coherent gust field.
    GrassTip,
    /// Falling streak — wind-driven non-linear path.
    Rain,
    /// Clear-air dust mote — slow drifting ember of sunlight.
    Mote,
    /// Translucent drifting fog band — atmosphere above the world, never
    /// emissive (review 2026-10-25 P1).
    FogBand,
}

/// Resolved motion profile for one mode at one time. The gust is shared by
/// grass and rain so wind reads as one weather (coherence contract).
#[derive(Clone, Copy, Debug)]
struct Profile {
    grass_amplitude: f32,
    grass_speed: f32,
    rain_density_pct: u32,
    rain_speed: f32,
    wind_x: f32,
    wind_z: f32,
    curl: f32,
}

fn profile(mode: WeatherMode, t: f32) -> Profile {
    let gust = (t * 0.82).sin() * 0.5 + (t * 1.37 + 1.4).sin() * 0.3;
    match mode {
        WeatherMode::Clear => Profile {
            grass_amplitude: 0.015,
            grass_speed: 1.2,
            rain_density_pct: 0,
            rain_speed: 0.0,
            wind_x: 0.0,
            wind_z: 0.0,
            curl: 0.0,
        },
        WeatherMode::Rain => Profile {
            grass_amplitude: 0.12 + gust.abs() * 0.05,
            grass_speed: 2.2 + gust.abs() * 0.4,
            rain_density_pct: 24,
            rain_speed: 1.45 + gust.abs() * 0.18,
            wind_x: 0.20 + gust * 0.08,
            wind_z: -0.08 + gust * 0.04,
            curl: 0.30,
        },
        WeatherMode::Storm => Profile {
            grass_amplitude: 0.22 + gust.abs() * 0.09,
            grass_speed: 3.8 + gust.abs() * 0.7,
            rain_density_pct: 42,
            rain_speed: 2.15 + gust.abs() * 0.28,
            wind_x: 0.34 + gust * 0.12,
            wind_z: -0.14 + gust * 0.06,
            curl: 0.55,
        },
    }
}

/// Deterministic lattice hash (same construction as the play example's
/// `scene_hash`): stable positive key for a world cell.
fn lattice_hash(x: i32, z: i32) -> u32 {
    let mut h = (x as u32).wrapping_mul(0x27d4_eb2d);
    h ^= (z as u32).wrapping_mul(0x1656_67b1);
    h ^ h.rotate_left(13)
}

/// Deterministic seeded unit noise in [0, 1) — the same hash construction
/// as `fx::hash_unit`/`character::unit`, normalized.
fn unit(seed: u32) -> f32 {
    let mut x = seed.wrapping_mul(0x9e37_79b9).wrapping_add(0x7f4a_7c15);
    x ^= x >> 16;
    x = x.wrapping_mul(0x85eb_ca6b);
    x ^= x >> 13;
    (x >> 8) as f32 / (1u32 << 24) as f32
}

/// The coherent gust field sample at a world position: one shared
/// travelling-wave phase (pure smooth spatial terms — no per-cell hash, or
/// neighbouring tufts would jitter independently) drives every grass tip,
/// so the wind reads as weather rolling across the ground.
pub fn grass_sway(params: &WeatherParams, x: f32, z: f32, t: f32) -> f32 {
    let prof = profile(params.mode, t);
    (t * prof.grass_speed + x * 0.37 + z * 0.19).sin()
}

/// Deterministic storm lightning envelope in 0..=1: bolts arrive on a
/// seeded schedule (short attack, exponential decay) and the value is zero
/// most of the time — a flash, not a strobe. Callers scale it into a cool
/// whole-frame wash (review 2026-10-25 P1).
pub fn lightning_flash(t: f32, seed: u32) -> f32 {
    if !t.is_finite() || t <= 0.0 {
        return 0.0;
    }
    const PERIOD: f32 = 7.3;
    let envelope = |k: i32| -> f32 {
        let bolt_t = k as f32 * PERIOD + unit(seed.wrapping_add((k as u32) * 101)) * PERIOD;
        let age = t - bolt_t;
        if age < 0.0 || age > 1.2 {
            0.0
        } else {
            // Instant attack, fast-then-slow decay.
            (-age * 6.5).exp()
        }
    };
    let k = (t / PERIOD).ceil() as i32;
    envelope(k - 1).max(envelope(k)).clamp(0.0, 1.0)
}

/// Compose every weather glyph for `window` at presentation time `t`.
/// Deterministic in its inputs; output order is stable (lattice scan) so
/// callers can rely on stable indices per frame.
pub fn weather_fx(params: &WeatherParams, window: CoverageWindow, t: f32) -> Vec<WeatherFx> {
    let density = if params.density.is_finite() {
        params.density.clamp(0.0, 1.0)
    } else {
        1.0
    };
    let prof = profile(params.mode, t);
    let mut out = Vec::new();

    // Fog canopy (review 2026-10-25 P1): sparse translucent bands drifting
    // slowly across the whole scene in Rain/Storm — cool, quiet atmosphere
    // above the world ink and below labels/UI. Emitted first so the budget
    // can never starve atmosphere behind grass/rain detail.
    if params.mode != WeatherMode::Clear {
        let band_count = if params.mode == WeatherMode::Storm {
            5
        } else {
            3
        };
        for i in 0..band_count {
            let h1 = unit(0xF06 + i as u32);
            let h2 = unit(0xF06 + i as u32 + 9);
            let band_z = window.z0 as f32
                + (i as f32 + 0.5) / band_count as f32 * (window.z1 - window.z0) as f32;
            let mid_x = (window.x0 as f32 + window.x1 as f32) * 0.5;
            let x = mid_x + (t * (0.35 + h2 * 0.25) + h1 * 40.0).sin() * 12.0;
            out.push(WeatherFx {
                anchor: Vec3::new(x, 2.2 + h2 * 1.6, band_z),
                seed: 0xF06 + i as u32,
                glyph: '~',
                scale: Point2::new(8.0, 1.4),
                rotation: 0.0,
                skew: Point2::ZERO,
                color: Rgb::new(168, 178, 196),
                opacity: 0.05 + 0.05 * h1,
                kind: FxKind::FogBand,
            });
        }
    }

    // Grass lattice: world-locked even cells, half populated, density
    // scaled. Bases never move; tips lean on the coherent gust field
    // (see `grass_sway`).
    let grass_gate = (50.0 * density) as u32;
    for gz in (window.z0..=window.z1).step_by(5) {
        if out.len() >= WEATHER_PRIMITIVE_BUDGET {
            break;
        }
        for gx in (window.x0..=window.x1).step_by(5) {
            if out.len() >= WEATHER_PRIMITIVE_BUDGET {
                break;
            }
            let key = lattice_hash(gx, gz + 91);
            if key % 100 >= grass_gate {
                continue;
            }
            let x = gx as f32 + unit(lattice_hash(gx, gz)) * 0.9;
            let z = gz as f32 + unit(lattice_hash(gx, gz + 3)) * 0.9;
            if x.abs() < params.road_half_width {
                continue;
            }
            let seed = lattice_hash(gx + 17, gz - 23);
            let sway = grass_sway(params, x, z, t);
            let height = 0.26 + unit(lattice_hash(gx, gz + 401)) * 0.22;
            let tint = 0.75 + unit(seed + 7) * 0.25;
            let grass = Rgb::new(
                (54.0 * tint) as u8,
                (112.0 * tint) as u8,
                (60.0 * tint) as u8,
            );
            out.push(WeatherFx {
                anchor: Vec3::new(x, 0.04, z),
                seed,
                glyph: '"',
                scale: Point2::new(0.55, 0.42),
                rotation: 0.0,
                skew: Point2::ZERO,
                color: grass,
                opacity: 0.95,
                kind: FxKind::GrassBase,
            });
            out.push(WeatherFx {
                anchor: Vec3::new(
                    x + sway * prof.grass_amplitude * 0.4,
                    0.04 + height,
                    z + grass_sway(params, x + 3.0, z, t) * prof.grass_amplitude * 0.2,
                ),
                seed,
                glyph: '\'',
                scale: Point2::new(0.34, 0.30 + height * 0.9),
                rotation: sway * prof.grass_amplitude * 2.0,
                skew: Point2::new(sway * prof.grass_amplitude * 3.0, 0.0),
                color: grass,
                opacity: 0.9,
                kind: FxKind::GrassTip,
            });
        }
    }

    // Rain lattice: same world-locked cells, deterministic density gate
    // scaled by both the mode profile and the density scalar. Each drop
    // falls on a fractional phase with a seeded depth (length/opacity/
    // height variation) and a NON-linear lateral path: constant wind drift
    // plus a seeded sine curl, so streaks swing through the fall instead
    // of sliding along a straight diagonal.
    let rain_gate = (prof.rain_density_pct as f32 * density) as u32;
    if rain_gate > 0 {
        for rz in (window.z0..=window.z1).step_by(5) {
            if out.len() >= WEATHER_PRIMITIVE_BUDGET {
                break;
            }
            for rx in (window.x0..=window.x1).step_by(5) {
                if out.len() >= WEATHER_PRIMITIVE_BUDGET {
                    break;
                }
                if lattice_hash(rx, rz + 701) % 100 >= rain_gate {
                    continue;
                }
                let seed = lattice_hash(rx + 31, rz - 47);
                let phase = unit(seed);
                let depth = unit(seed + 13);
                let fall = (t * prof.rain_speed + phase).fract();
                let base_x = rx as f32 + unit(lattice_hash(rx, rz + 703)) * 1.8;
                let base_z = rz as f32 + unit(lattice_hash(rx, rz + 709)) * 1.8;
                let top_y = 5.0 + depth * 2.6;
                let y = top_y - fall * 7.0;
                if y < -0.4 || y > 7.4 {
                    continue;
                }
                let drift = fall * 1.4;
                let curl_swing =
                    (fall * std::f32::consts::PI * (1.0 + depth)).sin() * prof.curl * 0.5;
                let x = base_x + prof.wind_x * drift + curl_swing;
                let z = base_z + prof.wind_z * drift + curl_swing * 0.4;
                let length = 0.58 + depth * 0.42;
                let lean = prof.wind_x * 0.55;
                let steel = 0.8 + depth * 0.25;
                out.push(WeatherFx {
                    anchor: Vec3::new(x, y, z),
                    seed,
                    glyph: '|',
                    scale: Point2::new(0.22 + depth * 0.1, length),
                    rotation: lean,
                    skew: Point2::ZERO,
                    color: Rgb::new(
                        (108.0 * steel) as u8,
                        (142.0 * steel) as u8,
                        (186.0 * steel) as u8,
                    ),
                    opacity: (0.45 + depth * 0.4) * density.max(0.35),
                    kind: FxKind::Rain,
                });
            }
        }
    }

    // Clear-air motes: sparse warm dust drifting on slow sine paths through
    // the light band above the ground — the clear mode's environmental
    // identity (no rain, calm grass, sun motes).
    if params.mode == WeatherMode::Clear {
        let mote_count = (24.0 * density) as usize; // already well inside the budget
        for i in 0..mote_count {
            let h1 = unit(0x50 + i as u32);
            let h2 = unit(0x50 + i as u32 + 41);
            let h3 = unit(0x50 + i as u32 + 83);
            let lifetime = 4.0 + h2 * 3.0;
            let age = (t * 0.5 + h1 * lifetime).rem_euclid(lifetime);
            let p = age / lifetime;
            let fade = (p * std::f32::consts::PI).sin();
            let x = window.x0 as f32
                + h1 * (window.x1 - window.x0) as f32
                + (t * 0.3 + h3 * 6.0).sin() * 0.6;
            let z = window.z0 as f32 + h2 * (window.z1 - window.z0) as f32;
            let y = 0.6 + h3 * 3.2 + (t * 0.45 + h1 * 4.0).sin() * 0.25;
            out.push(WeatherFx {
                anchor: Vec3::new(x, y, z),
                seed: 0x50 + i as u32,
                glyph: '\u{b7}',
                scale: Point2::new(0.22, 0.22),
                rotation: 0.0,
                skew: Point2::ZERO,
                color: Rgb::new(235, 214, 160),
                opacity: 0.28 * fade,
                kind: FxKind::Mote,
            });
        }
    }

    out.truncate(WEATHER_PRIMITIVE_BUDGET);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window() -> CoverageWindow {
        CoverageWindow {
            x0: -34,
            x1: 34,
            z0: -46,
            z1: 46,
        }
    }

    fn params(mode: WeatherMode) -> WeatherParams {
        WeatherParams::new(mode).with_road_half_width(4.4)
    }

    fn of_kind(fx: &[WeatherFx], kind: FxKind) -> Vec<&WeatherFx> {
        fx.iter().filter(|f| f.kind == kind).collect()
    }

    #[test]
    fn budget_is_bounded_for_every_mode_and_phase() {
        for mode in [WeatherMode::Clear, WeatherMode::Rain, WeatherMode::Storm] {
            for t in [0.0, 0.37, 1.11, 2.73, 4.2] {
                let fx = weather_fx(&params(mode), window(), t);
                assert!(
                    fx.len() <= WEATHER_PRIMITIVE_BUDGET,
                    "{mode:?} t={t}: {} exceeds budget",
                    fx.len()
                );
            }
        }
    }

    #[test]
    fn composition_is_deterministic() {
        for mode in [WeatherMode::Clear, WeatherMode::Rain, WeatherMode::Storm] {
            let a = weather_fx(&params(mode), window(), 1.7);
            let b = weather_fx(&params(mode), window(), 1.7);
            assert_eq!(a.len(), b.len());
            assert!(a.iter().zip(b.iter()).all(|(x, y)| {
                x.anchor.x == y.anchor.x
                    && x.anchor.y == y.anchor.y
                    && x.anchor.z == y.anchor.z
                    && x.seed == y.seed
                    && x.glyph == y.glyph
                    && x.scale == y.scale
                    && x.rotation == y.rotation
                    && x.skew == y.skew
                    && x.color == y.color
                    && x.opacity == y.opacity
                    && x.kind == y.kind
            }));
        }
    }

    /// AC #3: the three modes have distinct deterministic density and
    /// motion — clear has motes and no rain, storm out-numbers and
    /// out-leans rain, and clear grass barely moves while storm grass
    /// sways hard.
    #[test]
    fn modes_have_distinct_density_and_motion() {
        let clear = weather_fx(&params(WeatherMode::Clear), window(), 1.0);
        let rain = weather_fx(&params(WeatherMode::Rain), window(), 1.0);
        let storm = weather_fx(&params(WeatherMode::Storm), window(), 1.0);

        assert!(!of_kind(&clear, FxKind::Mote).is_empty());
        assert!(of_kind(&rain, FxKind::Mote).is_empty());
        assert!(of_kind(&clear, FxKind::Rain).is_empty());
        let rain_count = of_kind(&rain, FxKind::Rain).len();
        let storm_count = of_kind(&storm, FxKind::Rain).len();
        assert!(storm_count > rain_count, "{storm_count} <= {rain_count}");

        let clear_sway = weather_fx(&params(WeatherMode::Clear), window(), 0.9)
            .iter()
            .filter(|f| f.kind == FxKind::GrassTip)
            .map(|f| f.skew.x.abs())
            .fold(0.0_f32, f32::max);
        let storm_sway = weather_fx(&params(WeatherMode::Storm), window(), 0.9)
            .iter()
            .filter(|f| f.kind == FxKind::GrassTip)
            .map(|f| f.skew.x.abs())
            .fold(0.0_f32, f32::max);
        assert!(
            storm_sway > clear_sway * 4.0,
            "storm sway {storm_sway} must dominate clear sway {clear_sway}"
        );

        let storm_lean = of_kind(&storm, FxKind::Rain)
            .iter()
            .map(|f| f.rotation.abs())
            .fold(0.0_f32, f32::max);
        assert!(storm_lean > 0.0, "storm streaks must lean with the wind");
    }

    /// AC #1: bases stay grounded (fixed anchor, no skew/rotation at any
    /// time) while tips ride the coherent gust field — every tip's lean
    /// equals the shared `grass_sway` sample, and the field itself is
    /// smooth across space (bounded local change), so neighbouring tufts
    /// wave together.
    #[test]
    fn grass_bases_are_grounded_while_tips_wave_coherently() {
        let params = params(WeatherMode::Storm);
        let early = weather_fx(&params, window(), 0.4);
        let late = weather_fx(&params, window(), 1.6);
        let early_tips: Vec<_> = of_kind(&early, FxKind::GrassTip);
        assert!(!early_tips.is_empty());
        for f in of_kind(&early, FxKind::GrassBase) {
            assert_eq!(f.rotation, 0.0);
            assert_eq!(f.skew, Point2::ZERO);
        }
        // Tips move: track the same seed across times.
        let mut moved = 0;
        for tip in &early_tips {
            if let Some(late_tip) = late
                .iter()
                .find(|f| f.kind == FxKind::GrassTip && f.seed == tip.seed)
            {
                if (tip.skew.x - late_tip.skew.x).abs() > 1e-4 {
                    moved += 1;
                }
            }
        }
        assert!(moved > 0, "grass tips must wave over time");
        // Tips ride the shared field: skew equals the gust sample (taken at
        // the grounded BASE anchor — the tip itself is offset by its sway)
        // times the mode amplitude scale.
        let amp = profile(WeatherMode::Storm, 0.4).grass_amplitude;
        for tip in &early_tips {
            let base = early
                .iter()
                .find(|f| f.kind == FxKind::GrassBase && f.seed == tip.seed)
                .expect("tip without grounded base");
            let sway = grass_sway(&params, base.anchor.x, base.anchor.z, 0.4);
            let expected = sway * amp * 3.0;
            assert!(
                (tip.skew.x - expected).abs() < 1e-3,
                "tip skew {} does not ride the gust field ({expected})",
                tip.skew.x
            );
        }
        // Field smoothness (coherence): half a world unit apart, the gust
        // sample changes only slightly — wind, not per-blade jitter.
        for (x, z) in [(0.0, 0.0), (12.5, -8.25), (-30.0, 40.0)] {
            let a = grass_sway(&params, x, z, 0.9);
            let b = grass_sway(&params, x + 0.5, z + 0.5, 0.9);
            assert!(
                (a - b).abs() <= 0.45,
                "gust field not smooth at ({x},{z}): {a} vs {b}"
            );
        }
    }

    /// AC #2: rain has directional wind, depth variation, fractional
    /// motion, and non-linear paths — the lateral offset along one drop's
    /// fall is not monotonic (seeded sine curl over constant drift).
    #[test]
    fn rain_paths_are_fractional_directional_and_non_linear() {
        let fx = weather_fx(&params(WeatherMode::Storm), window(), 0.0);
        let drops = of_kind(&fx, FxKind::Rain);
        assert!(drops.len() > 10, "storm must field a real rain count");

        // Depth variety: seeded scales and opacities differ across drops.
        let scales: std::collections::BTreeSet<i64> = drops
            .iter()
            .map(|d| (d.scale.y * 100.0).round() as i64)
            .collect();
        assert!(scales.len() >= 5, "rain needs depth-driven length variety");

        // Track one seed across times; its anchor must move fractionally
        // (sub-cell) and swing laterally, not just translate. Probe until a
        // drop survives all three samples without a fall-cycle wrap.
        let sample = |t: f32, seed: u32| {
            weather_fx(&params(WeatherMode::Storm), window(), t)
                .into_iter()
                .find(|f| f.kind == FxKind::Rain && f.seed == seed)
        };
        let mut probed = None;
        for d in &drops {
            let (Some(a), Some(b), Some(c)) = (
                sample(0.0, d.seed),
                sample(0.2, d.seed),
                sample(0.4, d.seed),
            ) else {
                continue;
            };
            // Monotonic fall without a fract-cycle wrap (a wrap jumps the
            // anchor back UP by the 7-unit fall span).
            if a.anchor.y > b.anchor.y && b.anchor.y > c.anchor.y && (a.anchor.y - c.anchor.y) < 7.0
            {
                probed = Some((a, b, c));
                break;
            }
        }
        let (a, b, c) = probed.expect("no storm drop survived three samples without wrapping");
        assert!(
            (a.anchor.y - b.anchor.y).abs() > 0.0,
            "drops must fall fractionally"
        );
        // Non-linearity: constant-wind fall would make lateral deltas
        // proportional to height deltas; the curl breaks proportionality.
        let dy_ab = (a.anchor.y - b.anchor.y).abs();
        let dy_bc = (b.anchor.y - c.anchor.y).abs();
        let dx_ab = (a.anchor.x - b.anchor.x).abs();
        let dx_bc = (b.anchor.x - c.anchor.x).abs();
        let ratio_ab = if dy_ab > 1e-6 { dx_ab / dy_ab } else { 0.0 };
        let ratio_bc = if dy_bc > 1e-6 { dx_bc / dy_bc } else { 0.0 };
        assert!(
            (ratio_ab - ratio_bc).abs() > 1e-3,
            "lateral motion is linear (no curl): {ratio_ab} vs {ratio_bc}"
        );
    }

    /// Fog canopy + lightning (review 2026-10-25 P1): fog bands field in
    /// rain/storm (never clear, always quiet translucent, never emissive
    /// by construction), and the storm lightning envelope flashes briefly
    /// on a deterministic schedule.
    #[test]
    fn fog_bands_are_quiet_atmosphere_for_rain_and_storm_only() {
        for mode in [WeatherMode::Rain, WeatherMode::Storm] {
            let fx = weather_fx(&params(mode), window(), 1.0);
            let fog = of_kind(&fx, FxKind::FogBand);
            assert!(!fog.is_empty(), "{mode:?} must field fog bands");
            assert!(fog.iter().all(|f| f.opacity <= 0.15 && f.scale.x >= 6.0));
        }
        let clear = weather_fx(&params(WeatherMode::Clear), window(), 1.0);
        assert!(
            of_kind(&clear, FxKind::FogBand).is_empty(),
            "Clear mode must not field fog"
        );
    }

    #[test]
    fn lightning_flash_is_deterministic_brief_and_rare() {
        for t in [0.0, 3.7, 9.1, 21.4] {
            assert_eq!(lightning_flash(t, 0x51), lightning_flash(t, 0x51));
        }
        // Rare: most sampled instants are dark.
        let lit = (0..400)
            .filter(|&i| lightning_flash(i as f32 * 0.1, 0x51) > 0.05)
            .count();
        assert!(lit < 60, "lightning lit {lit}/400 samples — too frequent");
        // Brief: half a second after a lit instant it has mostly decayed,
        // and the envelope never exceeds 1.
        let mut peak = 0.0_f32;
        for i in 0..2000 {
            let v = lightning_flash(i as f32 * 0.01, 0x51);
            peak = peak.max(v);
            if v > 0.5 {
                let later = lightning_flash(i as f32 * 0.01 + 0.5, 0x51);
                assert!(later < 0.1, "flash must decay fast ({later})");
            }
        }
        assert!(peak > 0.5, "schedule must produce real bolts (peak {peak})");
        assert!(peak <= 1.0);
    }

    /// AC #4: the density scalar reduces effect counts without touching
    /// anything else — positions and seeds of surviving effects match the
    /// full-density set.
    #[test]
    fn density_scales_counts_without_changing_survivors() {
        let full = weather_fx(&params(WeatherMode::Storm), window(), 1.3);
        let half = weather_fx(
            &WeatherParams::new(WeatherMode::Storm)
                .with_density(0.5)
                .with_road_half_width(4.4),
            window(),
            1.3,
        );
        let full_rain = of_kind(&full, FxKind::Rain).len();
        let half_rain = of_kind(&half, FxKind::Rain).len();
        assert!(half_rain > 0, "half density keeps some rain");
        assert!(half_rain < full_rain, "{half_rain} !< {full_rain}");
        // Survivors are a subset: same seed -> same placement.
        for d in of_kind(&half, FxKind::Rain) {
            assert!(
                full.iter()
                    .any(|f| f.seed == d.seed && f.kind == FxKind::Rain),
                "density changed a survivor's identity"
            );
        }
    }
}
