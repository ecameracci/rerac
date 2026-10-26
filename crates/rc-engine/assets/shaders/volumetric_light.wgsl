// Lighting Enhanced's volumetric light (crate::volumetric_light): in-scattering through the sun's cascaded shadow map.
// `march` (half resolution): each pixel's view ray from the camera to the surface or the open sea (or a short way
// toward the sky),
// dithered steps packed toward the camera, lit steps adding σ·exp(−σt)·dt, each weighted by the shadow around it
// (crate::volumetric_light::Shafts: bold next to long shadows, faint in open lit air), times the Henyey-Greenstein phase;
// rgb = the in-scattered light, a = the sample's view distance. `composite` (full resolution): a 4×4 depth-aware blur
// of that target, less the share it has in common with its surroundings (crate::volumetric_light::flatten), added to
// the frame. view.extra.xyz = toward the light (world).

#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput
#import rerac::fx_view::{view, fx_view_pos, fx_noise}

struct SunCascades {
    clip_from_world: array<mat4x4<f32>, 4>,
    far: vec4<f32>,
    texel: vec4<f32>,
}

// crate::volumetric_light::VolParams.
struct VolParams {
    // Tint × strength; w: the cascade count.
    tint: vec4<f32>,
    // Steps, density σ, phase g, the longest ray.
    march: vec4<f32>,
    // The sky ray, the debug gain (> 0: the shafts alone, times it), the target's width and height.
    extra: vec4<f32>,
    // crate::volumetric_light::ShaftShape: open, edge, shade, after_lit.
    shape: vec4<f32>,
    // crate::volumetric_light::SeaLevel: its y, 1 while known.
    sea: vec4<f32>,
    // crate::volumetric_light::flatten: the share taken out, the surroundings' radius (target pixels).
    flat: vec4<f32>,
}

#ifdef MULTISAMPLED
@group(0) @binding(1) var depth: texture_depth_multisampled_2d;
#else
@group(0) @binding(1) var depth: texture_depth_2d;
#endif
@group(0) @binding(2) var<uniform> sun: SunCascades;
@group(0) @binding(3) var shadow_map: texture_depth_2d_array;
@group(0) @binding(4) var shadow_compare: sampler_comparison;
@group(0) @binding(5) var source: texture_2d<f32>;
@group(0) @binding(6) var scatter_tex: texture_2d<f32>;
@group(0) @binding(7) var<uniform> vol: VolParams;

// The view distance a sky sample stores (no surface).
const SKY_DIST: f32 = 1.0e4;

fn inside_viewport(coord: vec2<f32>) -> bool {
    return all(coord >= view.viewport.xy) && all(coord < view.viewport.xy + view.viewport.zw);
}

// crate::volumetric_light::phase.
fn phase(cos_theta: f32, g: f32) -> f32 {
    let d = max(1.0 + g * g - 2.0 * g * cos_theta, 1.0e-4);
    return (1.0 - g * g) / (12.566371 * d * sqrt(d));
}

// crate::volumetric_light::Shafts::close: a lit run's weighted in-scattering. r = (Σ s, Σ s·exp(−d/edge),
// Σ s·exp(d/edge), its length), d from its start; before / after: the weights of the shadows bounding it; cam: the
// camera's shade.
fn close_run(r: vec4<f32>, before: f32, after_shadow: f32, cam: f32) -> f32 {
    let open = vol.shape.x;
    let after = after_shadow * (vol.shape.w + (1.0 - vol.shape.w) * cam);
    let ab = exp(-min(r.w / vol.shape.y, 60.0));
    let near = before * r.y + after * ab * r.z - before * after * ab * r.x;
    return open * r.x + (1.0 - open) * near;
}

// crate::volumetric_light::Shafts::end_shadow: a shadow's weight from its length along the ray.
fn shadow_weight(len: f32) -> f32 {
    return smoothstep(0.25 * vol.shape.z, vol.shape.z, len);
}

// 1 lit, 0 shadowed: one comparison tap (bilinear) in the cascade holding view depth `z`; lit outside the cascades.
fn sun_visibility(world: vec3<f32>, z: f32) -> f32 {
    var cascade = -1;
    let count = i32(vol.tint.w);
    for (var i = 0; i < count; i += 1) {
        if (z < sun.far[i]) { cascade = i; break; }
    }
    if (cascade < 0) { return 1.0; }
    let c = sun.clip_from_world[cascade] * vec4<f32>(world, 1.0);
    let ndc = c.xyz / c.w;
    let uv = ndc.xy * vec2<f32>(0.5, -0.5) + 0.5;
    if (any(uv < vec2<f32>(0.0)) || any(uv > vec2<f32>(1.0))) { return 1.0; }
    return textureSampleCompareLevel(shadow_map, shadow_compare, uv, cascade, ndc.z);
}

@fragment
fn march(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let size = vol.extra.zw;
    let coord = clamp(floor(in.uv * size), vec2<f32>(0.0), size - 1.0) + 0.5;
    if (!inside_viewport(coord)) { return vec4<f32>(0.0, 0.0, 0.0, SKY_DIST); }
    let d = textureLoad(depth, vec2<i32>(coord), 0);
    let ray_view = normalize(fx_view_pos(coord, 1.0));
    var len = vol.extra.x;
    var dist = SKY_DIST;
    if (d > 0.0) {
        dist = length(fx_view_pos(coord, d));
        len = min(dist, vol.march.w);
    }
    let eye = view.world_from_view[3].xyz;
    let ray = normalize((view.world_from_view * vec4<f32>(ray_view, 0.0)).xyz);
    // The open sea writes no depth: the ray stops at its surface, and from under it there is no air to light.
    if (vol.sea.y > 0.5) {
        if (eye.y <= vol.sea.x) { return vec4<f32>(0.0, 0.0, 0.0, dist); }
        if (ray.y < 0.0) { len = min(len, (vol.sea.x - eye.y) / ray.y); }
    }
    let forward = -ray_view.z;
    let n = i32(vol.march.x);
    let sigma = vol.march.y;
    let jitter = fx_noise(in.position.xy);
    // crate::volumetric_light::Shafts: the current lit run (see close_run; start < 0: none) and the weight of the
    // shadow before it, a run ended by the current shadow (pending), the current shadow's length (< 0: lit), the
    // weight of the last shadow, the camera's shade (< 0: not yet known).
    var sum = 0.0;
    var run = vec4<f32>(0.0);
    var start = -1.0;
    var run_before = 0.0;
    var pending = vec4<f32>(0.0);
    var pending_before = 0.0;
    var has_pending = false;
    var shadow = -1.0;
    var before = 0.0;
    var cam = -1.0;
    for (var i = 0; i < n; i += 1) {
        // crate::volumetric_light::step.
        let u = (f32(i) + jitter) / f32(n);
        let t = len * u * u;
        let dt = len * 2.0 * u / f32(n);
        let vis = sun_visibility(eye + ray * t, t * forward);
        let s = vis * exp(-sigma * t) * sigma * dt;
        if (vis >= 0.5) {
            if (shadow >= 0.0) {
                let w = shadow_weight(shadow);
                if (cam < 0.0) { cam = w; }
                if (has_pending) { sum += close_run(pending, pending_before, w, cam); has_pending = false; }
                before = w;
                shadow = -1.0;
            }
            if (cam < 0.0) { cam = 0.0; }
            if (start < 0.0) { start = t; run = vec4<f32>(0.0); run_before = before; }
            let d = min((t - start) / vol.shape.y, 60.0);
            run = vec4<f32>(run.x + s, run.y + s * exp(-d), run.z + s * exp(d), t - start);
        } else {
            if (start >= 0.0) { pending = run; pending_before = run_before; has_pending = true; start = -1.0; }
            shadow = max(shadow, 0.0) + dt;
            sum += s;
        }
    }
    if (shadow >= 0.0) {
        let w = shadow_weight(shadow);
        if (cam < 0.0) { cam = w; }
        if (has_pending) { sum += close_run(pending, pending_before, w, cam); }
    }
    if (start >= 0.0) { sum += close_run(run, run_before, 0.0, max(cam, 0.0)); }
    let light = vol.tint.rgb * sum * phase(dot(ray, view.extra.xyz), vol.march.z) * 12.566371;
    return vec4<f32>(light, dist);
}

fn view_dist(p: vec2<i32>) -> f32 {
    let d = textureLoad(depth, p, 0);
    if (d <= 0.0) { return SKY_DIST; }
    return length(fx_view_pos(vec2<f32>(p) + 0.5, d));
}

// crate::volumetric_light::flatten's surroundings: the depth-weighted mean of the pixel's own light and FLAT_TAPS
// march samples on a golden-angle spiral disc of vol.flat.y target pixels (fixed, so stable from frame to frame).
const FLAT_TAPS: i32 = 12;
fn surroundings(centre: vec2<f32>, z: f32, own: vec3<f32>) -> vec3<f32> {
    let lo_size = vec2<i32>(textureDimensions(scatter_tex));
    var sum = own;
    var weight = 1.0;
    for (var i = 0; i < FLAT_TAPS; i += 1) {
        let k = f32(i) + 0.5;
        let a = k * 2.3999632;
        let o = vec2<f32>(cos(a), sin(a)) * vol.flat.y * sqrt(k / f32(FLAT_TAPS));
        let q = clamp(vec2<i32>(round(centre + o)), vec2<i32>(0), lo_size - 1);
        let s = textureLoad(scatter_tex, q, 0);
        let w = exp(-abs(s.a - z) / (0.05 * min(z, s.a) + 0.1));
        sum += s.rgb * w;
        weight += w;
    }
    return sum / weight;
}

@fragment
fn composite(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let p = vec2<i32>(floor(in.position.xy));
    let base = textureLoad(source, p, 0);
    if (!inside_viewport(in.position.xy)) { return base; }
    let z = view_dist(p);
    let lo_size = vec2<i32>(textureDimensions(scatter_tex));
    let centre = in.position.xy * 0.5 - 0.5;
    let origin = vec2<i32>(floor(centre));
    let f = centre - floor(centre);
    var sum = vec3<f32>(0.0);
    var weight = 0.0;
    var nearest = vec3<f32>(0.0);
    var nearest_dz = 1.0e9;
    for (var y = -1; y <= 2; y += 1) {
        for (var x = -1; x <= 2; x += 1) {
            let q = clamp(origin + vec2<i32>(x, y), vec2<i32>(0), lo_size - 1);
            let s = textureLoad(scatter_tex, q, 0);
            let o = vec2<f32>(f32(x), f32(y)) - f;
            let spatial = exp(-0.5 * dot(o, o));
            let dz = abs(s.a - z);
            let w = spatial * exp(-dz / (0.05 * min(z, s.a) + 0.1));
            sum += s.rgb * w;
            weight += w;
            if (dz < nearest_dz) { nearest_dz = dz; nearest = s.rgb; }
        }
    }
    let own = select(nearest, sum / weight, weight > 1.0e-3);
    let light = own - vol.flat.x * min(own, surroundings(centre, z, own));
    if (vol.extra.y > 0.5) { return vec4<f32>(light * vol.extra.y, 1.0); }
    return vec4<f32>(base.rgb + light, base.a);
}
