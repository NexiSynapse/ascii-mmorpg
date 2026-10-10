//! Wire types and deterministic game rules shared by every executable.

pub mod codec;
pub mod combat;
pub mod content;
pub mod loot;
pub mod protocol;

pub use combat::{resolve_attack, CombatResult};
pub use content::{ItemDef, SpellDef, ZoneDef};
pub use loot::{roll_loot, LootEntry, LootTable};
pub use protocol::*;
