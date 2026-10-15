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

use super::sprite;
use super::{Anim, AnimCell, Axis, Camera, Cell, ColorTrack, Layer, Partition, Rgb, Scene, Track};
use std::sync::OnceLock;

/// Legacy world grid used by [`scene_from_view`].
pub const VIEW_W: i32 = 64;
pub const VIEW_H: i32 = 18;
/// Pinned HUD rows above the world and log rows below it.
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
const PANEL_BG: Rgb = Rgb::new(8, 12, 23);

fn layout(width: i32, height: i32) -> (i32, i32, i32, i32) {
    let width = width.max(1);
    let height = height.max(3);
    let hud = if height >= 7 { HUD_H } else { 1 };
    let log = if height >= 6 { LOG_H } else { 1 };
    let world = (height - hud - log).max(1);
    (width, hud, world, log)
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
}

const BG: Rgb = Rgb::new(10, 14, 26);

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
    let f = 0.7 + 0.5 * ((y - MEADOW_Y0) as f32 / span);
    base.scaled(f)
}

/// Compact volumetric actor art (`engine/assets/actors/*.txt`, `---`
/// separated phases, same format as the terminal set). Sized for windowed
/// grids: ~9–13 rows against 20+-row views, foot-anchored, with the kind's
/// focal glyph held steady at one local cell in every phase so camera,
/// palette, and glow contracts keep working.
const WARRIOR_ART: &str = include_str!("../assets/actors/warrior.txt");
const MOSSLING_ART: &str = include_str!("../assets/actors/mossling.txt");
const SKELETON_ART: &str = include_str!("../assets/actors/skeleton.txt");
const HERBALIST_ART: &str = include_str!("../assets/actors/herbalist.txt");

/// Flip-book speed of actor art: one phase per 1/6 s, so poses never repeat
/// inside the reference's no-repeat window.
const ART_FPS: f32 = 6.0;

fn cached_frames(source: &'static str) -> &'static Vec<Vec<String>> {
    static WARRIOR: OnceLock<Vec<Vec<String>>> = OnceLock::new();
    static MOSSLING: OnceLock<Vec<Vec<String>>> = OnceLock::new();
    static SKELETON: OnceLock<Vec<Vec<String>>> = OnceLock::new();
    static HERBALIST: OnceLock<Vec<Vec<String>>> = OnceLock::new();
    // The four call sites each pass one of the statics above; pointer
    // identity routes to a dedicated cache so parsing happens once.
    if std::ptr::eq(source, WARRIOR_ART) {
        WARRIOR.get_or_init(|| sprite::parse_frames(source))
    } else if std::ptr::eq(source, MOSSLING_ART) {
        MOSSLING.get_or_init(|| sprite::parse_frames(source))
    } else if std::ptr::eq(source, SKELETON_ART) {
        SKELETON.get_or_init(|| sprite::parse_frames(source))
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

fn in_water(x: i32, y: i32) -> bool {
    // Still pond, south-east of the meadow center.
    let dx = x - 30;
    let dy = (y - 14) * 2;
    dx * dx + dy * dy <= 81
}

fn push_ground(layer: &mut Layer) {
    for y in MEADOW_Y0..MEADOW_Y1 {
        for x in MEADOW_X0..MEADOW_X1 {
            if y == 8 {
                // Cobble road: stable full-width east-west route.
                let g = if hash2(x, y, 3) % 3 == 0 { '#' } else { '=' };
                layer.push(AnimCell::still(
                    x,
                    y,
                    Cell::new(g, depth_shade(Rgb::new(95, 88, 70), y), BG),
                ));
                continue;
            }
            if in_water(x, y) {
                layer.push(AnimCell::animated(
                    x,
                    y,
                    Cell::new('~', Rgb::new(80, 160, 255), Rgb::new(8, 16, 32)),
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
            // Flow-field strokes: orientation coheres on a coarse lattice so
            // neighbors agree (reference grass reads as flow lines, not hash
            // speckle). Coarse sampling keeps the field stable under camera.
            let g = match hash2(x.div_euclid(4), y.div_euclid(4), 77) % 3 {
                0 => '/',
                1 => '|',
                _ => '\\',
            };
            let green = Rgb::new(38 + (hash2(x, y, 17) % 18) as u8, 88, 58);
            layer.push(AnimCell::still(
                x,
                y,
                Cell::new(g, depth_shade(green, y), BG),
            ));
        }
    }
}

fn push_props(layer: &mut Layer) {
    const TREES: [(i32, i32); 6] = [(6, 4), (14, 11), (24, 3), (38, 12), (46, 7), (10, 18)];
    for (i, (x, y)) in TREES.iter().enumerate() {
        layer.push(AnimCell::still(
            *x,
            *y,
            Cell::new('|', Rgb::new(135, 85, 50), BG),
        ));
        layer.push(AnimCell::animated(
            *x,
            *y - 1,
            Cell::new('▲', Rgb::new(45, 130, 70), BG),
            Anim::still().with(Track::Pulse {
                low: 0.85,
                high: 1.1,
                period: 3.0,
                phase: i as f32 * 1.1,
            }),
        ));
    }
    for (x, y) in [(12, 6), (28, 10), (42, 14)] {
        layer.push(AnimCell::still(
            x,
            y,
            Cell::new('o', Rgb::new(130, 130, 140), BG),
        ));
    }
    // Torch: stick plus a flickering flame, the demo's pattern reused.
    layer.push(AnimCell::still(
        20,
        5,
        Cell::new('|', Rgb::new(120, 80, 40), BG),
    ));
    layer.push(AnimCell::animated(
        20,
        4,
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

fn push_rain(layer: &mut Layer) {
    for y in MEADOW_Y0..MEADOW_Y1 {
        for x in MEADOW_X0..MEADOW_X1 {
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

/// Stamp volumetric art foot-anchored: the focal cell lands on the actor
/// position, the feet on the row below, every other phase-varying cell
/// flip-books at [`ART_FPS`]. The focal cell keeps the palette track and
/// aura glow. Returns the art's top world row for plate anchoring.
fn stamp_art(layer: &mut Layer, a: &SceneActor, source: &'static str, tint: Rgb, aura: f32) -> i32 {
    let frames = cached_frames(source);
    let (w, h) = sprite::sprite_size(frames);
    let (fx, fy) = focal_of(frames, a.glyph);
    let (ox, oy) = (a.x - fx, a.y - fy);
    let palette = a.palette.unwrap_or(ColorTrack::Static(tint));
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
            let mut cell = match palette {
                ColorTrack::Gradient {
                    start, end, axis, ..
                } => Cell::new(glyphs[0], start, BG).with_fg_gradient(end, axis),
                _ => Cell::new(glyphs[0], tint, BG),
            };
            cell.bold = true;
            let mut placed = if frames.len() > 1 && glyphs.iter().any(|g| *g != glyphs[0]) {
                AnimCell::animated(
                    ox + cx as i32,
                    oy + cy as i32,
                    cell,
                    Anim::still().with(Track::Cycle {
                        frames: glyphs,
                        fps: ART_FPS,
                    }),
                )
            } else {
                AnimCell::still(ox + cx as i32, oy + cy as i32, cell)
            };
            if (cx as i32, cy as i32) == (fx, fy) {
                placed = placed.colored(palette);
                placed.cell.glow = aura;
            }
            layer.push(placed);
        }
    }
    oy
}
fn push_actor(
    layer: &mut Layer,
    labels: &mut Layer,
    a: &SceneActor,
    label_rows: &mut Vec<(i32, i32, i32)>,
) {
    // Shadow first (ground-side cue), actor body above it.
    layer.push(AnimCell::still(
        a.x,
        a.y + 1,
        Cell::new('.', Rgb::new(30, 34, 48), BG),
    ));
    // Volumetric art for bodies, mini glyphs for items. Returns the art's
    // top world row so plates float above the head, not the feet.
    let art_top = match art_for(a) {
        Some((source, tint, aura)) => stamp_art(layer, a, source, tint, aura),
        None => {
            for &(dx, dy, glyph, part_fg, glow) in SWORD {
                let mut cell = Cell::new(glyph, part_fg, BG);
                cell.bold = true;
                cell.glow = glow;
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
    labels.push_text(x0, label_y, &a.name, Rgb::new(230, 230, 230), BG);
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
        labels.push_text(a.x - 5, label_y - 1, &bar, color, BG);
    }
}

/// Build the 2.5D scene for `view`: pinned HUD top bar, camera-following
/// world (ground / props / actors / labels / rain), pinned log strip.
/// Returns the scene plus a camera centered on the player.
pub fn scene_from_view(view: &SceneView) -> (Scene, Camera) {
    scene_from_view_sized(view, VIEW_W, HUD_H + VIEW_H + LOG_H)
}

/// Build a scene for an arbitrary composed grid. `width`/`height` are the
/// complete frame dimensions, including the pinned HUD and log. Small grids
/// gracefully collapse those panels to one row while retaining a world row.
pub fn scene_from_view_sized(view: &SceneView, width: i32, height: i32) -> (Scene, Camera) {
    let (width, hud_h, world_h, log_h) = layout(width, height);
    let mut scene = Scene::default();

    let mut hud = Layer::new("hud", 4);
    hud.fill_rect(0, 0, width, hud_h, Cell::new(' ', HUD_DIM, PANEL_BG));
    fit_text(
        &mut hud,
        1,
        0,
        width - 2,
        &format!(
            "{}  HP {}/{}  Tick {}",
            view.zone, view.player.hp, view.player.max_hp, view.tick
        ),
        HUD_FG,
        true,
    );
    if hud_h > 1 {
        fit_text(
            &mut hud,
            1,
            1,
            width - 2,
            "[WASD] move   [F] attack   [R] rest   [Q/Esc] quit",
            HUD_DIM,
            false,
        );
    }
    if hud_h > 2 {
        fit_text(
            &mut hud,
            1,
            2,
            width - 2,
            "LIVE WORLD  |  camera follows player",
            HUD_DIM,
            false,
        );
    }
    scene.layers.push(hud);

    let mut ground = Layer::new("ground", 0);
    push_ground(&mut ground);
    scene.layers.push(ground);

    let mut props = Layer::new("props", 1);
    push_props(&mut props);
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

    // Below actors: streaks read as weather behind entities, never as
    // glyphs plastered over heads (a wave-displaced drop at t=0 covered '@').
    let mut rain = Layer::new("rain", 1);
    push_rain(&mut rain);
    scene.layers.push(rain);

    let mut log = Layer::new("log", 4);
    log.fill_rect(0, 0, width, log_h, Cell::new(' ', HUD_DIM, PANEL_BG));
    for (i, message) in view.messages.iter().rev().take(log_h as usize).enumerate() {
        fit_text(&mut log, 1, i as i32, width - 2, message, HUD_DIM, false);
    }
    scene.layers.push(log);

    let hud_idx = scene.layer("hud").unwrap();
    let ground_idx = scene.layer("ground").unwrap();
    let props_idx = scene.layer("props").unwrap();
    let actors_idx = scene.layer("actors").unwrap();
    let labels_idx = scene.layer("labels").unwrap();
    let rain_idx = scene.layer("rain").unwrap();
    let log_idx = scene.layer("log").unwrap();
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
        vec![ground_idx, props_idx, actors_idx, labels_idx, rain_idx],
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

    let camera = Camera {
        x: view.player.x - width / 2,
        y: view.player.y - world_h / 2,
    };
    (scene, camera)
}

/// Demo view mirroring the terminal client's `sample_world`: an adventurer,
/// two monsters, an NPC, and an item, so the `game` example shows the same
/// cast as `--demo` without a server.
pub fn demo_view() -> SceneView {
    SceneView {
        player: SceneActor {
            x: 12,
            y: 12,
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
                x: 30,
                y: 12,
                kind: ActorKind::Monster,
                name: "Mossling".into(),
                glyph: 'm',
                hp: 6,
                max_hp: 8,
                palette: Some(ColorTrack::Static(Rgb::new(120, 220, 120))),
            },
            SceneActor {
                x: 40,
                y: 17,
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
                x: -10,
                y: 14,
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
                x: 4,
                y: 9,
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

    #[test]
    fn player_stays_centered_and_named() {
        let view = demo_view();
        let rows = frame_rows(&view, 0.0);
        assert_eq!(rows.len(), (HUD_H + VIEW_H + LOG_H) as usize);
        let (cx, cy) = ((VIEW_W / 2) as usize, (HUD_H + VIEW_H / 2) as usize);
        assert_eq!(rows[cy].chars().nth(cx).unwrap(), '@');
        assert!(
            rows.iter().any(|row| row.contains("Adventurer")),
            "nameplate floats above"
        );
        assert!(rows[0].contains("HP 20/20"), "HUD shows player vitals");
    }

    #[test]
    fn camera_follows_the_player() {
        let mut view = demo_view();
        view.player.x += 6;
        let rows = frame_rows(&view, 0.0);
        let (cx, cy) = ((VIEW_W / 2) as usize, (HUD_H + VIEW_H / 2) as usize);
        assert_eq!(rows[cy].chars().nth(cx).unwrap(), '@');
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
        let (scene, camera) = scene_from_view(&view);
        let a = compose(&scene, &camera, 0.0)
            .get(VIEW_W / 2, HUD_H + VIEW_H / 2)
            .expect("player cell");
        let b = compose(&scene, &camera, 0.5)
            .get(VIEW_W / 2, HUD_H + VIEW_H / 2)
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
        let (scene, camera) = scene_from_view(&view);
        let frame = compose(&scene, &camera, 0.0);
        let adjacent = frame
            .get(VIEW_W / 2 + 1, HUD_H + VIEW_H / 2)
            .expect("ground beside player");
        assert_ne!(
            adjacent.bg, BG,
            "actor glow should spill into nearby ground"
        );
        assert!(
            adjacent.bg.r > adjacent.bg.g,
            "spill follows resolved actor colour"
        );
    }

    #[test]
    fn actor_gradient_reaches_composed_frame_data() {
        let mut view = demo_view();
        view.actors.clear();
        let (scene, camera) = scene_from_view(&view);
        let cell = compose(&scene, &camera, 0.0)
            .get(VIEW_W / 2, HUD_H + VIEW_H / 2)
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
        let (scene, camera) = scene_from_view_sized(&view, 64, 23);
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
        // Volumetric art: a warrior head/crown row, not a bare focal glyph.
        assert!(
            frame.rows().iter().any(|row| row.contains("/▲")),
            "warrior art top should be stamped"
        );
    }

    #[test]
    fn focal_glyph_stays_steady_every_phase() {
        // Volumetric art pins the kind's focal glyph at the actor position in
        // every phase (camera/palette/glow contracts depend on it).
        let mut view = demo_view();
        view.actors.clear();
        let (scene, camera) = scene_from_view(&view);
        for t in [0.0, 0.17, 0.34, 0.5, 0.67, 0.84] {
            let cell = compose(&scene, &camera, t)
                .get(VIEW_W / 2, HUD_H + VIEW_H / 2)
                .expect("player focal cell");
            assert_eq!(cell.glyph, '@', "focal glyph must hold at t={t}");
        }
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
}
