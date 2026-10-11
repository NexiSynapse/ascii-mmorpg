// Offscreen diorama lighting pass. Units are world cells; y is up.
// Per-vertex albedo keeps the client dependency-free (no material tables).

struct Camera {
    mvp: mat4x4<f32>,
    light_a: vec3<f32>,
    ambient: f32,
    light_b: vec3<f32>,
    boost: f32,
};

@group(0) @binding(0) var<uniform> cam: Camera;

struct VsIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) albedo: vec3<f32>,
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) albedo: vec3<f32>,
};

@vertex
fn vs_main(in: VsIn) -> VsOut {
    var out: VsOut;
    // DEBUG EXPERIMENT: row-vector convention to test for a
    // CPU/GPU majorness mismatch.
    out.clip = vec4<f32>(in.pos, 1.0) * cam.mvp;
    out.world = in.pos;
    out.normal = normalize(in.normal);
    out.albedo = in.albedo;
    return out;
}

fn point_light(world: vec3<f32>, normal: vec3<f32>, light_pos: vec3<f32>) -> f32 {
    let dir = light_pos - world;
    let dist = max(length(dir), 0.001);
    let atten = 1.0 / (1.0 + 0.015 * dist * dist);
    return max(dot(normal, dir / dist), 0.0) * atten;
}

fn hash12(p: vec2<f32>) -> f32 {
    var h = dot(p, vec2<f32>(127.1, 311.7));
    return fract(sin(h) * 43758.5453);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let n = normalize(in.normal);
    var light = cam.ambient;
    light += point_light(in.world, n, cam.light_a);
    light += point_light(in.world, n, cam.light_b);
    // Fine grain so flat faces still quantize into varied glyphs.
    let grain = (hash12(floor(in.world.xz * 5.0) + floor(in.world.y * 5.0)) - 0.5) * 0.06;
    let shaded = in.albedo * clamp(light, 0.0, cam.boost) + grain;
    return vec4<f32>(max(shaded, vec3<f32>(0.0)), 1.0);
}
