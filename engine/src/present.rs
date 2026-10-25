//! Windowed presenter: opens a real OS window and draws engine [`Frame`]s
//! with a GPU glyph atlas (one instanced quad per cell, atlas built by
//! [`atlas`]). Runs anywhere a window can open — no terminal required.
//!
//! `run` presents one static scene; `run_dynamic` takes a [`SceneSource`]
//! for live scenes plus key input (see the `play` example).

pub mod atlas;
mod bloom;
pub mod gpu3d;

use super::overlay::{tessellate, text_quads, OverlayVert, Stroke, TextRun};
use super::{compose, Camera, ContinuousScene, Frame, Scene};
use crate::emission::EmissionSource;
use crate::raster::{Camera3, DepthOpts, Mesh, PointLight, Vec3};
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;

/// World presentation path used by the windowed presenter.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PresentMode {
    /// Render logical cells as continuously positioned glyph quads on a
    /// letterboxed black canvas. The authored grid remains the coordinate
    /// system, but glyphs are no longer stretched to fill the surface:
    /// enlarging viewports snap to integer texel factors (2x, 3x, ...) so
    /// the atlas is never fractionally resampled — the leftover pixels
    /// become centered dark margins.
    #[default]
    Continuous,
    /// Keep the original surface-filling grid path for compatibility and
    /// visual comparison while the continuous path is being accepted.
    GridFallback,
}

/// How to open the window and pace the loop.
pub struct PresentOptions {
    pub title: String,
    /// Font size in px; also fixes the cell grid density.
    pub cell_px: f32,
    /// Logical grid when a windowed window opens. Explicit dimensions also cap
    /// the logical grid during fullscreen/resizes; the presenter scale-to-fits
    /// those cells into the physical surface instead of silently increasing
    /// scene resolution. A zero dimension uses the monitor/surface size with
    /// no cap. This keeps existing struct literals source-compatible while
    /// making `Default` fullscreen.
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
    /// Select the new continuous canvas or the legacy grid fallback.
    pub mode: PresentMode,
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
            mode: PresentMode::Continuous,
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
    /// Localized world lights used by the same coarse face-centre lighting
    /// contract as the CPU raster fallback.
    pub lights: Vec<PointLight>,
    /// Layer depth tag for the produced layer.
    pub z: i32,
    /// Index of the placeholder `scene.layers` entry to replace.
    pub layer_slot: usize,
}

pub trait SceneSource {
    /// Let a live source select presentation-specific lighting ownership before
    /// it builds the scene. The default keeps existing sources unchanged.
    fn present_mode(&mut self, _mode: PresentMode) {}
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
    /// Optional native continuous primitives in the authored frame-grid space.
    /// The presenter samples these once at the same fixed frame time, renders
    /// their glyph geometry, and merges their float emission into the
    /// continuous canvas. World-space sources should project through the same
    /// camera used by `scene_at_size` before returning this scene. The composed
    /// frame remains the compatibility source for the legacy grid path.
    fn continuous_scene(&mut self, _t: f32) -> Option<ContinuousScene> {
        None
    }
    /// Whether the float bloom field should own world lighting for this
    /// source on the continuous canvas, instead of the legacy CPU glow pass
    /// plus localized point lights. Default keeps every existing source on
    /// the legacy world-lighting path; the spec-gated torch migration opts
    /// in explicitly (the in-world migration must not run before the
    /// standalone showcase is visually accepted).
    fn continuous_bloom_owns_world_lighting(&mut self) -> bool {
        false
    }
    /// Key press (`pressed`) / release. Esc/Q and close always exit in
    /// addition to this; return value is reserved (currently ignored).
    fn key(&mut self, _code: winit::keyboard::KeyCode, _pressed: bool) {}
    /// Stage anchors for local ground clearance: continuous-grid foot
    /// positions of actors/figures that must read as strong silhouettes.
    /// The presenter suppresses composed WORLD cells inside a small radius
    /// of each anchor (dithered edge) so terrain punctuation does not
    /// compete with the figure — the reference's quiet-local-ground rule
    /// (review 2026-10-25, P0). UI cells and native primitives are never
    /// suppressed. Default: none.
    fn stage_anchors(&mut self, _t: f32) -> Vec<super::Point2> {
        Vec::new()
    }
    /// Pointer feedback: absolute surface-pixel position, surface size, button edges,
    /// cursor deltas, and wheel ticks in one event (OSRS-style camera orbit
    /// wants the latter three; point-and-click can use the first two).
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
    /// Absolute cursor position in physical surface pixels. `None` means the
    /// presenter has not received a cursor position yet (or focus was reset).
    pub position: Option<(f32, f32)>,
    /// Physical surface dimensions used to convert `position` into the
    /// presenter's authored logical grid.
    pub surface_size: (u32, u32),
    /// Buttons currently held: 0 = left, 1 = right.
    pub held: [bool; 2],
    /// Button press edges in this event: 0 = left, 1 = right.
    pub pressed: [bool; 2],
    /// Button release edges in this event: 0 = left, 1 = right.
    pub released: [bool; 2],
    /// Cursor delta in physical pixels since the previous pointer event
    /// (zero for button and wheel events, which do not move the cursor).
    pub delta: (f32, f32),
    /// Signed wheel ticks this event (positive = scroll up).
    pub wheel: f32,
}

impl PointerEvent {
    /// Convert the absolute surface position into a grid cell, rejecting
    /// positions outside the surface.
    ///
    /// This only maps screen coordinates; it deliberately does not select a
    /// world target or perform any pathfinding.
    pub fn grid_position(
        &self,
        surface_width: u32,
        surface_height: u32,
        grid_width: u32,
        grid_height: u32,
    ) -> Option<(u32, u32)> {
        let (x, y) = self.position?;
        if surface_width == 0
            || surface_height == 0
            || grid_width == 0
            || grid_height == 0
            || !x.is_finite()
            || !y.is_finite()
            || x < 0.0
            || y < 0.0
            || x >= surface_width as f32
            || y >= surface_height as f32
        {
            return None;
        }
        let gx = (x * grid_width as f32 / surface_width as f32).floor() as u32;
        let gy = (y * grid_height as f32 / surface_height as f32).floor() as u32;
        Some((gx.min(grid_width - 1), gy.min(grid_height - 1)))
    }

    /// Convert a surface position into the authored grid inside an
    /// aspect-preserving viewport. Letterbox/pillarbox margins are rejected.
    pub fn grid_position_fitted(
        &self,
        surface_width: u32,
        surface_height: u32,
        grid_width: u32,
        grid_height: u32,
        cell_width: u32,
        cell_height: u32,
    ) -> Option<(u32, u32)> {
        let (x, y) = self.position?;
        let viewport = fitted_viewport(
            surface_width,
            surface_height,
            grid_width,
            grid_height,
            cell_width,
            cell_height,
        );
        if !x.is_finite()
            || !y.is_finite()
            || viewport.width <= 0.0
            || viewport.height <= 0.0
            || x < viewport.x
            || y < viewport.y
            || x >= viewport.x + viewport.width
            || y >= viewport.y + viewport.height
        {
            return None;
        }
        let gx = ((x - viewport.x) * grid_width as f32 / viewport.width).floor() as u32;
        let gy = ((y - viewport.y) * grid_height as f32 / viewport.height).floor() as u32;
        Some((
            gx.min(grid_width.saturating_sub(1)),
            gy.min(grid_height.saturating_sub(1)),
        ))
    }
}

/// App-side pointer bookkeeping: last cursor position and held buttons.
#[derive(Default)]
struct PointerTrack {
    last_pos: Option<(f64, f64)>,
    held: [bool; 2],
    surface_size: (u32, u32),
}

impl PointerTrack {
    fn event(
        &self,
        delta: (f32, f32),
        wheel: f32,
        pressed: [bool; 2],
        released: [bool; 2],
    ) -> PointerEvent {
        PointerEvent {
            position: self.last_pos.map(|(x, y)| (x as f32, y as f32)),
            surface_size: self.surface_size,
            held: self.held,
            pressed,
            released,
            delta,
            wheel,
        }
    }

    fn set_surface_size(&mut self, width: u32, height: u32) {
        self.surface_size = (width, height);
    }

    /// Drop all physical button state after the window loses focus.  Without
    /// this, a right-drag can remain logically held after Alt-Tab and the next
    /// cursor event unexpectedly rotates the camera.
    fn clear_focus(&mut self) -> PointerEvent {
        let released = self.held;
        self.held = [false; 2];
        self.last_pos = None;
        self.event((0.0, 0.0), 0.0, [false; 2], released)
    }

    /// Handle a button edge and return the event to forward.
    fn button(&mut self, slot: usize, pressed: bool) -> Option<PointerEvent> {
        let was_held = self.held[slot];
        self.held[slot] = pressed;
        let mut pressed_edges = [false; 2];
        let mut released_edges = [false; 2];
        pressed_edges[slot] = pressed && !was_held;
        released_edges[slot] = !pressed && was_held;
        Some(self.event((0.0, 0.0), 0.0, pressed_edges, released_edges))
    }

    /// Handle a cursor move: accumulate the delta into this event.
    fn moved(&mut self, x: f64, y: f64) -> Option<PointerEvent> {
        let delta = match self.last_pos {
            Some((lx, ly)) => ((x - lx) as f32, (y - ly) as f32),
            None => (0.0, 0.0),
        };
        self.last_pos = Some((x, y));
        Some(self.event(delta, 0.0, [false; 2], [false; 2]))
    }

    /// Handle a wheel tick line event.
    fn wheel(&mut self, ticks: f32) -> Option<PointerEvent> {
        Some(self.event((0.0, 0.0), ticks, [false; 2], [false; 2]))
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
struct ContinuousGlyphInstance {
    /// Centre position in authored grid coordinates, not surface pixels.
    position: [f32; 2],
    /// Billboard size in authored cells.
    scale: [f32; 2],
    rotation: f32,
    skew: [f32; 2],
    glyph: u32,
    fg_start: [f32; 3],
    fg_end: [f32; 3],
    bg: [f32; 3],
    /// 0 = flat, 1 = horizontal, 2 = vertical.
    fg_gradient_axis: u32,
    opacity: f32,
    /// World content is kept separate from pinned UI for future camera glide.
    world: u32,
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
    /// (the fields below are also shared by the continuous and overlay
    /// pipelines).
    _pad: [f32; 2],
    viewport_origin: [f32; 2],
    viewport_size: [f32; 2],
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
    /// Continuous instanced pipeline + float bloom resources. Created only
    /// for `PresentMode::Continuous`: the grid fallback must stand up
    /// identical GPU state without float textures or a continuous pipeline.
    continuous_pipe: Option<wgpu::RenderPipeline>,
    overlay_pipe: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    uniform_buf: wgpu::Buffer,
    instance_buf: wgpu::Buffer,
    instance_cap: usize,
    continuous_buf: wgpu::Buffer,
    continuous_cap: usize,
    bloom: Option<bloom::GpuBloom>,
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
    /// Whether the emissive-natives-without-ownership note has been logged
    /// (once per run; see the spec-gap guard in `render`).
    warned_bloom_gap: bool,
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
            warned_bloom_gap: false,
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
        // Float emission/bloom resources exist only for the continuous
        // canvas (spec: the grid fallback must remain independent of the
        // bloom path), so `--grid` allocates neither bloom textures nor a
        // continuous pipeline.
        let (bloom, continuous_pipe) = if self.opts.mode == PresentMode::Continuous {
            let bloom = bloom::GpuBloom::new(&device);
            let continuous_layout =
                device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("continuous-present-layout"),
                    bind_group_layouts: &[&bgl, bloom.layout()],
                    push_constant_ranges: &[],
                });
            let continuous_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("continuous-glyph-shader"),
                source: wgpu::ShaderSource::Wgsl(CONTINUOUS_SHADER.into()),
            });
            let continuous_pipe = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("continuous-glyph-pipeline"),
                layout: Some(&continuous_layout),
                vertex: wgpu::VertexState {
                    module: &continuous_shader,
                    entry_point: Some("cvs"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers: &[wgpu::VertexBufferLayout {
                        array_stride: size_of::<ContinuousGlyphInstance>() as u64,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &[
                            wgpu::VertexAttribute {
                                format: wgpu::VertexFormat::Float32x2,
                                offset: 0,
                                shader_location: 0,
                            },
                            wgpu::VertexAttribute {
                                format: wgpu::VertexFormat::Float32x2,
                                offset: 8,
                                shader_location: 1,
                            },
                            wgpu::VertexAttribute {
                                format: wgpu::VertexFormat::Float32,
                                offset: 16,
                                shader_location: 2,
                            },
                            wgpu::VertexAttribute {
                                format: wgpu::VertexFormat::Float32x2,
                                offset: 20,
                                shader_location: 3,
                            },
                            wgpu::VertexAttribute {
                                format: wgpu::VertexFormat::Uint32,
                                offset: 28,
                                shader_location: 4,
                            },
                            wgpu::VertexAttribute {
                                format: wgpu::VertexFormat::Float32x3,
                                offset: 32,
                                shader_location: 5,
                            },
                            wgpu::VertexAttribute {
                                format: wgpu::VertexFormat::Float32x3,
                                offset: 44,
                                shader_location: 6,
                            },
                            wgpu::VertexAttribute {
                                format: wgpu::VertexFormat::Float32x3,
                                offset: 56,
                                shader_location: 7,
                            },
                            wgpu::VertexAttribute {
                                format: wgpu::VertexFormat::Uint32,
                                offset: 68,
                                shader_location: 8,
                            },
                            wgpu::VertexAttribute {
                                format: wgpu::VertexFormat::Float32,
                                offset: 72,
                                shader_location: 9,
                            },
                            wgpu::VertexAttribute {
                                format: wgpu::VertexFormat::Uint32,
                                offset: 76,
                                shader_location: 10,
                            },
                        ],
                    }],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &continuous_shader,
                    entry_point: Some("cfs"),
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
            (Some(bloom), Some(continuous_pipe))
        } else {
            (None, None)
        };

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

        let (grid_w, grid_h) = grid_for_surface_limited(
            w,
            h,
            self.atlas.cell_w,
            self.atlas.cell_h,
            self.opts.width_cells,
            self.opts.height_cells,
        );
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
        let continuous_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("continuous-instances"),
            size: continuous_capacity(grid_w, grid_h),
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.window = Some(window);
        self.pointer.set_surface_size(w, h);
        self.gpu = Some(Gpu {
            _instance: instance,
            device,
            queue,
            surface,
            config,
            pipeline,
            continuous_pipe,
            overlay_pipe,
            bind_group,
            uniform_buf,
            instance_buf,
            instance_cap: (grid_w * grid_h) as usize,
            continuous_buf,
            continuous_cap: (grid_w * grid_h) as usize,
            bloom,
            overlay_buf,
            overlay_cap,
            grid_w,
            grid_h,
            world3d,
        });
        Ok(())
    }

    fn resize(&mut self, width: u32, height: u32) {
        self.pointer.set_surface_size(width, height);
        let Some(gpu) = self.gpu.as_mut() else {
            return;
        };
        if width == 0 || height == 0 {
            return;
        }
        gpu.config.width = width;
        gpu.config.height = height;
        gpu.surface.configure(&gpu.device, &gpu.config);
        (gpu.grid_w, gpu.grid_h) = grid_for_surface_limited(
            width,
            height,
            self.atlas.cell_w,
            self.atlas.cell_h,
            self.opts.width_cells,
            self.opts.height_cells,
        );
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
        if required > gpu.continuous_cap {
            gpu.continuous_buf = gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("continuous-instances"),
                size: continuous_capacity(gpu.grid_w, gpu.grid_h),
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            gpu.continuous_cap = required;
        }
    }

    fn render(&mut self) -> Result<(), String> {
        let Some(gpu) = self.gpu.as_mut() else {
            return Ok(());
        };
        let t = self.started.elapsed().as_secs_f32();
        let t0 = std::time::Instant::now();
        self.source.present_mode(self.opts.mode);
        let (mut scene, camera) = self.source.scene_at_size(t, gpu.grid_w, gpu.grid_h);
        let continuous_mode = self.opts.mode == PresentMode::Continuous;
        let continuous_primitives = if continuous_mode {
            self.source
                .continuous_scene(t)
                .map(|scene| scene.sample(t))
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        // Stage anchors (quiet local ground around figures) — sampled once
        // per frame in continuous mode; UI cells and natives are never
        // suppressed, so labels/modals keep painting over the cleared floor.
        let stage_anchors = if continuous_mode {
            self.source.stage_anchors(t)
        } else {
            Vec::new()
        };
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
        // World-lighting ownership on the continuous canvas: a source that
        // opts in lets the float bloom field replace the legacy CPU glow
        // pass (and localized point lights); every other source keeps the
        // grid composer and its 8-bit world glow for grid-identical torches.
        let bloom_owned = self.opts.mode == PresentMode::Continuous
            && self.source.continuous_bloom_owns_world_lighting();
        // Spec-gap guard (review 2026-10-25): an emissive continuous scene
        // without ownership silently produces no bloom. Ownership stays an
        // explicit opt-in contract, but the gap is at least loud now.
        if !bloom_owned
            && !self.warned_bloom_gap
            && continuous_primitives
                .iter()
                .any(|p| p.material.emission.is_some())
        {
            self.warned_bloom_gap = true;
            eprintln!(
                "[present] note: continuous primitives carry emission but the source does not \
                 claim bloom ownership (continuous_bloom_owns_world_lighting); their light is \
                 not rendered"
            );
        }
        let frame = if bloom_owned {
            crate::compose::compose_for_continuous(&scene, &camera, t)
        } else {
            compose(&scene, &camera, t)
        };
        if self.opts.mode == PresentMode::Continuous {
            let mut sources = if bloom_owned {
                Vec::with_capacity(continuous_primitives.len() + frame.emission_sources().len())
            } else {
                Vec::new()
            };
            if bloom_owned {
                for primitive in &continuous_primitives {
                    if let Some(source) = EmissionSource::from_resolved_primitive(*primitive, true)
                    {
                        sources.push(source);
                    }
                }
                sources.extend(EmissionSource::from_frame(&frame));
            }
            // Continuous shaders always bind the bloom texture. Upload an
            // explicit empty source list when ownership changes back to the
            // legacy path so a prior frame's field cannot remain visible.
            gpu.bloom
                .as_mut()
                .expect("continuous mode creates bloom resources")
                .prepare(&gpu.device, &gpu.queue, [gpu.grid_w, gpu.grid_h], &sources);
        }
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
        let viewport = fitted_viewport(
            gpu.config.width,
            gpu.config.height,
            gpu.grid_w,
            gpu.grid_h,
            self.atlas.cell_w,
            self.atlas.cell_h,
        );
        let uniforms = Uniforms {
            grid_w: gpu.grid_w,
            grid_h: gpu.grid_h,
            surf_w: gpu.config.width,
            surf_h: gpu.config.height,
            wash_rgb,
            wash_strength,
            cam: [camera.ox, camera.oy],
            _pad: [0.0, 0.0],
            viewport_origin: [viewport.x, viewport.y],
            viewport_size: [viewport.width, viewport.height],
        };
        gpu.queue
            .write_buffer(&gpu.uniform_buf, 0, bytemuck::bytes_of(&uniforms));

        let t_inst = std::time::Instant::now();
        let (instances, continuous_instances) = {
            let rects: Vec<(i32, i32, i32, i32)> = scene
                .partitions
                .iter()
                .filter(|p| p.camera)
                .map(|p| (p.x, p.y, p.w, p.h))
                .collect();
            match self.opts.mode {
                PresentMode::Continuous => (
                    Vec::new(),
                    frame_continuous_instances(
                        &frame,
                        gpu.grid_w,
                        gpu.grid_h,
                        &rects,
                        &continuous_primitives,
                        &stage_anchors,
                    ),
                ),
                PresentMode::GridFallback => (
                    frame_instances(&frame, gpu.grid_w, gpu.grid_h, &rects),
                    Vec::new(),
                ),
            }
        };
        let inst_count = instances.len();
        let continuous_count = continuous_instances.len();
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
        if !continuous_instances.is_empty() {
            let need = continuous_instances.len();
            if need > gpu.continuous_cap {
                gpu.continuous_buf = gpu.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("continuous-instances"),
                    size: (need * size_of::<ContinuousGlyphInstance>()) as u64,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                gpu.continuous_cap = need;
            }
            gpu.queue.write_buffer(
                &gpu.continuous_buf,
                0,
                bytemuck::cast_slice(&continuous_instances),
            );
        }

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
        if self.opts.mode == PresentMode::Continuous {
            gpu.bloom
                .as_ref()
                .expect("continuous mode creates bloom resources")
                .encode(&mut encoder);
        }
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("glyph-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            // The world viewport is a real black void. Every
                            // environmental colour arrives from a glyph's
                            // foreground alpha, never from the clear colour.
                            r: 0.0,
                            g: 0.0,
                            b: 0.0,
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
            match self.opts.mode {
                PresentMode::Continuous => {
                    let pipeline = gpu
                        .continuous_pipe
                        .as_ref()
                        .expect("continuous mode creates the continuous pipeline");
                    let bloom = gpu
                        .bloom
                        .as_ref()
                        .expect("continuous mode creates bloom resources");
                    pass.set_pipeline(pipeline);
                    pass.set_bind_group(0, &gpu.bind_group, &[]);
                    pass.set_bind_group(1, bloom.bind_group(), &[]);
                    pass.set_vertex_buffer(0, gpu.continuous_buf.slice(..));
                    pass.draw(0..6, 0..continuous_count as u32);
                }
                PresentMode::GridFallback => {
                    pass.set_pipeline(&gpu.pipeline);
                    pass.set_bind_group(0, &gpu.bind_group, &[]);
                    pass.set_vertex_buffer(0, gpu.instance_buf.slice(..));
                    pass.draw(0..6, 0..inst_count as u32);
                }
            }
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
                "[profile] build={:?} world3d={:?} compose={:?} inst={:?} n={} continuous={} mode={:?} grid={}x{}",
                t1.duration_since(t0),
                gpu3d_ms.unwrap_or_default(),
                t2.duration_since(t1),
                inst_d,
                inst_count,
                continuous_count,
                self.opts.mode,
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
            let in_world = frame.is_world(xi, yi)
                || world
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
                bg: if in_world {
                    [0.0, 0.0, 0.0]
                } else {
                    [
                        f32::from(cell.bg.r) / 255.0,
                        f32::from(cell.bg.g) / 255.0,
                        f32::from(cell.bg.b) / 255.0,
                    ]
                },
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

/// A physical viewport for the authored logical grid. The cell aspect comes
/// from the embedded font, so a fullscreen surface letterboxes instead of
/// stretching glyphs horizontally or vertically.
#[derive(Clone, Copy, Debug, PartialEq)]
struct ViewportRect {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

fn fitted_viewport(
    surface_width: u32,
    surface_height: u32,
    grid_width: u32,
    grid_height: u32,
    cell_width: u32,
    cell_height: u32,
) -> ViewportRect {
    let sw = surface_width as f32;
    let sh = surface_height as f32;
    let authored_width = (grid_width.max(1) * cell_width.max(1)) as f32;
    let authored_height = (grid_height.max(1) * cell_height.max(1)) as f32;
    if sw <= 0.0 || sh <= 0.0 {
        return ViewportRect {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
        };
    }
    let scale = (sw / authored_width).min(sh / authored_height);
    // Glyph coverage rasterizes once into the atlas; any fractional upscale
    // resamples it and reads soft/low-res on large windows and fullscreen
    // (the authored grid stays capped there, so the viewport stretches).
    // Snap enlarging scales down to integer factors so glyph texels map
    // 1:1 (2x, 3x, ...); the leftover pixels become centered dark margins
    // instead of resampling artifacts. Shrinking scales (windows smaller
    // than the authored grid) still shrink-to-fit to keep filling the
    // surface.
    let scale = if scale >= 1.0 {
        scale.floor().max(1.0)
    } else {
        scale
    };
    let width = authored_width * scale;
    let height = authored_height * scale;
    ViewportRect {
        x: (sw - width) * 0.5,
        y: (sh - height) * 0.5,
        width,
        height,
    }
}

/// Cell-stable unit hash for dither decisions (same construction family
/// as the game-side `hash2` lattices).
fn hash2(x: i32, y: i32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x27d4_eb2d);
    h ^= (y as u32).wrapping_mul(0x1656_67b1);
    h ^= h.rotate_left(13);
    (h >> 8) as f32 / (1u32 << 24) as f32
}

fn frame_continuous_instances(
    frame: &Frame,
    grid_w: u32,
    grid_h: u32,
    world: &[(i32, i32, i32, i32)],
    native: &[super::ResolvedPrimitive],
    stage: &[super::Point2],
) -> Vec<ContinuousGlyphInstance> {
    let cols = grid_w.min(frame.w.max(0) as u32);
    let rows = grid_h.min(frame.h.max(0) as u32);
    let mut out = Vec::with_capacity((cols * rows) as usize + native.len());
    // Composed UI cells (HUD, labels, inventory/modal, map captions) are
    // collected separately and appended AFTER the native primitives: the
    // instance buffer is the paint order, so the layer contract becomes
    // world ink → native world content → UI/modal (review 2026-10-25:
    // natives appending last let actors/weather/torches draw over the HUD
    // and modals). Composed cells still carry no per-cell depth, so
    // cross-depth interleaving between world ink and natives stays
    // intentionally unmodelled — see `native_continuous_instance`.
    let mut ui_cells = Vec::new();
    if cols > 0 && rows > 0 {
        for y in 0..rows {
            for x in 0..cols {
                let Some(cell) = frame.get(x as i32, y as i32) else {
                    continue;
                };
                if cell.transparent {
                    continue;
                }
                let xi = x as i32;
                let yi = y as i32;
                let in_world = frame.is_world(xi, yi)
                    || world.iter().any(|&(px, py, pw, ph)| {
                        xi >= px && xi < px + pw && yi >= py && yi < py + ph
                    });
                // Quiet local ground (review 2026-10-25 P0): suppress
                // composed WORLD cells around stage anchors so figures read
                // as strong silhouettes over a locally uniform floor — the
                // inner radius clears outright, the outer band dithers by
                // a cell-stable hash for a ragged arena edge. UI cells and
                // native primitives are never suppressed.
                if in_world && !stage.is_empty() {
                    let cx = x as f32 + 0.5 + cell.offset_x;
                    let cy = y as f32 + 0.5 + cell.offset_y;
                    let nearest = stage
                        .iter()
                        .map(|a| (a.x - cx).hypot(a.y - cy))
                        .fold(f32::INFINITY, f32::min);
                    if nearest < 2.2 {
                        continue;
                    }
                    if nearest < 3.2 {
                        let dither = hash2(xi, yi);
                        if dither < (3.2 - nearest) / 1.0 {
                            continue;
                        }
                    }
                }
                let bold = if cell.bold { 1.25 } else { 1.0 };
                let f = |c: u8| ((f32::from(c) / 255.0) * bold).min(1.0);
                let instance = ContinuousGlyphInstance {
                    position: [
                        x as f32 + 0.5 + cell.offset_x,
                        y as f32 + 0.5 + cell.offset_y,
                    ],
                    scale: [cell.scale.clamp(0.25, 2.0), cell.scale.clamp(0.25, 2.0)],
                    rotation: 0.0,
                    skew: [0.0, 0.0],
                    glyph: atlas::slot(cell.glyph),
                    fg_start: [f(cell.fg.r), f(cell.fg.g), f(cell.fg.b)],
                    fg_end: [f(cell.fg_end.r), f(cell.fg_end.g), f(cell.fg_end.b)],
                    bg: if in_world {
                        [0.0, 0.0, 0.0]
                    } else {
                        [
                            f32::from(cell.bg.r) / 255.0,
                            f32::from(cell.bg.g) / 255.0,
                            f32::from(cell.bg.b) / 255.0,
                        ]
                    },
                    fg_gradient_axis: match cell.fg_gradient {
                        None => 0,
                        Some(super::Axis::X) => 1,
                        Some(super::Axis::Y) => 2,
                    },
                    opacity: 1.0,
                    world: in_world as u32,
                };
                if in_world {
                    out.push(instance);
                } else {
                    ui_cells.push(instance);
                }
            }
        }
    }
    // Native primitives render above composed world ink (their own depth
    // order only) but below every UI/modal cell.
    out.extend(native.iter().filter_map(native_continuous_instance));
    out.extend(ui_cells);
    out
}

/// Convert one resolved native primitive into a continuous instanced glyph.
///
/// Narrowed guarantee (reviewed 2026-10-23, ordering revised 2026-10-25):
/// native primitives carry no depth into the GPU pipeline —
/// `ContinuousGlyphInstance` has no depth channel and this pipeline has no
/// depth attachment — so every native primitive renders above all composed
/// WORLD cells (in its own depth order only) but below every composed UI
/// cell (HUD/labels/modals paint last; see `frame_continuous_instances`),
/// and ribbons/streaks/rays collapse to a single glyph (`-`/`^`) because
/// width, length, and tip geometry have no instance representation yet.
/// Deep interleaving with world ink and true primitive geometry stay
/// deferred until a dedicated depth/geometry milestone; emission parity is
/// unaffected.
fn native_continuous_instance(
    primitive: &super::ResolvedPrimitive,
) -> Option<ContinuousGlyphInstance> {
    let glyph = match primitive.kind {
        super::PrimitiveKind::Glyph { glyph } | super::PrimitiveKind::Particle { glyph } => glyph,
        super::PrimitiveKind::Ribbon { .. } | super::PrimitiveKind::Streak { .. } => '-',
        super::PrimitiveKind::Ray { .. } => '^',
    };
    let finite = |value: f32, fallback: f32| {
        if value.is_finite() {
            value
        } else {
            fallback
        }
    };
    let scale_x = finite(primitive.transform.scale.x, 1.0).clamp(0.0, 8.0);
    let scale_y = finite(primitive.transform.scale.y, 1.0).clamp(0.0, 8.0);
    let position_x = finite(primitive.transform.position.x, 0.0) + 0.5;
    let position_y = finite(primitive.transform.position.y, 0.0) + 0.5;
    let (fg_start, fg_end, gradient_axis) = match primitive.material.gradient {
        Some(gradient) => (
            gradient.start,
            gradient.end,
            match gradient.axis {
                super::GradientAxis::Horizontal => 1,
                super::GradientAxis::Vertical => 2,
                super::GradientAxis::Radial => 3,
            },
        ),
        None => (
            primitive.material.foreground,
            primitive.material.foreground,
            0,
        ),
    };
    let rgb = |color: super::Rgb| {
        [
            f32::from(color.r) / 255.0,
            f32::from(color.g) / 255.0,
            f32::from(color.b) / 255.0,
        ]
    };
    Some(ContinuousGlyphInstance {
        position: [position_x, position_y],
        scale: [scale_x, scale_y],
        rotation: finite(primitive.transform.rotation, 0.0),
        skew: [
            finite(primitive.transform.skew.x, 0.0).clamp(-8.0, 8.0),
            finite(primitive.transform.skew.y, 0.0).clamp(-8.0, 8.0),
        ],
        glyph: atlas::slot(glyph),
        fg_start: rgb(fg_start),
        fg_end: rgb(fg_end),
        bg: rgb(primitive.material.background),
        fg_gradient_axis: gradient_axis,
        opacity: finite(primitive.material.opacity, 0.0).clamp(0.0, 1.0),
        world: 1,
    })
}

fn grid_for_surface(width: u32, height: u32, cell_w: u32, cell_h: u32) -> (u32, u32) {
    ((width / cell_w).max(1), (height / cell_h).max(1))
}

fn grid_for_surface_limited(
    width: u32,
    height: u32,
    cell_w: u32,
    cell_h: u32,
    max_width: u32,
    max_height: u32,
) -> (u32, u32) {
    let (grid_w, grid_h) = grid_for_surface(width, height, cell_w, cell_h);
    (
        if max_width == 0 {
            grid_w
        } else {
            grid_w.min(max_width.max(1))
        },
        if max_height == 0 {
            grid_h
        } else {
            grid_h.min(max_height.max(1))
        },
    )
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

fn continuous_capacity(grid_w: u32, grid_h: u32) -> u64 {
    (grid_w.max(1) as u64 * grid_h.max(1) as u64 * size_of::<ContinuousGlyphInstance>() as u64)
        .max(1)
}

const SHADER: &str = r#"
struct U {
  grid: vec2<u32>,
  surf: vec2<u32>,
  wash_rgb: vec3<f32>,
  wash_strength: f32,
  cam: vec2<f32>,
  _pad: vec2<f32>,
  viewport_origin: vec2<f32>,
  viewport_size: vec2<f32>,
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
    @interpolate(flat) @location(6) world: u32,
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
    let cell = u.viewport_size / vec2<f32>(u.grid);
    // Fractional follow: the scene composes on integer cells while the
    // view glides between them (no cell-stepping while walking).
    // Fractional follow: world instances glide between cells while pinned
    // UI stays put (no cell-stepping while walking).
    let shift = select(vec2<f32>(0.0, 0.0), u.cam, world != 0u);
    let pix = u.viewport_origin + (vec2<f32>(grid) + motion_offset - shift + c0) * cell;
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
    o.world = world;
    return o;
}

@fragment
fn fs(i: VOut) -> @location(0) vec4<f32> {
    let a = textureSample(atlas, samp, i.uv).r;
    var blend = 0.0;
    if (i.gradient_axis == 1u) {
        blend = i.glyph_uv.x;
    } else if (i.gradient_axis == 2u) {
        blend = i.glyph_uv.y;
    } else if (i.gradient_axis == 3u) {
        blend = clamp(distance(i.glyph_uv, vec2<f32>(0.5, 0.5)) * 2.0, 0.0, 1.0);
    }
    let fg = mix(i.fg_start, i.fg_end, blend);
    let col = mix(i.bg, fg, a);
    let washed = mix(col, u.wash_rgb, u.wash_strength);
    let final_col = select(washed, col, i.world != 0u);
    return vec4<f32>(final_col, 1.0);
}
"#;

const CONTINUOUS_SHADER: &str = r#"
struct U {
  grid: vec2<u32>,
  surf: vec2<u32>,
  wash_rgb: vec3<f32>,
  wash_strength: f32,
  cam: vec2<f32>,
  _pad: vec2<f32>,
  viewport_origin: vec2<f32>,
  viewport_size: vec2<f32>,
};
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var atlas: texture_2d<f32>;
@group(0) @binding(2) var samp: sampler;
@group(1) @binding(0) var bloom_field: texture_2d<f32>;

struct VOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) glyph_uv: vec2<f32>,
    @location(2) fg_start: vec3<f32>,
    @location(3) fg_end: vec3<f32>,
    @location(4) bg: vec3<f32>,
    @interpolate(flat) @location(5) gradient_axis: u32,
    @location(6) opacity: f32,
    @interpolate(flat) @location(7) world: u32,
};

@vertex
fn cvs(@builtin(vertex_index) vi: u32,
       @location(0) position: vec2<f32>,
       @location(1) scale: vec2<f32>,
       @location(2) rotation: f32,
       @location(3) skew: vec2<f32>,
       @location(4) glyph: u32,
       @location(5) fg_start: vec3<f32>,
       @location(6) fg_end: vec3<f32>,
       @location(7) bg: vec3<f32>,
       @location(8) gradient_axis: u32,
       @location(9) opacity: f32,
       @location(10) world: u32) -> VOut {
    var c = vec2<f32>(0.0, 0.0);
    if (vi == 1u) { c = vec2<f32>(1.0, 0.0); }
    else if (vi == 2u || vi == 4u) { c = vec2<f32>(1.0, 1.0); }
    else if (vi == 5u) { c = vec2<f32>(0.0, 1.0); }
    let local = (c - 0.5) * scale;
    let sheared = vec2<f32>(
        local.x + skew.x * local.y,
        local.y + skew.y * local.x,
    );
    let cs = cos(rotation);
    let sn = sin(rotation);
    let rotated = vec2<f32>(
        cs * sheared.x - sn * sheared.y,
        sn * sheared.x + cs * sheared.y,
    );
    let logical = position + rotated;
    let pix = u.viewport_origin + logical / vec2<f32>(u.grid) * u.viewport_size;
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
    o.opacity = opacity;
    o.world = world;
    return o;
}

@fragment
fn cfs(i: VOut) -> @location(0) vec4<f32> {
    let a = textureSample(atlas, samp, i.uv).r * i.opacity;
    var blend = 0.0;
    if (i.gradient_axis == 1u) {
        blend = i.glyph_uv.x;
    } else if (i.gradient_axis == 2u) {
        blend = i.glyph_uv.y;
    } else if (i.gradient_axis == 3u) {
        blend = clamp(distance(i.glyph_uv, vec2<f32>(0.5, 0.5)) * 2.0, 0.0, 1.0);
    }
    let fg = mix(i.fg_start, i.fg_end, blend);
    let field_dimensions = vec2<i32>(textureDimensions(bloom_field));
    let logical = ((i.pos.xy - u.viewport_origin) / u.viewport_size) * vec2<f32>(u.grid);
    let field_position = clamp(
        vec2<i32>(floor(logical / vec2<f32>(u.grid) * vec2<f32>(field_dimensions))),
        vec2<i32>(0),
        field_dimensions - vec2<i32>(1),
    );
    let bloom = textureLoad(bloom_field, field_position, 0).rgb;
    // Tone-mapped light response (review 2026-10-25): a soft energy curve
    // replaces the raw additive clamp — flame cores saturate smoothly,
    // warm hue ratios survive, and the pool falls off gently instead of
    // every lit cell clipping to the same orange/white.
    let raw = select(vec3<f32>(0.0), bloom, i.world != 0u);
    let peak = max(raw.r, max(raw.g, raw.b));
    let energy = 1.0 - exp(-peak * 1.7);
    let light = raw / max(peak, 0.0001) * energy;
    let lit_fg = min(fg + light, vec3<f32>(1.0));
    let col = mix(i.bg, lit_fg, a);
    let washed = mix(col, u.wash_rgb, u.wash_strength);
    // World cells: light-receiver response (review 2026-10-25) — inside a
    // strong field the glyph gaps gain a faint warm fill, so sparse ground
    // reads as an illuminated surface while true void (no field) stays
    // pure black. Ink compositing is unchanged (same premasked colour and
    // alpha as before); UI cells keep their opaque washed background.
    if (i.world != 0u) {
        let recv = energy * 0.38;
        let ink = lit_fg * a * a;
        let fill = light * recv * (1.0 - a);
        let out_a = max(a, recv);
        let out_c = (ink + fill) / max(out_a, 0.0001);
        return vec4<f32>(out_c, out_a);
    }
    return vec4<f32>(washed, 1.0);
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
                // DPI-aware glyph atlas: the window above is sized in
                // LOGICAL pixels, but the atlas rasterizes at PHYSICAL
                // pixels per cell, so any display scale factor > 1 stretches
                // every glyph (the "low-res window" symptom). Rebuild both
                // atlases at the window's scale factor before GPU init —
                // cells then cover scale × more physical pixels and the
                // grid/viewport math (which reads `atlas.cell_w/h` in
                // physical units) keeps a 1:1 texel mapping.
                let scale = window.scale_factor();
                if scale > 1.0 {
                    let base = self.opts.cell_px * scale as f32;
                    match atlas::build(base) {
                        Ok(hi) => self.atlas = hi,
                        Err(err) => eprintln!("presenter hi-dpi atlas failed: {err}"),
                    }
                    match atlas::build(base * 2.0) {
                        Ok(hi) => self.atlas2x = hi,
                        Err(err) => eprintln!("presenter hi-dpi overlay atlas failed: {err}"),
                    }
                }
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
            WindowEvent::Focused(false) => {
                // Forward synthetic releases so dynamic sources cannot keep
                // accelerating after the OS consumes a key-up during focus
                // loss. Pointer state is cleared through the same event path.
                for code in self.pressed_keys.drain() {
                    self.source.key(code, false);
                }
                self.source.pointer(self.pointer.clear_focus());
            }
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
                // Winit's cursor position is already relative to this window
                // in physical surface pixels; retain it for click mapping.
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
  viewport_origin: vec2<f32>,
  viewport_size: vec2<f32>,
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
    let cell = u.viewport_size / vec2<f32>(u.grid);
    let pix = u.viewport_origin + pos * cell;
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
        continuous_capacity, fitted_viewport, frame_continuous_instances, frame_instances,
        glide_camera, grid_for_surface, grid_for_surface_limited, instance_capacity,
        ContinuousGlyphInstance, GlyphInstance, CONTINUOUS_SHADER, OVERLAY_SHADER, SHADER,
    };
    use crate::{Axis, Cell, EmissionSource, Frame, Rgb};

    #[test]
    fn grid_uses_surface_pixels_and_keeps_small_surfaces_visible() {
        assert_eq!(grid_for_surface(1920, 1080, 20, 32), (96, 33));
        assert_eq!(grid_for_surface(3, 3, 20, 32), (1, 1));
    }

    #[test]
    fn explicit_logical_grid_caps_fullscreen_without_changing_small_windows() {
        assert_eq!(
            grid_for_surface_limited(1920, 1080, 6, 10, 160, 56),
            (160, 56)
        );
        assert_eq!(
            grid_for_surface_limited(640, 480, 6, 10, 160, 56),
            (106, 48)
        );
        assert_eq!(
            grid_for_surface_limited(1920, 1080, 6, 10, 0, 0),
            grid_for_surface(1920, 1080, 6, 10)
        );
    }

    #[test]
    fn fitted_viewport_snaps_enlarging_scales_to_integer_texel_factors() {
        // Authored grid 160x56 at 10x16 cells = 1600x896. A 1920x1080
        // surface fits at 1.2x; the snap renders 1x glyph texels with
        // centered dark margins instead of resampling the atlas.
        let viewport = fitted_viewport(1920, 1080, 160, 56, 10, 16);
        assert_eq!(viewport.width, 1600.0);
        assert_eq!(viewport.height, 896.0);
        assert_eq!(viewport.x, 160.0);
        assert_eq!(viewport.y, 92.0);

        // Wider surface cannot buy more than the integer factor allows.
        let wide = fitted_viewport(2560, 1080, 160, 56, 10, 16);
        assert_eq!(wide.width, 1600.0);
        assert_eq!(wide.height, 896.0);
        assert_eq!(wide.x, 480.0);
        assert_eq!(wide.y, 92.0);

        // A 4K surface fits 2x, so glyphs render at exactly two texels per
        // axis — still a 1:1 mapping, just doubled.
        let uhd = fitted_viewport(3840, 2160, 160, 56, 10, 16);
        assert_eq!(uhd.width, 3200.0);
        assert_eq!(uhd.height, 1792.0);
        assert_eq!(uhd.x, 320.0);
        assert_eq!(uhd.y, 184.0);

        // Surfaces smaller than the authored grid still shrink-to-fit; small
        // windows must keep filling the surface.
        let small = fitted_viewport(800, 448, 160, 56, 10, 16);
        assert_eq!(small.width, 800.0);
        assert_eq!(small.height, 448.0);
        assert_eq!(small.x, 0.0);
        assert_eq!(small.y, 0.0);
    }

    #[test]
    fn instance_capacity_covers_every_grid_cell() {
        let bytes = instance_capacity(80, 25);
        assert_eq!(bytes, 80 * 25 * std::mem::size_of::<GlyphInstance>() as u64);
        assert!(instance_capacity(0, 0) >= std::mem::size_of::<GlyphInstance>() as u64);
        assert!(continuous_capacity(0, 0) >= std::mem::size_of::<ContinuousGlyphInstance>() as u64);
    }

    /// Quiet local ground (review 2026-10-25 P0): world cells inside a
    /// stage anchor's inner radius are suppressed (with a dithered outer
    /// band), while UI cells at the same positions are never suppressed —
    /// and with no anchors nothing changes.
    #[test]
    fn stage_anchors_clear_world_cells_but_never_ui_cells() {
        let mut frame = Frame::blank(12, 1);
        for i in 0..12 {
            frame.cells[i] = Cell::new('.', Rgb::new(90, 90, 90), Rgb::new(0, 0, 0));
        }
        let world = &[(0, 0, 12, 1)];
        // No anchors: every world cell renders.
        let plain = frame_continuous_instances(&frame, 12, 1, world, &[], &[]);
        assert_eq!(plain.len(), 12);
        // Anchor at (5.5, 0.5): the inner radius clears the cells around
        // it; the far edges (well outside the dither band) survive.
        let stage = [crate::Point2::new(5.5, 0.5)];
        let cleared = frame_continuous_instances(&frame, 12, 1, world, &[], &stage);
        assert!(
            cleared.len() < plain.len(),
            "stage anchors must suppress world cells"
        );
        let x_positions: Vec<f32> = cleared.iter().map(|i| i.position[0]).collect();
        for x in [0.5, 11.5] {
            assert!(
                x_positions.contains(&x),
                "far cells at x={x} must survive the stage clearance"
            );
        }
        // UI cells (no world partition) are never suppressed by anchors.
        let ui = frame_continuous_instances(&frame, 12, 1, &[], &[], &stage);
        assert_eq!(
            ui.len(),
            12,
            "UI cells must ignore stage clearance entirely"
        );
    }

    #[test]
    fn continuous_instances_use_fractional_positions_and_black_world_backgrounds() {
        let mut frame = Frame::blank(2, 1);
        frame.cells[0] = Cell::new('@', Rgb::new(255, 200, 80), Rgb::new(50, 20, 10))
            .with_offset(0.25, -0.125)
            .with_scale(0.75);
        frame.cells[1] = Cell::new('h', Rgb::new(200, 220, 255), Rgb::new(50, 20, 10));
        let instances = frame_continuous_instances(&frame, 2, 1, &[(0, 0, 2, 1)], &[], &[]);
        assert_eq!(instances.len(), 2);
        assert_eq!(instances[0].position, [0.75, 0.375]);
        assert_eq!(instances[0].scale, [0.75, 0.75]);
        assert_eq!(instances[0].bg, [0.0, 0.0, 0.0]);
        assert_eq!(instances[1].position, [1.5, 0.5]);
        assert_eq!(instances[1].world, 1);
    }

    /// Layer contract (review 2026-10-25): the instance buffer is the paint
    /// order, so a UI cell sharing its cell with a native world primitive
    /// must be emitted AFTER the native — HUD/labels/modals can never be
    /// occluded by native actors, weather, torches, or map primitives.
    #[test]
    fn ui_cells_paint_after_native_primitives_at_overlapping_positions() {
        let mut frame = Frame::blank(2, 2);
        // (0,0) world partition cell; (1,1) UI cell outside every partition.
        frame.cells[0] = Cell::new('.', Rgb::new(90, 90, 90), Rgb::new(0, 0, 0));
        frame.cells[3] = Cell::new('H', Rgb::new(240, 240, 240), Rgb::new(20, 20, 20));
        let primitive = crate::Primitive::new(
            crate::PrimitiveKind::particle('*'),
            crate::Transform2::new(crate::Point2::new(1.2, 1.3), 0.0),
            crate::ContinuousMaterial::new(Rgb::new(255, 140, 40)),
        )
        .sample(0.0);
        let instances =
            frame_continuous_instances(&frame, 2, 2, &[(0, 0, 1, 1)], &[primitive], &[]);
        assert_eq!(instances.len(), 3);
        let world_index = instances
            .iter()
            .position(|i| i.world == 1 && i.position[0] < 1.0)
            .expect("world cell instance");
        let native_index = instances
            .iter()
            .position(|i| i.world == 1 && i.position[0] > 1.0 && i.position[0] < 2.0)
            .expect("native instance");
        let ui_index = instances
            .iter()
            .position(|i| i.world == 0)
            .expect("ui cell instance");
        assert!(
            world_index < native_index,
            "native must paint above composed world ink"
        );
        assert!(
            native_index < ui_index,
            "UI cell must paint above an overlapping native primitive"
        );
    }

    #[test]
    fn continuous_instances_render_native_primitives_with_transforms() {
        let frame = Frame::blank(1, 1);
        let primitive = crate::Primitive::new(
            crate::PrimitiveKind::ray(2.0, 0.4, 0.1),
            crate::Transform2::new(crate::Point2::new(0.25, 0.75), 0.0)
                .with_scale(crate::Point2::new(1.5, 0.5))
                .with_rotation(0.4)
                .with_skew(crate::Point2::new(0.2, -0.1)),
            crate::ContinuousMaterial::new(Rgb::new(255, 120, 20))
                .with_opacity(0.7)
                .with_gradient(crate::ColorGradient::new(
                    Rgb::new(255, 220, 80),
                    Rgb::new(220, 40, 10),
                    crate::GradientAxis::Horizontal,
                )),
        )
        .sample(0.0);
        let instances = frame_continuous_instances(&frame, 1, 1, &[], &[primitive], &[]);

        assert_eq!(instances.len(), 1);
        assert_eq!(instances[0].glyph, crate::present::atlas::slot('^'));
        assert_eq!(instances[0].position, [0.75, 1.25]);
        assert_eq!(instances[0].scale, [1.5, 0.5]);
        assert_eq!(instances[0].rotation, 0.4);
        assert_eq!(instances[0].skew, [0.2, -0.1]);
        assert_eq!(instances[0].fg_gradient_axis, 1);
        assert_eq!(instances[0].opacity, 0.7);
        assert_eq!(instances[0].world, 1);
    }

    #[test]
    fn native_emission_uses_the_same_half_cell_centre_as_the_glyph() {
        let primitive = crate::Primitive::new(
            crate::PrimitiveKind::glyph('*'),
            crate::Transform2::new(crate::Point2::new(3.25, 4.75), 0.0),
            crate::ContinuousMaterial::new(Rgb::new(255, 120, 20))
                .with_emission(crate::Emission::new(Rgb::new(255, 80, 10), 1.0, 2.0)),
        )
        .sample(0.0);
        let instance =
            frame_continuous_instances(&Frame::blank(0, 0), 0, 0, &[], &[primitive], &[])
                .pop()
                .expect("native-only primitive should render");
        let source = EmissionSource::from_resolved_primitive(primitive, true)
            .expect("emissive primitive should produce a source");
        assert_eq!(instance.position, [3.75, 5.25]);
        assert_eq!(
            source.position,
            crate::EmissionPosition::Continuous(crate::Point2::new(
                instance.position[0],
                instance.position[1]
            ))
        );
    }

    #[test]
    fn native_primitives_render_when_the_composed_frame_is_empty() {
        let primitive = crate::Primitive::new(
            crate::PrimitiveKind::particle('*'),
            crate::Transform2::new(crate::Point2::new(1.25, 2.5), 0.0),
            crate::ContinuousMaterial::new(Rgb::new(255, 255, 255)),
        )
        .sample(0.0);
        let instances =
            frame_continuous_instances(&Frame::blank(0, 0), 0, 0, &[], &[primitive], &[]);
        assert_eq!(instances.len(), 1);
        assert_eq!(instances[0].position, [1.75, 3.0]);
    }

    #[test]
    fn radial_gradients_keep_a_distinct_native_shader_contract() {
        let primitive = crate::Primitive::new(
            crate::PrimitiveKind::glyph('*'),
            crate::Transform2::new(crate::Point2::ZERO, 0.0),
            crate::ContinuousMaterial::new(Rgb::new(255, 255, 255)).with_gradient(
                crate::ColorGradient::new(
                    Rgb::new(255, 255, 255),
                    Rgb::new(0, 0, 0),
                    crate::GradientAxis::Radial,
                ),
            ),
        )
        .sample(0.0);
        let instances =
            frame_continuous_instances(&Frame::blank(0, 0), 0, 0, &[], &[primitive], &[]);
        assert_eq!(instances[0].fg_gradient_axis, 3);
        assert!(CONTINUOUS_SHADER.contains("distance(i.glyph_uv"));
        assert!(CONTINUOUS_SHADER.contains("gradient_axis == 3u"));
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
    fn pointer_track_emits_absolute_position_and_button_edges() {
        use super::PointerTrack;
        let mut track = PointerTrack::default();
        // Wheel: single slot with held-up ticks, zero delta.
        let wheel = track.wheel(1.0).expect("wheel event");
        assert_eq!(wheel.position, None, "no cursor position before movement");
        assert!(!wheel.held[0] && !wheel.held[1]);
        assert_eq!(wheel.pressed, [false, false]);
        assert_eq!(wheel.released, [false, false]);
        assert_eq!(wheel.delta, (0.0, 0.0));
        assert_eq!(wheel.wheel, 1.0);
        // Right press flips the held slot; drag then reports it with deltas.
        let down = track.button(1, true).expect("button event");
        assert_eq!(down.position, None);
        assert!(down.held[1], "right slot held");
        assert_eq!(down.pressed, [false, true]);
        assert_eq!(down.released, [false, false]);
        assert_eq!(down.delta, (0.0, 0.0));
        let drag = track.moved(400.0, 300.0).expect("first move");
        assert_eq!(drag.position, Some((400.0, 300.0)));
        assert_eq!(drag.delta, (0.0, 0.0), "first move has no previous cursor");
        let drag = track.moved(364.0, 300.0).expect("second move");
        assert_eq!(drag.position, Some((364.0, 300.0)));
        assert_eq!(drag.delta, (-36.0, 0.0));
        assert!(drag.held[1], "held slot sticks across moves");
        assert_eq!(drag.pressed, [false, false]);
        assert_eq!(drag.released, [false, false]);
        // Release clears the slot; left wheel of 0 is dropped by the caller.
        let up = track.button(1, false).expect("button event");
        assert_eq!(up.position, Some((364.0, 300.0)));
        assert!(!up.held[1]);
        assert_eq!(up.pressed, [false, false]);
        assert_eq!(up.released, [false, true]);
        let duplicate_up = track.button(1, false).expect("button event");
        assert_eq!(duplicate_up.pressed, [false, false]);
        assert_eq!(duplicate_up.released, [false, false]);
        track.last_pos = Some((10.0, 10.0));
        let tick = track.wheel(-2.0).expect("wheel event");
        assert_eq!(tick.position, Some((10.0, 10.0)));
        assert_eq!(tick.wheel, -2.0);
        assert_eq!(
            tick.delta,
            (0.0, 0.0),
            "wheel events never carry cursor deltas"
        );
        let cleared = track.clear_focus();
        assert_eq!(cleared.held, [false, false]);
        assert_eq!(cleared.pressed, [false, false]);
        assert_eq!(cleared.released, [false, false]);
        assert_eq!(cleared.position, None);
        assert_eq!(track.last_pos, None, "focus reset drops the drag baseline");
    }

    #[test]
    fn pointer_event_maps_surface_pixels_to_grid_cells() {
        use super::{PointerEvent, PointerTrack};

        let event = PointerEvent {
            position: Some((639.0, 479.0)),
            surface_size: (640, 480),
            held: [false; 2],
            pressed: [false; 2],
            released: [false; 2],
            delta: (0.0, 0.0),
            wheel: 0.0,
        };
        assert_eq!(event.grid_position(640, 480, 80, 30), Some((79, 29)));
        assert_eq!(event.grid_position_fitted(640, 480, 80, 30, 10, 16), None);
        let fitted = PointerEvent {
            position: Some((320.0, 240.0)),
            ..event
        };
        assert_eq!(
            fitted.grid_position_fitted(640, 480, 80, 30, 10, 16),
            Some((40, 15))
        );
        assert_eq!(event.grid_position(640, 480, 0, 30), None);

        let outside = PointerEvent {
            position: Some((640.0, 480.0)),
            ..event
        };
        assert_eq!(outside.grid_position(640, 480, 80, 30), None);
        assert_eq!(
            PointerEvent {
                position: None,
                ..event
            }
            .grid_position(640, 480, 80, 30),
            None
        );

        let mut held = PointerTrack::default();
        held.button(0, true).expect("button event");
        let cleared = held.clear_focus();
        assert_eq!(cleared.released, [true, false]);
    }

    #[test]
    fn shader_contract_interpolates_glyph_uv_and_preserves_flat_cells() {
        assert!(SHADER.contains("mix(i.fg_start, i.fg_end, blend)"));
        assert!(SHADER.contains("i.glyph_uv.y"));
        assert!(SHADER.contains("i.gradient_axis == 2u"));
        assert!(SHADER.contains("u.viewport_origin"));
        assert!(CONTINUOUS_SHADER.contains("textureLoad(bloom_field"));
        assert!(CONTINUOUS_SHADER.contains("lit_fg"));
        assert!(CONTINUOUS_SHADER.contains("select(vec3<f32>(0.0), bloom, i.world != 0u)"));
    }

    #[test]
    fn continuous_shader_applies_transform_channels_and_opacity() {
        assert!(CONTINUOUS_SHADER.contains("cos(rotation)"));
        assert!(CONTINUOUS_SHADER.contains("skew.x * local.y"));
        assert!(CONTINUOUS_SHADER.contains("u.viewport_size"));
        // Tone-mapped light + receiver contract (review 2026-10-25): the
        // raw additive clamp is gone; world cells respond through the
        // energy curve and gain a faint fill inside strong fields.
        assert!(CONTINUOUS_SHADER.contains("1.0 - exp(-peak * 1.7)"));
        assert!(CONTINUOUS_SHADER.contains("let recv = energy * 0.38;"));
        assert!(!CONTINUOUS_SHADER.contains("min(fg + select(vec3<f32>(0.0), bloom"));
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
            *cell = Cell::new('x', Rgb::new(1, 1, 1), Rgb::new(80, 40, 20));
        }
        let instances = frame_instances(&frame, 4, 1, &[(1, 0, 2, 1)]);
        let flags: Vec<u32> = instances.iter().map(|i| i.world).collect();
        assert_eq!(flags, vec![0, 1, 1, 0]);
        assert_eq!(instances[1].bg, [0.0, 0.0, 0.0]);
        assert_eq!(instances[0].bg, [80.0 / 255.0, 40.0 / 255.0, 20.0 / 255.0]);
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
