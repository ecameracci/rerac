// Particles Enhanced (crate::particle_lights): the bright effects' point lights on the opaque world. Each pixel's
// world position and normal from the depth buffer; per light the window (1 − d²/r²)² times a wrapped n·l; the frame is
// multiplied by 1 + L, drawn three times (8-bit target: the blend source is clamped to 1), so each draw returns
// ∛(1 + L) − 1.
// view.params = (light count, intensity factor).

#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput
#import rerac::fx_view::{view, fx_view_pos, fx_normal}

struct ParticleLights {
    // xyz position (Bevy world), w radius.
    pos_radius: array<vec4<f32>, 64>,
    // rgb colour × intensity × fade (linear).
    colour: array<vec4<f32>, 64>,
}

@group(0) @binding(1) var<uniform> lights: ParticleLights;
#ifdef MULTISAMPLED
@group(0) @binding(2) var depth: texture_depth_multisampled_2d;
#else
@group(0) @binding(2) var depth: texture_depth_2d;
#endif

// How far round the terminator a light still reaches (the rebuilt normals are coarse at edges).
const WRAP: f32 = 0.35;

fn depth_at(p: vec2<i32>) -> f32 {
    let size = vec2<i32>(textureDimensions(depth));
    return textureLoad(depth, clamp(p, vec2<i32>(0), size - 1), 0);
}

fn pos_at(p: vec2<i32>) -> vec3<f32> {
    return fx_view_pos(vec2<f32>(p) + 0.5, depth_at(p));
}

fn light_window(d2: f32, r: f32) -> f32 {
    let x = max(1.0 - d2 / (r * r), 0.0);
    return x * x;
}

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let p = vec2<i32>(floor(in.position.xy));
    let d = depth_at(p);
    if (d <= 0.0) { return vec4<f32>(0.0); }
    let c = fx_view_pos(in.position.xy, d);
    let world = (view.world_from_view * vec4<f32>(c, 1.0)).xyz;
    let count = u32(view.params.x);
    var reached = false;
    for (var i = 0u; i < count; i += 1u) {
        let l = lights.pos_radius[i];
        let v = l.xyz - world;
        if (dot(v, v) < l.w * l.w) { reached = true; break; }
    }
    if (!reached) { return vec4<f32>(0.0); }
    let n_view = fx_normal(c, pos_at(p - vec2<i32>(1, 0)), pos_at(p + vec2<i32>(1, 0)), pos_at(p - vec2<i32>(0, 1)), pos_at(p + vec2<i32>(0, 1)));
    let n = normalize((view.world_from_view * vec4<f32>(n_view, 0.0)).xyz);
    var sum = vec3<f32>(0.0);
    for (var i = 0u; i < count; i += 1u) {
        let l = lights.pos_radius[i];
        let v = l.xyz - world;
        let d2 = dot(v, v);
        if (d2 >= l.w * l.w) { continue; }
        let facing = clamp((dot(n, v * inverseSqrt(max(d2, 1.0e-6))) + WRAP) / (1.0 + WRAP), 0.0, 1.0);
        sum += lights.colour[i].rgb * light_window(d2, l.w) * facing;
    }
    let k = sum * view.params.y;
    return vec4<f32>(pow(vec3<f32>(1.0) + k, vec3<f32>(1.0 / 3.0)) - 1.0, 0.0);
}

// RC_PARTICLE_LIGHTS_DEBUG: each light's sphere as its outline and its centre, over everything.
@fragment
fn debug(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let dir = normalize(fx_view_pos(in.position.xy, 1.0));
    let px = length(fx_view_pos(in.position.xy + vec2<f32>(1.0, 0.0), 1.0) - fx_view_pos(in.position.xy, 1.0)) / length(fx_view_pos(in.position.xy, 1.0));
    let count = u32(view.params.x);
    var out = vec4<f32>(0.0);
    for (var i = 0u; i < count; i += 1u) {
        let l = lights.pos_radius[i];
        let centre = (view.view_from_world * vec4<f32>(l.xyz, 1.0)).xyz;
        let t = dot(centre, dir);
        if (t <= 0.0) { continue; }
        let miss = length(centre - dir * t);
        let col = lights.colour[i].rgb / max(max(lights.colour[i].r, max(lights.colour[i].g, lights.colour[i].b)), 1.0e-4);
        // 1.5 pixels wide at the light's distance.
        let edge = 1.5 * px * t;
        if (abs(miss - l.w) < edge) { out = vec4<f32>(col, 0.9); }
        else if (miss < max(0.15, 2.0 * edge)) { out = vec4<f32>(1.0, 1.0, 1.0, 1.0); }
        else if (miss < l.w && out.a < 0.1) { out = vec4<f32>(col, 0.08); }
    }
    return out;
}
