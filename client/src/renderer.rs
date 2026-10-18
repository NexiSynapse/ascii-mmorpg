//! A small terminal-native scene renderer.
//!
//! This is intentionally a client-only visual layer. It consumes authoritative
//! snapshots, derives animation from the snapshot tick, and never changes
//! gameplay state or sends commands to the server.

use ascii_mmorpg_shared::{ActorSnapshot, EntityKind, WorldSnapshot};

use crate::assets::{AssetLibrary, PropKind};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Ink {
    Background,
    Stone,
    StoneDark,
    StoneLight,
    Moss,
    Foliage,
    Rain,
    RainBright,
    Player,
    Monster,
    Npc,
    Item,
    Spell,
    #[allow(dead_code)]
    Torch,
    Text,
    Muted,
    // Vibrant Dark MMORPG & 3D Materials
    KnightSteel,
    KnightGold,
    KnightCape,
    KnightVisor,
    DropShadow,
    CobbleRoad,
    CobbleGlow,
    CobbleBorder,
    GrassMeadow,
    GrassDark,
    GrassTorch,
    PineTree,
    PineCanopy,
    TreeShadow,
    TorchFlame,
    TorchGlow,
    Lightning,
    LightningWhite,
    BannerRed,
    WoodPost,
    FloatingDamage,
    // Monster depth palette
    MossBody,
    MossEye,
    MossHide,
    // Skeleton bone palette (pale undead, reference bone-white)
    BoneLight,
    Bone,
    BoneDark,
    BoneEye,
    // NPC depth palette
    NpcRobe,
    NpcSkin,
    // Ruin depth palette
    RuinWall,
    RuinWindow,
    // Atmospheric Weather Shader Tokens
    Snow,
    SnowGlow,
    SnowSoft,
    Splash,
    // Living Water Surface & Bridge Palette
    WaterDeep,
    WaterShallow,
    WaterRipple,
    WaterShimmer,
    BridgeWood,
}

impl Ink {
    /// TrueColor table for composed-frame blitting.
    /// Former `38;5;N` palette entries use standard xterm RGB approximations.
    const fn rgb(self) -> (u8, u8, u8) {
        match self {
            Self::Background => (12, 15, 22),
            Self::Stone => (135, 135, 95),
            Self::StoneDark => (65, 60, 55),
            Self::StoneLight => (175, 165, 150),
            Self::Moss => (95, 135, 95),
            Self::Foliage => (0, 95, 0),
            Self::Rain => (95, 95, 135),
            Self::RainBright => (135, 215, 215),
            Self::Player => (135, 215, 255),
            Self::Monster => (255, 95, 95),
            Self::Npc => (255, 215, 95),
            Self::Item => (255, 215, 0),
            Self::Spell => (175, 215, 255),
            Self::Torch => (255, 215, 0),
            Self::Text => (208, 208, 208),
            Self::Muted => (138, 138, 138),
            Self::KnightSteel => (225, 232, 240),
            Self::KnightGold => (255, 210, 75),
            Self::KnightCape => (195, 35, 48),
            Self::KnightVisor => (140, 180, 220),
            Self::DropShadow => (16, 20, 28),
            Self::CobbleRoad => (125, 115, 105),
            Self::CobbleGlow => (200, 160, 95),
            Self::CobbleBorder => (65, 60, 55),
            Self::GrassMeadow => (32, 75, 55),
            Self::GrassDark => (22, 50, 38),
            Self::GrassTorch => (85, 155, 115),
            Self::PineTree => (35, 80, 65),
            Self::PineCanopy => (25, 60, 48),
            Self::TreeShadow => (12, 20, 16),
            Self::TorchFlame => (255, 95, 25),
            Self::TorchGlow => (200, 145, 45),
            Self::Lightning => (76, 201, 240),
            Self::LightningWhite => (255, 255, 255),
            Self::BannerRed => (165, 25, 38),
            Self::WoodPost => (135, 85, 50),
            Self::FloatingDamage => (255, 220, 50),
            Self::MossBody => (88, 130, 72),
            Self::MossEye => (255, 200, 60),
            Self::MossHide => (55, 85, 48),
            Self::BoneLight => (216, 212, 200),
            Self::Bone => (168, 164, 150),
            Self::BoneDark => (110, 106, 96),
            Self::BoneEye => (255, 233, 168),
            Self::NpcRobe => (160, 140, 100),
            Self::NpcSkin => (220, 185, 150),
            Self::RuinWall => (110, 100, 88),
            Self::RuinWindow => (45, 55, 80),
            Self::Snow => (230, 245, 255),
            Self::SnowGlow => (255, 255, 255),
            Self::SnowSoft => (160, 195, 225),
            Self::Splash => (180, 225, 255),
            Self::WaterDeep => (24, 62, 120),
            Self::WaterShallow => (48, 128, 185),
            Self::WaterRipple => (85, 188, 225),
            Self::WaterShimmer => (215, 242, 255),
            Self::BridgeWood => (130, 85, 48),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Cell {
    glyph: char,
    ink: Ink,
}

impl Cell {
    const fn new(glyph: char, ink: Ink) -> Self {
        Self { glyph, ink }
    }
}

struct Canvas {
    width: usize,
    height: usize,
    cells: Vec<Cell>,
}

/// A point light source in world space.
struct Light {
    x: i16,
    y: i16,
    radius: i16,
}

impl Canvas {
    fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            cells: vec![Cell::new(' ', Ink::Background); width * height],
        }
    }

    fn set(&mut self, x: isize, y: isize, glyph: char, ink: Ink) {
        if x < 0 || y < 0 || x as usize >= self.width || y as usize >= self.height {
            return;
        }
        self.cells[y as usize * self.width + x as usize] = Cell::new(glyph, ink);
    }

    fn get(&self, x: isize, y: isize) -> Cell {
        if x < 0 || y < 0 || x as usize >= self.width || y as usize >= self.height {
            Cell::new(' ', Ink::Background)
        } else {
            self.cells[y as usize * self.width + x as usize]
        }
    }

    fn text(&mut self, x: isize, y: isize, text: &str, ink: Ink) {
        for (offset, glyph) in text.chars().enumerate() {
            self.set(x + offset as isize, y, glyph, ink);
        }
    }

    /// Lowers the canvas to shared scene cells for composed-frame blitting.
    /// Background stays the terminal default (`bg: None`).
    fn scene_cells(&self) -> Vec<crate::partitions::SceneCell> {
        self.cells
            .iter()
            .map(|cell| crate::partitions::SceneCell {
                glyph: cell.glyph,
                fg: cell.ink.rgb(),
                bg: None,
            })
            .collect()
    }
}

/// Weather simulation modes for the dedicated post-processing overlay shader.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum WeatherMode {
    #[default]
    Rain,
    Snow,
    Storm,
}

/// Procedural ANSI renderer for the alpha client.
///
/// `frame` is separate from the authoritative tick so a replay or demo can
/// render several animation frames from the same server snapshot. The regular
/// client calls [`Self::render`], which uses the snapshot tick.
pub struct Renderer {
    pub width: i16,
    pub height: i16,
    pub weather: WeatherMode,
    pub active_modal: crate::ui::ActiveModal,
    pub last_player_pos: std::cell::Cell<Option<ascii_mmorpg_shared::Point>>,
    pub last_move_instant: std::cell::Cell<Option<std::time::Instant>>,
}

impl Default for Renderer {
    fn default() -> Self {
        Self::new(80, 24)
    }
}

impl Renderer {
    pub fn new(width: i16, height: i16) -> Self {
        Self {
            width: width.max(1),
            height: height.max(1),
            weather: WeatherMode::Rain,
            active_modal: crate::ui::ActiveModal::None,
            last_player_pos: std::cell::Cell::new(None),
            last_move_instant: std::cell::Cell::new(None),
        }
    }

    pub fn with_weather(mut self, weather: WeatherMode) -> Self {
        self.weather = weather;
        self
    }

    pub fn weather(&self) -> WeatherMode {
        self.weather
    }

    pub fn set_weather(&mut self, weather: WeatherMode) {
        self.weather = weather;
    }

    pub fn active_modal(&self) -> crate::ui::ActiveModal {
        self.active_modal
    }

    pub fn set_active_modal(&mut self, modal: crate::ui::ActiveModal) {
        self.active_modal = modal;
    }

    pub fn resize(&mut self, width: i16, height: i16) {
        self.width = width.max(1);
        self.height = height.max(1);
    }

    /// Evaluates whether the player is currently walking based on recent position changes
    /// within a 280ms stride window, returning true if in active motion.
    /// When the player is stationary (idle), returns false so feet remain firmly planted on the ground.
    pub fn is_player_walking(&self, world: &WorldSnapshot) -> bool {
        if !world.player.stats.is_alive() {
            return false;
        }

        let current_pos = world.player.position;
        let now = std::time::Instant::now();
        let last_pos = self.last_player_pos.get();

        let moved = match last_pos {
            Some(last) => last != current_pos,
            None => false,
        };

        if moved {
            self.last_player_pos.set(Some(current_pos));
            self.last_move_instant.set(Some(now));
        } else if last_pos.is_none() {
            self.last_player_pos.set(Some(current_pos));
        }

        self.last_move_instant
            .get()
            .is_some_and(|t| now.duration_since(t).as_millis() < 280)
    }

    /// Overrides movement state for unit tests or deterministic replay.
    pub fn set_player_walking_for_test(&self, walking: bool) {
        if walking {
            self.last_move_instant.set(Some(std::time::Instant::now()));
        } else {
            self.last_move_instant.set(None);
        }
    }

    pub fn render(&self, world: &WorldSnapshot) -> String {
        self.render_frame(world, world.tick)
    }

    pub fn render_frame(&self, world: &WorldSnapshot, frame: u64) -> String {
        // Composed frame: the scene blits into one shared Ratatui buffer with
        // the HUD top bar, side panel, log strip, legend, and any modal
        // overlay (real occlusion via `Clear`), so the output always fits the
        // terminal exactly and dirty-line diffing never sees a line-count
        // change mid-session.
        let canvas = self.render_canvas(world, frame);
        let scene = canvas.scene_cells();
        let mut hud = crate::ui::RatatuiHud::new();
        hud.render_composed_frame(crate::ui::ComposedScene {
            world,
            width: self.width.max(20) as u16,
            height: self.height.max(10) as u16,
            weather: self.weather,
            backend_name: "CPU PROCEDURAL",
            modal: self.active_modal,
            cells: &scene,
            cells_width: self.width.max(1) as usize,
            labels: &[],
        })
    }

    /// Full scene pipeline without the text footer, so tests can inspect cells.
    fn render_canvas(&self, world: &WorldSnapshot, frame: u64) -> Canvas {
        let width = self.width.max(1) as usize;
        let height = self.height.max(1) as usize;
        let mut canvas = Canvas::new(width, height);
        let assets = AssetLibrary::new();
        self.draw_backdrop(
            &mut canvas,
            &assets,
            world.player.position,
            &world.entities,
            frame,
        );

        let actors = world
            .entities
            .iter()
            .chain(std::iter::once(&world.player))
            .collect::<Vec<_>>();
        self.draw_effects(&mut canvas, world, &actors, frame);
        // Stagger nameplate rows so overlapping actors keep readable labels:
        // each actor claims the nearest free (label, bar) row pair.
        let mut claimed: Vec<isize> = Vec::new();
        let mut label_rows: Vec<isize> = Vec::new();
        for actor in &actors {
            let (_, y) = self.screen_position(actor.position, world.player.position);
            let mut offset = 0isize;
            let row = loop {
                let candidate = (y - 4)
                    + if offset == 0 {
                        0
                    } else if offset % 2 == 1 {
                        -(offset + 1) / 2
                    } else {
                        offset / 2
                    };
                if !claimed.contains(&candidate) && !claimed.contains(&(candidate + 1)) {
                    break candidate;
                }
                offset += 1;
            };
            claimed.push(row);
            claimed.push(row + 1);
            label_rows.push(row);
        }
        let is_walking = self.is_player_walking(world);
        for (actor, label_row) in actors.iter().zip(label_rows) {
            let actor_frame = if actor.kind == EntityKind::Player {
                if is_walking {
                    frame
                } else {
                    0 // Stationary idle posture: feet firmly planted side-by-side
                }
            } else {
                frame
            };
            self.draw_actor(
                &mut canvas,
                &assets,
                actor,
                world.player.position,
                label_row,
                actor_frame,
            );
            if actor.kind == EntityKind::Monster && actor.stats.is_alive() {
                let (x, y) = self.screen_position(actor.position, world.player.position);
                canvas.set(x, y + 2, '+', Ink::Spell);
            }
        }

        // Aerial perspective: sink far terrain toward the void before the
        // weather pass, so rain stays bright foreground over dim background.
        self.apply_depth_veil(&mut canvas);

        // Apply Weather Shader as a dedicated top-level post-processing overlay pass
        self.apply_weather_shader(&mut canvas, world.player.position, frame);
        canvas
    }

    /// All light sources in world space: player-carried lantern follows camera,
    /// fixed lanterns, and dynamic ground lighting pools around living actors and loot.
    fn lights(
        camera: ascii_mmorpg_shared::Point,
        entities: &[ascii_mmorpg_shared::ActorSnapshot],
    ) -> Vec<Light> {
        let mut lights = vec![
            Light {
                x: camera.x,
                y: camera.y,
                radius: 7,
            },
            Light {
                x: -6,
                y: 2,
                radius: 6,
            },
            Light {
                x: 6,
                y: -4,
                radius: 6,
            },
        ];
        for entity in entities {
            match entity.kind {
                EntityKind::Npc => lights.push(Light {
                    x: entity.position.x,
                    y: entity.position.y,
                    radius: 6,
                }),
                EntityKind::Monster if entity.stats.is_alive() => lights.push(Light {
                    x: entity.position.x,
                    y: entity.position.y,
                    radius: 5,
                }),
                EntityKind::Item => lights.push(Light {
                    x: entity.position.x,
                    y: entity.position.y,
                    radius: 4,
                }),
                EntityKind::Player if entity.position != camera => lights.push(Light {
                    x: entity.position.x,
                    y: entity.position.y,
                    radius: 7,
                }),
                _ => {}
            }
        }
        lights
    }

    /// 0.0 (void) .. 1.0 (torch core): ambient moonlight plus radial falloff
    /// with a slight eastward directional bias so lit faces read brighter.
    /// Quantized to 0.05 steps so camera-relative wobble can never flip a
    /// ground band between adjacent camera positions (anti-shimmer).
    pub fn light_level(wx: i16, wy: i16, camera: ascii_mmorpg_shared::Point) -> f32 {
        Self::light_level_with_lights(wx, wy, camera, &Self::lights(camera, &[]))
    }

    fn light_level_with_lights(
        wx: i16,
        wy: i16,
        camera: ascii_mmorpg_shared::Point,
        lights: &[Light],
    ) -> f32 {
        let mut level = 0.10;
        for light in lights {
            let dist = (wx - light.x).abs() as f32 + (wy - light.y).abs() as f32;
            level += (1.0 - dist / light.radius as f32).max(0.0) * 0.9;
        }
        level += (wx - camera.x).clamp(-8, 8) as f32 * 0.012;
        (level.clamp(0.0, 1.0) * 20.0).round() / 20.0
    }

    fn draw_backdrop(
        &self,
        canvas: &mut Canvas,
        assets: &AssetLibrary,
        camera: ascii_mmorpg_shared::Point,
        entities: &[ascii_mmorpg_shared::ActorSnapshot],
        frame: u64,
    ) {
        let lights = Self::lights(camera, entities);
        for y in 0..canvas.height {
            for x in 0..canvas.width {
                let wx = camera.x + x as i16 - canvas.width as i16 / 2;
                let wy = camera.y + y as i16 - canvas.height as i16 / 2;
                let patch = (wx * 17 + wy * 31).rem_euclid(23);
                let path = (wx - wy / 3).rem_euclid(13) <= 2;
                let level = Self::light_level_with_lights(wx, wy, camera, &lights);
                let river_center = 8 + (wx / 5);
                let river_dist = (wy - river_center).abs();
                let is_river = river_dist <= 1;
                let is_bridge = is_river && path;

                let (glyph, ink) = if is_bridge {
                    if river_dist == 0 {
                        ('=', Ink::BridgeWood)
                    } else {
                        ('#', Ink::BridgeWood)
                    }
                } else if is_river {
                    let ripple_phase = (wx.wrapping_mul(7) as i32
                        + wy.wrapping_mul(11) as i32
                        + (frame / 3) as i32)
                        .rem_euclid(10);
                    if level > 0.6 {
                        match ripple_phase {
                            0 => ('~', Ink::TorchGlow),
                            1 | 2 => ('≈', Ink::WaterShimmer),
                            3 | 4 => ('~', Ink::WaterShallow),
                            _ => ('-', Ink::WaterDeep),
                        }
                    } else {
                        match ripple_phase {
                            0 => ('~', Ink::WaterShimmer),
                            1 | 2 => ('≈', Ink::WaterRipple),
                            3 | 4 => ('~', Ink::WaterShallow),
                            5 | 6 => ('-', Ink::WaterDeep),
                            _ => ('·', Ink::WaterDeep),
                        }
                    }
                } else if river_dist == 2 && !path && level <= 0.55 {
                    match patch % 4 {
                        0 => ('%', Ink::Moss),
                        1 => (',', Ink::GrassMeadow),
                        2 => ('~', Ink::WaterDeep),
                        _ => ('.', Ink::GrassDark),
                    }
                } else if level > 0.8 {
                    // Blazing torch/lantern core
                    match patch % 3 {
                        0 => ('*', Ink::TorchFlame),
                        1 => ('^', Ink::TorchGlow),
                        _ => ('.', Ink::CobbleGlow),
                    }
                } else if level > 0.55 {
                    // Radial warm amber illumination on road & grass
                    if path {
                        match patch % 4 {
                            0 => ('o', Ink::CobbleGlow),
                            1 => ('·', Ink::CobbleGlow),
                            2 => ('Q', Ink::CobbleGlow),
                            _ => ('.', Ink::CobbleGlow),
                        }
                    } else {
                        match patch % 4 {
                            0 => ('"', Ink::GrassTorch),
                            1 => ('\'', Ink::GrassTorch),
                            2 => ('·', Ink::GrassTorch),
                            _ => ('.', Ink::GrassMeadow),
                        }
                    }
                } else if path && level > 0.35 {
                    // Ancient cobblestone pathway matching the dark fantasy reference
                    let road_col = (wx - wy / 3).rem_euclid(13);
                    if road_col == 0 || road_col == 2 {
                        (if road_col == 0 { '/' } else { '\\' }, Ink::CobbleBorder)
                    } else {
                        match patch % 6 {
                            0 => ('o', Ink::CobbleRoad),
                            1 => ('·', Ink::CobbleBorder),
                            2 => ('Q', Ink::CobbleBorder),
                            3 => ('.', Ink::CobbleBorder),
                            _ => (' ', Ink::Background),
                        }
                    }
                } else if level > 0.30 {
                    // Ambient twilight meadow with calm subtle grass blades
                    match patch % 6 {
                        0 => (',', Ink::GrassDark),
                        1 => ('·', Ink::GrassDark),
                        2 => ('.', Ink::GrassDark),
                        3 => ('"', Ink::GrassMeadow),
                        _ => (' ', Ink::Background),
                    }
                } else {
                    Self::surface_cell(wx, wy, camera, patch)
                };
                canvas.set(x as isize, y as isize, glyph, ink);
            }
        }

        // 3D Pine Trees and Ancient Stone Ruins scattered on a fixed world
        // grid: deterministic per world cell, so props never swim or pop as
        // the camera moves. The gate below stays a fixed landmark.
        let half_w = canvas.width as i16 / 2 + 12;
        let half_h = canvas.height as i16 / 2 + 12;
        for gx in (camera.x - half_w).div_euclid(11)..=(camera.x + half_w).div_euclid(11) {
            for gy in (camera.y - half_h).div_euclid(9)..=(camera.y + half_h).div_euclid(9) {
                let seed = Self::terrain_seed(gx, gy);
                let wx = gx * 11 + (seed % 5) as i16;
                let wy = gy * 9 + ((seed / 7) % 5) as i16;
                if seed % 9 == 0 {
                    let screen =
                        self.screen_position(ascii_mmorpg_shared::Point { x: wx, y: wy }, camera);
                    canvas.text(screen.0 - 4, screen.1 + 4, "( _____ )", Ink::TreeShadow);
                    self.draw_tree_prop(
                        canvas,
                        assets.prop(PropKind::Tree, frame / 16),
                        screen.0 - 5,
                        screen.1 - 4,
                    );
                } else if seed % 23 == 1 {
                    let screen =
                        self.screen_position(ascii_mmorpg_shared::Point { x: wx, y: wy }, camera);
                    canvas.text(screen.0 - 4, screen.1 + 3, "( _____ )", Ink::TreeShadow);
                    self.draw_ruin_prop(
                        canvas,
                        assets.prop(PropKind::Ruin, frame / 32),
                        screen.0 - 6,
                        screen.1 - 3,
                    );
                }
            }
        }

        // Medieval Stone Gate
        let gate = self.screen_position(ascii_mmorpg_shared::Point { x: 2, y: -8 }, camera);
        self.draw_gate_prop(
            canvas,
            assets.prop(PropKind::Gate, 0),
            gate.0 - 5,
            gate.1 - 5,
        );

        // Hanging Crimson War Banners
        for (x, y) in [(-8, 3), (8, -3)] {
            let screen = self.screen_position(ascii_mmorpg_shared::Point { x, y }, camera);
            canvas.set(screen.0, screen.1, '|', Ink::WoodPost);
            canvas.set(screen.0, screen.1 + 1, '▓', Ink::BannerRed);
            canvas.set(screen.0, screen.1 + 2, '▼', Ink::BannerRed);
        }

        // Calm, sparse wild grass tufts
        for (x, y) in [(-14, 4), (-11, 5), (4, 10), (11, 3), (17, 6)] {
            let screen = self.screen_position(ascii_mmorpg_shared::Point { x, y }, camera);
            canvas.set(screen.0, screen.1, '·', Ink::GrassDark);
            canvas.set(screen.0 - 1, screen.1 + 1, '/', Ink::GrassDark);
            canvas.set(screen.0 + 1, screen.1 + 1, '\\', Ink::GrassDark);
            canvas.set(screen.0, screen.1 + 1, '"', Ink::Moss);
        }

        // Timber Lantern Posts with radiating amber light
        for (x, y) in [(-6, 2), (6, -4)] {
            let screen = self.screen_position(ascii_mmorpg_shared::Point { x, y }, camera);
            canvas.set(screen.0, screen.1, '*', Ink::TorchFlame);
            canvas.set(screen.0 - 1, screen.1, 'r', Ink::WoodPost);
            canvas.set(screen.0 - 1, screen.1 + 1, '|', Ink::WoodPost);
            canvas.set(screen.0 - 1, screen.1 + 2, '|', Ink::WoodPost);
            canvas.set(screen.0 + 1, screen.1, '·', Ink::TorchGlow);
            canvas.set(screen.0, screen.1 + 1, '·', Ink::TorchGlow);
        }

        canvas.text(2, 1, "+", Ink::Muted);
    }

    /// Inks the weather and dynamic water surface overlay is allowed to paint.
    /// Everything else counts as stable scene content for the shimmer metric.
    #[cfg(test)]
    fn is_weather_ink(ink: Ink) -> bool {
        matches!(
            ink,
            Ink::Rain
                | Ink::RainBright
                | Ink::Snow
                | Ink::SnowGlow
                | Ink::SnowSoft
                | Ink::Splash
                | Ink::LightningWhite
                | Ink::WaterShimmer
                | Ink::WaterRipple
                | Ink::WaterShallow
                | Ink::WaterDeep
        )
    }

    /// One aerial-perspective step: terrain and prop inks sink toward the
    /// void with distance. Actor, light, weather, and UI inks are never
    /// dimmed, so the foreground always pops against the background.
    fn dim_ink(ink: Ink) -> Ink {
        match ink {
            Ink::StoneLight => Ink::Stone,
            Ink::Stone => Ink::StoneDark,
            Ink::StoneDark => Ink::Background,
            Ink::RuinWall => Ink::StoneDark,
            Ink::RuinWindow => Ink::Background,
            Ink::Moss => Ink::GrassDark,
            Ink::GrassMeadow => Ink::GrassDark,
            Ink::GrassTorch => Ink::GrassMeadow,
            Ink::GrassDark => Ink::Background,
            Ink::Foliage => Ink::PineCanopy,
            Ink::PineTree => Ink::PineCanopy,
            Ink::PineCanopy => Ink::Background,
            Ink::CobbleGlow => Ink::CobbleRoad,
            Ink::CobbleRoad => Ink::CobbleBorder,
            Ink::CobbleBorder => Ink::StoneDark,
            Ink::WaterShimmer => Ink::WaterRipple,
            Ink::WaterRipple => Ink::WaterShallow,
            Ink::WaterShallow => Ink::WaterDeep,
            Ink::WaterDeep => Ink::Background,
            Ink::BridgeWood => Ink::WoodPost,
            other => other,
        }
    }

    /// Aerial perspective over terrain and props: cells far from the player
    /// sink toward the void while actors, lights, weather, and UI keep full
    /// brightness, so foreground and background channels are trivially
    /// distinguishable. Depth is player-relative by construction (the camera
    /// always centers the player), so no camera argument is needed.
    fn apply_depth_veil(&self, canvas: &mut Canvas) {
        let half_w = canvas.width as isize / 2;
        let half_h = canvas.height as isize / 2;
        for y in 0..canvas.height as isize {
            // Inverse of the oblique projection (fuzzy at odd rows, which is
            // fine for a soft depth boundary).
            let dy = 2 * (y - half_h);
            for x in 0..canvas.width as isize {
                let dx = (x - half_w) - dy / 2;
                let depth = dx.abs().max(dy.abs());
                if depth > 16 {
                    let cell = canvas.get(x, y);
                    let dimmed = Self::dim_ink(cell.ink);
                    let dimmed = if depth > 28 {
                        Self::dim_ink(dimmed)
                    } else {
                        dimmed
                    };
                    if dimmed != cell.ink {
                        canvas.set(x, y, cell.glyph, dimmed);
                    }
                }
            }
        }
    }

    fn is_ui_or_anchor_cell(cell: Cell) -> bool {
        cell.ink == Ink::Text
            || cell.glyph == '@'
            || cell.glyph == 'm'
            || cell.glyph == '['
            || cell.glyph == ']'
            || cell.glyph == '#'
            || cell.glyph == '-'
            || (cell.glyph == '+' && cell.ink == Ink::Spell)
    }

    fn apply_weather_shader(
        &self,
        canvas: &mut Canvas,
        camera: ascii_mmorpg_shared::Point,
        frame: u64,
    ) {
        match self.weather {
            WeatherMode::Rain => self.shader_rain(canvas, camera, frame, false),
            WeatherMode::Storm => self.shader_rain(canvas, camera, frame, true),
            WeatherMode::Snow => self.shader_snow(canvas, camera, frame),
        }
    }

    fn shader_rain(
        &self,
        canvas: &mut Canvas,
        _camera: ascii_mmorpg_shared::Point,
        frame: u64,
        is_storm: bool,
    ) {
        let width = canvas.width;
        let height = canvas.height as isize;
        let layers = if is_storm { 4 } else { 3 };
        let is_lightning_flash = is_storm && (frame % 45 < 2);

        for layer in 0..layers {
            for x in (layer..width).step_by(4) {
                let fall_step = if is_storm {
                    match layer {
                        0 => frame / 8,
                        1 => frame / 6,
                        2 => frame / 4,
                        _ => (frame * 3) / 8,
                    }
                } else {
                    match layer {
                        0 => frame / 24,
                        1 => frame / 18,
                        _ => frame / 12,
                    }
                } as usize;

                let span = canvas.height.max(1) + 9;
                let phase = (x * 29 + layer * 47 + fall_step + 11) % span;
                let head = phase as isize - 4;
                let wind_drift = if is_storm { head / 4 } else { 0 };
                let px = (x as isize + wind_drift).rem_euclid(width as isize);

                // Slender diagonal streaks: bright head, dim `/` tail.
                for offset in 0..3isize {
                    let y = head + offset;
                    if y < 0 || y >= height {
                        continue;
                    }

                    let current = canvas.get(px, y);
                    if Self::is_ui_or_anchor_cell(current) {
                        continue;
                    }

                    let (glyph, ink) = if is_lightning_flash {
                        ('/', Ink::LightningWhite)
                    } else if offset == 0 {
                        ('/', Ink::RainBright)
                    } else {
                        ('/', Ink::Rain)
                    };

                    canvas.set(px, y, glyph, ink);
                }

                if head >= height - 3 && head < height {
                    let splash_x = (x as isize + 1).rem_euclid(width as isize);
                    let splash_y = (head + 1).min(height - 1);
                    let current = canvas.get(splash_x, splash_y);
                    if !Self::is_ui_or_anchor_cell(current) && current.glyph != ' ' {
                        let splash_glyph = if ((frame / 6) + x as u64) % 2 == 0 {
                            '~'
                        } else {
                            '.'
                        };
                        canvas.set(splash_x, splash_y, splash_glyph, Ink::Splash);
                    }
                }
            }
        }
    }

    fn shader_snow(&self, canvas: &mut Canvas, _camera: ascii_mmorpg_shared::Point, frame: u64) {
        let width = canvas.width as isize;
        let height = canvas.height as isize;

        for layer in 0..3usize {
            let num_columns = canvas.width;
            let fall_step = match layer {
                0 => frame / 24,
                1 => frame / 16,
                _ => frame / 20,
            } as usize;
            let span = canvas.height.max(1) + 8;

            for x in (layer..num_columns).step_by(4) {
                let seed = x * 37 + layer * 73;
                let phase = (seed + fall_step) % span;
                let y = phase as isize - 2;

                if y < 0 || y >= height {
                    continue;
                }

                let sway_freq = 0.25;
                let sway_amp = if layer == 0 { 2.5 } else { 1.5 };
                let sway = ((y as f64 * sway_freq + (frame as f64 / 6.0) * 0.1 + seed as f64).sin()
                    * sway_amp) as isize;
                let px = (x as isize + sway).rem_euclid(width);

                let current = canvas.get(px, y);
                if Self::is_ui_or_anchor_cell(current) {
                    continue;
                }

                let (glyph, ink) = match (layer, (seed + (frame / 6) as usize) % 4) {
                    (0, 0) => ('*', Ink::SnowGlow),
                    (0, _) => ('*', Ink::Snow),
                    (1, 0) => ('•', Ink::Snow),
                    (1, _) => ('·', Ink::SnowSoft),
                    (_, _) => ('.', Ink::SnowSoft),
                };

                canvas.set(px, y, glyph, ink);
            }
        }
    }

    fn draw_effects(
        &self,
        canvas: &mut Canvas,
        world: &WorldSnapshot,
        actors: &[&ActorSnapshot],
        frame: u64,
    ) {
        // Effects are event-shaped: they only appear on combat ticks, never as
        // a persistent beam. The reference look is a strike flash plus impact.
        if !Self::combat_active(world) {
            return;
        }
        let target = actors
            .iter()
            .find(|actor| actor.kind == EntityKind::Monster)
            .map(|actor| actor.position)
            .unwrap_or(ascii_mmorpg_shared::Point {
                x: world.player.position.x.saturating_add(6),
                y: world.player.position.y,
            });

        let target = self.screen_position(target, world.player.position);
        canvas.set(target.0, target.1, '+', Ink::Spell);

        // Lightning arc cracking from the sky above target
        let bolt_x = target.0;
        let bolt_y = target.1;
        for offset in 1..=4 {
            let lx = bolt_x + if offset % 2 == 0 { -1 } else { 1 };
            let ly = bolt_y - offset;
            let glyph = if offset % 2 == 0 { '/' } else { '\\' };
            canvas.set(
                lx,
                ly,
                glyph,
                if offset == 1 {
                    Ink::LightningWhite
                } else {
                    Ink::Lightning
                },
            );
        }

        // Fiery explosion sparks around target
        let phase = (frame % 4) as isize;
        for (dx, dy) in [(-2, 0), (2, 0), (0, -1), (0, 1), (-1, -1), (1, 1)] {
            let px = target.0 + dx + (phase % 2) - 1;
            let py = target.1 + dy;
            let ember = match (dx + dy + phase).rem_euclid(3) {
                0 => '*',
                1 => '#',
                _ => '%',
            };
            canvas.set(px, py, ember, Ink::TorchFlame);
        }

        // Floating combat damage indicator: the real figure from the latest
        // damage message, never a placeholder.
        if let Some(number) = Self::last_damage_dealt(world) {
            canvas.text(target.0 - 2, target.1 - 4, &number, Ink::FloatingDamage);
        }
    }

    fn combat_active(world: &WorldSnapshot) -> bool {
        world.messages.iter().any(|message| {
            let message = message.to_ascii_lowercase();
            message.contains("cast")
                || message.contains("attack")
                || message.contains("hit")
                || message.contains("damage")
        })
    }

    /// Most recent integer damage figure in the visible messages: the `3` in
    /// "You hit Mossling for 3 damage." or the `12` in
    /// "Combat 1 -> 2: 12 damage (defeated)".
    fn last_damage_dealt(world: &WorldSnapshot) -> Option<String> {
        world.messages.iter().rev().find_map(|message| {
            let end = message.to_ascii_lowercase().find("damage")?;
            let digits: String = message[..end]
                .trim_end()
                .chars()
                .rev()
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>()
                .chars()
                .rev()
                .collect();
            if digits.is_empty() {
                None
            } else {
                Some(digits)
            }
        })
    }

    fn draw_actor(
        &self,
        canvas: &mut Canvas,
        assets: &AssetLibrary,
        actor: &ActorSnapshot,
        camera: ascii_mmorpg_shared::Point,
        label_row: isize,
        frame: u64,
    ) {
        let (x, y) = self.screen_position(actor.position, camera);
        let ink = match actor.kind {
            EntityKind::Player => Ink::Player,
            EntityKind::Monster => Ink::Monster,
            EntityKind::Npc => Ink::Npc,
            EntityKind::Item => Ink::Item,
        };

        let artwork = assets.for_entity(&actor.kind, &actor.name, frame);
        let artwork_width = artwork
            .lines()
            .map(|line| line.trim_end_matches('\r').chars().count())
            .max()
            .unwrap_or(1) as isize;

        // 3D ground drop shadow under character
        canvas.set(x - 2, y + 3, '(', Ink::DropShadow);
        canvas.set(x - 1, y + 3, '_', Ink::DropShadow);
        canvas.set(x, y + 3, '_', Ink::DropShadow);
        canvas.set(x + 1, y + 3, '_', Ink::DropShadow);
        canvas.set(x + 2, y + 3, ')', Ink::DropShadow);

        match actor.kind {
            EntityKind::Player => {
                // Volumetric knight plate armor with golden crest, glowing visor, and crimson cape
                self.draw_player_knight(canvas, artwork, x - artwork_width / 2, y - 2);
            }
            EntityKind::Monster => {
                // Pale skeletal undead shade by label; everything else stays a mossling.
                if actor.name.to_ascii_lowercase().contains("skeleton") {
                    self.draw_monster_skeleton(canvas, artwork, x - artwork_width / 2, y - 2);
                } else {
                    self.draw_monster_mossling(canvas, artwork, x - artwork_width / 2, y - 2);
                }
            }
            EntityKind::Npc => {
                // Robed NPC herbalist with linen folds and gentle idle breathing
                self.draw_npc_herbalist(canvas, artwork, x - artwork_width / 2, y - 2);
            }
            EntityKind::Item => {
                self.draw_asset(canvas, artwork, x - artwork_width / 2, y - 2, ink);
            }
        }

        // Keep the server-provided glyph visible as a compact identity cue
        canvas.set(x, y, actor.glyph, ink);

        let label_x = x - actor.name.chars().count() as isize / 2;
        canvas.text(label_x, label_row, &actor.name, Ink::Text);
        let max_hp = actor.stats.max_hp.max(1);
        let hp = actor.stats.hp.clamp(0, max_hp);
        let filled = (hp * 8 / max_hp) as usize;
        let bar = format!("[{}{}]", "#".repeat(filled), "-".repeat(8 - filled));
        canvas.text(x - 5, label_row + 1, &bar, Ink::Muted);
    }

    fn draw_player_knight(&self, canvas: &mut Canvas, artwork: &str, x: isize, y: isize) {
        for (row, line) in artwork.lines().enumerate() {
            for (column, glyph) in line.trim_end_matches('\r').chars().enumerate() {
                if glyph.is_whitespace() {
                    continue;
                }
                let px = x + column as isize;
                let py = y + row as isize;
                let ink = match glyph {
                    '▲' | '^' => Ink::KnightGold,              // Golden crest / horns
                    ':' | '=' if row <= 2 => Ink::KnightVisor, // Visor eye slit / chin guard
                    'o' if row == 1 => Ink::NpcSkin,           // Humanoid face feature / eyes
                    '@' => Ink::KnightSteel, // Plate fill: bright steel, not blue wash
                    '▓' | '8' | '[' | ']' => Ink::KnightSteel, // Gleaming plate armor
                    '|' | '<' | '>' | '(' | ')' if row <= 6 => Ink::KnightSteel,
                    '/' | '\\' if (column <= 2 || column >= 11) && row <= 6 => Ink::KnightCape, // Crimson cape edges
                    '/' | '\\' => Ink::KnightSteel, // Steel greaves / arms / legs
                    '_' => Ink::DropShadow,         // Ground shadow
                    _ => Ink::Player,
                };
                canvas.set(px, py, glyph, ink);
            }
        }
    }

    fn draw_monster_mossling(&self, canvas: &mut Canvas, artwork: &str, x: isize, y: isize) {
        for (row, line) in artwork.lines().enumerate() {
            for (column, glyph) in line.trim_end_matches('\r').chars().enumerate() {
                if glyph.is_whitespace() {
                    continue;
                }
                let px = x + column as isize;
                let py = y + row as isize;
                let ink = match glyph {
                    '^' | 'v' | '.' if row == 2 || row == 3 => Ink::MossEye, // Glowing amber eyes
                    '8' | 'o' | 'O' => Ink::MossBody, // Organic moss body mass
                    '#' | '%' => Ink::MossHide,       // Shadowed hide core
                    '{' | '}' | '/' | '\\' | '|' => Ink::Moss, // Living moss tendrils
                    '_' => Ink::DropShadow,           // Ground drop shadow
                    _ => Ink::Monster,
                };
                canvas.set(px, py, glyph, ink);
            }
        }
    }

    fn draw_monster_skeleton(&self, canvas: &mut Canvas, artwork: &str, x: isize, y: isize) {
        for (row, line) in artwork.lines().enumerate() {
            for (column, glyph) in line.trim_end_matches('\r').chars().enumerate() {
                if glyph.is_whitespace() {
                    continue;
                }
                let px = x + column as isize;
                let py = y + row as isize;
                let ink = match glyph {
                    'o' => Ink::BoneEye,                            // Hollow socket glint
                    'O' | '"' => Ink::BoneLight,                    // Cranium mass and bared teeth
                    '=' => Ink::Bone,                               // Rib striations
                    ';' | ':' | 'x' => Ink::Bone,                   // Tattered sinew and joints
                    '#' | '%' => Ink::BoneDark,                     // Shadowed bone core
                    '.' | '-' if row == 1 => Ink::BoneDark,         // Closed lid / skull seam
                    '[' | ']' | '>' => Ink::KnightSteel,            // Warrior shield and blade
                    '^' | '▓' => Ink::KnightSteel,                  // Warrior crest and pauldrons
                    '(' | ')' | '|' | '/' | '\\' => Ink::BoneLight, // Lit bone ridges
                    '_' => Ink::DropShadow,                         // Ground drop shadow
                    _ => Ink::Bone,
                };
                canvas.set(px, py, glyph, ink);
            }
        }
    }

    fn draw_npc_herbalist(&self, canvas: &mut Canvas, artwork: &str, x: isize, y: isize) {
        for (row, line) in artwork.lines().enumerate() {
            for (column, glyph) in line.trim_end_matches('\r').chars().enumerate() {
                if glyph.is_whitespace() {
                    continue;
                }
                let px = x + column as isize;
                let py = y + row as isize;
                let ink = match glyph {
                    'o' | '.' | '-' if row <= 2 => Ink::NpcSkin, // Subtle facial features
                    '@' | '|' | '/' | '\\' => Ink::NpcRobe,      // Worn woven linen robes
                    '_' => Ink::DropShadow,
                    _ => Ink::Npc,
                };
                canvas.set(px, py, glyph, ink);
            }
        }
    }

    fn draw_tree_prop(&self, canvas: &mut Canvas, artwork: &str, x: isize, y: isize) {
        for (row, line) in artwork.lines().enumerate() {
            for (column, glyph) in line.trim_end_matches('\r').chars().enumerate() {
                if glyph.is_whitespace() {
                    continue;
                }
                let px = x + column as isize;
                let py = y + row as isize;
                let ink = match glyph {
                    '▲' => Ink::PineTree,
                    '▓' | '▒' => Ink::PineCanopy,
                    '|' => Ink::WoodPost,
                    '(' | ')' if row >= 18 => Ink::WoodPost,
                    '_' => Ink::TreeShadow,
                    _ => Ink::Foliage,
                };
                canvas.set(px, py, glyph, ink);
            }
        }
    }

    fn draw_ruin_prop(&self, canvas: &mut Canvas, artwork: &str, x: isize, y: isize) {
        for (row, line) in artwork.lines().enumerate() {
            for (column, glyph) in line.trim_end_matches('\r').chars().enumerate() {
                if glyph.is_whitespace() {
                    continue;
                }
                let px = x + column as isize;
                let py = y + row as isize;
                let ink = match glyph {
                    '=' | '-' => Ink::StoneLight,
                    '[' | ']' => Ink::RuinWindow,
                    '/' | '\\' => Ink::StoneDark,
                    '|' => Ink::RuinWall,
                    '_' => Ink::TreeShadow,
                    _ => Ink::Stone,
                };
                canvas.set(px, py, glyph, ink);
            }
        }
    }

    fn draw_gate_prop(&self, canvas: &mut Canvas, artwork: &str, x: isize, y: isize) {
        for (row, line) in artwork.lines().enumerate() {
            for (column, glyph) in line.trim_end_matches('\r').chars().enumerate() {
                if glyph.is_whitespace() {
                    continue;
                }
                let px = x + column as isize;
                let py = y + row as isize;
                let ink = match glyph {
                    '#' => Ink::RuinWall,
                    '[' | ']' => Ink::RuinWindow,
                    '=' | '-' => Ink::StoneLight,
                    '/' | '\\' => Ink::StoneDark,
                    '|' => Ink::WoodPost,
                    _ => Ink::Stone,
                };
                canvas.set(px, py, glyph, ink);
            }
        }
    }

    fn screen_position(
        &self,
        position: ascii_mmorpg_shared::Point,
        camera: ascii_mmorpg_shared::Point,
    ) -> (isize, isize) {
        // Oblique projection: one shared camera transform for every world
        // layer, so walking scrolls the world while actors keep screen scale.
        let dx = position.x as isize - camera.x as isize;
        let dy = position.y as isize - camera.y as isize;
        (
            self.width.max(1) as isize / 2 + dx + dy / 2,
            self.height.max(1) as isize / 2 + dy / 2,
        )
    }

    fn surface_cell(x: i16, y: i16, camera: ascii_mmorpg_shared::Point, patch: i16) -> (char, Ink) {
        let distance = i32::from(x - camera.x).abs() + i32::from(y - camera.y).abs();
        let seed = Self::terrain_seed(x, y);
        // Unlit ground thins out toward the void; lit ground keeps texture.
        let shade = ((1.0 - Self::light_level(x, y, camera)) * 12.0) as i32;
        if shade >= 10 {
            // Deep void: near-black with only rare dither specks.
            if seed % 41 < 2 {
                ('·', Ink::Muted)
            } else {
                (' ', Ink::Background)
            }
        } else if shade <= 4 {
            // Bright pool: full masonry ramp with letter-glyph body.
            let ridge =
                ((seed % 17) as i32 + i32::from(patch) * 2 - distance / 8 - shade * 2).max(0);
            let glyphs = [' ', '·', '.', ':', 'o', 'Q', '8', 'B', '%', '#'];
            if ridge < 5 {
                (' ', Ink::Background)
            } else {
                let glyph_index = (ridge / 2 + i32::from(patch) + (seed / 19) as i32)
                    .rem_euclid(glyphs.len() as i32) as usize;
                let glyph = glyphs[glyph_index];
                let ink = if ridge > 15 {
                    Ink::StoneLight
                } else if ridge > 10 {
                    Ink::Moss
                } else {
                    Ink::GrassDark
                };
                (glyph, ink)
            }
        } else {
            // Dim fringe: stipple dither only, no letter soup.
            let ridge = ((seed % 13) as i32 + i32::from(patch) - distance / 10).max(0);
            let glyphs = [' ', '·', '.', '·', ':', '·', '.', ' '];
            if ridge < 6 {
                (' ', Ink::Background)
            } else {
                let glyph_index = (ridge + i32::from(patch) + (seed / 19) as i32)
                    .rem_euclid(glyphs.len() as i32) as usize;
                (glyphs[glyph_index], Ink::GrassDark)
            }
        }
    }

    fn terrain_seed(x: i16, y: i16) -> u32 {
        let mut value = (i32::from(x) as u32).wrapping_mul(0x45d9f3b);
        value ^= (i32::from(y) as u32).wrapping_mul(0x119de1f3);
        value ^= value >> 16;
        value.wrapping_mul(0x45d9f3b) ^ (value >> 13)
    }

    fn draw_asset(&self, canvas: &mut Canvas, artwork: &str, x: isize, y: isize, ink: Ink) {
        for (row, line) in artwork.lines().enumerate() {
            for (column, glyph) in line.trim_end_matches('\r').chars().enumerate() {
                if !glyph.is_whitespace() {
                    canvas.set(x + column as isize, y + row as isize, glyph, ink);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ascii_mmorpg_shared::{ActorSnapshot, Point, Stats};

    fn scene_world() -> WorldSnapshot {
        WorldSnapshot {
            tick: 7,
            player: ActorSnapshot {
                id: 1,
                name: "Ranger".into(),
                kind: EntityKind::Player,
                position: Point { x: 8, y: 7 },
                stats: Stats::healthy(20, 6, 2),
                glyph: '@',
            },
            entities: vec![ActorSnapshot {
                id: 2,
                name: "Mossling".into(),
                kind: EntityKind::Monster,
                position: Point { x: 18, y: 7 },
                stats: Stats {
                    max_hp: 8,
                    hp: 5,
                    attack: 3,
                    defense: 1,
                    luck: 0,
                },
                glyph: 'm',
            }],
            messages: vec!["You hit Mossling for 3 damage.".into()],
        }
    }

    #[test]
    fn frame_output_is_deterministic_for_same_tick() {
        let renderer = Renderer::new(32, 14).with_weather(WeatherMode::Rain);
        let world = scene_world();
        assert_eq!(
            renderer.render_frame(&world, 4),
            renderer.render_frame(&world, 4)
        );
    }

    #[test]
    fn scene_contains_entity_metadata_visuals_labels_bars_and_effect() {
        // 100 cols: the side panel takes 28, leaving a 72-col scene slice so
        // the mossling HP bar (canvas cols ~45-55) is not clipped.
        let renderer = Renderer::new(100, 30).with_weather(WeatherMode::Rain);
        let frame = renderer.render_frame(&scene_world(), 4);
        assert!(frame.contains("Ranger"));
        assert!(frame.contains("Mossling"));
        assert!(frame.contains("[########]"));
        assert!(frame.contains("[#####---]"));
        // Composed frames encode scene inks as TrueColor (xterm palette RGB).
        assert!(frame.contains("\x1b[38;2;175;215;255m"));
        assert!(frame.contains("\x1b[38;2;255;95;95m"));
        assert!(frame.contains("\x1b[38;2;135;215;255m"));
        assert!(frame.contains('/'));
    }

    #[test]
    fn weather_shader_snow_and_storm_modes_render_distinct_overlays() {
        let mut renderer = Renderer::new(40, 18).with_weather(WeatherMode::Snow);
        let world = scene_world();
        let snow_frame = renderer.render_frame(&world, 4);
        assert!(snow_frame.contains('*') || snow_frame.contains('·'));
        assert!(
            snow_frame.contains("\x1b[38;2;230;245;255m")
                || snow_frame.contains("\x1b[38;2;255;255;255m")
                || snow_frame.contains("\x1b[38;2;160;195;225m")
        );

        renderer.weather = WeatherMode::Storm;
        let storm_frame = renderer.render_frame(&world, 4);
        assert!(storm_frame.contains('/'));
        assert_ne!(snow_frame, storm_frame);
    }

    #[test]
    fn animation_phase_changes_frame_without_changing_snapshot() {
        let renderer = Renderer::new(80, 30).with_weather(WeatherMode::Rain);
        let world = scene_world();
        let first = renderer.render_frame(&world, 0);
        let second = renderer.render_frame(&world, 1);
        assert_ne!(first, second);
        assert_eq!(world.tick, 7);
        assert_eq!(world.player.position, Point { x: 8, y: 7 });
    }

    #[test]
    fn player_camera_keeps_player_fixed_and_scrolls_mob() {
        let renderer = Renderer::new(40, 20).with_weather(WeatherMode::Rain);
        let mut world = scene_world();
        let before = renderer.render_frame(&world, 4);
        crate::apply_local_command(
            &mut world,
            &ascii_mmorpg_shared::ClientCommand::new(
                8,
                ascii_mmorpg_shared::Command::Move(ascii_mmorpg_shared::Direction::East),
            ),
        );
        let after = renderer.render_frame(&world, 4);
        let anchor = |frame: &str, glyph| {
            frame
                .lines()
                .enumerate()
                .find_map(|(row, line)| {
                    let mut escape = false;
                    let mut column = 0;
                    for ch in line.chars() {
                        if ch == '\x1b' {
                            escape = true;
                            continue;
                        }
                        if escape {
                            if ch == 'm' {
                                escape = false;
                            }
                            continue;
                        }
                        if ch == glyph {
                            return Some((column, row));
                        }
                        column += 1;
                    }
                    None
                })
                .expect("actor glyph should be visible")
        };
        assert_eq!(anchor(&before, '@'), anchor(&after, '@'));
        // Composed frames put a 3-row HUD top bar above the scene, so canvas
        // row 10 lands on frame row 13.
        assert_eq!(anchor(&before, 'm'), (30, 13));
        assert_eq!(anchor(&after, 'm'), (29, 13));
        assert_eq!(world.entities[0].position, Point { x: 18, y: 7 });
    }

    #[test]
    fn rendering_does_not_mutate_authoritative_snapshot() {
        let renderer = Renderer::new(32, 14).with_weather(WeatherMode::Rain);
        let world = scene_world();
        let snapshot_before = world.clone();
        let _ = renderer.render_frame(&world, 12);
        assert_eq!(world, snapshot_before);
    }

    #[test]
    fn surface_sampling_is_deterministic_and_depth_varies_glyph_density() {
        let camera = Point { x: 4, y: 9 };
        assert_eq!(
            Renderer::surface_cell(4, 9, camera, 3),
            Renderer::surface_cell(4, 9, camera, 3)
        );
        assert_ne!(
            Renderer::surface_cell(4, 9, camera, 0).0,
            Renderer::surface_cell(4, 9, camera, 7).0
        );
    }

    #[test]
    fn damage_number_comes_from_messages_not_a_placeholder() {
        let renderer = Renderer::new(32, 14).with_weather(WeatherMode::Rain);
        let frame = renderer.render_frame(&scene_world(), 4);
        assert!(!frame.contains("91"));
        assert!(frame.contains('3'));
    }

    #[test]
    fn damage_figure_parses_local_and_server_message_shapes() {
        assert_eq!(
            Renderer::last_damage_dealt(&WorldSnapshot {
                messages: vec!["You hit Mossling for 3 damage.".into()],
                ..scene_world()
            }),
            Some("3".into())
        );
        assert_eq!(
            Renderer::last_damage_dealt(&WorldSnapshot {
                messages: vec!["Combat 1 -> 2: 12 damage (defeated)".into()],
                ..scene_world()
            }),
            Some("12".into())
        );
        assert_eq!(
            Renderer::last_damage_dealt(&WorldSnapshot {
                messages: vec!["You cast Fireball; effects are queued.".into()],
                ..scene_world()
            }),
            None
        );
    }

    #[test]
    fn light_level_falls_off_with_distance_and_follows_player() {
        let far = Point { x: -20, y: 20 };
        let core = Renderer::light_level(-6, 2, far);
        let edge = Renderer::light_level(1, 2, far);
        assert!(core > 0.9, "lantern core {core}");
        assert!(edge < core, "edge {edge} should be dimmer than core {core}");
        let camera = Point { x: -40, y: 17 };
        assert!(Renderer::light_level(-40, 17, camera) > 0.9);
    }

    #[test]
    fn oblique_camera_shifts_world_by_exact_pixels() {
        let renderer = Renderer::new(40, 20).with_weather(WeatherMode::Rain);
        let origin = Point { x: 0, y: 0 };
        let before = renderer.screen_position(Point { x: -14, y: 5 }, origin);
        let after = renderer.screen_position(Point { x: -14, y: 5 }, Point { x: 1, y: 0 });
        assert_eq!((before.0 - after.0, before.1 - after.1), (1, 0));
        assert_eq!(renderer.screen_position(origin, origin), (20, 10));
    }

    #[test]
    fn scattered_props_are_world_locked_when_camera_moves() {
        use crate::assets::AssetLibrary;

        let renderer = Renderer::new(40, 20).with_weather(WeatherMode::Rain);
        let assets = AssetLibrary::new();
        // Backdrop only: no actors, weather, or veil, so every remaining
        // prop-ink cell must be perfectly world-locked.
        let mut first = Canvas::new(40, 20);
        renderer.draw_backdrop(&mut first, &assets, Point { x: 8, y: 7 }, &[], 4);
        let mut second = Canvas::new(40, 20);
        renderer.draw_backdrop(&mut second, &assets, Point { x: 9, y: 7 }, &[], 4);
        fn is_prop_ink(ink: Ink) -> bool {
            matches!(
                ink,
                Ink::PineTree
                    | Ink::PineCanopy
                    | Ink::TreeShadow
                    | Ink::RuinWall
                    | Ink::RuinWindow
                    | Ink::WoodPost
                    | Ink::BannerRed
                    | Ink::TorchFlame
                    | Ink::TorchGlow
            )
        }
        let mut checked = 0;
        for y in 0..20isize {
            for x in 1..40isize {
                let cell = first.get(x, y);
                if is_prop_ink(cell.ink) {
                    let moved = second.get(x - 1, y);
                    assert_eq!(
                        (moved.glyph, moved.ink),
                        (cell.glyph, cell.ink),
                        "prop drift at ({x},{y})"
                    );
                    checked += 1;
                }
            }
        }
        assert!(checked > 20, "expected scattered props in view");
    }

    #[test]
    fn depth_veil_sinks_terrain_but_spares_foreground() {
        assert_eq!(Renderer::dim_ink(Ink::StoneLight), Ink::Stone);
        assert_eq!(Renderer::dim_ink(Ink::Stone), Ink::StoneDark);
        assert_eq!(Renderer::dim_ink(Ink::StoneDark), Ink::Background);
        assert_eq!(Renderer::dim_ink(Ink::PineTree), Ink::PineCanopy);
        // Foreground channels pass through untouched.
        for ink in [
            Ink::Text,
            Ink::Player,
            Ink::Monster,
            Ink::BoneLight,
            Ink::MossBody,
            Ink::KnightSteel,
            Ink::Rain,
            Ink::TorchFlame,
            Ink::FloatingDamage,
            Ink::Muted,
            Ink::Background,
        ] {
            assert_eq!(Renderer::dim_ink(ink), ink);
        }
        let renderer = Renderer::new(40, 20).with_weather(WeatherMode::Rain);
        let canvas = renderer.render_canvas(&scene_world(), 4);
        // Player anchor at the center keeps full brightness.
        assert_eq!(canvas.get(20, 10).glyph, '@');
        // Far corners hold no bright terrain inks.
        for (x, y) in [(0, 0), (39, 0), (0, 19), (39, 19)] {
            let cell = canvas.get(x, y);
            assert!(
                !matches!(
                    cell.ink,
                    Ink::StoneLight | Ink::CobbleGlow | Ink::GrassTorch | Ink::Moss | Ink::PineTree
                ),
                "bright terrain at corner ({x},{y}): {cell:?}"
            );
        }
    }

    /// Snapshot dumps for visual review against `data/videos/`. Opt-in via
    /// `ASCII_DUMP=1` so normal runs stay side-effect free; output goes to
    /// the OS temp dir, never the repo.
    #[test]
    fn dump_reference_snapshots_for_visual_review() {
        if std::env::var("ASCII_DUMP").is_err() {
            return;
        }
        let dir = std::env::temp_dir().join("ascii-mmorpg-snaps");
        std::fs::create_dir_all(&dir).unwrap();
        let mut warrior_world = scene_world();
        warrior_world.entities.push(ActorSnapshot {
            id: 3,
            name: "Skeleton Warrior".into(),
            kind: EntityKind::Monster,
            position: Point { x: 12, y: 5 },
            stats: Stats {
                max_hp: 14,
                hp: 9,
                attack: 5,
                defense: 2,
                luck: 0,
            },
            glyph: 'w',
        });
        warrior_world.messages = vec!["You hit Skeleton Warrior for 6 damage.".into()];
        let scenes: Vec<(&str, WorldSnapshot)> = vec![
            ("overworld", crate::sample_world()),
            ("combat", scene_world()),
            ("warrior", warrior_world),
        ];
        for (name, world) in &scenes {
            let renderer = Renderer::new(80, 40).with_weather(WeatherMode::Rain);
            let frame = renderer.render_frame(world, 4);
            std::fs::write(dir.join(format!("{name}.ansi.txt")), frame).unwrap();
        }
    }

    #[test]
    fn overlapping_actors_keep_distinct_label_rows() {
        let renderer = Renderer::new(60, 24).with_weather(WeatherMode::Rain);
        let mut world = scene_world();
        world.entities.push(ActorSnapshot {
            id: 3,
            name: "Skeleton".into(),
            kind: EntityKind::Monster,
            position: Point { x: 10, y: 7 },
            stats: Stats::healthy(10, 4, 1),
            glyph: 's',
        });
        let canvas = renderer.render_canvas(&world, 4);
        let row_text = |row: usize| -> String {
            canvas.cells[row * canvas.width..(row + 1) * canvas.width]
                .iter()
                .map(|cell| cell.glyph)
                .collect()
        };
        let moss_row = (0..canvas.height)
            .find(|row| row_text(*row).contains("Mossling"))
            .expect("mossling label");
        let skel_row = (0..canvas.height)
            .find(|row| row_text(*row).contains("Skeleton"))
            .expect("skeleton label");
        let ranger_row = (0..canvas.height)
            .find(|row| row_text(*row).contains("Ranger"))
            .expect("player label");
        assert_ne!(moss_row, skel_row);
        assert_ne!(moss_row, ranger_row);
        assert_ne!(skel_row, ranger_row);
    }

    #[test]
    fn frame_background_dominates_scene() {
        let renderer = Renderer::new(80, 40).with_weather(WeatherMode::Rain);
        let canvas = renderer.render_canvas(&scene_world(), 4);
        let total = (canvas.width * canvas.height) as f64;
        let dark = canvas
            .cells
            .iter()
            .filter(|cell| cell.ink == Ink::Background)
            .count() as f64;
        assert!(
            dark / total >= 0.45,
            "background should dominate: {dark}/{total}"
        );
    }

    #[test]
    fn still_scene_cells_do_not_shimmer_between_frames() {
        let renderer = Renderer::new(40, 20).with_weather(WeatherMode::Rain);
        let world = scene_world();
        // Same animation frame index (4 % 4 == 8 % 4) so authored art is
        // identical; only weather is allowed to move still-air cells.
        let first = renderer.render_canvas(&world, 4);
        let second = renderer.render_canvas(&world, 8);
        let mut actor_zones = Vec::new();
        for actor in world.entities.iter().chain(std::iter::once(&world.player)) {
            let (x, y) = renderer.screen_position(actor.position, world.player.position);
            actor_zones.push((x, y));
        }
        let near_actor = |x: isize, y: isize| {
            actor_zones
                .iter()
                .any(|(ax, ay)| (x - ax).abs() <= 8 && (y - ay).abs() <= 7)
        };
        for y in 0..first.height as isize {
            for x in 0..first.width as isize {
                let before = first.get(x, y);
                let after = second.get(x, y);
                if before.glyph != after.glyph || before.ink != after.ink {
                    assert!(
                        Renderer::is_weather_ink(before.ink)
                            || Renderer::is_weather_ink(after.ink)
                            || near_actor(x, y),
                        "shimmer at ({x},{y}): {before:?} -> {after:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn test_cpu_player_feet_do_not_move_when_idle() {
        let renderer = Renderer::new(40, 20).with_weather(WeatherMode::Rain);
        let world = scene_world();
        // Stationary player is idle: feet must remain in Frame 0 pose across frames
        assert!(
            !renderer.is_player_walking(&world),
            "Stationary player should be idle"
        );
        let canvas0 = renderer.render_canvas(&world, 0);
        let canvas1 = renderer.render_canvas(&world, 1);
        let canvas3 = renderer.render_canvas(&world, 3);
        let (px, py) = renderer.screen_position(world.player.position, world.player.position);
        // Boots are at py + 1. Left boot: px - 1, Right boot: px + 1
        assert_eq!(
            canvas0.get(px - 1, py + 1),
            canvas1.get(px - 1, py + 1),
            "Boots must stay planted between frame 0 and 1 while idle"
        );
        assert_eq!(
            canvas0.get(px - 1, py + 1),
            canvas3.get(px - 1, py + 1),
            "Boots must stay planted between frame 0 and 3 while idle"
        );
    }
}
