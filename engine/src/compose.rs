use super::{emission::EmissionSource, sample, Camera, Cell, ColorTrack, Rgb, Scene, Wash};

/// One flattened frame: resolved glyphs + style per grid cell. This is what a
/// presenter (terminal today, windowed GUI next) draws; the engine itself
/// never touches I/O.
#[derive(Clone, Debug)]
pub struct Frame {
    pub w: i32,
    pub h: i32,
    pub cells: Vec<Cell>,
    /// Cells contributed by a camera/world partition. World cells are kept
    /// separate from pinned interface cells so the presenter can enforce the
    /// renderer contract: the world has a pure black void and only glyph
    /// foregrounds carry environmental colour.
    world_cells: Vec<bool>,
    /// Glow sources retained independently of the visible topmost cell. This
    /// preserves lower-layer emitters for float bloom after composition.
    emission_sources: Vec<EmissionSource>,
}

impl Frame {
    pub fn blank(w: i32, h: i32) -> Self {
        Self {
            w,
            h,
            cells: vec![Cell::CLEAR; w.max(0) as usize * h.max(0) as usize],
            world_cells: vec![false; w.max(0) as usize * h.max(0) as usize],
            emission_sources: Vec::new(),
        }
    }

    pub fn get(&self, x: i32, y: i32) -> Option<Cell> {
        if x < 0 || y < 0 || x >= self.w || y >= self.h {
            return None;
        }
        self.cells.get((y * self.w + x) as usize).copied()
    }

    fn set(&mut self, x: i32, y: i32, cell: Cell) {
        self.set_partition_cell(x, y, cell, false);
    }

    fn set_partition_cell(&mut self, x: i32, y: i32, cell: Cell, world: bool) {
        if x < 0 || y < 0 || x >= self.w || y >= self.h {
            return;
        }
        let index = (y * self.w + x) as usize;
        self.cells[index] = cell;
        self.world_cells[index] = world;
    }

    /// Whether a composed cell belongs to the camera/world viewport rather
    /// than a pinned HUD or modal partition.
    pub fn is_world(&self, x: i32, y: i32) -> bool {
        if x < 0 || y < 0 || x >= self.w || y >= self.h {
            return false;
        }
        self.world_cells[(y * self.w + x) as usize]
    }

    /// Bounded world glow sources sampled before lighting, including emitters
    /// hidden under a later opaque layer. UI and noncontributors are excluded.
    pub fn emission_sources(&self) -> &[EmissionSource] {
        &self.emission_sources
    }

    fn retain_world_emission(&mut self, x: i32, y: i32, cell: Cell) {
        if self.emission_sources.len() < super::emission::MAX_SOURCE_COUNT {
            if let Some(source) = EmissionSource::from_world_cell(x, y, cell) {
                self.emission_sources.push(source);
            }
        }
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
    compose_inner(scene, camera, t, false)
}

/// Compose raw world foregrounds for continuous emission/bloom. World glow is
/// retained as sources instead of being baked into 8-bit foreground colours;
/// pinned UI glow and the scene's background wash keep their legacy behaviour.
pub fn compose_for_continuous(scene: &Scene, camera: &Camera, t: f32) -> Frame {
    compose_inner(scene, camera, t, true)
}

fn compose_inner(scene: &Scene, camera: &Camera, t: f32, continuous: bool) -> Frame {
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
    // Keep light emitters independent from the visible topmost cell. A glow
    // source can intentionally sit underneath a later world layer (for
    // example, a projected torch halo under the rasterized terrain). If we
    // only inspect `frame.cells` after compositing, that source disappears as
    // soon as the terrain glyph wins the same cell.
    let mut glow_sources: Vec<(i32, i32, Cell, bool)> = Vec::new();

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
                        if resolved.glow.is_finite() && resolved.glow > 0.0 {
                            if partition.camera {
                                frame.retain_world_emission(x, y, resolved);
                            }
                            if !continuous || !partition.camera {
                                glow_sources.push((x, y, resolved, partition.camera));
                            }
                        }
                        frame.set_partition_cell(x, y, resolved, partition.camera);
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
                if resolved.glow.is_finite() && resolved.glow > 0.0 {
                    if partition.camera {
                        frame.retain_world_emission(x, y, resolved);
                    }
                    if !continuous || !partition.camera {
                        glow_sources.push((x, y, resolved, partition.camera));
                    }
                }
                frame.set_partition_cell(x, y, resolved, partition.camera);
            }
        }
    }
    apply_glow(&mut frame, &glow_sources);
    if let Some(wash) = scene.wash {
        apply_wash(&mut frame, wash);
    }
    // Final world contract: camera partitions are foreground-only. This is
    // repeated after glow/wash so a legacy world layer cannot smuggle a
    // coloured fill back into the viewport between composition and display.
    for (cell, &is_world) in frame.cells.iter_mut().zip(&frame.world_cells) {
        if is_world {
            cell.bg = Rgb::new(0, 0, 0);
        }
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
                if cell.transparent || frame.is_world(x, y) {
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
/// reach = glow × 5 cells, quadratic falloff. World emitters use a stronger
/// amplitude than pinned UI emitters because world partitions deliberately
/// keep their backgrounds black; existing world ink needs enough foreground
/// lift to read as luminance at the low logical resolution.
const GLOW_RADIUS_PER_UNIT: f32 = 5.0;
const GLOW_AMPLITUDE: f32 = 0.12;
const WORLD_GLOW_AMPLITUDE: f32 = 0.42;

/// Light buffer: every stamped cell with `glow > 0` washes its foreground
/// color onto neighboring cells with quadratic falloff (reference torch halos
/// and spell-core blooms tint nearby glyphs). Runs after all partitions so
/// halos cross partition borders like real light; world partitions lift
/// existing foreground ink while pinned UI partitions wash their backgrounds.
fn apply_glow(frame: &mut Frame, glow_sources: &[(i32, i32, Cell, bool)]) {
    // Index-based scan: the old per-cell `get()` paid bounds checks plus an
    // Option unwrap on every frame cell; the spill below does one manual
    // bounds check per target instead of two (get + set). Behavior — which
    // cells wash and by how much — is unchanged.
    let w = frame.w;
    let h = frame.h;
    for &(sx, sy, mut source, source_is_world) in glow_sources {
        source.glow = source.glow.min(8.0);
        let radius = (source.glow * GLOW_RADIUS_PER_UNIT).ceil() as i32;
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                let dist = ((dx * dx + dy * dy) as f32).sqrt();
                if dist > radius as f32 + 0.5 {
                    continue;
                }
                let fall = source.glow * (1.0 - dist / (radius as f32 + 1.0));
                let amplitude = if source_is_world {
                    WORLD_GLOW_AMPLITUDE
                } else {
                    GLOW_AMPLITUDE
                };
                let fall = fall * fall * amplitude;
                if fall <= 0.0 {
                    continue;
                }
                let (tx, ty) = (sx + dx, sy + dy);
                if tx < 0 || ty < 0 || tx >= w || ty >= h {
                    continue;
                }
                let target_index = (ty * w + tx) as usize;
                let target = &mut frame.cells[target_index];
                let target_is_world = frame.world_cells[target_index];
                if target_is_world {
                    // World light is ink-only: brighten/tint an existing
                    // visible glyph, but never paint a halo into an empty
                    // space cell. This is the character-cell equivalent of a
                    // light buffer with no coloured framebuffer background.
                    if source_is_world && target.glyph != ' ' {
                        let tint = (fall * 0.48).clamp(0.0, 0.48);
                        target.fg = target.fg.lerp(source.fg, tint).scaled(1.0 + fall * 0.55);
                    }
                } else if !source_is_world {
                    let wash = |bg: u8, fg: u8| {
                        (f32::from(bg) + fall * f32::from(fg)).clamp(0.0, 255.0) as u8
                    };
                    target.bg = Rgb::new(
                        wash(target.bg.r, source.fg.r),
                        wash(target.bg.g, source.fg.g),
                        wash(target.bg.b, source.fg.b),
                    );
                }
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
    fn world_partition_forces_black_cell_backgrounds() {
        let mut scene = Scene::default();
        let mut world = Layer::new("world", 0);
        world.push(AnimCell::still(
            0,
            0,
            Cell::new('@', Rgb::new(120, 220, 140), Rgb::new(80, 40, 20)).with_glow(1.0),
        ));
        scene.layers.push(world);
        scene
            .partitions
            .push(Partition::new("world", 0, 0, 3, 1, vec![0], true));
        scene.wash = Some(Wash {
            tint: Rgb::new(255, 120, 20),
            strength: 1.0,
        });

        let frame = compose(&scene, &Camera::default(), 0.0);
        let cell = frame.get(0, 0).expect("world cell");
        assert!(frame.is_world(0, 0));
        assert_eq!(cell.bg, Rgb::new(0, 0, 0));
        assert_ne!(cell.fg, Rgb::new(0, 0, 0));
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
    fn glow_source_survives_a_later_layer_at_the_same_cell() {
        let mut scene = Scene::default();
        let mut emitter = Layer::new("emitter", -1);
        emitter.push(AnimCell::still(
            3,
            2,
            Cell::new(' ', Rgb::new(255, 120, 20), Rgb::new(0, 0, 0)).with_glow(1.0),
        ));
        let mut terrain = Layer::new("terrain", 0);
        terrain.push(AnimCell::still(
            3,
            2,
            Cell::new('.', Rgb::new(20, 20, 20), Rgb::new(0, 0, 0)),
        ));
        terrain.push(AnimCell::still(
            4,
            2,
            Cell::new('.', Rgb::new(20, 20, 20), Rgb::new(0, 0, 0)),
        ));
        scene.layers.push(emitter);
        scene.layers.push(terrain);
        scene
            .partitions
            .push(Partition::new("world", 0, 0, 8, 5, vec![0, 1], true));

        let frame = compose(&scene, &Camera::default(), 0.0);
        assert_eq!(frame.get(3, 2).unwrap().glyph, '.');
        assert!(
            frame.get(4, 2).unwrap().fg.r > 20,
            "a hidden lower-layer emitter must still warm nearby world ink"
        );
        assert!(
            frame.get(4, 2).unwrap().fg.r >= 50,
            "world glow must be visibly brighter than normal terrain colouring"
        );
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

    #[test]
    fn continuous_composition_replaces_legacy_world_glow() {
        let mut scene = Scene::default();
        let mut world = Layer::new("world", 0);
        world.push(AnimCell::still(
            0,
            0,
            Cell::new('*', Rgb::new(255, 0, 0), Rgb::new(0, 0, 0)).with_glow(1.0),
        ));
        let ink = Cell::new('#', Rgb::new(20, 20, 20), Rgb::new(0, 0, 0))
            .with_fg_gradient(Rgb::new(5, 10, 15), Axis::X);
        world.push(AnimCell::still(1, 0, ink));
        scene.layers.push(world);
        scene
            .partitions
            .push(Partition::new("world", 0, 0, 2, 1, vec![0], true));

        let legacy = compose(&scene, &Camera::default(), 0.0);
        let mut continuous =
            crate::compose::compose_for_continuous(&scene, &Camera::default(), 0.0);
        assert_eq!(legacy.get(1, 0).unwrap().fg, Rgb::new(60, 19, 19));
        assert_eq!(continuous.get(1, 0), Some(ink));

        let field = crate::emission::BloomField::from_frame(&continuous);
        field.apply_to_frame_world_ink(&mut continuous);
        let lit = continuous.get(1, 0).unwrap();
        assert_eq!(lit.fg, Rgb::new(77, 20, 20), "float spill is applied once");
        assert_eq!(lit.fg_end, Rgb::new(62, 10, 15));
        assert_eq!(lit.bg, Rgb::new(0, 0, 0));
    }

    #[test]
    fn continuous_hud_over_world_keeps_legacy_ui_glow_and_wash() {
        let mut scene = Scene::default();
        let mut world = Layer::new("world", 0);
        world.push(AnimCell::still(
            0,
            0,
            Cell::new('*', Rgb::new(255, 0, 0), Rgb::new(0, 0, 0)).with_glow(1.0),
        ));
        let ink = Cell::new('#', Rgb::new(20, 20, 20), Rgb::new(0, 0, 0));
        world.push(AnimCell::still(1, 0, ink));
        let mut hud = Layer::new("hud", 1);
        hud.push(AnimCell::still(
            0,
            0,
            Cell::new('H', Rgb::new(0, 255, 0), Rgb::new(1, 2, 3)).with_glow(1.0),
        ));
        scene.layers = vec![world, hud];
        scene.partitions = vec![
            Partition::new("world", 0, 0, 2, 1, vec![0], true),
            Partition::new("hud", 0, 0, 1, 1, vec![1], false),
        ];
        scene.wash = Some(Wash {
            tint: Rgb::new(10, 20, 30),
            strength: 0.5,
        });

        let legacy = compose(&scene, &Camera::default(), 0.0);
        let mut continuous = compose_for_continuous(&scene, &Camera::default(), 0.0);
        let ui = continuous.get(0, 0).unwrap();
        assert!(!continuous.is_world(0, 0));
        assert_eq!(ui, legacy.get(0, 0).unwrap());
        assert_eq!(ui.bg, Rgb::new(6, 42, 18));
        assert_eq!(
            continuous.get(1, 0),
            Some(ink),
            "UI glow cannot light world ink"
        );

        let field = crate::emission::BloomField::from_frame(&continuous);
        assert_eq!(
            field.source_count(),
            1,
            "only the hidden world source emits"
        );
        field.apply_to_frame_world_ink(&mut continuous);
        assert_eq!(continuous.get(0, 0), Some(ui));
        assert!(!continuous.is_world(0, 0));
        assert_eq!(continuous.get(1, 0).unwrap().fg, Rgb::new(77, 20, 20));
    }

    #[test]
    fn continuous_source_budget_skips_ui_and_invalid_world_emitters() {
        use crate::emission::MAX_SOURCE_COUNT;

        let red = Rgb::new(255, 0, 0);
        let mut scene = Scene::default();
        let mut hud = Layer::new("hud", 0);
        let mut world = Layer::new("world", 0);
        for _ in 0..=MAX_SOURCE_COUNT {
            hud.push(AnimCell::still(
                0,
                0,
                Cell::new('H', red, Rgb::new(0, 0, 0)).with_glow(1.0),
            ));
            for (color, glow) in [
                (Rgb::new(0, 0, 0), 1.0),
                (red, 0.0),
                (red, f32::NAN),
                (red, f32::INFINITY),
            ] {
                let mut cell = Cell::new('*', color, Rgb::new(0, 0, 0));
                cell.glow = glow;
                world.push(AnimCell::still(0, 0, cell));
            }
        }
        let emitter = Cell::new('*', Rgb::new(255, 90, 20), Rgb::new(0, 0, 0)).with_glow(1.0);
        world.push(AnimCell::still(1, 0, emitter));
        scene.layers = vec![hud, world];
        scene.partitions = vec![
            Partition::new("hud", 0, 0, 2, 1, vec![0], false),
            Partition::new("world", 0, 0, 2, 1, vec![1], true),
        ];

        let frame = compose_for_continuous(&scene, &Camera::default(), 0.0);
        assert_eq!(frame.emission_sources().len(), 1);
        let sources = EmissionSource::from_frame(&frame);
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].color, emitter.fg);
        assert!(sources[0].world);

        for _ in 0..=MAX_SOURCE_COUNT {
            scene.layers[1].push(AnimCell::still(1, 0, emitter));
        }
        let capped = compose_for_continuous(&scene, &Camera::default(), 0.0);
        assert_eq!(capped.emission_sources().len(), MAX_SOURCE_COUNT);
        assert_eq!(EmissionSource::from_frame(&capped).len(), MAX_SOURCE_COUNT);
    }

    #[test]
    fn continuous_retains_hidden_fractional_emitters_in_both_sampling_paths() {
        use crate::emission::EmissionPosition;
        use crate::Point2;

        for animated in [false, true] {
            let mut scene = Scene::default();
            let mut glow = Layer::new("glow", -1);
            let mut emitter = AnimCell::still(
                5,
                3,
                Cell::new(' ', Rgb::new(255, 80, 20), Rgb::new(0, 0, 0))
                    .with_glow(1.0)
                    .with_offset(0.25, -0.5),
            );
            if animated {
                emitter.anim = Anim::still().with(Track::Pulse {
                    low: 0.5,
                    high: 0.5,
                    period: 1.0,
                    phase: 0.0,
                });
            }
            glow.push(emitter);
            let mut terrain = Layer::new("terrain", 0);
            let ink = Cell::new('#', Rgb::new(20, 20, 20), Rgb::new(0, 0, 0));
            terrain.push(AnimCell::still(5, 3, ink));
            scene.layers = vec![glow, terrain];
            scene
                .partitions
                .push(Partition::new("world", 2, 1, 4, 3, vec![0, 1], true));
            let camera = Camera {
                x: 4,
                y: 2,
                ox: 0.3,
                oy: 0.7,
            };

            let frame = compose_for_continuous(&scene, &camera, 0.25);
            assert_eq!(frame.get(3, 2), Some(ink));
            let sources = EmissionSource::from_frame(&frame);
            assert_eq!(sources.len(), 1);
            assert_eq!(
                sources[0].position,
                EmissionPosition::Continuous(Point2::new(3.25, 1.5))
            );
            assert_eq!(
                sources[0].color,
                if animated {
                    Rgb::new(127, 40, 10)
                } else {
                    Rgb::new(255, 80, 20)
                }
            );
        }
    }

    #[test]
    fn legacy_glow_is_not_capped_by_the_continuous_source_budget() {
        for (world, animated) in [(false, false), (false, true), (true, false), (true, true)] {
            let mut scene = Scene::default();
            let mut layer = Layer::new("emitters", 0);
            let emitter = Cell::new('*', Rgb::new(255, 0, 0), Rgb::new(0, 0, 0)).with_glow(1.0);
            for _ in 0..=crate::emission::MAX_SOURCE_COUNT {
                layer.push(AnimCell::still(0, 0, emitter));
            }
            // The last emitter is isolated from the first 129. Check both
            // sampling paths independently for an accidental legacy cap.
            let anim = if animated {
                Anim::still().with(Track::Pulse {
                    low: 1.0,
                    high: 1.0,
                    period: 1.0,
                    phase: 0.0,
                })
            } else {
                Anim::still()
            };
            layer.push(AnimCell::animated(16, 0, emitter, anim));
            layer.push(AnimCell::still(
                17,
                0,
                Cell::new('#', Rgb::new(20, 20, 20), Rgb::new(0, 0, 0)),
            ));
            scene.layers.push(layer);
            scene
                .partitions
                .push(Partition::new("all", 0, 0, 18, 1, vec![0], world));

            let frame = compose(&scene, &Camera::default(), 0.0);
            let target = frame.get(17, 0).unwrap();
            if world {
                assert_eq!(target.fg, Rgb::new(60, 19, 19));
            } else {
                assert_eq!(target.bg, Rgb::new(21, 0, 0));
            }
        }
    }
}
