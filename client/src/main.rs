use ascii_mmorpg_client::{
    apply_local_command, network::ClientConnection, parse_command, sample_world, ActiveRenderer,
    PlainTerminal, Renderer, SceneRenderer, Terminal, WeatherMode,
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
    if args.iter().any(|arg| arg == "--wgpu-demo") {
        return run_wgpu_demo(&args);
    }
    if args.iter().any(|arg| arg == "--line") {
        return run_line_mode();
    }

    let fps = target_fps(&args);
    let weather = weather_mode(&args);
    let force_cpu =
        args.iter().any(|arg| arg == "--cpu") && !args.iter().any(|arg| arg == "--wgpu");
    let address = connect_address(&args)?;
    if args.iter().any(|arg| arg == "--notcurses") {
        #[cfg(feature = "notcurses")]
        {
            return run_realtime_notcurses(address.as_deref(), fps, weather, force_cpu);
        }
        #[cfg(not(feature = "notcurses"))]
        {
            return Err(
                "--notcurses requires cargo run -p ascii-mmorpg-client --features notcurses".into(),
            );
        }
    }
    run_realtime(address.as_deref(), fps, weather, force_cpu)
}

fn weather_mode(args: &[String]) -> WeatherMode {
    if let Some(index) = args.iter().rposition(|arg| arg == "--weather") {
        if let Some(val) = args.get(index + 1) {
            match val.to_lowercase().as_str() {
                "snow" | "snowfall" => return WeatherMode::Snow,
                "storm" | "tempest" => return WeatherMode::Storm,
                "rain" => return WeatherMode::Rain,
                _ => {}
            }
        }
    }
    WeatherMode::Rain
}

fn target_fps(args: &[String]) -> u32 {
    if let Some(index) = args.iter().rposition(|arg| arg == "--fps") {
        if let Some(val) = args.get(index + 1) {
            if val == "uncapped" || val == "max" {
                return 0;
            }
            if let Ok(fps) = val.parse::<u32>() {
                return fps;
            }
        }
    }
    60
}

fn run_demo_mode() -> Result<(), Box<dyn std::error::Error>> {
    let mut terminal = PlainTerminal::new();
    let (width, height) = terminal.size();
    let renderer = Renderer::new(width as i16, height.saturating_sub(6) as i16)
        .with_weather(WeatherMode::Rain);
    let mut world = sample_world();
    world.messages =
        vec!["Finite glyph-scene demo: authored frames, ruins, rain, and spell trail.".into()];
    for frame in 0..6 {
        world.tick = frame;
        terminal.draw(&renderer.render_frame(&world, frame))?;
    }
    Ok(())
}

fn run_wgpu_demo(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let frames = args
        .iter()
        .position(|arg| arg == "--wgpu-demo")
        .and_then(|index| args.get(index + 1))
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(10);

    #[cfg(feature = "gpu")]
    {
        use ascii_mmorpg_client::gpu::GpuRenderer;
        let mut renderer = GpuRenderer::new(80, 40, WeatherMode::Rain)
            .map_err(|e| format!("wgpu init failed: {e}"))?;
        let adapter_info = renderer.ctx.adapter.get_info();
        eprintln!(
            "adapter: {} (backend: {:?}, device_type: {:?})",
            adapter_info.name, adapter_info.backend, adapter_info.device_type,
        );
        let world = sample_world();
        let started = Instant::now();
        let mut last = String::new();
        for frame in 0..frames.max(1) {
            last = renderer.render_frame(&world, frame);
        }
        let elapsed = started.elapsed();
        let ms = elapsed.as_secs_f64() * 1000.0 / frames.max(1) as f64;
        let fps = if ms > 0.0 { 1000.0 / ms } else { 0.0 };
        println!("{last}");
        eprintln!(
            "wgpu benchmark: {} frames rendered in {:.2?} ({:.2} ms/frame, {:.1} FPS equivalent)",
            frames.max(1),
            elapsed,
            ms,
            fps,
        );
        Ok(())
    }
    #[cfg(not(feature = "gpu"))]
    {
        Err("--wgpu-demo requires cargo run -p ascii-mmorpg-client --features gpu".into())
    }
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
    let renderer = Renderer::new(width as i16, (height.saturating_sub(5)) as i16)
        .with_weather(WeatherMode::Rain);
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

fn run_realtime(
    address: Option<&str>,
    fps: u32,
    weather: WeatherMode,
    force_cpu: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    enable_raw_mode()?;
    run_realtime_with_terminal(address, PlainTerminal::new(), fps, weather, force_cpu)
}

#[cfg(feature = "notcurses")]
fn run_realtime_notcurses(
    address: Option<&str>,
    fps: u32,
    weather: WeatherMode,
    force_cpu: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    enable_raw_mode()?;
    let term = ascii_mmorpg_client::NotcursesTerminal::new()?;
    run_realtime_with_terminal(address, term, fps, weather, force_cpu)
}

fn run_realtime_with_terminal<T: Terminal>(
    address: Option<&str>,
    mut terminal: T,
    fps: u32,
    weather: WeatherMode,
    force_cpu: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut connection = address.map(ClientConnection::connect).transpose()?;
    let (width, height) = terminal.size();
    let render_w = width as i16;
    let render_h = height.saturating_sub(5) as i16;

    let mut renderer = if !force_cpu {
        #[cfg(feature = "gpu")]
        {
            match ascii_mmorpg_client::gpu::GpuRenderer::new(render_w, render_h, weather) {
                Ok(gpu_r) => ActiveRenderer::Gpu(Box::new(gpu_r)),
                Err(err) => {
                    eprintln!("[WGPU] GPU offscreen init failed ({err}). Falling back to CPU.");
                    ActiveRenderer::Cpu(Renderer::new(render_w, render_h).with_weather(weather))
                }
            }
        }
        #[cfg(not(feature = "gpu"))]
        ActiveRenderer::Cpu(Renderer::new(render_w, render_h).with_weather(weather))
    } else {
        ActiveRenderer::Cpu(Renderer::new(render_w, render_h).with_weather(weather))
    };

    let mut world = sample_world();
    world.messages = vec![match address {
        Some(address) => {
            format!("Connecting to {address}... WASD move, F attack, R rest, I inv, C char, M map, Q quit.")
        }
        None => "Real-time mode: WASD move, F attack, R rest, I inv, C char, M map, Q quit.".into(),
    }];

    // Drain any terminal capability responses or stale escape codes from stdin
    std::thread::sleep(Duration::from_millis(30));
    while event::poll(Duration::ZERO)? {
        let _ = event::read()?;
    }

    terminal.draw(&renderer.render(&world))?;

    let result = realtime_loop(
        &mut terminal,
        &mut renderer,
        &mut world,
        connection.as_mut(),
        fps,
    );
    let disable_result = disable_raw_mode();
    if let Some(connection) = connection.as_mut() {
        connection.close();
    }
    use std::io::Write;
    println!("\x1b[?25h\x1b[0m");
    let _ = io::stdout().flush();
    disable_result?;
    result
}

fn realtime_loop(
    terminal: &mut impl Terminal,
    renderer: &mut impl SceneRenderer,
    world: &mut ascii_mmorpg_shared::WorldSnapshot,
    mut connection: Option<&mut ClientConnection>,
    target_fps: u32,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut tick = world.tick;
    let mut animation_frame = world.tick;
    let mut welcomed = connection.is_none();
    let mut held = [false; 4];
    let mut next_move = Instant::now();
    let mut fps_count = 0u32;
    let mut fps_timer = Instant::now();
    let mut display_fps;
    let mut last_draw_ms = 0.0f64;
    'game: loop {
        let (width, height) = terminal.refresh_size();
        let hud_reserve = if renderer.active_modal() != ascii_mmorpg_client::ActiveModal::None {
            10
        } else {
            5
        };
        renderer.resize(width as i16, height.saturating_sub(hud_reserve) as i16);
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
        let frame_start = Instant::now();
        while event::poll(Duration::ZERO)? {
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
                        KeyCode::Char('q') | KeyCode::Char('Q') => {
                            if renderer.active_modal() != ascii_mmorpg_client::ActiveModal::None {
                                renderer.set_active_modal(ascii_mmorpg_client::ActiveModal::None);
                                None
                            } else {
                                break 'game;
                            }
                        }
                        KeyCode::Esc => {
                            if renderer.active_modal() != ascii_mmorpg_client::ActiveModal::None {
                                renderer.set_active_modal(ascii_mmorpg_client::ActiveModal::None);
                                None
                            } else {
                                break 'game;
                            }
                        }
                        KeyCode::Char('i') | KeyCode::Char('I') => {
                            renderer.toggle_modal(ascii_mmorpg_client::ActiveModal::Inventory);
                            None
                        }
                        KeyCode::Char('c') | KeyCode::Char('C') => {
                            renderer.toggle_modal(ascii_mmorpg_client::ActiveModal::CharacterSheet);
                            None
                        }
                        KeyCode::Char('m') | KeyCode::Char('M') => {
                            renderer.toggle_modal(ascii_mmorpg_client::ActiveModal::WorldMap);
                            None
                        }
                        KeyCode::Char('f') | KeyCode::Char('F') => Some("f"),
                        KeyCode::Char('r') | KeyCode::Char('R') => Some("r"),
                        KeyCode::Char('v')
                        | KeyCode::Char('t')
                        | KeyCode::Char('V')
                        | KeyCode::Char('T') => {
                            let next_weather = match renderer.weather() {
                                WeatherMode::Rain => WeatherMode::Snow,
                                WeatherMode::Snow => WeatherMode::Storm,
                                WeatherMode::Storm => WeatherMode::Rain,
                            };
                            renderer.set_weather(next_weather);
                            let weather_label = match next_weather {
                                WeatherMode::Rain => "Rain",
                                WeatherMode::Snow => "Snowfall",
                                WeatherMode::Storm => "Storm",
                            };
                            world.messages =
                                vec![format!("Weather overlay changed to {weather_label}.")];
                            None
                        }
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
        let draw_start = Instant::now();
        terminal.draw(&renderer.render_frame(world, animation_frame))?;
        let draw_ms = draw_start.elapsed().as_secs_f64() * 1000.0;
        last_draw_ms = if last_draw_ms == 0.0 {
            draw_ms
        } else {
            last_draw_ms * 0.8 + draw_ms * 0.2
        };

        fps_count += 1;
        if fps_timer.elapsed() >= Duration::from_secs(1) {
            display_fps = fps_count;
            fps_count = 0;
            fps_timer = Instant::now();
            if connection.is_none()
                && (world.messages.is_empty() || world.messages[0].starts_with("Real-time mode"))
            {
                let target_label = if target_fps == 0 {
                    "uncapped".to_string()
                } else {
                    format!("{target_fps} target")
                };
                let weather_label = match renderer.weather() {
                    WeatherMode::Rain => "Rain",
                    WeatherMode::Snow => "Snowfall",
                    WeatherMode::Storm => "Storm",
                };
                world.messages = vec![format!(
                    "Real-time mode: WASD move, F attack, R rest, I inv, C char, M map, V weather ({weather_label}), Q quit. [{display_fps} FPS ({target_label}) | draw: {last_draw_ms:.2}ms]"
                )];
            }
        }

        let target_frame = if target_fps == 0 {
            Duration::ZERO
        } else {
            Duration::from_nanos(1_000_000_000 / target_fps as u64)
        };

        let frame_elapsed = frame_start.elapsed();
        if frame_elapsed < target_frame {
            let wait = target_frame - frame_elapsed;
            // Sleep for the bulk of the wait to avoid pegging CPU cores
            if wait > Duration::from_millis(6) {
                let sleep_duration = wait - Duration::from_millis(4);
                let _ = event::poll(sleep_duration)?;
            }
            // Tail yield/spin to achieve sub-millisecond timer precision on Windows
            while frame_start.elapsed() < target_frame {
                if event::poll(Duration::ZERO)? {
                    break;
                }
                std::thread::yield_now();
            }
        }
    }
    Ok(())
}
