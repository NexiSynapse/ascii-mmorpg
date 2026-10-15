//! Windowed presenter: opens a real OS window and draws engine [`Frame`]s
//! with a GPU glyph atlas (one instanced quad per cell, atlas built by
//! [`atlas`]). Runs anywhere a window can open — no terminal required.
//!
//! `run` presents one static scene; `run_dynamic` takes a [`SceneSource`]
//! for live scenes plus key input (see the `play` example).

pub mod atlas;

use super::{compose, Camera, Frame, Scene};
use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How to open the window and pace the loop.
pub struct PresentOptions {
    pub title: String,
    /// Font size in px; also fixes the cell grid density.
    pub cell_px: f32,
    /// Initial grid when a windowed window opens (grows/shrinks with resizes).
    /// A zero dimension uses the monitor/surface size. This keeps existing
    /// struct literals source-compatible while making `Default` fullscreen.
    pub width_cells: u32,
    pub height_cells: u32,
    /// Frame budget (0 = uncapped).
    pub fps: u32,
    /// Exit after this many presented frames (automated proof without keys).
    pub max_frames: Option<u64>,
}

impl Default for PresentOptions {
    fn default() -> Self {
        Self {
            title: "ASCII Engine".into(),
            cell_px: 20.0,
            width_cells: 0,
            height_cells: 0,
            fps: 60,
            max_frames: None,
        }
    }
}

/// Open a window and present `scene` (re-composed at elapsed `t` every frame)
/// until closed. Returns the presented frame count. Headless CI fallback:
/// the `wave` example dumps the same scene as text without opening a window.
pub fn run(scene: Scene, camera: Camera, opts: PresentOptions) -> Result<u64, String> {
    run_dynamic(StaticSource(scene, camera), opts)
}

/// A live scene provider: rebuilt (or re-fetched) every frame at elapsed `t`
/// seconds, plus key input. Games implement this over their simulation (the
/// `play` example drives a local server `Simulation`); static demos use
/// [`run`] instead.
pub trait SceneSource {
    /// Current scene and camera. Called once per presented frame.
    fn scene_at(&mut self, t: f32) -> (Scene, Camera);
    /// Variant that can adapt a scene to the live glyph grid. The default
    /// preserves sources that use a fixed world/UI layout.
    fn scene_at_size(&mut self, t: f32, _width: u32, _height: u32) -> (Scene, Camera) {
        self.scene_at(t)
    }
    /// Key press (`pressed`) / release. Esc/Q and close always exit in
    /// addition to this; return value is reserved (currently ignored).
    fn key(&mut self, _code: winit::keyboard::KeyCode, _pressed: bool) {}
}

struct StaticSource(Scene, Camera);

impl SceneSource for StaticSource {
    fn scene_at(&mut self, _t: f32) -> (Scene, Camera) {
        (self.0.clone(), self.1)
    }
}

/// Like [`run`] but the scene (and camera) come from `source` every frame,
/// and WASD-style keys reach `source` too. Returns the presented count.
pub fn run_dynamic(
    source: impl SceneSource + 'static,
    opts: PresentOptions,
) -> Result<u64, String> {
    use winit::event_loop::EventLoop;
    let event_loop = EventLoop::new().map_err(|e| format!("winit event loop: {e}"))?;
    let mut app = App::new(Box::new(source), opts)?;
    event_loop
        .run_app(&mut app)
        .map_err(|e| format!("winit run: {e}"))?;
    // Logged before `app` drops: wgpu teardown itself can crash on some
    // drivers (see example), so this line is the proof the loop ran.
    eprintln!("[present] presented {} frames", app.presented);
    Ok(app.presented)
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct GlyphInstance {
    gx: u32,
    gy: u32,
    glyph: u32,
    fg_start: [f32; 3],
    fg_end: [f32; 3],
    bg: [f32; 3],
    /// 0 = flat, 1 = horizontal, 2 = vertical.
    fg_gradient_axis: u32,
    /// Billboard size multiplier (1 = full cell, centered).
    scale: f32,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    grid_w: u32,
    grid_h: u32,
    surf_w: u32,
    surf_h: u32,
}

struct Gpu {
    // Kept alive: dropping the Instance tears down backend displays
    // (EGL/Wayland) while surfaces from it still exist -> segfault.
    _instance: wgpu::Instance,
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    uniform_buf: wgpu::Buffer,
    instance_buf: wgpu::Buffer,
    instance_cap: usize,
    grid_w: u32,
    grid_h: u32,
}

struct App {
    source: Box<dyn SceneSource>,
    opts: PresentOptions,
    window: Option<Arc<winit::window::Window>>,
    gpu: Option<Gpu>,
    atlas: atlas::Atlas,
    started: Instant,
    last_frame: Instant,
    presented: u64,
    pressed_keys: HashSet<winit::keyboard::KeyCode>,
}

impl App {
    fn new(source: Box<dyn SceneSource>, opts: PresentOptions) -> Result<Self, String> {
        Ok(Self {
            source,
            atlas: atlas::build(opts.cell_px)?,
            opts,
            window: None,
            gpu: None,
            started: Instant::now(),
            last_frame: Instant::now(),
            presented: 0,
            pressed_keys: HashSet::new(),
        })
    }

    fn init_gpu(&mut self, window: Arc<winit::window::Window>) -> Result<(), String> {
        let size = window.inner_size();
        let (w, h) = (size.width.max(1), size.height.max(1));
        // NOTE: Vulkan-only. wgpu's GLES backend segfaults inside mesa's
        // eglTerminate while tearing down on some systems (verified with gdb:
        // wgpu_hal::gles drop at process exit), which turns even a clean
        // shutdown into SIGSEGV. Skipping GL keeps the exit path clean;
        // adapter-less machines get a readable error instead of a crash.
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::VULKAN,
            ..Default::default()
        });
        // SAFETY: surface outlives the window via the shared Arc.
        let surface = instance
            .create_surface(window.clone())
            .map_err(|e| format!("wgpu surface: {e}"))?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }))
        .ok_or_else(|| "no compatible GPU adapter for the window".to_string())?;
        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("ascii-presenter"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                memory_hints: wgpu::MemoryHints::Performance,
            },
            None,
        ))
        .map_err(|e| format!("wgpu device: {e}"))?;

        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .find(|f| f.is_srgb())
            .copied()
            .unwrap_or(caps.formats[0]);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: w.max(1),
            height: h.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        // Atlas texture with 256-byte row padding (write_texture requirement).
        let pad_w = atlas::ATLAS_COLS * self.atlas.cell_w;
        let pad_h = atlas::ATLAS_ROWS * self.atlas.cell_h;
        let pitch = ((pad_w + 255) / 256) * 256;
        let mut padded = vec![0u8; (pitch * pad_h) as usize];
        for row in 0..pad_h {
            let src = (row * self.atlas.stride) as usize;
            let dst = (row * pitch) as usize;
            padded[dst..dst + pad_w as usize]
                .copy_from_slice(&self.atlas.data[src..src + pad_w as usize]);
        }
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("glyph-atlas"),
            size: wgpu::Extent3d {
                width: pad_w,
                height: pad_h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        // Staging buffer rows are padded to 256 bytes (copy requirement);
        // the texture itself is exactly pad_w wide so UVs stay exact.
        let staging = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("atlas-staging"),
            size: padded.len() as u64,
            usage: wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&staging, 0, &padded);
        let mut up = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("atlas-upload"),
        });
        up.copy_buffer_to_texture(
            wgpu::TexelCopyBufferInfo {
                buffer: &staging,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(pitch),
                    rows_per_image: Some(pad_h),
                },
            },
            wgpu::TexelCopyTextureInfo {
                texture: &tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::Extent3d {
                width: pad_w,
                height: pad_h,
                depth_or_array_layers: 1,
            },
        );
        queue.submit([up.finish()]);
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("glyph-sampler"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let uniform_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("uniforms"),
            size: size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("present-bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("present-bg"),
            layout: &bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("glyph-shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("present-layout"),
            bind_group_layouts: &[&bgl],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("glyph-pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: size_of::<GlyphInstance>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &[
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Uint32x2,
                            offset: 0,
                            shader_location: 0,
                        },
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Uint32,
                            offset: 8,
                            shader_location: 1,
                        },
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x3,
                            offset: 12,
                            shader_location: 2,
                        },
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x3,
                            offset: 24,
                            shader_location: 3,
                        },
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x3,
                            offset: 36,
                            shader_location: 4,
                        },
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Uint32,
                            offset: 48,
                            shader_location: 5,
                        },
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32,
                            offset: 52,
                            shader_location: 6,
                        },
                    ],
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let (grid_w, grid_h) = grid_for_surface(w, h, self.atlas.cell_w, self.atlas.cell_h);
        let instance_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("instances"),
            size: instance_capacity(grid_w, grid_h),
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.window = Some(window);
        self.gpu = Some(Gpu {
            _instance: instance,
            device,
            queue,
            surface,
            config,
            pipeline,
            bind_group,
            uniform_buf,
            instance_buf,
            instance_cap: (grid_w * grid_h) as usize,
            grid_w,
            grid_h,
        });
        Ok(())
    }

    fn resize(&mut self, width: u32, height: u32) {
        let Some(gpu) = self.gpu.as_mut() else {
            return;
        };
        if width == 0 || height == 0 {
            return;
        }
        gpu.config.width = width;
        gpu.config.height = height;
        gpu.surface.configure(&gpu.device, &gpu.config);
        (gpu.grid_w, gpu.grid_h) =
            grid_for_surface(width, height, self.atlas.cell_w, self.atlas.cell_h);
        let required = (gpu.grid_w * gpu.grid_h) as usize;
        if required > gpu.instance_cap {
            gpu.instance_buf = gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("instances"),
                size: instance_capacity(gpu.grid_w, gpu.grid_h),
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            gpu.instance_cap = required;
        }
    }

    fn render(&mut self) -> Result<(), String> {
        let Some(gpu) = self.gpu.as_mut() else {
            return Ok(());
        };
        let t = self.started.elapsed().as_secs_f32();
        let (scene, camera) = self.source.scene_at_size(t, gpu.grid_w, gpu.grid_h);
        let frame = compose(&scene, &camera, t);

        let uniforms = Uniforms {
            grid_w: gpu.grid_w,
            grid_h: gpu.grid_h,
            surf_w: gpu.config.width,
            surf_h: gpu.config.height,
        };
        gpu.queue
            .write_buffer(&gpu.uniform_buf, 0, bytemuck::bytes_of(&uniforms));

        let instances = frame_instances(&frame, gpu.grid_w, gpu.grid_h);
        let need = instances.len().max(1);
        if need > gpu.instance_cap {
            gpu.instance_buf = gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("instances"),
                size: (need * size_of::<GlyphInstance>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            gpu.instance_cap = need;
        }
        gpu.queue
            .write_buffer(&gpu.instance_buf, 0, bytemuck::cast_slice(&instances));

        let output = match gpu.surface.get_current_texture() {
            Ok(frame) => frame,
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                gpu.surface.configure(&gpu.device, &gpu.config);
                return Ok(());
            }
            Err(wgpu::SurfaceError::Timeout) => return Ok(()),
            Err(wgpu::SurfaceError::Other) => return Ok(()),
            Err(wgpu::SurfaceError::OutOfMemory) => {
                return Err("wgpu surface out of memory".into());
            }
        };
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("glyph-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.03,
                            g: 0.04,
                            b: 0.08,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&gpu.pipeline);
            pass.set_bind_group(0, &gpu.bind_group, &[]);
            pass.set_vertex_buffer(0, gpu.instance_buf.slice(..));
            pass.draw(0..6, 0..instances.len() as u32);
        }
        gpu.queue.submit([encoder.finish()]);
        output.present();
        self.presented += 1;
        Ok(())
    }
}

/// Flatten a [`Frame`] into GPU instances, clipped to the live grid.
fn frame_instances(frame: &Frame, grid_w: u32, grid_h: u32) -> Vec<GlyphInstance> {
    let mut out = Vec::new();
    for y in 0..grid_h.min(frame.h.max(0) as u32) {
        for x in 0..grid_w.min(frame.w.max(0) as u32) {
            let Some(cell) = frame.get(x as i32, y as i32) else {
                continue;
            };
            if cell.transparent {
                continue;
            }
            let bold = if cell.bold { 1.25 } else { 1.0 };
            let f = |c: u8| ((f32::from(c) / 255.0) * bold).min(1.0);
            out.push(GlyphInstance {
                gx: x,
                gy: y,
                glyph: atlas::slot(cell.glyph),
                fg_start: [f(cell.fg.r), f(cell.fg.g), f(cell.fg.b)],
                fg_end: [f(cell.fg_end.r), f(cell.fg_end.g), f(cell.fg_end.b)],
                bg: [
                    f32::from(cell.bg.r) / 255.0,
                    f32::from(cell.bg.g) / 255.0,
                    f32::from(cell.bg.b) / 255.0,
                ],
                fg_gradient_axis: match cell.fg_gradient {
                    None => 0,
                    Some(super::Axis::X) => 1,
                    Some(super::Axis::Y) => 2,
                },
                scale: cell.scale.clamp(0.25, 2.0),
            });
        }
    }
    out
}

fn grid_for_surface(width: u32, height: u32, cell_w: u32, cell_h: u32) -> (u32, u32) {
    ((width / cell_w).max(1), (height / cell_h).max(1))
}

fn instance_capacity(grid_w: u32, grid_h: u32) -> u64 {
    (grid_w.max(1) as u64 * grid_h.max(1) as u64 * size_of::<GlyphInstance>() as u64).max(1)
}

const SHADER: &str = r#"
struct U { grid: vec2<u32>, surf: vec2<u32> };
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var atlas: texture_2d<f32>;
@group(0) @binding(2) var samp: sampler;

struct VOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) glyph_uv: vec2<f32>,
    @location(2) fg_start: vec3<f32>,
    @location(3) fg_end: vec3<f32>,
    @location(4) bg: vec3<f32>,
    @interpolate(flat) @location(5) gradient_axis: u32,
};

@vertex
fn vs(@builtin(vertex_index) vi: u32,
      @location(0) grid: vec2<u32>,
      @location(1) glyph: u32,
       @location(2) fg_start: vec3<f32>,
       @location(3) fg_end: vec3<f32>,
       @location(4) bg: vec3<f32>,
       @location(5) gradient_axis: u32,
       @location(6) size: f32) -> VOut {
    var c = vec2<f32>(0.0, 0.0);
    if (vi == 1u) { c = vec2<f32>(1.0, 0.0); }
    else if (vi == 2u || vi == 4u) { c = vec2<f32>(1.0, 1.0); }
    else if (vi == 5u) { c = vec2<f32>(0.0, 1.0); }
    // Centered billboard scale: 1.0 fills the cell, smaller floats inside it
    // (reference spark particles vary in size); the atlas UV stays full-glyph.
    let c0 = (c - 0.5) * size + 0.5;
    let cell = vec2<f32>(f32(u.surf.x) / f32(u.grid.x), f32(u.surf.y) / f32(u.grid.y));
    let pix = (vec2<f32>(grid) + c0) * cell;
    let ndc = vec2<f32>(pix.x / f32(u.surf.x), 1.0 - pix.y / f32(u.surf.y)) * 2.0 - vec2<f32>(1.0);
    let tile = vec2<f32>(f32(glyph % 32u), f32(glyph / 32u));
    var o: VOut;
    o.pos = vec4<f32>(ndc, 0.0, 1.0);
    o.uv = (tile + c) / vec2<f32>(32.0, 16.0);
    o.glyph_uv = c;
    o.fg_start = fg_start;
    o.fg_end = fg_end;
    o.bg = bg;
    o.gradient_axis = gradient_axis;
    return o;
}

@fragment
fn fs(i: VOut) -> @location(0) vec4<f32> {
    let a = textureSample(atlas, samp, i.uv).r;
    let position = select(i.glyph_uv.x, i.glyph_uv.y, i.gradient_axis == 2u);
    let blend = select(0.0, position, i.gradient_axis != 0u);
    let fg = mix(i.fg_start, i.fg_end, blend);
    return vec4<f32>(mix(i.bg, fg, a), 1.0);
}
"#;

impl winit::application::ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        event_loop.set_control_flow(winit::event_loop::ControlFlow::Poll);
        let attrs = winit::window::Window::default_attributes()
            .with_title(self.opts.title.clone())
            .with_resizable(true);
        let attrs = if self.opts.width_cells == 0 || self.opts.height_cells == 0 {
            attrs.with_fullscreen(Some(winit::window::Fullscreen::Borderless(None)))
        } else {
            attrs.with_inner_size(winit::dpi::LogicalSize::new(
                self.opts.width_cells * self.atlas.cell_w,
                self.opts.height_cells * self.atlas.cell_h,
            ))
        };
        match event_loop.create_window(attrs) {
            Ok(window) => {
                if let Err(err) = self.init_gpu(Arc::new(window)) {
                    eprintln!("presenter GPU init failed: {err}");
                    event_loop.exit();
                }
            }
            Err(err) => {
                eprintln!("presenter window creation failed: {err}");
                event_loop.exit();
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        _id: winit::window::WindowId,
        event: winit::event::WindowEvent,
    ) {
        use winit::event::WindowEvent;
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::KeyboardInput { event, .. } => {
                use winit::event::ElementState;
                use winit::keyboard::{KeyCode, PhysicalKey};
                if let PhysicalKey::Code(code) = event.physical_key {
                    let pressed = event.state == ElementState::Pressed;
                    // Repeat is an additional press notification, not a new
                    // edge. Releases are forwarded only for keys we observed
                    // being pressed, so every source key gets one edge pair.
                    if pressed && event.repeat {
                        return;
                    }
                    if pressed {
                        if !self.pressed_keys.insert(code) {
                            return;
                        }
                    } else if !self.pressed_keys.remove(&code) {
                        return;
                    }
                    if pressed && matches!(code, KeyCode::Escape | KeyCode::KeyQ) {
                        event_loop.exit();
                        return;
                    }
                    if pressed && code == KeyCode::F11 {
                        let size = {
                            let window = self.window.as_ref().expect("keyboard event window");
                            let fullscreen = if window.fullscreen().is_some() {
                                None
                            } else {
                                Some(winit::window::Fullscreen::Borderless(
                                    window.current_monitor(),
                                ))
                            };
                            window.set_fullscreen(fullscreen);
                            window.inner_size()
                        };
                        self.resize(size.width, size.height);
                        return;
                    }
                    if code == KeyCode::F11 {
                        return;
                    }
                    self.source.key(code, pressed);
                }
            }
            WindowEvent::Resized(size) => self.resize(size.width, size.height),
            WindowEvent::RedrawRequested => {
                let budget = if self.opts.fps == 0 {
                    Duration::ZERO
                } else {
                    Duration::from_nanos(1_000_000_000 / self.opts.fps as u64)
                };
                if self.last_frame.elapsed() >= budget {
                    self.last_frame = Instant::now();
                    if let Err(err) = self.render() {
                        eprintln!("presenter render failed: {err}");
                        event_loop.exit();
                        return;
                    }
                    if let Some(max) = self.opts.max_frames {
                        if self.presented >= max {
                            event_loop.exit();
                            return;
                        }
                    }
                }
                if let Some(window) = self.window.as_ref() {
                    window.request_redraw();
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &winit::event_loop::ActiveEventLoop) {
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{frame_instances, grid_for_surface, instance_capacity, GlyphInstance, SHADER};
    use crate::{Axis, Cell, Frame, Rgb};

    #[test]
    fn grid_uses_surface_pixels_and_keeps_small_surfaces_visible() {
        assert_eq!(grid_for_surface(1920, 1080, 20, 32), (96, 33));
        assert_eq!(grid_for_surface(3, 3, 20, 32), (1, 1));
    }

    #[test]
    fn instance_capacity_covers_every_grid_cell() {
        let bytes = instance_capacity(80, 25);
        assert_eq!(bytes, 80 * 25 * std::mem::size_of::<GlyphInstance>() as u64);
        assert!(instance_capacity(0, 0) >= std::mem::size_of::<GlyphInstance>() as u64);
    }

    #[test]
    fn instance_propagates_gradient_endpoints_and_axis() {
        let mut frame = Frame::blank(1, 1);
        frame.cells[0] = Cell::new('@', Rgb::new(10, 20, 30), Rgb::new(0, 0, 0))
            .with_fg_gradient(Rgb::new(200, 210, 220), Axis::Y);
        let instances = frame_instances(&frame, 1, 1);
        assert_eq!(
            instances[0].fg_start,
            [10.0 / 255.0, 20.0 / 255.0, 30.0 / 255.0]
        );
        assert_eq!(
            instances[0].fg_end,
            [200.0 / 255.0, 210.0 / 255.0, 220.0 / 255.0]
        );
        assert_eq!(instances[0].fg_gradient_axis, 2);
    }

    #[test]
    fn shader_contract_interpolates_glyph_uv_and_preserves_flat_cells() {
        assert!(SHADER.contains("mix(i.fg_start, i.fg_end, blend)"));
        assert!(SHADER.contains("i.glyph_uv.y"));
        assert!(SHADER.contains("i.gradient_axis != 0u"));
    }
}
