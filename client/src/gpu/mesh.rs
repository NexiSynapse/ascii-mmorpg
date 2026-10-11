//! 3D Meshes, procedural geometry primitives, and volumetric scene generators.

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
        }
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
        material_params: [-1.0, -1.0, 0.0, 0.0],
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
            material_params: [material_override, roughness_override, 0.0, 0.0],
        }
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
        let rain_color = [0.65, 0.82, 1.0, 0.85];
        let snow_color = [0.95, 0.98, 1.0, 0.9];
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
}

impl MeshBuffer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear(&mut self) {
        self.vertices.clear();
        self.indices.clear();
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
        self.vertices
            .push(Vertex::new(p0, normal, color, material_id, roughness));
        self.vertices
            .push(Vertex::new(p1, normal, color, material_id, roughness));
        self.vertices
            .push(Vertex::new(p2, normal, color, material_id, roughness));
        self.vertices
            .push(Vertex::new(p3, normal, color, material_id, roughness));

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
            self.vertices
                .push(Vertex::new(p1, normal, color, material_id, roughness));
            self.vertices
                .push(Vertex::new(p2, normal, color, material_id, roughness));
            self.vertices
                .push(Vertex::new(apex, normal, color, material_id, roughness));

            self.indices
                .extend_from_slice(&[base_idx, base_idx + 1, base_idx + 2]);
        }
    }

    /// Procedural 3D Humanoid Warrior with articulated joints (Path C).
    pub fn add_humanoid(
        &mut self,
        pos: [f32; 3],
        _facing_angle: f32,
        walk_phase: f32,
        attack_phase: f32,
    ) {
        let (x, y, z) = (pos[0], pos[1], pos[2]);
        let armor_color = [0.82, 0.86, 0.94, 1.0]; // Bright gleaming steel
        let gold_trim = [0.95, 0.78, 0.28, 1.0]; // Polished gilded inlay
        let visor_glow = [0.30, 0.85, 1.0, 1.0]; // Cyan enchanted visor
        let cape_red = [0.65, 0.16, 0.22, 1.0]; // Deep crimson cloak
        let leather = [0.35, 0.24, 0.18, 1.0];

        // Head & Greathelm
        self.add_box([x, y + 1.75, z], [0.45, 0.45, 0.45], armor_color, 1.0, 0.2);
        // Visor slit
        self.add_box(
            [x, y + 1.76, z + 0.23],
            [0.32, 0.08, 0.05],
            visor_glow,
            4.0,
            0.1,
        );
        // Plume / Helm Crest
        self.add_box(
            [x, y + 2.05, z - 0.05],
            [0.12, 0.22, 0.35],
            gold_trim,
            1.0,
            0.3,
        );

        // Cuirass (Chestplate)
        self.add_box([x, y + 1.25, z], [0.65, 0.65, 0.40], armor_color, 1.0, 0.2);
        self.add_box(
            [x, y + 1.25, z + 0.21],
            [0.40, 0.50, 0.04],
            gold_trim,
            1.0,
            0.2,
        );

        // Pauldrons (Spike Shoulder Guards)
        self.add_box(
            [x - 0.45, y + 1.45, z],
            [0.32, 0.25, 0.35],
            armor_color,
            1.0,
            0.2,
        );
        self.add_box(
            [x + 0.45, y + 1.45, z],
            [0.32, 0.25, 0.35],
            armor_color,
            1.0,
            0.2,
        );

        // Flowing Cape (Back of Armor, gently swaying)
        let cape_sway = (walk_phase * 1.5).sin() * 0.15;
        self.add_box(
            [x, y + 0.95, z - 0.25 + cape_sway],
            [0.55, 0.85, 0.08],
            cape_red,
            2.0,
            0.6,
        );

        // Leg articulation with walking cycle
        let leg_swing = (walk_phase).sin() * 0.3;
        self.add_box(
            [x - 0.18, y + 0.55, z + leg_swing],
            [0.22, 0.65, 0.24],
            armor_color,
            1.0,
            0.3,
        );
        self.add_box(
            [x + 0.18, y + 0.55, z - leg_swing],
            [0.22, 0.65, 0.24],
            armor_color,
            1.0,
            0.3,
        );
        // Sabatons (Boots)
        self.add_box(
            [x - 0.18, y + 0.12, z + leg_swing + 0.06],
            [0.24, 0.24, 0.36],
            leather,
            2.0,
            0.4,
        );
        self.add_box(
            [x + 0.18, y + 0.12, z - leg_swing + 0.06],
            [0.24, 0.24, 0.36],
            leather,
            2.0,
            0.4,
        );

        // Left Arm & Kite Shield
        self.add_box(
            [x - 0.48, y + 1.05, z],
            [0.20, 0.55, 0.20],
            armor_color,
            1.0,
            0.3,
        );
        self.add_box(
            [x - 0.55, y + 1.05, z + 0.25],
            [0.12, 0.70, 0.45],
            gold_trim,
            1.0,
            0.2,
        );

        // Right Arm & Broadsword (swings during attack)
        let arm_swing = if attack_phase > 0.0 {
            (attack_phase * PI).sin() * 0.8
        } else {
            -(walk_phase).sin() * 0.25
        };
        self.add_box(
            [x + 0.48, y + 1.05, z + arm_swing],
            [0.20, 0.55, 0.20],
            armor_color,
            1.0,
            0.3,
        );
        // Sword Blade & Crossguard
        self.add_box(
            [x + 0.50, y + 1.35, z + arm_swing + 0.40],
            [0.06, 1.10, 0.16],
            [0.95, 0.98, 1.0, 1.0],
            1.0,
            0.05,
        );
        self.add_box(
            [x + 0.50, y + 0.82, z + arm_swing + 0.20],
            [0.35, 0.08, 0.10],
            gold_trim,
            1.0,
            0.2,
        );
    }

    /// Procedural 3D Mossling Forest Lurker
    pub fn add_mossling(&mut self, pos: [f32; 3], anim_phase: f32) {
        let (x, y, z) = (pos[0], pos[1], pos[2]);
        let breathe = (anim_phase * 2.0).sin() * 0.05;
        let moss_green = [0.25, 0.55, 0.22, 1.0];
        let moss_dark = [0.15, 0.38, 0.14, 1.0];
        let eye_glow = [1.0, 0.25, 0.15, 1.0];

        // Bulky mossy body
        self.add_box(
            [x, y + 0.65 + breathe, z],
            [0.95, 0.85, 0.75],
            moss_green,
            3.0,
            0.7,
        );
        // Lower legs / quadruped base
        self.add_box(
            [x - 0.35, y + 0.20, z - 0.20],
            [0.28, 0.40, 0.28],
            moss_dark,
            3.0,
            0.8,
        );
        self.add_box(
            [x + 0.35, y + 0.20, z - 0.20],
            [0.28, 0.40, 0.28],
            moss_dark,
            3.0,
            0.8,
        );
        self.add_box(
            [x - 0.35, y + 0.20, z + 0.20],
            [0.28, 0.40, 0.28],
            moss_dark,
            3.0,
            0.8,
        );
        self.add_box(
            [x + 0.35, y + 0.20, z + 0.20],
            [0.28, 0.40, 0.28],
            moss_dark,
            3.0,
            0.8,
        );

        // Glowing predatory eyes
        self.add_box(
            [x - 0.22, y + 0.85 + breathe, z + 0.38],
            [0.10, 0.08, 0.08],
            eye_glow,
            4.0,
            0.1,
        );
        self.add_box(
            [x + 0.22, y + 0.85 + breathe, z + 0.38],
            [0.10, 0.08, 0.08],
            eye_glow,
            4.0,
            0.1,
        );

        // Spikes / fungal horns
        self.add_cone(
            [x - 0.28, y + 1.05 + breathe, z],
            0.14,
            0.45,
            6,
            moss_dark,
            3.0,
            0.6,
        );
        self.add_cone(
            [x + 0.28, y + 1.05 + breathe, z],
            0.14,
            0.45,
            6,
            moss_dark,
            3.0,
            0.6,
        );
    }

    /// Procedural 3D Herbalist NPC
    pub fn add_herbalist(&mut self, pos: [f32; 3], anim_phase: f32) {
        let (x, y, z) = (pos[0], pos[1], pos[2]);
        let robe_color = [0.22, 0.38, 0.58, 1.0]; // Scholar blue
        let cowl_color = [0.16, 0.26, 0.42, 1.0];
        let wood_color = [0.42, 0.28, 0.16, 1.0];
        let lantern_amber = [1.0, 0.78, 0.32, 1.0];

        // Robe body
        self.add_box([x, y + 0.85, z], [0.55, 1.30, 0.45], robe_color, 2.0, 0.6);
        // Hood / Cowl
        self.add_box([x, y + 1.65, z], [0.48, 0.42, 0.48], cowl_color, 2.0, 0.6);

        // Staff
        self.add_box(
            [x + 0.42, y + 0.95, z + 0.15],
            [0.08, 1.90, 0.08],
            wood_color,
            3.0,
            0.5,
        );
        // Staff Lantern (Emissive Glow)
        let flicker = (anim_phase * 6.0).sin() * 0.08;
        self.add_box(
            [x + 0.42, y + 1.85, z + 0.15],
            [0.22 + flicker, 0.28 + flicker, 0.22 + flicker],
            lantern_amber,
            4.0,
            0.1,
        );
    }

    /// Procedural 3D Pine Tree
    pub fn add_pine_tree(&mut self, pos: [f32; 3], wind_sway: f32) {
        let (x, y, z) = (pos[0], pos[1], pos[2]);
        let trunk_color = [0.38, 0.24, 0.14, 1.0];
        let pine_green = [0.14, 0.34, 0.18, 1.0];

        // Trunk
        self.add_box([x, y + 1.25, z], [0.38, 2.50, 0.38], trunk_color, 3.0, 0.7);

        // 3 Tiers of conical foliage
        let sway = wind_sway * 0.08;
        self.add_cone(
            [x + sway * 0.5, y + 1.6, z],
            1.65,
            1.6,
            8,
            pine_green,
            3.0,
            0.6,
        );
        self.add_cone(
            [x + sway * 1.0, y + 2.7, z],
            1.30,
            1.5,
            8,
            pine_green,
            3.0,
            0.6,
        );
        self.add_cone(
            [x + sway * 1.5, y + 3.8, z],
            0.90,
            1.4,
            8,
            pine_green,
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

    /// Procedural 3D Volumetric Terrain (Cobblestone Highway & Meadow)
    pub fn add_terrain(&mut self, center_x: f32, center_z: f32, radius: f32) {
        let step = 1.5;
        let road_width = 3.2;

        let x_min = center_x - radius;
        let x_max = center_x + radius;
        let z_min = center_z - radius;
        let z_max = center_z + radius;

        let mut x = x_min;
        while x < x_max {
            let mut z = z_min;
            while z < z_max {
                let dist_from_road = x.abs();
                let (color, mat, rough, y_offset) = if dist_from_road < road_width {
                    // Cobblestone Highway
                    let tile_pattern = ((x * 2.0).floor() + (z * 2.0).floor()) % 2.0 == 0.0;
                    let c = if tile_pattern {
                        [0.46, 0.48, 0.52, 1.0]
                    } else {
                        [0.38, 0.40, 0.44, 1.0]
                    };
                    (c, 0.0, 0.45, 0.0)
                } else if dist_from_road < road_width + 0.6 {
                    // Stone Road Border
                    ([0.55, 0.53, 0.48, 1.0], 0.0, 0.5, 0.08)
                } else {
                    // Lush Meadow with natural elevation roll
                    let hill = (x * 0.25).sin() * (z * 0.25).cos() * 0.15;
                    let grass = [0.18, 0.38, 0.19, 1.0];
                    (grass, 3.0, 0.75, hill)
                };

                self.add_quad(
                    [x, y_offset, z],
                    [x, y_offset, z + step],
                    [x + step, y_offset, z + step],
                    [x + step, y_offset, z],
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
        let rain_color = [0.65, 0.82, 1.0, 0.85];
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
                        [0.95, 0.98, 1.0, 0.9],
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
        let trunk_color = [0.38, 0.24, 0.14, 1.0];
        let pine_green = [0.14, 0.34, 0.18, 1.0];

        // Trunk
        mb.add_box([0.0, 1.25, 0.0], [0.38, 2.50, 0.38], trunk_color, 3.0, 0.7);

        // 3 Tiers of conical foliage
        mb.add_cone([0.0, 1.6, 0.0], 1.65, 1.6, 8, pine_green, 3.0, 0.6);
        mb.add_cone([0.0, 2.7, 0.0], 1.30, 1.5, 8, pine_green, 3.0, 0.6);
        mb.add_cone([0.0, 3.8, 0.0], 0.90, 1.4, 8, pine_green, 3.0, 0.6);
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
}
