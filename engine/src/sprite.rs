//! Import workspace-authored ASCII sprites (`assets/ascii/*.txt`) as engine
//! layers.
//!
//! The asset format is shared with the terminal client (`client/src/assets.rs`):
//! frames are separated by a line containing exactly `---` (line-based, so art
//! may contain runs like `|---|`), matched tolerantly across CRLF checkouts.
//! Each frame becomes one step of a per-cell [`Track::Cycle`](crate::Track)
//! flip-book, so a warrior's walk or a tree's sway plays back in any presenter.
//!
//! Spaces are transparent holes (never stamped), so sprite margins cost no
//! cells. Cells that are blank in *some* frames but drawn in others stamp a
//! space glyph on the blank steps — an opaque flash that occludes the layers
//! below for that step. Author sprites with full per-cell coverage (as the
//! shipped `assets/ascii/` set does) or accept the flash; a sparse-overlay
//! mode is deferred until a caller needs it.
//!
//! Colors are uniform per import (`fg`/`bg`); per-glyph palettes are deferred.

use super::{Anim, AnimCell, Cell, Layer, Rgb, Track};

/// Frame separator: a line containing exactly `---`.
pub const FRAME_SEPARATOR: &str = "---";

fn is_separator(line: &str) -> bool {
    line.trim_end_matches('\r') == FRAME_SEPARATOR
}

/// Split sprite `source` into frames; each frame is its lines with the
/// trailing `\r` stripped. Always returns at least one frame. Leading and
/// trailing fully-empty lines of each frame are dropped (mirrors the
/// client's newline trim); leading *spaces* on content lines are kept.
pub fn parse_frames(source: &str) -> Vec<Vec<String>> {
    let mut frames: Vec<Vec<String>> = Vec::new();
    let mut current: Vec<String> = Vec::new();
    // `split('\n')` on an empty source yields one empty line: exactly the
    // single empty frame the client split produces.
    for raw in source.split('\n') {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if is_separator(line) {
            frames.push(trim_empty_edges(std::mem::take(&mut current)));
            continue;
        }
        current.push(line.to_string());
    }
    frames.push(trim_empty_edges(current));
    frames
}

fn trim_empty_edges(mut lines: Vec<String>) -> Vec<String> {
    while lines.first().is_some_and(|l| l.is_empty()) {
        lines.remove(0);
    }
    while lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    lines
}

/// `(width, height)` of the frame set in characters x lines: the padded grid
/// every frame is normalized to before layering. Width counts `char`s, not
/// bytes (art uses multi-byte glyphs like `▲`/`▓`).
pub fn sprite_size(frames: &[Vec<String>]) -> (i32, i32) {
    let mut w = 0i32;
    let mut h = 0i32;
    for frame in frames {
        h = h.max(frame.len() as i32);
        for line in frame {
            w = w.max(line.chars().count() as i32);
        }
    }
    (w, h)
}

fn frame_glyph(frame: &[String], row: usize, col: usize) -> char {
    frame
        .get(row)
        .and_then(|line| line.chars().nth(col))
        .unwrap_or(' ')
}

/// Placement and style for [`sprite_layer`]: top-left origin, uniform
/// colors, and flip-book speed. Grouped so the constructor stays under the
/// clippy argument limit.
#[derive(Clone, Copy, Debug)]
pub struct SpriteDraw {
    pub x: i32,
    pub y: i32,
    pub fg: Rgb,
    pub bg: Rgb,
    /// Frames per second of the flip-book; `<= 0.0` (or a single frame)
    /// renders still.
    pub fps: f32,
}

impl SpriteDraw {
    pub fn new(x: i32, y: i32, fg: Rgb, bg: Rgb, fps: f32) -> Self {
        Self { x, y, fg, bg, fps }
    }
}

/// Build a layer from parsed `frames` (see [`parse_frames`]), placing the
/// sprite's top-left at `draw`'s origin with uniform colors.
///
/// One frame (or `fps <= 0.0`): every drawn cell is still. Several frames:
/// cells whose glyph varies across frames get a `Cycle` flip-book at `fps`;
/// uniform cells stay still. Cells blank in every frame are skipped.
pub fn sprite_layer(
    name: impl Into<String>,
    z: i32,
    frames: &[Vec<String>],
    draw: SpriteDraw,
) -> Layer {
    let mut layer = Layer::new(name, z);
    if frames.is_empty() {
        return layer;
    }
    let (w, h) = sprite_size(frames);
    let animated = frames.len() > 1 && draw.fps > 0.0;
    for row in 0..h as usize {
        for col in 0..w as usize {
            let glyphs: Vec<char> = frames.iter().map(|f| frame_glyph(f, row, col)).collect();
            if glyphs.iter().all(|g| *g == ' ') {
                continue;
            }
            let first = glyphs[0];
            let cell = Cell::new(first, draw.fg, draw.bg);
            let (cx, cy) = (draw.x + col as i32, draw.y + row as i32);
            if animated && glyphs.iter().any(|g| *g != first) {
                layer.push(AnimCell::animated(
                    cx,
                    cy,
                    cell,
                    Anim::still().with(Track::Cycle {
                        frames: glyphs,
                        fps: draw.fps,
                    }),
                ));
            } else {
                layer.push(AnimCell::still(cx, cy, cell));
            }
        }
    }
    layer
}

#[cfg(test)]
mod tests {
    use super::super::{compose, Camera, Partition, Scene};
    use super::*;

    const WARRIOR: &str = include_str!("../../assets/ascii/player_warrior.txt");

    #[test]
    fn separator_is_line_based_and_crlf_tolerant() {
        // `|---|` inside art must not split; `\r\n` line endings must.
        let frames = parse_frames("a|---|b\r\n---\r\nc");
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0], vec!["a|---|b".to_string()]);
        assert_eq!(frames[1], vec!["c".to_string()]);
    }

    #[test]
    fn single_frame_sprite_is_still_and_skips_margins() {
        let frames = parse_frames(" @ \n@@@");
        assert_eq!(frames.len(), 1);
        let layer = sprite_layer(
            "one",
            0,
            &frames,
            SpriteDraw::new(5, 7, Rgb::new(1, 2, 3), Rgb::new(0, 0, 0), 2.0),
        );
        // 4 drawn cells, no margins, no animation tracks.
        assert_eq!(layer.cells.len(), 4);
        assert!(layer.cells.iter().all(|c| c.anim.tracks.is_empty()));
        assert!(layer.cells.iter().any(|c| c.x == 6 && c.y == 7));
    }

    #[test]
    fn multi_frame_sprite_cycles_varying_cells_only() {
        let frames = parse_frames("ab\n---\nax");
        let layer = sprite_layer(
            "two",
            0,
            &frames,
            SpriteDraw::new(0, 0, Rgb::new(1, 1, 1), Rgb::new(0, 0, 0), 2.0),
        );
        assert_eq!(layer.cells.len(), 2);
        let varying = layer.cells.iter().find(|c| c.x == 1).unwrap();
        assert_eq!(varying.anim.tracks.len(), 1);
        let steady = layer.cells.iter().find(|c| c.x == 0).unwrap();
        assert!(steady.anim.tracks.is_empty());
    }

    #[test]
    fn shipped_warrior_loads_four_frames_and_animates() {
        let frames = parse_frames(WARRIOR);
        assert_eq!(frames.len(), 4);
        let (w, h) = sprite_size(&frames);
        assert!(w > 10 && h > 10, "warrior is a volumetric sprite");
        let layer = sprite_layer(
            "warrior",
            1,
            &frames,
            SpriteDraw::new(0, 0, Rgb::new(200, 200, 100), Rgb::new(0, 0, 0), 4.0),
        );
        assert!(!layer.cells.is_empty());
        assert!(layer.cells.iter().any(|c| !c.anim.tracks.is_empty()));

        // Composed frames actually flip between timesteps. The warrior's
        // frames 0/2 and 1/3 are pairs, so step a quarter-second (flip-book
        // index 0 -> 1 at fps 4), not a half-second (index 0 -> 2, identical).
        let mut scene = Scene::default();
        let index = scene.layers.len();
        scene.layers.push(layer);
        scene.partitions.push(Partition::new(
            "all",
            0,
            0,
            w + 2,
            h + 2,
            vec![index],
            false,
        ));
        let a = compose(&scene, &Camera::default(), 0.0).rows().join("\n");
        let b = compose(&scene, &Camera::default(), 0.25).rows().join("\n");
        assert_ne!(a, b, "sprite flip-book must advance with time");
    }
}
