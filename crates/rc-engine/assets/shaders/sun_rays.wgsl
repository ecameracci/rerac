// Sun rays On (crate::sun_rays): a half-resolution mask of the bright sky around the sun, blurred radially toward the
// sun and added to the frame. view.params = (sun UV x, y, intensity, aspect), view.extra = (target width, height,
// taps, -).

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
    let size = view.extra.xy;
    let p = vec2<i32>(clamp(floor(in.uv * size), vec2<f32>(0.0), size - 1.0));
    if (textureLoad(depth, p, 0) > 0.0) { return vec4<f32>(0.0, 0.0, 0.0, 1.0); }
    let c = textureSampleLevel(source, linear, in.uv, 0.0).rgb;
    let lum = dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
    let d = (in.uv - view.params.xy) * vec2<f32>(view.params.w, 1.0);
    let disc = 1.0 - smoothstep(0.0, 0.6, length(d));
    return vec4<f32>(c * (0.25 + 2.0 * max(lum - 0.6, 0.0)) * disc, 1.0);
}

@fragment
fn composite(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let base = textureLoad(source, vec2<i32>(floor(in.position.xy)), 0);
    let taps = i32(view.extra.z);
    let step = (in.uv - view.params.xy) * (0.9 / f32(taps));
    var tc = in.uv;
    var weight = 1.0;
    var acc = vec3<f32>(0.0);
    for (var i = 0; i < taps; i += 1) {
        tc -= step;
        acc += textureSampleLevel(mask_tex, linear, tc, 0.0).rgb * weight;
        weight *= 0.95;
    }
    return vec4<f32>(base.rgb + acc * (view.params.z / f32(taps)) * 4.0, base.a);
}
