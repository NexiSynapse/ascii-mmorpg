//! CPU glyph atlas: rasterizes an embedded monospace font into one R8
//! coverage texture (32x16 slots: 0..=255 direct codepoints, 256+ curated
//! extras like shading blocks).
//!
//! Missing glyphs fall back to `?` (notably color emoji, which a later slice
//! can serve from a dedicated atlas).

const FONT_BYTES: &[u8] = include_bytes!("../../assets/LiberationMono-Regular.ttf");

pub const ATLAS_COLS: u32 = 32;
pub const ATLAS_ROWS: u32 = 16;

/// Extra non-ASCII cells the game uses (shading blocks, box drawing).
/// Slots 256.. live past the direct codepoint range; see [`slot`].
const EXTRA_GLYPHS: [char; 12] = [
    '\u{2591}', '\u{2592}', '\u{2593}', '\u{2588}', '\u{2502}', '\u{2500}', '\u{250C}', '\u{2510}',
    '\u{2514}', '\u{2518}', '\u{253C}', '\u{25A0}',
];

pub struct Atlas {
    /// R8 coverage rows, tightly packed (upload pads rows to 256 bytes).
    pub data: Vec<u8>,
    /// Unpadded row width in bytes (= pixels, one byte per pixel).
    pub stride: u32,
    pub width: u32,
    pub height: u32,
    pub cell_w: u32,
    pub cell_h: u32,
}

/// Slot for `glyph`: direct codepoint for 0..=255, curated extras at 256..,
/// anything else → `?`.
pub fn slot(glyph: char) -> u32 {
    let code = glyph as u32;
    if code < 256 {
        return code;
    }
    if let Some(i) = EXTRA_GLYPHS.iter().position(|&c| c == glyph) {
        return 256 + i as u32;
    }
    b'?' as u32
}

pub fn build(cell_px: f32) -> Result<Atlas, String> {
    let font = fontdue::Font::from_bytes(FONT_BYTES, fontdue::FontSettings::default())
        .map_err(|e| format!("embedded font failed to parse: {e}"))?;
    // Monospace advance is uniform; measure once.
    let advance = font.metrics('M', cell_px).advance_width.max(1.0);
    let cell_w = advance.ceil() as u32;
    let cell_h = cell_px.ceil() as u32;
    let width = ATLAS_COLS * cell_w;
    let height = ATLAS_ROWS * cell_h;
    let mut data = vec![0u8; (width * height) as usize];

    let blit = |data: &mut [u8], slot: u32, bitmap: &[u8], bw: u32, bh: u32| {
        let ox = (slot % ATLAS_COLS) * cell_w;
        let oy = (slot / ATLAS_COLS) * cell_h;
        for row in 0..bh.min(cell_h) {
            for col in 0..bw.min(cell_w) {
                data[((oy + row) * width + ox + col) as usize] = bitmap[(row * bw + col) as usize];
            }
        }
    };

    // Rasterize printable ASCII first (also seeds the '?' fallback).
    for code in 32u32..=126 {
        // SAFETY: range is valid ASCII by construction.
        let c = char::from_u32(code).unwrap_or('?');
        let (metrics, bitmap) = font.rasterize(c, cell_px);
        blit(
            &mut data,
            code,
            &bitmap,
            metrics.width as u32,
            metrics.height as u32,
        );
    }
    let fallback = data.clone();
    let fallback_of = |slot: u32| -> (Vec<u8>, u32, u32) {
        let ox = (slot % ATLAS_COLS) * cell_w;
        let oy = (slot / ATLAS_COLS) * cell_h;
        let mut tile = vec![0u8; (cell_w * cell_h) as usize];
        for row in 0..cell_h {
            for col in 0..cell_w {
                tile[(row * cell_w + col) as usize] =
                    fallback[((oy + row) * width + ox + col) as usize];
            }
        }
        (tile, cell_w, cell_h)
    };
    let is_blank = |tile: &[u8]| tile.iter().all(|&b| b == 0);

    for (slot_offset, &c) in EXTRA_GLYPHS.iter().enumerate() {
        let slot = 256 + slot_offset as u32;
        let (metrics, bitmap) = font.rasterize(c, cell_px);
        let used = !bitmap.iter().all(|&b| b == 0);
        if used {
            blit(
                &mut data,
                slot,
                &bitmap,
                metrics.width as u32,
                metrics.height as u32,
            );
        } else {
            // Missing from the font: stamp the '?' tile instead of a hole.
            let (tile, _, _) = fallback_of(b'?' as u32);
            if !is_blank(&tile) {
                blit(&mut data, slot, &tile, cell_w, cell_h);
            }
        }
    }
    // Space stays empty by construction (never rasterized).
    Ok(Atlas {
        data,
        stride: width,
        width,
        height,
        cell_w,
        cell_h,
    })
}

#[cfg(all(test, feature = "present"))]
mod tests {
    use super::*;

    #[test]
    fn atlas_rasterizes_ascii_and_falls_back() {
        let atlas = build(20.0).unwrap();
        assert!(atlas.cell_w >= 8 && atlas.cell_h == 20);
        // 'A' must have ink; space must be empty.
        let tile = |slot: u32| {
            let ox = (slot % ATLAS_COLS) * atlas.cell_w;
            let oy = (slot / ATLAS_COLS) * atlas.cell_h;
            (0..atlas.cell_h).any(|row| {
                (0..atlas.cell_w)
                    .any(|col| atlas.data[((oy + row) * atlas.stride + ox + col) as usize] != 0)
            })
        };
        assert!(tile(b'A' as u32), "'A' must have ink");
        assert!(!tile(b' ' as u32), "space must be empty");
        assert_eq!(slot('~'), b'~' as u32);
        assert_eq!(slot('\u{1F332}'), b'?' as u32);
        assert_eq!(slot('\u{2588}'), 256 + 3, "full block lives past ASCII");
        assert!(tile(256 + 3), "'█' must have ink, not a hole");
    }
}
