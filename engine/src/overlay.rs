//! Vector overlay strokes: smooth polylines with glow halos, drawn by the
//! presenter as GPU quads under/over the glyph layer. The reference
//! lightning is an anti-aliased vector stroke with node halos — not ASCII
//! cells — so bolts live here, not in [`crate::fx`].
//!
//! Coordinates are frame grid cells (floats allowed); the caller owns the
//! camera math (all current scenes are single full-frame partitions, so
//! world and frame coincide). All generation is deterministic per input.

/// One glowing polyline: bright core plus soft halo plus node dots.
#[derive(Clone, Debug)]
pub struct Stroke {
    /// Frame-space path; consecutive duplicates are skipped.
    pub points: Vec<(f32, f32)>,
    /// Core half-width in cells.
    pub width: f32,
    /// Core color (linear 0..1).
    pub color: [f32; 3],
    /// Halo half-width in cells.
    pub glow_width: f32,
    /// Halo color (linear 0..1).
    pub glow: [f32; 3],
    /// Halo peak alpha (edges fade to 0).
    pub glow_alpha: f32,
    /// Extra bright dots (chain nodes) along the path.
    pub nodes: Vec<(f32, f32)>,
}

/// One GPU overlay vertex: frame-space position, color, coverage alpha,
/// plus an atlas lookup for text (solid quads use [`SOLID_GLYPH`).
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "present", derive(bytemuck::Pod, bytemuck::Zeroable))]
#[repr(C)]
pub struct OverlayVert {
    pub pos: [f32; 2],
    pub color: [f32; 3],
    pub alpha: f32,
    pub glyph: u32,
    pub uv: [f32; 2],
}

/// Glyph marker for solid (non-text) overlay quads.
pub const SOLID_GLYPH: u32 = u32::MAX;

/// Deterministic forked branch: a jagged walk from `origin` along `dir`.
/// Used for lightning splits (reference strokes branch 2–3 ways).
pub fn fork_points(seed: u32, origin: (f32, f32), dir: (f32, f32), len: usize) -> Vec<(f32, f32)> {
    let mut points = vec![origin];
    let (mut x, mut y) = origin;
    let (mut dx, mut dy) = dir;
    let norm = (dx * dx + dy * dy).sqrt().max(1e-6);
    dx /= norm;
    dy /= norm;
    for i in 0..len {
        // Stable hash jitter rotates the march direction each step.
        let mut h = seed
            .wrapping_add(i as u32)
            .wrapping_mul(0x9e37_79b9)
            .wrapping_add(0x7f4a_7c15);
        h ^= h >> 16;
        h = h.wrapping_mul(0x85eb_ca6b);
        h ^= h >> 13;
        let jitter = (h as f32 / u32::MAX as f32 - 0.5) * 1.2;
        let (sin, cos) = (jitter.sin(), jitter.cos());
        let (ndx, ndy) = (dx * cos - dy * sin, dx * sin + dy * cos);
        dx = ndx;
        dy = ndy;
        x += dx * 1.2;
        y += dy * 1.2;
        points.push((x, y));
    }
    points
}

/// Tessellate a stroke into halo quads (under glyphs) and core quads (over).
/// Returns `(halo_verts, core_verts)` as triangle lists.
pub fn tessellate(stroke: &Stroke) -> (Vec<OverlayVert>, Vec<OverlayVert>) {
    let mut halo = Vec::new();
    let mut core = Vec::new();
    let hw = stroke.glow_width.max(0.0);
    let w = stroke.width.max(0.0);
    let ribbon = |ax: f32,
                  ay: f32,
                  bx: f32,
                  by: f32,
                  half: f32,
                  color: [f32; 3],
                  alpha: f32,
                  out: &mut Vec<OverlayVert>| {
        let (dx, dy) = (bx - ax, by - ay);
        let len = (dx * dx + dy * dy).sqrt();
        if len < 1e-6 || half <= 0.0 {
            return;
        }
        let (nx, ny) = (-dy / len * half, dx / len * half);
        // Left half and right half share the center line at full alpha.
        let l0 = [ax - nx, ay - ny];
        let c0 = [ax, ay];
        let l1 = [bx - nx, by - ny];
        let c1 = [bx, by];
        let r0 = [ax + nx, ay + ny];
        let r1 = [bx + nx, by + ny];
        for (p, a) in [(l0, 0.0), (c0, alpha), (c1, alpha)] {
            out.push(OverlayVert {
                pos: p,
                color,
                alpha: a,
                glyph: SOLID_GLYPH,
                uv: [0.0, 0.0],
            });
        }
        for (p, a) in [(l0, 0.0), (c1, alpha), (l1, 0.0)] {
            out.push(OverlayVert {
                pos: p,
                color,
                alpha: a,
                glyph: SOLID_GLYPH,
                uv: [0.0, 0.0],
            });
        }
        for (p, a) in [(c0, alpha), (r0, 0.0), (r1, 0.0)] {
            out.push(OverlayVert {
                pos: p,
                color,
                alpha: a,
                glyph: SOLID_GLYPH,
                uv: [0.0, 0.0],
            });
        }
        for (p, a) in [(c0, alpha), (r1, 0.0), (c1, alpha)] {
            out.push(OverlayVert {
                pos: p,
                color,
                alpha: a,
                glyph: SOLID_GLYPH,
                uv: [0.0, 0.0],
            });
        }
    };
    for pair in stroke.points.windows(2) {
        let (ax, ay) = pair[0];
        let (bx, by) = pair[1];
        ribbon(
            ax,
            ay,
            bx,
            by,
            hw,
            stroke.glow,
            stroke.glow_alpha,
            &mut halo,
        );
        ribbon(ax, ay, bx, by, w, stroke.color, 1.0, &mut core);
    }
    for &(nx, ny) in &stroke.nodes {
        let s = hw * 0.75;
        let corners = [
            [nx - s, ny - s],
            [nx + s, ny - s],
            [nx + s, ny + s],
            [nx - s, ny + s],
        ];
        for [a, b, c] in [
            [corners[0], corners[1], corners[2]],
            [corners[0], corners[2], corners[3]],
        ] {
            for p in [a, b, c] {
                halo.push(OverlayVert {
                    pos: p,
                    color: stroke.glow,
                    alpha: stroke.glow_alpha,
                    glyph: SOLID_GLYPH,
                    uv: [0.0, 0.0],
                });
            }
        }
        let s = (w * 1.6).max(0.12);
        let corners = [
            [nx - s, ny - s],
            [nx + s, ny - s],
            [nx + s, ny + s],
            [nx - s, ny + s],
        ];
        for [a, b, c] in [
            [corners[0], corners[1], corners[2]],
            [corners[0], corners[2], corners[3]],
        ] {
            for p in [a, b, c] {
                core.push(OverlayVert {
                    pos: p,
                    color: stroke.color,
                    alpha: 1.0,
                    glyph: SOLID_GLYPH,
                    uv: [0.0, 0.0],
                });
            }
        }
    }
    (halo, core)
}

/// HUD/info text run: crisp overlay text at a multiple of cell size.
/// Terminal grids cannot do per-region fonts; the windowed presenter can,
/// so boss/target/zone readouts render here at 2x while the world stays 1x.
/// Coordinates are frame grid cells (floats); `size` is line height in cells.
#[derive(Clone, Debug)]
pub struct TextRun {
    pub x: f32,
    pub y: f32,
    pub text: String,
    pub size: f32,
    pub fg: [f32; 3],
    pub bg: [f32; 3],
}

/// Tessellate a text run into overlay quads: opaque backing rect plus the
/// atlas glyph per character, drawn over everything. Uniform 2D scale keeps
/// glyph aspect exact: quads are size x size cells on a size-cell advance
/// (grid pitch, not font-em pitch — 0.6 would squash double-width glyphs).
/// Empty text or non-positive size emits nothing. Spaces advance without
/// quads (backing callers fill first).
pub fn text_quads(run: &TextRun, slot_of: impl Fn(char) -> u32) -> Vec<OverlayVert> {
    let mut out = Vec::new();
    if run.size <= 0.0 || run.text.is_empty() {
        return out;
    }
    // Overlay text shares the terminal grid with the cell renderer. Fractional
    // positions/sizes look attractive in a fixed window but become visibly
    // misregistered when fullscreen changes the grid dimensions, so snap the
    // overlay to whole cells at the final tessellation boundary.
    let x = run.x.round();
    let y = run.y.round();
    let size = run.size.round();
    if size < 1.0 {
        return out;
    }
    let adv = size;
    for (i, glyph) in run.text.chars().enumerate() {
        if glyph == ' ' {
            continue;
        }
        let slot = slot_of(glyph);
        let cx = x + i as f32 * adv;
        let cy = y;
        let backing = [
            [cx, cy],
            [cx + adv, cy],
            [cx + adv, cy + size],
            [cx, cy + size],
        ];
        for [a, b, c] in [
            [backing[0], backing[1], backing[2]],
            [backing[0], backing[2], backing[3]],
        ] {
            for p in [a, b, c] {
                out.push(OverlayVert {
                    pos: p,
                    color: run.bg,
                    alpha: 1.0,
                    glyph: SOLID_GLYPH,
                    uv: [0.0, 0.0],
                });
            }
        }
        let uvs = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
        let corners = [
            [cx, cy],
            [cx + adv, cy],
            [cx + adv, cy + size],
            [cx, cy + size],
        ];
        let tris = [
            (
                (corners[0], uvs[0]),
                (corners[1], uvs[1]),
                (corners[2], uvs[2]),
            ),
            (
                (corners[0], uvs[0]),
                (corners[2], uvs[2]),
                (corners[3], uvs[3]),
            ),
        ];
        for ((a, ua), (b, ub), (c, uc)) in tris {
            for (p, u) in [(a, ua), (b, ub), (c, uc)] {
                out.push(OverlayVert {
                    pos: p,
                    color: run.fg,
                    alpha: 1.0,
                    glyph: slot,
                    uv: u,
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stroke() -> Stroke {
        Stroke {
            points: vec![(0.0, 0.0), (4.0, 0.0), (8.0, 2.0)],
            width: 0.15,
            color: [1.0, 1.0, 1.0],
            glow_width: 1.0,
            glow: [0.4, 0.6, 1.0],
            glow_alpha: 0.5,
            nodes: vec![(4.0, 0.0)],
        }
    }

    #[test]
    fn ribbons_emit_center_bright_edge_clear_triangles() {
        let (halo, core) = tessellate(&stroke());
        // 2 segments x 12 verts each, plus node quads (6 each).
        assert_eq!(halo.len(), 2 * 12 + 6);
        assert_eq!(core.len(), 2 * 12 + 6);
        assert!(halo.iter().any(|v| v.alpha == 0.0));
        assert!(halo.iter().any(|v| v.alpha == 0.5));
        // Core edges fade to 0 like the halo (anti-aliased line); only
        // edge/center values may appear, never partial coverage.
        let mut alphas: Vec<f32> = core.iter().map(|v| v.alpha).collect();
        alphas.sort_by(|a, b| a.partial_cmp(b).unwrap());
        alphas.dedup();
        assert_eq!(alphas, vec![0.0, 1.0]);
        assert!(core.iter().any(|v| v.alpha == 1.0));
    }

    #[test]
    fn text_quads_emit_backing_plus_mapped_glyphs() {
        let run = TextRun {
            x: 2.0,
            y: 3.0,
            text: "A B".into(),
            size: 2.0,
            fg: [1.0, 1.0, 1.0],
            bg: [0.0, 0.0, 0.0],
        };
        let quads = text_quads(&run, |g| g as u32);
        // Two non-space chars x (backing + glyph) x 6 verts.
        assert_eq!(quads.len(), 24);
        assert!(quads.iter().all(|v| v.alpha == 1.0));
        assert_eq!(quads.iter().filter(|v| v.glyph == 'A' as u32).count(), 6);
        // Advance pitch counts spaces too: 'B' sits two advances right.
        let a_x = quads.iter().find(|v| v.glyph == 'A' as u32).unwrap().pos[0];
        let b_x = quads.iter().find(|v| v.glyph == 'B' as u32).unwrap().pos[0];
        assert!((b_x - a_x - 4.0).abs() < 1e-5);
        // Empty and zero-size emit nothing.
        let mut blank = run.clone();
        blank.text.clear();
        assert!(text_quads(&blank, |g| g as u32).is_empty());
        blank.text = "A".into();
        blank.size = 0.0;
        assert!(text_quads(&blank, |g| g as u32).is_empty());
    }

    #[test]
    fn fractional_overlay_text_snaps_to_terminal_cells() {
        let run = TextRun {
            x: 2.4,
            y: 0.35,
            text: "AB".into(),
            size: 1.35,
            fg: [1.0, 1.0, 1.0],
            bg: [0.0, 0.0, 0.0],
        };
        let quads = text_quads(&run, |g| g as u32);
        assert!(!quads.is_empty());
        assert!(quads.iter().all(|vertex| {
            (vertex.pos[0] - vertex.pos[0].round()).abs() < 1e-5
                && (vertex.pos[1] - vertex.pos[1].round()).abs() < 1e-5
        }));
        let a_x = quads.iter().find(|v| v.glyph == 'A' as u32).unwrap().pos[0];
        let b_x = quads.iter().find(|v| v.glyph == 'B' as u32).unwrap().pos[0];
        assert!((b_x - a_x - 1.0).abs() < 1e-5);
    }

    #[test]
    fn empty_and_degenerate_paths_emit_nothing_extra() {
        let mut empty = stroke();
        empty.points.clear();
        empty.nodes.clear();
        assert_eq!(tessellate(&empty), (vec![], vec![]));
        let mut dup = stroke();
        dup.points = vec![(3.0, 3.0), (3.0, 3.0)];
        dup.nodes.clear();
        assert_eq!(tessellate(&dup), (vec![], vec![]));
    }

    #[test]
    fn forks_are_deterministic_bounded_and_grow() {
        let a = fork_points(7, (0.0, 0.0), (1.0, 0.0), 6);
        let b = fork_points(7, (0.0, 0.0), (1.0, 0.0), 6);
        assert_eq!(a, b);
        assert_eq!(a.len(), 7);
        assert_eq!(a[0], (0.0, 0.0));
        assert!(a.iter().all(|&(x, y)| x.abs() < 12.0 && y.abs() < 12.0));
        // Marches dominantly forward.
        assert!(a[6].0 > 3.0);
    }
}
