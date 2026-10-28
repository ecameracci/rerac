// Lighting Enhanced: global illumination (crate::gi_render): one bounce of the lit colour between nearby surfaces.
// view.params = (radius in world units, taps, strength, maximum distance); view.extra = (cell size in target pixels,
// 1: the bounce-only view, 0, 0).
//
// On a grid of cells (one per cell×cell target pixels): `geometry` keeps the view normal and the depth of the cell's
// centre pixel (w = 0: sky, off the viewport or past the distance); `colour` the cell's lit colour (its pixels at the
// centre's depth, averaged). `bounce`: taps S_i on a per-cell rotated spiral of the radius's grid size, each standing
// for its share of the disc's world area at the sender and sending its colour by `form_factor`: Σ colour · form
// factor, the light one bounce brings (crate::gi_render::tests::model mirrors `spiral`, `form_factor`, the tap area
// and the fade). `blur_x` / `blur_y`:
// a 7-cell depth- and normal-aware blur across and down. `composite`: the four nearest cells, bilinear and
// depth-aware, the bounce light × strength, faded out by the distance, × the gain that turns the receiver's lit colour
// into its reflectance (MEAN_ALBEDO / its luminance); the pass blends dst · (1 + that), so the bounce takes the
// receiving surface's own colour.

#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput
#import rerac::fx_view::{view, fx_view_pos, fx_normal, fx_noise}

#ifdef MULTISAMPLED
@group(0) @binding(1) var depth: texture_depth_multisampled_2d;
#else
@group(0) @binding(1) var depth: texture_depth_2d;
#endif
// The pass's colour input: the frame (`colour`), the cells' colour (the others).
@group(0) @binding(2) var src: texture_2d<f32>;
// The cells' geometry (unused by `geometry`).
@group(0) @binding(3) var geom: texture_2d<f32>;
// The bounce being blurred (`blur_x`, `blur_y`) or composited.
@group(0) @binding(4) var aux: texture_2d<f32>;

const GOLDEN: f32 = 2.3999632;
const TAU: f32 = 6.2831853;
// The reflectance a lit colour of luminance 1 / MEAN_ALBEDO stands for, and the largest gain.
const MEAN_ALBEDO: f32 = 0.5;
const MAX_GAIN: f32 = 3.0;
// The spiral's largest radius in cells.
const MAX_CELLS: f32 = 32.0;

fn cell() -> i32 { return i32(view.extra.x); }

fn in_viewport(p: vec2<i32>) -> vec2<i32> {
    let lo = vec2<i32>(view.viewport.xy);
    return clamp(p, lo, lo + vec2<i32>(view.viewport.zw) - 1);
}

fn depth_at(p: vec2<i32>) -> f32 { return textureLoad(depth, in_viewport(p), 0); }

fn pos_at(p: vec2<i32>) -> vec3<f32> { return fx_view_pos(vec2<f32>(p) + 0.5, depth_at(p)); }

// The target pixel at the centre of cell q.
fn cell_pixel(q: vec2<i32>) -> vec2<i32> { return q * cell() + cell() / 2; }

// Cell q's geometry, 0 outside the grid.
fn geom_at(q: vec2<i32>) -> vec4<f32> {
    let size = vec2<i32>(textureDimensions(geom));
    if (any(q < vec2<i32>(0)) || any(q >= size)) { return vec4<f32>(0.0); }
    return textureLoad(geom, q, 0);
}

// The view position of cell q of geometry g.
fn cell_pos(q: vec2<i32>, g: vec4<f32>) -> vec3<f32> { return fx_view_pos(vec2<f32>(cell_pixel(q)) + 0.5, g.w); }

fn depth_weight(z: f32, z0: f32) -> f32 { return exp(-abs(z - z0) / (0.05 * z0 + 0.01)); }

fn luma(c: vec3<f32>) -> f32 { return dot(c, vec3<f32>(0.2126, 0.7152, 0.0722)); }

// Tap i of `taps` on the unit disc: the golden-angle spiral, rotated by `rot`, its radius t = (i + ½) / taps growing
// linearly (denser near the centre, where the bounce is strongest); the tap stands for 2t / taps of the disc's area.
fn spiral(i: i32, taps: i32, rot: f32) -> vec2<f32> {
    let t = (f32(i) + 0.5) / f32(taps);
    let a = rot + f32(i) * GOLDEN;
    return vec2<f32>(cos(a), sin(a)) * t;
}

// The share of a receiver's light a sender of `area` at v from it sends: the disc-to-point form factor
// A·cos θr·cos θs / (π d² + A) (at most 1; each cosine less 0.05, so a flat surface sends nothing to itself), cut off
// from 0.75 to 1 radius.
fn form_factor(nr: vec3<f32>, ns: vec3<f32>, v: vec3<f32>, area: f32, radius: f32) -> f32 {
    let d2 = dot(v, v);
    if (d2 < 1e-8) { return 0.0; }
    let d = sqrt(d2);
    let dir = v / d;
    let cr = max(0.0, dot(nr, dir) - 0.05);
    let cs = max(0.0, -dot(ns, dir) - 0.05);
    return area * cr * cs / (3.14159265 * d2 + area) * (1.0 - smoothstep(0.75 * radius, radius, d));
}

@fragment
fn geometry(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let p = cell_pixel(vec2<i32>(floor(in.position.xy)));
    if (any(p != in_viewport(p))) { return vec4<f32>(0.0); }
    let d = depth_at(p);
    if (d <= 0.0) { return vec4<f32>(0.0); }
    let c = fx_view_pos(vec2<f32>(p) + 0.5, d);
    if (-c.z >= view.params.w) { return vec4<f32>(0.0); }
    let n = fx_normal(c, pos_at(p - vec2<i32>(1, 0)), pos_at(p + vec2<i32>(1, 0)), pos_at(p - vec2<i32>(0, 1)), pos_at(p + vec2<i32>(0, 1)));
    return vec4<f32>(n, d);
}

@fragment
fn colour(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let q = vec2<i32>(floor(in.position.xy));
    let g = textureLoad(geom, q, 0);
    if (g.w <= 0.0) { return vec4<f32>(0.0); }
    let z0 = -cell_pos(q, g).z;
    let n = cell();
    var sum = vec3<f32>(0.0);
    var w_sum = 0.0;
    for (var y = 0; y < n; y += 1) {
        for (var x = 0; x < n; x += 1) {
            let p = in_viewport(q * n + vec2<i32>(x, y));
            let d = textureLoad(depth, p, 0);
            if (d <= 0.0) { continue; }
            let w = depth_weight(-fx_view_pos(vec2<f32>(p) + 0.5, d).z, z0);
            sum += textureLoad(src, p, 0).rgb * w;
            w_sum += w;
        }
    }
    return vec4<f32>(sum / max(w_sum, 1e-6), 1.0);
}

@fragment
fn bounce(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let q = vec2<i32>(floor(in.position.xy));
    let g = textureLoad(geom, q, 0);
    if (g.w <= 0.0) { return vec4<f32>(0.0); }
    let c = cell_pos(q, g);
    let radius = view.params.x;
    // Target pixels per world unit at depth 1.
    let focal = view.clip_from_view[1][1] * view.viewport.w * 0.5;
    let r_cells = clamp(radius * focal / -c.z / f32(cell()), 1.0, MAX_CELLS);
    let taps = i32(view.params.y);
    let disc = 3.14159265 * r_cells * r_cells;
    let rot = fx_noise(in.position.xy) * TAU;
    var sum = vec3<f32>(0.0);
    for (var i = 0; i < taps; i += 1) {
        let s = q + vec2<i32>(round(spiral(i, taps, rot) * r_cells));
        if (all(s == q)) { continue; }
        let gs = geom_at(s);
        if (gs.w <= 0.0) { continue; }
        let ps = cell_pos(s, gs);
        // The cells the tap stands for, as world area at the sender: a cell's side at its depth, squared, over its
        // slant to the view.
        let side = f32(cell()) * -ps.z / focal;
        let share = disc * 2.0 * (f32(i) + 0.5) / f32(taps * taps);
        let area = share * side * side / max(abs(dot(gs.xyz, normalize(ps))), 0.2);
        sum += textureLoad(src, s, 0).rgb * form_factor(g.xyz, gs.xyz, ps - c, area, radius);
    }
    return vec4<f32>(sum, 1.0);
}

fn blur(q: vec2<i32>, axis: vec2<i32>) -> vec4<f32> {
    let g = textureLoad(geom, q, 0);
    if (g.w <= 0.0) { return vec4<f32>(0.0); }
    let z0 = -cell_pos(q, g).z;
    var sum = vec3<f32>(0.0);
    var w_sum = 0.0;
    for (var k = -3; k <= 3; k += 1) {
        let s = q + axis * k;
        let gs = geom_at(s);
        if (gs.w <= 0.0) { continue; }
        let w = exp(-f32(k * k) / 8.0) * depth_weight(-cell_pos(s, gs).z, z0) * pow(max(dot(gs.xyz, g.xyz), 0.0), 8.0);
        sum += textureLoad(aux, s, 0).rgb * w;
        w_sum += w;
    }
    return vec4<f32>(sum / max(w_sum, 1e-6), 1.0);
}

@fragment
fn blur_x(in: FullscreenVertexOutput) -> @location(0) vec4<f32> { return blur(vec2<i32>(floor(in.position.xy)), vec2<i32>(1, 0)); }

@fragment
fn blur_y(in: FullscreenVertexOutput) -> @location(0) vec4<f32> { return blur(vec2<i32>(floor(in.position.xy)), vec2<i32>(0, 1)); }

@fragment
fn composite(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let none = vec4<f32>(0.0, 0.0, 0.0, 1.0);
    let d = depth_at(vec2<i32>(floor(in.position.xy)));
    if (d <= 0.0) { return none; }
    let z = -fx_view_pos(in.position.xy, d).z;
    let far = view.params.w;
    if (z >= far) { return none; }
    let n = f32(cell());
    let u = (in.position.xy - f32(cell() / 2) - 0.5) / n;
    let q0 = vec2<i32>(floor(u));
    let f = u - floor(u);
    var light = vec3<f32>(0.0);
    var own = vec3<f32>(0.0);
    var w_sum = 0.0;
    for (var j = 0; j < 4; j += 1) {
        let o = vec2<i32>(j % 2, j / 2);
        let q = q0 + o;
        let g = geom_at(q);
        if (g.w <= 0.0) { continue; }
        let b = select(1.0 - f, f, o == vec2<i32>(1));
        let w = (b.x * b.y + 1e-3) * depth_weight(-cell_pos(q, g).z, z);
        light += textureLoad(aux, q, 0).rgb * w;
        own += textureLoad(src, q, 0).rgb * w;
        w_sum += w;
    }
    if (w_sum < 1e-4) { return none; }
    light *= view.params.z * (1.0 - smoothstep(far * 0.66, far, z)) / w_sum;
    if (view.extra.y > 0.5) { return vec4<f32>(light, 1.0); }
    let gain = min(MEAN_ALBEDO / max(luma(own / w_sum), 1e-3), MAX_GAIN);
    return vec4<f32>(min(light * gain, vec3<f32>(1.0)), 1.0);
}
