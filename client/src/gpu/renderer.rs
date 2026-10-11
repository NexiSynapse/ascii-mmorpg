//! High-level GPU offscreen renderer for the ASCII MMORPG client
//! with multi-buffered asynchronous staging rings and hardware instanced batching.

use glam::{Mat4, Vec3};

use ascii_mmorpg_shared::{EntityKind, WorldSnapshot};

use super::context::GpuContext;
use super::mesh::{MeshBuffer, SceneBatches};
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

        let (camera_uniforms, lighting) = self.build_scene_frame(world, frame);

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

    /// Renders a batch of world snapshots continuously across the double-buffered GPU ring,
    /// keeping the GPU hardware at full utilization and eliminating CPU-GPU idle bubbles.
    pub fn render_batch(&mut self, snapshots: &[WorldSnapshot]) -> Vec<String> {
        if snapshots.is_empty() {
            return Vec::new();
        }

        let mut batch_data = Vec::with_capacity(snapshots.len());
        for (i, snapshot) in snapshots.iter().enumerate() {
            let mut mesh = MeshBuffer::new();
            let mut batches = SceneBatches::new();
            let (cam, light) = Self::populate_scene(
                self.width,
                self.height,
                self.weather,
                snapshot,
                snapshot.tick.max(i as u64),
                &mut mesh,
                &mut batches,
            );
            batch_data.push((mesh, batches, cam, light));
        }

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
        Self::populate_scene(
            self.width,
            self.height,
            self.weather,
            world,
            frame,
            &mut self.mesh,
            &mut self.batches,
        )
    }

    /// Populates scene geometry and fills hardware instance batches for a given target buffer set.
    pub fn populate_scene(
        width: i16,
        height: i16,
        weather: WeatherMode,
        world: &WorldSnapshot,
        frame: u64,
        mesh: &mut MeshBuffer,
        batches: &mut SceneBatches,
    ) -> (CameraUniforms, LightingUniforms) {
        let time = frame as f32 * 0.05;
        let p_pos = world.player.position;
        let px = p_pos.x as f32 * 0.8;
        let pz = p_pos.y as f32 * 0.8;

        // 1. Setup Camera (Isometric 2.5D perspective)
        let cam_eye = Vec3::new(px, 11.5, pz - 13.5);
        let cam_target = Vec3::new(px, 0.8, pz);
        let cam_up = Vec3::new(0.0, 1.0, 0.0);

        let view = Mat4::look_at_rh(cam_eye, cam_target, cam_up);
        let aspect = (width as f32 / height as f32) * 0.48;
        let proj = Mat4::perspective_rh(42.0f32.to_radians(), aspect, 0.1, 100.0);
        let view_proj = proj * view;

        let weather_id = match weather {
            WeatherMode::Rain => 0,
            WeatherMode::Snow => 1,
            WeatherMode::Storm => 2,
        };

        let camera_uniforms = CameraUniforms {
            view_proj: view_proj.to_cols_array_2d(),
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

        // Player Hip Lantern (Warm flickering light)
        let player_flicker = (time * 8.0).sin() * 0.12;
        lights.push(GpuPointLight {
            position: [px + 0.4, 1.2, pz + 0.2, 9.5],
            color: [1.0, 0.82, 0.45, 1.2 + player_flicker],
        });

        // Gate Torches at fixed world points
        let gate_flicker1 = (time * 9.5).sin() * 0.15;
        let gate_flicker2 = (time * 11.2).cos() * 0.15;
        lights.push(GpuPointLight {
            position: [-2.2, 2.6, 0.65, 8.5],
            color: [1.0, 0.62, 0.20, 1.4 + gate_flicker1],
        });
        lights.push(GpuPointLight {
            position: [2.2, 2.6, 0.65, 8.5],
            color: [1.0, 0.62, 0.20, 1.4 + gate_flicker2],
        });

        // Herbalist Staff Lantern
        for entity in &world.entities {
            if entity.kind == EntityKind::Npc {
                let nx = entity.position.x as f32 * 0.8;
                let nz = entity.position.y as f32 * 0.8;
                let herb_flicker = (time * 7.0).sin() * 0.1;
                lights.push(GpuPointLight {
                    position: [nx + 0.42, 1.85, nz + 0.15, 8.0],
                    color: [0.95, 0.80, 0.35, 1.3 + herb_flicker],
                });
            }
        }

        let light_count = lights.len().min(8);
        lighting.num_point_lights = light_count as u32;
        for (i, l) in lights.iter().take(8).enumerate() {
            lighting.point_lights[i] = *l;
        }

        // 3. Dynamic Scene Geometry (Cobblestone Highway, Altar, Gate, Entities)
        mesh.add_terrain(px, pz, 24.0);
        mesh.add_gate([0.0, 0.0, -10.0]);
        mesh.add_ruin_altar([-6.5, 0.0, 4.0]);

        for entity in &world.entities {
            let ex = entity.position.x as f32 * 0.8;
            let ez = entity.position.y as f32 * 0.8;
            match entity.kind {
                EntityKind::Monster => {
                    mesh.add_mossling([ex, 0.0, ez], time);
                }
                EntityKind::Npc => {
                    mesh.add_herbalist([ex, 0.0, ez], time);
                }
                EntityKind::Player => {
                    mesh.add_humanoid([ex, 0.0, ez], 0.0, time * 3.0, 0.0);
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

        let walk_phase = if world.player.stats.is_alive() {
            time * 4.0
        } else {
            0.0
        };
        mesh.add_humanoid([px, 0.0, pz], 0.0, walk_phase, 0.0);

        // 4. Instanced Props (Pine Trees, Ruin Pillars, Weather Particles)
        batches.add_pine_tree([-5.0, 0.0, -4.0], 1.0, (time * 2.0).sin());
        batches.add_pine_tree([6.0, 0.0, -2.5], 1.1, (time * 2.3).sin());
        batches.add_pine_tree([7.5, 0.0, 8.0], 0.95, (time * 1.8).sin());
        batches.add_pine_tree([-8.0, 0.0, 10.0], 1.05, (time * 2.1).sin());

        batches.add_pillar([-7.9, 0.0, 4.0], 1.0);
        batches.add_pillar([-5.1, 0.0, 4.0], 0.76);

        batches.add_weather_particles(px, pz, time, weather_id);

        (camera_uniforms, lighting)
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

        // HUD Status Bar
        let p = &world.player;
        let hp_bar = format_bar(p.stats.hp, p.stats.max_hp, 18, "\x1b[38;2;230;70;70m");
        let status = format!(
            "\x1b[1;37m[WARRIOR]\x1b[0m HP:{} \x1b[1;37m{}/{}\x1b[0m | ATK: \x1b[38;2;255;120;120m{}\x1b[0m DEF: \x1b[38;2;120;180;255m{}\x1b[0m LUCK: \x1b[38;2;255;220;80m{}\x1b[0m | POS: ({}, {}) | \x1b[38;2;120;220;255mWGPU 3D BATCHED PIPELINE\x1b[0m",
            hp_bar,
            p.stats.hp,
            p.stats.max_hp,
            p.stats.attack,
            p.stats.defense,
            p.stats.luck,
            p.position.x,
            p.position.y
        );
        out.push_str(&status);
        out.push_str("\r\n");

        if let Some(msg) = world.messages.last() {
            out.push_str(&format!("\x1b[38;2;160;200;230m> {}\x1b[0m\r\n", msg));
        }

        out
    }
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

fn format_bar(val: i32, max: i32, width: usize, color: &str) -> String {
    let safe_max = max.max(1);
    let ratio = (val as f32 / safe_max as f32).clamp(0.0, 1.0);
    let filled = (ratio * width as f32).round() as usize;
    let empty = width.saturating_sub(filled);
    format!(
        "{}[{}{}]\x1b[0m",
        color,
        "█".repeat(filled),
        "░".repeat(empty)
    )
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
            &mut mesh,
            &mut batches,
        );
        assert!(
            batches.total_instance_count() > 100,
            "Should batch weather particles, trees, and pillars"
        );
        assert_eq!(
            batches.tree_instances.len(),
            4,
            "Should have 4 instanced trees"
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
}
