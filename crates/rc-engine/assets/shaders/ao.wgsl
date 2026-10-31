// Ambient occlusion On (crate::ao_render): depth-only screen-space ambient occlusion.
// view.params = (radius in world units, taps, strength, maximum distance).
//
// `occlusion`: for a pixel at view position C with normal N, taps S_i on a per-pixel rotated spiral of the radius's
// screen size; each occludes by max(0, (S_i − C)·N − ε) / (|S_i − C|² + δ), faded by its distance over the radius.
// ao = 1 − 2·Σ/taps (clamped), written to an R8 target. `apply`: a 4×4 depth-aware blur of that target, returned as
// the factor mix(1, ao, strength) that the pass multiplies into the frame, except on the mobys (`moby_mask`).

#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput
#import rerac::fx_view::{view, fx_view_pos, fx_normal, fx_noise}

#ifdef MULTISAMPLED
@group(0) @binding(1) var depth: texture_depth_multisampled_2d;
#else
@group(0) @binding(1) var depth: texture_depth_2d;
#endif
@group(0) @binding(2) var ao_tex: texture_2d<f32>;

// crate::ao_render::AoMobys.
struct AoMobys {
    boxes: array<mat4x4<f32>, 32>,
    fade: array<vec4<f32>, 32>,
    count: u32,
}
@group(0) @binding(3) var<uniform> mobys: AoMobys;

// 1 on a moby: inside one of the boxes (world → [-1, 1]³), fading to 0 over the fade's share of each half extent
// outside each face (a box face is the moby's own surface), so a wall it hangs against keeps its occlusion beyond
// that; at the bottom face (box z −1: its feet) the fade is inside, so the floor it stands on keeps its occlusion under
// an overhanging edge too. `c` is the pixel's view position.
fn moby_mask(c: vec3<f32>) -> f32 {
    let w = view.world_from_view * vec4<f32>(c, 1.0);
    var k = 0.0;
    for (var i = 0u; i < mobys.count; i += 1u) {
        let s = (mobys.boxes[i] * w).xyz;
        let f = mobys.fade[i].xyz;
        var e = vec3<f32>(1.0) - smoothstep(vec3<f32>(1.0), vec3<f32>(1.0) + f, abs(s));
        if (s.z < 0.0) { e.z = smoothstep(-1.0, -1.0 + f.z, s.z); }
        k = max(k, e.x * e.y * e.z);
    }
    return k;
}

fn depth_at(p: vec2<i32>) -> f32 {
    let lo = vec2<i32>(view.viewport.xy);
    let hi = lo + vec2<i32>(view.viewport.zw) - 1;
    return textureLoad(depth, clamp(p, lo, hi), 0);
}

fn pos_at(p: vec2<i32>) -> vec3<f32> {
    return fx_view_pos(vec2<f32>(p) + 0.5, depth_at(p));
}

@fragment
fn occlusion(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let p = vec2<i32>(floor(in.position.xy));
    let d = depth_at(p);
    if (d <= 0.0) { return vec4<f32>(1.0); }
    let c = fx_view_pos(in.position.xy, d);
    let dist = -c.z;
    let far = view.params.w;
    if (dist >= far) { return vec4<f32>(1.0); }
    let n = fx_normal(c, pos_at(p - vec2<i32>(1, 0)), pos_at(p + vec2<i32>(1, 0)), pos_at(p - vec2<i32>(0, 1)), pos_at(p + vec2<i32>(0, 1)));

    let radius = view.params.x;
    let r_px = clamp(radius / dist * view.clip_from_view[1][1] * view.viewport.w * 0.5, 1.0, 96.0);
    let taps = i32(view.params.y);
    let rot = fx_noise(in.position.xy) * 6.2831853;
    var sum = 0.0;
    for (var i = 0; i < taps; i += 1) {
        let t = (f32(i) + 0.5) / f32(taps);
        let a = rot + f32(i) * 2.3999632;
        let o = vec2<f32>(cos(a), sin(a)) * (r_px * t);
        let sp = vec2<i32>(floor(in.position.xy + o));
        let sd = depth_at(sp);
        if (sd <= 0.0) { continue; }
        let v = fx_view_pos(vec2<f32>(sp) + 0.5, sd) - c;
        let d2 = dot(v, v);
        let range = 1.0 - smoothstep(radius * radius, 4.0 * radius * radius, d2);
        sum += max(0.0, dot(v, n) - 0.002 * dist) / (d2 + 0.01 * radius * radius) * range;
    }
    var ao = clamp(1.0 - 2.0 * radius * sum / f32(taps), 0.0, 1.0);
    ao = mix(ao, 1.0, smoothstep(far * 0.66, far, dist));
    return vec4<f32>(ao, 0.0, 0.0, 1.0);
}

@fragment
fn apply(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let p = vec2<i32>(floor(in.position.xy));
    let d = depth_at(p);
    if (d <= 0.0) { return vec4<f32>(1.0); }
    let c = fx_view_pos(in.position.xy, d);
    let z = -c.z;
    let size = vec2<i32>(textureDimensions(ao_tex));
    var sum = 0.0;
    var weight = 0.0;
    for (var y = -2; y < 2; y += 1) {
        for (var x = -2; x < 2; x += 1) {
            let q = clamp(p + vec2<i32>(x, y), vec2<i32>(0), size - 1);
            let qd = depth_at(q);
            let qz = -fx_view_pos(vec2<f32>(q) + 0.5, qd).z;
            let w = select(exp(-abs(qz - z) / (0.05 * z + 0.01)), 0.0, qd <= 0.0);
            sum += textureLoad(ao_tex, q, 0).r * w;
            weight += w;
        }
    }
    var ao = select(1.0, sum / weight, weight > 0.0);
    ao = mix(ao, 1.0, moby_mask(c));
    let k = mix(1.0, ao, view.params.z);
    return vec4<f32>(k, k, k, 1.0);
}
