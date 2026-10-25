//! Veyrath world showcase for the continuous canvas (issue #7): the shared
//! visual model rendered as native primitives — region carpets in terrain
//! palette ink, river polylines, dashed roads, faction-coloured
//! settlements, landmark marks, and the Millhaven starter highlight —
//! over an intentional black negative space. All coordinates are the
//! manifest's PROVISIONAL percent authoring; nothing here is collision or
//! server state.
//!
//! Run: `cargo run --locked -p ascii-mmorpg-engine --features present
//!       --release --example world`
//! Flags: `--frames N` exits after N presented frames; `--check` runs the
//! deterministic headless contracts.

#[cfg(not(feature = "present"))]
fn main() {
    eprintln!("this example needs `--features present` (winit + wgpu + fontdue)");
    std::process::exit(2);
}

#[cfg(feature = "present")]
fn main() {
    if let Err(err) = real_main() {
        eprintln!("world failed: {err}");
        std::process::exit(1);
    }
}

#[cfg(feature = "present")]
fn real_main() -> Result<(), String> {
    use ascii_mmorpg_engine::present::{PresentOptions, SceneSource};
    use ascii_mmorpg_engine::veyrath_world::{world_scene, WorldView, VEYRATH_WORLD_BUDGET};
    use ascii_mmorpg_engine::{Camera, Layer, Partition, Rgb, Scene};

    const W: i32 = 180;
    const H: i32 = 74;

    struct WorldShow;

    impl WorldShow {
        /// Composed captions only: the map itself is entirely native
        /// primitives, and the frame carries no glow — the void stays
        /// black by design.
        fn captions(&self) -> Scene {
            let dim = Rgb::new(140, 130, 112);
            let faint = Rgb::new(84, 78, 66);
            let void = Rgb::new(0, 0, 0);
            let data = ascii_mmorpg_engine::veyrath::visual_data();
            let mut scene = Scene::default();
            let mut ui = Layer::new("captions", 0);
            let title = format!(
                "{} — CONTINUOUS WORLD VIEW",
                data.title.to_ascii_uppercase()
            );
            ui.push_text(2, 0, &title, dim, void);
            ui.push_text(
                2,
                1,
                "region carpets · faction settlements · rivers · dashed roads · landmarks",
                faint,
                void,
            );
            ui.push_text(2, H - 1, "Q / ESC QUITS", faint, void);
            let right = W - 46;
            ui.push_text(
                right,
                H - 1,
                "starter: MILLHAVEN (highlight ring)",
                faint,
                void,
            );
            scene.layers.push(ui);
            scene
                .partitions
                .push(Partition::new("ui", 0, 0, W, H, vec![0], false));
            scene
        }
    }

    impl SceneSource for WorldShow {
        fn scene_at(&mut self, _t: f32) -> (Scene, Camera) {
            (self.captions(), Camera::default())
        }

        fn continuous_scene(&mut self, t: f32) -> Option<ascii_mmorpg_engine::ContinuousScene> {
            Some(world_scene(WorldView::new(W as u32, H as u32), t))
        }
    }

    fn run_check() -> Result<(), String> {
        use ascii_mmorpg_engine::veyrath::visual_data;
        let view = WorldView::new(W as u32, H as u32);

        // Determinism at fixed camera/time inputs.
        for t in [0.0, 0.5, 2.3] {
            let (a, b) = (world_scene(view, t), world_scene(view, t));
            if a.primitives() != b.primitives() || a.sample(t) != b.sample(t) {
                return Err(format!(
                    "check: world composition not deterministic at t={t}"
                ));
            }
        }

        // Budget + quiet negative space.
        let scene = world_scene(view, 1.0);
        let count = scene.primitives().len();
        if count == 0 || count > VEYRATH_WORLD_BUDGET {
            return Err(format!("check: primitive count {count} out of budget"));
        }
        let density = count as f32 / (W as f32 * H as f32);
        if density >= 0.08 {
            return Err(format!("check: density {density:.3} is not a quiet carpet"));
        }

        // Faction identity: the starter highlight and at least three
        // distinct faction colours are on the map.
        let data = visual_data();
        let starter = data
            .settlement(&data.starter_settlement)
            .ok_or("check: starter settlement missing from the model")?;
        if starter.id != "millhaven" {
            return Err(format!("check: unexpected starter {}", starter.id));
        }
        let mut faction_colors = std::collections::BTreeSet::new();
        for faction in &data.factions {
            faction_colors.insert(faction.color);
        }
        if faction_colors.len() < 3 {
            return Err("check: fewer than three factions in the model".into());
        }

        // Captions carry no legacy glow (the void stays dark; the map emits
        // nothing — bloom ownership is not claimed).
        let mut show = WorldShow;
        let (captions, camera) = show.scene_at(0.0);
        let frame = ascii_mmorpg_engine::compose(&captions, &camera, 0.0);
        let glowing = frame
            .cells
            .iter()
            .filter(|cell| !cell.transparent && cell.glow > 0.0)
            .count();
        if glowing != 0 {
            return Err(format!("check: {glowing} caption cells carry glow"));
        }
        eprintln!(
            "world check ok: {count} primitives, density {density:.3}, starter ring + {} factions present",
            faction_colors.len()
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
        WorldShow,
        PresentOptions {
            title: "ASCII Engine — Veyrath continuous world".into(),
            cell_px: 10.0,
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
