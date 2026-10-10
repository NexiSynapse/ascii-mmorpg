use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u16 = 1;

pub type Tick = u64;
pub type EntityId = u64;

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct Point {
    pub x: i16,
    pub y: i16,
}

impl Point {
    pub fn step(self, direction: Direction) -> Self {
        let (dx, dy) = direction.delta();
        Self {
            x: self.x.saturating_add(dx),
            y: self.y.saturating_add(dy),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum Direction {
    North,
    South,
    East,
    West,
}

impl Direction {
    pub const fn delta(self) -> (i16, i16) {
        match self {
            Self::North => (0, -1),
            Self::South => (0, 1),
            Self::East => (1, 0),
            Self::West => (-1, 0),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum Command {
    Move(Direction),
    Attack,
    Rest,
    Say(String),
    Cast { spell: String, target: EntityId },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ClientCommand {
    pub protocol_version: u16,
    pub client_tick: Tick,
    pub command: Command,
}

impl ClientCommand {
    pub fn new(client_tick: Tick, command: Command) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            client_tick,
            command,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Stats {
    pub max_hp: i32,
    pub hp: i32,
    pub attack: i32,
    pub defense: i32,
    pub luck: u8,
}

impl Stats {
    pub fn healthy(max_hp: i32, attack: i32, defense: i32) -> Self {
        Self {
            max_hp,
            hp: max_hp,
            attack,
            defense,
            luck: 0,
        }
    }

    pub fn is_alive(&self) -> bool {
        self.hp > 0
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum EntityKind {
    Player,
    Monster,
    Npc,
    Item,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ActorSnapshot {
    pub id: EntityId,
    pub name: String,
    pub kind: EntityKind,
    pub position: Point,
    pub stats: Stats,
    pub glyph: char,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorldSnapshot {
    pub tick: Tick,
    pub player: ActorSnapshot,
    pub entities: Vec<ActorSnapshot>,
    pub messages: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LootDrop {
    pub item: String,
    pub quantity: u16,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ServerEvent {
    Welcome {
        protocol_version: u16,
        player_id: EntityId,
    },
    Snapshot(WorldSnapshot),
    Combat {
        attacker: EntityId,
        defender: EntityId,
        damage: i32,
        defeated: bool,
    },
    Loot {
        owner: EntityId,
        drops: Vec<LootDrop>,
    },
    Notice(String),
    Error(String),
}
