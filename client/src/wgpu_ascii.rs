//! Offscreen wgpu diorama with a compute-shader ASCII pass.
//!
//! Feature `wgpu-scene` only, so default builds stay GPU-free. Headless: the
//! scene renders into a texture (never a window), a compute shader reduces
//! texel blocks to packed cell records, and the CPU maps one small buffer
//! per frame and formats ANSI for any [`crate::Terminal`].

use ascii_mmorpg_shared::{EntityKind, WorldSnapshot};

/// Glyph ramp shared with `shaders/ascii_comp.wgsl` (same order).
#[cfg(test)]
pub const RAMP_CHARS: [char; 10] = [' ', '.', ':', '-', '=', '+', '*', '#', '@', '%'];
#[cfg(test)]
const RAMP_CODEPOINTS: [u32; 10] = [32, 46, 58, 45, 61, 43, 42, 35, 64, 37];

const SCENE_WGSL: &str = include_str!("shaders/scene.wgsl");
const ASCII_WGSL: &str = include_str!("shaders/ascii_comp.wgsl");

/// Cell texel block: the scene texture is exactly 2x2 texels per cell.
const BLOCK: u32 = 2;
const MAX_CELLS_X: u32 = 160;
const MAX_CELLS_Y: u32 = 90;

pub fn unpack_rgb(packed: u32) -> (u8, u8, u8) {
    ((packed >> 16) as u8, (packed >> 8) as u8, packed as u8)
}

#[cfg(test)]
fn pack_rgb(r: u8, g: u8, b: u8) -> u32 {
    ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)
}

/// Format packed `(glyph_codepoint, fg_rgb)` cells as ANSI, collapsing ink runs.
pub fn format_cells(cells: &[(u32, u32)], cells_x: usize) -> String {
    assert!(cells_x > 0);
    let mut out = String::new();
    let mut current: Option<(u8, u8, u8)> = None;
    for (i, (glyph, fg)) in cells.iter().enumerate() {
        let (r, g, b) = unpack_rgb(*fg);
        if current != Some((r, g, b)) {
            out.push_str(&format!("\x1b[38;2;{r};{g};{b}m"));
            current = Some((r, g, b));
        }
        out.push(char::from_u32(*glyph).unwrap_or(' '));
        if (i + 1) % cells_x == 0 {
            out.push_str("\x1b[0m\x1b[K\n");
            current = None;
        }
    }
    out.pop();
    out
}

// ---------------------------------------------------------------------------
// Minimal column-major math (no linear-algebra dependency for one camera).
// ---------------------------------------------------------------------------

type Vec3 = [f32; 3];
type Mat4 = [[f32; 4]; 4];

fn sub(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: Vec3, b: Vec3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: Vec3, b: Vec3) -> Vec3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn norm(v: Vec3) -> Vec3 {
    let len = dot(v, v).sqrt().max(1e-6);
    [v[0] / len, v[1] / len, v[2] / len]
}

fn look_at(eye: Vec3, target: Vec3, up: Vec3) -> Mat4 {
    // Column-major view matrix: column i holds the i-th world basis axis
    // expressed in components, i.e. column 0 = (x0, y0, z0).
    let z = norm(sub(eye, target));
    let x = norm(cross(up, z));
    let y = cross(z, x);
    [
        [x[0], y[0], z[0], 0.0],
        [x[1], y[1], z[1], 0.0],
        [x[2], y[2], z[2], 0.0],
        [-dot(x, eye), -dot(y, eye), -dot(z, eye), 1.0],
    ]
}

fn ortho(left: f32, right: f32, bottom: f32, top: f32, near: f32, far: f32) -> Mat4 {
    [
        [2.0 / (right - left), 0.0, 0.0, 0.0],
        [0.0, 2.0 / (top - bottom), 0.0, 0.0],
        [0.0, 0.0, -2.0 / (far - near), 0.0],
        [
            -(right + left) / (right - left),
            -(top + bottom) / (top - bottom),
            -(far + near) / (far - near),
            1.0,
        ],
    ]
}

fn mat_mul(a: &Mat4, b: &Mat4) -> Mat4 {
    let mut out = [[0.0; 4]; 4];
    for col in 0..4 {
        for row in 0..4 {
            out[col][row] = a[0][row] * b[col][0]
                + a[1][row] * b[col][1]
                + a[2][row] * b[col][2]
                + a[3][row] * b[col][3];
        }
    }
    out
}

#[cfg(test)]
fn transform_point(m: &Mat4, p: Vec3) -> [f32; 4] {
    [
        m[0][0] * p[0] + m[1][0] * p[1] + m[2][0] * p[2] + m[3][0],
        m[0][1] * p[0] + m[1][1] * p[1] + m[2][1] * p[2] + m[3][1],
        m[0][2] * p[0] + m[1][2] * p[1] + m[2][2] * p[2] + m[3][2],
        m[0][3] * p[0] + m[1][3] * p[1] + m[2][3] * p[2] + m[3][3],
    ]
}

// ---------------------------------------------------------------------------
// Diorama geometry: axis-aligned boxes plus 6-sided cones, baked per frame.
// Vertex layout is 9 floats: pos.xyz, normal.xyz, albedo.rgb.
// ---------------------------------------------------------------------------

fn push_tri(verts: &mut Vec<f32>, a: Vec3, b: Vec3, c: Vec3, albedo: [f32; 3]) {
    let n = norm(cross(sub(b, a), sub(c, a)));
    for p in [a, b, c] {
        verts.extend_from_slice(&p);
        verts.extend_from_slice(&n);
        verts.extend_from_slice(&albedo);
    }
}

fn push_box(verts: &mut Vec<f32>, min: Vec3, max: Vec3, albedo: [f32; 3]) {
    let corners = [
        [min[0], min[1], min[2]],
        [max[0], min[1], min[2]],
        [max[0], max[1], min[2]],
        [min[0], max[1], min[2]],
        [min[0], min[1], max[2]],
        [max[0], min[1], max[2]],
        [max[0], max[1], max[2]],
        [min[0], max[1], max[2]],
    ];
    // (a, b, c, d) quads, counter-clockwise seen from outside.
    for [a, b, c, d] in [
        [4, 5, 6, 7], // +z
        [1, 0, 3, 2], // -z
        [5, 1, 2, 6], // +x
        [0, 4, 7, 3], // -x
        [7, 6, 2, 3], // +y
        [0, 1, 5, 4], // -y
    ] {
        push_tri(verts, corners[a], corners[b], corners[c], albedo);
        push_tri(verts, corners[a], corners[c], corners[d], albedo);
    }
}

fn push_cone(verts: &mut Vec<f32>, base: Vec3, radius: f32, height: f32, albedo: [f32; 3]) {
    const SEGS: usize = 6;
    let apex = [base[0], base[1] + height, base[2]];
    let mut ring = Vec::with_capacity(SEGS);
    for i in 0..SEGS {
        let a = (i as f32) / (SEGS as f32) * std::f32::consts::TAU;
        ring.push([
            base[0] + a.cos() * radius,
            base[1],
            base[2] + a.sin() * radius,
        ]);
    }
    for i in 0..SEGS {
        let next = (i + 1) % SEGS;
        push_tri(verts, apex, ring[next], ring[i], albedo);
        push_tri(verts, base, ring[i], ring[next], albedo);
    }
}

fn hash2(x: i64, y: i64) -> u64 {
    let mut h = (x as u64)
        .wrapping_mul(0x45d9f3b)
        .wrapping_add((y as u64).wrapping_mul(0x119de1f3));
    h ^= h >> 16;
    h = h.wrapping_mul(0x45d9f3b);
    h ^ (h >> 13)
}

const STONE: [f32; 3] = [0.42, 0.39, 0.35];
const LEAF: [f32; 3] = [0.14, 0.30, 0.18];
const TRUNK: [f32; 3] = [0.35, 0.25, 0.16];
const GROUND: [f32; 3] = [0.09, 0.11, 0.09];
const LIME: [f32; 3] = [0.55, 1.0, 0.25];
const MOSS: [f32; 3] = [0.35, 0.52, 0.30];
const BONE: [f32; 3] = [0.85, 0.82, 0.72];
const TAN: [f32; 3] = [0.55, 0.48, 0.34];
const GOLD: [f32; 3] = [0.95, 0.75, 0.25];
const EMBER: [f32; 3] = [2.2, 1.1, 0.25];

/// Bake the diorama for one snapshot: ground, gatehouse landmark, scattered
/// trees, lanterns, the local player, and nearby entities.
fn bake_scene(world: &WorldSnapshot, frame: u64) -> Vec<f32> {
    let mut verts = Vec::with_capacity(4096 * 9);
    let px = world.player.position.x as f32;
    let pz = world.player.position.y as f32;

    // Ground slab under the camera.
    push_box(
        &mut verts,
        [px - 30.0, -0.5, pz - 30.0],
        [px + 30.0, 0.0, pz + 30.0],
        GROUND,
    );
    // Gatehouse landmark, same world anchor as the 2D gate prop.
    push_box(&mut verts, [0.0, 0.0, -9.0], [1.0, 4.0, -8.0], STONE);
    push_box(&mut verts, [3.0, 0.0, -9.0], [4.0, 4.0, -8.0], STONE);
    push_box(&mut verts, [-0.5, 4.0, -9.2], [4.5, 5.0, -7.8], STONE);
    // Lantern posts double as the two point-light anchors.
    for (lx, lz) in [(-6.0, 2.0), (6.0, -4.0)] {
        push_box(
            &mut verts,
            [lx - 0.15, 0.0, lz - 0.15],
            [lx + 0.15, 1.6, lz + 0.15],
            TRUNK,
        );
        push_box(
            &mut verts,
            [lx - 0.3, 1.6, lz - 0.3],
            [lx + 0.3, 2.0, lz + 0.3],
            EMBER,
        );
    }
    // Scattered pines on a fixed world grid around the player.
    let gx0 = (px as i64 - 21) / 7;
    let gx1 = (px as i64 + 21) / 7;
    let gz0 = (pz as i64 - 21) / 7;
    let gz1 = (pz as i64 + 21) / 7;
    for gx in gx0..=gx1 {
        for gz in gz0..=gz1 {
            let h = hash2(gx, gz);
            if h % 5 != 0 {
                continue;
            }
            let tx = gx as f32 * 7.0 + ((h / 7) % 5) as f32 - 2.0;
            let tz = gz as f32 * 7.0 + ((h / 13) % 5) as f32 - 2.0;
            if (tx - px).abs() + (tz - pz).abs() < 3.0 {
                continue; // Never plant on the player.
            }
            push_box(
                &mut verts,
                [tx - 0.2, 0.0, tz - 0.2],
                [tx + 0.2, 1.5, tz + 0.2],
                TRUNK,
            );
            push_cone(&mut verts, [tx, 1.2, tz], 1.5, 2.8, LEAF);
        }
    }
    // Local player: slim lime pillar with a gentle idle bob.
    let bob = ((frame % 24) as f32 / 24.0 * std::f32::consts::TAU).sin() * 0.06;
    push_box(
        &mut verts,
        [px - 0.35, 0.0, pz - 0.35],
        [px + 0.35, 1.6 + bob, pz + 0.35],
        LIME,
    );
    // Nearby entities, nearest first, capped so one frame stays small.
    let mut mobs: Vec<_> = world.entities.iter().collect();
    mobs.sort_by_key(|m| {
        (m.position.x - world.player.position.x).abs()
            + (m.position.y - world.player.position.y).abs()
    });
    for mob in mobs.into_iter().take(12) {
        let mx = mob.position.x as f32;
        let mz = mob.position.y as f32;
        let albedo = match mob.kind {
            EntityKind::Player => LIME,
            EntityKind::Monster if mob.name.to_ascii_lowercase().contains("skeleton") => BONE,
            EntityKind::Monster => MOSS,
            EntityKind::Npc => TAN,
            EntityKind::Item => GOLD,
        };
        let tall = match mob.kind {
            EntityKind::Item => 0.4,
            _ => 1.1,
        };
        push_box(
            &mut verts,
            [mx - 0.4, 0.0, mz - 0.4],
            [mx + 0.4, tall, mz + 0.4],
            albedo,
        );
    }
    verts
}

// ---------------------------------------------------------------------------
// GPU backend.
// ---------------------------------------------------------------------------

#[repr(C)]
struct CameraUniform {
    mvp: Mat4,
    light_a: [f32; 3],
    ambient: f32,
    light_b: [f32; 3],
    boost: f32,
}

impl CameraUniform {
    fn to_bytes(&self) -> [u8; 96] {
        let mut out = [0u8; 96];
        for (i, col) in self.mvp.iter().enumerate() {
            for (j, v) in col.iter().enumerate() {
                out[i * 16 + j * 4..i * 16 + j * 4 + 4].copy_from_slice(&v.to_le_bytes());
            }
        }
        for (i, v) in self.light_a.iter().enumerate() {
            out[64 + i * 4..64 + i * 4 + 4].copy_from_slice(&v.to_le_bytes());
        }
        out[76..80].copy_from_slice(&self.ambient.to_le_bytes());
        for (i, v) in self.light_b.iter().enumerate() {
            out[80 + i * 4..80 + i * 4 + 4].copy_from_slice(&v.to_le_bytes());
        }
        out[92..96].copy_from_slice(&self.boost.to_le_bytes());
        out
    }
}

struct SizedResources {
    cells_x: u32,
    cells_y: u32,
    #[allow(dead_code)]
    scene_tex: wgpu::Texture,
    scene_view: wgpu::TextureView,
    cell_buf: wgpu::Buffer,
    staging_buf: wgpu::Buffer,
    params_buf: wgpu::Buffer,
    comp_bind: wgpu::BindGroup,
}

pub struct WgpuAscii {
    device: wgpu::Device,
    queue: wgpu::Queue,
    adapter_name: String,
    fallback: bool,
    scene_pipe: wgpu::RenderPipeline,
    comp_pipe: wgpu::ComputePipeline,
    comp_bind_layout: wgpu::BindGroupLayout,
    cam_buf: wgpu::Buffer,
    scene_bind: wgpu::BindGroup,
    sized: Option<SizedResources>,
    last_submit_ms: f64,
}

impl WgpuAscii {
    /// Headless init: no surface required. With `allow_fallback`, a missing
    /// hardware adapter retries on the software rasterizer (slow, portable).
    /// `WGPU_FORCE_FALLBACK=1` skips hardware entirely (driver isolation).
    /// `WGPU_POWER=low` prefers integrated GPUs (potency over raw power).
    pub fn init(allow_fallback: bool) -> Result<Self, String> {
        let instance = wgpu::Instance::default();
        let low_power = std::env::var("WGPU_POWER").as_deref() == Ok("low");
        let try_adapter = |fallback: bool| {
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: if low_power {
                    wgpu::PowerPreference::LowPower
                } else {
                    wgpu::PowerPreference::HighPerformance
                },
                force_fallback_adapter: fallback,
                compatible_surface: None,
                apply_limit_buckets: false,
            }))
        };
        let force_software = std::env::var("WGPU_FORCE_FALLBACK").is_ok();
        let mut adapter = if force_software {
            None
        } else {
            try_adapter(false).ok()
        };
        let mut used_fallback = false;
        if allow_fallback && adapter.is_none() {
            adapter = try_adapter(true).ok();
            used_fallback = adapter.is_some();
        }
        let adapter = adapter.ok_or_else(|| {
            "no usable GPU adapter; --wgpu-fallback retries on software rendering".to_string()
        })?;
        if !adapter
            .get_downlevel_capabilities()
            .flags
            .contains(wgpu::DownlevelFlags::COMPUTE_SHADERS)
        {
            return Err("adapter lacks compute shaders".to_string());
        }
        let adapter_name = format!("{:?}", adapter.get_info());
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("ascii-device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::downlevel_defaults(),
            ..Default::default()
        }))
        .map_err(|e| format!("device request failed: {e:?}"))?;

        let scene_mod = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("scene"),
            source: wgpu::ShaderSource::Wgsl(SCENE_WGSL.into()),
        });
        let comp_mod = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("ascii-comp"),
            source: wgpu::ShaderSource::Wgsl(ASCII_WGSL.into()),
        });

        let scene_bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("scene-bind-layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let comp_bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("comp-bind-layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: false },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let scene_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("scene-layout"),
            bind_group_layouts: &[Some(&scene_bind_layout)],
            immediate_size: 0,
        });
        let comp_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("comp-layout"),
            bind_group_layouts: &[Some(&comp_bind_layout)],
            immediate_size: 0,
        });

        let vert_layout = wgpu::VertexBufferLayout {
            array_stride: 36,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x3,
                    offset: 0,
                    shader_location: 0,
                },
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x3,
                    offset: 12,
                    shader_location: 1,
                },
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x3,
                    offset: 24,
                    shader_location: 2,
                },
            ],
        };
        let scene_pipe = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("scene-pipe"),
            layout: Some(&scene_layout),
            vertex: wgpu::VertexState {
                module: &scene_mod,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(vert_layout)],
            },
            fragment: Some(wgpu::FragmentState {
                module: &scene_mod,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let comp_pipe = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("ascii-pipe"),
            layout: Some(&comp_layout),
            module: &comp_mod,
            entry_point: Some("ascii_main"),
            compilation_options: Default::default(),
            cache: None,
        });

        let cam_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("camera"),
            size: 96,
            usage: wgpu::BufferUsages::UNIFORM
                | wgpu::BufferUsages::COPY_DST
                | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let scene_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("scene-bind"),
            layout: &scene_bind_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: cam_buf.as_entire_binding(),
            }],
        });

        Ok(Self {
            device,
            queue,
            adapter_name,
            fallback: used_fallback,
            scene_pipe,
            comp_pipe,
            comp_bind_layout,
            cam_buf,
            scene_bind,
            sized: None,
            last_submit_ms: 0.0,
        })
    }

    pub fn adapter_name(&self) -> &str {
        &self.adapter_name
    }

    pub fn used_fallback(&self) -> bool {
        self.fallback
    }

    pub fn last_submit_ms(&self) -> f64 {
        self.last_submit_ms
    }

    fn ensure_size(&mut self, cells_x: u32, cells_y: u32) {
        let cells_x = cells_x.clamp(20, MAX_CELLS_X);
        let cells_y = cells_y.clamp(10, MAX_CELLS_Y);
        if matches!(&self.sized, Some(s) if s.cells_x == cells_x && s.cells_y == cells_y) {
            return;
        }
        let tex_w = cells_x * BLOCK;
        let tex_h = cells_y * BLOCK;
        let scene_tex = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("scene-target"),
            size: wgpu::Extent3d {
                width: tex_w,
                height: tex_h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let scene_view = scene_tex.create_view(&wgpu::TextureViewDescriptor::default());
        let cell_bytes = (cells_x * cells_y * 16) as u64;
        let cell_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("cells"),
            size: cell_bytes,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let staging_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("cells-staging"),
            size: cell_bytes,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let params_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ascii-params"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let comp_bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("comp-bind"),
            layout: &self.comp_bind_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&scene_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: cell_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: params_buf.as_entire_binding(),
                },
            ],
        });
        self.sized = Some(SizedResources {
            cells_x,
            cells_y,
            scene_tex,
            scene_view,
            cell_buf,
            staging_buf,
            params_buf,
            comp_bind,
        });
    }

    /// Render one snapshot to an ANSI frame sized for a `width` x `height`
    /// terminal, footer included like the CPU renderer.
    pub fn render_snapshot(
        &mut self,
        world: &WorldSnapshot,
        frame: u64,
        width: u16,
        height: u16,
    ) -> Result<String, String> {
        use wgpu::util::DeviceExt;

        self.ensure_size(width.max(20) as u32, height.max(10) as u32);
        let sized = self.sized.as_ref().expect("sized resources built");
        let (cells_x, cells_y) = (sized.cells_x, sized.cells_y);
        let (tex_w, tex_h) = (cells_x * BLOCK, cells_y * BLOCK);

        let px = world.player.position.x as f32;
        let pz = world.player.position.y as f32;
        let eye = [px, 15.0, pz + 10.0];
        let view = look_at(eye, [px, 0.0, pz], [0.0, 1.0, 0.0]);
        let aspect = tex_w as f32 / tex_h as f32;
        let half_h = 6.0;
        let proj = ortho(
            -half_h * aspect,
            half_h * aspect,
            -half_h,
            half_h,
            1.0,
            60.0,
        );
        let mvp = mat_mul(&proj, &view);
        // Torch flicker: the player-carried light breathes with the frame.
        let flick = ((frame % 48) as f32 / 48.0 * std::f32::consts::TAU).sin() * 0.2;
        let cam = CameraUniform {
            mvp,
            light_a: [px + flick, 2.5, pz],
            ambient: 0.32,
            light_b: [-6.0, 2.0, 2.0],
            boost: 1.6,
        };
        self.queue.write_buffer(&self.cam_buf, 0, &cam.to_bytes());
        let params = [cells_x, cells_y, tex_w, tex_h];
        let mut param_bytes = [0u8; 16];
        for (i, v) in params.iter().enumerate() {
            param_bytes[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
        }
        self.queue.write_buffer(&sized.params_buf, 0, &param_bytes);

        let verts = bake_scene(world, frame);
        let vert_bytes: Vec<u8> = verts.iter().flat_map(|v| v.to_le_bytes()).collect();
        let vert_buf = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("diorama"),
                contents: &vert_bytes,
                usage: wgpu::BufferUsages::VERTEX,
            });
        let nverts = (verts.len() / 9) as u32;

        let started = std::time::Instant::now();
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("scene"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &sized.scene_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 12.0 / 255.0,
                            g: 15.0 / 255.0,
                            b: 22.0 / 255.0,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.scene_pipe);
            pass.set_bind_group(0, &self.scene_bind, &[]);
            pass.set_vertex_buffer(0, vert_buf.slice(..));
            pass.draw(0..nverts, 0..1);
        }
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("ascii"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.comp_pipe);
            pass.set_bind_group(0, &sized.comp_bind, &[]);
            pass.dispatch_workgroups((cells_x + 7) / 8, (cells_y + 7) / 8, 1);
        }
        encoder.copy_buffer_to_buffer(
            &sized.cell_buf,
            0,
            &sized.staging_buf,
            0,
            (cells_x * cells_y * 16) as u64,
        );
        self.queue.submit(Some(encoder.finish()));

        let slice = sized.staging_buf.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|e| format!("gpu poll failed: {e:?}"))?;
        rx.recv()
            .map_err(|e| format!("gpu map channel failed: {e}"))?
            .map_err(|e| format!("gpu map failed: {e:?}"))?;
        let cells: Vec<(u32, u32)> = {
            let view = slice
                .get_mapped_range()
                .map_err(|e| format!("gpu map range failed: {e:?}"))?;
            let mut out = Vec::with_capacity((cells_x * cells_y) as usize);
            for chunk in view.chunks_exact(16) {
                let glyph = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                let fg = u32::from_le_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]);
                out.push((glyph, fg));
            }
            out
        };
        sized.staging_buf.unmap();
        self.last_submit_ms = started.elapsed().as_secs_f64() * 1000.0;

        let mut output = format_cells(&cells, cells_x as usize);
        output.push_str(&format!(
            "\n\n\x1b[38;5;252mTick {}  HP {}/{}\x1b[0m\x1b[K",
            world.tick, world.player.stats.hp, world.player.stats.max_hp
        ));
        for message in &world.messages {
            output.push_str(&format!("\n\x1b[38;5;245m{message}\x1b[0m\x1b[K"));
        }
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ascii_mmorpg_shared::{ActorSnapshot, EntityKind, Point, Stats};

    #[test]
    fn shader_ramp_matches_cpu_ramp() {
        let seq: String = RAMP_CODEPOINTS
            .iter()
            .map(|c| format!("{c}u"))
            .collect::<Vec<_>>()
            .join(",");
        let compact: String = ASCII_WGSL.chars().filter(|c| !c.is_whitespace()).collect();
        assert!(
            compact.contains(&seq),
            "compute ramp drifted from RAMP_CODEPOINTS"
        );
        assert_eq!(RAMP_CHARS.len(), RAMP_CODEPOINTS.len());
        for (ch, code) in RAMP_CHARS.iter().zip(RAMP_CODEPOINTS.iter()) {
            assert_eq!(*ch as u32, *code);
        }
    }

    #[test]
    fn rgb_packing_round_trips() {
        assert_eq!(unpack_rgb(pack_rgb(12, 15, 22)), (12, 15, 22));
        assert_eq!(unpack_rgb(0xB2FF5A), (0xB2, 0xFF, 0x5A));
    }

    #[test]
    fn format_cells_collapses_runs_and_breaks_rows() {
        let cells = vec![
            (64, pack_rgb(255, 0, 0)),
            (64, pack_rgb(255, 0, 0)),
            (32, pack_rgb(0, 0, 0)),
        ];
        // 3 cells on one row of width 3: single red run, then a black space.
        assert_eq!(
            format_cells(&cells, 3),
            "\x1b[38;2;255;0;0m@@\x1b[38;2;0;0;0m \x1b[0m\x1b[K"
        );
    }

    #[test]
    fn look_at_centers_its_target() {
        let view = look_at([0.0, 0.0, 5.0], [0.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        let eye_at_origin = transform_point(&view, [0.0, 0.0, 5.0]);
        assert!((eye_at_origin[0]).abs() < 1e-5);
        assert!((eye_at_origin[1]).abs() < 1e-5);
        assert!((eye_at_origin[2]).abs() < 1e-5);
        let target = transform_point(&view, [0.0, 0.0, 0.0]);
        assert!((target[0]).abs() < 1e-5);
        assert!((target[1]).abs() < 1e-5);
        assert!(target[2] < -4.9 && target[2] > -5.1);
        // Asymmetric case: the target must sit on the view -z axis even when
        // the camera is off to the side (catches transposed bases).
        let tilted = look_at([2.0, 15.0, 12.0], [2.0, 0.0, 2.0], [0.0, 1.0, 0.0]);
        let centered = transform_point(&tilted, [2.0, 0.0, 2.0]);
        assert!((centered[0] / centered[3]).abs() < 1e-4);
        assert!((centered[1] / centered[3]).abs() < 1e-4);
        assert!(centered[2] < 0.0);
    }

    #[test]
    fn ortho_maps_unit_cube_to_ndc() {
        let proj = ortho(-1.0, 1.0, -1.0, 1.0, 0.1, 10.0);
        let corner = transform_point(&proj, [1.0, 1.0, -1.0]);
        assert!((corner[0] - 1.0).abs() < 1e-5);
        assert!((corner[1] - 1.0).abs() < 1e-5);
    }

    #[test]
    fn baked_geometry_projects_inside_clip() {
        let world = smoke_world();
        let verts = bake_scene(&world, 4);
        assert!(!verts.is_empty());
        let (px, pz) = (2.0, 2.0);
        let view = look_at([px, 15.0, pz + 10.0], [px, 0.0, pz], [0.0, 1.0, 0.0]);
        let proj = ortho(-12.0, 12.0, -6.0, 6.0, 1.0, 60.0);
        let mvp = mat_mul(&proj, &view);
        let mut inside = 0;
        for tri in verts.chunks_exact(9) {
            let c = transform_point(&mvp, [tri[0], tri[1], tri[2]]);
            let ndc = [c[0] / c[3], c[1] / c[3], c[2] / c[3]];
            if ndc[0].abs() <= 1.0 && ndc[1].abs() <= 1.0 && ndc[2].abs() <= 1.0 {
                inside += 1;
            }
        }
        assert!(inside > 100, "only {inside} verts inside clip");
    }

    #[test]
    fn gpu_error_scope_reports_validation_issues() {
        let mut backend = match WgpuAscii::init(true) {
            Ok(backend) => backend,
            Err(error) => {
                eprintln!("SKIP: {error}");
                return;
            }
        };
        let scope = backend
            .device
            .push_error_scope(wgpu::ErrorFilter::Validation);
        let rendered = backend.render_snapshot(&smoke_world(), 4, 80, 40);
        eprintln!("rendered ok: {}", rendered.is_ok());
        let err = pollster::block_on(scope.pop());
        eprintln!("validation scope: {err:?}");
    }

    fn smoke_world() -> WorldSnapshot {
        WorldSnapshot {
            tick: 3,
            player: ActorSnapshot {
                id: 1,
                name: "Ranger".into(),
                kind: EntityKind::Player,
                position: Point { x: 2, y: 2 },
                stats: Stats::healthy(20, 6, 2),
                glyph: '@',
            },
            entities: vec![ActorSnapshot {
                id: 2,
                name: "Skeleton".into(),
                kind: EntityKind::Monster,
                position: Point { x: 5, y: 3 },
                stats: Stats::healthy(10, 4, 1),
                glyph: 's',
            }],
            messages: vec!["You hit Skeleton for 4 damage.".into()],
        }
    }

    #[test]
    fn gpu_offscreen_smoke_renders_a_frame() {
        let mut backend = match WgpuAscii::init(true) {
            Ok(backend) => backend,
            Err(error) => {
                eprintln!("SKIP gpu smoke: {error}");
                return;
            }
        };
        eprintln!("gpu smoke on {}", backend.adapter_name());
        let frame = backend
            .render_snapshot(&smoke_world(), 4, 80, 40)
            .expect("frame renders");
        assert!(frame.contains("\x1b["));
        assert!(frame.contains("Tick 3"));
        assert!(frame.len() > 1000);
    }
}
