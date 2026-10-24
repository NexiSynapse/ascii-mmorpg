//! Bounded floating-point emission and bloom for continuous scene output.
//!
//! The field is a presenter-independent CPU reference/testing utility. Callers
//! describe emissive [`EmissionSource`] values, accumulate them in a bounded
//! [`BloomField`], sample the deterministic floating-point result, and apply
//! the result to an existing world-ink buffer. Empty cells are never painted:
//! bloom is a light contribution, not a substitute for world geometry.

use crate::{Cell, ContinuousScene, Frame, Point2, ResolvedPrimitive, Rgb};

/// Maximum width or height retained by a [`BloomField`].
pub const MAX_FIELD_DIMENSION: usize = 256;
/// Maximum number of source contributions accepted by one field.
pub const MAX_SOURCE_COUNT: usize = 128;
/// Maximum source radius in field coordinates.
pub const MAX_SOURCE_RADIUS: f32 = 32.0;
/// Maximum source strength retained after sanitization.
pub const MAX_SOURCE_STRENGTH: f32 = 64.0;
/// Radius, in cells, used when bridging legacy `Cell::glow` into the float
/// emission field.
pub const FRAME_GLOW_RADIUS_PER_UNIT: f32 = 5.0;
/// Strength scale for world glow retained by the continuous frame bridge.
/// Continuous bloom replaces the legacy 8-bit world glow pass.
pub const FRAME_GLOW_STRENGTH_SCALE: f32 = 0.35;

const MAX_POSITION: f32 = 1_000_000.0;

/// The two coordinate forms accepted by an [`EmissionSource`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EmissionPosition {
    /// An integer world/grid coordinate.
    Grid { x: i32, y: i32 },
    /// A fractional continuous coordinate in the same field space.
    Continuous(Point2),
}

impl EmissionPosition {
    pub const fn grid(x: i32, y: i32) -> Self {
        Self::Grid { x, y }
    }

    pub const fn continuous(position: Point2) -> Self {
        Self::Continuous(position)
    }

    fn point(self) -> Point2 {
        match self {
            Self::Grid { x, y } => Point2::new(x as f32, y as f32),
            Self::Continuous(position) => position,
        }
    }

    fn sanitized(self) -> Self {
        match self {
            Self::Grid { x, y } => Self::Grid {
                x: x.clamp(-(MAX_POSITION as i32), MAX_POSITION as i32),
                y: y.clamp(-(MAX_POSITION as i32), MAX_POSITION as i32),
            },
            Self::Continuous(position) => Self::Continuous(sanitize_point(position)),
        }
    }
}

/// One finite emissive light contribution.
///
/// `world` is part of the source rather than the field so a caller can retain
/// UI/effect sources in the same presentation description without allowing
/// them to illuminate world ink accidentally. [`BloomField`] only accumulates
/// sources marked as world sources.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EmissionSource {
    pub position: EmissionPosition,
    pub color: Rgb,
    pub strength: f32,
    pub radius: f32,
    pub world: bool,
}

impl EmissionSource {
    pub fn new(
        position: EmissionPosition,
        color: Rgb,
        strength: f32,
        radius: f32,
        world: bool,
    ) -> Self {
        Self {
            position: position.sanitized(),
            color,
            strength: sanitize_strength(strength),
            radius: sanitize_radius(radius),
            world,
        }
    }

    pub fn grid(x: i32, y: i32, color: Rgb, strength: f32, radius: f32, world: bool) -> Self {
        Self::new(EmissionPosition::grid(x, y), color, strength, radius, world)
    }

    pub fn continuous(
        position: Point2,
        color: Rgb,
        strength: f32,
        radius: f32,
        world: bool,
    ) -> Self {
        Self::new(
            EmissionPosition::continuous(position),
            color,
            strength,
            radius,
            world,
        )
    }

    pub const fn with_world(mut self, world: bool) -> Self {
        self.world = world;
        self
    }

    pub(crate) fn from_world_cell(x: i32, y: i32, cell: Cell) -> Option<Self> {
        if cell.transparent || !cell.glow.is_finite() {
            return None;
        }
        // Cell (0, 0) has centre (0, 0) in emission coordinates. The
        // presenter's half-cell canvas translation is not part of this space.
        let source = Self::continuous(
            Point2::new(x as f32 + cell.offset_x, y as f32 + cell.offset_y),
            cell.fg,
            cell.glow,
            cell.glow * FRAME_GLOW_RADIUS_PER_UNIT,
            true,
        );
        source.contributes_to_world().then_some(source)
    }

    fn contributes_to_world(self) -> bool {
        self.world
            && self.strength.is_finite()
            && self.strength > 0.0
            && self.color != Rgb::new(0, 0, 0)
    }

    /// Convert an animated continuous scene into fixed-time world sources.
    ///
    /// The scene is sampled exactly once at `time`; its primitive animation is
    /// therefore deterministic and cannot advance while a field is built.
    /// Continuous scene primitives have no partition metadata yet, so this
    /// adapter treats their emissions as world sources. Zero/black emissions
    /// are skipped before the retained source count is capped.
    pub fn from_continuous_scene(scene: &ContinuousScene, time: f32) -> Vec<Self> {
        Self::from_continuous_scene_with_world(scene, time, true)
    }

    /// Variant of [`Self::from_continuous_scene`] with an explicit world/UI
    /// scope for callers that share a continuous scene builder across layers.
    /// UI-scoped scenes (`world == false`) produce no world sources.
    pub fn from_continuous_scene_with_world(
        scene: &ContinuousScene,
        time: f32,
        world: bool,
    ) -> Vec<Self> {
        scene
            .sample(time)
            .into_iter()
            .filter_map(|primitive| Self::from_resolved_primitive(primitive, world))
            .filter(|source| source.contributes_to_world())
            .take(MAX_SOURCE_COUNT)
            .collect()
    }

    /// Convert retained pre-lighting world glow into float emission sources,
    /// including hidden lower-layer emitters. Compose with the continuous
    /// entrypoint to avoid baking legacy world glow into the input as well.
    /// If no sources were retained, scan visible world cells instead. UI,
    /// zero-strength, and black sources never consume the source budget.
    pub fn from_frame(frame: &Frame) -> Vec<Self> {
        if !frame.emission_sources().is_empty() {
            return frame
                .emission_sources()
                .iter()
                .map(|source| {
                    Self::new(
                        source.position,
                        source.color,
                        source.strength * FRAME_GLOW_STRENGTH_SCALE,
                        source.radius,
                        source.world,
                    )
                })
                .filter(|source| source.contributes_to_world())
                .take(MAX_SOURCE_COUNT)
                .collect();
        }

        let mut sources = Vec::new();
        for y in 0..frame.h.max(0) {
            for x in 0..frame.w.max(0) {
                if !frame.is_world(x, y) {
                    continue;
                }
                let Some(cell) = frame.get(x, y) else {
                    continue;
                };
                let Some(mut source) = Self::from_world_cell(x, y, cell) else {
                    continue;
                };
                source.strength *= FRAME_GLOW_STRENGTH_SCALE;
                if !source.contributes_to_world() {
                    continue;
                }
                sources.push(source);
                if sources.len() >= MAX_SOURCE_COUNT {
                    return sources;
                }
            }
        }
        sources
    }

    /// Extract an emission source from one resolved continuous primitive.
    ///
    /// Continuous primitive positions are authored in frame-grid cell-corner
    /// coordinates. The native presenter translates the glyph quad centre by
    /// half a cell, so the emission bridge makes the same translation before
    /// handing the source to the bloom field. This keeps the source centre and
    /// the rendered glyph centre identical for fractional positions.
    ///
    /// The source colour, strength (opacity-scaled), radius, and position
    /// are intact and sanitized; non-emissive primitives resolve to `None`.
    pub fn from_resolved_primitive(primitive: ResolvedPrimitive, world: bool) -> Option<Self> {
        let emission = primitive.material.emission?;
        let opacity = sanitize_opacity(primitive.material.opacity);
        let position = primitive.transform.position;
        Some(Self::new(
            EmissionPosition::continuous(Point2::new(position.x + 0.5, position.y + 0.5)),
            emission.color,
            emission.strength * opacity,
            emission.radius,
            world,
        ))
    }
}

/// A finite RGB contribution stored in normalized floating-point channels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FloatRgb {
    pub r: f32,
    pub g: f32,
    pub b: f32,
}

impl FloatRgb {
    pub const ZERO: Self = Self {
        r: 0.0,
        g: 0.0,
        b: 0.0,
    };

    pub const fn new(r: f32, g: f32, b: f32) -> Self {
        Self { r, g, b }
    }

    pub fn from_rgb(color: Rgb) -> Self {
        Self::new(
            f32::from(color.r) / 255.0,
            f32::from(color.g) / 255.0,
            f32::from(color.b) / 255.0,
        )
    }

    pub fn is_finite(self) -> bool {
        self.r.is_finite() && self.g.is_finite() && self.b.is_finite()
    }

    pub fn to_rgb(self) -> Rgb {
        Rgb::new(to_channel(self.r), to_channel(self.g), to_channel(self.b))
    }

    fn scaled(self, factor: f32) -> Self {
        Self::new(self.r * factor, self.g * factor, self.b * factor)
    }

    fn add_assign(&mut self, other: Self) {
        self.r = finite_sum(self.r, other.r);
        self.g = finite_sum(self.g, other.g);
        self.b = finite_sum(self.b, other.b);
    }
}

/// A bounded, deterministic floating-point bloom field.
///
/// Sources are accumulated in stable row-major storage. The field clamps its
/// dimensions, contributing source count, radius, strength, and invalid float
/// inputs. Storage and work per accepted source are bounded; inspecting an
/// input iterator still depends on its length (see [`Self::add_sources`]).
#[derive(Clone, Debug, PartialEq)]
pub struct BloomField {
    width: usize,
    height: usize,
    values: Vec<FloatRgb>,
    source_count: usize,
}

impl BloomField {
    pub fn new(width: usize, height: usize) -> Self {
        let width = width.min(MAX_FIELD_DIMENSION);
        let height = height.min(MAX_FIELD_DIMENSION);
        Self {
            width,
            height,
            values: vec![FloatRgb::ZERO; width.saturating_mul(height)],
            source_count: 0,
        }
    }

    /// Build a field from a finite source iterator. See [`Self::add_sources`]
    /// for the distinction between inspected and contributing source counts.
    pub fn from_sources(
        width: usize,
        height: usize,
        sources: impl IntoIterator<Item = EmissionSource>,
    ) -> Self {
        let mut field = Self::new(width, height);
        field.add_sources(sources);
        field
    }

    pub fn from_continuous_scene(
        width: usize,
        height: usize,
        scene: &ContinuousScene,
        time: f32,
    ) -> Self {
        Self::from_sources(
            width,
            height,
            EmissionSource::from_continuous_scene(scene, time),
        )
    }

    /// Build a bounded float bloom field from visible world-cell glow data.
    pub fn from_frame(frame: &Frame) -> Self {
        Self::from_sources(
            frame.w.max(0) as usize,
            frame.h.max(0) as usize,
            EmissionSource::from_frame(frame),
        )
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }

    pub fn dimensions(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    pub fn source_count(&self) -> usize {
        self.source_count
    }

    pub fn clear(&mut self) {
        self.values.fill(FloatRgb::ZERO);
        self.source_count = 0;
    }

    /// Inspect a finite iterator until it ends or the contribution cap is met.
    /// Rejected UI/zero/black entries do not consume that cap, so the number
    /// inspected is bounded by the caller's finite input, not MAX_SOURCE_COUNT.
    pub fn add_sources(&mut self, sources: impl IntoIterator<Item = EmissionSource>) {
        for source in sources {
            if self.source_count >= MAX_SOURCE_COUNT {
                break;
            }
            let _ = self.add_source(source);
        }
    }

    /// Accumulate one source. Returns `false` when it cannot contribute to the
    /// world field (non-world, zero strength, black colour, empty field, or cap).
    pub fn add_source(&mut self, source: EmissionSource) -> bool {
        if self.width == 0
            || self.height == 0
            || self.source_count >= MAX_SOURCE_COUNT
            || !source.world
        {
            return false;
        }

        let source = EmissionSource::new(
            source.position,
            source.color,
            source.strength,
            source.radius,
            source.world,
        );
        if !source.contributes_to_world() {
            return false;
        }

        self.accumulate(source);
        self.source_count += 1;
        true
    }

    /// Return the deterministic normalized floating-point contribution at a
    /// grid cell. Out-of-bounds samples are zero rather than panicking.
    pub fn sample(&self, x: usize, y: usize) -> FloatRgb {
        if x >= self.width || y >= self.height {
            return FloatRgb::ZERO;
        }
        self.values
            .get(y * self.width + x)
            .copied()
            .unwrap_or(FloatRgb::ZERO)
    }

    /// Apply bloom to a row-major world-cell buffer. Transparent cells and
    /// space glyphs are void, so they remain byte-for-byte unchanged.
    pub fn apply_to_world_ink(&self, cells: &mut [Cell]) {
        let count = self.values.len().min(cells.len());
        for (index, cell) in cells.iter_mut().enumerate().take(count) {
            if cell.transparent || cell.glyph == ' ' {
                continue;
            }
            let light = self.values[index];
            if !light.is_finite() {
                continue;
            }
            cell.fg = add_light(cell.fg, light);
            cell.fg_end = add_light(cell.fg_end, light);
        }
    }

    /// Apply the field only to visible cells in a composed world partition.
    /// HUD cells and black world void remain untouched.
    pub fn apply_to_frame_world_ink(&self, frame: &mut Frame) {
        let frame_width = frame.w.max(0) as usize;
        let frame_height = frame.h.max(0) as usize;
        let width = self.width.min(frame_width);
        let height = self.height.min(frame_height);
        for y in 0..height {
            for x in 0..width {
                if !frame.is_world(x as i32, y as i32) {
                    continue;
                }
                let frame_index = y * frame_width + x;
                let Some(cell) = frame.cells.get_mut(frame_index) else {
                    continue;
                };
                if cell.transparent || cell.glyph == ' ' {
                    continue;
                }
                let light = self.values[y * self.width + x];
                if light.is_finite() {
                    cell.fg = add_light(cell.fg, light);
                    cell.fg_end = add_light(cell.fg_end, light);
                }
            }
        }
    }

    fn accumulate(&mut self, source: EmissionSource) {
        let position = source.position.point();
        let radius = source.radius;
        let color = FloatRgb::from_rgb(source.color);

        if radius <= f32::EPSILON {
            let x = position.x.round() as isize;
            let y = position.y.round() as isize;
            self.add_at(x, y, color.scaled(source.strength));
            return;
        }

        let min_x = ((position.x - radius).floor() as isize).max(0);
        let min_y = ((position.y - radius).floor() as isize).max(0);
        let max_x =
            ((position.x + radius).ceil() as isize).min(self.width.saturating_sub(1) as isize);
        let max_y =
            ((position.y + radius).ceil() as isize).min(self.height.saturating_sub(1) as isize);
        if min_x > max_x || min_y > max_y {
            return;
        }

        for y in min_y..=max_y {
            for x in min_x..=max_x {
                let dx = x as f32 - position.x;
                let dy = y as f32 - position.y;
                let distance = (dx * dx + dy * dy).sqrt();
                if distance > radius {
                    continue;
                }
                let normalized = (distance / radius).clamp(0.0, 1.0);
                let falloff = (1.0 - normalized).powi(2);
                self.add_at(x, y, color.scaled(source.strength * falloff));
            }
        }
    }

    fn add_at(&mut self, x: isize, y: isize, contribution: FloatRgb) {
        if x < 0 || y < 0 || x as usize >= self.width || y as usize >= self.height {
            return;
        }
        let index = y as usize * self.width + x as usize;
        self.values[index].add_assign(contribution);
    }
}

fn sanitize_point(point: Point2) -> Point2 {
    Point2::new(sanitize_coordinate(point.x), sanitize_coordinate(point.y))
}

fn sanitize_coordinate(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(-MAX_POSITION, MAX_POSITION)
    } else {
        0.0
    }
}

fn sanitize_strength(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0.0, MAX_SOURCE_STRENGTH)
    } else {
        0.0
    }
}

fn sanitize_radius(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0.0, MAX_SOURCE_RADIUS)
    } else {
        0.0
    }
}

fn sanitize_opacity(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

fn finite_sum(left: f32, right: f32) -> f32 {
    let sum = left + right;
    if sum.is_finite() {
        sum.max(0.0)
    } else {
        f32::MAX
    }
}

fn to_channel(value: f32) -> u8 {
    if !value.is_finite() {
        return 0;
    }
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

fn add_light(base: Rgb, light: FloatRgb) -> Rgb {
    let add = |channel: u8, contribution: f32| {
        if !contribution.is_finite() || contribution <= 0.0 {
            return channel;
        }
        let value = f32::from(channel) + contribution.clamp(0.0, f32::MAX / 255.0) * 255.0;
        value.clamp(0.0, 255.0).round() as u8
    };
    Rgb::new(
        add(base.r, light.r),
        add(base.g, light.g),
        add(base.b, light.b),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::continuous::{Animation, Emission, Material, Primitive, PrimitiveKind, Transform2};
    use crate::{Layer, Partition, Scene};

    fn source(color: Rgb, strength: f32, radius: f32) -> EmissionSource {
        EmissionSource::grid(4, 4, color, strength, radius, true)
    }

    #[test]
    fn source_accumulation_adds_floating_point_contributions() {
        let mut field = BloomField::new(9, 9);
        let red = source(Rgb::new(255, 0, 0), 0.5, 2.0);
        assert!(!field.add_source(red.with_world(false)));
        assert!(field.add_source(red));
        let once = field.sample(4, 4);
        assert!(field.add_source(red));
        let twice = field.sample(4, 4);

        assert!(once.r > 0.0);
        assert_eq!(twice.r, once.r * 2.0);
        assert_eq!(twice.g, 0.0);
        assert_eq!(twice.b, 0.0);
    }

    #[test]
    fn spatial_falloff_is_monotonic_and_bounded_by_radius() {
        let field = BloomField::from_sources(9, 9, [source(Rgb::new(255, 255, 255), 1.0, 3.0)]);

        assert!(field.sample(4, 4).r > field.sample(5, 4).r);
        assert!(field.sample(5, 4).r > field.sample(6, 4).r);
        assert_eq!(field.sample(8, 4), FloatRgb::ZERO);
    }

    #[test]
    fn coloured_spill_changes_only_the_matching_channels() {
        let field = BloomField::from_sources(
            5,
            5,
            [EmissionSource::grid(
                2,
                2,
                Rgb::new(255, 32, 0),
                0.2,
                2.0,
                true,
            )],
        );
        let mut cells = vec![Cell::new('x', Rgb::new(10, 20, 30), Rgb::new(0, 0, 0)); 25];
        let before = cells[2 * 5 + 3];

        field.apply_to_world_ink(&mut cells);
        let after = cells[2 * 5 + 3];

        assert!(after.fg.r > before.fg.r);
        assert!(after.fg.g > before.fg.g);
        assert_eq!(after.fg.b, before.fg.b);
    }

    #[test]
    fn void_darkness_preserves_empty_and_transparent_cells() {
        let field = BloomField::from_sources(
            3,
            1,
            [EmissionSource::grid(
                2,
                0,
                Rgb::new(255, 120, 20),
                1.0,
                1.0,
                true,
            )],
        );
        let clear = Cell::CLEAR;
        let empty = Cell::new(' ', Rgb::new(1, 2, 3), Rgb::new(4, 5, 6));
        let ink = Cell::new('x', Rgb::new(1, 2, 3), Rgb::new(4, 5, 6));
        let mut cells = vec![clear, empty, ink];

        field.apply_to_world_ink(&mut cells);

        assert_eq!(cells[0], clear);
        assert_eq!(cells[1], empty);
        assert_ne!(cells[2], ink);
    }

    #[test]
    fn frame_bridge_spills_into_world_ink_but_not_hud_or_void() {
        let mut frame = Frame::blank(5, 1);
        frame.cells[0] = Cell::new('*', Rgb::new(255, 80, 10), Rgb::new(0, 0, 0)).with_glow(1.0);
        frame.cells[1] = Cell::new('x', Rgb::new(10, 20, 30), Rgb::new(0, 0, 0));
        frame.cells[2] = Cell::new(' ', Rgb::new(10, 20, 30), Rgb::new(0, 0, 0));
        frame.cells[3] = Cell::new('h', Rgb::new(10, 20, 30), Rgb::new(2, 3, 4));
        frame.cells[4] = Cell::new(' ', Rgb::new(10, 20, 30), Rgb::new(2, 3, 4));

        // Frame's public construction starts unpartitioned; mark the first
        // three cells as world through the real composition seam.
        let mut scene = Scene::default();
        let mut world = Layer::new("world", 0);
        world.push(crate::AnimCell::still(0, 0, frame.cells[0]));
        world.push(crate::AnimCell::still(1, 0, frame.cells[1]));
        world.push(crate::AnimCell::still(2, 0, frame.cells[2]));
        let mut hud = Layer::new("hud", 1);
        hud.push(crate::AnimCell::still(3, 0, frame.cells[3]));
        hud.push(crate::AnimCell::still(4, 0, frame.cells[4]));
        let world_index = scene.layers.len();
        scene.layers.push(world);
        let hud_index = scene.layers.len();
        scene.layers.push(hud);
        scene
            .partitions
            .push(Partition::new("world", 0, 0, 3, 1, vec![world_index], true));
        scene
            .partitions
            .push(Partition::new("hud", 3, 0, 2, 1, vec![hud_index], false).with_view(3, 0));
        let mut composed =
            crate::compose::compose_for_continuous(&scene, &crate::Camera::default(), 0.0);
        let before_world = composed.cells[1].fg;
        let before_hud = composed.cells[3].fg;
        let before_void = composed.cells[2];

        let field = BloomField::from_frame(&composed);
        field.apply_to_frame_world_ink(&mut composed);

        assert!(composed.cells[1].fg.r > before_world.r);
        assert_eq!(composed.cells[3].fg, before_hud);
        assert_eq!(composed.cells[2], before_void);
    }

    #[test]
    fn frame_bridge_retains_a_hidden_lower_layer_emitter() {
        let mut scene = Scene::default();
        let mut glow = Layer::new("glow", 0);
        glow.push(crate::AnimCell::still(
            0,
            0,
            Cell::new(' ', Rgb::new(255, 90, 20), Rgb::new(0, 0, 0)).with_glow(1.0),
        ));
        let mut terrain = Layer::new("terrain", 1);
        terrain.push(crate::AnimCell::still(
            0,
            0,
            Cell::new('#', Rgb::new(20, 20, 20), Rgb::new(0, 0, 0)),
        ));
        terrain.push(crate::AnimCell::still(
            1,
            0,
            Cell::new('#', Rgb::new(20, 20, 20), Rgb::new(0, 0, 0)),
        ));
        scene.layers.push(glow);
        scene.layers.push(terrain);
        scene
            .partitions
            .push(Partition::new("world", 0, 0, 2, 1, vec![0, 1], true));

        let mut frame =
            crate::compose::compose_for_continuous(&scene, &crate::Camera::default(), 0.0);
        let before = frame.cells[1].fg;
        let field = BloomField::from_frame(&frame);

        assert_eq!(field.source_count(), 1);
        field.apply_to_frame_world_ink(&mut frame);
        assert!(frame.cells[1].fg.r > before.r);
    }

    #[test]
    fn repeated_sampling_is_deterministic() {
        let field = BloomField::from_sources(
            12,
            8,
            [
                EmissionSource::continuous(
                    Point2::new(3.25, 2.5),
                    Rgb::new(255, 20, 4),
                    0.8,
                    3.5,
                    true,
                ),
                EmissionSource::grid(8, 5, Rgb::new(5, 80, 255), 0.6, 2.25, true),
            ],
        );
        let first: Vec<_> = (0..8)
            .flat_map(|y| {
                let field = &field;
                (0..12).map(move |x| field.sample(x, y))
            })
            .collect();
        let second: Vec<_> = (0..8)
            .flat_map(|y| {
                let field = &field;
                (0..12).map(move |x| field.sample(x, y))
            })
            .collect();

        assert_eq!(first, second);
        assert!(first.iter().all(|value| value.is_finite()));
    }

    #[test]
    fn continuous_scene_emissions_are_extracted_at_fixed_time() {
        let primitive = Primitive::new(
            PrimitiveKind::particle('*'),
            Transform2::new(Point2::new(1.0, 2.0), 0.0),
            Material::new(Rgb::new(255, 120, 20))
                .with_opacity(0.5)
                .with_emission(Emission::new(Rgb::new(255, 40, 8), 2.0, 3.0)),
        )
        .with_animation(Animation::still().with_velocity(Point2::new(0.5, 0.0)));
        let mut scene = ContinuousScene::new();
        scene.push(primitive);

        let sources = EmissionSource::from_continuous_scene(&scene, 2.0);

        assert_eq!(sources.len(), 1);
        assert_eq!(
            sources[0].position,
            EmissionPosition::Continuous(Point2::new(2.5, 2.5))
        );
        assert_eq!(sources[0].color, Rgb::new(255, 40, 8));
        assert_eq!(sources[0].strength, 1.0);
        assert_eq!(sources[0].radius, 3.0);
        assert!(sources[0].world);

        let field = BloomField::from_continuous_scene(8, 8, &scene, 2.0);
        assert_eq!(field.source_count(), 1);
        assert!(field.sample(2, 2).r > 0.0);
    }

    #[test]
    fn source_selection_skips_noncontributors_before_the_cap() {
        let red = Rgb::new(255, 0, 0);
        let mut scene = ContinuousScene::new();
        for _ in 0..=MAX_SOURCE_COUNT {
            for (color, strength, opacity) in [
                (Rgb::new(0, 0, 0), 1.0, 1.0),
                (red, 0.0, 1.0),
                (red, f32::NAN, 1.0),
                (red, 1.0, 0.0),
            ] {
                scene.push(Primitive::new(
                    PrimitiveKind::particle('*'),
                    Transform2::new(Point2::new(4.0, 4.0), 0.0),
                    Material::new(red)
                        .with_opacity(opacity)
                        .with_emission(Emission::new(color, strength, 2.0)),
                ));
            }
        }
        scene.push(Primitive::new(
            PrimitiveKind::particle('*'),
            Transform2::new(Point2::new(4.25, 4.0), 0.0),
            Material::new(red).with_emission(Emission::new(red, 1.0, 2.0)),
        ));

        let sources = EmissionSource::from_continuous_scene(&scene, 0.0);
        assert_eq!(sources.len(), 1);
        assert_eq!(
            sources[0].position,
            EmissionPosition::Continuous(Point2::new(4.75, 4.5))
        );
        assert!(EmissionSource::from_continuous_scene_with_world(&scene, 0.0, false).is_empty());

        let ignored = source(Rgb::new(0, 0, 0), 1.0, 2.0);
        let mut direct = vec![ignored; MAX_SOURCE_COUNT + 1];
        direct.extend(vec![
            source(red, 1.0, 2.0).with_world(false);
            MAX_SOURCE_COUNT + 1
        ]);
        direct.extend(sources);
        let field = BloomField::from_sources(9, 9, direct);
        assert_eq!(field.source_count(), 1);
        assert!(field.sample(4, 4).r > 0.0);
    }

    #[test]
    fn visible_frame_source_scan_skips_noncontributors_and_keeps_offsets() {
        let mut scene = Scene::default();
        let mut world = Layer::new("world", 0);
        for x in 0..MAX_SOURCE_COUNT as i32 + 2 {
            world.push(crate::AnimCell::still(
                x,
                0,
                Cell::new('*', Rgb::new(0, 0, 0), Rgb::new(0, 0, 0)).with_glow(1.0),
            ));
        }
        scene.layers.push(world);
        scene.partitions.push(Partition::new(
            "world",
            0,
            0,
            MAX_SOURCE_COUNT as i32 + 2,
            1,
            vec![0],
            true,
        ));
        let mut frame =
            crate::compose::compose_for_continuous(&scene, &crate::Camera::default(), 0.0);
        assert!(frame.emission_sources().is_empty());
        // Exercise the public frame's visible-cell fallback after editing its
        // cells, with no composition-time source metadata to use instead.
        frame.cells[0].glow = 0.0;
        frame.cells[1].glow = f32::NAN;
        frame.cells[2].glow = f32::INFINITY;
        let x = MAX_SOURCE_COUNT + 1;
        frame.cells[x] = Cell::new('*', Rgb::new(255, 90, 20), Rgb::new(0, 0, 0))
            .with_glow(1.0)
            .with_offset(0.25, -0.5);

        let sources = EmissionSource::from_frame(&frame);
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].color, Rgb::new(255, 90, 20));
        assert_eq!(
            sources[0].position,
            EmissionPosition::Continuous(Point2::new(x as f32 + 0.25, -0.5))
        );
    }

    #[test]
    fn invalid_inputs_are_sanitized_and_work_remains_bounded() {
        let source = EmissionSource::continuous(
            Point2::new(f32::NAN, f32::INFINITY),
            Rgb::new(255, 0, 0),
            f32::INFINITY,
            f32::NEG_INFINITY,
            true,
        );
        assert_eq!(source.position, EmissionPosition::Continuous(Point2::ZERO));
        assert_eq!(source.strength, 0.0);
        assert_eq!(source.radius, 0.0);
        let capped = EmissionSource::grid(0, 0, Rgb::new(1, 2, 3), f32::MAX, f32::MAX, true);
        assert_eq!(capped.strength, MAX_SOURCE_STRENGTH);
        assert_eq!(capped.radius, MAX_SOURCE_RADIUS);

        let mut field = BloomField::new(usize::MAX, usize::MAX);
        assert_eq!(
            field.dimensions(),
            (MAX_FIELD_DIMENSION, MAX_FIELD_DIMENSION)
        );
        assert!(!field.add_source(source));
        assert_eq!(field.sample(usize::MAX, usize::MAX), FloatRgb::ZERO);

        let primitive = Primitive::new(
            PrimitiveKind::glyph('@'),
            Transform2::new(Point2::new(f32::NAN, f32::NEG_INFINITY), 0.0),
            Material::new(Rgb::new(255, 255, 255)).with_emission(Emission::new(
                Rgb::new(1, 2, 3),
                f32::NAN,
                f32::INFINITY,
            )),
        );
        let mut scene = ContinuousScene::new();
        scene.push(primitive);
        let extracted = EmissionSource::from_continuous_scene(&scene, f32::NAN);
        assert!(
            extracted.is_empty(),
            "invalid strength is not a contributor"
        );
    }
}
