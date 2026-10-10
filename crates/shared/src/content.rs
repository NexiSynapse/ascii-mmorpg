//! Data-only schemas shared by the content validator and the runtime.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SpellDef {
    pub id: String,
    pub name: String,
    pub cost: u16,
    pub cooldown_ticks: u16,
    pub range: u16,
    pub damage: i32,
    pub radius: u16,
    pub visual: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ItemDef {
    pub id: String,
    pub name: String,
    pub slot: String,
    pub rarity: String,
    pub glyph: char,
    pub power: i32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ZoneDef {
    pub id: String,
    pub name: String,
    pub width: u16,
    pub height: u16,
    pub spawn: (i16, i16),
}
