use ascii_mmorpg_shared::{ItemDef, LootEntry, LootTable, SpellDef, Stats, ZoneDef};
use serde::Deserialize;
use std::collections::HashSet;
use std::path::Path;

#[derive(Clone, Debug, Deserialize)]
pub struct MonsterDef {
    pub id: String,
    pub name: String,
    pub glyph: char,
    pub stats: Stats,
    pub loot: String,
}

pub fn validate_content(monsters_path: &Path, loot_path: &Path) -> Result<usize, String> {
    let monsters_text =
        std::fs::read_to_string(monsters_path).map_err(|error| error.to_string())?;
    let loot_text = std::fs::read_to_string(loot_path).map_err(|error| error.to_string())?;
    let monsters: Vec<MonsterDef> =
        ron::from_str(&monsters_text).map_err(|error| format!("monsters: {error}"))?;
    let loot: Vec<LootTable> =
        ron::from_str(&loot_text).map_err(|error| format!("loot: {error}"))?;
    let mut loot_ids = HashSet::new();
    for table in &loot {
        if table.id.is_empty() || !loot_ids.insert(table.id.as_str()) {
            return Err(format!("duplicate or empty loot table id: {}", table.id));
        }
    }
    let mut ids = HashSet::new();
    for monster in &monsters {
        if monster.id.is_empty() || !ids.insert(monster.id.as_str()) {
            return Err(format!("duplicate or empty monster id: {}", monster.id));
        }
        if monster.name.is_empty() || monster.glyph.is_control() {
            return Err(format!("invalid monster presentation: {}", monster.id));
        }
        if monster.stats.max_hp <= 0
            || monster.stats.hp <= 0
            || monster.stats.hp > monster.stats.max_hp
            || monster.stats.attack < 0
            || monster.stats.defense < 0
        {
            return Err(format!("invalid stats for {}", monster.id));
        }
        if !loot_ids.contains(monster.loot.as_str()) {
            return Err(format!(
                "monster {} references missing loot table {}",
                monster.id, monster.loot
            ));
        }
    }
    for table in &loot {
        if table.entries.is_empty() {
            return Err(format!("loot table {} is empty", table.id));
        }
        for entry in &table.entries {
            validate_entry(entry).map_err(|error| format!("loot {}: {error}", table.id))?;
        }
    }
    Ok(monsters.len() + loot.len())
}

pub fn validate_alpha_content(root: &Path) -> Result<usize, String> {
    let base = validate_content(&root.join("monsters.ron"), &root.join("loot.ron"))?;
    let spells: Vec<SpellDef> = parse(root, "spells.ron")?;
    let items: Vec<ItemDef> = parse(root, "items.ron")?;
    let zones: Vec<ZoneDef> = parse(root, "zones.ron")?;
    if spells.len() != 3 {
        return Err(format!(
            "expected three alpha spells, found {}",
            spells.len()
        ));
    }
    if items.is_empty() || zones.len() < 2 {
        return Err("alpha needs items and at least two zones".into());
    }
    if spells
        .iter()
        .any(|spell| spell.id.is_empty() || spell.cooldown_ticks == 0 || spell.damage < 0)
    {
        return Err("invalid spell definition".into());
    }
    if zones
        .iter()
        .any(|zone| zone.width == 0 || zone.height == 0 || zone.name.is_empty())
    {
        return Err("invalid zone definition".into());
    }
    Ok(base + spells.len() + items.len() + zones.len())
}

fn parse<T: for<'de> Deserialize<'de>>(root: &Path, file: &str) -> Result<T, String> {
    let text =
        std::fs::read_to_string(root.join(file)).map_err(|error| format!("{file}: {error}"))?;
    ron::from_str(&text).map_err(|error| format!("{file}: {error}"))
}

fn validate_entry(entry: &LootEntry) -> Result<(), &'static str> {
    if entry.item.is_empty() {
        return Err("item is empty");
    }
    if entry.weight == 0 {
        return Err("weight must be positive");
    }
    if entry.min == 0 || entry.min > entry.max {
        return Err("invalid quantity range");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn validates_checked_in_content() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../content");
        assert!(validate_content(&root.join("monsters.ron"), &root.join("loot.ron")).is_ok());
        assert!(validate_alpha_content(&root).is_ok());
    }

    #[test]
    fn rejects_missing_loot_reference() {
        let dir = std::env::temp_dir().join(format!("ascii-mmorpg-content-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("monsters.ron"), "[(id: \"rat\", name: \"Rat\", glyph: 'r', stats: (max_hp: 2, hp: 2, attack: 1, defense: 0, luck: 0), loot: \"missing\")]" ).unwrap();
        fs::write(
            dir.join("loot.ron"),
            "[(id: \"coins\", entries: [(item: \"coin\", weight: 1, min: 1, max: 1)])]",
        )
        .unwrap();
        assert!(validate_content(&dir.join("monsters.ron"), &dir.join("loot.ron")).is_err());
        let _ = fs::remove_dir_all(dir);
    }
}
