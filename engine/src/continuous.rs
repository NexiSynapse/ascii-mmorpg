//! Continuous presentation primitives for the rebuilt world renderer.
//!
//! This module deliberately does not know about windows, GPU buffers, server
//! snapshots, or terminal cells. It is the public scene seam between a
//! presentation author and a future continuous renderer: authors describe
//! glyphs and simple ribbons in continuous coordinates, then sample the same
//! deterministic scene at a presentation time.

use super::Rgb;

/// Per-frame cap on resolved native primitives. Bounded per-frame GPU work:
/// each resolved primitive becomes one GPU instance and one emission scan per
/// present, so the sample output (and therefore the continuous instance
/// buffer and upload) can never grow with scene size. Generous enough for
/// particle-heavy showcase scenes; builders that exceed it are truncated in
/// `ContinuousScene::sample` order.
pub const MAX_NATIVE_PRIMITIVES: usize = 1024;

/// A continuous two-dimensional point or vector in scene space.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point2 {
    pub x: f32,
    pub y: f32,
}

impl Point2 {
    pub const ZERO: Self = Self { x: 0.0, y: 0.0 };

    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

impl std::ops::Add for Point2 {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self::new(self.x + rhs.x, self.y + rhs.y)
    }
}

impl std::ops::Mul<f32> for Point2 {
    type Output = Self;

    fn mul(self, rhs: f32) -> Self::Output {
        Self::new(self.x * rhs, self.y * rhs)
    }
}

/// Independent transform channels shared by every primitive kind.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform2 {
    /// Continuous scene position. Fractional values are intentional.
    pub position: Point2,
    /// Painter/depth ordering value. Lower values resolve first.
    pub depth: f32,
    /// Independent horizontal and vertical scale.
    pub scale: Point2,
    /// Rotation in radians.
    pub rotation: f32,
    /// Shear/tilt factors applied by the eventual renderer.
    pub skew: Point2,
}

impl Transform2 {
    pub const fn new(position: Point2, depth: f32) -> Self {
        Self {
            position,
            depth,
            scale: Point2::new(1.0, 1.0),
            rotation: 0.0,
            skew: Point2::ZERO,
        }
    }

    pub const fn with_scale(mut self, scale: Point2) -> Self {
        self.scale = scale;
        self
    }

    pub const fn with_rotation(mut self, rotation: f32) -> Self {
        self.rotation = rotation;
        self
    }

    pub const fn with_skew(mut self, skew: Point2) -> Self {
        self.skew = skew;
        self
    }
}

/// Axis used by a per-primitive foreground gradient.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GradientAxis {
    Horizontal,
    Vertical,
    Radial,
}

/// Two endpoint colours retained until the continuous renderer evaluates the
/// primitive. This allows one glyph to contain multiple colours instead of
/// forcing one terminal foreground colour per character.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ColorGradient {
    pub start: Rgb,
    pub end: Rgb,
    pub axis: GradientAxis,
}

impl ColorGradient {
    pub const fn new(start: Rgb, end: Rgb, axis: GradientAxis) -> Self {
        Self { start, end, axis }
    }
}

/// Floating-point lighting metadata carried by a primitive until the emission
/// and bloom pipeline is available. `radius` is expressed in scene units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Emission {
    pub color: Rgb,
    pub strength: f32,
    pub radius: f32,
}

impl Emission {
    pub fn new(color: Rgb, strength: f32, radius: f32) -> Self {
        Self {
            color,
            strength: finite_nonnegative(strength),
            radius: finite_nonnegative(radius),
        }
    }
}

/// Appearance channels shared by glyphs, ribbons, rays, streaks, and
/// particles.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Material {
    pub foreground: Rgb,
    pub background: Rgb,
    pub gradient: Option<ColorGradient>,
    pub opacity: f32,
    pub emission: Option<Emission>,
}

impl Material {
    pub const fn new(foreground: Rgb) -> Self {
        Self {
            foreground,
            background: Rgb::new(0, 0, 0),
            gradient: None,
            opacity: 1.0,
            emission: None,
        }
    }

    pub const fn with_background(mut self, background: Rgb) -> Self {
        self.background = background;
        self
    }

    pub const fn with_gradient(mut self, gradient: ColorGradient) -> Self {
        self.gradient = Some(gradient);
        self
    }

    pub fn with_opacity(mut self, opacity: f32) -> Self {
        self.opacity = opacity.clamp(0.0, 1.0);
        self
    }

    pub fn with_emission(mut self, emission: Emission) -> Self {
        self.emission = Some(emission);
        self
    }
}

/// Simple deterministic animation channels for the first continuous scene
/// seam. More expressive seeded curves and particle emitters build on this in
/// later renderer tickets.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Animation {
    pub velocity: Point2,
    pub acceleration: Point2,
    pub rotation_velocity: f32,
    pub scale_velocity: Point2,
    pub opacity_velocity: f32,
}

impl Animation {
    pub const fn still() -> Self {
        Self {
            velocity: Point2::ZERO,
            acceleration: Point2::ZERO,
            rotation_velocity: 0.0,
            scale_velocity: Point2::ZERO,
            opacity_velocity: 0.0,
        }
    }

    pub const fn with_velocity(mut self, velocity: Point2) -> Self {
        self.velocity = velocity;
        self
    }

    pub const fn with_acceleration(mut self, acceleration: Point2) -> Self {
        self.acceleration = acceleration;
        self
    }

    pub const fn with_rotation_velocity(mut self, velocity: f32) -> Self {
        self.rotation_velocity = velocity;
        self
    }

    pub const fn with_scale_velocity(mut self, velocity: Point2) -> Self {
        self.scale_velocity = velocity;
        self
    }

    pub const fn with_opacity_velocity(mut self, velocity: f32) -> Self {
        self.opacity_velocity = velocity;
        self
    }
}

/// Geometry family understood by the continuous renderer seam. The renderer
/// decides how each local shape becomes GPU triangles or glyph-mask quads.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PrimitiveKind {
    Glyph {
        glyph: char,
    },
    Particle {
        glyph: char,
    },
    Ribbon {
        length: f32,
        width: f32,
    },
    Streak {
        length: f32,
        width: f32,
    },
    Ray {
        length: f32,
        width: f32,
        tip_width: f32,
    },
}

impl PrimitiveKind {
    pub const fn glyph(glyph: char) -> Self {
        Self::Glyph { glyph }
    }

    pub const fn particle(glyph: char) -> Self {
        Self::Particle { glyph }
    }

    pub fn ribbon(length: f32, width: f32) -> Self {
        Self::Ribbon {
            length: finite_nonnegative(length),
            width: finite_nonnegative(width),
        }
    }

    pub fn streak(length: f32, width: f32) -> Self {
        Self::Streak {
            length: finite_nonnegative(length),
            width: finite_nonnegative(width),
        }
    }

    pub fn ray(length: f32, width: f32, tip_width: f32) -> Self {
        Self::Ray {
            length: finite_nonnegative(length),
            width: finite_nonnegative(width),
            tip_width: finite_nonnegative(tip_width),
        }
    }
}

/// An authored primitive plus deterministic presentation animation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Primitive {
    pub kind: PrimitiveKind,
    pub transform: Transform2,
    pub material: Material,
    pub animation: Animation,
}

impl Primitive {
    pub const fn new(kind: PrimitiveKind, transform: Transform2, material: Material) -> Self {
        Self {
            kind,
            transform,
            material,
            animation: Animation::still(),
        }
    }

    pub const fn with_animation(mut self, animation: Animation) -> Self {
        self.animation = animation;
        self
    }

    /// Resolve this primitive at presentation time without touching game
    /// state. Negative, NaN, and infinite times fall back to t=0.
    pub fn sample(&self, time: f32) -> ResolvedPrimitive {
        let t = finite_time(time);
        let half_t2 = 0.5 * t * t;
        let velocity = finite_point(self.animation.velocity);
        let acceleration = finite_point(self.animation.acceleration);
        let rotation_velocity = finite_or_zero(self.animation.rotation_velocity);
        let scale_velocity = finite_point(self.animation.scale_velocity);
        let opacity_velocity = finite_or_zero(self.animation.opacity_velocity);
        let mut transform = self.transform;
        transform.position = transform.position + velocity * t + acceleration * half_t2;
        transform.rotation += rotation_velocity * t;
        transform.scale = Point2::new(
            (transform.scale.x + scale_velocity.x * t).max(0.0),
            (transform.scale.y + scale_velocity.y * t).max(0.0),
        );
        let mut material = self.material;
        material.opacity = (material.opacity + opacity_velocity * t).clamp(0.0, 1.0);
        ResolvedPrimitive {
            kind: self.kind,
            transform,
            material,
        }
    }
}

/// A primitive after its presentation animation has been sampled.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ResolvedPrimitive {
    pub kind: PrimitiveKind,
    pub transform: Transform2,
    pub material: Material,
}

/// Renderer-agnostic continuous scene. It owns presentation descriptions only
/// and never contains authoritative snapshots or mutable gameplay positions.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ContinuousScene {
    primitives: Vec<Primitive>,
}

impl ContinuousScene {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a primitive and return its stable insertion index for callers that
    /// want to retain a presentation handle.
    pub fn push(&mut self, primitive: Primitive) -> usize {
        let id = self.primitives.len();
        self.primitives.push(primitive);
        id
    }

    pub fn primitives(&self) -> &[Primitive] {
        &self.primitives
    }

    /// Retain only the first `cap` authored primitives (no-op when the
    /// scene is already within the cap). Builders that author in priority
    /// order use this to bound their own generation contract — unlike
    /// [`ContinuousScene::sample`]'s cap, which keeps the nearest-depth
    /// tail and can drop semantically important primitives behind quiet
    /// background ink.
    pub fn truncate(&mut self, cap: usize) {
        self.primitives.truncate(cap);
    }

    /// Sample and depth-sort the scene. Sorting is stable, so equal-depth
    /// primitives retain authoring order and produce deterministic output.
    ///
    /// Bounded GPU work: the presenter materializes one GPU instance and one
    /// emission scan per resolved primitive, and [`ContinuousScene::push`]
    /// has no per-builder limit, so `sample` caps its own output. When the
    /// scene exceeds [`MAX_NATIVE_PRIMITIVES`], truncation keeps the
    /// nearest-depth tail (the primitives drawn last and therefore on top)
    /// and drops the farthest ones first; the retained set and order stay
    /// deterministic for any identical scene and time input.
    pub fn sample(&self, time: f32) -> Vec<ResolvedPrimitive> {
        // Keep only the nearest depth tail while sampling. The previous
        // implementation materialized and sorted every authored primitive,
        // which made a particle-heavy scene allocate and sort an unbounded
        // list before applying the GPU cap. The retained list never exceeds
        // the presenter budget; its final sort preserves stable depth order.
        let mut retained: Vec<(usize, &Primitive)> = Vec::with_capacity(MAX_NATIVE_PRIMITIVES);
        for (index, primitive) in self.primitives.iter().enumerate() {
            if retained.len() < MAX_NATIVE_PRIMITIVES {
                retained.push((index, primitive));
                continue;
            }

            let farthest = (0..retained.len())
                .min_by(|&left, &right| {
                    retained[left]
                        .1
                        .transform
                        .depth
                        .total_cmp(&retained[right].1.transform.depth)
                        .then_with(|| retained[left].0.cmp(&retained[right].0))
                })
                .expect("retained list is full");
            let farthest_key = retained[farthest];
            let depth_order = primitive
                .transform
                .depth
                .total_cmp(&farthest_key.1.transform.depth);
            let replaces = depth_order == std::cmp::Ordering::Greater
                || (depth_order == std::cmp::Ordering::Equal && index > farthest_key.0);
            if replaces {
                retained[farthest] = (index, primitive);
            }
        }

        retained.sort_by(|(left_index, left), (right_index, right)| {
            left.transform
                .depth
                .total_cmp(&right.transform.depth)
                .then_with(|| left_index.cmp(right_index))
        });
        retained
            .into_iter()
            .map(|(_, primitive)| primitive.sample(time))
            .collect()
    }
}

fn finite_time(time: f32) -> f32 {
    if time.is_finite() {
        time.max(0.0)
    } else {
        0.0
    }
}

fn finite_nonnegative(value: f32) -> f32 {
    if value.is_finite() {
        value.max(0.0)
    } else {
        0.0
    }
}

fn finite_or_zero(value: f32) -> f32 {
    if value.is_finite() {
        value
    } else {
        0.0
    }
}

fn finite_point(point: Point2) -> Point2 {
    Point2::new(finite_or_zero(point.x), finite_or_zero(point.y))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn orange() -> Rgb {
        Rgb::new(255, 128, 32)
    }

    #[test]
    fn fixed_time_sampling_is_deterministic_and_depth_sorted() {
        let mut scene = ContinuousScene::new();
        scene.push(Primitive::new(
            PrimitiveKind::glyph('@'),
            Transform2::new(Point2::new(4.25, 2.75), 4.0),
            Material::new(orange()),
        ));
        scene.push(Primitive::new(
            PrimitiveKind::particle('*'),
            Transform2::new(Point2::new(1.5, 0.25), -2.0),
            Material::new(Rgb::new(255, 220, 80)),
        ));

        let first = scene.sample(1.25);
        let second = scene.sample(1.25);
        assert_eq!(first, second);
        assert_eq!(first[0].transform.depth, -2.0);
        assert_eq!(first[1].transform.depth, 4.0);
        assert_eq!(first[1].transform.position, Point2::new(4.25, 2.75));
    }

    #[test]
    fn sample_output_is_capped_and_keeps_the_nearest_depth_tail() {
        let mut scene = ContinuousScene::new();
        for index in 0..(MAX_NATIVE_PRIMITIVES + 256) {
            // A distinct deep->shallow depth sweep: index 0 is farthest.
            scene.push(Primitive::new(
                PrimitiveKind::glyph('*'),
                Transform2::new(Point2::new(index as f32, 0.0), 100.0 + index as f32),
                Material::new(orange()),
            ));
        }
        let resolved = scene.sample(0.0);
        assert_eq!(resolved.len(), MAX_NATIVE_PRIMITIVES);
        // Truncation drops the farthest head: every survivor's depth exceeds
        // the depths that were cut, and the survivor order stays sorted.
        let survivor_depths: Vec<f32> = resolved
            .iter()
            .map(|primitive| primitive.transform.depth)
            .collect();
        assert_eq!(survivor_depths.first().copied(), Some(100.0 + 256.0));
        assert_eq!(
            survivor_depths.last().copied(),
            Some(100.0 + (MAX_NATIVE_PRIMITIVES + 255) as f32)
        );
        assert_eq!(scene.sample(0.0), resolved);
    }

    #[test]
    fn primitives_preserve_continuous_transform_and_emission_metadata() {
        let gradient = ColorGradient::new(
            Rgb::new(255, 255, 80),
            Rgb::new(255, 48, 12),
            GradientAxis::Radial,
        );
        let material = Material::new(orange())
            .with_background(Rgb::new(0, 0, 0))
            .with_gradient(gradient)
            .with_opacity(0.72)
            .with_emission(Emission::new(orange(), 2.5, 4.0));
        let primitive = Primitive::new(
            PrimitiveKind::ray(3.5, 0.08, 0.02),
            Transform2::new(Point2::new(2.125, 3.875), 0.5)
                .with_scale(Point2::new(0.65, 1.35))
                .with_rotation(0.37)
                .with_skew(Point2::new(0.12, -0.08)),
            material,
        );

        let resolved = primitive.sample(0.0);
        assert_eq!(resolved.transform.position, Point2::new(2.125, 3.875));
        assert_eq!(resolved.transform.scale, Point2::new(0.65, 1.35));
        assert_eq!(resolved.transform.rotation, 0.37);
        assert_eq!(resolved.transform.skew, Point2::new(0.12, -0.08));
        assert_eq!(resolved.material.opacity, 0.72);
        assert_eq!(resolved.material.gradient, Some(gradient));
        assert_eq!(resolved.material.emission.unwrap().radius, 4.0);
    }

    #[test]
    fn animation_moves_scales_rotates_and_fades_without_grid_quantization() {
        let primitive = Primitive::new(
            PrimitiveKind::particle('·'),
            Transform2::new(Point2::new(1.2, 2.4), 0.0).with_scale(Point2::new(1.0, 1.0)),
            Material::new(Rgb::new(255, 255, 255)).with_opacity(0.9),
        )
        .with_animation(
            Animation::still()
                .with_velocity(Point2::new(0.4, -0.2))
                .with_acceleration(Point2::new(0.2, 0.4))
                .with_rotation_velocity(0.5)
                .with_scale_velocity(Point2::new(-0.25, 0.1))
                .with_opacity_velocity(-0.3),
        );

        let resolved = primitive.sample(2.0);
        assert_eq!(resolved.transform.position, Point2::new(2.4, 2.8));
        assert_eq!(resolved.transform.rotation, 1.0);
        assert_eq!(resolved.transform.scale, Point2::new(0.5, 1.2));
        assert!((resolved.material.opacity - 0.3).abs() < 1e-6);
        assert_ne!(resolved.transform.position.x.fract(), 0.0);
        assert_ne!(resolved.transform.position.y.fract(), 0.0);
    }

    #[test]
    fn all_primitive_families_are_retained_by_the_scene_seam() {
        let kinds = [
            PrimitiveKind::glyph('a'),
            PrimitiveKind::particle('*'),
            PrimitiveKind::ribbon(2.0, 0.1),
            PrimitiveKind::streak(1.0, 0.04),
            PrimitiveKind::ray(3.0, 0.08, 0.01),
        ];
        let mut scene = ContinuousScene::new();
        for (index, kind) in kinds.into_iter().enumerate() {
            scene.push(Primitive::new(
                kind,
                Transform2::new(Point2::new(index as f32 + 0.25, 0.5), index as f32),
                Material::new(Rgb::new(200, 200, 200)),
            ));
        }
        let sampled = scene.sample(0.0);
        assert_eq!(sampled.len(), kinds.len());
        assert!(sampled
            .iter()
            .zip(kinds)
            .all(|(primitive, kind)| primitive.kind == kind));
    }

    #[test]
    fn invalid_animation_inputs_fall_back_to_safe_values() {
        let primitive = Primitive::new(
            PrimitiveKind::streak(f32::NAN, f32::INFINITY),
            Transform2::new(Point2::new(0.0, 0.0), 0.0),
            Material::new(Rgb::new(1, 2, 3)),
        )
        .with_animation(
            Animation::still()
                .with_velocity(Point2::new(f32::NAN, f32::INFINITY))
                .with_opacity_velocity(f32::NAN),
        );
        let sampled = primitive.sample(f32::NAN);
        assert_eq!(sampled.transform.position, Point2::ZERO);
        assert_eq!(sampled.material.opacity, 1.0);
        assert_eq!(
            sampled.kind,
            PrimitiveKind::Streak {
                length: 0.0,
                width: 0.0
            }
        );
    }
}
