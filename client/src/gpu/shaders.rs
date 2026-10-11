//! Embedded WGSL shaders for 3D scene rasterization and ASCII conversion post-processing.

/// 3D Scene Rasterization Shader: Evaluates Blinn-Phong lighting, multiple point lights,
/// specular highlights, atmospheric depth fog, and encodes Color, Normal, and Depth.
pub const SCENE_3D_WGSL: &str = r#"
struct CameraUniforms {
    view_proj: mat4x4<f32>,
    camera_pos: vec4<f32>,
    time: f32,
    weather_mode: u32,
    width: u32,
    height: u32,
};

struct PointLight {
    position: vec4<f32>, // xyz, radius in w
    color: vec4<f32>,    // rgb, intensity in w
};

struct LightingUniforms {
    sun_dir: vec4<f32>,
    sun_color: vec4<f32>,
    ambient_color: vec4<f32>,
    point_lights: array<PointLight, 8>,
    num_point_lights: u32,
    pad0: u32,
    pad1: u32,
    pad2: u32,
};

@group(0) @binding(0) var<uniform> camera: CameraUniforms;
@group(0) @binding(1) var<uniform> lighting: LightingUniforms;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>,
    @location(3) material_id: f32,
    @location(4) roughness: f32,
};

struct InstanceInput {
    @location(5) model_col0: vec4<f32>,
    @location(6) model_col1: vec4<f32>,
    @location(7) model_col2: vec4<f32>,
    @location(8) model_col3: vec4<f32>,
    @location(9) instance_color: vec4<f32>,
    @location(10) instance_material: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_pos: vec3<f32>,
    @location(1) world_normal: vec3<f32>,
    @location(2) color: vec4<f32>,
    @location(3) material_id: f32,
    @location(4) roughness: f32,
};

@vertex
fn vs_main(in: VertexInput, inst: InstanceInput) -> VertexOutput {
    var out: VertexOutput;
    let model = mat4x4<f32>(
        inst.model_col0,
        inst.model_col1,
        inst.model_col2,
        inst.model_col3,
    );
    let world_pos4 = model * vec4<f32>(in.position, 1.0);
    out.world_pos = world_pos4.xyz;
    out.world_normal = normalize((model * vec4<f32>(in.normal, 0.0)).xyz);
    out.color = in.color * inst.instance_color;
    out.material_id = select(in.material_id, inst.instance_material.x, inst.instance_material.x >= 0.0);
    out.roughness = select(in.roughness, inst.instance_material.y, inst.instance_material.y >= 0.0);
    out.clip_position = camera.view_proj * world_pos4;
    return out;
}

struct FragmentOutput {
    @location(0) color: vec4<f32>,
    @location(1) normal_depth: vec4<f32>,
};

@fragment
fn fs_main(in: VertexOutput) -> FragmentOutput {
    var out: FragmentOutput;

    // Emissive materials (Torches, lanterns, visor glow, spell particles)
    if (in.material_id >= 3.9 && in.material_id <= 4.1) {
        out.color = in.color * 1.5;
        out.normal_depth = vec4<f32>(in.world_normal * 0.5 + 0.5, in.clip_position.z);
        return out;
    }

    // Weather particles
    if (in.material_id >= 4.9) {
        out.color = in.color;
        out.normal_depth = vec4<f32>(vec3<f32>(0.5, 1.0, 0.5), in.clip_position.z);
        return out;
    }

    let N = normalize(in.world_normal);
    let V = normalize(camera.camera_pos.xyz - in.world_pos);

    // Directional Sunlight / Moonlight
    let L_sun = normalize(lighting.sun_dir.xyz);
    let NdotL = max(dot(N, L_sun), 0.0);
    var diffuse_light = lighting.ambient_color.rgb + lighting.sun_color.rgb * NdotL;

    // Specular highlight from sun
    let H_sun = normalize(L_sun + V);
    let spec_power = max(1.0 / (in.roughness * in.roughness + 0.001), 2.0);
    let NdotH = max(dot(N, H_sun), 0.0);
    var specular_light = pow(NdotH, spec_power) * (1.0 - in.roughness) * lighting.sun_color.rgb * 0.4;

    // Evaluate dynamic point lights (torches, campfires, magic glows)
    let light_count = min(lighting.num_point_lights, 8u);
    for (var i = 0u; i < light_count; i = i + 1u) {
        let light = lighting.point_lights[i];
        let light_vec = light.position.xyz - in.world_pos;
        let dist = length(light_vec);
        let radius = light.position.w;

        if (dist < radius) {
            let L_point = light_vec / dist;
            let atten = clamp(1.0 - (dist / radius), 0.0, 1.0);
            let atten_sq = atten * atten * light.color.w;

            let point_NdotL = max(dot(N, L_point), 0.0);
            diffuse_light += light.color.rgb * point_NdotL * atten_sq;

            let H_point = normalize(L_point + V);
            let point_NdotH = max(dot(N, H_point), 0.0);
            specular_light += pow(point_NdotH, spec_power) * light.color.rgb * (1.0 - in.roughness) * atten_sq * 0.5;
        }
    }

    // Final lit color
    var lit_color = in.color.rgb * diffuse_light + specular_light;

    // Calm Atmospheric Distance Fog (calm twilight deep indigo falloff)
    let dist_to_cam = length(camera.camera_pos.xyz - in.world_pos);
    let fog_start = 14.0;
    let fog_end = 36.0;
    let fog_factor = clamp((dist_to_cam - fog_start) / (fog_end - fog_start), 0.0, 0.75);
    let fog_color = vec3<f32>(0.06, 0.08, 0.12);
    lit_color = mix(lit_color, fog_color, fog_factor);

    out.color = vec4<f32>(lit_color, in.color.a);
    out.normal_depth = vec4<f32>(N * 0.5 + 0.5, in.clip_position.z);
    return out;
}
"#;

/// ASCII Conversion Post-Processing Compute Shader:
/// Runs over the offscreen rendered G-buffer, evaluates Sobel edge detection,
/// surface luminance, dithering, and outputs packed cell data:
/// Cell format: [glyph_codepoint (u32), fg_r (u8), fg_g (u8), fg_b (u8), bg_r (u8), bg_g (u8), bg_b (u8)]
pub const ASCII_POST_WGSL: &str = r#"
struct GpuCell {
    glyph: u32,
    fg_r: u32,
    fg_g: u32,
    fg_b: u32,
    bg_r: u32,
    bg_g: u32,
    bg_b: u32,
    _pad: u32,
};

struct CellBuffer {
    cells: array<GpuCell>,
};

@group(0) @binding(0) var color_tex: texture_2d<f32>;
@group(0) @binding(1) var normal_depth_tex: texture_2d<f32>;
@group(0) @binding(2) var<storage, read_write> cell_output: CellBuffer;

struct ConfigUniform {
    width: u32,
    height: u32,
    time: f32,
    contrast_boost: f32,
};

@group(0) @binding(3) var<uniform> config: ConfigUniform;

// 4x4 Bayer Dither Matrix for velvety smooth luminance gradations
fn bayer4(x: u32, y: u32) -> f32 {
    let m = array<f32, 16>(
        0.0 / 16.0,  8.0 / 16.0,  2.0 / 16.0, 10.0 / 16.0,
       12.0 / 16.0,  4.0 / 16.0, 14.0 / 16.0,  6.0 / 16.0,
        3.0 / 16.0, 11.0 / 16.0,  1.0 / 16.0,  9.0 / 16.0,
       15.0 / 16.0,  7.0 / 16.0, 13.0 / 16.0,  5.0 / 16.0
    );
    let idx = (y % 4u) * 4u + (x % 4u);
    return m[idx] - 0.5;
}

@compute @workgroup_size(16, 16)
fn cs_main(@builtin(global_invocation_id) id: vec3<u32>) {
    let x = id.x;
    let y = id.y;
    if (x >= config.width || y >= config.height) {
        return;
    }

    let coords = vec2<i32>(i32(x), i32(y));
    let center_color = textureLoad(color_tex, coords, 0);
    let center_nd = textureLoad(normal_depth_tex, coords, 0);

    // Sobel edge filter over 3x3 normal and depth neighborhood
    let x_prev = max(coords.x - 1, 0);
    let x_next = min(coords.x + 1, i32(config.width) - 1);
    let y_prev = max(coords.y - 1, 0);
    let y_next = min(coords.y + 1, i32(config.height) - 1);

    let nd_top = textureLoad(normal_depth_tex, vec2<i32>(coords.x, y_prev), 0);
    let nd_bot = textureLoad(normal_depth_tex, vec2<i32>(coords.x, y_next), 0);
    let nd_left = textureLoad(normal_depth_tex, vec2<i32>(x_prev, coords.y), 0);
    let nd_right = textureLoad(normal_depth_tex, vec2<i32>(x_next, coords.y), 0);

    let depth_diff_x = abs(nd_right.w - nd_left.w);
    let depth_diff_y = abs(nd_bot.w - nd_top.w);
    let normal_diff_x = length(nd_right.xyz - nd_left.xyz);
    let normal_diff_y = length(nd_bot.xyz - nd_top.xyz);

    let edge_x = depth_diff_x * 8.0 + normal_diff_x;
    let edge_y = depth_diff_y * 8.0 + normal_diff_y;
    let edge_mag = edge_x + edge_y;

    var glyph: u32 = 32u; // ' ' default
    var is_edge = false;

    // Detect structural edges (silhouette and geometry creases)
    if (edge_mag > 0.42 && center_nd.w < 0.999) {
        is_edge = true;
        if (edge_y > edge_x * 1.8) {
            glyph = 95u; // '_'
        } else if (edge_x > edge_y * 1.8) {
            glyph = 124u; // '|'
        } else if (edge_x > 0.3 && edge_y > 0.3) {
            glyph = 43u; // '+'
        } else {
            glyph = 47u; // '/'
        }
    }

    // Perceived luminance
    let luma = dot(center_color.rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
    let dither = bayer4(x, y) * 0.06;
    let dithered_luma = clamp(luma + dither, 0.0, 1.0);

    if (!is_edge) {
        // High-fidelity Ansenjeo volumetric glyph ramp
        if (dithered_luma < 0.04) {
            glyph = 32u; // ' '
        } else if (dithered_luma < 0.10) {
            glyph = 46u; // '.'
        } else if (dithered_luma < 0.18) {
            glyph = 58u; // ':'
        } else if (dithered_luma < 0.28) {
            glyph = 42u; // '*'
        } else if (dithered_luma < 0.40) {
            glyph = 61u; // '='
        } else if (dithered_luma < 0.54) {
            glyph = 111u; // 'o'
        } else if (dithered_luma < 0.68) {
            glyph = 79u; // 'O'
        } else if (dithered_luma < 0.82) {
            glyph = 37u; // '%'
        } else if (dithered_luma < 0.92) {
            glyph = 35u; // '#'
        } else {
            glyph = 64u; // '@'
        }
    }

    // Boosted vibrant foreground color
    let fg_rgb = clamp(center_color.rgb * 1.15, vec3<f32>(0.0), vec3<f32>(1.0));
    // Atmospheric dark background tint for deep contrast
    let bg_rgb = clamp(center_color.rgb * 0.12, vec3<f32>(0.0), vec3<f32>(1.0));

    let out_idx = y * config.width + x;
    var cell: GpuCell;
    cell.glyph = glyph;
    cell.fg_r = u32(clamp(fg_rgb.r * 255.0, 0.0, 255.0));
    cell.fg_g = u32(clamp(fg_rgb.g * 255.0, 0.0, 255.0));
    cell.fg_b = u32(clamp(fg_rgb.b * 255.0, 0.0, 255.0));
    cell.bg_r = u32(clamp(bg_rgb.r * 255.0, 0.0, 255.0));
    cell.bg_g = u32(clamp(bg_rgb.g * 255.0, 0.0, 255.0));
    cell.bg_b = u32(clamp(bg_rgb.b * 255.0, 0.0, 255.0));
    cell._pad = 0u;

    cell_output.cells[out_idx] = cell;
}
"#;
