// Port-only (the Fur option): Ratchet's fur shells (crate::fur). Each vertex is skinned and lit as moby.wgsl does it
// (the same records, palette, normal table and `MobyLod`; GPU light mode, else unlit white), then pushed out along
// its skinned normal by its shell's height h × the fur length, the outer shells bent by a little wind and droop
// (h²). Fragment: the texture × the vertex colour as on his skin, the GS fog, then a strand mask: the texture's UV
// cut into cells of `params.misc.z` per unit, each holding one strand (a jittered centre, its own length and
// thickness) that thins towards its tip; the alpha tapers with h, the roots are darkened (self-shadowing).
// `params.misc.x` = 0 (Fur Original): every vertex is dropped.

#import bevy_pbr::{
    mesh_functions,
    mesh_view_bindings::globals,
    view_transformations::{position_world_to_clip, position_world_to_view},
}

struct MobyFog {
    color: vec4<f32>,
    params: vec4<f32>,
}

struct MobyInst {
    model: mat4x4<f32>,
    light_rows: array<vec4<f32>, 3>,
    light_colors: array<vec4<f32>, 3>,
    neg_k: vec4<f32>,
    ambient: vec4<f32>,
    misc: vec4<u32>,
}

struct MobyLod {
    misc: vec4<u32>,
    e: array<vec4<f32>, 3>,
}

struct FurParams {
    // x = on, y = length (world units), z = strands per texture unit, w = shells.
    misc: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var tex: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var tex_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var<storage, read> fog: MobyFog;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var<storage, read> insts: array<MobyInst>;
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var<storage, read> palette: array<mat4x4<f32>>;
@group(#{MATERIAL_BIND_GROUP}) @binding(5) var<storage, read> normal_table: array<vec2<f32>>;
@group(#{MATERIAL_BIND_GROUP}) @binding(6) var<storage, read> lods: array<MobyLod>;
@group(#{MATERIAL_BIND_GROUP}) @binding(7) var<uniform> params: FurParams;

struct FurVertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) skin: vec4<u32>,
    @location(3) h: f32,
}

struct FurOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(linear) color: vec4<f32>,
    @location(2) @interpolate(linear) fog: f32,
    @location(3) h: f32,
}

fn s16(x: i32) -> i32 { return (x << 16u) >> 16u; }

@vertex
fn vertex(v: FurVertex) -> FurOut {
    var out: FurOut;
    let tag = mesh_functions::get_tag(v.instance_index);
    let inst = &insts[tag];
    let lod = &lods[tag];
    // Off, or the moby is drawn into the sun's shadow map only (crate::moby_lod `FLAG_SHADOW_ONLY`).
    if (params.misc.x < 0.5 || ((*lod).misc.y & 4u) != 0u) {
        out.position = vec4<f32>(2.0, 2.0, 2.0, 1.0);
        return out;
    }
    let base = (*inst).misc.x;
    let alpha = f32((*lod).misc.x);
    let count = (v.skin.x >> 16u) & 0xffu;
    let j = vec3<u32>(v.skin.y & 0xffu, (v.skin.y >> 8u) & 0xffu, (v.skin.y >> 16u) & 0xffu);
    var m = palette[base + j.x];
    if (count == 0u) {
        m = mat4x4<f32>(vec4<f32>(1.0, 0.0, 0.0, 0.0), vec4<f32>(0.0, 1.0, 0.0, 0.0), vec4<f32>(0.0, 0.0, 1.0, 0.0), vec4<f32>(0.0, 0.0, 0.0, 1.0));
    }
    if (count >= 2u) {
        let w = vec3<f32>(f32(v.skin.z & 0x3ffu), f32((v.skin.z >> 10u) & 0x3ffu), f32((v.skin.z >> 20u) & 0x3ffu)) / 256.0;
        m = palette[base + j.x] * w.x + palette[base + j.y] * w.y;
        if (count >= 3u) { m = m + palette[base + j.z] * w.z; }
    }
    // The skin's own position (moby.wgsl: ftoi0 and the s16 wrap), so the shells start on his surface.
    let sp = clamp(trunc((m * vec4<f32>(v.position, 1.0)).xyz), vec3<f32>(-2147483520.0), vec3<f32>(2147483520.0));
    let pi = vec3<i32>(s16(i32(sp.x)), s16(i32(sp.y)), s16(i32(sp.z)));
    let skin_world = ((*inst).model * vec4<f32>(vec3<f32>(pi), 1.0)).xyz;

    let ta = normal_table[v.skin.x & 0xffu];
    let te = normal_table[(v.skin.x >> 8u) & 0xffu];
    let n = vec3<f32>(ta.x * te.x, ta.y * te.x, te.y);
    let np = (m[0] * n.x + m[1] * n.y + m[2] * n.z).xyz;
    let nw = normalize(((*inst).model * vec4<f32>(np, 0.0)).xyz + vec3<f32>(0.0, 1e-6, 0.0));

    // Out along the normal; the outer shells sway (two slow waves, offset per position) and droop.
    let len = params.misc.y;
    let t = globals.time;
    let phase = dot(skin_world, vec3<f32>(3.1, 1.7, 2.3));
    let wind = vec3<f32>(sin(t * 1.9 + phase), 0.0, cos(t * 1.4 + phase * 0.7)) * 0.35 + vec3<f32>(0.0, -0.45, 0.0);
    let world = skin_world + nw * (len * v.h) + wind * (len * v.h * v.h);
    out.position = position_world_to_clip(world);
    let scroll = (*inst).misc.w;
    out.uv = v.uv + vec2<f32>(f32(s16(i32(scroll & 0xffffu))), f32(s16(i32(scroll >> 16u)))) / 4096.0;
    out.h = v.h;

    if ((*inst).misc.y == 0u) {
        let d = (*inst).light_rows[0] * np.x + (*inst).light_rows[1] * np.y + (*inst).light_rows[2] * np.z;
        let f = max(d, d * (*inst).neg_k);
        let s = (*inst).light_colors[0] * f.x + (*inst).light_colors[1] * f.y + (*inst).light_colors[2] * f.z;
        let q = inverseSqrt(np.x * np.x + np.y * np.y + np.z * np.z);
        let c = vec4<f32>((*inst).ambient.xyz, alpha) + floor(s * q * 128.0);
        let mult = vec4<i32>(unpack4x8unorm(v.skin.w) * 255.0 + 0.5);
        let hh = vec4<i32>(s16(i32(c.x)), s16(i32(c.y)), s16(i32(c.z)), s16(i32(c.w)));
        let p = clamp(hh * mult, vec4<i32>(-32768), vec4<i32>(32767)) >> vec4<u32>(7u);
        out.color = vec4<f32>(p & vec4<i32>(0xff)) / 128.0;
    } else {
        out.color = vec4<f32>(1.0, 1.0, 1.0, alpha / 128.0);
    }
    out.color.a = alpha / 128.0;
    let depth = -position_world_to_view(world).z * 1024.0;
    let fg = max(min(depth * fog.params.x + fog.params.y, fog.params.w), fog.params.z);
    out.fog = trunc(fg) / 255.0;
    return out;
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + 0.055) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}

fn hash2(p: vec2<f32>) -> vec2<f32> {
    let q = vec2<f32>(dot(p, vec2<f32>(127.1, 311.7)), dot(p, vec2<f32>(269.5, 183.3)));
    return fract(sin(q) * 43758.5453);
}

@fragment
fn fragment(in: FurOut) -> @location(0) vec4<f32> {
    // One strand per cell: its root jittered in the cell, its length 55..100 % of the fur, its root radius.
    let g = in.uv * params.misc.z;
    let cell = floor(g);
    var cover = 0.0;
    // The cell and its neighbours, so a strand bent near an edge is not cut.
    for (var dy = -1; dy <= 1; dy += 1) {
        for (var dx = -1; dx <= 1; dx += 1) {
            let c = cell + vec2<f32>(f32(dx), f32(dy));
            let r = hash2(c);
            let r2 = hash2(c + vec2<f32>(17.0, 59.0));
            let tip = 0.55 + 0.45 * r2.x;
            if (in.h > tip) { continue; }
            let centre = c + 0.15 + 0.7 * r;
            let radius = (0.38 + 0.2 * r2.y) * (1.0 - in.h / tip);
            let d = length(g - centre);
            cover = max(cover, 1.0 - smoothstep(radius * 0.6, radius, d));
        }
    }
    if (cover < 0.02) { discard; }
    // Level 0 (the moby textures have no mips): no derivatives after the discard.
    let t = textureSampleLevel(tex, tex_sampler, in.uv, 0.0);
    // The roots in their neighbours' shade, the tips catching the light.
    var rgb = min(t.rgb * in.color.rgb * mix(0.72, 1.08, in.h), vec3<f32>(1.0));
    if (fog.color.w > 0.5) {
        rgb = mix(fog.color.rgb, rgb, in.fog);
    }
    let a = cover * (1.0 - in.h * in.h) * 0.95 * clamp(in.color.a, 0.0, 1.0);
    return vec4<f32>(srgb_to_linear(rgb), a);
}
