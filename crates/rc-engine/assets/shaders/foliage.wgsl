// Foliage pass (port-only, the Foliage option): grass clumps, flower clusters, ferns and copies of the level's small
// plant shrubs.
// Rust side: foliage.rs (the records, the chunks, the per-frame buffer); placement: foliage/plan.rs.
//
// A chunk entity's mesh is a template repeated once per slot (`slot`); its record is insts[mesh tag + slot]. A record
// with zero size is an empty slot (the vertex is dropped).
// * Thinning (plan::thin): density f = clamp((end - d) / (end - full), 0, 1) at the camera distance d of the root; the
//   plant is drawn at smoothstep((f (1 + band) - rank) / band) of its size, so it shrinks to nothing before its cut-off.
// * Wind: the terms of ShrubProc's sway (rc_formats::shrub::wind_sway, shrub_render.rs update_sway) on the 60 Hz tick
//   with the record's seed as the matrix block address, the same shear x += sx * h, z -= sy * h (Bevy axes, h = the
//   vertex's height over the root), mode 1 (halved) for grass, the class's own mode for copies.
// * Bend: every mover (Ratchet and the drawn mobys near the camera, foliage.rs `movers`) is a capsule from its
//   trail's tail to its feet; a root within its footprint radius of it (and from 0.8 under to 0.6 over its lowest
//   point) is pushed away from the nearest point by (1 - d / r)^2, weighted from 1 at the feet down the tail (to 0
//   at its end once it is TAIL_FADE long), the pushes summed and capped; the plant turns by up to BEND_MAX about its root (h sin, h (1 - cos)). The tail follows
//   on the CPU, so the plant springs back as it passes.
// * Fog: one F per plant from its root's depth, the tfrag slope / offset / clamps (shrub.wgsl's form).
// * Colour (display-encoded, converted to linear at the end like every world shader): blades and ferns = the root
//   colour blended to the tip colour up the blade / along the frond, shaded per blade; flowers = a blade-coloured
//   stem, white or yellow petals (info.w bit 16) round a yellow centre, dimmed with the ground's brightness; copies =
//   texture (GS mip rule, sample_world) MODULATE the source instance's palette entry, alpha-tested at 0x40.

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
    // Grass, flowers, ferns: cos yaw, sin yaw, height, width. Copy: 1, 0, scale, 1.
    shape: vec4<f32>,
    // Root RGBA8, tip RGBA8 (copy: source shrub instance (its palette), 0), the seed, kind (0 blades, 1 copy,
    // 2 flowers, 3 fern) | sway mode << 8 | yellow flowers << 16.
    info: vec4<u32>,
    // Copy: class -> Bevy-world linear map (columns; yaw and scale included).
    m0: vec4<f32>,
    m1: vec4<f32>,
    m2: vec4<f32>,
}

struct Mover {
    // xyz = feet now (Bevy world), w = footprint radius.
    now: vec4<f32>,
    // xyz = the trail's tail, w = the lowest point (y).
    tail: vec4<f32>,
}

struct FoliageFrame {
    // x = 60 Hz tick, y = sway on, z = movers.
    tick: vec4<u32>,
    // x = full-density distance, y = end distance, z = shrink band.
    dist: vec4<f32>,
    eye: vec4<f32>,
    // The box around every mover's capsule, footprint and height window.
    lo: vec4<f32>,
    hi: vec4<f32>,
    movers: array<Mover, 48>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var tex: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var tex_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var<storage, read> fog: WorldFog;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var<uniform> params: FoliageParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var<storage, read> insts: array<FoliageInst>;
@group(#{MATERIAL_BIND_GROUP}) @binding(5) var<storage, read> palettes: array<u32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(6) var<storage, read> frame: FoliageFrame;

// The most a plant turns about its root (radians), at a full push.
const BEND_MAX: f32 = 1.2;
// The tail length (world units) from which the push fades to nothing at the tail's end (foliage.rs TAIL_MAX's
// shorter one).
const TAIL_FADE: f32 = 0.8;

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

// The push on a root from every mover (header): its direction (Bevy x, z) times its strength 0..1.
fn push(root: vec3<f32>) -> vec2<f32> {
    if (any(root < frame.lo.xyz) || any(root > frame.hi.xyz)) { return vec2<f32>(0.0); }
    var sum = vec2<f32>(0.0);
    for (var k = 0u; k < frame.tick.z; k += 1u) {
        let m = frame.movers[k];
        let dy = root.y - m.tail.w;
        if (dy < -0.8 || dy > 0.6) { continue; }
        let a = m.tail.xz;
        let ab = m.now.xz - a;
        let l2 = dot(ab, ab);
        let t = select(1.0, clamp(dot(root.xz - a, ab) / l2, 0.0, 1.0), l2 > 1e-6);
        let v = root.xz - (a + ab * t);
        let d = length(v);
        if (d >= m.now.w) { continue; }
        let f = 1.0 - d / m.now.w;
        // 1 at the feet, down to 0 at the tail once the tail is TAIL_FADE long (continuous as it grows from 0).
        let w = 1.0 - (1.0 - t) * min(sqrt(l2) / TAIL_FADE, 1.0);
        sum += select(vec2<f32>(1.0, 0.0), v / d, d > 1e-3) * f * f * w;
    }
    let s = length(sum);
    return select(sum, sum / s, s > 1.0);
}

// The offset of a vertex `h` over its root under the push `p`: a turn about the root, away from the mover.
fn bend(p: vec2<f32>, h: f32) -> vec3<f32> {
    let s = length(p);
    if (s <= 0.0) { return vec3<f32>(0.0); }
    let a = s * BEND_MAX;
    let dir = p / s;
    return vec3<f32>(dir.x * h * sin(a), -h * (1.0 - cos(a)), dir.y * h * sin(a));
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
    if (kind != 1u) {
        let p = v.position;
        let c = inst.shape.x;
        let sn = inst.shape.y;
        // Thinned clumps a little wider, so the far field keeps its cover.
        let w = inst.shape.w * select(1.0, 1.0 + 0.4 * (1.0 - f), kind == 0u);
        off = vec3<f32>((c * p.x + sn * p.z) * w, p.y * inst.shape.z, (-sn * p.x + c * p.z) * w) * s;
        let root_c = unpack4x8unorm(inst.info.x);
        let tip_c = unpack4x8unorm(inst.info.y);
        let shade = 0.88 + 0.24 * fract(v.uv.y);
        let ramp = select(pow(clamp(v.uv.x, 0.0, 1.0), 0.8), v.uv.x, kind == 3u);
        var rgb = min(mix(root_c.rgb, tip_c.rgb, ramp) * mix(1.0, shade, clamp(v.uv.x * 2.0, 0.0, 1.0)), vec3<f32>(1.0));
        if (kind == 2u && v.uv.y > 1.5) {
            // The ground's brightness against Novalis-like grass (luma about 0.34), so heads do not glow in shade.
            let light = clamp(dot(root_c.rgb, vec3<f32>(0.3, 0.59, 0.11)) / 0.3, 0.45, 1.0);
            let petal = select(vec3<f32>(0.96, 0.95, 0.9), vec3<f32>(1.0, 0.86, 0.22), ((inst.info.w >> 16u) & 1u) != 0u);
            rgb = mix(petal, vec3<f32>(0.98, 0.72, 0.14), clamp(v.uv.y - 2.0, 0.0, 1.0)) * light;
        }
        color = vec4<f32>(rgb, 1.0);
    } else {
        off = mat3x3<f32>(inst.m0.xyz, inst.m1.xyz, inst.m2.xyz) * v.position * s;
        let entry = (v.info & 0xfffu) % 24u;
        let c = unpack4x8unorm(palettes[inst.info.x * 24u + entry]) * (255.0 / 128.0);
        color = vec4<f32>(c.rgb, 1.0);
        out.k = f32(bitcast<i32>(v.info) >> 20u) / 16.0;
    }
    let h = max(off.y, 0.0);
    let sw = wind((inst.info.w >> 8u) & 3u, inst.info.z, root);
    off = vec3<f32>(off.x + sw.x * h, off.y, off.z - sw.y * h) + bend(push(root), h);
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
