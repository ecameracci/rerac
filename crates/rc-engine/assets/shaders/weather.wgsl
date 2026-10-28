// Particles Enhanced's rain and snow (weather_render.rs): the game's type-0 / type-73 records drawn as thin
// motion-blurred streaks and soft fluttering flakes, each flake record with COPIES - 1 extra flakes around it. Per
// vertex: quad << 2 | corner (particle_render's quad mesh), quad = sprite index * COPIES + copy. Header: (count, ticks, fog RGBA8, 0). Rain: p = (tail, half-width), q = (head, 0), c = (RGBA tail, 1,
// RGBA head, index). Snow: p = (centre, a.x), q = (a.y, b.x, b.y, 0) in 512x416 game pixels, c = (RGBA, 2, 0, index).
// RGBA bytes as the game's (0x80 = 1.0, alpha already faded by PartProc's near / far rule). Output premultiplied
// (One, OneMinusSrcAlpha) in linear light; soft where it meets the opaque scene (scene_copy's depth, 1.5 units, as
// Particles Soft).

#import bevy_pbr::view_transformations::{position_world_to_clip, depth_ndc_to_view_z}
#import bevy_pbr::mesh_view_bindings::view

struct Sprite {
    p: vec4<f32>,
    q: vec4<f32>,
    c: vec4<u32>,
}

struct Sprites {
    head: vec4<u32>,
    items: array<Sprite>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<storage, read> sprites: Sprites;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var scene_depth: texture_2d<f32>;

const SOFT: f32 = 1.5;
// The flake's share of the game's quad (its texture is a small dot in the middle of the sprite).
const FLAKE: f32 = 0.32;
// The share of a flake's coverage that hides the scene (the rest adds, as the game's additive flake).
const FLAKE_COVER: f32 = 0.3;
// The flakes' largest sideways flutter (world units).
const FLUTTER: f32 = 0.18;
// Quads per sprite (weather_render.rs FLAKE_COPIES): a snow record draws its flake and COPIES − 1 extra ones up to
// EXTRA_SPREAD units off on each axis; a rain record only the first.
const COPIES: u32 = 4u;
const EXTRA_SPREAD: vec3<f32> = vec3<f32>(2.2, 2.6, 2.2);

fn half_w() -> f32 { return 256.0 * 0.775 * view.clip_from_view[1][1] / view.clip_from_view[0][0]; }

fn hash(x: u32) -> u32 {
    var v = x * 747796405u + 2891336453u;
    v = ((v >> ((v >> 28u) + 4u)) ^ v) * 277803737u;
    return (v >> 22u) ^ v;
}

fn unit(x: u32) -> f32 { return f32(hash(x) & 0xffffu) / 65535.0; }

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + 0.055) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}

fn bytes(c: u32) -> vec4<f32> {
    return vec4<f32>(f32(c & 0xffu), f32((c >> 8u) & 0xffu), f32((c >> 16u) & 0xffu), f32(c >> 24u));
}

fn soft_fade(frag: vec4<f32>) -> f32 {
    let size = vec2<i32>(textureDimensions(scene_depth));
    if (size.x <= 1) { return 1.0; }
    let d = textureLoad(scene_depth, clamp(vec2<i32>(floor(frag.xy)), vec2<i32>(0), size - 1), 0).r;
    if (d <= 0.0) { return 1.0; }
    return saturate((depth_ndc_to_view_z(frag.z) - depth_ndc_to_view_z(d)) / SOFT);
}

struct WeatherVertex {
    @location(0) sprite_corner: u32,
}

struct WeatherOut {
    @builtin(position) position: vec4<f32>,
    // Rain: (across in half-widths, along 0 tail .. 1 head). Snow: the sprite's ST.
    @location(0) uv: vec2<f32>,
    // Opacity before the profile (0..), and the flake's blur (0 sharp .. ).
    @location(1) alpha: f32,
    @location(2) blur: f32,
    @location(3) @interpolate(flat) style: u32,
}

fn culled() -> WeatherOut {
    var out: WeatherOut;
    out.position = vec4<f32>(2.0, 2.0, 2.0, 1.0);
    return out;
}

@vertex
fn vertex(v: WeatherVertex) -> WeatherOut {
    let i = (v.sprite_corner >> 2u) / COPIES;
    let j = (v.sprite_corner >> 2u) % COPIES;
    let k = v.sprite_corner & 3u;
    if (i >= sprites.head.x) { return culled(); }
    let s = sprites.items[i];
    if (j != 0u && s.c.y != 2u) { return culled(); }
    var out: WeatherOut;
    out.style = s.c.y;
    let vp = view.viewport.zw;
    let res = max(vp.y / 720.0, 1.0);
    if (s.c.y == 1u) {
        let a = position_world_to_clip(s.p.xyz);
        let b = position_world_to_clip(s.q.xyz);
        if (a.w < 0.05 || b.w < 0.05) { return culled(); }
        let sa = a.xy / a.w * 0.5 * vp;
        let sb = b.xy / b.w * 0.5 * vp;
        let d = sb - sa;
        let len = length(d);
        let dir = select(vec2<f32>(0.0, 1.0), d / len, len > 1e-3);
        let n = vec2<f32>(-dir.y, dir.x);
        // The game's half-width at the streak's depth, in frame pixels; drawn about one pixel wide, its coverage kept.
        let wpx = s.p.w * view.clip_from_view[1][1] * 0.5 * vp.y / (0.5 * (a.w + b.w));
        let hw = clamp(wpx, 0.55 * res, 1.1 * res);
        let energy = clamp(wpx / hw, 1.0, 1.4);
        let head = k >= 2u;
        let side = select(1.0, -1.0, (k & 1u) == 1u);
        var clip = select(a, b, head);
        let off = n * (2.0 * hw * side) + dir * select(-hw, hw, head);
        clip = vec4<f32>(clip.xy + off / (0.5 * vp) * clip.w, clip.zw);
        out.position = clip;
        out.uv = vec2<f32>(2.0 * side, select(0.0, 1.0, head));
        let c = bytes(select(s.c.x, s.c.z, head));
        out.alpha = c.a / 128.0 * energy;
        out.blur = 0.0;
        return out;
    }
    // Snow. Copy j > 0 is an extra flake at a hashed offset from the record's (within EXTRA_SPREAD), sized for its
    // own depth, a little smaller and fainter.
    let seed = s.c.w + j * 0x10000u;
    var centre = s.p.xyz;
    var size_k = 1.0;
    var fade = 1.0;
    if (j != 0u) {
        let o = vec3<f32>(unit(seed + 606u), unit(seed + 707u), unit(seed + 808u)) * 2.0 - 1.0;
        centre = centre + o * EXTRA_SPREAD;
        let w0 = position_world_to_clip(s.p.xyz).w;
        let w1 = position_world_to_clip(centre).w;
        if (w1 < 0.3) { return culled(); }
        size_k = (max(w0, 0.0) + 0.5) / (w1 + 0.5) * (0.6 + 0.3 * unit(seed + 909u));
        fade = 0.85;
    }
    let t = f32(sprites.head.y) / 60.0;
    let ph = vec3<f32>(unit(seed), unit(seed + 101u), unit(seed + 202u)) * 6.2831853;
    let f = vec3<f32>(0.9, 1.3, 2.3) * (0.8 + 0.4 * unit(seed + 303u));
    let amp = FLUTTER * (0.45 + 0.55 * unit(seed + 404u));
    let sway = vec3<f32>(
        sin(t * f.x + ph.x) + 0.35 * sin(t * f.z + ph.z),
        0.25 * sin(t * f.z * 0.7 + ph.y),
        cos(t * f.y + ph.y) + 0.35 * cos(t * f.z + ph.x),
    ) * amp;
    var clip = position_world_to_clip(centre + sway);
    let depth = clip.w;
    let blur = saturate((depth - 12.0) / 30.0) * 0.8 + saturate((1.6 - depth) / 1.2) * 1.2;
    let scale = FLAKE * size_k * (0.7 + 0.6 * unit(seed + 505u)) * (1.0 + blur);
    let a = vec2<f32>(s.p.w, s.q.x) * scale;
    let b = s.q.yz * scale;
    var corner = a;
    var st = vec2<f32>(0.0, 0.0);
    if (k == 1u) { corner = b; st = vec2<f32>(1.0, 0.0); }
    if (k == 2u) { corner = -b; st = vec2<f32>(0.0, 1.0); }
    if (k == 3u) { corner = -a; st = vec2<f32>(1.0, 1.0); }
    clip.x += corner.x * (1.0 / half_w()) * clip.w;
    clip.y -= corner.y * (1.0 / 208.0) * clip.w;
    out.position = clip;
    out.uv = st;
    out.alpha = fade * bytes(s.c.x).a / 128.0 / ((1.0 + blur) * (1.0 + blur));
    out.blur = blur;
    return out;
}

@fragment
fn fragment(in: WeatherOut) -> @location(0) vec4<f32> {
    let fog = bytes(sprites.head.z).rgb / 255.0;
    let soft = soft_fade(in.position);
    if (in.style == 1u) {
        let x = in.uv.x;
        let along = in.uv.y;
        let profile = exp(-x * x * 0.9);
        let taper = smoothstep(0.0, 0.45, along) * (1.0 - 0.5 * smoothstep(0.9, 1.0, along));
        let a = saturate(in.alpha * profile * taper * soft * 0.9);
        let col = srgb_to_linear(mix(vec3<f32>(0.72, 0.76, 0.84), fog, 0.3) + 0.08);
        return vec4<f32>(col * a, a);
    }
    let r = length(in.uv * 2.0 - 1.0);
    let core = mix(0.35, 0.0, saturate(in.blur));
    var profile = 1.0 - smoothstep(core, 1.0, r);
    profile = profile * profile;
    let a = saturate(in.alpha * profile * soft * 1.6);
    let col = srgb_to_linear(mix(vec3<f32>(0.95, 0.97, 1.0), fog, 0.15));
    return vec4<f32>(col * a, a * FLAKE_COVER);
}
