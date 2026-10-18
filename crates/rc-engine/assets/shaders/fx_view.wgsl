// The port's modern full-screen passes (crate::fx_view): the view matrices and the camera's viewport, and positions
// and normals rebuilt from the scene depth. Every function takes the target pixel's centre (`@builtin(position).xy`).
// Every pass binds the view at group 0, binding 0 (declared here: naga_oil cannot pass this uniform's struct across
// modules by value, and renames module identifiers that end in a digit).

#define_import_path rerac::fx_view

struct FxView {
    clip_from_view: mat4x4<f32>,
    view_from_clip: mat4x4<f32>,
    world_from_view: mat4x4<f32>,
    view_from_world: mat4x4<f32>,
    // The camera's viewport in target pixels: x, y, width, height.
    viewport: vec4<f32>,
    // The pass's own values (each pass documents them).
    params: vec4<f32>,
    extra: vec4<f32>,
}

@group(0) @binding(0) var<uniform> view: FxView;

// NDC (x right, y up) of a target pixel.
fn fx_ndc(coord: vec2<f32>) -> vec2<f32> {
    let uv = (coord - view.viewport.xy) / view.viewport.zw;
    return vec2<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0);
}

// The target pixel of an NDC position.
fn fx_coord(ndc: vec2<f32>) -> vec2<f32> {
    return view.viewport.xy + vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5) * view.viewport.zw;
}

// The view-space position of a pixel at a depth (Bevy view space: −z forward).
fn fx_view_pos(coord: vec2<f32>, depth: f32) -> vec3<f32> {
    let p = view.view_from_clip * vec4<f32>(fx_ndc(coord), depth, 1.0);
    return p.xyz / p.w;
}

// The view-space normal from the centre and its four neighbours (left, right, up, down on the screen), each side
// taken from the neighbour nearer in depth so silhouettes do not bend it; it faces the camera.
fn fx_normal(c: vec3<f32>, l: vec3<f32>, r: vec3<f32>, u: vec3<f32>, d: vec3<f32>) -> vec3<f32> {
    let dx = select(c - l, r - c, abs(r.z - c.z) < abs(c.z - l.z));
    let dy = select(c - u, d - c, abs(d.z - c.z) < abs(c.z - u.z));
    var n = normalize(cross(dy, dx));
    if (dot(n, -c) < 0.0) { n = -n; }
    return n;
}

// Interleaved gradient noise (0..1) of a pixel: a per-pixel rotation the passes' blurs average out.
fn fx_noise(coord: vec2<f32>) -> f32 {
    return fract(52.9829189 * fract(dot(floor(coord), vec2<f32>(0.06711056, 0.00583715))));
}
