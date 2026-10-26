//! CPU depth-buffered 3D-to-ASCII backend renderer.
//!
//! Takes triangle meshes (3D assets) and resolves them into an engine
//! [`Layer`](super::Layer), so the windowed presenter draws real shaded
//! geometry instead of hand-stamped glyphs.
//!
//! Ideas (not code) borrowed from msokalski/asciicker (MIT): a supersampled
//! depth buffer with a per-sample z-test, flat diffuse from the geometric
//! face normal, and glyph choice driven by depth discontinuities. Unlike
//! asciicker's ANSI software renderer this emits engine layers —
//! deterministic, dependency-free, and composable with everything else.
//!
//! Deliberately v1-scoped: face colors (no textures), no backface culling
//! (closed solids resolve correctly through the depth test alone), world
//! -axis-aligned boxes as the model builders. Transforms, model-file
//! loading, and windowed presentation ride follow-up slices.

use super::{AnimCell, Cell, Layer, Rgb};

/// Minimal 3D vector (f32 world units, y-up, camera looks along +z at
/// yaw = pitch = 0).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl std::ops::Sub for Vec3 {
    type Output = Vec3;
    /// Difference vector; overload keeps scene math reading like algebra
    /// without an ambiguous inherent `.sub` method.
    fn sub(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x - o.x, self.y - o.y, self.z - o.z)
    }
}

impl std::ops::Add for Vec3 {
    type Output = Vec3;

    fn add(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x + o.x, self.y + o.y, self.z + o.z)
    }
}

impl std::ops::Mul<f32> for Vec3 {
    type Output = Vec3;

    fn mul(self, scale: f32) -> Vec3 {
        Vec3::new(self.x * scale, self.y * scale, self.z * scale)
    }
}

impl Vec3 {
    pub fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    // The four basis ops are public so scene authors (examples) can pose
    // cameras and projectors against the same math the rasterizer uses.
    pub fn dot(self, o: Vec3) -> f32 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }

    pub fn cross(self, o: Vec3) -> Vec3 {
        Vec3::new(
            self.y * o.z - self.z * o.y,
            self.z * o.x - self.x * o.z,
            self.x * o.y - self.y * o.x,
        )
    }

    pub fn norm(self) -> Vec3 {
        let len = self.dot(self).sqrt().max(1e-9);
        Vec3::new(self.x / len, self.y / len, self.z / len)
    }
}

/// One shaded triangle: three world-space verts plus a flat albedo. Shading
/// is per-face (like the reference look), so no vertex colors or UVs yet.
#[derive(Clone, Debug)]
pub struct Tri {
    pub v: [Vec3; 3],
    pub albedo: Rgb,
}

/// A 3D asset: a flat list of triangles in world coordinates.
#[derive(Clone, Debug, Default)]
pub struct Mesh {
    pub tris: Vec<Tri>,
}

/// Presentation-only pose for the windowed humanoid model.
///
/// The authoritative simulation still owns cells and collision.  These values
/// only decide how the already-authoritative body is articulated between
/// snapshots, which keeps animation responsive without changing the wire
/// protocol or server rules.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HumanoidPose {
    /// Continuous locomotion phase in radians.  Opposite legs use opposite
    /// signs so the feet exchange weight instead of translating as one block.
    pub walk_phase: f32,
    /// Normalised strike phase, 0..=1.  The useful motion is eased in and out
    /// by the mesh builder rather than snapping the sword between poses.
    pub attack_phase: f32,
    /// Slow idle clock used for a barely-visible breathing/cloth shift.
    pub idle_phase: f32,
}

impl HumanoidPose {
    pub const fn new(walk_phase: f32, attack_phase: f32, idle_phase: f32) -> Self {
        Self {
            walk_phase,
            attack_phase,
            idle_phase,
        }
    }
}

impl Mesh {
    pub fn new() -> Self {
        Self::default()
    }

    fn push_quad(&mut self, a: Vec3, b: Vec3, c: Vec3, d: Vec3, albedo: Rgb) {
        self.tris.push(Tri {
            v: [a, b, c],
            albedo,
        });
        self.tris.push(Tri {
            v: [a, c, d],
            albedo,
        });
    }

    /// Axis-aligned box centered at `center` with full extents `size`.
    /// Winding is arbitrary: with no backface culling the depth test alone
    /// resolves closed solids, so builders never have to agree on an order.
    pub fn boxed(center: Vec3, size: Vec3, albedo: Rgb) -> Mesh {
        let hx = size.x / 2.0;
        let hy = size.y / 2.0;
        let hz = size.z / 2.0;
        let (cx, cy, cz) = (center.x, center.y, center.z);
        let c000 = Vec3::new(cx - hx, cy - hy, cz - hz);
        let c100 = Vec3::new(cx + hx, cy - hy, cz - hz);
        let c110 = Vec3::new(cx + hx, cy + hy, cz - hz);
        let c010 = Vec3::new(cx - hx, cy + hy, cz - hz);
        let c001 = Vec3::new(cx - hx, cy - hy, cz + hz);
        let c101 = Vec3::new(cx + hx, cy - hy, cz + hz);
        let c111 = Vec3::new(cx + hx, cy + hy, cz + hz);
        let c011 = Vec3::new(cx - hx, cy + hy, cz + hz);
        let mut mesh = Mesh::new();
        mesh.push_quad(c001, c101, c111, c011, albedo); // front
        mesh.push_quad(c100, c000, c010, c110, albedo); // back
        mesh.push_quad(c000, c001, c011, c010, albedo); // left
        mesh.push_quad(c101, c100, c110, c111, albedo); // right
        mesh.push_quad(c011, c111, c110, c010, albedo); // top
        mesh.push_quad(c000, c100, c101, c001, albedo); // bottom
        mesh
    }

    /// Low-poly conical volume for organic silhouettes such as pines and
    /// canvas tents.  Side triangles are intentionally flat-shaded: the
    /// rasteriser's face lighting then gives each terminal cell a coherent
    /// facet instead of a plastic smooth blob.
    pub fn cone(base: Vec3, radius: f32, height: f32, segments: u32, albedo: Rgb) -> Mesh {
        let segments = segments.max(3);
        let apex = Vec3::new(base.x, base.y + height.max(0.0), base.z);
        let mut mesh = Mesh::new();
        for i in 0..segments {
            let a0 = i as f32 / segments as f32 * std::f32::consts::TAU;
            let a1 = (i + 1) as f32 / segments as f32 * std::f32::consts::TAU;
            let p0 = Vec3::new(
                base.x + radius.max(0.0) * a0.cos(),
                base.y,
                base.z + radius.max(0.0) * a0.sin(),
            );
            let p1 = Vec3::new(
                base.x + radius.max(0.0) * a1.cos(),
                base.y,
                base.z + radius.max(0.0) * a1.sin(),
            );
            mesh.tris.push(Tri {
                v: [p0, p1, apex],
                albedo,
            });
        }
        mesh
    }

    /// Unit cube at the origin (test/fixture helper).
    pub fn cube(size: f32, albedo: Rgb) -> Mesh {
        Mesh::boxed(
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(size, size, size),
            albedo,
        )
    }

    /// Stacked-box humanoid (~3.9 units tall, feet at y = 0): the depth
    /// demo's actor, with cloth/skin/metal part tints.
    pub fn humanoid() -> Mesh {
        let cloth = Rgb::new(52, 58, 92);
        let leather = Rgb::new(122, 86, 52);
        let skin = Rgb::new(198, 158, 126);
        let steel = Rgb::new(150, 170, 190);
        let mut mesh = Mesh::new();
        let mut add = |m: Mesh| mesh.tris.extend(m.tris);
        // Legs.
        add(Mesh::boxed(
            Vec3::new(-0.32, 0.8, 0.0),
            Vec3::new(0.5, 1.6, 0.55),
            cloth,
        ));
        add(Mesh::boxed(
            Vec3::new(0.32, 0.8, 0.0),
            Vec3::new(0.5, 1.6, 0.55),
            cloth,
        ));
        // Torso + belt + pauldrons.
        add(Mesh::boxed(
            Vec3::new(0.0, 2.35, 0.0),
            Vec3::new(1.3, 1.5, 0.7),
            leather,
        ));
        add(Mesh::boxed(
            Vec3::new(-0.85, 2.9, 0.0),
            Vec3::new(0.4, 0.35, 0.6),
            steel,
        ));
        add(Mesh::boxed(
            Vec3::new(0.85, 2.9, 0.0),
            Vec3::new(0.4, 0.35, 0.6),
            steel,
        ));
        // Arms + head.
        add(Mesh::boxed(
            Vec3::new(-0.95, 2.1, 0.0),
            Vec3::new(0.38, 1.4, 0.42),
            skin,
        ));
        add(Mesh::boxed(
            Vec3::new(0.95, 2.1, 0.0),
            Vec3::new(0.38, 1.4, 0.42),
            skin,
        ));
        add(Mesh::boxed(
            Vec3::new(0.0, 3.55, 0.0),
            Vec3::new(0.72, 0.72, 0.72),
            skin,
        ));
        // Helm + blade.
        add(Mesh::boxed(
            Vec3::new(0.0, 3.95, 0.0),
            Vec3::new(0.78, 0.18, 0.78),
            steel,
        ));
        add(Mesh::boxed(
            Vec3::new(1.25, 2.3, 0.2),
            Vec3::new(0.12, 1.7, 0.12),
            steel,
        ));
        mesh
    }

    /// Articulated humanoid used by the live windowed play path.
    ///
    /// This deliberately remains a small box-based model: the raster backend
    /// has no skinning or skeletal GPU pass.  Joint-pivot transforms still give
    /// the silhouette believable shoulder, elbow, hip and knee articulation,
    /// while overlapping armour pieces hide seams at terminal-cell scale.
    /// `humanoid()` remains the legacy rigid fixture used by older examples and
    /// tests; new callers should pass a pose here.
    pub fn humanoid_pose(pose: HumanoidPose) -> Mesh {
        let cloth = Rgb::new(72, 84, 126);
        let leather = Rgb::new(150, 96, 52);
        let skin = Rgb::new(212, 170, 132);
        let steel = Rgb::new(190, 205, 220);
        let steel_dark = Rgb::new(120, 135, 160);
        let gold = Rgb::new(222, 178, 62);
        let mut mesh = Mesh::new();
        let mut add = |part: Mesh| mesh.tris.extend(part.tris);
        let posed_box = |center: Vec3, size: Vec3, albedo: Rgb, pivot: Vec3, pitch: f32| -> Mesh {
            Mesh::boxed(center, size, albedo).transformed_about(
                pivot,
                &Transform {
                    pitch,
                    ..Transform::identity()
                },
            )
        };

        let walk = pose.walk_phase;
        let leg_swing = walk.sin() * 0.28;
        let arm_swing = (walk + std::f32::consts::PI).sin() * 0.20;
        let strike = (pose.attack_phase.clamp(0.0, 1.0) * std::f32::consts::PI).sin();
        let attack_lift = strike * 0.78;
        let breath = if pose.walk_phase.abs() < 0.01 {
            pose.idle_phase.sin() * 0.025
        } else {
            0.0
        };

        // Pelvis, torso and neck overlap by a few centimetres.  This is less
        // brittle than touching boxes and prevents visible holes at the hips.
        add(Mesh::boxed(
            Vec3::new(0.0, 0.84 + breath * 0.5, 0.0),
            Vec3::new(0.82, 0.36, 0.58),
            leather,
        ));
        add(Mesh::boxed(
            Vec3::new(0.0, 1.63 + breath, 0.0),
            Vec3::new(1.28, 1.28, 0.68),
            cloth,
        ));
        add(Mesh::boxed(
            Vec3::new(0.0, 2.23 + breath, 0.02),
            Vec3::new(1.05, 0.18, 0.72),
            steel,
        ));

        // Head and helm are kept independent from the torso so the tiny idle
        // lift reads as breathing instead of a rigid statue.
        add(Mesh::boxed(
            Vec3::new(0.0, 3.08 + breath, 0.0),
            Vec3::new(0.72, 0.72, 0.72),
            skin,
        ));
        add(Mesh::boxed(
            Vec3::new(0.0, 3.48 + breath, 0.0),
            Vec3::new(0.78, 0.18, 0.78),
            steel,
        ));
        // Small authored face and heraldry planes keep the actor readable at
        // close zoom without turning the model into a solid featureless box.
        add(Mesh::boxed(
            Vec3::new(0.0, 3.08 + breath, 0.37),
            Vec3::new(0.46, 0.12, 0.08),
            steel_dark,
        ));
        add(Mesh::boxed(
            Vec3::new(0.0, 2.18 + breath, 0.37),
            Vec3::new(0.14, 0.42, 0.08),
            gold,
        ));
        add(Mesh::boxed(
            Vec3::new(0.0, 3.66 + breath, 0.10),
            Vec3::new(0.14, 0.22, 0.28),
            gold,
        ));

        // Each leg is built as two overlapping segments around a hip and knee
        // pivot.  The foot boxes stay close to y=0 throughout the gait cycle.
        add(posed_box(
            Vec3::new(-0.30, 0.91, leg_swing * 0.16),
            Vec3::new(0.42, 0.78, 0.48),
            cloth,
            Vec3::new(-0.30, 1.27, 0.0),
            leg_swing * 0.34,
        ));
        add(posed_box(
            Vec3::new(-0.30, 0.36, leg_swing * 0.42),
            Vec3::new(0.38, 0.68, 0.44),
            steel_dark,
            Vec3::new(-0.30, 0.61, leg_swing * 0.16),
            -leg_swing * 0.20,
        ));
        add(Mesh::boxed(
            Vec3::new(-0.30, 0.13, 0.10 + leg_swing * 0.52),
            Vec3::new(0.48, 0.24, 0.62),
            leather,
        ));
        add(posed_box(
            Vec3::new(0.30, 0.91, -leg_swing * 0.16),
            Vec3::new(0.42, 0.78, 0.48),
            cloth,
            Vec3::new(0.30, 1.27, 0.0),
            -leg_swing * 0.34,
        ));
        add(posed_box(
            Vec3::new(0.30, 0.36, -leg_swing * 0.42),
            Vec3::new(0.38, 0.68, 0.44),
            steel_dark,
            Vec3::new(0.30, 0.61, -leg_swing * 0.16),
            leg_swing * 0.20,
        ));
        add(Mesh::boxed(
            Vec3::new(0.30, 0.13, 0.10 - leg_swing * 0.52),
            Vec3::new(0.48, 0.24, 0.62),
            leather,
        ));

        // Shoulder and forearm pieces use the same shoulder pivot.  The
        // overlap is intentional: it keeps the elbow silhouette connected
        // while preserving a visibly different upper/lower arm angle.
        let left_arm = arm_swing * 0.55;
        let right_arm = -arm_swing * 0.55 - attack_lift;
        add(posed_box(
            Vec3::new(-0.76, 1.94, 0.0),
            Vec3::new(0.34, 0.72, 0.40),
            skin,
            Vec3::new(-0.76, 2.28, 0.0),
            left_arm,
        ));
        add(posed_box(
            Vec3::new(-0.76, 1.48, 0.04),
            Vec3::new(0.30, 0.62, 0.36),
            steel_dark,
            Vec3::new(-0.76, 1.66, 0.04),
            -left_arm * 0.65,
        ));
        add(posed_box(
            Vec3::new(0.76, 1.94, 0.0),
            Vec3::new(0.34, 0.72, 0.40),
            skin,
            Vec3::new(0.76, 2.28, 0.0),
            right_arm,
        ));
        add(posed_box(
            Vec3::new(0.76, 1.48, 0.10),
            Vec3::new(0.30, 0.62, 0.36),
            steel_dark,
            Vec3::new(0.76, 1.66, 0.10),
            -right_arm * 0.72,
        ));
        add(posed_box(
            Vec3::new(0.76, 1.92, 0.42),
            Vec3::new(0.12, 1.35, 0.14),
            steel,
            Vec3::new(0.76, 1.66, 0.10),
            -right_arm * 0.55,
        ));
        add(Mesh::boxed(
            Vec3::new(0.76, 1.67, 0.10),
            Vec3::new(0.48, 0.08, 0.10),
            gold,
        ));

        // A final global correction keeps the authored feet grounded even at
        // the ends of the small gait envelope.  It never changes the relative
        // pose or the camera-facing anchor.
        let min_y = mesh
            .tris
            .iter()
            .flat_map(|tri| tri.v.iter())
            .map(|v| v.y)
            .fold(f32::INFINITY, f32::min);
        if min_y < 0.0 {
            for tri in &mut mesh.tris {
                for v in &mut tri.v {
                    v.y -= min_y;
                }
            }
        }
        mesh
    }

    /// Low, broad forest creature with a readable head, muzzle, horns, and
    /// four planted feet.  It intentionally shares the same flat-facet
    /// language as the humanoid instead of becoming a noisy texture blob.
    pub fn mossling_pose(pose: HumanoidPose) -> Mesh {
        let green = Rgb::new(74, 132, 66);
        let dark = Rgb::new(42, 82, 44);
        let bark = Rgb::new(104, 74, 48);
        let eye = Rgb::new(255, 194, 64);
        let mut mesh = Mesh::new();
        let breathe = pose.idle_phase.sin() * 0.04;
        let stride = pose.walk_phase.sin() * 0.15;
        let lunge = (pose.attack_phase.clamp(0.0, 1.0) * std::f32::consts::PI).sin() * 0.22;
        let mut add = |part: Mesh| mesh.tris.extend(part.tris);
        add(Mesh::boxed(
            Vec3::new(0.0, 0.78 + breathe, 0.0),
            Vec3::new(1.45, 0.92, 1.05),
            green,
        ));
        add(Mesh::boxed(
            Vec3::new(0.0, 1.38 + breathe, 0.22 + lunge),
            Vec3::new(0.92, 0.56, 0.78),
            dark,
        ));
        add(Mesh::boxed(
            Vec3::new(0.0, 1.28 + breathe, 0.67 + lunge),
            Vec3::new(0.62, 0.24, 0.36),
            bark,
        ));
        add(Mesh::boxed(
            Vec3::new(-0.20, 1.42 + breathe, 0.62 + lunge),
            Vec3::new(0.12, 0.12, 0.06),
            eye,
        ));
        add(Mesh::boxed(
            Vec3::new(0.20, 1.42 + breathe, 0.62 + lunge),
            Vec3::new(0.12, 0.12, 0.06),
            eye,
        ));
        // Mossling bark spots and a small brow/teeth break up the broad green
        // body into stable, material-specific planes.
        for (x, y, z, size) in [
            (-0.38, 0.78, 0.54, 0.16),
            (0.38, 0.86, 0.53, 0.13),
            (-0.08, 0.58, 0.54, 0.11),
        ] {
            add(Mesh::boxed(
                Vec3::new(x, y + breathe, z + lunge),
                Vec3::new(size, size, 0.06),
                dark,
            ));
        }
        add(Mesh::boxed(
            Vec3::new(0.0, 1.54 + breathe, 0.56 + lunge),
            Vec3::new(0.56, 0.08, 0.08),
            bark,
        ));
        for (x, z, phase) in [
            (-0.48, -0.32, stride),
            (0.48, -0.32, -stride),
            (-0.48, 0.32, -stride),
            (0.48, 0.32, stride),
        ] {
            add(Mesh::boxed(
                Vec3::new(x, 0.32, z + phase),
                Vec3::new(0.30, 0.60, 0.30),
                bark,
            ));
        }
        add(Mesh::cone(
            Vec3::new(-0.32, 1.62 + breathe, 0.10),
            0.14,
            0.48,
            6,
            bark,
        ));
        add(Mesh::cone(
            Vec3::new(0.32, 1.62 + breathe, 0.10),
            0.14,
            0.48,
            6,
            bark,
        ));
        mesh
    }

    /// Lean undead silhouette: skull/jaw, ribcage, pelvis, separated limbs,
    /// and a single rusty blade.  The gaps are deliberate negative space so
    /// it reads as a skeleton rather than a pale humanoid block.
    pub fn skeleton_pose(pose: HumanoidPose) -> Mesh {
        let bone = Rgb::new(202, 196, 174);
        let shadow = Rgb::new(104, 92, 78);
        let rust = Rgb::new(136, 72, 48);
        let ember = Rgb::new(255, 118, 62);
        let sway = pose.walk_phase.sin() * 0.10;
        let strike = (pose.attack_phase.clamp(0.0, 1.0) * std::f32::consts::PI).sin() * 0.42;
        let mut mesh = Mesh::new();
        let mut add = |part: Mesh| mesh.tris.extend(part.tris);
        add(Mesh::boxed(
            Vec3::new(0.0, 1.95, 0.0),
            Vec3::new(0.62, 0.58, 0.58),
            bone,
        ));
        add(Mesh::boxed(
            Vec3::new(0.0, 1.70, 0.25),
            Vec3::new(0.38, 0.18, 0.18),
            shadow,
        ));
        add(Mesh::boxed(
            Vec3::new(0.0, 1.32, 0.0),
            Vec3::new(0.18, 0.72, 0.18),
            bone,
        ));
        add(Mesh::boxed(
            Vec3::new(-0.24, 1.42, 0.0),
            Vec3::new(0.10, 0.48, 0.10),
            bone,
        ));
        add(Mesh::boxed(
            Vec3::new(0.24, 1.42, 0.0),
            Vec3::new(0.10, 0.48, 0.10),
            bone,
        ));
        for y in [1.56, 1.38, 1.20] {
            add(Mesh::boxed(
                Vec3::new(0.0, y, 0.10),
                Vec3::new(0.52, 0.08, 0.10),
                bone,
            ));
        }
        add(Mesh::boxed(
            Vec3::new(0.0, 0.88, 0.0),
            Vec3::new(0.48, 0.18, 0.28),
            shadow,
        ));
        for (x, z) in [(-0.16, sway), (0.16, -sway)] {
            add(Mesh::boxed(
                Vec3::new(x, 0.50, z),
                Vec3::new(0.10, 0.62, 0.10),
                bone,
            ));
            add(Mesh::boxed(
                Vec3::new(x, 0.13, z + 0.10),
                Vec3::new(0.12, 0.14, 0.32),
                shadow,
            ));
        }
        add(Mesh::boxed(
            Vec3::new(-0.36, 1.20, 0.0),
            Vec3::new(0.10, 0.52, 0.10),
            bone,
        ));
        add(Mesh::boxed(
            Vec3::new(0.36, 1.20, strike),
            Vec3::new(0.10, 0.52, 0.10),
            bone,
        ));
        add(Mesh::boxed(
            Vec3::new(0.38, 0.85, strike + 0.16),
            Vec3::new(0.08, 0.34, 0.08),
            shadow,
        ));
        add(Mesh::boxed(
            Vec3::new(0.38, 1.30, strike + 0.38),
            Vec3::new(0.06, 0.96, 0.16),
            rust,
        ));
        add(Mesh::boxed(
            Vec3::new(-0.13, 2.02, 0.30),
            Vec3::new(0.08, 0.08, 0.04),
            ember,
        ));
        add(Mesh::boxed(
            Vec3::new(0.13, 2.02, 0.30),
            Vec3::new(0.08, 0.08, 0.04),
            ember,
        ));
        mesh
    }

    /// Readable robed NPC with hood, staff, and a bright lantern accent.
    pub fn herbalist_pose(idle_phase: f32) -> Mesh {
        let robe = Rgb::new(52, 92, 132);
        let hood = Rgb::new(36, 56, 92);
        let leather = Rgb::new(124, 72, 46);
        let brass = Rgb::new(212, 168, 66);
        let lantern = Rgb::new(255, 198, 72);
        let flicker = idle_phase.sin() * 0.04;
        let mut mesh = Mesh::new();
        let mut add = |part: Mesh| mesh.tris.extend(part.tris);
        add(Mesh::boxed(
            Vec3::new(0.0, 0.82, 0.0),
            Vec3::new(0.82, 1.42, 0.62),
            robe,
        ));
        add(Mesh::boxed(
            Vec3::new(0.0, 1.70, 0.0),
            Vec3::new(0.66, 0.58, 0.66),
            hood,
        ));
        add(Mesh::boxed(
            Vec3::new(0.0, 1.64, 0.31),
            Vec3::new(0.34, 0.22, 0.08),
            Rgb::new(18, 22, 32),
        ));
        add(Mesh::boxed(
            Vec3::new(0.0, 1.43, 0.34),
            Vec3::new(0.86, 0.10, 0.12),
            leather,
        ));
        add(Mesh::boxed(
            Vec3::new(0.0, 0.78, 0.34),
            Vec3::new(0.68, 0.08, 0.10),
            hood,
        ));
        add(Mesh::boxed(
            Vec3::new(0.46, 1.04, 0.12),
            Vec3::new(0.10, 2.08, 0.10),
            leather,
        ));
        add(Mesh::boxed(
            Vec3::new(0.46, 1.92, 0.12),
            Vec3::new(0.28, 0.32, 0.28),
            brass,
        ));
        add(Mesh::boxed(
            Vec3::new(0.46, 1.92, 0.12),
            Vec3::new(0.16 + flicker, 0.20 + flicker, 0.16 + flicker),
            lantern,
        ));
        mesh
    }

    /// Flat axis-aligned quad pressed into the y = `center.y` plane: two
    /// triangles, no side faces — the cheap building block for zoned ground
    /// fields and the ocean sheet (a `boxed` per block would be ~6x the
    /// triangles for no visible gain at near-flat incidence).
    pub fn quad_y(center: Vec3, half_x: f32, half_z: f32, albedo: Rgb) -> Mesh {
        let mut mesh = Mesh::new();
        let (cx, cy, cz) = (center.x, center.y, center.z);
        let a = Vec3::new(cx - half_x, cy, cz - half_z);
        let b = Vec3::new(cx + half_x, cy, cz - half_z);
        let c = Vec3::new(cx + half_x, cy, cz + half_z);
        let d = Vec3::new(cx - half_x, cy, cz + half_z);
        mesh.push_quad(a, b, c, d, albedo);
        mesh
    }

    /// Flat ground decal rotated by `yaw` around its center: the long axis
    /// points along (sin yaw, cos yaw), matching the camera-facing yaw
    /// convention. FX sweep rings and burst dashes are quarter-scale ground
    /// decals; a box per dash adds side faces nobody sees at grazing angles.
    pub fn decal_y(center: Vec3, yaw: f32, half_long: f32, half_wide: f32, albedo: Rgb) -> Mesh {
        let (s, c) = yaw.sin_cos();
        let (dx, dz) = (s, c);
        let (sx, sz) = (c, -s);
        let mut mesh = Mesh::new();
        let a = Vec3::new(
            center.x - dx * half_long - sx * half_wide,
            center.y,
            center.z - dz * half_long - sz * half_wide,
        );
        let b = Vec3::new(
            center.x + dx * half_long - sx * half_wide,
            center.y,
            center.z + dz * half_long - sz * half_wide,
        );
        let c2 = Vec3::new(
            center.x + dx * half_long + sx * half_wide,
            center.y,
            center.z + dz * half_long + sz * half_wide,
        );
        let d = Vec3::new(
            center.x - dx * half_long + sx * half_wide,
            center.y,
            center.z - dz * half_long + sz * half_wide,
        );
        mesh.push_quad(a, b, c2, d, albedo);
        mesh
    }

    /// A pair of crossed vertical ribbons for a grass blade. `tip_offset`
    /// gives the blade a presentation-only lean while the base remains fixed
    /// to the authoritative ground position, so weather can wave foliage
    /// without changing collision or world coordinates.
    pub fn blade_y(
        base: Vec3,
        height: f32,
        half_width: f32,
        tip_offset: Vec3,
        yaw: f32,
        albedo: Rgb,
    ) -> Mesh {
        let height = height.max(0.0);
        let half_width = half_width.max(0.0);
        let tip = Vec3::new(
            base.x + tip_offset.x,
            base.y + height + tip_offset.y,
            base.z + tip_offset.z,
        );
        let mut mesh = Mesh::new();
        for angle in [yaw, yaw + std::f32::consts::FRAC_PI_2] {
            let (s, c) = angle.sin_cos();
            let side = Vec3::new(c * half_width, 0.0, -s * half_width);
            let a = base - side;
            let b = base + side;
            let tip_side = side * 0.3;
            let c2 = tip + tip_side;
            let d = tip - tip_side;
            mesh.push_quad(a, b, c2, d, albedo);
        }
        mesh
    }

    /// A thin diagonal ribbon used for rain. The endpoints are supplied in
    /// world space so the caller can animate falling and wind drift with
    /// fractional positions; no terminal-cell stepping is involved.
    pub fn streak(start: Vec3, end: Vec3, half_width: f32, albedo: Rgb) -> Mesh {
        let half_width = half_width.max(0.0);
        let mut mesh = Mesh::new();
        // Two perpendicular vertical ribbons give short streaks enough face
        // area to survive the elevated ASCII raster at oblique angles.
        for side in [
            Vec3::new(half_width, 0.0, 0.0),
            Vec3::new(0.0, 0.0, half_width),
        ] {
            mesh.push_quad(start - side, start + side, end + side, end - side, albedo);
        }
        mesh
    }

    /// Repaint every face whose albedo matches `from`; stack-box recipes
    /// keep their material breaks while whole figures take a new palette.
    pub fn retinted(mut self, from: Rgb, to: Rgb) -> Mesh {
        for tri in &mut self.tris {
            if tri.albedo == from {
                tri.albedo = to;
            }
        }
        self
    }

    /// Large ground slab (top face at y = 0) for depth context under actors.
    pub fn ground_slab(albedo: Rgb) -> Mesh {
        Mesh::boxed(
            Vec3::new(0.0, -0.35, 6.0),
            Vec3::new(34.0, 0.7, 34.0),
            albedo,
        )
    }

    /// Bake a transform into every vertex, returning a new mesh. Face
    /// shading stays exact: normals are recomputed from vertices at raster
    /// time, so baked rotation can never stale the lighting.
    pub fn transformed(&self, t: &Transform) -> Mesh {
        self.transformed_about(Vec3::new(0.0, 0.0, 0.0), t)
    }

    /// Like [`transformed`](Self::transformed), but rotation and scale pivot
    /// around `pivot` (a joint anchor for posing limbs) instead of the
    /// origin: `v' = R·S·(v − pivot) + pivot + translation`.
    pub fn transformed_about(&self, pivot: Vec3, t: &Transform) -> Mesh {
        let (sy, cy) = t.yaw.sin_cos();
        let (sp, cp) = t.pitch.sin_cos();
        let (sr, cr) = t.roll.sin_cos();
        // R = Ry(yaw) · Rx(pitch) · Rz(roll), applied to the scaled offset.
        let rot = |v: Vec3| {
            let x1 = cr * v.x - sr * v.y;
            let y1 = sr * v.x + cr * v.y;
            let z1 = v.z;
            let x2 = x1;
            let y2 = cp * y1 - sp * z1;
            let z2 = sp * y1 + cp * z1;
            Vec3::new(cy * x2 + sy * z2, y2, -sy * x2 + cy * z2)
        };
        let mut out = Mesh::new();
        for tri in &self.tris {
            let map = |v: Vec3| {
                let o = v - pivot;
                let s = Vec3::new(o.x * t.scale.x, o.y * t.scale.y, o.z * t.scale.z);
                let r = rot(s);
                Vec3::new(
                    r.x + pivot.x + t.translation.x,
                    r.y + pivot.y + t.translation.y,
                    r.z + pivot.z + t.translation.z,
                )
            };
            out.tris.push(Tri {
                v: [map(tri.v[0]), map(tri.v[1]), map(tri.v[2])],
                albedo: tri.albedo,
            });
        }
        out
    }
}

/// Object placement (and later, joint posing): non-uniform scale, then
/// yaw/pitch/roll rotation, then translation. Baked into vertices by
/// [`Mesh::transformed`], so raster files never carry matrices.
#[derive(Clone, Copy, Debug)]
pub struct Transform {
    pub translation: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub roll: f32,
    pub scale: Vec3,
}

impl Transform {
    /// No rotation, unit scale, no offset.
    pub fn identity() -> Self {
        Self {
            translation: Vec3::new(0.0, 0.0, 0.0),
            yaw: 0.0,
            pitch: 0.0,
            roll: 0.0,
            scale: Vec3::new(1.0, 1.0, 1.0),
        }
    }

    /// Pure placement: move without rotating or resizing.
    pub fn placed(x: f32, y: f32, z: f32) -> Self {
        Self {
            translation: Vec3::new(x, y, z),
            ..Self::identity()
        }
    }
}

/// Load a Wavefront OBJ model (the `v`/`f` subset) into a [`Mesh`] with one
/// flat `albedo`. Faces may be triangles, quads, or n-gons (fan
/// triangulated); vertex tokens may carry `v/vt/vn` suffixes (only the
/// position index is read); indices are 1-based with OBJ-negative
/// from-the-end allowed. Everything else (`vt`, `vn`, `g`, `o`, `mtllib`,
/// `usemtl`, `s`, comments, blank lines, CRLF) is ignored. Takes `&str` so
/// the engine stays I/O-free — callers embed with `include_str!` (like the
/// ASCII art) or read files in examples/tools. Errors name the line.
pub fn load_obj(text: &str, albedo: Rgb) -> Result<Mesh, String> {
    let mut verts: Vec<Vec3> = Vec::new();
    let mut mesh = Mesh::new();
    for (lineno, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.split_whitespace();
        match parts.next() {
            Some("v") => {
                let coords: Result<Vec<f32>, _> = parts.take(3).map(|c| c.parse()).collect();
                let coords =
                    coords.map_err(|_| format!("obj line {}: bad vertex `{line}`", lineno + 1))?;
                if coords.len() < 3 {
                    return Err(format!(
                        "obj line {}: vertex needs 3 coords `{line}`",
                        lineno + 1
                    ));
                }
                verts.push(Vec3::new(coords[0], coords[1], coords[2]));
            }
            Some("f") => {
                let mut idx = Vec::new();
                for token in parts {
                    let pos = token.split('/').next().unwrap_or("");
                    let i: i32 = pos.parse().map_err(|_| {
                        format!("obj line {}: bad face index `{token}`", lineno + 1)
                    })?;
                    // OBJ: 1-based, negatives count back from the last vertex.
                    let resolved = if i > 0 { i - 1 } else { verts.len() as i32 + i };
                    if resolved < 0 || resolved >= verts.len() as i32 {
                        return Err(format!(
                            "obj line {}: index out of range `{token}`",
                            lineno + 1
                        ));
                    }
                    idx.push(resolved as usize);
                }
                if idx.len() < 3 {
                    return Err(format!("obj line {}: face needs 3+ verts", lineno + 1));
                }
                // Fan triangulation: exact for convex polys, the asset norm.
                for i in 1..idx.len() - 1 {
                    mesh.tris.push(Tri {
                        v: [verts[idx[0]], verts[idx[i]], verts[idx[i + 1]]],
                        albedo,
                    });
                }
            }
            _ => continue,
        }
    }
    Ok(mesh)
}

/// Pinhole camera. `focal` is in cell units: a target `h` units tall at
/// distance `d` spans `focal * h / d` rows.
#[derive(Clone, Copy, Debug)]
pub struct Camera3 {
    pub pos: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub focal: f32,
    pub width: i32,
    pub height: i32,
}

impl Camera3 {
    pub fn new(pos: Vec3, yaw: f32, pitch: f32, focal: f32, width: i32, height: i32) -> Self {
        Self {
            pos,
            yaw,
            pitch,
            focal,
            width,
            height,
        }
    }
}

/// Depth/render tuning.
#[derive(Clone, Copy, Debug)]
pub struct DepthOpts {
    /// Closer than this is clipped.
    pub near: f32,
    /// Fog reaches full strength here; also the depth range normalizer.
    pub far: f32,
    /// Floor for unlit faces (0..1). Low enough that modeling shadows read;
    /// the rim pass (not ambient) separates actors from the ground.
    pub ambient: f32,
    /// World-unit depth step inside one supersampled cell block that reads as
    /// an edge.
    pub edge_step: f32,
    /// Cool directional tint used to make the night scene read as moonlit
    /// instead of uniformly neutral gray. Strength stays deliberately low so
    /// warm local materials can still carry their own identity.
    pub moon_tint: Rgb,
    pub moonlight_strength: f32,
}

impl Default for DepthOpts {
    fn default() -> Self {
        Self {
            near: 0.5,
            far: 40.0,
            ambient: 0.28,
            edge_step: 0.36,
            moon_tint: Rgb::new(178, 198, 238),
            moonlight_strength: 0.15,
        }
    }
}

/// A localized world-space light. It contributes only to visible geometry
/// samples; it never creates a screen-space halo or paints empty cells.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointLight {
    pub position: Vec3,
    pub color: Rgb,
    pub radius: f32,
    pub intensity: f32,
}

impl PointLight {
    pub const fn new(position: Vec3, color: Rgb, radius: f32, intensity: f32) -> Self {
        Self {
            position,
            color,
            radius,
            intensity,
        }
    }
}

/// Samples per character cell on each axis. Four-by-four keeps diagonal
/// silhouettes and small props from collapsing into a single block while the
/// final output remains one native terminal glyph per cell.
pub(crate) const SAMPLE_GRID: usize = 4;

/// Material families deliberately extend the old global luminance ramp rather
/// than letting every surface collapse to the same ten characters.
const GENERIC_GLYPHS: &[u8] = b" .,:;`'\"_-+=*/\\|![]{}()<>xX%#@Oo0&$?^v~";
const FOLIAGE_GLYPHS: &[u8] = b" .,:;`'\"+*xX#%&@Oo";
const BARK_GLYPHS: &[u8] = b" ._-'`|!iIl1tT/\\+=*#%";
const EARTH_GLYPHS: &[u8] = b" .,:;`'\"_-~=+*#%@";
const STONE_GLYPHS: &[u8] = b" .,:;._-+=/\\xX#%&@$O0";
const WATER_GLYPHS: &[u8] = b" .,-_~^v=<>";
const FIRE_GLYPHS: &[u8] = b".,:;*+xX#%@Oo";
const MAGIC_GLYPHS: &[u8] = b".:+*xX#%@Oo0";
const METAL_GLYPHS: &[u8] = b" .-_=+*/\\|!xX#%&@$O0";
/// The world renderer never paints a framebuffer background. A resolved
/// geometry cell owns a foreground glyph; everything between glyphs is this
/// exact black clear value.
const WORLD_BLACK: Rgb = Rgb::new(0, 0, 0);

/// Coarse material identity used by the cell resolver. The mesh API still
/// accepts compact RGB albedos, but the rasterizer does not reduce every
/// surface to one global luminance ramp: each class gets its own glyph
/// language and density thresholds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MaterialKind {
    Generic,
    Foliage,
    Bark,
    Earth,
    Stone,
    Water,
    Fire,
    Magic,
    Metal,
}

impl MaterialKind {
    pub(crate) fn classify(c: Rgb) -> Self {
        let r = c.r as i16;
        let g = c.g as i16;
        let b = c.b as i16;
        if r > 175 && g > 85 && b < g - 20 {
            return Self::Fire;
        }
        if b > r + 28 && g > r + 8 {
            return Self::Water;
        }
        if b > r + 28 && b > g + 12 {
            return Self::Magic;
        }
        if g > r + 12 && g > b + 6 {
            return Self::Foliage;
        }
        if r > g + 18 && g > b + 8 {
            // Bright, high-value warm surfaces are sand/earth/leather rather
            // than bark. Darker, red-brown faces retain the vertical bark
            // language used by trunks, roots, and wooden props.
            return if r > 112 && g > 78 {
                Self::Earth
            } else if r > 72 {
                Self::Bark
            } else {
                Self::Earth
            };
        }
        if (r - g).abs() < 28 && (g - b).abs() < 28 && r < 190 {
            return Self::Stone;
        }
        if r > 155 && g > 155 && b > 155 {
            return Self::Metal;
        }
        Self::Generic
    }

    pub(crate) fn index(self) -> usize {
        match self {
            Self::Generic => 0,
            Self::Foliage => 1,
            Self::Bark => 2,
            Self::Earth => 3,
            Self::Stone => 4,
            Self::Water => 5,
            Self::Fire => 6,
            Self::Magic => 7,
            Self::Metal => 8,
        }
    }

    pub(crate) fn from_index(index: usize) -> Self {
        match index {
            1 => Self::Foliage,
            2 => Self::Bark,
            3 => Self::Earth,
            4 => Self::Stone,
            5 => Self::Water,
            6 => Self::Fire,
            7 => Self::Magic,
            8 => Self::Metal,
            _ => Self::Generic,
        }
    }

    fn glyphs(self) -> &'static [u8] {
        match self {
            Self::Generic => GENERIC_GLYPHS,
            Self::Foliage => FOLIAGE_GLYPHS,
            Self::Bark => BARK_GLYPHS,
            Self::Earth => EARTH_GLYPHS,
            Self::Stone => STONE_GLYPHS,
            Self::Water => WATER_GLYPHS,
            Self::Fire => FIRE_GLYPHS,
            Self::Magic => MAGIC_GLYPHS,
            Self::Metal => METAL_GLYPHS,
        }
    }
}

/// One supersampled fragment: perspective-correct depth, shaded albedo
/// (0..1 floats), flat-face view normal, and its luminance. A `SAMPLE_GRID` by
/// `SAMPLE_GRID` block resolves to one cell.
#[derive(Clone, Copy)]
pub(crate) struct Sample {
    pub(crate) depth: f32,
    pub(crate) r: f32,
    pub(crate) g: f32,
    pub(crate) b: f32,
    pub(crate) lum: f32,
    pub(crate) lum_sq: f32,
    pub(crate) normal_x: f32,
    pub(crate) normal_y: f32,
    pub(crate) normal_z: f32,
    pub(crate) material: MaterialKind,
}

impl Sample {
    pub(crate) fn empty() -> Self {
        Self {
            depth: f32::INFINITY,
            r: 0.0,
            g: 0.0,
            b: 0.0,
            lum: 0.0,
            lum_sq: 0.0,
            normal_x: 0.0,
            normal_y: 0.0,
            normal_z: 0.0,
            material: MaterialKind::Generic,
        }
    }
}

/// Render `meshes` through `cam` into a still-cell layer at height `z`.
///
/// Pipeline per frame: project → flat-shade each face → barycentric raster
/// into a 4x supersampled depth buffer (near wins) → resolve each 4x4 block
/// to a material-aware glyph, with depth-edge strokes where a block spans a
/// step and distance-aware detail/fog throughout.
pub fn render_layer(meshes: &[Mesh], cam: &Camera3, sun: Vec3, opts: &DepthOpts, z: i32) -> Layer {
    render_layer_with_lights(meshes, cam, sun, opts, &[], z)
}

/// Render a mesh layer with localized world lights. The original
/// [`render_layer`] remains the zero-light convenience path used by fixtures
/// and examples; the playable scene supplies torches through this function so
/// only nearby geometry glyphs receive their warm contribution.
pub fn render_layer_with_lights(
    meshes: &[Mesh],
    cam: &Camera3,
    sun: Vec3,
    opts: &DepthOpts,
    lights: &[PointLight],
    z: i32,
) -> Layer {
    let sun = sun.norm();
    let w2 = cam.width.max(1) as usize * SAMPLE_GRID;
    let h2 = cam.height.max(1) as usize * SAMPLE_GRID;
    let mut buf = vec![Sample::empty(); w2 * h2];

    for mesh in meshes {
        for tri in &mesh.tris {
            rasterize_tri(&mut buf, w2, h2, tri, cam, sun, opts, lights);
        }
    }

    resolve_layer(&buf, cam, opts, z)
}

/// Resolve + rim for a full supersampled buffer. Shared by the CPU fill
/// ([`render_layer`]) and the GPU raster path (`present::gpu3d`), so both
/// produce the same glyph language by construction.
pub(crate) fn resolve_layer(buf: &[Sample], cam: &Camera3, opts: &DepthOpts, z: i32) -> Layer {
    let w2 = cam.width.max(1) as usize * SAMPLE_GRID;
    let mut layer = Layer::new("mesh3d", z);
    for cy in 0..cam.height.max(1) {
        for cx in 0..cam.width.max(1) {
            if let Some(cell) = resolve_cell(buf, w2, cx, cy, opts) {
                layer.push(AnimCell::still(cx, cy, cell));
            }
        }
    }
    apply_rim(&mut layer, cam.width.max(1), cam.height.max(1));
    layer
}

/// Rim light: cells on the coverage silhouette (a void 4-neighbor) get a
/// brighter foreground, so actors read against ground of similar tone.
/// This is what separates the legs from the slab in the viewer: same glyph
/// family, ~1.45x the light. Interior cells are untouched.
pub(crate) fn apply_rim(layer: &mut Layer, w: i32, h: i32) {
    // Boolean occupancy grid: the naive any-scan per neighbor is O(n²)
    // over ~2k cells every frame, so index into a flat grid instead.
    // Crate-visible so the GPU raster path (`present::gpu3d`) shares the
    // same rim pass.
    let wu = w.max(1) as usize;
    let hu = h.max(1) as usize;
    let mut covered = vec![false; wu * hu];
    for c in &layer.cells {
        if c.x >= 0 && c.y >= 0 && c.x < w && c.y < h {
            covered[c.y as usize * wu + c.x as usize] = true;
        }
    }
    let is_covered = |x: i32, y: i32| {
        x >= 0 && y >= 0 && x < w && y < h && covered[y as usize * wu + x as usize]
    };
    // Direct mutable iteration: O(n) instead of O(n²) linear search per
    // silhouette cell. The `covered` grid gives O(1) neighbor checks.
    for c in layer.cells.iter_mut() {
        let (x, y) = (c.x, c.y);
        if x < 0 || y < 0 || x >= w || y >= h {
            continue;
        }
        if !is_covered(x - 1, y)
            || !is_covered(x + 1, y)
            || !is_covered(x, y - 1)
            || !is_covered(x, y + 1)
        {
            c.cell.fg = c.cell.fg.scaled(1.45);
        }
    }
}

/// Camera basis: forward/right/up for the yaw/pitch pose.
pub(crate) fn basis(cam: &Camera3) -> (Vec3, Vec3, Vec3) {
    let (sy, cy) = cam.yaw.sin_cos();
    let (sp, cp) = cam.pitch.sin_cos();
    let fwd = Vec3::new(sy * cp, sp, cy * cp);
    let right = Vec3::new(cy, 0.0, -sy);
    // Note the order: right.cross(fwd) points DOWN (−y); the camera up is
    // fwd.cross(right) (verified: yaw = pitch = 0 must yield +y).
    let up = fwd.cross(right).norm();
    (fwd, right, up)
}

/// Rasterize one face: project, flat-shade from its geometric normal,
/// barycentric-fill with perspective-correct depth, near-wins per sample.
#[allow(clippy::too_many_arguments)]
fn rasterize_tri(
    buf: &mut [Sample],
    w2: usize,
    h2: usize,
    tri: &Tri,
    cam: &Camera3,
    sun: Vec3,
    opts: &DepthOpts,
    lights: &[PointLight],
) {
    let (fwd, right, up) = basis(cam);
    // View space first so near-plane straddlers clip instead of dropping:
    // a ground slab with one corner past the lens must still draw its
    // visible half (the old code dropped the whole face).
    let view = tri.v.map(|p| {
        let d = p - cam.pos;
        Vec3::new(d.dot(right), d.dot(up), d.dot(fwd))
    });

    let n = (tri.v[1] - tri.v[0]).cross(tri.v[2] - tri.v[0]).norm();
    let (sr, sg, sb, lum) = shade_face_with_lights(tri, sun, opts, lights);
    let view_normal = Vec3::new(n.dot(right), n.dot(up), n.dot(fwd)).norm();

    let cx = cam.width as f32 / 2.0;
    let cy = cam.height as f32 / 2.0;
    for clipped in clip_near(view, opts.near) {
        let mut sx = [0.0f32; 3];
        let mut syv = [0.0f32; 3];
        let mut inv_z = [0.0f32; 3];
        for (i, v) in clipped.iter().enumerate() {
            // Supersample space: one cell covers SAMPLE_GRID×SAMPLE_GRID
            // samples. Keeping projection in the same coordinate system as
            // the depth buffer preserves native terminal resolution.
            sx[i] = (cx + cam.focal * v.x / v.z) * SAMPLE_GRID as f32;
            syv[i] = (cy - cam.focal * v.y / v.z) * SAMPLE_GRID as f32;
            inv_z[i] = 1.0 / v.z;
        }
        fill_tri(
            buf,
            w2,
            h2,
            &sx,
            &syv,
            &inv_z,
            sr,
            sg,
            sb,
            lum,
            view_normal,
            MaterialKind::classify(tri.albedo),
        );
    }
}

/// Sutherland–Hodgman clip of one triangle against the near plane
/// (`vz >= near` survives). Yields 0, 1, or 2 triangles; view-space
/// positions interpolate linearly, which is exact for planes.
pub(crate) fn clip_near(tri: [Vec3; 3], near: f32) -> Vec<[Vec3; 3]> {
    let cross = |a: Vec3, b: Vec3| {
        // Intersection of segment a→b with the near plane. Called only for
        // straddling edges, so the denominator is never zero.
        let t = (near - a.z) / (b.z - a.z);
        Vec3::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t, near)
    };
    let mut poly = Vec::with_capacity(4);
    let mut prev = tri[2];
    let mut prev_in = prev.z >= near;
    for cur in tri {
        let cur_in = cur.z >= near;
        if cur_in {
            if !prev_in {
                poly.push(cross(prev, cur));
            }
            poly.push(cur);
        } else if prev_in {
            poly.push(cross(prev, cur));
        }
        prev = cur;
        prev_in = cur_in;
    }
    match poly.len() {
        3 => vec![[poly[0], poly[1], poly[2]]],
        4 => vec![[poly[0], poly[1], poly[2]], [poly[0], poly[2], poly[3]]],
        _ => Vec::new(),
    }
}

/// Flat diffuse shading of one face: shared by the CPU fill
/// ([`rasterize_tri`]) and the GPU raster path (`present::gpu3d`, which
/// pre-shades on the CPU so the shader is a pure projection + depth test).
/// Two-sided lighting (|dot|): builders don't agree on winding, and closed
/// solids viewed from outside show outward faces either way, so the
/// absolute value lights them exactly as an outward normal would.
pub(crate) fn shade_face(tri: &Tri, sun: Vec3, ambient: f32) -> (f32, f32, f32, f32) {
    let n = (tri.v[1] - tri.v[0]).cross(tri.v[2] - tri.v[0]).norm();
    let diff = ambient + (1.0 - ambient) * n.dot(sun).abs().min(1.0);
    let sr = f32::from(tri.albedo.r) / 255.0 * diff;
    let sg = f32::from(tri.albedo.g) / 255.0 * diff;
    let sb = f32::from(tri.albedo.b) / 255.0 * diff;
    let lum = 0.2126 * sr + 0.7152 * sg + 0.0722 * sb;
    (sr, sg, sb, lum)
}

/// Flat sun shading plus coarse face-centre point-light response. The GPU
/// pre-pass calls this same helper before uploading triangles, keeping dynamic
/// local lighting identical between the CPU fallback and the default GPU fill.
pub(crate) fn shade_face_with_lights(
    tri: &Tri,
    sun: Vec3,
    opts: &DepthOpts,
    lights: &[PointLight],
) -> (f32, f32, f32, f32) {
    let (mut sr, mut sg, mut sb, _) = shade_face(tri, sun, opts.ambient);
    // Character-cell rasterization intentionally uses the face centre instead
    // of per-sample point-light interpolation. This keeps local sources stable
    // instead of producing shimmer while still brightening actual surfaces.
    let center = Vec3::new(
        (tri.v[0].x + tri.v[1].x + tri.v[2].x) / 3.0,
        (tri.v[0].y + tri.v[1].y + tri.v[2].y) / 3.0,
        (tri.v[0].z + tri.v[1].z + tri.v[2].z) / 3.0,
    );
    let response = match MaterialKind::classify(tri.albedo) {
        MaterialKind::Metal => 0.82,
        MaterialKind::Water => 0.52,
        MaterialKind::Foliage | MaterialKind::Earth => 0.72,
        MaterialKind::Fire | MaterialKind::Magic => 0.95,
        _ => 0.64,
    };
    for light in lights {
        let radius = light.radius.max(0.001);
        let distance = (center - light.position)
            .dot(center - light.position)
            .sqrt();
        let falloff = (1.0 - distance / radius).max(0.0);
        let contribution = falloff * falloff * light.intensity.max(0.0) * response;
        if contribution <= 0.0 {
            continue;
        }
        sr = (sr + f32::from(light.color.r) / 255.0 * contribution).min(1.0);
        sg = (sg + f32::from(light.color.g) / 255.0 * contribution).min(1.0);
        sb = (sb + f32::from(light.color.b) / 255.0 * contribution).min(1.0);
    }
    let lum = 0.2126 * sr + 0.7152 * sg + 0.0722 * sb;
    (sr, sg, sb, lum)
}

/// Barycentric fill of one projected triangle with perspective-correct
/// depth; near depth wins per sample.
#[allow(clippy::too_many_arguments)]
fn fill_tri(
    buf: &mut [Sample],
    w2: usize,
    h2: usize,
    sx: &[f32; 3],
    syv: &[f32; 3],
    inv_z: &[f32; 3],
    sr: f32,
    sg: f32,
    sb: f32,
    lum: f32,
    normal: Vec3,
    material: MaterialKind,
) {
    // Edge functions over the sample-space bbox, clipped to the buffer.
    let min_x = sx
        .iter()
        .cloned()
        .fold(f32::INFINITY, f32::min)
        .floor()
        .max(0.0) as i32;
    let max_x = sx
        .iter()
        .cloned()
        .fold(f32::NEG_INFINITY, f32::max)
        .ceil()
        .min(w2 as f32) as i32;
    let min_y = syv
        .iter()
        .cloned()
        .fold(f32::INFINITY, f32::min)
        .floor()
        .max(0.0) as i32;
    let max_y = syv
        .iter()
        .cloned()
        .fold(f32::NEG_INFINITY, f32::max)
        .ceil()
        .min(h2 as f32) as i32;
    let edge = |ax: f32, ay: f32, bx: f32, by: f32, px: f32, py: f32| {
        (bx - ax) * (py - ay) - (by - ay) * (px - ax)
    };
    let area = edge(sx[0], syv[0], sx[1], syv[1], sx[2], syv[2]);
    if area.abs() < 1e-9 {
        return;
    }
    for py in min_y..max_y {
        for px in min_x..max_x {
            // Sample centers (half-open top-left fill rule keeps shared
            // edges watertight between adjacent faces).
            let fx = px as f32 + 0.5;
            let fy = py as f32 + 0.5;
            let b0 = edge(sx[1], syv[1], sx[2], syv[2], fx, fy) / area;
            let b1 = edge(sx[2], syv[2], sx[0], syv[0], fx, fy) / area;
            let b2 = edge(sx[0], syv[0], sx[1], syv[1], fx, fy) / area;
            if b0 < 0.0 || b1 < 0.0 || b2 < 0.0 {
                continue;
            }
            // Perspective-correct depth via 1/z, then near-wins.
            let w = b0 * inv_z[0] + b1 * inv_z[1] + b2 * inv_z[2];
            if w <= 0.0 {
                continue;
            }
            let s = &mut buf[py as usize * w2 + px as usize];
            let z = 1.0 / w;
            if z < s.depth {
                s.depth = z;
                s.r = sr;
                s.g = sg;
                s.b = sb;
                s.lum = lum;
                s.lum_sq = lum * lum;
                s.normal_x = normal.x;
                s.normal_y = normal.y;
                s.normal_z = normal.z;
                s.material = material;
            }
        }
    }
}

/// Resolve one cell's 4x4 samples: material glyph by mean luminance and
/// stable local variation, a depth edge glyph where the block spans a step,
/// and — for partially covered blocks — positional sub-cell glyphs placed
/// where the coverage actually sits; distance-aware detail and fog
/// throughout. `None` when no sample hit (void shows through).
pub(crate) fn resolve_cell(
    buf: &[Sample],
    w2: usize,
    cx: i32,
    cy: i32,
    opts: &DepthOpts,
) -> Option<Cell> {
    let mut hits = 0u32;
    let mut lum = 0.0f32;
    let mut r = 0.0f32;
    let mut g = 0.0f32;
    let mut b = 0.0f32;
    let mut lum_sq = 0.0f32;
    let mut max_lum = 0.0f32;
    let mut max_r = 0.0f32;
    let mut max_g = 0.0f32;
    let mut max_b = 0.0f32;
    let mut normal_x = 0.0f32;
    let mut normal_y = 0.0f32;
    let mut normal_z = 0.0f32;
    // Sub-cell detail accumulators: which samples hit (mask), where the ink
    // sits (position-weighted luma), and per-channel second moments for the
    // facet-normal / colour spread the detail channel reads.
    let mut mask: u16 = 0;
    let mut lum_x = 0.0f32;
    let mut lum_y = 0.0f32;
    let mut nx_sq = 0.0f32;
    let mut ny_sq = 0.0f32;
    let mut nz_sq = 0.0f32;
    let mut r_sq = 0.0f32;
    let mut g_sq = 0.0f32;
    let mut b_sq = 0.0f32;
    let mut zmin = f32::INFINITY;
    let mut zmax = f32::NEG_INFINITY;
    let mut material_votes = [0u32; 9];
    // Corner depths for the edge orientation (fall back to the block mean
    // where a corner missed — silhouettes keep their shaded side).
    let mut corner = [f32::NAN; 4];
    for dy in 0..SAMPLE_GRID {
        for dx in 0..SAMPLE_GRID {
            let s = buf[(cy as usize * SAMPLE_GRID + dy) * w2 + cx as usize * SAMPLE_GRID + dx];
            if s.depth.is_finite() {
                hits += 1;
                lum += s.lum;
                r += s.r;
                g += s.g;
                b += s.b;
                lum_sq += s.lum_sq;
                mask |= 1 << (dy * SAMPLE_GRID + dx);
                lum_x += s.lum * dx as f32;
                lum_y += s.lum * dy as f32;
                if s.lum > max_lum {
                    max_lum = s.lum;
                    max_r = s.r;
                    max_g = s.g;
                    max_b = s.b;
                }
                normal_x += s.normal_x;
                normal_y += s.normal_y;
                normal_z += s.normal_z;
                nx_sq += s.normal_x * s.normal_x;
                ny_sq += s.normal_y * s.normal_y;
                nz_sq += s.normal_z * s.normal_z;
                r_sq += s.r * s.r;
                g_sq += s.g * s.g;
                b_sq += s.b * s.b;
                zmin = zmin.min(s.depth);
                zmax = zmax.max(s.depth);
                material_votes[s.material.index()] += 1;
                if dx == 0 && dy == 0 {
                    corner[0] = s.depth;
                } else if dx == SAMPLE_GRID - 1 && dy == 0 {
                    corner[1] = s.depth;
                } else if dx == 0 && dy == SAMPLE_GRID - 1 {
                    corner[2] = s.depth;
                } else if dx == SAMPLE_GRID - 1 && dy == SAMPLE_GRID - 1 {
                    corner[3] = s.depth;
                }
            }
        }
    }
    if hits == 0 {
        return None;
    }
    let n = hits as f32;
    let mean_lum = lum / n;
    let variation = (lum_sq / n - mean_lum * mean_lum).max(0.0).sqrt();
    let view_normal = Vec3::new(normal_x / n, normal_y / n, normal_z / n).norm();
    let coverage = hits as f32 / (SAMPLE_GRID * SAMPLE_GRID) as f32;
    let var = |sum: f32, sq: f32| (sq / n - (sum / n) * (sum / n)).max(0.0);
    // Luma-weighted ink centre (falls back to the hits' geometric centre
    // when the whole block is unlit, so black silhouettes still position).
    let (mut centroid_x, mut centroid_y) = if lum > 1e-6 {
        ((lum_x / lum - 1.5) / 1.5, (lum_y / lum - 1.5) / 1.5)
    } else {
        let (mut hx, mut hy) = (0.0f32, 0.0f32);
        for dy in 0..SAMPLE_GRID {
            for dx in 0..SAMPLE_GRID {
                if mask & (1 << (dy * SAMPLE_GRID + dx)) != 0 {
                    hx += dx as f32;
                    hy += dy as f32;
                }
            }
        }
        ((hx / n - 1.5) / 1.5, (hy / n - 1.5) / 1.5)
    };
    centroid_x = centroid_x.clamp(-1.0, 1.0);
    centroid_y = centroid_y.clamp(-1.0, 1.0);
    let feat = CellDetail {
        coverage,
        variation,
        mask,
        centroid: (centroid_x, centroid_y),
        normal_var: var(normal_x, nx_sq) + var(normal_y, ny_sq) + var(normal_z, nz_sq),
        chroma_var: var(r, r_sq) + var(g, g_sq) + var(b, b_sq),
    };
    let material = material_votes
        .iter()
        .enumerate()
        .max_by_key(|(_, votes)| **votes)
        .map(|(index, _)| MaterialKind::from_index(index))
        .unwrap_or(MaterialKind::Generic);
    let zavg = (zmin + zmax) / 2.0;
    // Distance fog: far geometry sinks toward the void.
    let fog = ((zavg - opts.near) / (opts.far - opts.near)).clamp(0.0, 1.0);
    // Keep distant ground readable without flattening the depth cue. The old
    // 75% reduction turned most of the world into near-black punctuation at
    // the default orbit distance, so foreground objects lost their context.
    let dim = 1.0 - 0.52 * fog;
    // Hotspot retention: the cell mean washes a small bright feature (a
    // glint, an ember, a torch-lit edge) into its dark surroundings. Bias
    // the displayed energy toward the brightest sample so minor details
    // keep ink; the quiet dark ground (max ≈ mean) is untouched.
    const HOTSPOT: f32 = 0.22;
    let mean_r = r / n;
    let mean_g = g / n;
    let mean_b = b / n;
    let mix = |mean: f32, hot: f32| mean + (hot - mean) * HOTSPOT;
    let to_u8 = |c: f32| (c * dim * 255.0).clamp(0.0, 255.0) as u8;
    let fg = Rgb::new(
        to_u8(mix(mean_r, max_r)),
        to_u8(mix(mean_g, max_g)),
        to_u8(mix(mean_b, max_b)),
    );
    let edge = if zmax - zmin > opts.edge_step {
        let at = |i: usize| {
            if corner[i].is_finite() {
                corner[i]
            } else {
                zavg
            }
        };
        let gx = (at(1) + at(3)) / 2.0 - (at(0) + at(2)) / 2.0;
        let gy = (at(2) + at(3)) / 2.0 - (at(0) + at(1)) / 2.0;
        if gx.abs() > gy.abs() * 1.6 {
            '|'
        } else if gy.abs() > gx.abs() * 1.6 {
            '-'
        } else if gx * gy > 0.0 {
            '\\'
        } else {
            '/'
        }
    } else {
        '\0'
    };
    let shown_lum = (mean_lum + (max_lum - mean_lum).max(0.0) * HOTSPOT) * dim;
    let glyph = select_glyph(material, shown_lum, edge, fog, feat, view_normal, cx, cy);
    // Foreground-only world contract: a hit cell may carry a coloured glyph,
    // but neither it nor the empty space around it paints a terrain fill.
    Some(Cell::new(glyph, fg, WORLD_BLACK))
}

/// Sub-cell detail features extracted from one cell's supersample block.
/// Everything derives from the samples (never time), so glyph choices stay
/// world-stable while the camera moves.
#[derive(Clone, Copy)]
struct CellDetail {
    coverage: f32,
    variation: f32,
    /// One bit per supersample hit (row-major, `dy * SAMPLE_GRID + dx`).
    mask: u16,
    /// Luma-weighted ink centre as -1..1 offsets from the cell centre
    /// (negative y = toward the cell top).
    centroid: (f32, f32),
    /// Facet-normal spread inside the cell — surface relief (seams, bark,
    /// armour plating) that luminance alone cannot see.
    normal_var: f32,
    /// Per-channel colour spread — coloured detail with flat luminance
    /// (moss on stone, ember tint) lives here.
    chroma_var: f32,
}

/// Rows and columns the coverage mask spans (1..=SAMPLE_GRID each).
fn mask_spans(mask: u16) -> (usize, usize) {
    let row = (1u16 << SAMPLE_GRID) - 1;
    let mut rows = 0u16;
    let mut cols = 0u16;
    for dy in 0..SAMPLE_GRID {
        if mask >> (dy * SAMPLE_GRID) & row != 0 {
            rows |= 1 << dy;
        }
        for dx in 0..SAMPLE_GRID {
            if mask >> (dy * SAMPLE_GRID + dx) & 1 != 0 {
                cols |= 1 << dx;
            }
        }
    }
    (rows.count_ones() as usize, cols.count_ones() as usize)
}

/// Mean ink column of the mask's top (dy < 2) or bottom half, if it has
/// any hits — the two halves' columns decide a diagonal sliver's slant.
fn half_col_mean(mask: u16, top: bool) -> Option<f32> {
    let (mut sum, mut count) = (0.0f32, 0.0f32);
    for dy in 0..SAMPLE_GRID {
        if (dy < 2) != top {
            continue;
        }
        for dx in 0..SAMPLE_GRID {
            if mask & (1 << (dy * SAMPLE_GRID + dx)) != 0 {
                sum += dx as f32;
                count += 1.0;
            }
        }
    }
    (count > 0.0).then_some(sum / count)
}

/// Material-specific density and shape selection. The first-class edge
/// strokes are retained for structural geometry, while organic materials use
/// branching/cluster glyphs instead of a uniform global ramp. Coverage,
/// supersample variance (luminance, facets, colour), the ink's position
/// inside the cell, view orientation, and fog all participate so close
/// geometry gets fine texture while distant geometry simplifies
/// deterministically.
#[allow(clippy::too_many_arguments)]
fn select_glyph(
    material: MaterialKind,
    luminance: f32,
    edge: char,
    fog: f32,
    feat: CellDetail,
    view_normal: Vec3,
    x: i32,
    y: i32,
) -> char {
    let hash = cell_hash(x, y);
    // Front-facing and upward facets carry more authored detail; grazing
    // facets stay sparse so the silhouette remains legible instead of turning
    // into a wall of texture.
    let orientation =
        view_normal.z.abs() * 0.16 + view_normal.y.max(0.0) * 0.10 - view_normal.x.abs() * 0.04;
    let density = (luminance * 1.52 + orientation - fog * 0.08).clamp(0.0, 1.0);
    // Facet relief and colour spread widen the detail channel: a cell whose
    // samples disagree about the surface normal or hue is textured even when
    // its luminance is flat. Capped so the channel cannot pin at 1 and turn
    // the jitter into noise.
    let detail_level = (feat.variation * 3.0
        + feat.coverage * 0.42
        + (1.0 - fog) * 0.34
        + (feat.normal_var * 2.5).min(0.35)
        + (feat.chroma_var * 6.0).min(0.30))
    .clamp(0.0, 1.0);

    if edge != '\0' {
        return edge_glyph(material, edge, hash, view_normal);
    }

    // Sub-cell silhouette detail: when only part of the block is covered,
    // place ink where the coverage actually sits instead of averaging the
    // whole cell — limbs, blade tips, and silhouette edges gain sub-cell
    // position resolution. Fully sample-driven, so it is deterministic and
    // camera-stable like every other channel.
    if feat.coverage < 0.5 && fog < 0.6 && !matches!(material, MaterialKind::Water) {
        let (row_span, col_span) = mask_spans(feat.mask);
        if col_span == 1 && row_span >= 2 {
            return '|'; // tall sliver: trunk edges, limbs, pole arms
        }
        if row_span == 1 && col_span >= 2 {
            return if luminance > 0.45 { '=' } else { '_' }; // flat ledge
        }
        if feat.coverage < 0.35 && row_span >= 2 && col_span >= 2 {
            if let (Some(top), Some(bot)) = (
                half_col_mean(feat.mask, true),
                half_col_mean(feat.mask, false),
            ) {
                if (top - bot).abs() > 0.5 {
                    return if top > bot { '/' } else { '\\' };
                }
            }
        }
        let bright = luminance > 0.42;
        if feat.centroid.1 < -0.33 {
            return if bright { '"' } else { '\'' }; // ink sits high
        }
        if feat.centroid.1 > 0.33 {
            return if bright { ',' } else { '.' }; // ink sits low
        }
        if feat.coverage < 0.2 {
            return if bright { ':' } else { '.' }; // centred speck
        }
        // Centred, chunkier partial coverage: fall through to the family.
    }

    let family = material.glyphs();
    let mut glyph = family_glyph(family, density, detail_level, fog, hash);
    // A partially covered geometry cell should never disappear solely because
    // its density family begins with a space. The black void remains untouched;
    // this only gives a visible ink mark to actual silhouette coverage.
    if glyph == ' ' && feat.coverage >= 0.18 {
        glyph = family.get(1).copied().unwrap_or(b'.') as char;
    }

    // Near-facing water keeps its wave language even where coverage is
    // partial (the positional slivers above exclude water so shorelines and
    // crests keep reading as waves).
    if detail_level > 0.68
        && feat.coverage < 0.48
        && matches!(material, MaterialKind::Water)
        && view_normal.y > 0.45
    {
        glyph = if hash & 1 == 0 { '~' } else { '-' };
    }
    glyph
}

fn family_glyph(family: &[u8], density: f32, detail: f32, fog: f32, hash: u32) -> char {
    if family.is_empty() {
        return ' ';
    }
    let max = family.len() - 1;
    let base = (density * max as f32).round() as i32;
    // Close, varied faces can move one slot around their density band. The
    // offset is a spatial hash, never a time-varying random source, so the
    // world does not shimmer while the camera moves.
    let jitter_span = if detail > 0.44 && fog < 0.72 { 1 } else { 0 };
    let jitter = if jitter_span == 0 {
        0
    } else {
        ((hash.rotate_left(7) % 3) as i32) - 1
    };
    let index = (base + jitter).clamp(0, max as i32) as usize;
    family[index] as char
}

fn edge_glyph(material: MaterialKind, edge: char, hash: u32, normal: Vec3) -> char {
    let family: &[u8] = match edge {
        '|' => match material {
            MaterialKind::Bark | MaterialKind::Foliage => b"|!iIl1T",
            MaterialKind::Stone => b"|[]!I",
            MaterialKind::Metal => b"|!I1",
            _ => b"|!I",
        },
        '-' => match material {
            MaterialKind::Foliage => b"-_=+~",
            MaterialKind::Bark => b"-_=+",
            MaterialKind::Water => b"-_~=",
            MaterialKind::Stone | MaterialKind::Metal => b"-_+=",
            _ => b"-_+=",
        },
        '/' | '\\' => match material {
            MaterialKind::Foliage => b"/\\xX*",
            MaterialKind::Bark => b"/\\|T",
            MaterialKind::Stone => b"/\\xX",
            MaterialKind::Magic | MaterialKind::Fire => b"/\\*+",
            _ => b"/\\|x",
        },
        _ => material.glyphs(),
    };
    let mut glyph = family[(hash as usize) % family.len()] as char;
    if edge == '|' && normal.y.abs() > 0.7 && matches!(material, MaterialKind::Bark) {
        glyph = if hash & 1 == 0 { 'I' } else { '!' };
    }
    if edge == '-' && normal.z.abs() > 0.7 && matches!(material, MaterialKind::Water) {
        glyph = if hash & 1 == 0 { '~' } else { '=' };
    }
    glyph
}

fn cell_hash(x: i32, y: i32) -> u32 {
    let mut h = (x as u32).wrapping_mul(374_761_393) ^ (y as u32).wrapping_mul(668_265_263);
    h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
    h ^ (h >> 16)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Partially covered blocks must place their ink where the coverage
    /// sits: tall slivers become bars, thin diagonals get their slant, and
    /// top/bottom-heavy ink lands as positioned punctuation.
    #[test]
    fn sparse_coverage_places_ink_at_sub_cell_positions() {
        let normal = Vec3::new(0.0, 0.7, 0.7).norm();
        let feat = |coverage: f32, mask: u16, cy: f32| CellDetail {
            coverage,
            variation: 0.1,
            mask,
            centroid: (0.0, cy),
            normal_var: 0.0,
            chroma_var: 0.0,
        };
        // bit(dy, dx): the coverage-mask bit for one supersample.
        let bit = |dy: usize, dx: usize| 1u16 << (dy * SAMPLE_GRID + dx);
        // Tall sliver (column 1, all rows) → bar.
        let bar = (0..SAMPLE_GRID).fold(0u16, |m, dy| m | bit(dy, 1));
        assert_eq!(
            select_glyph(
                MaterialKind::Metal,
                0.3,
                '\0',
                0.1,
                feat(0.25, bar, 0.0),
                normal,
                3,
                5
            ),
            '|'
        );
        // Flat sliver (row 2, all columns) → ledge.
        let flat = (0..SAMPLE_GRID).fold(0u16, |m, dx| m | bit(2, dx));
        assert_eq!(
            select_glyph(
                MaterialKind::Stone,
                0.3,
                '\0',
                0.1,
                feat(0.25, flat, 0.0),
                normal,
                3,
                6
            ),
            '_'
        );
        // Thin diagonal: top half's ink right of the bottom half's → '/'.
        let diag = bit(0, 2) | bit(0, 3) | bit(3, 0) | bit(3, 1);
        assert_eq!(
            select_glyph(
                MaterialKind::Bark,
                0.3,
                '\0',
                0.1,
                feat(0.25, diag, 0.0),
                normal,
                3,
                7
            ),
            '/'
        );
        // Ink sitting high/low lands as positioned punctuation, not a family
        // average (masks hit rows 0-1 / rows 2-3 on the left columns so no
        // sliver shape applies).
        let high = bit(0, 0) | bit(1, 1);
        let low = bit(2, 0) | bit(3, 1);
        let high_glyph = select_glyph(
            MaterialKind::Earth,
            0.3,
            '\0',
            0.1,
            feat(0.125, high, -0.8),
            normal,
            3,
            8,
        );
        assert!(
            high_glyph == '\'' || high_glyph == '"',
            "high ink must read as a top mark, got '{high_glyph}'"
        );
        let low_glyph = select_glyph(
            MaterialKind::Earth,
            0.3,
            '\0',
            0.1,
            feat(0.125, low, 0.8),
            normal,
            3,
            9,
        );
        assert!(
            low_glyph == ',' || low_glyph == '.',
            "low ink must read as a bottom mark, got '{low_glyph}'"
        );
    }

    fn test_cam() -> Camera3 {
        Camera3::new(Vec3::new(0.0, 2.2, -9.0), 0.0, 0.0, 60.0, 96, 35)
    }

    /// One bright sample inside an otherwise dark block must keep visible
    /// ink: the fg color lifts toward the hot sample and the glyph never
    /// gets sparser than the all-dark resolve (hotspot retention).
    #[test]
    fn hotspot_sample_keeps_minor_detail_ink() {
        let cam = test_cam();
        let opts = DepthOpts::default();
        let w2 = cam.width as usize * SAMPLE_GRID;
        let mut buf = vec![Sample::empty(); w2 * cam.height as usize * SAMPLE_GRID];
        let dark = Sample {
            depth: 10.0,
            r: 0.02,
            g: 0.03,
            b: 0.02,
            lum: 0.027,
            lum_sq: 0.027 * 0.027,
            normal_x: 0.0,
            normal_y: 1.0,
            normal_z: 0.0,
            material: MaterialKind::Earth,
        };
        let hot = Sample {
            r: 0.95,
            g: 0.55,
            b: 0.15,
            lum: 0.606,
            lum_sq: 0.606 * 0.606,
            ..dark
        };
        let block =
            |buf: &mut Vec<Sample>, cx: usize, cy: usize, hot_at: Option<(usize, usize)>| {
                for dy in 0..SAMPLE_GRID {
                    for dx in 0..SAMPLE_GRID {
                        buf[(cy * SAMPLE_GRID + dy) * w2 + cx * SAMPLE_GRID + dx] =
                            if hot_at == Some((dx, dy)) { hot } else { dark };
                    }
                }
            };
        let (cx, cy) = (48, 16);
        block(&mut buf, cx, cy, Some((2, 2)));
        let hotspot = resolve_cell(&buf, w2, cx as i32, cy as i32, &opts).expect("hotspot cell");
        block(&mut buf, cx, cy, None);
        let plain = resolve_cell(&buf, w2, cx as i32, cy as i32, &opts).expect("plain cell");
        assert!(
            hotspot.fg.r > plain.fg.r && hotspot.fg.g > plain.fg.g,
            "hotspot must lift fg ink: {:?} vs {:?}",
            (hotspot.fg.r, hotspot.fg.g, hotspot.fg.b),
            (plain.fg.r, plain.fg.g, plain.fg.b)
        );
        let family = MaterialKind::Earth.glyphs();
        let density = |g: char| {
            family
                .iter()
                .position(|&c| c as char == g)
                .expect("glyph belongs to the material family")
        };
        assert!(
            density(hotspot.glyph) >= density(plain.glyph),
            "hotspot glyph must not be sparser: '{}' vs '{}'",
            hotspot.glyph,
            plain.glyph
        );
    }

    fn white() -> Rgb {
        Rgb::new(255, 255, 255)
    }

    #[test]
    fn glyph_families_cover_broad_printable_ascii() {
        let families = [
            GENERIC_GLYPHS,
            FOLIAGE_GLYPHS,
            BARK_GLYPHS,
            EARTH_GLYPHS,
            STONE_GLYPHS,
            WATER_GLYPHS,
            FIRE_GLYPHS,
            MAGIC_GLYPHS,
            METAL_GLYPHS,
        ];
        let mut seen = [false; 128];
        for family in families {
            for &glyph in family {
                assert!(
                    glyph.is_ascii_graphic() || glyph == b' ',
                    "non-printable glyph {glyph}"
                );
                seen[glyph as usize] = true;
            }
        }
        let unique = seen.into_iter().filter(|present| *present).count();
        assert!(
            unique >= 45,
            "glyph vocabulary collapsed to only {unique} symbols"
        );
    }

    #[test]
    fn glyph_selection_is_material_aware_and_stable() {
        let normal = Vec3::new(0.0, 0.7, 0.7).norm();
        let materials = [
            MaterialKind::Foliage,
            MaterialKind::Bark,
            MaterialKind::Stone,
            MaterialKind::Water,
            MaterialKind::Fire,
            MaterialKind::Magic,
            MaterialKind::Metal,
        ];
        let mut outputs = [false; 128];
        let feat = CellDetail {
            coverage: 0.82,
            variation: 0.18,
            mask: 0xFFFF,
            centroid: (0.0, 0.0),
            normal_var: 0.0,
            chroma_var: 0.0,
        };
        for (index, material) in materials.into_iter().enumerate() {
            for band in 0..=8 {
                let glyph = select_glyph(
                    material,
                    band as f32 / 8.0,
                    '\0',
                    0.12,
                    feat,
                    normal,
                    (index * 11) as i32,
                    band,
                );
                assert!(glyph.is_ascii(), "selection must stay ASCII");
                outputs[glyph as usize] = true;
                assert_eq!(
                    glyph,
                    select_glyph(
                        material,
                        band as f32 / 8.0,
                        '\0',
                        0.12,
                        feat,
                        normal,
                        (index * 11) as i32,
                        band,
                    ),
                    "selection must be deterministic"
                );
            }
        }
        let unique = outputs.into_iter().filter(|present| *present).count();
        assert!(
            unique >= 20,
            "material families produced only {unique} glyphs"
        );
    }

    #[test]
    fn warm_materials_keep_earth_distinct_from_dark_bark() {
        assert_eq!(
            MaterialKind::classify(Rgb::new(132, 104, 66)),
            MaterialKind::Earth,
            "bright sand should use earth glyph language"
        );
        assert_eq!(
            MaterialKind::classify(Rgb::new(92, 58, 38)),
            MaterialKind::Bark,
            "dark red-brown timber should keep bark glyph language"
        );
    }

    #[test]
    fn near_geometry_occludes_far() {
        // Small white cube in front of a big red one, same line of sight:
        // the center cell must read near-white, not red.
        let near = Mesh::cube(1.5, white());
        let mut far = Mesh::cube(6.0, Rgb::new(220, 40, 40));
        for tri in &mut far.tris {
            for v in &mut tri.v {
                *v = Vec3::new(v.x, v.y + 2.2, v.z + 6.0);
            }
        }
        let mut shifted = Mesh::new();
        for tri in &near.tris {
            let mut t = tri.clone();
            for v in &mut t.v {
                *v = Vec3::new(v.x, v.y + 2.2, v.z);
            }
            shifted.tris.push(t);
        }
        let layer = render_layer(
            &[far, shifted],
            &test_cam(),
            Vec3::new(-0.35, 0.55, -0.75),
            &DepthOpts::default(),
            2,
        );
        let center = layer
            .cells
            .iter()
            .find(|c| c.x == 48 && c.y == 17)
            .expect("center must be covered");
        // Front-lit white (fogged slightly) reads ~175 all channels; the
        // red wall behind would show g,b under 40.
        assert!(center.cell.fg.r > 150 && center.cell.fg.g > 150 && center.cell.fg.b > 150);
    }

    #[test]
    fn depth_step_emits_edge_glyphs() {
        // Small cube floating in front of a big back wall: the silhouette
        // boundary must contain unambiguous depth-edge strokes.
        let mut wall = Mesh::cube(10.0, Rgb::new(120, 120, 130));
        for tri in &mut wall.tris {
            for v in &mut tri.v {
                *v = Vec3::new(v.x, v.y + 2.0, v.z + 8.0);
            }
        }
        let mut knob = Mesh::cube(2.0, white());
        for tri in &mut knob.tris {
            for v in &mut tri.v {
                *v = Vec3::new(v.x, v.y + 2.0, v.z + 2.0);
            }
        }
        let layer = render_layer(
            &[wall, knob],
            &test_cam(),
            Vec3::new(-0.35, 0.55, -0.75),
            &DepthOpts::default(),
            2,
        );
        let strokes = layer
            .cells
            .iter()
            .filter(|c| matches!(c.cell.glyph, '|' | '-' | '/' | '\\'))
            .count();
        assert!(
            strokes >= 4,
            "expected edge strokes around the knob, got {strokes}"
        );
    }

    #[test]
    fn empty_scene_resolves_to_no_cells() {
        let layer = render_layer(
            &[],
            &test_cam(),
            Vec3::new(0.0, -1.0, 0.5),
            &DepthOpts::default(),
            2,
        );
        assert!(layer.cells.is_empty());
        // Behind the lens is void too.
        // Behind the lens is void too: the origin cube shifted past the
        // camera (z < -9) projects nothing.
        let mut behind = Mesh::cube(1.0, white());
        for tri in &mut behind.tris {
            for v in &mut tri.v {
                *v = Vec3::new(v.x, v.y, v.z - 12.0);
            }
        }
        let layer = render_layer(
            &[behind],
            &test_cam(),
            Vec3::new(0.0, -1.0, 0.5),
            &DepthOpts::default(),
            2,
        );
        assert!(
            layer.cells.is_empty(),
            "cube behind the camera must not rasterize"
        );
    }

    #[test]
    fn resolved_geometry_keeps_the_world_background_black() {
        let mut mesh = Mesh::cube(2.0, Rgb::new(32, 112, 52));
        for tri in &mut mesh.tris {
            for v in &mut tri.v {
                *v = Vec3::new(v.x, v.y + 1.4, v.z + 2.0);
            }
        }
        let layer = render_layer(
            &[mesh],
            &test_cam(),
            Vec3::new(-0.35, 0.55, -0.75),
            &DepthOpts::default(),
            0,
        );
        assert!(!layer.cells.is_empty());
        assert!(layer
            .cells
            .iter()
            .all(|cell| cell.cell.bg == Rgb::new(0, 0, 0)));
    }

    #[test]
    fn point_light_changes_visible_ink_without_a_background_halo() {
        let mut mesh = Mesh::cube(2.0, Rgb::new(52, 78, 48));
        for tri in &mut mesh.tris {
            for v in &mut tri.v {
                *v = Vec3::new(v.x, v.y + 1.4, v.z + 2.0);
            }
        }
        let cam = test_cam();
        let sun = Vec3::new(-0.35, 0.55, -0.75);
        let opts = DepthOpts::default();
        let plain = render_layer(&[mesh.clone()], &cam, sun, &opts, 0);
        let lit = render_layer_with_lights(
            &[mesh],
            &cam,
            sun,
            &opts,
            &[PointLight::new(
                Vec3::new(0.0, 2.0, 2.0),
                Rgb::new(255, 130, 40),
                6.0,
                1.0,
            )],
            0,
        );
        let plain_center = plain
            .cells
            .iter()
            .find(|cell| cell.x == 48 && cell.y == 17)
            .expect("plain center");
        let lit_center = lit
            .cells
            .iter()
            .find(|cell| cell.x == 48 && cell.y == 17)
            .expect("lit center");
        assert_ne!(plain_center.cell.fg, lit_center.cell.fg);
        assert_eq!(lit_center.cell.bg, Rgb::new(0, 0, 0));
    }

    #[test]
    fn distant_geometry_fogs_dimmer() {
        // Same lit cube near vs far: the far center cell must sit lower on
        // the ramp (fog), proving depth cueing is monotonic.
        let render_center = |z: f32| {
            let mut m = Mesh::cube(2.0, white());
            for tri in &mut m.tris {
                for v in &mut tri.v {
                    *v = Vec3::new(v.x, v.y + 2.2, v.z + z);
                }
            }
            let layer = render_layer(
                &[m],
                &test_cam(),
                Vec3::new(-0.35, 0.55, -0.75),
                &DepthOpts::default(),
                2,
            );
            layer
                .cells
                .iter()
                .find(|c| c.x == 48 && c.y == 17)
                .map(|c| {
                    (
                        c.cell.glyph,
                        f32::from(c.cell.fg.r) * 0.2126
                            + f32::from(c.cell.fg.g) * 0.7152
                            + f32::from(c.cell.fg.b) * 0.0722,
                    )
                })
        };
        let (near_glyph, near_luma) = render_center(0.0).expect("near cube covers center");
        let (far_glyph, far_luma) = render_center(20.0).expect("far cube still covers center");
        assert!(
            far_luma < near_luma,
            "far '{far_glyph}' ({far_luma:.1}) must be dimmer than near '{near_glyph}' ({near_luma:.1})"
        );
    }

    #[test]
    fn renders_are_deterministic() {
        let meshes = vec![Mesh::humanoid(), Mesh::ground_slab(Rgb::new(45, 50, 60))];
        let cam = test_cam();
        let sun = Vec3::new(-0.35, 0.55, -0.75);
        let opts = DepthOpts::default();
        let signature = |layer: &Layer| {
            layer
                .cells
                .iter()
                .map(|c| (c.x, c.y, c.cell.glyph, c.cell.fg))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            signature(&render_layer(&meshes, &cam, sun, &opts, 2)),
            signature(&render_layer(&meshes, &cam, sun, &opts, 2))
        );
    }

    fn assert_close(a: Vec3, b: Vec3) {
        let d = a - b;
        assert!(d.dot(d) < 1e-10, "expected {b:?}, got {a:?}");
    }

    #[test]
    fn identity_transform_is_exact() {
        let mesh = Mesh::cube(2.0, white());
        let out = mesh.transformed(&Transform::identity());
        assert_eq!(out.tris.len(), mesh.tris.len());
        for (a, b) in out.tris.iter().zip(mesh.tris.iter()) {
            for (u, v) in a.v.iter().zip(b.v.iter()) {
                assert_eq!(*u, *v, "identity must be bit-exact");
            }
        }
    }

    #[test]
    fn yaw_quarter_turn_maps_axes() {
        let t = Transform {
            yaw: std::f32::consts::FRAC_PI_2,
            ..Transform::identity()
        };
        let mesh = Mesh::cube(2.0, white()).transformed(&t);
        // +x swings to −z, +z swings to +x (right-hand rule about +y).
        let mut xs = 0;
        let mut zs = 0;
        for tri in &mesh.tris {
            for v in &tri.v {
                if (v.x - 1.0).abs() < 1e-5 {
                    xs += 1;
                }
                if (v.z + 1.0).abs() < 1e-5 {
                    zs += 1;
                }
            }
        }
        assert!(xs > 0 && zs > 0, "quarter yaw must move ±x verts onto ∓z");
        assert_close(
            t_yaw_point(Vec3::new(0.0, 0.0, 1.0), std::f32::consts::FRAC_PI_2),
            Vec3::new(1.0, 0.0, 0.0),
        );
    }

    /// Single-point helper through the public path (avoids duplicating the
    /// rotation math in the test).
    fn t_yaw_point(p: Vec3, yaw: f32) -> Vec3 {
        let mut m = Mesh::new();
        m.tris.push(Tri {
            v: [p, p, p],
            albedo: white(),
        });
        let t = Transform {
            yaw,
            ..Transform::identity()
        };
        m.transformed(&t).tris[0].v[0]
    }

    #[test]
    fn pivot_rotation_swings_like_a_joint() {
        // Unit arm along +y pivoted at its base: 90° pitch must lay it
        // along +z, base unmoved — the joint primitive posing needs.
        let arm = Mesh::boxed(Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.2, 2.0, 0.2), white());
        let t = Transform {
            pitch: std::f32::consts::FRAC_PI_2,
            ..Transform::identity()
        };
        let out = arm.transformed_about(Vec3::new(0.0, 0.0, 0.0), &t);
        // The arm now lies flat: length along +z, thin in y, base at z≈0.
        let mut zmax = f32::NEG_INFINITY;
        let mut zmin = f32::INFINITY;
        for tri in &out.tris {
            for v in &tri.v {
                assert!(v.y.abs() <= 0.11, "arm must lie flat, got {v:?}");
                zmax = zmax.max(v.z);
                zmin = zmin.min(v.z);
            }
        }
        assert!((zmax - 2.0).abs() < 1e-4, "tip must reach z=2, got {zmax}");
        assert!(zmin.abs() < 1e-4, "base must stay at z=0, got {zmin}");
    }

    #[test]
    fn humanoid_keeps_all_ten_parts() {
        // 2 legs + torso + 2 pauldrons + 2 arms + head + helm + blade.
        assert_eq!(Mesh::humanoid().tris.len(), 10 * 12);
    }

    #[test]
    fn posed_humanoid_articulates_without_losing_ground_contact() {
        let idle = Mesh::humanoid_pose(HumanoidPose::new(0.0, 0.0, 0.0));
        let stride = Mesh::humanoid_pose(HumanoidPose::new(std::f32::consts::FRAC_PI_2, 0.0, 0.0));
        assert_eq!(idle.tris.len(), stride.tris.len());
        assert!(
            idle.tris
                .iter()
                .zip(stride.tris.iter())
                .flat_map(|(a, b)| a.v.iter().zip(b.v.iter()))
                .any(|(a, b)| (*a - *b).dot(*a - *b) > 0.0001),
            "a stride pose must change articulated vertices"
        );
        let min_y = stride
            .tris
            .iter()
            .flat_map(|tri| tri.v.iter())
            .map(|v| v.y)
            .fold(f32::INFINITY, f32::min);
        assert!(
            min_y >= -1e-5,
            "pose must not sink feet below ground: {min_y}"
        );
    }

    #[test]
    fn posed_humanoid_attack_moves_the_weapon_smoothly() {
        let idle = Mesh::humanoid_pose(HumanoidPose::default());
        let strike = Mesh::humanoid_pose(HumanoidPose::new(0.0, 0.5, 0.0));
        assert_eq!(idle.tris.len(), strike.tris.len());
        assert!(
            idle.tris
                .iter()
                .zip(strike.tris.iter())
                .flat_map(|(a, b)| a.v.iter().zip(b.v.iter()))
                .any(|(a, b)| (a.z - b.z).abs() > 0.1),
            "attack phase must move the arm/weapon chain"
        );
    }

    #[test]
    fn windowed_actor_recipes_have_distinct_grounded_silhouettes() {
        let humanoid = Mesh::humanoid_pose(HumanoidPose::default());
        let mossling = Mesh::mossling_pose(HumanoidPose::default());
        let skeleton = Mesh::skeleton_pose(HumanoidPose::default());
        let herbalist = Mesh::herbalist_pose(0.0);
        let signature = |mesh: &Mesh| {
            let mut min = Vec3::new(f32::INFINITY, f32::INFINITY, f32::INFINITY);
            let mut max = Vec3::new(f32::NEG_INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);
            for v in mesh.tris.iter().flat_map(|tri| tri.v) {
                min.x = min.x.min(v.x);
                min.y = min.y.min(v.y);
                min.z = min.z.min(v.z);
                max.x = max.x.max(v.x);
                max.y = max.y.max(v.y);
                max.z = max.z.max(v.z);
            }
            (
                mesh.tris.len(),
                (max.x - min.x).round(),
                (max.y - min.y).round(),
            )
        };
        assert_ne!(signature(&humanoid), signature(&mossling));
        assert_ne!(signature(&mossling), signature(&skeleton));
        assert_ne!(signature(&skeleton), signature(&herbalist));
        for mesh in [&mossling, &skeleton, &herbalist] {
            let min_y = mesh
                .tris
                .iter()
                .flat_map(|tri| tri.v.iter())
                .map(|v| v.y)
                .fold(f32::INFINITY, f32::min);
            assert!(
                min_y >= -1e-5,
                "actor recipe must stay above ground: {min_y}"
            );
        }
    }

    #[test]
    fn quad_y_and_decal_are_two_flat_tris() {
        // Ground sheets and FX decals pay exactly two triangles — the whole
        // point of the builders (a box per dash is 6x, with dead side faces).
        assert_eq!(
            Mesh::quad_y(Vec3::new(0.0, 0.0, 0.0), 2.0, 3.0, white())
                .tris
                .len(),
            2
        );
        assert_eq!(
            Mesh::decal_y(Vec3::new(0.0, 0.0, 0.0), 0.5, 1.0, 0.2, white())
                .tris
                .len(),
            2
        );
    }

    #[test]
    fn animated_blade_keeps_its_base_grounded_while_the_tip_leans() {
        let still = Mesh::blade_y(
            Vec3::new(2.0, 0.02, 3.0),
            0.7,
            0.08,
            Vec3::new(0.0, 0.0, 0.0),
            0.2,
            white(),
        );
        let bent = Mesh::blade_y(
            Vec3::new(2.0, 0.02, 3.0),
            0.7,
            0.08,
            Vec3::new(0.22, 0.0, -0.08),
            0.2,
            white(),
        );
        let min_y = |mesh: &Mesh| {
            mesh.tris
                .iter()
                .flat_map(|tri| tri.v.iter())
                .map(|v| v.y)
                .fold(f32::INFINITY, f32::min)
        };
        assert!((min_y(&still) - 0.02).abs() < 1e-5);
        assert!((min_y(&bent) - 0.02).abs() < 1e-5);
        assert!(
            still
                .tris
                .iter()
                .zip(bent.tris.iter())
                .flat_map(|(a, b)| a.v.iter().zip(b.v.iter()))
                .any(|(a, b)| (a.x - b.x).abs() > 0.1 || (a.z - b.z).abs() > 0.05),
            "weather sway must move the blade tip without moving its base"
        );
    }

    #[test]
    fn rain_streak_has_two_crossed_ribbons_and_preserves_endpoints() {
        let mesh = Mesh::streak(
            Vec3::new(1.0, 4.0, 2.0),
            Vec3::new(1.2, 3.1, 2.1),
            0.025,
            white(),
        );
        assert_eq!(mesh.tris.len(), 4);
        assert!(mesh
            .tris
            .iter()
            .flat_map(|tri| tri.v)
            .any(|v| { (v.y - 4.0).abs() < 0.03 && (v.x - 1.0).abs() < 0.04 }));
        assert!(mesh
            .tris
            .iter()
            .flat_map(|tri| tri.v)
            .any(|v| { (v.y - 3.1).abs() < 0.03 && (v.x - 1.2).abs() < 0.04 }));
    }

    #[test]
    fn cone_has_deterministic_facets_and_grounded_base() {
        let cone = Mesh::cone(Vec3::new(1.0, 0.2, 2.0), 1.5, 3.0, 8, white());
        assert_eq!(cone.tris.len(), 8);
        let min_y = cone
            .tris
            .iter()
            .flat_map(|tri| tri.v.iter())
            .map(|v| v.y)
            .fold(f32::INFINITY, f32::min);
        let max_y = cone
            .tris
            .iter()
            .flat_map(|tri| tri.v.iter())
            .map(|v| v.y)
            .fold(f32::NEG_INFINITY, f32::max);
        assert_eq!(min_y, 0.2);
        assert_eq!(max_y, 3.2);
    }

    #[test]
    fn decal_long_axis_follows_yaw() {
        // Long axis = (sin yaw, cos yaw): at yaw 0 a decal spans ±half_long
        // along +z, at yaw π/2 along +x. Central cell isn't sampled (fills
        // are sample-quantized), so scan every tri vertex jointly.
        let long = |mesh: &Mesh| {
            let mut span = 0.0f32;
            for tri in &mesh.tris {
                for v in tri.v {
                    span = span.max(v.x.abs()).max(v.z.abs());
                }
            }
            span
        };
        let y_spin = long(&Mesh::decal_y(
            Vec3::new(0.0, 0.0, 0.0),
            0.0,
            2.0,
            0.2,
            white(),
        ));
        let x_spin = long(&Mesh::decal_y(
            Vec3::new(0.0, 0.0, 0.0),
            std::f32::consts::FRAC_PI_2,
            2.0,
            0.2,
            white(),
        ));
        // Both spans include the wide edge's ±0.2 bite; long dominates.
        assert!(y_spin > 1.9, "yaw 0 must stretch along z, got {y_spin}");
        assert!(x_spin > 1.9, "yaw π/2 must stretch along x, got {x_spin}");
        // Corner check: yaw 0's extremal vertex sits on +z (not ±x).
        let far_z = Mesh::decal_y(Vec3::new(0.0, 0.0, 0.0), 0.0, 2.0, 0.2, white())
            .tris
            .iter()
            .flat_map(|t| t.v)
            .map(|v| v.z)
            .fold(0.0f32, f32::max);
        assert!((far_z - 2.0).abs() < 1e-4, "long tip at z=2, got {far_z}");
    }

    #[test]
    fn retinted_swaps_only_matching_faces() {
        let a = Rgb::new(10, 20, 30);
        let b = Rgb::new(40, 50, 60);
        let to = Rgb::new(200, 100, 50);
        let mut mesh = Mesh::new();
        mesh.push_quad(
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(1.0, 1.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            a,
        );
        mesh.push_quad(
            Vec3::new(2.0, 0.0, 0.0),
            Vec3::new(3.0, 0.0, 0.0),
            Vec3::new(3.0, 1.0, 0.0),
            Vec3::new(2.0, 1.0, 0.0),
            b,
        );
        let tinted = mesh.retinted(a, to).retinted(Rgb::new(1, 2, 3), to);
        let hits: Vec<Rgb> = tinted.tris.iter().map(|t| t.albedo).collect();
        assert_eq!(hits[0], to, "matching face repaints");
        assert_eq!(hits[3], b, "other material keeps its albedo");
        assert!(hits.iter().all(|c| *c == to || *c == b), "no stray swaps");
    }

    #[test]
    fn figure_reads_brighter_than_ground() {
        // The viewer clarity contract: a lit figure must average well above
        // the dark slab, so actors separate from the ground in the window.
        let cam = test_cam();
        let sun = Vec3::new(-0.35, 0.55, -0.75);
        let opts = DepthOpts::default();
        let mean_luma = |layer: &Layer| {
            let mut sum = 0.0f32;
            let mut n = 0u32;
            for c in &layer.cells {
                sum += f32::from(c.cell.fg.r) * 0.2126
                    + f32::from(c.cell.fg.g) * 0.7152
                    + f32::from(c.cell.fg.b) * 0.0722;
                n += 1;
            }
            sum / n.max(1) as f32
        };
        let figure = render_layer(&[Mesh::humanoid()], &cam, sun, &opts, 2);
        let ground = render_layer(
            &[Mesh::ground_slab(Rgb::new(24, 27, 38))],
            &cam,
            sun,
            &opts,
            2,
        );
        let (f, g) = (mean_luma(&figure), mean_luma(&ground));
        assert!(f > g * 2.0, "figure {f:.1} must clear ground {g:.1} by 2x");
    }

    #[test]
    fn rim_lightens_silhouette_over_interior() {
        // Centered cube: silhouette cells must read brighter than the
        // interior average — the actor/ground separation mechanism.
        let mut m = Mesh::cube(3.0, Rgb::new(120, 100, 80));
        for tri in &mut m.tris {
            for v in &mut tri.v {
                *v = Vec3::new(v.x, v.y + 2.2, v.z);
            }
        }
        let layer = render_layer(
            &[m],
            &test_cam(),
            Vec3::new(-0.35, 0.55, -0.75),
            &DepthOpts::default(),
            2,
        );
        assert!(!layer.cells.is_empty());
        let covered = |x: i32, y: i32| layer.cells.iter().any(|c| c.x == x && c.y == y);
        let luma = |c: &AnimCell| {
            f32::from(c.cell.fg.r) * 0.2126
                + f32::from(c.cell.fg.g) * 0.7152
                + f32::from(c.cell.fg.b) * 0.0722
        };
        let mut rim_sum = 0.0f32;
        let mut rim_n = 0u32;
        let mut inner_sum = 0.0f32;
        let mut inner_n = 0u32;
        for c in &layer.cells {
            let edge = !(covered(c.x - 1, c.y)
                && covered(c.x + 1, c.y)
                && covered(c.x, c.y - 1)
                && covered(c.x, c.y + 1));
            if edge {
                rim_sum += luma(c);
                rim_n += 1;
            } else {
                inner_sum += luma(c);
                inner_n += 1;
            }
        }
        assert!(rim_n > 0 && inner_n > 0, "cube needs both rim and interior");
        assert!(
            rim_sum / rim_n as f32 > inner_sum / inner_n as f32 * 1.2,
            "rim must clearly exceed interior"
        );
    }

    #[test]
    fn obj_parses_cube_and_fans_quads() {
        let text = "# unit cube shell\n\
            v 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\n\
            v 0 0 1\nv 1 0 1\nv 1 1 1\nv 0 1 1\n\
            f 1 2 3 4\nf 5 8 7 6\nf 1 5 6 2\n\
            f 2 6 7 3\nf 3 7 8 4\nf 5 1 4 8\n";
        let mesh = load_obj(text, white()).expect("cube must parse");
        assert_eq!(mesh.tris.len(), 12, "6 quads fan into 12 tris");
        assert_eq!(mesh.tris[0].v[0], Vec3::new(0.0, 0.0, 0.0));
    }

    #[test]
    fn obj_supports_ngons_suffixes_and_negative_indices() {
        // Pentagon fan + v/vt/vn tokens + from-the-end refs + ignored noise.
        let text = "v 0 0 0\r\nv 2 0 0\r\nv 2 2 0\r\nv 1 3 0\r\nv 0 2 0\r\n\
            vt 0 0\nvn 0 0 1\ng front\no pentagon\nusemtl none\ns 1\n\
            f 1/1/1 2/2/1 3/3/1 4/4/1 5/5/1\n\
            f -5 -4 -3\n";
        let mesh = load_obj(text, white()).expect("mixed OBJ must parse");
        assert_eq!(mesh.tris.len(), 3 + 1, "pentagon fans to 3, plus one tri");
    }

    #[test]
    fn obj_rejects_bad_geometry_with_line_numbers() {
        assert!(load_obj("v 0 oops 0\n", white()).is_err());
        assert!(load_obj("v 0 0\n", white()).is_err());
        assert!(load_obj("v 0 0 0\nf 1 2\n", white()).is_err());
        assert!(
            load_obj("v 0 0 0\nf 1 0 1\n", white()).is_err(),
            "index 0 is invalid"
        );
        assert!(load_obj("v 0 0 0\nf 1 2 9\n", white()).is_err());
        let err = load_obj("v 0 0 0\nv 1 0 0\nf 1 2 x\n", white()).unwrap_err();
        assert!(err.contains("line 3"), "errors name the line, got `{err}`");
    }
}
