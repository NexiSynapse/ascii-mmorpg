//! Playable windowed port with real depth: the local server
//! [`Simulation`](ascii_mmorpg_server) ticks at 20 Hz while the world is
//! rasterized through the 3D depth backend (`raster::`) behind an OSRS-style
//! elevated orbit camera. WASD/arrows walk relative to what the camera faces
//! (screen-up = camera forward, snapped by motion to the step grid), hold the
//! right mouse button and drag to orbit, wheel to zoom, `F` attacks, `1-3`
//! cast, `I` opens the inventory, `R` rests, `Q`/Esc quits.
//!
//! Every frame builds a mesh list (zoned ground, ocean sheet, trees, item
//! drops, humanoid actors, combat FX as world-space decals and boxes), one
//! `raster::render_layer` pass carves it into the world partition, and a
//! screen-space label layer floats nameplates + damage digits over the same
//! camera projection the rasterizer used. FX state is observed presentation
//!-side from sim messages (`SceneFx`), exactly like the bridge path.
//!
//! Run: `cargo run -p ascii-mmorpg-engine --features present --example play`
//! Flags: `--frames N` exits after N presented frames (automated proof);
//! `--check` runs a headless movement + combat + modal self-test (no window).
//!
//! Still loopback this slice: one local player, no TCP yet. The terminal
//! client is untouched.

#[cfg(not(feature = "present"))]
fn main() {
    eprintln!("this example needs `--features present` (winit + wgpu + fontdue)");
    std::process::exit(2);
}

#[cfg(feature = "present")]
fn main() {
    if let Err(err) = real_main() {
        eprintln!("play failed: {err}");
        std::process::exit(1);
    }
}

#[cfg(feature = "present")]
fn real_main() -> Result<(), String> {
    use ascii_mmorpg_engine::motion::{smooth_angle, KinematicMotion};
    use ascii_mmorpg_engine::present::{GpuWorld, PointerEvent, PresentOptions, SceneSource};
    use ascii_mmorpg_engine::raster::{
        self, Camera3, DepthOpts, HumanoidPose, Mesh, PointLight, Transform, Vec3,
    };
    use ascii_mmorpg_engine::{
        bridge, overlay::TextRun, Camera, Cell, Layer, Partition, Rgb, Scene,
    };
    use std::collections::{HashMap, HashSet};
    use std::time::Duration;
    use winit::keyboard::KeyCode;

    use ascii_mmorpg_server::{FixedTickClock, Simulation};
    use ascii_mmorpg_shared::{
        ClientCommand, Command, Direction, EntityId, EntityKind, Tick, WorldSnapshot,
    };

    const PLAYER_ID: EntityId = 1;
    const TICK_HZ: u64 = 20;
    /// Stall cap: a dragged window must not repay minutes of ticks in one
    /// frame (strict-scheduler backlog is dropped, matching burst coalescing).
    const MAX_TICKS_PER_FRAME: u32 = 5;

    /// Frame grid: HUD band on top, 3D world in the middle, log strip below
    /// (cell_px 8 → a 1536x560 window, same shape as the bridge layout).
    const GRID_W: u32 = 160;
    const GRID_H: u32 = 56;
    const HUD_H: i32 = 7;
    const LOG_H: i32 = 5;

    /// Camera defaults: OSRS-mid pull-back looking down at the player.
    const CAM_DIST: f32 = 13.5;
    const CAM_PITCH: f32 = -0.68; // close readable OSRS gameplay framing
    const CAM_PITCH_MIN: f32 = -1.12;
    const CAM_PITCH_MAX: f32 = -0.35;
    const CAM_DIST_MIN: f32 = 7.0;
    const CAM_DIST_MAX: f32 = 24.0;
    /// Focal (cell units): at the 17-unit pull-back one world unit spans
    /// ~2 cells, so the 192-column view covers ~96 world cells — the same
    /// wing as the bridge camera.
    const CAM_FOCAL: f32 = 48.0;
    const ATTACK_PHASE_SECONDS: f32 = 0.42;
    const FACING_RESPONSE: f32 = 18.0;

    // Panel palette (dark slate HUD, gold accents — the TUI modal's tones).
    const PANEL_BG: Rgb = Rgb::new(10, 14, 24);
    const PANEL_EDGE: Rgb = Rgb::new(215, 175, 95);
    const HUD_TEXT: Rgb = Rgb::new(238, 232, 216);
    const HUD_DIM: Rgb = Rgb::new(164, 180, 202);
    const HP_FILL: Rgb = Rgb::new(220, 60, 50);
    const HP_EMPTY: Rgb = Rgb::new(90, 30, 26);
    const LABEL_BG: Rgb = Rgb::new(0, 0, 0);
    const DIGIT_GOLD: Rgb = Rgb::new(255, 230, 160);
    const TARGET_ORANGE: Rgb = Rgb::new(255, 170, 58);

    /// Centralized foreground-only world palette. These colours are albedos
    /// for visible geometry and are resolved into glyph foregrounds by the
    /// depth rasterizer; none of them is used as a world-cell background.
    #[derive(Clone, Copy)]
    struct WorldPalette {
        foliage_shadow: Rgb,
        foliage: Rgb,
        foliage_moon: Rgb,
        bark: Rgb,
        root: Rgb,
        path: Rgb,
        path_edge: Rgb,
        grass: Rgb,
        stone: Rgb,
        stone_light: Rgb,
        water: Rgb,
        sand: Rgb,
        fire: Rgb,
        ember: Rgb,
        magic: Rgb,
        shadow: Rgb,
    }

    const WORLD: WorldPalette = WorldPalette {
        foliage_shadow: Rgb::new(28, 72, 44),
        foliage: Rgb::new(44, 108, 58),
        foliage_moon: Rgb::new(86, 156, 112),
        bark: Rgb::new(92, 58, 38),
        root: Rgb::new(62, 44, 34),
        path: Rgb::new(74, 76, 82),
        path_edge: Rgb::new(142, 116, 72),
        grass: Rgb::new(54, 112, 60),
        stone: Rgb::new(78, 88, 108),
        stone_light: Rgb::new(132, 144, 158),
        water: Rgb::new(38, 92, 128),
        sand: Rgb::new(132, 104, 66),
        fire: Rgb::new(255, 148, 48),
        ember: Rgb::new(255, 196, 76),
        magic: Rgb::new(154, 92, 220),
        shadow: Rgb::new(12, 18, 22),
    };

    /// Deterministic scenery list (hand-placed so the highway corridor and
    /// the demo spawn area stay walkable; the small glyph meshes rebuild every
    /// frame — no mesh cache is needed at this scale).
    fn tree_spots() -> [(f32, f32); 15] {
        [
            (12.0, 6.0),
            (-9.0, 9.0),
            (20.0, -4.0),
            (-15.0, -8.0),
            (9.0, -14.0),
            (24.0, 18.0),
            (-22.0, 18.0),
            (16.0, 26.0),
            (-5.0, 24.0),
            (-26.0, 4.0),
            (28.0, 8.0),
            (-24.0, -8.0),
            (29.0, -15.0),
            (12.0, 32.0),
            (-19.0, 31.0),
        ]
    }

    fn scene_hash(x: i32, y: i32) -> u32 {
        let mut h = (x as u32).wrapping_mul(374_761_393) ^ (y as u32).wrapping_mul(668_265_263);
        h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
        h ^ (h >> 16)
    }

    /// Current zone label for the HUD (first "Zone: " message).
    fn scene_zone(snapshot: &WorldSnapshot) -> String {
        snapshot
            .messages
            .first()
            .and_then(|first| first.strip_prefix("Zone: "))
            .unwrap_or("Wilderness")
            .to_string()
    }

    /// Message -> SceneFx translation; `None` for non-combat lines. Melee
    /// hits resolve the target's world cell by display name (duplicate mob
    /// names pick the first — presentation ambiguity only), casts anchor at
    /// the caster. Hue is one per damage/event school (clarity study: FX
    /// are bright saturated single-hue ink on the dark stage).
    fn combat_fx_from_message(message: &str, snapshot: &WorldSnapshot) -> Option<bridge::SceneFx> {
        const EMBER: Rgb = Rgb::new(250, 140, 40);
        const ARC: Rgb = Rgb::new(247, 170, 45);
        const SPIRIT: Rgb = Rgb::new(140, 240, 255);
        const LIGHTNING: Rgb = Rgb::new(128, 184, 255);
        const NEUTRAL: Rgb = Rgb::new(200, 200, 215);

        let fx = |origin: (f32, f32), color: Rgb, radius: f32, telegraph: f32, digits: &str| {
            bridge::SceneFx {
                origin,
                born_s: 0.0, // patched by the caller's presentation clock
                radius,
                color,
                telegraph_s: telegraph,
                digits: digits.to_string(),
            }
        };
        if let Some(rest) = message.strip_prefix("You hit ") {
            let name_end = rest.find(" for ")?;
            let name = rest[..name_end].trim();
            let digits_end = rest.find(" damage")?;
            let digits: String = rest[name_end + 5..digits_end]
                .chars()
                .filter(|c| c.is_ascii_digit())
                .collect();
            let target = snapshot
                .entities
                .iter()
                .find(|entity| entity.name == name)?;
            return Some(fx(
                (target.position.x as f32, target.position.y as f32),
                EMBER,
                1.6,
                0.0,
                &digits,
            ));
        }
        if let Some(rest) = message.strip_prefix("You cast ") {
            let spell = rest
                .split(';')
                .next()
                .unwrap_or(rest)
                .trim()
                .to_ascii_lowercase();
            let (color, radius, telegraph) = if spell.contains("thunder") {
                (LIGHTNING, 3.0, 0.9)
            } else if spell.contains("iron") || spell.contains("will") {
                (SPIRIT, 1.8, 0.6)
            } else if spell.contains("cleave") {
                (ARC, 2.0, 0.7)
            } else {
                (NEUTRAL, 1.5, 0.8)
            };
            return Some(fx(
                (
                    snapshot.player.position.x as f32,
                    snapshot.player.position.y as f32,
                ),
                color,
                radius,
                telegraph,
                "",
            ));
        }
        None
    }

    /// Ground-projected camera basis, mirroring `raster::basis` (which is
    /// private): forward = (sin yaw, cos yaw) on the x/z plane.
    fn ground_frame(yaw: f32) -> ((f32, f32), (f32, f32)) {
        let (s, c) = yaw.sin_cos();
        ((s, c), (c, -s)) // (forward, right) ground vectors
    }

    /// Project a world point to the raster view's cell coords (same mapping
    /// as `rasterize_tri`'s fill math) so labels glue to 3D bodies.
    fn project(p: Vec3, cam: &Camera3) -> Option<(i32, i32)> {
        let (s, c) = cam.yaw.sin_cos();
        let (sp, cp) = cam.pitch.sin_cos();
        let fwd = Vec3::new(s * cp, sp, c * cp);
        let right = Vec3::new(c, 0.0, -s);
        let up = fwd.cross(right).norm();
        let rel = p - cam.pos;
        let depth = rel.dot(fwd);
        if depth < 0.4 {
            return None;
        }
        let sx = (cam.width as f32 / 2.0 + cam.focal * rel.dot(right) / depth).round() as i32;
        let sy = (cam.height as f32 / 2.0 - cam.focal * rel.dot(up) / depth).round() as i32;
        Some((sx, sy))
    }

    /// Screen-space text helpers (labels layer, partition-relative coords).
    struct LocalGame {
        sim: Simulation,
        clock: FixedTickClock,
        last_t: f32,
        client_tick: Tick,
        held: HashSet<KeyCode>,
        motion: KinematicMotion,
        pending: Vec<Command>,
        // Camera orbit state (presentation-only; the sim is world-space).
        cam_yaw: f32,
        cam_pitch: f32,
        cam_dist: f32,
        follow: Option<(f32, f32)>,
        /// Entity render positions smoothed toward snapshot positions.
        smoothed: HashMap<u64, (f32, f32)>,
        /// Presentation-side combat FX state (observed from sim messages).
        combat_fx: Vec<bridge::SceneFx>,
        seen_combat_events: Vec<String>,
        inv_open: bool,
        /// Mesh facing angle (yaw radians); starts facing east like the TUI.
        facing: f32,
        facing_target: f32,
        gait_phase: f32,
        attack_started_at: Option<f32>,
        attack_queued: bool,
        /// GPU raster (default) vs `--cpu` CPU fill.
        gpu3d: bool,
        /// Stashed during the last `scene_at_size`: meshes + camera for the
        /// presenter's GPU raster pass (consumed by `SceneSource::gpu_world`).
        pending_world: Option<GpuWorld>,
    }

    impl LocalGame {
        fn new() -> Self {
            Self::with_gpu3d(!std::env::args().any(|arg| arg == "--cpu"))
        }

        fn with_gpu3d(gpu3d: bool) -> Self {
            let mut sim = Simulation::demo();
            sim.connect_player(PLAYER_ID);
            Self {
                sim,
                clock: FixedTickClock::new(TICK_HZ),
                last_t: 0.0,
                client_tick: 0,
                held: HashSet::new(),
                motion: KinematicMotion::default(),
                pending: Vec::new(),
                cam_yaw: std::f32::consts::PI, // start looking north
                cam_pitch: CAM_PITCH,
                cam_dist: CAM_DIST,
                follow: None,
                smoothed: HashMap::new(),
                combat_fx: Vec::new(),
                seen_combat_events: Vec::new(),
                inv_open: false,
                facing: std::f32::consts::PI,
                facing_target: std::f32::consts::PI,
                gait_phase: 0.0,
                attack_started_at: None,
                attack_queued: false,
                gpu3d,
                pending_world: None,
            }
        }

        fn tick_once(&mut self) {
            self.client_tick = self.client_tick.saturating_add(1);
            let mut commands = Vec::new();
            for command in self.pending.drain(..) {
                commands.push((PLAYER_ID, ClientCommand::new(self.client_tick, command)));
            }
            self.sim.advance_tick_with(commands);
        }

        /// WASD/arrows held (screen axes): dx east-ish, dy south-ish.
        fn held_axes(&self) -> (f32, f32) {
            let north = self.held.contains(&KeyCode::KeyW) || self.held.contains(&KeyCode::ArrowUp);
            let south =
                self.held.contains(&KeyCode::KeyS) || self.held.contains(&KeyCode::ArrowDown);
            let east =
                self.held.contains(&KeyCode::KeyD) || self.held.contains(&KeyCode::ArrowRight);
            let west =
                self.held.contains(&KeyCode::KeyA) || self.held.contains(&KeyCode::ArrowLeft);
            let dx = (east as i8) - (west as i8);
            let dy = (south as i8) - (north as i8);
            (f32::from(dx), f32::from(dy))
        }

        /// Camera-relative input: screen-up walks the camera forward, snapped
        /// by the motion model to the 8 compass steps (same loop as the TUI's
        /// orbit mode, fed through `camera_axes` before `update`).
        fn camera_axes(&self) -> (f32, f32) {
            let (screen_dx, screen_dy) = self.held_axes();
            if screen_dx == 0.0 && screen_dy == 0.0 {
                return (0.0, 0.0);
            }
            let (fwd, right) = ground_frame(self.cam_yaw);
            let world_dx = fwd.0 * -screen_dy + right.0 * screen_dx;
            let world_dy = fwd.1 * -screen_dy + right.1 * screen_dx;
            (world_dx, world_dy)
        }

        /// Advance sim ticks + local motion for the elapsed frame time. Owns the
        /// `last_t` bookkeeping; callers reuse the returned `dt` for smoothing.
        fn advance(&mut self, t: f32) -> (f32, WorldSnapshot) {
            let dt = (t - self.last_t).max(0.0);
            self.last_t = t;
            let input = self.camera_axes();
            let input_len = input.0.hypot(input.1);
            if input_len > 0.01 {
                self.facing_target = f32::atan2(input.0, input.1);
            }
            self.facing = smooth_angle(self.facing, self.facing_target, FACING_RESPONSE, dt);
            if let Some((dx, dy)) = self.motion.update(input, dt) {
                self.pending.push(Command::Move(match (dx, dy) {
                    (0, -1) => Direction::North,
                    (0, 1) => Direction::South,
                    (1, 0) => Direction::East,
                    (-1, 0) => Direction::West,
                    (1, -1) => Direction::NorthEast,
                    (-1, -1) => Direction::NorthWest,
                    (1, 1) => Direction::SouthEast,
                    (-1, 1) => Direction::SouthWest,
                    _ => Direction::East,
                }));
            }
            let speed = self.motion.velocity().0.hypot(self.motion.velocity().1);
            let phase_dt = dt.clamp(0.0, 0.1);
            if input_len > 0.01 || speed > 0.12 {
                // Faster motion produces a quicker, but still bounded, stride.
                self.gait_phase = (self.gait_phase + phase_dt * (5.5 + speed * 2.2))
                    .rem_euclid(std::f32::consts::TAU);
            } else {
                self.gait_phase *= (-phase_dt * 5.0).exp();
                if self.gait_phase.abs() < 0.01 {
                    self.gait_phase = 0.0;
                }
            }
            if self.attack_queued {
                self.attack_started_at = Some(t);
                self.attack_queued = false;
            }
            for _ in 0..self
                .clock
                .advance(Duration::from_secs_f32(dt))
                .min(MAX_TICKS_PER_FRAME)
            {
                self.tick_once();
            }
            let snapshot = self.sim.snapshot_for(PLAYER_ID);
            self.observe_combat_fx(&snapshot, t);
            (dt, snapshot)
        }

        fn attack_phase(&self, t: f32) -> f32 {
            self.attack_started_at
                .map(|born| ((t - born) / ATTACK_PHASE_SECONDS).clamp(0.0, 1.0))
                .filter(|phase| *phase < 1.0)
                .unwrap_or(0.0)
        }

        /// Smoothed render position for one entity: glide toward the real
        /// cell at a fixed rate, snap when the server teleports further than
        /// a couple of cells (death/reposition). Presentation-only.
        fn smooth_pos(&mut self, id: u64, target: (f32, f32), dt: f32) -> (f32, f32) {
            let entry = self.smoothed.entry(id).or_insert(target);
            let drift = (target.0 - entry.0, target.1 - entry.1);
            if drift.0.hypot(drift.1) > 2.5 {
                *entry = target;
                return target;
            }
            let glide = 1.0 - (-dt * 14.0).exp();
            *entry = (entry.0 + drift.0 * glide, entry.1 + drift.1 * glide);
            *entry
        }

        /// Orbit camera for this frame: pan-glides toward the smoothed
        /// player anchor and looks down along the drag-yaw/pitch. The raster
        /// camera has no look-at, so pose = place the eye at `aim − fwd·d`
        /// (the forward ray then passes exactly through the anchor).
        fn camera3(&mut self, w: i32, world_h: i32, dt: f32, snapshot: &WorldSnapshot) -> Camera3 {
            let id = snapshot.player.id;
            let target = *self.smoothed.get(&id).unwrap_or(&(
                snapshot.player.position.x as f32,
                snapshot.player.position.y as f32,
            ));
            let next = match self.follow {
                None => target,
                Some(f) => {
                    let k = 1.0 - (-dt * 8.0).exp();
                    if (target.0 - f.0).hypot(target.1 - f.1) > 4.0 {
                        target
                    } else {
                        (f.0 + (target.0 - f.0) * k, f.1 + (target.1 - f.1) * k)
                    }
                }
            };
            self.follow = Some(next);
            let (sp, cp) = self.cam_pitch.sin_cos();
            let (sy, cy) = self.cam_yaw.sin_cos();
            let fwd = Vec3::new(sy * cp, sp, cy * cp);
            let aim = Vec3::new(next.0, 1.6, next.1); // hips anchor
            let eye = Vec3::new(
                aim.x - fwd.x * self.cam_dist,
                aim.y - fwd.y * self.cam_dist,
                aim.z - fwd.z * self.cam_dist,
            );
            // Preserve gameplay scale across resizes: a wider/taller window
            // should reveal more context without shrinking the controlled
            // character into a few cells.
            let width_scale = w as f32 / GRID_W as f32;
            let height_scale = world_h as f32 / (GRID_H as i32 - HUD_H - LOG_H) as f32;
            let focal_scale = width_scale.min(height_scale).clamp(0.82, 1.28);
            Camera3::new(
                eye,
                self.cam_yaw,
                self.cam_pitch,
                CAM_FOCAL * focal_scale,
                w,
                world_h,
            )
        }

        /// Presentation-side combat FX: watch the sim's message log the
        /// same way the terminal GPU renderer does (dedup by exact line),
        /// translate melee hits + spell casts into `bridge::SceneFx`,
        /// prune expired events. The sim stays authoritative; this is view
        /// state only.
        fn observe_combat_fx(&mut self, snapshot: &WorldSnapshot, t: f32) {
            for message in &snapshot.messages {
                if self.seen_combat_events.iter().any(|seen| seen == message) {
                    continue;
                }
                if let Some(mut event) = combat_fx_from_message(message, snapshot) {
                    // The translator is intentionally pure; stamp the actual
                    // observation time here so a delayed snapshot cannot make
                    // a fresh hit appear half-expired.
                    event.born_s = t;
                    self.combat_fx.push(event);
                }
                self.seen_combat_events.push(message.clone());
            }
            if self.seen_combat_events.len() > 64 {
                self.seen_combat_events
                    .drain(..self.seen_combat_events.len() - 64);
            }
            if self.combat_fx.len() > 16 {
                self.combat_fx.drain(..self.combat_fx.len() - 16);
            }
            self.combat_fx.retain(|fx| {
                t - fx.born_s < fx.telegraph_s + bridge::FX_BURST_S + bridge::FX_CORE_S
            });
        }

        /// Mesh list for one frame: ground zones + ocean + trees + items +
        /// actors + FX decals. World region: sim grid (x, y) maps to x/z
        /// plane, y is up. 1 unit = 1 cell. `dt` is this frame's elapsed
        /// time (already consumed by `advance`) and drives render smoothing.
        fn world_meshes(&mut self, snapshot: &WorldSnapshot, t: f32, dt: f32) -> Vec<Mesh> {
            let mut meshes = Vec::new();
            let player = self.smooth_pos(
                snapshot.player.id,
                (
                    snapshot.player.position.x as f32,
                    snapshot.player.position.y as f32,
                ),
                dt,
            );

            // The world has no coloured floor, skybox, fog card, or terrain
            // fill. Selective decals and small solids describe the path,
            // shoreline, grass, and stones; every other viewport cell is the
            // presenter's exact black clear colour.
            for z in -30..=30 {
                if z % 2 != 0 {
                    continue;
                }
                let skip = scene_hash(z, z + 17) % 5 == 0;
                if !skip {
                    for side in [-1.0f32, 1.0] {
                        let x = side * (3.1 + (scene_hash(z, 4) % 3) as f32 * 0.12);
                        meshes.push(Mesh::decal_y(
                            Vec3::new(x, 0.018, z as f32),
                            0.0,
                            0.48,
                            0.055,
                            WORLD.path_edge,
                        ));
                    }
                }
                if z % 6 == 0 {
                    meshes.push(Mesh::decal_y(
                        Vec3::new(-1.0 + (scene_hash(z, 8) % 3) as f32, 0.02, z as f32 + 0.5),
                        0.0,
                        0.34,
                        0.09,
                        WORLD.path,
                    ));
                }
                if z % 4 == 0 {
                    for (offset, yaw) in [(-1.65, 0.12), (1.55, -0.15)] {
                        meshes.push(Mesh::decal_y(
                            Vec3::new(offset, 0.021, z as f32 + 0.72),
                            yaw,
                            0.24,
                            0.07,
                            WORLD.path,
                        ));
                    }
                }
            }
            // A deterministic terrain weave keeps the meadow legible at
            // wide zoom without reverting to a solid floor. The repeated
            // 2-cell motif alternates direction by row, skips the highway
            // corridor, and leaves generous black negative space between
            // ink marks.
            for gz in (-28i32..=30).step_by(2) {
                let row = (gz + 28) / 2;
                for gx in (-24i32..=24).step_by(2) {
                    let column = (gx + 24) / 2;
                    let motif = (column + row * 3).rem_euclid(7);
                    if motif > 1 || (gx.abs() <= 4 && row % 3 != 0) {
                        continue;
                    }
                    let yaw = if (row + column) % 2 == 0 { 0.24 } else { -0.24 };
                    let color = if gx.abs() <= 4 {
                        WORLD.path
                    } else if motif == 0 {
                        WORLD.grass
                    } else {
                        WORLD.foliage_shadow
                    };
                    meshes.push(Mesh::decal_y(
                        Vec3::new(gx as f32 + (row % 2) as f32 * 0.18, 0.020, gz as f32 + 0.18),
                        yaw,
                        0.30,
                        0.065,
                        color,
                    ));
                }
            }
            // Dry shoreline and sparse wave crests remain readable as ink,
            // not as beach/water rectangles.
            for row in 0..8 {
                let z = 36.0 + row as f32 * 2.8;
                let color = if row < 2 { WORLD.sand } else { WORLD.water };
                for segment in -7..=7 {
                    if scene_hash(segment, row + 40) % 3 == 0 {
                        continue;
                    }
                    meshes.push(Mesh::decal_y(
                        Vec3::new(segment as f32 * 2.6, 0.01, z),
                        0.0,
                        0.65 + row as f32 * 0.05,
                        0.045,
                        color,
                    ));
                }
            }
            // Sparse grass tufts are intentional detail, not a per-cell
            // carpet. Hashing keeps their positions stable while the player
            // moves and preserves a quiet black surround.
            for i in 0..72i32 {
                let x = (scene_hash(i, 91) % 28) as f32 - 14.0;
                let z = (scene_hash(i, 137) % 34) as f32 - 17.0;
                if x.abs() < 4.4 || z.abs() > 29.0 {
                    continue;
                }
                meshes.push(Mesh::decal_y(
                    Vec3::new(x, 0.022, z),
                    if scene_hash(i, 211) & 1 == 0 {
                        0.18
                    } else {
                        -0.18
                    },
                    0.24,
                    0.045,
                    WORLD.grass,
                ));
                if scene_hash(i, 313) & 1 == 0 {
                    meshes.push(Mesh::decal_y(
                        Vec3::new(x + 0.08, 0.024, z + 0.05),
                        if scene_hash(i, 317) & 1 == 0 {
                            0.72
                        } else {
                            -0.72
                        },
                        0.18,
                        0.035,
                        WORLD.foliage_moon,
                    ));
                }
            }

            // Trees (trunk + canopy), off the road corridor.
            for (i, (tx, tz)) in tree_spots().into_iter().enumerate() {
                let h = 2.0 + ((i * 37) % 3) as f32 * 0.6;
                meshes.push(Mesh::boxed(
                    Vec3::new(tx, h / 2.0, tz),
                    Vec3::new(0.35, h, 0.35),
                    WORLD.bark,
                ));
                meshes.push(Mesh::boxed(
                    Vec3::new(tx, 0.16, tz),
                    Vec3::new(1.15, 0.32, 1.15),
                    WORLD.root,
                ));
                meshes.push(Mesh::boxed(
                    Vec3::new(tx + if i % 2 == 0 { 0.34 } else { -0.34 }, h - 0.28, tz),
                    Vec3::new(1.25, 0.13, 0.16),
                    WORLD.bark,
                ));
                meshes.push(Mesh::boxed(
                    Vec3::new(tx, h + 0.18, tz + if i % 2 == 0 { 0.26 } else { -0.26 }),
                    Vec3::new(0.16, 0.12, 1.05),
                    WORLD.bark,
                ));
                meshes.push(Mesh::cone(
                    Vec3::new(tx, h - 0.05, tz),
                    1.45,
                    2.5,
                    8,
                    if i % 3 == 0 {
                        WORLD.foliage_shadow
                    } else {
                        WORLD.foliage
                    },
                ));
                meshes.push(Mesh::cone(
                    Vec3::new(tx, h + 0.95, tz),
                    1.05,
                    1.8,
                    8,
                    WORLD.foliage_moon,
                ));
                meshes.push(Mesh::cone(
                    Vec3::new(tx, h + 1.72, tz + if i % 2 == 0 { -0.10 } else { 0.10 }),
                    0.62,
                    1.05,
                    7,
                    WORLD.foliage_shadow,
                ));
                for root in 0..3 {
                    let angle = (root as f32 + i as f32 * 0.35) * std::f32::consts::TAU / 3.0;
                    meshes.push(Mesh::decal_y(
                        Vec3::new(tx + angle.sin() * 0.34, 0.026, tz + angle.cos() * 0.34),
                        angle,
                        0.34,
                        0.045,
                        WORLD.root,
                    ));
                }
            }

            // Torches are visible glyph geometry and their light is applied to
            // nearby visible faces in the raster pass. There is deliberately
            // no circular pool mesh underneath them.
            for (tx, tz) in [(-7.5, -3.5), (8.5, 4.0), (-4.5, 1.5)] {
                meshes.push(Mesh::boxed(
                    Vec3::new(tx, 0.95, tz),
                    Vec3::new(0.16, 1.9, 0.16),
                    WORLD.bark,
                ));
                meshes.push(Mesh::boxed(
                    Vec3::new(tx, 1.88, tz),
                    Vec3::new(0.34, 0.22, 0.34),
                    WORLD.ember,
                ));
                meshes.push(Mesh::cone(
                    Vec3::new(tx, 1.92, tz),
                    0.28,
                    0.62,
                    7,
                    WORLD.fire,
                ));
            }

            // One ancient stone gate gives the forest a memorable destination
            // silhouette and a cool counterpoint to the warm torches.
            for x in [-10.5, -7.5] {
                meshes.push(Mesh::boxed(
                    Vec3::new(x, 1.55, -9.5),
                    Vec3::new(0.85, 3.1, 0.85),
                    WORLD.stone,
                ));
                meshes.push(Mesh::boxed(
                    Vec3::new(x, 3.12, -9.5),
                    Vec3::new(1.1, 0.26, 1.0),
                    WORLD.stone_light,
                ));
                meshes.push(Mesh::boxed(
                    Vec3::new(x + if x < -9.0 { 0.38 } else { -0.38 }, 2.55, -9.15),
                    Vec3::new(0.08, 0.92, 0.62),
                    Rgb::new(124, 38, 54),
                ));
            }
            meshes.push(Mesh::boxed(
                Vec3::new(-9.0, 3.02, -9.5),
                Vec3::new(3.2, 0.44, 0.86),
                WORLD.stone,
            ));
            for x in [-9.0, -8.55] {
                meshes.push(Mesh::boxed(
                    Vec3::new(x, 0.22, -8.72),
                    Vec3::new(0.16, 0.44, 0.16),
                    WORLD.magic,
                ));
            }
            meshes.push(Mesh::boxed(
                Vec3::new(-9.0, 3.43, -9.5),
                Vec3::new(0.46, 0.20, 0.92),
                WORLD.stone_light,
            ));
            for (x, z, scale) in [(-11.3, -8.9, 0.7), (-7.0, -8.8, 0.55), (-10.9, -10.1, 0.45)] {
                meshes.push(Mesh::boxed(
                    Vec3::new(x, 0.18 * scale, z),
                    Vec3::new(0.72 * scale, 0.36 * scale, 0.55 * scale),
                    WORLD.stone,
                ));
            }

            // Ground fragments: items stay small emissive-yellow boxes.
            for entity in snapshot
                .entities
                .iter()
                .chain(std::iter::once(&snapshot.player))
            {
                if entity.kind != EntityKind::Item {
                    continue;
                }
                let (gx, gy) = self.smooth_pos(
                    entity.id,
                    (entity.position.x as f32, entity.position.y as f32),
                    dt,
                );
                meshes.push(Mesh::boxed(
                    Vec3::new(gx, 0.18, gy),
                    Vec3::new(0.52, 0.52, 0.52),
                    Rgb::new(246, 212, 92),
                ));
                meshes.push(Mesh::decal_y(
                    Vec3::new(gx, 0.025, gy),
                    0.0,
                    0.48,
                    0.18,
                    Rgb::new(112, 88, 36),
                ));
            }

            // Near-field stone clusters provide scale and a clear foreground
            // silhouette. They stay low so the player and nameplates remain
            // the primary read.
            for (x, z, scale) in [
                (-4.5, 3.4, 1.0),
                (4.3, 4.8, 0.8),
                (-5.8, -3.6, 0.72),
                (6.2, -5.0, 0.92),
            ] {
                meshes.push(Mesh::boxed(
                    Vec3::new(x, 0.22 * scale, z),
                    Vec3::new(0.9 * scale, 0.44 * scale, 0.72 * scale),
                    WORLD.stone,
                ));
                meshes.push(Mesh::boxed(
                    Vec3::new(x - 0.18 * scale, 0.48 * scale, z + 0.12 * scale),
                    Vec3::new(0.45 * scale, 0.18 * scale, 0.38 * scale),
                    WORLD.stone_light,
                ));
            }

            // Actors: humanoid meshes from the foot anchor, yawed to face.
            let facing = self.facing;
            let attack_phase = self.attack_phase(t);
            let place = |mesh: Mesh, x: f32, z: f32, yaw: f32, scale: f32| {
                mesh.transformed_about(
                    Vec3::new(0.0, 0.0, 0.0),
                    &Transform {
                        translation: Vec3::new(x, 0.0, z),
                        yaw,
                        scale: Vec3::new(scale, scale, scale),
                        ..Transform::identity()
                    },
                )
            };
            let player_scale = 1.15;
            meshes.push(Mesh::decal_y(
                Vec3::new(player.0, 0.018, player.1),
                facing,
                0.82,
                0.30,
                WORLD.shadow,
            ));
            meshes.push(place(
                Mesh::humanoid_pose(HumanoidPose::new(self.gait_phase, attack_phase, t * 1.7)),
                player.0,
                player.1,
                facing,
                player_scale,
            ));
            for entity in &snapshot.entities {
                if entity.kind == EntityKind::Item {
                    continue;
                }
                let (gx, gy) = self.smooth_pos(
                    entity.id,
                    (entity.position.x as f32, entity.position.y as f32),
                    dt,
                );
                let idle_phase = t * 1.35 + entity.id as f32 * 0.37;
                let label = entity.name.to_ascii_lowercase();
                let actor = match entity.kind {
                    EntityKind::Npc => Mesh::herbalist_pose(idle_phase),
                    EntityKind::Monster if label.contains("skeleton") => {
                        Mesh::skeleton_pose(HumanoidPose::new(idle_phase, attack_phase, idle_phase))
                    }
                    EntityKind::Monster => {
                        Mesh::mossling_pose(HumanoidPose::new(idle_phase, attack_phase, idle_phase))
                    }
                    EntityKind::Player => {
                        Mesh::humanoid_pose(HumanoidPose::new(idle_phase, attack_phase, idle_phase))
                    }
                    EntityKind::Item => continue,
                };
                let scale = match entity.kind {
                    EntityKind::Npc => 1.05,
                    EntityKind::Monster if label.contains("skeleton") => 1.08,
                    EntityKind::Monster => 1.12,
                    EntityKind::Player => 1.15,
                    EntityKind::Item => 1.0,
                };
                meshes.push(Mesh::decal_y(
                    Vec3::new(gx, 0.018, gy),
                    facing,
                    if entity.kind == EntityKind::Monster {
                        0.78
                    } else {
                        0.68
                    },
                    0.26,
                    WORLD.shadow,
                ));
                meshes.push(place(actor, gx, gy, facing, scale));
            }

            // Target marker is a world-space object, not only a label. This
            // keeps combat readability intact when the target is moving or
            // partially overlaps the player silhouette.
            if let Some(target) = snapshot
                .entities
                .iter()
                .filter(|entity| {
                    entity.kind == EntityKind::Monster
                        && entity.stats.is_alive()
                        && (entity.position.x - snapshot.player.position.x).abs()
                            + (entity.position.y - snapshot.player.position.y).abs()
                            <= 6
                })
                .min_by_key(|entity| {
                    (entity.position.x - snapshot.player.position.x).abs()
                        + (entity.position.y - snapshot.player.position.y).abs()
                })
            {
                let (tx, tz) = self.smooth_pos(
                    target.id,
                    (target.position.x as f32, target.position.y as f32),
                    dt,
                );
                for i in 0..12 {
                    let angle = i as f32 * std::f32::consts::TAU / 12.0;
                    let point = Vec3::new(tx + angle.sin() * 2.15, 0.075, tz + angle.cos() * 1.35);
                    meshes.push(Mesh::decal_y(
                        point,
                        angle + std::f32::consts::FRAC_PI_2,
                        0.22,
                        0.09,
                        TARGET_ORANGE,
                    ));
                }
            }

            // Combat FX: telegraph decal ring → core/ring burst → embers,
            // the mesh re-expression of the TUI's FX packs.
            for fx in &self.combat_fx {
                let age = t - fx.born_s;
                let origin = Vec3::new(fx.origin.0, 0.0, fx.origin.1);
                let radius = fx.radius.max(0.5);
                if age < fx.telegraph_s {
                    let p = if fx.telegraph_s > 0.0 {
                        age / fx.telegraph_s
                    } else {
                        1.0
                    };
                    let r = radius * (0.35 + 0.65 * (1.0 - p));
                    for i in 0..12 {
                        let a = i as f32 * std::f32::consts::TAU / 12.0
                            + 0.4 * (fx.born_s.fract() * 6.0);
                        let dash = Vec3::new(origin.x + r * a.sin(), 0.04, origin.z + r * a.cos());
                        meshes.push(Mesh::decal_y(
                            dash,
                            a + std::f32::consts::FRAC_PI_2,
                            0.22,
                            0.1,
                            fx.color,
                        ));
                    }
                } else {
                    let burst_age = (age - fx.telegraph_s).max(0.0);
                    let p = (burst_age / bridge::FX_BURST_S).clamp(0.0, 1.0);
                    // White-hot core: mixes toward white and shrinks out.
                    let hot = mix(fx.color, Rgb::new(255, 255, 245), 0.65);
                    let core = Mesh::boxed(
                        Vec3::new(origin.x, 0.5 * (1.0 - p), origin.z),
                        Vec3::new(
                            0.7 * (1.0 - p) + 0.2,
                            1.0 * (1.0 - p) + 0.2,
                            0.7 * (1.0 - p) + 0.2,
                        ),
                        hot,
                    );
                    meshes.push(core);
                    // Expanding ground ring.
                    let r = radius * (0.25 + 0.75 * p);
                    for i in 0..10 {
                        let a = i as f32 * std::f32::consts::TAU / 10.0;
                        let dash = Vec3::new(origin.x + r * a.sin(), 0.05, origin.z + r * a.cos());
                        meshes.push(Mesh::decal_y(
                            dash,
                            a + std::f32::consts::FRAC_PI_2,
                            0.3,
                            0.12,
                            fx.color,
                        ));
                    }
                    // Six rising embers (TUI parity #2).
                    for i in 0..6 {
                        let a = i as f32 * std::f32::consts::TAU / 6.0;
                        let e_r = 0.3 + p * 1.4;
                        let ember = Vec3::new(
                            origin.x + a.sin() * e_r,
                            0.4 + p * p * 2.2,
                            origin.z + a.cos() * e_r,
                        );
                        meshes.push(Mesh::boxed(
                            ember,
                            Vec3::new(0.16, 0.16, 0.16),
                            mix(fx.color, Rgb::new(255, 190, 80), 0.4),
                        ));
                    }
                }
            }

            meshes
        }
    }

    fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
        let ch = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t) as u8;
        Rgb::new(ch(a.r, b.r), ch(a.g, b.g), ch(a.b, b.b))
    }

    impl SceneSource for LocalGame {
        fn scene_at(&mut self, t: f32) -> (Scene, Camera) {
            self.scene_at_size(t, GRID_W, GRID_H)
        }

        fn gpu_world(&mut self) -> Option<GpuWorld> {
            // Once per frame, paired with scene_at_size above. `--check`
            // and the engine tests run scene builders without the
            // presenter, so leftovers here are presentation-state only.
            self.pending_world.take()
        }

        fn scene_at_size(&mut self, t: f32, width: u32, height: u32) -> (Scene, Camera) {
            let (w, h) = (width as i32, height as i32);
            let world_h = (h - HUD_H - LOG_H).max(4);
            let (dt, snapshot) = self.advance(t);
            // Resolve the player's presentation anchor before building the
            // camera.  The world mesh and camera therefore follow the same
            // interpolated point instead of alternating one frame apart.
            self.smooth_pos(
                snapshot.player.id,
                (
                    snapshot.player.position.x as f32,
                    snapshot.player.position.y as f32,
                ),
                dt,
            );
            let camera = self.camera3(w, world_h, dt, &snapshot);
            let mut scene = Scene::default();

            // ---- HUD band ----
            let mut hud = Layer::new("hud", 0);
            hud.fill_rect(0, 0, w, HUD_H, Cell::new(' ', HUD_TEXT, PANEL_BG));
            hud.border_rect(0, 0, w, HUD_H, PANEL_EDGE, PANEL_BG);
            hud.push_text(2, 5, "MOONLIT WILDERNESS  |  SAFE PATH", HUD_DIM, PANEL_BG);
            hud.push_text(
                2,
                3,
                "WASD Move   F Attack   1-3 Cast   I Inventory   R Rest   RMB Orbit   Wheel Zoom   Q Quit",
                HUD_DIM,
                PANEL_BG,
            );
            hud.push_text(2, 4, "HP", HP_FILL, PANEL_BG);
            // Bar spans x 5..w-8; integer math keeps the fill split crisp.
            let bar_w = (w - 13).max(0);
            let hp = snapshot.player.stats.hp.max(0);
            let max_hp = snapshot.player.stats.max_hp.max(1);
            let hp_lit = (bar_w * hp / max_hp).clamp(0, bar_w);
            hud.push_text(5, 4, &"█".repeat(hp_lit as usize), HP_FILL, PANEL_BG);
            hud.push_text(
                5 + hp_lit,
                4,
                &"░".repeat((bar_w - hp_lit) as usize),
                HP_EMPTY,
                PANEL_BG,
            );
            let hud_idx = scene.layers.len();
            scene.layers.push(hud);
            scene.overlay_text.push(TextRun {
                x: 15.0,
                y: 0.35,
                text: "WHISPERING FOREST".into(),
                size: 1.35,
                fg: [0.92, 0.84, 0.58],
                bg: [0.02, 0.03, 0.06],
            });
            scene.overlay_text.push(TextRun {
                x: 15.0,
                y: 1.8,
                text: format!(
                    "{}   HP {}/{}   TICK {}",
                    scene_zone(&snapshot),
                    snapshot.player.stats.hp,
                    snapshot.player.stats.max_hp,
                    snapshot.tick
                ),
                size: 1.0,
                fg: [0.82, 0.90, 0.98],
                bg: [0.02, 0.03, 0.06],
            });

            // ---- World partition: one raster pass + screen labels ----
            //
            // GPU mode (default): the triangle fill runs on the GPU
            // (`present::gpu3d`) — stash meshes for the presenter and push a
            // placeholder layer whose slot the presenter swaps with the GPU-
            // resolved raster before compose. `--cpu` keeps the CPU fill
            // (`raster::render_layer`), the exact pre-parity path.
            let meshes = self.world_meshes(&snapshot, t, dt);
            let opts = DepthOpts {
                near: 0.5,
                far: 64.0,
                ambient: 0.46,
                edge_step: 0.48,
                moon_tint: Rgb::new(178, 198, 238),
                moonlight_strength: 0.22,
            };
            let mut lights = vec![
                PointLight::new(Vec3::new(-7.5, 1.8, -3.5), WORLD.fire, 5.0, 1.15),
                PointLight::new(Vec3::new(8.5, 1.8, 4.0), WORLD.fire, 5.0, 1.15),
                PointLight::new(Vec3::new(-4.5, 1.8, 1.5), WORLD.fire, 4.5, 1.0),
            ];
            for fx in &self.combat_fx {
                let age = t - fx.born_s;
                if age >= 0.0 && age < fx.telegraph_s + bridge::FX_BURST_S {
                    lights.push(PointLight::new(
                        Vec3::new(fx.origin.0, 0.9, fx.origin.1),
                        fx.color,
                        fx.radius.max(1.5) * 1.7,
                        0.75,
                    ));
                }
            }
            // Sun tilts out of the south-west so actor verticals stay lit
            // while selective ground glyphs keep their dark meadow tone.
            let sun = Vec3::new(0.55, 0.85, 0.3);
            let terrain_idx = scene.layers.len();
            if self.gpu3d {
                self.pending_world = Some(GpuWorld {
                    meshes,
                    camera,
                    sun,
                    opts,
                    z: 0,
                    layer_slot: terrain_idx,
                });
                scene.layers.push(Layer::new("mesh3d", 0)); // placeholder
            } else {
                let terrain =
                    raster::render_layer_with_lights(&meshes, &camera, sun, &opts, &lights, 0);
                scene.layers.push(terrain);
            }

            // Nameplates + mini-bars + damage digits (screen coords in the
            // raster view space; the same projection math the fill used).
            let mut labels = Layer::new("labels", 3);
            for entity in snapshot
                .entities
                .iter()
                .chain(std::iter::once(&snapshot.player))
            {
                if entity.kind == EntityKind::Item {
                    continue;
                }
                let (gx, gy) = *self
                    .smoothed
                    .get(&entity.id)
                    .unwrap_or(&(entity.position.x as f32, entity.position.y as f32));
                let entity_label = entity.name.to_ascii_lowercase();
                let head_y = match entity.kind {
                    EntityKind::Monster if entity_label.contains("skeleton") => 2.62,
                    EntityKind::Monster => 2.28,
                    EntityKind::Npc => 3.05,
                    EntityKind::Player => 4.05,
                    EntityKind::Item => 0.8,
                };
                if let Some((sx, sy)) = project(Vec3::new(gx, head_y, gy), &camera) {
                    let fg = match entity.kind {
                        EntityKind::Player => Rgb::new(255, 235, 160),
                        EntityKind::Npc => Rgb::new(150, 220, 255),
                        _ => Rgb::new(255, 230, 220),
                    };
                    let name = format!(" {} ", entity.name);
                    let name_x = sx - name.chars().count() as i32 / 2;
                    labels.fill_rect(
                        name_x - 1,
                        sy - 1,
                        name.chars().count() as i32 + 2,
                        3,
                        Cell::new(' ', fg, LABEL_BG),
                    );
                    labels.push_text(name_x, sy, &name, fg, LABEL_BG);
                    // HP mini-bar: 8 gutters, filled proportionally.
                    let hp_ratio =
                        entity.stats.hp.max(0) as f32 / entity.stats.max_hp.max(1) as f32;
                    let lit = (8.0 * hp_ratio).round().clamp(0.0, 8.0) as usize;
                    let mini = format!("{}{}", "█".repeat(lit), "░".repeat(8 - lit));
                    let row = format!("{} {} ", mini, entity.stats.hp.max(0));
                    labels.push_text(
                        sx - row.chars().count() as i32 / 2,
                        sy + 1,
                        &row,
                        HP_FILL,
                        LABEL_BG,
                    );
                }
            }
            // Damage digits from the observed FX (TUI parity #2/#3): lift out
            // of the burst core over the burst window.
            for fx in &self.combat_fx {
                if fx.digits.is_empty() {
                    continue;
                }
                let burst_age = (t - fx.born_s - fx.telegraph_s).max(0.0);
                if burst_age > bridge::FX_BURST_S + 0.15 {
                    continue;
                }
                let lift = 2.4 + 2.0 * (burst_age / bridge::FX_BURST_S).clamp(0.0, 1.0);
                if let Some((sx, sy)) = project(Vec3::new(fx.origin.0, lift, fx.origin.1), &camera)
                {
                    labels.push_text_bold(
                        sx - fx.digits.chars().count() as i32 / 2,
                        sy,
                        &fx.digits,
                        DIGIT_GOLD,
                        LABEL_BG,
                    );
                }
            }
            let labels_idx = scene.layers.len();
            scene.layers.push(labels);

            // ---- Log strip ----
            let mut log = Layer::new("log", 0);
            log.fill_rect(0, 0, w, LOG_H, Cell::new(' ', HUD_TEXT, PANEL_BG));
            log.border_rect(0, 0, w, LOG_H, Rgb::new(90, 108, 132), PANEL_BG);
            for (i, message) in snapshot
                .messages
                .iter()
                .rev()
                .take((LOG_H - 1) as usize)
                .enumerate()
            {
                let shown: String = message.chars().take((w - 4) as usize).collect();
                log.push_text(2, 1 + i as i32, &shown, HUD_DIM, PANEL_BG);
            }
            let log_idx = scene.layers.len();
            scene.layers.push(log);

            scene
                .partitions
                .push(Partition::new("hud", 0, 0, w, HUD_H, vec![hud_idx], false));
            scene.partitions.push(Partition::new(
                "world",
                0,
                HUD_H,
                w,
                world_h,
                vec![terrain_idx, labels_idx],
                true,
            ));
            scene.partitions.push(Partition::new(
                "log",
                0,
                HUD_H + world_h,
                w,
                LOG_H,
                vec![log_idx],
                false,
            ));

            // ---- Inventory modal (TUI fixture, OSRS-panel tone) ----
            if self.inv_open {
                let mw = 74.min(w - 4);
                let mh = 16;
                let mx = (w - mw) / 2;
                let my = ((h - mh) / 2).max(2);
                let mut modal = Layer::new("modal", 0);
                modal.fill_rect(0, 0, mw, mh, Cell::new(' ', HUD_TEXT, PANEL_BG));
                modal.border_rect(0, 0, mw, mh, PANEL_EDGE, PANEL_BG);
                modal.push_text_bold(
                    2,
                    0,
                    " Inventory & Equipment [I to Close] ",
                    DIGIT_GOLD,
                    PANEL_BG,
                );
                modal.push_text(4, 2, "Equipped Gear:", Rgb::new(220, 190, 120), PANEL_BG);
                modal.push_text(
                    4,
                    4,
                    " Main Hand:  /  Rusted Sword (+3 ATK)",
                    HUD_TEXT,
                    PANEL_BG,
                );
                modal.push_text(
                    4,
                    5,
                    " Off-Hand:   O  Forest Shield (+2 DEF)",
                    HUD_TEXT,
                    PANEL_BG,
                );
                modal.push_text(
                    4,
                    6,
                    " Amulet:     *  Moss Charm (Sp+1)",
                    HUD_TEXT,
                    PANEL_BG,
                );
                modal.push_text(mw / 2 + 4, 2, "Pouch:", Rgb::new(220, 190, 120), PANEL_BG);
                modal.push_text(mw / 2 + 4, 4, " /  Rusted Sword", HUD_TEXT, PANEL_BG);
                modal.push_text(mw / 2 + 4, 5, " O  Forest Shield", HUD_TEXT, PANEL_BG);
                modal.push_text(mw / 2 + 4, 6, " *  Moss Charm", HUD_TEXT, PANEL_BG);
                modal.push_text(
                    4,
                    mh - 3,
                    "Fixture inventory (matches the terminal's TUI modal); data-driven gear is a later slice.",
                    HUD_DIM,
                    PANEL_BG,
                );
                let modal_idx = scene.layers.len();
                scene.layers.push(modal);
                scene.partitions.push(Partition::new(
                    "modal",
                    mx,
                    my,
                    mw,
                    mh,
                    vec![modal_idx],
                    false,
                ));
            }

            // The 3D camera is already baked into the raster pass, so the
            // world partition uses the camera flag only to mark its cells as
            // foreground-only world ink. HUD/log/modal partitions stay pinned
            // and retain their intentional interface styling.
            (scene, Camera::default())
        }

        fn key(&mut self, code: KeyCode, pressed: bool) {
            if pressed {
                match code {
                    KeyCode::Digit1 | KeyCode::Numpad1 => self.pending.push(Command::Cast {
                        spell: "cleave".into(),
                        target: 2,
                    }),
                    KeyCode::Digit2 | KeyCode::Numpad2 => self.pending.push(Command::Cast {
                        spell: "iron-will".into(),
                        target: 2,
                    }),
                    KeyCode::Digit3 | KeyCode::Numpad3 => self.pending.push(Command::Cast {
                        spell: "thunder-mark".into(),
                        target: 2,
                    }),
                    KeyCode::KeyI => self.inv_open = !self.inv_open,
                    _ => {}
                }
            }
            match code {
                KeyCode::KeyF if pressed => {
                    self.pending.push(Command::Attack);
                    self.attack_queued = true;
                }
                KeyCode::KeyR if pressed => self.pending.push(Command::Rest),
                KeyCode::KeyW
                | KeyCode::KeyA
                | KeyCode::KeyS
                | KeyCode::KeyD
                | KeyCode::ArrowUp
                | KeyCode::ArrowDown
                | KeyCode::ArrowLeft
                | KeyCode::ArrowRight => {
                    if pressed {
                        self.held.insert(code);
                    } else {
                        self.held.remove(&code);
                    }
                }
                _ => {}
            }
        }

        fn pointer(&mut self, event: PointerEvent) {
            // Right-drag orbits; drag-y tilts between a low sweep and a high
            // top-down. Wheel dollies the orbit distance (OSRS-zoom feel).
            if event.held[1] {
                self.cam_yaw += event.delta.0 * 0.006;
                self.cam_pitch =
                    (self.cam_pitch + event.delta.1 * 0.003).clamp(CAM_PITCH_MIN, CAM_PITCH_MAX);
            }
            if event.wheel != 0.0 {
                let factor = if event.wheel > 0.0 { 0.92 } else { 1.08 };
                self.cam_dist = (self.cam_dist * factor).clamp(CAM_DIST_MIN, CAM_DIST_MAX);
            }
        }
    }

    fn run_check() -> Result<(), String> {
        // Combat path: fresh game, mossling adjacent at (3, 2). One scene
        // build must (a) damage the mob through the sim, (b) observe the
        // message into FX state, (c) ink a 3D raster world, and (d) raise a
        // damage digit into the screen-space label overlay.
        // CPU raster: `--check` runs headless (no presenter, no GPU swap),
        // so it validates the CPU fill path and the label math directly.
        let mut game = LocalGame::with_gpu3d(false);
        game.key(KeyCode::KeyF, true);
        let (scene, _) = game.scene_at(0.06);
        let snap = game.sim.snapshot_for(PLAYER_ID);
        let mossling = snap
            .entities
            .iter()
            .find(|entity| entity.name == "Mossling")
            .ok_or("check: mossling missing from snapshot")?;
        if mossling.stats.hp >= 8 {
            return Err("check: F attack did not damage the mossling".into());
        }
        if game.combat_fx.is_empty() {
            return Err("check: hit message spawned no combat FX".into());
        }
        let terrain = scene
            .layer("mesh3d")
            .and_then(|i| scene.layers.get(i))
            .ok_or("check: 3D terrain layer missing")?;
        if terrain.cells.is_empty() {
            return Err("check: raster world inked no cells".into());
        }
        let labels = scene
            .layer("labels")
            .and_then(|i| scene.layers.get(i))
            .ok_or("check: labels layer missing")?;
        if !labels.cells.iter().any(|c| c.cell.glyph.is_ascii_digit()) {
            return Err("check: damage digits missing from the label overlay".into());
        }
        if scene.partitions.len() != 3 {
            return Err(format!(
                "check: expected 3 partitions with modal closed, got {}",
                scene.partitions.len()
            ));
        }
        if scene.overlay_text.len() < 2 {
            return Err("check: readable title/status overlays missing".into());
        }

        // Movement path: default camera looks north, so screen-right (D)
        // walks the world-west step that the raster projects back screen-
        // right — the input is basis-relative end to end. (CPU raster, as
        // above: headless check.)
        let mut game = LocalGame::with_gpu3d(false);
        game.key(KeyCode::KeyD, true);
        game.scene_at(0.0);
        for frame in 1..=40 {
            game.scene_at(frame as f32 * 0.05);
        }
        let x = game.sim.snapshot_for(PLAYER_ID).player.position.x;
        if x >= 2 {
            return Err(format!("check: held D did not move the player (x={x})"));
        }

        // Inventory modal toggles a fourth partition on and back off.
        game.key(KeyCode::KeyI, true);
        let (scene, _) = game.scene_at(2.05);
        if scene.partitions.len() != 4 {
            return Err(format!(
                "check: inventory modal missing ({} partitions)",
                scene.partitions.len()
            ));
        }
        let modal = scene
            .layer("modal")
            .and_then(|i| scene.layers.get(i))
            .ok_or("check: modal layer missing")?;
        if modal.cells.is_empty() {
            return Err("check: modal inked no cells".into());
        }
        // Press again closes (toggle on key-down, like the terminal client).
        game.key(KeyCode::KeyI, true);
        let (scene, _) = game.scene_at(2.1);
        if scene.partitions.len() != 3 {
            return Err("check: inventory modal did not close".into());
        }
        Ok(())
    }

    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|arg| arg == "--check") {
        run_check()?;
        eprintln!("play check ok: 3D world, FX digits, orbit movement, inventory modal");
        return Ok(());
    }
    let frames = args
        .iter()
        .position(|arg| arg == "--frames")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse::<u64>().ok());

    match ascii_mmorpg_engine::present::run_dynamic(
        LocalGame::new(),
        PresentOptions {
            // GPU raster is the default; `--cpu` forces the CPU fill.
            title: "ASCII MMORPG — 3D orbit camera (OSRS controls)".into(),
            cell_px: 10.0,
            width_cells: GRID_W,
            height_cells: GRID_H,
            max_frames: frames,
            ..Default::default()
        },
    ) {
        Ok(presented) => {
            eprintln!("presented {presented} frames");
            std::process::exit(0);
        }
        Err(err) => Err(err),
    }
}
