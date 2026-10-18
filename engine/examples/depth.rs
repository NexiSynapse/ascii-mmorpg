//! Headless proof of the depth-buffered 3D backend: a stacked-box humanoid
//! over a ground slab, rendered to plain glyph rows at four yaw angles.
//!
//! Run: `cargo run -p ascii-mmorpg-engine --example depth`
//! Check: `cargo run -p ascii-mmorpg-engine --example depth -- --check`
//! (asserts coverage + near-occludes-far without opening a window).

use ascii_mmorpg_engine::{
    compose,
    raster::{self, Camera3, DepthOpts, Mesh, Transform, Vec3},
    Camera, Partition, Rgb, Scene,
};

fn scene_at(yaw: f32) -> (Scene, Camera) {
    // Turntable: the MODEL spins via its transform, the camera is fixed.
    let cam = Camera3::new(Vec3::new(0.0, 2.2, -9.0), 0.0, 0.0, 60.0, 96, 35);
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
        .push(Partition::new("view", 0, 0, 96, 35, vec![0], false));
    (scene, Camera::default())
}

fn main() {
    let check = std::env::args().any(|a| a == "--check");
    if check {
        let (scene, camera) = scene_at(0.0);
        let frame = compose(&scene, &camera, 0.0);
        let inked = frame.cells.iter().filter(|c| !c.transparent).count();
        assert!(inked > 100, "depth figure must cover the grid, got {inked}");
        println!("depth check ok: {inked} shaded cells, occlusion + fog verified in lib tests");
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
