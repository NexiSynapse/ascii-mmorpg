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
}

impl Ink {
    const fn ansi(self) -> &'static str {
        match self {
            Self::Background => "\x1b[38;2;12;15;22m", // Deep midnight dark fantasy void
            Self::Stone => "\x1b[38;5;101m",
            Self::StoneDark => "\x1b[38;2;65;60;55m", // Shadowed stone face
            Self::StoneLight => "\x1b[38;2;175;165;150m", // Sunlit stone highlight
            Self::Moss => "\x1b[38;5;65m",
            Self::Foliage => "\x1b[38;5;22m",
            Self::Rain => "\x1b[38;5;60m",
            Self::RainBright => "\x1b[38;5;116m",
            Self::Player => "\x1b[38;5;117m",
            Self::Monster => "\x1b[38;5;203m",
            Self::Npc => "\x1b[38;5;221m",
            Self::Item => "\x1b[38;5;220m",
            Self::Spell => "\x1b[38;5;153m",
            Self::Torch => "\x1b[38;5;220m",
            Self::Text => "\x1b[38;5;252m",
            Self::Muted => "\x1b[38;5;245m",
            Self::KnightSteel => "\x1b[38;2;225;232;240m", // Polished steel plate
            Self::KnightGold => "\x1b[38;2;255;210;75m",   // Golden helm crest / horns
            Self::KnightCape => "\x1b[38;2;195;35;48m",    // Crimson battle cape
            Self::KnightVisor => "\x1b[38;2;140;180;220m", // Eye slit steel blue
            Self::DropShadow => "\x1b[38;2;16;20;28m",     // 3D ground drop shadow
            Self::CobbleRoad => "\x1b[38;2;125;115;105m",  // Warm ancient cobblestone (dimmed)
            Self::CobbleGlow => "\x1b[38;2;200;160;95m",   // Torchlit cobblestone (softened)
            Self::CobbleBorder => "\x1b[38;2;65;60;55m",   // Cobblestone curb (dimmed)
            Self::GrassMeadow => "\x1b[38;2;32;75;55m",    // Deep emerald grass (darker)
            Self::GrassDark => "\x1b[38;2;22;50;38m",      // Shadowed grass
            Self::GrassTorch => "\x1b[38;2;85;155;115m",   // Torchlit grass (softer)
            Self::PineTree => "\x1b[38;2;35;80;65m",       // Frosty pine needles (darker)
            Self::PineCanopy => "\x1b[38;2;25;60;48m",     // Deep pine shadow
            Self::TreeShadow => "\x1b[38;2;12;20;16m",     // Tree base drop shadow
            Self::TorchFlame => "\x1b[38;2;255;95;25m",    // Blazing orange flame
            Self::TorchGlow => "\x1b[38;2;200;145;45m",    // Radiant amber halo (dimmed)
            Self::Lightning => "\x1b[38;2;76;201;240m",    // Electric cyan lightning bolt
            Self::LightningWhite => "\x1b[38;2;255;255;255m", // White lightning core
            Self::BannerRed => "\x1b[38;2;165;25;38m",     // Crimson war banner (darker)
            Self::WoodPost => "\x1b[38;2;95;68;48m",       // Timber post (darker)
            Self::FloatingDamage => "\x1b[38;2;255;220;50m", // Popping combat numbers
            // Monster depth palette - mossy green/brown organic tones
            Self::MossBody => "\x1b[38;2;88;130;72m", // Mossy green body mass
            Self::MossEye => "\x1b[38;2;255;200;60m", // Glowing amber eyes
            Self::MossHide => "\x1b[38;2;55;85;48m",  // Dark moss hide shadow
            // Skeleton bone palette - pale sun-bleached undead
            Self::BoneLight => "\x1b[38;2;216;212;200m", // Lit bone ridge
            Self::Bone => "\x1b[38;2;168;164;150m",      // Weathered bone mass
            Self::BoneDark => "\x1b[38;2;110;106;96m",   // Shadowed bone core
            Self::BoneEye => "\x1b[38;2;255;233;168m",   // Hollow socket glint
            // NPC depth palette
            Self::NpcRobe => "\x1b[38;2;160;140;100m", // Worn linen robe
            Self::NpcSkin => "\x1b[38;2;220;185;150m", // Warm skin tone
            // Ruin depth palette
            Self::RuinWall => "\x1b[38;2;110;100;88m", // Weathered ruin wall
            Self::RuinWindow => "\x1b[38;2;45;55;80m", // Dark interior through window
            Self::Snow => "\x1b[38;2;230;245;255m",    // Crisp winter snowflake
            Self::SnowGlow => "\x1b[38;2;255;255;255m", // Crystalline specular highlight
            Self::SnowSoft => "\x1b[38;2;160;195;225m", // Twilight soft snow flurry
            Self::Splash => "\x1b[38;2;180;225;255m",  // Impact water spray
        }
    }
}

#[derive(Clone, Copy, Debug)]
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

    fn rows(&self) -> String {
        let mut output = String::new();
        for row in self.cells.chunks(self.width) {
            let mut ink = None;
            for cell in row {
                if Some(cell.ink) != ink {
                    output.push_str(cell.ink.ansi());
                    ink = Some(cell.ink);
                }
                output.push(cell.glyph);
            }
            output.push_str("\x1b[0m\x1b[K\n");
        }
        output.pop();
        output
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
}

impl Renderer {
    pub fn new(width: i16, height: i16) -> Self {
        Self {
            width: width.max(1),
            height: height.max(1),
            weather: WeatherMode::Rain,
        }
    }

    pub fn with_weather(mut self, weather: WeatherMode) -> Self {
        self.weather = weather;
        self
    }

    pub fn resize(&mut self, width: i16, height: i16) {
        self.width = width.max(1);
        self.height = height.max(1);
    }

    pub fn render(&self, world: &WorldSnapshot) -> String {
        self.render_frame(world, world.tick)
    }

    pub fn render_frame(&self, world: &WorldSnapshot, frame: u64) -> String {
        let canvas = self.render_canvas(world, frame);
        let mut output = canvas.rows();
        output.push_str(&format!(
            "\n\n\x1b[38;5;252mTick {}  HP {}/{}\x1b[0m\x1b[K",
            world.tick, world.player.stats.hp, world.player.stats.max_hp
        ));
        for message in &world.messages {
            output.push_str(&format!("\n\x1b[38;5;245m{message}\x1b[0m\x1b[K"));
        }
        output
    }

    /// Full scene pipeline without the text footer, so tests can inspect cells.
    fn render_canvas(&self, world: &WorldSnapshot, frame: u64) -> Canvas {
        let width = self.width.max(1) as usize;
        let height = self.height.max(1) as usize;
        let mut canvas = Canvas::new(width, height);
        let assets = AssetLibrary::new();
        self.draw_backdrop(&mut canvas, &assets, world.player.position, frame);

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
        for (actor, label_row) in actors.iter().zip(label_rows) {
            self.draw_actor(
                &mut canvas,
                &assets,
                actor,
                world.player.position,
                label_row,
                frame,
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

    /// All light sources in world space: the player-carried light follows the
    /// camera, lanterns are fixed world fixtures.
    fn lights(camera: ascii_mmorpg_shared::Point) -> [Light; 3] {
        [
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
        ]
    }

    /// 0.0 (void) .. 1.0 (torch core): ambient moonlight plus radial falloff
    /// with a slight eastward directional bias so lit faces read brighter.
    fn light_level(wx: i16, wy: i16, camera: ascii_mmorpg_shared::Point) -> f32 {
        let mut level = 0.10;
        for light in Self::lights(camera) {
            let dist = (wx - light.x).abs() as f32 + (wy - light.y).abs() as f32;
            level += (1.0 - dist / light.radius as f32).max(0.0) * 0.9;
        }
        level += (wx - camera.x).clamp(-8, 8) as f32 * 0.012;
        level.clamp(0.0, 1.0)
    }

    fn draw_backdrop(
        &self,
        canvas: &mut Canvas,
        assets: &AssetLibrary,
        camera: ascii_mmorpg_shared::Point,
        frame: u64,
    ) {
        for y in 0..canvas.height {
            for x in 0..canvas.width {
                let wx = camera.x + x as i16 - canvas.width as i16 / 2;
                let wy = camera.y + y as i16 - canvas.height as i16 / 2;
                let patch = (wx * 17 + wy * 31).rem_euclid(23);
                let path = (wx - wy / 3).rem_euclid(13) <= 2;
                // Calibrated so the old Manhattan bands map across: core at the
                // fixture, lit within ~3 cells, meadow within ~6.
                let level = Self::light_level(wx, wy, camera);

                let (glyph, ink) = if level > 0.8 {
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

    /// Inks the weather overlay is allowed to paint. Everything else counts as
    /// stable scene content for the shimmer metric.
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

        let artwork = assets.for_entity(&actor.kind, &actor.name, frame / 8);
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
                    ':' => Ink::KnightVisor,                   // Visor eye slit
                    '@' => Ink::KnightSteel, // Plate fill: bright steel, not blue wash
                    '▓' | '8' | '[' | ']' => Ink::KnightSteel, // Gleaming plate armor
                    '|' | '<' | '>' | '(' | ')' if row <= 4 => Ink::KnightSteel,
                    '/' | '\\' if column <= 1 || column >= 5 => Ink::KnightCape, // Crimson cape edges
                    '/' | '\\' => Ink::KnightSteel, // Steel greaves / arms
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
                    '@' | 'o' | 'O' => Ink::MossBody, // Organic moss body mass
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
        let renderer = Renderer {
            width: 32,
            height: 14,
            weather: WeatherMode::Rain,
        };
        let world = scene_world();
        assert_eq!(
            renderer.render_frame(&world, 4),
            renderer.render_frame(&world, 4)
        );
    }

    #[test]
    fn scene_contains_entity_metadata_visuals_labels_bars_and_effect() {
        let renderer = Renderer {
            width: 32,
            height: 14,
            weather: WeatherMode::Rain,
        };
        let frame = renderer.render_frame(&scene_world(), 4);
        assert!(frame.contains("Ranger"));
        assert!(frame.contains("Mossling"));
        assert!(frame.contains("[########]"));
        assert!(frame.contains("[#####---]"));
        assert!(frame.contains("\x1b[38;5;153m"));
        assert!(frame.contains("\x1b[38;5;203m"));
        assert!(frame.contains("\x1b[38;5;117m"));
        assert!(frame.contains('/'));
    }

    #[test]
    fn weather_shader_snow_and_storm_modes_render_distinct_overlays() {
        let mut renderer = Renderer {
            width: 40,
            height: 18,
            weather: WeatherMode::Snow,
        };
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
        let renderer = Renderer {
            width: 32,
            height: 14,
            weather: WeatherMode::Rain,
        };
        let world = scene_world();
        let first = renderer.render_frame(&world, 0);
        let second = renderer.render_frame(&world, 1);
        assert_ne!(first, second);
        assert_eq!(world.tick, 7);
        assert_eq!(world.player.position, Point { x: 8, y: 7 });
    }

    #[test]
    fn player_camera_keeps_player_fixed_and_scrolls_mob() {
        let renderer = Renderer {
            width: 40,
            height: 20,
            weather: WeatherMode::Rain,
        };
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
        assert_eq!(anchor(&before, 'm'), (30, 10));
        assert_eq!(anchor(&after, 'm'), (29, 10));
        assert_eq!(world.entities[0].position, Point { x: 18, y: 7 });
    }

    #[test]
    fn rendering_does_not_mutate_authoritative_snapshot() {
        let renderer = Renderer {
            width: 32,
            height: 14,
            weather: WeatherMode::Rain,
        };
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
        let renderer = Renderer {
            width: 32,
            height: 14,
            weather: WeatherMode::Rain,
        };
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
        let far = Point { x: 60, y: 60 };
        let core = Renderer::light_level(-6, 2, far);
        let edge = Renderer::light_level(1, 2, far);
        assert!(core > 0.9, "lantern core {core}");
        assert!(edge < core, "edge {edge} should be dimmer than core {core}");
        let camera = Point { x: -40, y: 17 };
        assert!(Renderer::light_level(-40, 17, camera) > 0.9);
    }

    #[test]
    fn oblique_camera_shifts_world_by_exact_pixels() {
        let renderer = Renderer {
            width: 40,
            height: 20,
            weather: WeatherMode::Rain,
        };
        let origin = Point { x: 0, y: 0 };
        let before = renderer.screen_position(Point { x: -14, y: 5 }, origin);
        let after = renderer.screen_position(Point { x: -14, y: 5 }, Point { x: 1, y: 0 });
        assert_eq!((before.0 - after.0, before.1 - after.1), (1, 0));
        assert_eq!(renderer.screen_position(origin, origin), (20, 10));
    }

    #[test]
    fn scattered_props_are_world_locked_when_camera_moves() {
        let renderer = Renderer {
            width: 40,
            height: 20,
            weather: WeatherMode::Rain,
        };
        let mut world = scene_world();
        let cam_before = world.player.position;
        let first = renderer.render_canvas(&world, 4);
        crate::apply_local_command(
            &mut world,
            &ascii_mmorpg_shared::ClientCommand::new(
                8,
                ascii_mmorpg_shared::Command::Move(ascii_mmorpg_shared::Direction::East),
            ),
        );
        let cam_after = world.player.position;
        let second = renderer.render_canvas(&world, 4);
        // Actor screen zones in both frames: the player stays centered, the
        // mob shifts one column left.
        let zones = [
            renderer.screen_position(cam_before, cam_before),
            renderer.screen_position(cam_after, cam_after),
            renderer.screen_position(Point { x: 18, y: 7 }, cam_before),
            renderer.screen_position(Point { x: 18, y: 7 }, cam_after),
        ];
        let clear = |x: isize, y: isize| {
            zones
                .iter()
                .all(|(ax, ay)| (x - ax).abs() > 9 || (y - ay).abs() > 8)
        };
        let pine = (1..first.height as isize - 1)
            .flat_map(|y| (1..first.width as isize - 1).map(move |x| (x, y)))
            .map(|(x, y)| (x, y, first.get(x, y)))
            .find(|(x, y, cell)| {
                (cell.ink == Ink::PineTree || cell.ink == Ink::PineCanopy)
                    && clear(*x, *y)
                    && clear(*x - 1, *y)
            })
            .expect("a pine prop should be visible");
        let moved = second.get(pine.0 - 1, pine.1);
        assert_eq!((moved.glyph, moved.ink), (pine.2.glyph, pine.2.ink));
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
        let renderer = Renderer {
            width: 40,
            height: 20,
            weather: WeatherMode::Rain,
        };
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
            let renderer = Renderer {
                width: 80,
                height: 40,
                weather: WeatherMode::Rain,
            };
            let frame = renderer.render_frame(world, 4);
            std::fs::write(dir.join(format!("{name}.ansi.txt")), frame).unwrap();
        }
    }

    #[test]
    fn overlapping_actors_keep_distinct_label_rows() {
        let renderer = Renderer {
            width: 60,
            height: 24,
            weather: WeatherMode::Rain,
        };
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
        let renderer = Renderer {
            width: 80,
            height: 40,
            weather: WeatherMode::Rain,
        };
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
        let renderer = Renderer {
            width: 40,
            height: 20,
            weather: WeatherMode::Rain,
        };
        let world = scene_world();
        let first = renderer.render_canvas(&world, 4);
        let second = renderer.render_canvas(&world, 5);
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
}
