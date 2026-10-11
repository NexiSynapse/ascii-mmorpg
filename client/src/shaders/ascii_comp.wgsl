// ASCII post-process: one invocation per terminal cell averages its texel
// block, maps luminance through the brightness ramp, and writes a packed
// cell record for CPU readback. No CPU per-pixel work stays on this path.

struct Params {
    cells_x: u32,
    cells_y: u32,
    tex_w: u32,
    tex_h: u32,
};

@group(0) @binding(0) var scene_tex: texture_2d<f32>;
@group(0) @binding(1) var<storage, read_write> cells: array<vec4<u32>>;
@group(0) @binding(2) var<uniform> params: Params;

// Brightness ramp codepoints, sparsest to densest:
// [' ', '.', ':', '-', '=', '+', '*', '#', '@', '%']
const RAMP: array<u32, 10> = array<u32, 10>(
    32u, 46u, 58u, 45u, 61u, 43u, 42u, 35u, 64u, 37u,
);
const RAMP_LEN: u32 = 10u;

@compute @workgroup_size(8, 8)
fn ascii_main(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= params.cells_x || id.y >= params.cells_y) {
        return;
    }
    let sx = params.tex_w / params.cells_x;
    let sy = params.tex_h / params.cells_y;
    var rgb = vec3<f32>(0.0);
    var n = 0u;
    for (var oy = 0u; oy < sy; oy++) {
        for (var ox = 0u; ox < sx; ox++) {
            let tx = min(id.x * sx + ox, params.tex_w - 1u);
            let ty = min(id.y * sy + oy, params.tex_h - 1u);
            rgb += textureLoad(scene_tex, vec2<u32>(tx, ty), 0).rgb;
            n++;
        }
    }
    rgb /= f32(max(n, 1u));
    let lum = clamp(0.2126 * rgb.r + 0.7152 * rgb.g + 0.0722 * rgb.b, 0.0, 1.0);
    // Lift the mids so torch-lit geometry reads; void stays void.
    let boosted = rgb * 1.35 + vec3<f32>(0.02);
    let fg = (u32(clamp(boosted.r, 0.0, 1.0) * 255.0) << 16u)
        | (u32(clamp(boosted.g, 0.0, 1.0) * 255.0) << 8u)
        | u32(clamp(boosted.b, 0.0, 1.0) * 255.0);
    var glyph = RAMP[u32(lum * 9.999)];
    if (lum < 0.012) {
        glyph = 32u;
    }
    // Midnight void background, matching the ANSI renderer.
    let bg = (12u << 16u) | (15u << 8u) | 22u;
    cells[id.y * params.cells_x + id.x] = vec4<u32>(glyph, fg, bg, 0u);
}
