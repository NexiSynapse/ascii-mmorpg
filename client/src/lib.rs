use ascii_mmorpg_shared::{
    in_melee_range, resolve_attack, ActorSnapshot, ClientCommand, Command, Direction, EntityKind,
    Point, Stats, WorldSnapshot,
};
use std::io::{self, Write};

pub mod assets;
pub mod camera;
#[cfg(feature = "gpu")]
pub mod gpu;
pub mod network;
pub mod partitions;
pub mod renderer;
pub mod term;
#[cfg(any(feature = "notcurses", test))]
mod terminal_colors;
pub mod ui;
pub mod veyrath;
pub use partitions::{FontProfile, GameState, Partitions};
pub use renderer::{Renderer, WeatherMode};
pub use term::{
    Cell, CellMatrix, EncoderStrategy, FrameStats, SpanDiffEncoder, WinTimerPrecisionGuard,
};
pub use ui::{ActiveModal, RatatuiHud};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminalEvent {
    Key(crossterm::event::KeyEvent),
    Mouse(crossterm::event::MouseEvent),
}

pub trait SceneRenderer {
    fn resize(&mut self, width: i16, height: i16);
    fn weather(&self) -> WeatherMode;
    fn set_weather(&mut self, weather: WeatherMode);
    fn active_modal(&self) -> ActiveModal {
        ActiveModal::None
    }
    fn set_active_modal(&mut self, modal: ActiveModal) {
        let _ = modal;
    }
    fn toggle_modal(&mut self, modal: ActiveModal) {
        if self.active_modal() == modal {
            self.set_active_modal(ActiveModal::None);
        } else {
            self.set_active_modal(modal);
        }
    }
    fn is_paused(&self) -> bool {
        self.active_modal() == ActiveModal::Paused
    }
    /// Toggles the pause overlay (`P`). Any other open modal is replaced by
    /// pause; toggling while paused resumes play.
    fn toggle_pause(&mut self) {
        if self.is_paused() {
            self.set_active_modal(ActiveModal::None);
        } else {
            self.set_active_modal(ActiveModal::Paused);
        }
    }
    fn render(&mut self, world: &WorldSnapshot) -> String {
        self.render_frame(world, world.tick)
    }
    fn render_frame(&mut self, world: &WorldSnapshot, frame: u64) -> String;
    /// Rotates the presentation camera. CPU rendering intentionally ignores
    /// this because it has no 3D camera; GPU mode implements the orbit.
    fn orbit_camera(&mut self, _dx: i16, _dy: i16) {}
    fn reset_camera(&mut self) {}
    /// Current horizontal orbit angle, radians (0 = default view). Renderers
    /// without an orbit camera report 0.0, so movement stays world-relative.
    /// Used to steer WASD input relative to what the camera faces.
    fn camera_yaw(&self) -> f32 {
        0.0
    }
}

impl SceneRenderer for Renderer {
    fn resize(&mut self, width: i16, height: i16) {
        self.resize(width, height);
    }
    fn weather(&self) -> WeatherMode {
        self.weather
    }
    fn set_weather(&mut self, weather: WeatherMode) {
        self.weather = weather;
    }
    fn active_modal(&self) -> ActiveModal {
        self.active_modal
    }
    fn set_active_modal(&mut self, modal: ActiveModal) {
        self.active_modal = modal;
    }
    fn render_frame(&mut self, world: &WorldSnapshot, frame: u64) -> String {
        Renderer::render_frame(self, world, frame)
    }
}

#[cfg(feature = "gpu")]
impl SceneRenderer for gpu::GpuRenderer {
    fn resize(&mut self, width: i16, height: i16) {
        self.resize(width, height);
    }
    fn weather(&self) -> WeatherMode {
        self.weather
    }
    fn set_weather(&mut self, weather: WeatherMode) {
        self.weather = weather;
    }
    fn active_modal(&self) -> ActiveModal {
        self.active_modal
    }
    fn set_active_modal(&mut self, modal: ActiveModal) {
        self.active_modal = modal;
    }
    fn render_frame(&mut self, world: &WorldSnapshot, frame: u64) -> String {
        self.render_frame(world, frame)
    }
    fn orbit_camera(&mut self, dx: i16, dy: i16) {
        self.orbit_camera(dx, dy);
    }
    fn reset_camera(&mut self) {
        self.reset_camera();
    }
    fn camera_yaw(&self) -> f32 {
        self.camera.yaw
    }
}

pub enum ActiveRenderer {
    Cpu(Renderer),
    #[cfg(feature = "gpu")]
    Gpu(Box<gpu::GpuRenderer>),
}

impl SceneRenderer for ActiveRenderer {
    fn resize(&mut self, width: i16, height: i16) {
        match self {
            Self::Cpu(r) => r.resize(width, height),
            #[cfg(feature = "gpu")]
            Self::Gpu(r) => r.resize(width, height),
        }
    }
    fn weather(&self) -> WeatherMode {
        match self {
            Self::Cpu(r) => r.weather(),
            #[cfg(feature = "gpu")]
            Self::Gpu(r) => r.weather(),
        }
    }
    fn set_weather(&mut self, weather: WeatherMode) {
        match self {
            Self::Cpu(r) => r.set_weather(weather),
            #[cfg(feature = "gpu")]
            Self::Gpu(r) => r.set_weather(weather),
        }
    }
    fn active_modal(&self) -> ActiveModal {
        match self {
            Self::Cpu(r) => r.active_modal(),
            #[cfg(feature = "gpu")]
            Self::Gpu(r) => r.active_modal(),
        }
    }
    fn set_active_modal(&mut self, modal: ActiveModal) {
        match self {
            Self::Cpu(r) => r.set_active_modal(modal),
            #[cfg(feature = "gpu")]
            Self::Gpu(r) => r.set_active_modal(modal),
        }
    }
    fn render_frame(&mut self, world: &WorldSnapshot, frame: u64) -> String {
        match self {
            Self::Cpu(r) => r.render_frame(world, frame),
            #[cfg(feature = "gpu")]
            Self::Gpu(r) => r.render_frame(world, frame),
        }
    }
    fn orbit_camera(&mut self, dx: i16, dy: i16) {
        match self {
            Self::Cpu(r) => r.orbit_camera(dx, dy),
            #[cfg(feature = "gpu")]
            Self::Gpu(r) => r.orbit_camera(dx, dy),
        }
    }
    fn reset_camera(&mut self) {
        match self {
            Self::Cpu(r) => r.reset_camera(),
            #[cfg(feature = "gpu")]
            Self::Gpu(r) => r.reset_camera(),
        }
    }
    fn camera_yaw(&self) -> f32 {
        match self {
            Self::Cpu(_) => 0.0,
            #[cfg(feature = "gpu")]
            Self::Gpu(r) => r.camera_yaw(),
        }
    }
}

pub trait Terminal {
    fn size(&self) -> (u16, u16);
    fn refresh_size(&mut self) -> (u16, u16) {
        self.size()
    }
    fn draw(&mut self, frame: &str) -> io::Result<()>;
    /// Non-blocking key poll. Returns the next pending key, if any.
    ///
    /// Backends that own terminal input (e.g. notcurses, which reads the tty
    /// itself) must override this — the game loop must never read stdin
    /// through a different backend than the one driving the terminal, or keys
    /// are swallowed and controls die.
    fn next_key(&mut self) -> io::Result<Option<crossterm::event::KeyEvent>> {
        use std::time::Duration;
        if crossterm::event::poll(Duration::ZERO)? {
            if let crossterm::event::Event::Key(key) = crossterm::event::read()? {
                return Ok(Some(key));
            }
        }
        Ok(None)
    }
    fn next_event(&mut self) -> io::Result<Option<TerminalEvent>> {
        Ok(self.next_key()?.map(TerminalEvent::Key))
    }
    /// Whether key releases may never arrive, so held movement keys must be
    /// decayed after each consumed move tick instead of on release.
    fn decay_held_keys(&self) -> bool {
        false
    }
}

pub struct PlainTerminal {
    width: u16,
    height: u16,
    buffer: Vec<u8>,
    /// FNV-1a hash per previously drawn line. Hash diffing avoids cloning the
    /// whole frame (~250 KB at zoomed-out grids) every frame; a 64-bit hash
    /// makes a missed update effectively impossible.
    prev_hashes: Vec<u64>,
    pub encoder: term::SpanDiffEncoder,
    _timer_guard: term::WinTimerPrecisionGuard,
}

impl PlainTerminal {
    pub fn new() -> Self {
        let (width, height) = crossterm::terminal::size().unwrap_or((80, 24));
        Self {
            width,
            height,
            buffer: Vec::with_capacity((width as usize) * (height as usize) * 4),
            prev_hashes: Vec::new(),
            encoder: term::SpanDiffEncoder::new(width, height),
            _timer_guard: term::WinTimerPrecisionGuard::new(),
        }
    }

    pub fn refresh_size(&mut self) -> (u16, u16) {
        if let Ok((width, height)) = crossterm::terminal::size() {
            if width != self.width || height != self.height {
                self.width = width;
                self.height = height;
                self.prev_hashes.clear();
                self.encoder.resize(width, height);
            }
        }
        self.size()
    }

    /// Draws a structured CellMatrix into writer using the requested encoding strategy.
    pub fn draw_matrix_into<W: Write>(
        &mut self,
        matrix: &term::CellMatrix,
        strategy: term::EncoderStrategy,
        writer: &mut W,
    ) -> io::Result<term::FrameStats> {
        let mut buf = Vec::new();
        let stats = self.encoder.encode(matrix, strategy, &mut buf);
        writer.write_all(&buf)?;
        Ok(stats)
    }

    pub fn draw_into<W: Write>(&mut self, frame: &str, writer: &mut W) -> io::Result<()> {
        let current_lines: Vec<&str> = frame
            .split('\n')
            .map(|s| s.trim_end_matches('\r'))
            .collect();

        // If line count changed or initial frame, perform full redraw with clean clear
        if self.prev_hashes.len() != current_lines.len() {
            self.buffer.clear();
            // Synchronized-update markers (DECSET 2026): Windows Terminal
            // defers painting the whole frame until the closing marker, so
            // the clean+repaint sequence can never show half-painted or
            // transitionally-black rows. Terminals without 2026 ignore the
            // private mode and behave exactly as before.
            self.buffer.extend_from_slice(b"\x1b[?2026h\x1b[?25l\x1b[H");
            // Absolute cursor addressing per row, exactly like the
            // differential branch below. The former \r\n chaining broke on
            // full-width rows: the auto-wrapped cursor + carriage return +
            // line feed advanced one row too far in Windows ConPTY
            // (historical handoff note: "\r\n landed one row too far down
            // and left stale cells"). Every relaunch and every resize runs
            // this branch, so a mis-landed full repaint was re-applied
            // deterministically and then preserved forever by the
            // differential branch, which never rewrites rows its hashes
            // consider unchanged. That was the persistent duplicated
            // panel-title ghost. Absolute jumps are immune to wrap state.
            for (i, line) in current_lines.iter().enumerate() {
                use std::io::Write as _;
                write!(&mut self.buffer, "\x1b[{};1H", i + 1)?;
                self.buffer.extend_from_slice(line.as_bytes());
                if !line.ends_with("\x1b[K") {
                    self.buffer.extend_from_slice(b"\x1b[K");
                }
            }
            self.buffer.extend_from_slice(b"\x1b[J\x1b[?2026l");
            writer.write_all(&self.buffer)?;
            self.prev_hashes = current_lines.iter().map(|line| fnv1a_hash(line)).collect();
            return Ok(());
        }

        // Differential row-level rendering: only transmit dirty rows!
        // Lines are compared by hash (single pass, no per-frame clone).
        let current_hashes: Vec<u64> = current_lines.iter().map(|line| fnv1a_hash(line)).collect();
        self.buffer.clear();
        let mut dirty_count = 0;

        for (i, ((&curr, &hash), prev_hash)) in current_lines
            .iter()
            .zip(current_hashes.iter())
            .zip(self.prev_hashes.iter())
            .enumerate()
        {
            if hash != *prev_hash {
                dirty_count += 1;
                // Always use absolute cursor positioning. The old CRLF
                // chaining assumed the previous row's cursor ended at
                // column 1, but a full-width row auto-wraps the terminal
                // cursor to the next line, so "\r\n" lands one row too far
                // down and leaves stale rows (the idle flicker). Absolute
                // jumps are immune to wrap.
                use std::io::Write as _;
                write!(&mut self.buffer, "\x1b[{};1H", i + 1)?;
                self.buffer.extend_from_slice(curr.as_bytes());
                if !curr.ends_with("\x1b[K") {
                    self.buffer.extend_from_slice(b"\x1b[K");
                }
            }
        }

        if dirty_count > 0 {
            // Wrap the dirty payload in synchronized-update markers so the
            // scattered cursor jumps + row fills commit atomically. Idle
            // torch/grass animation keeps nearly every scene row dirty each
            // frame; without 2026 Windows Terminal can paint between the
            // jumps, which read as sub-millisecond transient lines.
            let mut framed = Vec::with_capacity(self.buffer.len() + 16);
            framed.extend_from_slice(b"\x1b[?2026h");
            framed.extend_from_slice(&self.buffer);
            framed.extend_from_slice(b"\x1b[?2026l");
            writer.write_all(&framed)?;
            self.prev_hashes = current_hashes;
        }

        Ok(())
    }
}

/// FNV-1a 64-bit hash for dirty-line diffing. Single pass, no allocation.
fn fnv1a_hash(line: &str) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in line.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
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
    fn next_event(&mut self) -> io::Result<Option<TerminalEvent>> {
        use std::time::Duration;
        if crossterm::event::poll(Duration::ZERO)? {
            return Ok(match crossterm::event::read()? {
                crossterm::event::Event::Key(key) => Some(TerminalEvent::Key(key)),
                crossterm::event::Event::Mouse(mouse) => Some(TerminalEvent::Mouse(mouse)),
                _ => None,
            });
        }
        Ok(None)
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

/// Maps a notcurses input event onto the crossterm key model used by the game
/// loop, so both terminal backends share one input handler.
#[cfg(feature = "notcurses")]
fn map_notcurses_input(input: &notcurses::Input) -> Option<crossterm::event::KeyEvent> {
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
    use notcurses::{InputType, Key, Received};

    let code = match input.received {
        Received::Char(c) => KeyCode::Char(c),
        Received::Key(key) if key == Key::Up => KeyCode::Up,
        Received::Key(key) if key == Key::Down => KeyCode::Down,
        Received::Key(key) if key == Key::Left => KeyCode::Left,
        Received::Key(key) if key == Key::Right => KeyCode::Right,
        Received::Key(key) if key == Key::Esc => KeyCode::Esc,
        Received::Key(key) if key == Key::Enter => KeyCode::Enter,
        _ => return None,
    };
    let kind = match input.itype {
        InputType::Release => KeyEventKind::Release,
        InputType::Repeat => KeyEventKind::Repeat,
        // Press and Unknown (legacy terminals) both act as a fresh press.
        _ => KeyEventKind::Press,
    };
    let mut modifiers = KeyModifiers::NONE;
    if input.keymod.has_shift() {
        modifiers |= KeyModifiers::SHIFT;
    }
    if input.keymod.has_alt() {
        modifiers |= KeyModifiers::ALT;
    }
    if input.keymod.has_ctrl() {
        modifiers |= KeyModifiers::CONTROL;
    }
    if input.keymod.has_super() {
        modifiers |= KeyModifiers::SUPER;
    }
    let mut event = KeyEvent::new(code, modifiers);
    event.kind = kind;
    Some(event)
}

#[cfg(feature = "notcurses")]
impl Terminal for NotcursesTerminal {
    fn size(&self) -> (u16, u16) {
        (self.width, self.height)
    }

    /// Input must come from notcurses itself: its input layer owns the tty,
    /// so crossterm reads on stdin would starve and every key would die.
    fn next_key(&mut self) -> io::Result<Option<crossterm::event::KeyEvent>> {
        let input = self.context.poll_event().map_err(notcurses_error)?;
        Ok(map_notcurses_input(&input))
    }

    /// Releases only arrive with the kitty keyboard protocol; decay held
    /// movement keys after each consumed move tick instead.
    fn decay_held_keys(&self) -> bool {
        true
    }

    fn refresh_size(&mut self) -> (u16, u16) {
        let (width, height): (u32, u32) = self.plane.size().into();
        self.width = width.min(u16::MAX as u32) as u16;
        self.height = height.min(u16::MAX as u32) as u16;
        self.size()
    }

    fn draw(&mut self, frame: &str) -> io::Result<()> {
        use terminal_colors::{consume_escape, TerminalColors};

        let raw_plane = self.plane.into_ref_mut();
        let defaults = TerminalColors::default();
        let mut colors = defaults;
        set_plane_channels(raw_plane, colors.fg, colors.bg);

        let max_w = self.width as usize;
        let max_h = self.height as usize;
        let mut row = 0usize;

        for line in frame.lines() {
            if row >= max_h {
                break;
            }
            let mut col = 0usize;
            let mut remaining = line;
            let mut run = String::with_capacity(max_w);
            let mut run_start_col = 0usize;
            // Padding uses default channels; ANSI colors still carry across rows.
            set_plane_channels(raw_plane, colors.fg, colors.bg);

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

            while let Some(ch) = remaining.chars().next() {
                remaining = &remaining[ch.len_utf8()..];
                if ch == '\x1b' {
                    let (rest, parameters) = consume_escape(remaining);
                    remaining = rest;
                    if let Some(parameters) = parameters {
                        let next = colors.apply_sgr(parameters);
                        if next != colors {
                            flush_run(raw_plane, row, run_start_col, &mut run);
                            colors = next;
                            set_plane_channels(raw_plane, colors.fg, colors.bg);
                            run_start_col = col;
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
                set_plane_channels(raw_plane, defaults.fg, defaults.bg);
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
            set_plane_channels(raw_plane, defaults.fg, defaults.bg);
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
fn notcurses_error(error: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::Other, error.to_string())
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
            let obstacles = ascii_mmorpg_shared::overworld_obstacles();
            let outcome = ascii_mmorpg_shared::resolve_movement(
                world.player.position,
                *direction,
                &obstacles,
            );
            if outcome.blocked {
                let obstacle = outcome.obstacle.unwrap_or("an obstacle");
                world.messages.push(format!("Blocked by {obstacle}."));
            } else {
                world.player.position = outcome.resolved_position;
                world.messages.push(format!(
                    "You move to ({}, {}).",
                    world.player.position.x, world.player.position.y
                ));
            }
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
        entities: vec![
            ActorSnapshot {
                id: 2,
                name: "Mossling".into(),
                kind: EntityKind::Monster,
                position: Point { x: 3, y: 2 },
                stats: Stats::healthy(8, 3, 1),
                glyph: 'm',
            },
            ActorSnapshot {
                id: 3,
                name: "Skeleton".into(),
                kind: EntityKind::Monster,
                position: Point { x: 9, y: 7 },
                stats: Stats::healthy(10, 4, 1),
                glyph: 's',
            },
        ],
        messages: vec!["Glyph scene ready. Type w/a/s/d, f, r, or chat.".into()],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renderer_places_player_and_status() {
        let frame = Renderer::new(80, 30)
            .with_weather(WeatherMode::Rain)
            .render(&sample_world());
        assert!(frame.contains("HP 20/20"));
        // Player glyph lives in the scene partition below the 3-row top bar.
        let scene_hit = frame.lines().skip(3).any(|line| line.contains('@'));
        assert!(scene_hit, "scene partition must show the player");
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
        assert!(out1.starts_with(b"\x1b[?2026h\x1b[?25l\x1b[H"));

        // Identical frame produces zero output
        let mut out2 = Vec::new();
        term.draw_into(frame1, &mut out2).unwrap();
        assert_eq!(out2.len(), 0);

        // Frame with only row 2 modified emits only row 2 jump and text,
        // wrapped in synchronized-update markers (no mid-frame tearing)
        let frame2 = "Line 1\x1b[0m\x1b[K\nModified 2\x1b[0m\x1b[K\nLine 3\x1b[0m\x1b[K";
        let mut out3 = Vec::new();
        term.draw_into(frame2, &mut out3).unwrap();
        let s3 = String::from_utf8(out3).unwrap();
        assert!(s3.contains("\x1b[2;1HModified 2\x1b[0m\x1b[K"));
        assert!(!s3.contains("Line 1"));
        assert!(!s3.contains("Line 3"));
        assert!(s3.starts_with("\x1b[?2026h"));
        assert!(s3.ends_with("\x1b[?2026l"));

        // Contiguous dirty rows each get their own absolute cursor jump; the
        // old CRLF chaining mis-landed one row too far down on full-width
        // rows (auto-wrap), leaving stale rows.
        let frame3 = "Line 1\x1b[0m\x1b[K\nChanged 2\x1b[0m\x1b[K\nChanged 3\x1b[0m\x1b[K";
        let mut out4 = Vec::new();
        term.draw_into(frame3, &mut out4).unwrap();
        let s4 = String::from_utf8(out4).unwrap();
        assert!(s4.contains("\x1b[2;1HChanged 2\x1b[0m\x1b[K"));
        assert!(s4.contains("\x1b[3;1HChanged 3\x1b[0m\x1b[K"));
        assert!(!s4.contains("\r\n"));
    }

    #[test]
    fn full_redraw_uses_absolute_row_addressing_only() {
        // Regression for the persistent duplicated panel-title ghost: the
        // full-redraw branch (first frame, relaunch, resize) chained rows
        // with \r\n, which mis-landed one row too far down in Windows
        // ConPTY on full-width rows and permanently skewed the layout
        // (the differential branch never rewrote rows it considered
        // unchanged). Both branches must address rows absolutely.
        let mut term = PlainTerminal::new();
        let frame = "Top\x1b[0m\x1b[K\nMiddle\x1b[0m\x1b[K\nBottom\x1b[0m\x1b[K";
        let mut out = Vec::new();
        term.draw_into(frame, &mut out).unwrap();
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("\x1b[1;1HTop\x1b[0m\x1b[K"));
        assert!(s.contains("\x1b[2;1HMiddle\x1b[0m\x1b[K"));
        assert!(s.contains("\x1b[3;1HBottom\x1b[0m\x1b[K"));
        assert!(!s.contains("\r\n"));
    }

    #[test]
    fn local_move_into_obstacle_is_blocked() {
        let mut world = sample_world();
        // Place player adjacent to Timber Lantern Post at (-6, 2)
        world.player.position = Point { x: -5, y: 2 };
        let command = parse_command("a", 1).unwrap(); // Step West into lantern post
        apply_local_command(&mut world, &command);
        assert_eq!(world.player.position, Point { x: -5, y: 2 });
        assert!(world.messages[0].contains("Blocked by Timber Lantern Post"));
    }

    #[test]
    fn pause_toggle_flips_overlay_and_renders_pause_menu() {
        let mut renderer = Renderer::new(80, 30);
        assert!(!renderer.is_paused());
        renderer.toggle_pause();
        assert!(renderer.is_paused());
        assert_eq!(renderer.active_modal(), ActiveModal::Paused);
        let frame = renderer.render_frame(&sample_world(), 0);
        assert!(frame.contains("PAUSED"), "paused frame must show the menu");
        assert!(frame.contains("Resume play"));
        renderer.toggle_pause();
        assert!(!renderer.is_paused());
    }
}

#[cfg(all(test, feature = "notcurses"))]
mod notcurses_input_tests {
    use super::map_notcurses_input;
    use crossterm::event::{KeyCode, KeyEventKind};
    use notcurses::{Input, InputType, Key, KeyMod, Received};

    fn input(received: Received, itype: InputType) -> Input {
        Input {
            received,
            keymod: KeyMod::None,
            itype,
            cell: None,
            offset: None,
        }
    }

    #[test]
    fn chars_arrows_esc_and_enter_map_to_game_keys() {
        let key = map_notcurses_input(&input(Received::Char('q'), InputType::Press)).unwrap();
        assert_eq!(key.code, KeyCode::Char('q'));
        assert_eq!(key.kind, KeyEventKind::Press);
        let key = map_notcurses_input(&input(Received::Key(Key::Up), InputType::Press)).unwrap();
        assert_eq!(key.code, KeyCode::Up);
        let key = map_notcurses_input(&input(Received::Key(Key::Down), InputType::Press)).unwrap();
        assert_eq!(key.code, KeyCode::Down);
        let key = map_notcurses_input(&input(Received::Key(Key::Left), InputType::Press)).unwrap();
        assert_eq!(key.code, KeyCode::Left);
        let key = map_notcurses_input(&input(Received::Key(Key::Right), InputType::Press)).unwrap();
        assert_eq!(key.code, KeyCode::Right);
        let key = map_notcurses_input(&input(Received::Key(Key::Esc), InputType::Press)).unwrap();
        assert_eq!(key.code, KeyCode::Esc);
        let key = map_notcurses_input(&input(Received::Key(Key::Enter), InputType::Press)).unwrap();
        assert_eq!(key.code, KeyCode::Enter);
    }

    #[test]
    fn release_and_repeat_kinds_survive_mapping() {
        let key = map_notcurses_input(&input(Received::Char('w'), InputType::Release)).unwrap();
        assert_eq!(key.kind, KeyEventKind::Release);
        let key = map_notcurses_input(&input(Received::Char('w'), InputType::Repeat)).unwrap();
        assert_eq!(key.kind, KeyEventKind::Repeat);
        // Legacy terminals report Unknown; treat it as a fresh press.
        let key = map_notcurses_input(&input(Received::Char('w'), InputType::Unknown)).unwrap();
        assert_eq!(key.kind, KeyEventKind::Press);
    }

    #[test]
    fn empty_and_unbound_input_maps_to_none() {
        assert!(map_notcurses_input(&input(Received::NoInput, InputType::Unknown)).is_none());
        assert!(map_notcurses_input(&input(Received::Key(Key::PgUp), InputType::Press)).is_none());
    }
}
