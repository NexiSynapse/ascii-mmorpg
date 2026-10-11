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
    @location(5) entity_id: f32,
    @location(6) entity_class: f32,
};

struct InstanceInput {
    @location(7) model_col0: vec4<f32>,
    @location(8) model_col1: vec4<f32>,
    @location(9) model_col2: vec4<f32>,
    @location(10) model_col3: vec4<f32>,
    @location(11) instance_color: vec4<f32>,
    @location(12) instance_material: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_pos: vec3<f32>,
    @location(1) world_normal: vec3<f32>,
    @location(2) color: vec4<f32>,
    @location(3) material_id: f32,
    @location(4) roughness: f32,
    @location(5) @interpolate(flat) entity_id: f32,
    @location(6) @interpolate(flat) entity_class: f32,
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
    let entity_id = select(in.entity_id, inst.instance_material.z, inst.instance_material.z >= 0.0);
    let entity_class = select(in.entity_class, inst.instance_material.w, inst.instance_material.z >= 0.0);

    // Procedural wind sway for foliage (material_id == 3.0: pine needles, leaves, canopies)
    // Anchors tree base/ground while allowing elevated canopy cones and boughs to oscillate
    if (mat_id >= 2.8 && mat_id <= 3.2 && world_pos4.y > 0.8) {
        let wind_intensity = min((world_pos4.y - 0.8) * 0.045, 0.18);
        let sway = sin(camera.time * 2.4 + world_pos4.x * 0.65 + world_pos4.z * 0.45) * wind_intensity;
        world_pos4.x += sway;
        world_pos4.z += sway * 0.45;
    }

    // Procedural water surface waves (material_id == 6.0: streams, rivers, pools)
    if (mat_id >= 5.8 && mat_id <= 6.2) {
        let wave = sin(camera.time * 2.6 + world_pos4.x * 1.8 + world_pos4.z * 1.2) * 0.035;
        world_pos4.y += wave;
    }

    out.world_pos = world_pos4.xyz;
    out.world_normal = normalize((model * vec4<f32>(in.normal, 0.0)).xyz);
    out.color = in.color * inst.instance_color;
    out.material_id = mat_id;
    out.roughness = select(in.roughness, inst.instance_material.y, inst.instance_material.y >= 0.0);
    // Preserve the entity channel through the scene pass. Without these
    // assignments the metadata G-buffer is always zero, so the compute pass
    // cannot distinguish player/monster silhouettes from world geometry.
    out.entity_id = entity_id;
    out.entity_class = entity_class;
    out.clip_position = camera.view_proj * world_pos4;
    return out;
}

struct FragmentOutput {
    @location(0) color: vec4<f32>,
    @location(1) normal_depth: vec4<f32>,
    @location(2) metadata: vec4<f32>,
};

fn pack_metadata(material_id: f32, entity_id: f32, entity_class: f32) -> vec4<f32> {
    return vec4<f32>(
        clamp(material_id, 0.0, 255.0) / 255.0,
        clamp(entity_id, 0.0, 255.0) / 255.0,
        clamp(entity_class, 0.0, 255.0) / 255.0,
        0.0,
    );
}

@fragment
fn fs_main(in: VertexOutput) -> FragmentOutput {
    var out: FragmentOutput;

    // Emissive materials (Torches, lanterns, visor glow, spell particles)
    if (in.material_id >= 3.9 && in.material_id <= 4.1) {
        out.color = in.color * 1.5;
        out.normal_depth = vec4<f32>(in.world_normal * 0.5 + 0.5, in.clip_position.z);
        out.metadata = pack_metadata(in.material_id, in.entity_id, in.entity_class);
        return out;
    }

    // Weather particles
    if (in.material_id >= 4.9 && in.material_id <= 5.1) {
        out.color = in.color;
        out.normal_depth = vec4<f32>(vec3<f32>(0.5, 1.0, 0.5), in.clip_position.z);
        out.metadata = pack_metadata(in.material_id, in.entity_id, in.entity_class);
        return out;
    }

    let V = normalize(camera.camera_pos.xyz - in.world_pos);

    // Water surfaces: azure water color with dynamic wave normal specular shimmer
    if (in.material_id >= 5.8 && in.material_id <= 6.2) {
        let wave_t = camera.time * 2.6;
        let wave_norm = normalize(vec3<f32>(
            sin(wave_t + in.world_pos.x * 2.2) * 0.25,
            0.95,
            cos(wave_t + in.world_pos.z * 2.0) * 0.25
        ));
        let water_base = vec3<f32>(0.09, 0.25, 0.48);
        let water_NdotL = max(dot(wave_norm, normalize(lighting.sun_dir.xyz)), 0.0);
        let water_H = normalize(normalize(lighting.sun_dir.xyz) + V);
        let water_spec = pow(max(dot(wave_norm, water_H), 0.0), 16.0) * 0.85;
        let water_color = water_base * (lighting.ambient_color.rgb + lighting.sun_color.rgb * water_NdotL) + vec3<f32>(water_spec);
        out.color = vec4<f32>(water_color, 0.85);
        out.normal_depth = vec4<f32>(wave_norm * 0.5 + 0.5, in.clip_position.z);
        out.metadata = pack_metadata(in.material_id, in.entity_id, in.entity_class);
        return out;
    }

    let N = normalize(in.world_normal);

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
    // Fragment @builtin(position).z is already device depth (0..1).
    out.normal_depth = vec4<f32>(N * 0.5 + 0.5, in.clip_position.z);
    out.metadata = pack_metadata(in.material_id, in.entity_id, in.entity_class);
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
@group(0) @binding(2) var metadata_tex: texture_2d<f32>;
@group(0) @binding(3) var<storage, read_write> cell_output: CellBuffer;

struct ConfigUniform {
    width: u32,
    height: u32,
    time: f32,
    contrast_boost: f32,
};

@group(0) @binding(4) var<uniform> config: ConfigUniform;

fn material_glyph(material: u32, band: u32) -> u32 {
    if (material == 0u) { // terrain: keep the ground readable without a solid block of '@'
        if (band == 0u) { return 32u; }
        if (band == 1u) { return 46u; }
        if (band == 2u) { return 58u; }
        if (band == 3u) { return 95u; }
        if (band == 4u) { return 45u; }
        return 61u;
    }
    if (material == 3u) { // foliage: organic strokes and clustered leaves
        if (band == 0u) { return 32u; }
        if (band == 1u) { return 44u; }
        if (band == 2u) { return 59u; }
        if (band == 3u) { return 42u; }
        if (band == 4u) { return 38u; }
        return 38u;
    }
    if (material == 5u) { // weather: sparse points and directional streaks
        if (band == 0u) { return 32u; }
        if (band == 1u) { return 46u; }
        if (band == 2u) { return 183u; }
        if (band == 3u) { return 47u; }
        if (band == 4u) { return 124u; }
        return 47u;
    }
    if (material == 6u) { // water: horizontal ripple language
        if (band == 0u) { return 32u; }
        if (band == 1u) { return 183u; }
        if (band == 2u) { return 126u; }
        if (band == 3u) { return 45u; }
        if (band == 4u) { return 61u; }
        return 126u;
    }
    if (material == 1u) { // metal: hard creases
        if (band == 0u) { return 32u; }
        if (band == 1u) { return 45u; }
        if (band == 2u) { return 61u; }
        if (band == 3u) { return 43u; }
        if (band == 4u) { return 35u; }
        return 64u;
    }
    if (band == 0u) { return 32u; }
    if (band == 1u) { return 46u; }
    if (band == 2u) { return 58u; }
    if (band == 3u) { return 42u; }
    if (band == 4u) { return 35u; }
    return 64u;
}

fn luma_at(x: i32, y: i32) -> f32 {
    let color = textureLoad(color_tex, vec2<i32>(x, y), 0).rgb;
    return dot(color, vec3<f32>(0.2126, 0.7152, 0.0722));
}

fn shape_region(base_x: i32, base_y: i32, region: u32) -> f32 {
    if (region == 0u) {
        return (luma_at(base_x, base_y) + luma_at(base_x + 1, base_y)) * 0.5;
    }
    if (region == 1u) {
        return (luma_at(base_x + 2, base_y) + luma_at(base_x + 3, base_y)) * 0.5;
    }
    if (region == 2u) {
        return (luma_at(base_x, base_y + 1) + luma_at(base_x + 1, base_y + 1)
            + luma_at(base_x, base_y + 2) + luma_at(base_x + 1, base_y + 2)) * 0.25;
    }
    if (region == 3u) {
        return (luma_at(base_x + 2, base_y + 1) + luma_at(base_x + 3, base_y + 1)
            + luma_at(base_x + 2, base_y + 2) + luma_at(base_x + 3, base_y + 2)) * 0.25;
    }
    if (region == 4u) {
        return (luma_at(base_x, base_y + 3) + luma_at(base_x + 1, base_y + 3)) * 0.5;
    }
    return (luma_at(base_x + 2, base_y + 3) + luma_at(base_x + 3, base_y + 3)) * 0.5;
}

fn shape_distance(
    s0: f32, s1: f32, s2: f32, s3: f32, s4: f32, s5: f32,
    v0: f32, v1: f32, v2: f32, v3: f32, v4: f32, v5: f32,
) -> f32 {
    let d0 = s0 - v0;
    let d1 = s1 - v1;
    let d2 = s2 - v2;
    let d3 = s3 - v3;
    let d4 = s4 - v4;
    let d5 = s5 - v5;
    return d0 * d0 + d1 * d1 + d2 * d2 + d3 * d3 + d4 * d4 + d5 * d5;
}

fn shape_glyph(material: u32, base_x: i32, base_y: i32) -> u32 {
    let s0 = shape_region(base_x, base_y, 0u);
    let s1 = shape_region(base_x, base_y, 1u);
    let s2 = shape_region(base_x, base_y, 2u);
    let s3 = shape_region(base_x, base_y, 3u);
    let s4 = shape_region(base_x, base_y, 4u);
    let s5 = shape_region(base_x, base_y, 5u);
    let peak = max(max(max(s0, s1), max(s2, s3)), max(s4, s5));
    let n0 = pow(s0 / max(peak, 0.001), 0.72);
    let n1 = pow(s1 / max(peak, 0.001), 0.72);
    let n2 = pow(s2 / max(peak, 0.001), 0.72);
    let n3 = pow(s3 / max(peak, 0.001), 0.72);
    let n4 = pow(s4 / max(peak, 0.001), 0.72);
    let n5 = pow(s5 / max(peak, 0.001), 0.72);

    var best = 0u;
    var best_distance = shape_distance(n0, n1, n2, n3, n4, n5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
    let dot_distance = shape_distance(n0, n1, n2, n3, n4, n5, 0.12, 0.12, 0.12, 0.12, 0.12, 0.12);
    if (dot_distance < best_distance) { best = 1u; best_distance = dot_distance; }
    let horizontal_distance = shape_distance(n0, n1, n2, n3, n4, n5, 0.15, 0.15, 0.25, 0.25, 0.80, 0.80);
    if (horizontal_distance < best_distance) { best = 2u; best_distance = horizontal_distance; }
    let cross_distance = shape_distance(n0, n1, n2, n3, n4, n5, 0.55, 0.55, 0.75, 0.75, 0.55, 0.55);
    if (cross_distance < best_distance) { best = 3u; best_distance = cross_distance; }
    let diagonal_distance = shape_distance(n0, n1, n2, n3, n4, n5, 0.80, 0.15, 0.15, 0.80, 0.15, 0.80);
    if (diagonal_distance < best_distance) { best = 4u; best_distance = diagonal_distance; }
    let dense_distance = shape_distance(n0, n1, n2, n3, n4, n5, 0.88, 0.88, 0.88, 0.88, 0.88, 0.88);
    if (dense_distance < best_distance) { best = 5u; }
    return material_glyph(material, best);
}

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

    let base_x = i32(x * 4u);
    let base_y = i32(y * 4u);
    let tex_w = i32(config.width * 4u);
    let tex_h = i32(config.height * 4u);

    let p00 = vec2<i32>(base_x, base_y);
    let p10 = vec2<i32>(min(base_x + 3, tex_w - 1), base_y);
    let p01 = vec2<i32>(base_x, min(base_y + 3, tex_h - 1));
    let p11 = vec2<i32>(min(base_x + 3, tex_w - 1), min(base_y + 3, tex_h - 1));

    var avg_color = vec4<f32>(0.0);
    var avg_nd = vec4<f32>(0.0);
    for (var sx = 0; sx < 4; sx = sx + 1) {
        for (var sy = 0; sy < 4; sy = sy + 1) {
            let sample_pos = vec2<i32>(base_x + sx, base_y + sy);
            avg_color = avg_color + textureLoad(color_tex, sample_pos, 0);
            avg_nd = avg_nd + textureLoad(normal_depth_tex, sample_pos, 0);
        }
    }
    avg_color = avg_color * 0.0625;
    avg_nd = avg_nd * 0.0625;

    let nd00 = textureLoad(normal_depth_tex, p00, 0);
    let nd10 = textureLoad(normal_depth_tex, p10, 0);
    let nd01 = textureLoad(normal_depth_tex, p01, 0);
    let nd11 = textureLoad(normal_depth_tex, p11, 0);

    // 2. Sub-Pixel Horizontal & Vertical Gradient Evaluation (Discontinuity detection)
    // Horizontal difference (Left vs Right)
    let depth_diff_x = abs((nd10.w + nd11.w) - (nd00.w + nd01.w)) * 0.5;
    let normal_diff_x = length((nd10.xyz + nd11.xyz) - (nd00.xyz + nd01.xyz)) * 0.5;

    // Vertical difference (Top vs Bottom)
    let depth_diff_y = abs((nd01.w + nd11.w) - (nd00.w + nd10.w)) * 0.5;
    let normal_diff_y = length((nd01.xyz + nd11.xyz) - (nd00.xyz + nd10.xyz)) * 0.5;

    // Expanded one-cell neighborhood context.
    let p_left = vec2<i32>(max(base_x - 2, 0), base_y + 2);
    let p_right = vec2<i32>(min(base_x + 6, tex_w - 1), base_y + 2);
    let p_top = vec2<i32>(base_x + 2, max(base_y - 2, 0));
    let p_bot = vec2<i32>(base_x + 2, min(base_y + 6, tex_h - 1));

    let nd_l = textureLoad(normal_depth_tex, p_left, 0);
    let nd_r = textureLoad(normal_depth_tex, p_right, 0);
    let nd_t = textureLoad(normal_depth_tex, p_top, 0);
    let nd_b = textureLoad(normal_depth_tex, p_bot, 0);

    let center_meta = textureLoad(metadata_tex, vec2<i32>(base_x + 2, base_y + 2), 0) * 255.0;
    let left_meta = textureLoad(metadata_tex, p_left, 0) * 255.0;
    let right_meta = textureLoad(metadata_tex, p_right, 0) * 255.0;
    let top_meta = textureLoad(metadata_tex, p_top, 0) * 255.0;
    let bottom_meta = textureLoad(metadata_tex, p_bot, 0) * 255.0;
    let material_id = u32(center_meta.r + 0.5);
    let entity_id = u32(center_meta.g + 0.5);
    let entity_class = u32(center_meta.b + 0.5);
    let entity_edge_x = entity_id != 0u
        && (u32(left_meta.g + 0.5) != entity_id || u32(right_meta.g + 0.5) != entity_id);
    let entity_edge_y = entity_id != 0u
        && (u32(top_meta.g + 0.5) != entity_id || u32(bottom_meta.g + 0.5) != entity_id);

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

    if (!is_edge) {
        let luma = dot(avg_color.rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
        let dither = bayer4(x, y) * 0.05;
        // The reference keeps most of the distant world as negative space.
        // Device depth is concentrated near 1.0 under perspective, so use a
        // calibrated aerial-void gate for world geometry while never fading
        // entity metadata (actors remain legible in the distance).
        let far_void = entity_id == 0u && avg_nd.w > 0.9965;
        var depth_luma = luma;
        if (far_void) {
            depth_luma = luma * clamp((0.9995 - avg_nd.w) * 20.0, 0.0, 1.0);
        }
        let dithered_luma = clamp(depth_luma + dither, 0.0, 1.0);
        var band = 0u;
        if (dithered_luma < 0.04) {
            band = 0u;
        } else if (dithered_luma < 0.10) {
            band = 1u;
        } else if (dithered_luma < 0.18) {
            band = 2u;
        } else if (dithered_luma < 0.28) {
            band = 3u;
        } else if (dithered_luma < 0.40) {
            band = 3u;
        } else if (dithered_luma < 0.54) {
            band = 4u;
        } else if (dithered_luma < 0.68) {
            band = 4u;
        } else if (dithered_luma < 0.82) {
            band = 5u;
        } else if (dithered_luma < 0.92) {
            band = 5u;
        } else {
            band = 5u;
        }
        glyph = material_glyph(material_id, band);
        if (far_void && dithered_luma < 0.12) {
            glyph = 32u;
        } else if (dithered_luma >= 0.04) {
            glyph = shape_glyph(material_id, base_x, base_y);
        }
    }

    // Entity-ID outlines win over physical edges at actor silhouettes.
    if (entity_edge_x || entity_edge_y) {
        if (entity_edge_x && entity_edge_y) {
            glyph = 43u;
        } else if (entity_edge_x) {
            glyph = 124u;
        } else {
            glyph = 95u;
        }
        is_edge = true;
    }

    // textureLoad from the sRGB target is averaged in linear space. Apply a
    // filmic shoulder, convert to display space, then pull the result toward a
    // compact dark-fantasy palette to reduce ANSI color churn.
    let mapped_color = avg_color.rgb / (vec3<f32>(1.0) + avg_color.rgb);
    let display_color = pow(max(mapped_color, vec3<f32>(0.0)), vec3<f32>(0.4545));
    let palette_color = floor(display_color * 7.0 + vec3<f32>(0.5)) / 7.0;
    var fg_rgb = clamp(mix(display_color, palette_color, 0.35) * config.contrast_boost, vec3<f32>(0.0), vec3<f32>(1.0));
    var bg_rgb = clamp(palette_color * 0.12, vec3<f32>(0.0), vec3<f32>(1.0));
    if (entity_edge_x || entity_edge_y) {
        if (entity_class == 1u) { fg_rgb = vec3<f32>(0.60, 1.00, 0.35); }
        if (entity_class == 2u) { fg_rgb = vec3<f32>(1.00, 0.28, 0.16); }
        if (entity_class == 3u) { fg_rgb = vec3<f32>(1.00, 0.78, 0.25); }
        if (entity_class == 4u) { fg_rgb = vec3<f32>(0.35, 0.85, 1.00); }
        bg_rgb = fg_rgb * 0.08;
    }

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

#[cfg(test)]
mod tests {
    use super::{ASCII_POST_WGSL, SCENE_3D_WGSL};

    #[test]
    fn scene_shader_preserves_entity_metadata_for_post_processing() {
        assert!(SCENE_3D_WGSL.contains("out.entity_id = entity_id;"));
        assert!(SCENE_3D_WGSL.contains("out.entity_class = entity_class;"));
        assert!(
            SCENE_3D_WGSL.contains("pack_metadata(in.material_id, in.entity_id, in.entity_class)")
        );
        assert!(ASCII_POST_WGSL.contains("entity_edge_x"));
        assert!(ASCII_POST_WGSL.contains("entity_class == 1u"));
        assert!(ASCII_POST_WGSL.contains("material == 0u"));
        assert!(ASCII_POST_WGSL.contains("solid block of '@'"));
    }
}
