// Tfrag base pass: GS TFX = MODULATE, then GS fog (PRIM FGE = 1), no lighting.
//
// GS texture function MODULATE (TCC = 1): Cv = (Ct * Cf) >> 7, Av = (At * Af) >> 7, clamped to 0xff.
// The vertex colour attribute already carries rgba / 128 (0x80 = 1.0), so here out = tex * color.
// The texture's alpha channel holds the raw GS texel alpha (0..0x80, stored as byte / 255).
//
// Interpolation (PRIM 0x7c: IIP = 1 Gouraud, TME, FGE, ABE): the GS interpolates RGBA and F linearly in
// screen space and only ST perspective-correctly (divided by the interpolated Q). So `color` and `fog`
// are @interpolate(linear) (WGSL noperspective), `uv` keeps WGSL's default perspective interpolation.
//
// Alpha: the tfrag pass runs with ALPHA_1 = 0x8000000044 (A = Cs, B = Cd, C = As, D = Cd, FIX 0x80:
// Cout = ((Cs - Cd) * As >> 7) + Cd) and TEST_1 = 0x5360b (ATE, ATST = GEQUAL, AREF = 0x60,
// AFAIL = RGB_ONLY: a failing pixel still writes its blended RGB, only Z is not written).
// The fragment returns alpha = As / 128. A batch whose As is 0x80 everywhere is one opaque draw (the blend
// is then Cs); any other batch is two draws of the same mesh (gs_state.rs): GS_ATEST_PASS keeps As >= AREF
// (blended, Z written), GS_ATEST_FAIL keeps As < AREF (blended, no Z).
// The texture is Rgba8Unorm holding the raw display-encoded bytes the GS sees, so the product is
// computed in display space (as the GS does) and only then converted to linear for Bevy's sRGB
// view target, which re-encodes it to exactly the GS value.
//
// Fog (docs/plan/game_camera_fog.md): the VU1 tfrag program writes the F lane of XYZF2 as
// trunc(clamp(w + qw661.w, qw656.y, qw656.z)), where w is the projection's 4th output
// = camera depth (integer units) * slope. The GS interpolates F linearly in screen space and
// blends RGB only: C = FOGCOL + (C - FOGCOL) * F / 255 (F = 255: no fog).
//
// LOD (docs/formats/tfrag_rac1.md §3b; tfrag_render.rs / tfrag_lod.rs for the data): every strip list of every
// tfrag is in the mesh; `modes[tfrag]` is this frame's `TfragProc` decision (0 = culled, 2 = clipping path,
// else the VU1 MSCAL entry 6 / 8 / 0xa / 0xe / 0x10 / 0x14) and a vertex of another list is dropped. The rest
// replays the VU1 program for that entry:
//   L18 (modes 8, 0xa, 0xe) morphs LOD-01 positions, L17 (0xe, 0x10) LOD-0 positions (after L18, so LOD-0
//   vertices blend toward already-morphed LOD-01 parents in mode 0xe):
//     t.xy = clamp(w * qw66a.xy + qw66b.xy, 0, (0.5, 1))  = (u/2, 1 - u),  w = the vertex's own VU w
//     pos' = (P1 + P2) * t.x + pos * t.y,  col' = ftoi0((C1 + C2) * t.x + col * t.y)
//   The VU blends clip-space xyzw, which is affine in the world position, so blending world positions and
//   projecting afterwards is the same map. UVs do not morph.
//   L26 (mode 8, LOD-01) / L25 (mode 0xe, LOD-0): if both parents have w < qw66a.w (depth beyond D0 / D1),
//   the morphed position is not stored and the vertex-info entry is replaced by parent 1's (UV and position).
//   L48 / L47: the same collapse test for the "extra" entries (parent 1 from unk_indices_2), after the morph.
// w = depth * slope with the fog slope (negative), exactly the VU's w lane before the fog offset.
//
// Mip level (GS, TEX1 LCM = 0, L = 0): LOD = log2(1 / |Q|) + K with Q = qw656.x / w = n / z (z = camera depth
// in raw units, n = 32), so LOD = log2(z / 32) + K. MMIN = LINEAR_MIPMAP_NEAREST: level = round(LOD) clamped to
// 0..MXL (LOD < 0 is magnification, MMAG = LINEAR on level 0), bilinear inside the level. Q is interpolated
// perspective-correctly per pixel, so z is the exact per-pixel depth.
//
// Point lights (`LightTfrags`' point pass, crate::world_lights): when the tfrag's nibble list names any, every
// position slot the vertex reads gets them added to its baked RGBA (tfrag_lit below) before the morph
// blends, as VU1 reads the relit RGBA block. With an empty list (0xffff) nothing changes. The lists (one u32 per
// tfrag) and the bank (8 × 8 f32 as u32 bits) follow the draw modes in `modes`; a slot's normal is packed in its
// last word. (No bindings of their own: a changed material layout reorders draws and changes lightless frames.)
//
// End fade (the Draw distance option above Original, `fog.tex_mode.z` = its factor s; tfrag_lod.rs DRAW_DISTANCE): the
// cull at 500·s units is per tfrag, so each pixel fades by its own depth over the last 16·s units before it
// (crate::graphics::end_fade), dithered 4x4 (`end_fade_shown`); nothing past 500·s is drawn, so the cull never pops.
// s = 1 (Original): no fade, the game's cut.

#import bevy_pbr::view_transformations::{position_world_to_clip, position_world_to_view}
#ifndef PREPASS_PIPELINE
#import bevy_pbr::mesh_view_bindings::{view, globals}
#endif
#import rerac::world_lights::{WorldLight, NO_LIGHTS}
#ifdef WORLD_METAL
#import rerac::metal_look::{metal_normal, metal_shade}
#endif

struct TfragFog {
    // rgb = FOGCOL (display-encoded 0..1), w = 1 when fog is enabled.
    color: vec4<f32>,
    // (slope per integer unit of depth, offset qw661.w, lower clamp qw656.y, upper clamp qw656.z).
    params: vec4<f32>,
    // x: the texture option (crate::graphics; sample_world), z: the Draw distance factor (the end fade).
    tex_mode: vec4<f32>,
#ifdef WORLD_METAL
    // crate::metal::MetalEnv: to the sun, its colour, the sky probe (`metal_look.wgsl`).
    metal: array<vec4<f32>, 27>,
#endif
}

struct TfragLod {
    k666: vec4<f32>,
    k667: vec4<f32>,
    k668: vec4<f32>,
    k669: vec4<f32>,
    // x = VU w per raw unit of depth, y = 1: tint by LOD, z = near (32), w = the shine (crate::metal; > 0: WORLD_METAL).
    misc: vec4<f32>,
    // y = MXL.
    tex: vec4<f32>,
}

struct Slot {
    pos: vec3<f32>,
    color: u32,
    p1: u32,
    p2: u32,
    // 0 none, 1 = written by a LOD-01 primary, 2 = by a LOD-0 primary.
    tier: u32,
    // The LightTfrags record's normal: azimuth | elevation << 8 | 1 << 16 (0: no record).
    normal: u32,
}

struct VInfo {
    uv: vec2<f32>,
    slot: u32,
    // 0 common, 1 LOD-01, 2 LOD-0 (collapse candidates).
    tier: u32,
    p1: u32,
    p2: u32,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var tex: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var tex_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var<storage, read> fog: TfragFog;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var<uniform> lod: TfragLod;
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var<storage, read> slots: array<Slot>;
@group(#{MATERIAL_BIND_GROUP}) @binding(5) var<storage, read> vinfos: array<VInfo>;
@group(#{MATERIAL_BIND_GROUP}) @binding(6) var<storage, read> modes: array<u32>;

// The texture option (crate::graphics, `fog.tex_mode.x`): 0 Original = the GS level round(L) clamped to 0..MXL (one
// level, bilinear inside it); 1 Smooth = L unrounded, the two nearest levels blended by the sampler (no switch line);
// 2 Sharp = the GPU's own level from the UV footprint `duv` (dx, dy), anisotropic. The sampler's mip filter is linear:
// an integer level reads that level alone, as the GS does.
fn sample_world(uv: vec2<f32>, l: f32, mxl: f32, duv: vec4<f32>) -> vec4<f32> {
    let mode = fog.tex_mode.x;
    if mode > 1.5 {
        return textureSampleGrad(tex, tex_sampler, uv, duv.xy, duv.zw);
    }
    let level = select(clamp(floor(l + 0.5), 0.0, mxl), clamp(l, 0.0, mxl), mode > 0.5);
    return textureSampleLevel(tex, tex_sampler, uv, level);
}

// `modes` = n draw modes, n nibble lists, then the bank.
fn tfrag_count() -> u32 { return (arrayLength(&modes) - 64u) / 2u; }

// Bank slot k (0..7), read where it is needed: a copy of the whole bank per call (an 8-light local array indexed by a
// variable) made NVIDIA's Vulkan compiler take ~90 s per tfrag pipeline on a cold driver cache (G-REN-037: own() is
// inlined ~20 times per vertex through the LOD morphs).
fn bank_light(k: u32) -> WorldLight {
    let o = 2u * tfrag_count() + 8u * k;
    return WorldLight(
        bitcast<vec4<f32>>(vec4<u32>(modes[o], modes[o + 1u], modes[o + 2u], modes[o + 3u])),
        bitcast<vec4<f32>>(vec4<u32>(modes[o + 4u], modes[o + 5u], modes[o + 6u], modes[o + 7u])),
    );
}

// `LightTfrags`, point pass (crate::world_lights; after the directional one, on its packed result): per listed light
// whose radius reaches the vertex (r² − |P − L|² not negative), d = (−N)·(P − L)·(1 − dist/r)/dist, d = max(d, d·w),
// then rgba += floor(128 · (colour, w) · d) (all four lanes: the intensity also feeds alpha), rgb clamped at 255.
// `c0` = the vertex's baked bytes, `p` = its position, `n` = (N, 1 when the vertex has a light record).
fn tfrag_lit(list: u32, c0: vec4<f32>, p: vec3<f32>, n: vec4<f32>) -> vec4<f32> {
    if (n.w == 0.0) { return c0; }
    var c = round(c0);
    var l = list | 0xf0000u;
    loop {
        let i = l & 0xfu;
        if (i == 0xfu) { break; }
        l = l >> 4u;
        let light = bank_light(i & 7u);
        let v = p - light.pos.xyz;
        let d2 = dot(v, v);
        let r = light.pos.w;
        if (r * r - d2 < 0.0 || d2 <= 0.0) { continue; }
        let dist = sqrt(d2);
        var d = -dot(n.xyz, v) * ((1.0 - dist / r) / dist);
        d = max(d, d * light.col.w);
        let rgb = clamp(c.rgb + floor(128.0 * light.col.rgb * d), vec3<f32>(0.0), vec3<f32>(255.0));
        let a = c.a + floor(128.0 * light.col.w * d);
        c = vec4<f32>(rgb, a - 256.0 * floor(a / 256.0));
    }
    return c;
}

// N = (cos az · cos el, sin az · cos el, sin el) in game axes (256 steps per turn), in Bevy axes (x, z, −y);
// w = 1 when the slot has a light record.
fn slot_normal(p: u32) -> vec4<f32> {
    let a = f32(p & 0xffu) * (6.2831853 / 256.0);
    let e = f32((p >> 8u) & 0xffu) * (6.2831853 / 256.0);
    return vec4<f32>(cos(a) * cos(e), sin(e), -sin(a) * cos(e), f32((p >> 16u) & 1u));
}

// This vertex's tfrag's nibble list (set once per vertex, read by `own`).
var<private> point_list: u32 = 0xffffu;

struct TfragVertex {
    @location(0) position: vec3<f32>,
    // (global vertex-info index, tfrag index | strip list << 16 | TEX1 K (s12, 1/16 units) << 18)
    @location(1) tref: vec2<u32>,
#ifdef TFRAG_LIQUID
    // The triangle's ST → world xz map (crate::tfrag_render::ATTRIBUTE_ST_MAP).
    @location(2) st_map: vec4<f32>,
#endif
}

struct TfragVertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    // Gouraud RGBA / 128, screen-space linear like the GS.
    @location(1) @interpolate(linear) color: vec4<f32>,
    // GS F / 255, screen-space linear like the GS.
    @location(2) @interpolate(linear) fog: f32,
    // Camera depth in raw units, perspective-correct (= n / Q per pixel).
    @location(3) depth: f32,
    // Debug tint: 0 none, 1 LOD 1, 2 LOD 2, 3 clipping path.
    @location(4) @interpolate(flat) tint: u32,
    // TEX1 K of the triangle's ad-gif, in mip levels.
    @location(5) @interpolate(flat) k: f32,
#ifdef TFRAG_LIQUID
    // The Bevy world position (the view, `goo_look`) and the pattern's plane: the ST through the triangle's map.
    @location(6) world: vec3<f32>,
    @location(7) plane: vec2<f32>,
#endif
#ifdef WORLD_METAL
    // Shiny metal (crate::metal): the Bevy world position and the slot's normal (zero without a light record).
    @location(8) mworld: vec3<f32>,
    @location(9) mnormal: vec3<f32>,
#endif
}

// Position (world, game units) and colour (0..255) of a VU position slot.
struct Vtx {
    pos: vec3<f32>,
    col: vec4<f32>,
}

fn depth_raw(p: vec3<f32>) -> f32 {
    // Camera-space depth in the game's integer units (Bevy view space looks down -z, game units).
    return -position_world_to_view(p).z * 1024.0;
}

fn w_of(p: vec3<f32>) -> f32 { return depth_raw(p) * lod.misc.x; }

fn own(s: u32) -> Vtx {
    let sl = slots[s];
    let c = unpack4x8unorm(sl.color) * 255.0;
    if (point_list == NO_LIGHTS) { return Vtx(sl.pos, c); }
    return Vtx(sl.pos, tfrag_lit(point_list, c, sl.pos, slot_normal(sl.normal)));
}

fn blend(c: Vtx, p1: Vtx, p2: Vtx, t: vec2<f32>) -> Vtx {
    return Vtx((p1.pos + p2.pos) * t.x + c.pos * t.y, floor((p1.col + p2.col) * t.x + c.col * t.y));
}

// A common or LOD-01 slot after L18 / L26 in mode m.
fn value01(s: u32, m: u32) -> Vtx {
    let sl = slots[s];
    let c = own(s);
    if (sl.tier != 1u || !(m == 8u || m == 10u || m == 14u)) { return c; }
    let p1 = own(sl.p1);
    let p2 = own(sl.p2);
    if (m == 8u && w_of(p1.pos) < lod.k666.w && w_of(p2.pos) < lod.k666.w) { return c; }
    let t = clamp(w_of(c.pos) * lod.k666.xy + lod.k668.xy, vec2<f32>(0.0), vec2<f32>(0.5, 1.0));
    return blend(c, p1, p2, t);
}

// Any slot after all morph passes of mode m.
fn value(s: u32, m: u32) -> Vtx {
    let sl = slots[s];
    if (sl.tier != 2u) { return value01(s, m); }
    let c = own(s);
    if (!(m == 14u || m == 16u)) { return c; }
    let p1 = value01(sl.p1, m);
    let p2 = value01(sl.p2, m);
    if (m == 14u && w_of(p1.pos) < lod.k667.w && w_of(p2.pos) < lod.k667.w) { return c; }
    let t = clamp(w_of(c.pos) * lod.k667.xy + lod.k669.xy, vec2<f32>(0.0), vec2<f32>(0.5, 1.0));
    return blend(c, p1, p2, t);
}

#ifdef PREPASS_PIPELINE
// The sun's shadow map (crate::sun_shadow): every tfrag casts its LOD-0 list (mode 0x14's) at the slots' own
// positions, whatever this frame's mode, so a shadow does not change shape as the camera's LOD and cull change
// (the morphs follow the view's depth, which here is the light's); only the alpha test's passing half
// (gs_state::specialize_shadow).
struct TfragShadowOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) alpha: f32,
#ifdef UNCLIPPED_DEPTH_ORTHO_EMULATION
    @location(2) unclipped_depth: f32,
#endif
}

@vertex
fn vertex(v: TfragVertex) -> TfragShadowOutput {
    var out: TfragShadowOutput;
    let list = (v.tref.y >> 16u) & 3u;
#ifdef SHADOW_SKIP
    let skip = true;
#else
    let skip = list != 0u;
#endif
    if (skip) {
        out.position = vec4<f32>(2.0, 2.0, 2.0, 1.0);
        return out;
    }
    let e = vinfos[v.tref.x];
    let sl = slots[e.slot];
    out.position = position_world_to_clip(sl.pos);
    out.uv = e.uv;
    out.alpha = unpack4x8unorm(sl.color).a * (255.0 / 128.0);
#ifdef UNCLIPPED_DEPTH_ORTHO_EMULATION
    out.unclipped_depth = out.position.z;
    out.position.z = min(out.position.z, 1.0);
#endif
    return out;
}

fn shadow_test(in: TfragShadowOutput) {
#ifdef GS_ATEST_PASS
    let t = textureSampleLevel(tex, tex_sampler, in.uv, 0.0);
    let a_s = min(floor(round(t.a * 255.0) * round(in.alpha * 128.0) / 128.0), 255.0);
    if (a_s < f32(#{GS_AREF})) { discard; }
#endif
}

#ifdef UNCLIPPED_DEPTH_ORTHO_EMULATION
@fragment
fn fragment(in: TfragShadowOutput) -> @builtin(frag_depth) f32 {
    shadow_test(in);
    return in.unclipped_depth;
}
#else
@fragment
fn fragment(in: TfragShadowOutput) { shadow_test(in); }
#endif
#else
@vertex
fn vertex(v: TfragVertex) -> TfragVertexOutput {
    var out: TfragVertexOutput;
    let tf = v.tref.y & 0xffffu;
    let list = (v.tref.y >> 16u) & 3u;
    let m = modes[tf];
    // Strip list the mode draws: LOD 2 for MSCAL 6, LOD 1 for 8 / 0xa, LOD 0 otherwise (incl. the clip path).
    let mlist = select(select(0u, 1u, m == 8u || m == 10u), 2u, m == 6u);
    if (m == 0u || mlist != list) {
        // Not drawn this frame: all three vertices of the triangle land on one point outside the clip volume.
        out.position = vec4<f32>(2.0, 2.0, 2.0, 1.0);
        return out;
    }
    point_list = modes[tfrag_count() + tf] & 0xffffu;
    var e = vinfos[v.tref.x];
    if ((m == 8u && e.tier == 1u) || (m == 14u && e.tier == 2u)) {
        let thr = select(lod.k667.w, lod.k666.w, m == 8u);
        let pe = vinfos[e.p1];
        if (w_of(value(pe.slot, m).pos) < thr && w_of(value(e.p2, m).pos) < thr) { e = pe; }
    }
    let r = value(e.slot, m);
    out.position = position_world_to_clip(r.pos);
#ifdef TFRAG_LIQUID
    out.world = r.pos;
    out.plane = vec2<f32>(dot(v.st_map.xy, e.uv), dot(v.st_map.zw, e.uv));
#endif
#ifdef WORLD_METAL
    out.mworld = r.pos;
    let sn = slot_normal(slots[e.slot].normal);
    out.mnormal = sn.xyz * sn.w;
#endif
    out.uv = e.uv;
    out.color = r.col / 128.0;
    let depth = depth_raw(r.pos);
    out.depth = depth;
    // VU: w = depth * slope (the w row of the matrix), += qw661.w, min qw656.z, max qw656.y, ftoi4.
    let f = max(min(depth * fog.params.x + fog.params.y, fog.params.w), fog.params.z);
    out.fog = trunc(f) / 255.0;
    out.k = f32(bitcast<i32>(v.tref.y << 2u) >> 20u) / 16.0;
    out.tint = select(0u, select(list, 3u, m == 2u), lod.misc.y > 0.5);
    return out;
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + 0.055) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}

// The end fade's dither (crate::graphics::end_fade_shown): pixel `p` is drawn at visibility `vis` (0..128).
fn end_fade_shown(vis: u32, p: vec2<f32>) -> bool {
    let q = vec2<u32>(p);
    let e = q.x ^ q.y;
    let b = ((e & 1u) << 3u) | ((q.y & 1u) << 2u) | (e & 2u) | ((q.y >> 1u) & 1u);
    return vis > 8u * b + 4u;
}

// crate::graphics::end_fade at raw depth `depth` for the cull at 500·s units (512000 raw).
fn end_fade(depth: f32, s: f32) -> u32 {
    let steps = (512000.0 * s - depth) * (8.0 / (1024.0 * s));
    return select(min(u32(steps), 128u), 0u, steps <= 0.0);
}

// Port-only (Water Enhanced): the tfrag liquids (crate::liquid_render::TFRAG_LIQUIDS, Orxon's goo), on while
// `lod.tex.z` = 1. The game's texture is not drawn: its mean colour (`lod.tex.x`, packed r + 256 g + 65536 b by
// crate::liquid_render::pack_mean) under the vertex colours, the texture's look redrawn procedurally and alive: eyes that
// swell and pop, drifting squiggles, an uneven swell (`goo_look`). Original never takes this branch.

fn goo_hash(p: vec2<f32>) -> vec3<f32> {
    var q = fract(p.xyx * vec3<f32>(0.1031, 0.1030, 0.0973));
    q += dot(q, q.yzx + 33.33);
    return fract((q.xxy + q.yzz) * q.zyx);
}

// The texture's mean colour (display bytes / 255).
fn goo_mean() -> vec3<f32> {
    let v = lod.tex.x;
    let b = floor(v / 65536.0);
    let g = floor((v - b * 65536.0) / 256.0);
    return vec3<f32>(v - b * 65536.0 - g * 256.0, g, b) / 255.0;
}

// Gradient noise over `p` (about −0.7..0.7).
fn goo_noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let g00 = goo_hash(i).xy * 2.0 - 1.0;
    let g10 = goo_hash(i + vec2<f32>(1.0, 0.0)).xy * 2.0 - 1.0;
    let g01 = goo_hash(i + vec2<f32>(0.0, 1.0)).xy * 2.0 - 1.0;
    let g11 = goo_hash(i + vec2<f32>(1.0, 1.0)).xy * 2.0 - 1.0;
    let x0 = mix(dot(g00, f), dot(g10, f - vec2<f32>(1.0, 0.0)), u.x);
    let x1 = mix(dot(g01, f - vec2<f32>(0.0, 1.0)), dot(g11, f - vec2<f32>(1.0, 1.0)), u.x);
    return mix(x0, x1, u.y);
}

// The goo's swell: three octaves of noise (about 6, 2.5 and 1.1 units) drifting in three directions, so it rolls
// unevenly with no regular crests. Height in about −1..1.
fn goo_swell(q: vec2<f32>, t: f32) -> f32 {
    let a = goo_noise(q * 0.17 + vec2<f32>(t * 0.05, t * 0.02));
    let b = goo_noise(q * 0.4 + vec2<f32>(-t * 0.08, t * 0.05) + vec2<f32>(3.7, 1.1));
    let c = goo_noise(q * 0.9 + vec2<f32>(t * 0.03, -t * 0.11) + vec2<f32>(8.3, 5.9));
    return (a * 0.55 + b * 0.3 + c * 0.15) * 1.6;
}

// The game's texture made alive: "eyes" (its orange bubbles, yellow centre, dark rim) in GOO_EYE-unit cells that
// wander, swell and pop over 3..6.5 s into a neon ring running out to 2.1 radii; around a living one, a neon arc (the
// texture's green crescents). `slope` the domes' and rings' height gradient over xz. `foot` the pixel's world size:
// the edges widen to it (thin lines keep their weight as they widen) and the domes' shading and glints fade, so the
// eyes hold without aliasing out to a few pixels across.
const GOO_EYE: f32 = 1.4;
struct GooEyes {
    slope: vec2<f32>,
    eye: f32,
    rim: f32,
    neon: f32,
    glint: f32,
}

fn goo_eyes(q: vec2<f32>, t: f32, foot: f32) -> GooEyes {
    var o = GooEyes(vec2<f32>(0.0), 0.0, 0.0, 0.0, 0.0);
    let c = floor(q / GOO_EYE);
    for (var j = -1; j <= 1; j += 1) {
        for (var i = -1; i <= 1; i += 1) {
            let id = c + vec2<f32>(f32(i), f32(j));
            let h = goo_hash(id + 31.0);
            if (h.x > 0.85) { continue; }
            let wander = 0.12 * vec2<f32>(sin(t * 0.3 + h.x * 6.28), cos(t * 0.27 + h.y * 6.28));
            let centre = (id + 0.5 + (h.yz - 0.5) * 0.5 + wander) * GOO_EYE;
            let period = mix(3.0, 6.5, h.y);
            let age = fract(t / period + h.z);
            let size = GOO_EYE * mix(0.28, 0.42, fract(h.x * 17.3 + h.z * 5.1));
            let d = q - centre;
            let dl = length(d);
            if (age < 0.88) {
                let rise = smoothstep(0.0, 0.88, age);
                let rad = size * (0.4 + 0.6 * sqrt(rise));
                let s = dl / rad;
                // The pixel in units of this eye's radius.
                let sw = foot / rad;
                let fine = 1.0 - smoothstep(0.1, 0.4, sw);
                if (s < 1.0) {
                    let up = sqrt(1.0 - s * s);
                    o.slope -= d / (rad * max(up, 0.3)) * (0.06 + 0.14 * rise) * fine;
                    o.glint = max(o.glint, smoothstep(0.25, 0.08, length(d / rad - vec2<f32>(-0.3, -0.35))) * rise * fine);
                }
                o.eye = max(o.eye, smoothstep(0.8 + sw, 0.1, s));
                // A thick dark ring round the yolk, softening outside the dome.
                o.rim = max(o.rim, smoothstep(0.5 - sw, 0.85 + sw * 0.5, s) * smoothstep(1.35 + sw, 0.95 - sw * 0.5, s));
                // The crescent: half a ring just outside, turning slowly.
                let ang = atan2(d.y, d.x) + h.x * 6.28 + t * 0.2;
                let w2 = 1.0 / 60.0 + sw * sw;
                let arc = exp(-(s - 1.35) * (s - 1.35) / w2) * sqrt(1.0 / 60.0 / w2);
                o.neon = max(o.neon, arc * smoothstep(0.2, 0.7, sin(ang)) * 0.8);
            } else {
                // The pop: a neon ring and a ring wave running out to 2.1 radii, fading; it stays inside the cells
                // searched (a centre is at least 0.13 cells from its cell's edge, so 1.13 cells around it are seen:
                // `reach` closes it off before, or it is cut along the cell edges as straight lines).
                let a = (age - 0.88) / 0.12;
                let front = size * (0.6 + 1.5 * a);
                let w0 = size * (0.1 + 0.06 * a);
                let w = max(w0, foot);
                let reach = 1.0 - smoothstep(0.95, 1.1, dl / GOO_EYE);
                let env = exp(-(dl - front) * (dl - front) / (w * w)) * (1.0 - a) * reach;
                o.neon = max(o.neon, env * w0 / w);
                o.slope += d / max(dl, 1e-3) * (0.6 * env * cos((dl - front) * 8.0 / size)) * (1.0 - smoothstep(0.03, 0.1, foot / size));
            }
        }
    }
    return o;
}


// The goo's look over its base `rgb` (the texture's mean colour × the vertex colours, display space, before the fog):
// the texture's palette around that base (olive-brown mottling, yellow-orange eyes with dark rims, neon green
// squiggles and crescents) drawn procedurally on the world plane, carried by the swell's flow and alive: the eyes
// swell and pop ([`goo_eyes`]), the squiggles drift, the mottling breathes. Shaded by the swell's and the eyes'
// normal under a fixed light from above (Orxon is night), a faint Fresnel sheen of the fog colour, a soft highlight
// on the swell's high ground and a glint on each eye. The details widen to the pixel's footprint and fade only once
// they would be under a few pixels (the eyes from 0.25 units a pixel, the squiggles from 0.4). `world` the Bevy
// position (the view), `xz` the plane the pattern is drawn on: the texture's ST mapped to world units by each
// triangle's own map (crate::tfrag_render::liquid_st_map). Not the world position: the goo's mesh has overlapping
// pieces at slightly different heights, where the world position jumps by the parallax and the pattern showed thin
// lines of a shifted copy (marker 1 of session 2026-11-02_12-34-52); the ST runs on across them, as the texture does.
fn goo_look(rgb: vec3<f32>, world: vec3<f32>, xz: vec2<f32>, t: f32) -> vec3<f32> {
    let to_cam = view.world_position - world;
    let e = 0.08;
    let h0 = goo_swell(xz, t);
    let g = vec2<f32>(goo_swell(xz + vec2<f32>(e, 0.0), t) - h0, goo_swell(xz + vec2<f32>(0.0, e), t) - h0) / e * 0.35;
    // The pattern rides the swell: warped by it and by a slow flow.
    let q = xz + g * 0.6 + 0.3 * vec2<f32>(goo_noise(xz * 0.5 + vec2<f32>(t * 0.04, 0.0)), goo_noise(xz * 0.5 + vec2<f32>(5.2, -t * 0.03)));
    // The pixel's world size; the eyes hold to about 0.6 units a pixel, the squiggles to 0.9.
    let foot = length(fwidth(xz));
    let keep = 1.0 - smoothstep(0.25, 0.6, foot);
    let keep_s = 1.0 - smoothstep(0.4, 0.9, foot);
    var eyes = GooEyes(vec2<f32>(0.0), 0.0, 0.0, 0.0, 0.0);
    if (keep > 0.0) { eyes = goo_eyes(q, t, foot); }
    // The squiggles: a band of one drifting noise layer, broken into short pieces by a second; the bands widen to the
    // pixel (about half a noise unit a world unit) and keep their weight.
    let n1 = goo_noise(q * 0.9 + vec2<f32>(t * 0.06, -t * 0.04));
    let n2 = goo_noise(q * 0.7 + vec2<f32>(-t * 0.05, t * 0.07) + vec2<f32>(2.9, 7.7));
    let n3 = goo_noise(q * 1.4 + vec2<f32>(t * 0.04, t * 0.05) + vec2<f32>(6.1, 0.4));
    let aa1 = foot * 0.45;
    let aa3 = foot * 0.7;
    let band1 = smoothstep(0.11 + aa1, 0.04, abs(n1 - 0.2)) * 0.075 / (0.075 + aa1) * smoothstep(-0.15, 0.2, n2);
    let band2 = smoothstep(0.09 + aa3, 0.03, abs(n3 + 0.2)) * 0.06 / (0.06 + aa3) * smoothstep(-0.1, 0.25, -n2);
    let squiggle = max(band1, band2 * 0.85) * keep_s;
    let mottle = smoothstep(-0.15, 0.35, goo_noise(q * 0.55 + vec2<f32>(t * 0.015, 1.3)));
    // The palette around the base colour.
    let dark = rgb * vec3<f32>(0.62, 0.55, 0.3);
    let yolk = rgb * vec3<f32>(1.25, 1.15, 0.7);
    let neon = min(rgb * vec3<f32>(0.7, 1.35, 0.55) + vec3<f32>(0.0, 0.04, 0.0), vec3<f32>(1.0));
    var col = mix(rgb, dark, mottle * 0.6);
    col = mix(col, dark * 0.9, eyes.rim * 0.7 * keep);
    col = mix(col, yolk, eyes.eye * keep);
    col = mix(col, neon, max(squiggle, eyes.neon * keep) * 0.8);
    let slope = g + eyes.slope * keep;
    let n = normalize(vec3<f32>(-slope.x, 1.0, -slope.y));
    let l = normalize(vec3<f32>(0.35, 1.0, 0.25));
    let to_eye = normalize(to_cam);
    col *= clamp(1.0 + 1.6 * (dot(n, l) - l.y), 0.6, 1.4) * (1.0 + 0.12 * h0);
    let r = reflect(-to_eye, n);
    let sky = fog.color.rgb * mix(0.5, 1.5, clamp(r.y, 0.0, 1.0));
    let fres = 0.03 + 0.97 * pow(1.0 - max(dot(n, to_eye), 0.0), 5.0);
    col = mix(col, sky, clamp(fres * 0.5, 0.0, 0.4));
    let nh = max(dot(n, normalize(l + to_eye)), 0.0);
    let high = smoothstep(0.3, 0.8, h0);
    let spec = pow(nh, 40.0) * 0.12 * (0.4 + high) + pow(nh, 250.0) * 0.2 * eyes.eye * keep + eyes.glint * keep * 0.25;
    return col + vec3<f32>(1.0, 1.0, 0.8) * spec;
}

#ifdef TFRAG_LIQUID
// The swirl's flow on the plane (world units a second): the curl of two drifting noise layers (eddies of about 8 and 3
// units), so it turns without sources or sinks and no part of the pool thins out or piles up.
fn swirl_flow(q: vec2<f32>, t: f32) -> vec2<f32> {
    let e = 0.35;
    let f = vec2<f32>(t * 0.013, -t * 0.009);
    let n = goo_noise(q * 0.12 + f) + 0.45 * goo_noise(q * 0.31 - f * 1.7 + vec2<f32>(7.1, 2.3));
    let nx = goo_noise((q + vec2<f32>(e, 0.0)) * 0.12 + f) + 0.45 * goo_noise((q + vec2<f32>(e, 0.0)) * 0.31 - f * 1.7 + vec2<f32>(7.1, 2.3));
    let ny = goo_noise((q + vec2<f32>(0.0, e)) * 0.12 + f) + 0.45 * goo_noise((q + vec2<f32>(0.0, e)) * 0.31 - f * 1.7 + vec2<f32>(7.1, 2.3));
    return vec2<f32>(ny - n, -(nx - n)) / e * 9.0;
}

// World units a tile of the pool's texture covers in [`swirl_look`]: one scale for the whole pool. The game's maps
// differ triangle by triangle (Quartu's tex 10: 1.7 to 4.9 units a tile, turned and sheared; the Fleet's tex 57: 2.2 by
// 2.9), so the texture moved through each triangle's own map slid apart at the edges (seams, session
// 2026-11-06_15-02-26 marker 6). The mottling has no direction, so the world plane draws it the same.
const SWIRL_TILE: f32 = 2.5;

// The pool's texture on the world plane `q` (Bevy xz) moved by `d` world units, at `k` times its tile density.
fn swirl_sample(in: TfragVertexOutput, q: vec2<f32>, d: vec2<f32>, k: f32) -> vec3<f32> {
    let s = k / SWIRL_TILE;
    // Wrapped here (the batch's sampler may clamp); the footprint comes from the unwrapped plane.
    let st = fract((q - d) * s);
    let duv = vec4<f32>(dpdx(q) * s, dpdy(q) * s);
    return sample_world(st, log2(in.depth / lod.misc.z) + in.k + log2(k), lod.tex.y, duv).rgb;
}

// Quartu's and the Fleet's green pools (crate::liquid_render::TfragLook::Swirl, `lod.tex.w` = 2): the game's texture
// (on the world plane at one scale, [`SWIRL_TILE`]) and vertex colours, carried by a slow swirling flow ([`swirl_flow`]; two copies half a cycle apart, each moved along the flow
// for a 7-second cycle and faded out before it stretches, so the texture keeps its look while it turns); a finer
// copy flowing the other way multiplies in at a third of its contrast (detail under the texture's own texels); a gentle
// swell (`goo_swell`, about 5 units) shades it under a light from above, with a sheen of the fog colour by Fresnel and
// a soft highlight. The mean colour is the Original's.
fn swirl_look(in: TfragVertexOutput, t: f32) -> vec3<f32> {
    // The world plane, not the ST plane: the pools are flat, and it runs on unbroken across the triangles.
    let q = in.world.xz;
    let flow = swirl_flow(q, t);
    let cycle = t / 7.0;
    let p0 = fract(cycle);
    let p1 = fract(cycle + 0.5);
    let a = swirl_sample(in, q, flow * p0 * 1.6, 1.0);
    let b = swirl_sample(in, q, flow * p1 * 1.6 + vec2<f32>(3.7, 2.1), 1.0);
    var col = mix(a, b, abs(2.0 * p0 - 1.0));
    // The blend of two copies flattens the mottling a little: back to the texture's contrast round its mean.
    let mean = goo_mean();
    col = max(mean + (col - mean) * 1.25, vec3<f32>(0.0));
    let fine = swirl_sample(in, q, -flow * fract(cycle * 1.3) * 0.8 + vec2<f32>(1.3, 4.9), 2.7);
    let lum = dot(fine, vec3<f32>(0.3, 0.59, 0.11)) / max(dot(mean, vec3<f32>(0.3, 0.59, 0.11)), 0.02);
    col *= mix(1.0, clamp(lum, 0.5, 1.6), 0.33);
    col *= in.color.rgb;
    let e = 0.1;
    let qs = q * 1.4;
    let h0 = goo_swell(qs, t * 0.7);
    let g = vec2<f32>(goo_swell(qs + vec2<f32>(e, 0.0), t * 0.7) - h0, goo_swell(qs + vec2<f32>(0.0, e), t * 0.7) - h0) / e * 0.2;
    let n = normalize(vec3<f32>(-g.x, 1.0, -g.y));
    let l = normalize(vec3<f32>(0.35, 1.0, 0.25));
    let to_eye = normalize(view.world_position - in.world);
    col *= clamp(1.0 + 1.4 * (dot(n, l) - l.y), 0.75, 1.25);
    let r = reflect(-to_eye, n);
    let sky = fog.color.rgb * mix(0.6, 1.4, clamp(r.y, 0.0, 1.0));
    let fres = 0.03 + 0.97 * pow(1.0 - max(dot(n, to_eye), 0.0), 5.0);
    col = mix(col, sky, clamp(fres * 0.35, 0.0, 0.3));
    let nh = max(dot(n, normalize(l + to_eye)), 0.0);
    return col + vec3<f32>(0.85, 1.0, 0.8) * (pow(nh, 60.0) * 0.1 * (0.5 + smoothstep(0.2, 0.8, h0)));
}
#endif

@fragment
fn fragment(in: TfragVertexOutput) -> @location(0) vec4<f32> {
    let duv = vec4<f32>(dpdx(in.uv), dpdy(in.uv));
    // GS mip level: round(log2(z / n) + K), clamped to 0..MXL (or the texture option's level: sample_world).
    let t = sample_world(in.uv, log2(in.depth / lod.misc.z) + in.k, lod.tex.y, duv);
    var rgb = min(t.rgb * in.color.rgb, vec3<f32>(1.0));
#ifdef TFRAG_LIQUID
    // The liquid's own pipeline (crate::tfrag_render::TfragKey): the world position and the pattern's plane from its
    // vertex outputs (crate::tfrag_render::liquid_st_map).
    if (lod.tex.w > 1.5) {
        rgb = min(swirl_look(in, globals.time), vec3<f32>(1.0));
    } else {
        let base = min(goo_mean() * in.color.rgb, vec3<f32>(1.0));
        rgb = min(goo_look(base, in.world, in.plane, globals.time), vec3<f32>(1.0));
    }
#endif
#ifdef WORLD_METAL
    // Shiny metal (crate::metal, Lighting Enhanced; `lod.misc.w` its shine): before the fog, as the game's colour.
    let to_eye = normalize(view.world_position - in.mworld);
    let mn = metal_normal(in.mnormal, in.mworld, to_eye);
    let env = fog.metal;
    rgb = min(metal_shade(rgb, in.color.rgb, mn, to_eye, lod.misc.w, env), vec3<f32>(1.0));
#endif
    if (fog.color.w > 0.5) {
        rgb = mix(fog.color.rgb, rgb, in.fog);
    }
    if (in.tint == 1u) { rgb = mix(rgb, vec3<f32>(1.0, 0.0, 0.0), 0.5); }
    if (in.tint == 2u) { rgb = mix(rgb, vec3<f32>(0.0, 0.2, 1.0), 0.5); }
    if (in.tint == 3u) { rgb = mix(rgb, vec3<f32>(0.0, 1.0, 0.0), 0.5); }
    // As = (At * Af) >> 7 in GS units (At raw 0..0x80, Af 0x80 = 1.0), clamped to 0xff; FGE leaves it alone.
    let a_s = min(floor(round(t.a * 255.0) * round(in.color.a * 128.0) / 128.0), 255.0);
    // GS TEST_1 alpha test (ATST GEQUAL AREF, AFAIL RGB_ONLY): which half of the split this draw is (gs_state.rs).
#ifdef GS_ATEST_PASS
    if (a_s < f32(#{GS_AREF})) { discard; }
#endif
#ifdef GS_ATEST_FAIL
    if (a_s >= f32(#{GS_AREF})) { discard; }
#endif
    let s = fog.tex_mode.z;
    if (s > 1.0 && !end_fade_shown(end_fade(in.depth, s), in.position.xy)) { discard; }
    return vec4<f32>(srgb_to_linear(rgb), a_s / 128.0);
}
#endif
