use crate::protocol::LootDrop;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LootEntry {
    pub item: String,
    pub weight: u32,
    pub min: u16,
    pub max: u16,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LootTable {
    pub id: String,
    pub entries: Vec<LootEntry>,
}

pub fn roll_loot(table: &LootTable, seed: u64) -> Vec<LootDrop> {
    let total: u64 = table
        .entries
        .iter()
        .map(|entry| u64::from(entry.weight))
        .sum();
    if total == 0 || table.entries.is_empty() {
        return Vec::new();
    }
    let mut value = seed % total;
    for entry in &table.entries {
        let weight = u64::from(entry.weight);
        if value < weight {
            let span = entry.max.saturating_sub(entry.min).saturating_add(1);
            let quantity = entry.min + ((seed >> 8) as u16 % span);
            return vec![LootDrop {
                item: entry.item.clone(),
                quantity,
            }];
        }
        value -= weight;
    }
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weighted_loot_is_deterministic_and_in_range() {
        let table = LootTable {
            id: "test".into(),
            entries: vec![LootEntry {
                item: "coin".into(),
                weight: 1,
                min: 2,
                max: 4,
            }],
        };
        let drops = roll_loot(&table, 9);
        assert_eq!(drops.len(), 1);
        assert!((2..=4).contains(&drops[0].quantity));
        assert_eq!(drops, roll_loot(&table, 9));
    }
}
