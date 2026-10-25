// Water Enhanced's other liquids on the seas' surfaces (crate::liquid_render): opaque world surfaces drawn with
// liquid_look.wgsl over the draw's own colour. Vertex: position (Bevy space), ST, GS RGBA bytes / 128
// (crate::fx_draw::PrimBuf). The colour is the GS's MODULATE (C = Ct·Cv >> 7) on the display bytes, the fog the VU's
// F like fx_prim.wgsl, applied after the look.

#import bevy_pbr::view_transformations::{position_world_to_clip, position_world_to_view}
#import bevy_pbr::mesh_view_bindings::{view, globals}
#import rerac::liquid_look::{liquid_look, srgb_to_linear}

struct LiquidFog {
    color: vec4<f32>,
    params: vec4<f32>,
}

struct LiquidParams {
    // x = kind.
    misc: vec4<f32>,
    // xy = the second layer's ST drift per second, z = its ST offset.
    flow: vec4<f32>,
    // xyz = toward the key light (world).
    sun: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var tex: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var tex_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var<uniform> fog: LiquidFog;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var<uniform> params: LiquidParams;

struct LiquidVertex {
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
}

struct LiquidVertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(linear) color: vec4<f32>,
    @location(2) @interpolate(linear) fog: f32,
    @location(3) world: vec3<f32>,
}

@vertex
fn vertex(v: LiquidVertex) -> LiquidVertexOutput {
    var out: LiquidVertexOutput;
    out.position = position_world_to_clip(v.position);
    out.uv = v.uv;
    out.color = v.color;
    out.world = v.position;
    let depth = -position_world_to_view(v.position).z * 1024.0;
    let f = max(min(depth * fog.params.x + fog.params.y, fog.params.w), fog.params.z);
    out.fog = trunc(f) / 255.0;
    return out;
}

// MODULATE on the display bytes: floor(Ct·Cv / 128), clamped to 255.
fn modulate(t: vec4<f32>, c: vec3<f32>) -> vec3<f32> {
    return min(floor(round(t.rgb * 255.0) * c + vec3<f32>(1e-3)), vec3<f32>(255.0)) / 255.0;
}

@fragment
fn fragment(in: LiquidVertexOutput) -> @location(0) vec4<f32> {
    let rgb = modulate(textureSample(tex, tex_sampler, in.uv), in.color.rgb);
    // Four taps around for the mud's low-contrast colour.
    let d = 0.37;
    let soft = (modulate(textureSample(tex, tex_sampler, in.uv + vec2<f32>(d, 0.0)), in.color.rgb)
        + modulate(textureSample(tex, tex_sampler, in.uv + vec2<f32>(-d, d)), in.color.rgb)
        + modulate(textureSample(tex, tex_sampler, in.uv + vec2<f32>(0.0, -d)), in.color.rgb) + rgb) * 0.25;
    let to_eye = normalize(view.world_position - in.world);
    let sky = srgb_to_linear(mix(fog.color.rgb, vec3<f32>(1.0), 0.25));
    var col = liquid_look(u32(params.misc.x + 0.5), rgb, soft, in.uv, in.world, vec2<f32>(0.0), params.flow, normalize(params.sun.xyz), sky, to_eye, globals.time);
    if (fog.color.w > 0.5) {
        col = mix(srgb_to_linear(fog.color.rgb), col, in.fog);
    }
    return vec4<f32>(col, 1.0);
}
