use ascii_mmorpg_client::{
    apply_local_command, network::ClientConnection, parse_command, sample_world, ActiveRenderer,
    PlainTerminal, Renderer, SceneRenderer, Terminal, TerminalEvent, WeatherMode,
};
use ascii_mmorpg_shared::{ServerEvent, PROTOCOL_VERSION};
use crossterm::event::{self, KeyCode, KeyEvent, KeyEventKind, MouseButton, MouseEventKind};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use std::io::{self, BufRead};
use std::time::{Duration, Instant};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|arg| arg == "--demo") {
        return run_demo_mode();
    }
    if args.iter().any(|arg| arg == "--map-demo") {
        return run_map_demo_mode();
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
    let renderer = Renderer::new(width as i16, height as i16).with_weather(WeatherMode::Rain);
    let mut world = sample_world();
    world.messages =
        vec!["Finite glyph-scene demo: authored frames, ruins, rain, and spell trail.".into()];
    for frame in 0..6 {
        world.tick = frame;
        terminal.draw(&renderer.render_frame(&world, frame))?;
    }
    Ok(())
}

fn run_map_demo_mode() -> Result<(), Box<dyn std::error::Error>> {
    let mut terminal = PlainTerminal::new();
    let (width, height) = terminal.size();
    let mut renderer = Renderer::new(width as i16, height as i16).with_weather(WeatherMode::Rain);
    renderer.set_active_modal(ascii_mmorpg_client::ActiveModal::WorldMap);
    let mut world = sample_world();
    world.messages = vec!["Veyrath ASCII atlas loaded from the political checkpoint.".into()];
    terminal.draw(&renderer.render_frame(&world, 0))?;
    Ok(())
}

fn run_wgpu_demo(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let frames = args
        .iter()
        .position(|arg| arg == "--wgpu-demo")
        .and_then(|index| args.get(index + 1))
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(10);
    // Optional render size `WxH` right after the frame count, e.g.
    // `--wgpu-demo 10 220x70`, to benchmark zoomed-out terminal grids.
    let (demo_w, demo_h) = args
        .iter()
        .position(|arg| arg == "--wgpu-demo")
        .and_then(|index| args.get(index + 2))
        .and_then(|value| {
            let (w, h) = value.split_once('x')?;
            Some((w.parse::<i16>().ok()?, h.parse::<i16>().ok()?))
        })
        .unwrap_or((80, 40));

    #[cfg(feature = "gpu")]
    {
        use ascii_mmorpg_client::gpu::GpuRenderer;
        let mut renderer = GpuRenderer::new(demo_w, demo_h, WeatherMode::Rain)
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
    let renderer = Renderer::new(width as i16, height as i16).with_weather(WeatherMode::Rain);
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
    let _raw_mode = RawModeGuard;
    crossterm::execute!(io::stdout(), event::EnableMouseCapture)?;
    let _mouse_capture = MouseCaptureGuard;
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
    let _raw_mode = RawModeGuard;
    let term = ascii_mmorpg_client::NotcursesTerminal::new()?;
    run_realtime_with_terminal(address, term, fps, weather, force_cpu)
}

struct RawModeGuard;

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        use std::io::Write;
        let _ = write!(io::stdout(), "\x1b[?25h\x1b[0m");
        let _ = io::stdout().flush();
    }
}

struct MouseCaptureGuard;

impl Drop for MouseCaptureGuard {
    fn drop(&mut self) {
        let _ = crossterm::execute!(io::stdout(), event::DisableMouseCapture);
    }
}

/// Maps held movement keys to a compass direction. `yaw` is the presentation
/// camera's horizontal orbit angle (GPU renderer); screen-up always means the
/// direction the camera faces, so the world direction rotates with the camera:
/// screen-up is `(-sin yaw, -cos yaw)` in world axes, screen-right is
/// `(cos yaw, -sin yaw)`. The result snaps to the nearest of the protocol's
/// eight directions (yaw 0 reproduces the fixed mapping: up = North).
fn movement_direction(held: [bool; 4], yaw: f32) -> Option<ascii_mmorpg_shared::Direction> {
    use ascii_mmorpg_shared::Direction::*;
    let dx = i8::from(held[3]) - i8::from(held[1]);
    let dy = i8::from(held[2]) - i8::from(held[0]);
    if dx == 0 && dy == 0 {
        return None;
    }
    let (sx, sy) = (f32::from(dx), f32::from(dy));
    // Rotate the screen-intent vector into world axes (x east, y south on the
    // world grid; North = (0, -1)). up(+screen) contributes the facing vector,
    // right contributes the camera-right vector.
    let wx = sy * yaw.sin() + sx * yaw.cos();
    let wy = sy * yaw.cos() - sx * yaw.sin();
    // Compass angle: 0 = North, positive clockwise (atan2(x, -y)).
    let dirs = [
        North, NorthEast, East, SouthEast, South, SouthWest, West, NorthWest,
    ];
    let octant = (wx.atan2(-wy) / (std::f32::consts::FRAC_PI_4)).round();
    Some(dirs[(octant as i32).rem_euclid(8) as usize])
}

fn handle_mouse_event(
    event: crossterm::event::MouseEvent,
    right_dragging: &mut bool,
    last_mouse: &mut Option<(u16, u16)>,
    renderer: &mut impl SceneRenderer,
) {
    match event.kind {
        MouseEventKind::Down(MouseButton::Right) => {
            *right_dragging = true;
            *last_mouse = Some((event.column, event.row));
        }
        MouseEventKind::Drag(MouseButton::Right) if *right_dragging => {
            let previous = last_mouse.replace((event.column, event.row));
            if let Some((old_x, old_y)) = previous {
                let dx = i32::from(event.column) - i32::from(old_x);
                let dy = i32::from(event.row) - i32::from(old_y);
                renderer.orbit_camera(
                    dx.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16,
                    dy.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16,
                );
            }
        }
        MouseEventKind::Up(MouseButton::Right) => {
            *right_dragging = false;
            *last_mouse = None;
        }
        MouseEventKind::Moved if *right_dragging => {
            let previous = last_mouse.replace((event.column, event.row));
            if let Some((old_x, old_y)) = previous {
                renderer.orbit_camera(
                    (i32::from(event.column) - i32::from(old_x))
                        .clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16,
                    (i32::from(event.row) - i32::from(old_y))
                        .clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16,
                );
            }
        }
        _ => {}
    }
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
    let render_h = height as i16;

    let mut gpu_notice: Option<String> = None;
    let mut renderer = if !force_cpu {
        #[cfg(feature = "gpu")]
        {
            match ascii_mmorpg_client::gpu::GpuRenderer::new(render_w, render_h, weather) {
                Ok(gpu_r) => ActiveRenderer::Gpu(Box::new(gpu_r)),
                Err(err) => {
                    // Route through the in-game log, not stderr: raw error
                    // text on the fullscreen terminal reads as glitch "code".
                    gpu_notice = Some(format!("GPU unavailable ({err}); CPU renderer active."));
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
    let help = match address {
        Some(address) => {
            format!("Connecting to {address}... WASD move, hold RMB + drag camera, F attack, R rest, I inv, C char, M map, P pause, Q quit.")
        }
        None => {
            "Real-time mode: WASD move, hold RMB + drag camera, F attack, R rest, I inv, C char, M map, P pause, Q quit."
                .into()
        }
    };
    world.messages = match gpu_notice {
        Some(notice) => vec![notice, help],
        None => vec![help],
    };

    // Drain any terminal capability responses or stale escape codes. Each
    // backend drains through its own input reader: notcurses owns the tty in
    // `--notcurses` mode, so crossterm must not touch stdin there.
    std::thread::sleep(Duration::from_millis(30));
    while terminal.next_event()?.is_some() {}

    terminal.draw(&renderer.render(&world))?;

    let result = realtime_loop(
        &mut terminal,
        &mut renderer,
        &mut world,
        connection.as_mut(),
        fps,
    );
    if let Some(connection) = connection.as_mut() {
        connection.close();
    }
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
    let mut right_dragging = false;
    let mut last_mouse = None;
    let mut next_move = Instant::now();
    let mut fps_count = 0u32;
    let mut fps_timer = Instant::now();
    let mut display_fps;
    let mut last_draw_ms = 0.0f64;
    'game: loop {
        let (width, height) = terminal.refresh_size();
        // Composed frames always fill the terminal exactly (scene + HUD +
        // overlay in one buffer), so no rows are reserved here.
        renderer.resize(width as i16, height as i16);
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
        while let Some(input) = terminal.next_event()? {
            let TerminalEvent::Key(KeyEvent { code, kind, .. }) = input else {
                if let TerminalEvent::Mouse(mouse) = input {
                    handle_mouse_event(mouse, &mut right_dragging, &mut last_mouse, renderer);
                }
                continue;
            };
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
                        if renderer.active_modal() == ascii_mmorpg_client::ActiveModal::Paused {
                            // Quit directly from the pause menu.
                            break 'game;
                        }
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
                    KeyCode::Char('p') | KeyCode::Char('P') => {
                        renderer.toggle_pause();
                        None
                    }
                    KeyCode::Char('f') | KeyCode::Char('F') => {
                        if renderer.is_paused() {
                            None
                        } else {
                            Some("f")
                        }
                    }
                    KeyCode::Char('r') | KeyCode::Char('R') => {
                        if renderer.is_paused() {
                            None
                        } else {
                            Some("r")
                        }
                    }
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

        if Instant::now() >= next_move {
            // Pausing freezes local input; the overlay still renders. In online
            // mode the server world keeps ticking (pause is client-side only).
            if !renderer.is_paused() {
                if let Some(direction) = movement_direction(held, renderer.camera_yaw()) {
                    tick += 1;
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
            }
            // Some backends only report presses (releases need kitty keyboard
            // protocol), so decay held keys after each consumed tick instead
            // of waiting for a release that may never arrive. Held movement
            // still works: OS key repeat re-presses faster than the 90 ms tick.
            if terminal.decay_held_keys() {
                held = [false; 4];
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
                    "Real-time mode: WASD move (camera-relative), hold RMB + drag camera, F attack, R rest, I inv, C char, M map, P pause, V weather ({weather_label}), Q quit. [{display_fps} FPS ({target_label}) | draw: {last_draw_ms:.2}ms]"
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
                std::thread::sleep(sleep_duration);
            }
            // Tail yield/spin to achieve sub-millisecond timer precision on Windows
            while frame_start.elapsed() < target_frame {
                std::thread::yield_now();
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyModifiers, MouseEvent};

    struct CameraSink {
        orbit: Vec<(i16, i16)>,
    }

    impl SceneRenderer for CameraSink {
        fn resize(&mut self, _width: i16, _height: i16) {}
        fn weather(&self) -> WeatherMode {
            WeatherMode::Rain
        }
        fn set_weather(&mut self, _weather: WeatherMode) {}
        fn render_frame(
            &mut self,
            _world: &ascii_mmorpg_shared::WorldSnapshot,
            _frame: u64,
        ) -> String {
            String::new()
        }
        fn orbit_camera(&mut self, dx: i16, dy: i16) {
            self.orbit.push((dx, dy));
        }
    }

    #[test]
    fn movement_cancels_opposites_and_preserves_each_axis() {
        for bits in 0..16 {
            let held = std::array::from_fn(|i| bits & (1 << i) != 0);
            let origin = ascii_mmorpg_shared::Point { x: 10, y: 10 };
            let mut world = sample_world();
            world.player.position = origin;
            if let Some(direction) = movement_direction(held, 0.0) {
                let command = ascii_mmorpg_shared::ClientCommand::new(
                    1,
                    ascii_mmorpg_shared::Command::Move(direction),
                );
                apply_local_command(&mut world, &command);
            }
            assert_eq!(
                world.player.position.x,
                origin.x + i16::from(held[3]) - i16::from(held[1]),
                "keys: {held:?}"
            );
            assert_eq!(
                world.player.position.y,
                origin.y + i16::from(held[2]) - i16::from(held[0]),
                "keys: {held:?}"
            );
        }
    }

    use ascii_mmorpg_shared::Direction;

    #[test]
    fn camera_yaw_zero_keeps_world_relative_mapping() {
        assert_eq!(
            movement_direction([true, false, false, false], 0.0),
            Some(Direction::North)
        );
        assert_eq!(
            movement_direction([false, false, false, true], 0.0),
            Some(Direction::East)
        );
    }

    #[test]
    fn movement_rotates_with_camera_yaw() {
        let quarter = std::f32::consts::FRAC_PI_2;
        // Camera a quarter-turn clockwise (east of the player, facing west):
        // screen-up must walk west, screen-right must walk north.
        assert_eq!(
            movement_direction([true, false, false, false], quarter),
            Some(Direction::West)
        );
        assert_eq!(
            movement_direction([false, false, false, true], quarter),
            Some(Direction::North)
        );
        // Diagonals: up+right at this yaw is northwest on screen.
        assert_eq!(
            movement_direction([true, false, false, true], quarter),
            Some(Direction::NorthWest)
        );
        // Negative yaw (camera west, facing east): up walks east, right walks
        // south (compass-right of east).
        assert_eq!(
            movement_direction([true, false, false, false], -quarter),
            Some(Direction::East)
        );
        assert_eq!(
            movement_direction([false, false, false, true], -quarter),
            Some(Direction::South)
        );
        // Eighth-turn snapping: yaw = 45 deg puts the camera at the player's
        // southeast, facing northwest — screen-up walks northwest.
        assert_eq!(
            movement_direction([true, false, false, false], std::f32::consts::FRAC_PI_4),
            Some(Direction::NorthWest)
        );
        // No keys held stays None at any yaw.
        assert_eq!(movement_direction([false; 4], quarter), None);
    }

    #[test]
    fn right_drag_orbits_only_after_a_mouse_baseline() {
        let mut sink = CameraSink { orbit: Vec::new() };
        let mut dragging = false;
        let mut last = None;
        let down = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Right),
            column: 10,
            row: 5,
            modifiers: KeyModifiers::NONE,
        };
        handle_mouse_event(down, &mut dragging, &mut last, &mut sink);
        assert!(dragging);
        assert!(sink.orbit.is_empty());

        let drag = MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Right),
            column: 14,
            row: 8,
            modifiers: KeyModifiers::NONE,
        };
        handle_mouse_event(drag, &mut dragging, &mut last, &mut sink);
        assert_eq!(sink.orbit, vec![(4, 3)]);

        let up = MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Right),
            column: 14,
            row: 8,
            modifiers: KeyModifiers::NONE,
        };
        handle_mouse_event(up, &mut dragging, &mut last, &mut sink);
        assert!(!dragging);
        assert!(last.is_none());
    }
}
