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
    use ascii_mmorpg_engine::motion::KinematicMotion;
    use ascii_mmorpg_engine::present::{GpuWorld, PointerEvent, PresentOptions, SceneSource};
    use ascii_mmorpg_engine::raster::{self, Camera3, DepthOpts, Mesh, Transform, Vec3};
    use ascii_mmorpg_engine::{bridge, Camera, Cell, Layer, Partition, Rgb, Scene};
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
    const GRID_W: u32 = 192;
    const GRID_H: u32 = 70;
    const HUD_H: i32 = 6;
    const LOG_H: i32 = 4;

    /// Camera defaults: OSRS-mid pull-back looking down at the player.
    const CAM_DIST: f32 = 17.0;
    const CAM_PITCH: f32 = -0.62; // ~35° OSRS mid; tilt via right-drag
    const CAM_PITCH_MIN: f32 = -1.05;
    const CAM_PITCH_MAX: f32 = -0.3;
    const CAM_DIST_MIN: f32 = 9.0;
    const CAM_DIST_MAX: f32 = 30.0;
    /// Focal (cell units): at the 17-unit pull-back one world unit spans
    /// ~2 cells, so the 192-column view covers ~96 world cells — the same
    /// wing as the bridge camera.
    const CAM_FOCAL: f32 = 34.0;

    // Panel palette (dark slate HUD, gold accents — the TUI modal's tones).
    const PANEL_BG: Rgb = Rgb::new(12, 11, 16);
    const PANEL_EDGE: Rgb = Rgb::new(215, 175, 95);
    const HUD_TEXT: Rgb = Rgb::new(216, 208, 190);
    const HUD_DIM: Rgb = Rgb::new(110, 120, 135);
    const HP_FILL: Rgb = Rgb::new(220, 60, 50);
    const HP_EMPTY: Rgb = Rgb::new(90, 30, 26);
    const LABEL_BG: Rgb = Rgb::new(10, 10, 14);
    const DIGIT_GOLD: Rgb = Rgb::new(255, 230, 160);

    // Meadow palette: dim saturated greens so actors/light FX pop (quiet-
    // carpet contract from the video study, factored for the 3D zones).
    const MEADOW: [Rgb; 4] = [
        Rgb::new(18, 46, 28),
        Rgb::new(14, 40, 24),
        Rgb::new(22, 54, 30),
        Rgb::new(12, 34, 22),
    ];

    /// Deterministic scenery list (hand-placed so the highway corridor and
    /// the demo spawn area stay walkable; `quad_y`/`boxed` scenes rebuild
    /// these every frame — no mesh cache needed at this scale).
    fn tree_spots() -> [(f32, f32); 9] {
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
        ]
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
                facing: std::f32::consts::FRAC_PI_2,
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
            if let Some((dx, dy)) = self.motion.update(self.camera_axes(), dt) {
                self.facing = f32::atan2(f32::from(dx), f32::from(dy));
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
            Camera3::new(eye, self.cam_yaw, self.cam_pitch, CAM_FOCAL, w, world_h)
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
                if let Some(event) = combat_fx_from_message(message, snapshot) {
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

            // Ground: 14-unit zone blocks in a window around the player.
            // Much cheaper than per-cell work and reads the same at this
            // pull-back distance.
            const BLOCK: f32 = 14.0;
            const RADIUS: i32 = 4; // blocks either side of the player
            let px = (player.0 / BLOCK).round();
            let pz = (player.1 / BLOCK).round();
            for bx in (px - RADIUS as f32) as i32..=(px + RADIUS as f32) as i32 {
                for bz in (pz - RADIUS as f32) as i32..=(pz + RADIUS as f32) as i32 {
                    // Tiny integer hash for zone variety (unsigned fold).
                    let mix = (bx + 64) * 7 + (bz + 64) * 13 + ((bx * bz) & 3);
                    let albedo = MEADOW[mix.unsigned_abs() as usize % MEADOW.len()];
                    meshes.push(Mesh::quad_y(
                        Vec3::new(bx as f32 * BLOCK, 0.0, bz as f32 * BLOCK),
                        BLOCK / 2.0,
                        BLOCK / 2.0,
                        albedo,
                    ));
                }
            }
            // Southern ocean + dry-then-wet sand beach south of the world
            // bound (35): same quiet sea language as the TUI, flat sheets.
            meshes.push(Mesh::quad_y(
                Vec3::new(0.0, -0.12, 47.0),
                160.0,
                8.0,
                Rgb::new(62, 54, 40), // sand 39..55
            ));
            meshes.push(Mesh::quad_y(
                Vec3::new(0.0, -0.2, 87.0),
                160.0,
                32.0,
                Rgb::new(10, 30, 48), // open water 55..119
            ));

            // Trees (trunk + canopy), off the road corridor.
            for (i, (tx, tz)) in tree_spots().into_iter().enumerate() {
                let h = 2.0 + ((i * 37) % 3) as f32 * 0.6;
                meshes.push(Mesh::boxed(
                    Vec3::new(tx, h / 2.0, tz),
                    Vec3::new(0.35, h, 0.35),
                    Rgb::new(74, 58, 40),
                ));
                meshes.push(Mesh::boxed(
                    Vec3::new(tx, h + 1.3, tz),
                    Vec3::new(1.7, 2.6, 1.7),
                    Rgb::new(26, 66, 40),
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
                    Vec3::new(0.3, 0.36, 0.3),
                    Rgb::new(230, 200, 90),
                ));
            }

            // Actors: humanoid meshes from the foot anchor, yawed to face.
            let facing = self.facing;
            let stick = |x: f32, z: f32, yaw: f32, scale: f32| {
                Mesh::humanoid().transformed_about(
                    Vec3::new(0.0, 0.0, 0.0),
                    &Transform {
                        translation: Vec3::new(x, 0.0, z),
                        yaw,
                        scale: Vec3::new(scale, scale, scale),
                        ..Transform::identity()
                    },
                )
            };
            let player_scale = 0.62;
            meshes.push(stick(player.0, player.1, facing, player_scale));
            for entity in &snapshot.entities {
                if entity.kind == EntityKind::Item {
                    continue;
                }
                let (gx, gy) = self.smooth_pos(
                    entity.id,
                    (entity.position.x as f32, entity.position.y as f32),
                    dt,
                );
                // Moss-tinted mob palette; the merchant wears rust-red cloth.
                let remaps: &[(&str, Rgb)] = match entity.kind {
                    EntityKind::Npc => &[("cloth", Rgb::new(122, 52, 44))],
                    _ => &[
                        ("leather", Rgb::new(46, 92, 44)),
                        ("cloth", Rgb::new(30, 64, 38)),
                        ("steel", Rgb::new(96, 128, 96)),
                    ],
                };
                let mut mob = stick(gx, gy, facing, 0.55);
                for (part, color) in remaps {
                    let (from, to) = part_tint(part, *color);
                    mob = mob.retinted(from, to);
                }
                meshes.push(mob);
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

    fn part_tint(part: &str, color: Rgb) -> (Rgb, Rgb) {
        match part {
            "leather" => (Rgb::new(122, 86, 52), color),
            "cloth" => (Rgb::new(52, 58, 92), color),
            _ => (Rgb::new(150, 170, 190), color),
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
            let camera = self.camera3(w, world_h, dt, &snapshot);
            let mut scene = Scene::default();

            // ---- HUD band ----
            let mut hud = Layer::new("hud", 0);
            hud.fill_rect(0, 0, w, HUD_H, Cell::new(' ', HUD_TEXT, PANEL_BG));
            let zone_line = format!(
                "{}  HP {}/{}  Tick {}",
                scene_zone(&snapshot),
                snapshot.player.stats.hp,
                snapshot.player.stats.max_hp,
                snapshot.tick
            );
            hud.push_text(2, 1, &zone_line, HUD_TEXT, PANEL_BG);
            hud.push_text(
                2,
                2,
                "[WASD] move  [F] attack  [1-3] cast  [I] inventory  [R] rest  [Right-drag] orbit  [Wheel] zoom  [Q] quit",
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
                far: 44.0,
                ambient: 0.3,
                edge_step: 0.28,
            };
            // Sun tilts out of the south-west so actor verticals stay lit
            // while ground zones keep their dark meadow tone.
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
                let terrain = raster::render_layer(&meshes, &camera, sun, &opts, 0);
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
                if let Some((sx, sy)) = project(Vec3::new(gx, 3.2, gy), &camera) {
                    let fg = if entity.kind == EntityKind::Player {
                        HUD_TEXT
                    } else {
                        Rgb::new(220, 220, 220)
                    };
                    let name = format!(" {} ", entity.name);
                    labels.push_text(
                        sx - name.chars().count() as i32 / 2,
                        sy,
                        &name,
                        fg,
                        LABEL_BG,
                    );
                    // HP mini-bar: 8 gutters, filled proportionally.
                    let hp_ratio =
                        entity.stats.hp.max(0) as f32 / entity.stats.max_hp.max(1) as f32;
                    let lit = (8.0 * hp_ratio).round().clamp(0.0, 8.0) as usize;
                    let mini = format!("{}{}", "▮".repeat(lit), "▯".repeat(8 - lit));
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
                false,
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

            // No camera-follow: every partition is pinned; the 3D camera
            // pans inside the single raster pass instead.
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
                KeyCode::KeyF if pressed => self.pending.push(Command::Attack),
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
            cell_px: 8.0,
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
