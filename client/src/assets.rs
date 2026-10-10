//! Compile-time embedded ASCII art used by the terminal renderer.
//!
//! The files live in the workspace so they can be reviewed and edited as
//! ordinary text, while `include_str!` keeps the client independent of its
//! current working directory at runtime. A frame is selected by the render
//! frame supplied by the client; no asset can change an authoritative snapshot.

use ascii_mmorpg_shared::EntityKind;

const FRAME_SEPARATOR: &str = "---";

#[derive(Clone, Copy, Debug)]
pub struct AsciiAsset {
    name: &'static str,
    source: &'static str,
}

impl AsciiAsset {
    const fn new(name: &'static str, source: &'static str) -> Self {
        Self { name, source }
    }

    pub fn name(self) -> &'static str {
        self.name
    }

    pub fn frame_count(self) -> usize {
        self.source.matches(FRAME_SEPARATOR).count() + 1
    }

    pub fn frame(self, render_frame: u64) -> &'static str {
        let index = render_frame as usize % self.frame_count();
        self.source
            .split(FRAME_SEPARATOR)
            .nth(index)
            .unwrap_or("")
            .trim_matches('\n')
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PropKind {
    Tree,
    Ruin,
}

/// All visual assets used by the client. This is intentionally a zero-sized
/// handle: the actual text is statically embedded in the executable.
#[derive(Clone, Copy, Debug, Default)]
pub struct AssetLibrary;

impl AssetLibrary {
    pub const fn new() -> Self {
        Self
    }

    pub fn for_entity(self, kind: &EntityKind, label: &str, render_frame: u64) -> &'static str {
        self.entity_asset(kind, label).frame(render_frame)
    }

    pub fn entity_asset(self, kind: &EntityKind, label: &str) -> AsciiAsset {
        match kind {
            EntityKind::Player => PLAYER_WARRIOR,
            EntityKind::Monster => MONSTER_MOSSLING,
            EntityKind::Npc => NPC_HERBALIST,
            EntityKind::Item => {
                let label = label.to_ascii_lowercase();
                if label.contains("sword") {
                    ITEM_ICONS.with_variant(0)
                } else if label.contains("shield") {
                    ITEM_ICONS.with_variant(1)
                } else {
                    ITEM_ICONS.with_variant(2)
                }
            }
        }
    }

    pub fn prop(self, kind: PropKind, render_frame: u64) -> &'static str {
        match kind {
            PropKind::Tree => TREE.frame(render_frame),
            PropKind::Ruin => RUIN.frame(render_frame),
        }
    }

    pub fn spell_glyph(self, render_frame: u64) -> char {
        SPELL_EFFECT
            .frame(render_frame)
            .chars()
            .find(|glyph| !glyph.is_whitespace())
            .unwrap_or('*')
    }
}

#[derive(Clone, Copy, Debug)]
struct ItemIcons {
    source: &'static str,
}

impl ItemIcons {
    fn with_variant(self, variant: usize) -> AsciiAsset {
        let frame = self
            .source
            .split(FRAME_SEPARATOR)
            .nth(variant)
            .unwrap_or("");
        // `AsciiAsset::frame` is the only place that performs animation
        // selection. Item variants are represented as one-frame assets here.
        AsciiAsset::new("item-icon", frame)
    }
}

const PLAYER_WARRIOR: AsciiAsset = AsciiAsset::new(
    "player-warrior",
    include_str!("../../assets/ascii/player_warrior.txt"),
);
const MONSTER_MOSSLING: AsciiAsset = AsciiAsset::new(
    "monster-mossling",
    include_str!("../../assets/ascii/monster_mossling.txt"),
);
const NPC_HERBALIST: AsciiAsset = AsciiAsset::new(
    "npc-herbalist",
    include_str!("../../assets/ascii/npc_herbalist.txt"),
);
const TREE: AsciiAsset = AsciiAsset::new("tree", include_str!("../../assets/ascii/tree.txt"));
const RUIN: AsciiAsset = AsciiAsset::new("ruin", include_str!("../../assets/ascii/ruin.txt"));
const ITEM_ICONS: ItemIcons = ItemIcons {
    source: include_str!("../../assets/ascii/item_icons.txt"),
};
const SPELL_EFFECT: AsciiAsset = AsciiAsset::new(
    "spell-effect",
    include_str!("../../assets/ascii/spell_effect.txt"),
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entity_kind_selects_authored_animation_frames() {
        let library = AssetLibrary::new();
        let first = library.for_entity(&EntityKind::Player, "Ranger", 0);
        let second = library.for_entity(&EntityKind::Player, "Ranger", 1);
        assert_ne!(first, second);
        assert!(first.contains("|()|"));
        assert!(second.contains("|<>|"));
    }

    #[test]
    fn item_labels_select_sword_shield_and_generic_icons() {
        let library = AssetLibrary::new();
        assert!(library
            .entity_asset(&EntityKind::Item, "Iron Sword")
            .frame(0)
            .contains("/_o|"));
        assert!(library
            .entity_asset(&EntityKind::Item, "Oak Shield")
            .frame(0)
            .contains("|[]|"));
        assert!(library
            .entity_asset(&EntityKind::Item, "Herb")
            .frame(0)
            .contains("(   )"));
    }

    #[test]
    fn spell_glyph_selection_is_deterministic_and_bounded() {
        let library = AssetLibrary::new();
        assert_eq!(library.spell_glyph(0), '*');
        assert_eq!(library.spell_glyph(4), '*');
        assert_eq!(library.spell_glyph(3), 'X');
    }
}
