//! Offscreen WGPU 3D rasterization and ASCII compute post-processing pipeline
//! with multi-buffered asynchronous staging rings and hardware instanced batching.

use super::context::GpuContext;
use super::mesh::{InstanceRaw, MeshBuffer, SceneBatches, Vertex};
use super::shaders::{ASCII_POST_WGSL, SCENE_3D_WGSL};
use super::uniforms::{CameraUniforms, LightingUniforms};

pub const RING_SIZE: usize = 2;
pub const SSAA_FACTOR: u32 = 4;

/// Adaptive SSAA: full 4x quality on small grids, 2x once the grid exceeds
/// the 4x sweet spot. Halving SSAA quarters offscreen texels.
pub fn adaptive_ssaa(width: u32, height: u32) -> u32 {
    if width.saturating_mul(height) <= 4096 {
        4
    } else {
        2
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct GpuCell {
    pub glyph: u32,
    pub fg_r: u32,
    pub fg_g: u32,
    pub fg_b: u32,
    pub bg_r: u32,
    pub bg_g: u32,
    pub bg_b: u32,
    pub depth_bits: u32, // Encodes IEEE-754 f32 depth from GPU Depth32Float for O(1) unprojection
}

impl GpuCell {
    /// Returns the floating-point depth value (0.0 = camera near, 1.0 = camera far).
    pub fn depth(&self) -> f32 {
        f32::from_bits(self.depth_bits)
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct PostConfigUniform {
    width: u32,
    height: u32,
    time: f32,
    contrast_boost: f32,
}

pub struct PrototypeMesh {
    pub vertex_buffer: wgpu::Buffer,
    pub index_buffer: wgpu::Buffer,
    pub index_count: u32,
}

impl PrototypeMesh {
    pub fn from_mesh(ctx: &GpuContext, mesh: &MeshBuffer, label: &str) -> Self {
        let vertex_buffer = ctx.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(&format!("{label} Vertex Buffer")),
            size: (mesh.vertices.len() * std::mem::size_of::<Vertex>()).max(64) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let index_buffer = ctx.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(&format!("{label} Index Buffer")),
            size: (mesh.indices.len() * std::mem::size_of::<u32>()).max(64) as u64,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        if !mesh.vertices.is_empty() {
            ctx.queue
                .write_buffer(&vertex_buffer, 0, bytemuck::cast_slice(&mesh.vertices));
        }
        if !mesh.indices.is_empty() {
            ctx.queue
                .write_buffer(&index_buffer, 0, bytemuck::cast_slice(&mesh.indices));
        }

        Self {
            vertex_buffer,
            index_buffer,
            index_count: mesh.indices.len() as u32,
        }
    }
}

pub struct FrameSlot {
    pub camera_buffer: wgpu::Buffer,
    pub lighting_buffer: wgpu::Buffer,
    pub config_buffer: wgpu::Buffer,
    pub scene_bind_group: wgpu::BindGroup,
    pub cell_output_buffer: wgpu::Buffer,
    pub staging_buffer: wgpu::Buffer,
    pub ascii_bind_group: wgpu::BindGroup,
    pub receiver: Option<std::sync::mpsc::Receiver<Result<(), wgpu::BufferAsyncError>>>,
    pub is_mapped: bool,
    pub in_flight: bool,
}

impl FrameSlot {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        device: &wgpu::Device,
        scene_bind_group_layout: &wgpu::BindGroupLayout,
        ascii_bind_group_layout: &wgpu::BindGroupLayout,
        color_view: &wgpu::TextureView,
        normal_depth_view: &wgpu::TextureView,
        metadata_view: &wgpu::TextureView,
        width: u32,
        height: u32,
        slot_idx: usize,
    ) -> Self {
        let camera_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(&format!("Camera Uniform Buffer (Slot {slot_idx})")),
            size: std::mem::size_of::<CameraUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let lighting_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(&format!("Lighting Uniform Buffer (Slot {slot_idx})")),
            size: std::mem::size_of::<LightingUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let config_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(&format!("Post Config Uniform Buffer (Slot {slot_idx})")),
            size: std::mem::size_of::<PostConfigUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let scene_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(&format!("Scene Bind Group (Slot {slot_idx})")),
            layout: scene_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: camera_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: lighting_buffer.as_entire_binding(),
                },
            ],
        });

        let cell_count = (width * height) as u64;
        let cell_buffer_size = cell_count * std::mem::size_of::<GpuCell>() as u64;

        let cell_output_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(&format!("Cell Output Buffer (Slot {slot_idx})")),
            size: cell_buffer_size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });

        let staging_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(&format!("Staging Readback Buffer (Slot {slot_idx})")),
            size: cell_buffer_size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let ascii_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(&format!("ASCII Bind Group (Slot {slot_idx})")),
            layout: ascii_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(color_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(normal_depth_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(metadata_view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: cell_output_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: config_buffer.as_entire_binding(),
                },
            ],
        });

        Self {
            camera_buffer,
            lighting_buffer,
            config_buffer,
            scene_bind_group,
            cell_output_buffer,
            staging_buffer,
            ascii_bind_group,
            receiver: None,
            is_mapped: false,
            in_flight: false,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn resize(
        &mut self,
        device: &wgpu::Device,
        ascii_bind_group_layout: &wgpu::BindGroupLayout,
        color_view: &wgpu::TextureView,
        normal_depth_view: &wgpu::TextureView,
        metadata_view: &wgpu::TextureView,
        width: u32,
        height: u32,
        slot_idx: usize,
    ) {
        let cell_count = (width * height) as u64;
        let cell_buffer_size = cell_count * std::mem::size_of::<GpuCell>() as u64;

        self.cell_output_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(&format!("Cell Output Buffer (Resized Slot {slot_idx})")),
            size: cell_buffer_size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });

        self.staging_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(&format!(
                "Staging Readback Buffer (Resized Slot {slot_idx})"
            )),
            size: cell_buffer_size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        self.ascii_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(&format!("ASCII Bind Group (Resized Slot {slot_idx})")),
            layout: ascii_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(color_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(normal_depth_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(metadata_view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: self.cell_output_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: self.config_buffer.as_entire_binding(),
                },
            ],
        });
    }
}

pub struct GpuPipeline {
    pub width: u32,
    pub height: u32,
    /// Active SSAA factor for the current grid (4 or 2, see `adaptive_ssaa`).
    pub ssaa: u32,

    // 3D Scene Pipeline
    pub scene_pipeline: wgpu::RenderPipeline,
    pub scene_bind_group_layout: wgpu::BindGroupLayout,

    // ASCII Compute Pipeline
    pub ascii_pipeline: wgpu::ComputePipeline,
    pub ascii_bind_group_layout: wgpu::BindGroupLayout,

    // Textures
    pub color_texture: wgpu::Texture,
    pub color_view: wgpu::TextureView,
    pub normal_depth_texture: wgpu::Texture,
    pub normal_depth_view: wgpu::TextureView,
    pub metadata_texture: wgpu::Texture,
    pub metadata_view: wgpu::TextureView,
    pub depth_texture: wgpu::Texture,
    pub depth_view: wgpu::TextureView,

    // Multi-Buffered Staging Ring
    pub slots: [FrameSlot; RING_SIZE],
    pub current_slot: usize,

    // Geometry buffers
    pub vertex_buffer: wgpu::Buffer,
    pub index_buffer: wgpu::Buffer,
    pub vertex_capacity: usize,
    pub index_capacity: usize,

    // Hardware instanced batching buffers
    pub instance_buffer: wgpu::Buffer,
    pub instance_capacity: usize,
    pub default_instance_buffer: wgpu::Buffer,

    // Pre-baked prototype meshes for GPU hardware instancing
    pub pine_tree_proto: PrototypeMesh,
    pub weather_particle_proto: PrototypeMesh,
    pub ruin_pillar_proto: PrototypeMesh,
}

impl GpuPipeline {
    pub fn new(ctx: &GpuContext, width: u32, height: u32) -> Result<Self, String> {
        let width = width.max(16);
        let height = height.max(8);
        let ssaa = adaptive_ssaa(width, height);
        let device = &ctx.device;

        // 1. Compile Shaders
        let scene_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Scene 3D Shader"),
            source: wgpu::ShaderSource::Wgsl(SCENE_3D_WGSL.into()),
        });

        let ascii_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("ASCII Post Shader"),
            source: wgpu::ShaderSource::Wgsl(ASCII_POST_WGSL.into()),
        });

        // 2. Bind Group Layouts
        let scene_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Scene Bind Group Layout"),
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
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                ],
            });

        let ascii_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("ASCII Bind Group Layout"),
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
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: false },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: false },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 3,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: false },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 4,
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

        // 3. Render Pipeline Layouts & Pipelines
        let scene_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Scene Pipeline Layout"),
                bind_group_layouts: &[&scene_bind_group_layout],
                push_constant_ranges: &[],
            });

        let vertex_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
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
                    format: wgpu::VertexFormat::Float32x4,
                    offset: 24,
                    shader_location: 2,
                },
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32,
                    offset: 40,
                    shader_location: 3,
                },
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32,
                    offset: 44,
                    shader_location: 4,
                },
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32,
                    offset: 48,
                    shader_location: 5,
                },
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32,
                    offset: 52,
                    shader_location: 6,
                },
            ],
        };

        let instance_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<InstanceRaw>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &[
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x4,
                    offset: 0,
                    shader_location: 7,
                },
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x4,
                    offset: 16,
                    shader_location: 8,
                },
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x4,
                    offset: 32,
                    shader_location: 9,
                },
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x4,
                    offset: 48,
                    shader_location: 10,
                },
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x4,
                    offset: 64,
                    shader_location: 11,
                },
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x4,
                    offset: 80,
                    shader_location: 12,
                },
            ],
        };

        let scene_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Scene 3D Pipeline"),
            layout: Some(&scene_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &scene_shader,
                entry_point: Some("vs_main"),
                buffers: &[vertex_layout, instance_layout],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &scene_shader,
                entry_point: Some("fs_main"),
                targets: &[
                    Some(wgpu::ColorTargetState {
                        format: wgpu::TextureFormat::Rgba8UnormSrgb,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    }),
                    Some(wgpu::ColorTargetState {
                        // Device depth clusters near 1 under perspective;
                        // an 8-bit channel collapses separate surfaces.
                        format: wgpu::TextureFormat::Rgba32Float,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    }),
                    Some(wgpu::ColorTargetState {
                        format: wgpu::TextureFormat::Rgba8Unorm,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    }),
                ],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let ascii_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("ASCII Compute Pipeline Layout"),
                bind_group_layouts: &[&ascii_bind_group_layout],
                push_constant_ranges: &[],
            });

        let ascii_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("ASCII Compute Pipeline"),
            layout: Some(&ascii_pipeline_layout),
            module: &ascii_shader,
            entry_point: Some("cs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            cache: None,
        });

        // 4. Create Textures at Super-Sampled Resolution for shape-aware cells.
        let render_width = width * ssaa;
        let render_height = height * ssaa;
        let (color_texture, color_view) = create_target_texture(
            device,
            render_width,
            render_height,
            wgpu::TextureFormat::Rgba8UnormSrgb,
            "Color",
        );
        let (normal_depth_texture, normal_depth_view) = create_target_texture(
            device,
            render_width,
            render_height,
            wgpu::TextureFormat::Rgba32Float,
            "NormalDepth",
        );
        let (metadata_texture, metadata_view) = create_target_texture(
            device,
            render_width,
            render_height,
            wgpu::TextureFormat::Rgba8Unorm,
            "Metadata",
        );
        let (depth_texture, depth_view) = create_depth_texture(device, render_width, render_height);

        // 5. Create Multi-Buffered Frame Slots
        let slot0 = FrameSlot::new(
            device,
            &scene_bind_group_layout,
            &ascii_bind_group_layout,
            &color_view,
            &normal_depth_view,
            &metadata_view,
            width,
            height,
            0,
        );
        let slot1 = FrameSlot::new(
            device,
            &scene_bind_group_layout,
            &ascii_bind_group_layout,
            &color_view,
            &normal_depth_view,
            &metadata_view,
            width,
            height,
            1,
        );
        let slots = [slot0, slot1];

        // 6. Geometry & Instancing Buffers
        let vertex_capacity = 64_000;
        let index_capacity = 128_000;
        let instance_capacity = 2_048;

        let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Vertex Buffer"),
            size: (vertex_capacity * std::mem::size_of::<Vertex>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Index Buffer"),
            size: (index_capacity * std::mem::size_of::<u32>()) as u64,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Instance Buffer"),
            size: (instance_capacity * std::mem::size_of::<InstanceRaw>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let default_instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Default Instance Buffer"),
            size: std::mem::size_of::<InstanceRaw>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        ctx.queue.write_buffer(
            &default_instance_buffer,
            0,
            bytemuck::bytes_of(&InstanceRaw::IDENTITY),
        );

        // 7. Pre-baked Prototypes for GPU Hardware Instancing
        let pine_tree_proto = PrototypeMesh::from_mesh(
            ctx,
            &MeshBuffer::create_pine_tree_prototype(),
            "Pine Tree Prototype",
        );
        let weather_particle_proto = PrototypeMesh::from_mesh(
            ctx,
            &MeshBuffer::create_weather_particle_prototype(),
            "Weather Particle Prototype",
        );
        let ruin_pillar_proto = PrototypeMesh::from_mesh(
            ctx,
            &MeshBuffer::create_ruin_pillar_prototype(),
            "Ruin Pillar Prototype",
        );

        Ok(Self {
            width,
            height,
            ssaa,
            scene_pipeline,
            scene_bind_group_layout,
            ascii_pipeline,
            ascii_bind_group_layout,
            color_texture,
            color_view,
            normal_depth_texture,
            normal_depth_view,
            metadata_texture,
            metadata_view,
            depth_texture,
            depth_view,
            slots,
            current_slot: 0,
            vertex_buffer,
            index_buffer,
            vertex_capacity,
            index_capacity,
            instance_buffer,
            instance_capacity,
            default_instance_buffer,
            pine_tree_proto,
            weather_particle_proto,
            ruin_pillar_proto,
        })
    }

    pub fn resize(&mut self, ctx: &GpuContext, width: u32, height: u32) {
        let width = width.max(16);
        let height = height.max(8);
        let ssaa = adaptive_ssaa(width, height);
        if self.width == width && self.height == height && self.ssaa == ssaa {
            return;
        }

        // Drain any pending in-flight mapped buffers before resizing
        for slot in &mut self.slots {
            if slot.in_flight {
                ctx.device.poll(wgpu::Maintain::Wait);
                if let Some(rx) = slot.receiver.take() {
                    let _ = rx.recv();
                }
                if slot.is_mapped {
                    slot.staging_buffer.unmap();
                    slot.is_mapped = false;
                }
                slot.in_flight = false;
            }
        }

        self.width = width;
        self.height = height;
        self.ssaa = ssaa;
        let device = &ctx.device;

        let render_width = width * ssaa;
        let render_height = height * ssaa;

        let (c_tex, c_view) = create_target_texture(
            device,
            render_width,
            render_height,
            wgpu::TextureFormat::Rgba8UnormSrgb,
            "Color",
        );
        let (nd_tex, nd_view) = create_target_texture(
            device,
            render_width,
            render_height,
            wgpu::TextureFormat::Rgba32Float,
            "NormalDepth",
        );
        let (meta_tex, meta_view) = create_target_texture(
            device,
            render_width,
            render_height,
            wgpu::TextureFormat::Rgba8Unorm,
            "Metadata",
        );
        let (d_tex, d_view) = create_depth_texture(device, render_width, render_height);

        self.color_texture = c_tex;
        self.color_view = c_view;
        self.normal_depth_texture = nd_tex;
        self.normal_depth_view = nd_view;
        self.metadata_texture = meta_tex;
        self.metadata_view = meta_view;
        self.depth_texture = d_tex;
        self.depth_view = d_view;

        for (i, slot) in self.slots.iter_mut().enumerate() {
            slot.resize(
                device,
                &self.ascii_bind_group_layout,
                &self.color_view,
                &self.normal_depth_view,
                &self.metadata_view,
                width,
                height,
                i,
            );
        }
    }

    /// Submits a frame of dynamic geometry, instanced batches, and compute passes into the GPU ring.
    /// Does NOT synchronously wait on the CPU, allowing true pipelined GPU throughput.
    pub fn submit_frame(
        &mut self,
        ctx: &GpuContext,
        mesh: &MeshBuffer,
        batches: &SceneBatches,
        camera: &CameraUniforms,
        lighting: &LightingUniforms,
    ) -> Result<usize, String> {
        let device = &ctx.device;
        let queue = &ctx.queue;
        let slot_idx = self.current_slot;

        // If the slot is currently in-flight from a prior frame, complete its readback cycle
        if self.slots[slot_idx].in_flight {
            ctx.device.poll(wgpu::Maintain::Wait);
            if let Some(rx) = self.slots[slot_idx].receiver.take() {
                let _ = rx.recv();
            }
            if self.slots[slot_idx].is_mapped {
                self.slots[slot_idx].staging_buffer.unmap();
                self.slots[slot_idx].is_mapped = false;
            }
            self.slots[slot_idx].in_flight = false;
        }

        // 1. Ensure dynamic vertex/index capacity
        if mesh.vertices.len() > self.vertex_capacity {
            self.vertex_capacity = (mesh.vertices.len() * 2).max(1024);
            self.vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Reallocated Vertex Buffer"),
                size: (self.vertex_capacity * std::mem::size_of::<Vertex>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        if mesh.indices.len() > self.index_capacity {
            self.index_capacity = (mesh.indices.len() * 2).max(2048);
            self.index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Reallocated Index Buffer"),
                size: (self.index_capacity * std::mem::size_of::<u32>()) as u64,
                usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }

        // 2. Prepare Batched Instance Buffer
        let total_instances = batches.total_instance_count();
        if total_instances > self.instance_capacity {
            self.instance_capacity = (total_instances * 2).max(512);
            self.instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Reallocated Instance Buffer"),
                size: (self.instance_capacity * std::mem::size_of::<InstanceRaw>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }

        let mut packed_instances = Vec::with_capacity(total_instances);
        let tree_start = packed_instances.len();
        packed_instances.extend_from_slice(&batches.tree_instances);
        let tree_end = packed_instances.len();

        let particle_start = packed_instances.len();
        packed_instances.extend_from_slice(&batches.particle_instances);
        let particle_end = packed_instances.len();

        let pillar_start = packed_instances.len();
        packed_instances.extend_from_slice(&batches.pillar_instances);
        let pillar_end = packed_instances.len();

        if !packed_instances.is_empty() {
            queue.write_buffer(
                &self.instance_buffer,
                0,
                bytemuck::cast_slice(&packed_instances),
            );
        }

        // 3. Upload dynamic geometry & uniforms for current slot
        if !mesh.vertices.is_empty() {
            queue.write_buffer(&self.vertex_buffer, 0, bytemuck::cast_slice(&mesh.vertices));
        }
        if !mesh.indices.is_empty() {
            queue.write_buffer(&self.index_buffer, 0, bytemuck::cast_slice(&mesh.indices));
        }

        let slot = &mut self.slots[slot_idx];
        queue.write_buffer(&slot.camera_buffer, 0, bytemuck::bytes_of(camera));
        queue.write_buffer(&slot.lighting_buffer, 0, bytemuck::bytes_of(lighting));

        let config = PostConfigUniform {
            width: self.width,
            height: self.height,
            time: camera.time,
            contrast_boost: 1.15,
        };
        queue.write_buffer(&slot.config_buffer, 0, bytemuck::bytes_of(&config));

        // 4. Record Command Encoder
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Batched Render & ASCII Post Encoder"),
        });

        // Pass 1: 3D Rasterization Pass
        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("3D Batched Scene Pass"),
                color_attachments: &[
                    Some(wgpu::RenderPassColorAttachment {
                        view: &self.color_view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color {
                                r: 0.05,
                                g: 0.06,
                                b: 0.09,
                                a: 1.0,
                            }),
                            store: wgpu::StoreOp::Store,
                        },
                    }),
                    Some(wgpu::RenderPassColorAttachment {
                        view: &self.normal_depth_view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color {
                                r: 0.0,
                                g: 1.0,
                                b: 0.0,
                                a: 1.0,
                            }),
                            store: wgpu::StoreOp::Store,
                        },
                    }),
                    Some(wgpu::RenderPassColorAttachment {
                        view: &self.metadata_view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color {
                                r: 0.0,
                                g: 0.0,
                                b: 0.0,
                                a: 0.0,
                            }),
                            store: wgpu::StoreOp::Store,
                        },
                    }),
                ],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            render_pass.set_pipeline(&self.scene_pipeline);
            render_pass.set_bind_group(0, &slot.scene_bind_group, &[]);

            // Draw dynamic scene geometry (Terrain, Humanoids, NPCs, Altar, Gate)
            if !mesh.indices.is_empty() {
                render_pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
                render_pass.set_vertex_buffer(1, self.default_instance_buffer.slice(..));
                render_pass
                    .set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                render_pass.draw_indexed(0..mesh.indices.len() as u32, 0, 0..1);
            }

            // Draw instanced pine trees (1 GPU draw call for all trees)
            if !batches.tree_instances.is_empty() {
                let start_b = (tree_start * std::mem::size_of::<InstanceRaw>()) as u64;
                let end_b = (tree_end * std::mem::size_of::<InstanceRaw>()) as u64;
                render_pass.set_vertex_buffer(0, self.pine_tree_proto.vertex_buffer.slice(..));
                render_pass.set_vertex_buffer(1, self.instance_buffer.slice(start_b..end_b));
                render_pass.set_index_buffer(
                    self.pine_tree_proto.index_buffer.slice(..),
                    wgpu::IndexFormat::Uint32,
                );
                render_pass.draw_indexed(
                    0..self.pine_tree_proto.index_count,
                    0,
                    0..batches.tree_instances.len() as u32,
                );
            }

            // Draw instanced weather particles (1 GPU draw call for hundreds of particles)
            if !batches.particle_instances.is_empty() {
                let start_b = (particle_start * std::mem::size_of::<InstanceRaw>()) as u64;
                let end_b = (particle_end * std::mem::size_of::<InstanceRaw>()) as u64;
                render_pass
                    .set_vertex_buffer(0, self.weather_particle_proto.vertex_buffer.slice(..));
                render_pass.set_vertex_buffer(1, self.instance_buffer.slice(start_b..end_b));
                render_pass.set_index_buffer(
                    self.weather_particle_proto.index_buffer.slice(..),
                    wgpu::IndexFormat::Uint32,
                );
                render_pass.draw_indexed(
                    0..self.weather_particle_proto.index_count,
                    0,
                    0..batches.particle_instances.len() as u32,
                );
            }

            // Draw instanced ruin pillars (1 GPU draw call)
            if !batches.pillar_instances.is_empty() {
                let start_b = (pillar_start * std::mem::size_of::<InstanceRaw>()) as u64;
                let end_b = (pillar_end * std::mem::size_of::<InstanceRaw>()) as u64;
                render_pass.set_vertex_buffer(0, self.ruin_pillar_proto.vertex_buffer.slice(..));
                render_pass.set_vertex_buffer(1, self.instance_buffer.slice(start_b..end_b));
                render_pass.set_index_buffer(
                    self.ruin_pillar_proto.index_buffer.slice(..),
                    wgpu::IndexFormat::Uint32,
                );
                render_pass.draw_indexed(
                    0..self.ruin_pillar_proto.index_count,
                    0,
                    0..batches.pillar_instances.len() as u32,
                );
            }
        }

        // Pass 2: ASCII Post-Processing Compute Pass
        {
            let mut compute_pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("ASCII Post Compute Pass"),
                timestamp_writes: None,
            });
            compute_pass.set_pipeline(&self.ascii_pipeline);
            compute_pass.set_bind_group(0, &slot.ascii_bind_group, &[]);
            let workgroups_x = (self.width + 15) / 16;
            let workgroups_y = (self.height + 15) / 16;
            compute_pass.dispatch_workgroups(workgroups_x, workgroups_y, 1);
        }

        // Copy Cell Storage Buffer to Staging Buffer for this slot
        let copy_size = (self.width * self.height) as u64 * std::mem::size_of::<GpuCell>() as u64;
        encoder.copy_buffer_to_buffer(
            &slot.cell_output_buffer,
            0,
            &slot.staging_buffer,
            0,
            copy_size,
        );

        queue.submit(Some(encoder.finish()));

        // Asynchronously map staging buffer for non-blocking readback
        let (tx, rx) = std::sync::mpsc::channel();
        slot.staging_buffer
            .slice(..copy_size)
            .map_async(wgpu::MapMode::Read, move |res| {
                let _ = tx.send(res);
            });

        slot.receiver = Some(rx);
        slot.in_flight = true;
        slot.is_mapped = false;

        self.current_slot = (self.current_slot + 1) % RING_SIZE;
        Ok(slot_idx)
    }

    /// Reads back cell data from an in-flight slot.
    pub fn readback_slot(
        &mut self,
        ctx: &GpuContext,
        slot_idx: usize,
    ) -> Result<Vec<GpuCell>, String> {
        let slot = &mut self.slots[slot_idx];
        if !slot.in_flight {
            return Err("Slot is not in-flight".into());
        }

        if !slot.is_mapped {
            ctx.device.poll(wgpu::Maintain::Poll);
            let rx = slot.receiver.as_ref().ok_or("No receiver in slot")?;
            match rx.try_recv() {
                Ok(res) => {
                    res.map_err(|e| format!("Buffer map async error: {e}"))?;
                    slot.is_mapped = true;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    ctx.device.poll(wgpu::Maintain::Wait);
                    rx.recv()
                        .map_err(|e| format!("Channel receive error: {e}"))?
                        .map_err(|e| format!("Buffer map async error: {e}"))?;
                    slot.is_mapped = true;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    return Err("Channel disconnected before buffer was mapped".into());
                }
            }
        }

        let copy_size = (self.width * self.height) as u64 * std::mem::size_of::<GpuCell>() as u64;
        let data = slot.staging_buffer.slice(..copy_size).get_mapped_range();
        let cells: Vec<GpuCell> = bytemuck::cast_slice(&data).to_vec();
        drop(data);

        slot.staging_buffer.unmap();
        slot.is_mapped = false;
        slot.in_flight = false;
        slot.receiver = None;

        Ok(cells)
    }

    /// Synchronous render and readback without custom instanced batches.
    pub fn render_and_readback(
        &mut self,
        ctx: &GpuContext,
        mesh: &MeshBuffer,
        camera: &CameraUniforms,
        lighting: &LightingUniforms,
    ) -> Result<Vec<GpuCell>, String> {
        let empty_batches = SceneBatches::default();
        self.render_and_readback_batched(ctx, mesh, &empty_batches, camera, lighting)
    }

    /// Synchronous render and readback with custom instanced batches.
    pub fn render_and_readback_batched(
        &mut self,
        ctx: &GpuContext,
        mesh: &MeshBuffer,
        batches: &SceneBatches,
        camera: &CameraUniforms,
        lighting: &LightingUniforms,
    ) -> Result<Vec<GpuCell>, String> {
        let slot = self.submit_frame(ctx, mesh, batches, camera, lighting)?;
        self.readback_slot(ctx, slot)
    }

    /// Renders a batch of frames through the double-buffered GPU pipeline ring,
    /// keeping the GPU continuously saturated with in-flight commands.
    pub fn render_batch(
        &mut self,
        ctx: &GpuContext,
        frames: &[(
            &MeshBuffer,
            &SceneBatches,
            &CameraUniforms,
            &LightingUniforms,
        )],
    ) -> Result<Vec<Vec<GpuCell>>, String> {
        if frames.is_empty() {
            return Ok(Vec::new());
        }
        if frames.len() == 1 {
            let (m, b, c, l) = frames[0];
            let cells = self.render_and_readback_batched(ctx, m, b, c, l)?;
            return Ok(vec![cells]);
        }

        let mut results = Vec::with_capacity(frames.len());
        let mut in_flight_slots = std::collections::VecDeque::new();

        for &(mesh, batches, camera, lighting) in frames {
            if in_flight_slots.len() >= RING_SIZE {
                if let Some(oldest_slot) = in_flight_slots.pop_front() {
                    let cells = self.readback_slot(ctx, oldest_slot)?;
                    results.push(cells);
                }
            }

            let slot_idx = self.submit_frame(ctx, mesh, batches, camera, lighting)?;
            in_flight_slots.push_back(slot_idx);
        }

        while let Some(slot) = in_flight_slots.pop_front() {
            let cells = self.readback_slot(ctx, slot)?;
            results.push(cells);
        }

        Ok(results)
    }

    /// O(1) mathematical screen-to-world unprojection borrowed from Asciicker's depth unprojection logic.
    /// Maps a screen cell coordinate (screen_x, screen_y) and its GPU depth buffer value into exact 3D world coordinates.
    pub fn unproject_screen_to_world(
        &self,
        screen_x: f32,
        screen_y: f32,
        depth: f32,
        camera: &CameraUniforms,
    ) -> glam::Vec3 {
        camera.unproject_screen_to_world(screen_x, screen_y, depth)
    }
}

fn create_target_texture(
    device: &wgpu::Device,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
    label: &str,
) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(&format!("{label} Texture")),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view)
}

fn create_depth_texture(
    device: &wgpu::Device,
    width: u32,
    height: u32,
) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Depth Texture"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Depth32Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view)
}
