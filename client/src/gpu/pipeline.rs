//! Offscreen WGPU 3D rasterization and ASCII compute post-processing pipeline.

use super::context::GpuContext;
use super::mesh::{MeshBuffer, Vertex};
use super::shaders::{ASCII_POST_WGSL, SCENE_3D_WGSL};
use super::uniforms::{CameraUniforms, LightingUniforms};

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
    pub _pad: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct PostConfigUniform {
    width: u32,
    height: u32,
    time: f32,
    contrast_boost: f32,
}

pub struct GpuPipeline {
    pub width: u32,
    pub height: u32,

    // 3D Scene Pipeline
    pub scene_pipeline: wgpu::RenderPipeline,
    pub scene_bind_group_layout: wgpu::BindGroupLayout,
    pub scene_bind_group: wgpu::BindGroup,

    // ASCII Compute Pipeline
    pub ascii_pipeline: wgpu::ComputePipeline,
    pub ascii_bind_group_layout: wgpu::BindGroupLayout,
    pub ascii_bind_group: wgpu::BindGroup,

    // Textures
    pub color_texture: wgpu::Texture,
    pub color_view: wgpu::TextureView,
    pub normal_depth_texture: wgpu::Texture,
    pub normal_depth_view: wgpu::TextureView,
    pub depth_texture: wgpu::Texture,
    pub depth_view: wgpu::TextureView,

    // Uniform Buffers
    pub camera_buffer: wgpu::Buffer,
    pub lighting_buffer: wgpu::Buffer,
    pub config_buffer: wgpu::Buffer,

    // Output Storage & Staging
    pub cell_output_buffer: wgpu::Buffer,
    pub staging_buffer: wgpu::Buffer,

    // Geometry buffers
    pub vertex_buffer: wgpu::Buffer,
    pub index_buffer: wgpu::Buffer,
    pub vertex_capacity: usize,
    pub index_capacity: usize,
}

impl GpuPipeline {
    pub fn new(ctx: &GpuContext, width: u32, height: u32) -> Result<Self, String> {
        let width = width.max(16);
        let height = height.max(8);
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

        // 2. Uniform Buffers
        let camera_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Camera Uniform Buffer"),
            size: std::mem::size_of::<CameraUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let lighting_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Lighting Uniform Buffer"),
            size: std::mem::size_of::<LightingUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let config_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Post Config Uniform Buffer"),
            size: std::mem::size_of::<PostConfigUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // 3. Bind Group Layouts
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

        let scene_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Scene Bind Group"),
            layout: &scene_bind_group_layout,
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
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                ],
            });

        // 4. Render Pipeline Layouts & Pipelines
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
            ],
        };

        let scene_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Scene 3D Pipeline"),
            layout: Some(&scene_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &scene_shader,
                entry_point: Some("vs_main"),
                buffers: &[vertex_layout],
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
                cull_mode: None, // Double-sided geometry for terminals
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

        // 5. Create Textures and Output Buffers
        let (color_texture, color_view) = create_target_texture(
            device,
            width,
            height,
            wgpu::TextureFormat::Rgba8UnormSrgb,
            "Color",
        );
        let (normal_depth_texture, normal_depth_view) = create_target_texture(
            device,
            width,
            height,
            wgpu::TextureFormat::Rgba8Unorm,
            "NormalDepth",
        );
        let (depth_texture, depth_view) = create_depth_texture(device, width, height);

        let cell_count = (width * height) as u64;
        let cell_buffer_size = cell_count * std::mem::size_of::<GpuCell>() as u64;

        let cell_output_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Cell Output Buffer"),
            size: cell_buffer_size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });

        let staging_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Staging Readback Buffer"),
            size: cell_buffer_size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let ascii_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ASCII Bind Group"),
            layout: &ascii_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&color_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&normal_depth_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: cell_output_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: config_buffer.as_entire_binding(),
                },
            ],
        });

        // 6. Geometry Buffers with default capacity
        let vertex_capacity = 32_000;
        let index_capacity = 64_000;

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

        Ok(Self {
            width,
            height,
            scene_pipeline,
            scene_bind_group_layout,
            scene_bind_group,
            ascii_pipeline,
            ascii_bind_group_layout,
            ascii_bind_group,
            color_texture,
            color_view,
            normal_depth_texture,
            normal_depth_view,
            depth_texture,
            depth_view,
            camera_buffer,
            lighting_buffer,
            config_buffer,
            cell_output_buffer,
            staging_buffer,
            vertex_buffer,
            index_buffer,
            vertex_capacity,
            index_capacity,
        })
    }

    pub fn resize(&mut self, ctx: &GpuContext, width: u32, height: u32) {
        let width = width.max(16);
        let height = height.max(8);
        if self.width == width && self.height == height {
            return;
        }

        self.width = width;
        self.height = height;
        let device = &ctx.device;

        let (c_tex, c_view) = create_target_texture(
            device,
            width,
            height,
            wgpu::TextureFormat::Rgba8UnormSrgb,
            "Color",
        );
        let (nd_tex, nd_view) = create_target_texture(
            device,
            width,
            height,
            wgpu::TextureFormat::Rgba8Unorm,
            "NormalDepth",
        );
        let (d_tex, d_view) = create_depth_texture(device, width, height);

        self.color_texture = c_tex;
        self.color_view = c_view;
        self.normal_depth_texture = nd_tex;
        self.normal_depth_view = nd_view;
        self.depth_texture = d_tex;
        self.depth_view = d_view;

        let cell_count = (width * height) as u64;
        let cell_buffer_size = cell_count * std::mem::size_of::<GpuCell>() as u64;

        self.cell_output_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Cell Output Buffer (Resized)"),
            size: cell_buffer_size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });

        self.staging_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Staging Readback Buffer (Resized)"),
            size: cell_buffer_size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        self.ascii_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ASCII Bind Group (Resized)"),
            layout: &self.ascii_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&self.color_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&self.normal_depth_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: self.cell_output_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: self.config_buffer.as_entire_binding(),
                },
            ],
        });
    }

    /// Renders 3D scene and runs compute ASCII post-processing pass, reading back cells.
    pub fn render_and_readback(
        &mut self,
        ctx: &GpuContext,
        mesh: &MeshBuffer,
        camera: &CameraUniforms,
        lighting: &LightingUniforms,
    ) -> Result<Vec<GpuCell>, String> {
        let device = &ctx.device;
        let queue = &ctx.queue;

        // Ensure dynamic vertex/index capacity
        if mesh.vertices.len() > self.vertex_capacity {
            self.vertex_capacity = mesh.vertices.len() * 2;
            self.vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Reallocated Vertex Buffer"),
                size: (self.vertex_capacity * std::mem::size_of::<Vertex>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        if mesh.indices.len() > self.index_capacity {
            self.index_capacity = mesh.indices.len() * 2;
            self.index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Reallocated Index Buffer"),
                size: (self.index_capacity * std::mem::size_of::<u32>()) as u64,
                usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }

        // Upload geometry & uniforms
        if !mesh.vertices.is_empty() {
            queue.write_buffer(&self.vertex_buffer, 0, bytemuck::cast_slice(&mesh.vertices));
        }
        if !mesh.indices.is_empty() {
            queue.write_buffer(&self.index_buffer, 0, bytemuck::cast_slice(&mesh.indices));
        }
        queue.write_buffer(&self.camera_buffer, 0, bytemuck::bytes_of(camera));
        queue.write_buffer(&self.lighting_buffer, 0, bytemuck::bytes_of(lighting));

        let config = PostConfigUniform {
            width: self.width,
            height: self.height,
            time: camera.time,
            contrast_boost: 1.15,
        };
        queue.write_buffer(&self.config_buffer, 0, bytemuck::bytes_of(&config));

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Render & ASCII Encoder"),
        });

        // Pass 1: 3D Rasterization Pass
        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("3D Scene Pass"),
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

            if !mesh.indices.is_empty() {
                render_pass.set_pipeline(&self.scene_pipeline);
                render_pass.set_bind_group(0, &self.scene_bind_group, &[]);
                render_pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
                render_pass
                    .set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                render_pass.draw_indexed(0..mesh.indices.len() as u32, 0, 0..1);
            }
        }

        // Pass 2: ASCII Post-Processing Compute Pass
        {
            let mut compute_pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("ASCII Post Compute Pass"),
                timestamp_writes: None,
            });
            compute_pass.set_pipeline(&self.ascii_pipeline);
            compute_pass.set_bind_group(0, &self.ascii_bind_group, &[]);
            let workgroups_x = (self.width + 15) / 16;
            let workgroups_y = (self.height + 15) / 16;
            compute_pass.dispatch_workgroups(workgroups_x, workgroups_y, 1);
        }

        // Copy Cell Storage Buffer to Staging Buffer
        let copy_size = (self.width * self.height) as u64 * std::mem::size_of::<GpuCell>() as u64;
        encoder.copy_buffer_to_buffer(
            &self.cell_output_buffer,
            0,
            &self.staging_buffer,
            0,
            copy_size,
        );

        queue.submit(Some(encoder.finish()));

        // Map and Readback to CPU
        let buffer_slice = self.staging_buffer.slice(..copy_size);
        let (tx, rx) = std::sync::mpsc::channel();
        buffer_slice.map_async(wgpu::MapMode::Read, move |res| {
            let _ = tx.send(res);
        });

        ctx.device.poll(wgpu::Maintain::Wait);
        rx.recv()
            .map_err(|e| format!("Channel receive error: {e}"))?
            .map_err(|e| format!("Buffer map async error: {e}"))?;

        let data = buffer_slice.get_mapped_range();
        let cells: Vec<GpuCell> = bytemuck::cast_slice(&data).to_vec();
        drop(data);
        self.staging_buffer.unmap();

        Ok(cells)
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
