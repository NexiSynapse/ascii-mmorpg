use ascii_mmorpg_shared::{
    in_melee_range, resolve_attack, ActorSnapshot, ClientCommand, Command, Direction, EntityKind,
    Point, Stats, WorldSnapshot,
};
use std::io::{self, Write};

pub mod assets;
pub mod network;
pub mod renderer;
pub use renderer::Renderer;

pub trait Terminal {
    fn size(&self) -> (u16, u16);
    fn draw(&mut self, frame: &str) -> io::Result<()>;
}

pub struct PlainTerminal {
    width: u16,
    height: u16,
}

impl PlainTerminal {
    pub fn new() -> Self {
        let (width, height) = crossterm::terminal::size().unwrap_or((80, 24));
        Self { width, height }
    }

    pub fn refresh_size(&mut self) -> (u16, u16) {
        if let Ok((width, height)) = crossterm::terminal::size() {
            self.width = width;
            self.height = height;
        }
        self.size()
    }
}

impl Default for PlainTerminal {
    fn default() -> Self {
        Self::new()
    }
}

impl Terminal for PlainTerminal {
    fn size(&self) -> (u16, u16) {
        (self.width, self.height)
    }
    fn draw(&mut self, frame: &str) -> io::Result<()> {
        let mut stdout = io::stdout().lock();
        writeln!(stdout, "\x1b[2J\x1b[H{frame}")?;
        stdout.flush()
    }
}

/// Optional future notcurses adapter. The default build intentionally has no native dependency.
#[cfg(feature = "notcurses")]
pub struct NotcursesTerminal;

pub fn parse_command(line: &str, tick: u64) -> Option<ClientCommand> {
    let command = match line.trim().to_ascii_lowercase().as_str() {
        "w" | "north" => Command::Move(Direction::North),
        "s" | "south" => Command::Move(Direction::South),
        "a" | "west" => Command::Move(Direction::West),
        "d" | "east" => Command::Move(Direction::East),
        "f" | "attack" => Command::Attack,
        "r" | "rest" => Command::Rest,
        text if text.starts_with("cast ") => {
            let spell = text.trim_start_matches("cast ").trim().to_string();
            if spell.is_empty() {
                return None;
            }
            Command::Cast { spell, target: 2 }
        }
        "" => return None,
        text => Command::Say(text.to_string()),
    };
    Some(ClientCommand::new(tick, command))
}

/// Apply a command to the local fallback world so the standalone client is
/// playable while the network client is still being wired to server snapshots.
pub fn apply_local_command(world: &mut WorldSnapshot, command: &ClientCommand) {
    world.tick = command.client_tick;
    world.messages.clear();
    match &command.command {
        Command::Move(direction) => {
            world.player.position = world.player.position.step(*direction);
            world.messages.push(format!(
                "You move to ({}, {}).",
                world.player.position.x, world.player.position.y
            ));
        }
        Command::Rest => {
            world.player.stats.hp = (world.player.stats.hp + 1).min(world.player.stats.max_hp);
            world.messages.push("You catch your breath.".into());
        }
        Command::Say(message) => {
            world.messages.push(format!(
                "You: {}",
                message.chars().take(80).collect::<String>()
            ));
        }
        Command::Cast { spell, .. } => {
            world
                .messages
                .push(format!("You cast {spell}; spell effects are queued."));
        }
        Command::Attack => {
            let player_position = world.player.position;
            let attacker_stats = world.player.stats.clone();
            let target = world.entities.iter_mut().find(|actor| {
                actor.kind == EntityKind::Monster
                    && actor.stats.is_alive()
                    && in_melee_range(player_position, actor.position)
            });
            if let Some(target) = target {
                let result =
                    resolve_attack(&attacker_stats, &mut target.stats, world.tick ^ target.id);
                world.messages.push(format!(
                    "You hit {} for {} damage{}.",
                    target.name,
                    result.damage,
                    if result.defeated { "; defeated" } else { "" }
                ));
            } else {
                world.messages.push("There is nothing in reach.".into());
            }
        }
    }
}

pub fn sample_world() -> WorldSnapshot {
    WorldSnapshot {
        tick: 0,
        player: ActorSnapshot {
            id: 1,
            name: "Adventurer".into(),
            kind: EntityKind::Player,
            position: Point { x: 2, y: 2 },
            stats: Stats::healthy(20, 6, 2),
            glyph: '@',
        },
        entities: vec![ActorSnapshot {
            id: 2,
            name: "Mossling".into(),
            kind: EntityKind::Monster,
            position: Point { x: 3, y: 2 },
            stats: Stats::healthy(8, 3, 1),
            glyph: 'm',
        }],
        messages: vec!["Glyph scene ready. Type w/a/s/d, f, r, or chat.".into()],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renderer_places_player_and_status() {
        let frame = Renderer {
            width: 5,
            height: 5,
        }
        .render(&sample_world());
        assert!(frame.lines().nth(2).unwrap().contains('@'));
        assert!(frame.contains("HP 20/20"));
    }

    #[test]
    fn parser_maps_shortcuts_and_chat() {
        assert_eq!(
            parse_command("w", 2).unwrap().command,
            Command::Move(Direction::North)
        );
        assert_eq!(
            parse_command("hello", 2).unwrap().command,
            Command::Say("hello".into())
        );
        assert!(parse_command("", 2).is_none());
    }

    #[test]
    fn local_move_updates_player_position() {
        let mut world = sample_world();
        let command = parse_command("d", 1).unwrap();
        apply_local_command(&mut world, &command);
        assert_eq!(world.player.position, Point { x: 3, y: 2 });
        assert!(world.messages[0].contains("(3, 2)"));
    }

    #[test]
    fn local_attack_hits_adjacent_mossling() {
        let mut world = sample_world();
        world.entities[0].position = Point { x: 5, y: 2 };
        apply_local_command(&mut world, &ClientCommand::new(1, Command::Attack));
        assert!(world.entities[0].stats.hp < world.entities[0].stats.max_hp);
        assert!(world.messages[0].contains("hit Mossling"));
    }
}
