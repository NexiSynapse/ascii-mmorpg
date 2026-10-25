//! Veyrath world composition for the continuous canvas (issue #7).
//!
//! Renders the shared [`crate::veyrath`] visual model — region carpets,
//! rivers, roads, settlements, landmarks, and the Millhaven starter
//! identity — as native continuous primitives on an intentional black
//! negative space. The manifest's percent coordinates map directly onto
//! the target grid via [`map_point`]; they stay PROVISIONAL: this module
//! only reads the model and never derives collision, server state, or
//! simulation positions. Everything is a pure function of the view and
//! presentation time, so fixed camera/time inputs project identically.

use crate::continuous::{ContinuousScene, Point2, Primitive, PrimitiveKind, Transform2};
use crate::veyrath::{
    visual_data, Landmark, PointPercent, Region, RoadKind, Settlement, VeyrathVisualData,
};
use crate::{ContinuousMaterial, Rgb};

/// Presentation budget for one composed world. The presenter's hard cap is
/// [`MAX_NATIVE_PRIMITIVES`]; this tighter bound keeps room for other
/// gated layers in combined views.
pub const VEYRATH_WORLD_BUDGET: usize = 900;

/// Target continuous grid the percent canvas maps onto.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorldView {
    pub width: u32,
    pub height: u32,
}

impl WorldView {
    pub const fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }
}

/// Provisional percent position (origin northwest, x east, y south) to
/// fractional grid coordinates. South is grid-down, so the mapping is
/// direct; the return value stays fractional for sub-cell fidelity.
pub fn map_point(at: &PointPercent, view: WorldView) -> Point2 {
    Point2::new(
        at.x * view.width as f32 / 100.0,
        at.y * view.height as f32 / 100.0,
    )
}

fn rgb(color: crate::veyrath::Color) -> Rgb {
    Rgb::new(color[0], color[1], color[2])
}

/// Darken a palette colour so region carpets stay quiet negative-space
/// punctuation, never a filled sheet.
fn quiet(color: Rgb, factor: f32) -> Rgb {
    Rgb::new(
        (f32::from(color.r) * factor) as u8,
        (f32::from(color.g) * factor) as u8,
        (f32::from(color.b) * factor) as u8,
    )
}

/// Deterministic lattice hash for absolute grid cells (same construction
/// family as `weather::lattice_hash`).
fn cell_hash(x: i32, y: i32) -> u32 {
    let mut h = (x as u32).wrapping_mul(0x27d4_eb2d);
    h ^= (y as u32).wrapping_mul(0x1656_67b1);
    h ^ h.rotate_left(13)
}

fn unit(seed: u32) -> f32 {
    let mut x = seed.wrapping_mul(0x9e37_79b9).wrapping_add(0x7f4a_7c15);
    x ^= x >> 16;
    x = x.wrapping_mul(0x85eb_ca6b);
    x ^= x >> 13;
    (x >> 8) as f32 / (1u32 << 24) as f32
}

/// Terrain family for a region's keyword list, resolved against the
/// manifest palette: mountains, forest, wetland, or lowland.
fn terrain_family(region: &Region) -> (char, Rgb, Rgb) {
    let palette = &visual_data().palette;
    let text = region.terrain.join(" ").to_ascii_lowercase();
    if text.contains("mountain") || text.contains("glacier") || text.contains("alpine") {
        ('^', rgb(palette.mountain), rgb(palette.snow_highlight))
    } else if text.contains("forest") || text.contains("woodland") {
        ('&', rgb(palette.forest), quiet(rgb(palette.forest), 1.6))
    } else if text.contains("fen")
        || text.contains("bog")
        || text.contains("reed")
        || text.contains("floodplain")
    {
        ('~', rgb(palette.water), rgb(palette.river_highlight))
    } else {
        ('.', rgb(palette.lowland), rgb(palette.ruins_and_borders))
    }
}

fn push_glyph(
    scene: &mut ContinuousScene,
    position: Point2,
    depth: f32,
    glyph: char,
    scale: Point2,
    color: Rgb,
    opacity: f32,
) {
    scene.push(Primitive::new(
        PrimitiveKind::glyph(glyph),
        Transform2::new(position, depth).with_scale(scale),
        ContinuousMaterial::new(color).with_opacity(opacity),
    ));
}

/// Sparse region carpet: world-locked lattice over each region's extent
/// hint, terrain-keyed glyph + palette colour, mostly void. The carpet
/// gives each region a readable tint without filling the negative space.
fn push_regions(scene: &mut ContinuousScene, view: WorldView, data: &VeyrathVisualData) {
    for region in &data.regions {
        let (glyph, base, accent) = terrain_family(region);
        let [x0, y0, x1, y1] = region.extent_hint;
        let gx0 = (x0 * view.width as f32 / 100.0).floor() as i32;
        let gx1 = (x1 * view.width as f32 / 100.0).ceil() as i32;
        let gy0 = (y0 * view.height as f32 / 100.0).floor() as i32;
        let gy1 = (y1 * view.height as f32 / 100.0).ceil() as i32;
        let step = 6;
        for gy in (gy0..=gy1).step_by(step) {
            for gx in (gx0..=gx1).step_by(step) {
                let key = cell_hash(gx, gy);
                if key % 10 < 6 {
                    continue;
                }
                let jitter_x = unit(key) * step as f32 * 0.8;
                let jitter_y = unit(key + 17) * step as f32 * 0.8;
                let position = Point2::new(
                    (gx as f32 + jitter_x).clamp(0.0, view.width as f32 - 0.5),
                    (gy as f32 + jitter_y).clamp(0.0, view.height as f32 - 0.5),
                );
                let color = if key % 7 == 0 { accent } else { base };
                push_glyph(
                    scene,
                    position,
                    -5.0,
                    glyph,
                    Point2::new(0.5, 0.5),
                    quiet(color, 0.75),
                    0.5 + unit(key + 31) * 0.2,
                );
            }
        }
    }
}

/// River polylines walked at a fixed grid arc step: `~` glyphs in the
/// river highlight colour with a slow travelling shimmer. Principal
/// rivers read wider than tributaries.
fn push_rivers(scene: &mut ContinuousScene, view: WorldView, data: &VeyrathVisualData, t: f32) {
    let palette = &data.palette;
    let river_color = rgb(palette.river_highlight);
    for river in &data.rivers {
        let width = if river.kind == "principal" { 1.0 } else { 0.7 };
        for window in river.path.windows(2) {
            let a = map_point(&window[0], view);
            let b = map_point(&window[1], view);
            let length = (b.x - a.x).hypot(b.y - a.y);
            let steps = ((length / 2.2).ceil() as usize).max(1);
            for i in 0..steps {
                let p = i as f32 / steps as f32;
                let position = Point2::new(a.x + (b.x - a.x) * p, a.y + (b.y - a.y) * p);
                let arc = a.x + (b.x - a.x) * p + a.y;
                let shimmer = 0.75 + 0.25 * (t * 2.0 + arc * 0.35).sin();
                push_glyph(
                    scene,
                    position,
                    -4.0,
                    '~',
                    Point2::new(0.8 * width, 0.6 * width),
                    river_color,
                    shimmer,
                );
            }
        }
    }
}

/// Resolve a road node name to a percent position: settlement ids, then
/// landmark ids (wilderness trails target landmarks). Pass nodes and
/// unmodelled names resolve to `None` and are skipped — node sequences
/// are connectivity intent, not straight-line geometry, and must never
/// become authoritative movement paths.
fn resolve_node(name: &str, data: &VeyrathVisualData) -> Option<PointPercent> {
    data.settlement(name)
        .map(|s| s.at)
        .or_else(|| data.landmarks.iter().find(|l| l.id == name).map(|l| l.at))
}

/// Dashed road runs between resolved waypoints: royal ':' brightest,
/// regional '.', wilderness ',', maritime '=' faint. The dash spacing
/// keeps roads quiet ink over the dark carpet.
fn push_roads(scene: &mut ContinuousScene, view: WorldView, data: &VeyrathVisualData) {
    let palette = &data.palette;
    let road_color = rgb(palette.road);
    for road in &data.roads {
        let (glyph, opacity, step) = match road.kind {
            RoadKind::Royal => (':', 0.95, 2.4),
            RoadKind::Regional => ('.', 0.65, 2.8),
            RoadKind::WildernessTrail => (',', 0.45, 3.2),
            RoadKind::Maritime => ('=', 0.35, 3.6),
        };
        let points: Vec<Point2> = road
            .nodes
            .iter()
            .filter_map(|node| resolve_node(node, data))
            .map(|at| map_point(&at, view))
            .collect();
        for window in points.windows(2) {
            let a = window[0];
            let b = window[1];
            let length = (b.x - a.x).hypot(b.y - a.y);
            let steps = ((length / step).ceil() as usize).max(1);
            for i in 0..steps {
                if i % 3 != 0 {
                    continue;
                }
                let p = i as f32 / steps as f32;
                push_glyph(
                    scene,
                    Point2::new(a.x + (b.x - a.x) * p, a.y + (b.y - a.y) * p),
                    -3.5,
                    glyph,
                    Point2::new(0.5, 0.5),
                    road_color,
                    opacity,
                );
            }
        }
    }
}

fn settlement_glyph(settlement: &Settlement) -> (char, f32) {
    match settlement.kind.as_str() {
        "capital" => ('@', 1.7),
        "city" => ('O', 1.25),
        "fortified_town" => ('#', 1.05),
        "village" => ('o', 0.85),
        _ => ('\u{b7}', 0.65),
    }
}

/// Settlement marks in their FACTION colours — the faction palette
/// visibly owns the political layer. The starter settlement (Millhaven)
/// carries the palette highlight plus a pulsing ring so the starter
/// identity reads at a glance.
fn push_settlements(
    scene: &mut ContinuousScene,
    view: WorldView,
    data: &VeyrathVisualData,
    t: f32,
) {
    let palette = &data.palette;
    let starter_id = &data.starter_settlement;
    for settlement in &data.settlements {
        let position = map_point(&settlement.at, view);
        let (glyph, scale) = settlement_glyph(settlement);
        let faction_color = data
            .faction(&settlement.faction)
            .map(|f| rgb(f.color))
            .unwrap_or_else(|| rgb(palette.labels));
        let starter = settlement.id == *starter_id;
        let (color, mark_scale) = if starter {
            (rgb(palette.highlight), scale * 1.2)
        } else {
            (faction_color, scale)
        };
        push_glyph(
            scene,
            position,
            -1.0,
            glyph,
            Point2::new(mark_scale * 0.6, mark_scale),
            color,
            0.95,
        );
        if starter {
            let pulse = 0.5 + 0.5 * (t * 2.4).sin();
            for i in 0..8u32 {
                let angle = i as f32 * std::f32::consts::TAU / 8.0 + t * 0.4;
                let radius = 1.6 + 0.5 * pulse;
                push_glyph(
                    scene,
                    Point2::new(
                        position.x + angle.cos() * radius,
                        position.y + angle.sin() * radius * 0.55,
                    ),
                    -0.5,
                    '\u{b7}',
                    Point2::new(0.35, 0.35),
                    color,
                    0.4 + 0.4 * pulse,
                );
            }
        }
    }
}

fn landmark_glyph(landmark: &Landmark) -> char {
    match landmark.kind.as_str() {
        "fortification" => '\u{25b2}',
        "ruin" => 'x',
        _ => '*',
    }
}

/// Landmark marks: fortifications, ruins, and specials; principal ones
/// read bright, secondary ones stay whispers over the carpet.
fn push_landmarks(scene: &mut ContinuousScene, view: WorldView, data: &VeyrathVisualData) {
    let palette = &data.palette;
    let ruin_color = rgb(palette.ruins_and_borders);
    let special_color = rgb(palette.highlight);
    for landmark in &data.landmarks {
        let position = map_point(&landmark.at, view);
        let color = if landmark.kind == "ruin" {
            ruin_color
        } else {
            special_color
        };
        let (opacity, scale) = if landmark.principal {
            (0.9, 0.9)
        } else {
            (0.4, 0.55)
        };
        push_glyph(
            scene,
            position,
            -2.0,
            landmark_glyph(landmark),
            Point2::new(scale * 0.6, scale),
            color,
            opacity,
        );
    }
}

/// Compose the whole Veyrath world for `view` at presentation time `t`.
/// Deterministic in its inputs. Authors in VISUAL PRIORITY order —
/// settlements, landmarks, rivers, roads, then the quiet region carpet —
/// and truncates to [`VEYRATH_WORLD_BUDGET`], so at oversized views the
/// budget consumes carpet ink first and never silently drops a settlement
/// or landmark behind background punctuation (review 2026-10-25). Paint
/// order is unaffected: the presenter depth-sorts (carpet depths render
/// under marks regardless of authoring order).
pub fn world_scene(view: WorldView, t: f32) -> ContinuousScene {
    let data = visual_data();
    let mut scene = ContinuousScene::new();
    push_settlements(&mut scene, view, data, t);
    push_landmarks(&mut scene, view, data);
    push_rivers(&mut scene, view, data, t);
    push_roads(&mut scene, view, data);
    push_regions(&mut scene, view, data);
    scene.truncate(VEYRATH_WORLD_BUDGET);
    scene
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::continuous::PrimitiveKind;

    fn view() -> WorldView {
        WorldView::new(180, 74)
    }

    fn glyphs(scene: &ContinuousScene) -> Vec<(char, Point2, Rgb, f32)> {
        scene
            .primitives()
            .iter()
            .filter_map(|p| match p.kind {
                PrimitiveKind::Glyph { glyph } | PrimitiveKind::Particle { glyph } => Some((
                    glyph,
                    p.transform.position,
                    p.material.foreground,
                    p.material.opacity,
                )),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn composition_is_deterministic() {
        for t in [0.0, 0.5, 1.9, 4.7] {
            let a = world_scene(view(), t);
            let b = world_scene(view(), t);
            assert_eq!(a.primitives(), b.primitives());
            assert_eq!(a.sample(t), b.sample(t));
        }
    }

    #[test]
    fn budget_is_bounded_and_scene_is_sparse() {
        let scene = world_scene(view(), 1.0);
        assert!(scene.primitives().len() <= VEYRATH_WORLD_BUDGET);
        // Negative space contract: the world must read as a quiet carpet
        // over void, never a filled sheet.
        let density = scene.primitives().len() as f32 / (180.0 * 74.0);
        assert!(
            density < 0.08,
            "world density {density:.3} is not a quiet carpet"
        );
    }

    /// AC #2: settlements and landmarks sit exactly at their provisional
    /// percent projections at any fixed view/time.
    #[test]
    fn settlements_and_landmarks_project_consistently() {
        let data = visual_data();
        let scene = world_scene(view(), 1.0);
        let marks = glyphs(&scene);
        for settlement in data.settlements.iter() {
            let want = map_point(&settlement.at, view());
            assert!(
                marks
                    .iter()
                    .any(|(_, p, _, _)| (p.x - want.x).abs() < 0.01 && (p.y - want.y).abs() < 0.01),
                "settlement {} not projected at its percent position",
                settlement.id
            );
        }
        for landmark in data.landmarks.iter().take(10) {
            let want = map_point(&landmark.at, view());
            assert!(
                marks
                    .iter()
                    .any(|(_, p, _, _)| (p.x - want.x).abs() < 0.01 && (p.y - want.y).abs() < 0.01),
                "landmark {} not projected at its percent position",
                landmark.id
            );
        }
    }

    /// AC #1: the faction palette visibly owns the political layer —
    /// settlement marks carry their faction's colour, and several
    /// distinct factions are on the map at once.
    #[test]
    fn faction_palette_owns_settlement_marks() {
        let data = visual_data();
        let scene = world_scene(view(), 1.0);
        let marks = glyphs(&scene);
        let mut seen_factions = std::collections::BTreeSet::new();
        for settlement in data.settlements.iter().filter(|s| !s.starter) {
            let want = map_point(&settlement.at, view());
            let faction_color = data
                .faction(&settlement.faction)
                .map(|f| rgb(f.color))
                .expect("settlement faction must resolve");
            let matches = marks.iter().any(|(_, p, color, _)| {
                (p.x - want.x).abs() < 0.01
                    && (p.y - want.y).abs() < 0.01
                    && *color == faction_color
            });
            assert!(
                matches,
                "settlement {} does not wear its faction colour",
                settlement.id
            );
            seen_factions.insert(settlement.faction.clone());
        }
        assert!(seen_factions.len() >= 3, "too few factions visible");
    }

    /// AC #1: region carpets use terrain-family palette colours — the
    /// mountain crescent reads different ink than the forests.
    #[test]
    fn regions_carry_terrain_palette() {
        let data = visual_data();
        let scene = world_scene(view(), 1.0);
        let palette = &data.palette;
        let mountain = rgb(palette.mountain);
        let forest = rgb(palette.forest);
        let marks = glyphs(&scene);
        let in_extent = |region: &crate::veyrath::Region, color: Rgb| {
            let [x0, y0, x1, y1] = region.extent_hint;
            marks.iter().any(|(g, p, c, _)| {
                *g == terrain_family(region).0
                    && *c == quiet(color, 0.75)
                    && p.x >= x0 * 1.8
                    && p.x <= x1 * 1.8
                    && p.y >= y0 * 0.74
                    && p.y <= y1 * 0.74
            })
        };
        let ashen = data.region("ashen-crown").expect("ashen-crown");
        assert!(
            in_extent(ashen, mountain),
            "mountain region carpet missing its terrain ink"
        );
        let gallowmere = data.region("gallowmere").expect("gallowmere");
        assert!(
            in_extent(gallowmere, forest),
            "forest region carpet missing its terrain ink"
        );
    }

    /// Rivers follow their authored polylines: every waypoint has river
    /// ink near it, in the river-highlight colour.
    #[test]
    fn rivers_follow_authored_paths() {
        let data = visual_data();
        let scene = world_scene(view(), 1.0);
        let river_color = rgb(data.palette.river_highlight);
        let marks = glyphs(&scene);
        for river in data.rivers.iter() {
            for waypoint in river.path.iter() {
                let want = map_point(waypoint, view());
                let near = marks.iter().any(|(g, p, c, _)| {
                    *g == '~'
                        && *c == river_color
                        && (p.x - want.x).abs() <= 2.5
                        && (p.y - want.y).abs() <= 2.5
                });
                assert!(near, "river {} has no ink near waypoint", river.id);
            }
        }
    }

    /// Roads project between resolved waypoints as dashes; the Crown Road
    /// (royal) must be present between Crownspire and the capital.
    #[test]
    fn roads_dash_between_resolved_waypoints() {
        let data = visual_data();
        let scene = world_scene(view(), 1.0);
        let road_color = rgb(data.palette.road);
        let marks = glyphs(&scene);
        let a = map_point(&data.settlement("crownspire").unwrap().at, view());
        let b = map_point(&data.settlement("veyrath").unwrap().at, view());
        let mid = Point2::new((a.x + b.x) * 0.5, (a.y + b.y) * 0.5);
        assert!(
            marks.iter().any(|(g, p, c, _)| *g == ':'
                && *c == road_color
                && (p.x - mid.x).abs() <= 3.0
                && (p.y - mid.y).abs() <= 3.0),
            "Crown Road has no royal dashes between Crownspire and Veyrath"
        );
    }

    /// Starter identity: Millhaven wears the palette highlight and a
    /// pulsing ring that breathes with time.
    #[test]
    fn starter_identity_reads_at_millhaven() {
        let data = visual_data();
        let starter = data
            .settlement(&data.starter_settlement)
            .expect("starter settlement must resolve");
        let highlight = rgb(data.palette.highlight);
        let early = glyphs(&world_scene(view(), 0.2));
        let want = map_point(&starter.at, view());
        assert!(
            early.iter().any(|(_, p, c, _)| (p.x - want.x).abs() < 0.01
                && (p.y - want.y).abs() < 0.01
                && *c == highlight),
            "Millhaven must wear the highlight colour"
        );
        let ring = |scene: &ContinuousScene| -> Vec<f32> {
            scene
                .primitives()
                .iter()
                .filter(|p| p.transform.depth == -0.5)
                .map(|p| p.material.opacity)
                .collect()
        };
        let ring_early = ring(&world_scene(view(), 0.2));
        let ring_late = ring(&world_scene(view(), 1.5));
        assert!(!ring_early.is_empty(), "starter ring missing");
        assert_ne!(ring_early, ring_late, "starter ring must pulse");
    }

    /// AC #4 guard: the composer reads the shared model only — the
    /// manifest's coordinate status stays provisional and the model is
    /// never mutated (it is a static shared reference by construction).
    #[test]
    fn provisional_coordinates_stay_presentation_only() {
        assert_eq!(
            visual_data().coordinate_status,
            crate::veyrath::CoordinateStatus::Provisional
        );
        let before = (
            visual_data().settlements.len(),
            visual_data().landmarks.len(),
            visual_data().regions.len(),
        );
        let _ = world_scene(view(), 2.0);
        let after = (
            visual_data().settlements.len(),
            visual_data().landmarks.len(),
            visual_data().regions.len(),
        );
        assert_eq!(before, after);
    }
}
