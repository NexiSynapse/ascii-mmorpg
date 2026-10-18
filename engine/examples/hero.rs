//! Sparse-hero showcase: a contour-built avatar (few bright cells, dark
//! interior, like the reference Senje) patrols, turns via mirrored art, and
//! casts a chain-lightning bolt on a timer.
//!
//! Run: `cargo run -p ascii-mmorpg-engine --features present --example hero`
//! Flags: `--frames N` exits after N presented frames (automated proof);
//! `--check` runs headless walk/cast assertions (no window).

#[cfg(not(feature = "present"))]
fn main() {
    eprintln!("this example needs `--features present` (winit + wgpu + fontdue)");
    std::process::exit(2);
}

#[cfg(feature = "present")]
fn main() {
    if let Err(err) = real_main() {
        eprintln!("hero failed: {err}");
        std::process::exit(1);
    }
}

#[cfg(feature = "present")]
fn real_main() -> Result<(), String> {
    use ascii_mmorpg_engine::overlay::{self, Stroke};
    use ascii_mmorpg_engine::present::{PresentOptions, SceneSource};
    use ascii_mmorpg_engine::sprite::{self, SpriteDraw};
    use ascii_mmorpg_engine::{Anim, AnimCell, Camera, Cell, Layer, Partition, Rgb, Scene, Track};

    const HERO: &str = include_str!("../assets/actors/hero.txt");
    const BG: Rgb = Rgb::new(10, 14, 26);
    const W: i32 = 72;
    const H: i32 = 26;
    const FEET_Y: i32 = 22;
    const LOOP: f32 = 12.0;
    const WALK_FPS: f32 = 8.0;

    #[derive(Clone, Copy, PartialEq)]
    enum Mode {
        Walk,
        Cast,
        Stand,
    }

    struct HeroShow {
        walk_right: Vec<Vec<String>>,
        walk_left: Vec<Vec<String>>,
        cast: Vec<Vec<String>>,
        stand: Vec<Vec<String>>,
    }

    impl HeroShow {
        fn new() -> Self {
            let frames = sprite::parse_frames(HERO);
            let left = sprite::mirror_frames(&frames);
            Self {
                walk_right: frames[0..4].to_vec(),
                walk_left: left[0..4].to_vec(),
                cast: frames[4..5].to_vec(),
                stand: frames[0..1].to_vec(),
            }
        }

        /// (hero-x, frames, mode, facing-right) at time `t`.
        fn pose_at(&self, t: f32) -> (f32, &[Vec<String>], Mode, bool) {
            let phase = t.rem_euclid(LOOP);
            if phase < 4.5 {
                (8.0 + phase * 6.4, &self.walk_right, Mode::Walk, true)
            } else if phase < 6.0 {
                (36.8, &self.cast, Mode::Cast, true)
            } else if phase < 6.8 {
                (36.8, &self.stand, Mode::Stand, false)
            } else if phase < 11.3 {
                (
                    36.8 - (phase - 6.8) * 6.4,
                    &self.walk_left,
                    Mode::Walk,
                    false,
                )
            } else {
                (8.0, &self.stand, Mode::Stand, true)
            }
        }

        fn build(&self, t: f32) -> (Scene, Camera) {
            let (hx, frames, mode, _facing_right) = self.pose_at(t);
            let mut scene = Scene::default();

            // Dark arena band with a stone floor line.
            let mut ground = Layer::new("ground", 0);
            ground.fill_rect(0, 20, W, 6, Cell::new(' ', Rgb::new(0, 0, 0), BG));
            for x in 0..W {
                ground.push(AnimCell::still(
                    x,
                    20,
                    Cell::new('=', Rgb::new(70, 72, 84), BG),
                ));
            }
            scene.layers.push(ground);

            // Torch sconce, warm halo.
            let mut props = Layer::new("props", 1);
            props.push(AnimCell::still(
                58,
                19,
                Cell::new('|', Rgb::new(120, 80, 40), BG),
            ));
            props.push(AnimCell::animated(
                58,
                18,
                Cell::new('*', Rgb::new(255, 170, 60), Rgb::new(26, 14, 6)).with_glow(1.0),
                Anim::still()
                    .with(Track::Pulse {
                        low: 0.55,
                        high: 1.3,
                        period: 1.1,
                        phase: 0.0,
                    })
                    .with(Track::Cycle {
                        frames: vec!['*', '+', 'x', '+'],
                        fps: 6.0,
                    }),
            ));
            scene.layers.push(props);

            // Hero: sparse contour art, bright rim tint, chest aura.
            let mut actors = Layer::new("actors", 2);
            let (w, h) = sprite::sprite_size(frames);
            let mut hero = sprite::sprite_layer(
                "hero",
                2,
                frames,
                SpriteDraw::new(
                    hx.round() as i32 - w / 2,
                    FEET_Y - (h - 1),
                    Rgb::new(225, 232, 245),
                    BG,
                    if mode == Mode::Walk { WALK_FPS } else { 0.0 },
                ),
            );
            actors.cells.append(&mut hero.cells);
            // Chest aura + nameplate track the hero.
            actors.push(AnimCell::still(
                hx.round() as i32,
                FEET_Y - 14,
                Cell::new('@', Rgb::new(200, 220, 255), BG).with_glow(0.5),
            ));
            let mut labels = Layer::new("labels", 3);
            labels.push_text(
                hx.round() as i32 - 2,
                FEET_Y - 21,
                "Senje",
                Rgb::new(235, 235, 235),
                BG,
            );
            scene.layers.push(actors);
            scene.layers.push(labels);

            // Vector bolt lives in strokes(), not in a cell layer.

            // HUD title.
            let mut hud = Layer::new("hud", 5);
            hud.push_text(1, 0, "SPARSE HERO", Rgb::new(220, 190, 120), BG);
            hud.push_text(1, 1, "walk + turn + bolt", Rgb::new(140, 160, 190), BG);
            scene.layers.push(hud);

            let idx: Vec<usize> = (0..scene.layers.len()).collect();
            scene
                .partitions
                .push(Partition::new("all", 0, 0, W, H, idx, false));
            (scene, Camera::default())
        }

        /// Vector bolt for time `t`: main path plus two seeded splits,
        /// growing across the cast window and fading at its end.
        fn bolt_strokes(&self, t: f32) -> Vec<Stroke> {
            let (hx, _, _, facing_right) = self.pose_at(t);
            let phase = t.rem_euclid(LOOP);
            if !(4.5..6.0).contains(&phase) {
                return Vec::new();
            }
            let age = phase - 4.5;
            let env = (age / 0.4).min(1.0) * ((1.5 - age) / 0.5).min(1.0);
            if env < 0.05 {
                return Vec::new();
            }
            let dir = if facing_right { 1.0 } else { -1.0 };
            let (ox, oy) = (hx + 6.0 * dir, (FEET_Y - 12) as f32);
            let total = 10;
            let revealed = ((age / 1.5 * total as f32) as usize).clamp(2, total);
            let mut points = vec![(ox, oy)];
            for i in 1..revealed {
                let jitter = ((i as i32 * 37 + 11) % 3) as f32 - 1.0;
                points.push((ox + dir * (i as f32 + jitter), oy + i as f32));
            }
            let scale = |c: f32| (c * env).min(1.0);
            let nodes: Vec<(f32, f32)> = points.iter().step_by(3).copied().collect();
            let mut strokes = vec![Stroke {
                points,
                width: 0.14,
                color: [scale(0.9), scale(0.95), 1.0],
                glow_width: 1.1,
                glow: [scale(0.45), scale(0.6), 1.0],
                glow_alpha: 0.55,
                nodes,
            }];
            for (k, joint) in [3usize, 6].iter().enumerate() {
                let j = (*joint).min(revealed.saturating_sub(1));
                let branch = overlay::fork_points(
                    100 + k as u32,
                    (ox + dir * j as f32, oy + j as f32),
                    (dir, 1.0),
                    5,
                );
                let bnodes: Vec<(f32, f32)> = branch.iter().step_by(3).copied().collect();
                strokes.push(Stroke {
                    points: branch,
                    width: 0.1,
                    color: [scale(0.75), scale(0.85), 1.0],
                    glow_width: 0.7,
                    glow: [scale(0.4), scale(0.55), 1.0],
                    glow_alpha: 0.45,
                    nodes: bnodes,
                });
            }
            strokes
        }
    }

    impl SceneSource for HeroShow {
        fn scene_at(&mut self, t: f32) -> (Scene, Camera) {
            self.build(t)
        }

        fn strokes(&mut self, t: f32) -> Vec<Stroke> {
            self.bolt_strokes(t)
        }
    }

    fn run_check() -> Result<(), String> {
        let show = HeroShow::new();
        // Walk advances the hero (frames differ across two seconds).
        let rows_at = |t: f32| {
            use ascii_mmorpg_engine::compose;
            let (scene, camera) = show.build(t);
            compose(&scene, &camera, t).rows().join("\n")
        };
        if rows_at(0.0) == rows_at(2.0) {
            return Err("check: hero did not walk".into());
        }
        // Bolt fires inside the cast window, silent outside it.
        if show.bolt_strokes(5.2).is_empty() {
            return Err("check: bolt missing during cast".into());
        }
        if !show.bolt_strokes(8.0).is_empty() {
            return Err("check: bolt leaked outside cast".into());
        }
        // Mirrored patrol faces both ways (left walk differs from right).
        if rows_at(8.0) == rows_at(2.0) {
            return Err("check: mirrored walk identical to right walk".into());
        }
        Ok(())
    }

    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|arg| arg == "--check") {
        run_check()?;
        eprintln!("hero check ok: patrol walks, turns, and casts");
        return Ok(());
    }
    let frames = args
        .iter()
        .position(|arg| arg == "--frames")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse::<u64>().ok());

    match ascii_mmorpg_engine::present::run_dynamic(
        HeroShow::new(),
        PresentOptions {
            title: "ASCII Engine — sparse hero".into(),
            width_cells: W as u32,
            height_cells: H as u32,
            max_frames: frames,
            ..Default::default()
        },
    ) {
        Ok(presented) => {
            eprintln!("presented {presented} frames");
            std::process::exit(0);
        }
        Err(err) => Err(err),
    }
}
