//! Playable windowed port with real depth: the local server
//! [`Simulation`](ascii_mmorpg_server) ticks at 20 Hz while the world is
//! rasterized through the 3D depth backend (`raster::`) behind an OSRS-style
//! fixed elevated oblique 2.5D camera. WASD/arrows walk in fixed world
//! directions, left-click ground targeting, `F` attacks, `1-3` cast, `I` opens
//! the inventory, `R` rests, `F11` toggles fullscreen, and `Q`/Esc quits.
//! Pass `--grid` to compare the legacy grid presenter with the default
//! continuous, aspect-preserving canvas.
//!
//! Play-path integration (issue #10): the rebuilt continuous renderer is
//! the DEFAULT — native torch flame/crackles with float bloom owning
//! world lighting, layered `engine::character` actors, and
//! `engine::weather` grass/rain/motes all run together on the continuous
//! canvas. `M` opens the Veyrath world map (`engine::veyrath_world`,
//! provisional presentation-only coordinates). The legacy mesh paths
//! remain selectable for acceptance comparison: `--legacy` restores all
//! of them (or `--legacy-torch` / `--legacy-actors` / `--legacy-weather`
//! per system), and `--grid` selects the legacy surface-filling grid
//! presenter. The old world rendering is retired only after explicit
//! visual acceptance — the mesh paths stay compiled until then.
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
//! `--window` opens the authored 160x56 comparison window instead of the
//! default borderless fullscreen (whose grid derives from the surface at
//! native glyph density); `--check` runs a headless movement + combat +
//! modal self-test (no window).
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
    use ascii_mmorpg_engine::present::{
        GpuWorld, PointerEvent, PresentMode, PresentOptions, SceneSource,
    };
    use ascii_mmorpg_engine::raster::{
        self, Camera3, DepthOpts, HumanoidPose, Mesh, PointLight, Transform, Vec3,
    };
    use ascii_mmorpg_engine::{
        bridge, cell_text_width, compose, AnimCell, Camera, Cell, ContinuousMaterial,
        ContinuousScene, Emission, EmissionSource, Layer, Partition, Point2, Primitive,
        PrimitiveKind, Rgb, Scene, Transform2, Wash, MAX_NATIVE_PRIMITIVES,
    };
    use std::collections::{HashMap, HashSet, VecDeque};
    use std::time::Duration;
    use winit::keyboard::KeyCode;

    use ascii_mmorpg_server::{FixedTickClock, Simulation};
    use ascii_mmorpg_shared::{
        is_point_obstructed, overworld_obstacles, ClientCommand, Command, Direction, EntityId,
        EntityKind, Point, Tick, WorldSnapshot,
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

    /// Fixed camera: OSRS-mid pull-back looking down at the player.
    const CAM_YAW: f32 = std::f32::consts::PI; // fixed north-facing view
    const CAM_DIST: f32 = 13.5;
    const CAM_PITCH: f32 = -0.68; // close readable OSRS gameplay framing
    /// Focal (cell units): at the fixed pull-back one world unit spans about
    /// two cells, so the 160-column view keeps the bridge-scale world read.
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

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Weather {
        Clear,
        Rain,
        Storm,
    }

    #[derive(Clone, Copy)]
    struct WeatherProfile {
        grass_amplitude: f32,
        grass_speed: f32,
        rain_density: u32,
        rain_speed: f32,
        wind_x: f32,
        wind_z: f32,
        rain_color: Rgb,
    }

    fn weather_profile(weather: Weather, time: f32) -> WeatherProfile {
        let gust = (time * 0.82).sin() * 0.5 + (time * 1.37 + 1.4).sin() * 0.3;
        match weather {
            Weather::Clear => WeatherProfile {
                grass_amplitude: 0.015,
                grass_speed: 1.2,
                rain_density: 0,
                rain_speed: 0.0,
                wind_x: 0.0,
                wind_z: 0.0,
                rain_color: Rgb::new(0, 0, 0),
            },
            Weather::Rain => WeatherProfile {
                grass_amplitude: 0.12 + gust.abs() * 0.05,
                grass_speed: 2.2 + gust.abs() * 0.4,
                rain_density: 24,
                rain_speed: 1.45 + gust.abs() * 0.18,
                wind_x: 0.20 + gust * 0.08,
                wind_z: -0.08 + gust * 0.04,
                rain_color: Rgb::new(108, 142, 186),
            },
            Weather::Storm => WeatherProfile {
                grass_amplitude: 0.22 + gust.abs() * 0.09,
                grass_speed: 3.8 + gust.abs() * 0.7,
                rain_density: 42,
                rain_speed: 2.15 + gust.abs() * 0.28,
                wind_x: 0.34 + gust * 0.12,
                wind_z: -0.14 + gust * 0.06,
                rain_color: Rgb::new(132, 166, 210),
            },
        }
    }

    fn weather_from_args() -> Weather {
        let mut args = std::env::args();
        while let Some(arg) = args.next() {
            if arg != "--weather" {
                continue;
            }
            return match args.next().as_deref() {
                Some("clear") => Weather::Clear,
                Some("storm") => Weather::Storm,
                _ => Weather::Rain,
            };
        }
        Weather::Rain
    }

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

    const TORCH_CRACKLE_COUNT: usize = 6;
    const TORCH_CRACKLE_PERIOD: f32 = 0.92;
    const TORCH_CRACKLE_LIFETIME: f32 = 0.34;

    #[derive(Clone, Copy)]
    struct TorchPulse {
        light_gain: f32,
        radius_scale: f32,
        flame_scale: f32,
        offset: (f32, f32),
    }

    fn hash01(seed: u32) -> f32 {
        scene_hash(seed as i32, 991) as f32 / u32::MAX as f32
    }

    fn torch_spots() -> [(f32, f32); 3] {
        [(-7.5, -3.5), (8.5, 4.0), (-4.5, 1.5)]
    }

    /// One deterministic pulse drives both the flame geometry and its point
    /// light, so the visible flame never flickers independently of the light
    /// pool it produces.
    fn torch_pulse(time: f32, index: usize) -> TorchPulse {
        let seed = 700_u32.wrapping_add(index as u32 * 37);
        let phase = hash01(seed) * std::f32::consts::TAU;
        let fast = (time * (9.0 + hash01(seed + 1) * 3.0) + phase).sin();
        let slow = (time * (3.2 + hash01(seed + 2) * 1.4) + phase * 0.61).sin();
        let flicker = (0.84 + fast * 0.08 + slow * 0.05).clamp(0.68, 1.02);
        TorchPulse {
            light_gain: flicker,
            radius_scale: (0.91 + flicker * 0.12).clamp(0.86, 1.04),
            flame_scale: (0.90 + flicker * 0.16).clamp(0.84, 1.08),
            offset: (fast * 0.045 + slow * 0.018, slow * 0.035 - fast * 0.012),
        }
    }

    /// Deterministic short-lived spark around one torch. The cycle phase is
    /// seeded per spark, while its direction, rise, colour and size remain
    /// stable across frames. The fade reaches zero before the spark vanishes.
    fn torch_crackle(
        time: f32,
        torch_index: usize,
        crackle_index: usize,
        origin: (f32, f32),
    ) -> Option<(Vec3, Rgb, f32)> {
        let seed = 2_000_u32
            .wrapping_add(torch_index as u32 * 97)
            .wrapping_add(crackle_index as u32 * 13);
        let phase = hash01(seed) * TORCH_CRACKLE_PERIOD;
        let age = (time + phase).rem_euclid(TORCH_CRACKLE_PERIOD);
        if age >= TORCH_CRACKLE_LIFETIME {
            return None;
        }
        let progress = (age / TORCH_CRACKLE_LIFETIME).clamp(0.0, 1.0);
        let angle = hash01(seed + 1) * std::f32::consts::TAU;
        let speed = 0.36 + hash01(seed + 2) * 0.54;
        let radial = speed * age;
        let rise = 0.62 + hash01(seed + 3) * 0.85;
        let wobble = (time * (7.0 + hash01(seed + 4) * 4.0) + angle).sin() * 0.06;
        let fade = (1.0 - progress).powf(0.72);
        let palette = match crackle_index % 3 {
            0 => Rgb::new(244, 58, 28),
            1 => Rgb::new(255, 126, 30),
            _ => Rgb::new(255, 216, 74),
        };
        let color = palette.scaled(0.62 + fade * 0.38);
        let position = Vec3::new(
            origin.0 + angle.cos() * radial + wobble,
            1.91 + rise * age,
            origin.1 + angle.sin() * radial - wobble * 0.7,
        );
        Some((position, color, fade))
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

    /// Project a world point to the raster view's cell coords (same mapping
    /// as `rasterize_tri`'s fill math) so labels glue to 3D bodies.
    fn project(p: Vec3, cam: &Camera3) -> Option<(i32, i32)> {
        let (sx, sy) = project_frac(p, cam)?;
        Some((sx.round() as i32, sy.round() as i32))
    }

    /// Fractional variant of `project` for continuous primitives: the flame
    /// anchor and crackles keep their true sub-cell offsets and move through
    /// the frame grid without snapping at column/row boundaries (spec:
    /// glyphs must move between logical rows and columns, not behave like a
    /// terminal). Render and emission use the same fractional cell-corner
    /// coordinates, so the glyph and its bloom spread stay aligned.
    fn project_frac(p: Vec3, cam: &Camera3) -> Option<(f32, f32)> {
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
        let sx = cam.width as f32 / 2.0 + cam.focal * rel.dot(right) / depth;
        let sy = cam.height as f32 / 2.0 - cam.focal * rel.dot(up) / depth;
        Some((sx, sy))
    }

    /// Convert a left-click in the world partition into the nearest ground
    /// tile. The presenter supplies physical surface coordinates; this uses
    /// the same authored logical grid and camera basis as the raster pass.
    fn pointer_world_target(
        event: &PointerEvent,
        camera: &Camera3,
        grid_width: u32,
        grid_height: u32,
    ) -> Option<Point> {
        let (surface_width, surface_height) = event.surface_size;
        let (gx, gy) = event.grid_position_fitted(
            surface_width,
            surface_height,
            grid_width,
            grid_height,
            10,
            16,
        )?;
        let world_top = HUD_H as u32;
        let world_bottom = world_top.saturating_add(camera.height.max(0) as u32);
        if gx >= camera.width.max(0) as u32 || gy < world_top || gy >= world_bottom {
            return None;
        }

        let (s, c) = camera.yaw.sin_cos();
        let (sp, cp) = camera.pitch.sin_cos();
        let fwd = Vec3::new(s * cp, sp, c * cp);
        let right = Vec3::new(c, 0.0, -s);
        let up = fwd.cross(right).norm();
        let screen_x = gx as f32 + 0.5;
        let screen_y = (gy - world_top) as f32 + 0.5;
        let ray = Vec3::new(
            fwd.x
                + right.x * (screen_x - camera.width as f32 / 2.0) / camera.focal
                + up.x * (camera.height as f32 / 2.0 - screen_y) / camera.focal,
            fwd.y
                + right.y * (screen_x - camera.width as f32 / 2.0) / camera.focal
                + up.y * (camera.height as f32 / 2.0 - screen_y) / camera.focal,
            fwd.z
                + right.z * (screen_x - camera.width as f32 / 2.0) / camera.focal
                + up.z * (camera.height as f32 / 2.0 - screen_y) / camera.focal,
        )
        .norm();
        if ray.y.abs() < 1e-4 {
            return None;
        }
        let distance = -camera.pos.y / ray.y;
        if !distance.is_finite() || distance < 0.0 {
            return None;
        }
        let hit = Vec3::new(
            camera.pos.x + ray.x * distance,
            0.0,
            camera.pos.z + ray.z * distance,
        );
        if !hit.x.is_finite() || !hit.z.is_finite() {
            return None;
        }
        Some(Point {
            x: hit.x.round() as i16,
            y: hit.z.round() as i16,
        })
    }

    /// Build a bounded, collision-aware cardinal path. Cardinal steps avoid
    /// diagonal corner cutting and keep each queued command equivalent to a
    /// server-authoritative `Command::Move(Direction)`.
    fn path_to_target(start: Point, target: Point) -> Option<Vec<(Direction, Point)>> {
        if start == target {
            return Some(Vec::new());
        }
        let obstacles = overworld_obstacles();
        if is_point_obstructed(target, &obstacles).is_some() {
            return None;
        }
        let directions = [
            Direction::North,
            Direction::South,
            Direction::East,
            Direction::West,
        ];
        let mut frontier = VecDeque::from([(start.x, start.y)]);
        let mut parents: HashMap<(i16, i16), ((i16, i16), Direction)> = HashMap::new();
        parents.insert((start.x, start.y), ((start.x, start.y), Direction::North));

        while let Some(current) = frontier.pop_front() {
            for direction in directions {
                let (dx, dy) = direction.delta();
                let next = (current.0.saturating_add(dx), current.1.saturating_add(dy));
                if parents.contains_key(&next) {
                    continue;
                }
                let point = Point {
                    x: next.0,
                    y: next.1,
                };
                if is_point_obstructed(point, &obstacles).is_some() {
                    continue;
                }
                parents.insert(next, (current, direction));
                if next == (target.x, target.y) {
                    let mut path = Vec::new();
                    let mut cursor = next;
                    while cursor != (start.x, start.y) {
                        let (previous, step) = parents[&cursor];
                        path.push((
                            step,
                            Point {
                                x: cursor.0,
                                y: cursor.1,
                            },
                        ));
                        cursor = previous;
                    }
                    path.reverse();
                    return Some(path);
                }
                frontier.push_back(next);
            }
        }
        None
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
        click_path: VecDeque<(Direction, Point)>,
        click_target: Option<Point>,
        // Camera follow is presentation-only; orientation and scale are fixed
        // constants, while the smoothed focus follows the local player.
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
        /// Presentation-only weather mode. It changes foliage/rain geometry,
        /// never the authoritative simulation or collision map.
        weather: Weather,
        /// Last logical world view used for click-to-ground projection.
        last_camera: Option<Camera3>,
        last_grid: (u32, u32),
        /// Continuous bloom owns torch spill in the continuous presenter; the
        /// grid fallback keeps the legacy point-light path.
        continuous_canvas: bool,
        /// In-world torch migration gate. DEFAULT ON since the play-path
        /// integration (issue #10, after the standalone showcase was
        /// accepted): torches emit native flame/crackle primitives with
        /// continuous float bloom owning torch spill. The legacy mesh
        /// flame + point-light path is the `--legacy-torch` fallback.
        continuous_torch: bool,
        /// Spec gate for the layered continuous-character migration
        /// (issue #8). DEFAULT ON since the play-path integration (issue
        /// #10): the mesh humanoids are the `--legacy-actors` fallback.
        continuous_actors: bool,
        /// Spec gate for the continuous weather migration (issue #9).
        /// DEFAULT ON since the play-path integration (issue #10): the
        /// mesh grass/rain are the `--legacy-weather` fallback.
        continuous_weather: bool,
        /// World-map modal (`M`): swaps the frame for Veyrath world
        /// visuals composed from the shared model (issue #7/#10).
        map_open: bool,
        /// Coverage window captured by `world_meshes` for the gated weather
        /// path, consumed by `continuous_scene` the same frame.
        weather_window: ascii_mmorpg_engine::weather::CoverageWindow,
        /// Smoothed foot anchors + roles captured by `world_meshes` on the
        /// gated path, consumed by `continuous_scene` the same frame.
        native_actor_anchors: Vec<NativeActorAnchor>,
    }

    /// One gated-path actor record: where the layered glyph character is
    /// composed from (presentation-only; authoritative positions stay in the
    /// simulation).
    #[derive(Clone)]
    struct NativeActorAnchor {
        pos: (f32, f32),
        kind: EntityKind,
        skeleton: bool,
        is_player: bool,
        phase: f32,
    }

    impl LocalGame {
        fn new() -> Self {
            // Play-path integration defaults (issue #10): all continuous
            // migrations ON; `--legacy` (or per-system `--legacy-torch` /
            // `--legacy-actors` / `--legacy-weather`) restores the old mesh
            // paths for visual comparison during acceptance. `--grid`
            // selects the legacy grid presenter entirely.
            let legacy_all = std::env::args().any(|arg| arg == "--legacy");
            Self::with_gpu3d_and_weather(
                !std::env::args().any(|arg| arg == "--cpu"),
                weather_from_args(),
            )
            .with_continuous_torch(
                !legacy_all && !std::env::args().any(|arg| arg == "--legacy-torch"),
            )
            .with_continuous_actors(
                !legacy_all && !std::env::args().any(|arg| arg == "--legacy-actors"),
            )
            .with_continuous_weather(
                !legacy_all && !std::env::args().any(|arg| arg == "--legacy-weather"),
            )
        }

        /// Enable the spec-gated in-world torch migration (see the
        /// `continuous_torch` field). Headless `--check` games default to
        /// gate-off and simulate both sides directly.
        fn with_continuous_torch(mut self, enabled: bool) -> Self {
            self.continuous_torch = enabled;
            self
        }

        /// Enable the spec-gated layered-character migration (see the
        /// `continuous_actors` field). Headless `--check` games default to
        /// gate-off and assert both paths directly.
        fn with_continuous_actors(mut self, enabled: bool) -> Self {
            self.continuous_actors = enabled;
            self
        }

        /// Enable the spec-gated weather migration (see the
        /// `continuous_weather` field). Headless `--check` games default to
        /// gate-off and assert both paths directly.
        fn with_continuous_weather(mut self, enabled: bool) -> Self {
            self.continuous_weather = enabled;
            self
        }

        fn with_gpu3d(gpu3d: bool) -> Self {
            Self::with_gpu3d_and_weather(gpu3d, Weather::Rain)
        }

        fn with_gpu3d_and_weather(gpu3d: bool, weather: Weather) -> Self {
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
                click_path: VecDeque::new(),
                click_target: None,
                follow: None,
                smoothed: HashMap::new(),
                combat_fx: Vec::new(),
                seen_combat_events: Vec::new(),
                inv_open: false,
                map_open: false,
                facing: std::f32::consts::PI,
                facing_target: std::f32::consts::PI,
                gait_phase: 0.0,
                attack_started_at: None,
                attack_queued: false,
                gpu3d,
                pending_world: None,
                weather,
                last_camera: None,
                last_grid: (GRID_W, GRID_H),
                continuous_canvas: false,
                // Play-path integration (issue #10): the continuous
                // migrations are the DEFAULT presentation; the CLI
                // `--legacy*` flags in `new()` opt back into the mesh
                // paths. `--check` constructs games explicitly and
                // overrides these per contract block.
                continuous_torch: true,
                continuous_actors: true,
                continuous_weather: true,
                weather_window: ascii_mmorpg_engine::weather::CoverageWindow {
                    x0: 0,
                    x1: 0,
                    z0: 0,
                    z1: 0,
                },
                native_actor_anchors: Vec::new(),
            }
        }
        /// Native flame/crackle primitives for the gated in-world torch
        /// migration (see `continuous_scene`).
        fn push_native_torches(&mut self, scene: &mut ContinuousScene, t: f32, camera: &Camera3) {
            for (torch_index, (tx, tz)) in torch_spots().into_iter().enumerate() {
                let pulse = torch_pulse(t, torch_index);
                let anchor = Vec3::new(tx + pulse.offset.0, 1.92, tz + pulse.offset.1);
                let Some((sx, sy)) = project_frac(anchor, camera) else {
                    continue;
                };
                let flame_y = sy + HUD_H as f32;

                // Shared ASCII flame builder (review 2026-10-25 P1): one
                // function for the in-world torch and the standalone
                // showcase so the accepted read cannot drift.
                ascii_mmorpg_engine::flame::push_ascii_flame(
                    scene,
                    Point2::new(sx, flame_y),
                    ascii_mmorpg_engine::flame::FlamePulse {
                        light_gain: pulse.light_gain,
                        flame_scale: pulse.flame_scale,
                        radius_scale: pulse.radius_scale,
                        sway: pulse.offset.0,
                    },
                    torch_index as f32,
                    t,
                );

                for crackle_index in 0..TORCH_CRACKLE_COUNT {
                    let Some((spark, color, fade)) =
                        torch_crackle(t, torch_index, crackle_index, (anchor.x, anchor.z))
                    else {
                        continue;
                    };
                    let Some((spark_x, spark_y)) = project_frac(spark, camera) else {
                        continue;
                    };
                    scene.push(Primitive::new(
                        PrimitiveKind::particle('*'),
                        Transform2::new(Point2::new(spark_x, spark_y + HUD_H as f32), 1.0)
                            .with_scale(Point2::new(0.34 + fade * 0.38, 0.34 + fade * 0.38))
                            .with_rotation((crackle_index as f32) * 0.7),
                        ContinuousMaterial::new(color)
                            .with_opacity(fade)
                            .with_emission(Emission::new(color, 0.10 * fade, 1.5)),
                    ));
                }
            }
        }

        /// Weather glyphs for the gated weather migration (issue #9):
        /// compose `weather::` world-anchored grass/rain/mote records over
        /// the captured coverage window and project each anchor through
        /// the same camera the raster pass used. Depth keeps weather under
        /// torch/actor primitives; rain has no emission (the bloom field
        /// is reserved for actual light sources).
        fn push_native_weather(&mut self, scene: &mut ContinuousScene, t: f32, camera: &Camera3) {
            use ascii_mmorpg_engine::weather::{weather_fx, FxKind, WeatherMode, WeatherParams};
            let mode = match self.weather {
                Weather::Clear => WeatherMode::Clear,
                Weather::Rain => WeatherMode::Rain,
                Weather::Storm => WeatherMode::Storm,
            };
            let params = WeatherParams::new(mode).with_road_half_width(4.4);
            for fx in weather_fx(&params, self.weather_window, t) {
                let Some((sx, sy)) = project_frac(fx.anchor, camera) else {
                    continue;
                };
                let (kind, depth) = match fx.kind {
                    FxKind::GrassBase => (PrimitiveKind::glyph(fx.glyph), -3.0),
                    FxKind::GrassTip => (PrimitiveKind::glyph(fx.glyph), -2.95),
                    FxKind::Rain => (PrimitiveKind::streak(fx.scale.y, fx.scale.x * 0.2), -2.5),
                    FxKind::Mote => (PrimitiveKind::particle(fx.glyph), -2.8),
                    // Fog canopy: atmosphere above world ink and rain, far
                    // below labels/UI (which paint last).
                    FxKind::FogBand => (PrimitiveKind::glyph(fx.glyph), -2.2),
                };
                scene.push(Primitive::new(
                    kind,
                    Transform2::new(Point2::new(sx, sy + HUD_H as f32), depth)
                        .with_scale(fx.scale)
                        .with_rotation(fx.rotation)
                        .with_skew(fx.skew),
                    ContinuousMaterial::new(fx.color).with_opacity(fx.opacity),
                ));
            }
        }

        /// Layered glyph characters for the gated actor migration (issue #8):
        /// compose `character::` layers at each smoothed, projected foot
        /// anchor captured by `world_meshes` this frame. Palettes mirror the
        /// mesh dispatch (player / skeleton / mossling / herbalist); the
        /// player alone carries an aura. Positions are presentation-only —
        /// the simulation's authoritative entities are untouched.
        fn push_layered_actors(&mut self, scene: &mut ContinuousScene, t: f32, camera: &Camera3) {
            use ascii_mmorpg_engine::character::{
                push_character, CharacterMotion, CharacterPalette, CharacterSpec,
            };
            let attack_phase = self.attack_phase(t);
            let anchors: Vec<NativeActorAnchor> = self.native_actor_anchors.clone();
            for actor in &anchors {
                let Some((sx, sy)) = project_frac(Vec3::new(actor.pos.0, 0.0, actor.pos.1), camera)
                else {
                    continue;
                };
                let anchor = Point2::new(sx, sy + HUD_H as f32);
                let (palette, scale, aura) = match (&actor.kind, actor.skeleton) {
                    (EntityKind::Player, _) => (
                        CharacterPalette {
                            head: Rgb::new(215, 255, 175),
                            torso: Rgb::new(130, 225, 95),
                            limb: Rgb::new(95, 195, 75),
                            gear: Rgb::new(240, 220, 150),
                            aura: Rgb::new(255, 225, 130),
                        },
                        // User verdict 2026-10-25: the layered player reads
                        // too small against the world — doubled from 1.25.
                        2.5,
                        Some(Rgb::new(255, 225, 130)),
                    ),
                    (EntityKind::Monster, true) => (
                        CharacterPalette {
                            head: Rgb::new(238, 235, 220),
                            torso: Rgb::new(225, 220, 200),
                            limb: Rgb::new(205, 200, 185),
                            gear: Rgb::new(150, 145, 135),
                            aura: Rgb::new(0, 0, 0),
                        },
                        1.15,
                        None,
                    ),
                    (EntityKind::Monster, false) => (
                        CharacterPalette {
                            head: Rgb::new(150, 215, 120),
                            torso: Rgb::new(95, 175, 80),
                            limb: Rgb::new(75, 150, 65),
                            gear: Rgb::new(120, 160, 90),
                            aura: Rgb::new(0, 0, 0),
                        },
                        1.18,
                        None,
                    ),
                    _ => (
                        CharacterPalette {
                            head: Rgb::new(200, 245, 235),
                            torso: Rgb::new(120, 210, 200),
                            limb: Rgb::new(95, 180, 175),
                            gear: Rgb::new(230, 200, 140),
                            aura: Rgb::new(0, 0, 0),
                        },
                        1.10,
                        None,
                    ),
                };
                let motion = CharacterMotion {
                    gait: if actor.is_player {
                        self.gait_phase
                    } else {
                        0.0
                    },
                    action: attack_phase,
                    idle: actor.phase,
                    facing: self.facing,
                    t,
                };
                push_character(
                    scene,
                    &CharacterSpec {
                        palette,
                        scale,
                        aura,
                    },
                    motion,
                    anchor,
                );
            }
        }

        fn tick_once(&mut self) {
            self.client_tick = self.client_tick.saturating_add(1);
            if self.movement_axes().0.abs() < 0.01 && self.movement_axes().1.abs() < 0.01 {
                if let Some((direction, _step)) = self.click_path.pop_front() {
                    self.facing_target = match direction {
                        Direction::North => 0.0,
                        Direction::South => std::f32::consts::PI,
                        Direction::East => std::f32::consts::FRAC_PI_2,
                        Direction::West => -std::f32::consts::FRAC_PI_2,
                        Direction::NorthEast => std::f32::consts::FRAC_PI_4,
                        Direction::NorthWest => -std::f32::consts::FRAC_PI_4,
                        Direction::SouthEast => 3.0 * std::f32::consts::FRAC_PI_4,
                        Direction::SouthWest => -3.0 * std::f32::consts::FRAC_PI_4,
                    };
                    self.pending.push(Command::Move(direction));
                }
            }
            let mut commands = Vec::new();
            for command in self.pending.drain(..) {
                commands.push((PLAYER_ID, ClientCommand::new(self.client_tick, command)));
            }
            self.sim.advance_tick_with(commands);
        }

        /// WASD/arrows held as fixed world axes: W/North, S/South, A/West,
        /// D/East. The motion model snaps the result to the eight protocol
        /// directions before sending an intent to the server.
        fn movement_axes(&self) -> (f32, f32) {
            // The world map is a modal: gameplay input is frozen while it
            // is open (server ticks continue, exactly like the TUI pause
            // contract).
            if self.map_open {
                return (0.0, 0.0);
            }
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

        /// Advance sim ticks + local motion for the elapsed frame time. Owns the
        /// `last_t` bookkeeping; callers reuse the returned `dt` for smoothing.
        fn advance(&mut self, t: f32) -> (f32, WorldSnapshot) {
            let dt = (t - self.last_t).max(0.0);
            self.last_t = t;
            let input = self.movement_axes();
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
            if input_len > 0.01 || speed > 0.12 || self.click_target.is_some() {
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
            if self.click_target == Some(snapshot.player.position) && self.click_path.is_empty() {
                self.click_target = None;
            }
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

        /// Fixed oblique camera for this frame: pan-glides toward the smoothed
        /// player anchor and looks down along the constant yaw/pitch. The
        /// raster camera has no look-at, so pose = place the eye at
        /// `aim − fwd·d` (the forward ray then passes exactly through anchor).
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
            let (sp, cp) = CAM_PITCH.sin_cos();
            let (sy, cy) = CAM_YAW.sin_cos();
            let fwd = Vec3::new(sy * cp, sp, cy * cp);
            let aim = Vec3::new(next.0, 1.6, next.1); // hips anchor
            let eye = Vec3::new(
                aim.x - fwd.x * CAM_DIST,
                aim.y - fwd.y * CAM_DIST,
                aim.z - fwd.z * CAM_DIST,
            );
            // Preserve gameplay scale across resizes: fullscreen often adds
            // far more rows than the 160x56 window preset. Using the smaller
            // scale here made the camera reveal extra vertical world while
            // keeping the actor at the old size, which turned fullscreen into
            // sparse horizontal terrain bands with unreadable silhouettes.
            // Match the largest axis so the controlled character keeps its
            // readable cell height; the larger frustum is intentional only
            // when the surface itself is genuinely larger in both axes.
            let width_scale = w as f32 / GRID_W as f32;
            let height_scale = world_h as f32 / (GRID_H as i32 - HUD_H - LOG_H) as f32;
            let focal_scale = width_scale.max(height_scale).clamp(0.82, 2.25);
            Camera3::new(eye, CAM_YAW, CAM_PITCH, CAM_FOCAL * focal_scale, w, world_h)
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
            // Retention follows the shared phase model (review
            // 2026-10-25 P1): one definition of when an effect ends.
            self.combat_fx.retain(|fx| {
                ascii_mmorpg_engine::fx_timeline::EffectTimeline::new(
                    fx.born_s,
                    fx.telegraph_s,
                    bridge::FX_BURST_S,
                    bridge::FX_CORE_S,
                )
                .is_live(t)
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
            // The same smoothed focus the camera aims at, so coverage and
            // frustum always agree on "here".
            let focus = player;
            // Camera-following coverage anchors. The lattices below slide
            // with the player but stay locked to the world's even grid, so
            // every decal keeps one permanent world position (hash inputs
            // are absolute coordinates). The z margin covers the fullscreen
            // fog-limited ground span (~40 units at the fixed pull-back) with headroom;
            // the x margin covers the horizontal frustum at ink-visible depths.
            // Beyond the fog radius the fill renders pure void regardless,
            // so wider margins would only burn triangles.
            let focus_x = focus.0.round() as i32;
            let focus_z = focus.1.round() as i32;
            let even = |v: i32| v.div_euclid(2) * 2;
            let zone_x0 = even(focus_x);
            let zone_z0 = even(focus_z);
            const COVERAGE_DZ: i32 = 46;
            const COVERAGE_DX: i32 = 34;
            let weather = weather_profile(self.weather, t);
            // The gated weather path re-anchors `weather::` primitives to
            // this same coverage window each frame, so the mesh lattice and
            // the continuous lattice always agree on "here".
            self.weather_window = ascii_mmorpg_engine::weather::CoverageWindow {
                x0: zone_x0 - COVERAGE_DX,
                x1: zone_x0 + COVERAGE_DX,
                z0: zone_z0 - COVERAGE_DZ,
                z1: zone_z0 + COVERAGE_DZ,
            };
            let native_weather = self.continuous_canvas && self.continuous_weather;

            // The world has no coloured floor, skybox, fog card, or terrain
            // fill. Selective decals and small solids describe the path,
            // shoreline, grass, and stones; every other viewport cell is the
            // presenter's exact black clear colour.
            for z in (zone_z0 - COVERAGE_DZ)..=(zone_z0 + COVERAGE_DZ) {
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
            // wide fullscreen surface without reverting to a solid floor. The repeated
            // 2-cell motif alternates direction by row, skips the highway
            // corridor, and leaves generous black negative space between
            // ink marks. The lattice spans the camera-following coverage
            // box: world-locked (absolute gx/gz hash + absolute row/column
            // motif indices), so walking or camera-following never re-shuffles the
            // pattern underfoot.
            for gz in (zone_z0 - COVERAGE_DZ..=(zone_z0 + COVERAGE_DZ)).step_by(2) {
                let row = gz.div_euclid(2);
                for gx in (zone_x0 - COVERAGE_DX..=(zone_x0 + COVERAGE_DX)).step_by(2) {
                    let column = gx.div_euclid(2);
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
                for segment in -14..=14 {
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
            // carpet. They live on a world-locked coarse lattice that
            // scrolls with the coverage anchors (hash inputs are absolute
            // coordinates), so positions stay stable while the player
            // moves and the black surround stays quiet. Unlike the older
            // ground-only decals, each tuft has a grounded blade whose tip
            // leans with the current weather gust. The gated path composes
            // the same idea as continuous `weather::` primitives instead.
            if !native_weather {
                for gz in (zone_z0 - COVERAGE_DZ..=(zone_z0 + COVERAGE_DZ)).step_by(4) {
                    for gx in (zone_x0 - COVERAGE_DX..=(zone_x0 + COVERAGE_DX)).step_by(4) {
                        if scene_hash(gx, gz + 91) % 2 != 0 {
                            continue;
                        }
                        let x = gx as f32 + (scene_hash(gx, gz) % 3) as f32 * 0.33;
                        let z = gz as f32 + (scene_hash(gx, gz + 3) % 3) as f32 * 0.9;
                        if x.abs() < 4.4 {
                            continue;
                        }
                        let phase = hash01(scene_hash(gx + 17, gz - 23)) * std::f32::consts::TAU
                            + x * 0.37
                            + z * 0.19;
                        let sway = (t * weather.grass_speed + phase).sin();
                        let cross = (t * weather.grass_speed * 0.71 + phase * 1.7).cos();
                        let tip_offset = Vec3::new(
                            sway * weather.grass_amplitude,
                            0.0,
                            cross * weather.grass_amplitude * 0.38,
                        );
                        let height = 0.26
                            + (scene_hash(gx, gz + 401) % 5) as f32 * 0.045
                            + sway.abs() * 0.035;
                        let yaw = if scene_hash(gx, gz + 211) & 1 == 0 {
                            0.18
                        } else {
                            -0.18
                        };
                        meshes.push(Mesh::blade_y(
                            Vec3::new(x, 0.022, z),
                            height,
                            0.055,
                            tip_offset,
                            yaw,
                            WORLD.grass,
                        ));
                        meshes.push(Mesh::decal_y(
                            Vec3::new(x, 0.022, z),
                            yaw,
                            0.24,
                            0.045,
                            WORLD.grass,
                        ));
                        if scene_hash(gx, gz + 313) & 1 == 0 {
                            meshes.push(Mesh::decal_y(
                                Vec3::new(x + 0.08, 0.024, z + 0.05),
                                if scene_hash(gx, gz + 317) & 1 == 0 {
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
                }
            }

            // Directional rain is a presentation-only field. World-locked
            // seeds keep drops stable as the camera follows, while the
            // fractional fall phase and wind drift move each streak smoothly
            // through depth instead of replacing one terminal slash with
            // another. Storms increase density, fall speed, and lean. The
            // gated path composes `weather::` streak primitives instead.
            if weather.rain_density > 0 && !native_weather {
                for rz in (zone_z0 - COVERAGE_DZ..=(zone_z0 + COVERAGE_DZ)).step_by(4) {
                    for rx in (zone_x0 - COVERAGE_DX..=(zone_x0 + COVERAGE_DX)).step_by(4) {
                        if scene_hash(rx, rz + 701) % 100 >= weather.rain_density {
                            continue;
                        }
                        let seed = scene_hash(rx + 31, rz - 47);
                        let phase = hash01(seed);
                        let depth = (seed % 100) as f32 / 99.0;
                        let fall = (t * weather.rain_speed + phase).fract();
                        let base_x = rx as f32 + (scene_hash(rx, rz + 703) % 17) as f32 * 0.11;
                        let base_z = rz as f32 + (scene_hash(rx, rz + 709) % 17) as f32 * 0.11;
                        let top_y = 5.0 + depth * 2.6;
                        let y = top_y - fall * 7.0;
                        if y < -0.8 || y > 7.8 {
                            continue;
                        }
                        let drift = fall * 1.4;
                        let start = Vec3::new(
                            base_x + weather.wind_x * drift,
                            y,
                            base_z + weather.wind_z * drift,
                        );
                        let length = 0.58 + depth * 0.42;
                        let end = start
                            + Vec3::new(weather.wind_x * 0.30, -length, weather.wind_z * 0.30);
                        meshes.push(Mesh::streak(
                            start,
                            end,
                            0.018 + depth * 0.012,
                            weather.rain_color,
                        ));
                    }
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

            // Torches are visible glyph geometry and their synchronized point
            // lights are applied to nearby visible faces in the raster pass.
            // Crackles are short-lived world sparks: warm, directional and
            // fully deterministic, with no circular pool mesh underneath.
            // The gated native path keeps the wooden torch base in the world
            // mesh but moves flame/crackle geometry to the continuous scene so
            // the two representations cannot render on top of each other.
            let native_torch = self.continuous_canvas && self.continuous_torch;
            for (i, (tx, tz)) in torch_spots().into_iter().enumerate() {
                let pulse = torch_pulse(t, i);
                meshes.push(Mesh::boxed(
                    Vec3::new(tx, 0.95, tz),
                    Vec3::new(0.16, 1.9, 0.16),
                    WORLD.bark,
                ));
                if !native_torch {
                    let flame = WORLD.fire.lerp(WORLD.ember, pulse.light_gain * 0.42);
                    meshes.push(Mesh::boxed(
                        Vec3::new(tx + pulse.offset.0, 1.88, tz + pulse.offset.1),
                        Vec3::new(
                            0.34 * pulse.flame_scale,
                            0.22 * pulse.flame_scale,
                            0.34 * pulse.flame_scale,
                        ),
                        flame,
                    ));
                    meshes.push(Mesh::cone(
                        Vec3::new(tx + pulse.offset.0, 1.92, tz + pulse.offset.1),
                        0.28 * pulse.flame_scale,
                        0.62 * pulse.flame_scale,
                        7,
                        flame,
                    ));
                    for crackle_index in 0..TORCH_CRACKLE_COUNT {
                        let Some((position, color, fade)) = torch_crackle(
                            t,
                            i,
                            crackle_index,
                            (tx + pulse.offset.0, tz + pulse.offset.1),
                        ) else {
                            continue;
                        };
                        let size = 0.07 + 0.045 * fade;
                        meshes.push(Mesh::boxed(
                            position,
                            Vec3::new(size, size * 1.35, size),
                            color,
                        ));
                    }
                }
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
            // The layered-character gate keeps the anchors and labels but
            // swaps the mesh silhouettes + shadow decals for native
            // continuous glyph layers (see `continuous_scene`).
            let native_actors = self.continuous_canvas && self.continuous_actors;
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
            self.native_actor_anchors.clear();
            if !native_actors {
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
            } else {
                self.native_actor_anchors.push(NativeActorAnchor {
                    pos: player,
                    kind: EntityKind::Player,
                    skeleton: false,
                    is_player: true,
                    phase: self.gait_phase,
                });
            }
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
                if native_actors {
                    self.native_actor_anchors.push(NativeActorAnchor {
                        pos: (gx, gy),
                        kind: entity.kind.clone(),
                        skeleton: entity.name.to_ascii_lowercase().contains("skeleton"),
                        is_player: false,
                        phase: idle_phase,
                    });
                    continue;
                }
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

            // Point-and-click destination and a short, quiet path preview.
            // These are ground decals, so they share the same depth ordering
            // as terrain and never become a screen-fixed UI element.
            for (index, (_direction, point)) in self.click_path.iter().take(12).enumerate() {
                let tint = if index % 2 == 0 {
                    WORLD.path_edge
                } else {
                    mix(WORLD.path_edge, TARGET_ORANGE, 0.45)
                };
                meshes.push(Mesh::decal_y(
                    Vec3::new(point.x as f32, 0.042, point.y as f32),
                    0.0,
                    0.18,
                    0.055,
                    tint,
                ));
            }
            if let Some(destination) = self.click_target {
                for i in 0..8 {
                    let angle = i as f32 * std::f32::consts::TAU / 8.0;
                    meshes.push(Mesh::decal_y(
                        Vec3::new(
                            destination.x as f32 + angle.sin() * 0.72,
                            0.048,
                            destination.y as f32 + angle.cos() * 0.72,
                        ),
                        angle + std::f32::consts::FRAC_PI_2,
                        0.18,
                        0.08,
                        TARGET_ORANGE,
                    ));
                }
            }

            // Combat FX: telegraph decal ring → core/ring burst → embers,
            // the mesh re-expression of the TUI's FX packs. All effects
            // share the `fx_timeline` phase model (review 2026-10-25 P1):
            // one place decides telegraph/impact/decay boundaries and
            // retention; the renderer below only draws phases.
            for fx in &self.combat_fx {
                use ascii_mmorpg_engine::fx_timeline::{EffectPhase, EffectTimeline};
                let timeline = EffectTimeline::new(
                    fx.born_s,
                    fx.telegraph_s,
                    bridge::FX_BURST_S,
                    bridge::FX_CORE_S,
                );
                let origin = Vec3::new(fx.origin.0, 0.0, fx.origin.1);
                let radius = fx.radius.max(0.5);
                match timeline.phase(t) {
                    EffectPhase::Telegraph { progress } => {
                        let p = progress;
                        let r = radius * (0.35 + 0.65 * (1.0 - p));
                        for i in 0..12 {
                            let a = i as f32 * std::f32::consts::TAU / 12.0
                                + 0.4 * (fx.born_s.fract() * 6.0);
                            let dash =
                                Vec3::new(origin.x + r * a.sin(), 0.04, origin.z + r * a.cos());
                            meshes.push(Mesh::decal_y(
                                dash,
                                a + std::f32::consts::FRAC_PI_2,
                                0.22,
                                0.1,
                                fx.color,
                            ));
                        }
                    }
                    // Impact renders the burst; the clamped tail of Decay
                    // keeps the eased end-state until retention drops it
                    // (same visuals the raw age math produced).
                    EffectPhase::Impact { .. } | EffectPhase::Decay { .. } => {
                        let p = match timeline.phase(t) {
                            EffectPhase::Impact { progress } => progress,
                            EffectPhase::Decay { .. } => 1.0,
                            _ => 1.0,
                        };
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
                            let dash =
                                Vec3::new(origin.x + r * a.sin(), 0.05, origin.z + r * a.cos());
                            meshes.push(Mesh::decal_y(
                                dash,
                                a + std::f32::consts::FRAC_PI_2,
                                0.3,
                                0.12,
                                fx.color,
                            ));
                        }
                        // Six rising embers (TUI parity #2) rise on the
                        // shared ease-in curve.
                        for i in 0..6 {
                            let a = i as f32 * std::f32::consts::TAU / 6.0;
                            let e_r = 0.3 + p * 1.4;
                            let ember = Vec3::new(
                                origin.x + a.sin() * e_r,
                                0.4 + ascii_mmorpg_engine::fx_timeline::ease_in(p) * 2.2,
                                origin.z + a.cos() * e_r,
                            );
                            meshes.push(Mesh::boxed(
                                ember,
                                Vec3::new(0.16, 0.16, 0.16),
                                mix(fx.color, Rgb::new(255, 190, 80), 0.4),
                            ));
                        }
                    }
                    EffectPhase::Done => {}
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
        fn present_mode(&mut self, mode: PresentMode) {
            self.continuous_canvas = mode == PresentMode::Continuous;
        }

        /// Either spec-gated migration (torch or layered actors) hands world
        /// lighting to the float bloom field; every other configuration keeps
        /// the legacy CPU glow pass and localized point lights.
        fn continuous_bloom_owns_world_lighting(&mut self) -> bool {
            self.continuous_torch || self.continuous_actors
        }

        /// Quiet local ground around the native figures (review 2026-10-25
        /// P0): the presenter clears composed world ink near these
        /// projected foot anchors so silhouettes stand on a locally uniform
        /// floor. Only the native-actor path claims a stage; the legacy
        /// mesh path keeps its original look.
        fn stage_anchors(&mut self, _t: f32) -> Vec<Point2> {
            if !(self.continuous_canvas && self.continuous_actors) {
                return Vec::new();
            }
            let Some(camera) = self.last_camera else {
                return Vec::new();
            };
            let anchors = self.native_actor_anchors.clone();
            anchors
                .iter()
                .filter_map(|actor| {
                    project_frac(Vec3::new(actor.pos.0, 0.0, actor.pos.1), &camera)
                        .map(|(sx, sy)| Point2::new(sx, sy + HUD_H as f32))
                })
                .collect()
        }

        fn scene_at(&mut self, t: f32) -> (Scene, Camera) {
            self.scene_at_size(t, GRID_W, GRID_H)
        }

        fn gpu_world(&mut self) -> Option<GpuWorld> {
            // Once per frame, paired with scene_at_size above. `--check`
            // and the engine tests run scene builders without the
            // presenter, so leftovers here are presentation-state only.
            self.pending_world.take()
        }

        /// Native primitives for the in-play continuous layer: the world
        /// map modal (`M`, composed from the shared Veyrath model) takes
        /// over the whole continuous scene while open; otherwise the
        /// gated migrations contribute torch flame/crackles, weather
        /// glyphs, and layered characters (all default-on since issue
        /// #10; `--legacy*` flags restore the mesh paths). Coordinates
        /// are authored frame-grid cell-corner space, projected
        /// fractionally so motion stays sub-cell.
        fn continuous_scene(&mut self, _t: f32) -> Option<ContinuousScene> {
            if self.map_open {
                let (w, h) = self.last_grid;
                return Some(ascii_mmorpg_engine::veyrath_world::world_scene(
                    ascii_mmorpg_engine::veyrath_world::WorldView::new(w, h),
                    _t,
                ));
            }
            if !self.continuous_torch && !self.continuous_actors && !self.continuous_weather {
                return None;
            }
            let camera = self.last_camera?;
            let mut scene = ContinuousScene::new();
            if self.continuous_torch {
                self.push_native_torches(&mut scene, _t, &camera);
            }
            if self.continuous_weather {
                self.push_native_weather(&mut scene, _t, &camera);
            }
            if self.continuous_actors {
                self.push_layered_actors(&mut scene, _t, &camera);
            }
            Some(scene)
        }

        fn scene_at_size(&mut self, t: f32, width: u32, height: u32) -> (Scene, Camera) {
            let (w, h) = (width as i32, height as i32);
            // World-map modal: the frame swaps to a caption sheet (the map
            // itself is native continuous primitives sized to the live
            // grid); no world meshes are built while it is open.
            if self.map_open {
                // The server keeps ticking while the modal is open (same
                // contract as the TUI pause); only presentation swaps.
                self.advance(t);
                self.last_grid = (width, height);
                // Video mimicry S1: the modal's caption sheet runs at a
                // coarser cell pitch than the world — the reference game's
                // inventory uses ~8x12px glyphs against the world's
                // ~6.5x9.5 — so captions are authored on a w/PITCH x
                // h/PITCH rect and the partition scales them back to
                // full-screen at emission. The native map art below stays
                // at world pitch (fine detail).
                const MAP_PITCH: f32 = 1.3;
                let cw = ((w as f32) / MAP_PITCH).floor().max(8.0) as i32;
                let ch = ((h as f32) / MAP_PITCH).floor().max(4.0) as i32;
                let dim = Rgb::new(150, 140, 118);
                let faint = Rgb::new(90, 84, 70);
                let void = Rgb::new(0, 0, 0);
                let mut scene = Scene::default();
                let mut ui = Layer::new("map-ui", 0);
                let title = ascii_mmorpg_engine::veyrath::visual_data()
                    .title
                    .to_ascii_uppercase();
                ui.push_text_clipped(
                    2,
                    0,
                    (cw - 4).max(0),
                    &format!("{title} — WORLD MAP"),
                    dim,
                    void,
                );
                ui.push_text_clipped(
                    2,
                    1,
                    (cw - 4).max(0),
                    "regions · rivers · roads · faction settlements · landmarks — starter: MILLHAVEN",
                    faint,
                    void,
                );
                // Esc is the presenter's global quit (it never reaches the
                // source), so the caption must only advertise M.
                ui.push_text_clipped(2, ch - 1, (cw - 4).max(0), "M CLOSES", faint, void);
                scene.layers.push(ui);
                scene.partitions.push(
                    Partition::new("map-ui", 0, 0, cw, ch, vec![0], false).with_pitch(MAP_PITCH),
                );
                return (scene, Camera::default());
            }
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
            self.last_camera = Some(camera);
            self.last_grid = (width, height);
            let mut scene = Scene::default();

            // ---- HUD band ----
            let mut hud = Layer::new("hud", 0);
            hud.fill_rect(0, 0, w, HUD_H, Cell::new(' ', HUD_TEXT, PANEL_BG));
            hud.border_rect(0, 0, w, HUD_H, PANEL_EDGE, PANEL_BG);
            let hud_text_x = 18.min((w - 2).max(1));
            let hud_text_width = (w - hud_text_x - 1).max(0);
            // The play renderer consumes the same typed Veyrath source as the
            // terminal World Map. These are provisional visual anchors only;
            // they never author simulation positions or collision.
            let veyrath = ascii_mmorpg_engine::veyrath::visual_data();
            let region_title = veyrath
                .region("greyfall")
                .map(|region| region.name.to_ascii_uppercase())
                .unwrap_or_else(|| "WHISPERING FOREST".to_string());
            hud.push_text_clipped_bold(
                hud_text_x,
                0,
                hud_text_width,
                &region_title,
                DIGIT_GOLD,
                PANEL_BG,
            );
            let status = format!(
                "{}   HP {}/{}   TICK {}",
                scene_zone(&snapshot),
                snapshot.player.stats.hp,
                snapshot.player.stats.max_hp,
                snapshot.tick
            );
            hud.push_text_clipped(hud_text_x, 1, hud_text_width, &status, HUD_TEXT, PANEL_BG);
            hud.push_text_clipped(
                2,
                3,
                (w - 4).max(0),
                "WASD / Left-click Move   F Attack   1-3 Cast   I Inventory   M Map   R Rest   F11 Fullscreen   Q Quit",
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

            // Emit into the engine light buffer at the exact projected flame
            // anchor. This layer is below the 3D world, so its blank source
            // cell never occludes terrain; `compose::apply_glow` washes the
            // torch colour over existing world glyphs for the soft reference
            // halo instead of painting a flat screen-space circle.
            // Per-system lighting (review 2026-10-25): when the torch system
            // runs LEGACY (mesh flame + point lights) while bloom ownership
            // is claimed by another migration, this glow layer is skipped —
            // otherwise the torch would double-light (raster point light +
            // bloom from these cells). When the torch migration is native,
            // the cells bridge into the bloom field as before.
            let torch_native = self.continuous_canvas && self.continuous_torch;
            let bloom_owned =
                self.continuous_canvas && (self.continuous_torch || self.continuous_actors);
            let torch_glow_idx = scene.layers.len();
            if !(bloom_owned && !torch_native) {
                let mut torch_glow = Layer::new("torch-glow", -1);
                for (i, (tx, tz)) in torch_spots().into_iter().enumerate() {
                    let pulse = torch_pulse(t, i);
                    if let Some((sx, sy)) = project(
                        Vec3::new(tx + pulse.offset.0, 1.92, tz + pulse.offset.1),
                        &camera,
                    ) {
                        let glow_color = WORLD.fire.lerp(WORLD.ember, 0.30);
                        torch_glow.push(AnimCell::still(
                            sx,
                            sy,
                            Cell::new(' ', glow_color, Rgb::new(0, 0, 0))
                                .with_glow(0.82 * pulse.light_gain),
                        ));
                    }
                }
                scene.layers.push(torch_glow);
            }

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
            let mut lights = Vec::with_capacity(3 + 3 * TORCH_CRACKLE_COUNT + self.combat_fx.len());
            for (i, (tx, tz)) in torch_spots().into_iter().enumerate() {
                let pulse = torch_pulse(t, i);
                lights.push(PointLight::new(
                    Vec3::new(tx + pulse.offset.0, 1.8, tz + pulse.offset.1),
                    WORLD.fire.lerp(WORLD.ember, 0.28),
                    5.0 * pulse.radius_scale,
                    1.15 * pulse.light_gain,
                ));
                for crackle_index in 0..TORCH_CRACKLE_COUNT {
                    let Some((position, color, fade)) = torch_crackle(
                        t,
                        i,
                        crackle_index,
                        (tx + pulse.offset.0, tz + pulse.offset.1),
                    ) else {
                        continue;
                    };
                    lights.push(PointLight::new(position, color, 0.95 * fade, 0.34 * fade));
                }
            }
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
            // Lighting ownership is per-system (review 2026-10-25): legacy
            // torch point lights stay active whenever the TORCH system runs
            // its mesh path — even if actors/weather run native and claim
            // bloom ownership — so `--legacy-torch` (or full `--legacy`)
            // remains visually equivalent. Only the torch migration itself
            // drops the raster lights (its light comes from native flame
            // emission + the glow bridge).
            let active_lights = if torch_native {
                Vec::new()
            } else {
                lights.clone()
            };
            if self.gpu3d {
                self.pending_world = Some(GpuWorld {
                    meshes,
                    camera,
                    sun,
                    opts,
                    lights: active_lights,
                    z: 0,
                    layer_slot: terrain_idx,
                });
                scene.layers.push(Layer::new("mesh3d", 0)); // placeholder
            } else {
                let terrain = raster::render_layer_with_lights(
                    &meshes,
                    &camera,
                    sun,
                    &opts,
                    &active_lights,
                    0,
                );
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
                    let name_width = cell_text_width(&name) as i32;
                    let name_x = sx - name_width / 2;
                    labels.fill_rect(
                        name_x - 1,
                        sy - 1,
                        name_width + 2,
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
                        sx - cell_text_width(&row) as i32 / 2,
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
                        sx - cell_text_width(&fx.digits) as i32 / 2,
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
                log.push_text_clipped(2, 1 + i as i32, (w - 4).max(0), message, HUD_DIM, PANEL_BG);
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
                vec![torch_glow_idx, terrain_idx],
                true,
            ));
            // Labels are UI ink (review 2026-10-25 P1): their own
            // non-camera partition classifies them as UI cells, so the
            // presenter paints them AFTER native actors/weather —
            // nameplates and HP bars can never be occluded by figures.
            scene.partitions.push(Partition::new(
                "labels",
                0,
                HUD_H,
                w,
                world_h,
                vec![labels_idx],
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
                modal.push_text_clipped(
                    4,
                    mh - 3,
                    (mw - 8).max(0),
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
            // Storm lightning (review 2026-10-25 P1): a deterministic
            // bolt schedule briefly lifts the whole frame with a cool
            // wash — atmosphere, not a strobe; labels keep their backing.
            if matches!(self.weather, Weather::Storm) {
                let flash = ascii_mmorpg_engine::weather::lightning_flash(t, 0x5702);
                if flash > 0.01 {
                    scene.wash = Some(Wash {
                        tint: Rgb::new(190, 200, 235),
                        strength: 0.35 * flash,
                    });
                }
            }
            (scene, Camera::default())
        }

        fn key(&mut self, code: KeyCode, pressed: bool) {
            // World-map modal: `M` toggles; every other press is frozen
            // (releases still clear held keys so nothing latches when the
            // map closes).
            if self.map_open {
                if pressed {
                    if code == KeyCode::KeyM {
                        self.map_open = false;
                    }
                    return;
                }
                self.held.remove(&code);
                return;
            }
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
                    KeyCode::KeyM => self.map_open = true,
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
                        self.click_path.clear();
                        self.click_target = None;
                        self.held.insert(code);
                    } else {
                        self.held.remove(&code);
                    }
                }
                _ => {}
            }
        }

        fn pointer(&mut self, event: PointerEvent) {
            if self.inv_open || !event.pressed[0] {
                return;
            }
            let Some(camera) = self.last_camera else {
                return;
            };
            let Some(target) =
                pointer_world_target(&event, &camera, self.last_grid.0, self.last_grid.1)
            else {
                return;
            };
            let start = self.sim.snapshot_for(PLAYER_ID).player.position;
            let Some(path) = path_to_target(start, target) else {
                self.click_path.clear();
                self.click_target = None;
                return;
            };
            self.click_path = path.into_iter().collect();
            self.click_target = if self.click_path.is_empty() {
                None
            } else {
                Some(target)
            };
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
        if scene.partitions.len() != 4 {
            return Err(format!(
                "check: expected 4 partitions with modal closed (hud/world/labels/log), got {}",
                scene.partitions.len()
            ));
        }
        // Labels are UI ink now (own non-camera partition): they must NOT
        // be part of the camera-marked world partition, so the presenter
        // paints them after native actors/weather (review 2026-10-25 P1).
        let labels_idx = scene
            .layers
            .iter()
            .position(|layer| layer.name == "labels")
            .ok_or("check: labels layer index missing")?;
        if let Some(world) = scene.partitions.iter().find(|p| p.name == "world") {
            if !world.camera || world.layers.contains(&labels_idx) {
                return Err("check: labels must live in their own non-camera partition".into());
            }
        } else {
            return Err("check: world partition missing".into());
        }
        let labels_partition = scene
            .partitions
            .iter()
            .find(|p| p.layers.contains(&labels_idx))
            .ok_or("check: labels partition missing")?;
        if labels_partition.camera {
            return Err("check: labels partition must not be camera-marked".into());
        }
        let hud = scene
            .layer("hud")
            .and_then(|i| scene.layers.get(i))
            .ok_or("check: HUD layer missing")?;
        if !hud.cells.iter().any(|cell| cell.cell.glyph == 'W')
            || !hud.cells.iter().any(|cell| cell.cell.glyph == 'H')
            || !hud.cells.iter().any(|cell| cell.cell.glyph == 'Q')
        {
            return Err("check: cell-aligned HUD title/status/help text missing".into());
        }
        if !scene.overlay_text.is_empty() {
            return Err("check: play HUD still relies on fractional overlay text".into());
        }
        let torch_glow = scene
            .layer("torch-glow")
            .and_then(|i| scene.layers.get(i))
            .ok_or("check: torch glow layer missing")?;
        if !torch_glow
            .cells
            .iter()
            .any(|cell| cell.cell.glow.is_finite() && cell.cell.glow > 0.0)
        {
            return Err("check: torch glow layer emitted no light-buffer sources".into());
        }
        let composed = compose(&scene, &Camera::default(), 0.06);
        let mut tinted_world_cells = 0usize;
        for cell in &terrain.cells {
            let Some(resolved) = composed.get(cell.x, cell.y) else {
                continue;
            };
            if resolved.glyph != cell.cell.glyph {
                continue;
            }
            let delta = resolved
                .fg
                .r
                .abs_diff(cell.cell.fg.r)
                .max(resolved.fg.g.abs_diff(cell.cell.fg.g))
                .max(resolved.fg.b.abs_diff(cell.cell.fg.b));
            if delta > 0 {
                tinted_world_cells += 1;
            }
        }
        if tinted_world_cells == 0 {
            return Err("check: composed torch glow warmed no world ink".into());
        }

        // Continuous-canvas world-lighting contract: since the play-path
        // integration (issue #10) the continuous migrations are DEFAULT
        // ON. The fully-legacy fallback is selected explicitly here
        // (`--legacy` at the CLI): it keeps the mesh grass/rain/actors
        // plus legacy point lights, and produces no native primitives.
        let mut legacy_canvas = LocalGame::with_gpu3d(true);
        legacy_canvas.continuous_canvas = true;
        legacy_canvas.continuous_torch = false;
        legacy_canvas.continuous_actors = false;
        legacy_canvas.continuous_weather = false;
        legacy_canvas.scene_at(0.06);
        let (legacy_mesh_count, legacy_has_lights) = legacy_canvas
            .pending_world
            .as_ref()
            .map(|world| (world.meshes.len(), !world.lights.is_empty()))
            .ok_or("check: continuous canvas world missing")?;
        if !legacy_has_lights {
            return Err("check: continuous canvas must keep legacy point lights by default".into());
        }
        if legacy_canvas.continuous_scene(0.06).is_some() {
            return Err("check: in-world torch primitives ran without the gate flag".into());
        }

        let mut gated = LocalGame::with_gpu3d(true);
        gated.continuous_canvas = true;
        gated.continuous_torch = true;
        gated.continuous_actors = false;
        gated.continuous_weather = false;
        gated.scene_at(0.06);
        let gated_world = gated
            .pending_world
            .as_ref()
            .ok_or("check: gated continuous world missing")?;
        if !gated_world.lights.is_empty() {
            return Err(
                "check: gated continuous torch path must not double-light with legacy point lights"
                    .into(),
            );
        }
        if gated_world.meshes.len() >= legacy_mesh_count {
            return Err(format!(
                "check: native torch gate retained legacy flame geometry (legacy={}, gated={})",
                legacy_mesh_count,
                gated_world.meshes.len()
            ));
        }
        let gated_camera = gated_world.camera;
        let native = gated
            .continuous_scene(0.06)
            .ok_or("check: gated continuous scene missing")?;
        let primitives = native.primitives();
        if primitives.is_empty() || primitives.len() >= MAX_NATIVE_PRIMITIVES {
            return Err(format!(
                "check: gated continuous primitive count out of range ({})",
                primitives.len()
            ));
        }
        // Fractional sub-cell mapping: the flame rows must sit on the
        // fractional projection of the pulsed anchor (render and emission
        // share the same cell-corner coordinates).
        let (tx0, tz0) = torch_spots()[0];
        let pulse0 = torch_pulse(0.06, 0);
        let anchor0 = Vec3::new(tx0 + pulse0.offset.0, 1.92, tz0 + pulse0.offset.1);
        let (flame_x, flame_y) =
            project_frac(anchor0, &gated_camera).ok_or("check: torch anchor projects offscreen")?;
        let flame_anchor = Point2::new(flame_x, flame_y + HUD_H as f32);
        // Flame rows: ASCII glyphs within the row stack above the anchor.
        let is_flame_row = |p: &&Primitive| -> bool {
            let glyph = match p.kind {
                PrimitiveKind::Glyph { glyph } => glyph,
                _ => return false,
            };
            let d = (p.transform.position.x - flame_anchor.x)
                .hypot(p.transform.position.y - flame_anchor.y);
            glyph.is_ascii_punctuation() && d <= 3.4
        };
        let flame_rows: Vec<_> = primitives.iter().filter(is_flame_row).collect();
        if flame_rows.is_empty() {
            return Err("check: gated continuous flame rows missing".into());
        }
        // ASCII contract (user verdict 2026-10-25: the flame must be plain
        // punctuation, not unicode shapes).
        if flame_rows
            .iter()
            .any(|p| !matches!(p.kind, PrimitiveKind::Glyph { glyph } if glyph.is_ascii()))
        {
            return Err("check: flame body contains non-ASCII glyphs".into());
        }
        // Row ramp contract: rows must span several cell heights above the
        // anchor (a real stack, not one row) with colours that differ from
        // base to top (the smooth ramp) and CHANGE over time (dynamic).
        let topmost = flame_rows
            .iter()
            .map(|p| flame_anchor.y - p.transform.position.y)
            .fold(0.0_f32, f32::max);
        if topmost < 2.0 {
            return Err(format!(
                "check: flame stack is only {topmost:.2} rows tall (need a real taper)"
            ));
        }
        let ramp_spread = {
            let colors: Vec<_> = flame_rows.iter().map(|p| p.material.foreground).collect();
            colors
                .iter()
                .flat_map(|a| {
                    colors.iter().map(move |b| {
                        (i16::from(a.r) - i16::from(b.r)).abs().max(
                            (i16::from(a.g) - i16::from(b.g))
                                .abs()
                                .max((i16::from(a.b) - i16::from(b.b)).abs()),
                        )
                    })
                })
                .fold(0_i16, i16::max)
        };
        if ramp_spread < 80 {
            return Err(format!(
                "check: flame rows show no vivid colour ramp (spread {ramp_spread})"
            ));
        }
        // Brightness + body contract (user verdicts 2026-10-25): the lower
        // rows must be emissive at showcase calibration combined strength —
        // not a single weak dot leaning on crackle/frame-glow sources.
        let emissive_layers: Vec<_> = flame_rows
            .iter()
            .filter(|p| p.material.emission.is_some())
            .collect();
        if emissive_layers.len() < 3 {
            return Err(format!(
                "check: flame body has only {} emissive rows (need the hot base rows)",
                emissive_layers.len()
            ));
        }
        let total_strength: f32 = emissive_layers
            .iter()
            .filter_map(|p| p.material.emission.map(|e| e.strength))
            .sum();
        let max_radius = emissive_layers
            .iter()
            .filter_map(|p| p.material.emission.map(|e| e.radius))
            .fold(0.0_f32, f32::max);
        if total_strength < 0.9 || max_radius < 4.5 {
            return Err(format!(
                "check: flame emission below showcase calibration (strength total {total_strength:.2}, max radius {max_radius:.2})"
            ));
        }
        // Integrated acceptance (review 2026-10-25): the flame must not
        // wash the whole scene orange. The one-torch-light layout sums to
        // ~1.8 (single coherent source + four local glows); the bound sits
        // above that so only gross regressions (e.g. per-glyph lamps
        // returning) fail.
        if total_strength > 3.0 {
            return Err(format!(
                "check: flame emission floods the scene (strength total {total_strength:.2} > 3.0)"
            ));
        }
        // Light-field profile contract (review 2026-10-25): the checks
        // above prove emission EXISTS; this one proves the field is SHAPED
        // like a torch — hot coherent core, warm mid pool, subdued outer,
        // near-black far field, warm hue order — using the CPU reference
        // field (test-pinned parity with the GPU blur pass; the GPU's two
        // blur passes spread energy but preserve this profile's shape).
        {
            use ascii_mmorpg_engine::flame::{push_ascii_flame, FlamePulse};
            use ascii_mmorpg_engine::BloomField;
            let mut flame_scene = ContinuousScene::new();
            push_ascii_flame(
                &mut flame_scene,
                Point2::new(30.0, 30.0),
                FlamePulse {
                    light_gain: 0.9,
                    flame_scale: 1.0,
                    radius_scale: 1.0,
                    sway: 0.0,
                },
                0.0,
                0.06,
            );
            let field = BloomField::from_sources(
                64,
                64,
                EmissionSource::from_continuous_scene(&flame_scene, 0.06),
            );
            let core = field.sample(30, 30);
            let pool = field.sample(32, 30);
            let outer = field.sample(34, 30);
            let far = field.sample(39, 30);
            if core.r < 0.5 {
                return Err(format!("check: torch core too dim ({core:?})"));
            }
            if !(core.r > pool.r && pool.r > outer.r) {
                return Err(format!(
                    "check: torch field not monotonic (core {:.3} > pool {:.3} > outer {:.3})",
                    core.r, pool.r, outer.r
                ));
            }
            if core.r < pool.r * 1.2 {
                return Err("check: torch core must dominate its pool".into());
            }
            if outer.r > pool.r * 0.6 {
                return Err(format!(
                    "check: outer pool too hot (outer {:.3} vs pool {:.3})",
                    outer.r, pool.r
                ));
            }
            if far.r > 0.02 {
                return Err(format!("check: far field not near-black ({far:?})"));
            }
            let warm = |v: ascii_mmorpg_engine::FloatRgb| v.r >= v.g && v.g >= v.b;
            if !warm(core) || !warm(pool) {
                return Err("check: torch pool lost its warm hue order".into());
            }
        }

        // Dynamic ramp: the row colours must shift between samples (the
        // wobble breathes the hot zone; positions sway sub-cell too).
        let native_second = gated
            .continuous_scene(0.66)
            .ok_or("check: second gated continuous scene missing")?;
        let flame_second: Vec<_> = native_second
            .primitives()
            .iter()
            .filter(is_flame_row)
            .collect();
        let colors_move = flame_rows.iter().zip(flame_second.iter()).any(|(a, b)| {
            a.material.foreground != b.material.foreground
                || (a.transform.position.x - b.transform.position.x).abs() > 1e-4
        });
        if !colors_move {
            return Err("check: flame ramp/positions do not animate between samples".into());
        }
        // Emission coupling: crackle primitives feed the float emission seam
        // at a deterministic time (opacity-scaled strength preserved).
        let emission_fed = [0.06f32, 0.24, 0.41, 0.66, 0.9, 1.3, 1.7]
            .iter()
            .any(|&sample_t| {
                gated
                    .continuous_scene(sample_t)
                    .map(|scene| {
                        !EmissionSource::from_continuous_scene(&scene, sample_t).is_empty()
                    })
                    .unwrap_or(false)
            });
        if !emission_fed {
            return Err("check: gated crackle primitives produced no emission sources".into());
        }

        // Layered-character migration contract (issue #8,
        // `--continuous-actors`): the gate drops mesh silhouettes, composes
        // native glyph character layers (torso/weapon/shadow/motes + player
        // aura), hands lighting ownership to bloom, keeps the nameplate
        // label layer composed, and never touches the authoritative
        // simulation.
        let mut actor_gate = LocalGame::with_gpu3d(true);
        actor_gate.continuous_canvas = true;
        actor_gate.continuous_actors = true;
        actor_gate.continuous_torch = false;
        actor_gate.continuous_weather = false;
        let actor_scene = actor_gate.scene_at(0.06).0;
        let (actor_lights_kept, actor_mesh_count) = actor_gate
            .pending_world
            .as_ref()
            .map(|world| (!world.lights.is_empty(), world.meshes.len()))
            .ok_or("check: layered-actor world missing")?;
        // Per-system lighting (review 2026-10-25): this configuration runs
        // the torch system legacy beside native actors, so the torch point
        // lights legitimately survive (bloom ownership is still claimed
        // for the actors — asserted in the hybrid block below).
        if !actor_lights_kept {
            return Err("check: hybrid config lost the legacy torch point lights".into());
        }
        if actor_mesh_count >= legacy_mesh_count {
            return Err(format!(
                "check: layered-actor gate retained mesh silhouettes (legacy={}, gated={})",
                legacy_mesh_count, actor_mesh_count
            ));
        }
        if !actor_gate.continuous_bloom_owns_world_lighting() {
            return Err("check: layered-actor gate must claim bloom ownership".into());
        }
        let actor_native = actor_gate
            .continuous_scene(0.06)
            .ok_or("check: layered-actor continuous scene missing")?;
        let actor_primitives = actor_native.primitives();
        if actor_primitives.is_empty() || actor_primitives.len() >= MAX_NATIVE_PRIMITIVES {
            return Err(format!(
                "check: layered-actor primitive count out of range ({})",
                actor_primitives.len()
            ));
        }
        let has_glyph = |want: char| {
            actor_primitives.iter().any(|p| match p.kind {
                PrimitiveKind::Glyph { glyph } | PrimitiveKind::Particle { glyph } => glyph == want,
                _ => false,
            })
        };
        for (want, what) in [
            ('┼', "torso"),
            ('|', "weapon"),
            ('▀', "shadow"),
            ('·', "motes"),
        ] {
            if !has_glyph(want) {
                return Err(format!("check: layered-actor {what} layer missing"));
            }
        }
        if !actor_primitives.iter().any(|p| {
            p.material
                .gradient
                .is_some_and(|g| g.axis == ascii_mmorpg_engine::GradientAxis::Radial)
        }) {
            return Err("check: layered-actor player aura missing".into());
        }
        // Nameplates remain composed in both paths (readability contract).
        let labels_composed = actor_scene
            .layers
            .iter()
            .any(|layer| layer.name == "labels");
        if !labels_composed {
            return Err("check: layered-actor path lost the nameplate label layer".into());
        }
        // Authority untouched: the same tick sequence drives both paths to
        // identical player positions.
        actor_gate.scene_at(0.12);
        legacy_canvas.scene_at(0.12);
        let gated_player = actor_gate.sim.snapshot_for(PLAYER_ID).player.position;
        let legacy_player = legacy_canvas.sim.snapshot_for(PLAYER_ID).player.position;
        if gated_player != legacy_player {
            return Err(format!(
                "check: layered-actor gate mutated authoritative positions ({gated_player:?} vs {legacy_player:?})"
            ));
        }

        // Fullscreen-native density contract: the presenter now defaults to
        // a surface-derived grid (one atlas texel per pixel), so fullscreen
        // runs at ~320x108 instead of the capped 160x56. The camera scales
        // its focal with the grid and the coverage lattices slide with the
        // focus, so world ink per cell must hold — this guards the old
        // fullscreen sparse-band regression headlessly (CPU raster, one
        // frame per size).
        let world_ink = |w: u32, h: u32| -> Result<usize, String> {
            let mut game = LocalGame::with_gpu3d(false);
            let (scene, camera) = game.scene_at_size(0.06, w, h);
            let frame = compose(&scene, &camera, 0.06);
            let rows = (HUD_H as u32..(h - LOG_H as u32)).map(|y| {
                (0..w)
                    .filter(|&x| {
                        frame
                            .get(x as i32, y as i32)
                            .is_some_and(|cell| cell.glyph != ' ')
                    })
                    .count()
            });
            Ok(rows.sum())
        };
        let authored_ink = world_ink(GRID_W, GRID_H)?;
        let fullscreen_ink = world_ink(320, 108)?;
        if authored_ink == 0 {
            return Err("check: authored world composed no ink".into());
        }
        let ratio = fullscreen_ink as f64 / authored_ink as f64;
        if ratio < 1.6 {
            return Err(format!(
                "check: fullscreen 320x108 ink ratio {ratio:.2} below 1.6 — sparse-band regression (authored={authored_ink}, fullscreen={fullscreen_ink})"
            ));
        }

        // Weather migration contract (issue #9, `--continuous-weather`):
        // the gate drops mesh grass/rain, composes world-anchored weather
        // primitives (grounded grass bases + waving tips, seeded streaks in
        // Rain mode, motes only in Clear mode), and never touches the
        // authoritative simulation.
        let mut weather_gate = LocalGame::with_gpu3d(true);
        weather_gate.continuous_canvas = true;
        weather_gate.continuous_weather = true;
        weather_gate.continuous_torch = false;
        weather_gate.continuous_actors = false;
        weather_gate.scene_at(0.06);
        let weather_world = weather_gate
            .pending_world
            .as_ref()
            .ok_or("check: weather-gate world missing")?;
        if weather_world.meshes.len() >= legacy_mesh_count {
            return Err(format!(
                "check: weather gate retained mesh grass/rain (legacy={}, gated={})",
                legacy_mesh_count,
                weather_world.meshes.len()
            ));
        }
        let weather_native = weather_gate
            .continuous_scene(0.06)
            .ok_or("check: weather-gate continuous scene missing")?;
        let weather_primitives = weather_native.primitives();
        let has_glyph = |want: char| {
            weather_primitives.iter().any(|p| match p.kind {
                PrimitiveKind::Glyph { glyph } | PrimitiveKind::Particle { glyph } => glyph == want,
                _ => false,
            })
        };
        // Default check weather is Rain: grounded bases, waving tips, and
        // streaks present; no clear-air motes.
        if !has_glyph('"') || !has_glyph('\'') {
            return Err("check: weather gate lost the grass tuft layers".into());
        }
        if !weather_primitives
            .iter()
            .any(|p| matches!(p.kind, PrimitiveKind::Streak { .. }))
        {
            return Err("check: weather gate produced no rain streaks in Rain mode".into());
        }
        if has_glyph('\u{b7}') {
            return Err("check: Rain mode must not field clear-air motes".into());
        }
        // Tips wave: the same seed's tip skew changes between samples.
        let late_weather = weather_gate
            .continuous_scene(1.1)
            .ok_or("check: weather-gate second sample missing")?;
        let tip_skew = |scene: &ContinuousScene| {
            scene
                .primitives()
                .iter()
                .filter_map(|p| match p.kind {
                    PrimitiveKind::Glyph { glyph: '\'' } => Some(p.transform.skew.x),
                    _ => None,
                })
                .collect::<Vec<f32>>()
        };
        let early_skew = tip_skew(&weather_native);
        let late_skew = tip_skew(&late_weather);
        if early_skew == late_skew {
            return Err("check: grass tips do not wave between samples".into());
        }
        // Clear mode swaps rain for motes without touching the simulation.
        let mut clear_gate = LocalGame::with_gpu3d_and_weather(true, Weather::Clear);
        clear_gate.continuous_canvas = true;
        clear_gate.continuous_weather = true;
        clear_gate.continuous_torch = false;
        clear_gate.continuous_actors = false;
        clear_gate.scene_at(0.06);
        let clear_native = clear_gate
            .continuous_scene(0.06)
            .ok_or("check: clear-weather continuous scene missing")?;
        let clear_primitives = clear_native.primitives();
        if !clear_primitives
            .iter()
            .any(|p| matches!(p.kind, PrimitiveKind::Particle { glyph: '\u{b7}' }))
        {
            return Err("check: Clear mode produced no dust motes".into());
        }
        if clear_primitives
            .iter()
            .any(|p| matches!(p.kind, PrimitiveKind::Streak { .. }))
        {
            return Err("check: Clear mode must not field rain streaks".into());
        }
        let weather_player = weather_gate.sim.snapshot_for(PLAYER_ID).player.position;
        if weather_player != legacy_player {
            return Err(format!(
                "check: weather gate mutated authoritative positions ({weather_player:?} vs {legacy_player:?})"
            ));
        }

        // Play-path integration default (issue #10): a fresh game on the
        // continuous canvas runs ALL migrations at once — bloom owns
        // world lighting, the mesh list sheds every migrated family, and
        // the native scene carries torch, character, and weather glyphs.
        let mut default_mode = LocalGame::with_gpu3d(true);
        default_mode.continuous_canvas = true;
        default_mode.scene_at(0.06);
        let (default_lights_empty, default_mesh_count) = default_mode
            .pending_world
            .as_ref()
            .map(|world| (world.lights.is_empty(), world.meshes.len()))
            .ok_or("check: default-mode world missing")?;
        if !default_lights_empty {
            return Err("check: default mode must hand world lighting to bloom".into());
        }
        if !default_mode.continuous_bloom_owns_world_lighting() {
            return Err("check: default mode must claim bloom ownership".into());
        }
        if default_mesh_count >= legacy_mesh_count {
            return Err(format!(
                "check: default mode retained legacy mesh families (legacy={}, default={})",
                legacy_mesh_count, default_mesh_count
            ));
        }
        let default_native = default_mode
            .continuous_scene(0.06)
            .ok_or("check: default-mode continuous scene missing")?;
        let default_primitives = default_native.primitives();
        let has_glyph = |want: char| {
            default_primitives.iter().any(|p| match p.kind {
                PrimitiveKind::Glyph { glyph } | PrimitiveKind::Particle { glyph } => glyph == want,
                _ => false,
            })
        };
        if !has_glyph('*') || !has_glyph('\u{253c}') || !has_glyph('"') {
            return Err(
                "check: default mode must field torch flame, character torso, and grass glyphs"
                    .into(),
            );
        }
        if !default_primitives
            .iter()
            .any(|p| matches!(p.kind, PrimitiveKind::Streak { .. }))
        {
            return Err("check: default mode must field rain streaks (default weather)".into());
        }

        // Per-system fallback lighting (review 2026-10-25): with the torch
        // system legacy while actors run native, the torch point lights
        // must survive (the fallback stays visually equivalent) while the
        // actors keep bloom ownership — and no configuration double-lights.
        let mut hybrid = LocalGame::with_gpu3d(true);
        hybrid.continuous_canvas = true;
        hybrid.continuous_torch = false;
        hybrid.continuous_actors = true;
        hybrid.continuous_weather = false;
        hybrid.scene_at(0.06);
        let hybrid_lights_kept = hybrid
            .pending_world
            .as_ref()
            .is_some_and(|world| !world.lights.is_empty());
        if !hybrid_lights_kept {
            return Err("check: legacy torch must keep its point lights in a hybrid config".into());
        }
        if !hybrid.continuous_bloom_owns_world_lighting() {
            return Err("check: native actors must still claim bloom ownership".into());
        }

        // World-map modal (issues #7/#10): `M` swaps the frame to captions,
        // the continuous scene becomes the Veyrath world (faction
        // settlements + starter ring), world meshes stop building, and
        // gameplay input freezes while the server keeps ticking.
        let mut map_game = LocalGame::with_gpu3d(true);
        map_game.continuous_canvas = true;
        map_game.map_open = true;
        let map_frame = map_game.scene_at(0.06).0;
        if map_frame
            .layers
            .iter()
            .any(|layer| layer.name == "labels" || layer.name == "mesh3d")
        {
            return Err("check: map frame must not compose world layers".into());
        }
        if map_game.pending_world.is_some() {
            return Err("check: map frame must not build world meshes".into());
        }
        let map_native = map_game
            .continuous_scene(0.06)
            .ok_or("check: map continuous scene missing")?;
        let map_primitives = map_native.primitives();
        if !map_primitives.iter().any(|p| match p.kind {
            PrimitiveKind::Glyph { glyph } => glyph == '@',
            _ => false,
        }) {
            return Err("check: map must show faction settlements (capital mark)".into());
        }
        if !map_primitives.iter().any(|p| p.transform.depth == -0.5) {
            return Err("check: map must show the Millhaven starter ring".into());
        }
        // Video mimicry S1: the map caption sheet runs at a coarser cell
        // pitch than the world (reference inventory ~1.25x bigger glyphs),
        // and its authored rect must still cover the frame when scaled
        // back up at emission.
        let map_ui = map_frame
            .partitions
            .iter()
            .find(|p| p.name == "map-ui")
            .ok_or("check: map-ui partition missing")?;
        if map_ui.pitch_scale <= 1.0 {
            return Err("check: map modal must run at a coarser cell pitch (S1)".into());
        }
        if GRID_W as f32 / map_ui.pitch_scale - map_ui.w as f32 >= 1.0 {
            return Err("check: coarse map rect must still cover the frame at pitch".into());
        }
        map_game.held.insert(KeyCode::KeyW);
        if map_game.movement_axes() != (0.0, 0.0) {
            return Err("check: movement must freeze while the map is open".into());
        }
        let pending_before = map_game.pending.len();
        map_game.key(KeyCode::KeyF, true);
        if map_game.pending.len() != pending_before {
            return Err("check: attack must freeze while the map is open".into());
        }
        map_game.key(KeyCode::KeyM, true);
        if map_game.map_open {
            return Err("check: M must close the map".into());
        }
        if map_game.movement_axes() == (0.0, 0.0) {
            return Err("check: movement must resume after closing the map".into());
        }

        // Fixed-control path: D always sends the world-east intent regardless
        // of presentation input, and the fixed camera never exposes a pointer
        // mutation. (CPU raster, as above: headless check.)
        let mut camera_game = LocalGame::with_gpu3d(true);
        camera_game.pointer(PointerEvent {
            position: Some((640.0, 320.0)),
            surface_size: (1280, 640),
            held: [false, true],
            pressed: [false, false],
            released: [false, false],
            delta: (480.0, -180.0),
            wheel: 6.0,
        });
        camera_game.scene_at(0.0);
        let first_camera = camera_game
            .pending_world
            .as_ref()
            .map(|world| world.camera)
            .ok_or("check: fixed camera world missing")?;
        camera_game.scene_at(0.05);
        let second_camera = camera_game
            .pending_world
            .as_ref()
            .map(|world| world.camera)
            .ok_or("check: fixed camera world missing after pointer input")?;
        if (first_camera.yaw - CAM_YAW).abs() > 1e-6
            || (first_camera.pitch - CAM_PITCH).abs() > 1e-6
            || (first_camera.focal - CAM_FOCAL).abs() > 1e-6
            || (second_camera.pos.x - first_camera.pos.x).abs() > 1e-6
            || (second_camera.pos.y - first_camera.pos.y).abs() > 1e-6
            || (second_camera.pos.z - first_camera.pos.z).abs() > 1e-6
            || (second_camera.yaw - first_camera.yaw).abs() > 1e-6
            || (second_camera.pitch - first_camera.pitch).abs() > 1e-6
            || (second_camera.focal - first_camera.focal).abs() > 1e-6
        {
            return Err("check: pointer input changed the fixed camera".into());
        }

        let click_path = path_to_target(Point { x: 2, y: 2 }, Point { x: 5, y: 2 })
            .ok_or("check: ground click produced no walkable path")?;
        if click_path.last().map(|(_, point)| *point) != Some(Point { x: 5, y: 2 }) {
            return Err("check: click path did not end at its ground target".into());
        }
        if path_to_target(Point { x: 2, y: 2 }, Point { x: -8, y: -5 }).is_some() {
            return Err("check: click path accepted an obstructed ground tile".into());
        }

        let mut torch_game = LocalGame::with_gpu3d(true);
        let mut crackle_frame_found = false;
        for frame in 0..32 {
            torch_game.scene_at(frame as f32 * 0.05);
            let world = torch_game
                .pending_world
                .as_ref()
                .ok_or("check: torch GPU world missing")?;
            if world.lights.len() > 3 {
                crackle_frame_found = true;
                break;
            }
        }
        if !crackle_frame_found {
            return Err("check: torch crackle lights never became active".into());
        }

        let clear_profile = weather_profile(Weather::Clear, 0.75);
        let rain_profile = weather_profile(Weather::Rain, 0.75);
        let storm_profile = weather_profile(Weather::Storm, 0.75);
        if clear_profile.rain_density != 0
            || storm_profile.rain_density <= rain_profile.rain_density
            || storm_profile.grass_amplitude <= rain_profile.grass_amplitude
        {
            return Err("check: weather profiles do not increase storm motion/density".into());
        }
        let mut clear_game = LocalGame::with_gpu3d_and_weather(true, Weather::Clear);
        clear_game.scene_at(0.75);
        let clear_meshes = clear_game
            .pending_world
            .as_ref()
            .ok_or("check: clear-weather GPU world missing")?
            .meshes
            .len();
        let mut storm_game = LocalGame::with_gpu3d_and_weather(true, Weather::Storm);
        storm_game.scene_at(0.75);
        let storm_meshes = storm_game
            .pending_world
            .as_ref()
            .ok_or("check: storm-weather GPU world missing")?
            .meshes
            .len();
        if storm_meshes <= clear_meshes {
            return Err(format!(
                "check: storm weather did not add rain geometry (clear={clear_meshes}, storm={storm_meshes})"
            ));
        }

        let mut game = LocalGame::with_gpu3d(false);
        game.key(KeyCode::KeyD, true);
        game.scene_at(0.0);
        for frame in 1..=40 {
            game.scene_at(frame as f32 * 0.05);
        }
        let x = game.sim.snapshot_for(PLAYER_ID).player.position.x;
        if x <= 2 {
            return Err(format!("check: held D did not move the player (x={x})"));
        }

        // Inventory modal toggles a fifth partition on and back off (the
        // labels partition split from world made four the closed-modal
        // baseline).
        game.key(KeyCode::KeyI, true);
        let (scene, _) = game.scene_at(2.05);
        if scene.partitions.len() != 5 {
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
        if scene.partitions.len() != 4 {
            return Err("check: inventory modal did not close".into());
        }
        Ok(())
    }

    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|arg| arg == "--check") {
        run_check()?;
        eprintln!(
            "play check ok: 3D world, FX digits, fixed camera/controls, inventory modal, play-path integration: default continuous renderer + legacy fallbacks + world map"
        );
        return Ok(());
    }
    let frames = args
        .iter()
        .position(|arg| arg == "--frames")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse::<u64>().ok());
    let mode = if args.iter().any(|arg| arg == "--grid") {
        PresentMode::GridFallback
    } else {
        PresentMode::Continuous
    };
    // Fullscreen-native by default: zero explicit cells open a borderless
    // fullscreen window whose logical grid derives from the surface at
    // exactly one atlas texel per pixel — edge-to-edge fill with zero glyph
    // resampling (the authored-grid cap stretches viewport instead, which
    // reads soft; see `fitted_viewport`). `--window` keeps the authored
    // 160x56 comparison window with its capped grid. The camera scales its
    // focal with the grid (clamped), so world ink density per cell — and
    // the sliding coverage margins — hold at fullscreen sizes; the
    // `--check` density contract guards that.
    let windowed = args.iter().any(|arg| arg == "--window");

    match ascii_mmorpg_engine::present::run_dynamic(
        LocalGame::new(),
        PresentOptions {
            // GPU raster is the default; `--cpu` forces the CPU fill.
            title: "ASCII MMORPG — fixed oblique 2.5D camera".into(),
            cell_px: 10.0,
            width_cells: if windowed { GRID_W } else { 0 },
            height_cells: if windowed { GRID_H } else { 0 },
            max_frames: frames,
            mode,
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
