//! Isolated torch/sun showcase for the continuous primitive + float bloom
//! pipeline (issue #5): a multi-glyph flame and a radiant sun on a black
//! void, with seeded curved particles and varied rays. The showcase opts
//! into bloom-owned world lighting — it is the standalone acceptance gate
//! the in-world torch migration waits on.
//!
//! Run: `cargo run --locked -p ascii-mmorpg-engine --features present
//!       --release --example torch`
//! Flags: `--frames N` exits after N presented frames; `--check` runs the
//! deterministic headless showcase contracts.

#[cfg(not(feature = "present"))]
fn main() {
    eprintln!("this example needs `--features present` (winit + wgpu + fontdue)");
    std::process::exit(2);
}

#[cfg(feature = "present")]
fn main() {
    if let Err(err) = real_main() {
        eprintln!("torch failed: {err}");
        std::process::exit(1);
    }
}

#[cfg(feature = "present")]
fn real_main() -> Result<(), String> {
    use ascii_mmorpg_engine::present::{PresentOptions, SceneSource};
    use ascii_mmorpg_engine::{Camera, Layer, Partition, Rgb, Scene};

    const W: i32 = 72;
    const H: i32 = 26;

    struct TorchSunShow;

    impl TorchSunShow {
        /// Composed-frame captions only. The showcase void is pure black and
        /// carries no legacy glow: every light in the scene is a continuous
        /// primitive emission resolved by the float bloom field.
        fn captions(&self) -> Scene {
            let dim = Rgb::new(96, 104, 122);
            let void = Rgb::new(0, 0, 0);
            let mut scene = Scene::default();
            let mut ui = Layer::new("captions", 0);
            ui.push_text(21, 1, "CONTINUOUS TORCH + SUN SHOWCASE", dim, void);
            ui.push_text(15, 24, "TORCH", dim, void);
            ui.push_text(50, 24, "SUN", dim, void);
            ui.push_text(58, 24, "Q / ESC QUITS", dim, void);
            scene.layers.push(ui);
            scene
                .partitions
                .push(Partition::new("ui", 0, 0, W, H, vec![0], false));
            scene
        }
    }

    impl SceneSource for TorchSunShow {
        fn scene_at(&mut self, _t: f32) -> (Scene, Camera) {
            (self.captions(), Camera::default())
        }

        fn continuous_scene(&mut self, t: f32) -> Option<ascii_mmorpg_engine::ContinuousScene> {
            Some(showcase::showcase_scene(t))
        }

        /// The showcase exists to prove float bloom can own world lighting;
        /// opting in swaps the legacy CPU glow pass for the emission field.
        fn continuous_bloom_owns_world_lighting(&mut self) -> bool {
            true
        }
    }

    fn run_check() -> Result<(), String> {
        use ascii_mmorpg_engine::{
            EmissionSource, GradientAxis, PrimitiveKind, MAX_NATIVE_PRIMITIVES, MAX_SOURCE_COUNT,
        };

        // Determinism: identical scene inputs must sample identically.
        for t in [0.0, 0.5, 1.37, 3.9] {
            let (a, b) = (showcase::showcase_scene(t), showcase::showcase_scene(t));
            if a.primitives() != b.primitives() || a.sample(t) != b.sample(t) {
                return Err(format!(
                    "check: showcase sampling is not deterministic at t={t}"
                ));
            }
        }

        // Budgets: the presenter's per-frame instance and emission work stays
        // capped at every phase of the loops.
        for t in [0.0, 0.37, 1.11, 2.73, 4.2] {
            let scene = showcase::showcase_scene(t);
            let sampled = scene.sample(t);
            if sampled.len() > MAX_NATIVE_PRIMITIVES {
                return Err(format!(
                    "check: {t}: {} primitives exceed the native cap",
                    sampled.len()
                ));
            }
            let sources = EmissionSource::from_continuous_scene(&scene, t);
            if sources.len() > MAX_SOURCE_COUNT {
                return Err(format!(
                    "check: {t}: {} emission sources exceed the field cap",
                    sources.len()
                ));
            }
        }

        let scene = showcase::showcase_scene(1.2);
        let authored = scene.primitives();

        // Flame body: the shared ASCII row builder — plain punctuation
        // rows near the torch anchor (no unicode shapes), with the hot
        // base rows feeding the bloom field.
        let near_torch: Vec<_> = authored
            .iter()
            .filter(|p| {
                (p.transform.position.x - showcase::TORCH_X).abs() < 4.0
                    && (p.transform.position.y - showcase::TORCH_FLAME_Y).abs() < 3.2
            })
            .collect();
        let flame_rows: Vec<_> = near_torch
            .iter()
            .filter(|p| match p.kind {
                PrimitiveKind::Glyph { glyph } => glyph.is_ascii_punctuation(),
                _ => false,
            })
            .collect();
        if flame_rows.len() < 6 {
            return Err(format!(
                "check: flame body has only {} ASCII rows (shared builder missing?)",
                flame_rows.len()
            ));
        }
        if flame_rows
            .iter()
            .filter(|p| p.material.emission.is_some())
            .count()
            < 3
        {
            return Err("check: flame base rows carry no emission".into());
        }
        let radial = authored
            .iter()
            .filter(|p| {
                p.material
                    .gradient
                    .is_some_and(|g| g.axis == GradientAxis::Radial)
            })
            .count();
        let vertical = authored
            .iter()
            .filter(|p| {
                p.material
                    .gradient
                    .is_some_and(|g| g.axis == GradientAxis::Vertical)
            })
            .count();
        if radial < 2 || vertical < 2 {
            return Err(format!(
                "check: cores must mix radial ({radial}) and vertical ({vertical}) gradients"
            ));
        }
        let strong = authored
            .iter()
            .filter(|p| {
                p.material
                    .emission
                    .is_some_and(|e| e.strength >= 0.5 && e.radius >= 3.0)
            })
            .count();
        if strong < 4 {
            return Err(format!(
                "check: only {strong} core emitters reach bloom strength"
            ));
        }

        // Rays: length, thickness, opacity, scale, and direction all vary.
        let rays: Vec<_> = authored
            .iter()
            .filter(|p| matches!(p.kind, PrimitiveKind::Ray { .. }))
            .collect();
        if rays.len() < 20 {
            return Err(format!("check: only {} rays in the corona", rays.len()));
        }
        let mut lens = std::collections::BTreeSet::new();
        let mut widths = std::collections::BTreeSet::new();
        let mut rotations = std::collections::BTreeSet::new();
        let mut opacities = std::collections::BTreeSet::new();
        let mut scales = std::collections::BTreeSet::new();
        for ray in &rays {
            if let PrimitiveKind::Ray { length, width, .. } = ray.kind {
                lens.insert((length * 10.0).round() as i64);
                widths.insert((width * 100.0).round() as i64);
            }
            rotations.insert((ray.transform.rotation * 100.0).round() as i64);
            opacities.insert((ray.material.opacity * 100.0).round() as i64);
            scales.insert((ray.transform.scale.y * 20.0).round() as i64);
        }
        if lens.len() < 6
            || widths.len() < 5
            || rotations.len() < 12
            || opacities.len() < 6
            || scales.len() < 6
        {
            return Err(format!(
                "check: ray variety too low (len {}, width {}, rot {}, opacity {}, scale {})",
                lens.len(),
                widths.len(),
                rotations.len(),
                opacities.len(),
                scales.len()
            ));
        }

        // Particles: mixed glyphs/sizes, and the sampled animation actually
        // crosses rows, shrinks, fades, and rotates over time.
        let particles: Vec<_> = authored
            .iter()
            .filter(|p| matches!(p.kind, PrimitiveKind::Particle { .. }))
            .collect();
        if particles.len() < 20 {
            return Err(format!(
                "check: only {} showcase particles",
                particles.len()
            ));
        }
        let glyphs: std::collections::HashSet<_> = particles
            .iter()
            .filter_map(|p| match p.kind {
                PrimitiveKind::Particle { glyph } => Some(glyph),
                _ => None,
            })
            .collect();
        if glyphs.len() < 4 {
            return Err(format!(
                "check: only {} distinct particle glyphs",
                glyphs.len()
            ));
        }
        let early: Vec<_> = showcase::showcase_scene(0.5)
            .sample(0.5)
            .into_iter()
            .filter(|p| matches!(p.kind, PrimitiveKind::Particle { .. }))
            .collect();
        let late: Vec<_> = showcase::showcase_scene(1.6)
            .sample(1.6)
            .into_iter()
            .filter(|p| matches!(p.kind, PrimitiveKind::Particle { .. }))
            .collect();
        let (mut row_crossings, mut fades, mut shrinks, mut spins) = (0, 0, 0, 0);
        for (a, b) in early.iter().zip(late.iter()) {
            if (a.transform.position.y - b.transform.position.y).abs() >= 1.0 {
                row_crossings += 1;
            }
            if (a.material.opacity - b.material.opacity).abs() >= 0.2 {
                fades += 1;
            }
            if (a.transform.scale.y - b.transform.scale.y).abs() >= 0.2 {
                shrinks += 1;
            }
            if (a.transform.rotation - b.transform.rotation).abs() >= 0.2 {
                spins += 1;
            }
        }
        if row_crossings == 0 || fades == 0 || shrinks == 0 || spins == 0 {
            return Err(format!(
                "check: particle motion too weak (rows {row_crossings}, fades {fades}, shrinks {shrinks}, spins {spins})"
            ));
        }

        // Bloom readable over a dark void: enough live sources, at least one
        // sun-scale emitter, and zero legacy glow in the composed frame.
        let sources = EmissionSource::from_continuous_scene(&scene, 1.2);
        if sources.len() < 10 {
            return Err(format!(
                "check: only {} emission sources — bloom would be unreadable",
                sources.len()
            ));
        }
        if !sources.iter().any(|s| s.strength >= 1.0 && s.radius >= 8.0) {
            return Err("check: no sun-scale emitter reaches core bloom strength".into());
        }
        let mut show = TorchSunShow;
        let (captions, camera) = show.scene_at(0.0);
        let frame = ascii_mmorpg_engine::compose(&captions, &camera, 0.0);
        let glowing = frame
            .cells
            .iter()
            .filter(|cell| !cell.transparent && cell.glow > 0.0)
            .count();
        if glowing != 0 {
            return Err(format!(
                "check: {glowing} composed cells carry legacy glow alongside bloom ownership"
            ));
        }

        // 60 FPS smoke (deterministic part): per-frame CPU cost is a bounded,
        // constant primitive/source count; the timing loop is reported for
        // visibility without asserting wall-clock time (CI variance).
        let start = std::time::Instant::now();
        let mut peak = 0usize;
        let mut built = 0usize;
        let mut i = 0u32;
        while i < 240 {
            let t = i as f32 / 60.0;
            let scene = showcase::showcase_scene(t);
            let sampled = scene.sample(t);
            peak = peak.max(sampled.len());
            let _ = EmissionSource::from_continuous_scene(&scene, t);
            built += 1;
            i += 4;
        }
        let per_frame_us = start.elapsed().as_micros() as f64 / built as f64;
        eprintln!(
            "torch check ok: {built} frames built+sampled in {:?} (peak {peak} primitives, {per_frame_us:.1} µs/frame), bloom sources {}",
            start.elapsed(),
            sources.len()
        );
        Ok(())
    }

    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|arg| arg == "--check") {
        run_check()?;
        return Ok(());
    }
    let frames = args
        .iter()
        .position(|arg| arg == "--frames")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse::<u64>().ok());
    match ascii_mmorpg_engine::present::run_dynamic(
        TorchSunShow,
        PresentOptions {
            title: "ASCII Engine — torch & sun bloom showcase".into(),
            width_cells: W as u32,
            height_cells: H as u32,
            max_frames: frames,
            ..Default::default()
        },
    ) {
        Ok(presented) => {
            eprintln!("presented {presented} frames");
            Ok(())
        }
        Err(err) => Err(err),
    }
}

#[cfg(feature = "present")]
mod showcase {
    use ascii_mmorpg_engine::{
        ColorGradient, ContinuousMaterial, ContinuousScene, Emission, GradientAxis, Point2,
        Primitive, PrimitiveKind, Rgb, Transform2,
    };

    pub const TORCH_X: f32 = 18.0;
    pub const TORCH_FLAME_Y: f32 = 18.4;
    const SUN: Point2 = Point2::new(52.0, 10.4);

    const EMBER_COUNT: usize = 12;
    const SMOKE_COUNT: usize = 5;
    const FLARE_COUNT: usize = 10;
    const OUTER_RAYS: usize = 16;
    const INNER_RAYS: usize = 6;

    const EMBER_GLYPHS: [char; 5] = ['*', '+', '\u{b7}', '.', 'o'];
    const FLARE_GLYPHS: [char; 5] = ['*', 'o', '+', '\u{b7}', '.'];

    /// Deterministic seeded unit noise in [0, 1). Same hash construction as
    /// `fx::hash_unit` (private there), normalized to a unit range.
    fn unit(seed: u32) -> f32 {
        let mut x = seed.wrapping_mul(0x9e37_79b9).wrapping_add(0x7f4a_7c15);
        x ^= x >> 16;
        x = x.wrapping_mul(0x85eb_ca6b);
        x ^= x >> 13;
        (x >> 8) as f32 / (1u32 << 24) as f32
    }

    fn lerp(a: Rgb, b: Rgb, t: f32) -> Rgb {
        let ch = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t) as u8;
        Rgb::new(ch(a.r, b.r), ch(a.g, b.g), ch(a.b, b.b))
    }

    /// Build the whole showcase at presentation time `t`. Every channel is a
    /// pure function of `t` and a fixed per-index seed, so the presenter's
    /// single `sample(t)` (and the emission derivation behind it) is
    /// deterministic and loopable.
    pub fn showcase_scene(t: f32) -> ContinuousScene {
        let mut scene = ContinuousScene::new();
        push_torch(&mut scene, t);
        push_sun(&mut scene, t);
        scene
    }

    fn push_torch(scene: &mut ContinuousScene, t: f32) {
        // Wooden stick and iron bowl: dark, non-emissive anchors.
        for y in [19.7, 20.7] {
            scene.push(Primitive::new(
                PrimitiveKind::glyph('\u{2502}'),
                Transform2::new(Point2::new(TORCH_X, y), -1.0),
                ContinuousMaterial::new(Rgb::new(104, 70, 36)),
            ));
        }
        scene.push(Primitive::new(
            PrimitiveKind::glyph('\u{25a0}'),
            Transform2::new(Point2::new(TORCH_X, 19.05), -1.0).with_scale(Point2::new(1.3, 0.6)),
            ContinuousMaterial::new(Rgb::new(70, 66, 74)),
        ));

        let pulse = 0.5 + 0.5 * (t * 6.1 + 0.8 * (t * 2.3).sin()).sin();
        let sway = (t * 3.05).sin();

        // Flame body: the SHARED ASCII row builder (review 2026-10-25 P1 —
        // the showcase and the in-world play torch must be one function so
        // the accepted read cannot drift): plain punctuation rows, vivid
        // animated ramp, emissive base rows.
        ascii_mmorpg_engine::flame::push_ascii_flame(
            scene,
            Point2::new(TORCH_X, TORCH_FLAME_Y + 1.0),
            ascii_mmorpg_engine::flame::FlamePulse {
                light_gain: 0.84 + 0.18 * pulse,
                flame_scale: 0.94 + 0.14 * pulse,
                radius_scale: 0.92 + 0.12 * pulse,
                sway: sway * 0.05,
            },
            0.0,
            t,
        );

        // Embers: seeded curved rise — lateral sine arcs, per-index lifetimes,
        // shrinking/fading/rotating, mixing glyphs and sizes, crossing many
        // logical rows per cycle. Weak individual emissions trail the core.
        for i in 0..EMBER_COUNT {
            let h1 = unit(0x70C1 + i as u32);
            let h2 = unit(0x70C1 + i as u32 + 101);
            let h3 = unit(0x70C1 + i as u32 + 211);
            let lifetime = 1.9 + h2 * 1.3;
            let age = (t + h1 * lifetime).rem_euclid(lifetime);
            let p = age / lifetime;
            let fade = (p * std::f32::consts::PI).sin();
            let dir = if i % 2 == 0 { 1.0 } else { -1.0 } * (0.5 + h3);
            let arc = (p * std::f32::consts::PI * (1.0 + h3 * 0.8)).sin() * (0.9 + h2) * dir;
            let x = TORCH_X + arc + (h3 - 0.5) * 0.6;
            let y = TORCH_FLAME_Y - 0.4 - p * (5.5 + h2 * 3.0);
            let s = (0.85 + h2 * 0.5) * (1.0 - 0.62 * p);
            let color = lerp(Rgb::new(255, 196, 90), Rgb::new(255, 84, 28), p);
            scene.push(Primitive::new(
                PrimitiveKind::particle(EMBER_GLYPHS[i % EMBER_GLYPHS.len()]),
                Transform2::new(Point2::new(x, y), 0.4)
                    .with_scale(Point2::new(s, s))
                    .with_rotation(p * (h1 - 0.5) * 7.0 + t * 0.4 * dir),
                ContinuousMaterial::new(color)
                    .with_opacity(fade * (0.62 + 0.35 * h1))
                    .with_emission(Emission::new(color, 0.12 * fade, 1.3)),
            ));
        }

        // Smoke: sparse shading-block puffs above the embers, wide and faint,
        // no emission — the void must stay dark above the flame.
        for i in 0..SMOKE_COUNT {
            let h1 = unit(0x5A0 + i as u32);
            let h2 = unit(0x5A0 + i as u32 + 37);
            let lifetime = 3.4 + h2 * 1.6;
            let age = (t * 0.8 + h1 * lifetime).rem_euclid(lifetime);
            let p = age / lifetime;
            let fade = (p * std::f32::consts::PI).sin();
            let swing = if i % 2 == 0 { 1.0 } else { -1.0 };
            let x = TORCH_X + (p * std::f32::consts::PI).sin() * (0.8 + h2) * swing;
            let y = TORCH_FLAME_Y - 5.6 - p * 8.0;
            let grow = 1.1 + p * 1.3;
            scene.push(Primitive::new(
                PrimitiveKind::particle('\u{2591}'),
                Transform2::new(Point2::new(x, y), -0.5)
                    .with_scale(Point2::new(grow, grow * 0.8))
                    .with_rotation((h1 - 0.5) * p * 2.0),
                ContinuousMaterial::new(Rgb::new(118, 126, 138)).with_opacity(0.05 + 0.10 * fade),
            ));
        }
    }

    fn push_sun(scene: &mut ContinuousScene, t: f32) {
        let breathe = 0.5 + 0.5 * (t * 2.2).sin();
        let drift = t * 0.05;

        // Core: one large white-hot radial block plus four radial petals in a
        // 2:1-compensated ring, breathing together. The centre carries the
        // strongest bloom source in the showcase.
        let core_scale = 3.1 + 0.2 * breathe;
        scene.push(Primitive::new(
            PrimitiveKind::glyph('\u{25a0}'),
            Transform2::new(SUN, 2.0).with_scale(Point2::new(core_scale * 0.62, core_scale)),
            ContinuousMaterial::new(Rgb::new(255, 236, 170))
                .with_gradient(ColorGradient::new(
                    Rgb::new(255, 252, 234),
                    Rgb::new(255, 156, 44),
                    GradientAxis::Radial,
                ))
                .with_emission(Emission::new(Rgb::new(255, 214, 130), 1.35, 9.5)),
        ));
        for (i, (ox, oy)) in [(1.9, 0.0), (-1.9, 0.0), (0.0, 1.4), (0.0, -1.4)]
            .into_iter()
            .enumerate()
        {
            let h = unit(0x519 + i as u32);
            let petal = 1.5 + 0.25 * (t * 2.6 + h * 6.0).sin();
            scene.push(Primitive::new(
                PrimitiveKind::glyph('O'),
                Transform2::new(
                    Point2::new(SUN.x + ox * (1.0 + 0.05 * breathe), SUN.y + oy),
                    1.5,
                )
                .with_scale(Point2::new(petal * 0.62, petal)),
                ContinuousMaterial::new(Rgb::new(255, 190, 96))
                    .with_gradient(ColorGradient::new(
                        Rgb::new(255, 244, 200),
                        Rgb::new(255, 128, 30),
                        GradientAxis::Radial,
                    ))
                    .with_opacity(0.88)
                    .with_emission(Emission::new(Rgb::new(255, 170, 70), 0.55, 6.2)),
            ));
        }

        // Corona: two seeded ray rings. Length, width, tip width, opacity,
        // scale, and direction all vary per ray and pulse over time; the
        // whole ring slowly rotates. Ray geometry rides the seam as authored
        // data; until the dedicated ray renderer lands, glyph scale tracks
        // the authored length so the variety stays visible.
        for (ring, count) in [(0, OUTER_RAYS), (1, INNER_RAYS)] {
            let base_len = if ring == 0 { 3.4 } else { 1.7 };
            let base_dist = if ring == 0 { 5.4 } else { 3.6 };
            for i in 0..count {
                let s = (ring * OUTER_RAYS + i) as u32;
                let angle = i as f32 * std::f32::consts::TAU / count as f32
                    + ring as f32 * 0.22
                    + 0.10 * (t * 0.8 + i as f32 * 1.3).sin()
                    + drift;
                let len = base_len + 2.4 * unit(0x9 + s) + 0.8 * (t * 1.5 + i as f32 * 2.4).sin();
                let width = 0.05 + 0.09 * unit(0x9 + s + 31);
                let tip = 0.02 + 0.05 * unit(0x9 + s + 77);
                let dir = Point2::new(angle.cos(), angle.sin());
                let dist = base_dist + 0.4 * (t * 2.0 + i as f32).sin() + len * 0.18;
                let opacity = ((0.35 + 0.5 * unit(0x9 + s + 5))
                    * (0.75 + 0.25 * (t * 1.9 + i as f32).sin()))
                .clamp(0.0, 1.0);
                let scale_y = (0.55 * len).clamp(0.4, 2.6);
                let scale_x = (0.45 + width * 3.0).clamp(0.3, 1.4);
                scene.push(Primitive::new(
                    PrimitiveKind::ray(len, width, tip),
                    Transform2::new(
                        Point2::new(SUN.x + dir.x * dist, SUN.y + dir.y * dist * 0.62),
                        0.8 + unit(0x9 + s + 3) * 0.4,
                    )
                    .with_scale(Point2::new(scale_x, scale_y))
                    .with_rotation(angle + std::f32::consts::FRAC_PI_2),
                    ContinuousMaterial::new(Rgb::new(255, 176, 60))
                        .with_gradient(ColorGradient::new(
                            Rgb::new(255, 246, 214),
                            Rgb::new(255, 132, 26),
                            GradientAxis::Vertical,
                        ))
                        .with_opacity(opacity),
                ));
            }
        }

        // Flares: seeded spiral ejecta — curved randomized paths outward from
        // the core, shrinking and fading with mixed glyphs/sizes; the inner
        // four keep a small emission so sparks read inside the bloom wash.
        for i in 0..FLARE_COUNT {
            let h1 = unit(0xF1A + i as u32);
            let h2 = unit(0xF1A + i as u32 + 53);
            let h3 = unit(0xF1A + i as u32 + 97);
            let lifetime = 1.6 + h2 * 1.4;
            let age = (t * 1.1 + h1 * lifetime).rem_euclid(lifetime);
            let p = age / lifetime;
            let fade = (p * std::f32::consts::PI).sin();
            let spin_dir = if i % 2 == 0 { 1.0 } else { -1.0 };
            let theta =
                h1 * std::f32::consts::TAU + p * (0.7 + h3) * spin_dir * std::f32::consts::TAU;
            let reach = 1.2 + p * (4.6 + 3.2 * h2);
            let x = SUN.x + theta.cos() * reach;
            let y = SUN.y + theta.sin() * reach * 0.62 - p * 1.1;
            let s = (1.05 + h2 * 0.5) * (1.0 - 0.72 * p);
            let color = lerp(Rgb::new(255, 248, 214), Rgb::new(255, 150, 44), p);
            let mut material = ContinuousMaterial::new(color).with_opacity(fade * (0.7 + 0.3 * h1));
            if i < 4 {
                material = material.with_emission(Emission::new(color, 0.13 * fade, 1.5));
            }
            scene.push(Primitive::new(
                PrimitiveKind::particle(FLARE_GLYPHS[i % FLARE_GLYPHS.len()]),
                Transform2::new(Point2::new(x, y), 0.6 + h3)
                    .with_scale(Point2::new(s, s))
                    .with_rotation(theta + p * 2.4 * spin_dir),
                material,
            ));
        }
    }
}
