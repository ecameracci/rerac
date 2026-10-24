// Water Enhanced under water (crate::underwater_render): the in-water part of each view ray absorbed and filled with
// the water's colour, the light fading with depth, caustics on the surfaces below the plane, light shafts and a wobble.
// view.extra = (target width, height, -, -); `uw` = crate::underwater_render::UnderwaterUniform. Bevy world axes (y up).

#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput
#import rerac::fx_view::{view, fx_view_pos, fx_normal, fx_noise}

#ifdef MULTISAMPLED
@group(0) @binding(1) var depth: texture_depth_multisampled_2d;
#else
@group(0) @binding(1) var depth: texture_depth_2d;
#endif
@group(0) @binding(2) var source: texture_2d<f32>;
@group(0) @binding(3) var linear: sampler;

struct Underwater {
    // Plane y, plane known (0 / 1), fade, time.
    plane: vec4<f32>,
    // Water colour (linear), strength.
    water: vec4<f32>,
    // Absorption per unit (r, g, b), wobble in target pixels.
    absorb: vec4<f32>,
    // Light falloff per unit of depth (r, g, b), shaft taps.
    light: vec4<f32>,
    // Toward the light inside the water (unit, up), debug mode.
    sun: vec4<f32>,
}
@group(0) @binding(4) var<uniform> uw: Underwater;

// The sky's distance (depth 0) and the furthest a shaft tap goes (world units).
const FAR: f32 = 1000.0;
const SHAFT_RANGE: f32 = 28.0;

fn depth_at(p: vec2<i32>) -> f32 {
    let lo = vec2<i32>(view.viewport.xy);
    let hi = lo + vec2<i32>(view.viewport.zw) - 1;
    return textureLoad(depth, clamp(p, lo, hi), 0);
}

fn pos_at(p: vec2<i32>) -> vec3<f32> {
    return fx_view_pos(vec2<f32>(p) + 0.5, max(depth_at(p), 1e-7));
}

// A gradient per lattice point (−1..1) from fract arithmetic only: a sine hash loses its precision at the levels'
// coordinates (hundreds of units) and shows as blocks.
fn hash2(p: vec2<f32>) -> vec2<f32> {
    var q = fract(vec3<f32>(p.xyx) * vec3<f32>(0.1031, 0.1030, 0.0973));
    q += dot(q, q.yzx + 33.33);
    return fract((q.xx + q.yz) * q.zy) * 2.0 - 1.0;
}

// Gradient noise, about −0.7..0.7.
fn gnoise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = dot(hash2(i), f);
    let b = dot(hash2(i + vec2<f32>(1.0, 0.0)), f - vec2<f32>(1.0, 0.0));
    let c = dot(hash2(i + vec2<f32>(0.0, 1.0)), f - vec2<f32>(0.0, 1.0));
    let d = dot(hash2(i + vec2<f32>(1.0, 1.0)), f - vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

// A caustic layer: the sharpened zero lines of warped, drifting noise (0..1).
fn caustic_layer(p: vec2<f32>, t: f32, drift: vec2<f32>) -> f32 {
    let v = 1.0 - abs(gnoise(p + drift * t) * 2.2);
    return pow(clamp(v, 0.0, 1.0), 6.0);
}

// The caustics at plane point `xz`, each layer faded once its cells (1 / frequency) shrink below about two pixels of
// `footprint` (world units per pixel).
fn caustics(xz: vec2<f32>, t: f32, footprint: f32) -> f32 {
    let warp = vec2<f32>(gnoise(xz * 0.35 + vec2<f32>(t * 0.05, 0.0)), gnoise(xz * 0.35 + vec2<f32>(4.1, -t * 0.045)));
    let p = xz * 0.9 + warp * 1.1;
    let fa = 1.0 - smoothstep(0.12, 0.35, footprint * 0.9);
    let fb = 1.0 - smoothstep(0.12, 0.35, footprint * 1.5);
    let a = caustic_layer(p, t, vec2<f32>(0.21, 0.13));
    let b = caustic_layer(p * 1.67 + 3.7, t, vec2<f32>(-0.17, 0.23));
    return a * fa * 0.9 + b * fb * 0.6 + (0.9 * (1.0 - fa) + 0.6 * (1.0 - fb)) * 0.05;
}

// The shafts' coarse pattern on the plane (0..1).
fn shaft_pattern(xz: vec2<f32>, t: f32) -> f32 {
    let w = gnoise(xz * 0.08 + vec2<f32>(t * 0.02, 1.3)) * 2.0;
    let v = 1.0 - abs(gnoise(xz * 0.22 + vec2<f32>(w, t * 0.04)) * 2.0);
    return pow(clamp(v, 0.0, 1.0), 5.0);
}

// The in-water part (t0, t1) of the ray eye_y + t·dir_y, 0 ≤ t ≤ ts (crate::underwater_render::water_segment).
fn water_segment(eye_y: f32, dir_y: f32, ts: f32) -> vec2<f32> {
    if (uw.plane.y < 0.5) { return vec2<f32>(0.0, ts); }
    let h = uw.plane.x;
    if (eye_y <= h) {
        var exit = 3.0e38;
        if (dir_y > 1e-5) { exit = (h - eye_y) / dir_y; }
        return vec2<f32>(0.0, min(ts, exit));
    }
    if (dir_y < -1e-5) {
        let enter = (h - eye_y) / dir_y;
        return vec2<f32>(min(enter, ts), ts);
    }
    return vec2<f32>(0.0);
}

fn transmittance(sigma: vec3<f32>, d: f32) -> vec3<f32> {
    return exp(-sigma * max(d, 0.0));
}

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let size = view.extra.xy;
    let lo = view.viewport.xy;
    let hi = lo + view.viewport.zw;
    let here = in.position.xy;
    if (any(here < lo) || any(here >= hi)) {
        return textureLoad(source, vec2<i32>(floor(here)), 0);
    }
    let k = uw.plane.z;
    let t = uw.plane.w;
    let s = uw.water.w;

    // The wobble: a slow sine offset of where the pixel reads the picture and its depth.
    let vuv = (here - lo) / view.viewport.zw;
    let wob = vec2<f32>(sin(vuv.y * 17.0 + t * 1.7) + 0.5 * sin(vuv.y * 31.0 - t * 2.3), cos(vuv.x * 13.0 + t * 1.3)) * uw.absorb.w * k;
    let at = clamp(here + wob, lo + 0.5, hi - 0.5);
    let px = vec2<i32>(floor(at));
    let src = textureSampleLevel(source, linear, at / size, 0.0);
    var col = src.rgb;

    // The pixel's own centre, as its neighbours' in `fx_normal`.
    let d = depth_at(px);
    let c = pos_at(px);
    let eye = view.world_from_view[3].xyz;
    let wp = (view.world_from_view * vec4<f32>(c, 1.0)).xyz;
    let dir = normalize(wp - eye);
    var ts = FAR;
    if (d > 0.0) { ts = distance(wp, eye); }
    let seg = water_segment(eye.y, dir.y, ts);
    let len = max(seg.y - seg.x, 0.0);
    let h = select(eye.y + 4.0, uw.plane.x, uw.plane.y > 0.5);

    // Light from above on a surface below the plane, and its caustics.
    var caust = 0.0;
    var nrm = vec3<f32>(0.0);
    if (d > 0.0 && wp.y < h) {
        let depth_below = h - wp.y;
        col *= mix(vec3<f32>(1.0), transmittance(uw.light.xyz, depth_below), k);
        let n_view = fx_normal(c, pos_at(px - vec2<i32>(1, 0)), pos_at(px + vec2<i32>(1, 0)), pos_at(px - vec2<i32>(0, 1)), pos_at(px + vec2<i32>(0, 1)));
        let n = normalize((view.world_from_view * vec4<f32>(n_view, 0.0)).xyz);
        nrm = n;
        nrm = n;
        // Walls get none: a pattern projected along the sun streaks down them.
        let facing = smoothstep(0.35, 0.8, dot(n, uw.sun.xyz));
        let q = wp + uw.sun.xyz * (depth_below / max(uw.sun.y, 0.2));
        // A pixel's footprint on the surface, longer where it is seen edge-on.
        let footprint = ts * 2.0 / (view.clip_from_view[1][1] * view.viewport.w) / max(abs(dot(n, dir)), 0.15);
        let fade = exp(-depth_below * 0.06) * (1.0 - smoothstep(25.0, 60.0, ts));
        caust = caustics(q.xz, t, footprint) * facing * fade;
        col += (col * 2.0 + uw.water.rgb * 0.5 + vec3<f32>(0.02, 0.04, 0.05)) * caust * s * k;
    }

    // Absorption and in-scatter along the in-water part, toward the water's colour, darker the deeper the ray is (the
    // game's fog has already taken the picture toward that colour: this mostly shifts the hue and adds depth).
    let mid_y = eye.y + dir.y * 0.5 * (seg.x + seg.y);
    let lit = transmittance(uw.light.xyz, h - min(mid_y, h)) * 0.8 + 0.1;
    let tr = transmittance(uw.absorb.xyz, len);
    let fogged = col * tr + uw.water.rgb * lit * (vec3<f32>(1.0) - tr);
    col = mix(col, fogged, k);

    // Light shafts: taps along the in-water ray, each lit by the pattern where the sun's line from it meets the plane.
    let taps = i32(uw.light.w);
    let t0 = seg.x;
    let t1 = min(seg.y, seg.x + SHAFT_RANGE);
    var shafts = 0.0;
    if (t1 > t0 && taps > 0) {
        let step = (t1 - t0) / f32(taps);
        var tt = t0 + step * fx_noise(here);
        for (var i = 0; i < taps; i += 1) {
            let p = eye + dir * tt;
            let below = h - p.y;
            if (below > 0.0) {
                let q = p + uw.sun.xyz * (below / max(uw.sun.y, 0.2));
                shafts += shaft_pattern(q.xz, t) * exp(-below * 0.09 - tt * 0.07) * step;
            }
            tt += step;
        }
        let phase = 0.6 + 0.8 * pow(max(dot(dir, uw.sun.xyz), 0.0), 3.0);
        shafts *= phase * 0.022;
    }
    col += (uw.water.rgb * 0.6 + vec3<f32>(0.3, 0.42, 0.5)) * shafts * s * k;

    let dbg = u32(uw.sun.w + 0.5);
    if (dbg == 1u) { return vec4<f32>(vec3<f32>(len / 32.0), src.a); }
    if (dbg == 2u) { return vec4<f32>(vec3<f32>(caust), src.a); }
    if (dbg == 3u) { return vec4<f32>(vec3<f32>(shafts * 4.0), src.a); }
    if (dbg == 4u) { return vec4<f32>(nrm * 0.5 + 0.5, src.a); }
    if (dbg == 4u) { return vec4<f32>(nrm * 0.5 + 0.5, src.a); }
    return vec4<f32>(col, src.a);
}
