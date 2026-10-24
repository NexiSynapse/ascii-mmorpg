//! GPU-only floating-point emission and bounded, separable bloom.
//!
//! `prepare` uploads descriptions, never CPU light samples. `encode` overwrites
//! three RGBA32Float textures (emission -> horizontal -> final), including on
//! empty frames. The fragment binding exposes the final **unfilterable** texture
//! at binding 0; the presenter must use `textureLoad`/manual bilinear sampling.
//! A world position maps to normalized field UV as `(position + 0.5) / grid`.
//!
//! The field covers the entire authored domain, at twice the grid resolution
//! capped independently to 512 per axis. It is independent of window size. Each
//! frame uses at most 128 effective sources and two five-tap binomial kernels.
//! Input inspection has a separate 16,384-description budget: ignored sources
//! do not consume the effective source cap, but cannot cause an unbounded scan.

use crate::emission::{EmissionPosition, EmissionSource, MAX_SOURCE_COUNT};
use std::mem::size_of;

const MAX_DIMENSION: u32 = 512;
const MAX_SOURCE_SCAN: usize = 16_384;
const WORKGROUP_SIZE: u32 = 8;
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba32Float;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Config {
    grid: [f32; 2],
    source_count: u32,
    _pad: u32,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Source {
    // Integer coordinates are legacy cell centres, not cell corners.
    position_radius_strength: [f32; 4],
    color: [f32; 4],
}

struct Targets {
    dimensions: [u32; 2],
    // Retain all texture handles; only tests ever copy them back to the CPU.
    _textures: [wgpu::Texture; 3],
    emission_bind: wgpu::BindGroup,
    horizontal_bind: wgpu::BindGroup,
    vertical_bind: wgpu::BindGroup,
    composite_bind: wgpu::BindGroup,
}

/// One presenter-owned bloom field. Call `prepare`, then `encode`, before the
/// render pass that uses `bind_group`. Resizing replaces the bind group, so
/// acquire it after preparation. Submit an encoded frame before preparing the
/// next one: queue writes share the source/uniform buffers between frames.
pub(super) struct GpuBloom {
    composite_layout: wgpu::BindGroupLayout,
    emission_layout: wgpu::BindGroupLayout,
    blur_layout: wgpu::BindGroupLayout,
    emission_pipeline: wgpu::ComputePipeline,
    horizontal_pipeline: wgpu::ComputePipeline,
    vertical_pipeline: wgpu::ComputePipeline,
    config: wgpu::Buffer,
    sources: wgpu::Buffer,
    targets: Targets,
}

impl GpuBloom {
    pub(super) fn new(device: &wgpu::Device) -> Self {
        let composite_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("bloom-composite-layout"),
            entries: &[texture_entry(0, wgpu::ShaderStages::FRAGMENT)],
        });
        let emission_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("bloom-emission-layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(size_of::<Config>() as u64),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(size_of::<Source>() as u64),
                    },
                    count: None,
                },
                storage_entry(2),
            ],
        });
        let blur_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("bloom-blur-layout"),
            entries: &[
                texture_entry(0, wgpu::ShaderStages::COMPUTE),
                storage_entry(1),
            ],
        });
        let emission_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("bloom-emission-shader"),
            source: wgpu::ShaderSource::Wgsl(EMISSION_SHADER.into()),
        });
        let blur_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("bloom-blur-shader"),
            source: wgpu::ShaderSource::Wgsl(BLUR_SHADER.into()),
        });
        let emission_pipeline = pipeline(device, &emission_layout, &emission_shader, "emission");
        let horizontal_pipeline = pipeline(device, &blur_layout, &blur_shader, "horizontal");
        let vertical_pipeline = pipeline(device, &blur_layout, &blur_shader, "vertical");
        // WebGPU zero initialization makes encode-before-prepare an empty frame.
        let config = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("bloom-config"),
            size: size_of::<Config>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let sources = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("bloom-sources"),
            size: (size_of::<Source>() * MAX_SOURCE_COUNT) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let targets = Targets::new(
            device,
            [1, 1],
            &emission_layout,
            &blur_layout,
            &composite_layout,
            &config,
            &sources,
        );
        Self {
            composite_layout,
            emission_layout,
            blur_layout,
            emission_pipeline,
            horizontal_pipeline,
            vertical_pipeline,
            config,
            sources,
            targets,
        }
    }

    pub(super) fn layout(&self) -> &wgpu::BindGroupLayout {
        &self.composite_layout
    }

    pub(super) fn bind_group(&self) -> &wgpu::BindGroup {
        &self.targets.composite_bind
    }

    /// Sanitize even publicly mutated fields. Inactive, black, non-world and
    /// wholly offscreen sources are rejected before the 128-effective-source
    /// cap. Inspection stops after 16,384 input descriptions or 128 accepted
    /// sources, whichever comes first; no source-sized allocation is made.
    pub(super) fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        grid: [u32; 2],
        sources: &[EmissionSource],
    ) {
        let dimensions = field_dimensions(grid);
        if dimensions != self.targets.dimensions {
            self.targets = Targets::new(
                device,
                dimensions,
                &self.emission_layout,
                &self.blur_layout,
                &self.composite_layout,
                &self.config,
                &self.sources,
            );
        }
        let (upload, count) = pack_sources(grid, dimensions, sources);
        let config = Config {
            grid: [grid[0] as f32, grid[1] as f32],
            source_count: count as u32,
            _pad: 0,
        };
        queue.write_buffer(&self.config, 0, bytemuck::bytes_of(&config));
        if count > 0 {
            queue.write_buffer(&self.sources, 0, bytemuck::cast_slice(&upload[..count]));
        }
    }

    pub(super) fn encode(&self, encoder: &mut wgpu::CommandEncoder) {
        let [width, height] = self.targets.dimensions;
        // Separate passes give wgpu explicit storage-write -> texture-read
        // transitions; input/output never alias within a dispatch.
        for (pipeline, bind, label) in [
            (
                &self.emission_pipeline,
                &self.targets.emission_bind,
                "bloom-emission",
            ),
            (
                &self.horizontal_pipeline,
                &self.targets.horizontal_bind,
                "bloom-horizontal",
            ),
            (
                &self.vertical_pipeline,
                &self.targets.vertical_bind,
                "bloom-vertical",
            ),
        ] {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some(label),
                timestamp_writes: None,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, bind, &[]);
            pass.dispatch_workgroups(
                (width + WORKGROUP_SIZE - 1) / WORKGROUP_SIZE,
                (height + WORKGROUP_SIZE - 1) / WORKGROUP_SIZE,
                1,
            );
        }
    }

    #[cfg(test)]
    pub(super) fn emission_texture(&self) -> &wgpu::Texture {
        &self.targets._textures[0]
    }

    #[cfg(test)]
    pub(super) fn final_texture(&self) -> &wgpu::Texture {
        &self.targets._textures[2]
    }
}

impl Targets {
    fn new(
        device: &wgpu::Device,
        dimensions: [u32; 2],
        emission_layout: &wgpu::BindGroupLayout,
        blur_layout: &wgpu::BindGroupLayout,
        composite_layout: &wgpu::BindGroupLayout,
        config: &wgpu::Buffer,
        sources: &wgpu::Buffer,
    ) -> Self {
        let textures = ["bloom-emission", "bloom-horizontal", "bloom-final"].map(|label| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: dimensions[0],
                    height: dimensions[1],
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: FORMAT,
                usage: wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::STORAGE_BINDING
                    | if cfg!(test) {
                        wgpu::TextureUsages::COPY_SRC
                    } else {
                        wgpu::TextureUsages::empty()
                    },
                view_formats: &[],
            })
        });
        let views: [_; 3] = std::array::from_fn(|index| {
            textures[index].create_view(&wgpu::TextureViewDescriptor::default())
        });
        let emission_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bloom-emission-bind"),
            layout: emission_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: config.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: sources.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&views[0]),
                },
            ],
        });
        let blur_bind = |input: usize, output: usize, label| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(label),
                layout: blur_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&views[input]),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&views[output]),
                    },
                ],
            })
        };
        let horizontal_bind = blur_bind(0, 1, "bloom-horizontal-bind");
        let vertical_bind = blur_bind(1, 2, "bloom-vertical-bind");
        let composite_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bloom-composite-bind"),
            layout: composite_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&views[2]),
            }],
        });
        Self {
            dimensions,
            _textures: textures,
            emission_bind,
            horizontal_bind,
            vertical_bind,
            composite_bind,
        }
    }
}

fn field_dimensions(grid: [u32; 2]) -> [u32; 2] {
    grid.map(|axis| axis.saturating_mul(2).clamp(1, MAX_DIMENSION))
}

fn pack_sources(
    grid: [u32; 2],
    dimensions: [u32; 2],
    sources: &[EmissionSource],
) -> ([Source; MAX_SOURCE_COUNT], usize) {
    let mut upload = [Source {
        position_radius_strength: [0.0; 4],
        color: [0.0; 4],
    }; MAX_SOURCE_COUNT];
    let mut count = 0;
    if grid.contains(&0) {
        return (upload, count);
    }
    let grid = [grid[0] as f32, grid[1] as f32];
    for source in sources.iter().take(MAX_SOURCE_SCAN) {
        if !source.world || (source.color.r == 0 && source.color.g == 0 && source.color.b == 0) {
            continue;
        }
        // Public fields can bypass the constructors, so reapply the public
        // seam's finite-coordinate/radius/strength policy at this boundary.
        let source = EmissionSource::new(
            source.position,
            source.color,
            source.strength,
            source.radius,
            source.world,
        );
        if source.strength <= 0.0 {
            continue;
        }
        let position = match source.position {
            EmissionPosition::Grid { x, y } => [x as f32, y as f32],
            EmissionPosition::Continuous(point) => [point.x, point.y],
        };
        if source.radius == 0.0 {
            // A point outside the half-open authored domain must not get
            // clamped onto the edge and consume an effective source slot.
            if (0..2).any(|axis| position[axis] < -0.5 || position[axis] >= grid[axis] - 0.5) {
                continue;
            }
        } else {
            // The nearest field sample is the closest point of the *discrete*
            // grid. If it lies outside the radial support, every texel is zero.
            // This also rejects circles that only overlap a viewport corner's
            // bounding box, or miss every sample at heavily reduced resolution.
            let mut delta = [0.0; 2];
            for axis in 0..2 {
                let pixels = dimensions[axis] as f32;
                let nearest = ((position[axis] + 0.5) / grid[axis] * pixels)
                    .floor()
                    .clamp(0.0, pixels - 1.0);
                let sample = (nearest + 0.5) / pixels * grid[axis] - 0.5;
                delta[axis] = sample - position[axis];
            }
            if delta[0].hypot(delta[1]) >= source.radius {
                continue;
            }
        }
        upload[count] = Source {
            position_radius_strength: [position[0], position[1], source.radius, source.strength],
            color: [
                f32::from(source.color.r) / 255.0,
                f32::from(source.color.g) / 255.0,
                f32::from(source.color.b) / 255.0,
                0.0,
            ],
        };
        count += 1;
        if count == MAX_SOURCE_COUNT {
            break;
        }
    }
    (upload, count)
}

fn texture_entry(binding: u32, visibility: wgpu::ShaderStages) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: false },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

fn storage_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::StorageTexture {
            access: wgpu::StorageTextureAccess::WriteOnly,
            format: FORMAT,
            view_dimension: wgpu::TextureViewDimension::D2,
        },
        count: None,
    }
}

fn pipeline(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    shader: &wgpu::ShaderModule,
    entry_point: &str,
) -> wgpu::ComputePipeline {
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(entry_point),
        bind_group_layouts: &[layout],
        push_constant_ranges: &[],
    });
    device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some(entry_point),
        layout: Some(&layout),
        module: shader,
        entry_point: Some(entry_point),
        compilation_options: wgpu::PipelineCompilationOptions::default(),
        cache: None,
    })
}

const EMISSION_SHADER: &str = r#"
struct Config {
    grid: vec2<f32>,
    source_count: u32,
    _pad: u32,
};
struct Source {
    position_radius_strength: vec4<f32>,
    color: vec4<f32>,
};
@group(0) @binding(0) var<uniform> config: Config;
@group(0) @binding(1) var<storage, read> sources: array<Source>;
@group(0) @binding(2) var field_out: texture_storage_2d<rgba32float, write>;

@compute @workgroup_size(8, 8)
fn emission(@builtin(global_invocation_id) id: vec3<u32>) {
    let dimensions = textureDimensions(field_out);
    if any(id.xy >= dimensions) { return; }
    let point = (vec2<f32>(id.xy) + vec2<f32>(0.5)) / vec2<f32>(dimensions)
        * config.grid - vec2<f32>(0.5);
    var accumulated = vec3<f32>(0.0);
    for (var i = 0u; i < min(config.source_count, 128u); i += 1u) {
        let source = sources[i];
        let position = source.position_radius_strength.xy;
        let radius = source.position_radius_strength.z;
        let strength = source.position_radius_strength.w;
        var falloff = 0.0;
        if radius == 0.0 {
            let nearest = clamp(
                vec2<i32>(floor((position + vec2<f32>(0.5)) / config.grid * vec2<f32>(dimensions))),
                vec2<i32>(0), vec2<i32>(dimensions) - vec2<i32>(1));
            if all(vec2<i32>(id.xy) == nearest) { falloff = 1.0; }
        } else {
            let radial = max(1.0 - distance(point, position) / radius, 0.0);
            falloff = radial * radial;
        }
        accumulated += source.color.rgb * (strength * falloff);
    }
    // All channels remain floating point; no saturation/tone map/quantization.
    textureStore(field_out, vec2<i32>(id.xy), vec4<f32>(accumulated, 0.0));
}
"#;

const BLUR_SHADER: &str = r#"
@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var field_out: texture_storage_2d<rgba32float, write>;

fn blur(pixel: vec2<i32>, axis: vec2<i32>) {
    let dimensions = vec2<i32>(textureDimensions(source));
    // A normalized binomial kernel approximates a sigma=1 Gaussian. The two
    // passes have fixed 5x5 support regardless of authored radius/window size.
    // Zero extension lets light leave the field without bright edge smearing.
    var value = vec3<f32>(0.0);
    for (var tap = -2; tap <= 2; tap += 1) {
        let sample = pixel + axis * tap;
        if all(sample >= vec2<i32>(0)) && all(sample < dimensions) {
            var weight = 1.0;
            if abs(tap) == 1 { weight = 4.0; }
            if tap == 0 { weight = 6.0; }
            value += textureLoad(source, sample, 0).rgb * weight;
        }
    }
    textureStore(field_out, pixel, vec4<f32>(value / 16.0, 0.0));
}

@compute @workgroup_size(8, 8)
fn horizontal(@builtin(global_invocation_id) id: vec3<u32>) {
    if any(id.xy >= textureDimensions(field_out)) { return; }
    blur(vec2<i32>(id.xy), vec2<i32>(1, 0));
}

@compute @workgroup_size(8, 8)
fn vertical(@builtin(global_invocation_id) id: vec3<u32>) {
    if any(id.xy >= textureDimensions(field_out)) { return; }
    blur(vec2<i32>(id.xy), vec2<i32>(0, 1));
}
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Point2, Rgb};

    fn red(x: i32, y: i32, strength: f32, radius: f32) -> EmissionSource {
        EmissionSource::grid(x, y, Rgb::new(255, 0, 0), strength, radius, true)
    }

    #[test]
    fn field_dimensions_are_nonzero_bounded_and_overflow_safe() {
        assert_eq!(field_dimensions([0, 0]), [1, 1]);
        assert_eq!(field_dimensions([160, 56]), [320, 112]);
        assert_eq!(field_dimensions([1024, 12]), [512, 24]);
        assert_eq!(field_dimensions([u32::MAX, u32::MAX]), [512, 512]);
    }

    #[test]
    fn inactive_and_offscreen_descriptions_do_not_consume_the_effective_cap() {
        let mut sources = Vec::new();
        for _ in 0..MAX_SOURCE_COUNT {
            sources.extend([
                red(0, 0, 1.0, 0.0).with_world(false),
                red(0, 0, 0.0, 0.0),
                EmissionSource::grid(0, 0, Rgb::new(0, 0, 0), 1.0, 1.0, true),
                red(-20, -20, 1.0, 1.0),
                red(-1, -1, 1.0, 0.0),
                // Bounding boxes overlap, but the circle misses the field.
                red(-1, -1, 1.0, 0.9),
            ]);
        }
        sources.extend(std::iter::repeat(red(2, 2, 2.0, 3.0)).take(MAX_SOURCE_COUNT + 1));
        let (upload, count) = pack_sources([8, 8], [16, 16], &sources);
        assert_eq!(count, MAX_SOURCE_COUNT);
        assert!(upload
            .iter()
            .all(|source| source.position_radius_strength == [2.0, 2.0, 3.0, 2.0]));
    }

    #[test]
    fn mutated_fields_are_sanitized_and_input_inspection_is_bounded() {
        let mut source = red(0, 0, 1.0, 1.0);
        source.position = EmissionPosition::Continuous(Point2::new(f32::NAN, f32::INFINITY));
        source.strength = f32::MAX;
        source.radius = f32::MAX;
        let (upload, count) = pack_sources([8, 8], [16, 16], &[source]);
        assert_eq!(count, 1);
        assert_eq!(upload[0].position_radius_strength, [0.0, 0.0, 32.0, 64.0]);
        source.strength = f32::NAN;
        assert_eq!(pack_sources([8, 8], [16, 16], &[source]).1, 0);
        source.strength = 1.0;
        source.radius = f32::NEG_INFINITY;
        let (upload, count) = pack_sources([8, 8], [16, 16], &[source]);
        assert_eq!(count, 1);
        assert_eq!(upload[0].position_radius_strength[2], 0.0);
        assert_eq!(pack_sources([0, 8], [1, 16], &[source]).1, 0);

        let mut descriptions = vec![source.with_world(false); MAX_SOURCE_SCAN];
        descriptions.push(source);
        assert_eq!(pack_sources([8, 8], [16, 16], &descriptions).1, 0);
    }

    fn device() -> Option<(wgpu::Device, wgpu::Queue)> {
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
            eprintln!("bloom readback test: no Vulkan adapter; skipping GPU assertions");
            return None;
        };
        eprintln!("bloom readback adapter: {:?}", adapter.get_info());
        Some(
            pollster::block_on(adapter.request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("bloom-readback-test"),
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::default(),
                    memory_hints: wgpu::MemoryHints::Performance,
                },
                None,
            ))
            .expect("bloom test device"),
        )
    }

    fn capture(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bloom: &mut GpuBloom,
        grid: [u32; 2],
        sources: &[EmissionSource],
    ) -> (Vec<[f32; 4]>, Vec<[f32; 4]>) {
        bloom.prepare(device, queue, grid, sources);
        // Exercise the integration API as well as the readback seam.
        let _ = bloom.layout();
        let _ = bloom.bind_group();
        let [width, height] = bloom.targets.dimensions;
        let alignment = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let pitch = (width * 16 + alignment - 1) / alignment * alignment;
        let stride = u64::from(pitch) * u64::from(height);
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("bloom-readback"),
            size: stride * 2,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("bloom-readback-encoder"),
        });
        bloom.encode(&mut encoder);
        for (index, texture) in [bloom.emission_texture(), bloom.final_texture()]
            .into_iter()
            .enumerate()
        {
            encoder.copy_texture_to_buffer(
                wgpu::TexelCopyTextureInfo {
                    texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyBufferInfo {
                    buffer: &readback,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: index as u64 * stride,
                        bytes_per_row: Some(pitch),
                        rows_per_image: Some(height),
                    },
                },
                wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
            );
        }
        queue.submit(Some(encoder.finish()));
        let (sender, receiver) = std::sync::mpsc::channel();
        readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                sender.send(result).expect("readback receiver");
            });
        let _ = device.poll(wgpu::Maintain::Wait);
        receiver
            .recv()
            .expect("readback callback")
            .expect("map readback");
        let bytes = readback.slice(..).get_mapped_range();
        let mut fields = [Vec::new(), Vec::new()];
        for (index, field) in fields.iter_mut().enumerate() {
            for y in 0..height as usize {
                let start = index * stride as usize + y * pitch as usize;
                for pixel in bytes[start..start + width as usize * 16].chunks_exact(16) {
                    field.push(std::array::from_fn(|channel| {
                        f32::from_le_bytes(pixel[channel * 4..channel * 4 + 4].try_into().unwrap())
                    }));
                }
            }
        }
        drop(bytes);
        readback.unmap();
        let [emission, final_field] = fields;
        (emission, final_field)
    }

    fn close(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() < 0.0001,
            "expected {expected}, got {actual}"
        );
    }

    #[test]
    fn gpu_float_accumulation_blur_and_empty_frame_clear() {
        let Some((device, queue)) = device() else {
            return;
        };
        let mut bloom = GpuBloom::new(&device);
        let sources = [
            red(2, 2, 8.0, 0.0),
            red(2, 2, 8.0, 0.0),
            EmissionSource::grid(2, 2, Rgb::new(0, 255, 0), 4.0, 0.0, true),
        ];
        let (emission, blurred) = capture(&device, &queue, &mut bloom, [8, 8], &sources);
        let centre = 5 * 16 + 5;
        close(emission[centre][0], 16.0);
        close(emission[centre][1], 4.0);
        assert_eq!(emission.iter().filter(|pixel| pixel[0] > 0.0).count(), 1);
        // The actual GPU convolution is the separable outer product of
        // [1,4,6,4,1]/16, not a CPU approximation or a shader-string test.
        close(blurred[centre][0], 16.0 * 6.0 * 6.0 / 256.0);
        close(blurred[centre + 1][0], 16.0 * 4.0 * 6.0 / 256.0);
        close(blurred[centre + 16][0], blurred[centre + 1][0]);
        close(blurred[centre + 2 * 16 + 2][0], 16.0 / 256.0);
        close(blurred[centre + 3][0], 0.0);
        close(blurred.iter().map(|pixel| pixel[0]).sum(), 16.0);
        assert!(blurred
            .iter()
            .all(|pixel| pixel[2] == 0.0 && pixel[3] == 0.0));

        let inactive = [sources[0].with_world(false), red(-30, 0, 1.0, 1.0)];
        for sources in [&inactive[..], &[][..]] {
            let (emission, blurred) = capture(&device, &queue, &mut bloom, [8, 8], sources);
            assert!(emission
                .iter()
                .chain(&blurred)
                .all(|pixel| *pixel == [0.0; 4]));
        }
        let (emission, blurred) = capture(&device, &queue, &mut bloom, [0, 0], &sources);
        assert_eq!(emission, [[0.0; 4]]);
        assert_eq!(blurred, [[0.0; 4]]);
    }

    #[test]
    fn gpu_analytic_falloff_fractional_motion_and_full_domain_mapping() {
        let Some((device, queue)) = device() else {
            return;
        };
        let mut bloom = GpuBloom::new(&device);
        let mut source = EmissionSource::continuous(
            Point2::new(3.25, 3.25),
            Rgb::new(255, 64, 0),
            1.0,
            4.0,
            true,
        );
        let (emission, blurred) = capture(&device, &queue, &mut bloom, [8, 8], &[source]);
        let centre = 7 * 16 + 7;
        close(emission[centre][0], 1.0);
        close(emission[centre + 2][0], 0.75 * 0.75);
        close(emission[centre + 8][0], 0.0);
        close(emission[centre][1], 64.0 / 255.0);
        assert!(blurred[centre + 1][0] < blurred[centre][0]);
        source.position = EmissionPosition::Continuous(Point2::new(3.5, 3.25));
        let (moved, _) = capture(&device, &queue, &mut bloom, [8, 8], &[source]);
        assert!(moved[centre][0] < emission[centre][0]);
        assert!(moved[centre + 1][0] > emission[centre + 1][0]);
        close(moved[centre][0], moved[centre + 1][0]);

        // Beyond the cap, the right-hand authored domain remains represented.
        let source = red(900, 3, 1.0, 3.0);
        let (emission, blurred) = capture(&device, &queue, &mut bloom, [1024, 8], &[source]);
        assert_eq!(bloom.targets.dimensions, [512, 16]);
        let sample = 7 * 512 + 450;
        assert!(emission[sample][0] > 0.0 && blurred[sample][0] > 0.0);
        assert_eq!(emission[7 * 512 + 200][0], 0.0);

        // The allocation stays capped, but uniforms must follow a new domain.
        let (emission, _) = capture(&device, &queue, &mut bloom, [2048, 8], &[source]);
        assert!(emission[7 * 512 + 225][0] > 0.0);
        assert_eq!(emission[sample][0], 0.0);
        assert!(emission.iter().flatten().all(|value| value.is_finite()));
    }
}
