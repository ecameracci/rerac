// The scene copy (crate::scene_copy): the main view's depth (sample 0 with MSAA) into an R32Float image and, for the
// water, the frame drawn so far (the opaque pass's resolve) into an sRGB image, pixel for pixel.

#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

#ifdef MULTISAMPLED
@group(0) @binding(0) var depth: texture_depth_multisampled_2d;
#else
@group(0) @binding(0) var depth: texture_depth_2d;
#endif
@group(0) @binding(1) var color: texture_2d<f32>;

struct CopyOut {
    @location(0) depth: vec4<f32>,
#ifdef COPY_COLOR
    @location(1) color: vec4<f32>,
#endif
}

@fragment
fn fragment(in: FullscreenVertexOutput) -> CopyOut {
    let p = vec2<i32>(floor(in.position.xy));
    var out: CopyOut;
    out.depth = vec4<f32>(textureLoad(depth, p, 0), 0.0, 0.0, 1.0);
#ifdef COPY_COLOR
    out.color = textureLoad(color, p, 0);
#endif
    return out;
}
