//! GPU Uniform structures for camera, lighting, and post-processing.

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct CameraUniforms {
    pub view_proj: [[f32; 4]; 4],
    pub camera_pos: [f32; 4],
    pub time: f32,
    pub weather_mode: u32,
    pub width: u32,
    pub height: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct GpuPointLight {
    pub position: [f32; 4], // xyz, radius in w
    pub color: [f32; 4],    // rgb, intensity in w
}

impl Default for GpuPointLight {
    fn default() -> Self {
        Self {
            position: [0.0, 0.0, 0.0, 10.0],
            color: [1.0, 0.8, 0.5, 1.0],
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct LightingUniforms {
    pub sun_dir: [f32; 4],
    pub sun_color: [f32; 4],
    pub ambient_color: [f32; 4],
    pub point_lights: [GpuPointLight; 8],
    pub num_point_lights: u32,
    pub pad0: u32,
    pub pad1: u32,
    pub pad2: u32,
}

impl Default for LightingUniforms {
    fn default() -> Self {
        Self {
            sun_dir: [0.4, 0.8, 0.45, 0.0],
            sun_color: [0.55, 0.65, 0.85, 1.0], // Calm twilight blue-amber
            ambient_color: [0.12, 0.16, 0.22, 1.0], // Deep atmospheric moonlight
            point_lights: [GpuPointLight::default(); 8],
            num_point_lights: 0,
            pad0: 0,
            pad1: 0,
            pad2: 0,
        }
    }
}
