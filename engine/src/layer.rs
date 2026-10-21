use super::{Anim, Cell, ColorTrack, Rgb};

/// One animated glyph placed on a [`Layer`]: grid position plus base look
/// plus the tracks that move it. Position is in layer-local cells; the wave
/// axis coordinate doubles as the track's `pos` argument.
#[derive(Clone, Debug)]
pub struct AnimCell {
    pub x: i32,
    pub y: i32,
    pub cell: Cell,
    pub anim: Anim,
    pub color: ColorTrack,
}

impl AnimCell {
    pub fn still(x: i32, y: i32, cell: Cell) -> Self {
        let color = ColorTrack::Static(cell.fg);
        Self {
            x,
            y,
            cell,
            anim: Anim::still(),
            color,
        }
    }

    pub fn animated(x: i32, y: i32, cell: Cell, anim: Anim) -> Self {
        let color = ColorTrack::Static(cell.fg);
        Self {
            x,
            y,
            cell,
            anim,
            color,
        }
    }

    /// Set the dynamic foreground color source while preserving the cell API.
    pub fn with_color(mut self, color: ColorTrack) -> Self {
        self.color = color;
        self
    }

    /// Alias for [`AnimCell::with_color`] that reads naturally at call sites.
    pub fn colored(self, color: ColorTrack) -> Self {
        self.with_color(color)
    }
}

/// One compositing stratum: background, world/foreground, effects, hud — each
/// with its own z, visibility, and cell list (three.js analogy: a renderable
/// layer of the scene graph, flattened for ASCII compositing).
#[derive(Clone, Debug)]
pub struct Layer {
    pub name: String,
    pub z: i32,
    pub visible: bool,
    pub cells: Vec<AnimCell>,
}

impl Layer {
    pub fn new(name: impl Into<String>, z: i32) -> Self {
        Self {
            name: name.into(),
            z,
            visible: true,
            cells: Vec::new(),
        }
    }

    pub fn push(&mut self, cell: AnimCell) {
        self.cells.push(cell);
    }

    /// Stamp one line (or `\n`-separated lines) of text at `(x, y)`.
    /// Spaces become transparent holes so the layers below show through;
    /// every other glyph is an opaque still cell. `\r` is ignored so
    /// CRLF-checked-out sources behave.
    pub fn push_text(&mut self, x: i32, y: i32, text: &str, fg: Rgb, bg: Rgb) {
        self.push_text_styled(x, y, text, fg, bg, false);
    }

    /// [`Layer::push_text`] with a bold flag for headings and highlights.
    pub fn push_text_bold(&mut self, x: i32, y: i32, text: &str, fg: Rgb, bg: Rgb) {
        self.push_text_styled(x, y, text, fg, bg, true);
    }

    /// Stamp text while limiting each line to a fixed number of terminal
    /// cells. This is the cell-safe boundary for HUD/modal copy: callers can
    /// lay out a panel first, then guarantee that text cannot spill into its
    /// border or an adjacent partition.
    pub fn push_text_clipped(&mut self, x: i32, y: i32, width: i32, text: &str, fg: Rgb, bg: Rgb) {
        self.push_text_clipped_with_style(
            x,
            y,
            width,
            text,
            TextStyle {
                fg,
                bg,
                bold: false,
            },
        );
    }

    /// [`Layer::push_text_clipped`] with a bold flag for headings.
    pub fn push_text_clipped_bold(
        &mut self,
        x: i32,
        y: i32,
        width: i32,
        text: &str,
        fg: Rgb,
        bg: Rgb,
    ) {
        self.push_text_clipped_with_style(x, y, width, text, TextStyle { fg, bg, bold: true });
    }

    fn push_text_clipped_with_style(
        &mut self,
        x: i32,
        y: i32,
        width: i32,
        text: &str,
        style: TextStyle,
    ) {
        if width <= 0 {
            return;
        }
        for (dy, line) in text.split('\n').enumerate() {
            let clipped: String = line
                .chars()
                .filter(|glyph| *glyph != '\r')
                .take(width as usize)
                .collect();
            self.push_text_styled(x, y + dy as i32, &clipped, style.fg, style.bg, style.bold);
        }
    }

    fn push_text_styled(&mut self, x: i32, y: i32, text: &str, fg: Rgb, bg: Rgb, bold: bool) {
        for (dy, line) in text.split('\n').enumerate() {
            for (dx, glyph) in line.chars().filter(|c| *c != '\r').enumerate() {
                if glyph == ' ' {
                    continue;
                }
                let mut cell = Cell::new(glyph, fg, bg);
                cell.bold = bold;
                self.push(AnimCell::still(x + dx as i32, y + dy as i32, cell));
            }
        }
    }

    /// Fill the `w` x `h` rect at `(x, y)` with copies of `cell`.
    /// Zero/negative sizes push nothing.
    pub fn fill_rect(&mut self, x: i32, y: i32, w: i32, h: i32, cell: Cell) {
        if cell.transparent {
            return;
        }
        for dy in 0..h.max(0) {
            for dx in 0..w.max(0) {
                self.push(AnimCell::still(x + dx, y + dy, cell));
            }
        }
    }

    /// Mosaic volume fill: the `w` x `h` rect at `(x, y)` stamped with a
    /// block glyph, rows lerping `top` → `bottom` (reference dragon-wing
    /// volumes shade as flat graded rows, not letterforms). Zero/negative
    /// sizes push nothing.
    pub fn fill_shaded_rect(&mut self, x: i32, y: i32, w: i32, h: i32, shade: Shade) {
        if w <= 0 || h <= 0 {
            return;
        }
        for dy in 0..h {
            let f = if h <= 1 {
                0.0
            } else {
                dy as f32 / (h - 1) as f32
            };
            let row = Cell::new(shade.glyph, shade.top.lerp(shade.bottom, f), shade.bg);
            for dx in 0..w {
                self.push(AnimCell::still(x + dx, y + dy, row));
            }
        }
    }

    /// Draw a `+`/`-`/`|` box border around the `w` x `h` rect at `(x, y)`.
    /// Degenerate sizes still draw what fits (a lone `+` for 1x1, a `-`
    /// line for a flat row, a `|` line for a thin column).
    pub fn border_rect(&mut self, x: i32, y: i32, w: i32, h: i32, fg: Rgb, bg: Rgb) {
        if w <= 0 || h <= 0 {
            return;
        }
        let corner = Cell::new('+', fg, bg);
        let horiz = Cell::new('-', fg, bg);
        let vert = Cell::new('|', fg, bg);
        if w == 1 && h == 1 {
            self.push(AnimCell::still(x, y, corner));
            return;
        }
        for dx in 0..w {
            let top = if dx == 0 || dx == w - 1 {
                corner
            } else {
                horiz
            };
            self.push(AnimCell::still(x + dx, y, top));
            if h > 1 {
                let bottom = if dx == 0 || dx == w - 1 {
                    corner
                } else {
                    horiz
                };
                self.push(AnimCell::still(x + dx, y + h - 1, bottom));
            }
        }
        for dy in 1..h - 1 {
            self.push(AnimCell::still(x, y + dy, vert));
            if w > 1 {
                self.push(AnimCell::still(x + w - 1, y + dy, vert));
            }
        }
    }
}

#[derive(Clone, Copy)]
struct TextStyle {
    fg: Rgb,
    bg: Rgb,
    bold: bool,
}

/// Count the terminal cells occupied by a single-line text string.
///
/// Windowed HUD text intentionally uses the engine's single-cell glyph
/// contract. Carriage returns are ignored because they are line-ending
/// plumbing, not visible cells; callers that need to handle newlines should
/// measure each line independently.
pub fn cell_text_width(text: &str) -> usize {
    text.chars()
        .filter(|glyph| *glyph != '\r' && *glyph != '\n')
        .count()
}

/// Palette for [`Layer::fill_shaded_rect`]: block glyph plus vertical
/// gradient ends and background. Grouped so the call stays under the clippy
/// argument limit.
#[derive(Clone, Copy, Debug)]
pub struct Shade {
    pub glyph: char,
    pub top: Rgb,
    pub bottom: Rgb,
    pub bg: Rgb,
}

impl Shade {
    pub fn new(glyph: char, top: Rgb, bottom: Rgb, bg: Rgb) -> Self {
        Self {
            glyph,
            top,
            bottom,
            bg,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_skips_spaces_and_wraps_lines() {
        let mut layer = Layer::new("hud", 3);
        layer.push_text(10, 4, "Hi\nA B", Rgb::new(255, 255, 255), Rgb::new(0, 0, 0));
        // "Hi" + "A" + "B" (space and newline place, never stamp).
        assert_eq!(layer.cells.len(), 4);
        assert_eq!(layer.cells[0].x, 10);
        assert_eq!(layer.cells[2].x, 10);
        assert_eq!(layer.cells[2].y, 5);
        assert_eq!(layer.cells[2].cell.glyph, 'A');
        assert_eq!(layer.cells[3].x, 12);
    }

    #[test]
    fn clipped_text_stays_inside_requested_cell_width() {
        let mut layer = Layer::new("hud", 0);
        layer.push_text_clipped(2, 3, 4, "ABCDE", Rgb::new(255, 255, 255), Rgb::new(0, 0, 0));

        let xs: Vec<i32> = layer.cells.iter().map(|cell| cell.x).collect();
        assert_eq!(xs, vec![2, 3, 4, 5]);
    }

    #[test]
    fn cell_text_width_ignores_line_ending_plumbing() {
        assert_eq!(cell_text_width("A\rB"), 2);
    }

    #[test]
    fn fill_and_border_counts() {
        let mut layer = Layer::new("panel", 0);
        layer.fill_rect(
            0,
            0,
            3,
            2,
            Cell::new('.', Rgb::new(1, 1, 1), Rgb::new(0, 0, 0)),
        );
        assert_eq!(layer.cells.len(), 6);
        layer.border_rect(0, 0, 4, 3, Rgb::new(1, 1, 1), Rgb::new(0, 0, 0));
        // 4x3 border: 4 top + 4 bottom + 2 sides. Top/bottom push
        // interleaved per column: +, +, -, -, -, -, +, + ...
        assert_eq!(layer.cells.len(), 6 + 10);
        assert_eq!(layer.cells[6].cell.glyph, '+');
        assert_eq!(layer.cells[8].cell.glyph, '-');
    }

    #[test]
    fn shaded_rect_grades_rows_and_ignores_degenerate() {
        let mut layer = Layer::new("wing", 0);
        layer.fill_shaded_rect(
            0,
            0,
            2,
            3,
            Shade::new(
                '\u{2588}',
                Rgb::new(200, 160, 110),
                Rgb::new(80, 40, 20),
                Rgb::new(0, 0, 0),
            ),
        );
        assert_eq!(layer.cells.len(), 6);
        assert_eq!(layer.cells[0].cell.fg, Rgb::new(200, 160, 110));
        let mid = layer.cells[2].cell.fg;
        assert!(mid.r > 80 && mid.r < 200, "middle row blends ends");
        assert_eq!(layer.cells[4].cell.fg, Rgb::new(80, 40, 20));
        layer.fill_shaded_rect(
            0,
            0,
            0,
            3,
            Shade::new('x', Rgb::new(1, 1, 1), Rgb::new(2, 2, 2), Rgb::new(0, 0, 0)),
        );
        assert_eq!(layer.cells.len(), 6, "zero width pushes nothing");
    }
}
