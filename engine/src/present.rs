//! Windowed presenter: opens a real OS window and draws engine [`Frame`]s
//! with a GPU glyph atlas (one instanced quad per cell, atlas built by
//! [`atlas`]). Runs anywhere a window can open — no terminal required.
//!
//! `run` presents one static scene; `run_dynamic` takes a [`SceneSource`]
//! for live scenes plus key input (see the `play` example).

pub mod atlas;
pub mod gpu3d;

use super::overlay::{tessellate, text_quads, OverlayVert, Stroke, TextRun};
use super::{compose, Camera, Frame, Scene};
use crate::raster::{Camera3, DepthOpts, Mesh, Vec3};
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;

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
    /// Frame pacing: 0 = uncapped (Immediate present, benchmark mode).
    /// Nonzero = vsync-paced at the display rate (AutoVsync). Note this is
    /// deliberately NOT a skip-based budget gate on top of vsync: gating
    /// rendered frames against the clock while present() also blocks on
    /// vsync phase-slips to every-other-vsync (measured 29fps instead of
    /// 60 on a 60Hz display), so capped mode renders every redraw and lets
    /// vsync itself set the rate.
    pub fps: u32,
    /// Exit after this many presented frames (automated proof without keys).
    pub max_frames: Option<u64>,
}

impl Default for PresentOptions {
    fn default() -> Self {
        Self {
            title: "ASCII Engine".into(),
            cell_px: 16.0,
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
/// The GPU raster contract handed over by a [`SceneSource`] after its
/// `scene_at_size` call (the source advances its simulation there, so the
/// meshes/camera are the frame's truth). `layer_slot` names the placeholder
/// layer index the source pushed in the scene; the presenter runs the GPU
/// pass and swaps the resolved layer into that slot before `compose` runs.
pub struct GpuWorld {
    pub meshes: Vec<Mesh>,
    pub camera: Camera3,
    /// Flat-shading sun direction (normalized inside the pass).
    pub sun: Vec3,
    pub opts: DepthOpts,
    /// Layer depth tag for the produced layer.
    pub z: i32,
    /// Index of the placeholder `scene.layers` entry to replace.
    pub layer_slot: usize,
}

pub trait SceneSource {
    /// Current scene and camera. Called once per presented frame.
    fn scene_at(&mut self, t: f32) -> (Scene, Camera);
    /// Variant that can adapt a scene to the live glyph grid. The default
    /// preserves sources that use a fixed world/UI layout.
    fn scene_at_size(&mut self, t: f32, _width: u32, _height: u32) -> (Scene, Camera) {
        self.scene_at(t)
    }
    /// Meshes for the GPU raster path. Called right after `scene_at_size`
    /// of the same frame; `None` keeps every source on the CPU raster.
    /// Sources that skip the CPU fill in `scene_at_size` return `Some`
    /// here and let the presenter swap the GPU-resolved world into their
    /// placeholder layer slot (see [`GpuWorld`]).
    fn gpu_world(&mut self) -> Option<GpuWorld> {
        None
    }
    /// Key press (`pressed`) / release. Esc/Q and close always exit in
    /// addition to this; return value is reserved (currently ignored).
    fn key(&mut self, _code: winit::keyboard::KeyCode, _pressed: bool) {}
    /// Pointer feedback: button edges, cursor deltas, and wheel ticks in
    /// one event (OSRS-style camera orbit wants exactly these three).
    /// Buttons: slot 0 = left, slot 1 = right. Cursor deltas are physical
    /// pixels since the previous pointer event. No-op by default.
    fn pointer(&mut self, _event: PointerEvent) {}
    /// Vector overlay strokes (lightning, arcs) for this frame, in frame
    /// grid cells. Empty by default; the presenter tessellates them into
    /// glow quads under the glyphs and bright cores over them.
    fn strokes(&mut self, _t: f32) -> Vec<Stroke> {
        Vec::new()
    }
}

/// Pointer state snapshot handed to [`SceneSource::pointer`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointerEvent {
    /// Buttons currently held: 0 = left, 1 = right.
    pub held: [bool; 2],
    /// Cursor delta in physical pixels since the previous pointer event
    /// (zero for button and wheel events, which do not move the cursor).
    pub delta: (f32, f32),
    /// Signed wheel ticks this event (positive = scroll up).
    pub wheel: f32,
}

/// App-side pointer bookkeeping: last cursor position and held buttons.
#[derive(Default)]
struct PointerTrack {
    last_pos: Option<(f64, f64)>,
    held: [bool; 2],
}

impl PointerTrack {
    /// Handle a button edge and return the event to forward.
    fn button(&mut self, slot: usize, pressed: bool) -> Option<PointerEvent> {
        self.held[slot] = pressed;
        Some(PointerEvent {
            held: self.held,
            delta: (0.0, 0.0),
            wheel: 0.0,
        })
    }

    /// Handle a cursor move: accumulate the delta into this event.
    fn moved(&mut self, x: f64, y: f64) -> Option<PointerEvent> {
        let delta = match self.last_pos {
            Some((lx, ly)) => ((x - lx) as f32, (y - ly) as f32),
            None => (0.0, 0.0),
        };
        self.last_pos = Some((x, y));
        Some(PointerEvent {
            held: self.held,
            delta,
            wheel: 0.0,
        })
    }

    /// Handle a wheel tick line event.
    fn wheel(&mut self, ticks: f32) -> Option<PointerEvent> {
        Some(PointerEvent {
            held: self.held,
            delta: (0.0, 0.0),
            wheel: ticks,
        })
    }
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
    /// 1 = world layer (rides the fractional camera), 0 = pinned UI.
    world: u32,
    offset: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    grid_w: u32,
    grid_h: u32,
    surf_w: u32,
    surf_h: u32,
    /// Fullscreen wash tint (linear 0..1) and strength; warms the void and
    /// every quad like the reference amber washes.
    wash_rgb: [f32; 3],
    wash_strength: f32,
    /// Sub-cell camera glide offset in grid cells (fractional follow).
    cam: [f32; 2],
    /// Explicit pad: WGSL uniform structs round up to a 16-byte multiple
    /// (40 bytes of fields need a 48-byte buffer).
    _pad: [f32; 2],
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
    overlay_pipe: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    uniform_buf: wgpu::Buffer,
    instance_buf: wgpu::Buffer,
    instance_cap: usize,
    overlay_buf: wgpu::Buffer,
    overlay_cap: usize,
    grid_w: u32,
    grid_h: u32,
    /// GPU triangle raster for the 3D world (`SceneSource::gpu_world`).
    world3d: gpu3d::World3d,
}

struct App {
    source: Box<dyn SceneSource>,
    opts: PresentOptions,
    window: Option<Arc<winit::window::Window>>,
    gpu: Option<Gpu>,
    atlas: atlas::Atlas,
    /// 2x raster of the same font/slots for crisp overlay text (big text
    /// samples this 1:1 instead of stretching base tiles chunky).
    atlas2x: atlas::Atlas,
    started: Instant,
    presented: u64,
    pressed_keys: HashSet<winit::keyboard::KeyCode>,
    /// Pointer bookkeeping feeding [`SceneSource::pointer`].
    pointer: PointerTrack,
    /// Smoothed frames-per-second shown in the corner readout.
    fps_ema: f32,
    prev_frame: Option<Instant>,
}

impl App {
    fn new(source: Box<dyn SceneSource>, opts: PresentOptions) -> Result<Self, String> {
        Ok(Self {
            source,
            atlas: atlas::build(opts.cell_px)?,
            atlas2x: atlas::build(opts.cell_px * 2.0)?,
            opts,
            window: None,
            gpu: None,
            started: Instant::now(),
            presented: 0,
            pressed_keys: HashSet::new(),
            pointer: PointerTrack::default(),
            fps_ema: 60.0,
            prev_frame: None,
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
        let info = adapter.get_info();
        eprintln!(
            "[present] adapter={} backend={:?} device={:?}",
            info.name, info.backend, info.device_type
        );
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
            // Capped budgets ride vsync; uncapped (fps 0) benchmarks run
            // tear-free off via Immediate so the CPU compose cost is visible.
            present_mode: if self.opts.fps == 0 {
                wgpu::PresentMode::Immediate
            } else {
                wgpu::PresentMode::AutoVsync
            },
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
        // 2x overlay atlas: same slots, double raster — big text samples
        // 1:1 instead of stretching base tiles chunky.
        let view2x = upload_atlas(&device, &queue, &self.atlas2x, "glyph-atlas-2x");
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
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
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
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
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
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&view2x),
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
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Uint32,
                            offset: 56,
                            shader_location: 7,
                        },
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x2,
                            offset: 60,
                            shader_location: 8,
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
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        let overlay_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("overlay-shader"),
            source: wgpu::ShaderSource::Wgsl(OVERLAY_SHADER.into()),
        });
        let overlay_pipe = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("overlay-pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &overlay_shader,
                entry_point: Some("ovs"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: size_of::<OverlayVert>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x2,
                            offset: 0,
                            shader_location: 0,
                        },
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x3,
                            offset: 8,
                            shader_location: 1,
                        },
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32,
                            offset: 20,
                            shader_location: 2,
                        },
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Uint32,
                            offset: 24,
                            shader_location: 3,
                        },
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x2,
                            offset: 28,
                            shader_location: 4,
                        },
                    ],
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: &overlay_shader,
                entry_point: Some("ofs"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
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
        let world3d = gpu3d::World3d::new(&device)?;
        let overlay_cap = 4096usize;
        let overlay_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("overlay"),
            size: (overlay_cap * size_of::<OverlayVert>()) as u64,
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
            overlay_pipe,
            bind_group,
            uniform_buf,
            instance_buf,
            instance_cap: (grid_w * grid_h) as usize,
            overlay_buf,
            overlay_cap,
            grid_w,
            grid_h,
            world3d,
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
        let t0 = std::time::Instant::now();
        let (mut scene, camera) = self.source.scene_at_size(t, gpu.grid_w, gpu.grid_h);
        // GPU world raster: the source (e.g. `play`) skipped the CPU fill
        // and pushed a placeholder layer; run the triangle pass and swap
        // the resolved layer in before compose. `None` = CPU raster frame.
        let mut gpu3d_ms: Option<std::time::Duration> = None;
        if let Some(world) = self.source.gpu_world() {
            let t3d = std::time::Instant::now();
            let layer = gpu
                .world3d
                .run(&gpu.device, &gpu.queue, &world)
                .map_err(|e| format!("gpu world raster: {e}"))?;
            if world.layer_slot < scene.layers.len() {
                scene.layers[world.layer_slot] = layer;
            }
            gpu3d_ms = Some(t3d.elapsed());
        }
        let t1 = std::time::Instant::now();
        let frame = compose(&scene, &camera, t);
        let t2 = std::time::Instant::now();

        let (wash_rgb, wash_strength) = match scene.wash {
            Some(wash) => (
                [
                    f32::from(wash.tint.r) / 255.0,
                    f32::from(wash.tint.g) / 255.0,
                    f32::from(wash.tint.b) / 255.0,
                ],
                wash.strength.clamp(0.0, 1.0),
            ),
            None => ([0.0, 0.0, 0.0], 0.0),
        };
        let uniforms = Uniforms {
            grid_w: gpu.grid_w,
            grid_h: gpu.grid_h,
            surf_w: gpu.config.width,
            surf_h: gpu.config.height,
            wash_rgb,
            wash_strength,
            cam: [camera.ox, camera.oy],
            _pad: [0.0, 0.0],
        };
        gpu.queue
            .write_buffer(&gpu.uniform_buf, 0, bytemuck::bytes_of(&uniforms));

        let t_inst = std::time::Instant::now();
        let instances = {
            let rects: Vec<(i32, i32, i32, i32)> = scene
                .partitions
                .iter()
                .filter(|p| p.camera)
                .map(|p| (p.x, p.y, p.w, p.h))
                .collect();
            frame_instances(&frame, gpu.grid_w, gpu.grid_h, &rects)
        };
        let inst_count = instances.len();
        let inst_d = t_inst.elapsed();
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

        // Vector overlay: halo quads paint first (under glyphs), bright
        // cores last (over glyphs). One buffer, two draw ranges.
        let strokes = self.source.strokes(t);
        let mut halo: Vec<OverlayVert> = Vec::new();
        let mut core: Vec<OverlayVert> = Vec::new();
        for stroke in &strokes {
            let (mut h, mut c) = tessellate(stroke);
            halo.append(&mut h);
            core.append(&mut c);
        }
        let halo_count = halo.len() as u32;
        let core_count = core.len() as u32;
        halo.extend(core);
        // Crisp overlay text (boss/target readouts) draws over everything.
        let mut text: Vec<OverlayVert> = Vec::new();
        for run in &scene.overlay_text {
            text.extend(text_quads(run, atlas::slot));
        }
        // Live FPS readout, top-left corner.
        let now = Instant::now();
        if let Some(prev) = self.prev_frame {
            let dt = (now - prev).as_secs_f32().max(1e-3);
            self.fps_ema += (1.0 / dt - self.fps_ema) * 0.1;
        }
        self.prev_frame = Some(now);
        text.extend(text_quads(
            &TextRun {
                x: 1.0,
                y: 0.0,
                text: format!("{} FPS", self.fps_ema.round() as u32),
                size: 2.0,
                fg: [0.75, 0.85, 0.95],
                bg: [0.03, 0.04, 0.07],
            },
            atlas::slot,
        ));
        let text_count = text.len() as u32;
        halo.extend(text);
        let over_count = core_count + text_count;
        let need_overlay = halo.len().max(1);
        if need_overlay > gpu.overlay_cap {
            gpu.overlay_buf = gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("overlay"),
                size: (need_overlay * size_of::<OverlayVert>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            gpu.overlay_cap = need_overlay;
        }
        gpu.queue
            .write_buffer(&gpu.overlay_buf, 0, bytemuck::cast_slice(&halo));

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
                            // Void must read darkest: below vignetted ground
                            // so empty blocks sink instead of glowing.
                            r: 0.005,
                            g: 0.006,
                            b: 0.012,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&gpu.overlay_pipe);
            pass.set_bind_group(0, &gpu.bind_group, &[]);
            pass.set_vertex_buffer(0, gpu.overlay_buf.slice(..));
            pass.draw(0..halo_count, 0..1);
            pass.set_pipeline(&gpu.pipeline);
            pass.set_bind_group(0, &gpu.bind_group, &[]);
            pass.set_vertex_buffer(0, gpu.instance_buf.slice(..));
            pass.draw(0..6, 0..instances.len() as u32);
            pass.set_pipeline(&gpu.overlay_pipe);
            pass.set_bind_group(0, &gpu.bind_group, &[]);
            pass.set_vertex_buffer(0, gpu.overlay_buf.slice(..));
            pass.draw(halo_count..(halo_count + over_count), 0..1);
        }
        gpu.queue.submit([encoder.finish()]);
        output.present();
        self.presented += 1;
        if std::env::var("PRESENT_PROFILE").is_ok() && self.presented % 60 == 0 {
            eprintln!(
                "[profile] build={:?} world3d={:?} compose={:?} inst={:?} n={} grid={}x{}",
                t1.duration_since(t0),
                gpu3d_ms.unwrap_or_default(),
                t2.duration_since(t1),
                inst_d,
                inst_count,
                gpu.grid_w,
                gpu.grid_h
            );
        }
        Ok(())
    }
}

/// Exponential camera follow: eases the float follow-point toward the
/// integer target and splits it into an integer cell plus a sub-cell glide
/// fraction (examples feed the result back every frame; `None` snaps).
/// Returns the new follow-point and the camera to present.
pub fn glide_camera(
    current: Option<(f32, f32)>,
    tx: f32,
    ty: f32,
    dt: f32,
) -> ((f32, f32), crate::Camera) {
    let (cx, cy) = current.unwrap_or((tx, ty));
    let k = 1.0 - (-dt.max(0.0) * 6.0).exp();
    let nx = cx + (tx - cx) * k;
    let ny = cy + (ty - cy) * k;
    let cam = crate::Camera {
        x: nx.floor() as i32,
        y: ny.floor() as i32,
        ox: nx - nx.floor(),
        oy: ny - ny.floor(),
    };
    ((nx, ny), cam)
}

/// Flatten a [`Frame`] into GPU instances, clipped to the live grid.
/// `world` carries the camera-following partition rects (frame coords);
/// those instances ride the fractional glide, pinned UI does not.
///
/// Emission is row-banded across a persistent worker pool: per-cell output
/// is independent (unlike `compose`, whose layer overwrites are
/// order-dependent and stays serial), and bands concatenate in order so the
/// result is bit-identical to a serial loop. The pool is intentional:
/// spawning threads per frame cost ~1ms on its own (measured release
/// regression), so workers are created once and park on a channel.
fn frame_instances(
    frame: &Frame,
    grid_w: u32,
    grid_h: u32,
    world: &[(i32, i32, i32, i32)],
) -> Vec<GlyphInstance> {
    let cols = grid_w.min(frame.w.max(0) as u32);
    let rows = grid_h.min(frame.h.max(0) as u32);
    if cols == 0 || rows == 0 {
        return Vec::new();
    }
    // Main thread takes band 0, pool workers take the rest (4 bands max:
    // more lanes never paid off in profiles).
    let lanes = (rows as usize).min(4);
    if lanes == 1 {
        return frame_band(frame, world, 0, rows, cols);
    }
    let pool = BandPool::get();
    let lanes = lanes.min(pool.jobs.len() + 1);
    let mut edges = Vec::with_capacity(lanes + 1);
    for i in 0..=lanes {
        edges.push((rows as usize * i / lanes) as u32);
    }
    for i in 1..lanes {
        let job = RawBandJob {
            frame: FramePtr(frame as *const Frame),
            world: world.to_vec(),
            y0: edges[i],
            y1: edges[i + 1],
            cols,
        };
        // A dead worker fails here AND on recv below; either way the band
        // falls back to serial, never to a stall.
        let _ = pool.jobs[i - 1].send(job);
    }
    // Pre-sized upper bound (one instance per covered cell at most).
    let mut out = Vec::with_capacity((cols * rows) as usize);
    out.extend(frame_band(frame, world, edges[0], edges[1], cols));
    for i in 1..lanes {
        let band = pool.results[i - 1]
            .lock()
            .expect("band result lock")
            .recv()
            .unwrap_or_else(|_| frame_band(frame, world, edges[i], edges[i + 1], cols));
        out.extend(band);
    }
    out
}

/// Long-lived row-band workers (see [`frame_instances`]).
struct BandPool {
    jobs: Vec<std::sync::mpsc::Sender<RawBandJob>>,
    // Receivers are `Send` but not `Sync`; the mutex (uncontended: only the
    // presenting thread ever receives) makes the static pool shareable.
    results: Vec<std::sync::Mutex<std::sync::mpsc::Receiver<Vec<GlyphInstance>>>>,
}

/// Opaque frame handle for band jobs. Raw because the pool outlives any
/// single frame; workers only dereference it while the caller is blocked
/// collecting results, so the borrow is always live (see SAFETY at the use
/// site).
struct FramePtr(*const Frame);

// SAFETY: the pointer never outlives the frame it points to — every job's
// result is collected before `frame_instances` returns, and workers retain
// nothing past one job.
unsafe impl Send for FramePtr {}

/// One band work item.
struct RawBandJob {
    frame: FramePtr,
    world: Vec<(i32, i32, i32, i32)>,
    y0: u32,
    y1: u32,
    cols: u32,
}

impl BandPool {
    fn get() -> &'static BandPool {
        static POOL: std::sync::OnceLock<BandPool> = std::sync::OnceLock::new();
        POOL.get_or_init(|| BandPool::spawn(3))
    }

    fn spawn(workers: usize) -> BandPool {
        let mut jobs = Vec::with_capacity(workers);
        let mut results = Vec::with_capacity(workers);
        for _ in 0..workers {
            let (job_tx, job_rx) = std::sync::mpsc::channel::<RawBandJob>();
            let (res_tx, res_rx) = std::sync::mpsc::channel::<Vec<GlyphInstance>>();
            std::thread::spawn(move || {
                for job in job_rx {
                    // SAFETY: the caller collects every band result before
                    // its frame is dropped, so the pointer is live for the
                    // whole dereference. Workers never retain it past one job.
                    let frame = unsafe { &*job.frame.0 };
                    let out = frame_band(frame, &job.world, job.y0, job.y1, job.cols);
                    if res_tx.send(out).is_err() {
                        break;
                    }
                }
            });
            jobs.push(job_tx);
            results.push(std::sync::Mutex::new(res_rx));
        }
        BandPool { jobs, results }
    }
}

/// One row band of [`frame_instances`]: serially emits every visible cell in
/// rows `y0..y1`.
fn frame_band(
    frame: &Frame,
    world: &[(i32, i32, i32, i32)],
    y0: u32,
    y1: u32,
    cols: u32,
) -> Vec<GlyphInstance> {
    let mut out = Vec::with_capacity(((y1 - y0) * cols) as usize);
    for y in y0..y1 {
        for x in 0..cols {
            let Some(cell) = frame.get(x as i32, y as i32) else {
                continue;
            };
            if cell.transparent {
                continue;
            }
            let xi = x as i32;
            let yi = y as i32;
            let in_world = world
                .iter()
                .any(|&(px, py, pw, ph)| xi >= px && xi < px + pw && yi >= py && yi < py + ph);
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
                world: in_world as u32,
                offset: [cell.offset_x, cell.offset_y],
            });
        }
    }
    out
}

fn grid_for_surface(width: u32, height: u32, cell_w: u32, cell_h: u32) -> (u32, u32) {
    ((width / cell_w).max(1), (height / cell_h).max(1))
}

/// Upload one atlas (256-byte padded rows) and return its texture view.
/// Shared by the base and 2x overlay atlases.
fn upload_atlas(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    atlas: &atlas::Atlas,
    label: &str,
) -> wgpu::TextureView {
    let pad_w = atlas::ATLAS_COLS * atlas.cell_w;
    let pad_h = atlas::ATLAS_ROWS * atlas.cell_h;
    let pitch = ((pad_w + 255) / 256) * 256;
    let mut padded = vec![0u8; (pitch * pad_h) as usize];
    for row in 0..pad_h {
        let src = (row * atlas.stride) as usize;
        let dst = (row * pitch) as usize;
        padded[dst..dst + pad_w as usize].copy_from_slice(&atlas.data[src..src + pad_w as usize]);
    }
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
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
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &tex,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &padded,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(pitch),
            rows_per_image: Some(pad_h),
        },
        wgpu::Extent3d {
            width: pad_w,
            height: pad_h,
            depth_or_array_layers: 1,
        },
    );
    tex.create_view(&wgpu::TextureViewDescriptor::default())
}

fn instance_capacity(grid_w: u32, grid_h: u32) -> u64 {
    (grid_w.max(1) as u64 * grid_h.max(1) as u64 * size_of::<GlyphInstance>() as u64).max(1)
}

const SHADER: &str = r#"
struct U {
  grid: vec2<u32>,
  surf: vec2<u32>,
  wash_rgb: vec3<f32>,
  wash_strength: f32,
  cam: vec2<f32>,
};
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
       @location(6) size: f32,
      @location(7) world: u32,
      @location(8) motion_offset: vec2<f32>) -> VOut {
    var c = vec2<f32>(0.0, 0.0);
    if (vi == 1u) { c = vec2<f32>(1.0, 0.0); }
    else if (vi == 2u || vi == 4u) { c = vec2<f32>(1.0, 1.0); }
    else if (vi == 5u) { c = vec2<f32>(0.0, 1.0); }
    // Centered billboard scale: 1.0 fills the cell, smaller floats inside it
    // (reference spark particles vary in size); the atlas UV stays full-glyph.
    let c0 = (c - 0.5) * size + 0.5;
    let cell = vec2<f32>(f32(u.surf.x) / f32(u.grid.x), f32(u.surf.y) / f32(u.grid.y));
    // Fractional follow: the scene composes on integer cells while the
    // view glides between them (no cell-stepping while walking).
    // Fractional follow: world instances glide between cells while pinned
    // UI stays put (no cell-stepping while walking).
    let shift = select(vec2<f32>(0.0, 0.0), u.cam, world != 0u);
    let pix = (vec2<f32>(grid) + motion_offset - shift + c0) * cell;
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
    let col = mix(i.bg, fg, a);
    return vec4<f32>(mix(col, u.wash_rgb, u.wash_strength), 1.0);
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
            WindowEvent::MouseInput { state, button, .. } => {
                use winit::event::{ElementState, MouseButton};
                // Orbit listens to left/right only; middle/back buttons are
                // noise for this presenter contract.
                let slot = match button {
                    MouseButton::Left => 0,
                    MouseButton::Right => 1,
                    _ => return,
                };
                if let Some(ev) = self.pointer.button(slot, state == ElementState::Pressed) {
                    self.source.pointer(ev);
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                if let Some(ev) = self.pointer.moved(position.x, position.y) {
                    self.source.pointer(ev);
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let ticks = match delta {
                    winit::event::MouseScrollDelta::LineDelta(_, y) => y,
                    winit::event::MouseScrollDelta::PixelDelta(pos) => pos.y as f32 / 24.0,
                };
                if ticks != 0.0 {
                    if let Some(ev) = self.pointer.wheel(ticks) {
                        self.source.pointer(ev);
                    }
                }
            }
            WindowEvent::Resized(size) => self.resize(size.width, size.height),
            WindowEvent::RedrawRequested => {
                // Vsync paces capped mode (see PresentOptions::fps): render
                // every redraw, no clock gate.
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

const OVERLAY_SHADER: &str = r#"
struct U {
  grid: vec2<u32>,
  surf: vec2<u32>,
  wash_rgb: vec3<f32>,
  wash_strength: f32,
  cam: vec2<f32>,
  _pad: vec2<f32>,
};
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var atlas: texture_2d<f32>;
@group(0) @binding(2) var samp: sampler;
@group(0) @binding(3) var atlas2x: texture_2d<f32>;

struct OOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) col: vec3<f32>,
    @location(1) alpha: f32,
    @location(2) uv: vec2<f32>,
    @location(3) glyph: u32,
};

@vertex
fn ovs(@location(0) pos: vec2<f32>,
       @location(1) col: vec3<f32>,
       @location(2) alpha: f32,
       @location(3) glyph: u32,
       @location(4) uv: vec2<f32>) -> OOut {
    let cell = vec2<f32>(f32(u.surf.x) / f32(u.grid.x), f32(u.surf.y) / f32(u.grid.y));
    let pix = pos * cell;
    let ndc = vec2<f32>(pix.x / f32(u.surf.x), 1.0 - pix.y / f32(u.surf.y)) * 2.0 - vec2<f32>(1.0);
    var o: OOut;
    o.pos = vec4<f32>(ndc, 0.0, 1.0);
    o.col = col;
    o.alpha = alpha;
    o.uv = uv;
    o.glyph = glyph;
    return o;
}

@fragment
fn ofs(i: OOut) -> @location(0) vec4<f32> {
    if (i.glyph == 4294967295u) {
        return vec4<f32>(mix(i.col, u.wash_rgb, u.wash_strength), i.alpha);
    }
    let tile = vec2<f32>(f32(i.glyph % 32u), f32(i.glyph / 32u));
    let a = textureSample(atlas2x, samp, (tile + i.uv) / vec2<f32>(32.0, 16.0)).r;
    let col = i.col * a;
    return vec4<f32>(mix(col, u.wash_rgb, u.wash_strength), a * i.alpha);
}
"#;

#[cfg(test)]
mod tests {
    use super::{
        frame_instances, glide_camera, grid_for_surface, instance_capacity, GlyphInstance,
        OVERLAY_SHADER, SHADER,
    };
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
        let instances = frame_instances(&frame, 1, 1, &[(0, 0, 1, 1)]);
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
    fn banded_instances_stay_row_major_and_deterministic() {
        // 4x3 frame, one transparent hole: banded threads must emit the
        // same row-major order as the old serial loop, every run.
        let mut frame = Frame::blank(4, 3);
        for cell in frame.cells.iter_mut() {
            *cell = Cell::new('x', Rgb::new(10, 20, 30), Rgb::new(0, 0, 0));
        }
        frame.cells[5].transparent = true;
        let world = [(0, 0, 2, 3)];
        let first = frame_instances(&frame, 4, 3, &world);
        assert_eq!(first.len(), 11);
        let mut prev = (0u32, 0u32);
        for (n, inst) in first.iter().enumerate() {
            if n > 0 {
                assert!((inst.gy, inst.gx) > prev, "instances must stay row-major");
            }
            prev = (inst.gy, inst.gx);
            let in_world = inst.gx < 2;
            assert_eq!(inst.world, in_world as u32);
        }
        // No transparent instance, and repeat runs agree exactly.
        assert!(first.iter().all(|i| !(i.gx == 1 && i.gy == 1)));
        let second = frame_instances(&frame, 4, 3, &world);
        assert_eq!(
            first.iter().map(|i| (i.gx, i.gy)).collect::<Vec<_>>(),
            second.iter().map(|i| (i.gx, i.gy)).collect::<Vec<_>>()
        );
        // Degenerate grids emit nothing instead of panicking (0-row bands).
        assert!(frame_instances(&frame, 0, 3, &world).is_empty());
        assert!(frame_instances(&frame, 4, 0, &world).is_empty());
    }

    #[test]
    fn pointer_track_emits_button_move_and_wheel_edges() {
        use super::PointerTrack;
        let mut track = PointerTrack::default();
        // Wheel: single slot with held-up ticks, zero delta.
        let wheel = track.wheel(1.0).expect("wheel event");
        assert!(!wheel.held[0] && !wheel.held[1]);
        assert_eq!(wheel.delta, (0.0, 0.0));
        assert_eq!(wheel.wheel, 1.0);
        // Right press flips the held slot; drag then reports it with deltas.
        let down = track.button(1, true).expect("button event");
        assert!(down.held[1], "right slot held");
        assert_eq!(down.delta, (0.0, 0.0));
        let drag = track.moved(400.0, 300.0).expect("first move");
        assert_eq!(drag.delta, (0.0, 0.0), "first move has no previous cursor");
        let drag = track.moved(364.0, 300.0).expect("second move");
        assert_eq!(drag.delta, (-36.0, 0.0));
        assert!(drag.held[1], "held slot sticks across moves");
        // Release clears the slot; left wheel of 0 is dropped by the caller.
        let up = track.button(1, false).expect("button event");
        assert!(!up.held[1]);
        track.last_pos = Some((10.0, 10.0));
        let tick = track.wheel(-2.0).expect("wheel event");
        assert_eq!(tick.wheel, -2.0);
        assert_eq!(
            tick.delta,
            (0.0, 0.0),
            "wheel events never carry cursor deltas"
        );
    }

    #[test]
    fn shader_contract_interpolates_glyph_uv_and_preserves_flat_cells() {
        assert!(SHADER.contains("mix(i.fg_start, i.fg_end, blend)"));
        assert!(SHADER.contains("i.glyph_uv.y"));
        assert!(SHADER.contains("i.gradient_axis != 0u"));
    }

    #[test]
    fn overlay_shader_blends_strokes_over_glyphs() {
        assert!(OVERLAY_SHADER.contains("mix(i.col, u.wash_rgb, u.wash_strength)"));
        // Halo and core share one buffer: the test module proves the split
        // (see overlay::tests), the shader just draws the ranges.
        assert!(OVERLAY_SHADER.contains("@location(2) alpha: f32"));
        // Crisp overlay text samples the 2x raster 1:1, never the base atlas.
        assert!(OVERLAY_SHADER.contains("var atlas2x: texture_2d<f32>"));
        assert!(OVERLAY_SHADER.contains("textureSample(atlas2x, samp,"));
    }

    #[test]
    fn glide_camera_snaps_holds_and_converges() {
        // No history snaps to the target with zero fraction.
        let (pos, cam) = glide_camera(None, 10.0, 4.0, 0.016);
        assert_eq!((pos.0, pos.1), (10.0, 4.0));
        assert_eq!((cam.x, cam.y), (10, 4));
        assert_eq!((cam.ox, cam.oy), (0.0, 0.0));
        // Zero dt holds still.
        let (pos2, _) = glide_camera(Some(pos), 20.0, 9.0, 0.0);
        assert_eq!(pos2, pos);
        // Easing halves the gap repeatedly and keeps cell+fraction coherent.
        let mut state = Some((0.0f32, 0.0f32));
        for _ in 0..200 {
            let (next, _) = glide_camera(state, 8.0, 3.0, 0.016);
            state = Some(next);
        }
        let ((nx, ny), cam) = glide_camera(state, 8.0, 3.0, 0.016);
        assert!((nx - 8.0).abs() < 0.01 && (ny - 3.0).abs() < 0.01);
        assert!((cam.x as f32 + cam.ox - nx).abs() < 1e-5);
        assert!((cam.y as f32 + cam.oy - ny).abs() < 1e-5);
        assert!((0.0..1.0).contains(&cam.ox) && (0.0..1.0).contains(&cam.oy));
    }

    #[test]
    fn world_flag_marks_camera_rects_only() {
        let mut frame = Frame::blank(4, 1);
        for cell in frame.cells.iter_mut() {
            *cell = Cell::new('x', Rgb::new(1, 1, 1), Rgb::new(0, 0, 0));
        }
        let instances = frame_instances(&frame, 4, 1, &[(1, 0, 2, 1)]);
        let flags: Vec<u32> = instances.iter().map(|i| i.world).collect();
        assert_eq!(flags, vec![0, 1, 1, 0]);
    }

    #[test]
    fn instance_preserves_fractional_actor_motion() {
        let mut frame = Frame::blank(1, 1);
        frame.cells[0] =
            Cell::new('@', Rgb::new(255, 255, 255), Rgb::new(0, 0, 0)).with_offset(0.35, -0.2);
        let instances = frame_instances(&frame, 1, 1, &[(0, 0, 1, 1)]);
        assert_eq!(instances[0].offset, [0.35, -0.2]);
        assert!(SHADER.contains("grid) + motion_offset - shift"));
    }
}
