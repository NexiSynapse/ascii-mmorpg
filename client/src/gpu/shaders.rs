//! Embedded WGSL shaders for 3D scene rasterization and ASCII conversion post-processing.

/// 3D Scene Rasterization Shader: Evaluates Blinn-Phong lighting, multiple point lights,
/// specular highlights, atmospheric depth fog, and encodes Color, Normal, and Depth.
pub const SCENE_3D_WGSL: &str = r#"
struct CameraUniforms {
    view_proj: mat4x4<f32>,
    inv_view_proj: mat4x4<f32>,
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
    var world_pos4 = model * vec4<f32>(in.position, 1.0);
    let mat_id = select(in.material_id, inst.instance_material.x, inst.instance_material.x >= 0.0);

    // Procedural wind sway for foliage (material_id == 3.0: pine needles, leaves, canopies)
    // Anchors tree base/ground while allowing elevated canopy cones and boughs to oscillate
    if (mat_id >= 2.8 && mat_id <= 3.2 && world_pos4.y > 0.8) {
        let wind_intensity = min((world_pos4.y - 0.8) * 0.045, 0.18);
        let sway = sin(camera.time * 2.4 + world_pos4.x * 0.65 + world_pos4.z * 0.45) * wind_intensity;
        world_pos4.x += sway;
        world_pos4.z += sway * 0.45;
    }

    out.world_pos = world_pos4.xyz;
    out.world_normal = normalize((model * vec4<f32>(in.normal, 0.0)).xyz);
    out.color = in.color * inst.instance_color;
    out.material_id = mat_id;
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
    depth_bits: u32,
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

    // 2x Super-Sampling Quadrant Texel Coordinates (Asciicker 2x sub-sample resolution logic)
    let base_x = i32(x * 2u);
    let base_y = i32(y * 2u);
    let tex_w = i32(config.width * 2u);
    let tex_h = i32(config.height * 2u);

    let p00 = vec2<i32>(base_x, base_y);
    let p10 = vec2<i32>(min(base_x + 1, tex_w - 1), base_y);
    let p01 = vec2<i32>(base_x, min(base_y + 1, tex_h - 1));
    let p11 = vec2<i32>(min(base_x + 1, tex_w - 1), min(base_y + 1, tex_h - 1));

    let c00 = textureLoad(color_tex, p00, 0);
    let c10 = textureLoad(color_tex, p10, 0);
    let c01 = textureLoad(color_tex, p01, 0);
    let c11 = textureLoad(color_tex, p11, 0);

    let nd00 = textureLoad(normal_depth_tex, p00, 0);
    let nd10 = textureLoad(normal_depth_tex, p10, 0);
    let nd01 = textureLoad(normal_depth_tex, p01, 0);
    let nd11 = textureLoad(normal_depth_tex, p11, 0);

    // 1. Integrated Sub-Pixel Color & Normal/Depth (SSAA Anti-Aliasing)
    let avg_color = (c00 + c10 + c01 + c11) * 0.25;
    let avg_nd = (nd00 + nd10 + nd01 + nd11) * 0.25;

    // 2. Sub-Pixel Horizontal & Vertical Gradient Evaluation (Discontinuity detection)
    // Horizontal difference (Left vs Right)
    let depth_diff_x = abs((nd10.w + nd11.w) - (nd00.w + nd01.w)) * 0.5;
    let normal_diff_x = length((nd10.xyz + nd11.xyz) - (nd00.xyz + nd01.xyz)) * 0.5;

    // Vertical difference (Top vs Bottom)
    let depth_diff_y = abs((nd01.w + nd11.w) - (nd00.w + nd10.w)) * 0.5;
    let normal_diff_y = length((nd01.xyz + nd11.xyz) - (nd00.xyz + nd10.xyz)) * 0.5;

    // Expanded 1-cell neighborhood context
    let p_left = vec2<i32>(max(base_x - 1, 0), base_y);
    let p_right = vec2<i32>(min(base_x + 2, tex_w - 1), base_y);
    let p_top = vec2<i32>(base_x, max(base_y - 1, 0));
    let p_bot = vec2<i32>(base_x, min(base_y + 2, tex_h - 1));

    let nd_l = textureLoad(normal_depth_tex, p_left, 0);
    let nd_r = textureLoad(normal_depth_tex, p_right, 0);
    let nd_t = textureLoad(normal_depth_tex, p_top, 0);
    let nd_b = textureLoad(normal_depth_tex, p_bot, 0);

    let neigh_depth_x = abs(nd_r.w - nd_l.w);
    let neigh_depth_y = abs(nd_b.w - nd_t.w);
    let neigh_norm_x = length(nd_r.xyz - nd_l.xyz);
    let neigh_norm_y = length(nd_b.xyz - nd_t.xyz);

    let edge_x = max(depth_diff_x * 10.0 + normal_diff_x, neigh_depth_x * 6.0 + neigh_norm_x);
    let edge_y = max(depth_diff_y * 10.0 + normal_diff_y, neigh_depth_y * 6.0 + neigh_norm_y);
    let edge_mag = edge_x + edge_y;

    var glyph: u32 = 32u; // ' ' default
    var is_edge = false;

    // Detect structural edges (silhouettes and creases)
    if (edge_mag > 0.38 && avg_nd.w < 0.999) {
        is_edge = true;
        if (edge_y > edge_x * 1.6) {
            glyph = 95u; // '_'
        } else if (edge_x > edge_y * 1.6) {
            glyph = 124u; // '|'
        } else if (edge_x > 0.22 && edge_y > 0.22) {
            let diag1 = abs(nd00.w - nd11.w);
            let diag2 = abs(nd10.w - nd01.w);
            if (diag1 < diag2) {
                glyph = 47u; // '/'
            } else {
                glyph = 92u; // '\'
            }
        } else {
            glyph = 43u; // '+'
        }
    }

    // Integrated perceived luminance with SSAA smoothing
    let luma = dot(avg_color.rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
    let dither = bayer4(x, y) * 0.05;
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
    let fg_rgb = clamp(avg_color.rgb * config.contrast_boost, vec3<f32>(0.0), vec3<f32>(1.0));
    // Atmospheric dark background tint for deep contrast
    let bg_rgb = clamp(avg_color.rgb * 0.12, vec3<f32>(0.0), vec3<f32>(1.0));

    let out_idx = y * config.width + x;
    var cell: GpuCell;
    cell.glyph = glyph;
    cell.fg_r = u32(clamp(fg_rgb.r * 255.0, 0.0, 255.0));
    cell.fg_g = u32(clamp(fg_rgb.g * 255.0, 0.0, 255.0));
    cell.fg_b = u32(clamp(fg_rgb.b * 255.0, 0.0, 255.0));
    cell.bg_r = u32(clamp(bg_rgb.r * 255.0, 0.0, 255.0));
    cell.bg_g = u32(clamp(bg_rgb.g * 255.0, 0.0, 255.0));
    cell.bg_b = u32(clamp(bg_rgb.b * 255.0, 0.0, 255.0));
    cell.depth_bits = bitcast<u32>(avg_nd.w);

    cell_output.cells[out_idx] = cell;
}
"#;
