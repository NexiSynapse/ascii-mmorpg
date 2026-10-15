//! Playable windowed port: a local server [`Simulation`](ascii_mmorpg_server)
//! ticks at 20 Hz inside the presenter loop, WASD/arrows move, F attacks,
//! R rests, Q/Esc quits. Snapshots map onto
//! [`bridge`](ascii_mmorpg_engine::bridge) views every frame.
//!
//! Run: `cargo run -p ascii-mmorpg-engine --features present --example play`
//! Flags: `--frames N` exits after N presented frames (automated proof);
//! `--check` runs a headless movement + combat self-test and exits (no window).
//!
//! Still loopback this slice: one local player, no TCP yet. The terminal
//! client is untouched.

#[cfg(not(feature = "present"))]
fn main() {
    eprintln!("this example needs `--features present` (winit + wgpu + fontdue)");
    std::process::exit(2);
}

#[cfg(feature = "present")]
fn main() {
    if let Err(err) = real_main() {
        eprintln!("play failed: {err}");
        std::process::exit(1);
    }
}

#[cfg(feature = "present")]
fn real_main() -> Result<(), String> {
    use ascii_mmorpg_engine::bridge::{HUD_H, LOG_H, VIEW_H, VIEW_W};
    use ascii_mmorpg_engine::present::{PresentOptions, SceneSource};
    use std::collections::HashSet;
    use std::time::Duration;
    use winit::keyboard::KeyCode;

    use ascii_mmorpg_engine::{bridge, Camera, Scene};
    use ascii_mmorpg_server::{FixedTickClock, Simulation};
    use ascii_mmorpg_shared::{
        ActorSnapshot, ClientCommand, Command, Direction, EntityId, EntityKind, Tick, WorldSnapshot,
    };

    const PLAYER_ID: EntityId = 1;
    const TICK_HZ: u64 = 20;
    /// Stall cap: a dragged window must not repay minutes of ticks in one
    /// frame (strict-scheduler backlog is dropped, matching burst coalescing).
    const MAX_TICKS_PER_FRAME: u32 = 5;

    fn actor_kind(kind: &EntityKind) -> bridge::ActorKind {
        match kind {
            EntityKind::Player => bridge::ActorKind::Player,
            EntityKind::Monster => bridge::ActorKind::Monster,
            EntityKind::Npc => bridge::ActorKind::Npc,
            EntityKind::Item => bridge::ActorKind::Item,
        }
    }

    fn scene_actor(a: &ActorSnapshot) -> bridge::SceneActor {
        bridge::SceneActor {
            x: a.position.x as i32,
            y: a.position.y as i32,
            kind: actor_kind(&a.kind),
            name: a.name.clone(),
            glyph: a.glyph,
            hp: a.stats.hp,
            max_hp: a.stats.max_hp,
            palette: None,
        }
    }

    fn scene_view_from_snapshot(snap: &WorldSnapshot) -> bridge::SceneView {
        let mut messages = snap.messages.iter();
        let zone = messages
            .next()
            .and_then(|first| first.strip_prefix("Zone: "))
            .unwrap_or("Wilderness")
            .to_string();
        bridge::SceneView {
            player: scene_actor(&snap.player),
            actors: snap.entities.iter().map(scene_actor).collect(),
            messages: messages.cloned().collect(),
            tick: snap.tick,
            zone,
        }
    }

    /// 8-way direction from held keys; opposites cancel (matches the
    /// terminal client's 16-state movement resolution).
    fn held_direction(held: &HashSet<KeyCode>) -> Option<Direction> {
        let north = held.contains(&KeyCode::KeyW) || held.contains(&KeyCode::ArrowUp);
        let south = held.contains(&KeyCode::KeyS) || held.contains(&KeyCode::ArrowDown);
        let east = held.contains(&KeyCode::KeyD) || held.contains(&KeyCode::ArrowRight);
        let west = held.contains(&KeyCode::KeyA) || held.contains(&KeyCode::ArrowLeft);
        let dx = (east as i8) - (west as i8);
        let dy = (south as i8) - (north as i8);
        match (dx, dy) {
            (0, -1) => Some(Direction::North),
            (0, 1) => Some(Direction::South),
            (1, 0) => Some(Direction::East),
            (-1, 0) => Some(Direction::West),
            (1, -1) => Some(Direction::NorthEast),
            (-1, -1) => Some(Direction::NorthWest),
            (1, 1) => Some(Direction::SouthEast),
            (-1, 1) => Some(Direction::SouthWest),
            _ => None,
        }
    }

    struct LocalGame {
        sim: Simulation,
        clock: FixedTickClock,
        last_t: f32,
        client_tick: Tick,
        held: HashSet<KeyCode>,
        pending: Vec<Command>,
    }

    impl LocalGame {
        fn new() -> Self {
            let mut sim = Simulation::demo();
            sim.connect_player(PLAYER_ID);
            Self {
                sim,
                clock: FixedTickClock::new(TICK_HZ),
                last_t: 0.0,
                client_tick: 0,
                held: HashSet::new(),
                pending: Vec::new(),
            }
        }

        fn tick_once(&mut self) {
            self.client_tick = self.client_tick.saturating_add(1);
            let mut commands = Vec::new();
            for command in self.pending.drain(..) {
                commands.push((PLAYER_ID, ClientCommand::new(self.client_tick, command)));
            }
            if let Some(direction) = held_direction(&self.held) {
                commands.push((
                    PLAYER_ID,
                    ClientCommand::new(self.client_tick, Command::Move(direction)),
                ));
            }
            self.sim.advance_tick_with(commands);
        }
    }

    impl SceneSource for LocalGame {
        fn scene_at(&mut self, t: f32) -> (Scene, Camera) {
            let dt = Duration::from_secs_f32((t - self.last_t).max(0.0));
            self.last_t = t;
            for _ in 0..self.clock.advance(dt).min(MAX_TICKS_PER_FRAME) {
                self.tick_once();
            }
            let snapshot = self.sim.snapshot_for(PLAYER_ID);
            bridge::scene_from_view(&scene_view_from_snapshot(&snapshot))
        }

        fn scene_at_size(&mut self, t: f32, width: u32, height: u32) -> (Scene, Camera) {
            let dt = Duration::from_secs_f32((t - self.last_t).max(0.0));
            self.last_t = t;
            for _ in 0..self.clock.advance(dt).min(MAX_TICKS_PER_FRAME) {
                self.tick_once();
            }
            let snapshot = self.sim.snapshot_for(PLAYER_ID);
            bridge::scene_from_view_sized(
                &scene_view_from_snapshot(&snapshot),
                width as i32,
                height as i32,
            )
        }

        fn key(&mut self, code: KeyCode, pressed: bool) {
            match code {
                KeyCode::KeyF if pressed => self.pending.push(Command::Attack),
                KeyCode::KeyR if pressed => self.pending.push(Command::Rest),
                KeyCode::KeyW
                | KeyCode::KeyA
                | KeyCode::KeyS
                | KeyCode::KeyD
                | KeyCode::ArrowUp
                | KeyCode::ArrowDown
                | KeyCode::ArrowLeft
                | KeyCode::ArrowRight => {
                    if pressed {
                        self.held.insert(code);
                    } else {
                        self.held.remove(&code);
                    }
                }
                _ => {}
            }
        }
    }

    fn run_check() -> Result<(), String> {
        // Combat path: fresh game, mossling adjacent at (3, 2).
        let mut game = LocalGame::new();
        game.key(KeyCode::KeyF, true);
        game.scene_at(0.06);
        let mossling = game
            .sim
            .snapshot_for(PLAYER_ID)
            .entities
            .into_iter()
            .find(|entity| entity.name == "Mossling")
            .ok_or("check: mossling missing from snapshot")?;
        if mossling.stats.hp >= 8 {
            return Err("check: F attack did not damage the mossling".into());
        }
        // Movement path: hold East for 12 ticks, player must advance.
        let mut game = LocalGame::new();
        game.key(KeyCode::KeyD, true);
        let (scene, camera) = game.scene_at(0.6);
        let x = game.sim.snapshot_for(PLAYER_ID).player.position.x;
        if x <= 2 {
            return Err(format!("check: held East did not move the player (x={x})"));
        }
        if camera.x != i32::from(x) - VIEW_W / 2 {
            return Err("check: camera did not follow the player".into());
        }
        if scene.partitions.len() != 3 {
            return Err("check: scene lost its HUD/world/log partitions".into());
        }
        Ok(())
    }

    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|arg| arg == "--check") {
        run_check()?;
        eprintln!("play check ok: attack damaged mossling, held key moved player");
        return Ok(());
    }
    let frames = args
        .iter()
        .position(|arg| arg == "--frames")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse::<u64>().ok());

    match ascii_mmorpg_engine::present::run_dynamic(
        LocalGame::new(),
        PresentOptions {
            title: "ASCII MMORPG — playable windowed port".into(),
            width_cells: VIEW_W as u32,
            height_cells: (HUD_H + VIEW_H + LOG_H) as u32,
            max_frames: frames,
            ..Default::default()
        },
    ) {
        Ok(presented) => {
            eprintln!("presented {presented} frames");
            std::process::exit(0);
        }
        Err(err) => Err(err),
    }
}
