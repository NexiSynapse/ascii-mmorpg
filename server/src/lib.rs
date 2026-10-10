use ascii_mmorpg_shared::protocol::Direction;
use ascii_mmorpg_shared::{
    resolve_attack, ActorSnapshot, ClientCommand, Command, EntityId, EntityKind, Point,
    ServerEvent, Stats, Tick, WorldSnapshot, PROTOCOL_VERSION,
};
use std::collections::HashMap;

pub const TICK_HZ: u64 = 20;

#[derive(Clone, Debug)]
struct Actor {
    id: EntityId,
    name: String,
    kind: EntityKind,
    position: Point,
    stats: Stats,
    glyph: char,
}

impl Actor {
    fn snapshot(&self) -> ActorSnapshot {
        ActorSnapshot {
            id: self.id,
            name: self.name.clone(),
            kind: self.kind.clone(),
            position: self.position,
            stats: self.stats.clone(),
            glyph: self.glyph,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Simulation {
    pub tick: Tick,
    players: HashMap<EntityId, Actor>,
    mobs: Vec<Actor>,
    messages: Vec<String>,
    zone: &'static str,
}

impl Simulation {
    pub fn demo() -> Self {
        let player = Actor {
            id: 1,
            name: "Adventurer".into(),
            kind: EntityKind::Player,
            position: Point { x: 2, y: 2 },
            stats: Stats::healthy(20, 6, 2),
            glyph: '@',
        };
        Self {
            tick: 0,
            players: HashMap::from([(player.id, player)]),
            mobs: vec![Actor {
                id: 2,
                name: "Mossling".into(),
                kind: EntityKind::Monster,
                position: Point { x: 3, y: 2 },
                stats: Stats::healthy(8, 3, 1),
                glyph: 'm',
            }],
            messages: vec!["Welcome to the moss-lit ruins.".into()],
            zone: "Whispering Forest",
        }
    }

    pub fn fixed_tick(&mut self, command: Option<ClientCommand>) -> Vec<ServerEvent> {
        self.fixed_tick_for(1, command)
    }

    pub fn fixed_tick_for(
        &mut self,
        player_id: EntityId,
        command: Option<ClientCommand>,
    ) -> Vec<ServerEvent> {
        self.ensure_player(player_id);
        self.tick = self.tick.saturating_add(1);
        self.messages.clear();
        if let Some(command) = command {
            if command.protocol_version != PROTOCOL_VERSION {
                return vec![ServerEvent::Error("protocol version mismatch".into())];
            }
            self.apply(player_id, command.command);
        }
        let snapshot = self.snapshot_for(player_id);
        vec![ServerEvent::Snapshot(snapshot)]
    }

    pub fn snapshot(&self) -> WorldSnapshot {
        self.snapshot_for(1)
    }

    pub fn snapshot_for(&self, player_id: EntityId) -> WorldSnapshot {
        let player = self
            .players
            .get(&player_id)
            .or_else(|| self.players.get(&1))
            .expect("demo simulation always has a player");
        WorldSnapshot {
            tick: self.tick,
            player: player.snapshot(),
            entities: self
                .players
                .values()
                .filter(|other| other.id != player.id)
                .chain(self.mobs.iter().filter(|mob| mob.stats.is_alive()))
                .map(Actor::snapshot)
                .collect(),
            messages: std::iter::once(format!("Zone: {}", self.zone))
                .chain(self.messages.iter().cloned())
                .collect(),
        }
    }

    fn ensure_player(&mut self, player_id: EntityId) {
        self.players.entry(player_id).or_insert_with(|| Actor {
            id: player_id,
            name: format!("Adventurer {player_id}"),
            kind: EntityKind::Player,
            position: Point { x: 2, y: 2 },
            stats: Stats::healthy(20, 6, 2),
            glyph: '@',
        });
    }

    fn apply(&mut self, player_id: EntityId, command: Command) {
        match command {
            Command::Move(direction) => self.move_player(player_id, direction),
            Command::Attack => self.attack(player_id),
            Command::Rest => {
                let player = self.players.get_mut(&player_id).expect("player is ensured");
                player.stats.hp = (player.stats.hp + 1).min(player.stats.max_hp);
                self.messages.push("You catch your breath.".into());
            }
            Command::Say(message) => self.messages.push(format!(
                "You: {}",
                message.chars().take(80).collect::<String>()
            )),
            Command::Cast { spell, .. } => self.messages.push(format!(
                "You cast {spell}; spell effects are queued for the next combat slice."
            )),
        }
    }

    fn move_player(&mut self, player_id: EntityId, direction: Direction) {
        let player = self.players.get_mut(&player_id).expect("player is ensured");
        player.position = player.position.step(direction);
        self.messages.push(format!(
            "You move to ({}, {}).",
            player.position.x, player.position.y
        ));
    }

    fn attack(&mut self, player_id: EntityId) {
        let player = self.players.get(&player_id).expect("player is ensured");
        let player_position = player.position;
        let attacker_stats = player.stats.clone();
        let target = self
            .mobs
            .iter_mut()
            .find(|mob| mob.stats.is_alive() && adjacent(player_position, mob.position));
        if let Some(target) = target {
            let result = resolve_attack(&attacker_stats, &mut target.stats, self.tick ^ target.id);
            self.messages.push(format!(
                "You hit {} for {} damage{}.",
                target.name,
                result.damage,
                if result.defeated { "; defeated" } else { "" }
            ));
        } else {
            self.messages.push("There is nothing in reach.".into());
        }
    }
}

fn adjacent(a: Point, b: Point) -> bool {
    // Actor positions are model anchors, not occupied pixels. The client
    // artwork has a multi-cell footprint, so two models can visibly overlap
    // while their logical anchors are equal. Treat the same anchor as a valid
    // melee hit as well as the four cardinal neighboring cells.
    (a.x - b.x).abs() + (a.y - b.y).abs() <= 1
}

#[derive(Debug)]
pub struct FixedTickClock {
    pub tick: Tick,
    accumulator: std::time::Duration,
    step: std::time::Duration,
}

impl FixedTickClock {
    pub fn new(hz: u64) -> Self {
        let micros = (1_000_000 / hz.max(1)).max(1);
        Self {
            tick: 0,
            accumulator: std::time::Duration::ZERO,
            step: std::time::Duration::from_micros(micros),
        }
    }
    pub fn advance(&mut self, elapsed: std::time::Duration) -> u32 {
        self.accumulator += elapsed;
        let mut count = 0;
        while self.accumulator >= self.step {
            self.accumulator -= self.step;
            self.tick += 1;
            count += 1;
        }
        count
    }
}

pub fn run_loopback_demo() -> Result<Vec<ServerEvent>, Box<dyn std::error::Error>> {
    use ascii_mmorpg_shared::codec::{decode_frame, encode_frame};
    let command = ClientCommand::new(0, Command::Attack);
    let bytes = encode_frame(&command)?;
    let (decoded, _) = decode_frame::<ClientCommand>(&bytes)?;
    Ok(Simulation::demo().fixed_tick(Some(decoded)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_tick_advances_at_a_constant_rate() {
        let mut clock = FixedTickClock::new(10);
        assert_eq!(clock.advance(std::time::Duration::from_millis(99)), 0);
        assert_eq!(clock.advance(std::time::Duration::from_millis(1)), 1);
        assert_eq!(clock.tick, 1);
    }

    #[test]
    fn attack_changes_snapshot_and_emits_message() {
        let mut simulation = Simulation::demo();
        let events = simulation.fixed_tick(Some(ClientCommand::new(0, Command::Attack)));
        let ServerEvent::Snapshot(snapshot) = &events[0] else {
            panic!("expected snapshot")
        };
        assert!(snapshot
            .messages
            .iter()
            .any(|message| message.contains("hit Mossling")));
        assert!(snapshot.entities[0].stats.hp < 8);
    }

    #[test]
    fn attack_hits_when_player_and_model_anchors_overlap() {
        let mut simulation = Simulation::demo();
        simulation.players.get_mut(&1).unwrap().position = Point { x: 3, y: 2 };

        let events = simulation.fixed_tick(Some(ClientCommand::new(0, Command::Attack)));
        let ServerEvent::Snapshot(snapshot) = &events[0] else {
            panic!("expected snapshot")
        };
        assert!(snapshot
            .messages
            .iter()
            .any(|message| message.contains("hit Mossling")));
        assert!(snapshot.entities[0].stats.hp < 8);
    }

    #[test]
    fn each_connected_player_gets_a_local_view_and_remote_player_entity() {
        let mut simulation = Simulation::demo();
        simulation.fixed_tick_for(
            2,
            Some(ClientCommand::new(0, Command::Move(Direction::East))),
        );

        let first_view = simulation.snapshot_for(1);
        assert_eq!(first_view.player.id, 1);
        assert_eq!(first_view.player.kind, EntityKind::Player);
        assert_eq!(first_view.entities[0].id, 2);
        assert_eq!(first_view.entities[0].kind, EntityKind::Player);
        assert_eq!(first_view.entities[0].position, Point { x: 3, y: 2 });

        let second_view = simulation.snapshot_for(2);
        assert_eq!(second_view.player.id, 2);
        assert!(second_view.entities.iter().any(|entity| entity.id == 1));
        assert!(second_view
            .entities
            .iter()
            .any(|entity| entity.kind == EntityKind::Monster));
    }
}
