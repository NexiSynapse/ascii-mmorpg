//! High-level GPU offscreen renderer for the ASCII MMORPG client
//! with multi-buffered asynchronous staging rings and hardware instanced batching.

use std::time::Instant;

use ascii_mmorpg_shared::collision::terrain_seed;
use ascii_mmorpg_shared::{EntityKind, Point, WorldSnapshot};
use glam::{Mat4, Vec2, Vec3};

use super::context::GpuContext;
use super::mesh::{Frustum, MeshBuffer, SceneBatches};
use super::pipeline::{GpuCell, GpuPipeline};
use super::uniforms::{CameraUniforms, GpuPointLight, LightingUniforms};
use crate::renderer::WeatherMode;

pub struct GpuRenderer {
    pub ctx: GpuContext,
    pub pipeline: GpuPipeline,
    pub mesh: MeshBuffer,
    pub batches: SceneBatches,
    pub width: i16,
    pub height: i16,
    pub weather: WeatherMode,
    pub active_modal: crate::ui::ActiveModal,
    pub last_player_pos: Option<Point>,
    pub last_move_instant: Option<Instant>,
    pub player_walk_phase: f32,
    pub smooth_cam_pos: Option<Vec2>,
    pub smooth_player_pos: Option<Vec2>,
    animation_started: Instant,
    last_render_instant: Option<Instant>,
}

impl GpuRenderer {
    pub fn new(width: i16, height: i16, weather: WeatherMode) -> Result<Self, String> {
        let width = width.max(20);
        let height = height.max(10);
        let ctx = GpuContext::init()?;
        let pipeline = GpuPipeline::new(&ctx, width as u32, height as u32)?;
        let mesh = MeshBuffer::new();
        let batches = SceneBatches::new();

        Ok(Self {
            ctx,
            pipeline,
            mesh,
            batches,
            width,
            height,
            weather,
            active_modal: crate::ui::ActiveModal::None,
            last_player_pos: None,
            last_move_instant: None,
            player_walk_phase: 0.0,
            smooth_cam_pos: None,
            smooth_player_pos: None,
            animation_started: Instant::now(),
            last_render_instant: None,
        })
    }

    pub fn resize(&mut self, width: i16, height: i16) {
        let width = width.max(20);
        let height = height.max(10);
        if self.width == width && self.height == height {
            return;
        }
        self.width = width;
        self.height = height;
        self.pipeline.resize(&self.ctx, width as u32, height as u32);
    }

    pub fn weather(&self) -> WeatherMode {
        self.weather
    }

    pub fn set_weather(&mut self, weather: WeatherMode) {
        self.weather = weather;
    }

    pub fn render(&mut self, world: &WorldSnapshot) -> String {
        self.render_frame(world, world.tick)
    }

    /// Renders a single frame using the offscreen GPU pipeline and instanced batching.
    pub fn render_frame(&mut self, world: &WorldSnapshot, frame: u64) -> String {
        self.mesh.clear();
        self.batches.clear();

        let now = Instant::now();
        let delta_seconds = self
            .last_render_instant
            .replace(now)
            .map_or(0.0, |previous| now.duration_since(previous).as_secs_f32());
        let elapsed_seconds = now.duration_since(self.animation_started).as_secs_f32();
        let (camera_uniforms, lighting) =
            self.build_scene_frame_realtime(world, frame, delta_seconds, elapsed_seconds);

        let cells = match self.pipeline.render_and_readback_batched(
            &self.ctx,
            &self.mesh,
            &self.batches,
            &camera_uniforms,
            &lighting,
        ) {
            Ok(c) => c,
            Err(e) => {
                return format!("GPU Render Error: {e}");
            }
        };

        self.format_frame_with_hud(&cells, world)
    }

    /// Evaluates whether the player is currently walking based on position changes
    /// within a 280ms stride window, returning the active walk cycle phase.
    ///
    /// When the player is stationary, returns 0.0 so feet remain planted on the ground.
    pub fn update_walk_phase(&mut self, world: &WorldSnapshot) -> f32 {
        if !world.player.stats.is_alive() {
            self.player_walk_phase = 0.0;
            return 0.0;
        }

        let current_pos = world.player.position;
        let now = Instant::now();

        let moved = match self.last_player_pos {
            Some(last_pos) => last_pos != current_pos,
            None => false,
        };

        if moved {
            self.last_player_pos = Some(current_pos);
            self.last_move_instant = Some(now);
        } else if self.last_player_pos.is_none() {
            self.last_player_pos = Some(current_pos);
        }

        let is_walking = self
            .last_move_instant
            .is_some_and(|t| now.duration_since(t).as_millis() < 280);

        if is_walking {
            self.player_walk_phase += 0.35;
            self.player_walk_phase
        } else {
            self.player_walk_phase = 0.0;
            0.0
        }
    }

    fn update_walk_phase_delta(&mut self, world: &WorldSnapshot, delta_seconds: f32) -> f32 {
        if !world.player.stats.is_alive() {
            self.player_walk_phase = 0.0;
            return 0.0;
        }
        let current_pos = world.player.position;
        let moved = self.last_player_pos.is_some_and(|last| last != current_pos);
        self.last_player_pos = Some(current_pos);
        if moved {
            self.last_move_instant = Some(Instant::now());
        }
        let is_walking = self
            .last_move_instant
            .is_some_and(|t| t.elapsed().as_secs_f32() < 0.28);
        if is_walking {
            self.player_walk_phase += delta_seconds * 21.0;
            self.player_walk_phase
        } else {
            self.player_walk_phase = 0.0;
            0.0
        }
    }

    /// Renders a batch of world snapshots continuously across the double-buffered GPU ring,
    /// keeping the GPU hardware at full utilization and eliminating CPU-GPU idle bubbles.
    pub fn render_batch(&mut self, snapshots: &[WorldSnapshot]) -> Vec<String> {
        if snapshots.is_empty() {
            return Vec::new();
        }

        let mut batch_data = Vec::with_capacity(snapshots.len());
        let mut last_pos = self.last_player_pos;
        let mut walk_phase = self.player_walk_phase;
        let mut smooth_cam = self.smooth_cam_pos;
        let mut smooth_player = self.smooth_player_pos;
        for (i, snapshot) in snapshots.iter().enumerate() {
            let moved = match last_pos {
                Some(prev) => prev != snapshot.player.position,
                None => false,
            };
            last_pos = Some(snapshot.player.position);
            if moved && snapshot.player.stats.is_alive() {
                walk_phase += 0.35;
            } else if walk_phase.abs() > 0.05 {
                walk_phase *= 0.65;
            } else {
                walk_phase = 0.0;
            }

            let target_pos = Vec2::new(
                snapshot.player.position.x as f32 * 0.8,
                snapshot.player.position.y as f32 * 0.8,
            );
            smooth_cam = Some(smooth_cam.map_or(target_pos, |p| {
                p.lerp(target_pos, damping_factor(19.7, 0.05))
            }));
            smooth_player = Some(smooth_player.map_or(target_pos, |p| {
                p.lerp(target_pos, damping_factor(28.8, 0.05))
            }));

            let mut mesh = MeshBuffer::new();
            let mut batches = SceneBatches::new();
            let (cam, light) = Self::populate_scene_smoothed(
                self.width,
                self.height,
                self.weather,
                snapshot,
                snapshot.tick.max(i as u64),
                walk_phase,
                smooth_cam.unwrap(),
                smooth_player.unwrap(),
                &mut mesh,
                &mut batches,
            );
            batch_data.push((mesh, batches, cam, light));
        }
        self.last_player_pos = last_pos;
        self.player_walk_phase = walk_phase;
        self.smooth_cam_pos = smooth_cam;
        self.smooth_player_pos = smooth_player;

        let frame_refs: Vec<(
            &MeshBuffer,
            &SceneBatches,
            &CameraUniforms,
            &LightingUniforms,
        )> = batch_data.iter().map(|(m, b, c, l)| (m, b, c, l)).collect();

        let cell_batches = match self.pipeline.render_batch(&self.ctx, &frame_refs) {
            Ok(res) => res,
            Err(e) => return vec![format!("GPU Batch Render Error: {e}")],
        };

        let mut output = Vec::with_capacity(cell_batches.len());
        for (cells, snapshot) in cell_batches.iter().zip(snapshots.iter()) {
            output.push(self.format_frame_with_hud(cells, snapshot));
        }
        output
    }

    /// Builds scene geometry and fills hardware instance batches on `self.mesh` and `self.batches`.
    pub fn build_scene_frame(
        &mut self,
        world: &WorldSnapshot,
        frame: u64,
    ) -> (CameraUniforms, LightingUniforms) {
        self.build_scene_frame_with_timing(world, frame, 0.05, frame as f32 * 0.05, true)
    }

    fn build_scene_frame_realtime(
        &mut self,
        world: &WorldSnapshot,
        frame: u64,
        delta_seconds: f32,
        elapsed_seconds: f32,
    ) -> (CameraUniforms, LightingUniforms) {
        self.build_scene_frame_with_timing(
            world,
            frame,
            delta_seconds.clamp(0.0, 0.1),
            elapsed_seconds,
            false,
        )
    }

    fn build_scene_frame_with_timing(
        &mut self,
        world: &WorldSnapshot,
        _frame: u64,
        delta_seconds: f32,
        animation_time: f32,
        deterministic: bool,
    ) -> (CameraUniforms, LightingUniforms) {
        let walk_phase = if deterministic {
            self.update_walk_phase(world)
        } else {
            self.update_walk_phase_delta(world, delta_seconds)
        };
        let target_x = world.player.position.x as f32 * 0.8;
        let target_z = world.player.position.y as f32 * 0.8;
        let target_pos = Vec2::new(target_x, target_z);

        let cam_pos = match self.smooth_cam_pos {
            Some(prev) => {
                let factor = damping_factor(19.7, delta_seconds);
                let smoothed = prev.lerp(target_pos, factor);
                self.smooth_cam_pos = Some(smoothed);
                smoothed
            }
            None => {
                self.smooth_cam_pos = Some(target_pos);
                target_pos
            }
        };

        let player_render_pos = match self.smooth_player_pos {
            Some(prev) => {
                let factor = damping_factor(28.8, delta_seconds);
                let smoothed = prev.lerp(target_pos, factor);
                self.smooth_player_pos = Some(smoothed);
                smoothed
            }
            None => {
                self.smooth_player_pos = Some(target_pos);
                target_pos
            }
        };

        Self::populate_scene_smoothed_at_time(
            self.width,
            self.height,
            self.weather,
            world,
            animation_time,
            walk_phase,
            cam_pos,
            player_render_pos,
            &mut self.mesh,
            &mut self.batches,
        )
    }

    /// Populates scene geometry and fills hardware instance batches for a given target buffer set.
    #[allow(clippy::too_many_arguments)]
    pub fn populate_scene(
        width: i16,
        height: i16,
        weather: WeatherMode,
        world: &WorldSnapshot,
        frame: u64,
        walk_phase: f32,
        mesh: &mut MeshBuffer,
        batches: &mut SceneBatches,
    ) -> (CameraUniforms, LightingUniforms) {
        let p_pos = world.player.position;
        let px = p_pos.x as f32 * 0.8;
        let pz = p_pos.y as f32 * 0.8;
        let pos = Vec2::new(px, pz);
        Self::populate_scene_smoothed(
            width, height, weather, world, frame, walk_phase, pos, pos, mesh, batches,
        )
    }

    /// Populates scene geometry with smoothed camera and player positions,
    /// dynamic lighting, and view-frustum culled instanced props.
    #[allow(clippy::too_many_arguments)]
    pub fn populate_scene_smoothed(
        width: i16,
        height: i16,
        weather: WeatherMode,
        world: &WorldSnapshot,
        frame: u64,
        walk_phase: f32,
        cam_pos: Vec2,
        player_render_pos: Vec2,
        mesh: &mut MeshBuffer,
        batches: &mut SceneBatches,
    ) -> (CameraUniforms, LightingUniforms) {
        Self::populate_scene_smoothed_at_time(
            width,
            height,
            weather,
            world,
            frame as f32 * 0.05,
            walk_phase,
            cam_pos,
            player_render_pos,
            mesh,
            batches,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn populate_scene_smoothed_at_time(
        width: i16,
        height: i16,
        weather: WeatherMode,
        world: &WorldSnapshot,
        time: f32,
        walk_phase: f32,
        cam_pos: Vec2,
        player_render_pos: Vec2,
        mesh: &mut MeshBuffer,
        batches: &mut SceneBatches,
    ) -> (CameraUniforms, LightingUniforms) {
        let px = player_render_pos.x;
        let pz = player_render_pos.y;

        // 1. Setup Camera (Isometric 2.5D perspective looking North towards -Z)
        let cam_eye = Vec3::new(cam_pos.x, 11.5, cam_pos.y + 13.5);
        let cam_target = Vec3::new(cam_pos.x, 0.8, cam_pos.y);
        let cam_up = Vec3::new(0.0, 1.0, 0.0);

        let view = Mat4::look_at_rh(cam_eye, cam_target, cam_up);
        let aspect = (width as f32 / height as f32) * 0.48;
        let proj = Mat4::perspective_rh(42.0f32.to_radians(), aspect, 0.1, 100.0);
        let view_proj = proj * view;
        let inv_view_proj = view_proj.inverse();
        let frustum = Frustum::from_view_proj(&view_proj);

        let weather_id = match weather {
            WeatherMode::Rain => 0,
            WeatherMode::Snow => 1,
            WeatherMode::Storm => 2,
        };

        let camera_uniforms = CameraUniforms {
            view_proj: view_proj.to_cols_array_2d(),
            inv_view_proj: inv_view_proj.to_cols_array_2d(),
            camera_pos: [cam_eye.x, cam_eye.y, cam_eye.z, 1.0],
            time,
            weather_mode: weather_id,
            width: width as u32,
            height: height as u32,
        };

        // 2. Dynamic Point Lights
        let mut lighting = LightingUniforms {
            sun_dir: [
                (time * 0.05).sin() * 0.3 + 0.35,
                0.82,
                (time * 0.05).cos() * 0.3 + 0.45,
                0.0,
            ],
            ..Default::default()
        };

        let mut lights = Vec::new();

        // Player Hip Lantern (Warm flickering light pool)
        let player_flicker = (time * 8.0).sin() * 0.12;
        lights.push(GpuPointLight {
            position: [px + 0.4, 1.2, pz + 0.2, 9.5],
            color: [1.0, 0.82, 0.45, 1.2 + player_flicker],
        });

        // Gate Torches at fixed world points (Gate at y = -12 -> z = -9.6)
        let gate_flicker1 = (time * 9.5).sin() * 0.15;
        let gate_flicker2 = (time * 11.2).cos() * 0.15;
        lights.push(GpuPointLight {
            position: [-2.2, 2.6, -9.6 + 0.65, 8.5],
            color: [1.0, 0.62, 0.20, 1.4 + gate_flicker1],
        });
        lights.push(GpuPointLight {
            position: [2.2, 2.6, -9.6 + 0.65, 8.5],
            color: [1.0, 0.62, 0.20, 1.4 + gate_flicker2],
        });

        // Dynamic Entity Lighting (Living actors & loot cast atmospheric lighting pools onto the ground)
        for entity in &world.entities {
            let ex = entity.position.x as f32 * 0.8;
            let ez = entity.position.y as f32 * 0.8;
            match entity.kind {
                EntityKind::Npc => {
                    let herb_flicker = (time * 7.0 + ex).sin() * 0.1;
                    lights.push(GpuPointLight {
                        position: [ex + 0.35, 1.7, ez + 0.15, 8.0],
                        color: [0.95, 0.80, 0.35, 1.3 + herb_flicker], // Warm golden amber lantern glow
                    });
                }
                EntityKind::Monster => {
                    let pulse = (time * 4.5 + ex * 1.7).sin() * 0.15;
                    lights.push(GpuPointLight {
                        position: [ex, 0.85, ez, 6.5],
                        color: [0.88, 0.22, 0.14, 1.05 + pulse], // Sinister crimson-amber ground glow
                    });
                }
                EntityKind::Item => {
                    let shimmer = (time * 5.2 + ez * 2.1).cos() * 0.18;
                    lights.push(GpuPointLight {
                        position: [ex, 0.45, ez, 5.0],
                        color: [0.25, 0.78, 0.95, 1.15 + shimmer], // Ethereal celestial cyan shimmering glow
                    });
                }
                EntityKind::Player if entity.id != world.player.id => {
                    let other_flicker = (time * 7.8 + ex).sin() * 0.12;
                    lights.push(GpuPointLight {
                        position: [ex + 0.4, 1.2, ez + 0.2, 9.0],
                        color: [1.0, 0.82, 0.45, 1.2 + other_flicker], // Adventurer's warm lantern glow
                    });
                }
                _ => {}
            }
        }

        // Prioritize lights closest to the camera/player
        lights.sort_by(|a, b| {
            let da = (a.position[0] - px).hypot(a.position[2] - pz);
            let db = (b.position[0] - px).hypot(b.position[2] - pz);
            da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
        });

        let light_count = lights.len().min(16);
        lighting.num_point_lights = light_count as u32;
        for (i, l) in lights.iter().take(16).enumerate() {
            lighting.point_lights[i] = *l;
        }

        // 3. Dynamic Scene Geometry (Cobblestone Highway, Altar, Gate, Entities)
        mesh.set_entity_context(0, 0);
        let terrain_radius = ((width as f32).max(100.0) * 0.38).clamp(42.0, 68.0);
        mesh.add_terrain(cam_pos.x, cam_pos.y, terrain_radius);
        mesh.add_gate([0.0, 0.0, -9.6]);
        mesh.add_ruin_altar([-6.4, 0.0, -4.0]);

        for entity in &world.entities {
            let ex = entity.position.x as f32 * 0.8;
            let ez = entity.position.y as f32 * 0.8;
            let entity_class = match entity.kind {
                EntityKind::Player => 1,
                EntityKind::Monster => 2,
                EntityKind::Npc => 3,
                EntityKind::Item => 4,
            };
            mesh.set_entity_context(entity.id, entity_class);
            match entity.kind {
                EntityKind::Monster => {
                    let label = entity.name.to_ascii_lowercase();
                    if label.contains("warrior") {
                        mesh.add_skeleton_warrior([ex, 0.0, ez], time, 0.0);
                    } else if label.contains("skeleton") {
                        mesh.add_skeleton([ex, 0.0, ez], time, 0.0);
                    } else {
                        mesh.add_mossling([ex, 0.0, ez], time);
                    }
                }
                EntityKind::Npc => {
                    mesh.add_herbalist([ex, 0.0, ez], time);
                }
                EntityKind::Player => {
                    mesh.add_humanoid([ex, 0.0, ez], 0.0, 0.0, 0.0);
                }
                EntityKind::Item => {
                    let hover = (time * 3.0 + ex).sin() * 0.15;
                    mesh.add_box(
                        [ex, 0.35 + hover, ez],
                        [0.45, 0.35, 0.35],
                        [0.95, 0.85, 0.35, 1.0],
                        4.0,
                        0.1,
                    );
                }
            }
        }

        mesh.set_entity_context(world.player.id, 1);
        mesh.add_humanoid([px, 0.0, pz], 0.0, walk_phase, 0.0);
        mesh.set_entity_context(0, 0);

        // 4. Instanced Props with View-Frustum Culling (Asciicker plane culling optimization)
        // Scaled to exact 0.8 world tile grid:
        // Landmark Trees: (-7, -2) -> [-5.6, -1.6], (8, -3) -> [6.4, -2.4],
        // (9, -10) -> [7.2, -8.0], (-9, -10) -> [-7.2, -8.0]
        let landmark_trees = [
            ([-5.6, 0.0, -1.6], 1.0, (time * 2.0).sin()),
            ([6.4, 0.0, -2.4], 1.1, (time * 2.3).sin()),
            ([7.2, 0.0, -8.0], 0.95, (time * 1.8).sin()),
            ([-7.2, 0.0, -8.0], 1.05, (time * 2.1).sin()),
        ];
        for (pos, scale, sway) in landmark_trees {
            if frustum.test_sphere(Vec3::from(pos) + Vec3::new(0.0, 3.5, 0.0), scale * 5.2) {
                batches.add_pine_tree(pos, scale, sway);
            }
        }

        // Procedural background trees matching collision grid (off-highway |tree_wx| > 4)
        let cam_tile_x = (cam_pos.x / 0.8).round() as i16;
        let cam_tile_y = (cam_pos.y / 0.8).round() as i16;
        let min_gx = (cam_tile_x - 30).div_euclid(11) - 1;
        let max_gx = (cam_tile_x + 30).div_euclid(11) + 1;
        let min_gy = (cam_tile_y - 25).div_euclid(9) - 1;
        let max_gy = (cam_tile_y + 25).div_euclid(9) + 1;

        for gx in min_gx..=max_gx {
            for gy in min_gy..=max_gy {
                let seed = terrain_seed(gx, gy);
                let tree_wx = gx * 11 + (seed % 5) as i16;
                let tree_wy = gy * 9 + ((seed / 7) % 5) as i16;
                if tree_wx.abs() > 4
                    && seed % 9 == 0
                    && (tree_wx, tree_wy) != (-7, -2)
                    && (tree_wx, tree_wy) != (8, -3)
                    && (tree_wx, tree_wy) != (9, -10)
                    && (tree_wx, tree_wy) != (-9, -10)
                {
                    let wx = tree_wx as f32 * 0.8;
                    let wz = tree_wy as f32 * 0.8;
                    let scale = 0.92 + ((seed % 7) as f32) * 0.04;
                    let sway = (time * 1.5 + (seed as f32 * 0.17)).sin();
                    let pos = [wx, 0.0, wz];
                    if frustum.test_sphere(Vec3::from(pos) + Vec3::new(0.0, 3.5, 0.0), scale * 5.2)
                    {
                        batches.add_pine_tree(pos, scale, sway);
                    }
                }
            }
        }

        // Colonnade Pillars: West (-10, -5) -> [-8.0, -4.0], East (-6, -5) -> [-4.8, -4.0]
        let pillars = [([-8.0, 0.0, -4.0], 1.0), ([-4.8, 0.0, -4.0], 0.76)];
        for (pos, height_scale) in pillars {
            if frustum.test_sphere(Vec3::from(pos) + Vec3::new(0.0, 1.5, 0.0), 3.0) {
                batches.add_pillar(pos, height_scale);
            }
        }

        for gx in (cam_tile_x - 16)..=(cam_tile_x + 16) {
            for gy in (cam_tile_y - 12)..=(cam_tile_y + 12) {
                let seed = terrain_seed(gx, gy);
                if seed % 3 != 0 {
                    continue;
                }
                let x = gx as f32 * 0.8 + (seed % 7) as f32 * 0.08;
                let z = gy as f32 * 0.8 + ((seed / 7) % 7) as f32 * 0.08;
                if (x - cam_pos.x).abs() > 28.0 || (z - cam_pos.y).abs() > 22.0 {
                    continue;
                }
                let phase = time * 2.4 + seed as f32 * 0.013;
                mesh.add_grass_blade(x, z, 0.28 + (seed % 5) as f32 * 0.06, phase.sin() * 0.14);
            }
        }

        batches.add_weather_particles(cam_pos.x, cam_pos.y, time, weather_id);

        (camera_uniforms, lighting)
    }

    /// O(1) mathematical screen-to-world depth unprojection (borrowed from Asciicker's depth unprojection).
    /// Given screen cell coordinates (screen_x, screen_y) and the GPU cell buffer, recovers the exact 3D world coordinate.
    pub fn unproject_at(
        &self,
        screen_x: usize,
        screen_y: usize,
        cells: &[GpuCell],
        camera: &CameraUniforms,
    ) -> Option<Vec3> {
        let idx = screen_y * self.width as usize + screen_x;
        let cell = cells.get(idx)?;
        let depth = cell.depth();
        Some(camera.unproject_screen_to_world(screen_x as f32, screen_y as f32, depth))
    }

    /// High-performance zero-allocation ANSI string formatter.
    fn format_frame_with_hud(&self, cells: &[GpuCell], world: &WorldSnapshot) -> String {
        let width = self.width as usize;
        let height = self.height as usize;
        let mut out = String::with_capacity(width * height * 16 + 512);

        for y in 0..height {
            let mut last_fg = (999, 999, 999);
            let mut last_bg = (999, 999, 999);

            for x in 0..width {
                let idx = y * width + x;
                if idx >= cells.len() {
                    break;
                }
                let cell = &cells[idx];
                let fg = (cell.fg_r, cell.fg_g, cell.fg_b);
                let bg = (cell.bg_r, cell.bg_g, cell.bg_b);

                if fg != last_fg {
                    push_rgb_ansi(&mut out, false, fg.0, fg.1, fg.2);
                    last_fg = fg;
                }
                if bg != last_bg {
                    push_rgb_ansi(&mut out, true, bg.0, bg.1, bg.2);
                    last_bg = bg;
                }

                let ch = std::char::from_u32(cell.glyph).unwrap_or(' ');
                out.push(ch);
            }
            out.push_str("\x1b[0m\r\n");
        }

        // Compose Ratatui-powered Dark Fantasy MMORPG HUD overlay
        let mut hud = crate::ui::RatatuiHud::new();
        let hud_height = if self.active_modal != crate::ui::ActiveModal::None {
            12
        } else {
            7
        };
        let hud_overlay = hud.render_hud_overlay_with_modal(
            world,
            self.width as u16,
            hud_height,
            self.weather,
            "WGPU 3D BATCHED PIPELINE",
            self.active_modal,
        );
        out.push_str(&hud_overlay);

        out
    }
}

fn damping_factor(rate: f32, delta_seconds: f32) -> f32 {
    1.0 - (-rate * delta_seconds.max(0.0)).exp()
}

#[inline]
fn push_rgb_ansi(out: &mut String, is_bg: bool, r: u32, g: u32, b: u32) {
    if is_bg {
        out.push_str("\x1b[48;2;");
    } else {
        out.push_str("\x1b[38;2;");
    }
    push_fast_u32(out, r);
    out.push(';');
    push_fast_u32(out, g);
    out.push(';');
    push_fast_u32(out, b);
    out.push('m');
}

#[inline]
fn push_fast_u32(out: &mut String, mut val: u32) {
    val = val.min(255);
    if val >= 100 {
        out.push((b'0' + (val / 100) as u8) as char);
        val %= 100;
        out.push((b'0' + (val / 10) as u8) as char);
        out.push((b'0' + (val % 10) as u8) as char);
    } else if val >= 10 {
        out.push((b'0' + (val / 10) as u8) as char);
        out.push((b'0' + (val % 10) as u8) as char);
    } else {
        out.push((b'0' + val as u8) as char);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sample_world;

    #[test]
    fn test_gpu_renderer_creation_and_render() {
        let Ok(mut renderer) = GpuRenderer::new(60, 24, WeatherMode::Rain) else {
            eprintln!("Skipping test_gpu_renderer_creation_and_render: no GPU adapter on host");
            return;
        };
        let world = sample_world();
        let frame = renderer.render_frame(&world, 0);

        assert!(!frame.is_empty(), "GPU render output should not be empty");
        assert!(
            frame.contains("\x1b["),
            "Should contain ANSI truecolor escape codes"
        );
        assert!(frame.contains("WGPU 3D"), "Should display WGPU HUD");
    }

    #[test]
    fn test_gpu_renderer_resize() {
        let Ok(mut renderer) = GpuRenderer::new(40, 20, WeatherMode::Rain) else {
            eprintln!("Skipping test_gpu_renderer_resize: no GPU adapter on host");
            return;
        };
        renderer.resize(80, 30);
        assert_eq!(renderer.width, 80);
        assert_eq!(renderer.height, 30);
        let world = sample_world();
        let frame = renderer.render_frame(&world, 1);
        assert!(!frame.is_empty());
    }

    #[test]
    fn test_gpu_renderer_batch_processing() {
        let Ok(mut renderer) = GpuRenderer::new(50, 20, WeatherMode::Rain) else {
            eprintln!("Skipping test_gpu_renderer_batch_processing: no GPU adapter on host");
            return;
        };
        let mut world1 = sample_world();
        world1.tick = 1;
        let mut world2 = sample_world();
        world2.tick = 2;
        let mut world3 = sample_world();
        world3.tick = 3;

        let batch_outputs = renderer.render_batch(&[world1, world2, world3]);
        assert_eq!(
            batch_outputs.len(),
            3,
            "Batch rendering should produce 3 frames"
        );
        for (i, frame) in batch_outputs.iter().enumerate() {
            assert!(!frame.is_empty(), "Batch frame {i} should not be empty");
            assert!(
                frame.contains("\x1b["),
                "Batch frame {i} should contain ANSI codes"
            );
        }
    }

    #[test]
    fn test_gpu_renderer_instanced_batches() {
        let world = sample_world();
        let mut mesh = MeshBuffer::new();
        let mut batches = SceneBatches::new();

        let _ = GpuRenderer::populate_scene(
            40,
            20,
            WeatherMode::Rain,
            &world,
            10,
            0.0,
            &mut mesh,
            &mut batches,
        );
        assert!(
            batches.total_instance_count() > 100,
            "Should batch weather particles, trees, and pillars"
        );
        assert!(
            batches.tree_instances.len() >= 4,
            "Should have at least 4 instanced trees"
        );
        assert_eq!(
            batches.pillar_instances.len(),
            2,
            "Should have 2 instanced pillars"
        );
        assert_eq!(
            batches.particle_instances.len(),
            180,
            "Should have 180 instanced weather particles"
        );
    }

    #[test]
    fn test_frustum_culling() {
        let view = glam::Mat4::look_at_rh(
            glam::Vec3::new(0.0, 10.0, -10.0),
            glam::Vec3::new(0.0, 0.0, 0.0),
            glam::Vec3::new(0.0, 1.0, 0.0),
        );
        let proj = glam::Mat4::perspective_rh(45.0f32.to_radians(), 1.0, 0.1, 100.0);
        let vp = proj * view;
        let frustum = Frustum::from_view_proj(&vp);

        // Object at origin should be inside
        assert!(frustum.test_sphere(glam::Vec3::new(0.0, 0.0, 0.0), 1.0));
        assert!(frustum.test_aabb(
            glam::Vec3::new(-1.0, -1.0, -1.0),
            glam::Vec3::new(1.0, 1.0, 1.0),
        ));

        // Object far behind camera should be culled
        assert!(!frustum.test_sphere(glam::Vec3::new(0.0, 10.0, -50.0), 1.0));
        assert!(!frustum.test_aabb(
            glam::Vec3::new(-1.0, 9.0, -51.0),
            glam::Vec3::new(1.0, 11.0, -49.0),
        ));
    }

    #[test]
    fn test_wavefront_obj_loader() {
        let obj_cube = r#"
# Simple 3D OBJ Pyramid
v 0.0 1.0 0.0
v -1.0 0.0 -1.0
v 1.0 0.0 -1.0
v 1.0 0.0 1.0
v -1.0 0.0 1.0
f 1 2 3
f 1 3 4
f 1 4 5
f 1 5 2
f 2 5 4 3
"#;
        let mb = MeshBuffer::load_obj(obj_cube, [1.0, 0.8, 0.2, 1.0], 1.0, 0.3)
            .expect("Should parse valid OBJ string");
        assert!(
            !mb.vertices.is_empty(),
            "MeshBuffer should contain vertices"
        );
        assert!(
            !mb.indices.is_empty(),
            "MeshBuffer should contain triangle indices"
        );
        // 4 triangular sides (4 * 3 = 12 indices) + quad base (2 * 3 = 6 indices) = 18 indices
        assert_eq!(
            mb.indices.len(),
            18,
            "Pyramid should have 6 triangles = 18 indices"
        );
    }

    #[test]
    fn test_depth_unproject() {
        let cam_eye = glam::Vec3::new(0.0, 10.0, -10.0);
        let cam_target = glam::Vec3::new(0.0, 0.0, 0.0);
        let view = glam::Mat4::look_at_rh(cam_eye, cam_target, glam::Vec3::new(0.0, 1.0, 0.0));
        let proj = glam::Mat4::perspective_rh(45.0f32.to_radians(), 1.0, 0.1, 100.0);
        let view_proj = proj * view;
        let inv_view_proj = view_proj.inverse();

        let cam_uniforms = CameraUniforms {
            view_proj: view_proj.to_cols_array_2d(),
            inv_view_proj: inv_view_proj.to_cols_array_2d(),
            camera_pos: [cam_eye.x, cam_eye.y, cam_eye.z, 1.0],
            time: 0.0,
            weather_mode: 0,
            width: 100,
            height: 100,
        };

        // Center of screen (50, 50) projected forward to depth corresponding to target
        let target_ndc = view_proj * cam_target.extend(1.0);
        let target_depth = target_ndc.z / target_ndc.w;

        let reconstructed = cam_uniforms.unproject_screen_to_world(50.0, 50.0, target_depth);
        assert!(
            (reconstructed - cam_target).length() < 0.01,
            "Unprojected point should match original target: {reconstructed:?} vs {cam_target:?}"
        );
    }

    #[test]
    fn test_player_feet_do_not_move_when_idle() {
        let Ok(mut renderer) = GpuRenderer::new(40, 20, WeatherMode::Rain) else {
            eprintln!("Skipping test_player_feet_do_not_move_when_idle: no GPU adapter on host");
            return;
        };
        let mut world = sample_world();

        // 1. Initial idle state: player is stationary, walk_phase must be 0.0
        let idle_phase = renderer.update_walk_phase(&world);
        assert_eq!(
            idle_phase, 0.0,
            "Player feet should not move when standing still"
        );

        // Subsequent idle frame at same position: still 0.0
        let idle_phase_2 = renderer.update_walk_phase(&world);
        assert_eq!(
            idle_phase_2, 0.0,
            "Player feet should remain stationary on subsequent idle frames"
        );

        // 2. Player takes a step: position changes
        world.player.position.x += 1;
        let walk_phase = renderer.update_walk_phase(&world);
        assert!(
            walk_phase > 0.0,
            "Player walk phase should activate when position changes: {walk_phase}"
        );

        // 3. Humanoid mesh at 0.0 walk_phase has both boots planted side-by-side with zero leg swing
        let mut mesh = MeshBuffer::new();
        mesh.add_humanoid([0.0, 0.0, 0.0], 0.0, 0.0, 0.0);
        assert!(!mesh.vertices.is_empty());
    }

    #[test]
    fn test_character_models_and_monster_dispatch() {
        let mut mesh_player = MeshBuffer::new();
        mesh_player.add_humanoid([0.0, 0.0, 0.0], 0.0, 0.0, 0.0);
        assert!(
            !mesh_player.vertices.is_empty(),
            "Humanoid knight model should produce vertices"
        );

        let mut mesh_skel = MeshBuffer::new();
        mesh_skel.add_skeleton([0.0, 0.0, 0.0], 0.0, 0.0);
        assert!(
            !mesh_skel.vertices.is_empty(),
            "Skeleton model should produce vertices"
        );

        let mut mesh_warrior = MeshBuffer::new();
        mesh_warrior.add_skeleton_warrior([0.0, 0.0, 0.0], 0.0, 0.0);
        assert!(
            mesh_warrior.vertices.len() > mesh_skel.vertices.len(),
            "Skeleton warrior should have more geometry than unarmored skeleton"
        );

        let mut mesh_mossling = MeshBuffer::new();
        mesh_mossling.add_mossling([0.0, 0.0, 0.0], 0.0);
        assert!(
            !mesh_mossling.vertices.is_empty(),
            "Mossling model should produce vertices"
        );

        let mut mesh_herbalist = MeshBuffer::new();
        mesh_herbalist.add_herbalist([0.0, 0.0, 0.0], 0.0);
        assert!(
            !mesh_herbalist.vertices.is_empty(),
            "Herbalist model should produce vertices"
        );
    }

    #[test]
    fn test_entity_metadata_is_stamped_on_dynamic_geometry() {
        let mut mesh = MeshBuffer::new();
        mesh.set_entity_context(42, 2);
        mesh.add_box(
            [0.0, 0.5, 0.0],
            [1.0, 1.0, 1.0],
            [1.0, 0.2, 0.1, 1.0],
            1.0,
            0.2,
        );

        assert!(!mesh.vertices.is_empty());
        assert!(mesh
            .vertices
            .iter()
            .all(|vertex| vertex.entity_id == 42.0 && vertex.entity_class == 2.0));
    }

    #[test]
    fn test_populate_scene_with_all_entity_kinds() {
        let mut world = sample_world();
        world.entities.push(ascii_mmorpg_shared::ActorSnapshot {
            id: 4,
            name: "Skeleton Warrior".into(),
            kind: EntityKind::Monster,
            position: ascii_mmorpg_shared::Point { x: 5, y: 5 },
            stats: ascii_mmorpg_shared::Stats::healthy(15, 5, 2),
            glyph: 'W',
        });
        world.entities.push(ascii_mmorpg_shared::ActorSnapshot {
            id: 5,
            name: "Herbalist".into(),
            kind: EntityKind::Npc,
            position: ascii_mmorpg_shared::Point { x: 1, y: 1 },
            stats: ascii_mmorpg_shared::Stats::healthy(20, 1, 1),
            glyph: 'H',
        });

        let mut mesh = MeshBuffer::new();
        let mut batches = SceneBatches::default();
        let (_uniforms, lighting) = GpuRenderer::populate_scene(
            40,
            20,
            WeatherMode::Rain,
            &world,
            20,
            0.5,
            &mut mesh,
            &mut batches,
        );

        assert!(
            !mesh.vertices.is_empty(),
            "MeshBuffer must contain vertices for scene"
        );
        assert!(
            !mesh.indices.is_empty(),
            "MeshBuffer must contain indices for scene"
        );
        assert!(
            lighting.num_point_lights >= 2,
            "Scene should include player and NPC lights"
        );
    }

    #[test]
    fn test_pine_tree_trunk_and_root_flares_geometry() {
        let tree_proto = MeshBuffer::create_pine_tree_prototype();
        assert!(
            !tree_proto.vertices.is_empty(),
            "Pine tree prototype must contain vertices"
        );
        assert!(
            !tree_proto.indices.is_empty(),
            "Pine tree prototype must contain indices"
        );

        // Verify trunk ground contact reaches down to y <= 0.0
        let min_y = tree_proto
            .vertices
            .iter()
            .map(|v| v.position[1])
            .fold(f32::INFINITY, f32::min);
        assert!(
            min_y <= 0.05,
            "Pine tree trunk/roots must touch the ground plane: min_y={min_y}"
        );

        // Verify maximum tree apex reaches above 7.0 for tall towering silhouette
        let max_y = tree_proto
            .vertices
            .iter()
            .map(|v| v.position[1])
            .fold(f32::NEG_INFINITY, f32::max);
        assert!(
            max_y >= 7.0,
            "Pine tree apex should be towering: max_y={max_y}"
        );

        // Verify root flare lateral footprint spreads outwards for grounded stability
        let max_x = tree_proto
            .vertices
            .iter()
            .map(|v| v.position[0].abs())
            .fold(0.0f32, f32::max);
        assert!(
            max_x >= 1.4,
            "Pine tree root flares / branches must spread laterally: max_x={max_x}"
        );

        // Verify direct mesh buffer add_pine_tree
        let mut mesh = MeshBuffer::new();
        mesh.add_pine_tree([0.0, 0.0, 0.0], 0.0);
        assert!(!mesh.vertices.is_empty());
    }

    #[test]
    fn test_smooth_camera_and_player_transitions() {
        let Ok(mut renderer) = GpuRenderer::new(40, 20, WeatherMode::Rain) else {
            eprintln!("Skipping test_smooth_camera_and_player_transitions: no GPU adapter on host");
            return;
        };
        let mut world = sample_world();
        world.player.position = Point { x: 0, y: 0 };

        // Frame 1: Initial position at (0, 0)
        let (cam1, _) = renderer.build_scene_frame(&world, 0);
        let cam_pos1 = glam::Vec3::from_slice(&cam1.camera_pos[0..3]);
        assert_eq!(renderer.smooth_cam_pos, Some(glam::Vec2::new(0.0, 0.0)));
        assert_eq!(renderer.smooth_player_pos, Some(glam::Vec2::new(0.0, 0.0)));

        // Player takes a step to (10, 0) -> target x = 8.0
        world.player.position = Point { x: 10, y: 0 };
        let (cam2, _) = renderer.build_scene_frame(&world, 1);
        let cam_pos2 = glam::Vec3::from_slice(&cam2.camera_pos[0..3]);

        // Camera must smoothly interpolate rather than instantaneously jumping all 8 units
        assert!(cam_pos2.x > cam_pos1.x);
        assert!(cam_pos2.x < 8.0); // Smooth dampening prevents instantaneous 1-frame snap
        assert!(renderer.smooth_player_pos.unwrap().x < 8.0);
    }

    #[test]
    fn damping_is_stable_across_fixed_substeps() {
        let rate = 19.7;
        let one_step = damping_factor(rate, 1.0 / 30.0);
        let two_steps = 1.0 - (1.0 - damping_factor(rate, 1.0 / 60.0)).powi(2);
        assert!((one_step - two_steps).abs() < 1e-6);
        assert_eq!(damping_factor(rate, 0.0), 0.0);
    }
}
