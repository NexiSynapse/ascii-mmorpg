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
    Player,
    Monster,
    Npc,
    Item,
    Spell,
    Text,
    Muted,
}

impl Ink {
    const fn ansi(self) -> &'static str {
        match self {
            Self::Background => "\x1b[38;5;236m",
            Self::Stone => "\x1b[38;5;101m",
            Self::Moss => "\x1b[38;5;65m",
            Self::Foliage => "\x1b[38;5;22m",
            Self::Rain => "\x1b[38;5;67m",
            Self::Player => "\x1b[38;5;117m",
            Self::Monster => "\x1b[38;5;203m",
            Self::Npc => "\x1b[38;5;221m",
            Self::Item => "\x1b[38;5;220m",
            Self::Spell => "\x1b[38;5;183m",
            Self::Text => "\x1b[38;5;252m",
            Self::Muted => "\x1b[38;5;245m",
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
            output.push_str("\x1b[0m\n");
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
    pub fn render(&self, world: &WorldSnapshot) -> String {
        self.render_frame(world, world.tick)
    }

    pub fn render_frame(&self, world: &WorldSnapshot, frame: u64) -> String {
        let width = self.width.max(1) as usize;
        let height = self.height.max(1) as usize;
        let mut canvas = Canvas::new(width, height);
        let assets = AssetLibrary::new();
        self.draw_backdrop(&mut canvas, &assets, frame);

        let actors = world
            .entities
            .iter()
            .chain(std::iter::once(&world.player))
            .collect::<Vec<_>>();
        self.draw_effects(&mut canvas, &assets, world, &actors, frame);
        for actor in &actors {
            self.draw_actor(&mut canvas, &assets, actor, frame);
        }

        let mut output = canvas.rows();
        output.push_str(&format!(
            "\n\n\x1b[38;5;252mTick {}  HP {}/{}\x1b[0m",
            world.tick, world.player.stats.hp, world.player.stats.max_hp
        ));
        for message in &world.messages {
            output.push_str(&format!("\n\x1b[38;5;245m{message}\x1b[0m"));
        }
        output
    }

    fn draw_backdrop(&self, canvas: &mut Canvas, assets: &AssetLibrary, frame: u64) {
        let horizon = (canvas.height * 2 / 3).max(1);
        for y in 0..canvas.height {
            for x in 0..canvas.width {
                let (glyph, ink) = if y < horizon {
                    if (x * 17 + y * 31 + frame as usize / 3) % 29 == 0 {
                        ('+', Ink::Muted)
                    } else {
                        (' ', Ink::Background)
                    }
                } else if y == horizon {
                    ('=', Ink::Moss)
                } else if (x + y) % 5 == 0 {
                    ('.', Ink::Stone)
                } else {
                    ('·', Ink::Moss)
                };
                canvas.set(x as isize, y as isize, glyph, ink);
            }
        }

        // Authored props keep the backdrop readable while allowing the same
        // viewport to suggest either a forest path or moss-lit ruins.
        for x in (2..canvas.width).step_by(13) {
            self.draw_asset(
                canvas,
                assets.prop(PropKind::Tree, frame / 4),
                x as isize,
                horizon.saturating_sub(5) as isize,
                Ink::Foliage,
            );
        }
        for x in (8..canvas.width).step_by(19) {
            self.draw_asset(
                canvas,
                assets.prop(PropKind::Ruin, frame / 5),
                x as isize,
                horizon.saturating_sub(4) as isize,
                Ink::Stone,
            );
        }

        // Rain is deterministic: the frame only advances its phase and no RNG
        // state leaks into gameplay or changes the network snapshot.
        for x in 0..canvas.width {
            let drop = (x * 37 + frame as usize * 5 + 13) % (canvas.height + 7);
            if drop < horizon.saturating_sub(1) {
                canvas.set(x as isize, drop as isize, '/', Ink::Rain);
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

        let origin = world.player.position;
        let steps = (target.x - origin.x).abs().max((target.y - origin.y).abs());
        for step in 1..steps {
            let x = origin.x + (target.x - origin.x) * step / steps;
            let y = origin.y + (target.y - origin.y) * step / steps;
            let glyph = assets.spell_glyph(frame + step as u64);
            canvas.set(x as isize, y as isize, glyph, Ink::Spell);
        }
        canvas.set(
            target.x as isize,
            target.y as isize,
            assets.spell_glyph(frame + 3),
            Ink::Spell,
        );
    }

    fn draw_actor(
        &self,
        canvas: &mut Canvas,
        assets: &AssetLibrary,
        actor: &ActorSnapshot,
        frame: u64,
    ) {
        let bob = ((frame + actor.id + 1) % 2) as isize;
        let x = actor.position.x as isize;
        let y = actor.position.y as isize - bob;
        let ink = match actor.kind {
            EntityKind::Player => Ink::Player,
            EntityKind::Monster => Ink::Monster,
            EntityKind::Npc => Ink::Npc,
            EntityKind::Item => Ink::Item,
        };

        let artwork = assets.for_entity(&actor.kind, &actor.name, frame + actor.id);
        let artwork_width = artwork
            .lines()
            .map(|line| line.trim_end_matches('\r').chars().count())
            .max()
            .unwrap_or(1) as isize;
        self.draw_asset(canvas, artwork, x - artwork_width / 2, y - 2, ink);
        // Keep the server-provided glyph visible as a compact identity cue,
        // while the authored asset supplies the silhouette around it.
        canvas.set(x, y, actor.glyph, ink);

        let label_x = x - actor.name.chars().count() as isize / 2;
        canvas.text(label_x, y - 4, &actor.name, Ink::Text);
        let max_hp = actor.stats.max_hp.max(1);
        let hp = actor.stats.hp.clamp(0, max_hp);
        let filled = (hp * 8 / max_hp) as usize;
        let bar = format!("[{}{}]", "#".repeat(filled), "-".repeat(8 - filled));
        canvas.text(x - 5, y - 3, &bar, Ink::Muted);
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
        assert!(frame.contains("\x1b[38;5;117m"));
        assert!(frame.contains("\x1b[38;5;203m"));
        assert!(frame.contains("\x1b[38;5;183m"));
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
}
