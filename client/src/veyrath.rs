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

pub fn atlas_note() -> String {
    veyrath::visual_data().atlas_note()
}

pub fn atlas_lines(max_width: usize, max_height: usize) -> Vec<String> {
    veyrath::visual_data().atlas_lines(max_width, max_height)
}

pub fn glyph_color(glyph: char) -> Color {
    veyrath::visual_data().glyph_color(glyph)
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
}
