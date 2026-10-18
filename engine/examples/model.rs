//! Model-file proof for the depth backend: loads `assets/meshes/sword.obj`
//! (embedded, like the ASCII art) and turntables it as glyph rows.
//!
//! Run: `cargo run -p ascii-mmorpg-engine --example model`
//! Check: `cargo run -p ascii-mmorpg-engine --example model -- --check`

use ascii_mmorpg_engine::{
    compose,
    raster::{self, Camera3, DepthOpts, Transform, Vec3},
    Camera, Partition, Rgb, Scene,
};

const SWORD_OBJ: &str = include_str!("../assets/meshes/sword.obj");

fn scene_at(yaw: f32) -> (Scene, Camera) {
    let cam = Camera3::new(Vec3::new(0.0, 0.45, -5.5), 0.0, 0.0, 60.0, 96, 35);
    let sun = Vec3::new(-0.35, 0.55, -0.75);
    let opts = DepthOpts::default();
    let sword = raster::load_obj(SWORD_OBJ, Rgb::new(170, 185, 205))
        .expect("bundled sword.obj must parse")
        .transformed(&Transform {
            yaw,
            ..Transform::identity()
        });
    let mut scene = Scene::default();
    scene
        .layers
        .push(raster::render_layer(&[sword], &cam, sun, &opts, 2));
    scene
        .partitions
        .push(Partition::new("view", 0, 0, 96, 35, vec![0], false));
    (scene, Camera::default())
}

fn main() {
    let check = std::env::args().any(|a| a == "--check");
    if check {
        let (scene, camera) = scene_at(0.3);
        let frame = compose(&scene, &camera, 0.0);
        let inked = frame.cells.iter().filter(|c| !c.transparent).count();
        assert!(inked > 60, "loaded sword must cover cells, got {inked}");
        println!("model check ok: sword.obj parsed and shaded ({inked} cells)");
        return;
    }
    for step in 0..4 {
        let yaw = step as f32 * std::f32::consts::FRAC_PI_2 / 3.0;
        println!("--- yaw = {:.2} rad ---", yaw);
        let (scene, camera) = scene_at(yaw);
        for row in compose(&scene, &camera, 0.0).rows() {
            println!("{row}");
        }
    }
}
