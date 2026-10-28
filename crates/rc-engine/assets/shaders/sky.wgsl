// Sky shells as the EE-built GS packets draw them (docs/plan/sky_render_notes.md, sky_render.rs).
//
// Transform (boot 0x22bf94): p = (x, y, z, 1)·SkyM·C with the raw s16 vertex integers, C = boot 0x187040 =
// rotation-only view × the world projection, then XY = 2048 + (p.xy / p.w)·(256, 208). SkyM (the shell's
// rotation; the space skies' scaled, turned and translated shells, crate::flight_render) is the mesh transform,
// applied to (x, y, z, 1) as the game does (a level sky's entity has no translation and no scale, so that is its
// rotation alone); the view's translation is dropped by transforming the result as a direction (w = 0).
// GS Z is 0 for every sky vertex (the builders write the XYZ2 high word as 0): clip.z = 0 gives depth 0,
// which is GS Z 0 under the port's depth = Z / 2^24 mapping (game_camera.rs), the far end.
//
// Interpolation: the GS interpolates RGBA linearly in screen space, and the textured builder sends Q = 1.0,
// so S/Q and T/Q are screen-space linear too (affine texture mapping): all attributes are
// @interpolate(linear).
//
// Colour, per shell kind (PRIM 0x4b gouraud / 0x5b textured, FGE 0 = no fog for both):
// * gouraud: the GS writes the vertex RGBA as is (display-encoded bytes); As = vertex alpha.
// * textured: TFX MODULATE, TCC 1 with RGBAQ = (0x80, 0x80, 0x80, a): Cv = min(Ct·0x80 >> 7, 0xff) = Ct,
//   As = (At·a) >> 7. The texture holds the raw texel alpha (0..0x80) as byte / 255.
// Blend: ALPHA_1 = (Cs − Cd)·As >> 7 + Cd for both; the fragment returns As / 128 for SrcAlpha /
// OneMinusSrcAlpha. The textured alpha test (GEQUAL 0x80, AFAIL FB_ONLY) never suppresses the colour write.

//
// SKY_STAR_GLOW (Particles Enhanced on the flight's textured shells, crate::flight_render): the stars the texture paints
// glow and twinkle, kept close to the game's starfield. A bright-pass (texel max channel above 0.3, by its alpha) at
// the pixel and on its 8 neighbours (0.85 × the larger of a texel and the pixel's footprint: no shimmer when the shell
// is minified) makes a faint core boost and a soft one-texel halo; each 2×2-texel cell twinkles ±15 % at its own rate
// (a hash of the cell, the frame's time) and is tinted slightly warm or cool; the glow, × the vertex alpha, is added to the GS blend's result (the pipeline's colour source factor One,
// the shell premultiplied here).

#import bevy_pbr::{forward_io::Vertex, mesh_functions, mesh_view_bindings::view}
#ifdef SKY_STAR_GLOW
#import bevy_pbr::mesh_view_bindings::globals
#endif

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var tex: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var tex_sampler: sampler;

struct SkyVertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) @interpolate(linear) uv: vec2<f32>,
    // Raw GS RGBA bytes (0..255).
    @location(1) @interpolate(linear) color: vec4<f32>,
}

@vertex
fn vertex(v: Vertex) -> SkyVertexOutput {
    var out: SkyVertexOutput;
    let world_from_local = mesh_functions::get_world_from_local(v.instance_index);
    // (x, y, z, 1)·SkyM: the whole shell matrix (rotation, scale, translation row).
    let dir = (world_from_local * vec4<f32>(v.position, 1.0)).xyz;
    // Rotation-only view (0x186f40 without the camera translation), then the world projection.
    let view_dir = (view.view_from_world * vec4<f32>(dir, 0.0)).xyz;
    var clip = view.clip_from_view * vec4<f32>(view_dir, 1.0);
    clip.z = 0.0;
    out.position = clip;
#ifdef VERTEX_UVS_A
    out.uv = v.uv;
#endif
#ifdef VERTEX_COLORS
    out.color = v.color;
#else
    out.color = vec4<f32>(128.0);
#endif
    return out;
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + 0.055) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}

#ifdef SKY_STAR_GLOW
fn star_hash(x: u32) -> u32 {
    var v = x * 747796405u + 2891336453u;
    v = ((v >> ((v >> 28u) + 4u)) ^ v) * 277803737u;
    return (v >> 22u) ^ v;
}

fn star_unit(x: u32) -> f32 { return f32(star_hash(x) & 0xffffu) / 65535.0; }

fn bright_pass(uv: vec2<f32>) -> f32 {
    let s = textureSampleLevel(tex, tex_sampler, uv, 0.0);
    return max(max(s.r, max(s.g, s.b)) - 0.3, 0.0) * min(s.a * 255.0 / 128.0, 1.0);
}

fn star_glow(uv: vec2<f32>) -> vec3<f32> {
    let dims = vec2<f32>(textureDimensions(tex));
    let foot = max(1.0 / dims, fwidth(uv));
    var halo = 0.0;
    // An even ring (no axis bias, so no cross): 8 taps at 22.5° + k·45°.
    for (var k = 0; k < 8; k++) {
        let t = (f32(k) + 0.5) * 0.7853982;
        halo += bright_pass(uv + vec2<f32>(cos(t), sin(t)) * foot * 1.1) * 0.035;
    }
    let core = bright_pass(uv);
    let cell = vec2<u32>(floor(uv * dims * 0.5));
    let seed = star_hash(cell.x * 7919u + cell.y);
    let f = 1.3 + 2.2 * star_unit(seed);
    let tw = 1.0 + 0.15 * sin(globals.time * f + 6.2831853 * star_unit(seed + 1u));
    let tint = mix(vec3<f32>(0.9, 0.94, 1.0), vec3<f32>(1.0, 0.95, 0.88), star_unit(seed + 2u));
    return srgb_to_linear(tint) * (core * 0.35 + halo) * tw;
}
#endif

@fragment
fn fragment(in: SkyVertexOutput) -> @location(0) vec4<f32> {
    // GS integer colour: the interpolated RGBA is truncated to 8 bits per channel.
    let c = floor(in.color);
#ifdef SKY_TEXTURED
    let t = textureSample(tex, tex_sampler, in.uv);
    let rgb = min(t.rgb * c.rgb / 128.0, vec3<f32>(1.0));
    let a_s = min(floor(round(t.a * 255.0) * c.a / 128.0), 255.0);
#else
    let rgb = c.rgb / 255.0;
    let a_s = c.a;
#endif
#ifdef SKY_STAR_GLOW
    return vec4<f32>(srgb_to_linear(rgb) * (a_s / 128.0) + star_glow(in.uv) * (c.a / 128.0), a_s / 128.0);
#else
    return vec4<f32>(srgb_to_linear(rgb), a_s / 128.0);
#endif
}
