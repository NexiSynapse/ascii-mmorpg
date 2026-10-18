//! Dynamic terminal partition system.
//!
//! The terminal font size is window-wide (Ctrl+/- zooms everything), so true
//! per-region font scaling is not possible inside a single terminal window.
//! Partitions therefore simulate density per region:
//!
//! - [`FontProfile::Dense`]: the game world keeps its authored detail as-is
//!   (CPU artwork glyphs / GPU shader ramp). No remapping — the source art is
//!   already the highest-density layer.
//! - [`FontProfile::Medium`]: HUD/status rows, standard glyphs.
//! - [`FontProfile::Large`]: pause and modal overlays, authored directly with
//!   heavy block glyphs (`█▓░━╋★`) plus wide padding so menus read at a glance.
//!
//! Layout adapts to [`GameState`]: while [`GameState::Playing`] the game grid
//! owns the lower rows and the HUD owns the top rows; while
//! [`GameState::Paused`] / [`GameState::ModalOpen`] a large-profile overlay
//! takes the upper third. The minimap stays inside the HUD overlay by design:
//! it needs no extra GPU pass, which keeps zoomed-out frame rates flat.

use crate::ui::ActiveModal;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};

/// Glyph-density profile for one screen region.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum FontProfile {
    /// Game world: authored detail preserved 1:1 (highest density layer).
    #[default]
    Dense,
    /// HUD/status: standard glyphs.
    Medium,
    /// Pause/modal overlays: heavy block glyphs, wide padding.
    Large,
}

/// What the player is doing; drives which partition layout is active.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum GameState {
    /// Normal play: medium HUD on top, dense game grid below.
    #[default]
    Playing,
    /// `P` pause overlay: large menu partition layered over the frozen game.
    /// Client-side only — in online mode the server world keeps ticking.
    Paused,
    /// Inventory / character sheet / world map modal open.
    ModalOpen,
}

impl GameState {
    /// Derives the partition state from the active modal.
    pub fn from_modal(modal: ActiveModal) -> Self {
        match modal {
            ActiveModal::None => GameState::Playing,
            ActiveModal::Paused => GameState::Paused,
            ActiveModal::Inventory | ActiveModal::CharacterSheet | ActiveModal::WorldMap => {
                GameState::ModalOpen
            }
        }
    }
}

/// A screen region: a range of terminal rows (0 = top) with one font profile.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Region {
    /// First row (inclusive, 0-based from the top of the terminal).
    pub start_row: usize,
    /// One past the last row (exclusive).
    pub end_row: usize,
    /// Density profile for this region.
    pub profile: FontProfile,
}

/// Current screen partition layout.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Partitions {
    /// Main game world rows (dense profile).
    pub game_region: Region,
    /// HUD overlay rows (medium profile).
    pub hud_region: Region,
    /// Minimap/side info rows, inside the HUD (medium profile, no extra pass).
    pub side_region: Region,
    /// Dynamic top layer for pause/modal overlays (large profile).
    pub top_layer: Region,
    /// Active game state.
    pub state: GameState,
}

impl Partitions {
    /// Default layout for a terminal of the given height: HUD occupies the top
    /// rows, the game grid owns everything below it.
    pub fn default_for(height: usize) -> Self {
        let hud_end = 5;
        let game_start = 6;
        Self {
            game_region: Region {
                start_row: game_start,
                end_row: height,
                profile: FontProfile::Dense,
            },
            hud_region: Region {
                start_row: 0,
                end_row: hud_end,
                profile: FontProfile::Medium,
            },
            side_region: Region {
                start_row: 0,
                end_row: height,
                profile: FontProfile::Medium,
            },
            top_layer: Region {
                start_row: 0,
                end_row: height / 3,
                profile: FontProfile::Large,
            },
            state: GameState::Playing,
        }
    }

    /// Returns a copy with the given game state applied.
    pub fn with_state(mut self, state: GameState) -> Self {
        self.state = state;
        self
    }

    /// Which density profile a terminal row should use under the current state.
    pub fn profile_for_row(&self, row: usize) -> FontProfile {
        match self.state {
            GameState::Paused | GameState::ModalOpen => {
                if row < self.top_layer.end_row {
                    FontProfile::Large
                } else {
                    FontProfile::Medium
                }
            }
            GameState::Playing => {
                if row < self.hud_region.end_row {
                    FontProfile::Medium
                } else if row >= self.game_region.start_row && row < self.game_region.end_row {
                    FontProfile::Dense
                } else {
                    FontProfile::Medium
                }
            }
        }
    }

    /// Maps a game-grid glyph through the dense profile. Identity today: the
    /// CPU artwork and GPU shader ramp are already the densest layer, and
    /// remapping them would destroy authored detail. Kept as the single choke
    /// point so a future texture pass (e.g. ground dithering) lands here.
    pub fn map_dense_glyph(glyph: char) -> char {
        glyph
    }
}

/// One composed-frame scene cell: a glyph plus TrueColor channels.
///
/// Both renderers lower their native scene representation to this shared type
/// so the full frame (scene + HUD + overlay) composes in a single Ratatui
/// [`Buffer`]: the CPU canvas maps [`crate::renderer`] inks to RGB, the GPU
/// pipeline already produces RGB channels per cell.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SceneCell {
    /// Visible glyph.
    pub glyph: char,
    /// Foreground color.
    pub fg: (u8, u8, u8),
    /// Background color; `None` keeps the terminal default.
    pub bg: Option<(u8, u8, u8)>,
}

impl SceneCell {
    /// Blank cell (terminal default colors).
    pub fn blank() -> Self {
        Self {
            glyph: ' ',
            fg: (0, 0, 0),
            bg: None,
        }
    }
}

/// Blits scene cells into `area` of `buf`, clipped to the area bounds.
/// `cells` is row-major with `cells_width` columns; excess rows/columns are
/// dropped, short grids leave the underlying buffer cells untouched.
pub fn blit_scene(buf: &mut Buffer, area: Rect, cells: &[SceneCell], cells_width: usize) {
    if cells_width == 0 || area.width == 0 || area.height == 0 {
        return;
    }
    for (i, cell) in cells.iter().enumerate() {
        let sx = (i % cells_width) as u16;
        let sy = (i / cells_width) as u16;
        if sx >= area.width || sy >= area.height {
            continue;
        }
        let target = &mut buf[(area.x + sx, area.y + sy)];
        target.set_char(cell.glyph);
        let mut style = Style::default().fg(Color::Rgb(cell.fg.0, cell.fg.1, cell.fg.2));
        if let Some(bg) = cell.bg {
            style = style.bg(Color::Rgb(bg.0, bg.1, bg.2));
        }
        target.set_style(style);
    }
}

/// Centers a `w` x `h` rectangle inside `area`, clamped to fit.
pub fn centered_rect(area: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(area.width).max(1);
    let h = h.min(area.height).max(1);
    let x = area.x + area.width.saturating_sub(w) / 2;
    let y = area.y + area.height.saturating_sub(h) / 2;
    Rect::new(x, y, w, h)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playing_routes_hud_medium_and_game_dense() {
        let p = Partitions::default_for(30);
        assert_eq!(p.profile_for_row(0), FontProfile::Medium);
        assert_eq!(p.profile_for_row(4), FontProfile::Medium);
        assert_eq!(p.profile_for_row(6), FontProfile::Dense);
        assert_eq!(p.profile_for_row(29), FontProfile::Dense);
    }

    #[test]
    fn paused_routes_top_layer_large() {
        let p = Partitions::default_for(30).with_state(GameState::Paused);
        assert_eq!(p.profile_for_row(0), FontProfile::Large);
        assert_eq!(p.profile_for_row(9), FontProfile::Large);
        assert_eq!(p.profile_for_row(10), FontProfile::Medium);
        assert_eq!(p.profile_for_row(29), FontProfile::Medium);
    }

    #[test]
    fn modal_open_routes_top_layer_large() {
        let p = Partitions::default_for(30).with_state(GameState::ModalOpen);
        assert_eq!(p.profile_for_row(0), FontProfile::Large);
        assert_eq!(p.profile_for_row(29), FontProfile::Medium);
    }

    #[test]
    fn state_derives_from_modal() {
        assert_eq!(GameState::from_modal(ActiveModal::None), GameState::Playing);
        assert_eq!(
            GameState::from_modal(ActiveModal::Paused),
            GameState::Paused
        );
        assert_eq!(
            GameState::from_modal(ActiveModal::Inventory),
            GameState::ModalOpen
        );
        assert_eq!(
            GameState::from_modal(ActiveModal::CharacterSheet),
            GameState::ModalOpen
        );
        assert_eq!(
            GameState::from_modal(ActiveModal::WorldMap),
            GameState::ModalOpen
        );
    }

    #[test]
    fn dense_mapping_preserves_authored_detail() {
        for glyph in ['@', 'm', '#', '.', '~', '^', '|', '-', '+', ' ', '░', '█'] {
            assert_eq!(Partitions::map_dense_glyph(glyph), glyph);
        }
    }

    #[test]
    fn blit_scene_places_cells_and_clips_to_area() {
        use ratatui::layout::Rect;
        let area = Rect::new(0, 0, 4, 3);
        let mut buf = Buffer::empty(area);
        let cells = vec![
            SceneCell {
                glyph: '@',
                fg: (255, 0, 0),
                bg: None,
            },
            SceneCell {
                glyph: 'm',
                fg: (0, 255, 0),
                bg: Some((0, 0, 0)),
            },
        ];
        // cells_width = 4: second cell lands at (1, 0).
        blit_scene(&mut buf, area, &cells, 4);
        assert_eq!(buf[(0, 0)].symbol(), "@");
        assert_eq!(buf[(1, 0)].symbol(), "m");
        assert_eq!(buf[(2, 0)].symbol(), " ");
    }

    #[test]
    fn centered_rect_stays_inside_area() {
        use ratatui::layout::Rect;
        let area = Rect::new(0, 0, 80, 24);
        let inner = centered_rect(area, 60, 16);
        assert_eq!(inner.width, 60);
        assert_eq!(inner.height, 16);
        assert_eq!(inner.x, 10);
        assert_eq!(inner.y, 4);
        // Oversized requests clamp instead of overflowing.
        let clamped = centered_rect(area, 200, 200);
        assert_eq!((clamped.width, clamped.height), (80, 24));
    }
}
