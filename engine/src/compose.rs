use super::{sample, Camera, Cell, ColorTrack, Rgb, Scene, Wash};

/// One flattened frame: resolved glyphs + style per grid cell. This is what a
/// presenter (terminal today, windowed GUI next) draws; the engine itself
/// never touches I/O.
#[derive(Clone, Debug)]
pub struct Frame {
    pub w: i32,
    pub h: i32,
    pub cells: Vec<Cell>,
}

impl Frame {
    pub fn blank(w: i32, h: i32) -> Self {
        Self {
            w,
            h,
            cells: vec![Cell::CLEAR; w.max(0) as usize * h.max(0) as usize],
        }
    }

    pub fn get(&self, x: i32, y: i32) -> Option<Cell> {
        if x < 0 || y < 0 || x >= self.w || y >= self.h {
            return None;
        }
        self.cells.get((y * self.w + x) as usize).copied()
    }

    fn set(&mut self, x: i32, y: i32, cell: Cell) {
        if x < 0 || y < 0 || x >= self.w || y >= self.h {
            return;
        }
        self.cells[(y * self.w + x) as usize] = cell;
    }

    /// Plain glyph rows (for tests and headless previews; presenter adds color).
    pub fn rows(&self) -> Vec<String> {
        (0..self.h)
            .map(|y| {
                (0..self.w)
                    .map(|x| self.get(x, y).map(|c| c.glyph).unwrap_or(' '))
                    .collect()
            })
            .collect()
    }
}

/// Render `scene` at time `t` seconds through `camera` (three.js analogy:
/// `Renderer.render`). For every partition, layers run bottom-up by `z`;
/// each animated cell is sampled, camera-shifted (camera partitions only),
/// clipped to the partition rect, and the topmost non-transparent cell wins.
pub fn compose(scene: &Scene, camera: &Camera, t: f32) -> Frame {
    let w = scene
        .partitions
        .iter()
        .map(|p| p.x + p.w)
        .max()
        .unwrap_or(0)
        .max(0);
    let h = scene
        .partitions
        .iter()
        .map(|p| p.y + p.h)
        .max()
        .unwrap_or(0)
        .max(0);
    let mut frame = Frame::blank(w, h);

    for partition in &scene.partitions {
        let mut order: Vec<usize> = partition.layers.to_vec();
        order.sort_by_key(|&i| scene.layers.get(i).map(|l| l.z).unwrap_or(0));
        let (vx, vy) = if partition.camera {
            (camera.x, camera.y)
        } else {
            (partition.vx, partition.vy)
        };
        for index in order {
            let Some(layer) = scene.layers.get(index) else {
                continue;
            };
            if !layer.visible {
                continue;
            }
            for cell in &layer.cells {
                if cell.cell.transparent {
                    continue;
                }
                // Fast path: still art with a flat color resolves by copy.
                // Identity-checked: empty tracks sample to (no glyph, no
                // offset, bright 1.0) and Static returns its color, so this
                // matches the slow path exactly while skipping two
                // evaluations per cell (the common case at large grids).
                if cell.anim.tracks.is_empty() {
                    if let ColorTrack::Static(flat) = cell.color {
                        let (x, y) = (cell.x - vx + partition.x, cell.y - vy + partition.y);
                        if !partition.contains(x, y) {
                            continue;
                        }
                        let mut resolved = cell.cell;
                        resolved.fg = flat;
                        frame.set(x, y, resolved);
                        continue;
                    }
                }
                let s = sample(&cell.anim, cell.x as f32, cell.y as f32, t);
                let (x, y) = (
                    cell.x - vx + partition.x + s.dx,
                    cell.y - vy + partition.y + s.dy,
                );
                if !partition.contains(x, y) {
                    continue;
                }
                let mut resolved = cell.cell;
                if let Some(glyph) = s.glyph {
                    resolved.glyph = glyph;
                }
                resolved.fg = cell.color.sample(
                    t,
                    (cell.x as f32 + s.dx as f32, cell.y as f32 + s.dy as f32),
                );
                resolved.fg = resolved.fg.scaled(s.bright);
                frame.set(x, y, resolved);
            }
        }
    }
    apply_glow(&mut frame);
    if let Some(wash) = scene.wash {
        apply_wash(&mut frame, wash);
    }
    frame
}

/// Global tint wash, applied after glow (reference amber washes during
/// bursts/boss phases). Runs after all partitions so the whole frame warms
/// evenly; writes backgrounds only.
fn apply_wash(frame: &mut Frame, wash: Wash) {
    let strength = wash.strength.clamp(0.0, 1.0);
    if strength <= 0.0 {
        return;
    }
    for y in 0..frame.h {
        for x in 0..frame.w {
            if let Some(mut cell) = frame.get(x, y) {
                if cell.transparent {
                    continue;
                }
                let warm = |bg: u8, tint: u8| {
                    (f32::from(bg) + strength * f32::from(tint)).clamp(0.0, 255.0) as u8
                };
                cell.bg = Rgb::new(
                    warm(cell.bg.r, wash.tint.r),
                    warm(cell.bg.g, wash.tint.g),
                    warm(cell.bg.b, wash.tint.b),
                );
                frame.set(x, y, cell);
            }
        }
    }
}

/// Measured reference curve (burst core, Video-47489 t=10.5 s): halo excess
/// ≈15 brightness levels at 1 cell out, fading by ~5 cells (~40 px at 8
/// px/cell), peak in the spark ring rather than the core. Tuned to match:
/// reach = glow × 5 cells, quadratic falloff, amplitude 0.12 (a glow-1.0
/// torch washes ≈21 levels at distance 1, ≈9 at 2, ≈4 at 3).
const GLOW_RADIUS_PER_UNIT: f32 = 5.0;
const GLOW_AMPLITUDE: f32 = 0.12;

/// Light buffer: every stamped cell with `glow > 0` washes its foreground
/// color onto neighboring backgrounds with quadratic falloff (reference
/// torch halos and spell-core blooms tint nearby glyphs). Runs after all
/// partitions so halos cross partition borders like real light; writes only
/// backgrounds, so emission order is irrelevant and the pass is stable.
fn apply_glow(frame: &mut Frame) {
    // Index-based scan: the old per-cell `get()` paid bounds checks plus an
    // Option unwrap on every frame cell; the spill below does one manual
    // bounds check per target instead of two (get + set). Behavior — which
    // cells wash and by how much — is unchanged.
    let w = frame.w;
    let h = frame.h;
    let sources: Vec<(i32, i32, Cell)> = frame
        .cells
        .iter()
        .enumerate()
        .filter_map(|(i, cell)| {
            if cell.transparent || !cell.glow.is_finite() || cell.glow <= 0.0 {
                return None;
            }
            Some(((i as i32 % w.max(1)), (i as i32 / w.max(1)), *cell))
        })
        .collect();
    for (sx, sy, mut source) in sources {
        source.glow = source.glow.min(8.0);
        let radius = (source.glow * GLOW_RADIUS_PER_UNIT).ceil() as i32;
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                let dist = ((dx * dx + dy * dy) as f32).sqrt();
                if dist > radius as f32 + 0.5 {
                    continue;
                }
                let fall = source.glow * (1.0 - dist / (radius as f32 + 1.0));
                let fall = fall * fall * GLOW_AMPLITUDE;
                if fall <= 0.0 {
                    continue;
                }
                let (tx, ty) = (sx + dx, sy + dy);
                if tx < 0 || ty < 0 || tx >= w || ty >= h {
                    continue;
                }
                let target = &mut frame.cells[(ty * w + tx) as usize];
                let wash =
                    |bg: u8, fg: u8| (f32::from(bg) + fall * f32::from(fg)).clamp(0.0, 255.0) as u8;
                target.bg = Rgb::new(
                    wash(target.bg.r, source.fg.r),
                    wash(target.bg.g, source.fg.g),
                    wash(target.bg.b, source.fg.b),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{
        Anim, AnimCell, Axis, Cell, ColorTrack, Layer, Partition, Rgb, Scene, Track,
    };
    use super::*;

    #[test]
    fn top_layer_wins_and_transparent_shows_through() {
        let mut scene = Scene::default();
        let mut back = Layer::new("bg", 0);
        back.push(AnimCell::still(
            0,
            0,
            Cell::new('.', Rgb::new(10, 10, 10), Rgb::new(0, 0, 0)),
        ));
        let mut front = Layer::new("fg", 1);
        front.push(AnimCell::still(1, 0, Cell::CLEAR));
        front.push(AnimCell::still(
            0,
            0,
            Cell::new('@', Rgb::new(255, 255, 255), Rgb::new(0, 0, 0)),
        ));
        scene.layers.push(back);
        scene.layers.push(front);
        scene
            .partitions
            .push(Partition::new("all", 0, 0, 4, 1, vec![0, 1], false));

        let frame = compose(&scene, &Camera::default(), 0.0);
        assert_eq!(frame.get(0, 0).unwrap().glyph, '@');
        // (1,0): front cell is transparent, nothing below -> stays clear.
        assert!(frame.get(1, 0).unwrap().transparent);
    }

    #[test]
    fn camera_partition_scrolls_world_but_not_hud() {
        let mut scene = Scene::default();
        let mut world = Layer::new("world", 0);
        world.push(AnimCell::still(
            5,
            2,
            Cell::new('T', Rgb::new(0, 255, 0), Rgb::new(0, 0, 0)),
        ));
        scene.layers.push(world);
        scene
            .partitions
            .push(Partition::new("view", 0, 0, 8, 4, vec![0], true));
        scene
            .partitions
            .push(Partition::new("hud", 8, 0, 8, 4, vec![0], false));

        let camera = Camera {
            x: 5,
            y: 2,
            ox: 0.0,
            oy: 0.0,
        };
        let frame = compose(&scene, &camera, 0.0);
        // World cell (5,2) appears at the view origin; the hud shows it raw.
        assert_eq!(frame.get(0, 0).unwrap().glyph, 'T');
        assert_eq!(frame.get(13, 2).unwrap().glyph, 'T');
    }

    #[test]
    fn wave_displaces_row_over_time() {
        use std::f32::consts::TAU;
        let mut scene = Scene::default();
        let mut fx = Layer::new("fx", 1);
        // One shared wave shape; per-cell phase staggers columns into a
        // crest traveling toward +x (left-to-right).
        for x in 0..8 {
            fx.push(AnimCell::animated(
                x,
                2,
                Cell::new('~', Rgb::new(80, 160, 255), Rgb::new(0, 0, 0)),
                Anim::still().with(Track::Wave {
                    axis: Axis::Y,
                    amp: 1.0,
                    wavelength: 8.0,
                    period: 2.0,
                    phase: x as f32 * TAU / 8.0,
                    brighten: 0.0,
                }),
            ));
        }
        scene.layers.push(fx);
        scene
            .partitions
            .push(Partition::new("all", 0, 0, 8, 5, vec![0], false));

        let rows = |t: f32| compose(&scene, &Camera::default(), t).rows();
        let r0 = rows(0.0);
        let r1 = rows(0.5);
        assert_ne!(r0, r1, "wave must move between frames");
        // Column 0: sway starts at +PI/2 (row 3), reaches 0 (row 2) a quarter
        // period later; column 2 does the reverse: 0 (row 2) -> +PI/2 (row 3).
        assert_eq!(r0[3].chars().next().unwrap(), '~');
        assert_eq!(r1[2].chars().next().unwrap(), '~');
        assert_eq!(r0[2].chars().nth(2).unwrap(), '~');
        assert_eq!(r1[3].chars().nth(2).unwrap(), '~');
    }

    #[test]
    fn pulse_and_cycle_apply_to_resolved_cell() {
        let mut scene = Scene::default();
        let mut fx = Layer::new("fx", 0);
        fx.push(AnimCell::animated(
            0,
            0,
            Cell::new('o', Rgb::new(100, 100, 100), Rgb::new(0, 0, 0)),
            Anim::still()
                .with(Track::Pulse {
                    low: 0.5,
                    high: 1.0,
                    period: 2.0,
                    phase: 0.0,
                })
                .with(Track::Cycle {
                    frames: vec!['o', 'O'],
                    fps: 1.0,
                }),
        ));
        scene.layers.push(fx);
        scene
            .partitions
            .push(Partition::new("all", 0, 0, 2, 1, vec![0], false));

        let dim = compose(&scene, &Camera::default(), 0.0);
        let lit = compose(&scene, &Camera::default(), 1.0);
        assert_eq!(dim.get(0, 0).unwrap().glyph, 'o');
        assert_eq!(lit.get(0, 0).unwrap().glyph, 'O');
        assert!(lit.get(0, 0).unwrap().fg.r > dim.get(0, 0).unwrap().fg.r);
    }

    fn glow_scene() -> Scene {
        // Emitter centered in a 13x7 frame: corners sit beyond the
        // radius-4 spill and stay pure black as control cells.
        let mut scene = Scene::default();
        let mut layer = Layer::new("fx", 0);
        layer.push(AnimCell::still(
            6,
            3,
            Cell::new('*', Rgb::new(200, 100, 0), Rgb::new(0, 0, 0)).with_glow(1.0),
        ));
        scene.layers.push(layer);
        scene
            .partitions
            .push(Partition::new("all", 0, 0, 13, 7, vec![0], false));
        scene
    }

    #[test]
    fn glow_washes_neighbors_with_falloff() {
        let frame = compose(&glow_scene(), &Camera::default(), 0.0);
        let core = frame.get(6, 3).unwrap();
        let near = frame.get(7, 3).unwrap();
        let far = frame.get(0, 0).unwrap();
        // Neighbors pick up the source hue, closer ones more.
        assert!(near.bg.r > far.bg.r);
        assert!(near.bg.r > frame.get(8, 3).unwrap().bg.r);
        assert_eq!(far.bg, Rgb::new(0, 0, 0), "corners stay black");
        // The wash carries hue, not white: red dominates blue.
        assert!(near.bg.r > near.bg.b);
        // Core glyph itself is untouched by its own wash target check.
        assert_eq!(core.glyph, '*');
    }

    #[test]
    fn glow_ignores_transparent_and_stays_deterministic() {
        let mut scene = glow_scene();
        // Transparent emitter on top changes nothing: still a hole, no wash.
        scene.layers[0].push(AnimCell::still(0, 0, Cell::CLEAR));
        let a = compose(&scene, &Camera::default(), 0.3);
        let b = compose(&scene, &Camera::default(), 0.3);
        assert!(a.get(0, 0).unwrap().transparent);
        assert_eq!(a.get(0, 0).unwrap().bg, Rgb::new(0, 0, 0));
        assert_eq!(
            a.cells.iter().map(|c| (c.glyph, c.bg)).collect::<Vec<_>>(),
            b.cells.iter().map(|c| (c.glyph, c.bg)).collect::<Vec<_>>()
        );
    }

    #[test]
    fn glow_reaches_measured_reference_distance() {
        // Burst-core forensics: halo gone by ~5 cells. A glow-1.0 source
        // must tint distance 4 but leave distance 7 black on empty ground.
        let frame = compose(&glow_scene(), &Camera::default(), 0.0);
        assert_ne!(
            frame.get(6 + 4, 3).unwrap().bg,
            Rgb::new(0, 0, 0),
            "halo reaches 4 cells"
        );
        assert_eq!(
            frame.get(6 - 6, 0).unwrap().bg,
            Rgb::new(0, 0, 0),
            "halo gone by 7 cells"
        );
    }

    #[test]
    fn wash_warms_backgrounds_and_clamps() {
        // Wash works on stamped opaque cells (blank void stays transparent
        // and is tinted by the presenter clear color instead).
        let mut scene = Scene::default();
        let mut layer = Layer::new("ground", 0);
        layer.push(AnimCell::still(
            1,
            1,
            Cell::new('.', Rgb::new(10, 10, 10), Rgb::new(0, 0, 0)),
        ));
        scene.layers.push(layer);
        scene
            .partitions
            .push(Partition::new("all", 0, 0, 3, 3, vec![0], false));
        scene.wash = Some(Wash {
            tint: Rgb::new(100, 50, 0),
            strength: 0.5,
        });
        let frame = compose(&scene, &Camera::default(), 0.0);
        // Stamped black ground gains exactly half the tint.
        assert_eq!(frame.get(1, 1).unwrap().bg, Rgb::new(50, 25, 0));
        // Glyph itself survives the wash.
        assert_eq!(frame.get(1, 1).unwrap().glyph, '.');
        // Void stays transparent (presenter clear color handles it).
        assert!(frame.get(0, 0).unwrap().transparent);
        scene.wash = Some(Wash {
            tint: Rgb::new(255, 255, 255),
            strength: 9.0,
        });
        let frame = compose(&scene, &Camera::default(), 0.0);
        assert_eq!(frame.get(1, 1).unwrap().bg, Rgb::new(255, 255, 255));
    }

    #[test]
    fn dynamic_color_drives_glow() {
        let mut scene = Scene::default();
        let mut layer = Layer::new("fx", 0);
        layer.push(
            AnimCell::still(
                2,
                1,
                Cell::new('*', Rgb::new(255, 255, 255), Rgb::new(0, 0, 0)).with_glow(1.0),
            )
            .with_color(ColorTrack::Rainbow {
                period: 2.0,
                phase: 0.0,
                saturation: 1.0,
                value: 1.0,
            }),
        );
        scene.layers.push(layer);
        scene
            .partitions
            .push(Partition::new("all", 0, 0, 5, 3, vec![0], false));

        let first = compose(&scene, &Camera::default(), 0.0).get(1, 1).unwrap();
        let second = compose(&scene, &Camera::default(), 0.5).get(1, 1).unwrap();
        assert_ne!(first.bg, second.bg, "resolved emitter color must tint glow");
    }

    #[test]
    fn invalid_and_extreme_glow_is_bounded() {
        let mut scene = glow_scene();
        scene.layers[0].cells[0].cell.glow = f32::INFINITY;
        let invalid = compose(&scene, &Camera::default(), 0.0);
        assert_eq!(invalid.get(7, 3).unwrap().bg, Rgb::new(0, 0, 0));
        scene.layers[0].cells[0].cell.glow = f32::MAX;
        let extreme = compose(&scene, &Camera::default(), 0.0);
        assert!(extreme.get(7, 3).unwrap().bg.r > 0);
    }
}
