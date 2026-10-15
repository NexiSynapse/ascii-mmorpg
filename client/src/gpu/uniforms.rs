//! GPU Uniform structures for camera, lighting, and post-processing.

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct CameraUniforms {
    pub view_proj: [[f32; 4]; 4],
    pub inv_view_proj: [[f32; 4]; 4],
    pub camera_pos: [f32; 4],
    pub time: f32,
    pub weather_mode: u32,
    pub width: u32,
    pub height: u32,
}

impl CameraUniforms {
    /// O(1) mathematical screen-to-world unprojection borrowed from Asciicker's depth unprojection logic.
    /// Maps a screen cell coordinate (x, y) and its GPU depth buffer value into exact 3D world coordinates.
    pub fn unproject_screen_to_world(
        &self,
        screen_x: f32,
        screen_y: f32,
        depth: f32,
    ) -> glam::Vec3 {
        let inv_vp = glam::Mat4::from_cols_array_2d(&self.inv_view_proj);
        let ndc_x = (screen_x / self.width as f32) * 2.0 - 1.0;
        let ndc_y = 1.0 - (screen_y / self.height as f32) * 2.0; // Y is inverted in NDC
        let ndc_z = depth; // 0.0 to 1.0 in wgpu Depth32Float
        let ndc = glam::Vec4::new(ndc_x, ndc_y, ndc_z, 1.0);
        let world = inv_vp * ndc;
        if world.w.abs() > 1e-6 {
            glam::Vec3::new(world.x / world.w, world.y / world.w, world.z / world.w)
        } else {
            glam::Vec3::new(world.x, world.y, world.z)
        }
    }
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
    pub point_lights: [GpuPointLight; 16],
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
            point_lights: [GpuPointLight::default(); 16],
            num_point_lights: 0,
            pad0: 0,
            pad1: 0,
            pad2: 0,
        }
    }
}
