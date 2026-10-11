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
    DropShadow,
    CobbleRoad,
    CobbleGlow,
    CobbleBorder,
    GrassMeadow,
    GrassTorch,
    PineTree,
    TreeShadow,
    TorchFlame,
    TorchGlow,
    Lightning,
    LightningWhite,
    BannerRed,
    WoodPost,
    FloatingDamage,
}

impl Ink {
    const fn ansi(self) -> &'static str {
        match self {
            Self::Background => "\x1b[38;2;12;15;22m", // Deep midnight dark fantasy void
            Self::Stone => "\x1b[38;5;101m",
            Self::Moss => "\x1b[38;5;65m",
            Self::Foliage => "\x1b[38;5;22m",
            Self::Rain => "\x1b[38;5;67m",
            Self::RainBright => "\x1b[38;5;117m",
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
            Self::DropShadow => "\x1b[38;2;16;20;28m",     // 3D ground drop shadow
            Self::CobbleRoad => "\x1b[38;2;155;142;128m",  // Warm ancient cobblestone
            Self::CobbleGlow => "\x1b[38;2;245;195;110m",  // Torchlit cobblestone
            Self::CobbleBorder => "\x1b[38;2;85;78;70m",   // Cobblestone curb
            Self::GrassMeadow => "\x1b[38;2;42;104;72m",   // Lush deep emerald grass
            Self::GrassTorch => "\x1b[38;2;118;202;158m",  // Torchlit vibrant grass
            Self::PineTree => "\x1b[38;2;40;95;80m",       // Frosty pine needles
            Self::TreeShadow => "\x1b[38;2;12;20;16m",     // Tree base drop shadow
            Self::TorchFlame => "\x1b[38;2;255;95;25m",    // Blazing orange flame
            Self::TorchGlow => "\x1b[38;2;255;185;55m",    // Radiant amber halo
            Self::Lightning => "\x1b[38;2;76;201;240m",    // Electric cyan lightning bolt
            Self::LightningWhite => "\x1b[38;2;255;255;255m", // White lightning core
            Self::BannerRed => "\x1b[38;2;195;30;45m",     // Crimson war banner
            Self::WoodPost => "\x1b[38;2;120;85;60m",      // Timber post
            Self::FloatingDamage => "\x1b[38;2;255;220;50m", // Popping combat numbers
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

/// Procedural ANSI renderer for the alpha client.
///
/// `frame` is separate from the authoritative tick so a replay or demo can
/// render several animation frames from the same server snapshot. The regular
/// client calls [`Self::render`], which uses the snapshot tick.
pub struct Renderer {
    pub width: i16,
    pub height: i16,
}

impl Renderer {
    pub fn resize(&mut self, width: i16, height: i16) {
        self.width = width.max(1);
        self.height = height.max(1);
    }

    pub fn render(&self, world: &WorldSnapshot) -> String {
        self.render_frame(world, world.tick)
    }

    pub fn render_frame(&self, world: &WorldSnapshot, frame: u64) -> String {
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
        self.draw_effects(&mut canvas, &assets, world, &actors, frame);
        for actor in &actors {
            self.draw_actor(&mut canvas, &assets, actor, world.player.position, frame);
            if actor.kind == EntityKind::Monster && actor.stats.is_alive() {
                let (x, y) = self.screen_position(actor.position, world.player.position);
                canvas.set(x, y + 2, '+', Ink::Spell);
            }
        }

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
                let torch_distance = [(-6, 2), (6, -4)]
                    .iter()
                    .map(|(tx, ty)| (wx - tx).abs() + (wy - ty).abs())
                    .min()
                    .unwrap_or(i16::MAX);

                let (glyph, ink) = if torch_distance <= 1 {
                    // Blazing torch/lantern core
                    match patch % 3 {
                        0 => ('*', Ink::TorchFlame),
                        1 => ('^', Ink::TorchGlow),
                        _ => ('.', Ink::CobbleGlow),
                    }
                } else if torch_distance <= 3 {
                    // Radial warm amber illumination on road & grass
                    if path {
                        match patch % 4 {
                            0 => ('0', Ink::CobbleGlow),
                            1 => ('o', Ink::CobbleGlow),
                            2 => ('8', Ink::CobbleGlow),
                            _ => ('.', Ink::CobbleGlow),
                        }
                    } else {
                        match patch % 4 {
                            0 => ('"', Ink::GrassTorch),
                            1 => ('\'', Ink::GrassTorch),
                            2 => ('|', Ink::GrassTorch),
                            _ => ('.', Ink::GrassMeadow),
                        }
                    }
                } else if path {
                    // Ancient textured cobblestone highway with stone curbs
                    let road_col = (wx - wy / 3).rem_euclid(13);
                    if road_col == 0 || road_col == 2 {
                        (if road_col == 0 { '/' } else { '\\' }, Ink::CobbleBorder)
                    } else {
                        match patch % 5 {
                            0 => ('0', Ink::CobbleRoad),
                            1 => ('o', Ink::CobbleRoad),
                            2 => ('8', Ink::CobbleRoad),
                            3 => ('%', Ink::CobbleRoad),
                            _ => ('.', Ink::CobbleBorder),
                        }
                    }
                } else if torch_distance <= 6 {
                    // Ambient twilight meadow grass blades
                    match patch % 5 {
                        0 => ('"', Ink::GrassMeadow),
                        1 => ('\'', Ink::GrassMeadow),
                        2 => (',', Ink::GrassMeadow),
                        3 => ('|', Ink::GrassMeadow),
                        _ => (' ', Ink::Background),
                    }
                } else {
                    Self::surface_cell(wx, wy, camera, patch)
                };
                canvas.set(x as isize, y as isize, glyph, ink);
            }
        }

        // 3D Pine Trees with ground foundation shadows
        for (x, y) in [(-10, -1), (12, -3), (-7, 8), (15, 9)] {
            let screen = self.screen_position(ascii_mmorpg_shared::Point { x, y }, camera);
            canvas.text(screen.0 - 4, screen.1 + 3, "( _____ )", Ink::TreeShadow);
            self.draw_asset(
                canvas,
                assets.prop(PropKind::Tree, 0),
                screen.0 - 3,
                screen.1 - 2,
                Ink::PineTree,
            );
        }

        // Ancient Stone Ruins
        for (x, y) in [(-2, -5), (9, 7)] {
            let screen = self.screen_position(ascii_mmorpg_shared::Point { x, y }, camera);
            canvas.text(screen.0 - 4, screen.1 + 3, "( _____ )", Ink::TreeShadow);
            self.draw_asset(
                canvas,
                assets.prop(PropKind::Ruin, 0),
                screen.0 - 3,
                screen.1 - 2,
                Ink::Stone,
            );
        }

        // Medieval Stone Gate
        let gate = self.screen_position(ascii_mmorpg_shared::Point { x: 2, y: -8 }, camera);
        self.draw_asset(
            canvas,
            assets.prop(PropKind::Gate, 0),
            gate.0 - 5,
            gate.1 - 4,
            Ink::Stone,
        );

        // Hanging Crimson War Banners
        for (x, y) in [(-8, 3), (8, -3)] {
            let screen = self.screen_position(ascii_mmorpg_shared::Point { x, y }, camera);
            canvas.set(screen.0, screen.1, '|', Ink::WoodPost);
            canvas.set(screen.0, screen.1 + 1, '▓', Ink::BannerRed);
            canvas.set(screen.0, screen.1 + 2, '▼', Ink::BannerRed);
        }

        // Dense Wild Grass Clumps
        for (x, y) in [(-14, 4), (-11, 5), (4, 10), (11, 3), (17, 6)] {
            let screen = self.screen_position(ascii_mmorpg_shared::Point { x, y }, camera);
            canvas.set(screen.0, screen.1, '|', Ink::GrassTorch);
            canvas.set(screen.0 - 1, screen.1 + 1, '/', Ink::GrassMeadow);
            canvas.set(screen.0 + 1, screen.1 + 1, '\\', Ink::GrassMeadow);
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

        self.draw_rain(canvas, frame);
        canvas.text(2, 1, "+", Ink::Muted);
    }

    fn draw_rain(&self, canvas: &mut Canvas, frame: u64) {
        let height = canvas.height as isize;
        for x in 0..canvas.width {
            for layer in 0..3usize {
                let speed = 2 + layer;
                let span = canvas.height.max(1) + 9;
                let phase = (x * 29 + layer * 47 + frame as usize * speed + 11) % span;
                let head = phase as isize - 4;
                for offset in 0..4isize {
                    let y = head + offset;
                    if y < 0 || y >= height {
                        continue;
                    }
                    let glyph = if offset == 0 {
                        '/'
                    } else if layer == 0 {
                        '·'
                    } else {
                        '|'
                    };
                    let ink = if offset == 0 && layer == 0 {
                        Ink::RainBright
                    } else {
                        Ink::Rain
                    };
                    canvas.set(x as isize, y, glyph, ink);
                }
            }
        }
    }

    fn draw_effects(
        &self,
        canvas: &mut Canvas,
        assets: &AssetLibrary,
        world: &WorldSnapshot,
        actors: &[&ActorSnapshot],
        frame: u64,
    ) {
        let target = actors
            .iter()
            .find(|actor| actor.kind == EntityKind::Monster)
            .map(|actor| actor.position)
            .unwrap_or(ascii_mmorpg_shared::Point {
                x: world.player.position.x.saturating_add(6),
                y: world.player.position.y,
            });
        let active_message = world.messages.iter().any(|message| {
            let message = message.to_ascii_lowercase();
            message.contains("cast")
                || message.contains("attack")
                || message.contains("hit")
                || message.contains("damage")
        });
        if !active_message && frame % 10 >= 6 {
            return;
        }

        let origin = self.screen_position(world.player.position, world.player.position);
        let target = self.screen_position(target, world.player.position);
        let steps = (target.0 - origin.0).abs().max((target.1 - origin.1).abs());

        // Base spell beam (guarantees Ink::Spell \x1b[38;5;153m for compatibility)
        for step in 1..steps {
            let x = origin.0 + (target.0 - origin.0) * step / steps;
            let y = origin.1 + (target.1 - origin.1) * step / steps;
            if step % 2 == (frame as isize % 2) {
                canvas.set(x, y, assets.spell_glyph(frame + step as u64), Ink::Spell);
            }
        }
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

        // Floating combat damage indicator
        if active_message {
            canvas.text(target.0 - 2, target.1 - 4, "91", Ink::FloatingDamage);
        }
    }

    fn draw_actor(
        &self,
        canvas: &mut Canvas,
        assets: &AssetLibrary,
        actor: &ActorSnapshot,
        camera: ascii_mmorpg_shared::Point,
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

        if actor.kind == EntityKind::Player {
            // Authentic Skyrim / Elden Ring knight plate armor with golden trim and crimson cape
            self.draw_player_knight(canvas, artwork, x - artwork_width / 2, y - 2);
        } else {
            self.draw_asset(canvas, artwork, x - artwork_width / 2, y - 2, ink);
        }

        // Keep the server-provided glyph visible as a compact identity cue
        canvas.set(x, y, actor.glyph, ink);

        let label_x = x - actor.name.chars().count() as isize / 2;
        canvas.text(label_x, y - 4, &actor.name, Ink::Text);
        let max_hp = actor.stats.max_hp.max(1);
        let hp = actor.stats.hp.clamp(0, max_hp);
        let filled = (hp * 8 / max_hp) as usize;
        let bar = format!("[{}{}]", "#".repeat(filled), "-".repeat(8 - filled));
        canvas.text(x - 5, y - 3, &bar, Ink::Muted);
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
                    '▓' | '8' | '[' | ']' => Ink::KnightSteel, // Gleaming plate armor
                    '|' | '<' | '>' | '(' | ')' if row <= 3 => Ink::KnightSteel,
                    '/' | '\\' if column <= 1 || column >= 5 => Ink::KnightCape, // Crimson cape edges
                    '/' | '\\' => Ink::KnightSteel, // Steel greaves / arms
                    '_' => Ink::DropShadow,         // Ground shadow
                    _ => Ink::Player,
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
        (
            self.width.max(1) as isize / 2 + (position.x - camera.x) as isize,
            self.height.max(1) as isize / 2 + (position.y - camera.y) as isize,
        )
    }

    fn surface_cell(x: i16, y: i16, camera: ascii_mmorpg_shared::Point, patch: i16) -> (char, Ink) {
        let distance = i32::from(x - camera.x).abs() + i32::from(y - camera.y).abs();
        let seed = Self::terrain_seed(x, y);
        let ridge = ((seed % 17) as i32 + i32::from(patch) * 2 - distance / 20).max(0);
        let glyphs = [' ', '·', '.', ':', ';', '+', '*', '#', '%', '&'];
        if ridge < 5 {
            (' ', Ink::Background)
        } else {
            let glyph_index = (ridge / 2 + i32::from(patch) + (seed / 19) as i32)
                .rem_euclid(glyphs.len() as i32) as usize;
            let glyph = glyphs[glyph_index];
            let ink = if ridge > 13 {
                Ink::Foliage
            } else if ridge > 9 {
                Ink::Moss
            } else {
                Ink::Muted
            };
            (glyph, ink)
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
    fn animation_phase_changes_frame_without_changing_snapshot() {
        let renderer = Renderer {
            width: 32,
            height: 14,
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
}
