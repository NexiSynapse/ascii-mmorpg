use super::{Axis, Rgb};

/// One addressable glyph on the grid: character plus style.
///
/// This is the engine's "material" (three.js analogy): it describes HOW a
/// cell looks, while [`crate::Track`]s describe how it MOVES. The final look
/// of a frame is `base cell + sampled animation`, computed per frame.
///
/// Two reference-driven channels beyond fg/bg:
///
/// - `glow`: brightness washing onto neighboring backgrounds at compose time
///   (the engine's light buffer — torch halos, spell cores, player aura).
///   0 = none, ~0.35 = player aura, ~1.0 = torch/flash core.
/// - `scale`: billboard size multiplier for the glyph quad (reference spark
///   particles vary in size). 1.0 = full cell; the presenter clamps to a
///   sane range.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cell {
    pub glyph: char,
    pub fg: Rgb,
    /// End color for within-glyph foreground shading. Flat cells keep this
    /// equal to `fg` and leave `fg_gradient` unset.
    pub fg_end: Rgb,
    pub fg_gradient: Option<Axis>,
    pub bg: Rgb,
    pub bold: bool,
    /// Transparent cells are holes: lower layers show through.
    pub transparent: bool,
    pub glow: f32,
    pub scale: f32,
}

impl Cell {
    /// Fully transparent hole (renders as blank, never occludes, emits no glow).
    pub const CLEAR: Self = Self {
        glyph: ' ',
        fg: Rgb::new(0, 0, 0),
        fg_end: Rgb::new(0, 0, 0),
        fg_gradient: None,
        bg: Rgb::new(0, 0, 0),
        bold: false,
        transparent: true,
        glow: 0.0,
        scale: 1.0,
    };

    pub const fn new(glyph: char, fg: Rgb, bg: Rgb) -> Self {
        Self {
            glyph,
            fg,
            fg_end: fg,
            fg_gradient: None,
            bg,
            bold: false,
            transparent: false,
            glow: 0.0,
            scale: 1.0,
        }
    }

    pub const fn bold(mut self) -> Self {
        self.bold = true;
        self
    }

    /// Wash `glow` worth of this cell's foreground color onto surrounding
    /// backgrounds (see [`crate::compose`]).
    pub fn with_glow(mut self, glow: f32) -> Self {
        self.glow = glow.max(0.0);
        self
    }

    /// Billboard size multiplier (1.0 = full cell). Values below 1.0 shrink
    /// the quad toward the cell center; the uncovered margin falls back to
    /// the clear color, so sub-cell sprites read best on dark scenes (the
    /// reference spark use case).
    pub fn with_scale(mut self, scale: f32) -> Self {
        self.scale = scale;
        self
    }

    /// Shade the glyph from its foreground `fg` toward `other` across the
    /// glyph quad. The presenter owns the interpolation; headless composers
    /// preserve both endpoints unchanged in the frame.
    pub const fn with_fg_gradient(mut self, other: Rgb, axis: Axis) -> Self {
        self.fg_end = other;
        self.fg_gradient = Some(axis);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_cells_default_to_flat_foreground_endpoints() {
        let cell = Cell::new('@', Rgb::new(12, 34, 56), Rgb::new(0, 0, 0));
        assert_eq!(cell.fg_end, cell.fg);
        assert_eq!(cell.fg_gradient, None);
    }
}
