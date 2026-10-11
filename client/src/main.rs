use ascii_mmorpg_client::{
    apply_local_command, network::ClientConnection, parse_command, sample_world, PlainTerminal,
    Renderer, Terminal,
};
use ascii_mmorpg_shared::{ServerEvent, PROTOCOL_VERSION};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use std::io::{self, BufRead};
use std::time::{Duration, Instant};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|arg| arg == "--demo") {
        return run_demo_mode();
    }
    if args.iter().any(|arg| arg == "--line") {
        return run_line_mode();
    }

    let address = connect_address(&args)?;
    run_realtime(address.as_deref())
}

fn run_demo_mode() -> Result<(), Box<dyn std::error::Error>> {
    let mut terminal = PlainTerminal::new();
    let (width, height) = terminal.size();
    let renderer = Renderer {
        width: width as i16,
        height: height.saturating_sub(6) as i16,
    };
    let mut world = sample_world();
    world.messages =
        vec!["Finite glyph-scene demo: authored frames, ruins, rain, and spell trail.".into()];
    for frame in 0..6 {
        world.tick = frame;
        terminal.draw(&renderer.render_frame(&world, frame))?;
    }
    Ok(())
}

fn connect_address(args: &[String]) -> Result<Option<String>, String> {
    let Some(index) = args.iter().position(|arg| arg == "--connect") else {
        return Ok(None);
    };
    args.get(index + 1)
        .filter(|address| !address.starts_with('-'))
        .cloned()
        .map(Some)
        .ok_or_else(|| "--connect requires an address, such as 127.0.0.1:4000".into())
}

fn run_line_mode() -> Result<(), Box<dyn std::error::Error>> {
    let mut terminal = PlainTerminal::new();
    let (width, height) = terminal.size();
    let renderer = Renderer {
        width: width as i16,
        height: (height.saturating_sub(5)) as i16,
    };
    let mut world = sample_world();
    terminal.draw(&renderer.render(&world))?;
    println!("Commands are local in alpha; networking is available through the server protocol.");
    for (tick, line) in io::stdin().lock().lines().enumerate() {
        let Some(command) = parse_command(&line?, tick as u64 + 1) else {
            continue;
        };
        apply_local_command(&mut world, &command);
        terminal.draw(&renderer.render(&world))?;
    }
    Ok(())
}

fn run_realtime(address: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
    let mut connection = address.map(ClientConnection::connect).transpose()?;
    let mut terminal = PlainTerminal::new();
    let (width, height) = terminal.size();
    let mut renderer = Renderer {
        width: width as i16,
        height: (height.saturating_sub(5)) as i16,
    };
    let mut world = sample_world();
    world.messages = vec![match address {
        Some(address) => {
            format!("Connecting to {address}... WASD/arrows move, F attacks, R rests, Q quits.")
        }
        None => "Real-time mode: WASD/arrows move, F attacks, R rests, Q quits.".into(),
    }];
    terminal.draw(&renderer.render(&world))?;

    enable_raw_mode()?;
    let result = realtime_loop(
        &mut terminal,
        &mut renderer,
        &mut world,
        connection.as_mut(),
    );
    let disable_result = disable_raw_mode();
    if let Some(connection) = connection.as_mut() {
        connection.close();
    }
    println!();
    disable_result?;
    result
}

fn realtime_loop(
    terminal: &mut PlainTerminal,
    renderer: &mut Renderer,
    world: &mut ascii_mmorpg_shared::WorldSnapshot,
    mut connection: Option<&mut ClientConnection>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut tick = world.tick;
    let mut animation_frame = world.tick;
    let mut welcomed = connection.is_none();
    let mut held = [false; 4];
    let mut next_move = Instant::now();
    loop {
        let (width, height) = terminal.refresh_size();
        renderer.resize(width as i16, height.saturating_sub(5) as i16);
        let mut disconnected = false;
        if let Some(connection) = connection.as_deref_mut() {
            while let Some(event) = connection.try_recv() {
                match event {
                    ascii_mmorpg_client::network::ConnectionEvent::Server(event) => match event {
                        ServerEvent::Welcome {
                            protocol_version,
                            player_id,
                        } => {
                            if protocol_version != PROTOCOL_VERSION {
                                return Err(format!(
                                    "server protocol version {protocol_version} is not supported"
                                )
                                .into());
                            }
                            welcomed = true;
                            world.messages = vec![format!(
                                    "Connected as player {player_id}. Awaiting authoritative snapshots."
                                )];
                        }
                        ServerEvent::Snapshot(snapshot) => *world = snapshot,
                        ServerEvent::Error(message) => {
                            world.messages.push(format!("Server error: {message}"));
                        }
                        ServerEvent::Notice(message) => world.messages.push(message),
                        ServerEvent::Combat {
                            attacker,
                            defender,
                            damage,
                            defeated,
                        } => world.messages.push(format!(
                            "Combat {attacker} -> {defender}: {damage} damage{}",
                            if defeated { " (defeated)" } else { "" }
                        )),
                        ServerEvent::Loot { owner, drops } => world.messages.push(format!(
                            "Player {owner} found {} loot item(s).",
                            drops.len()
                        )),
                    },
                    ascii_mmorpg_client::network::ConnectionEvent::Eof => {
                        world.messages.push("Server closed the connection.".into());
                        disconnected = true;
                    }
                    ascii_mmorpg_client::network::ConnectionEvent::Error(message) => {
                        world.messages.push(format!("Network error: {message}"));
                        disconnected = true;
                    }
                }
            }
        }
        if disconnected {
            return Ok(());
        }
        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(KeyEvent { code, kind, .. }) = event::read()? {
                let movement = match code {
                    KeyCode::Char('w') | KeyCode::Up => Some(0),
                    KeyCode::Char('a') | KeyCode::Left => Some(1),
                    KeyCode::Char('s') | KeyCode::Down => Some(2),
                    KeyCode::Char('d') | KeyCode::Right => Some(3),
                    _ => None,
                };
                if let Some(index) = movement {
                    held[index] = kind != KeyEventKind::Release;
                } else if kind == KeyEventKind::Press {
                    let input = match code {
                        KeyCode::Char('q') | KeyCode::Esc => break,
                        KeyCode::Char('f') => Some("f"),
                        KeyCode::Char('r') => Some("r"),
                        _ => None,
                    };
                    if let Some(input) = input {
                        tick += 1;
                        if let Some(command) = parse_command(input, tick) {
                            if let Some(connection) = connection.as_deref_mut() {
                                if !welcomed {
                                    world.messages =
                                        vec!["Still waiting for the server welcome.".into()];
                                } else {
                                    connection.send_command(&command)?;
                                }
                            } else {
                                apply_local_command(world, &command);
                            }
                        }
                    }
                }
            }
        }

        if Instant::now() >= next_move {
            let vertical = if held[0] {
                Some("w")
            } else if held[2] {
                Some("s")
            } else {
                None
            };
            let horizontal = if held[1] {
                Some("a")
            } else if held[3] {
                Some("d")
            } else {
                None
            };
            let input = match (vertical, horizontal) {
                (Some("w"), Some("a")) => "nw",
                (Some("w"), Some("d")) => "ne",
                (Some("s"), Some("a")) => "sw",
                (Some("s"), Some("d")) => "se",
                (Some(input), None) | (None, Some(input)) => input,
                (Some(_), Some(_)) => "",
                (None, None) => "",
            };
            if !input.is_empty() {
                tick += 1;
                let direction = match input {
                    "nw" => ascii_mmorpg_shared::Direction::NorthWest,
                    "ne" => ascii_mmorpg_shared::Direction::NorthEast,
                    "sw" => ascii_mmorpg_shared::Direction::SouthWest,
                    "se" => ascii_mmorpg_shared::Direction::SouthEast,
                    "w" => ascii_mmorpg_shared::Direction::North,
                    "a" => ascii_mmorpg_shared::Direction::West,
                    "s" => ascii_mmorpg_shared::Direction::South,
                    "d" => ascii_mmorpg_shared::Direction::East,
                    _ => unreachable!("movement shortcut is valid"),
                };
                let command = ascii_mmorpg_shared::ClientCommand::new(
                    tick,
                    ascii_mmorpg_shared::Command::Move(direction),
                );
                if let Some(connection) = connection.as_deref_mut() {
                    if welcomed {
                        connection.send_command(&command)?;
                    }
                } else {
                    apply_local_command(world, &command);
                }
            }
            next_move = Instant::now() + Duration::from_millis(90);
        }

        animation_frame = animation_frame.wrapping_add(1);
        terminal.draw(&renderer.render_frame(world, animation_frame))?;
    }
    Ok(())
}
