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
    fn refresh_size(&mut self) -> (u16, u16) {
        self.size()
    }
    fn draw(&mut self, frame: &str) -> io::Result<()>;
}

pub struct PlainTerminal {
    width: u16,
    height: u16,
    buffer: Vec<u8>,
    prev_lines: Vec<String>,
}

impl PlainTerminal {
    pub fn new() -> Self {
        let (width, height) = crossterm::terminal::size().unwrap_or((80, 24));
        Self {
            width,
            height,
            buffer: Vec::with_capacity((width as usize) * (height as usize) * 4),
            prev_lines: Vec::new(),
        }
    }

    pub fn refresh_size(&mut self) -> (u16, u16) {
        if let Ok((width, height)) = crossterm::terminal::size() {
            if width != self.width || height != self.height {
                self.width = width;
                self.height = height;
                self.prev_lines.clear();
            }
        }
        self.size()
    }

    pub fn draw_into<W: Write>(&mut self, frame: &str, writer: &mut W) -> io::Result<()> {
        let current_lines: Vec<&str> = frame
            .split('\n')
            .map(|s| s.trim_end_matches('\r'))
            .collect();

        // If line count changed or initial frame, perform full redraw with clean clear
        if self.prev_lines.len() != current_lines.len() {
            self.buffer.clear();
            self.buffer.extend_from_slice(b"\x1b[?25l\x1b[H");
            for (i, line) in current_lines.iter().enumerate() {
                if i > 0 {
                    self.buffer.extend_from_slice(b"\r\n");
                }
                self.buffer.extend_from_slice(line.as_bytes());
                if !line.ends_with("\x1b[K") {
                    self.buffer.extend_from_slice(b"\x1b[K");
                }
            }
            self.buffer.extend_from_slice(b"\x1b[J");
            writer.write_all(&self.buffer)?;
            self.prev_lines = current_lines.iter().map(|&s| s.to_string()).collect();
            return Ok(());
        }

        // Differential row-level rendering: only transmit dirty rows!
        self.buffer.clear();
        let mut dirty_count = 0;
        let mut last_written_line: Option<usize> = None;

        for (i, (&curr, prev)) in current_lines.iter().zip(self.prev_lines.iter()).enumerate() {
            if curr != prev {
                dirty_count += 1;
                if last_written_line == Some(i.saturating_sub(1)) && i > 0 {
                    // Contiguous dirty rows: emit CRLF directly (0 coordinate jump overhead)
                    self.buffer.extend_from_slice(b"\r\n");
                } else {
                    // Non-contiguous gap: jump directly to row (i + 1), col 1
                    use std::io::Write as _;
                    write!(&mut self.buffer, "\x1b[{};1H", i + 1)?;
                }
                self.buffer.extend_from_slice(curr.as_bytes());
                if !curr.ends_with("\x1b[K") {
                    self.buffer.extend_from_slice(b"\x1b[K");
                }
                last_written_line = Some(i);
            }
        }

        if dirty_count > 0 {
            writer.write_all(&self.buffer)?;
            self.prev_lines = current_lines.iter().map(|&s| s.to_string()).collect();
        }

        Ok(())
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
    fn refresh_size(&mut self) -> (u16, u16) {
        PlainTerminal::refresh_size(self)
    }
    fn draw(&mut self, frame: &str) -> io::Result<()> {
        let mut stdout = io::stdout().lock();
        self.draw_into(frame, &mut stdout)?;
        stdout.flush()
    }
}

/// Type alias for the pure-Rust double-buffered terminal engine.
pub type DoubleBufferedTerminal = PlainTerminal;

#[cfg(feature = "notcurses")]
pub struct NotcursesTerminal {
    #[allow(dead_code)]
    context: notcurses::Notcurses,
    plane: notcurses::Plane,
    width: u16,
    height: u16,
}

#[cfg(feature = "notcurses")]
impl NotcursesTerminal {
    pub fn new() -> io::Result<Self> {
        let context = notcurses::Notcurses::new().map_err(notcurses_error)?;
        let plane = context.cli_plane().map_err(notcurses_error)?;
        let (width, height): (u32, u32) = plane.size().into();
        Ok(Self {
            context,
            plane,
            width: width.min(u16::MAX as u32) as u16,
            height: height.min(u16::MAX as u32) as u16,
        })
    }
}

#[cfg(feature = "notcurses")]
fn set_plane_channels(plane: &mut notcurses::sys::NcPlane, fg: (u8, u8, u8), bg: (u8, u8, u8)) {
    let channels = notcurses::Channels::from((fg, bg));
    notcurses::sys::c_api::ncplane_set_channels(plane, channels.nc.0);
}

#[cfg(feature = "notcurses")]
impl Terminal for NotcursesTerminal {
    fn size(&self) -> (u16, u16) {
        (self.width, self.height)
    }

    fn refresh_size(&mut self) -> (u16, u16) {
        let (width, height): (u32, u32) = self.plane.size().into();
        self.width = width.min(u16::MAX as u32) as u16;
        self.height = height.min(u16::MAX as u32) as u16;
        self.size()
    }

    fn draw(&mut self, frame: &str) -> io::Result<()> {
        let raw_plane = self.plane.into_ref_mut();
        let bg = (16u8, 16u8, 24u8);
        let default_fg = (208u8, 208u8, 208u8);
        set_plane_channels(raw_plane, default_fg, bg);

        let max_w = self.width as usize;
        let max_h = self.height as usize;
        let mut row = 0usize;

        for line in frame.lines() {
            if row >= max_h {
                break;
            }
            let mut col = 0usize;
            let mut chars = line.chars().peekable();
            let mut run = String::with_capacity(max_w);
            let mut run_start_col = 0usize;

            let flush_run = |plane: &mut notcurses::sys::NcPlane,
                             row: usize,
                             start_col: usize,
                             text: &mut String| {
                if !text.is_empty() {
                    let _ = notcurses::sys::c_api::ncplane_putstr_yx(
                        plane,
                        Some(row as u32),
                        Some(start_col as u32),
                        text,
                    );
                    text.clear();
                }
            };

            while let Some(ch) = chars.next() {
                if ch == '\x1b' && chars.peek() == Some(&'[') {
                    chars.next(); // consume '['
                    let mut seq = [0u8; 16];
                    let mut seq_len = 0;
                    while let Some(&c) = chars.peek() {
                        chars.next();
                        if c.is_ascii_alphabetic() {
                            if seq_len < seq.len() {
                                seq[seq_len] = c as u8;
                                seq_len += 1;
                            }
                            break;
                        }
                        if seq_len < seq.len() {
                            seq[seq_len] = c as u8;
                            seq_len += 1;
                        }
                    }

                    if seq_len == 2 && &seq[..2] == b"0m" {
                        flush_run(raw_plane, row, run_start_col, &mut run);
                        set_plane_channels(raw_plane, default_fg, bg);
                        run_start_col = col;
                    } else if seq_len >= 6 && &seq[..5] == b"38;5;" && seq[seq_len - 1] == b'm' {
                        if let Ok(code_str) = std::str::from_utf8(&seq[5..seq_len - 1]) {
                            if let Ok(code) = code_str.parse::<u8>() {
                                flush_run(raw_plane, row, run_start_col, &mut run);
                                set_plane_channels(raw_plane, ansi256_to_rgb(code), bg);
                                run_start_col = col;
                            }
                        }
                    }
                } else if col < max_w {
                    run.push(ch);
                    col += 1;
                }
            }

            flush_run(raw_plane, row, run_start_col, &mut run);

            // Pad remainder of the line with spaces
            if col < max_w {
                set_plane_channels(raw_plane, default_fg, bg);
                let padding = " ".repeat(max_w - col);
                let _ = notcurses::sys::c_api::ncplane_putstr_yx(
                    raw_plane,
                    Some(row as u32),
                    Some(col as u32),
                    &padding,
                );
            }
            row += 1;
        }

        // Clear any remaining lines up to screen height
        while row < max_h {
            set_plane_channels(raw_plane, default_fg, bg);
            let padding = " ".repeat(max_w);
            let _ = notcurses::sys::c_api::ncplane_putstr_yx(
                raw_plane,
                Some(row as u32),
                Some(0),
                &padding,
            );
            row += 1;
        }

        self.plane.render().map_err(notcurses_error)
    }
}

#[cfg(feature = "notcurses")]
fn ansi256_to_rgb(code: u8) -> (u8, u8, u8) {
    match code {
        0 => (0, 0, 0),
        1 => (128, 0, 0),
        2 => (0, 128, 0),
        3 => (128, 128, 0),
        4 => (0, 0, 128),
        5 => (128, 0, 128),
        6 => (0, 128, 128),
        7 => (192, 192, 192),
        8 => (128, 128, 128),
        9 => (255, 0, 0),
        10 => (0, 255, 0),
        11 => (255, 255, 0),
        12 => (0, 0, 255),
        13 => (255, 0, 255),
        14 => (0, 255, 255),
        15 => (255, 255, 255),
        16..=231 => {
            let n = code - 16;
            let r = (n / 36) % 6;
            let g = (n / 6) % 6;
            let b = n % 6;
            let step = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
            (step(r), step(g), step(b))
        }
        232..=255 => {
            let gray = 8 + (code - 232) * 10;
            (gray, gray, gray)
        }
    }
}

#[cfg(feature = "notcurses")]
fn notcurses_error(error: impl std::fmt::Display) -> io::Error {
    io::Error::other(error.to_string())
}

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
    fn local_diagonal_move_updates_both_coordinates() {
        let mut world = sample_world();
        apply_local_command(
            &mut world,
            &ClientCommand::new(1, Command::Move(Direction::NorthEast)),
        );
        assert_eq!(world.player.position, Point { x: 3, y: 1 });
    }

    #[test]
    fn local_attack_hits_adjacent_mossling() {
        let mut world = sample_world();
        world.entities[0].position = Point { x: 5, y: 2 };
        apply_local_command(&mut world, &ClientCommand::new(1, Command::Attack));
        assert!(world.entities[0].stats.hp < world.entities[0].stats.max_hp);
        assert!(world.messages[0].contains("hit Mossling"));
    }

    #[test]
    fn plain_terminal_full_redraw_then_dirty_skipping() {
        let mut term = PlainTerminal::new();
        let frame1 = "Line 1\x1b[0m\x1b[K\nLine 2\x1b[0m\x1b[K\nLine 3\x1b[0m\x1b[K";
        let mut out1 = Vec::new();
        term.draw_into(frame1, &mut out1).unwrap();
        assert!(!out1.is_empty());
        assert!(out1.starts_with(b"\x1b[?25l\x1b[H"));

        // Identical frame produces zero output
        let mut out2 = Vec::new();
        term.draw_into(frame1, &mut out2).unwrap();
        assert_eq!(out2.len(), 0);

        // Frame with only row 2 modified emits only row 2 jump and text
        let frame2 = "Line 1\x1b[0m\x1b[K\nModified 2\x1b[0m\x1b[K\nLine 3\x1b[0m\x1b[K";
        let mut out3 = Vec::new();
        term.draw_into(frame2, &mut out3).unwrap();
        let s3 = String::from_utf8(out3).unwrap();
        assert!(s3.contains("\x1b[2;1HModified 2\x1b[0m\x1b[K"));
        assert!(!s3.contains("Line 1"));
        assert!(!s3.contains("Line 3"));

        // Contiguous dirty rows coalesce with CRLF without extra CUP jump
        let frame3 = "Line 1\x1b[0m\x1b[K\nChanged 2\x1b[0m\x1b[K\nChanged 3\x1b[0m\x1b[K";
        let mut out4 = Vec::new();
        term.draw_into(frame3, &mut out4).unwrap();
        let s4 = String::from_utf8(out4).unwrap();
        assert!(s4.contains("\x1b[2;1HChanged 2\x1b[0m\x1b[K\r\nChanged 3\x1b[0m\x1b[K"));
        assert!(!s4.contains("\x1b[3;1H"));
    }
}
