// Level-code water (docs/plan/world_animation.md §1, §2; Rust side and every constant: water_render.rs).
//
// One pipeline for the four draw kinds (params.misc.y):
//   0 / 1  strip layer 1 / 2 (FUN_002b96e0 + FUN_00262618): uv = base + scroll ± wobble[hash & 7], the wobble
//          table computed per frame on the CPU with the VU0 sine (rc_game::water::wobble_table);
//          z = z0·w + z1·(1 − w) with w = the class's z blend (761 / 1225 bob, else the stored 0).
//   2 / 3  ripple patch water / env pass (FUN_002b91c8): the CPU built positions, grey and both UV sets.
//   4      port-only chute sheet (crate::chute_water): uv = (across, along, flow dir x, flow dir y) in game units,
//          z1 = the edge fade (the output alpha), params.misc.z = the flow speed (u/s); Water Enhanced only (nothing
//          is drawn otherwise).
// Strip vertex: position = (game x, game y, z0), z1, uv = (u1, v1, u2, v2), tag = (RGBA, hash group); the
// per-frame w, scroll and wobble come from this draw's `frame` record (storage, index = params.ids.x).
// Ripple vertex: tag = (sub-block | strip index << 8, grid vertex); position, grey and UVs come from the
// patch's `ripple` record (first word params.ids.y: mask, 17x17 x (x, y, z, env u, env v, RGBA), then the 46
// water UVs at params.ids.z); a sub-block outside the mask is dropped.
//
// Fragment: GS MODULATE (C = Ct·Cv >> 7), then fog (PRIM FGE, VU1 program 57843's F like the tfrag program),
// then ALPHA = FIX<<32 | 0x64: C = Cd + (Cs − Cd)·FIX/128. FIX is baked into the output alpha and blended
// with SrcAlpha / OneMinusSrcAlpha (gs_state::GS_BLEND). The vertex alpha is 0, so As = 0 always fails the
// TEST (AREF 0x60, AFAIL RGB_ONLY): colour only, never Z (depth write off, GEQUAL test).

//
// Water Enhanced (crate::graphics; look.misc.x = 1, clear water only): the strip and ripple water passes (kinds 0, 2)
// draw the whole surface from the opaque scene behind it (crate::scene_copy), opaque over the frame; their second
// layers (kinds 1, 3) draw nothing. Per pixel:
// * a wave normal: a sum of travelling sine waves over the world position and the frame time (each fading out with the
//   distance, RIPPLE_LOD wavelengths), plus the ripple
//   module's slope on the ripple patches (from the patch's height grid, per vertex) and the expanding rings where
//   Ratchet meets the water (`rings`, crate::water_render::WaterRings);
// * the bed: the scene copy at the pixel bent by the waves (refraction; unbent where the bent pixel is in front of
//   the surface), absorbed over the water's thickness toward the water texture's own colour, with caustics (ridges of
//   moving gradient noise, fading with the depth under the surface) where the key light reaches;
// * a reflection along the wave normal, traced in screen space against the scene copy (the fog colour where the ray
//   leaves the screen), on the mirrored plane the planar reflection instead (`planar`, crate::water_mirror),
//   weighted by a Fresnel term, plus the key light's glint;
// * foam along the shore: thin water where the bed rises toward the surface (not over a flat shallow floor). At the
//   shore line (no thickness) the result is the scene behind, so the edge has no seam.
// A ripple water draw may sit params.misc.w over the game's height (water_render::SURFACE_LIFT), on Enhanced only.
// Pokitaru's ocean (params.misc.w = 1 on a kind 0 draw, crate::ocean_water) takes its own constants (OCEAN_*) and, for a
// reflection that meets nothing, the sky in the scene copy along the ray or its sky colour (params.ids.w) instead of the fog.
// A liquid of another kind (look.misc.x = 2: Gemlik's acid) draws liquid_look.wgsl's look instead (crate::liquid_render).
//
// The swell (crate::water_waves; look.sun.w = its scale, 0: none): on the vertices tagged SWELL_TAG (crate::sea_water's
// split seas) the height of `swell_wave`'s sum times the vertex's shore damping and the far fade, passed on as `swell`;
// elsewhere `swell` = −1 and the fragment damps the slope by the bed's vertical depth instead. Either way the swell's
// slope joins the wave normal.

#import bevy_pbr::view_transformations::{position_world_to_clip, position_world_to_view, depth_ndc_to_view_z, position_ndc_to_view, frag_coord_to_ndc, ndc_to_frag_coord}
#import bevy_pbr::mesh_view_bindings::{view, globals}
#import rerac::liquid_look::{liquid_look, still_frame, DRIFT}

struct WaterFog {
    color: vec4<f32>,
    params: vec4<f32>,
}

struct WaterParams {
    // x = FIX / 128, y = kind, z = an even absorption or a chute's flow speed, w = the Enhanced surface lift (game units;
    // ripple water only) or 1: Pokitaru's ocean (kind 0).
    misc: vec4<f32>,
    // x = frame slot, y = ripple record's first word, z = water UV offset in the record, w = the ocean's sky (RGB).
    ids: vec4<u32>,
}

struct FrameRecord {
    // x = z blend w, yz = this layer's scroll offset.
    misc: vec4<f32>,
    // The eight wobble offsets A·(sin θ, cos θ), two per vec4 (index = hash & 7).
    wobble: array<vec4<f32>, 4>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var tex: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var tex_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var<uniform> fog: WaterFog;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var<uniform> params: WaterParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var<storage, read> frame: array<FrameRecord>;
@group(#{MATERIAL_BIND_GROUP}) @binding(5) var<storage, read> ripple: array<u32>;

struct WaterLook {
    // x = 1: enhanced.
    misc: vec4<f32>,
    // xyz = toward the key light (world).
    sun: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(6) var scene_depth: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(7) var scene_color: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(8) var scene_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(9) var<uniform> look: WaterLook;

const MAX_RINGS: u32 = 16u;

struct WaterRings {
    // x = ring count.
    head: vec4<f32>,
    // (Bevy x, Bevy z, start time in globals.time, strength).
    ring: array<vec4<f32>, MAX_RINGS>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(10) var<storage, read> rings: WaterRings;

struct WaterMirror {
    // x = the mirror's weight while its view is drawn (0..1, crate::water_mirror `fade`), else 0; y = its plane's world y.
    state: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(11) var<storage, read> mirror: WaterMirror;
@group(#{MATERIAL_BIND_GROUP}) @binding(12) var mirror_color: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(13) var mirror_sampler: sampler;

// Grid vertices per patch side (rc_game::water::VG).
const VG: u32 = 17u;

struct WaterVertex {
    @location(0) position: vec3<f32>,
    @location(1) z1: f32,
    @location(2) uv: vec4<f32>,
    @location(3) tag: vec2<u32>,
}

struct WaterVertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    // Gouraud RGB bytes / 128 and GS F / 255, screen-space linear like the GS.
    @location(1) @interpolate(linear) color: vec3<f32>,
    @location(2) @interpolate(linear) fog: f32,
    // The ripple module's surface slope (d height / d Bevy x, d height / d Bevy z); 0 on the strips.
    @location(3) ripple_grad: vec2<f32>,
    // Kind 4: the vertex uv (across, along, flow dir) and the edge fade; 0 otherwise.
    @location(4) flow: vec4<f32>,
    @location(5) fade: f32,
    // The swell's share at this point (−1: the fragment's own damping).
    @location(6) swell: f32,
}

// crate::sea_water::SWELL_TAG; crate::water_waves::SWELL_FADE, WAVE_LOD, SHORE_DEPTH.
const SWELL_TAG: u32 = 0x80000000u;
const SWELL_FADE: vec2<f32> = vec2<f32>(50.0, 100.0);
const WAVE_LOD: f32 = 8.0;
const SHORE_DEPTH: vec2<f32> = vec2<f32>(0.5, 8.0);

// crate::water_waves::WAVES (direction x, z, wavenumber, amplitude), PHASE, SPEED, G.
fn swell_wave(i: i32) -> vec4<f32> {
    var w = array<vec4<f32>, 4>(
        vec4<f32>(0.8, 0.6, 0.2856, 0.11),
        vec4<f32>(0.98058, -0.19612, 0.4488, 0.08),
        vec4<f32>(0.45099, 0.89254, 0.69813, 0.06),
        vec4<f32>(0.92918, 0.36961, 1.05, 0.04),
    );
    return w[i];
}

fn swell_phase(w: vec4<f32>, i: i32, xz: vec2<f32>, t: f32) -> f32 {
    return dot(w.xy, xz) * w.z - 0.6 * sqrt(9.8 * w.z) * t + f32(i) * 1.7;
}

// A wave's share at horizontal distance d from the camera (crate::water_waves::wave_weight).
fn swell_weight(w: vec4<f32>, d: f32) -> f32 {
    let l = 6.2831855 / w.z;
    return 1.0 - smoothstep(WAVE_LOD * l, 2.0 * WAVE_LOD * l, d);
}

fn swell_height(xz: vec2<f32>, t: f32, d: f32) -> f32 {
    var h = 0.0;
    for (var i = 0; i < 4; i += 1) {
        let w = swell_wave(i);
        h += swell_weight(w, d) * w.w * sin(swell_phase(w, i, xz, t));
    }
    return h;
}

fn swell_grad(xz: vec2<f32>, t: f32, d: f32) -> vec2<f32> {
    var g = vec2<f32>(0.0);
    for (var i = 0; i < 4; i += 1) {
        let w = swell_wave(i);
        g += w.xy * (swell_weight(w, d) * w.w * w.z * cos(swell_phase(w, i, xz, t)));
    }
    return g;
}

fn ripple_pos(base: u32, g: u32) -> vec3<f32> {
    let k = base + 4u + g * 6u;
    return vec3<f32>(bitcast<f32>(ripple[k]), bitcast<f32>(ripple[k + 1u]), bitcast<f32>(ripple[k + 2u]));
}

fn wobble(f: FrameRecord, g: u32) -> vec2<f32> {
    let q = f.wobble[g >> 1u];
    return select(q.xy, q.zw, (g & 1u) == 1u);
}

@vertex
fn vertex(v: WaterVertex) -> WaterVertexOutput {
    var out: WaterVertexOutput;
    let kind = u32(params.misc.y + 0.5);
    var pos = v.position;
    var z = v.position.z;
    var uv = v.uv.xy;
    var c = v.tag.x;
    var drop = false;
    var grad = vec2<f32>(0.0);
    out.flow = vec4<f32>(0.0);
    out.fade = 0.0;
    if (kind == 4u) {
        out.flow = v.uv;
        out.fade = v.z1;
    } else if (kind < 2u) {
        let f = frame[params.ids.x];
        let w = f.misc.x;
        z = v.position.z * w + v.z1 * (1.0 - w);
        let wob = wobble(f, v.tag.y & 7u);
        uv = v.uv.xy + f.misc.yz + select(wob, -wob, kind == 1u);
    } else {
        let base = params.ids.y;
        // A sub-block outside the patch's mask is dropped (below).
        drop = ((ripple[base] >> (v.tag.x & 0xffu)) & 1u) == 0u;
        let g = base + 4u + v.tag.y * 6u;
        pos = vec3<f32>(bitcast<f32>(ripple[g]), bitcast<f32>(ripple[g + 1u]), bitcast<f32>(ripple[g + 2u]));
        z = pos.z;
        // Enhanced only: the drawn surface over the game's height (water_render::SURFACE_LIFT).
        if (look.misc.x > 0.5 && textureDimensions(scene_depth).x > 1u) { z += params.misc.w; }
        let k = base + params.ids.z + (v.tag.x >> 8u) * 2u;
        uv = vec2<f32>(bitcast<f32>(ripple[k]), bitcast<f32>(ripple[k + 1u]));
        if (kind == 3u) {
            uv = vec2<f32>(bitcast<f32>(ripple[g + 3u]), bitcast<f32>(ripple[g + 4u]));
        }
        c = ripple[g + 5u];
        // Central differences over the grid neighbours (one-sided at the patch edge), in game space.
        let row = v.tag.y / VG;
        let col = v.tag.y % VG;
        let l = ripple_pos(base, row * VG + max(col, 1u) - 1u);
        let r = ripple_pos(base, row * VG + min(col + 1u, VG - 1u));
        let u = ripple_pos(base, (max(row, 1u) - 1u) * VG + col);
        let d = ripple_pos(base, min(row + 1u, VG - 1u) * VG + col);
        let sx = r.x - l.x;
        let sy = d.y - u.y;
        let gx = select((r.z - l.z) / sx, 0.0, abs(sx) < 1e-4);
        let gy = select((d.z - u.z) / sy, 0.0, abs(sy) < 1e-4);
        // Game y is Bevy −z.
        grad = vec2<f32>(gx, -gy);
    }
    out.ripple_grad = grad;
    // Game (x, y, z) -> Bevy (x, z, -y).
    var p = vec3<f32>(pos.x, z, -pos.y);
    out.swell = -1.0;
    if (look.misc.x > 0.5 && kind == 0u && (v.tag.y & SWELL_TAG) != 0u) {
        let damp = f32((v.tag.y >> 8u) & 0xffffu) / 65535.0;
        let d = distance(p.xz, view.world_position.xz);
        out.swell = look.sun.w * damp;
        p.y += out.swell * (1.0 - smoothstep(SWELL_FADE.x, SWELL_FADE.y, d)) * swell_height(p.xz, globals.time, d);
    }
    out.position = position_world_to_clip(p);
    out.uv = uv;
    out.color = vec3<f32>(f32(c & 0xffu), f32((c >> 8u) & 0xffu), f32((c >> 16u) & 0xffu)) / 128.0;
    let depth = -position_world_to_view(p).z * 1024.0;
    let f = max(min(depth * fog.params.x + fog.params.y, fog.params.w), fog.params.z);
    out.fog = trunc(f) / 255.0;
    if (drop) {
        // All vertices of the sub-block on one point outside the clip volume: no fragments.
        out.position = vec4<f32>(2.0, 2.0, 2.0, 1.0);
    }
    return out;
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + 0.055) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}

// The scene copy's view z at a target pixel (None-like: 0 depth = the sky, returned as −1e6).
fn scene_z(p: vec2<i32>, size: vec2<i32>) -> f32 {
    let d = textureLoad(scene_depth, clamp(p, vec2<i32>(0), size - 1), 0).r;
    if (d <= 0.0) { return -1e6; }
    return depth_ndc_to_view_z(d);
}

// The target pixel of a view position (x: −1 when behind the camera).
fn view_to_coord(p: vec3<f32>) -> vec3<f32> {
    let clip = view.clip_from_view * vec4<f32>(p, 1.0);
    if (clip.w <= 0.0) { return vec3<f32>(-1.0, -1.0, 0.0); }
    let ndc = clip.xy / clip.w;
    return vec3<f32>(ndc_to_frag_coord(ndc), max(abs(ndc.x), abs(ndc.y)));
}

// The sky along view direction `rd` (the ocean's miss): the scene copy where that direction meets the screen if the
// sky shows there, else the ocean's sky colour (params.ids.w, RGB bytes).
fn ocean_sky(vp: vec3<f32>, rd: vec3<f32>, size: vec2<i32>) -> vec3<f32> {
    let s = params.ids.w;
    let fallback = srgb_to_linear(vec3<f32>(f32(s & 0xffu), f32((s >> 8u) & 0xffu), f32((s >> 16u) & 0xffu)) / 255.0);
    let c = view_to_coord(vp + rd * 4096.0);
    if (c.x < 0.0 || c.z > 1.0) { return fallback; }
    if (textureLoad(scene_depth, clamp(vec2<i32>(floor(c.xy)), vec2<i32>(0), size - 1), 0).r > 0.0) { return fallback; }
    let col = textureSampleLevel(scene_color, scene_sampler, c.xy / vec2<f32>(size), 0.0).rgb;
    return mix(fallback, col, 1.0 - smoothstep(0.85, 1.0, c.z));
}

// The reflection along `rd` from view position `vp`, traced against the scene copy (steps growing with the distance,
// then halved four times around the first crossing, which counts when the ray is less than `tol` steps behind the
// scene); `miss` when it meets nothing.
fn reflection(vp: vec3<f32>, rd: vec3<f32>, size: vec2<i32>, miss: vec3<f32>, tol: f32) -> vec3<f32> {
    if (rd.z > 0.35) { return miss; }
    let len = max(0.3, -vp.z * 0.03);
    var p = vp;
    var step = len;
    for (var i = 1; i <= 32; i += 1) {
        step = len * (1.0 + f32(i) * 0.15);
        p = p + rd * step;
        let c = view_to_coord(p);
        if (c.x < 0.0 || c.z > 1.0) { break; }
        let sz = scene_z(vec2<i32>(floor(c.xy)), size);
        if (sz > p.z && sz - p.z < step * tol) {
            var lo = p - rd * step;
            var hi = p;
            for (var j = 0; j < 4; j += 1) {
                let mid = (lo + hi) * 0.5;
                let mc = view_to_coord(mid);
                if (scene_z(vec2<i32>(floor(mc.xy)), size) > mid.z) { hi = mid; } else { lo = mid; }
            }
            let hc = view_to_coord(hi);
            let edge = 1.0 - smoothstep(0.85, 1.0, hc.z);
            let col = textureSampleLevel(scene_color, scene_sampler, hc.xy / vec2<f32>(size), 0.0).rgb;
            return mix(miss, col, edge);
        }
    }
    return miss;
}

// The planar reflection on the mirrored plane (crate::water_mirror): the mirror view at this pixel (drawn mirrored
// left to right: read at −x), bent by the wave normal; `ssr` off the plane and where the bent sample leaves the view.
fn planar(ssr: vec3<f32>, wp: vec3<f32>, n_world: vec3<f32>, frag: vec4<f32>, dist: f32) -> vec3<f32> {
    if (mirror.state.x <= 0.0) { return ssr; }
    let on = 1.0 - smoothstep(0.5, 1.0, abs(wp.y - mirror.state.y));
    if (on <= 0.0) { return ssr; }
    let tilt = (view.view_from_world * vec4<f32>(n_world.x, 0.0, n_world.z, 0.0)).xy;
    let ndc = frag_coord_to_ndc(frag).xy + tilt * (0.22 / (1.0 + dist * 0.06));
    let uv = vec2<f32>(0.5 - 0.5 * ndc.x, 0.5 - 0.5 * ndc.y);
    let inside = min(min(uv.x, 1.0 - uv.x), min(uv.y, 1.0 - uv.y));
    let m = textureSampleLevel(mirror_color, mirror_sampler, clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0)), 0.0).rgb;
    return mix(ssr, m, mirror.state.x * on * smoothstep(0.0, 0.04, inside));
}

// Wavelengths from the camera over which each ripple wave fades out (whole to RIPPLE_LOD, none from twice that): the
// short ones would alias into sparkle in the distance.
const RIPPLE_LOD: f32 = 30.0;

// Pokitaru's ocean (params.misc.w = 1, crate::ocean_water): the ripple normals' share; the absorption per unit of path
// (times params.misc.x), red first so the sand turns turquoise with depth, and the water's colour already scattered in
// at the surface (the share of the bed lost off the shore line); the path's slant share (the absorbed path is the
// bed's vertical depth plus this share of the rest of the view ray's run: low over the water the slant run hid the
// shelves the Original shows); the share of the bed that always shows, red least (the art's sand shelves lie some 30
// units down, which the Original's band shows at a quarter, its alpha 0x60); the refraction's bend (pixels, the grid seas' 18: the
// art's mirrored scene under the water wobbled into rings); the caustics' fade (view depth: they shimmered into specks
// on the far shelves); the crest shading; the Fresnel term's floor, power and cap; the slope's share in the reflection
// and the glint; the reflection's hit tolerance (steps; the grid seas' 4 caught rays passing behind the islands, which
// tore their reflection into rings); the glint's power and strength (smaller and softer than the grid seas'); the foam's share.
const OCEAN_RIPPLE: f32 = 0.6;
const OCEAN_ABSORB: vec3<f32> = vec3<f32>(1.0, 0.16, 0.16);
const OCEAN_HAZE: f32 = 0.35;
const OCEAN_SLANT: f32 = 0.3;
const OCEAN_CLEAR: vec3<f32> = vec3<f32>(0.12, 0.35, 0.35);
const OCEAN_BEND: f32 = 7.0;
const OCEAN_CAUSTIC_FADE: vec2<f32> = vec2<f32>(25.0, 60.0);
const OCEAN_CREST: f32 = 0.08;
const OCEAN_FRESNEL: vec3<f32> = vec3<f32>(0.03, 5.0, 0.5);
const OCEAN_SLOPE: f32 = 0.4;
const OCEAN_SSR_TOL: f32 = 1.0;
const OCEAN_GLINT: vec2<f32> = vec2<f32>(900.0, 0.8);
const OCEAN_FOAM: f32 = 0.15;

// The surface's height gradient over world xz from travelling sine waves: (direction, wavenumber, amplitude·wavenumber),
// seen from horizontal distance d.
fn wave_grad(xz: vec2<f32>, t: f32, d: f32) -> vec2<f32> {
    var waves = array<vec4<f32>, 6>(
        vec4<f32>(0.96, 0.28, 1.6, 0.050),
        vec4<f32>(-0.42, 0.91, 2.3, 0.040),
        vec4<f32>(0.71, -0.70, 3.4, 0.030),
        vec4<f32>(-0.89, -0.45, 5.1, 0.022),
        vec4<f32>(0.20, 0.98, 7.3, 0.016),
        vec4<f32>(-0.60, 0.80, 10.7, 0.012),
    );
    var g = vec2<f32>(0.0);
    for (var i = 0; i < 6; i += 1) {
        let w = waves[i];
        let k = w.z;
        let l = 6.2831855 / k;
        let phase = dot(w.xy, xz) * k + t * sqrt(9.8 * k) * 0.35;
        g += w.xy * (w.w * k * cos(phase) * (1.0 - smoothstep(RIPPLE_LOD * l, 2.0 * RIPPLE_LOD * l, d)));
    }
    return g;
}

// The height gradient of the rings: each a wave packet running outward from its point, widening and fading.
fn ring_grad(xz: vec2<f32>, t: f32) -> vec2<f32> {
    let k = 12.0;
    let speed = 1.3;
    var g = vec2<f32>(0.0);
    let n = min(u32(rings.head.x + 0.5), MAX_RINGS);
    for (var i = 0u; i < n; i += 1u) {
        let r = rings.ring[i];
        let age = t - r.z;
        if (age <= 0.0 || age > 3.0) { continue; }
        let off = xz - r.xy;
        let dist = length(off);
        if (dist < 1e-3) { continue; }
        let s = dist - speed * age;
        let w = 0.25 + 0.2 * age;
        let env = exp(-(s * s) / (w * w));
        let amp = r.w * 0.1 * exp(-1.3 * age) / (1.0 + dist);
        g += (off / dist) * (-amp * k * sin(k * s) * env);
    }
    return g;
}

fn hash2(p: vec2<f32>) -> vec2<f32> {
    let q = vec2<f32>(dot(p, vec2<f32>(127.1, 311.7)), dot(p, vec2<f32>(269.5, 183.3)));
    return fract(sin(q) * 43758.5453) * 2.0 - 1.0;
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

// Caustic light on the bed (0..~1.5): the zero lines of two drifting, warped noise layers, sharpened.
fn caustics(xz: vec2<f32>, t: f32) -> f32 {
    let warp = vec2<f32>(gnoise(xz * 0.45 + vec2<f32>(t * 0.07, 0.0)), gnoise(xz * 0.45 + vec2<f32>(5.2, -t * 0.06)));
    let p = xz * 1.4 + warp * 1.2;
    let a = 1.0 - abs(gnoise(p + vec2<f32>(t * 0.31, t * 0.17)) * 2.2);
    let b = 1.0 - abs(gnoise(p * 1.37 + vec2<f32>(-t * 0.23, t * 0.29) + 3.7) * 2.2);
    return pow(clamp(a, 0.0, 1.0), 7.0) * 0.8 + pow(clamp(b, 0.0, 1.0), 7.0) * 0.6;
}

// Kind 4: the chute surface's height over (across, along), carried downhill at `speed`.
fn chute_height(p: vec2<f32>, t: f32, speed: f32) -> f32 {
    let a = vec2<f32>(p.x * 1.6, (p.y - t * speed) * 0.9);
    let b = vec2<f32>(p.x * 3.3 + 1.7, (p.y - t * speed * 1.15) * 2.1);
    return gnoise(a) * 0.06 + gnoise(b) * 0.025;
}

@fragment
fn fragment(in: WaterVertexOutput) -> @location(0) vec4<f32> {
    // The surface's own normal (kind 4 slopes), from the screen-space derivatives before any branch.
    let wp_d = (view.world_from_view * vec4<f32>(position_ndc_to_view(frag_coord_to_ndc(in.position)), 1.0)).xyz;
    var n_geo = normalize(cross(dpdy(wp_d), dpdx(wp_d)));
    if (n_geo.y < 0.0) { n_geo = -n_geo; }
    // How steeply the bed rises under the surface (vertical depth per world unit along it), from the screen-space
    // derivatives before any branch: the foam's shore. A flat shallow floor (Rilgar's pool under its slide) has none.
    let size0 = vec2<i32>(textureDimensions(scene_depth));
    let ndc0 = frag_coord_to_ndc(in.position);
    let wp0 = (view.world_from_view * vec4<f32>(position_ndc_to_view(ndc0), 1.0)).xyz;
    let d0 = textureLoad(scene_depth, clamp(vec2<i32>(floor(in.position.xy)), vec2<i32>(0), max(size0 - 1, vec2<i32>(0))), 0).r;
    let bed0 = (view.world_from_view * vec4<f32>(position_ndc_to_view(vec3<f32>(ndc0.xy, max(d0, 1e-7))), 1.0)).xyz;
    let under0 = select(1e4, wp0.y - bed0.y, d0 > 0.0);
    let rise = max(abs(dpdx(under0)) / max(length(dpdx(wp0)), 1e-5), abs(dpdy(under0)) / max(length(dpdy(wp0)), 1e-5));
    let t = textureSample(tex, tex_sampler, in.uv);
    // MODULATE on the display bytes: floor(Ct·Cv / 128), clamped to 255.
    let ct = round(t.rgb * 255.0);
    var rgb = min(floor(ct * in.color + vec3<f32>(1e-3)), vec3<f32>(255.0)) / 255.0;
    let raw = rgb;
    if (fog.color.w > 0.5) {
        rgb = mix(fog.color.rgb, rgb, in.fog);
    }
    let size = vec2<i32>(textureDimensions(scene_depth));
    let kind = u32(params.misc.y + 0.5);
    if (look.misc.x < 0.5 || size.x <= 1) {
        if (kind == 4u) { return vec4<f32>(0.0); }
        return vec4<f32>(srgb_to_linear(rgb), params.misc.x);
    }

    if (kind == 1u || kind == 3u) { return vec4<f32>(0.0); }
    // Another liquid (look.misc.x = 2, look.misc.y = its kind: crate::liquid_render), opaque, fogged after the look.
    if (look.misc.x > 1.5) {
        let lp = (view.world_from_view * vec4<f32>(position_ndc_to_view(frag_coord_to_ndc(in.position)), 1.0)).xyz;
        let sky = srgb_to_linear(mix(fog.color.rgb, vec3<f32>(1.0), 0.25));
        var lc = liquid_look(u32(look.misc.y + 0.5), raw, raw, in.uv, lp, in.ripple_grad, DRIFT, normalize(look.sun.xyz), sky, normalize(view.world_position - lp), globals.time, still_frame());
        if (fog.color.w > 0.5) { lc = mix(srgb_to_linear(fog.color.rgb), lc, in.fog); }
        return vec4<f32>(lc, 1.0);
    }
    // params.misc.w = 1 on a kind 0 draw: Pokitaru's ocean (crate::ocean_water), clearer and lighter (OCEAN_* below).
    let ocean = kind == 0u && params.misc.w > 0.5;
    let vp = position_ndc_to_view(frag_coord_to_ndc(in.position));
    let wp = (view.world_from_view * vec4<f32>(vp, 1.0)).xyz;
    let t_now = globals.time;
    let here = vec2<i32>(floor(in.position.xy));
    let thickness = clamp(vp.z - scene_z(here, size), 0.0, 1e4);
    let chute = kind == 4u;
    // Far water (look.misc.yz = the fade's start / end view depth, crate::sea_water; 0: none): no bed, shore or foam.
    let far = select(0.0, smoothstep(look.misc.y, look.misc.z, -vp.z), look.misc.z > 0.0);
    // Kind 4 is a film everywhere: its edges fade by the vertex fade instead.
    let shore = select(max(smoothstep(0.0, 0.6, thickness), far), 1.0, chute);

    // The swell's share: the vertex's shore damping (split seas), else the bed's vertical depth under this pixel.
    var swell = select(in.swell, 0.0, chute);
    if (swell < 0.0) {
        var under = SHORE_DEPTH.y;
        let d0 = textureLoad(scene_depth, clamp(here, vec2<i32>(0), size - 1), 0).r;
        if (d0 > 0.0) {
            let bed0 = (view.world_from_view * vec4<f32>(position_ndc_to_view(vec3<f32>(frag_coord_to_ndc(in.position).xy, d0)), 1.0)).xyz;
            under = wp.y - bed0.y;
        }
        swell = look.sun.w * smoothstep(SHORE_DEPTH.x, SHORE_DEPTH.y, under);
    }
    let swell_d = distance(wp.xz, view.world_position.xz);
    var n_world: vec3<f32>;
    var g = vec2<f32>(0.0);
    var tilt: vec2<f32>;
    if (chute) {
        // Kind 4: the sloped sheet's normal tilted by the flowing height field (game flow dir -> Bevy (x, −y)).
        let speed = params.misc.z;
        let along = vec3<f32>(in.flow.z, 0.0, -in.flow.w);
        let across = vec3<f32>(-in.flow.w, 0.0, -in.flow.z);
        let e = 0.05;
        let gu = (chute_height(in.flow.xy + vec2<f32>(e, 0.0), t_now, speed) - chute_height(in.flow.xy - vec2<f32>(e, 0.0), t_now, speed)) / (2.0 * e);
        let gv = (chute_height(in.flow.xy + vec2<f32>(0.0, e), t_now, speed) - chute_height(in.flow.xy - vec2<f32>(0.0, e), t_now, speed)) / (2.0 * e);
        n_world = normalize(mix(n_geo, normalize(n_geo - along * gv - across * gu), shore));
        tilt = (n_world - n_geo).xz * 2.5;
    } else {
        g = wave_grad(wp.xz, t_now, swell_d) * select(1.0, OCEAN_RIPPLE, ocean) + in.ripple_grad + ring_grad(wp.xz, t_now) + swell_grad(wp.xz, t_now, swell_d) * swell;
        n_world = normalize(mix(vec3<f32>(0.0, 1.0, 0.0), normalize(vec3<f32>(-g.x, 1.0, -g.y)), shore));
        tilt = n_world.xz;
    }
    let n = normalize((view.view_from_world * vec4<f32>(n_world, 0.0)).xyz);
    let to_eye = normalize(-vp);
    let dist = -vp.z;

    // The bed through the surface: the pixel bent by the waves, kept only where the bent pixel lies under the water.
    let bend = tilt * (select(18.0, OCEAN_BEND, ocean) * clamp(select(thickness, max(thickness, 0.4), chute), 0.0, 2.0) / (1.0 + dist * 0.05));
    var coord = in.position.xy + vec2<f32>(bend.x, -bend.y);
    if (scene_z(vec2<i32>(floor(coord)), size) > vp.z) { coord = in.position.xy; }
    let ci = clamp(vec2<i32>(floor(coord)), vec2<i32>(0), size - 1);
    let bed_d = textureLoad(scene_depth, ci, 0).r;
    var bed = textureSampleLevel(scene_color, scene_sampler, coord / vec2<f32>(size), 0.0).rgb;
    var path = 60.0;
    if (bed_d > 0.0) {
        let bed_vp = position_ndc_to_view(vec3<f32>(frag_coord_to_ndc(vec4<f32>(coord, 0.0, 1.0)).xy, bed_d));
        let bed_wp = (view.world_from_view * vec4<f32>(bed_vp, 1.0)).xyz;
        path = max(length(bed_vp - vp), 0.0);
        let under = max(wp.y - bed_wp.y, 0.0);
        if (ocean) { path = mix(under, path, OCEAN_SLANT); }
        let sun_up = max(look.sun.y, 0.0);
        var c = caustics(bed_wp.xz, t_now) * smoothstep(0.05, 0.4, under) * exp(-under * 0.35) * (0.35 + 0.65 * sun_up);
        if (ocean) { c *= 1.0 - smoothstep(OCEAN_CAUSTIC_FADE.x, OCEAN_CAUSTIC_FADE.y, dist); }
        bed = bed * (1.0 + c * 1.6);
    }
    // Absorption toward the water's own colour (the texture as the game drew it, darkened with depth). Unfogged: the
    // fog goes over the finished colour at the end, reflection and glint included (the ocean keeps its fogged tint and
    // its own sky).
    let tint = srgb_to_linear(select(rgb, raw, !ocean));
    // look.misc.w: the deep water's share of the colour (crate::sea_water's seas; 0: 0.55).
    let deep = tint * select(0.55, look.misc.w, look.misc.w > 0.0);
    // params.misc.z: an even absorption per unit (crate::sea_water's translucent grids; 0: this coloured one), the
    // flow speed on a chute.
    let absorb = select(select(vec3<f32>(0.30, 0.12, 0.07) * (0.6 + params.misc.x), vec3<f32>(params.misc.z), params.misc.z > 0.0 && !chute), OCEAN_ABSORB * params.misc.x, ocean);
    let trans = max(exp(-absorb * path) * (1.0 - select(0.0, OCEAN_HAZE, ocean) * shore), select(vec3<f32>(0.0), OCEAN_CLEAR, ocean));
    var col = bed * trans + deep * (1.0 - trans);
    col = mix(col, tint, select(select(0.12, 0.4, chute), 0.0, ocean) * shore);
    col = mix(col, tint, far);
    // The swell's crests lighter, its troughs darker (crate::water_waves::CREST, by the height over MAX_HEIGHT).
    col = col * (1.0 + 0.35 * clamp(swell * swell_height(wp.xz, t_now, swell_d) / 0.3, -1.5, 1.5) * select(1.0, OCEAN_CREST / 0.35, ocean));
    if (chute) {
        // White streaks stretched along the flow and carried with it.
        let q = vec2<f32>(in.flow.x * 5.0, (in.flow.y - t_now * params.misc.z * 1.1) * 0.5);
        let streak = smoothstep(0.3, 0.55, gnoise(q) + 0.5 * gnoise(q * 2.3 + 4.1));
        col = mix(col, vec3<f32>(0.9, 0.95, 1.0), streak * 0.25 * shore);
    }

    // Reflection (Fresnel, raised a little so the surface reads as water from above) and the key light's glint.
    let cos_v = max(dot(n, to_eye), 0.0);
    let f = select(0.08 + 0.92 * pow(1.0 - cos_v, 4.0), OCEAN_FRESNEL.x + (1.0 - OCEAN_FRESNEL.x) * pow(1.0 - cos_v, OCEAN_FRESNEL.y), ocean) * shore;
    // The ocean reflects and glints on a flatter normal (the slope times OCEAN_SLOPE): the reflected islands broken, not
    // torn into rings, the glint fewer flecks gathered on the sun's path.
    let k_s = select(1.0, OCEAN_SLOPE, ocean);
    let n_s = normalize((view.view_from_world * vec4<f32>(normalize(mix(vec3<f32>(0.0, 1.0, 0.0), normalize(vec3<f32>(-g.x * k_s, 1.0, -g.y * k_s)), shore)), 0.0)).xyz);
    let n_r = select(n, n_s, ocean);
    let rd = reflect(-to_eye, n_r);
    let miss = select(srgb_to_linear(fog.color.rgb), ocean_sky(vp, rd, size), ocean);
    var r = reflection(vp, rd, size, miss, select(4.0, OCEAN_SSR_TOL, ocean));
    r = planar(r, wp, n_world, in.position, dist);
    let l = normalize((view.view_from_world * vec4<f32>(look.sun.xyz, 0.0)).xyz);
    let gl = select(vec2<f32>(300.0, 4.0), OCEAN_GLINT, ocean);
    let glint = pow(max(dot(n_r, normalize(l + to_eye)), 0.0), gl.x) * gl.y * smoothstep(0.0, 0.1, look.sun.y);
    // A chute is seen along its slope at grazing angles: its reflection is capped so the bed shows through.
    col = mix(col, r, clamp(f, 0.0, select(select(0.9, 0.55, chute), OCEAN_FRESNEL.z, ocean))) + vec3<f32>(1.0, 0.96, 0.88) * glint * shore;

    if (chute) { return vec4<f32>(col, in.fade); }
    // Foam along the shore (thin water over a rising bed), broken up by drifting noise.
    let foam_n = smoothstep(0.1, 0.5, gnoise(wp.xz * 3.0 + vec2<f32>(t_now * 0.2, -t_now * 0.15)) + 0.35);
    let foam = (1.0 - smoothstep(0.05, 0.45, thickness)) * smoothstep(0.02, 0.1, thickness) * smoothstep(0.02, 0.1, rise) * foam_n;
    col = mix(col, vec3<f32>(0.92, 0.96, 1.0), foam * select(0.55, OCEAN_FOAM, ocean) * (1.0 - far));
    // The fog over all of it, as on the game's fogged strip: unfogged, a far sea's reflection of the lit sky ends in a
    // hard line where the grid stops drawing its blocks (Batalia: 400 units out, past the grid fog's far end).
    if (fog.color.w > 0.5 && !ocean) { col = mix(srgb_to_linear(fog.color.rgb), col, in.fog); }
    return vec4<f32>(col, 1.0);
}
