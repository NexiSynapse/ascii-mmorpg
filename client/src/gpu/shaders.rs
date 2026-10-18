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

    // Procedural water surface waves (material_id == 6.0: the southern ocean).
    // Two-layer directional swell: the long swell rolls toward the shore
    // (its phase grows with world z, so as time advances a crest travels in
    // -z, i.e. north, onto the beach), plus a shorter cross-chop. The
    // fragment pass rides the exact same phases, so geometry bob and the
    // foam/crest shading never desync.
    if (mat_id >= 5.8 && mat_id <= 6.2) {
        world_pos4.y += sin(camera.time * 2.2 + world_pos4.z * 0.55) * 0.14
            + sin(camera.time * 3.1 + world_pos4.x * 0.9) * 0.05;
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

    // Water surfaces: the southern ocean. One directional rolling swell
    // drives everything: crest brightness bands scroll toward the shore
    // (phase grows with world z, so a constant phase travels in -z toward
    // the beach as time advances), foam breaks white where the swell peaks,
    // and the base gets deeper further from the shoreline (mesh z = 28.0 is
    // WORLD_BOUNDS.max_y * 0.8, shared with the terrain generator).
    // Spec 0.9â†’0.6 and fresnel 0.35â†’0.10: the previous sky-reflection lift
    // held distant water in mid luminance bands, so the whole sea textured
    // as letter noise (user-reported "grass texture everywhere"); dark calm
    // water must resolve to empty cells so wave lines carry the picture.
    if (in.material_id >= 5.8 && in.material_id <= 6.2) {
        let swell = sin(camera.time * 2.2 + in.world_pos.z * 0.55 + in.world_pos.x * 0.25);
        let chop = sin(camera.time * 3.1 + in.world_pos.x * 0.9) * 0.06;
        let wave_norm = normalize(vec3<f32>(
            swell * 0.30,
            1.0,
            (cos(camera.time * 1.4 + in.world_pos.z * 0.35) + chop) * 0.16
        ));
        // Depth gradient: shallow turquoise just off the beach sinking to
        // dark open water to the south (z here is already * 0.8, so the
        // band starts at the shoreline and saturates ~17 world cells out).
        let depth_t = clamp((in.world_pos.z - 28.0) / 14.0, 0.0, 1.0);
        let water_base = mix(vec3<f32>(0.10, 0.28, 0.52), vec3<f32>(0.05, 0.15, 0.33), depth_t);
        let water_NdotL = max(dot(wave_norm, normalize(lighting.sun_dir.xyz)), 0.0);
        let water_H = normalize(normalize(lighting.sun_dir.xyz) + V);
        let water_spec = pow(max(dot(wave_norm, water_H), 0.0), 32.0) * 0.6;
        var water_color = water_base
            * ((lighting.ambient_color.rgb + lighting.sun_color.rgb * water_NdotL) * 1.6)
            + vec3<f32>(water_spec);
        let fresnel = pow(1.0 - max(dot(V, wave_norm), 0.0), 3.0) * 0.10;
        water_color += vec3<f32>(0.35, 0.50, 0.70) * fresnel;
        // Foam: whitecaps where the rolling swell tops out (plus a little
        // cross-chop so the crest line breaks up instead of ruling). Foam
        // stays fully opaque; the sea surface itself stays translucent.
        let foam = smoothstep(0.58, 0.92, swell + chop);
        water_color = mix(water_color, vec3<f32>(0.88, 0.94, 1.0), foam * 0.85);
        out.color = vec4<f32>(water_color, select(0.88, 1.0, foam > 0.15));
        out.normal_depth = vec4<f32>(wave_norm * 0.5 + 0.5, in.clip_position.z);
        out.metadata = pack_metadata(in.material_id, in.entity_id, in.entity_class);
        return out;
    }

    let N = normalize(in.world_normal);

    // Directional Sunlight / Moonlight.
    // Character form shading (entity fragments only): the sun's NdotL is
    // quantized into stepped bands so lit/shadow facets stay distinct at
    // terminal resolution â€” Harri's cel-shading separation applied to the
    // surface itself, kciter's discrete Shade ramp. Terrain and props
    // (entity_id 0) keep the smooth lighting of the quiet dark carpet.
    let L_sun = normalize(lighting.sun_dir.xyz);
    let NdotL = max(dot(N, L_sun), 0.0);
    let cel_steps = 4.0;
    let sun_ndl = select(NdotL, floor(NdotL * cel_steps) / cel_steps, in.entity_id > 0.5);
    var diffuse_light = lighting.ambient_color.rgb + lighting.sun_color.rgb * sun_ndl;

    // Specular highlight from sun
    let H_sun = normalize(L_sun + V);
    let spec_power = max(1.0 / (in.roughness * in.roughness + 0.001), 2.0);
    let NdotH = max(dot(N, H_sun), 0.0);
    var specular_light = pow(NdotH, spec_power) * (1.0 - in.roughness) * lighting.sun_color.rgb * 0.35;

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
            specular_light += pow(point_NdotH, spec_power) * light.color.rgb * (1.0 - in.roughness) * atten_sq * 0.45;
        }
    }

    // Character-model definition (entity fragments only):
    // kciter's headlight â€” "assuming the light comes from the direction you
    // are looking" â€” keeps camera-facing body surfaces (chest, face) from
    // collapsing into the shadow band, and a rim light lifts the grazing
    // silhouette off the dark ground.
    if (in.entity_id > 0.5) {
        let facing = max(dot(N, V), 0.0);
        // Trimmed pops (clearness study: the old 0.30/0.35 adds blew the
        // player model to a clipped near-white blob once the surrounding
        // carpet went reference-dark; the rim must keep the silhouette
        // lifted, not clip the body).
        diffuse_light += vec3<f32>(1.0, 0.95, 0.85) * pow(facing, 2.0) * 0.24;
    }

    // Final lit color
    var lit_color = in.color.rgb * diffuse_light + specular_light;

    if (in.entity_id > 0.5) {
        let rim = pow(1.0 - max(dot(N, V), 0.0), 3.0);
        lit_color += vec3<f32>(0.85, 0.90, 1.0) * rim * 0.28;
    }

    // Calm Atmospheric Distance Fog (near-black void falloff).
    // The reference horizon (Videos 1, 2, 5, 6) falls to pure empty ' '
    // beyond the midground: fog luma must sit safely below the 0.04 Band-0
    // gate, so distant terrain resolves to space instead of band-1
    // punctuation speckle across the horizon.
    let dist_to_cam = length(camera.camera_pos.xyz - in.world_pos);
    let fog_start = 16.0;
    let fog_end = 42.0;
    let fog_factor = clamp((dist_to_cam - fog_start) / (fog_end - fog_start), 0.0, 1.0);
    let fog_color = vec3<f32>(0.006, 0.009, 0.016);
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
    inv_view_proj: mat4x4<f32>,
    width: u32,
    height: u32,
    time: f32,
    contrast_boost: f32,
};

@group(0) @binding(4) var<uniform> config: ConfigUniform;

// Screen cell -> world position via the averaged device depth of that
// cell's SSAA taps (same unprojection Asciicker-style math the CPU side
// uses). Lets the glyph/dither hashes lock to world-governed coordinates.
fn unproject_world(x: u32, y: u32, depth: f32) -> vec3<f32> {
    let ndc_x = (f32(x) + 0.5) / f32(config.width) * 2.0 - 1.0;
    let ndc_y = 1.0 - (f32(y) + 0.5) / f32(config.height) * 2.0;
    let clip = vec4<f32>(ndc_x, ndc_y, depth, 1.0);
    let w = config.inv_view_proj * clip;
    return w.xyz / max(abs(w.w), 1e-6);
}

fn hash2(x: u32, y: u32) -> u32 {
    var h = x * 374761393u + y * 668265263u;
    h = (h ^ (h >> 13u)) * 1274126177u;
    return h ^ (h >> 16u);
}

// Quantized metadata id read (0-255 range after the *255 scale in
// cs_main): exact ids survive (n.0 -> n) while interpolated fringe blends
// at shared triangle edges round down instead of hallucinating neighbors.
fn qid(v: f32) -> u32 {
    return u32(floor(v + 0.25));
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
        // Bright bands stay quiet (strokes, then rounded cobblestone
        // glyphs for light pools) so letter-detailed actors and props
        // pop by contrast. Reference roads/stone (Videos 1, 2, 4, 5)
        // never use block-shade glyphs: torch-lit stone resolves as
        // rounded loop glyphs with dark gaps between stones, never an
        // ANSI progress bar.
        if (band == 6u) {
            if (v == 0u) { return 111u; } // 'o'
            if (v == 1u) { return 99u; }  // 'c'
            if (v == 2u) { return 48u; }  // '0'
            return 81u;                   // 'Q' cobble
        }
        if (band == 7u) {
            if (v == 0u) { return 48u; }  // '0'
            if (v == 1u) { return 81u; }  // 'Q'
            if (v == 2u) { return 111u; } // 'o'
            return 99u;                   // 'c'
        }
        if (band == 8u) {
            if (v == 0u) { return 81u; }  // 'Q'
            if (v == 1u) { return 79u; }  // 'O'
            if (v == 2u) { return 48u; }  // '0'
            return 111u;                  // 'o'
        }
        return 81u;                       // 'Q' torch-core stone
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
        // Sparse-tuft density gate (measured carpet gap, quiet-ground
        // contract): the stored G-buffer is sRGB-encoded, so every lit
        // meadow tile lands in bands 2-3 with one glyph per cell, and
        // the ground reads as a dense punctuation sheet instead of the
        // reference's near-black carpet with isolated textural tufts.
        // Bands 0-3 emit glyphs only on every 4th world-locked tile (the
        // tufts stay planted through camera pans); brighter lit pools
        // (band 4+) and the sea keep full texture.
        if (band <= 3u && hash2(x, y) % 4u != 0u) { return 32u; }
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
                if (v == 1u) { return 59u; }  // ';' (was 'y': too lettered)
                return 44u;                   // ','
            } else if (wind_val < -0.35) {
                if (v == 0u) { return 92u; }  // '\'
                if (v == 1u) { return 59u; }  // ';'
                return 44u;                   // ','
            } else {
                if (v == 0u) { return 59u; }  // ';'
                if (v == 1u) { return 118u; } // 'v' tuft
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
        // Bright foliage stays leafy (tips, glints, mass) instead of alphabet
        // blocks, so lit grass reads as meadow rather than text.
        if (band == 4u) {
            if (v == 0u) { return 118u; } // 'v' leaf tip
            if (v == 1u) { return 121u; } // 'y'
            if (v == 2u) { return 119u; } // 'w'
            return 34u;                   // '"'
        }
        if (band == 5u) {
            if (v == 0u) { return 42u; }  // '*'
            if (v == 1u) { return 120u; } // 'x'
            if (v == 2u) { return 43u; }  // '+'
            return 34u;                   // '"'
        }
        if (band == 6u) {
            if (v == 0u) { return 35u; }  // '#'
            if (v == 1u) { return 42u; }  // '*'
            if (v == 2u) { return 43u; }  // '+'
            return 37u;                   // '%'
        }
        if (v == 0u) { return 35u; }      // '#'
        if (v == 1u) { return 35u; }      // '#'
        if (v == 2u) { return 42u; }      // '*'
        return 35u;                       // '#'
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
    if (material == 6u) { // water: rolling wave language, quiet by design
        // The sea reads as a mostly-empty dark surface crossed by wave
        // strokes: calm water (bands 0-1) stays empty, band 2 rolls '~',
        // band 3 mixes '~'/'=', and the bright foam caps break '='/~/'*'.
        // Letters ('c' 's') and punctuation ('.','-','`','^') are gone:
        // per-cell letter dithering rasterized the whole ocean as dense
        // texture (user-reported "grass texture everywhere"), while wave
        // LINES need adjacent cells in a band to agree.
        if (band <= 1u) { return 32u; }
        if (band == 2u) { return 126u; }  // '~'
        if (band == 3u) {
            if (v == 0u) { return 126u; } // '~'
            return 61u;                   // '='
        }
        if (v == 0u) { return 61u; }      // '='
        if (v == 1u) { return 126u; }     // '~'
        return 42u;                       // '*'
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
    // High-Fidelity Ansenjeo Volumetric Ramp fallback (quiet like terrain:
    // strokes and shaded blocks, never alphabet soup).
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
        if (v == 1u) { return 45u; }  // '-'
        if (v == 2u) { return 126u; } // '~'
        return 58u;                   // ':'
    }
    if (band == 5u) {
        if (v == 0u) { return 43u; }  // '+'
        if (v == 1u) { return 61u; }  // '='
        if (v == 2u) { return 42u; }  // '*'
        return 111u;                  // 'o' pebble
    }
    if (band == 6u) {
        if (v == 0u) { return 9617u; } // 'â–‘'
        if (v == 1u) { return 9618u; } // 'â–’'
        if (v == 2u) { return 9617u; } // 'â–‘'
        return 9618u;                  // 'â–’'
    }
    if (band == 7u) {
        if (v == 0u) { return 9618u; } // 'â–’'
        if (v == 1u) { return 9619u; } // 'â–“'
        if (v == 2u) { return 9618u; } // 'â–’'
        return 9619u;                  // 'â–“'
    }
    if (band == 8u) {
        if (v == 0u) { return 9619u; } // 'â–“'
        if (v == 1u) { return 9608u; } // 'â–ˆ'
        if (v == 2u) { return 9619u; } // 'â–“'
        return 9608u;                  // 'â–ˆ'
    }
    return 9608u;                     // 'â–ˆ'
}

fn shape_glyph(
    // Six-component internal sampling vector: top/middle/bottom x left/right.
    s0_in: f32, s1_in: f32, s2_in: f32, s3_in: f32, s4_in: f32, s5_in: f32,
    // Widened external sampling vector: for each internal component the
    // max luminance across its affecting OUTWARD neighbor texels and
    // corners (Harri's widened directional table). Computed in cs_main.
    e0: f32, e1: f32, e2: f32, e3: f32, e4: f32, e5: f32,
    edge_mag: f32,
    dark_field_terrain: bool,
    smooth_slope: bool,
    water_surface: bool,
    edge_hash: u32,
    fallback_glyph: u32,
) -> u32 {
    // Reuse the 4x4 supersamples collected by cs_main instead of loading the
    // same 12 texels again. This is both cheaper and closer to a real shape
    // vector: each cell retains top/middle/bottom and left/right coverage.
    var s0 = s0_in;
    var s1 = s1_in;
    var s2 = s2_in;
    var s3 = s3_in;
    var s4 = s4_in;
    var s5 = s5_in;

    // Directional contrast enhancement (Harri's method): each component is
    // normalized against the brightest of itself and its AFFECTING outward
    // neighbors (widened table â€” see the externals in cs_main), so a bright
    // region just outside the cell pushes the inside edge darker and the
    // reach spans whole cell rows rather than one texel. Boundary strokes
    // resolve a cell earlier instead of staircasing. Runs before the global
    // pass, as in Harri's pipeline.
    let dir_power = 2.0;
    let m0 = max(s0, e0);
    let m1 = max(s1, e1);
    let m2 = max(s2, e2);
    let m3 = max(s3, e3);
    let m4 = max(s4, e4);
    let m5 = max(s5, e5);
    if (m0 > 0.001) { s0 = m0 * pow(clamp(s0 / m0, 0.0, 1.0), dir_power); }
    if (m1 > 0.001) { s1 = m1 * pow(clamp(s1 / m1, 0.0, 1.0), dir_power); }
    if (m2 > 0.001) { s2 = m2 * pow(clamp(s2 / m2, 0.0, 1.0), dir_power); }
    if (m3 > 0.001) { s3 = m3 * pow(clamp(s3 / m3, 0.0, 1.0), dir_power); }
    if (m4 > 0.001) { s4 = m4 * pow(clamp(s4 / m4, 0.0, 1.0), dir_power); }
    if (m5 > 0.001) { s5 = m5 * pow(clamp(s5 / m5, 0.0, 1.0), dir_power); }

    let peak_in = max(max(max(s0, s1), max(s2, s3)), max(s4, s5));

    // Global contrast enhancement from Alex Harri's renderer. Normalize by
    // the brightest component before applying the exponent so flat gradients
    // remain stable while boundaries become easier to read.
    if (peak_in > 0.001) {
        let contrast_power = 1.22;
        s0 = peak_in * pow(clamp(s0 / peak_in, 0.0, 1.0), contrast_power);
        s1 = peak_in * pow(clamp(s1 / peak_in, 0.0, 1.0), contrast_power);
        s2 = peak_in * pow(clamp(s2 / peak_in, 0.0, 1.0), contrast_power);
        s3 = peak_in * pow(clamp(s3 / peak_in, 0.0, 1.0), contrast_power);
        s4 = peak_in * pow(clamp(s4 / peak_in, 0.0, 1.0), contrast_power);
        s5 = peak_in * pow(clamp(s5 / peak_in, 0.0, 1.0), contrast_power);
    }

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

    // Strokes belong on geometric discontinuities, not smooth shading
    // gradients: a fogged ground plane has a steady vertical luminance
    // slope that otherwise resolves to full-width '-' rules. The Sobel
    // edge magnitude (depth + normal jumps) separates the two â€” far-field
    // gradients score ~0.01, true silhouettes score 1+ â€” so strokes require
    // a minimum, and smooth slopes (steady coarse AND fine gradients, e.g.
    // the nearest-ground strip) are excluded outright. Volumetric texture
    // keeps the quiet areas.
    // Dark entity-0 terrain should not turn weak heightfield curvature into
    // a punctuation sheet. Keep the normal edge threshold for lit terrain
    // and actors, but demand a materially stronger edge in the dark field
    // (local luma < 0.08 sRGB, selected by cs_main) so only real contours
    // survive the stroke path.
    let stroke_edge_gate = select(0.12, 1.0, dark_field_terrain);
    if (edge_mag > stroke_edge_gate && !smooth_slope) {
        // Water never takes stroke overrides: the wobble normals of the
        // rolling swell are shading, not silhouettes, so '/' '\' '|'
        // creases speckle the open sea as noise. Wave language stays in
        // the volumetric ramp above.
        if (!water_surface) {
            // Strong horizontal stroke (ragged-rule guard: every third cell
            // keeps volumetric texture, mirroring the is_edge path).
            // Marginal bands stay texture: the stroke needs real band contrast,
            // not just a directional tilt, or treelines flicker as rules.
            // The bottom case ('_' when the top is dimmer) is excluded
            // entirely: full-cell underscores disconnect and rule the screen,
            // so those cells resolve as volumetric texture instead.
            if (diff_y > diff_x * 1.5 && diff_y > 0.22 && (peak - trough) > 0.24) {
                if (edge_hash == 2u) {
                    return fallback_glyph;
                }
                if (top_luma > bot_luma) {
                    return 45u; // '-'
                } else {
                    return fallback_glyph; // never '_': see above
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
        }
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
    var shape_s0 = 0.0;
    var shape_s1 = 0.0;
    var shape_s2 = 0.0;
    var shape_s3 = 0.0;
    var shape_s4 = 0.0;
    var shape_s5 = 0.0;
    for (var sx = 0; sx < 4; sx = sx + 1) {
        for (var sy = 0; sy < 4; sy = sy + 1) {
            let sample_pos = vec2<i32>(base_x + sx, base_y + sy);
            let sample_color = textureLoad(color_tex, sample_pos, 0);
            avg_color = avg_color + sample_color;
            avg_nd = avg_nd + textureLoad(normal_depth_tex, sample_pos, 0);

            // Build the six-component shape vector from the same samples used
            // for SSAA. The previous version called textureLoad 12 additional
            // times per cell from shape_glyph; this keeps the quality while
            // eliminating those redundant reads.
            let sample_luma = dot(sample_color.rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
            if (sy == 0) {
                if (sx < 2) { shape_s0 = shape_s0 + sample_luma; }
                else { shape_s1 = shape_s1 + sample_luma; }
            } else if (sy == 1 || sy == 2) {
                if (sx < 2) { shape_s2 = shape_s2 + sample_luma; }
                else { shape_s3 = shape_s3 + sample_luma; }
            } else {
                if (sx < 2) { shape_s4 = shape_s4 + sample_luma; }
                else { shape_s5 = shape_s5 + sample_luma; }
            }
        }
    }
    avg_color = avg_color * 0.0625;
    avg_nd = avg_nd * 0.0625;
    shape_s0 = shape_s0 * 0.5;
    shape_s1 = shape_s1 * 0.5;
    shape_s2 = shape_s2 * 0.25;
    shape_s3 = shape_s3 * 0.25;
    shape_s4 = shape_s4 * 0.5;
    shape_s5 = shape_s5 * 0.5;

    // Directional contrast externals (Harri's method): one neighbor-region
    // luminance per shape component, sampled just outside the cell in that
    // component's direction. Border cells clamp to their own edge texels,
    // which degrades to a no-op there instead of reading out of bounds.
    let luma_tex = vec3<f32>(0.2126, 0.7152, 0.0722);
    let ext_top = max(base_y - 1, 0);
    let ext_bot = min(base_y + 4, tex_h - 1);
    let ext_left = max(base_x - 1, 0);
    let ext_right = min(base_x + 4, tex_w - 1);
    let ext0 = (dot(textureLoad(color_tex, vec2<i32>(base_x, ext_top), 0).rgb, luma_tex)
        + dot(textureLoad(color_tex, vec2<i32>(base_x + 1, ext_top), 0).rgb, luma_tex)) * 0.5;
    let ext1 = (dot(textureLoad(color_tex, vec2<i32>(base_x + 2, ext_top), 0).rgb, luma_tex)
        + dot(textureLoad(color_tex, vec2<i32>(base_x + 3, ext_top), 0).rgb, luma_tex)) * 0.5;
    let ext2 = (dot(textureLoad(color_tex, vec2<i32>(ext_left, base_y + 1), 0).rgb, luma_tex)
        + dot(textureLoad(color_tex, vec2<i32>(ext_left, base_y + 2), 0).rgb, luma_tex)) * 0.5;
    let ext3 = (dot(textureLoad(color_tex, vec2<i32>(ext_right, base_y + 1), 0).rgb, luma_tex)
        + dot(textureLoad(color_tex, vec2<i32>(ext_right, base_y + 2), 0).rgb, luma_tex)) * 0.5;
    let ext4 = (dot(textureLoad(color_tex, vec2<i32>(base_x, ext_bot), 0).rgb, luma_tex)
        + dot(textureLoad(color_tex, vec2<i32>(base_x + 1, ext_bot), 0).rgb, luma_tex)) * 0.5;
    let ext5 = (dot(textureLoad(color_tex, vec2<i32>(base_x + 2, ext_bot), 0).rgb, luma_tex)
        + dot(textureLoad(color_tex, vec2<i32>(base_x + 3, ext_bot), 0).rgb, luma_tex)) * 0.5;

    // Widened directional reach (Harri's AFFECTING_EXTERNAL_INDICES spirit):
    // one texel per remaining edge segment plus the four outward corners.
    // Bright light just outside the cell now reaches deeper into the
    // sampling vector (top halves into the middle, middle halves into
    // top/bottom), which resolves boundaries a cell earlier and removes
    // the residual staircasing where the narrowed externals stop short.
    let ext_l_top = dot(textureLoad(color_tex, vec2<i32>(ext_left, base_y), 0).rgb, luma_tex);
    let ext_l_bot = dot(textureLoad(color_tex, vec2<i32>(ext_left, base_y + 3), 0).rgb, luma_tex);
    let ext_r_top = dot(textureLoad(color_tex, vec2<i32>(ext_right, base_y), 0).rgb, luma_tex);
    let ext_r_bot = dot(textureLoad(color_tex, vec2<i32>(ext_right, base_y + 3), 0).rgb, luma_tex);
    let ext_corner_ul = dot(textureLoad(color_tex, vec2<i32>(ext_left, ext_top), 0).rgb, luma_tex);
    let ext_corner_ur = dot(textureLoad(color_tex, vec2<i32>(ext_right, ext_top), 0).rgb, luma_tex);
    let ext_corner_ll = dot(textureLoad(color_tex, vec2<i32>(ext_left, ext_bot), 0).rgb, luma_tex);
    let ext_corner_lr = dot(textureLoad(color_tex, vec2<i32>(ext_right, ext_bot), 0).rgb, luma_tex);

    // Per-component max-of-affecting-externals, mirroring Harri's widened
    // table: both top circles reach both top components, the left-edge
    // column spans top+middle+bottom on that side, and the outward corner
    // gives the diagonal reach. Border cells clamp to their edge texels,
    // which keeps these extra reads no-ops instead of out-of-bounds.
    let e0c = max(max(ext0, ext1), max(ext_l_top, ext_corner_ul));
    let e1c = max(max(ext1, ext0), max(ext_r_top, ext_corner_ur));
    let e2c = max(max(ext2, ext0), max(ext_l_top, ext_l_bot));
    let e3c = max(max(ext3, ext1), max(ext_r_top, ext_r_bot));
    let e4c = max(max(ext4, ext5), max(ext_l_bot, ext_corner_ll));
    let e5c = max(max(ext5, ext4), max(ext_r_bot, ext_corner_lr));

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
    // Quantized id reads: interpolated metadata at shared triangle edges
    // yields fractional ids (0.5 reads as entity 1!), so fringe texels
    // hallucinate outlines. Floor-at-quarter keeps exact ids (n.0 -> n)
    // while rounding fringe blends down to the lower id.
    let material_id = qid(center_meta.r);
    let entity_id = qid(center_meta.g);
    let entity_class = qid(center_meta.b);
    let entity_edge_x = entity_id != 0u
        && (qid(left_meta.g) != entity_id || qid(right_meta.g) != entity_id);
    let entity_edge_y = entity_id != 0u
        && (qid(top_meta.g) != entity_id || qid(bottom_meta.g) != entity_id);

    // World-locked glyph hash ("shower-door" fix): the reference terrain is
    // glyph-locked to world tiles — sliding the camera by one cell must not
    // re-randomize a tile's volume glyph. Stationary geometry (entity_id 0)
    // unprojects its averaged depth to a world tile key; actors keep the
    // screen hash so their glyph detail tracks the pose.
    let wpos = unproject_world(x, y, avg_nd.w);
    let world_gx = u32(max(floor(wpos.x * 2.5) + 4096.0, 0.0));
    let world_gy = u32(max(floor(wpos.z * 2.5) + 4096.0, 0.0));
    let gx_hash = select(x, world_gx, entity_id == 0u);
    let gy_hash = select(y, world_gy, entity_id == 0u);

    let neigh_depth_x = abs(nd_r.w - nd_l.w);
    let neigh_depth_y = abs(nd_b.w - nd_t.w);
    let neigh_norm_x = length(nd_r.xyz - nd_l.xyz);
    let neigh_norm_y = length(nd_b.xyz - nd_t.xyz);

    let edge_x = max(depth_diff_x * 10.0 + normal_diff_x, neigh_depth_x * 6.0 + neigh_norm_x);
    let edge_y = max(depth_diff_y * 10.0 + normal_diff_y, neigh_depth_y * 6.0 + neigh_norm_y);
    let edge_mag = edge_x + edge_y;

    // Smooth-slope gate: a steep but steady gradient (the nearest-ground
    // strip at the viewport bottom, far fog terraces) is not an edge even
    // though its per-cell differences are large. The fine span covers 3
    // texels and the neighbor span 8, so a linear slope reads 8/3 ~= 2.67x
    // across coarse vs fine (a true step reads ~1x in both); the threshold
    // sits between at 2.0. Flat paint (light pools: no depth/normal slope
    // at all) correctly counts as non-smooth, keeping its contour.
    let fine_d = depth_diff_x + depth_diff_y;
    let coarse_d = neigh_depth_x + neigh_depth_y;
    let fine_n = normal_diff_x + normal_diff_y;
    let coarse_n = neigh_norm_x + neigh_norm_y;
    let smooth_slope = (coarse_d > fine_d * 2.0 + 0.004 && fine_d > 0.002)
        || (coarse_n > fine_n * 2.0 + 0.02 && fine_n > 0.01);

    var glyph: u32 = 32u; // ' ' default
    var is_edge = false;

    // Detect structural edges (silhouettes and creases)
    if (edge_mag > 0.38 && avg_nd.w < 0.999 && !smooth_slope) {
        is_edge = true;
        let edge_hash = hash2(x, y) % 3u;
        if (edge_y > edge_x * 1.6) {
            let is_top_crease = nd_t.w > avg_nd.w;
            if (is_top_crease) {
                if (edge_hash == 0u) { glyph = 45u; } // '-'
                else if (edge_hash == 1u) { glyph = 61u; } // '='
                else { glyph = 126u; } // '~'
            } else {
                // Bottom sill: never emit '_' â€” a full-cell underscore
                // disconnects from its neighbors and reads as an artifact
                // rule wherever a long boundary (treeline, bank, curb base)
                // spans the view. Volumetric texture keeps the boundary as
                // a ragged material transition instead. (is_edge=false
                // re-enters the ramp path below for a fresh glyph.)
                is_edge = false;
            }
        } else if (edge_x > edge_y * 1.6) {
            if (edge_hash == 0u) { glyph = 124u; } // '|'
            else if (edge_hash == 1u) { glyph = 33u; } // '!'
            else { glyph = 124u; } // '|' (never 'I': reads as text on terrain)
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
        // Dither is world-locked for terrain too: the Bayer pattern rides
        // the world tile, so the noise field no longer swims under pans.
        let dither = bayer4(gx_hash, gy_hash) * 0.05;

        // True aerial horizon void gate: only gates the extreme sky/map boundary approaching depth 1.0 (>= 0.9996).
        // Preserves 100% of visible terrain, foliage, and scenery across the top half of the screen.
        let far_void = entity_id == 0u && avg_nd.w > 0.9996;
        var depth_luma = luma;
        if (far_void) {
            depth_luma = luma * clamp((1.0 - avg_nd.w) * 2500.0, 0.0, 1.0);
        }
        let dithered_luma = clamp(depth_luma + dither, 0.0, 1.0);
        // Upscaler-lite: edge-adaptive micro-contrast. Cells near a detected
        // discontinuity get a small S-curve push so glyph bands resolve detail
        // without a separate FSR/temporal pass (still deferred pending profiling).
        let edge_sharpen = clamp(edge_mag, 0.0, 1.0) * 0.12;
        let sharpened_luma = clamp((dithered_luma - 0.5) * (1.0 + edge_sharpen) + 0.5, 0.0, 1.0);

        var band = 0u;
        if (sharpened_luma < 0.04) {
            band = 0u;
        } else if (sharpened_luma < 0.10) {
            band = 1u;
        } else if (sharpened_luma < 0.18) {
            band = 2u;
        } else if (sharpened_luma < 0.28) {
            band = 3u;
        } else if (sharpened_luma < 0.40) {
            band = 4u;
        } else if (sharpened_luma < 0.54) {
            band = 5u;
        } else if (sharpened_luma < 0.68) {
            band = 6u;
        } else if (sharpened_luma < 0.82) {
            band = 7u;
        } else if (sharpened_luma < 0.92) {
            band = 8u;
        } else {
            band = 9u;
        }

        let volumetric_glyph = material_glyph(material_id, band, gx_hash, gy_hash);
        if (far_void && sharpened_luma < 0.08) {
            glyph = 32u;
        } else if (sharpened_luma >= 0.04) {
            glyph = shape_glyph(
                shape_s0, shape_s1, shape_s2, shape_s3, shape_s4, shape_s5,
                e0c, e1c, e2c, e3c, e4c, e5c,
                edge_mag,
                entity_id == 0u && luma < 0.08,
                smooth_slope,
                material_id == 6u,
                hash2(x, y) % 3u,
                volumetric_glyph,
            );
        } else {
            glyph = volumetric_glyph;
        }
    }

    // Entity-ID outlines win over physical edges at actor silhouettes.
    if (entity_edge_x || entity_edge_y) {
        let is_left_edge = qid(left_meta.g) != entity_id;
        let is_right_edge = qid(right_meta.g) != entity_id;
        let is_top_edge = qid(top_meta.g) != entity_id;
        let is_bot_edge = qid(bottom_meta.g) != entity_id;

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
            // No bottom-sill overwrite: the physical path (Sobel / shape /
            // volumetric) already resolved this cell, and a full-cell '_'
            // under every actor reads as a floating rule. Top crests stay.
            if (is_top_edge) {
                glyph = 94u; // '^' top crest
            }
        }
        is_edge = true;
    }

    // Stage clearance (reference arena negative space, clarity study #2):
    // ground cells within a few cells of any actor emit pure space, so
    // silhouettes stand on a locally uniform floor instead of fighting
    // glyph-neighborhood noise. The scan reads the entity channel at one
    // texel per neighbor cell (stride 4 = one cell), clamped at borders;
    // rain/props/terrain carry entity 0 and never open a stage. Actor
    // surfaces (entity_id != 0) and the outline block above are untouched.
    var entity_near = false;
    var entity_dist = 99;
    var glow_accum = vec3<f32>(0.0);
    for (var dy = -3; dy <= 3; dy = dy + 1) {
        for (var dx = -3; dx <= 3; dx = dx + 1) {
            let near_x = clamp(base_x + 2 + dx * 4, 0, tex_w - 1);
            let near_y = clamp(base_y + 2 + dy * 4, 0, tex_h - 1);
            let near_meta = textureLoad(metadata_tex, vec2<i32>(near_x, near_y), 0) * 255.0;
            if (qid(near_meta.g) != 0u) {
                entity_near = true;
                let near_dist = max(abs(dx), abs(dy));
                entity_dist = min(entity_dist, near_dist);
            }
            // Radial background bloom (reference torch/lantern halos):
            // emissive surfaces (material 4 — torches, spell cores,
            // lantern glow) spill a soft quadratic halo onto the
            // background channel of surrounding cells, matching the
            // engine's compose::apply_glow spill. The channel quantizer
            // now keeps 2-level steps in the dark floor, so these halos
            // survive ANSI encoding instead of collapsing to zero.
            if (qid(near_meta.r) == 4u) {
                let near_col = textureLoad(color_tex, vec2<i32>(near_x, near_y), 0).rgb;
                let d = sqrt(f32(dx * dx + dy * dy));
                let falloff = clamp(1.0 - d / 4.2, 0.0, 1.0);
                glow_accum = glow_accum + near_col * (falloff * falloff * 0.08);
            }
        }
    }
    if (entity_near && entity_id == 0u) {
        // Ragged stage edge: the outermost shell dithers away by cell hash
        // so the quiet floor reads as a torn arena patch, not a stamped
        // rectangle (reference arenas have irregular dark boundaries).
        if (entity_dist < 3 || hash2(x, y) % 2u == 0u) {
            glyph = 32u; // stage: quiet floor around actors
        }
    }

    // Extended Reinhard tone mapping with contrast boost:
    // Preserves pure, deep dark-twilight backgrounds (no milky gamma lift) and vibrant saturated highlights
    let white_pt = 2.4;
    let tonemapped = (avg_color.rgb * (vec3<f32>(1.0) + avg_color.rgb / (white_pt * white_pt))) / (vec3<f32>(1.0) + avg_color.rgb);
    var fg_rgb = clamp(tonemapped * config.contrast_boost, vec3<f32>(0.0), vec3<f32>(1.0));
    // Background carries the scene's ambient dark wash plus the emissive
    // glow spill; capped so halos stay a dim wash, never a bright paint.
    var bg_rgb = clamp(tonemapped * 0.12 + glow_accum, vec3<f32>(0.0), vec3<f32>(0.35));

    if (entity_edge_x || entity_edge_y) {
        // Reference silhouettes (Videos 1-6) are crisp monochrome
        // silver-white rims — hostility and faction are communicated by
        // the floating nameplate and ground target ring, never a neon
        // outline (that split the player's hue dominance at 0.48).
        // Keep an 8% class accent so the shader contract still holds.
        fg_rgb = mix(fg_rgb, vec3<f32>(0.90, 0.93, 0.98), 0.35);
        if (entity_class == 1u) { fg_rgb = mix(fg_rgb, vec3<f32>(0.60, 1.00, 0.35), 0.08); }
        if (entity_class == 2u) { fg_rgb = mix(fg_rgb, vec3<f32>(1.00, 0.28, 0.16), 0.08); }
        if (entity_class == 3u) { fg_rgb = mix(fg_rgb, vec3<f32>(1.00, 0.78, 0.25), 0.08); }
        if (entity_class == 4u) { fg_rgb = mix(fg_rgb, vec3<f32>(0.35, 0.85, 1.00), 0.08); }
        bg_rgb = max(bg_rgb, fg_rgb * 0.10);
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
        assert!(ASCII_POST_WGSL.contains("contrast_power = 1.22"));
        assert!(ASCII_POST_WGSL.contains("shape_s0 = shape_s0 + sample_luma"));
        // Directional contrast: neighbor-region externals feed shape_glyph,
        // which enhances each component against its outward neighbor first.
        assert!(ASCII_POST_WGSL.contains("e0c, e1c, e2c, e3c, e4c, e5c,"));
        // Widened reach: each component takes the max over its affecting
        // outward texels AND corners, so light outside the cell resolves
        // boundaries deep into the sampling vector (Harri's widened table).
        assert!(ASCII_POST_WGSL.contains("ext_corner_ul"));
        assert!(ASCII_POST_WGSL
            .contains("let e4c = max(max(ext4, ext5), max(ext_l_bot, ext_corner_ll));"));
        assert!(ASCII_POST_WGSL.contains("dir_power = 2.0"));
        // Strokes require a geometric discontinuity; smooth fog gradients
        // must fall back to volumetric texture, never full-width rules.
        // Marginal treeline shimmer needs a strong step, not a bare pass.
        assert!(ASCII_POST_WGSL
            .contains("let stroke_edge_gate = select(0.12, 1.0, dark_field_terrain);"));
        assert!(ASCII_POST_WGSL.contains("if (edge_mag > stroke_edge_gate && !smooth_slope)"));
        assert!(ASCII_POST_WGSL.contains("entity_id == 0u && luma < 0.08"));
        assert!(ASCII_POST_WGSL.contains("coarse_d > fine_d * 2.0"));
        // Ragged treelines: every third horizontal-stroke cell keeps
        // volumetric texture instead of ruling the screen. Bottom sills
        // never emit '_' at all (volumetric fallback keeps the boundary as
        // a ragged material transition).
        assert!(ASCII_POST_WGSL.contains("if (edge_hash == 2u)"));
        assert!(ASCII_POST_WGSL.contains("(peak - trough) > 0.24"));
        assert!(ASCII_POST_WGSL.contains("fn qid(v: f32) -> u32"));
        assert!(ASCII_POST_WGSL.contains("let entity_id = qid(center_meta.g);"));
        assert!(!ASCII_POST_WGSL.contains("fn luma_at"));
    }

    #[test]
    fn terrain_glyphs_lock_to_world_tiles_and_resolve_stone_cobbles() {
        // World-locked glyph hash (the recorded "shower-door" shimmer):
        // stationary terrain/road/bark unproject their averaged depth to a
        // world tile key for glyph variants and Bayer dither, so a camera
        // pan no longer re-randomizes every ground glyph. Actors stay on
        // the screen hash.
        assert!(ASCII_POST_WGSL.contains("fn unproject_world("));
        assert!(ASCII_POST_WGSL.contains("let gx_hash = select(x, world_gx, entity_id == 0u);"));
        assert!(ASCII_POST_WGSL.contains("let gy_hash = select(y, world_gy, entity_id == 0u);"));
        assert!(ASCII_POST_WGSL.contains("material_glyph(material_id, band, gx_hash, gy_hash)"));
        assert!(ASCII_POST_WGSL.contains("bayer4(gx_hash, gy_hash)"));
        // Distance fog sinks below the 0.04 Band-0 gate so the horizon
        // resolves to pure ' ' (reference Videos 1/2/5/6), not band-1
        // punctuation speckle.
        assert!(SCENE_3D_WGSL.contains("let fog_color = vec3<f32>(0.006, 0.009, 0.016);"));
        // Stone (material 0) bands 6-9 emit rounded cobble/loop glyphs
        // instead of ░▒▓█ shade blocks; the only remaining block ramp is
        // the unknown-material fallback.
        assert!(ASCII_POST_WGSL.contains("'Q' cobble"));
        assert!(
            ASCII_POST_WGSL.contains("return 81u;                       // 'Q' torch-core stone"),
            "material-0 ramp must end on a cobble glyph"
        );
        assert!(
            !ASCII_POST_WGSL.contains("torch-core ground"),
            "material-0 must no longer end on a full block glyph"
        );
        // Monochrome silver rim + 8% class accents (reference actors have
        // no neon faction outlines; hue dominance was measured 0.48 under
        // the old 60% override).
        assert!(
            ASCII_POST_WGSL.contains("fg_rgb = mix(fg_rgb, vec3<f32>(0.90, 0.93, 0.98), 0.35);")
        );
        assert!(ASCII_POST_WGSL.contains(", 0.08); }"));
        // Emissive glow spill feeds the background channel that the old
        // shift-quantizer crushed to zero; the two-tier dark-floor
        // quantizer (Rust side) keeps it through ANSI encoding.
        assert!(ASCII_POST_WGSL.contains("var glow_accum = vec3<f32>(0.0);"));
        assert!(ASCII_POST_WGSL.contains("tonemapped * 0.12 + glow_accum"));
    }

    #[test]
    fn terrain_material_keeps_vertex_position_stationary() {
        assert!(SCENE_3D_WGSL.contains("mat_id >= 2.8 && mat_id <= 3.2 && world_pos4.y > 1.2"));
        assert!(!SCENE_3D_WGSL.contains("Meadow grass slow synchronized wind"));
    }

    #[test]
    fn ocean_water_rolls_toward_the_shore_and_breaks_foam() {
        // The southern ocean uses one directional rolling swell for both the
        // geometry bob and the fragment shading (shared phases so they never
        // desync); foam breaks white on the crests and the base deepens with
        // distance from the shoreline (mesh z = 28.0 = WORLD_BOUNDS * 0.8).
        assert!(SCENE_3D_WGSL.contains("sin(camera.time * 2.2 + world_pos4.z * 0.55) * 0.14"));
        assert!(SCENE_3D_WGSL.contains("sin(camera.time * 3.1 + world_pos4.x * 0.9) * 0.05"));
        assert!(SCENE_3D_WGSL.contains(
            "let swell = sin(camera.time * 2.2 + in.world_pos.z * 0.55 + in.world_pos.x * 0.25);"
        ));
        assert!(SCENE_3D_WGSL.contains("let foam = smoothstep(0.58, 0.92, swell + chop);"));
        assert!(SCENE_3D_WGSL.contains("mix(water_color, vec3<f32>(0.88, 0.94, 1.0), foam * 0.85)"));
        assert!(SCENE_3D_WGSL.contains("select(0.88, 1.0, foam > 0.15)"));
        assert!(SCENE_3D_WGSL.contains("clamp((in.world_pos.z - 28.0) / 14.0, 0.0, 1.0)"));
        // Quiet-sea tuning (user report: "grass texture everywhere" — the
        // sky-reflection lift held distant water in mid bands): spec 0.6,
        // fresnel 0.10, and a 1.6x water light gain keeping the wave
        // language at reference brightness while the global ambient/sun
        // rebalance (GPU datum below the sRGB target) darkens the carpet.
        assert!(SCENE_3D_WGSL.contains("0.0), 32.0) * 0.6;"));
        assert!(SCENE_3D_WGSL
            .contains("let fresnel = pow(1.0 - max(dot(V, wave_norm), 0.0), 3.0) * 0.10;"));
        // Wave language is quiet: calm bands stay empty, band 2 is a pure
        // '~', and the ramp header documents the no-letter rule.
        assert!(ASCII_POST_WGSL.contains("if (band <= 1u) { return 32u; }"));
        assert!(ASCII_POST_WGSL.contains("if (band == 2u) { return 126u; }"));
        assert!(ASCII_POST_WGSL.contains("water: rolling wave language, quiet by design"));
        // Water cells never take stroke overrides (wobble normals are
        // shading, not silhouettes).
        assert!(ASCII_POST_WGSL.contains("if (!water_surface) {"));
    }

    #[test]
    fn character_models_shade_with_cel_headlight_and_rim() {
        // Entity fragments (entity_id > 0; terrain and props stay 0 with the
        // smooth dark-carpet lighting) quantize the sun's NdotL into stepped
        // bands so lit/shadow facets stay distinct at terminal resolution
        // (Harri's cel-shading separation, kciter's discrete Shade ramp),
        // add kciter's headlight term ("the light comes from the direction
        // you are looking") so camera-facing surfaces keep their definition,
        // and get a rim light that lifts the silhouette off the dark ground.
        // The adds are trimmed (0.24/0.28) so actors stay the clear local
        // pop without clipping to a near-white blob once the surrounding
        // carpet matches the reference darkness.
        assert!(SCENE_3D_WGSL.contains("let cel_steps = 4.0;"));
        assert!(SCENE_3D_WGSL.contains(
            "let sun_ndl = select(NdotL, floor(NdotL * cel_steps) / cel_steps, in.entity_id > 0.5);"
        ));
        assert!(SCENE_3D_WGSL
            .contains("diffuse_light += vec3<f32>(1.0, 0.95, 0.85) * pow(facing, 2.0) * 0.24;"));
        assert!(SCENE_3D_WGSL.contains("let rim = pow(1.0 - max(dot(N, V), 0.0), 3.0);"));
        assert!(SCENE_3D_WGSL.contains("lit_color += vec3<f32>(0.85, 0.90, 1.0) * rim * 0.28;"));
    }

    #[test]
    fn actors_stand_on_a_locally_cleared_stage() {
        // Clarity study #2 (video-tools REPORT): the reference arenas are
        // locally uniform floors — ground cells near any actor emit pure
        // space so silhouettes read against negative space, not glyph
        // noise. The scan is one metadata tap per neighbor cell (stride 4),
        // suppression only fires on entity-0 cells, and rain/props/terrain
        // never open a stage.
        assert!(ASCII_POST_WGSL.contains("var entity_near = false;"));
        assert!(ASCII_POST_WGSL.contains(
            "let near_meta = textureLoad(metadata_tex, vec2<i32>(near_x, near_y), 0) * 255.0;"
        ));
        assert!(
            ASCII_POST_WGSL.contains("if (entity_near && entity_id == 0u) {"),
            "stage suppression must never touch actor surfaces"
        );
        assert!(
            ASCII_POST_WGSL.contains("glyph = 32u; // stage: quiet floor around actors"),
            "stage cells must resolve to pure space"
        );
        assert!(
            ASCII_POST_WGSL.contains("if (entity_dist < 3 || hash2(x, y) % 2u == 0u) {"),
            "stage outer shell must dither to a ragged edge"
        );
    }
}
