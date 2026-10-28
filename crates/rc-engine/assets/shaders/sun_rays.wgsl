// Sun rays (crate::sun_rays): a half-resolution mask of the sky's sun disc and glow, blurred radially toward the
// sun and added to the frame. view.params = (sun UV x, y, intensity, aspect), view.extra = (taps, the mask's falloff
// radius in target UV heights, the luminance key's low and high ends); the composite's view.extra = (taps, the soft
// cap's knee, sun_rays::KNEE).

#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput
#import rerac::fx_view::view

#ifdef MULTISAMPLED
@group(0) @binding(1) var depth: texture_depth_multisampled_2d;
#else
@group(0) @binding(1) var depth: texture_depth_2d;
#endif
@group(0) @binding(2) var source: texture_2d<f32>;
@group(0) @binding(3) var linear: sampler;
@group(0) @binding(4) var mask_tex: texture_2d<f32>;

@fragment
fn mask(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let size = vec2<f32>(textureDimensions(depth));
    let p = vec2<i32>(clamp(floor(in.uv * size), vec2<f32>(0.0), size - 1.0));
    if (textureLoad(depth, p, 0) > 0.0) { return vec4<f32>(0.0, 0.0, 0.0, 1.0); }
    let c = textureSampleLevel(source, linear, in.uv, 0.0).rgb;
    let lum = dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
    let key = smoothstep(view.extra.z, view.extra.w, lum);
    let d = (in.uv - view.params.xy) * vec2<f32>(view.params.w, 1.0) / view.extra.y;
    let falloff = exp(-dot(d, d));
    return vec4<f32>(c * key * falloff, 1.0);
}

@fragment
fn composite(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let base = textureLoad(source, vec2<i32>(floor(in.position.xy)), 0);
    let taps = i32(view.extra.x);
    let step = (in.uv - view.params.xy) * (0.9 / f32(taps));
    var tc = in.uv;
    var weight = 1.0;
    var acc = vec3<f32>(0.0);
    for (var i = 0; i < taps; i += 1) {
        tc -= step;
        acc += textureSampleLevel(mask_tex, linear, tc, 0.0).rgb * weight;
        weight *= 0.95;
    }
    let rays = acc * (view.params.z / f32(taps)) * 4.0;
    let capped = rays / (1.0 + dot(rays, vec3<f32>(0.2126, 0.7152, 0.0722)) / view.extra.y);
    return vec4<f32>(base.rgb + capped, base.a);
}
