//! Ready-made animated scenes for examples and smoke tests.
//!
//! `wave_scene` is the engine's "hello animation": a water line waving
//! left-to-right over a still bed, a breathing/cycling torch, and a sidebar
//! partition zoomed on the torch. Used by `examples/wave.rs` (headless dump)
//! and `examples/present.rs` (windowed) so both show the same scene.

use super::{Anim, AnimCell, Axis, Camera, Cell, Layer, Partition, Rgb, Scene, Track};
use std::f32::consts::TAU;

pub const VIEW_W: i32 = 42;
pub const VIEW_H: i32 = 14;

/// Builds `(scene, camera)`. World partition `0,0,V VIEW_W x VIEW_H` shows
/// both layers; sidebar `42,0,6x14` shows the torch layer around (32,3).
pub fn wave_scene() -> (Scene, Camera) {
    let mut scene = Scene::default();

    let mut water = Layer::new("water", 1);
    let fg = Rgb::new(80, 160, 255);
    let bg = Rgb::new(8, 16, 32);
    for x in 0..40 {
        water.push(AnimCell::animated(
            x,
            9,
            Cell::new('~', fg, bg),
            Anim::still().with(Track::Wave {
                axis: Axis::Y,
                amp: 1.0,
                wavelength: 10.0,
                period: 2.4,
                phase: x as f32 * TAU / 10.0,
                brighten: 0.25,
            }),
        ));
    }
    for x in 0..40 {
        water.push(AnimCell::still(
            x,
            11,
            Cell::new('=', Rgb::new(40, 60, 80), bg),
        ));
    }

    let mut torch = Layer::new("torch", 2);
    torch.push(AnimCell::animated(
        35,
        8,
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
    torch.push(AnimCell::still(
        35,
        9,
        Cell::new('|', Rgb::new(120, 80, 40), Rgb::new(16, 10, 6)),
    ));

    scene.layers.push(water);
    scene.layers.push(torch);
    let water = scene.layer("water").unwrap();
    let torch = scene.layer("torch").unwrap();
    scene.partitions.push(Partition::new(
        "world",
        0,
        0,
        VIEW_W,
        VIEW_H,
        vec![water, torch],
        false,
    ));
    scene
        .partitions
        .push(Partition::new("sidebar", 42, 0, 6, VIEW_H, vec![torch], false).with_view(32, 3));
    (scene, Camera::default())
}
