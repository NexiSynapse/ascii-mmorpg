//! GPU triangle fill for the windowed 3D world (the `play` example).
//!
//! The CPU depth backend ([`crate::raster`]) does: project → flat-shade →
//! barycentric-fill a 4x supersampled buffer → resolve cells. The fill is
//! the expensive part (~every frame, per-triangle bounding-box loops), and
//! is exactly what a GPU compute shader parallelizes. This module keeps the
//! *resolve* (glyph language, fog, rim) on the CPU, byte-identical, and
//! replaces only the fill:
//!
//! 1. The CPU runs raster's own clip→project→shade pass once per frame and
//!    uploads the screened triangles (sample-space coords, inv-z, flat
//!    color) to a storage buffer.
//! 2. A compute pass (one thread per 4x sample) replays `fill_tri`'s exact
//!    per-sample math — same edge functions, same float expression order,
//!    same triangle order per sample — writing `(depth, rgb, lum)` into a
//!    sample buffer. CPU-side tile bins (built from each tri's `fill_tri`
//!    bbox, indices ascending) keep each thread's visitation set and order
//!    identical to the CPU scan, so parity is arithmetic identity: only f32
//!    scheduling may move a boundary sample by one ulp.
//! 3. The buffer copies into a mapped readback that `submit` requests and
//!    `collect` decodes (after the presenter's swapchain acquire — the vsync
//!    wait doubles as the compute window) into the same `raster::Sample`
//!    layout the CPU fill produces.
//! 4. `raster::resolve_layer` finishes: identical glyphs, colors, fog and
//!    rim, by construction.
//!
//! (A first cut used a hardware raster pass; on the dev GPU fragments from
//! triangles with non-1 clip w were silently dropped, and even then the
//! rasterizer's coverage rules could never match the CPU's inclusive edge
//! test sample-for-sample. Replaying the CPU's fill math directly removes
//! every difference except floating-point scheduling.)

use super::GpuWorld;
use crate::raster::{self, Camera3, DepthOpts, Mesh, Vec3};
use std::mem::size_of;

/// Device-side state for the 3D world fill pass. Owned by the presenter
/// `Gpu`; created once per window, buffers regrow on resize.
pub struct World3d {
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::ComputePipeline,
    uniform_buf: wgpu::Buffer,
    bind: Option<wgpu::BindGroup>,
    tri_buf: wgpu::Buffer,
    tri_cap: usize,
    light_buf: wgpu::Buffer,
    light_cap: usize,
    bin_buf: wgpu::Buffer,
    bin_cap: u64,
    idx_buf: wgpu::Buffer,
    idx_cap: u64,
    out_buf: wgpu::Buffer,
    /// Two readback buffers in a ring. Frame N's fill cannot complete until
    /// the previous presents drain (DXGI frame latency), so this frame's map
    /// is never waited on: `retire_pending` decodes whichever maps already
    /// finished — last frame's at `submit` (steady state), this frame's at
    /// `collect` when the GPU was fast — and a straggler keeps its map
    /// pending for a later call at the cost of one stale world layer.
    readback: [Option<wgpu::Buffer>; 4],
    pending: [Option<std::sync::mpsc::Receiver<Result<(), wgpu::BufferAsyncError>>>; 4],
    /// Preferred buffer for the next fill (alternates each submitted frame).
    fill_index: usize,
    readback_pitch: u32,
    /// Current supersampled extents (0 = not yet created).
    w2: u32,
    h2: u32,
    /// Per-buffer camera/opts/z snapshot taken at submit, for the resolve.
    decode_ctx: [Option<(Camera3, DepthOpts, i32)>; 4],
    last_layer: Option<crate::Layer>,
    samples: Vec<raster::Sample>,
    prof_pre: f32,
    prof_bins: f32,
    prof_sub: f32,
    prof_tris: usize,
    prof_idxs: usize,
}

/// One uploaded fill triangle: screen-space supersample coordinates,
/// perspective-correct 1/z per vertex, and the flat shade — exactly the
/// inputs `raster::fill_tri` consumes after `clip_near` + projection.
/// 80 bytes (screen vertices, inverse depth, colour, luminance, view normal,
/// material index, and face-centre world position).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct GpuTri {
    sx: [f32; 3],
    sy: [f32; 3],
    iz: [f32; 3],
    col: [f32; 3],
    lum: f32,
    normal: [f32; 3],
    material: f32,
    center: [f32; 3],
}

/// Compute-side uniforms: sample-space extents, uploaded tri count, and
/// the tile-bin column count.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct FillUniform {
    w2: u32,
    h2: u32,
    tri_count: u32,
    tiles_x: u32,
    light_count: u32,
    _pad: [u32; 3],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct GpuLight {
    position_radius: [f32; 4],
    color_intensity: [f32; 4],
}

/// Sample tiles are 64x64 samples; bins keep `fill_tri`'s bbox visitation
/// set without making every sample thread scan every triangle.
const TILE: u32 = 64;

const FILL_SHADER: &str = r#"struct FillU {
    w2: u32, h2: u32, tri_count: u32, tiles_x: u32,
    light_count: u32, _pad0: u32, _pad1: u32, _pad2: u32,
};
// A screened fill triangle: the exact inputs `raster::fill_tri` loops over.
// array<f32,3> members keep 4-byte alignment, matching the Rust struct.
struct CTri {
    sx: array<f32, 3>,
    sy: array<f32, 3>,
    iz: array<f32, 3>,
    col: array<f32, 3>,
    lum: f32,
    normal: array<f32, 3>,
    material: f32,
    center: array<f32, 3>,
};
struct CLight {
    position_radius: vec4<f32>,
    color_intensity: vec4<f32>,
};
@group(0) @binding(0) var<storage, read> tris: array<CTri>;
@group(0) @binding(1) var<uniform> fu: FillU;
// Per sample: depth, r, g, b, lum, lum_sq, normal.xyz, material index.
@group(0) @binding(2) var<storage, read_write> out: array<f32>;
// Tile bins: (start, count) pairs into the flat tri-index list. The CPU
// registers each tri in exactly the tiles covered by its fill_tri bbox,
// appending in upload order, so per-sample tri order matches the CPU fill.
@group(0) @binding(3) var<storage, read> bins: array<vec2<u32>>;
@group(0) @binding(4) var<storage, read> idxs: array<u32>;
@group(0) @binding(5) var<storage, read> lights: array<CLight>;

fn edge(ax: f32, ay: f32, bx: f32, by: f32, px: f32, py: f32) -> f32 {
    return (bx - ax) * (py - ay) - (by - ay) * (px - ax);
}

fn light_response(material: f32) -> f32 {
    if (material == 8.0) { return 0.82; }
    if (material == 5.0) { return 0.52; }
    if (material == 1.0 || material == 3.0) { return 0.72; }
    if (material == 6.0 || material == 7.0) { return 0.95; }
    return 0.64;
}

@compute @workgroup_size(8, 8)
fn fill(@builtin(global_invocation_id) gid: vec3<u32>) {
    let w2 = fu.w2;
    let h2 = fu.h2;
    if (gid.x >= w2 || gid.y >= h2) {
        return;
    }
    let px = gid.x;
    let py = gid.y;
    // Sample centers — the CPU fill's exact probe points.
    let fx = f32(px) + 0.5;
    let fy = f32(py) + 0.5;
    // The CPU fill starts every sample clear (a negative depth marks the
    // miss sentinel for the decode) and overwrites on strictly nearer
    // first hits, scanning triangles in upload order: identical here, so
    // depth ties resolve to the same triangle on both paths.
    var bd = -1.0;
    var br = 0.0;
    var bg = 0.0;
    var bb = 0.0;
    var bl = 0.0;
    var bnx = 0.0;
    var bny = 0.0;
    var bnz = 1.0;
    var bm = 0.0;
    var bcx = 0.0;
    var bcy = 0.0;
    var bcz = 0.0;
    let span = bins[(py / 64u) * fu.tiles_x + (px / 64u)];
    var k = span.x;
    while (k < span.x + span.y) {
        let tri = tris[idxs[k]];
        let area = edge(tri.sx[0], tri.sy[0], tri.sx[1], tri.sy[1], tri.sx[2], tri.sy[2]);
        if (abs(area) >= 1e-9) {
            let b0 = edge(tri.sx[1], tri.sy[1], tri.sx[2], tri.sy[2], fx, fy) / area;
            let b1 = edge(tri.sx[2], tri.sy[2], tri.sx[0], tri.sy[0], fx, fy) / area;
            let b2 = edge(tri.sx[0], tri.sy[0], tri.sx[1], tri.sy[1], fx, fy) / area;
            if (b0 >= 0.0 && b1 >= 0.0 && b2 >= 0.0) {
                let w = b0 * tri.iz[0] + b1 * tri.iz[1] + b2 * tri.iz[2];
                if (w > 0.0) {
                    let z = 1.0 / w;
                    if (bd < 0.0 || z < bd) {
                        bd = z;
                        br = tri.col[0];
                        bg = tri.col[1];
                        bb = tri.col[2];
                        bl = tri.lum;
                        bnx = tri.normal[0];
                        bny = tri.normal[1];
                        bnz = tri.normal[2];
                        bm = tri.material;
                        bcx = tri.center[0];
                        bcy = tri.center[1];
                        bcz = tri.center[2];
                    }
                }
            }
        }
        k += 1u;
    }
    if (bd >= 0.0) {
        var lit = vec3<f32>(br, bg, bb);
        let response = light_response(bm);
        var li = 0u;
        while (li < fu.light_count) {
            let light = lights[li];
            let delta = vec3<f32>(bcx, bcy, bcz) - light.position_radius.xyz;
            let radius = max(light.position_radius.w, 0.001);
            let distance = length(delta);
            let falloff = max(1.0 - distance / radius, 0.0);
            let contribution = falloff * falloff * max(light.color_intensity.w, 0.0) * response;
            lit += light.color_intensity.xyz * contribution;
            li += 1u;
        }
        lit = clamp(lit, vec3<f32>(0.0), vec3<f32>(1.0));
        br = lit.x;
        bg = lit.y;
        bb = lit.z;
        bl = dot(lit, vec3<f32>(0.2126, 0.7152, 0.0722));
    }
    let base = (py * w2 + px) * 10u;
    out[base] = bd;
    out[base + 1u] = br;
    out[base + 2u] = bg;
    out[base + 3u] = bb;
    out[base + 4u] = bl;
    out[base + 5u] = bl * bl;
    out[base + 6u] = bnx;
    out[base + 7u] = bny;
    out[base + 8u] = bnz;
    out[base + 9u] = bm;
}
"#;

impl World3d {
    pub(crate) fn new(device: &wgpu::Device) -> Result<Self, String> {
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("world3d-bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: false },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("world3d-shader"),
            source: wgpu::ShaderSource::Wgsl(FILL_SHADER.into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("world3d-layout"),
            bind_group_layouts: &[&bgl],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("world3d-pipeline"),
            layout: Some(&pipeline_layout),
            module: &shader,
            entry_point: Some("fill"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            cache: None,
        });
        let uniform_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("world3d-uniforms"),
            size: size_of::<FillUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let tri_cap = 4096usize;
        let tri_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("world3d-tris"),
            size: (tri_cap * size_of::<GpuTri>()).max(1) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let light_cap = 64usize;
        let light_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("world3d-lights"),
            size: (light_cap * size_of::<GpuLight>()).max(1) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Ok(Self {
            layout: bgl,
            pipeline,
            uniform_buf,
            bind: None,
            tri_buf,
            tri_cap,
            light_buf,
            light_cap,
            bin_buf: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("world3d-bins"),
                size: 1,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            bin_cap: 0,
            idx_buf: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("world3d-idxs"),
                size: 1,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            idx_cap: 0,
            out_buf: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("world3d-outs"),
                size: 1,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            }),
            readback: [None, None, None, None],
            pending: [None, None, None, None],
            fill_index: 0,
            readback_pitch: 0,
            w2: 0,
            h2: 0,
            decode_ctx: [None, None, None, None],
            last_layer: None,
            samples: Vec::new(),
            prof_pre: 0.0,
            prof_bins: 0.0,
            prof_sub: 0.0,
            prof_tris: 0,
            prof_idxs: 0,
        })
    }

    /// Recreate the bind group — needed whenever `tri_buf` or `out_buf`
    /// regrow (bindings hold buffer handles by value).
    fn rebuild_bind(&mut self, device: &wgpu::Device) {
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("world3d-bg"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.tri_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: self.uniform_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: self.out_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: self.bin_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: self.idx_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: self.light_buf.as_entire_binding(),
                },
            ],
        });
        self.bind = Some(bind);
    }

    /// (Re)create the sample output + readback buffers for the grid; also
    /// (re)binds, since `out_buf` is part of the group.
    fn ensure_targets(&mut self, device: &wgpu::Device, w2: u32, h2: u32) {
        if self.w2 == w2 && self.h2 == h2 && self.readback[0].is_some() {
            return;
        }
        // 10 floats (40 bytes) per sample; rows pad to 256 bytes (copy rule).
        let pitch = ((w2 * 40) + 255) / 256 * 256;
        let size = u64::from(pitch) * u64::from(h2.max(1));
        // Dropping a mapped buffer is an error, so drain the ring first
        // (resize is rare — the same full retire the client ring does).
        for index in 0..4 {
            if let Some(rx) = self.pending[index].take() {
                device.poll(wgpu::Maintain::Wait);
                let _ = rx.recv();
                if let Some(readback) = &self.readback[index] {
                    readback.unmap();
                }
            }
        }
        self.out_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("world3d-outs"),
            size: size.max(1),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        for slot in &mut self.readback {
            *slot = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("world3d-readback"),
                size: size.max(1),
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        self.readback_pitch = pitch;
        self.w2 = w2;
        self.h2 = h2;
        self.rebuild_bind(device);
    }

    /// Rasterize `world`'s meshes on the GPU and resolve to a CPU glyph
    /// layer (the same type the CPU backend produced for this slot).
    /// Synchronous test wrapper (parity checks); the live presenter splits
    /// this into `submit` + `collect` so the fill overlaps the vsync wait.
    #[cfg(test)]
    pub(crate) fn run(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        world: &GpuWorld,
    ) -> Result<crate::Layer, String> {
        self.submit(device, queue, world)?;
        device.poll(wgpu::Maintain::Wait);
        self.retire_pending(device, std::time::Duration::from_secs(1))?;
        self.last_layer
            .clone()
            .ok_or_else(|| "world3d readback did not complete".to_string())
    }

    /// Queue this frame's fill and request its readback map. Non-blocking:
    /// the GPU fill runs while the presenter acquires the swapchain texture
    /// and composes the rest of the frame; `collect` finishes the readback.
    pub(crate) fn submit(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        world: &GpuWorld,
    ) -> Result<(), String> {
        // Decode the previous frame's completed readback (its fill ran during
        // the last vsync window) before refilling buffers. Bounded: a
        // straggler keeps its map pending and is decoded next frame.
        self.retire_pending(device, std::time::Duration::from_millis(5))?;
        let cam = world.camera;
        let w2 = (cam.width.max(1) as usize * raster::SAMPLE_GRID) as u32;
        let h2 = (cam.height.max(1) as usize * raster::SAMPLE_GRID) as u32;
        self.ensure_targets(device, w2, h2);

        // CPU pre-pass: raster's own clip→project→shade pipeline, so the
        // screened triangles are what `fill_tri` would see bit for bit.
        let t0 = std::time::Instant::now();
        let sun = world.sun.norm();
        let tris = screen_tris(&world.meshes, &cam, sun, &world.opts);
        let tri_count = tris.len();
        let t_pre = t0.elapsed();

        if tri_count > self.tri_cap {
            let new_cap = (tri_count * 2).max(4096);
            self.tri_buf = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("world3d-tris"),
                size: (new_cap * size_of::<GpuTri>()).max(1) as u64,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.tri_cap = new_cap;
            self.rebuild_bind(device);
        }

        // Tile bins: `fill_tri` visits a tri only inside its sample-space
        // bbox, which is what keeps the CPU fill O(covered×overdraw) rather
        // than O(samples×tris). Register each tri in exactly the tiles its
        // clamped bbox occupies — same floor/ceil expressions as fill_tri —
        // appending in upload order, so every tile's index list stays
        // globally ascending and per-sample tri order (hence depth ties)
        // matches the CPU fill bit for bit.
        let tiles_x = ((w2 + TILE - 1) / TILE).max(1);
        let tiles_y = ((h2 + TILE - 1) / TILE).max(1);
        let tiles = (tiles_x * tiles_y) as usize;
        let mut counts = vec![0u32; tiles];
        let mut rects: Vec<Rect> = Vec::with_capacity(tri_count);
        for t in &tris {
            let min_x =
                t.sx.iter()
                    .cloned()
                    .fold(f32::INFINITY, f32::min)
                    .floor()
                    .max(0.0) as i32;
            let max_x =
                t.sx.iter()
                    .cloned()
                    .fold(f32::NEG_INFINITY, f32::max)
                    .ceil()
                    .min(w2 as f32) as i32;
            let min_y =
                t.sy.iter()
                    .cloned()
                    .fold(f32::INFINITY, f32::min)
                    .floor()
                    .max(0.0) as i32;
            let max_y =
                t.sy.iter()
                    .cloned()
                    .fold(f32::NEG_INFINITY, f32::max)
                    .ceil()
                    .min(h2 as f32) as i32;
            rects.push((min_x, max_x, min_y, max_y));
            if min_x < max_x && min_y < max_y {
                let (tx0, tx1) = tile_range(min_x, max_x, tiles_x);
                let (ty0, ty1) = tile_range(min_y, max_y, tiles_y);
                for ty in ty0..=ty1 {
                    for tx in tx0..=tx1 {
                        counts[(ty * tiles_x + tx) as usize] += 1;
                    }
                }
            }
        }
        // (start, count) per tile + a flat index list grouped by tile; the
        // fill cursor starts at each bin's start offset.
        let mut bins = vec![[0u32; 2]; tiles];
        let mut running = 0u32;
        for (i, c) in counts.iter().enumerate() {
            bins[i][0] = running;
            bins[i][1] = *c;
            running += *c;
        }
        let mut cursor: Vec<u32> = bins.iter().map(|b| b[0]).collect();
        let mut idxs = vec![0u32; running as usize];
        for (ti, r) in rects.iter().enumerate() {
            if r.0 < r.1 && r.2 < r.3 {
                let (tx0, tx1) = tile_range(r.0, r.1, tiles_x);
                let (ty0, ty1) = tile_range(r.2, r.3, tiles_y);
                for ty in ty0..=ty1 {
                    for tx in tx0..=tx1 {
                        let t = (ty * tiles_x + tx) as usize;
                        idxs[cursor[t] as usize] = ti as u32;
                        cursor[t] += 1;
                    }
                }
            }
        }

        let bin_bytes = (bins.len() * 8).max(1) as u64;
        let t_bins = t0.elapsed();
        if bin_bytes > self.bin_cap {
            let cap = bin_bytes * 8;
            self.bin_buf = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("world3d-bins"),
                size: cap,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.bin_cap = cap;
            self.rebuild_bind(device);
        }
        let idx_bytes = (idxs.len() * 4).max(1) as u64;
        if idx_bytes > self.idx_cap {
            let cap = idx_bytes * 8;
            self.idx_buf = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("world3d-idxs"),
                size: cap,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.idx_cap = cap;
            self.rebuild_bind(device);
        }

        let light_data: Vec<GpuLight> = world
            .lights
            .iter()
            .map(|light| GpuLight {
                position_radius: [
                    light.position.x,
                    light.position.y,
                    light.position.z,
                    light.radius,
                ],
                color_intensity: [
                    f32::from(light.color.r) / 255.0,
                    f32::from(light.color.g) / 255.0,
                    f32::from(light.color.b) / 255.0,
                    light.intensity,
                ],
            })
            .collect();
        if light_data.len() > self.light_cap {
            let new_cap = (light_data.len() * 2).max(64);
            self.light_buf = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("world3d-lights"),
                size: (new_cap * size_of::<GpuLight>()).max(1) as u64,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.light_cap = new_cap;
            self.rebuild_bind(device);
        }

        // Pick a free readback buffer, rotating from the preferred slot; if
        // every ring slot still holds an unfinished map (GPU starved), skip
        // this fill — the world layer stays one frame older until it drains.
        let idx = (0..4)
            .map(|offset| (self.fill_index + offset) % 4)
            .find(|slot| self.pending[*slot].is_none());
        let Some(idx) = idx else {
            if std::env::var("PRESENT_PROFILE").is_ok() {
                eprintln!("[world3d] all readbacks pending: skipping fill");
            }
            return Ok(());
        };
        let readback = self.readback[idx].clone().expect("readback target");
        let bind = self.bind.as_ref().expect("world3d bind");
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        if !tris.is_empty() {
            queue.write_buffer(&self.tri_buf, 0, bytemuck::cast_slice(&tris));
        }
        if !bins.is_empty() {
            queue.write_buffer(&self.bin_buf, 0, bytemuck::cast_slice(&bins));
        }
        if !idxs.is_empty() {
            queue.write_buffer(&self.idx_buf, 0, bytemuck::cast_slice(&idxs));
        }
        if !light_data.is_empty() {
            queue.write_buffer(&self.light_buf, 0, bytemuck::cast_slice(&light_data));
        }
        let uniform = FillUniform {
            w2,
            h2,
            tri_count: tri_count as u32,
            tiles_x,
            light_count: light_data.len() as u32,
            _pad: [0; 3],
        };
        queue.write_buffer(&self.uniform_buf, 0, bytemuck::bytes_of(&uniform));
        {
            let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor::default());
            cpass.set_pipeline(&self.pipeline);
            cpass.set_bind_group(0, bind, &[]);
            cpass.dispatch_workgroups((w2 + 7) / 8, (h2 + 7) / 8, 1);
        }
        encoder.copy_buffer_to_buffer(&self.out_buf, 0, &readback, 0, self.out_size());
        queue.submit([encoder.finish()]);
        let t_sub = t0.elapsed();

        // Request the readback map but do not wait for it: the next `submit`
        // (or this frame's `collect` when the GPU is fast) decodes it.
        let slice = readback.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |res| {
            let _ = tx.send(res);
        });
        self.pending[idx] = Some(rx);
        self.decode_ctx[idx] = Some((world.camera, world.opts, world.z));
        self.fill_index = (idx + 1) % 4;
        if std::env::var("PRESENT_PROFILE").is_ok() {
            self.prof_pre = t_pre.as_secs_f32() * 1e3;
            self.prof_bins = (t_bins - t_pre).as_secs_f32() * 1e3;
            self.prof_sub = t_sub.as_secs_f32() * 1e3;
            self.prof_tris = tri_count;
            self.prof_idxs = idxs.len();
        }
        Ok(())
    }

    /// Serve the freshest decoded world layer. A short bounded drain decodes
    /// this frame's own map when the GPU already finished it (fast path: no
    /// staleness); otherwise the previous frame's layer is served and this
    /// frame's map completes by the next `submit`. Never blocks on presents.
    pub(crate) fn collect(
        &mut self,
        device: &wgpu::Device,
    ) -> Result<Option<crate::Layer>, String> {
        self.retire_pending(device, std::time::Duration::from_millis(5))?;
        Ok(self.last_layer.clone())
    }

    /// Decode every readback whose map has completed, waiting up to `budget`
    /// total. Stragglers keep their map pending for a later call; each
    /// completed buffer refreshes `last_layer`.
    fn retire_pending(
        &mut self,
        device: &wgpu::Device,
        budget: std::time::Duration,
    ) -> Result<(), String> {
        let deadline = std::time::Instant::now() + budget;
        for index in 0..4 {
            let Some(rx) = self.pending[index].take() else {
                continue;
            };
            // try_recv consumes the completion, so the loop exits WITH the
            // result; a straggler is put back and left for a later call.
            let mut result: Option<Result<Result<(), wgpu::BufferAsyncError>, String>> = None;
            loop {
                device.poll(wgpu::Maintain::Poll);
                match rx.try_recv() {
                    Ok(res) => {
                        result = Some(Ok(res));
                        break;
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => {}
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        result = Some(Err("world3d readback channel dropped".to_string()));
                        break;
                    }
                }
                if std::time::Instant::now() >= deadline {
                    break; // straggler: result stays None, map goes back
                }
                std::thread::sleep(std::time::Duration::from_micros(250));
            }
            let Some(result) = result else {
                self.pending[index] = Some(rx);
                continue;
            };
            result
                .map_err(|e| format!("world3d readback: {e}"))?
                .map_err(|e| format!("world3d readback: {e}"))?;
            self.decode_readback(index)?;
        }
        Ok(())
    }

    /// Decode a completed readback buffer into `last_layer`. Its map must
    /// already be complete (the receiver delivered) and still mapped.
    fn decode_readback(&mut self, index: usize) -> Result<(), String> {
        let readback = self.readback[index].clone().expect("readback target");
        let t_dec0 = std::time::Instant::now();
        // Map + decode into the CPU Sample layout the resolve speaks.
        let data = readback.slice(..).get_mapped_range();
        let w2 = self.w2 as usize;
        let chunk_len = w2 * 40;
        if self.samples.len() != w2 * self.h2 as usize {
            self.samples = vec![raster::Sample::empty(); w2 * self.h2 as usize];
        } else {
            self.samples
                .iter_mut()
                .for_each(|s| *s = raster::Sample::empty());
        }
        for row in 0..self.h2 as usize {
            let row_bytes = &data[row * self.readback_pitch as usize
                ..row * self.readback_pitch as usize + chunk_len];
            let floats: &[f32] = bytemuck::cast_slice(row_bytes);
            for px in 0..w2 {
                let z = floats[px * 10];
                if z < 0.0 {
                    continue; // no sample here (clear sentinel)
                }
                let material =
                    raster::MaterialKind::from_index(floats[px * 10 + 9].round() as usize);
                self.samples[row * w2 + px] = raster::Sample {
                    depth: z,
                    r: floats[px * 10 + 1],
                    g: floats[px * 10 + 2],
                    b: floats[px * 10 + 3],
                    lum: floats[px * 10 + 4],
                    lum_sq: floats[px * 10 + 5],
                    normal_x: floats[px * 10 + 6],
                    normal_y: floats[px * 10 + 7],
                    normal_z: floats[px * 10 + 8],
                    material,
                };
            }
        }
        drop(data);
        readback.unmap();
        let t_dec = t_dec0.elapsed();
        let (cam, opts, z) = self.decode_ctx[index].ok_or("world3d: missing decode context")?;
        let t_res0 = std::time::Instant::now();
        let layer = raster::resolve_layer(&self.samples, &cam, &opts, z);
        if std::env::var("PRESENT_PROFILE").is_ok() {
            eprintln!(
                "[world3d] pre={:.2}ms bins={:.2}ms sub={:.2}ms map+dec={:.2}ms resolve={:.2}ms tris={} idxs={} grid={}x{}",
                self.prof_pre,
                self.prof_bins,
                self.prof_sub,
                t_dec.as_secs_f32() * 1e3,
                t_res0.elapsed().as_secs_f32() * 1e3,
                self.prof_tris,
                self.prof_idxs,
                self.w2,
                self.h2
            );
        }
        self.last_layer = Some(layer);
        Ok(())
    }

    fn out_size(&self) -> u64 {
        u64::from(self.readback_pitch) * u64::from(self.h2.max(1))
    }
}

/// Clamped triangle bbox in integer sample space (`min <= x < max`).
type Rect = (i32, i32, i32, i32);

/// Tile coordinate range `[t0, t1]` for a half-open sample span `[min,max)`
/// that is already clamped into `[0, w2)/[0, h2)`.
fn tile_range(min: i32, max: i32, tiles: u32) -> (u32, u32) {
    (
        (min.max(0) as u32 / TILE).min(tiles - 1),
        ((max - 1).max(0) as u32 / TILE).min(tiles - 1),
    )
}

/// The CPU half of the fill: raster's exact view basis → shade → near-clip →
/// supersample projection, producing what `raster::fill_tri` consumes. The
/// GPU computes the per-sample barycentric part from these inputs.
fn screen_tris(meshes: &[Mesh], cam: &Camera3, sun: Vec3, opts: &DepthOpts) -> Vec<GpuTri> {
    let (fwd, right, up) = raster::basis(cam);
    let cx = cam.width as f32 * 0.5;
    let cy = cam.height as f32 * 0.5;
    let mut tris: Vec<GpuTri> = Vec::new();
    for mesh in meshes {
        for tri in &mesh.tris {
            // View space first so near-plane straddlers clip instead of
            // dropping whole faces (same order as `rasterize_tri`).
            let view = tri.v.map(|p| {
                let d = p - cam.pos;
                Vec3::new(d.dot(right), d.dot(up), d.dot(fwd))
            });
            // Flat diffuse from the world-space face normal (two-sided,
            // shared verbatim with the CPU fill).
            let (sr, sg, sb, lum) = raster::shade_face(tri, sun, opts.ambient);
            let center = Vec3::new(
                (tri.v[0].x + tri.v[1].x + tri.v[2].x) / 3.0,
                (tri.v[0].y + tri.v[1].y + tri.v[2].y) / 3.0,
                (tri.v[0].z + tri.v[1].z + tri.v[2].z) / 3.0,
            );
            let n = (tri.v[1] - tri.v[0]).cross(tri.v[2] - tri.v[0]).norm();
            let view_normal = Vec3::new(n.dot(right), n.dot(up), n.dot(fwd)).norm();
            let material = raster::MaterialKind::classify(tri.albedo).index() as f32;
            for clipped in raster::clip_near(view, opts.near) {
                let mut sx = [0.0f32; 3];
                let mut syv = [0.0f32; 3];
                let mut iz = [0.0f32; 3];
                for (i, v) in clipped.iter().enumerate() {
                    // Supersample space: one cell covers SAMPLE_GRID² samples.
                    sx[i] = (cx + cam.focal * v.x / v.z) * raster::SAMPLE_GRID as f32;
                    syv[i] = (cy - cam.focal * v.y / v.z) * raster::SAMPLE_GRID as f32;
                    iz[i] = 1.0 / v.z;
                }
                tris.push(GpuTri {
                    sx,
                    sy: syv,
                    iz,
                    col: [sr, sg, sb],
                    lum,
                    normal: [view_normal.x, view_normal.y, view_normal.z],
                    material,
                    center: [center.x, center.y, center.z],
                });
            }
        }
    }
    tris
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::raster::{shade_face, PointLight};
    use crate::Rgb;

    #[test]
    fn gpu_world_buffer_layout_is_packed_for_lights() {
        assert_eq!(size_of::<GpuTri>(), 80);
        assert_eq!(size_of::<FillUniform>(), 32);
        assert_eq!(size_of::<GpuLight>(), 32);
    }

    #[test]
    fn shade_face_matches_fill_math_two_sided() {
        // A face rotated 180° lights exactly the same (|dot|), and
        // luminance uses Rec709 weights — shared with the fill pass.
        let tri = raster::Tri {
            v: [
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
            ],
            albedo: Rgb::new(120, 200, 80),
        };
        let sun = Vec3::new(0.55, 0.85, 0.3).norm();
        let (r, g, b, lum) = shade_face(&tri, sun, 0.3);
        assert!(r > 0.08 && r < 1.0 && g > 0.08 && g < 1.0 && b > 0.08 && b < 1.0);
        let want = 0.2126 * r + 0.7152 * g + 0.0722 * b;
        assert!((lum - want).abs() < 1e-5);

        let flipped = raster::Tri {
            v: [
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
            ],
            albedo: Rgb::new(120, 200, 80),
        };
        let (r2, g2, _b2, lum2) = shade_face(&flipped, sun, 0.3);
        assert!((r2 - r).abs() < 1e-5 && (g2 - g).abs() < 1e-5);
        assert!((lum2 - lum).abs() < 1e-5);
    }

    #[test]
    fn empty_meshes_resolve_to_void_layer() {
        let cam = Camera3::new(Vec3::new(0.0, 2.2, -9.0), 0.0, 0.0, 60.0, 96, 35);
        let misses =
            vec![raster::Sample::empty(); 96 * raster::SAMPLE_GRID * 35 * raster::SAMPLE_GRID];
        let layer = raster::resolve_layer(&misses, &cam, &DepthOpts::default(), 2);
        assert!(layer.cells.is_empty(), "void shows through; no cells");
    }

    /// The screened-triangle prepass must produce finite, positive inv-z
    /// inputs in sample space — the core the compute fill loops over.
    #[test]
    fn screen_prepass_yields_finite_sample_space_inputs() {
        let cam = Camera3::new(Vec3::new(0.0, 3.0, -9.0), 0.0, 0.0, 60.0, 96, 35);
        let sun = Vec3::new(0.55, 0.85, 0.3);
        let opts = DepthOpts {
            near: 0.5,
            far: 44.0,
            ambient: 0.3,
            edge_step: 0.28,
            ..DepthOpts::default()
        };
        let wall = Mesh::boxed(
            Vec3::new(0.0, 1.0, 5.0),
            Vec3::new(10.0, 4.0, 0.05),
            Rgb::new(200, 60, 60),
        );
        let tris = screen_tris(&[wall], &cam, sun.norm(), &opts);
        assert!(!tris.is_empty(), "boxed wall yields screened tris");
        for t in &tris {
            for k in 0..3 {
                assert!(t.iz[k].is_finite() && t.iz[k] > 0.0);
                assert!(t.sx[k].is_finite() && t.sy[k].is_finite());
            }
        }
        // The wall's front face sits at constant view depth 14.025: the
        // first (CCW-positive) screened tri must carry that inv-z.
        let front = &tris[0];
        assert!((front.iz[0] - 1.0 / 14.025).abs() < 1e-4);
    }

    #[test]
    fn point_lights_are_resolved_by_the_compute_shader() {
        let cam = Camera3::new(Vec3::new(0.0, 3.0, -9.0), 0.0, 0.0, 60.0, 96, 35);
        let sun = Vec3::new(0.55, 0.85, 0.3);
        let opts = DepthOpts::default();
        let wall = Mesh::boxed(
            Vec3::new(0.0, 1.0, 5.0),
            Vec3::new(10.0, 4.0, 0.05),
            Rgb::new(120, 90, 70),
        );
        let tris = screen_tris(&[wall], &cam, sun.norm(), &opts);
        assert!(!tris.is_empty());
        assert!(tris[0].center.iter().all(|value| value.is_finite()));
        assert!(FILL_SHADER.contains("light_count"));
        assert!(FILL_SHADER.contains("light_response"));
        assert!(FILL_SHADER.contains("@group(0) @binding(5)"));
    }

    /// The real contract: a scene through the GPU fill must resolve to the
    /// same glyphs/colors as the CPU fill. Skips (instead of failing) when
    /// no GPU adapter exists (CI never compiles `present`, so this runs
    /// only on real dev machines with a window-capable GPU).
    #[test]
    fn gpu_resolve_matches_cpu_fill() {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::VULKAN,
            ..Default::default()
        });
        let Some(adapter) =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: None,
                force_fallback_adapter: false,
            }))
        else {
            eprintln!("gpu_resolve_matches_cpu_fill: no Vulkan adapter, skipping");
            return;
        };
        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("gpu3d-parity-test"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                memory_hints: wgpu::MemoryHints::Performance,
            },
            None,
        ))
        .expect("parity test device");

        let cam = Camera3::new(Vec3::new(0.0, 3.0, -9.0), 0.0, 0.0, 60.0, 96, 35);
        let sun = Vec3::new(0.55, 0.85, 0.3);
        let opts = DepthOpts {
            near: 0.5,
            far: 44.0,
            ambient: 0.3,
            edge_step: 0.28,
            ..DepthOpts::default()
        };
        // Cube on a ground slab, like the play world.
        let slab = Mesh::ground_slab(Rgb::new(18, 46, 28));
        let cube = Mesh::boxed(
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(1.4, 2.0, 1.4),
            Rgb::new(200, 200, 215),
        );
        let wood = Mesh::boxed(
            Vec3::new(6.0, 1.6, 4.0),
            Vec3::new(1.2, 3.2, 1.2),
            Rgb::new(90, 60, 40),
        );
        let meshes = vec![slab, cube, wood];
        let lights = vec![PointLight::new(
            Vec3::new(0.0, 1.8, 0.0),
            Rgb::new(255, 148, 48),
            5.0,
            1.15,
        )];
        let cpu = raster::render_layer_with_lights(&meshes, &cam, sun, &opts, &lights, 2);
        let mut world3d = World3d::new(&device).expect("world3d pipeline");
        let gpu = world3d
            .run(
                &device,
                &queue,
                &GpuWorld {
                    meshes,
                    camera: cam,
                    sun,
                    opts,
                    lights,
                    z: 2,
                    layer_slot: 0,
                },
            )
            .expect("gpu pass");

        // Map cells and compare. Boundary samples may drift by one ulp
        // (f32 scheduling differs between CPU and shader), so assert a
        // strong agreement rate plus per-color closeness on the overlap.
        use std::collections::HashMap;
        let mut cpu_map: HashMap<(i32, i32), crate::Cell> = HashMap::new();
        for c in &cpu.cells {
            cpu_map.insert((c.x, c.y), c.cell);
        }
        let mut covered = 0;
        let mut glyph_match = 0;
        let mut color_close = 0;
        let mut shown = 0;
        for c in &gpu.cells {
            if let Some(cpu_cell) = cpu_map.get(&(c.x, c.y)) {
                covered += 1;
                if cpu_cell.glyph == c.cell.glyph {
                    glyph_match += 1;
                } else if shown < 8 {
                    shown += 1;
                    eprintln!(
                        "({},{}) cpu='{}'({},{},{}) gpu='{}'({},{},{})",
                        c.x,
                        c.y,
                        cpu_cell.glyph,
                        cpu_cell.fg.r,
                        cpu_cell.fg.g,
                        cpu_cell.fg.b,
                        c.cell.glyph,
                        c.cell.fg.r,
                        c.cell.fg.g,
                        c.cell.fg.b
                    );
                }
                let near = |a: u8, b: u8| i32::from(a).abs_diff(i32::from(b)) <= 24;
                if near(cpu_cell.fg.r, c.cell.fg.r)
                    && near(cpu_cell.fg.g, c.cell.fg.g)
                    && near(cpu_cell.fg.b, c.cell.fg.b)
                {
                    color_close += 1;
                }
            }
        }
        eprintln!(
            "parity: gpu={} cpu={} covered={covered} glyph={glyph_match} color={color_close}",
            gpu.cells.len(),
            cpu.cells.len()
        );
        assert!(covered > 300, "overlap too small to prove anything");
        let rate = glyph_match as f32 / covered as f32;
        let color_rate = color_close as f32 / covered as f32;
        assert!(rate > 0.95, "glyph language diverges: match rate {rate:.3}");
        assert!(
            color_rate > 0.98,
            "color language diverges: match rate {color_rate:.3}"
        );
        assert!(cpu.cells.len() > 300, "cpu path produced too few cells");
    }
}
