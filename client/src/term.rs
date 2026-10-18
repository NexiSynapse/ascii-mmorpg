//! High-performance pure-Rust terminal engine (`term` module).
//!
//! Features:
//! - Double-buffered `CellMatrix` with TrueColor ANSI parser and scene generators.
//! - Span-diffing encoder with intelligent span merging (gaps <= 12 cells merged).
//! - Adaptive fallback: streams full screen if > 60% of cells changed.
//! - Mode 2026 Synchronized Update markers (`\x1b[?2026h` / `\x1b[?2026l`).
//! - Microsecond-precision benchmark harness comparing Per-Cell Jumps vs Row Streaming vs Span Diffing.
//! - Windows high-resolution timer precision guard (`timeBeginPeriod(1)`).

use std::io::Write;
use std::time::Instant;

/// Individual terminal cell containing glyph character and 24-bit TrueColor RGB foreground and background.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    pub glyph: char,
    pub fg: (u8, u8, u8),
    pub bg: (u8, u8, u8),
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            glyph: ' ',
            fg: (200, 200, 200),
            bg: (0, 0, 0),
        }
    }
}

/// Double-buffered 2D grid of terminal cells.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CellMatrix {
    pub width: u16,
    pub height: u16,
    pub cells: Vec<Cell>,
}

impl CellMatrix {
    pub fn new(width: u16, height: u16) -> Self {
        let size = (width as usize) * (height as usize);
        Self {
            width,
            height,
            cells: vec![Cell::default(); size],
        }
    }

    #[inline]
    pub fn index(&self, x: u16, y: u16) -> Option<usize> {
        if x < self.width && y < self.height {
            Some((y as usize) * (self.width as usize) + (x as usize))
        } else {
            None
        }
    }

    #[inline]
    pub fn get(&self, x: u16, y: u16) -> Option<&Cell> {
        self.index(x, y).and_then(|idx| self.cells.get(idx))
    }

    #[inline]
    pub fn get_mut(&mut self, x: u16, y: u16) -> Option<&mut Cell> {
        self.index(x, y).map(move |idx| &mut self.cells[idx])
    }

    #[inline]
    pub fn set(&mut self, x: u16, y: u16, cell: Cell) {
        if let Some(idx) = self.index(x, y) {
            self.cells[idx] = cell;
        }
    }

    pub fn resize(&mut self, width: u16, height: u16) {
        if self.width == width && self.height == height {
            return;
        }
        let mut new_matrix = Self::new(width, height);
        let copy_w = self.width.min(width);
        let copy_h = self.height.min(height);
        for y in 0..copy_h {
            for x in 0..copy_w {
                if let Some(c) = self.get(x, y) {
                    new_matrix.set(x, y, *c);
                }
            }
        }
        *self = new_matrix;
    }

    /// Parses an ANSI string into the structured CellMatrix.
    pub fn from_ansi_str(s: &str, width: u16, height: u16) -> Self {
        let mut matrix = Self::new(width, height);
        let mut cur_x: u16 = 0;
        let mut cur_y: u16 = 0;
        let mut cur_fg = (200, 200, 200);
        let mut cur_bg = (0, 0, 0);

        let bytes = s.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'\n' {
                cur_y = cur_y.saturating_add(1);
                cur_x = 0;
                i += 1;
                continue;
            } else if bytes[i] == b'\r' {
                cur_x = 0;
                i += 1;
                continue;
            } else if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'[' {
                // Parse CSI sequence
                let start = i + 2;
                let mut end = start;
                while end < bytes.len()
                    && !bytes[end].is_ascii_alphabetic()
                    && bytes[end] != b'~'
                    && bytes[end] != b'@'
                {
                    end += 1;
                }
                if end < bytes.len() {
                    let cmd = bytes[end];
                    let param_str = std::str::from_utf8(&bytes[start..end]).unwrap_or("");
                    if cmd == b'm' {
                        // SGR color code
                        Self::parse_sgr(param_str, &mut cur_fg, &mut cur_bg);
                    } else if cmd == b'H' || cmd == b'f' {
                        // Cursor position: 1-indexed row;col
                        let parts: Vec<&str> = param_str.split(';').collect();
                        if let (Some(r_str), Some(c_str)) = (parts.first(), parts.get(1)) {
                            let r: u16 = r_str.parse().unwrap_or(1);
                            let c: u16 = c_str.parse().unwrap_or(1);
                            cur_y = r.saturating_sub(1);
                            cur_x = c.saturating_sub(1);
                        }
                    }
                    i = end + 1;
                    continue;
                }
            }

            // Normal printable character
            let ch = match std::str::from_utf8(&bytes[i..]) {
                Ok(tail) => tail.chars().next().unwrap_or(' '),
                Err(_) => bytes[i] as char,
            };
            let char_len = ch.len_utf8();
            if cur_x < width && cur_y < height {
                matrix.set(
                    cur_x,
                    cur_y,
                    Cell {
                        glyph: ch,
                        fg: cur_fg,
                        bg: cur_bg,
                    },
                );
            }
            cur_x = cur_x.saturating_add(1);
            i += char_len;
        }

        matrix
    }

    fn parse_sgr(params: &str, cur_fg: &mut (u8, u8, u8), cur_bg: &mut (u8, u8, u8)) {
        let nums: Vec<u8> = params
            .split(';')
            .filter_map(|s| s.parse::<u8>().ok())
            .collect();
        let mut idx = 0;
        while idx < nums.len() {
            match nums[idx] {
                0 => {
                    *cur_fg = (200, 200, 200);
                    *cur_bg = (0, 0, 0);
                    idx += 1;
                }
                38 => {
                    // 38;2;r;g;b
                    if idx + 4 < nums.len() && nums[idx + 1] == 2 {
                        *cur_fg = (nums[idx + 2], nums[idx + 3], nums[idx + 4]);
                        idx += 5;
                    } else {
                        idx += 1;
                    }
                }
                48 => {
                    // 48;2;r;g;b
                    if idx + 4 < nums.len() && nums[idx + 1] == 2 {
                        *cur_bg = (nums[idx + 2], nums[idx + 3], nums[idx + 4]);
                        idx += 5;
                    } else {
                        idx += 1;
                    }
                }
                _ => idx += 1,
            }
        }
    }

    /// Generates a test static scene with stone ruins, roads, and trees.
    pub fn generate_static_scene(width: u16, height: u16) -> Self {
        let mut matrix = Self::new(width, height);
        for y in 0..height {
            for x in 0..width {
                let is_road = (x as i32 - (y as i32) / 3).rem_euclid(13) <= 2;
                let cell = if is_road {
                    Cell {
                        glyph: '#',
                        fg: (160, 160, 170),
                        bg: (20, 22, 28),
                    }
                } else if (x * 7 + y * 13) % 23 == 0 {
                    Cell {
                        glyph: '♣',
                        fg: (45, 120, 55),
                        bg: (8, 16, 10),
                    }
                } else {
                    Cell {
                        glyph: '.',
                        fg: (50, 90, 45),
                        bg: (5, 10, 6),
                    }
                };
                matrix.set(x, y, cell);
            }
        }
        matrix
    }

    /// Generates an animated rain scene with dynamic diagonal drops and puddle ripples.
    pub fn generate_rain_scene(width: u16, height: u16, frame: u64) -> Self {
        let mut matrix = Self::generate_static_scene(width, height);
        for y in 0..height {
            for x in 0..width {
                let rain_phase = (x as u64 * 3 + y as u64 * 5 + frame * 7) % 31;
                if rain_phase == 0 {
                    matrix.set(
                        x,
                        y,
                        Cell {
                            glyph: '/',
                            fg: (140, 190, 255),
                            bg: (10, 18, 30),
                        },
                    );
                } else if rain_phase == 1 {
                    matrix.set(
                        x,
                        y,
                        Cell {
                            glyph: '·',
                            fg: (100, 160, 230),
                            bg: (8, 14, 25),
                        },
                    );
                }
            }
        }
        matrix
    }

    /// Generates a high-entropy full-noise scene (every cell changing every frame).
    pub fn generate_noise_scene(width: u16, height: u16, frame: u64) -> Self {
        let mut matrix = Self::new(width, height);
        let chars = ['@', '#', '$', '%', '&', '*', '+', '=', 'o', 'x'];
        for y in 0..height {
            for x in 0..width {
                let h = ((x as u64)
                    .wrapping_mul(374761393)
                    .wrapping_add((y as u64).wrapping_mul(668265263))
                    .wrapping_add(frame.wrapping_mul(2246822519)))
                    % 1000;
                let ch = chars[(h % chars.len() as u64) as usize];
                let r = ((h * 13) % 256) as u8;
                let g = ((h * 29) % 256) as u8;
                let b = ((h * 53) % 256) as u8;
                matrix.set(
                    x,
                    y,
                    Cell {
                        glyph: ch,
                        fg: (r, g, b),
                        bg: (r / 4, g / 4, b / 4),
                    },
                );
            }
        }
        matrix
    }
}

/// Encoding strategies for terminal frame streaming.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EncoderStrategy {
    /// Emit a cursor jump (`\x1b[{y};{x}H`) for each individual changed cell (ConPTY stress test).
    PerCellJump,
    /// Compare rows; if dirty, emit row jump and stream full line.
    RowStreaming,
    /// Span diffing: find changed spans per row, merge gaps <= 12 cells, one jump per span.
    /// Has adaptive fallback to full streaming if > 60% of total cells changed.
    SpanDiff,
    /// Always stream the entire screen sequentially from top-left (`\x1b[H`).
    FullStream,
}

/// Detailed performance and bandwidth statistics from an encoding pass.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FrameStats {
    pub bytes_emitted: usize,
    pub cursor_jumps: usize,
    pub cells_updated: usize,
    pub encode_time_micros: u64,
    pub adaptive_fallback_triggered: bool,
}

/// Double-buffered terminal frame encoder.
pub struct SpanDiffEncoder {
    pub front: CellMatrix,
    pub back: CellMatrix,
    pub sync_markers_supported: bool,
}

impl SpanDiffEncoder {
    pub fn new(width: u16, height: u16) -> Self {
        Self {
            front: CellMatrix::new(width, height),
            back: CellMatrix::new(width, height),
            sync_markers_supported: true,
        }
    }

    pub fn resize(&mut self, width: u16, height: u16) {
        self.front.resize(width, height);
        self.back.resize(width, height);
    }

    /// Encodes current state against previous state using the requested strategy.
    pub fn encode(
        &mut self,
        current: &CellMatrix,
        strategy: EncoderStrategy,
        buf: &mut Vec<u8>,
    ) -> FrameStats {
        let start_time = Instant::now();
        buf.clear();

        // Mode 2026 Synchronized Update Begin: \x1b[?2026h
        if self.sync_markers_supported {
            buf.extend_from_slice(b"\x1b[?2026h");
        }
        // Ensure cursor hidden
        buf.extend_from_slice(b"\x1b[?25l");

        let mut stats = FrameStats::default();

        match strategy {
            EncoderStrategy::PerCellJump => {
                self.encode_per_cell_jump(current, buf, &mut stats);
            }
            EncoderStrategy::RowStreaming => {
                self.encode_row_streaming(current, buf, &mut stats);
            }
            EncoderStrategy::SpanDiff => {
                self.encode_span_diff(current, buf, &mut stats);
            }
            EncoderStrategy::FullStream => {
                self.encode_full_stream(current, buf, &mut stats);
            }
        }

        // Mode 2026 Synchronized Update End: \x1b[?2026l
        if self.sync_markers_supported {
            buf.extend_from_slice(b"\x1b[?2026l");
        }

        stats.bytes_emitted = buf.len();
        stats.encode_time_micros = start_time.elapsed().as_micros() as u64;

        // Advance front buffer to current state
        self.front = current.clone();

        stats
    }

    fn encode_per_cell_jump(
        &self,
        current: &CellMatrix,
        buf: &mut Vec<u8>,
        stats: &mut FrameStats,
    ) {
        let mut cur_fg = (255, 255, 255);
        let mut cur_bg = (0, 0, 0);

        for y in 0..current.height {
            for x in 0..current.width {
                let curr_cell = current.get(x, y).copied().unwrap_or_default();
                let prev_cell = self.front.get(x, y).copied().unwrap_or_default();

                if curr_cell != prev_cell {
                    stats.cells_updated += 1;
                    stats.cursor_jumps += 1;

                    // Emit cursor position: 1-indexed row, 1-indexed col
                    let _ = write!(buf, "\x1b[{};{}H", y + 1, x + 1);

                    Self::write_cell_colors(buf, curr_cell, &mut cur_fg, &mut cur_bg);
                    let mut glyph_buf = [0u8; 4];
                    buf.extend_from_slice(curr_cell.glyph.encode_utf8(&mut glyph_buf).as_bytes());
                }
            }
        }
        buf.extend_from_slice(b"\x1b[0m");
    }

    fn encode_row_streaming(
        &self,
        current: &CellMatrix,
        buf: &mut Vec<u8>,
        stats: &mut FrameStats,
    ) {
        let mut cur_fg = (255, 255, 255);
        let mut cur_bg = (0, 0, 0);

        for y in 0..current.height {
            // Check if row changed
            let mut row_changed = false;
            for x in 0..current.width {
                if current.get(x, y) != self.front.get(x, y) {
                    row_changed = true;
                    stats.cells_updated += 1;
                }
            }

            if row_changed {
                // Always use absolute cursor positioning. \r\n chaining
                // assumes the previous row ended at column 1, but a
                // full-width row auto-wraps the cursor to the next line,
                // so "\r\n" lands one row too far down and leaves stale
                // rows (the idle flicker). Absolute jumps are immune.
                stats.cursor_jumps += 1;
                let _ = write!(buf, "\x1b[{};1H", y + 1);

                for x in 0..current.width {
                    let cell = current.get(x, y).copied().unwrap_or_default();
                    Self::write_cell_colors(buf, cell, &mut cur_fg, &mut cur_bg);
                    let mut glyph_buf = [0u8; 4];
                    buf.extend_from_slice(cell.glyph.encode_utf8(&mut glyph_buf).as_bytes());
                }
            }
        }
        buf.extend_from_slice(b"\x1b[0m");
    }

    fn encode_span_diff(&self, current: &CellMatrix, buf: &mut Vec<u8>, stats: &mut FrameStats) {
        let total_cells = (current.width as usize) * (current.height as usize);
        let mut total_changed = 0;

        // Step 1: Count changed cells to test adaptive fallback threshold (60%)
        for y in 0..current.height {
            for x in 0..current.width {
                if current.get(x, y) != self.front.get(x, y) {
                    total_changed += 1;
                }
            }
        }
        stats.cells_updated = total_changed;

        // Adaptive Fallback: If > 60% of cells changed, full screen streaming is faster
        if total_changed > (total_cells * 6) / 10 {
            stats.adaptive_fallback_triggered = true;
            self.encode_full_stream(current, buf, stats);
            return;
        }

        let mut cur_fg = (255, 255, 255);
        let mut cur_bg = (0, 0, 0);

        // Step 2: Per dirty row, find spans of changed cells and merge gaps <= 12 cells
        const MAX_MERGE_GAP: u16 = 12;

        for y in 0..current.height {
            let mut spans: Vec<(u16, u16)> = Vec::new();
            let mut span_start: Option<u16> = None;

            for x in 0..current.width {
                let changed = current.get(x, y) != self.front.get(x, y);
                if changed {
                    if span_start.is_none() {
                        span_start = Some(x);
                    }
                } else if let Some(start) = span_start {
                    spans.push((start, x));
                    span_start = None;
                }
            }
            if let Some(start) = span_start {
                spans.push((start, current.width));
            }

            if spans.is_empty() {
                continue;
            }

            // Merge close spans where gap <= MAX_MERGE_GAP
            let mut merged_spans: Vec<(u16, u16)> = Vec::with_capacity(spans.len());
            for span in spans {
                if let Some(last) = merged_spans.last_mut() {
                    if span.0.saturating_sub(last.1) <= MAX_MERGE_GAP {
                        last.1 = span.1;
                    } else {
                        merged_spans.push(span);
                    }
                } else {
                    merged_spans.push(span);
                }
            }

            // Emit merged spans with single cursor jump per span
            for (start_x, end_x) in merged_spans {
                stats.cursor_jumps += 1;
                let _ = write!(buf, "\x1b[{};{}H", y + 1, start_x + 1);

                for x in start_x..end_x {
                    let cell = current.get(x, y).copied().unwrap_or_default();
                    Self::write_cell_colors(buf, cell, &mut cur_fg, &mut cur_bg);
                    let mut glyph_buf = [0u8; 4];
                    buf.extend_from_slice(cell.glyph.encode_utf8(&mut glyph_buf).as_bytes());
                }
            }
        }
        buf.extend_from_slice(b"\x1b[0m");
    }

    fn encode_full_stream(&self, current: &CellMatrix, buf: &mut Vec<u8>, stats: &mut FrameStats) {
        stats.cursor_jumps += 1;
        buf.extend_from_slice(b"\x1b[H"); // Jump to top-left once

        let mut cur_fg = (255, 255, 255);
        let mut cur_bg = (0, 0, 0);

        for y in 0..current.height {
            if y > 0 {
                buf.extend_from_slice(b"\r\n");
            }
            for x in 0..current.width {
                let cell = current.get(x, y).copied().unwrap_or_default();
                Self::write_cell_colors(buf, cell, &mut cur_fg, &mut cur_bg);
                let mut glyph_buf = [0u8; 4];
                buf.extend_from_slice(cell.glyph.encode_utf8(&mut glyph_buf).as_bytes());
            }
        }
        buf.extend_from_slice(b"\x1b[0m");
    }

    #[inline]
    fn write_cell_colors(
        buf: &mut Vec<u8>,
        cell: Cell,
        cur_fg: &mut (u8, u8, u8),
        cur_bg: &mut (u8, u8, u8),
    ) {
        if cell.fg != *cur_fg {
            let _ = write!(buf, "\x1b[38;2;{};{};{}m", cell.fg.0, cell.fg.1, cell.fg.2);
            *cur_fg = cell.fg;
        }
        if cell.bg != *cur_bg {
            if cell.bg == (0, 0, 0) {
                // Optimization: black background reset
                buf.extend_from_slice(b"\x1b[49m");
            } else {
                let _ = write!(buf, "\x1b[48;2;{};{};{}m", cell.bg.0, cell.bg.1, cell.bg.2);
            }
            *cur_bg = cell.bg;
        }
    }
}

/// Benchmark record comparing encoding strategies across resolution and scene entropy.
#[derive(Clone, Debug)]
pub struct BenchmarkResult {
    pub resolution: (u16, u16),
    pub scene_type: &'static str,
    pub strategy: EncoderStrategy,
    pub bytes_per_frame: usize,
    pub cursor_jumps: usize,
    pub encode_time_micros: u64,
    pub adaptive_fallback: bool,
}

/// Runs the isolated encoder benchmark suite covering 80x30, 160x45, and 240x67.
pub fn run_encoder_benchmark_suite() -> Vec<BenchmarkResult> {
    let sizes = [(80u16, 30u16), (160, 45), (240, 67)];
    let scenes = ["static", "rain", "noise"];
    let strategies = [
        EncoderStrategy::PerCellJump,
        EncoderStrategy::RowStreaming,
        EncoderStrategy::SpanDiff,
        EncoderStrategy::FullStream,
    ];

    let mut results = Vec::new();

    for &(w, h) in &sizes {
        for &scene in &scenes {
            for &strategy in &strategies {
                let mut encoder = SpanDiffEncoder::new(w, h);
                // Baseline frame 0
                let base = match scene {
                    "static" => CellMatrix::generate_static_scene(w, h),
                    "rain" => CellMatrix::generate_rain_scene(w, h, 0),
                    _ => CellMatrix::generate_noise_scene(w, h, 0),
                };
                let mut buf = Vec::new();
                encoder.encode(&base, EncoderStrategy::FullStream, &mut buf);

                // Frame 1
                let next = match scene {
                    "static" => CellMatrix::generate_static_scene(w, h),
                    "rain" => CellMatrix::generate_rain_scene(w, h, 1),
                    _ => CellMatrix::generate_noise_scene(w, h, 1),
                };
                let stats = encoder.encode(&next, strategy, &mut buf);

                results.push(BenchmarkResult {
                    resolution: (w, h),
                    scene_type: scene,
                    strategy,
                    bytes_per_frame: stats.bytes_emitted,
                    cursor_jumps: stats.cursor_jumps,
                    encode_time_micros: stats.encode_time_micros,
                    adaptive_fallback: stats.adaptive_fallback_triggered,
                });
            }
        }
    }

    results
}

/// RAII guard setting Windows timer resolution to 1ms for precise frame pacing.
pub struct WinTimerPrecisionGuard;

impl WinTimerPrecisionGuard {
    pub fn new() -> Self {
        #[cfg(windows)]
        {
            #[link(name = "winmm")]
            extern "system" {
                fn timeBeginPeriod(uPeriod: u32) -> u32;
            }
            unsafe {
                timeBeginPeriod(1);
            }
        }
        Self
    }
}

impl Default for WinTimerPrecisionGuard {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for WinTimerPrecisionGuard {
    fn drop(&mut self) {
        #[cfg(windows)]
        {
            #[link(name = "winmm")]
            extern "system" {
                fn timeEndPeriod(uPeriod: u32) -> u32;
            }
            unsafe {
                timeEndPeriod(1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cell_matrix_ansi_parsing_and_indexing() {
        let sample = "\x1b[38;2;255;100;50m\x1b[48;2;10;20;30m@\x1b[0m#\n.\r\n";
        let matrix = CellMatrix::from_ansi_str(sample, 10, 5);
        let c00 = matrix.get(0, 0).unwrap();
        assert_eq!(c00.glyph, '@');
        assert_eq!(c00.fg, (255, 100, 50));
        assert_eq!(c00.bg, (10, 20, 30));

        let c10 = matrix.get(1, 0).unwrap();
        assert_eq!(c10.glyph, '#');
        assert_eq!(c10.fg, (200, 200, 200)); // Reset
        assert_eq!(c10.bg, (0, 0, 0));
    }

    #[test]
    fn test_span_diff_encoder_merges_close_spans() {
        let mut encoder = SpanDiffEncoder::new(20, 5);
        let mut base = CellMatrix::new(20, 5);
        let mut buf = Vec::new();
        encoder.encode(&base, EncoderStrategy::FullStream, &mut buf);

        // Change cell at x=2 and cell at x=8 (gap of 5 cells, <= 12)
        base.set(
            2,
            0,
            Cell {
                glyph: 'X',
                fg: (255, 0, 0),
                bg: (0, 0, 0),
            },
        );
        base.set(
            8,
            0,
            Cell {
                glyph: 'Y',
                fg: (0, 255, 0),
                bg: (0, 0, 0),
            },
        );

        let stats = encoder.encode(&base, EncoderStrategy::SpanDiff, &mut buf);
        // Gaps merged into 1 span => exactly 1 cursor jump on row 0
        assert_eq!(stats.cursor_jumps, 1);
        assert_eq!(stats.cells_updated, 2);
    }

    #[test]
    fn test_span_diff_adaptive_fallback_on_high_entropy() {
        let mut encoder = SpanDiffEncoder::new(10, 10);
        let base0 = CellMatrix::generate_noise_scene(10, 10, 0);
        let mut buf = Vec::new();
        encoder.encode(&base0, EncoderStrategy::FullStream, &mut buf);

        let base1 = CellMatrix::generate_noise_scene(10, 10, 1);
        let stats = encoder.encode(&base1, EncoderStrategy::SpanDiff, &mut buf);
        // High entropy noise (> 60% changed) triggers adaptive fallback to full stream!
        assert!(stats.adaptive_fallback_triggered);
        assert_eq!(stats.cursor_jumps, 1); // Full stream uses single \x1b[H
    }

    #[test]
    fn test_benchmark_suite_runs_successfully() {
        let results = run_encoder_benchmark_suite();
        assert!(!results.is_empty());
        // Verify results contain 80x30, 160x45, 240x67
        assert!(results.iter().any(|r| r.resolution == (80, 30)));
        assert!(results.iter().any(|r| r.resolution == (160, 45)));
        assert!(results.iter().any(|r| r.resolution == (240, 67)));

        println!("\n=== TERMINAL ENCODER BENCHMARK MATRIX (M0 GATE) ===");
        println!(
            "{:<10} | {:<8} | {:<14} | {:<10} | {:<8} | {:<10}",
            "Res", "Scene", "Strategy", "Bytes/Frame", "Jumps", "Encode (us)"
        );
        println!(
            "{:-<10}-+-{:-<8}-+-{:-<14}-+-{:-<10}-+-{:-<8}-+-{:-<10}",
            "", "", "", "", "", ""
        );
        for r in &results {
            let strat_str = match r.strategy {
                EncoderStrategy::PerCellJump => "PerCellJump",
                EncoderStrategy::RowStreaming => "RowStreaming",
                EncoderStrategy::SpanDiff => "SpanDiff",
                EncoderStrategy::FullStream => "FullStream",
            };
            println!(
                "{:<10} | {:<8} | {:<14} | {:<10} | {:<8} | {:<10}",
                format!("{}x{}", r.resolution.0, r.resolution.1),
                r.scene_type,
                strat_str,
                r.bytes_per_frame,
                r.cursor_jumps,
                r.encode_time_micros,
            );
        }
    }
}
