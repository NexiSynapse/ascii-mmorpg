use ascii_mmorpg_client::{
    apply_local_command, network::ClientConnection, parse_command, sample_world, PlainTerminal,
    Renderer, Terminal,
};
use ascii_mmorpg_shared::{ServerEvent, PROTOCOL_VERSION};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use std::io::{self, BufRead};
use std::time::Duration;

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
    let renderer = Renderer {
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
    let result = realtime_loop(&mut terminal, &renderer, &mut world, connection.as_mut());
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
    renderer: &Renderer,
    world: &mut ascii_mmorpg_shared::WorldSnapshot,
    mut connection: Option<&mut ClientConnection>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut tick = world.tick;
    let mut welcomed = connection.is_none();
    loop {
        let mut redraw = false;
        let mut disconnected = false;
        if let Some(connection) = connection.as_deref_mut() {
            while let Some(event) = connection.try_recv() {
                match event {
                    ascii_mmorpg_client::network::ConnectionEvent::Server(event) => {
                        match event {
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
                        }
                        redraw = true;
                    }
                    ascii_mmorpg_client::network::ConnectionEvent::Eof => {
                        world.messages.push("Server closed the connection.".into());
                        redraw = true;
                        disconnected = true;
                    }
                    ascii_mmorpg_client::network::ConnectionEvent::Error(message) => {
                        world.messages.push(format!("Network error: {message}"));
                        redraw = true;
                        disconnected = true;
                    }
                }
            }
        }
        if redraw {
            terminal.draw(&renderer.render(world))?;
        }
        if disconnected {
            return Ok(());
        }
        if connection.is_some() && !event::poll(Duration::from_millis(50))? {
            continue;
        }

        if let Event::Key(KeyEvent { code, kind, .. }) = event::read()? {
            if kind != KeyEventKind::Press {
                continue;
            }
            let input = match code {
                KeyCode::Char('q') | KeyCode::Esc => break,
                KeyCode::Char('w') | KeyCode::Up => Some("w"),
                KeyCode::Char('a') | KeyCode::Left => Some("a"),
                KeyCode::Char('s') | KeyCode::Down => Some("s"),
                KeyCode::Char('d') | KeyCode::Right => Some("d"),
                KeyCode::Char('f') => Some("f"),
                KeyCode::Char('r') => Some("r"),
                _ => None,
            };
            let Some(input) = input else { continue };
            tick += 1;
            let Some(command) = parse_command(input, tick) else {
                continue;
            };
            if let Some(connection) = connection.as_deref_mut() {
                if !welcomed {
                    world.messages = vec!["Still waiting for the server welcome.".into()];
                    terminal.draw(&renderer.render(world))?;
                    continue;
                }
                connection.send_command(&command)?;
            } else {
                apply_local_command(world, &command);
                terminal.draw(&renderer.render(world))?;
            }
        }
    }
    Ok(())
}
