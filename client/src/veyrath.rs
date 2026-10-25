//! Compact printable Veyrath atlas backed by the shared semantic model.
//!
//! The atlas is a derived/fallback terminal view. Political colours, faction
//! names, region metadata, roads, rivers, settlements, and landmarks come from
//! `ascii-mmorpg-shared::veyrath`, so the World Map does not maintain a second
//! hand-authored copy of the visual facts.

use ascii_mmorpg_shared::veyrath::{self, Color};

pub fn ascii_atlas() -> &'static str {
    veyrath::visual_data().atlas
}

pub fn region_legend() -> impl Iterator<Item = (&'static str, char, Color)> {
    veyrath::visual_data()
        .factions
        .iter()
        .map(|faction| (faction.name.as_str(), faction.atlas_glyph, faction.color))
}

/// Presentation helpers for the derived printable atlas. These live in
/// the client by ownership (review 2026-10-25): the shared crate keeps
/// the semantic model; terminal presentation decisions stay here.
pub fn atlas_note() -> String {
    let data = veyrath::visual_data();
    let settlement = data
        .settlement(&data.starter_settlement)
        .map(|settlement| settlement.name.as_str())
        .unwrap_or("Starter settlement");
    let region = data
        .settlement(&data.starter_settlement)
        .and_then(|settlement| data.region(&settlement.region))
        .map(|region| region.name.as_str())
        .unwrap_or("provisional region");
    format!("{settlement} (*): starter settlement in {region}")
}

pub fn atlas_lines(max_width: usize, max_height: usize) -> Vec<String> {
    veyrath::visual_data()
        .atlas
        .lines()
        .take(max_height)
        .map(|line| line.chars().take(max_width).collect())
        .collect()
}

pub fn glyph_color(glyph: char) -> Color {
    let data = veyrath::visual_data();
    if let Some(faction) = data
        .factions
        .iter()
        .find(|faction| faction.atlas_glyph == glyph)
    {
        return faction.color;
    }
    let palette = &data.palette;
    match glyph {
        'A' | 'D' | 'G' | 'M' | 'B' | '@' => palette.labels,
        '*' => palette.highlight,
        '~' => palette.water,
        '^' => palette.mountain,
        ':' => palette.river_highlight,
        '=' => palette.road,
        '.' => palette.ruins_and_borders,
        '+' | '-' | '|' => palette.forest,
        _ => palette.labels,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atlas_is_printable_and_contains_the_checkpoint_shape() {
        let atlas = ascii_atlas();
        assert!(atlas.lines().count() >= 16);
        assert!(atlas.lines().all(|line| line.is_ascii()));
        assert!(atlas.contains('@'));
        assert!(atlas.contains('*'));
        assert!(atlas.contains('~'));
        assert!(atlas.contains('^'));
    }

    #[test]
    fn atlas_lines_fit_the_terminal_map_partition() {
        let lines = atlas_lines(38, 16);
        assert_eq!(lines.len(), 16);
        assert!(lines.iter().all(|line| line.chars().count() <= 38));
    }

    #[test]
    fn map_view_uses_shared_faction_colours_and_provisional_status() {
        let legend: Vec<_> = region_legend().collect();
        assert_eq!(legend.len(), 6);
        assert_eq!(
            legend.iter().find(|(_, glyph, _)| *glyph == 'v').unwrap().2,
            [138, 123, 96]
        );
        assert_eq!(
            ascii_mmorpg_shared::veyrath::visual_data().coordinate_status,
            ascii_mmorpg_shared::veyrath::CoordinateStatus::Provisional
        );
        assert!(atlas_note().contains("Millhaven"));
    }

    #[test]
    fn atlas_palette_and_faction_lookup_are_derived_from_the_shared_source() {
        let data = veyrath::visual_data();
        assert_eq!(
            data.factions
                .iter()
                .find(|faction| faction.atlas_glyph == 'v')
                .unwrap()
                .id,
            "greyfall"
        );
        assert_eq!(glyph_color('v'), [138, 123, 96]);
        assert_eq!(glyph_color('='), [182, 157, 121]);
        assert_eq!(
            atlas_note(),
            "Millhaven (*): starter settlement in Greyfall Vale"
        );
    }
}
