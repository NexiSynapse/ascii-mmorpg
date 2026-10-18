//! 3D Meshes, procedural geometry primitives, and volumetric scene generators.

use ascii_mmorpg_shared::collision::WORLD_BOUNDS;
use glam::{Mat4, Vec3};
use std::f32::consts::PI;

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub color: [f32; 4],
    pub material_id: f32, // 0=Terrain, 1=Metal, 2=Cloth, 3=Foliage, 4=Glow/Fire, 5=Weather
    pub roughness: f32,
    /// Nonzero frame-local entity identity written to the metadata G-buffer.
    pub entity_id: f32,
    /// Stable visual faction/class used by the ASCII outline pass.
    pub entity_class: f32,
}

impl Vertex {
    pub const fn new(
        position: [f32; 3],
        normal: [f32; 3],
        color: [f32; 4],
        material_id: f32,
        roughness: f32,
    ) -> Self {
        Self {
            position,
            normal,
            color,
            material_id,
            roughness,
            entity_id: 0.0,
            entity_class: 0.0,
        }
    }

    fn with_entity_context(mut self, entity_id: u32, entity_class: u32) -> Self {
        self.entity_id = entity_id as f32;
        self.entity_class = entity_class as f32;
        self
    }
}

/// GPU-side hardware instance raw descriptor for batched rendering.
/// Aligned to 96 bytes (16 bytes matrix, 16 bytes color, 16 bytes material parameters).
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct InstanceRaw {
    pub model_matrix: [[f32; 4]; 4],
    pub color_tint: [f32; 4],
    pub material_params: [f32; 4], // x=material_override, y=roughness_override
}

impl InstanceRaw {
    pub const IDENTITY: Self = Self {
        model_matrix: [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
        color_tint: [1.0, 1.0, 1.0, 1.0],
        material_params: [-1.0, -1.0, -1.0, 0.0],
    };

    pub fn new(
        translation: [f32; 3],
        scale: [f32; 3],
        rotation_y: f32,
        color_tint: [f32; 4],
        material_override: f32,
        roughness_override: f32,
    ) -> Self {
        let mat = Mat4::from_translation(Vec3::from(translation))
            * Mat4::from_rotation_y(rotation_y)
            * Mat4::from_scale(Vec3::from(scale));
        Self {
            model_matrix: mat.to_cols_array_2d(),
            color_tint,
            material_params: [material_override, roughness_override, -1.0, 0.0],
        }
    }
}

/// Camera view-frustum plane for high-speed AABB & bounding sphere culling
/// borrowed from Asciicker's plane culling optimization.
#[derive(Copy, Clone, Debug, Default)]
pub struct FrustumPlane {
    pub normal: Vec3,
    pub distance: f32,
}

impl FrustumPlane {
    #[inline]
    pub fn distance_to_point(&self, p: Vec3) -> f32 {
        self.normal.dot(p) + self.distance
    }
}

/// 6-plane Camera View Frustum for CPU instance culling.
#[derive(Copy, Clone, Debug, Default)]
pub struct Frustum {
    pub planes: [FrustumPlane; 6],
}

impl Frustum {
    /// Extracts the 6 frustum planes from a view-projection matrix (Gribb-Hartmann algorithm).
    pub fn from_view_proj(vp: &Mat4) -> Self {
        let r0 = vp.row(0);
        let r1 = vp.row(1);
        let r2 = vp.row(2);
        let r3 = vp.row(3);

        let raw = [
            r3 + r0, // Left
            r3 - r0, // Right
            r3 + r1, // Bottom
            r3 - r1, // Top
            r3 + r2, // Near
            r3 - r2, // Far
        ];

        let mut planes = [FrustumPlane::default(); 6];
        for (i, p) in raw.iter().enumerate() {
            let n = Vec3::new(p.x, p.y, p.z);
            let len = n.length();
            if len > 1e-6 {
                planes[i] = FrustumPlane {
                    normal: n / len,
                    distance: p.w / len,
                };
            }
        }
        Self { planes }
    }

    /// Tests if an axis-aligned bounding box intersects or lies within the view frustum.
    pub fn test_aabb(&self, min: Vec3, max: Vec3) -> bool {
        for plane in &self.planes {
            let p = Vec3::new(
                if plane.normal.x > 0.0 { max.x } else { min.x },
                if plane.normal.y > 0.0 { max.y } else { min.y },
                if plane.normal.z > 0.0 { max.z } else { min.z },
            );
            if plane.distance_to_point(p) < 0.0 {
                return false;
            }
        }
        true
    }

    /// Tests if a bounding sphere intersects or lies within the view frustum.
    pub fn test_sphere(&self, center: Vec3, radius: f32) -> bool {
        for plane in &self.planes {
            if plane.distance_to_point(center) < -radius {
                return false;
            }
        }
        true
    }
}

/// Batched prop and particle instances for high-throughput GPU instanced drawing.
#[derive(Default, Clone, Debug)]
pub struct SceneBatches {
    pub tree_instances: Vec<InstanceRaw>,
    pub particle_instances: Vec<InstanceRaw>,
    pub pillar_instances: Vec<InstanceRaw>,
    pub custom_instances: Vec<InstanceRaw>,
}

impl SceneBatches {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear(&mut self) {
        self.tree_instances.clear();
        self.particle_instances.clear();
        self.pillar_instances.clear();
        self.custom_instances.clear();
    }

    pub fn total_instance_count(&self) -> usize {
        self.tree_instances.len()
            + self.particle_instances.len()
            + self.pillar_instances.len()
            + self.custom_instances.len()
    }

    pub fn add_pine_tree(&mut self, pos: [f32; 3], scale: f32, wind_sway: f32) {
        let rot_y = wind_sway * 0.05;
        self.tree_instances.push(InstanceRaw::new(
            pos,
            [scale, scale, scale],
            rot_y,
            [1.0, 1.0, 1.0, 1.0],
            -1.0,
            -1.0,
        ));
    }

    pub fn add_pillar(&mut self, pos: [f32; 3], height_scale: f32) {
        self.pillar_instances.push(InstanceRaw::new(
            pos,
            [1.0, height_scale, 1.0],
            0.0,
            [1.0, 1.0, 1.0, 1.0],
            -1.0,
            -1.0,
        ));
    }

    pub fn add_weather_particles(
        &mut self,
        center_x: f32,
        center_z: f32,
        time: f32,
        weather_mode: u32,
    ) {
        // Weather ink stays a quiet accent (reference captures: rain/snow
        // are dim streaks over a near-black void, never the brightest
        // thing in the frame — the old 0.65/0.82/1.0 tint formed a large
        // saturated-white blob under the dark-rebalanced lighting).
        let rain_color = [0.34, 0.44, 0.62, 0.75];
        let snow_color = [0.55, 0.60, 0.70, 0.8];
        let num_particles = 180;

        for i in 0..num_particles {
            let fi = i as f32;
            let seed_x = (fi * 17.31 + time * 1.5).sin() * 22.0;
            let seed_z = (fi * 29.43 + time * 0.8).cos() * 22.0;

            let px = center_x + seed_x;
            let pz = center_z + seed_z;

            match weather_mode {
                0 | 2 => {
                    let speed = 14.0;
                    let py = 12.0 - ((time * speed + fi * 0.8) % 12.0);
                    self.particle_instances.push(InstanceRaw::new(
                        [px, py, pz],
                        [0.04, 0.65, 0.04],
                        0.0,
                        rain_color,
                        5.0,
                        0.1,
                    ));
                }
                1 => {
                    let speed = 2.5;
                    let py = 10.0 - ((time * speed + fi * 0.4) % 10.0);
                    let drift_x = px + (time * 2.0 + fi).sin() * 0.4;
                    self.particle_instances.push(InstanceRaw::new(
                        [drift_x, py, pz],
                        [0.12, 0.12, 0.12],
                        0.0,
                        snow_color,
                        5.0,
                        0.2,
                    ));
                }
                _ => {}
            }
        }
    }
}

#[derive(Default)]
pub struct MeshBuffer {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
    entity_id: u32,
    entity_class: u32,
}

impl MeshBuffer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear(&mut self) {
        self.vertices.clear();
        self.indices.clear();
        self.entity_id = 0;
        self.entity_class = 0;
    }

    /// Sets the metadata stamped onto subsequently generated geometry.
    ///
    /// The renderer uses this for entity silhouettes while keeping terrain and props at zero.
    pub fn set_entity_context(&mut self, entity_id: u64, entity_class: u8) {
        // Reserved id 0 ("no entity": terrain, props, water) must stay
        // exactly zero. The 1-based wrap `((id-1) % 255) + 1` folds 0
        // forward to 1, which lit the whole landscape through any
        // `entity_id > 0` shader gate, so it is special-cased here.
        self.entity_id = if entity_id == 0 {
            0
        } else {
            ((entity_id.saturating_sub(1) % 255) + 1) as u32
        };
        self.entity_class = u32::from(entity_class);
    }

    /// Uniformly resize newly added model geometry around its ground anchor.
    /// Normals and per-vertex entity metadata are unchanged; callers pass the
    /// vertex count captured immediately before generating one actor.
    pub fn scale_vertices_since(&mut self, first_vertex: usize, anchor: [f32; 3], scale: f32) {
        if !scale.is_finite() || scale <= 0.0 {
            return;
        }
        let anchor = Vec3::from(anchor);
        let first_vertex = first_vertex.min(self.vertices.len());
        for vertex in &mut self.vertices[first_vertex..] {
            let position = Vec3::from(vertex.position);
            vertex.position = (anchor + (position - anchor) * scale).to_array();
        }
    }

    /// Rotates every vertex appended since `first_vertex` about the vertical
    /// axis through `anchor` by `angle` radians (right-handed +Y rotation).
    /// Normals rotate with the geometry so face shading and scene lighting
    /// stay correct; indices and entity metadata are untouched. Callers pass
    /// the vertex count captured immediately before generating one actor.
    pub fn rotate_vertices_since(&mut self, first_vertex: usize, anchor: [f32; 3], angle: f32) {
        if !angle.is_finite() || angle.abs() < f32::EPSILON {
            return;
        }
        let (sin, cos) = angle.sin_cos();
        let pivot_x = anchor[0];
        let pivot_z = anchor[2];
        let first_vertex = first_vertex.min(self.vertices.len());
        for vertex in &mut self.vertices[first_vertex..] {
            let px = vertex.position[0] - pivot_x;
            let pz = vertex.position[2] - pivot_z;
            vertex.position[0] = px * cos + pz * sin + pivot_x;
            vertex.position[2] = -px * sin + pz * cos + pivot_z;
            let nx = vertex.normal[0];
            let nz = vertex.normal[2];
            vertex.normal[0] = nx * cos + nz * sin;
            vertex.normal[2] = -nx * sin + nz * cos;
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn add_quad(
        &mut self,
        p0: [f32; 3],
        p1: [f32; 3],
        p2: [f32; 3],
        p3: [f32; 3],
        normal: [f32; 3],
        color: [f32; 4],
        material_id: f32,
        roughness: f32,
    ) {
        let base_idx = self.vertices.len() as u32;
        self.vertices.push(
            Vertex::new(p0, normal, color, material_id, roughness)
                .with_entity_context(self.entity_id, self.entity_class),
        );
        self.vertices.push(
            Vertex::new(p1, normal, color, material_id, roughness)
                .with_entity_context(self.entity_id, self.entity_class),
        );
        self.vertices.push(
            Vertex::new(p2, normal, color, material_id, roughness)
                .with_entity_context(self.entity_id, self.entity_class),
        );
        self.vertices.push(
            Vertex::new(p3, normal, color, material_id, roughness)
                .with_entity_context(self.entity_id, self.entity_class),
        );

        self.indices.extend_from_slice(&[
            base_idx,
            base_idx + 1,
            base_idx + 2,
            base_idx,
            base_idx + 2,
            base_idx + 3,
        ]);
    }

    pub fn add_box(
        &mut self,
        center: [f32; 3],
        size: [f32; 3],
        color: [f32; 4],
        material_id: f32,
        roughness: f32,
    ) {
        let (cx, cy, cz) = (center[0], center[1], center[2]);
        let (hx, hy, hz) = (size[0] * 0.5, size[1] * 0.5, size[2] * 0.5);

        // Top (+Y)
        self.add_quad(
            [cx - hx, cy + hy, cz - hz],
            [cx - hx, cy + hy, cz + hz],
            [cx + hx, cy + hy, cz + hz],
            [cx + hx, cy + hy, cz - hz],
            [0.0, 1.0, 0.0],
            color,
            material_id,
            roughness,
        );
        // Bottom (-Y)
        self.add_quad(
            [cx - hx, cy - hy, cz + hz],
            [cx - hx, cy - hy, cz - hz],
            [cx + hx, cy - hy, cz - hz],
            [cx + hx, cy - hy, cz + hz],
            [0.0, -1.0, 0.0],
            color,
            material_id,
            roughness,
        );
        // Front (+Z)
        self.add_quad(
            [cx - hx, cy - hy, cz + hz],
            [cx + hx, cy - hy, cz + hz],
            [cx + hx, cy + hy, cz + hz],
            [cx - hx, cy + hy, cz + hz],
            [0.0, 0.0, 1.0],
            color,
            material_id,
            roughness,
        );
        // Back (-Z)
        self.add_quad(
            [cx + hx, cy - hy, cz - hz],
            [cx - hx, cy - hy, cz - hz],
            [cx - hx, cy + hy, cz - hz],
            [cx + hx, cy + hy, cz - hz],
            [0.0, 0.0, -1.0],
            color,
            material_id,
            roughness,
        );
        // Left (-X)
        self.add_quad(
            [cx - hx, cy - hy, cz - hz],
            [cx - hx, cy - hy, cz + hz],
            [cx - hx, cy + hy, cz + hz],
            [cx - hx, cy + hy, cz - hz],
            [-1.0, 0.0, 0.0],
            color,
            material_id,
            roughness,
        );
        // Right (+X)
        self.add_quad(
            [cx + hx, cy - hy, cz + hz],
            [cx + hx, cy - hy, cz - hz],
            [cx + hx, cy + hy, cz - hz],
            [cx + hx, cy + hy, cz + hz],
            [1.0, 0.0, 0.0],
            color,
            material_id,
            roughness,
        );
    }

    pub fn add_grass_blade(&mut self, x: f32, z: f32, height: f32, sway: f32) {
        let width = 0.07;
        let tip_x = x + sway * height;
        let color = [0.02, 0.105, 0.035, 1.0];
        self.add_quad(
            [x - width, 0.02, z],
            [x + width, 0.02, z],
            [tip_x + width * 0.35, height, z],
            [tip_x - width * 0.35, height, z],
            [0.0, 0.0, 1.0],
            color,
            3.0,
            0.8,
        );
        self.add_quad(
            [x, 0.02, z - width],
            [x, 0.02, z + width],
            [tip_x, height, z + width * 0.35],
            [tip_x, height, z - width * 0.35],
            [1.0, 0.0, 0.0],
            color,
            3.0,
            0.8,
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub fn add_cone(
        &mut self,
        base: [f32; 3],
        radius: f32,
        height: f32,
        segments: u32,
        color: [f32; 4],
        material_id: f32,
        roughness: f32,
    ) {
        let apex = [base[0], base[1] + height, base[2]];
        let seg_f = segments as f32;
        for i in 0..segments {
            let theta1 = (i as f32 / seg_f) * 2.0 * PI;
            let theta2 = ((i + 1) as f32 / seg_f) * 2.0 * PI;

            let p1 = [
                base[0] + radius * theta1.cos(),
                base[1],
                base[2] + radius * theta1.sin(),
            ];
            let p2 = [
                base[0] + radius * theta2.cos(),
                base[1],
                base[2] + radius * theta2.sin(),
            ];

            let mid_theta = (theta1 + theta2) * 0.5;
            let normal = [mid_theta.cos() * 0.707, 0.707, mid_theta.sin() * 0.707];

            let base_idx = self.vertices.len() as u32;
            self.vertices.push(
                Vertex::new(p1, normal, color, material_id, roughness)
                    .with_entity_context(self.entity_id, self.entity_class),
            );
            self.vertices.push(
                Vertex::new(p2, normal, color, material_id, roughness)
                    .with_entity_context(self.entity_id, self.entity_class),
            );
            self.vertices.push(
                Vertex::new(apex, normal, color, material_id, roughness)
                    .with_entity_context(self.entity_id, self.entity_class),
            );

            self.indices
                .extend_from_slice(&[base_idx, base_idx + 1, base_idx + 2]);
        }
    }

    /// Procedural 3D Humanoid Warrior with articulated joints (Path C).
    ///
    /// `facing_angle` rotates the whole figure â€” head, helm, body, arms,
    /// and sword as one â€” about the vertical axis through its foot anchor.
    /// The part geometry is authored facing +Z (toward the default orbit
    /// camera), so callers pass the camera yaw to keep the character facing
    /// the viewer as that camera orbits.
    pub fn add_humanoid(
        &mut self,
        pos: [f32; 3],
        facing_angle: f32,
        walk_phase: f32,
        attack_phase: f32,
    ) {
        let (x, y, z) = (pos[0], pos[1], pos[2]);
        let first_vertex = self.vertices.len();
        let armor_color = [0.84, 0.88, 0.96, 1.0]; // Bright gleaming steel
        let steel_dark = [0.55, 0.60, 0.68, 1.0]; // Shaded joint steel
        let gold_trim = [0.96, 0.80, 0.28, 1.0]; // Polished gilded inlay
        let visor_glow = [0.35, 0.90, 1.0, 1.0]; // Cyan enchanted visor
        let cape_red = [0.72, 0.16, 0.22, 1.0]; // Deep royal crimson cloak
        let leather = [0.36, 0.22, 0.14, 1.0]; // Dark oiled leather

        // --- 1. Head & Greathelm ---
        // Greathelm crown
        self.add_box([x, y + 1.76, z], [0.46, 0.44, 0.44], armor_color, 1.0, 0.15);
        // Golden brow band
        self.add_box(
            [x, y + 1.86, z + 0.02],
            [0.48, 0.07, 0.46],
            gold_trim,
            1.0,
            0.2,
        );
        // Cheek guards & visor bevel
        self.add_box(
            [x - 0.21, y + 1.66, z + 0.08],
            [0.08, 0.24, 0.32],
            steel_dark,
            1.0,
            0.25,
        );
        self.add_box(
            [x + 0.21, y + 1.66, z + 0.08],
            [0.08, 0.24, 0.32],
            steel_dark,
            1.0,
            0.25,
        );
        // Enchanted visor slit
        self.add_box(
            [x, y + 1.76, z + 0.23],
            [0.34, 0.07, 0.04],
            visor_glow,
            4.0,
            0.1,
        );
        // Winged Knight Plume / Crest (sweeps backward)
        self.add_box(
            [x, y + 2.06, z - 0.06],
            [0.10, 0.24, 0.38],
            cape_red,
            2.0,
            0.5,
        );
        self.add_box(
            [x, y + 2.14, z - 0.20],
            [0.08, 0.16, 0.28],
            gold_trim,
            1.0,
            0.3,
        );
        // Gorget / Neck collar
        self.add_box([x, y + 1.54, z], [0.38, 0.10, 0.36], steel_dark, 1.0, 0.3);

        // --- 2. Cuirass & Faulds (Chestplate & Waist) ---
        // Main pectoral breastplate
        self.add_box([x, y + 1.28, z], [0.66, 0.54, 0.42], armor_color, 1.0, 0.15);
        // Medial ridge & gold eagle/cross heraldry
        self.add_box(
            [x, y + 1.28, z + 0.22],
            [0.12, 0.46, 0.04],
            gold_trim,
            1.0,
            0.2,
        );
        self.add_box(
            [x, y + 1.36, z + 0.22],
            [0.38, 0.10, 0.04],
            gold_trim,
            1.0,
            0.2,
        );
        // Oiled leather belt & gold buckle
        self.add_box([x, y + 0.98, z], [0.60, 0.10, 0.38], leather, 2.0, 0.6);
        self.add_box(
            [x, y + 0.98, z + 0.20],
            [0.16, 0.12, 0.05],
            gold_trim,
            1.0,
            0.2,
        );
        // Segmented Fauld & Tassets (Hip/thigh guards)
        self.add_box(
            [x - 0.18, y + 0.86, z + 0.05],
            [0.26, 0.20, 0.32],
            steel_dark,
            1.0,
            0.3,
        );
        self.add_box(
            [x + 0.18, y + 0.86, z + 0.05],
            [0.26, 0.20, 0.32],
            steel_dark,
            1.0,
            0.3,
        );

        // --- 3. Layered Spiked Pauldrons (Shoulders) ---
        self.add_box(
            [x - 0.46, y + 1.48, z],
            [0.34, 0.26, 0.36],
            armor_color,
            1.0,
            0.2,
        );
        self.add_box(
            [x - 0.52, y + 1.56, z],
            [0.16, 0.14, 0.26],
            gold_trim,
            1.0,
            0.2,
        );
        self.add_box(
            [x + 0.46, y + 1.48, z],
            [0.34, 0.26, 0.36],
            armor_color,
            1.0,
            0.2,
        );
        self.add_box(
            [x + 0.52, y + 1.56, z],
            [0.16, 0.14, 0.26],
            gold_trim,
            1.0,
            0.2,
        );

        // --- 4. Flowing Cape ---
        let cape_sway = (walk_phase * 1.5).sin() * 0.15;
        self.add_box(
            [x, y + 0.96, z - 0.25 + cape_sway],
            [0.58, 0.90, 0.08],
            cape_red,
            2.0,
            0.6,
        );
        self.add_box(
            [x, y + 0.48, z - 0.28 + cape_sway * 1.3],
            [0.52, 0.30, 0.06],
            cape_red,
            2.0,
            0.65,
        );

        // --- 5. Articulated Legs & Sabatons ---
        let leg_swing = (walk_phase).sin() * 0.3;
        // Upper Cuisses & Knee Poleyns
        self.add_box(
            [x - 0.18, y + 0.68, z + leg_swing],
            [0.22, 0.36, 0.24],
            steel_dark,
            1.0,
            0.3,
        );
        self.add_box(
            [x - 0.18, y + 0.52, z + leg_swing + 0.10],
            [0.16, 0.14, 0.10],
            gold_trim,
            1.0,
            0.2,
        );
        self.add_box(
            [x + 0.18, y + 0.68, z - leg_swing],
            [0.22, 0.36, 0.24],
            steel_dark,
            1.0,
            0.3,
        );
        self.add_box(
            [x + 0.18, y + 0.52, z - leg_swing + 0.10],
            [0.16, 0.14, 0.10],
            gold_trim,
            1.0,
            0.2,
        );
        // Shin Greaves
        self.add_box(
            [x - 0.18, y + 0.32, z + leg_swing],
            [0.20, 0.36, 0.22],
            armor_color,
            1.0,
            0.2,
        );
        self.add_box(
            [x + 0.18, y + 0.32, z - leg_swing],
            [0.20, 0.36, 0.22],
            armor_color,
            1.0,
            0.2,
        );
        // Fluted Sabatons (Boots)
        self.add_box(
            [x - 0.18, y + 0.12, z + leg_swing + 0.06],
            [0.24, 0.22, 0.36],
            leather,
            2.0,
            0.4,
        );
        self.add_box(
            [x + 0.18, y + 0.12, z - leg_swing + 0.06],
            [0.24, 0.22, 0.36],
            leather,
            2.0,
            0.4,
        );

        // --- 6. Left Arm & Knight Heater Shield ---
        self.add_box(
            [x - 0.48, y + 1.10, z],
            [0.18, 0.52, 0.18],
            steel_dark,
            1.0,
            0.3,
        );
        // Heater Shield with gilded trim and heraldic cross
        self.add_box(
            [x - 0.56, y + 1.08, z + 0.25],
            [0.12, 0.76, 0.48],
            gold_trim,
            1.0,
            0.2,
        );
        self.add_box(
            [x - 0.58, y + 1.08, z + 0.25],
            [0.10, 0.68, 0.40],
            cape_red,
            2.0,
            0.4,
        );
        self.add_box(
            [x - 0.59, y + 1.08, z + 0.25],
            [0.06, 0.14, 0.28],
            gold_trim,
            1.0,
            0.2,
        );
        self.add_box(
            [x - 0.59, y + 1.08, z + 0.25],
            [0.06, 0.48, 0.10],
            gold_trim,
            1.0,
            0.2,
        );

        // --- 7. Right Arm & Crusader Broadsword ---
        let arm_swing = if attack_phase > 0.0 {
            (attack_phase * PI).sin() * 0.9
        } else {
            -(walk_phase).sin() * 0.25
        };
        self.add_box(
            [x + 0.48, y + 1.10, z + arm_swing],
            [0.18, 0.52, 0.18],
            steel_dark,
            1.0,
            0.3,
        );
        self.add_box(
            [x + 0.48, y + 0.80, z + arm_swing + 0.12],
            [0.16, 0.16, 0.16],
            leather,
            2.0,
            0.4,
        );
        // Broadsword Blade with fuller groove and high metallic luster
        self.add_box(
            [x + 0.50, y + 1.45, z + arm_swing + 0.42],
            [0.05, 1.25, 0.16],
            [0.96, 0.98, 1.0, 1.0],
            1.0,
            0.04,
        );
        // Swept Crossguard & Quillons
        self.add_box(
            [x + 0.50, y + 0.84, z + arm_swing + 0.22],
            [0.38, 0.08, 0.10],
            gold_trim,
            1.0,
            0.2,
        );
        // Wire-wrapped hilt & octagonal pommel stone
        self.add_box(
            [x + 0.50, y + 0.72, z + arm_swing + 0.15],
            [0.08, 0.18, 0.08],
            leather,
            2.0,
            0.5,
        );
        self.add_box(
            [x + 0.50, y + 0.61, z + arm_swing + 0.10],
            [0.12, 0.10, 0.12],
            gold_trim,
            1.0,
            0.2,
        );

        // Rotate the authored +Z-facing figure about its foot anchor so the
        // head and body follow the orbit camera (see facing_angle docs).
        self.rotate_vertices_since(first_vertex, pos, facing_angle);
    }

    /// Procedural 3D Skeletal Undead (Path C).
    pub fn add_skeleton(&mut self, pos: [f32; 3], anim_phase: f32, attack_phase: f32) {
        let (x, y, z) = (pos[0], pos[1], pos[2]);
        let bone_white = [0.84, 0.82, 0.76, 1.0];
        let bone_shadow = [0.52, 0.48, 0.42, 1.0];
        let eye_glow = [1.0, 0.35, 0.15, 1.0]; // Eerie ember soul glint
        let rust_steel = [0.42, 0.38, 0.34, 1.0];

        let shamble = (anim_phase * 3.0).sin() * 0.22;
        let head_tilt = (anim_phase * 2.5).cos() * 0.06;

        // --- Cranium & Skull ---
        self.add_box(
            [x + head_tilt, y + 1.70, z],
            [0.38, 0.36, 0.36],
            bone_white,
            3.0,
            0.6,
        );
        // Hollow eye sockets with burning embers
        self.add_box(
            [x + head_tilt - 0.10, y + 1.70, z + 0.19],
            [0.08, 0.08, 0.04],
            eye_glow,
            4.0,
            0.1,
        );
        self.add_box(
            [x + head_tilt + 0.10, y + 1.70, z + 0.19],
            [0.08, 0.08, 0.04],
            eye_glow,
            4.0,
            0.1,
        );
        // Maxilla & teeth / jaw
        self.add_box(
            [x + head_tilt, y + 1.54, z + 0.10],
            [0.24, 0.12, 0.22],
            bone_shadow,
            3.0,
            0.7,
        );

        // --- Spine & Cervical Vertebrae ---
        self.add_box([x, y + 1.44, z], [0.12, 0.18, 0.12], bone_white, 3.0, 0.6);

        // --- Thoracic Ribcage ---
        self.add_box([x, y + 1.20, z], [0.48, 0.38, 0.30], bone_white, 3.0, 0.6);
        self.add_box([x, y + 1.20, z], [0.32, 0.30, 0.34], bone_shadow, 3.0, 0.8);
        self.add_box([x, y + 0.94, z], [0.14, 0.22, 0.14], bone_shadow, 3.0, 0.7);

        // --- Pelvis ---
        self.add_box([x, y + 0.80, z], [0.40, 0.16, 0.26], bone_white, 3.0, 0.6);

        // --- Skeletal Legs (Shambling Stride) ---
        self.add_box(
            [x - 0.16, y + 0.50, z + shamble],
            [0.10, 0.50, 0.10],
            bone_white,
            3.0,
            0.6,
        );
        self.add_box(
            [x - 0.16, y + 0.18, z + shamble],
            [0.10, 0.44, 0.10],
            bone_shadow,
            3.0,
            0.6,
        );
        self.add_box(
            [x - 0.16, y + 0.06, z + shamble + 0.06],
            [0.12, 0.10, 0.22],
            bone_white,
            3.0,
            0.6,
        );

        self.add_box(
            [x + 0.16, y + 0.50, z - shamble],
            [0.10, 0.50, 0.10],
            bone_white,
            3.0,
            0.6,
        );
        self.add_box(
            [x + 0.16, y + 0.18, z - shamble],
            [0.10, 0.44, 0.10],
            bone_shadow,
            3.0,
            0.6,
        );
        self.add_box(
            [x + 0.16, y + 0.06, z - shamble + 0.06],
            [0.12, 0.10, 0.22],
            bone_white,
            3.0,
            0.6,
        );

        // --- Left Arm ---
        self.add_box(
            [x - 0.34, y + 1.10, z],
            [0.09, 0.46, 0.09],
            bone_white,
            3.0,
            0.6,
        );
        self.add_box(
            [x - 0.34, y + 0.76, z + 0.10],
            [0.08, 0.38, 0.08],
            bone_shadow,
            3.0,
            0.6,
        );

        // --- Right Arm & Rusted Blade ---
        let strike = if attack_phase > 0.0 {
            (attack_phase * PI).sin() * 0.9
        } else {
            -shamble * 0.5
        };
        self.add_box(
            [x + 0.34, y + 1.10, z + strike],
            [0.09, 0.46, 0.09],
            bone_white,
            3.0,
            0.6,
        );
        self.add_box(
            [x + 0.36, y + 0.78, z + strike + 0.16],
            [0.08, 0.38, 0.08],
            bone_shadow,
            3.0,
            0.6,
        );
        // Notched Rusted Scimitar
        self.add_box(
            [x + 0.38, y + 1.18, z + strike + 0.38],
            [0.05, 0.92, 0.14],
            rust_steel,
            2.0,
            0.4,
        );
        self.add_box(
            [x + 0.38, y + 0.76, z + strike + 0.22],
            [0.24, 0.06, 0.08],
            rust_steel,
            2.0,
            0.5,
        );
    }

    /// Procedural 3D Skeletal Champion Warrior with armor, tower shield, and broadsword (Path C).
    pub fn add_skeleton_warrior(&mut self, pos: [f32; 3], anim_phase: f32, attack_phase: f32) {
        let (x, y, z) = (pos[0], pos[1], pos[2]);
        let iron_plate = [0.38, 0.40, 0.44, 1.0];
        let brass_trim = [0.78, 0.62, 0.24, 1.0];
        let chainmail = [0.30, 0.32, 0.36, 1.0];

        let shamble = (anim_phase * 3.0).sin() * 0.25;

        // Base skeleton geometry
        self.add_skeleton(pos, anim_phase, attack_phase);

        // --- Horned Iron Spangenhelm ---
        self.add_box([x, y + 1.84, z], [0.42, 0.20, 0.42], iron_plate, 1.0, 0.3);
        self.add_box(
            [x, y + 1.88, z + 0.04],
            [0.44, 0.06, 0.44],
            brass_trim,
            1.0,
            0.2,
        );
        // Iron Horns protruding from helm
        self.add_box(
            [x - 0.26, y + 1.95, z],
            [0.08, 0.24, 0.08],
            iron_plate,
            1.0,
            0.4,
        );
        self.add_box(
            [x + 0.26, y + 1.95, z],
            [0.08, 0.24, 0.08],
            iron_plate,
            1.0,
            0.4,
        );

        // --- Spiked Iron Pauldrons ---
        self.add_box(
            [x - 0.42, y + 1.40, z],
            [0.28, 0.22, 0.30],
            iron_plate,
            1.0,
            0.3,
        );
        self.add_box(
            [x - 0.48, y + 1.48, z],
            [0.12, 0.12, 0.12],
            brass_trim,
            1.0,
            0.2,
        );
        self.add_box(
            [x + 0.42, y + 1.40, z],
            [0.28, 0.22, 0.30],
            iron_plate,
            1.0,
            0.3,
        );
        self.add_box(
            [x + 0.48, y + 1.48, z],
            [0.12, 0.12, 0.12],
            brass_trim,
            1.0,
            0.2,
        );

        // --- Tattered Chainmail Hauberk & Breastplate Fragment ---
        self.add_box(
            [x, y + 1.25, z + 0.04],
            [0.52, 0.36, 0.32],
            chainmail,
            2.0,
            0.5,
        );
        self.add_box(
            [x, y + 1.30, z + 0.18],
            [0.38, 0.24, 0.06],
            iron_plate,
            1.0,
            0.3,
        );

        // --- Reinforced Tower Shield on Left Arm ---
        self.add_box(
            [x - 0.50, y + 0.98, z + 0.26],
            [0.12, 0.88, 0.54],
            iron_plate,
            1.0,
            0.3,
        );
        self.add_box(
            [x - 0.52, y + 0.98, z + 0.26],
            [0.10, 0.82, 0.48],
            brass_trim,
            1.0,
            0.2,
        );
        // Central iron boss & skull emblem
        self.add_box(
            [x - 0.56, y + 0.98, z + 0.26],
            [0.08, 0.18, 0.18],
            iron_plate,
            1.0,
            0.2,
        );

        // --- Heavy Two-Handed Executioner's Broadsword ---
        let strike = if attack_phase > 0.0 {
            (attack_phase * PI).sin() * 0.9
        } else {
            -shamble * 0.5
        };
        self.add_box(
            [x + 0.48, y + 1.42, z + strike + 0.44],
            [0.06, 1.35, 0.20],
            iron_plate,
            1.0,
            0.15,
        );
        self.add_box(
            [x + 0.48, y + 0.78, z + strike + 0.24],
            [0.36, 0.08, 0.10],
            brass_trim,
            1.0,
            0.2,
        );
    }

    /// Procedural 3D Mossling Forest Lurker
    pub fn add_mossling(&mut self, pos: [f32; 3], anim_phase: f32) {
        let (x, y, z) = (pos[0], pos[1], pos[2]);
        let breathe = (anim_phase * 2.0).sin() * 0.06;
        let moss_green = [0.26, 0.58, 0.24, 1.0];
        let moss_dark = [0.14, 0.36, 0.12, 1.0];
        let bark_brown = [0.38, 0.26, 0.16, 1.0];
        let eye_glow = [1.0, 0.82, 0.20, 1.0]; // Predatory luminescent amber
        let spore_glow = [0.45, 0.95, 0.55, 1.0]; // Pulsing bioluminescent fungal spores

        // --- Bulky Moss-Covered Carapace ---
        self.add_box(
            [x, y + 0.68 + breathe, z],
            [1.02, 0.88, 0.82],
            moss_green,
            3.0,
            0.7,
        );
        // Underbelly & bark plating
        self.add_box(
            [x, y + 0.42 + breathe * 0.5, z],
            [0.85, 0.36, 0.70],
            bark_brown,
            3.0,
            0.8,
        );
        // Dorsal spine ridge
        self.add_box(
            [x, y + 1.10 + breathe, z],
            [0.22, 0.24, 0.65],
            moss_dark,
            3.0,
            0.6,
        );

        // --- Quadruped Muscular Stalker Limbs with Claws ---
        let stride = (anim_phase * 3.0).sin() * 0.12;
        self.add_box(
            [x - 0.38, y + 0.24, z - 0.22 + stride],
            [0.30, 0.46, 0.30],
            moss_dark,
            3.0,
            0.8,
        );
        self.add_box(
            [x + 0.38, y + 0.24, z - 0.22 - stride],
            [0.30, 0.46, 0.30],
            moss_dark,
            3.0,
            0.8,
        );
        self.add_box(
            [x - 0.38, y + 0.24, z + 0.22 - stride],
            [0.30, 0.46, 0.30],
            moss_dark,
            3.0,
            0.8,
        );
        self.add_box(
            [x + 0.38, y + 0.24, z + 0.22 + stride],
            [0.30, 0.46, 0.30],
            moss_dark,
            3.0,
            0.8,
        );
        // Root talons / claws
        self.add_box(
            [x - 0.38, y + 0.06, z - 0.14 + stride],
            [0.24, 0.10, 0.16],
            bark_brown,
            3.0,
            0.9,
        );
        self.add_box(
            [x + 0.38, y + 0.06, z - 0.14 - stride],
            [0.24, 0.10, 0.16],
            bark_brown,
            3.0,
            0.9,
        );
        self.add_box(
            [x - 0.38, y + 0.06, z + 0.30 - stride],
            [0.24, 0.10, 0.16],
            bark_brown,
            3.0,
            0.9,
        );
        self.add_box(
            [x + 0.38, y + 0.06, z + 0.30 + stride],
            [0.24, 0.10, 0.16],
            bark_brown,
            3.0,
            0.9,
        );

        // --- Fanged Maw & Mandibles ---
        self.add_box(
            [x, y + 0.58 + breathe, z + 0.42],
            [0.55, 0.28, 0.22],
            moss_dark,
            3.0,
            0.7,
        );
        self.add_box(
            [x - 0.18, y + 0.50 + breathe, z + 0.48],
            [0.08, 0.16, 0.12],
            [0.95, 0.95, 0.90, 1.0],
            2.0,
            0.3,
        );
        self.add_box(
            [x + 0.18, y + 0.50 + breathe, z + 0.48],
            [0.08, 0.16, 0.12],
            [0.95, 0.95, 0.90, 1.0],
            2.0,
            0.3,
        );

        // --- Multi-Faceted Luminescent Eyes ---
        self.add_box(
            [x - 0.24, y + 0.88 + breathe, z + 0.42],
            [0.11, 0.09, 0.08],
            eye_glow,
            4.0,
            0.1,
        );
        self.add_box(
            [x + 0.24, y + 0.88 + breathe, z + 0.42],
            [0.11, 0.09, 0.08],
            eye_glow,
            4.0,
            0.1,
        );
        self.add_box(
            [x - 0.12, y + 0.96 + breathe, z + 0.40],
            [0.08, 0.07, 0.06],
            eye_glow,
            4.0,
            0.1,
        );
        self.add_box(
            [x + 0.12, y + 0.96 + breathe, z + 0.40],
            [0.08, 0.07, 0.06],
            eye_glow,
            4.0,
            0.1,
        );

        // --- Bioluminescent Fungal Horns & Spore Pods ---
        self.add_cone(
            [x - 0.30, y + 1.12 + breathe, z],
            0.16,
            0.52,
            6,
            moss_dark,
            3.0,
            0.6,
        );
        self.add_cone(
            [x + 0.30, y + 1.12 + breathe, z],
            0.16,
            0.52,
            6,
            moss_dark,
            3.0,
            0.6,
        );
        let spore_pulse = (anim_phase * 4.0).sin() * 0.04;
        self.add_box(
            [x - 0.30, y + 1.42 + breathe, z],
            [0.12 + spore_pulse, 0.12 + spore_pulse, 0.12],
            spore_glow,
            4.0,
            0.1,
        );
        self.add_box(
            [x + 0.30, y + 1.42 + breathe, z],
            [0.12 + spore_pulse, 0.12 + spore_pulse, 0.12],
            spore_glow,
            4.0,
            0.1,
        );
    }

    /// Procedural 3D Herbalist NPC
    pub fn add_herbalist(&mut self, pos: [f32; 3], anim_phase: f32) {
        let (x, y, z) = (pos[0], pos[1], pos[2]);
        let robe_color = [0.20, 0.36, 0.56, 1.0]; // Scholar twilight blue
        let cowl_color = [0.15, 0.24, 0.40, 1.0];
        let wood_color = [0.42, 0.28, 0.16, 1.0];
        let gold_trim = [0.90, 0.74, 0.26, 1.0];
        let leather_bag = [0.38, 0.22, 0.14, 1.0];
        let potion_red = [0.92, 0.18, 0.22, 1.0]; // Healing draught
        let potion_blue = [0.24, 0.72, 1.0, 1.0]; // Mana potion
        let lantern_amber = [1.0, 0.82, 0.36, 1.0];

        // --- Layered Scholar Robe with Embroidered Hem ---
        self.add_box([x, y + 0.88, z], [0.58, 1.34, 0.48], robe_color, 2.0, 0.6);
        self.add_box([x, y + 0.26, z], [0.62, 0.14, 0.52], gold_trim, 1.0, 0.3);
        // Hood & Cowl with shadow veil
        self.add_box([x, y + 1.68, z], [0.50, 0.46, 0.50], cowl_color, 2.0, 0.6);
        self.add_box(
            [x, y + 1.64, z + 0.20],
            [0.32, 0.28, 0.10],
            [0.10, 0.12, 0.16, 1.0],
            2.0,
            0.8,
        );

        // --- Potion Satchel & Alchemical Flasks on Belt ---
        self.add_box(
            [x - 0.28, y + 0.85, z + 0.10],
            [0.20, 0.26, 0.18],
            leather_bag,
            2.0,
            0.5,
        );
        self.add_box(
            [x - 0.28, y + 0.90, z + 0.20],
            [0.08, 0.12, 0.08],
            potion_red,
            4.0,
            0.1,
        );
        self.add_box(
            [x - 0.18, y + 0.90, z + 0.20],
            [0.08, 0.12, 0.08],
            potion_blue,
            4.0,
            0.1,
        );

        // --- Gnarled Elderwood Staff with Brass Lantern ---
        self.add_box(
            [x + 0.44, y + 0.95, z + 0.15],
            [0.08, 1.94, 0.08],
            wood_color,
            3.0,
            0.5,
        );
        // Brass Filigree Lantern Cage
        self.add_box(
            [x + 0.44, y + 1.88, z + 0.15],
            [0.24, 0.30, 0.24],
            gold_trim,
            1.0,
            0.2,
        );
        // Flickering Inner Flame (Emissive Glow)
        let flicker = (anim_phase * 6.0).sin() * 0.08;
        self.add_box(
            [x + 0.44, y + 1.88, z + 0.15],
            [0.16 + flicker, 0.22 + flicker, 0.16 + flicker],
            lantern_amber,
            4.0,
            0.1,
        );
    }

    /// Procedural 3D Pine Tree with sculpted timber trunk, buttressed root flares,
    /// and an elevated multi-tier foliage canopy.
    pub fn add_pine_tree(&mut self, pos: [f32; 3], wind_sway: f32) {
        let (x, y, z) = (pos[0], pos[1], pos[2]);
        let trunk_color = [0.28, 0.155, 0.055, 1.0];
        let trunk_dark = [0.185, 0.095, 0.035, 1.0];
        let pine_green_dark = [0.03, 0.135, 0.05, 1.0];
        let pine_green = [0.05, 0.165, 0.065, 1.0];
        let pine_green_light = [0.07, 0.205, 0.08, 1.0];

        // --- Sculpted Timber Trunk & Buttressed Root Flares ---
        // Main vertical trunk column rising from ground up into the upper canopy (material 7.0: Wood/Timber)
        self.add_box([x, y + 2.70, z], [0.70, 5.40, 0.70], trunk_color, 7.0, 0.7);
        // Vertical bark contour / ridge slabs for depth and striations
        self.add_box([x, y + 2.50, z], [0.78, 5.00, 0.36], trunk_dark, 7.0, 0.8);
        self.add_box([x, y + 2.50, z], [0.36, 5.00, 0.78], trunk_dark, 7.0, 0.8);

        // Buttress root collar at ground level (firmly anchored into terrain)
        self.add_box([x, y + 0.24, z], [1.25, 0.48, 1.25], trunk_dark, 7.0, 0.8);
        // Surface roots spreading into the soil, extending forward towards camera (+Z)
        self.add_box(
            [x, y + 0.14, z + 0.2],
            [0.50, 0.28, 1.80],
            trunk_color,
            7.0,
            0.85,
        );
        self.add_box([x, y + 0.14, z], [1.80, 0.28, 0.50], trunk_color, 7.0, 0.85);

        // --- Elevated Multi-Tier Foliage Canopy with Wind Sway ---
        // Lowest Tier (elevated to y = 3.60 so trunk is prominently visible underneath)
        let sway = wind_sway * 0.08;
        self.add_cone(
            [x + sway * 0.4, y + 3.60, z],
            1.70,
            1.80,
            8,
            pine_green_dark,
            3.0,
            0.6,
        );
        // Middle Tier
        self.add_cone(
            [x + sway * 0.8, y + 4.80, z],
            1.35,
            1.70,
            8,
            pine_green,
            3.0,
            0.6,
        );
        // Upper Tier
        self.add_cone(
            [x + sway * 1.2, y + 6.00, z],
            0.95,
            1.50,
            8,
            pine_green,
            3.0,
            0.6,
        );
        // Spire / Crown Apex
        self.add_cone(
            [x + sway * 1.5, y + 7.10, z],
            0.55,
            1.30,
            8,
            pine_green_light,
            3.0,
            0.6,
        );
    }

    /// Procedural 3D Ancient Ruins
    pub fn add_ruins(&mut self, pos: [f32; 3]) {
        let (x, y, z) = (pos[0], pos[1], pos[2]);
        let stone = [0.52, 0.54, 0.58, 1.0];
        let moss_stone = [0.38, 0.48, 0.38, 1.0];

        // Twin pillars
        self.add_box([x - 1.4, y + 1.25, z], [0.65, 2.50, 0.65], stone, 0.0, 0.6);
        self.add_box([x + 1.4, y + 0.95, z], [0.65, 1.90, 0.65], stone, 0.0, 0.6);

        // Fallen / broken architrave
        self.add_box([x - 0.2, y + 2.45, z], [2.20, 0.45, 0.75], stone, 0.0, 0.6);
        // Overgrown altar pedestal
        self.add_box(
            [x, y + 0.35, z + 0.8],
            [1.10, 0.70, 0.90],
            moss_stone,
            0.0,
            0.8,
        );
    }

    /// Procedural 3D Stone Gate
    pub fn add_gate(&mut self, pos: [f32; 3]) {
        let (x, y, z) = (pos[0], pos[1], pos[2]);
        let stone = [0.48, 0.50, 0.55, 1.0];
        let iron = [0.22, 0.24, 0.28, 1.0];
        let flame = [1.0, 0.65, 0.15, 1.0];

        // Gateway towers
        self.add_box([x - 2.2, y + 1.85, z], [1.10, 3.70, 1.10], stone, 0.0, 0.5);
        self.add_box([x + 2.2, y + 1.85, z], [1.10, 3.70, 1.10], stone, 0.0, 0.5);
        // Top lintel arch
        self.add_box([x, y + 3.45, z], [3.80, 0.65, 1.10], stone, 0.0, 0.5);

        // Portcullis Iron Grate
        for i in 0..5 {
            let offset = (i as f32 - 2.0) * 0.75;
            self.add_box([x + offset, y + 1.6, z], [0.08, 2.80, 0.08], iron, 1.0, 0.3);
        }

        // Twin Guard Torches on towers
        self.add_box(
            [x - 2.2, y + 2.6, z + 0.65],
            [0.22, 0.32, 0.22],
            flame,
            4.0,
            0.05,
        );
        self.add_box(
            [x + 2.2, y + 2.6, z + 0.65],
            [0.22, 0.32, 0.22],
            flame,
            4.0,
            0.05,
        );
    }

    /// Ground telegraph/impact ring (video-tools REPORT #3): dashes spaced
    /// evenly around `radius`, Emissive material 4 so they read bright on
    /// the dark carpet, phase rotates the dash pattern. The caller stamps
    /// the FX entity context first so the cells survive stage clearance as
    /// single-hue emissive ink.
    pub fn add_ground_ring(
        &mut self,
        center: [f32; 3],
        radius: f32,
        color: [f32; 4],
        phase: f32,
        dash_count: u32,
    ) {
        let (x, y, z) = (center[0], center[1], center[2]);
        let dash_count = dash_count.clamp(3, 16);
        for i in 0..dash_count {
            let angle = (i as f32 + phase) * std::f32::consts::TAU / dash_count as f32;
            self.add_box(
                [x + radius * angle.cos(), y, z + radius * angle.sin()],
                [0.30, 0.06, 0.30],
                color,
                4.0,
                0.05,
            );
        }
    }

    /// Impact burst timeline (video-tools REPORT #2, reference 90418
    /// 10.7/24.5 + 47489 8.5-14.5): a white-hot solid core grows first, an
    /// expanding ground ring follows, then embers scatter upward while
    /// everything fades. `progress` is 0..1 over the burst's life; `scale`
    /// is the max ring radius in world units. Caller stamps the FX entity
    /// context first (see `add_ground_ring`).
    pub fn add_impact_burst(
        &mut self,
        center: [f32; 3],
        progress: f32,
        color: [f32; 4],
        scale: f32,
    ) {
        let (x, y, z) = (center[0], center[1], center[2]);
        let progress = progress.clamp(0.0, 1.0);
        let ease_out = 1.0 - (1.0 - progress) * (1.0 - progress);
        let fade = (1.0 - progress).max(0.0);
        // White-hot core: the burst starts as a solid bright mass (the
        // reference bursts begin bright before rings/scatter appear).
        let core_mix = |c: f32| c + (1.0 - c) * 0.65;
        let hot = [
            core_mix(color[0]),
            core_mix(color[1]),
            core_mix(color[2]),
            1.0,
        ];
        let core_s = 0.16 + 0.30 * ease_out;
        self.add_box([x, y + 0.15, z], [core_s, core_s, core_s], hot, 4.0, 0.05);
        // Expanding dashed ring on the ground.
        self.add_ground_ring(
            [x, y + 0.10, z],
            scale * (0.25 + 1.15 * ease_out),
            [color[0] * fade, color[1] * fade, color[2] * fade, 1.0],
            progress * 1.5,
            8,
        );
        // Ember scatter: small fading shards rising off the hit point.
        if progress > 0.18 {
            for i in 0..6 {
                let angle = i as f32 * 1.05 + progress * 2.0;
                let dy = 0.25 + 0.55 * ease_out * (0.5 + (i % 3) as f32 * 0.35);
                let er = 0.07 + 0.05 * fade;
                let ember = [
                    color[0] * fade,
                    color[1] * fade,
                    color[2] * (fade * 0.85 + 0.15),
                    1.0,
                ];
                self.add_box(
                    [
                        x + er * 3.0 * angle.cos(),
                        y + 0.15 + dy,
                        z + er * 3.0 * angle.sin(),
                    ],
                    [er, er, er],
                    ember,
                    4.0,
                    0.05,
                );
            }
        }
    }

    /// Procedural 3D Volumetric Terrain (Cobblestone Highway & Meadow)
    pub fn add_terrain(&mut self, center_x: f32, center_z: f32, radius: f32) {
        let step = 1.5;
        let road_width = 3.2;

        // Snap generation bounds to the world step grid so all quads are world-stationary.
        // This eliminates sliding quads, geometry jitter, and border popping when the player moves.
        let x_min = ((center_x - radius) / step).floor() * step;
        let x_max = ((center_x + radius) / step).ceil() * step;
        let z_min = ((center_z - radius) / step).floor() * step;
        let z_max = ((center_z + radius) / step).ceil() * step;

        let mut x = x_min;
        while x < x_max {
            let mut z = z_min;
            while z < z_max {
                let quad_center_x = x + step * 0.5;
                let quad_center_z = z + step * 0.5;
                let dist_from_road = quad_center_x.abs();
                // Meadow rolls: corner-sampled from one height field so
                // adjacent quads share edge heights exactly. The old
                // per-quad flat height left a cliff at every quad seam, and
                // each seam rendered as a full-width '-' rule at glancing
                // camera angles (the reported "lines"). Road and curb stay
                // flat: their edges are real features, not artifacts.
                let meadow_h = |qx: f32, qz: f32| (qx * 0.25).sin() * (qz * 0.25).cos() * 0.15;
                // Southern ocean: the walkable world ends at
                // `WORLD_BOUNDS.max_y` (mesh z = world y * 0.8, the same
                // world-to-scene scale the renderer uses for every position),
                // so the shoreline sits exactly on the map's bottom edge.
                // South of it the meadow gives way to a sand beach and open
                // water; this branch fires before the road so the Cobblestone
                // Highway visibly dead-ends into the sea.
                const MESH_SCALE: f32 = 0.8;
                let shore_z = WORLD_BOUNDS.max_y as f32 * MESH_SCALE;
                let (color, mat, rough, heights) = if quad_center_z >= shore_z {
                    // Open water: a flat plane sunk slightly below the land.
                    // The rolling swell and bright foam crests are driven by
                    // the water shaders, which key off material 6.
                    ([0.12, 0.30, 0.55, 1.0], 6.0, 0.25, [-0.12; 4])
                } else if quad_center_z >= shore_z - 2.4 {
                    // Beach: dry sand back from the water, darker wet sand on
                    // the shoreline row where the surf reaches.
                    let sand = if quad_center_z >= shore_z - 1.4 {
                        [0.42, 0.39, 0.31, 1.0] // wet sand
                    } else {
                        [0.78, 0.71, 0.54, 1.0] // dry sand
                    };
                    (sand, 0.0, 0.55, [0.04; 4])
                } else if dist_from_road < road_width {
                    // Cobblestone Highway
                    let tile_pattern =
                        ((quad_center_x * 2.0).floor() + (quad_center_z * 2.0).floor()) % 2.0
                            == 0.0;
                    let c = if tile_pattern {
                        [0.155, 0.18, 0.245, 1.0]
                    } else {
                        [0.13, 0.145, 0.20, 1.0]
                    };
                    (c, 0.0, 0.45, [0.0; 4])
                } else if dist_from_road < road_width + 0.6 {
                    // Stone Road Border
                    ([0.235, 0.205, 0.145, 1.0], 0.0, 0.5, [0.08; 4])
                } else {
                    // Lush Meadow with natural elevation roll. Base kept dim
                    // (quiet-ground contract, video-tools REPORT #1): lit
                    // luma must land in the low punctuation bands so the
                    // meadow reads as a carpet and lit actors/props pop.
                    // Hue identity (clarity study #4): the same luma with
                    // raised chroma — the reference carpet is dark AND
                    // saturated (deep navy/teal-greens), so bright/white
                    // actors separate from the ground by hue too.
                    //
                    // Clump-and-void (forest scene negative space, Videos
                    // 1/2/5/6): off-road floor is NOT a continuous sheet —
                    // a coherent world-space mask carves ~60% of quads into
                    // near-black void floor that resolves to Band 0 ' '
                    // until a torch/lens passes over, leaving discrete
                    // moss/foliage islands in the pinned compact color.
                    let clump = (quad_center_x * 0.42).sin() * (quad_center_z * 0.38).cos()
                        + (quad_center_x * 0.85 + quad_center_z * 0.75).sin() * 0.45;
                    let grass = if clump > 0.12 {
                        [0.012, 0.072, 0.028, 1.0] // pinned quiet-ground island
                    } else {
                        [0.002, 0.011, 0.004, 1.0] // inter-clump void floor
                    };
                    (
                        grass,
                        3.0,
                        0.75,
                        [
                            meadow_h(x, z),
                            meadow_h(x, z + step),
                            meadow_h(x + step, z + step),
                            meadow_h(x + step, z),
                        ],
                    )
                };

                self.add_quad(
                    [x, heights[0], z],
                    [x, heights[1], z + step],
                    [x + step, heights[2], z + step],
                    [x + step, heights[3], z],
                    [0.0, 1.0, 0.0],
                    color,
                    mat,
                    rough,
                );

                z += step;
            }
            x += step;
        }
    }

    /// Volumetric 3D Weather Particles (Falling Rain, Splashes, Snowflakes)
    pub fn add_weather_particles(
        &mut self,
        center_x: f32,
        center_z: f32,
        time: f32,
        weather_mode: u32,
    ) {
        // Quiet weather ink (mirrors the instanced path above): dim streaks
        // so rain stays a subtle accent over the near-black carpet.
        let rain_color = [0.34, 0.44, 0.62, 0.75];
        let num_particles = 180;

        for i in 0..num_particles {
            let fi = i as f32;
            let seed_x = (fi * 17.31 + time * 1.5).sin() * 22.0;
            let seed_z = (fi * 29.43 + time * 0.8).cos() * 22.0;

            let px = center_x + seed_x;
            let pz = center_z + seed_z;

            match weather_mode {
                // Rain: angled vertical streaks falling rapidly
                0 | 2 => {
                    let speed = 14.0;
                    let py = 12.0 - ((time * speed + fi * 0.8) % 12.0);
                    // Rain streak box
                    self.add_box([px, py, pz], [0.04, 0.65, 0.04], rain_color, 5.0, 0.1);
                    // Ground splash ring when hitting surface
                    if py < 0.6 {
                        let splash_size = (0.6 - py) * 1.2;
                        self.add_quad(
                            [px - splash_size, 0.02, pz - splash_size],
                            [px - splash_size, 0.02, pz + splash_size],
                            [px + splash_size, 0.02, pz + splash_size],
                            [px + splash_size, 0.02, pz - splash_size],
                            [0.0, 1.0, 0.0],
                            [0.7, 0.85, 1.0, 0.4],
                            5.0,
                            0.05,
                        );
                    }
                }
                // Snow: gently drifting flakes
                1 => {
                    let speed = 2.5;
                    let py = 10.0 - ((time * speed + fi * 0.4) % 10.0);
                    let drift_x = px + (time * 2.0 + fi).sin() * 0.4;
                    self.add_box(
                        [drift_x, py, pz],
                        [0.12, 0.12, 0.12],
                        [0.55, 0.60, 0.70, 0.8],
                        5.0,
                        0.2,
                    );
                }
                _ => {}
            }
        }
    }

    /// Overgrown ancient altar without pillars (pillars are instanced separately)
    pub fn add_ruin_altar(&mut self, pos: [f32; 3]) {
        let (x, y, z) = (pos[0], pos[1], pos[2]);
        let stone = [0.52, 0.54, 0.58, 1.0];
        let moss_stone = [0.38, 0.48, 0.38, 1.0];

        // Fallen / broken architrave
        self.add_box([x - 0.2, y + 2.45, z], [2.20, 0.45, 0.75], stone, 0.0, 0.6);
        // Overgrown altar pedestal
        self.add_box(
            [x, y + 0.35, z + 0.8],
            [1.10, 0.70, 0.90],
            moss_stone,
            0.0,
            0.8,
        );
    }

    /// Pre-bakes a high-resolution pine tree prototype mesh centered at origin for GPU hardware instancing.
    pub fn create_pine_tree_prototype() -> Self {
        let mut mb = Self::new();
        let trunk_color = [0.28, 0.155, 0.055, 1.0];
        let trunk_dark = [0.185, 0.095, 0.035, 1.0];
        let pine_green_dark = [0.03, 0.135, 0.05, 1.0];
        let pine_green = [0.05, 0.165, 0.065, 1.0];
        let pine_green_light = [0.07, 0.205, 0.08, 1.0];

        // --- Sculpted Timber Trunk & Buttressed Root Flares ---
        // Main vertical trunk column rising from ground up into the upper canopy (material 7.0: Wood/Timber)
        mb.add_box([0.0, 2.70, 0.0], [0.70, 5.40, 0.70], trunk_color, 7.0, 0.7);
        // Vertical bark contour / ridge slabs for depth and striations
        mb.add_box([0.0, 2.50, 0.0], [0.78, 5.00, 0.36], trunk_dark, 7.0, 0.8);
        mb.add_box([0.0, 2.50, 0.0], [0.36, 5.00, 0.78], trunk_dark, 7.0, 0.8);

        // Buttress root collar at ground level (firmly anchored into terrain)
        mb.add_box([0.0, 0.24, 0.0], [1.25, 0.48, 1.25], trunk_dark, 7.0, 0.8);
        // Surface roots spreading into the soil, extending forward towards camera (+Z)
        mb.add_box([0.0, 0.14, 0.2], [0.50, 0.28, 1.80], trunk_color, 7.0, 0.85);
        mb.add_box([0.0, 0.14, 0.0], [1.80, 0.28, 0.50], trunk_color, 7.0, 0.85);

        // --- Elevated Multi-Tier Foliage Canopy ---
        // Lowest Tier (elevated to y = 3.60 so trunk is prominently visible underneath)
        mb.add_cone([0.0, 3.60, 0.0], 1.70, 1.80, 8, pine_green_dark, 3.0, 0.6);
        // Middle Tier
        mb.add_cone([0.0, 4.80, 0.0], 1.35, 1.70, 8, pine_green, 3.0, 0.6);
        // Upper Tier
        mb.add_cone([0.0, 6.00, 0.0], 0.95, 1.50, 8, pine_green, 3.0, 0.6);
        // Spire / Crown Apex
        mb.add_cone([0.0, 7.10, 0.0], 0.55, 1.30, 8, pine_green_light, 3.0, 0.6);
        mb
    }

    /// Pre-bakes a unit particle prototype mesh for GPU hardware instanced rain and snow.
    pub fn create_weather_particle_prototype() -> Self {
        let mut mb = Self::new();
        mb.add_box(
            [0.0, 0.0, 0.0],
            [1.0, 1.0, 1.0],
            [1.0, 1.0, 1.0, 1.0],
            5.0,
            0.1,
        );
        mb
    }

    /// Pre-bakes a ruin pillar prototype mesh centered at origin for GPU hardware instancing.
    pub fn create_ruin_pillar_prototype() -> Self {
        let mut mb = Self::new();
        let stone = [0.52, 0.54, 0.58, 1.0];
        mb.add_box([0.0, 1.25, 0.0], [0.65, 2.50, 0.65], stone, 0.0, 0.6);
        mb
    }

    /// Parses a Wavefront `.obj` text string and appends all triangles to the mesh buffer.
    /// Transforms all vertices by `transform` and assigns `default_color`, `material_id`, and `roughness`.
    /// Supports vertex positions (v), vertex normals (vn), and face indices (f v, f v//vn, f v/vt/vn).
    pub fn add_obj(
        &mut self,
        obj_content: &str,
        transform: Mat4,
        default_color: [f32; 4],
        material_id: f32,
        roughness: f32,
    ) -> Result<usize, String> {
        let mut positions = Vec::new();
        let mut normals = Vec::new();
        let mut triangles_added = 0;

        for line in obj_content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            let mut parts = line.split_whitespace();
            let prefix = match parts.next() {
                Some(p) => p,
                None => continue,
            };

            match prefix {
                "v" => {
                    let x: f32 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0.0);
                    let y: f32 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0.0);
                    let z: f32 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0.0);
                    positions.push(Vec3::new(x, y, z));
                }
                "vn" => {
                    let x: f32 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0.0);
                    let y: f32 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0.0);
                    let z: f32 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(1.0);
                    normals.push(Vec3::new(x, y, z).normalize_or_zero());
                }
                "f" => {
                    let face_vertices: Vec<&str> = parts.collect();
                    if face_vertices.len() < 3 {
                        continue;
                    }

                    let parse_corner = |token: &str| -> (usize, Option<usize>) {
                        let mut sub = token.split('/');
                        let v_idx = sub.next().and_then(|s| s.parse::<i32>().ok()).unwrap_or(1);
                        let _vt = sub.next();
                        let vn_idx = sub.next().and_then(|s| s.parse::<i32>().ok());

                        let pos_idx = if v_idx > 0 {
                            (v_idx as usize).saturating_sub(1)
                        } else {
                            (positions.len() as i32 + v_idx).max(0) as usize
                        };

                        let norm_idx = vn_idx.map(|vn| {
                            if vn > 0 {
                                (vn as usize).saturating_sub(1)
                            } else {
                                (normals.len() as i32 + vn).max(0) as usize
                            }
                        });

                        (pos_idx, norm_idx)
                    };

                    let corners: Vec<(usize, Option<usize>)> =
                        face_vertices.iter().map(|&tok| parse_corner(tok)).collect();

                    // Triangulate n-gon using triangle fan
                    for i in 1..(corners.len() - 1) {
                        let c0 = corners[0];
                        let c1 = corners[i];
                        let c2 = corners[i + 1];

                        let p0 = *positions.get(c0.0).unwrap_or(&Vec3::ZERO);
                        let p1 = *positions.get(c1.0).unwrap_or(&Vec3::ZERO);
                        let p2 = *positions.get(c2.0).unwrap_or(&Vec3::ZERO);

                        let default_face_normal = (p1 - p0).cross(p2 - p0).normalize_or_zero();

                        let n0 =
                            c0.1.and_then(|idx| normals.get(idx).copied())
                                .unwrap_or(default_face_normal);
                        let n1 =
                            c1.1.and_then(|idx| normals.get(idx).copied())
                                .unwrap_or(default_face_normal);
                        let n2 =
                            c2.1.and_then(|idx| normals.get(idx).copied())
                                .unwrap_or(default_face_normal);

                        let tp0 = (transform * p0.extend(1.0)).truncate();
                        let tp1 = (transform * p1.extend(1.0)).truncate();
                        let tp2 = (transform * p2.extend(1.0)).truncate();

                        let tn0 = (transform * n0.extend(0.0)).truncate().normalize_or_zero();
                        let tn1 = (transform * n1.extend(0.0)).truncate().normalize_or_zero();
                        let tn2 = (transform * n2.extend(0.0)).truncate().normalize_or_zero();

                        let base = self.vertices.len() as u32;
                        self.vertices.push(
                            Vertex::new(
                                tp0.into(),
                                tn0.into(),
                                default_color,
                                material_id,
                                roughness,
                            )
                            .with_entity_context(self.entity_id, self.entity_class),
                        );
                        self.vertices.push(
                            Vertex::new(
                                tp1.into(),
                                tn1.into(),
                                default_color,
                                material_id,
                                roughness,
                            )
                            .with_entity_context(self.entity_id, self.entity_class),
                        );
                        self.vertices.push(
                            Vertex::new(
                                tp2.into(),
                                tn2.into(),
                                default_color,
                                material_id,
                                roughness,
                            )
                            .with_entity_context(self.entity_id, self.entity_class),
                        );

                        self.indices.extend_from_slice(&[base, base + 1, base + 2]);
                        triangles_added += 1;
                    }
                }
                _ => {}
            }
        }

        Ok(triangles_added)
    }

    /// Loads a Wavefront `.obj` file string directly into a new `MeshBuffer`.
    pub fn load_obj(
        obj_content: &str,
        default_color: [f32; 4],
        material_id: f32,
        roughness: f32,
    ) -> Result<Self, String> {
        let mut mb = Self::new();
        mb.add_obj(
            obj_content,
            Mat4::IDENTITY,
            default_color,
            material_id,
            roughness,
        )?;
        Ok(mb)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meadow_quads_share_edge_heights_exactly() {
        // Regression: per-quad flat heights left a cliff at every seam,
        // and each seam rendered as a full-width dash rule at glancing
        // angles. Corner-sampled heights must agree bit-exactly.
        let mut mb = MeshBuffer::new();
        mb.add_terrain(0.0, 0.0, 20.0);
        assert!(!mb.vertices.is_empty());
        let mut heights = std::collections::HashMap::new();
        for v in &mb.vertices {
            // Pure-meadow interior only: road/curb edges are real steps.
            if v.position[0].abs() > 4.5 {
                heights
                    .entry((v.position[0].to_bits(), v.position[2].to_bits()))
                    .or_insert_with(Vec::new)
                    .push(v.position[1]);
            }
        }
        assert!(!heights.is_empty());
        for (pos, ys) in &heights {
            let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
            for y in ys {
                lo = lo.min(*y);
                hi = hi.max(*y);
            }
            assert_eq!(lo, hi, "cliff at corner {pos:?}: {ys:?}");
        }
    }

    #[test]
    fn meadow_clumps_carve_dark_void_islands() {
        // Forest negative space (Videos 1/2/5/6): the off-road floor is
        // discrete moss/foliage islands between near-black void tiles, not
        // a continuous meadow sheet. The pinned quiet-ground island color
        // still exists (g <= 0.15 bound), and the void floor must stay
        // dark enough to resolve Band 0 ' ' when unlit.
        let mut mb = MeshBuffer::new();
        mb.add_terrain(0.0, 0.0, 24.0);
        let mut island = false;
        let mut void_tile = false;
        for v in &mb.vertices {
            if v.material_id < 2.8 || v.material_id > 3.2 {
                continue;
            }
            let (r, g) = (v.color[0], v.color[1]);
            assert!(
                g <= 0.15,
                "meadow vertex brightened past the quiet-ground bound: {:?}",
                v.color
            );
            if (r - 0.012).abs() < 1e-4 && (g - 0.072).abs() < 1e-4 {
                island = true;
            }
            if (r - 0.002).abs() < 1e-4 && (g - 0.011).abs() < 1e-4 {
                assert!(
                    (v.color[2] - 0.004).abs() < 1e-4,
                    "void floor must keep its near-black blue-green tint"
                );
                void_tile = true;
            }
        }
        assert!(island, "pinned moss islands must remain in the clump mask");
        assert!(
            void_tile,
            "void tiles must carve negative space between clumps"
        );
    }

    #[test]
    fn zero_entity_context_maps_to_metadata_zero() {
        // The "no entity" stamp (terrain, props, water) must stay exactly
        // zero: the 1-based id wrap previously folded entity 0 forward to
        // 1, which lit the whole landscape through the character-shading
        // gate (`entity_id > 0.5` in the fragment pass).
        let mut mb = MeshBuffer::new();
        mb.set_entity_context(0, 0);
        assert_eq!(mb.entity_id, 0, "reserved id 0 must map to metadata 0");

        mb.add_box(
            [0.0, 0.5, 0.0],
            [1.0, 1.0, 1.0],
            [1.0, 1.0, 1.0, 1.0],
            1.0,
            0.2,
        );
        assert!(
            mb.vertices.iter().all(|v| v.entity_id == 0.0),
            "terrain stamped at context (0, 0) must carry entity_id 0"
        );

        // Real 1-based entities keep their identity under the wrap.
        mb.set_entity_context(1, 1);
        assert_eq!(mb.entity_id, 1);
        mb.set_entity_context(42, 2);
        assert_eq!(mb.entity_id, 42);
        mb.set_entity_context(256, 2);
        assert_eq!(mb.entity_id, 1, "ids wrap modulo 255 into 1..=255");
    }

    #[test]
    fn southern_ocean_starts_exactly_at_the_world_boundary() {
        // The ocean band keys off the shared walkable world bounds, the
        // same WORLD_BOUNDS.max_y the server's collision uses: mesh
        // z = world y * 0.8 (the renderer's uniform world-to-scene scale).
        // South of it: water (material 6, one flat plane below the land);
        // the boundary rows: dry sand then a wet shoreline strip; north:
        // untouched meadow. The terrain grid snaps to 1.5-steps, so the
        // water's first vertex row and the land's last vertex row must
        // share the boundary edge bit-exactly (no gap, no overlap).
        let shore = WORLD_BOUNDS.max_y as f32 * 0.8;
        let mut mb = MeshBuffer::new();
        mb.add_terrain(0.0, shore - 5.0, 12.0);

        let is_water = |v: &Vertex| v.material_id >= 5.8 && v.material_id <= 6.2;
        let mut water_min_z = f32::INFINITY;
        let mut water_max_z = f32::NEG_INFINITY;
        let mut land_max_z = f32::NEG_INFINITY;
        let mut meadow_found = false;
        let mut wet_sand_found = false;
        let mut dry_sand_found = false;
        for v in &mb.vertices {
            if is_water(v) {
                water_min_z = water_min_z.min(v.position[2]);
                water_max_z = water_max_z.max(v.position[2]);
                assert!(
                    (v.position[1] + 0.12).abs() < 1e-5,
                    "water must be one flat plane sunk below the land: {:?}",
                    v.position
                );
            } else {
                land_max_z = land_max_z.max(v.position[2]);
                let (r, g, b) = (v.color[0], v.color[1], v.color[2]);
                if (r - 0.012).abs() < 1e-4 && (g - 0.072).abs() < 1e-4 {
                    meadow_found = true;
                }
                // Quiet-ground contract (video-tools REPORT #1): meadow
                // (material 3) base must stay dim so its cells resolve into
                // the low punctuation bands and lit actors/props pop by
                // contrast. Sand/road (material 0) keep their own palettes.
                // Hue identity (clarity study #4): same luma, raised chroma.
                if v.material_id == 3.0 && g > 0.15 {
                    panic!(
                        "meadow base brightened past the quiet-ground bound: {:?}",
                        v.color
                    );
                }
                if (r - 0.42).abs() < 1e-4 && (g - 0.39).abs() < 1e-4 && (b - 0.31).abs() < 1e-4 {
                    assert!(
                        v.position[2] >= shore - 1.5,
                        "wet sand must hug the shoreline"
                    );
                    wet_sand_found = true;
                }
                if (r - 0.78).abs() < 1e-4 && (g - 0.71).abs() < 1e-4 {
                    dry_sand_found = true;
                }
            }
        }
        assert!(
            water_exists_on(&mb) && water_max_z > water_min_z,
            "open water must exist"
        );
        assert!(meadow_found, "meadow north of the beach must be untouched");
        assert!(
            dry_sand_found && wet_sand_found,
            "beach must have dry and wet sand rows"
        );
        assert!(
            water_min_z > shore && water_min_z < shore + 1.0,
            "water must start at the first quad row south of the shoreline: {water_min_z}"
        );
        assert!(
            (land_max_z - water_min_z).abs() < 1e-5,
            "land and water must share the shoreline edge bit-exactly: land {land_max_z} vs water {water_min_z}"
        );
    }

    fn water_exists_on(mb: &MeshBuffer) -> bool {
        mb.vertices
            .iter()
            .any(|v| v.material_id >= 5.8 && v.material_id <= 6.2)
    }

    #[test]
    fn humanoid_facing_rotates_head_and_body_about_the_foot_anchor() {
        // The player model must turn toward the orbit camera: add_humanoid
        // authors its part geometry facing +Z (the yaw-0 camera eye), and
        // facing_angle rotates head + body + limbs as one unit about the
        // foot anchor. A quarter-turn maps every (x, z) to (z, -x).
        let mut front = MeshBuffer::new();
        front.add_humanoid([2.0, 0.0, -3.0], 0.0, 0.35, 0.0);
        let count = front.vertices.len();
        assert!(count > 0, "humanoid must generate geometry");

        let quarter = std::f32::consts::FRAC_PI_2;
        let mut turned = MeshBuffer::new();
        turned.add_humanoid([2.0, 0.0, -3.0], quarter, 0.35, 0.0);
        assert_eq!(turned.vertices.len(), count);

        let (sin, cos) = quarter.sin_cos();
        for (a, b) in front.vertices.iter().zip(turned.vertices.iter()) {
            let (px, pz) = (a.position[0] - 2.0, a.position[2] - -3.0);
            let expect_x = px * cos + pz * sin + 2.0;
            let expect_z = -px * sin + pz * cos - 3.0;
            assert!(
                (b.position[0] - expect_x).abs() < 1e-4 && (b.position[2] - expect_z).abs() < 1e-4,
                "vertex xz not rotated: {a:?} vs {b:?}"
            );
            assert_eq!(
                a.position[1], b.position[1],
                "rotation must preserve height"
            );
            let (nx, nz) = (a.normal[0], a.normal[2]);
            assert!(
                (b.normal[0] - (nx * cos + nz * sin)).abs() < 1e-4
                    && (b.normal[2] - (-nx * sin + nz * cos)).abs() < 1e-4,
                "normal xz not rotated: {a:?} vs {b:?}"
            );
        }

        // Zero facing is a true no-op bit-for-bit (delta rows never idle-jitter).
        let mut again = MeshBuffer::new();
        again.add_humanoid([2.0, 0.0, -3.0], 0.0, 0.35, 0.0);
        assert_eq!(again.vertices.len(), count);
        for (a, b) in front.vertices.iter().zip(again.vertices.iter()) {
            assert_eq!(a.position, b.position);
            assert_eq!(a.normal, b.normal);
        }
    }
}
