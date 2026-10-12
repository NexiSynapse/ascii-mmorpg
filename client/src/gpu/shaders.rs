//! Embedded WGSL shaders for 3D scene rasterization and ASCII conversion post-processing.

/// 3D Scene Rasterization Shader: Evaluates Blinn-Phong lighting, multiple point lights,
/// specular highlights, atmospheric depth fog, and encodes Color, Normal/Depth, and Material/Entity.
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
    point_lights: array<PointLight, 16>,
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

    // Only elevated foliage receives vertex displacement. Meadow terrain uses material 3
    // for glyph-level grass animation in the compute pass, but must remain stationary.
    if (mat_id >= 2.8 && mat_id <= 3.2 && world_pos4.y > 1.2) {
        let wind_intensity = min((world_pos4.y - 1.2) * 0.045, 0.22);
        let sway = sin(camera.time * 1.5 + world_pos4.x * 0.45 + world_pos4.z * 0.35) * wind_intensity;
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

    // Evaluate dynamic point lights (torches, campfires, entity lighting pools)
    let light_count = min(lighting.num_point_lights, 16u);
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
    let fog_end = 38.0;
    let fog_factor = clamp((dist_to_cam - fog_start) / (fog_end - fog_start), 0.0, 1.0);
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

fn hash2(x: u32, y: u32) -> u32 {
    var h = x * 374761393u + y * 668265263u;
    h = (h ^ (h >> 13u)) * 1274126177u;
    return h ^ (h >> 16u);
}

fn material_glyph(material: u32, band: u32, x: u32, y: u32) -> u32 {
    let v = hash2(x, y) % 4u;
    if (material == 0u) { // terrain: keep the ground readable without a solid block of '@'
        if (band == 0u) { return 32u; }  // ' '
        if (band == 1u) {
            if (v == 0u) { return 46u; }  // '.'
            if (v == 1u) { return 44u; }  // ','
            if (v == 2u) { return 39u; }  // '\''
            return 96u;                   // '`'
        }
        if (band == 2u) {
            if (v == 0u) { return 58u; }  // ':'
            if (v == 1u) { return 59u; }  // ';'
            if (v == 2u) { return 45u; }  // '-'
            return 126u;                  // '~'
        }
        if (band == 3u) {
            if (v == 0u) { return 42u; }  // '*'
            if (v == 1u) { return 43u; }  // '+'
            if (v == 2u) { return 61u; }  // '='
            return 34u;                   // '"'
        }
        if (band == 4u) {
            if (v == 0u) { return 61u; }  // '='
            if (v == 1u) { return 99u; }  // 'c'
            if (v == 2u) { return 115u; } // 's'
            return 114u;                  // 'r'
        }
        if (band == 5u) {
            if (v == 0u) { return 111u; } // 'o'
            if (v == 1u) { return 117u; } // 'u'
            if (v == 2u) { return 110u; } // 'n'
            return 120u;                  // 'x'
        }
        if (band == 6u) {
            if (v == 0u) { return 79u; }  // 'O'
            if (v == 1u) { return 48u; }  // '0'
            if (v == 2u) { return 83u; }  // 'S'
            return 68u;                   // 'D'
        }
        if (band == 7u) {
            if (v == 0u) { return 37u; }  // '%'
            if (v == 1u) { return 36u; }  // '$'
            if (v == 2u) { return 88u; }  // 'X'
            return 72u;                   // 'H'
        }
        if (band == 8u) {
            if (v == 0u) { return 35u; }  // '#'
            if (v == 1u) { return 77u; }  // 'M'
            if (v == 2u) { return 87u; }  // 'W'
            return 38u;                   // '&'
        }
        if (v == 0u) { return 64u; }      // '@'
        if (v == 1u) { return 35u; }      // '#'
        if (v == 2u) { return 37u; }      // '%'
        return 66u;                       // 'B'
    }
    if (material == 1u) { // metal: hard creases, rivets, and burnished plate
        if (band == 0u) { return 32u; }
        if (band == 1u) {
            if (v == 0u) { return 45u; }  // '-'
            if (v == 1u) { return 46u; }  // '.'
            if (v == 2u) { return 39u; }  // '\''
            return 124u;                  // '|'
        }
        if (band == 2u) {
            if (v == 0u) { return 61u; }  // '='
            if (v == 1u) { return 43u; }  // '+'
            if (v == 2u) { return 45u; }  // '-'
            return 126u;                  // '~'
        }
        if (band == 3u) {
            if (v == 0u) { return 43u; }  // '+'
            if (v == 1u) { return 42u; }  // '*'
            if (v == 2u) { return 120u; } // 'x'
            return 47u;                   // '/'
        }
        if (band == 4u) {
            if (v == 0u) { return 73u; }  // 'I'
            if (v == 1u) { return 84u; }  // 'T'
            if (v == 2u) { return 72u; }  // 'H'
            return 61u;                   // '='
        }
        if (band == 5u) {
            if (v == 0u) { return 88u; }  // 'X'
            if (v == 1u) { return 90u; }  // 'Z'
            if (v == 2u) { return 78u; }  // 'N'
            return 75u;                   // 'K'
        }
        if (band == 6u) {
            if (v == 0u) { return 35u; }  // '#'
            if (v == 1u) { return 36u; }  // '$'
            if (v == 2u) { return 38u; }  // '&'
            return 77u;                   // 'M'
        }
        if (v == 0u) { return 64u; }      // '@'
        if (v == 1u) { return 35u; }      // '#'
        if (v == 2u) { return 37u; }      // '%'
        return 87u;                       // 'W'
    }
    if (material == 2u) { // cloth: flowing drapes, weave texture, and hem embroidery
        if (band == 0u) { return 32u; }
        if (band == 1u) {
            if (v == 0u) { return 46u; }  // '.'
            if (v == 1u) { return 44u; }  // ','
            if (v == 2u) { return 39u; }  // '\''
            return 96u;                   // '`'
        }
        if (band == 2u) {
            if (v == 0u) { return 126u; } // '~'
            if (v == 1u) { return 45u; }  // '-'
            if (v == 2u) { return 115u; } // 's'
            return 114u;                  // 'r'
        }
        if (band == 3u) {
            if (v == 0u) { return 126u; } // '~'
            if (v == 1u) { return 122u; } // 'z'
            if (v == 2u) { return 99u; }  // 'c'
            return 61u;                   // '='
        }
        if (band == 4u) {
            if (v == 0u) { return 115u; } // 's'
            if (v == 1u) { return 83u; }  // 'S'
            if (v == 2u) { return 101u; } // 'e'
            return 97u;                   // 'a'
        }
        if (band == 5u) {
            if (v == 0u) { return 36u; }  // '$'
            if (v == 1u) { return 56u; }  // '8'
            if (v == 2u) { return 83u; }  // 'S'
            return 66u;                   // 'B'
        }
        if (band == 6u) {
            if (v == 0u) { return 37u; }  // '%'
            if (v == 1u) { return 38u; }  // '&'
            if (v == 2u) { return 35u; }  // '#'
            return 79u;                   // 'O'
        }
        if (v == 0u) { return 64u; }      // '@'
        if (v == 1u) { return 35u; }      // '#'
        if (v == 2u) { return 37u; }      // '%'
        return 87u;                       // 'W'
    }
    if (material == 3u) { // foliage & meadow grass: animated wind sway, organic blades, leaves
        // Wind current phase across the terrain: slow rolling wave left and right
        let wind_val = sin(config.time * 0.95 + f32(x) * 0.08 + f32(y) * 0.05);
        if (band == 0u) { return 32u; }  // ' '
        if (band == 1u) {
            if (wind_val > 0.35) {
                if (v == 0u) { return 47u; }  // '/' leaning right
                if (v == 1u) { return 39u; }  // '\''
                return 46u;                   // '.'
            } else if (wind_val < -0.35) {
                if (v == 0u) { return 92u; }  // '\' leaning left
                if (v == 1u) { return 44u; }  // ','
                return 96u;                   // '`'
            } else {
                if (v == 0u) { return 44u; }  // ','
                if (v == 1u) { return 46u; }  // '.'
                return 39u;                   // '\''
            }
        }
        if (band == 2u) {
            if (wind_val > 0.35) {
                if (v == 0u) { return 47u; }  // '/'
                if (v == 1u) { return 121u; } // 'y'
                return 44u;                   // ','
            } else if (wind_val < -0.35) {
                if (v == 0u) { return 92u; }  // '\'
                if (v == 1u) { return 59u; }  // ';'
                return 44u;                   // ','
            } else {
                if (v == 0u) { return 59u; }  // ';'
                if (v == 1u) { return 118u; } // 'v'
                return 34u;                   // '"'
            }
        }
        if (band == 3u) {
            if (wind_val > 0.35) {
                if (v == 0u) { return 47u; }  // '/'
                if (v == 1u) { return 121u; } // 'y'
                return 42u;                   // '*'
            } else if (wind_val < -0.35) {
                if (v == 0u) { return 92u; }  // '\'
                if (v == 1u) { return 118u; } // 'v'
                return 119u;                  // 'w'
            } else {
                if (v == 0u) { return 42u; }  // '*'
                if (v == 1u) { return 118u; } // 'v'
                return 119u;                  // 'w'
            }
        }
        if (band == 4u) {
            if (v == 0u) { return 111u; } // 'o'
            if (v == 1u) { return 119u; } // 'w'
            if (v == 2u) { return 109u; } // 'm'
            return 110u;                  // 'n'
        }
        if (band == 5u) {
            if (v == 0u) { return 37u; }  // '%'
            if (v == 1u) { return 38u; }  // '&'
            if (v == 2u) { return 56u; }  // '8'
            return 87u;                   // 'W'
        }
        if (band == 6u) {
            if (v == 0u) { return 38u; }  // '&'
            if (v == 1u) { return 87u; }  // 'W'
            if (v == 2u) { return 77u; }  // 'M'
            return 66u;                   // 'B'
        }
        if (v == 0u) { return 35u; }      // '#'
        if (v == 1u) { return 64u; }      // '@'
        if (v == 2u) { return 77u; }      // 'M'
        return 87u;                       // 'W'
    }
    if (material == 4u) { // emissive: fire, flickering torches, sparks, and glowing crystals
        if (band == 0u) { return 32u; }
        if (band == 1u) {
            if (v == 0u) { return 46u; }  // '.'
            if (v == 1u) { return 39u; }  // '\''
            if (v == 2u) { return 94u; }  // '^'
            return 96u;                   // '`'
        }
        if (band == 2u) {
            if (v == 0u) { return 42u; }  // '*'
            if (v == 1u) { return 43u; }  // '+'
            if (v == 2u) { return 94u; }  // '^'
            return 33u;                   // '!'
        }
        if (band == 3u) {
            if (v == 0u) { return 111u; } // 'o'
            if (v == 1u) { return 42u; }  // '*'
            if (v == 2u) { return 120u; } // 'x'
            return 43u;                   // '+'
        }
        if (band == 4u) {
            if (v == 0u) { return 79u; }  // 'O'
            if (v == 1u) { return 65u; }  // 'A'
            if (v == 2u) { return 111u; } // 'o'
            return 42u;                   // '*'
        }
        if (band == 5u) {
            if (v == 0u) { return 48u; }  // '0'
            if (v == 1u) { return 64u; }  // '@'
            if (v == 2u) { return 38u; }  // '&'
            return 42u;                   // '*'
        }
        if (v == 0u) { return 64u; }      // '@'
        if (v == 1u) { return 35u; }      // '#'
        if (v == 2u) { return 37u; }      // '%'
        return 42u;                       // '*'
    }
    if (material == 5u) { // weather: sparse points and directional streaks
        if (band == 0u) { return 32u; }
        if (band == 1u) {
            if (v == 0u) { return 46u; }  // '.'
            if (v == 1u) { return 44u; }  // ','
            if (v == 2u) { return 39u; }  // '\''
            return 58u;                   // ':'
        }
        if (band == 2u) {
            if (v == 0u) { return 47u; }  // '/'
            if (v == 1u) { return 92u; }  // '\\'
            if (v == 2u) { return 124u; } // '|'
            return 33u;                   // '!'
        }
        if (band == 3u) {
            if (v == 0u) { return 47u; }  // '/'
            if (v == 1u) { return 124u; } // '|'
            if (v == 2u) { return 92u; }  // '\\'
            return 42u;                   // '*'
        }
        if (band == 4u) {
            if (v == 0u) { return 124u; } // '|'
            if (v == 1u) { return 47u; }  // '/'
            if (v == 2u) { return 33u; }  // '!'
            return 42u;                   // '*'
        }
        return 47u;                       // '/'
    }
    if (material == 6u) { // water: horizontal ripple and wave language
        if (band == 0u) { return 32u; }
        if (band == 1u) {
            if (v == 0u) { return 46u; }  // '.'
            if (v == 1u) { return 126u; } // '~'
            if (v == 2u) { return 45u; }  // '-'
            return 96u;                   // '`'
        }
        if (band == 2u) {
            if (v == 0u) { return 126u; } // '~'
            if (v == 1u) { return 45u; }  // '-'
            if (v == 2u) { return 99u; }  // 'c'
            return 115u;                  // 's'
        }
        if (band == 3u) {
            if (v == 0u) { return 126u; } // '~'
            if (v == 1u) { return 61u; }  // '='
            if (v == 2u) { return 45u; }  // '-'
            return 94u;                   // '^'
        }
        if (v == 0u) { return 61u; }      // '='
        if (v == 1u) { return 126u; }     // '~'
        if (v == 2u) { return 42u; }      // '*'
        return 99u;                       // 'c'
    }
    if (material == 7u) { // wood / timber trunk: vertical bark striations, grain, and timber ridges
        if (band == 0u) { return 32u; }
        if (band == 1u) {
            if (v == 0u) { return 124u; } // '|'
            if (v == 1u) { return 58u; }  // ':'
            if (v == 2u) { return 33u; }  // '!'
            return 46u;                   // '.'
        }
        if (band == 2u) {
            if (v == 0u) { return 124u; } // '|'
            if (v == 1u) { return 73u; }  // 'I'
            if (v == 2u) { return 33u; }  // '!'
            return 91u;                   // '['
        }
        if (band == 3u) {
            if (v == 0u) { return 124u; } // '|'
            if (v == 1u) { return 72u; }  // 'H'
            if (v == 2u) { return 84u; }  // 'T'
            return 93u;                   // ']'
        }
        if (band == 4u) {
            if (v == 0u) { return 72u; }  // 'H'
            if (v == 1u) { return 61u; }  // '='
            if (v == 2u) { return 35u; }  // '#'
            return 73u;                   // 'I'
        }
        if (band == 5u) {
            if (v == 0u) { return 35u; }  // '#'
            if (v == 1u) { return 56u; }  // '8'
            if (v == 2u) { return 72u; }  // 'H'
            return 66u;                   // 'B'
        }
        if (v == 0u) { return 35u; }      // '#'
        if (v == 1u) { return 56u; }      // '8'
        if (v == 2u) { return 66u; }      // 'B'
        return 77u;                       // 'M'
    }
    // High-Fidelity Ansenjeo Volumetric Ramp fallback
    if (band == 0u) { return 32u; }  // ' '
    if (band == 1u) {
        if (v == 0u) { return 46u; }  // '.'
        if (v == 1u) { return 44u; }  // ','
        if (v == 2u) { return 39u; }  // '\''
        return 96u;                   // '`'
    }
    if (band == 2u) {
        if (v == 0u) { return 58u; }  // ':'
        if (v == 1u) { return 59u; }  // ';'
        if (v == 2u) { return 45u; }  // '-'
        return 126u;                  // '~'
    }
    if (band == 3u) {
        if (v == 0u) { return 42u; }  // '*'
        if (v == 1u) { return 43u; }  // '+'
        if (v == 2u) { return 61u; }  // '='
        return 34u;                   // '"'
    }
    if (band == 4u) {
        if (v == 0u) { return 61u; }  // '='
        if (v == 1u) { return 99u; }  // 'c'
        if (v == 2u) { return 115u; } // 's'
        return 114u;                  // 'r'
    }
    if (band == 5u) {
        if (v == 0u) { return 111u; } // 'o'
        if (v == 1u) { return 117u; } // 'u'
        if (v == 2u) { return 110u; } // 'n'
        return 101u;                  // 'e'
    }
    if (band == 6u) {
        if (v == 0u) { return 79u; }  // 'O'
        if (v == 1u) { return 48u; }  // '0'
        if (v == 2u) { return 83u; }  // 'S'
        return 68u;                   // 'D'
    }
    if (band == 7u) {
        if (v == 0u) { return 37u; }  // '%'
        if (v == 1u) { return 36u; }  // '$'
        if (v == 2u) { return 88u; }  // 'X'
        return 72u;                   // 'H'
    }
    if (band == 8u) {
        if (v == 0u) { return 35u; }  // '#'
        if (v == 1u) { return 77u; }  // 'M'
        if (v == 2u) { return 87u; }  // 'W'
        return 38u;                   // '&'
    }
    if (v == 0u) { return 64u; }      // '@'
    if (v == 1u) { return 35u; }      // '#'
    if (v == 2u) { return 37u; }      // '%'
    return 66u;                       // 'B'
}

fn luma_at(x: i32, y: i32) -> f32 {
    let rgb = textureLoad(color_tex, vec2<i32>(x, y), 0).rgb;
    return dot(rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
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

fn shape_glyph(material: u32, base_x: i32, base_y: i32, x: u32, y: u32, fallback_glyph: u32) -> u32 {
    let s0 = shape_region(base_x, base_y, 0u);
    let s1 = shape_region(base_x, base_y, 1u);
    let s2 = shape_region(base_x, base_y, 2u);
    let s3 = shape_region(base_x, base_y, 3u);
    let s4 = shape_region(base_x, base_y, 4u);
    let s5 = shape_region(base_x, base_y, 5u);
    let peak = max(max(max(s0, s1), max(s2, s3)), max(s4, s5));
    let trough = min(min(min(s0, s1), min(s2, s3)), min(s4, s5));

    // When the sub-pixel region has low directional contrast, preserve the volumetric texture glyph!
    if (peak - trough < 0.16) {
        return fallback_glyph;
    }

    let top_luma = (s0 + s1) * 0.5;
    let mid_luma = (s2 + s3) * 0.5;
    let bot_luma = (s4 + s5) * 0.5;
    let left_luma = (s0 + s2 + s4) * 0.3333;
    let right_luma = (s1 + s3 + s5) * 0.3333;

    let diff_y = abs(top_luma - bot_luma);
    let diff_x = abs(left_luma - right_luma);
    let d1 = (s0 + s3 + s5) * 0.3333;
    let d2 = (s1 + s2 + s4) * 0.3333;
    let diff_diag = abs(d1 - d2);

    // Strong horizontal stroke
    if (diff_y > diff_x * 1.5 && diff_y > 0.22) {
        if (top_luma > bot_luma) {
            return 45u; // '-'
        } else {
            return 95u; // '_'
        }
    }

    // Strong vertical stroke
    if (diff_x > diff_y * 1.5 && diff_x > 0.22) {
        return 124u; // '|'
    }

    // Strong diagonal stroke
    if (diff_diag > 0.24) {
        if (d1 > d2) {
            return 92u; // '\'
        } else {
            return 47u; // '/'
        }
    }

    // Centered bright accent
    let center_diff = mid_luma - (top_luma + bot_luma) * 0.5;
    if (center_diff > 0.24) {
        return 43u; // '+'
    }

    return fallback_glyph;
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
        let edge_hash = hash2(x, y) % 3u;
        if (edge_y > edge_x * 1.6) {
            let is_top_crease = nd_t.w > avg_nd.w;
            if (is_top_crease) {
                if (edge_hash == 0u) { glyph = 45u; } // '-'
                else if (edge_hash == 1u) { glyph = 61u; } // '='
                else { glyph = 126u; } // '~'
            } else {
                glyph = 95u; // '_'
            }
        } else if (edge_x > edge_y * 1.6) {
            if (edge_hash == 0u) { glyph = 124u; } // '|'
            else if (edge_hash == 1u) { glyph = 33u; } // '!'
            else { glyph = 73u; } // 'I'
        } else if (edge_x > 0.22 && edge_y > 0.22) {
            let diag1 = abs(nd00.w - nd11.w);
            let diag2 = abs(nd10.w - nd01.w);
            if (diag1 < diag2) {
                glyph = 47u; // '/'
            } else {
                glyph = 92u; // '\'
            }
        } else {
            if (edge_hash == 0u) { glyph = 43u; } // '+'
            else if (edge_hash == 1u) { glyph = 42u; } // '*'
            else { glyph = 120u; } // 'x'
        }
    }

    if (!is_edge) {
        let luma = dot(avg_color.rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
        let dither = bayer4(x, y) * 0.05;

        // True aerial horizon void gate: only gates the extreme sky/map boundary approaching depth 1.0 (>= 0.9996).
        // Preserves 100% of visible terrain, foliage, and scenery across the top half of the screen.
        let far_void = entity_id == 0u && avg_nd.w > 0.9996;
        var depth_luma = luma;
        if (far_void) {
            depth_luma = luma * clamp((1.0 - avg_nd.w) * 2500.0, 0.0, 1.0);
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
            band = 4u;
        } else if (dithered_luma < 0.54) {
            band = 5u;
        } else if (dithered_luma < 0.68) {
            band = 6u;
        } else if (dithered_luma < 0.82) {
            band = 7u;
        } else if (dithered_luma < 0.92) {
            band = 8u;
        } else {
            band = 9u;
        }

        let volumetric_glyph = material_glyph(material_id, band, x, y);
        if (far_void && dithered_luma < 0.08) {
            glyph = 32u;
        } else if (dithered_luma >= 0.04) {
            glyph = shape_glyph(material_id, base_x, base_y, x, y, volumetric_glyph);
        } else {
            glyph = volumetric_glyph;
        }
    }

    // Entity-ID outlines win over physical edges at actor silhouettes.
    if (entity_edge_x || entity_edge_y) {
        let is_left_edge = u32(left_meta.g + 0.5) != entity_id;
        let is_right_edge = u32(right_meta.g + 0.5) != entity_id;
        let is_top_edge = u32(top_meta.g + 0.5) != entity_id;
        let is_bot_edge = u32(bottom_meta.g + 0.5) != entity_id;

        if (entity_edge_x && entity_edge_y) {
            if (is_top_edge && is_left_edge) {
                glyph = 47u; // '/'
            } else if (is_top_edge && is_right_edge) {
                glyph = 92u; // '\'
            } else if (is_bot_edge && is_left_edge) {
                glyph = 92u; // '\'
            } else if (is_bot_edge && is_right_edge) {
                glyph = 47u; // '/'
            } else {
                glyph = 43u; // '+'
            }
        } else if (entity_edge_x) {
            if (is_left_edge) {
                glyph = 40u; // '(' curved left flank
            } else if (is_right_edge) {
                glyph = 41u; // ')' curved right flank
            } else {
                glyph = 124u; // '|'
            }
        } else {
            if (is_top_edge) {
                glyph = 94u; // '^' top crest
            } else {
                glyph = 95u; // '_' bottom sill
            }
        }
        is_edge = true;
    }

    // Extended Reinhard tone mapping with contrast boost:
    // Preserves pure, deep dark-twilight backgrounds (no milky gamma lift) and vibrant saturated highlights
    let white_pt = 2.4;
    let tonemapped = (avg_color.rgb * (vec3<f32>(1.0) + avg_color.rgb / (white_pt * white_pt))) / (vec3<f32>(1.0) + avg_color.rgb);
    var fg_rgb = clamp(tonemapped * config.contrast_boost, vec3<f32>(0.0), vec3<f32>(1.0));
    var bg_rgb = clamp(tonemapped * 0.12, vec3<f32>(0.0), vec3<f32>(1.0));

    if (entity_edge_x || entity_edge_y) {
        if (entity_class == 1u) { fg_rgb = mix(fg_rgb, vec3<f32>(0.60, 1.00, 0.35), 0.40); }
        if (entity_class == 2u) { fg_rgb = mix(fg_rgb, vec3<f32>(1.00, 0.28, 0.16), 0.40); }
        if (entity_class == 3u) { fg_rgb = mix(fg_rgb, vec3<f32>(1.00, 0.78, 0.25), 0.40); }
        if (entity_class == 4u) { fg_rgb = mix(fg_rgb, vec3<f32>(0.35, 0.85, 1.00), 0.40); }
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

    #[test]
    fn terrain_material_keeps_vertex_position_stationary() {
        assert!(SCENE_3D_WGSL.contains("mat_id >= 2.8 && mat_id <= 3.2 && world_pos4.y > 1.2"));
        assert!(!SCENE_3D_WGSL.contains("Meadow grass slow synchronized wind"));
    }
}
