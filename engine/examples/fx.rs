//! Data-driven effect showcase: the eight engine effect archetypes share one
//! record list, so sampling never rebuilds scripted particle geometry.
//!
//! Run: `cargo run -p ascii-mmorpg-engine --features present --example fx`
//! Flags: `--frames N` exits after N presented frames; `--check` runs the
//! deterministic headless growth/release checks.

#[cfg(not(feature = "present"))]
fn main() {
    eprintln!("this example needs `--features present` (winit + wgpu + fontdue)");
    std::process::exit(2);
}

#[cfg(feature = "present")]
fn main() {
    if let Err(err) = real_main() {
        eprintln!("fx failed: {err}");
        std::process::exit(1);
    }
}

#[cfg(feature = "present")]
fn real_main() -> Result<(), String> {
    use ascii_mmorpg_engine::fx::{layer as effect_layer, Effect, Emitter};
    use ascii_mmorpg_engine::present::{PresentOptions, SceneSource};
    use ascii_mmorpg_engine::{
        compose, AnimCell, Camera, Cell, Layer, Partition, Rgb, Scene, Shade, Wash,
    };

    const W: i32 = 64;
    const H: i32 = 23;
    const BG: Rgb = Rgb::new(10, 14, 26);
    const LOOP: f32 = 2.4;

    struct FxShow {
        effects: Vec<Emitter>,
    }

    impl FxShow {
        fn new() -> Self {
            // Coordinates in each effect are deliberately relative to its
            // origin. The records stay immutable for the lifetime of the
            // presenter; fx::layer performs only the time-dependent sampling.
            Self {
                effects: vec![
                    Emitter {
                        origin: (52.0, 5.0),
                        start: 0.0,
                        lifetime: 1.8,
                        cell: Cell::new('*', Rgb::new(255, 185, 90), BG).with_glow(0.8),
                        kind: Effect::Burst {
                            count: 13,
                            speed: 8.0,
                            seed: 0xB017,
                        },
                    },
                    Emitter {
                        origin: (16.0, 8.0),
                        start: 0.0,
                        lifetime: 1.8,
                        cell: Cell::new('o', Rgb::new(100, 205, 255), BG).with_glow(0.7),
                        kind: Effect::Ring {
                            count: 18,
                            radius: 5.0,
                        },
                    },
                    Emitter {
                        origin: (36.0, 3.0),
                        start: 0.0,
                        lifetime: 1.8,
                        cell: Cell::new('/', Rgb::new(200, 220, 255), BG).with_glow(1.0),
                        kind: Effect::Chain {
                            points: vec![
                                (0, 0),
                                (1, 1),
                                (-1, 2),
                                (1, 3),
                                (0, 4),
                                (2, 5),
                                (1, 6),
                                (3, 7),
                                (2, 8),
                                (4, 9),
                            ],
                            grow: 0.42,
                        },
                    },
                    Emitter {
                        origin: (48.0, 17.0),
                        start: 0.0,
                        lifetime: 1.8,
                        cell: Cell::new('|', Rgb::new(180, 235, 255), BG).with_glow(0.8),
                        kind: Effect::Spike {
                            length: 6,
                            direction: (0.0, -1.0),
                            grow: 0.36,
                        },
                    },
                    Emitter {
                        origin: (57.0, 18.0),
                        start: 0.0,
                        lifetime: 1.8,
                        cell: Cell::new('|', Rgb::new(210, 235, 255), BG).with_glow(1.0),
                        kind: Effect::LightningColumn {
                            height: 9,
                            width: 5,
                            grow: 0.18,
                            seed: 0x1CE,
                        },
                    },
                    Emitter {
                        origin: (24.0, 8.0),
                        start: 0.0,
                        lifetime: 1.8,
                        cell: Cell::new('^', Rgb::new(170, 220, 255), BG).with_glow(0.8),
                        kind: Effect::IcicleWave {
                            rows: 3,
                            length: 5,
                            direction: (1.0, 0.0),
                            spacing: 2.0,
                            grow: 0.3,
                        },
                    },
                    Emitter {
                        origin: (20.0, 18.0),
                        start: 0.0,
                        lifetime: 1.8,
                        cell: Cell::new('o', Rgb::new(170, 185, 205), BG).with_glow(0.55),
                        kind: Effect::SmokeRing {
                            count: 12,
                            radius: 4.0,
                            rise: 0.35,
                            seed: 0x5A0,
                        },
                    },
                    Emitter {
                        origin: (29.0, 16.0),
                        start: 0.0,
                        lifetime: 1.8,
                        cell: Cell::new('!', Rgb::new(255, 100, 100), BG).with_glow(0.6),
                        kind: Effect::Number {
                            text: "-42".into(),
                            rise: 3.0,
                        },
                    },
                ],
            }
        }

        fn build(&self, t: f32) -> (Scene, Camera) {
            let phase = t.rem_euclid(LOOP);
            let mut scene = Scene::default();

            let mut mosaic = Layer::new("mosaic", 0);
            mosaic.fill_shaded_rect(
                2,
                3,
                20,
                10,
                Shade::new('\u{2588}', Rgb::new(190, 140, 95), Rgb::new(70, 35, 18), BG),
            );
            mosaic.push_text(4, 14, "MOSAIC", Rgb::new(220, 200, 170), BG);
            mosaic.push_text(11, 1, "RING", Rgb::new(100, 205, 255), BG);
            mosaic.push_text(33, 1, "CHAIN", Rgb::new(200, 220, 255), BG);
            mosaic.push_text(48, 1, "BURST", Rgb::new(255, 185, 90), BG);
            mosaic.push_text(25, 21, "NUMBER", Rgb::new(255, 100, 100), BG);
            mosaic.push_text(44, 21, "SPIKE", Rgb::new(180, 235, 255), BG);
            mosaic.push_text(50, 1, "BOLT", Rgb::new(210, 235, 255), BG);
            mosaic.push_text(21, 1, "ICE", Rgb::new(170, 220, 255), BG);
            mosaic.push_text(16, 21, "SMOKE", Rgb::new(170, 185, 205), BG);
            scene.layers.push(mosaic);

            let mut torch = Layer::new("torch", 1);
            torch.push_text(7, 20, "GLOW", Rgb::new(255, 190, 120), BG);
            torch.push(AnimCell::still(
                8,
                19,
                Cell::new('|', Rgb::new(120, 80, 40), BG),
            ));
            torch.push(AnimCell::still(
                8,
                18,
                Cell::new('*', Rgb::new(255, 170, 60), Rgb::new(26, 14, 6)).with_glow(1.0),
            ));
            scene.layers.push(torch);

            let effects = effect_layer("effects", 2, &self.effects, phase);
            scene.layers.push(effects);
            // Amber wash flash: the burst ignites the whole frame early in
            // the loop, then the air cools back to black (reference wash).
            let flash = (1.0 - phase / 1.5).clamp(0.0, 1.0);
            if flash > 0.0 {
                scene.wash = Some(Wash {
                    tint: Rgb::new(150, 85, 25),
                    strength: flash * 0.3,
                });
            }
            let indices: Vec<usize> = (0..scene.layers.len()).collect();
            scene
                .partitions
                .push(Partition::new("all", 0, 0, W, H, indices, false));
            (scene, Camera::default())
        }

        fn visible_in(&self, t: f32, x0: i32, y0: i32, x1: i32, y1: i32) -> usize {
            let (scene, camera) = self.build(t);
            compose(&scene, &camera, t)
                .cells
                .iter()
                .enumerate()
                .filter(|(index, cell)| {
                    let x = *index as i32 % W;
                    let y = *index as i32 / W;
                    x >= x0 && x < x1 && y >= y0 && y < y1 && cell.glyph != ' '
                })
                .count()
        }
    }

    impl SceneSource for FxShow {
        fn scene_at(&mut self, t: f32) -> (Scene, Camera) {
            self.build(t)
        }
    }

    fn run_check() -> Result<(), String> {
        let show = FxShow::new();
        let a = show.build(0.73).0;
        let b = show.build(0.73).0;
        if compose(&a, &Camera::default(), 0.73).rows()
            != compose(&b, &Camera::default(), 0.73).rows()
        {
            return Err("check: effect sampling is not deterministic".into());
        }
        let regions = [
            (11, 3, 22, 14),
            (48, 2, 63, 14),
            (33, 2, 43, 14),
            (44, 10, 55, 20),
            (25, 12, 35, 20),
            (50, 2, 63, 21),
            (20, 2, 34, 13),
            (14, 12, 27, 21),
        ];
        for (i, region) in regions.into_iter().enumerate() {
            let visible = show.visible_in(0.8, region.0, region.1, region.2, region.3);
            if visible == 0 {
                return Err(format!("check: effect kind {i} is not visible"));
            }
        }
        let early = show.visible_in(0.1, 33, 2, 43, 14);
        let grown = show.visible_in(0.8, 33, 2, 43, 14);
        let released = show.visible_in(2.2, 33, 2, 43, 14);
        if grown <= early || released != 0 {
            return Err(format!(
                "check: chain growth/release failed ({early}, {grown}, {released})"
            ));
        }
        Ok(())
    }

    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|arg| arg == "--check") {
        run_check()?;
        eprintln!("fx check ok: eight data-driven effects grow and release");
        return Ok(());
    }
    let frames = args
        .iter()
        .position(|arg| arg == "--frames")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse::<u64>().ok());
    match ascii_mmorpg_engine::present::run_dynamic(
        FxShow::new(),
        PresentOptions {
            title: "ASCII Engine — shader fx showcase".into(),
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
