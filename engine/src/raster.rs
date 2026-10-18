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
    /// World-unit depth step inside one 2x2 block that reads as an edge.
    pub edge_step: f32,
}

impl Default for DepthOpts {
    fn default() -> Self {
        Self {
            near: 0.5,
            far: 40.0,
            ambient: 0.28,
            edge_step: 0.36,
        }
    }
}

/// Luminance ramp for filled faces (dark → bright).
const RAMP: &[u8] = b" .:-=+*#%@";
/// Near-black void the geometry floats in (matches the engine night look).
const VOID_BG: Rgb = Rgb::new(9, 12, 20);

/// One supersampled fragment: perspective-correct depth, shaded albedo
/// (0..1 floats), and its luminance. 2x2 of these resolve to one cell.
#[derive(Clone, Copy)]
struct Sample {
    depth: f32,
    r: f32,
    g: f32,
    b: f32,
    lum: f32,
}

impl Sample {
    fn empty() -> Self {
        Self {
            depth: f32::INFINITY,
            r: 0.0,
            g: 0.0,
            b: 0.0,
            lum: 0.0,
        }
    }
}

/// Render `meshes` through `cam` into a still-cell layer at height `z`.
///
/// Pipeline per frame: project → flat-shade each face → barycentric raster
/// into a 2x supersampled depth buffer (near wins) → resolve each 2x2 block
/// to a ramp glyph, or a depth-edge glyph (`| - / \`) where the block spans
/// a depth step, with distance fog throughout.
pub fn render_layer(meshes: &[Mesh], cam: &Camera3, sun: Vec3, opts: &DepthOpts, z: i32) -> Layer {
    let sun = sun.norm();
    let w2 = (cam.width.max(1) * 2) as usize;
    let h2 = (cam.height.max(1) * 2) as usize;
    let mut buf = vec![Sample::empty(); w2 * h2];

    for mesh in meshes {
        for tri in &mesh.tris {
            rasterize_tri(&mut buf, w2, h2, tri, cam, sun, opts);
        }
    }

    let mut layer = Layer::new("mesh3d", z);
    for cy in 0..cam.height.max(1) {
        for cx in 0..cam.width.max(1) {
            if let Some(cell) = resolve_cell(&buf, w2, cx, cy, opts) {
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
fn apply_rim(layer: &mut Layer, w: i32, h: i32) {
    // Boolean occupancy grid: the naive any-scan per neighbor is O(n²)
    // over ~2k cells every frame, so index into a flat grid instead.
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
    // Snapshot positions first: brightening must not cascade to neighbors.
    let silhouette: Vec<(i32, i32)> = layer
        .cells
        .iter()
        .map(|c| (c.x, c.y))
        .filter(|&(x, y)| {
            !is_covered(x - 1, y)
                || !is_covered(x + 1, y)
                || !is_covered(x, y - 1)
                || !is_covered(x, y + 1)
        })
        .collect();
    for (x, y) in silhouette {
        if let Some(cell) = layer.cells.iter_mut().find(|c| c.x == x && c.y == y) {
            cell.cell.fg = cell.cell.fg.scaled(1.45);
        }
    }
}

/// Camera basis: forward/right/up for the yaw/pitch pose.
fn basis(cam: &Camera3) -> (Vec3, Vec3, Vec3) {
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
fn rasterize_tri(
    buf: &mut [Sample],
    w2: usize,
    h2: usize,
    tri: &Tri,
    cam: &Camera3,
    sun: Vec3,
    opts: &DepthOpts,
) {
    let (fwd, right, up) = basis(cam);
    // View space first so near-plane straddlers clip instead of dropping:
    // a ground slab with one corner past the lens must still draw its
    // visible half (the old code dropped the whole face).
    let view = tri.v.map(|p| {
        let d = p - cam.pos;
        Vec3::new(d.dot(right), d.dot(up), d.dot(fwd))
    });

    // Flat diffuse from the world-space face normal. Two-sided (|dot|):
    // builders don't agree on winding, and closed solids viewed from
    // outside show outward faces either way, so the absolute value lights
    // them exactly as an outward normal would.
    let n = (tri.v[1] - tri.v[0]).cross(tri.v[2] - tri.v[0]).norm();
    let diff = opts.ambient + (1.0 - opts.ambient) * n.dot(sun).abs().min(1.0);
    let shade = |c: u8| f32::from(c) / 255.0 * diff;
    let (sr, sg, sb) = (
        shade(tri.albedo.r),
        shade(tri.albedo.g),
        shade(tri.albedo.b),
    );
    let lum = 0.2126 * sr + 0.7152 * sg + 0.0722 * sb;

    let cx = cam.width as f32 / 2.0;
    let cy = cam.height as f32 / 2.0;
    for clipped in clip_near(view, opts.near) {
        let mut sx = [0.0f32; 3];
        let mut syv = [0.0f32; 3];
        let mut inv_z = [0.0f32; 3];
        for (i, v) in clipped.iter().enumerate() {
            // Supersample space: one cell covers 2x2 samples.
            sx[i] = (cx + cam.focal * v.x / v.z) * 2.0;
            syv[i] = (cy - cam.focal * v.y / v.z) * 2.0;
            inv_z[i] = 1.0 / v.z;
        }
        fill_tri(buf, w2, h2, &sx, &syv, &inv_z, sr, sg, sb, lum);
    }
}

/// Sutherland–Hodgman clip of one triangle against the near plane
/// (`vz >= near` survives). Yields 0, 1, or 2 triangles; view-space
/// positions interpolate linearly, which is exact for planes.
fn clip_near(tri: [Vec3; 3], near: f32) -> Vec<[Vec3; 3]> {
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
            }
        }
    }
}

/// Resolve one cell's 2x2 samples: ramp glyph by mean luminance, or a depth
/// edge glyph where the block spans a step; distance fog throughout.
/// `None` when no sample hit (void shows through).
fn resolve_cell(buf: &[Sample], w2: usize, cx: i32, cy: i32, opts: &DepthOpts) -> Option<Cell> {
    let mut hits = 0u32;
    let mut lum = 0.0f32;
    let mut r = 0.0f32;
    let mut g = 0.0f32;
    let mut b = 0.0f32;
    let mut zmin = f32::INFINITY;
    let mut zmax = f32::NEG_INFINITY;
    // Corner depths for the edge orientation (fall back to the block mean
    // where a corner missed — silhouettes keep their shaded side).
    let mut corner = [f32::NAN; 4];
    for dy in 0..2 {
        for dx in 0..2 {
            let s = buf[(cy as usize * 2 + dy) * w2 + cx as usize * 2 + dx];
            if s.depth.is_finite() {
                hits += 1;
                lum += s.lum;
                r += s.r;
                g += s.g;
                b += s.b;
                zmin = zmin.min(s.depth);
                zmax = zmax.max(s.depth);
                corner[dy * 2 + dx] = s.depth;
            }
        }
    }
    if hits == 0 {
        return None;
    }
    let n = hits as f32;
    let zavg = (zmin + zmax) / 2.0;
    // Distance fog: far geometry sinks toward the void.
    let fog = ((zavg - opts.near) / (opts.far - opts.near)).clamp(0.0, 1.0);
    // Keep distant ground readable without flattening the depth cue. The old
    // 75% reduction turned most of the world into near-black punctuation at
    // the default orbit distance, so foreground objects lost their context.
    let dim = 1.0 - 0.52 * fog;
    let to_u8 = |c: f32| (c / n * dim * 255.0).clamp(0.0, 255.0) as u8;
    let fg = Rgb::new(to_u8(r), to_u8(g), to_u8(b));

    let glyph = if zmax - zmin > opts.edge_step {
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
        let idx = ((lum / n * dim) * (RAMP.len() - 1) as f32)
            .round()
            .clamp(0.0, (RAMP.len() - 1) as f32) as usize;
        RAMP[idx] as char
    };
    Some(Cell::new(glyph, fg, VOID_BG))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_cam() -> Camera3 {
        Camera3::new(Vec3::new(0.0, 2.2, -9.0), 0.0, 0.0, 60.0, 96, 35)
    }

    fn white() -> Rgb {
        Rgb::new(255, 255, 255)
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
                .map(|c| c.cell.glyph)
        };
        let ramp_pos = |g: char| RAMP.iter().position(|&c| c == g as u8).unwrap_or(0);
        let near_glyph = render_center(0.0).expect("near cube covers center");
        let far_glyph = render_center(20.0).expect("far cube still covers center");
        assert!(
            ramp_pos(far_glyph) < ramp_pos(near_glyph),
            "far '{far_glyph}' must be dimmer than near '{near_glyph}'"
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
