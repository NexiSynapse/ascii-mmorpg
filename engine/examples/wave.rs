//! Headless proof of the animated-cell model: prints 8 frames of
//! [`wave_scene`](crate::demo::wave_scene) as plain glyph rows.
//!
//! Run: `cargo run -p ascii-mmorpg-engine --example wave`

use ascii_mmorpg_engine::{compose, demo};

fn main() {
    let (scene, camera) = demo::wave_scene();
    for frame in 0..8 {
        let t = frame as f32 * 0.3;
        println!("--- t = {:.1}s ---", t);
        for row in compose(&scene, &camera, t).rows() {
            println!("{row}");
        }
    }
}
