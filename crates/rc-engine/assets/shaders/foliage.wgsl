// Foliage pass (port-only, the Foliage option): grass clumps and copies of the level's small plant shrubs.
// Rust side: foliage.rs (the records, the chunks, the per-frame buffer); placement: foliage/plan.rs.
//
// A chunk entity's mesh is a template repeated once per slot (`slot`); its record is insts[mesh tag + slot]. A record
// with zero size is an empty slot (the vertex is dropped).
// * Thinning (plan::thin): density f = clamp((end - d) / (end - full), 0, 1) at the camera distance d of the root; the
//   plant is drawn at smoothstep((f (1 + band) - rank) / band) of its size, so it shrinks to nothing before its cut-off.
// * Wind: the terms of ShrubProc's sway (rc_formats::shrub::wind_sway, shrub_render.rs update_sway) on the 60 Hz tick
//   with the record's seed as the matrix block address, the same shear x += sx * h, z -= sy * h (Bevy axes, h = the
//   vertex's height over the root), mode 1 (halved) for grass, the class's own mode for copies.
// * Ratchet: within BEND_RADIUS of his position now and over the last 0.4 s (weights in frame.hero[k].w) the plant
//   leans away from him and down, by the strongest of them; as he leaves it springs back.
// * Fog: one F per plant from its root's depth, the tfrag slope / offset / clamps (shrub.wgsl's form).
// * Colour (display-encoded, converted to linear at the end like every world shader): grass = its root colour
//   blended to its tip colour up the blade, shaded per blade; copies = texture (GS mip rule, sample_world) MODULATE
//   the source instance's palette entry, alpha-tested at 0x40.

#import bevy_pbr::{
    mesh_functions,
    view_transformations::{position_world_to_clip, position_world_to_view},
}

struct WorldFog {
    color: vec4<f32>,
    params: vec4<f32>,
    // x: the texture option (crate::graphics; sample_world).
    tex_mode: vec4<f32>,
}

struct FoliageParams {
    // x = kind (0 grass, 1 plant copy), y = MXL, z = near (32).
    misc: vec4<f32>,
}

struct FoliageInst {
    // xyz = root (Bevy world), w = rank.
    pos: vec4<f32>,
    // Grass: cos yaw, sin yaw, height, width. Copy: 1, 0, scale, 1.
    shape: vec4<f32>,
    // Grass: root RGBA8, tip RGBA8. Copy: source shrub instance (its palette), 0. Then the seed, kind | sway mode << 8.
    info: vec4<u32>,
    // Copy: class -> Bevy-world linear map (columns; yaw and scale included).
    m0: vec4<f32>,
    m1: vec4<f32>,
    m2: vec4<f32>,
}

struct FoliageFrame {
    // x = 60 Hz tick, y = sway on.
    tick: vec4<u32>,
    // x = full-density distance, y = end distance, z = shrink band.
    dist: vec4<f32>,
    eye: vec4<f32>,
    // xyz = Ratchet now and 8, 16, 24 ticks ago (Bevy world), w = weight (0: no Ratchet).
    hero: array<vec4<f32>, 4>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var tex: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var tex_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var<storage, read> fog: WorldFog;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var<uniform> params: FoliageParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var<storage, read> insts: array<FoliageInst>;
@group(#{MATERIAL_BIND_GROUP}) @binding(5) var<storage, read> palettes: array<u32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(6) var<storage, read> frame: FoliageFrame;

const BEND_RADIUS: f32 = 0.9;

// As shrub.wgsl / tfrag.wgsl (the texture option, `fog.tex_mode.x`).
fn sample_world(uv: vec2<f32>, l: f32, mxl: f32, duv: vec4<f32>) -> vec4<f32> {
    let mode = fog.tex_mode.x;
    if mode > 1.5 {
        return textureSampleGrad(tex, tex_sampler, uv, duv.xy, duv.zw);
    }
    let level = select(clamp(floor(l + 0.5), 0.0, mxl), clamp(l, 0.0, mxl), mode > 0.5);
    return textureSampleLevel(tex, tex_sampler, uv, level);
}

// ShrubProc's sine table entry i / 128 (rc_formats::shrub::sway_table: -127 sin(2 pi i / 256), truncated there).
fn sway_sin(i: u32) -> f32 {
    return -sin(f32(i & 0xffu) * (6.2831853 / 256.0)) * (127.0 / 128.0);
}

// rc_formats::shrub::wind_sway (vf8 = (0.1, 0.02, 1, 0), vf9 = (0.2, 6000, 1 / 6000, 0.5)).
fn wind(mode: u32, block: u32, root: vec3<f32>) -> vec2<f32> {
    if (mode == 0u || frame.tick.y == 0u) { return vec2<f32>(0.0); }
    let rel = root - frame.eye.xyz;
    let d2 = dot(rel, rel);
    if (6000.0 - d2 < 0.0) { return vec2<f32>(0.0); }
    let k = 1.0 - d2 / 6000.0;
    let tick = frame.tick.x;
    let twice = (mode >> 1u) != 0u;
    let u = block * 67u + tick;
    let w = block * 123u + tick + select(0u, tick, twice);
    let g = ((sway_sin(u) * sway_sin(u >> 1u)) * 0.2 + 0.8) * 0.1;
    var s = vec2<f32>(sway_sin(w + 64u) * 0.02 + g, sway_sin(w >> 1u) * 0.02) * k;
    if (!twice) { s = s * 0.5; }
    return s;
}

// The lean away from Ratchet of a vertex `h` over a root at `root`.
fn bend(root: vec3<f32>, h: f32) -> vec3<f32> {
    var best = 0.0;
    var dir = vec2<f32>(1.0, 0.0);
    for (var k = 0u; k < 4u; k += 1u) {
        let hp = frame.hero[k];
        if (hp.w <= 0.0 || abs(root.y - hp.y) > 1.5) { continue; }
        let v = root.xz - hp.xz;
        let d = length(v);
        if (d >= BEND_RADIUS) { continue; }
        let a = (1.0 - d / BEND_RADIUS) * (1.0 - d / BEND_RADIUS) * hp.w;
        if (a > best) {
            best = a;
            dir = select(vec2<f32>(1.0, 0.0), v / d, d > 1e-3);
        }
    }
    let lean = best * h;
    return vec3<f32>(dir.x * lean * 0.9, -lean * 0.45, dir.y * lean * 0.9);
}

struct FoliageVertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
    // Copies: palette entry | TEX1 K (s12, 1/16 units) << 20 (shrub_render.rs ATTRIBUTE_SHRUB_INFO).
    @location(2) info: u32,
    @location(3) slot: u32,
}

struct FoliageVertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) @interpolate(flat) fog: f32,
    @location(3) depth: f32,
    @location(4) @interpolate(flat) k: f32,
}

fn culled() -> FoliageVertexOutput {
    var out: FoliageVertexOutput;
    out.position = vec4<f32>(2.0, 2.0, 2.0, 1.0);
    return out;
}

@vertex
fn vertex(v: FoliageVertex) -> FoliageVertexOutput {
    let inst = insts[mesh_functions::get_tag(v.instance_index) + v.slot];
    if (inst.shape.z <= 0.0) { return culled(); }
    let root = inst.pos.xyz;
    let d = distance(frame.eye.xyz, root);
    let f = clamp((frame.dist.y - d) / max(frame.dist.y - frame.dist.x, 1e-3), 0.0, 1.0);
    let s = smoothstep(0.0, 1.0, clamp((f * (1.0 + frame.dist.z) - inst.pos.w) / frame.dist.z, 0.0, 1.0));
    if (s <= 0.0) { return culled(); }
    let kind = inst.info.w & 0xffu;
    var off: vec3<f32>;
    var color: vec4<f32>;
    var out: FoliageVertexOutput;
    if (kind == 0u) {
        let p = v.position;
        let c = inst.shape.x;
        let sn = inst.shape.y;
        // Thinned clumps a little wider, so the far field keeps its cover.
        let w = inst.shape.w * (1.0 + 0.4 * (1.0 - f));
        off = vec3<f32>((c * p.x + sn * p.z) * w, p.y * inst.shape.z, (-sn * p.x + c * p.z) * w) * s;
        let root_c = unpack4x8unorm(inst.info.x);
        let tip_c = unpack4x8unorm(inst.info.y);
        let shade = 0.88 + 0.24 * v.uv.y;
        color = vec4<f32>(min(mix(root_c.rgb, tip_c.rgb, pow(clamp(v.uv.x, 0.0, 1.0), 0.8)) * mix(1.0, shade, clamp(v.uv.x * 2.0, 0.0, 1.0)), vec3<f32>(1.0)), 1.0);
    } else {
        off = mat3x3<f32>(inst.m0.xyz, inst.m1.xyz, inst.m2.xyz) * v.position * s;
        let entry = (v.info & 0xfffu) % 24u;
        let c = unpack4x8unorm(palettes[inst.info.x * 24u + entry]) * (255.0 / 128.0);
        color = vec4<f32>(c.rgb, 1.0);
        out.k = f32(bitcast<i32>(v.info) >> 20u) / 16.0;
    }
    let h = max(off.y, 0.0);
    let sw = wind((inst.info.w >> 8u) & 3u, inst.info.z, root);
    off = vec3<f32>(off.x + sw.x * h, off.y, off.z - sw.y * h) + bend(root, h);
    let world = root + off;
    out.position = position_world_to_clip(world);
    out.uv = v.uv;
    out.color = color;
    out.depth = -position_world_to_view(world).z * 1024.0;
    let root_depth = -position_world_to_view(root).z;
    let fv = min(max(root_depth * 1024.0 * fog.params.x + fog.params.y, fog.params.z), fog.params.w);
    out.fog = trunc(fv) / 255.0;
    return out;
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + 0.055) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}

@fragment
fn fragment(in: FoliageVertexOutput) -> @location(0) vec4<f32> {
    let duv = vec4<f32>(dpdx(in.uv), dpdy(in.uv));
    var rgb = in.color.rgb;
    if (params.misc.x > 0.5) {
        let t = sample_world(in.uv, log2(max(in.depth, 1.0) / params.misc.z) + in.k, params.misc.y, duv);
        if (round(t.a * 255.0) < 64.0) { discard; }
        rgb = min(t.rgb * in.color.rgb, vec3<f32>(1.0));
    }
    if (fog.color.w > 0.5) {
        rgb = mix(fog.color.rgb, rgb, in.fog);
    }
    return vec4<f32>(srgb_to_linear(rgb), 1.0);
}
