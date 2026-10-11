//! High-level GPU offscreen renderer for the ASCII MMORPG client.

use glam::{Mat4, Vec3};

use ascii_mmorpg_shared::{EntityKind, WorldSnapshot};

use super::context::GpuContext;
use super::mesh::MeshBuffer;
use super::pipeline::{GpuCell, GpuPipeline};
use super::uniforms::{CameraUniforms, GpuPointLight, LightingUniforms};
use crate::renderer::WeatherMode;

pub struct GpuRenderer {
    pub ctx: GpuContext,
    pub pipeline: GpuPipeline,
    pub mesh: MeshBuffer,
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

        Ok(Self {
            ctx,
            pipeline,
            mesh,
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

    pub fn render_frame(&mut self, world: &WorldSnapshot, frame: u64) -> String {
        let time = frame as f32 * 0.05;
        let p_pos = world.player.position;
        // World coordinates to 3D space
        let px = p_pos.x as f32 * 0.8;
        let pz = p_pos.y as f32 * 0.8;

        // 1. Setup Camera (Isometric 2.5D perspective)
        let cam_eye = Vec3::new(px, 11.5, pz - 13.5);
        let cam_target = Vec3::new(px, 0.8, pz);
        let cam_up = Vec3::new(0.0, 1.0, 0.0);

        let view = Mat4::look_at_rh(cam_eye, cam_target, cam_up);
        // Correct aspect ratio for ~2:1 vertical rectangular terminal character cells
        let aspect = (self.width as f32 / self.height as f32) * 0.48;
        let proj = Mat4::perspective_rh(42.0f32.to_radians(), aspect, 0.1, 100.0);
        let view_proj = proj * view;

        let weather_id = match self.weather {
            WeatherMode::Rain => 0,
            WeatherMode::Snow => 1,
            WeatherMode::Storm => 2,
        };

        let camera_uniforms = CameraUniforms {
            view_proj: view_proj.to_cols_array_2d(),
            camera_pos: [cam_eye.x, cam_eye.y, cam_eye.z, 1.0],
            time,
            weather_mode: weather_id,
            width: self.width as u32,
            height: self.height as u32,
        };

        // 2. Setup Dynamic Lights (Sun + Torches)
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

        // Player Hip Lantern (Gentle warm flickering light)
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

        // Herbalist Lantern Staff
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

        // Fill point lights uniform
        let light_count = lights.len().min(8);
        lighting.num_point_lights = light_count as u32;
        for (i, l) in lights.iter().take(8).enumerate() {
            lighting.point_lights[i] = *l;
        }

        // 3. Build 3D Mesh for the Current Frame
        self.mesh.clear();

        // Procedural Volumetric Terrain (Cobblestone Highway & Meadow)
        self.mesh.add_terrain(px, pz, 24.0);

        // Scenery Props
        self.mesh.add_ruins([-6.5, 0.0, 4.0]);
        self.mesh.add_gate([0.0, 0.0, -10.0]);
        self.mesh
            .add_pine_tree([-5.0, 0.0, -4.0], (time * 2.0).sin());
        self.mesh
            .add_pine_tree([6.0, 0.0, -2.5], (time * 2.3).sin());
        self.mesh.add_pine_tree([7.5, 0.0, 8.0], (time * 1.8).sin());
        self.mesh
            .add_pine_tree([-8.0, 0.0, 10.0], (time * 2.1).sin());

        // Entities
        for entity in &world.entities {
            let ex = entity.position.x as f32 * 0.8;
            let ez = entity.position.y as f32 * 0.8;
            match entity.kind {
                EntityKind::Monster => {
                    self.mesh.add_mossling([ex, 0.0, ez], time);
                }
                EntityKind::Npc => {
                    self.mesh.add_herbalist([ex, 0.0, ez], time);
                }
                EntityKind::Player => {
                    self.mesh.add_humanoid([ex, 0.0, ez], 0.0, time * 3.0, 0.0);
                }
                EntityKind::Item => {
                    // Floating glowing loot chest / drop
                    let hover = (time * 3.0 + ex).sin() * 0.15;
                    self.mesh.add_box(
                        [ex, 0.35 + hover, ez],
                        [0.45, 0.35, 0.35],
                        [0.95, 0.85, 0.35, 1.0],
                        4.0,
                        0.1,
                    );
                }
            }
        }

        // Player Humanoid
        let walk_phase = if world.player.stats.is_alive() {
            time * 4.0
        } else {
            0.0
        };
        self.mesh.add_humanoid([px, 0.0, pz], 0.0, walk_phase, 0.0);

        // Volumetric 3D Weather Particles (Falling Rain, Splashes, Snowflakes)
        self.mesh.add_weather_particles(px, pz, time, weather_id);

        // 4. Render on GPU and Read Back ASCII Cells
        let cells = match self.pipeline.render_and_readback(
            &self.ctx,
            &self.mesh,
            &camera_uniforms,
            &lighting,
        ) {
            Ok(c) => c,
            Err(e) => {
                return format!("GPU Render Error: {e}");
            }
        };

        // 5. Format Cells into Terminal Output String with HUD
        self.format_frame_with_hud(&cells, world)
    }

    fn format_frame_with_hud(&self, cells: &[GpuCell], world: &WorldSnapshot) -> String {
        let width = self.width as usize;
        let height = self.height as usize;
        let mut out = String::with_capacity(width * height * 24);

        // Pre-allocate row buffer
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
                    out.push_str(&format!("\x1b[38;2;{};{};{}m", fg.0, fg.1, fg.2));
                    last_fg = fg;
                }
                if bg != last_bg {
                    out.push_str(&format!("\x1b[48;2;{};{};{}m", bg.0, bg.1, bg.2));
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
            "\x1b[1;37m[WARRIOR]\x1b[0m HP:{} \x1b[1;37m{}/{}\x1b[0m | ATK: \x1b[38;2;255;120;120m{}\x1b[0m DEF: \x1b[38;2;120;180;255m{}\x1b[0m LUCK: \x1b[38;2;255;220;80m{}\x1b[0m | POS: ({}, {}) | \x1b[38;2;120;220;255mWGPU 3D PIPELINE\x1b[0m",
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
        let mut renderer = GpuRenderer::new(60, 24, WeatherMode::Rain)
            .expect("GPU Renderer should initialize on host");
        let world = sample_world();
        let frame = renderer.render_frame(&world, 0);

        assert!(!frame.is_empty(), "GPU render output should not be empty");
        assert!(
            frame.contains("\x1b["),
            "Should contain ANSI truecolor escape codes"
        );
        assert!(
            frame.contains("WGPU 3D PIPELINE"),
            "Should display WGPU HUD"
        );
    }

    #[test]
    fn test_gpu_renderer_resize() {
        let mut renderer = GpuRenderer::new(40, 20, WeatherMode::Rain)
            .expect("GPU Renderer should initialize on host");
        renderer.resize(80, 30);
        assert_eq!(renderer.width, 80);
        assert_eq!(renderer.height, 30);
        let world = sample_world();
        let frame = renderer.render_frame(&world, 1);
        assert!(!frame.is_empty());
    }
}
