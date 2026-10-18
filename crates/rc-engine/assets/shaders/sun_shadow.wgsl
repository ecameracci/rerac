// Shadows Dynamic, the receiver (crate::sun_shadow): each pixel's world position from the depth buffer, looked up in
// the sun's cascaded shadow map; the frame is multiplied by 1 − strength where a light-facing surface is shadowed.
// view.params = (strength, cascade count, maximum distance, fade start), view.extra.xyz = toward the light (world).

#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput
#import rerac::fx_view::{view, fx_view_pos, fx_normal}

struct SunCascades {
    clip_from_world: array<mat4x4<f32>, 4>,
    far: vec4<f32>,
    texel: vec4<f32>,
}

@group(0) @binding(1) var<uniform> sun: SunCascades;
#ifdef MULTISAMPLED
@group(0) @binding(2) var depth: texture_depth_multisampled_2d;
#else
@group(0) @binding(2) var depth: texture_depth_2d;
#endif
@group(0) @binding(3) var shadow_map: texture_depth_2d_array;
@group(0) @binding(4) var shadow_compare: sampler_comparison;

fn depth_at(p: vec2<i32>) -> f32 {
    let size = vec2<i32>(textureDimensions(depth));
    return textureLoad(depth, clamp(p, vec2<i32>(0), size - 1), 0);
}

fn pos_at(p: vec2<i32>) -> vec3<f32> {
    return fx_view_pos(vec2<f32>(p) + 0.5, depth_at(p));
}

// 1 lit, 0 shadowed: 3×3 comparison taps (each bilinear) around the receiver's texel.
fn visibility(world: vec3<f32>, cascade: i32) -> f32 {
    let c = sun.clip_from_world[cascade] * vec4<f32>(world, 1.0);
    let ndc = c.xyz / c.w;
    let uv = ndc.xy * vec2<f32>(0.5, -0.5) + 0.5;
    if (any(uv < vec2<f32>(0.0)) || any(uv > vec2<f32>(1.0))) { return 1.0; }
    let step = 1.0 / vec2<f32>(textureDimensions(shadow_map));
    var lit = 0.0;
    for (var y = -1; y <= 1; y += 1) {
        for (var x = -1; x <= 1; x += 1) {
            lit += textureSampleCompareLevel(shadow_map, shadow_compare, uv + vec2<f32>(f32(x), f32(y)) * step, cascade, ndc.z);
        }
    }
    return lit / 9.0;
}

fn skip(code: vec3<f32>) -> vec4<f32> { return vec4<f32>(select(vec3<f32>(1.0), code, view.extra.w > 1.5), 1.0); }

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let p = vec2<i32>(floor(in.position.xy));
    let d = depth_at(p);
    if (d <= 0.0) { return skip(vec3<f32>(0.4, 0.4, 1.0)); }
    let c = fx_view_pos(in.position.xy, d);
    let dist = -c.z;
    if (dist >= view.params.z) { return skip(vec3<f32>(0.4, 1.0, 0.4)); }
    let n_view = fx_normal(c, pos_at(p - vec2<i32>(1, 0)), pos_at(p + vec2<i32>(1, 0)), pos_at(p - vec2<i32>(0, 1)), pos_at(p + vec2<i32>(0, 1)));
    let n = normalize((view.world_from_view * vec4<f32>(n_view, 0.0)).xyz);
    let l = view.extra.xyz;
    let facing = smoothstep(0.0, 0.25, dot(n, l));
    if (facing <= 0.0) { return skip(vec3<f32>(1.0, 0.4, 1.0)); }

    var cascade = -1;
    let count = i32(view.params.y);
    for (var i = 0; i < count; i += 1) {
        if (dist < sun.far[i]) { cascade = i; break; }
    }
    if (cascade < 0) { return skip(vec3<f32>(0.4, 1.0, 1.0)); }
    let texel = sun.texel[cascade];
    let world = (view.world_from_view * vec4<f32>(c, 1.0)).xyz + n * (texel * 1.8) + l * max(texel, 0.02);
    var vis = visibility(world, cascade);
    if (view.extra.w > 0.5) {
        let tint = array<vec3<f32>, 4>(vec3<f32>(1.0, 0.6, 0.6), vec3<f32>(0.6, 1.0, 0.6), vec3<f32>(0.6, 0.6, 1.0), vec3<f32>(1.0, 1.0, 0.6));
        return vec4<f32>(tint[cascade] * mix(0.15, 1.0, vis), 1.0);
    }
    vis = mix(vis, 1.0, smoothstep(view.params.w, view.params.z, dist));
    let k = 1.0 - view.params.x * (1.0 - vis) * facing;
    return vec4<f32>(k, k, k, 1.0);
}
