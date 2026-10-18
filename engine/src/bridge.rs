//! Port of the terminal game's 2.5D look into the windowed engine.
//!
//! The terminal client renders an elevated oblique world: terrain, actors,
//! props, and effects share one camera transform, actors carry floating
//! nameplates, and idle scenery stays stable while water/weather/torches
//! animate. This module rebuilds that look from plain data — no terminal,
//! no networking, no game logic.
//!
//! The engine stays dependency-free: callers map their own snapshot type
//! onto [`SceneView`] (one `match` on kind/glyph, like the client's asset
//! dispatch) and get back a `(Scene, Camera)` that any presenter can draw.
//! Bodies are volumetric art (`engine/assets/actors/*.txt` via
//! [`crate::sprite`]), foot-anchored with the kind's focal glyph steady at
//! the actor position for camera/palette/glow contracts. Items keep mini
//! glyph clusters.
//!
//! 2.5D cues used here (all cheap grid tricks, no depth buffer):
//!
//! - verticality: canopies, nameplates, and HP bars stack *above* the ground
//!   row; shadows sit one row *below* (south = foreground);
//! - depth shading: ground brightness grows toward larger `y` (foreground);
//! - one shared camera transform: every world layer scrolls together.

use super::overlay::TextRun;
use super::sprite;
use super::{Anim, AnimCell, Axis, Camera, Cell, ColorTrack, Layer, Partition, Rgb, Scene, Track};
use std::sync::OnceLock;

/// Legacy world grid used by [`scene_from_view`].
pub const VIEW_W: i32 = 96;
pub const VIEW_H: i32 = 30;
/// Pinned HUD rows above the world and log rows below it: zone readout
/// (2 overlay rows) plus one legend row.
pub const HUD_H: i32 = 3;
pub const LOG_H: i32 = 2;

/// Meadow extent: fixed world-space scenery so the camera scrolls over
/// stable ground (never generated around the player per-frame).
const MEADOW_X0: i32 = -160;
const MEADOW_X1: i32 = 160;
const MEADOW_Y0: i32 = -64;
const MEADOW_Y1: i32 = 96;

const HUD_FG: Rgb = Rgb::new(224, 205, 150);
const HUD_DIM: Rgb = Rgb::new(145, 170, 195);
const PANEL_BG: Rgb = Rgb::new(7, 9, 16);

/// Full-width `[###---]` bar at row `y`: `style.fill` for the held fraction,
/// `style.empty` for the remainder, on `style.bg`. `hp` clamps into
/// `0..=max_hp`.
fn push_bar(layer: &mut Layer, y: i32, width: i32, hp: i32, max_hp: i32, style: BarStyle) {
    if width < 5 {
        return;
    }
    let inner = width - 2;
    let held = ((hp.max(0) as i64 * inner as i64) / max_hp.max(1) as i64) as usize;
    let held = held.min(inner as usize);
    let mut bar = String::with_capacity(width as usize);
    bar.push('[');
    for i in 0..inner as usize {
        bar.push(if i < held { '#' } else { '-' });
    }
    bar.push(']');
    let chars: Vec<char> = bar.chars().collect();
    let held_text: String = chars[..1 + held].iter().collect();
    let empty_text: String = chars[1 + held..].iter().collect();
    layer.push_text(0, y, &held_text, style.fill, style.bg);
    layer.push_text(1 + held as i32, y, &empty_text, style.empty, style.bg);
}

/// Meter colors for [`push_bar`]. Grouped so the call stays under the clippy
/// argument limit.
#[derive(Clone, Copy, Debug)]
pub struct BarStyle {
    pub fill: Rgb,
    pub empty: Rgb,
    pub bg: Rgb,
}

fn layout(width: i32, height: i32, extra_hud: i32) -> (i32, i32, i32, i32) {
    let width = width.max(1);
    let height = height.max(3);
    let base = if height >= 7 { HUD_H } else { 1 };
    let log = if height >= 6 { LOG_H } else { 1 };
    // Boss/target rows only when the world keeps at least one row.
    let extra = extra_hud.max(0).min((height - base - log - 1).max(0));
    let hud = base + extra;
    let world = (height - hud - log).max(1);
    (width, hud, world, log)
}

/// Big overlay readout: 2x crisp text for boss/target/zone lines, drawn
/// over the frame by the presenter (terminal grids cannot do per-region
/// fonts). `center` picks centered (boss) vs x=1 (target, zone) anchoring.
fn push_hud_text(scene: &mut Scene, y: i32, width: i32, text: &str, fg: Rgb, center: bool) {
    let adv = 2.0;
    let len = text.chars().count() as f32;
    let x = if center {
        ((width as f32 - len * adv) / 2.0).max(0.0)
    } else {
        1.0
    };
    let f = |c: u8| f32::from(c) / 255.0;
    let bg = PANEL_BG;
    scene.overlay_text.push(TextRun {
        x,
        y: y as f32,
        text: text.to_string(),
        size: 2.0,
        fg: [f(fg.r), f(fg.g), f(fg.b)],
        bg: [f(bg.r), f(bg.g), f(bg.b)],
    });
}

fn fit_text(layer: &mut Layer, x: i32, y: i32, width: i32, text: &str, fg: Rgb, bold: bool) {
    if width <= 0 {
        return;
    }
    let clipped: String = text.chars().take(width as usize).collect();
    if bold {
        layer.push_text_bold(x, y, &clipped, fg, PANEL_BG);
    } else {
        layer.push_text(x, y, &clipped, fg, PANEL_BG);
    }
}

/// What an actor is, for palette dispatch (mirrors the terminal client's
/// label-text dispatch: skeleton bone-white, champion gold, mossling green).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActorKind {
    Player,
    Monster,
    Npc,
    Item,
}

/// One actor in engine-native form. `x`/`y` are world cells; the head glyph
/// renders at `(x, y)`, the nameplate at `y - 1`, HP pips at `y - 2` when
/// damaged, the shadow at `y + 1`.
#[derive(Clone, Debug)]
pub struct SceneActor {
    pub x: i32,
    pub y: i32,
    /// Fractional predicted motion, relative to the authoritative cell.
    pub render_offset: (f32, f32),
    /// Current planar speed; controls gait cadence.
    pub motion_speed: f32,
    /// Horizontal facing: positive is the source pose, negative mirrors it.
    pub facing_x: i8,
    pub kind: ActorKind,
    pub name: String,
    pub glyph: char,
    pub hp: i32,
    pub max_hp: i32,
    /// Optional resolved foreground palette for the actor glyph. `None` keeps
    /// the legacy actor-kind dispatch below.
    pub palette: Option<ColorTrack>,
}

/// Everything the bridge needs: the followed player, surrounding actors,
/// recent log lines, and HUD facts. No game types leak into the engine.
#[derive(Clone, Debug)]
pub struct SceneView {
    pub player: SceneActor,
    pub actors: Vec<SceneActor>,
    pub messages: Vec<String>,
    pub tick: u64,
    pub zone: String,
    /// Visual preset (lighting, ground, weather). Display name stays in
    /// `zone`; this drives the look (reference meadow ~38 vs dungeon ~30).
    pub zone_kind: ZoneKind,
    /// Boss bar pinned to the HUD top (reference dragon bar). `None` hides it.
    pub boss: Option<BossPanel>,
    /// Target panel under the boss bar (reference `Skeleton 55/55`).
    /// Selection semantics belong to the caller; `None` hides it.
    pub target: Option<TargetPanel>,
    /// Presentation-side combat FX events (impact bursts + cast
    /// telegraphs) the client observed. Order-stable; sampled at
    /// `presentation_time_s`.
    pub combat_fx: Vec<SceneFx>,
    /// Current presentation seconds (the caller's `scene_at` clock). Drives
    /// FX sampling and the target ring's dash rotation; deterministic per
    /// instant, no retained state.
    pub presentation_time_s: f32,
}

/// The melee-range target under the decal rule that mirrors the game's
/// `Command::Attack`: the nearest living monster within `MELEE_RANGE`
/// (Manhattan), first wins ties. `None` when no monster is in reach.
fn melee_target<'a>(player: &SceneActor, actors: &'a [SceneActor]) -> Option<&'a SceneActor> {
    let manhattan = |a: &SceneActor| (a.x - player.x).abs() + (a.y - player.y).abs();
    actors
        .iter()
        .filter(|a| a.kind == ActorKind::Monster && a.hp > 0)
        .filter(|a| manhattan(a) <= MELEE_RANGE)
        .min_by_key(|a| manhattan(a))
}

/// Lighting/ground/weather preset per zone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZoneKind {
    Meadow,
    Dungeon,
}

/// Boss bar data: full-width top bar with centered name, HP numbers, and an
/// optional phase label (reference `Dragon - phase 2`).
#[derive(Clone, Debug)]
pub struct BossPanel {
    pub name: String,
    pub hp: i32,
    pub max_hp: i32,
    pub phase: Option<String>,
}

/// Target panel data: name plus numeric HP under the boss bar.
#[derive(Clone, Debug)]
pub struct TargetPanel {
    pub name: String,
    pub hp: i32,
    pub max_hp: i32,
}

/// Presentation-side combat FX event: the client-side state behind the
/// combat-readability pack (impact bursts + cast telegraphs). Origins are
/// world cell coordinates; `born_s` is on the view's own presentation
/// timeline (the same clock `presentation_time_s` ticks). The bridge stays
/// stateless — clients observe messages and feed these in, exactly like
/// the terminal GPU renderer does.
#[derive(Clone, Debug)]
pub struct SceneFx {
    /// Ground anchor of the event in world cells.
    pub origin: (f32, f32),
    /// When the event was observed, in presentation seconds.
    pub born_s: f32,
    /// Telegraph sweep radius (max ring radius for the burst phase too).
    pub radius: f32,
    /// Single hue per damage/event school (reference: FX are solid
    /// single-color ink on the dark stage).
    pub color: Rgb,
    /// Seconds of dashed inward sweep before the burst resolves; 0.0 fires
    /// the burst immediately.
    pub telegraph_s: f32,
    /// Damage digits raised off the impact ("" for none).
    pub digits: String,
}

/// Melee decal range mirroring the terminal game's combat rule (shared
/// `combat::MELEE_RANGE`). Kept as a local constant so the engine stays
/// dependency-free; gameplay authority always belongs to the caller.
const MELEE_RANGE: i32 = 6;

/// Solid white-hot core lifetime at the start of every burst (reference
/// bursts begin as a bright solid mass before rings/scatter appear).
pub const FX_CORE_S: f32 = 0.18;

/// Ring/ember burst lifetime after any telegraph.
pub const FX_BURST_S: f32 = 0.45;

const STAGE_BG: Rgb = Rgb::new(7, 6, 5);
const STAGE_FG: Rgb = Rgb::new(10, 9, 8);

const BG: Rgb = Rgb::new(5, 6, 12);

fn hash2(x: i32, y: i32, seed: u32) -> u32 {
    let mut h = seed
        .wrapping_add((x as u32).wrapping_mul(374761393))
        .wrapping_add((y as u32).wrapping_mul(668265263));
    h = (h ^ (h >> 13)).wrapping_mul(1274126177);
    h ^ (h >> 16)
}

/// Foreground grows brighter toward larger `y` (south = closer to camera).
fn depth_shade(base: Rgb, y: i32) -> Rgb {
    let span = (MEADOW_Y1 - MEADOW_Y0).max(1) as f32;
    let f = 0.6 + 0.6 * ((y - MEADOW_Y0) as f32 / span);
    base.scaled(f)
}

/// World-radius vignette: full light within 26 cells of the anchor,
/// smoothstep down to a 0.25 floor by 64 out, plus static per-cell dither so
/// no clean ellipse edge ever forms (a linear kink reads as a pasted oval).
/// World-locked, so it survives camera moves.
fn vignette_factor(x: i32, y: i32, ax: i32, ay: i32) -> f32 {
    let dist = (((x - ax) * (x - ax) + (y - ay) * (y - ay)) as f32).sqrt();
    let t = ((dist - 26.0) / 38.0).clamp(0.0, 1.0);
    let smooth = t * t * (3.0 - 2.0 * t);
    let dither = (hash2(x, y, 503) % 100) as f32 / 100.0 - 0.5;
    (1.0 - smooth * 0.75 + dither * 0.08).clamp(0.25, 1.0)
}

/// Coarse void mask as tiered density: void 16x16 blocks thin out to 0.25
/// or 0.05 instead of switching fully off, so block borders read as natural
/// thinning rather than pasted rectangles (reference dark areas still carry
/// sparse glyphs). Near the anchor the field stays dense; roads and water
/// punch through below.
fn void_density(x: i32, y: i32, ax: i32, ay: i32) -> f32 {
    let dist = (((x - ax) * (x - ax) + (y - ay) * (y - ay)) as f32).sqrt();
    if dist < 28.0 {
        return 1.0;
    }
    if hash2(x.div_euclid(16), y.div_euclid(16), 191) % 4 != 0 {
        return 1.0;
    }
    if hash2(x.div_euclid(8), y.div_euclid(8), 193) % 2 == 0 {
        0.05
    } else {
        0.25
    }
}

/// Fine-cell volumetric actor art (`engine/assets/actors/*.txt`, `---`
/// separated phases, same format as the terminal set). At the presenter's
/// 8 px cell pitch, human models use 25–29 articulated rows; each cell adds
/// a contour, material break, or joint cue. Models are foot-anchored, with
/// the kind's focal glyph held steady at one local cell in every phase.
const WARRIOR_ART: &str = include_str!("../assets/actors/warrior.txt");
const MOSSLING_ART: &str = include_str!("../assets/actors/mossling.txt");
const SKELETON_ART: &str = include_str!("../assets/actors/skeleton.txt");
const HERBALIST_ART: &str = include_str!("../assets/actors/herbalist.txt");
// Doubled on both axes for the 8 px high-density scene grid.
const TREE_ART: &str = include_str!("../assets/actors/pine.txt");

/// Walking poses advance with speed; idle uses a quiet breathing cadence.
const WALK_FPS: f32 = 7.0;
const IDLE_FPS: f32 = 1.2;

fn cached_frames(source: &'static str) -> &'static Vec<Vec<String>> {
    static WARRIOR: OnceLock<Vec<Vec<String>>> = OnceLock::new();
    static MOSSLING: OnceLock<Vec<Vec<String>>> = OnceLock::new();
    static SKELETON: OnceLock<Vec<Vec<String>>> = OnceLock::new();
    static HERBALIST: OnceLock<Vec<Vec<String>>> = OnceLock::new();
    static TREE: OnceLock<Vec<Vec<String>>> = OnceLock::new();
    // The call sites each pass one of the statics above; pointer
    // identity routes to a dedicated cache so parsing happens once.
    if std::ptr::eq(source, WARRIOR_ART) {
        WARRIOR.get_or_init(|| sprite::parse_frames(source))
    } else if std::ptr::eq(source, MOSSLING_ART) {
        MOSSLING.get_or_init(|| sprite::parse_frames(source))
    } else if std::ptr::eq(source, SKELETON_ART) {
        SKELETON.get_or_init(|| sprite::parse_frames(source))
    } else if std::ptr::eq(source, TREE_ART) {
        TREE.get_or_init(|| sprite::parse_frames(source))
    } else {
        HERBALIST.get_or_init(|| sprite::parse_frames(source))
    }
}

/// Focal anchor: the local cell carrying the kind's focal glyph in the MOST
/// phases (ties break toward the art center). Camera, palette, and glow
/// contracts pin to this cell, so it must read as the focal glyph in every
/// phase — the center tests verify all four.
fn focal_of(frames: &[Vec<String>], glyph: char) -> (i32, i32) {
    use std::collections::HashMap;
    let mut votes: HashMap<(i32, i32), usize> = HashMap::new();
    for frame in frames {
        for (y, line) in frame.iter().enumerate() {
            for (x, g) in line.chars().enumerate() {
                if g == glyph {
                    *votes.entry((x as i32, y as i32)).or_insert(0) += 1;
                }
            }
        }
    }
    let (w, h) = sprite::sprite_size(frames);
    let center = (w / 2, h / 2);
    votes
        .into_iter()
        .max_by_key(|(pos, count)| {
            (
                *count,
                -((pos.0 - center.0).abs() + (pos.1 - center.1).abs()),
            )
        })
        .map(|(pos, _)| pos)
        .unwrap_or(center)
}

/// Volumetric art selection: source text, uniform tint, and focal aura per
/// kind (mirrors the terminal client's label-text dispatch).
fn art_for(a: &SceneActor) -> Option<(&'static str, Rgb, f32)> {
    match a.kind {
        ActorKind::Player => Some((WARRIOR_ART, Rgb::new(205, 210, 228), 0.4)),
        ActorKind::Npc => Some((HERBALIST_ART, Rgb::new(205, 175, 255), 0.15)),
        ActorKind::Item => None,
        ActorKind::Monster
            if a.name.to_ascii_lowercase().contains("skeleton")
                || a.glyph.eq_ignore_ascii_case(&'s') =>
        {
            Some((SKELETON_ART, Rgb::new(232, 232, 222), 0.2))
        }
        ActorKind::Monster => Some((MOSSLING_ART, Rgb::new(140, 225, 135), 0.2)),
    }
}

/// Keep actors at believable world scale beside the doubled 40-row pine.
/// The mossling is intentionally shorter, while the skeletal frame is a
/// little narrower than a human's so it reads as a lean undead.
fn actor_art_scale(a: &SceneActor) -> f32 {
    match a.kind {
        ActorKind::Player => 0.72,
        ActorKind::Npc => 0.72,
        ActorKind::Monster
            if a.name.to_ascii_lowercase().contains("skeleton")
                || a.glyph.eq_ignore_ascii_case(&'s') =>
        {
            0.70
        }
        ActorKind::Monster => 0.82,
        ActorKind::Item => 1.0,
    }
}

fn scaled_sprite_size(frames: &[Vec<String>], scale: f32) -> (i32, i32) {
    let (w, h) = sprite::sprite_size(frames);
    (
        ((w as f32 * scale).round() as i32).max(1),
        ((h as f32 * scale).round() as i32).max(1),
    )
}

/// Map a destination cell to its nearest source cell. The focal source cell
/// is kept exactly in `stamp_art`, avoiding loss of the camera anchor while
/// reducing the overall footprint.
fn source_cell(destination: i32, source_len: i32, destination_len: i32) -> i32 {
    if source_len <= 1 || destination_len <= 1 {
        0
    } else {
        (destination * (source_len - 1) + (destination_len - 1) / 2) / (destination_len - 1)
    }
}

fn destination_cell(source: i32, source_len: i32, destination_len: i32) -> i32 {
    if source_len <= 1 || destination_len <= 1 {
        0
    } else {
        (source * (destination_len - 1) + (source_len - 1) / 2) / (source_len - 1)
    }
}

/// Distinct glyph materials keep armor planes and woven cloth readable even
/// when the caller leaves the actor palette at its kind default.
fn material_ink(kind: ActorKind, glyph: char, base: Rgb) -> Rgb {
    match (kind, glyph) {
        (ActorKind::Player, '▓') => Rgb::new(155, 182, 216), // plate steel
        (ActorKind::Player, '▒') => Rgb::new(112, 92, 128),  // cloth underlayer
        (ActorKind::Player, '=' | '+') => Rgb::new(225, 178, 82), // brass fittings
        (ActorKind::Player, 'o') => Rgb::new(111, 202, 245), // visor glint
        (ActorKind::Npc, '#') => Rgb::new(127, 83, 146),     // woven robe
        (ActorKind::Npc, '=' | '-') => Rgb::new(218, 174, 99), // stitching / wrap
        (ActorKind::Npc, '.') => Rgb::new(221, 198, 159),    // face under hood
        (ActorKind::Monster, 's') => Rgb::new(115, 183, 205), // sternum marker
        (ActorKind::Monster, '#') if base.r > base.g => Rgb::new(222, 225, 213),
        (ActorKind::Monster, '▓') if base.g > base.r => Rgb::new(88, 167, 93),
        (ActorKind::Monster, '@') if base.g > base.r => Rgb::new(202, 228, 115),
        _ => base,
    }
}

fn in_water(x: i32, y: i32) -> bool {
    // Still pond, south-east of the meadow center.
    let dx = x - 30;
    let dy = (y - 14) * 2;
    dx * dx + dy * dy <= 81
}

/// A warm light source repainting nearby ground (reference torch pools:
/// stone/grass glyphs switch to amber tints for ~12 cells, then fall off).
/// Separate from halo wash: pools recolor foregrounds, halos lift
/// backgrounds.
struct Light {
    x: i32,
    y: i32,
    radius: f32,
    tint: Rgb,
}

/// Inverse-square-ish falloff: 1.0 at the source, ~0.08 at 12 cells.
fn light_falloff(dist: f32) -> f32 {
    1.0 / (1.0 + 0.08 * dist * dist)
}

/// Repaint pass over the ground layer: each cell takes the strongest nearby
/// light and mixes its foreground toward the tint (plus a faint background
/// lift). World-locked and deterministic; actors keep their own glow.
fn apply_light_pools(ground: &mut Layer, lights: &[Light]) {
    for cell in &mut ground.cells {
        let mut warmth = 0.0f32;
        let mut tint = Rgb::new(255, 190, 120);
        for light in lights {
            let dist = (((cell.x - light.x) * (cell.x - light.x)
                + (cell.y - light.y) * (cell.y - light.y)) as f32)
                .sqrt();
            if dist > light.radius {
                continue;
            }
            let f = light_falloff(dist);
            if f > warmth {
                warmth = f;
                tint = light.tint;
            }
        }
        if warmth <= 0.0 {
            continue;
        }
        let k = (warmth * 0.7).min(1.0);
        cell.cell.fg = cell.cell.fg.lerp(tint, k);
        cell.cell.bg = cell.cell.bg.lerp(tint.scaled(0.2), k * 0.25);
    }
}

/// Road wander: triangle wave, period 48, rows 7..10 (reference paths
/// wind instead of ruling straight lines).
fn road_center(x: i32) -> i32 {
    let t = x.rem_euclid(48);
    let off = if t < 24 { t / 8 } else { (48 - t) / 8 };
    8 + off - 1
}

/// Ground material by coarse 24-cell blocks: meadow, dark soil, gray stone,
/// teal moss (reference ground shifts palettes instead of one green fill).
fn ground_zone(x: i32, y: i32) -> u32 {
    hash2(x.div_euclid(24), y.div_euclid(24), 311) % 4
}

/// Build window for ranged ground construction: only this rectangle gets
/// stamped (all detail is a pure function of world coords, so ranging is
/// exact and far cheaper than the whole meadow).
#[derive(Clone, Copy, Debug)]
pub struct ViewBounds {
    pub x0: i32,
    pub x1: i32,
    pub y0: i32,
    pub y1: i32,
}

fn push_ground(
    layer: &mut Layer,
    ax: i32,
    ay: i32,
    zone: ZoneKind,
    lights: &[Light],
    bounds: ViewBounds,
) {
    let dungeon = zone == ZoneKind::Dungeon;
    for y in bounds.y0.max(MEADOW_Y0)..bounds.y1.min(MEADOW_Y1) {
        for x in bounds.x0.max(MEADOW_X0)..bounds.x1.min(MEADOW_X1) {
            let v = vignette_factor(x, y, ax, ay);
            let rc = road_center(x);
            if (y - rc).abs() <= 1 {
                // Cobble road (meadow) / flagstone corridor (dungeon).
                let g = if hash2(x, y, 3) % 3 == 0 { '#' } else { '=' };
                let base = if dungeon {
                    Rgb::new(78, 78, 90)
                } else {
                    Rgb::new(95, 88, 70)
                };
                layer.push(AnimCell::still(
                    x,
                    y,
                    Cell::new(g, depth_shade(base, y).scaled(v), BG.scaled(v)),
                ));
                continue;
            }
            if in_water(x, y) {
                layer.push(AnimCell::animated(
                    x,
                    y,
                    Cell::new(
                        '~',
                        Rgb::new(80, 160, 255).scaled(v),
                        Rgb::new(8, 16, 32).scaled(v),
                    ),
                    Anim::still().with(Track::Wave {
                        axis: Axis::X,
                        amp: 1.0,
                        wavelength: 8.0,
                        period: 2.0,
                        phase: x as f32 * 0.7,
                        brighten: 0.2,
                    }),
                ));
                continue;
            }
            // Curb stones edge the winding road.
            if (y - rc).abs() == 2 && hash2(x, y, 5) % 3 == 0 {
                layer.push(AnimCell::still(
                    x,
                    y,
                    Cell::new(
                        '#',
                        depth_shade(Rgb::new(72, 64, 50), y).scaled(v),
                        BG.scaled(v),
                    ),
                ));
                continue;
            }
            // Density falloff: far detail dissolves into flat dark quads
            // instead of dim noise (reference voids are sparse, not blank).
            // Roads, water, and torch pools are exempt landmarks.
            let mut lit = v * void_density(x, y, ax, ay);
            for light in lights {
                let dx = (x - light.x) as f32;
                let dy = (y - light.y) as f32;
                if dx.abs() > light.radius || dy.abs() > light.radius {
                    continue;
                }
                lit = lit.max(light_falloff((dx * dx + dy * dy).sqrt()));
            }
            let keep = (hash2(x, y, 601) % 100) as f32 / 100.0;
            if keep > lit * 1.15 + 0.05 {
                continue;
            }
            // Flow-field strokes: orientation coheres on a coarse lattice so
            // neighbors agree (reference grass reads as flow lines, not hash
            // speckle). Coarse sampling keeps the field stable under camera.
            let flow = match hash2(x.div_euclid(4), y.div_euclid(4), 77) % 3 {
                0 => '/',
                1 => '|',
                _ => '\\',
            };
            // Material zones select the base glyph set and palette; sparse
            // detail breaks the tiling at large grids. All deterministic.
            // Dungeon floors keep pebbles only (no tufts or flowers).
            let speck = hash2(x, y, 91);
            let (g, green) = if speck % 53 == 0 && !dungeon {
                ('"', Rgb::new(72, 132, 82))
            } else if speck % 29 == 0 {
                ('.', Rgb::new(105, 105, 118))
            } else if speck % 41 == 0 && !dungeon {
                ('*', Rgb::new(185, 150, 95))
            } else if dungeon {
                (
                    if hash2(x, y, 17) % 3 == 0 { '.' } else { '`' },
                    Rgb::new(58, 60, 72),
                )
            } else {
                match ground_zone(x, y) {
                    1 => (',', Rgb::new(62, 52, 40)),
                    2 => (
                        if hash2(x, y, 17) % 2 == 0 { '.' } else { '`' },
                        Rgb::new(100, 100, 112),
                    ),
                    3 => (if flow == '|' { ';' } else { '"' }, Rgb::new(64, 128, 118)),
                    _ => (flow, Rgb::new(36 + (hash2(x, y, 17) % 20) as u8, 112, 64)),
                }
            };
            layer.push(AnimCell::still(
                x,
                y,
                Cell::new(g, depth_shade(green, y).scaled(v), BG.scaled(v)),
            ));
        }
    }
}

/// Torch sconce: stick at (x, y), flickering halo flame above it.
fn push_torch(layer: &mut Layer, x: i32, y: i32) {
    layer.push(AnimCell::still(
        x,
        y,
        Cell::new('|', Rgb::new(120, 80, 40), BG),
    ));
    layer.push(AnimCell::animated(
        x,
        y - 1,
        Cell::new('*', Rgb::new(255, 170, 60), Rgb::new(26, 14, 6)).with_glow(1.0),
        Anim::still()
            .with(Track::Pulse {
                low: 0.55,
                high: 1.3,
                period: 1.1,
                phase: 0.0,
            })
            .with(Track::Cycle {
                frames: vec!['*', '+', 'x', '+'],
                fps: 6.0,
            }),
    ));
}

fn push_props(layer: &mut Layer, zone: ZoneKind) {
    if zone == ZoneKind::Dungeon {
        // Sunken vault: north wall band, pillar colonnade, rubble, sconces.
        // Laid around the delve party (y 8..16), not the meadow origin.
        for x in MEADOW_X0..MEADOW_X1 {
            for y in 4..7 {
                let g = if hash2(x, y, 303) % 4 == 0 { '#' } else { '|' };
                layer.push(AnimCell::still(
                    x,
                    y,
                    Cell::new(g, Rgb::new(88, 88, 102), BG),
                ));
            }
        }
        for x in (0..61).step_by(20) {
            for y in 7..17 {
                let g = if y % 2 == 0 { '|' } else { '#' };
                layer.push(AnimCell::still(
                    x,
                    y,
                    Cell::new(g, Rgb::new(96, 96, 110), BG),
                ));
            }
        }
        for (x, y) in [(12, 12), (28, 14), (42, 12)] {
            layer.push(AnimCell::still(
                x,
                y,
                Cell::new('o', Rgb::new(110, 110, 120), BG),
            ));
        }
        push_torch(layer, 12, 8);
        push_torch(layer, 32, 8);
        return;
    }
    // Landmark pines: full art with punched canopies, rim light, and brown
    // trunks (reference foliage breathes instead of blobbing).
    for (x, y) in [(6, 4), (14, 11), (24, 3), (38, 12), (46, 7), (10, 18)] {
        stamp_tree(layer, x, y);
    }
    for (x, y) in [(12, 6), (28, 10), (42, 14)] {
        layer.push(AnimCell::still(
            x,
            y,
            Cell::new('o', Rgb::new(130, 130, 140), BG),
        ));
    }
    push_torch(layer, 20, 5);
}

/// Ambient air: sparse slow-drifting motes with staggered blinks
/// (reference frames all carry spores/fireflies/embers; empty air reads
/// frozen). Stateless hash scatter + wave drift + pulse blink, so it loops
/// forever with no simulation.
fn push_motes(layer: &mut Layer, count: i32) {
    for i in 0..count {
        let span_x = MEADOW_X1 - MEADOW_X0;
        let span_y = MEADOW_Y1 - MEADOW_Y0;
        let x = MEADOW_X0 + (hash2(i, 7, 401) % span_x as u32) as i32;
        let y = MEADOW_Y0 + (hash2(i, 13, 401) % span_y as u32) as i32;
        let warm = i % 2 == 0;
        let glyph = if i % 5 == 0 { '*' } else { '·' };
        let fg = if warm {
            Rgb::new(255, 200, 130)
        } else {
            Rgb::new(170, 220, 255)
        };
        layer.push(AnimCell::animated(
            x,
            y,
            Cell::new(glyph, fg, BG).with_glow(0.3),
            Anim::still()
                .with(Track::Wave {
                    axis: Axis::X,
                    amp: 2.0,
                    wavelength: 12.0,
                    period: 7.0 + (i % 3) as f32,
                    phase: i as f32 * 0.9,
                    brighten: 0.0,
                })
                .with(Track::Pulse {
                    low: 0.25,
                    high: 1.0,
                    period: 2.5 + (i % 4) as f32,
                    phase: i as f32 * 1.7,
                }),
        ));
    }
}

fn push_rain(layer: &mut Layer, x0: i32, x1: i32, y0: i32, y1: i32) {
    for y in y0.max(MEADOW_Y0)..y1.min(MEADOW_Y1) {
        for x in x0.max(MEADOW_X0)..x1.min(MEADOW_X1) {
            if hash2(x, y, 71) % 29 != 0 {
                continue;
            }
            layer.push(AnimCell::animated(
                x,
                y,
                Cell::new('/', Rgb::new(120, 150, 190), BG),
                Anim::still().with(Track::Wave {
                    axis: Axis::X,
                    amp: 1.0,
                    wavelength: 6.0,
                    period: 0.7,
                    phase: x as f32,
                    brighten: 0.0,
                }),
            ));
        }
    }
}
/// Landmark pine: 40-row art with interior darkness punched out by hash,
/// brightened rim rows, and a brown trunk (reference canopies have dark
/// gaps and lit edges, not solid triangles). Mask and regions derive from
/// phase 0 so holes never flicker between sway phases.
fn stamp_tree(layer: &mut Layer, x: i32, y: i32) {
    let frames = cached_frames(TREE_ART);
    let (w, h) = sprite::sprite_size(frames);
    let trunk_top = h - 14;
    let (ox, oy) = (x - w / 2, y - (h - 1));
    let at = |fx: i32, fy: i32| -> char {
        frames
            .first()
            .and_then(|frame| frame.get(fy as usize))
            .and_then(|line| line.chars().nth(fx as usize))
            .unwrap_or(' ')
    };
    for cy in 0..h as usize {
        for cx in 0..w as usize {
            let glyphs: Vec<char> = frames
                .iter()
                .map(|frame| {
                    frame
                        .get(cy)
                        .and_then(|line| line.chars().nth(cx))
                        .unwrap_or(' ')
                })
                .collect();
            if glyphs.iter().all(|g| *g == ' ') {
                continue;
            }
            let fg = if cy as i32 >= trunk_top {
                Rgb::new(135, 85, 50)
            } else {
                let edge = at(cx as i32 - 1, cy as i32) == ' '
                    || at(cx as i32 + 1, cy as i32) == ' '
                    || at(cx as i32, cy as i32 - 1) == ' '
                    || at(cx as i32, cy as i32 + 1) == ' ';
                if !edge
                    && hash2(
                        (ox + cx as i32).div_euclid(2),
                        (oy + cy as i32).div_euclid(2),
                        203,
                    ) % 2
                        == 0
                {
                    // Interior darkness as near-black canopy cells (NOT
                    // skips: a transparent hole would reveal lit ground
                    // behind the tree and stay invisible).
                    layer.push(AnimCell::still(
                        ox + cx as i32,
                        oy + cy as i32,
                        Cell::new(glyphs[0], Rgb::new(10, 18, 14), BG),
                    ));
                    continue;
                }
                let base = Rgb::new(40, 82, 54);
                if edge {
                    base.scaled(1.5)
                } else {
                    base.scaled(0.45)
                }
            };
            let mut cell = Cell::new(glyphs[0], fg, BG);
            cell.bold = true;
            if frames.len() > 1 && glyphs.iter().any(|g| *g != glyphs[0]) {
                layer.push(AnimCell::animated(
                    ox + cx as i32,
                    oy + cy as i32,
                    cell,
                    Anim::still().with(Track::Cycle {
                        frames: glyphs,
                        fps: 1.5,
                    }),
                ));
            } else {
                layer.push(AnimCell::still(ox + cx as i32, oy + cy as i32, cell));
            }
        }
    }
}

/// Mini glyph cluster kept for items (loot glints read better small than
/// as volumetric bodies).
const SWORD: &[(i32, i32, char, Rgb, f32)] = &[
    (0, -2, '/', Rgb::new(245, 250, 255), 0.25),
    (0, -1, '|', Rgb::new(180, 220, 255), 0.2),
    (0, 0, '!', Rgb::new(255, 220, 110), 0.5),
    (-1, 0, '-', Rgb::new(255, 190, 80), 0.0),
    (1, 0, '-', Rgb::new(255, 190, 80), 0.0),
    (0, 1, '|', Rgb::new(170, 110, 55), 0.0),
];

/// Stamp volumetric art foot-anchored: the final sprite row lands on the
/// actor ground cell; the focal cell keeps the palette track and
/// aura glow. Returns the art's top world row for plate anchoring.
fn stamp_art(layer: &mut Layer, a: &SceneActor, source: &'static str, tint: Rgb, aura: f32) -> i32 {
    let frames = cached_frames(source);
    let (source_w, source_h) = sprite::sprite_size(frames);
    let (w, h) = scaled_sprite_size(frames, actor_art_scale(a));
    let (source_fx, source_fy) = focal_of(frames, a.glyph);
    let (fx, fy) = (
        destination_cell(source_fx, source_w, w),
        destination_cell(source_fy, source_h, h),
    );
    let (focal_screen_x, focal_source_x) = if a.facing_x < 0 {
        (w - 1 - fx, source_w - 1 - source_fx)
    } else {
        (fx, source_fx)
    };
    let (ox, oy) = (a.x - w / 2, a.y - (h - 1));
    let palette = a.palette.unwrap_or(ColorTrack::Static(tint));
    // Decode once: the loop below samples every frame per output cell, and
    // `str::chars().nth()` re-walks the line on every probe (the dominant
    // build cost at large grids). Indexing is frame-exact and far cheaper.
    let decoded: Vec<Vec<Vec<char>>> = frames
        .iter()
        .map(|frame| frame.iter().map(|line| line.chars().collect()).collect())
        .collect();
    let probe = |fi: usize, sy: i32, ax: i32| -> char {
        if sy < 0 || ax < 0 {
            return ' ';
        }
        decoded
            .get(fi)
            .and_then(|frame| frame.get(sy as usize))
            .and_then(|line| line.get(ax as usize))
            .copied()
            .unwrap_or(' ')
    };
    let at = |fx: i32, fy: i32| -> char {
        if fx < 0 || fy < 0 || fx >= w || fy >= h {
            return ' ';
        }
        let source_x = if fx == focal_screen_x {
            focal_source_x
        } else {
            source_cell(fx, source_w, w)
        };
        let source_y = if fy == self::destination_cell(source_fy, source_h, h) {
            source_fy
        } else {
            source_cell(fy, source_h, h)
        };
        let art_x = if a.facing_x < 0 {
            source_w - 1 - source_x
        } else {
            source_x
        };
        probe(0, source_y, art_x)
    };
    for cy in 0..h as usize {
        for cx in 0..w as usize {
            let source_x = if cx as i32 == focal_screen_x {
                focal_source_x
            } else {
                source_cell(cx as i32, source_w, w)
            };
            let source_y = if cy as i32 == fy {
                source_fy
            } else {
                source_cell(cy as i32, source_h, h)
            };
            let art_x = if a.facing_x < 0 {
                (source_w - 1 - source_x) as usize
            } else {
                source_x as usize
            };
            // Blank in every phase: skip without allocating. Variance is
            // probed the same way; the glyph Vec only exists for cells that
            // actually animate (moving + varied).
            if (0..decoded.len()).all(|fi| probe(fi, source_y, art_x as i32) == ' ') {
                continue;
            }
            let g0 = probe(0, source_y, art_x as i32);
            // Rim light: edge cells read luminous, interiors recede
            // (reference figures are dark shapes with lit outlines).
            let edge = at(cx as i32 - 1, cy as i32) == ' '
                || at(cx as i32 + 1, cy as i32) == ' '
                || at(cx as i32, cy as i32 - 1) == ' '
                || at(cx as i32, cy as i32 + 1) == ' ';
            let shade = if edge { 1.3 } else { 0.8 };
            let mut cell = match palette {
                ColorTrack::Gradient {
                    start, end, axis, ..
                } => Cell::new(g0, start.scaled(shade), BG).with_fg_gradient(end, axis),
                ColorTrack::Static(color) => {
                    let color = if a.palette.is_some() {
                        color
                    } else {
                        material_ink(a.kind, g0, color)
                    };
                    Cell::new(g0, color.scaled(shade), BG)
                }
                ColorTrack::Rainbow { .. } => Cell::new(g0, tint.scaled(shade), BG),
            };
            cell.bold = true;
            cell.offset_x = a.render_offset.0;
            cell.offset_y = a.render_offset.1;
            let varied = decoded.len() > 1
                && (1..decoded.len()).any(|fi| probe(fi, source_y, art_x as i32) != g0);
            let anim = if a.motion_speed > 0.15 && varied {
                Anim::still().with(Track::Cycle {
                    frames: (0..decoded.len())
                        .map(|fi| probe(fi, source_y, art_x as i32))
                        .collect(),
                    fps: IDLE_FPS + (WALK_FPS - IDLE_FPS) * (a.motion_speed / 4.0).clamp(0.0, 1.0),
                })
            } else if a.motion_speed <= 0.15 {
                Anim::still().with(Track::Pulse {
                    low: 0.97,
                    high: 1.03,
                    period: 2.4,
                    phase: (cx as f32 + cy as f32 * 0.7) * 0.22,
                })
            } else {
                Anim::still()
            };
            let mut placed = if anim.tracks.is_empty() {
                AnimCell::still(ox + cx as i32, oy + cy as i32, cell)
            } else {
                AnimCell::animated(ox + cx as i32, oy + cy as i32, cell, anim)
            };
            if (art_x as i32, source_y) == (source_fx, source_fy) {
                placed = placed.colored(palette);
                placed.cell.glow = aura;
            }
            layer.push(placed);
        }
    }
    oy
}
/// Opaque backing strip plus text: plates stay legible over bright art
/// (reference nameplates always float free of the texture). Spaces inside
/// the text keep the backing instead of punching holes.
fn push_backed_text(layer: &mut Layer, x: i32, y: i32, text: &str, fg: Rgb, offset: (f32, f32)) {
    let w = text.chars().count() as i32;
    let start = layer.cells.len();
    layer.fill_rect(x, y, w, 1, Cell::new(' ', fg, BG));
    layer.push_text(x, y, text, fg, BG);
    for cell in &mut layer.cells[start..] {
        cell.cell.offset_x = offset.0;
        cell.cell.offset_y = offset.1;
    }
}

fn push_actor(
    layer: &mut Layer,
    labels: &mut Layer,
    a: &SceneActor,
    label_rows: &mut Vec<(i32, i32, i32)>,
) {
    // Soft contact shadow: 3-wide dark ellipse on the row below the feet.
    for dx in -1..=1 {
        layer.push(AnimCell::still(
            a.x + dx,
            a.y + 1,
            Cell::new('.', Rgb::new(22, 26, 38), BG)
                .with_offset(a.render_offset.0, a.render_offset.1),
        ));
    }
    // Volumetric art for bodies, mini glyphs for items. Returns the art's
    // top world row so plates float above the head, not the feet.
    let art_top = match art_for(a) {
        Some((source, tint, aura)) => stamp_art(layer, a, source, tint, aura),
        None => {
            for &(dx, dy, glyph, part_fg, glow) in SWORD {
                let mut cell = Cell::new(glyph, part_fg, BG);
                cell.bold = true;
                cell.glow = glow;
                cell.offset_x = a.render_offset.0;
                cell.offset_y = a.render_offset.1;
                layer.push(AnimCell::still(a.x + dx, a.y + dy, cell));
            }
            a.y - 2
        }
    };
    // Place plates above the art top and move colliding plates upward. This is
    // deliberately data-driven rather than assuming demo actors are spaced.
    let w = a.name.chars().count() as i32;
    let x0 = a.x - w / 2;
    let mut label_y = art_top - 1;
    while label_rows
        .iter()
        .any(|&(left, right, y)| y == label_y && x0 <= right && x0 + w > left)
    {
        label_y -= 1;
    }
    label_rows.push((x0, x0 + w, label_y));
    push_backed_text(
        labels,
        x0,
        label_y,
        &a.name,
        Rgb::new(230, 230, 230),
        a.render_offset,
    );
    // HP pips only while damaged (and always for the followed player).
    let damaged = a.hp < a.max_hp;
    let is_player = a.kind == ActorKind::Player;
    if (damaged || is_player) && a.max_hp > 0 {
        let total = 8i32;
        let filled = ((a.hp.max(0) as i64 * total as i64) / a.max_hp.max(1) as i64) as i32;
        let mut bar = String::with_capacity(10);
        bar.push('[');
        for i in 0..total {
            bar.push(if i < filled { '#' } else { '-' });
        }
        bar.push(']');
        let color = if damaged {
            Rgb::new(255, 120, 100)
        } else {
            Rgb::new(140, 230, 120)
        };
        push_backed_text(labels, a.x - 5, label_y - 1, &bar, color, a.render_offset);
        // Register the pips row too, so later actors' plates stack above it
        // instead of overwriting the bar.
        label_rows.push((a.x - 5, a.x + 5, label_y - 1));
    }
}

/// Build the 2.5D scene for `view`: pinned HUD top bar, camera-following
/// world (ground / props / actors / labels / rain), pinned log strip.
/// Returns the scene plus a camera centered on the player.
pub fn scene_from_view(view: &SceneView) -> (Scene, Camera) {
    scene_from_view_sized(view, VIEW_W, HUD_H + VIEW_H + LOG_H)
}

/// Frame-to-frame cache for the static world layers (ground + props).
/// Animation evaluates in `compose`, so caching the build is exact: water
/// sway, torch flicker, and rain tracks all re-sample every frame. Ground
/// bakes player-anchored vignette and light pools, so it rebuilds when the
/// player cell (or zone) changes; props never change. This is what holds 60
/// fps: the meadow build stamps ~50k cells, but composition only samples.
#[derive(Clone, Debug, Default)]
pub struct StaticLayers {
    ground: Option<(i32, i32, ZoneKind, i32, i32, Layer)>,
    props: Option<(ZoneKind, Layer)>,
    rain: Option<(i32, i32, i32, i32, Layer)>,
}

/// Light list for a view: landmark torches (or sconces) plus the player's
/// own lantern, wider underground where it is the only light.
fn lights_for(view: &SceneView) -> Vec<Light> {
    let dungeon = view.zone_kind == ZoneKind::Dungeon;
    let mut lights = if dungeon {
        vec![
            Light {
                x: 12,
                y: 7,
                radius: 12.0,
                tint: Rgb::new(255, 175, 90),
            },
            Light {
                x: 32,
                y: 7,
                radius: 12.0,
                tint: Rgb::new(255, 175, 90),
            },
        ]
    } else {
        vec![Light {
            x: 20,
            y: 4,
            radius: 12.0,
            tint: Rgb::new(255, 175, 90),
        }]
    };
    lights.push(Light {
        x: view.player.x,
        y: view.player.y,
        radius: if dungeon { 16.0 } else { 14.0 },
        tint: Rgb::new(255, 220, 160),
    });
    lights
}

impl StaticLayers {
    pub fn new() -> Self {
        Self::default()
    }

    /// Ground layer, rebuilt only when the player cell or zone changed.
    /// Covers the camera view plus a margin (all ground detail is a pure
    /// function of world coords, so ranging the build is exact).
    pub fn ground_for(
        &mut self,
        view: &SceneView,
        cam_x: i32,
        cam_y: i32,
        width: i32,
        world_h: i32,
    ) -> Layer {
        if let Some((gx, gy, gz, gw, gh, layer)) = &self.ground {
            if *gx == view.player.x
                && *gy == view.player.y
                && *gz == view.zone_kind
                && *gw == width
                && *gh == world_h
            {
                return layer.clone();
            }
        }
        let mut ground = Layer::new("ground", 0);
        let lights = lights_for(view);
        push_ground(
            &mut ground,
            view.player.x,
            view.player.y,
            view.zone_kind,
            &lights,
            ViewBounds {
                x0: cam_x - 8,
                x1: cam_x + width + 8,
                y0: cam_y - 8,
                y1: cam_y + world_h + 8,
            },
        );
        apply_light_pools(&mut ground, &lights);
        self.ground = Some((
            view.player.x,
            view.player.y,
            view.zone_kind,
            width,
            world_h,
            ground.clone(),
        ));
        ground
    }

    /// Props layer: identical every frame for a zone, built once.
    pub fn props_for(&mut self, view: &SceneView) -> Layer {
        if let Some((gz, layer)) = &self.props {
            if *gz == view.zone_kind {
                return layer.clone();
            }
        }
        let mut props = Layer::new("props", 1);
        push_props(&mut props, view.zone_kind);
        self.props = Some((view.zone_kind, props.clone()));
        props
    }

    /// Rain streaks, rebuilt only when the camera window moves. The streak
    /// pattern is a pure function of world coords (the fall animates in
    /// `compose`), so caching is frame-exact. This is the hottest build
    /// cost at large grids: the loop scans the whole camera window plus
    /// margin every frame (measured ~5ms in debug at 384x135).
    pub fn rain_for(&mut self, cam_x: i32, cam_y: i32, width: i32, world_h: i32) -> Layer {
        if let Some((rx, ry, rw, rh, layer)) = &self.rain {
            if *rx == cam_x && *ry == cam_y && *rw == width && *rh == world_h {
                return layer.clone();
            }
        }
        let mut rain = Layer::new("rain", 1);
        push_rain(
            &mut rain,
            cam_x - 8,
            cam_x + width + 8,
            cam_y - 8,
            cam_y + world_h + 8,
        );
        self.rain = Some((cam_x, cam_y, width, world_h, rain.clone()));
        rain
    }
}

/// Build a scene for an arbitrary composed grid. `width`/`height` are the
/// complete frame dimensions, including the pinned HUD and log. Small grids
/// gracefully collapse those panels to one row while retaining a world row.
/// Stage clearance (clarity pack B, windowed edition): a dark opaque floor
/// patch near every actor so silhouettes and combat FX land on quiet
/// negative space instead of tufts, flowers, and road cobbles. The outer
/// shell dithers by world-coordinate hash for a ragged arena edge; props,
/// actors, and plates above stay untouched (the stage renders directly
/// over the ground layer, below props at the same z via insertion order).
fn stage_pass(view: &SceneView) -> Layer {
    let mut stage = Layer::new("stage", 0);
    for actor in std::iter::once(&view.player).chain(view.actors.iter()) {
        for dy in -3..=3 {
            for dx in -3..=3 {
                let dist = ((dx * dx + dy * dy) as f32).sqrt();
                if dist > 3.3 {
                    continue;
                }
                let x = actor.x + dx;
                let y = actor.y + dy;
                if dist > 2.4 && hash2(x, y, 997) % 2 == 0 {
                    continue; // ragged outer shell
                }
                stage.push(AnimCell::still(
                    x,
                    y,
                    Cell::new(' ', STAGE_FG, STAGE_BG)
                        .with_offset(actor.render_offset.0, actor.render_offset.1),
                ));
            }
        }
    }
    stage
}

/// Dashed orange decal ring under the melee-range target's feet (clarity
/// pack C, windowed edition). Dashes march around the ring; the ground-plane
/// ellipse squashes `ry` for the camera's foreshortened read.
fn target_ring_pass(view: &SceneView) -> Option<Layer> {
    let target = melee_target(&view.player, &view.actors)?;
    let mut ring = Layer::new("target-ring", 1);
    let samples: usize = 12;
    let (rx, ry) = (1.9f32, 1.15f32);
    let step = (view.presentation_time_s * 2.4).floor() as i32;
    for i in 0..samples {
        // Half the samples are gaps; the +1 phase shift each step makes
        // the dashes march around the ring (reference telegraph motion).
        if (i + step.unsigned_abs() as usize + 1) % 2 == 0 {
            continue;
        }
        let angle = (i as f32 + step as f32) * 360.0 / samples as f32;
        let radians = angle.to_radians();
        ring.push(AnimCell::still(
            target.x + (rx * radians.cos()).round() as i32,
            target.y + (ry * radians.sin()).round() as i32,
            Cell::new('=', Rgb::new(235, 150, 45), STAGE_BG)
                .with_glow(0.7)
                .with_offset(target.render_offset.0, target.render_offset.1),
        ));
    }
    Some(ring)
}

/// Combat FX sampling (combat-readability pack, REPORT items #2 + #3):
/// telegraphs sweep inward as dashes, then each burst resolves as a solid
/// white-hot core, an expanding ring, and a scatter of fading embers, with
/// optional damage digits rising off the impact. Pure: one explicit-seconds
/// timeline, no retained state.
fn fx_layer_pass(view: &SceneView) -> Layer {
    let t = view.presentation_time_s;
    let mut emitters: Vec<crate::fx::Emitter> = Vec::new();
    let mut cores: Vec<crate::AnimCell> = Vec::new();
    for event in &view.combat_fx {
        let age = t - event.born_s;
        if !(0.0..f32::INFINITY).contains(&age) {
            continue;
        }
        if event.telegraph_s > 0.0 && age < event.telegraph_s {
            emitters.push(crate::fx::Emitter {
                origin: event.origin,
                start: event.born_s,
                lifetime: event.telegraph_s,
                cell: Cell::new('-', event.color, BG).with_glow(0.9),
                kind: crate::fx::Effect::Implosion {
                    count: 12,
                    radius: event.radius,
                },
            });
            continue;
        }
        let born = event.born_s + event.telegraph_s;
        let burst_age = t - born;
        if (0.0..FX_CORE_S).contains(&burst_age) {
            // White-hot solid core: the impact's first read is one bright
            // mass, not a particle cloud.
            let hot = event
                .color
                .lerp(Rgb::new(255, 250, 235), 0.65)
                .scaled(1.0 - burst_age / FX_CORE_S);
            cores.push(crate::AnimCell::still(
                event.origin.0.round() as i32,
                event.origin.1.round() as i32,
                Cell::new('█', hot, STAGE_BG).with_glow(1.2).with_scale(1.6),
            ));
        }
        let ember_fade = 0.55;
        emitters.push(crate::fx::Emitter {
            origin: event.origin,
            start: born,
            lifetime: FX_BURST_S,
            cell: Cell::new('o', event.color, BG).with_glow(0.85),
            kind: crate::fx::Effect::Ring {
                count: 8,
                radius: event.radius.max(1.4),
            },
        });
        emitters.push(crate::fx::Emitter {
            origin: event.origin,
            start: born + 0.06,
            lifetime: FX_BURST_S + 0.1,
            cell: Cell::new('•', event.color, BG).with_glow(ember_fade),
            kind: crate::fx::Effect::Burst {
                count: 14,
                speed: 3.2,
                seed: hash2(
                    (event.origin.0 * 10.0) as i32,
                    (event.origin.1 * 10.0) as i32,
                    71,
                ),
            },
        });
        if !event.digits.is_empty() {
            let shift = event.digits.chars().count() as f32 / 2.0;
            emitters.push(crate::fx::Emitter {
                origin: (event.origin.0 - shift, event.origin.1 - 2.0),
                start: born,
                lifetime: 0.7,
                cell: Cell::new('0', Rgb::new(255, 240, 200), BG).with_glow(1.0),
                kind: crate::fx::Effect::Number {
                    text: event.digits.clone(),
                    rise: 1.1,
                },
            });
        }
    }
    let mut layer = crate::fx::layer("fx", 2, &emitters, t);
    layer.cells.reserve(cores.len());
    for core in cores {
        layer.push(core);
    }
    layer
}

pub fn scene_from_view_sized(view: &SceneView, width: i32, height: i32) -> (Scene, Camera) {
    scene_from_view_sized_cached(view, width, height, &mut StaticLayers::new())
}

/// Cached variant: static ground/props layers come from `cache` (rebuilt
/// only when the player cell or zone changed). Frame-identical to
/// [`scene_from_view_sized`]; examples hold one cache for 60 fps.
pub fn scene_from_view_sized_cached(
    view: &SceneView,
    width: i32,
    height: i32,
    cache: &mut StaticLayers,
) -> (Scene, Camera) {
    let extra_hud = (view.boss.is_some() as i32) * 3 + (view.target.is_some() as i32) * 3;
    let (width, hud_h, world_h, log_h) = layout(width, height, extra_hud);
    let dungeon = view.zone_kind == ZoneKind::Dungeon;
    // Camera first: ground and rain build only the visible range plus a
    // margin (all detail is a pure function of world coords, so this is
    // exact and an order of magnitude fewer cells than the whole meadow).
    // Aim at the body's center above the foot contact, so the full model sits
    // in the viewport rather than pushing its head off the top edge.
    let player_frames = art_for(&view.player).map(|(source, _, _)| cached_frames(source));
    let player_half_height = player_frames
        .map(|frames| scaled_sprite_size(frames, actor_art_scale(&view.player)).1 / 2)
        .unwrap_or(0);
    let camera = Camera {
        x: view.player.x - width / 2,
        y: view.player.y - world_h / 2 - player_half_height,
        ox: 0.0,
        oy: 0.0,
    };
    let mut scene = Scene::default();

    let mut hud = Layer::new("hud", 4);
    hud.fill_rect(0, 0, width, hud_h, Cell::new(' ', HUD_DIM, PANEL_BG));
    let mut row = 0;
    if let Some(boss) = &view.boss {
        // Big label occupies two rows; bar cells sit under it.
        if row + 2 < hud_h {
            let mut label = format!("{}  {}/{}", boss.name, boss.hp.max(0), boss.max_hp.max(1));
            if let Some(phase) = &boss.phase {
                label = format!(
                    "{} - {}  {}/{}",
                    boss.name,
                    phase,
                    boss.hp.max(0),
                    boss.max_hp.max(1)
                );
            }
            push_hud_text(
                &mut scene,
                row,
                width,
                &label,
                Rgb::new(255, 120, 110),
                true,
            );
            push_bar(
                &mut hud,
                row + 2,
                width,
                boss.hp,
                boss.max_hp,
                BarStyle {
                    fill: Rgb::new(220, 50, 40),
                    empty: Rgb::new(90, 25, 22),
                    bg: PANEL_BG,
                },
            );
            row += 3;
        }
    }
    if let Some(target) = &view.target {
        if row + 2 < hud_h {
            push_hud_text(
                &mut scene,
                row,
                width,
                &format!(
                    "Target: {}  {}/{}",
                    target.name,
                    target.hp.max(0),
                    target.max_hp.max(1)
                ),
                Rgb::new(235, 235, 235),
                false,
            );
            push_bar(
                &mut hud,
                row + 2,
                width,
                target.hp,
                target.max_hp,
                BarStyle {
                    fill: Rgb::new(225, 225, 225),
                    empty: Rgb::new(70, 70, 78),
                    bg: PANEL_BG,
                },
            );
            row += 3;
        }
    }
    if row + 1 < hud_h {
        push_hud_text(
            &mut scene,
            row,
            width,
            &format!(
                "{}  HP {}/{}  Tick {}",
                view.zone, view.player.hp, view.player.max_hp, view.tick
            ),
            HUD_FG,
            false,
        );
        row += 2;
    }
    if row < hud_h {
        fit_text(
            &mut hud,
            1,
            row,
            width - 2,
            "[WASD] move   [F] attack   [1-3] cast   [R] rest   [Q/Esc] quit",
            Rgb::new(110, 130, 150),
            false,
        );
    }
    scene.layers.push(hud);

    let ground = cache.ground_for(view, camera.x, camera.y, width, world_h);
    scene.layers.push(ground);
    scene.layers.push(stage_pass(view));

    let props = cache.props_for(view);
    scene.layers.push(props);

    let mut actors = Layer::new("actors", 2);
    let mut labels = Layer::new("labels", 3);
    let mut label_rows = Vec::new();
    push_actor(&mut actors, &mut labels, &view.player, &mut label_rows);
    for a in &view.actors {
        push_actor(&mut actors, &mut labels, a, &mut label_rows);
    }
    scene.layers.push(actors);
    scene.layers.push(labels);

    // Calm underground: no rain streaks in the vault (reference dungeon
    // footage holds still whole seconds; flashes live outside).
    if !dungeon {
        scene
            .layers
            .push(cache.rain_for(camera.x, camera.y, width, world_h));
    }

    // Front air: drifting motes pass in front of actors (fireflies cross
    // silhouettes in the reference). Sparse cool dust underground.
    let mut air = Layer::new("air", 2);
    push_motes(&mut air, if dungeon { 10 } else { 44 });
    scene.layers.push(air);

    // Combat FX above the actor bodies; the target decal below them (both
    // tie z with existing layers, so insertion order pins draw order).
    scene.layers.push(fx_layer_pass(view));
    if let Some(ring) = target_ring_pass(view) {
        scene.layers.push(ring);
    }

    let mut log = Layer::new("log", 4);
    log.fill_rect(0, 0, width, log_h, Cell::new(' ', HUD_DIM, PANEL_BG));
    for (i, message) in view.messages.iter().rev().take(log_h as usize).enumerate() {
        fit_text(&mut log, 1, i as i32, width - 2, message, HUD_DIM, false);
    }
    scene.layers.push(log);

    let hud_idx = scene.layer("hud").unwrap();
    let ground_idx = scene.layer("ground").unwrap();
    let stage_idx = scene.layer("stage").unwrap();
    let props_idx = scene.layer("props").unwrap();
    let actors_idx = scene.layer("actors").unwrap();
    let labels_idx = scene.layer("labels").unwrap();
    let air_idx = scene.layer("air").unwrap();
    let log_idx = scene.layer("log").unwrap();
    let mut world_layers = vec![
        ground_idx, stage_idx, props_idx, actors_idx, air_idx, labels_idx,
    ];
    if let Some(ring_idx) = scene.layer("target-ring") {
        world_layers.push(ring_idx);
    }
    let fx_idx = scene.layer("fx").unwrap();
    world_layers.push(fx_idx);
    if !dungeon {
        world_layers.push(scene.layer("rain").unwrap());
    }
    scene.partitions.push(Partition::new(
        "hud",
        0,
        0,
        width,
        hud_h,
        vec![hud_idx],
        false,
    ));
    scene.partitions.push(Partition::new(
        "world",
        0,
        hud_h,
        width,
        world_h,
        world_layers,
        true,
    ));
    scene.partitions.push(Partition::new(
        "log",
        0,
        hud_h + world_h,
        width,
        log_h,
        vec![log_idx],
        false,
    ));

    (scene, camera)
}

/// Demo view mirroring the terminal client's `sample_world`: an adventurer,
/// two monsters, an NPC, and an item, so the `game` example shows the same
/// cast as `--demo` without a server.
pub fn demo_view() -> SceneView {
    SceneView {
        player: SceneActor {
            x: 20,
            y: 14,
            render_offset: (0.0, 0.0),
            motion_speed: 0.0,
            facing_x: 1,
            kind: ActorKind::Player,
            name: "Adventurer".into(),
            glyph: '@',
            hp: 20,
            max_hp: 20,
            palette: Some(ColorTrack::Gradient {
                start: Rgb::new(170, 255, 90),
                end: Rgb::new(90, 220, 255),
                axis: Axis::X,
                span: 8.0,
            }),
        },
        actors: vec![
            SceneActor {
                x: 52,
                y: 14,
                render_offset: (0.0, 0.0),
                motion_speed: 0.0,
                facing_x: 1,
                kind: ActorKind::Monster,
                name: "Mossling".into(),
                glyph: 'm',
                hp: 6,
                max_hp: 8,
                palette: Some(ColorTrack::Static(Rgb::new(120, 220, 120))),
            },
            SceneActor {
                x: 76,
                y: 18,
                render_offset: (0.0, 0.0),
                motion_speed: 0.0,
                facing_x: 1,
                kind: ActorKind::Monster,
                name: "Skeleton".into(),
                glyph: 's',
                hp: 10,
                max_hp: 10,
                palette: Some(ColorTrack::Rainbow {
                    period: 2.4,
                    phase: 0.0,
                    saturation: 0.85,
                    value: 1.0,
                }),
            },
            SceneActor {
                x: -6,
                y: 16,
                render_offset: (0.0, 0.0),
                motion_speed: 0.0,
                facing_x: 1,
                kind: ActorKind::Npc,
                name: "Herbalist".into(),
                glyph: 'h',
                hp: 12,
                max_hp: 12,
                palette: Some(ColorTrack::Gradient {
                    start: Rgb::new(200, 170, 255),
                    end: Rgb::new(255, 170, 220),
                    axis: Axis::Y,
                    span: 6.0,
                }),
            },
            SceneActor {
                x: 36,
                y: 9,
                render_offset: (0.0, 0.0),
                motion_speed: 0.0,
                facing_x: 1,
                kind: ActorKind::Item,
                name: "Iron Sword".into(),
                glyph: '!',
                hp: 1,
                max_hp: 1,
                palette: Some(ColorTrack::Rainbow {
                    period: 1.2,
                    phase: 1.0,
                    saturation: 0.7,
                    value: 1.0,
                }),
            },
        ],
        messages: vec![
            "Glyph scene ready. Windowed 2.5D port of the terminal game.".into(),
            "Mossling blocks the road east.".into(),
        ],
        tick: 128,
        zone: "Whispering Meadow".into(),
        zone_kind: ZoneKind::Meadow,
        boss: Some(BossPanel {
            // Reference fixture values (reconstruction docs): shown to prove
            // the bar/phase layout, not live game balance.
            name: "Dragon".into(),
            hp: 4990,
            max_hp: 6000,
            phase: Some("phase 2".into()),
        }),
        target: Some(TargetPanel {
            name: "Mossling".into(),
            hp: 6,
            max_hp: 8,
        }),
        combat_fx: Vec::new(),
        presentation_time_s: 0.0,
    }
}

/// Sunken-vault fixture: the same hero delving with a skeleton guardian,
/// sconce light, and no boss or target panels (reference dungeon footage
/// shows neither).
pub fn dungeon_view() -> SceneView {
    SceneView {
        player: SceneActor {
            x: 20,
            y: 14,
            render_offset: (0.0, 0.0),
            motion_speed: 0.0,
            facing_x: 1,
            kind: ActorKind::Player,
            name: "Adventurer".into(),
            glyph: '@',
            hp: 20,
            max_hp: 20,
            palette: None,
        },
        actors: vec![
            SceneActor {
                x: 34,
                y: 12,
                render_offset: (0.0, 0.0),
                motion_speed: 0.0,
                facing_x: 1,
                kind: ActorKind::Monster,
                name: "Skeleton".into(),
                glyph: 's',
                hp: 10,
                max_hp: 10,
                palette: None,
            },
            SceneActor {
                x: 28,
                y: 8,
                render_offset: (0.0, 0.0),
                motion_speed: 0.0,
                facing_x: 1,
                kind: ActorKind::Item,
                name: "Rusty Stiletto".into(),
                glyph: '!',
                hp: 1,
                max_hp: 1,
                palette: None,
            },
        ],
        messages: vec![
            "The vault air is still. Your lantern is the only light.".into(),
            "Something scrapes stone to the east.".into(),
        ],
        tick: 64,
        zone: "The Sunken Vault".into(),
        zone_kind: ZoneKind::Dungeon,
        boss: None,
        target: None,
        combat_fx: Vec::new(),
        presentation_time_s: 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::super::compose;
    use super::*;

    fn frame_rows(view: &SceneView, t: f32) -> Vec<String> {
        let (scene, camera) = scene_from_view(view);
        compose(&scene, &camera, t).rows()
    }

    /// World-partition center in frame coords (HUD rows shift with boss /
    /// target panels, so tests derive geometry from the scene itself).
    fn world_center(view: &SceneView) -> (i32, i32) {
        let (scene, _) = scene_from_view(view);
        let world = scene
            .partitions
            .iter()
            .find(|p| p.name == "world")
            .expect("world partition");
        (world.x + world.w / 2, world.y + world.h / 2)
    }

    fn player_focal_center(view: &SceneView) -> (i32, i32) {
        let (cx, cy) = world_center(view);
        let frames = cached_frames(WARRIOR_ART);
        let (width, height) = scaled_sprite_size(frames, actor_art_scale(&view.player));
        let (source_fx, source_fy) = focal_of(frames, view.player.glyph);
        let (focal_x, focal_y) = (
            destination_cell(source_fx, sprite::sprite_size(frames).0, width),
            destination_cell(source_fy, sprite::sprite_size(frames).1, height),
        );
        (
            cx + focal_x - width / 2,
            cy + focal_y - (height - 1) + height / 2,
        )
    }

    #[test]
    fn player_stays_centered_and_named() {
        let view = demo_view();
        let rows = frame_rows(&view, 0.0);
        assert_eq!(rows.len(), (HUD_H + VIEW_H + LOG_H) as usize);
        let (cx, cy) = player_focal_center(&view);
        assert_eq!(rows[cy as usize].chars().nth(cx as usize).unwrap(), '@');
        assert!(
            rows.iter().any(|row| row.contains("Adventurer")),
            "nameplate floats above"
        );
        let (vitals, _) = scene_from_view(&view);
        assert!(
            vitals
                .overlay_text
                .iter()
                .any(|run| run.text.contains("HP 20/20")),
            "HUD shows player vitals"
        );
    }

    #[test]
    fn camera_follows_the_player() {
        let mut view = demo_view();
        view.player.x += 6;
        let rows = frame_rows(&view, 0.0);
        let (cx, cy) = player_focal_center(&view);
        assert_eq!(rows[cy as usize].chars().nth(cx as usize).unwrap(), '@');
    }

    #[test]
    fn damaged_monster_shows_hp_pips() {
        let rows = frame_rows(&demo_view(), 0.0);
        assert!(
            rows.iter().any(|r| r.contains("[######--]")),
            "6/8 mossling shows 6 of 8 pips"
        );
    }

    #[test]
    fn scene_is_deterministic_but_alive() {
        let view = demo_view();
        assert_eq!(frame_rows(&view, 1.0), frame_rows(&view, 1.0));
        assert_ne!(
            frame_rows(&view, 0.0),
            frame_rows(&view, 0.6),
            "rain/water/torch animate with time"
        );
    }

    #[test]
    fn actor_palette_changes_with_time() {
        let mut view = demo_view();
        view.actors.clear();
        view.player.palette = Some(ColorTrack::Rainbow {
            period: 2.0,
            phase: 0.0,
            saturation: 1.0,
            value: 1.0,
        });
        let (cx, cy) = player_focal_center(&view);
        let (scene, camera) = scene_from_view(&view);
        let a = compose(&scene, &camera, 0.0)
            .get(cx, cy)
            .expect("player cell");
        let b = compose(&scene, &camera, 0.5)
            .get(cx, cy)
            .expect("player cell");
        assert_eq!(a.glyph, '@');
        assert_eq!(b.glyph, '@');
        assert_ne!(a.fg, b.fg, "rainbow palette must animate actor colour");
    }

    #[test]
    fn dynamic_actor_glow_tints_nearby_background() {
        let mut view = demo_view();
        view.actors.clear();
        view.player.palette = Some(ColorTrack::Static(Rgb::new(255, 30, 20)));
        let (cx, cy) = player_focal_center(&view);
        let (scene, camera) = scene_from_view(&view);
        let frame = compose(&scene, &camera, 0.0);
        let adjacent = frame.get(cx + 1, cy).expect("ground beside player");
        assert_ne!(
            adjacent.bg, BG,
            "actor glow should spill into nearby ground"
        );
        assert!(
            adjacent.bg.r > BG.r,
            "spill carries the actor palette's red channel"
        );
    }

    #[test]
    fn actor_gradient_reaches_composed_frame_data() {
        let mut view = demo_view();
        view.actors.clear();
        let (cx, cy) = player_focal_center(&view);
        let (scene, camera) = scene_from_view(&view);
        let cell = compose(&scene, &camera, 0.0)
            .get(cx, cy)
            .expect("player focal cell");
        assert_eq!(cell.glyph, '@');
        assert_eq!(cell.fg_end, Rgb::new(90, 220, 255));
        assert_eq!(cell.fg_gradient, Some(Axis::X));
    }

    #[test]
    fn sized_scene_bounds_follow_requested_grid() {
        let view = demo_view();
        let (scene, camera) = scene_from_view_sized(&view, 37, 12);
        let frame = compose(&scene, &camera, 0.0);
        assert_eq!((frame.w, frame.h), (37, 12));
        assert!(scene.partitions.iter().all(|p| p.x >= 0 && p.y >= 0));
        assert!(scene
            .partitions
            .iter()
            .all(|p| p.x + p.w <= 37 && p.y + p.h <= 12));
    }

    #[test]
    fn actor_models_stamp_multiple_distinct_cells() {
        let view = demo_view();
        // Tall frame: the 27-row warrior helm must fit, not clip.
        let (scene, camera) = scene_from_view_sized(&view, 64, 40);
        let frame = compose(&scene, &camera, 0.0);
        let actor_glyphs = frame
            .rows()
            .into_iter()
            .flat_map(|row| row.chars().collect::<Vec<_>>())
            .filter(|glyph| matches!(glyph, '@' | 'm' | 's' | 'H' | '!'))
            .count();
        assert!(
            actor_glyphs >= 5,
            "model focal glyphs should all be visible"
        );
        // Boss-free tall view: HUD shrinks to 3 rows, so the scaled helm
        // clears the world partition top edge.
        let mut plain = demo_view();
        plain.boss = None;
        plain.target = None;
        let (scene, camera) = scene_from_view_sized(&plain, 64, 44);
        let rows = compose(&scene, &camera, 0.0).rows();
        // Volumetric body around the focal chest: '@' plus brass '=' armor
        // fittings nearby prove armor stamps, not a bare marker glyph.
        // (Exact helm text does not survive 0.72 downscale mapping.)
        let focal_row = rows
            .iter()
            .position(|row| row.contains('@'))
            .expect("focal chest stamps");
        assert!(
            rows[focal_row..]
                .iter()
                .take(4)
                .any(|row| row.contains('=')),
            "armor fittings render below the focal chest"
        );
    }

    #[test]
    fn focal_glyph_stays_steady_every_phase() {
        // Volumetric art pins the kind's focal glyph at the actor position in
        // every phase (camera/palette/glow contracts depend on it).
        let mut view = demo_view();
        view.actors.clear();
        let (cx, cy) = player_focal_center(&view);
        let (scene, camera) = scene_from_view(&view);
        for t in [0.0, 0.17, 0.34, 0.5, 0.67, 0.84] {
            let cell = compose(&scene, &camera, t)
                .get(cx, cy)
                .expect("player focal cell");
            assert_eq!(cell.glyph, '@', "focal glyph must hold at t={t}");
        }
    }

    #[test]
    fn boss_bar_shows_name_phase_and_proportional_fill() {
        let view = demo_view();
        let (scene, camera) = scene_from_view(&view);
        let texts: Vec<&str> = scene
            .overlay_text
            .iter()
            .map(|run| run.text.as_str())
            .collect();
        assert!(
            texts
                .iter()
                .any(|t| t.contains("Dragon") && t.contains("phase 2")),
            "boss label carries the phase"
        );
        assert!(
            texts.iter().any(|t| t.contains("4990/6000")),
            "boss HP numbers render"
        );
        assert!(
            scene
                .overlay_text
                .iter()
                .filter(|run| run.text.contains("Dragon"))
                .all(|run| run.size == 2.0),
            "boss readout renders big"
        );
        let rows = compose(&scene, &camera, 0.0).rows();
        let bar = rows
            .iter()
            .find(|row| row.starts_with('[') && row.ends_with(']') && row.len() > 20)
            .expect("full-width boss bar");
        let inner: String = bar.chars().skip(1).take(bar.len() - 2).collect();
        let held = inner.chars().filter(|&g| g == '#').count();
        let ratio = held as f32 / inner.chars().count() as f32;
        assert!(
            (ratio - 4990.0 / 6000.0).abs() < 0.05,
            "boss fill matches HP ratio, got {ratio}"
        );
    }

    #[test]
    fn target_panel_shows_selection_with_numbers() {
        let (scene, _) = scene_from_view(&demo_view());
        let texts: Vec<&str> = scene
            .overlay_text
            .iter()
            .map(|run| run.text.as_str())
            .collect();
        assert!(
            texts.iter().any(|t| t.contains("Target: Mossling")),
            "target name renders"
        );
        assert!(
            texts.iter().any(|t| t.contains("6/8")),
            "target HP numbers render"
        );
    }

    #[test]
    fn dungeon_renders_darker_than_meadow() {
        fn mean_bg(view: &SceneView) -> f32 {
            let (scene, camera) = scene_from_view(view);
            let frame = compose(&scene, &camera, 0.0);
            let mut sum = 0u32;
            let mut n = 0u32;
            for cell in &frame.cells {
                if !cell.transparent {
                    sum += cell.bg.r as u32 + cell.bg.g as u32 + cell.bg.b as u32;
                    n += 1;
                }
            }
            sum as f32 / n.max(1) as f32
        }
        let meadow = mean_bg(&demo_view());
        let dungeon = mean_bg(&dungeon_view());
        assert!(
            dungeon < meadow,
            "vault must read darker: dungeon={dungeon:.1} meadow={meadow:.1}"
        );
    }

    #[test]
    fn dungeon_has_walls_no_rain_and_cool_dust() {
        let (scene, camera) = scene_from_view(&dungeon_view());
        assert!(
            scene.layers.iter().all(|l| l.name != "rain"),
            "vault air holds still"
        );
        let frame = compose(&scene, &camera, 0.0);
        let wall_rows = frame
            .rows()
            .iter()
            .filter(|row| row.chars().filter(|&g| g == '#').count() > 15)
            .count();
        assert!(wall_rows >= 2, "north wall band spans the vault");
    }

    #[test]
    fn panels_shrink_the_world_and_plates_keep_clear() {
        let view = demo_view();
        let (scene, _) = scene_from_view(&view);
        let world = scene
            .partitions
            .iter()
            .find(|p| p.name == "world")
            .expect("world partition");
        // 96x35 with boss + target: 9 HUD rows, 2 log, 24 world.
        assert_eq!((world.y, world.h), (9, 24));
        // Mossling pips survive plate stacking (pips rows are registered
        // in the declutter set now).
        let rows = frame_rows(&view, 0.0);
        assert!(
            rows.iter().any(|r| r.contains("[######--]")),
            "damaged mossling pips stay legible"
        );
    }

    #[test]
    fn overlay_text_is_clipped_to_narrow_frame() {
        let view = demo_view();
        let (scene, camera) = scene_from_view_sized(&view, 16, 8);
        let frame = compose(&scene, &camera, 0.0);
        assert_eq!((frame.w, frame.h), (16, 8));
        assert!(frame.rows().iter().all(|row| row.chars().count() == 16));
        assert!(frame.rows()[0].chars().count() == 16);
    }

    #[test]
    fn default_frame_is_denser_grid() {
        let (scene, camera) = scene_from_view(&demo_view());
        let frame = compose(&scene, &camera, 0.0);
        assert_eq!((frame.w, frame.h), (VIEW_W, HUD_H + VIEW_H + LOG_H));
        assert_eq!((VIEW_W, VIEW_H, HUD_H), (96, 30, 3));
    }

    #[test]
    fn actor_art_phases_share_height() {
        // Feet must sit on the same row every phase or they flicker.
        for source in [WARRIOR_ART, MOSSLING_ART, SKELETON_ART, HERBALIST_ART] {
            let frames = sprite::parse_frames(source);
            assert_eq!(frames.len(), 4);
            let height = frames[0].len();
            assert!(
                frames.iter().all(|frame| frame.len() == height),
                "art phases must share height"
            );
        }
    }

    #[test]
    fn actor_models_are_ground_anchored_and_smaller_than_pines() {
        let warrior = sprite::parse_frames(WARRIOR_ART);
        let skeleton = sprite::parse_frames(SKELETON_ART);
        let mossling = sprite::parse_frames(MOSSLING_ART);
        let pine = sprite::parse_frames(TREE_ART);
        let height = |frames: &[Vec<String>]| frames[0].len() as i32;
        let actor = demo_view().player;
        let warrior_height = scaled_sprite_size(&warrior, actor_art_scale(&actor)).1;
        let skeleton_height = scaled_sprite_size(
            &skeleton,
            actor_art_scale(&SceneActor {
                kind: ActorKind::Monster,
                glyph: 's',
                name: "Skeleton".into(),
                ..actor.clone()
            }),
        )
        .1;
        let mossling_height = scaled_sprite_size(
            &mossling,
            actor_art_scale(&SceneActor {
                kind: ActorKind::Monster,
                glyph: 'm',
                name: "Mossling".into(),
                ..actor.clone()
            }),
        )
        .1;
        assert!(warrior_height <= height(&pine) / 2);
        assert!(skeleton_height < warrior_height + 2);
        assert!(mossling_height < skeleton_height);

        let mut layer = Layer::new("actor", 2);
        let top = stamp_art(
            &mut layer,
            &actor,
            WARRIOR_ART,
            Rgb::new(200, 200, 200),
            0.0,
        );
        assert_eq!(top + warrior_height - 1, actor.y);
        assert!(layer.cells.iter().any(|cell| cell.y == actor.y));
        assert!(layer.cells.iter().all(|cell| cell.y <= actor.y));
    }

    #[test]
    fn moving_actor_cells_carry_fractional_offset_and_mirrored_focal_glyph() {
        let mut actor = demo_view().player;
        actor.render_offset = (0.35, -0.2);
        actor.motion_speed = 4.0;
        actor.facing_x = -1;
        let frames = cached_frames(WARRIOR_ART);
        let (w, h) = scaled_sprite_size(frames, actor_art_scale(&actor));
        let (source_fx, source_fy) = focal_of(frames, actor.glyph);
        let (fx, fy) = (
            destination_cell(source_fx, sprite::sprite_size(frames).0, w),
            destination_cell(source_fy, sprite::sprite_size(frames).1, h),
        );
        let (ox, oy) = (actor.x - w / 2, actor.y - (h - 1));
        let mut layer = Layer::new("actor", 2);
        stamp_art(
            &mut layer,
            &actor,
            WARRIOR_ART,
            Rgb::new(200, 200, 200),
            0.0,
        );
        let focal = layer
            .cells
            .iter()
            .find(|cell| cell.x == ox + w - 1 - fx && cell.y == oy + fy)
            .expect("mirrored chest focal cell");
        assert_eq!(focal.cell.glyph, '@');
        assert_eq!(
            (focal.cell.offset_x, focal.cell.offset_y),
            actor.render_offset
        );
        assert!(layer.cells.iter().any(|cell| matches!(
            cell.anim.tracks.last(),
            Some(Track::Cycle { fps, .. }) if (*fps - WALK_FPS).abs() < 0.001
        )));

        actor.motion_speed = 0.0;
        let mut idle = Layer::new("idle", 2);
        stamp_art(&mut idle, &actor, WARRIOR_ART, Rgb::new(200, 200, 200), 0.0);
        assert!(idle.cells.iter().any(|cell| matches!(
            cell.anim.tracks.last(),
            Some(Track::Pulse { period, .. }) if (*period - 2.4).abs() < 0.001
        )));
        assert!(!idle
            .cells
            .iter()
            .any(|cell| matches!(cell.anim.tracks.last(), Some(Track::Cycle { .. }))));
    }

    #[test]
    fn landmark_trees_use_full_pine_art() {
        let rows = frame_rows(&demo_view(), 0.0);
        assert!(
            rows.iter().any(|row| row.contains("/▓")),
            "pine canopy art should be stamped"
        );
    }

    #[test]
    fn tree_canopy_has_holes_and_rim_shading() {
        use std::collections::HashSet;
        // Union of non-space art cells across all phases (the stampable set).
        let frames = sprite::parse_frames(TREE_ART);
        let (w, h) = sprite::sprite_size(&frames);
        let mut union = HashSet::new();
        for cy in 0..h as usize {
            for cx in 0..w as usize {
                if frames.iter().any(|frame| {
                    frame
                        .get(cy)
                        .and_then(|line| line.chars().nth(cx))
                        .is_some_and(|g| g != ' ')
                }) {
                    union.insert((cx as i32, cy as i32));
                }
            }
        }
        let mut layer = Layer::new("props", 1);
        stamp_tree(&mut layer, 24, 3);
        let stamped = layer.cells.len();
        // Every art position stamps exactly one cell (art or interior
        // darkness): nothing lost, nothing doubled.
        assert_eq!(
            stamped,
            union.len(),
            "each art cell accounted once, got {stamped} vs {}",
            union.len()
        );
        let bright = layer
            .cells
            .iter()
            .filter(|c| c.cell.fg.r as u32 + c.cell.fg.g as u32 > 150)
            .count();
        let dim = layer
            .cells
            .iter()
            .filter(|c| c.cell.fg.r as u32 + c.cell.fg.g as u32 <= 100)
            .count();
        let dark = layer
            .cells
            .iter()
            .filter(|c| c.cell.fg == Rgb::new(10, 18, 14))
            .count();
        assert!(
            bright > 0 && dim > 0,
            "rim/interior brightness split, bright={bright} dim={dim}"
        );
        assert!(dark > 20, "interior darkness must exist, got {dark}");
    }

    #[test]
    fn vignette_falls_off_with_distance_and_floors() {
        // Dither keeps full light near-exact, never over.
        assert!((vignette_factor(0, 0, 0, 0) - 1.0).abs() < 0.05);
        assert!((vignette_factor(10, 0, 0, 0) - 1.0).abs() < 0.05);
        assert!(vignette_factor(0, 0, 0, 0) <= 1.0);
        let mid = vignette_factor(45, 0, 0, 0);
        let far = vignette_factor(200, 0, 0, 0);
        assert!(mid < 1.0 && mid > far, "light fades with distance");
        assert!((far - 0.25).abs() < 0.05, "floor keeps far ground readable");
    }

    #[test]
    fn vignette_has_no_hard_ellipse_edge() {
        // Smoothstep falls gradually (no linear kink) and dither breaks
        // banding: average step small, distinct values many.
        let band: Vec<f32> = (26..64).map(|d| vignette_factor(d, 0, 0, 0)).collect();
        let drop = band[0] - band[band.len() - 1];
        assert!(
            drop / (band.len() as f32) < 0.03,
            "falloff is gradual overall, drop={drop}"
        );
        let distinct = band
            .iter()
            .map(|f| (f * 100.0) as i32)
            .collect::<std::collections::HashSet<_>>();
        assert!(distinct.len() > 10, "dither breaks banding");
    }

    #[test]
    fn road_wanders_smoothly_off_the_straight_line() {
        assert_eq!(road_center(0), 7);
        let centers: Vec<i32> = (0..96).map(road_center).collect();
        assert!(
            centers.iter().any(|&y| y != 8),
            "road must leave the ruler line"
        );
        assert!(
            centers.iter().all(|&y| (7..=10).contains(&y)),
            "wander stays in a narrow band"
        );
        assert!(
            centers.windows(2).all(|w| (w[1] - w[0]).abs() <= 1),
            "no vertical jumps between columns"
        );
    }

    #[test]
    fn ground_zones_cover_all_materials() {
        let mut seen = [false; 4];
        for x in (-96..96).step_by(3) {
            for y in (-32..64).step_by(3) {
                seen[ground_zone(x, y) as usize] = true;
            }
        }
        assert!(seen.iter().all(|&s| s), "all four zones occur");
    }

    #[test]
    fn plate_backing_fills_spaces_opaque() {
        let mut scene = Scene::default();
        let mut layer = Layer::new("labels", 3);
        push_backed_text(
            &mut layer,
            10,
            4,
            "A B",
            Rgb::new(230, 230, 230),
            (0.0, 0.0),
        );
        scene.layers.push(layer);
        scene
            .partitions
            .push(Partition::new("all", 0, 0, 20, 10, vec![0], false));
        let frame = compose(&scene, &Camera::default(), 0.0);
        // The space between words keeps its backing instead of punching a
        // hole to the layers below.
        let gap = frame.get(11, 4).expect("backed space cell");
        assert!(!gap.transparent);
        assert_eq!(gap.bg, BG);
        assert_eq!(frame.get(10, 4).unwrap().glyph, 'A');
    }

    #[test]
    fn ambient_motes_drift_through_the_view() {
        let count_at = |t: f32| {
            frame_rows(&demo_view(), t)
                .iter()
                .map(|row| row.chars().filter(|&g| g == '·').count())
                .sum::<usize>()
        };
        assert!(count_at(0.0) > 0, "motes visible at start");
        assert!(count_at(5.0) > 0, "motes still drifting later");
    }

    #[test]
    fn ground_detail_dissolves_with_distance_but_pools_hold() {
        // Density follows light: near ground dense, far ground sparse,
        // torch pools and roads exempt.
        let lights = [Light {
            x: 0,
            y: 0,
            radius: 12.0,
            tint: Rgb::new(255, 175, 90),
        }];
        let mut layer = Layer::new("ground", 0);
        push_ground(
            &mut layer,
            0,
            0,
            ZoneKind::Meadow,
            &lights,
            ViewBounds {
                x0: MEADOW_X0,
                x1: MEADOW_X1,
                y0: MEADOW_Y0,
                y1: MEADOW_Y1,
            },
        );
        let near = layer
            .cells
            .iter()
            .filter(|c| c.x.abs() <= 10 && c.y.abs() <= 10)
            .count();
        let far = layer
            .cells
            .iter()
            .filter(|c| (c.x - 100).abs() <= 10 && (c.y - 60).abs() <= 10)
            .count();
        assert!(
            far * 2 < near,
            "far field must thin out: near={near} far={far}"
        );
        // Roads punch through everywhere (navigation landmarks survive).
        assert!(
            layer
                .cells
                .iter()
                .any(|c| c.y == road_center(c.x) && c.x > 80),
            "far road stays stamped"
        );
    }

    #[test]
    fn cached_scene_matches_fresh_scene() {
        for mut view in [demo_view(), dungeon_view()] {
            let mut cache = StaticLayers::new();
            for t in [0.0, 0.33, 0.7] {
                let (fresh, fc) = scene_from_view_sized(&view, 96, 34);
                let (cached, cc) = scene_from_view_sized_cached(&view, 96, 34, &mut cache);
                assert_eq!(fc.x, cc.x);
                assert_eq!(fc.y, cc.y);
                assert_eq!(
                    compose(&fresh, &fc, t).rows(),
                    compose(&cached, &cc, t).rows(),
                    "cache must be frame-identical at t={t}"
                );
            }
            // Moving invalidates the ground cache but stays correct.
            view.player.x += 3;
            let (fresh, fc) = scene_from_view_sized(&view, 96, 34);
            let (cached, cc) = scene_from_view_sized_cached(&view, 96, 34, &mut cache);
            assert_eq!(
                compose(&fresh, &fc, 0.5).rows(),
                compose(&cached, &cc, 0.5).rows()
            );
        }
    }

    #[test]
    fn void_density_tiers_and_stays_dense_nearby() {
        assert_eq!(void_density(0, 0, 0, 0), 1.0);
        assert_eq!(void_density(20, 10, 0, 0), 1.0);
        // Far field tiers at 1.0 / 0.25 / 0.05 over a wide sample.
        let mut full = 0;
        let mut thin = 0;
        let mut sparse = 0;
        for x in (100..260).step_by(4) {
            for y in (100..220).step_by(4) {
                match void_density(x, y, 0, 0) {
                    d if d >= 1.0 => full += 1,
                    d if d >= 0.25 => thin += 1,
                    _ => sparse += 1,
                }
            }
        }
        assert!(full > (thin + sparse) * 2 && thin > 0 && sparse > 0);
    }

    #[test]
    fn light_falloff_starts_at_one_and_fades_by_radius() {
        assert_eq!(light_falloff(0.0), 1.0);
        assert!(light_falloff(6.0) < 0.3);
        assert!(light_falloff(12.0) < 0.1);
    }

    #[test]
    fn light_pool_repaints_ground_foregrounds_warm() {
        let mut layer = Layer::new("ground", 0);
        for (x, y) in [(0, 0), (1, 0), (30, 30)] {
            layer.push(AnimCell::still(
                x,
                y,
                Cell::new('/', Rgb::new(40, 88, 58), Rgb::new(5, 6, 12)),
            ));
        }
        apply_light_pools(
            &mut layer,
            &[Light {
                x: 0,
                y: 0,
                radius: 12.0,
                tint: Rgb::new(255, 175, 90),
            }],
        );
        let lit = layer.cells.iter().find(|c| c.x == 0 && c.y == 0).unwrap();
        assert!(
            lit.cell.fg.r > lit.cell.fg.g,
            "pooled ground reads warm, got {:?}",
            lit.cell.fg
        );
        let far = layer.cells.iter().find(|c| c.x == 30 && c.y == 30).unwrap();
        assert_eq!(
            far.cell.fg,
            Rgb::new(40, 88, 58),
            "ground past the radius stays untouched"
        );
    }

    #[test]
    fn world_has_dark_gaps_but_keeps_the_road() {
        let (scene, camera) = scene_from_view(&demo_view());
        let frame = compose(&scene, &camera, 0.0);
        let world = scene
            .partitions
            .iter()
            .find(|p| p.name == "world")
            .expect("world partition");
        let mut transparent = 0;
        let mut road = 0;
        for y in world.y..world.y + world.h {
            for x in world.x..world.x + world.w {
                let cell = frame.get(x, y).expect("world cell");
                if cell.transparent {
                    transparent += 1;
                }
                if cell.glyph == '=' || cell.glyph == '#' {
                    road += 1;
                }
            }
        }
        assert!(transparent > 0, "void mask must open dark gaps");
        assert!(road > 0, "road punches through the void");
    }

    /// Demo cast with the mossling parked beside the player (melee range).
    fn melee_view() -> SceneView {
        let mut view = demo_view();
        view.actors[0].x = view.player.x + 2;
        view.actors[0].y = view.player.y;
        view.actors[0].hp = 8;
        view
    }

    #[test]
    fn stage_quiets_ground_around_every_actor() {
        let view = melee_view();
        let (scene, _) = scene_from_view_sized(&view, 96, 50);
        let stage = scene
            .layer("stage")
            .and_then(|i| scene.layers.get(i))
            .expect("stage layer");
        assert!(!stage.cells.is_empty(), "stage stamped cells");
        // Every stage cell belongs to an actor's arena (the fixture uses
        // zero render offsets), and the floor is opaque, glyph-free dark.
        for cell in &stage.cells {
            assert!(
                std::iter::once(&view.player)
                    .chain(view.actors.iter())
                    .any(|a| (cell.x - a.x).abs() <= 3 && (cell.y - a.y).abs() <= 3),
                "stage cell ({},{}) away from every actor",
                cell.x,
                cell.y
            );
            assert_eq!(cell.cell.bg, STAGE_BG, "stage reads as dark floor");
            assert_eq!(cell.cell.glyph, ' ', "stage is quiet (no glyph ink)");
        }
    }

    #[test]
    fn melee_target_wears_the_decal_ring_and_others_do_not() {
        let view = melee_view();
        let (scene, _) = scene_from_view_sized(&view, 96, 50);
        let ring = scene
            .layer("target-ring")
            .and_then(|i| scene.layers.get(i))
            .expect("melee target ring layer");
        assert!(
            (4..=8).contains(&ring.cells.len()),
            "dashes, not a solid circle: {}",
            ring.cells.len()
        );
        for cell in &ring.cells {
            let dx = (cell.x - view.actors[0].x).abs();
            let dy = (cell.y - view.actors[0].y).abs();
            assert!(
                dx <= 2 && dy <= 2,
                "ring dash ({},{}) hugs the target",
                cell.x,
                cell.y
            );
            assert_eq!(cell.cell.glyph, '=');
            assert!(cell.cell.glow > 0.3, "decal glows warm");
        }
        // Out of melee range: no ring at all (demo positions are 32 apart).
        let (far, _) = scene_from_view(&demo_view());
        assert!(far.layer("target-ring").is_none(), "no ring when no target");
    }

    #[test]
    fn combat_fx_play_the_telegraph_then_burst_timeline() {
        let mut view = demo_view();
        view.combat_fx = vec![SceneFx {
            origin: (view.player.x as f32, view.player.y as f32),
            born_s: 0.0,
            radius: 2.4,
            color: Rgb::new(128, 184, 255),
            telegraph_s: 0.9,
            digits: String::new(),
        }];
        let sampled = |t: f32| {
            let mut v = view.clone();
            v.presentation_time_s = t;
            let (scene, _) = scene_from_view(&v);
            scene
                .layer("fx")
                .and_then(|i| scene.layers.get(i))
                .expect("fx layer")
                .clone()
        };
        // Wind-up: inward-sweeping dashes with remaining glow, no core yet.
        let telegraph = sampled(0.4);
        assert!(!telegraph.cells.is_empty(), "telegraph dashes present");
        assert!(
            telegraph
                .cells
                .iter()
                .all(|c| c.cell.glyph == '-' || c.cell.glyph == '|'),
            "dashes read as strokes, not dots"
        );
        assert!(
            telegraph.cells.iter().all(|c| c.cell.glow > 0.3),
            "telegraph ink still glows mid-sweep"
        );
        // Impact phase at 0.1 s into the burst: core + expanding ring.
        let mid = sampled(1.0);
        assert!(
            mid.cells.iter().any(|c| c.cell.glyph == '█'),
            "burst opens with the solid core"
        );
        assert!(
            mid.cells.iter().any(|c| c.cell.glyph == 'o'),
            "expanding ring follows the core"
        );
        // The core never lingers.
        let after = sampled(1.2);
        assert!(
            !after.cells.iter().any(|c| c.cell.glyph == '█'),
            "core fades quickly"
        );
    }

    #[test]
    fn combat_fx_raise_damage_digits() {
        let mut view = demo_view();
        view.presentation_time_s = 0.05;
        let moss = view.actors[0].clone();
        view.combat_fx = vec![SceneFx {
            origin: (moss.x as f32, moss.y as f32),
            born_s: 0.0,
            radius: 1.6,
            color: Rgb::new(250, 140, 40),
            telegraph_s: 0.0,
            digits: "5".into(),
        }];
        let (scene, _) = scene_from_view(&view);
        let fx = scene
            .layer("fx")
            .and_then(|i| scene.layers.get(i))
            .expect("fx layer");
        assert!(
            fx.cells.iter().any(|c| c.cell.glyph == '5'),
            "hit digit rises"
        );
        assert!(
            fx.cells.iter().all(|c| c.cell.fg.g > c.cell.fg.b),
            "ember hue stays warm"
        );
        assert!(
            fx.cells.iter().any(|c| c.cell.glyph == '█'),
            "instant hit shows the core"
        );
    }
}
