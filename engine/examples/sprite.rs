//! Headless proof that shipped game art composes as engine layers: the
//! warrior sprite (`assets/ascii/player_warrior.txt`) flip-books inside a
//! bordered panel with a text label.
//!
//! Run: `cargo run -p ascii-mmorpg-engine --example sprite`

use ascii_mmorpg_engine::{compose, sprite, Camera, Layer, Partition, Rgb, Scene};
use sprite::SpriteDraw;

const WARRIOR: &str = include_str!("../../assets/ascii/player_warrior.txt");

fn main() {
    let frames = sprite::parse_frames(WARRIOR);
    let (w, h) = sprite::sprite_size(&frames);
    let panel_w = w + 4;
    let panel_h = h + 4;

    let mut scene = Scene::default();
    let mut panel = Layer::new("panel", 0);
    panel.border_rect(
        0,
        0,
        panel_w,
        panel_h,
        Rgb::new(120, 120, 140),
        Rgb::new(8, 8, 12),
    );
    panel.push_text(2, 1, "WARRIOR", Rgb::new(255, 230, 150), Rgb::new(8, 8, 12));
    scene.layers.push(panel);
    scene.layers.push(sprite::sprite_layer(
        "warrior",
        1,
        &frames,
        SpriteDraw::new(2, 2, Rgb::new(220, 200, 120), Rgb::new(8, 8, 12), 4.0),
    ));
    let panel = scene.layer("panel").unwrap();
    let warrior = scene.layer("warrior").unwrap();
    scene.partitions.push(Partition::new(
        "all",
        0,
        0,
        panel_w,
        panel_h,
        vec![panel, warrior],
        false,
    ));

    for frame in 0..4 {
        let t = frame as f32 * 0.25;
        println!("--- t = {:.2}s ---", t);
        for row in compose(&scene, &Camera::default(), t).rows() {
            println!("{row}");
        }
    }
}
