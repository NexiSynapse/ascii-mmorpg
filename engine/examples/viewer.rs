//! Windowed viewer for the depth backend: the stacked-box humanoid turntables
//! over its ground slab through the real winit/wgpu presenter (GPU glyph
//! atlas, overlay FPS readout included).
//!
//! Run: `cargo run -p ascii-mmorpg-engine --features present --example viewer`
//! Flags: `--frames N` exits after N presented frames (automated proof);
//! `--check` runs the headless coverage proof without opening a window.

#[cfg(not(feature = "present"))]
fn main() {
    eprintln!("this example needs `--features present` (winit + wgpu + fontdue)");
    std::process::exit(2);
}

#[cfg(feature = "present")]
fn main() {
    use ascii_mmorpg_engine::{
        compose,
        present::{PresentOptions, SceneSource},
        raster::{self, Camera3, DepthOpts, Mesh, Transform, Vec3},
        Camera, Partition, Rgb, Scene,
    };

    const W: i32 = 96;
    const H: i32 = 35;

    fn build(yaw: f32, width: i32, height: i32) -> (Scene, Camera) {
        // Focal tracks the live width so the framing survives resizes.
        let focal = 60.0 * width.max(1) as f32 / 96.0;
        let cam = Camera3::new(Vec3::new(0.0, 2.2, -9.0), 0.0, 0.0, focal, width, height);
        let sun = Vec3::new(-0.35, 0.55, -0.75);
        let opts = DepthOpts::default();
        let figure = Mesh::humanoid().transformed(&Transform {
            yaw,
            ..Transform::identity()
        });
        let meshes = vec![figure, Mesh::ground_slab(Rgb::new(24, 27, 38))];
        let mut scene = Scene::default();
        scene
            .layers
            .push(raster::render_layer(&meshes, &cam, sun, &opts, 2));
        scene
            .partitions
            .push(Partition::new("view", 0, 0, width, height, vec![0], false));
        (scene, Camera::default())
    }

    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--check") {
        let (scene, camera) = build(0.3, W, H);
        let frame = compose(&scene, &camera, 0.0);
        let inked = frame.cells.iter().filter(|c| !c.transparent).count();
        assert!(inked > 100, "viewer scene must cover the grid, got {inked}");
        println!("viewer check ok: {inked} shaded cells through the composed path");
        return;
    }
    let frames = args
        .iter()
        .position(|a| a == "--frames")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse::<u64>().ok());

    struct Viewer {
        t: f32,
    }

    impl SceneSource for Viewer {
        fn scene_at(&mut self, t: f32) -> (Scene, Camera) {
            self.scene_at_size(t, W as u32, H as u32)
        }

        fn scene_at_size(&mut self, t: f32, width: u32, height: u32) -> (Scene, Camera) {
            self.t = t;
            build(t * 0.5, width as i32, height as i32)
        }
    }

    match ascii_mmorpg_engine::present::run_dynamic(
        Viewer { t: 0.0 },
        PresentOptions {
            title: "ASCII MMORPG — 3D depth viewer".into(),
            cell_px: 16.0,
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
        Err(err) => {
            eprintln!("presenter failed: {err}");
            std::process::exit(1);
        }
    }
}
