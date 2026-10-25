// Lighting Enhanced's post-process (crate::post_render): the bloom chain (prefilter, down, up) and the composite (CAS,
// the bloom added, the grade). Every colour is linear light: the frame's sRGB view decodes on load, encodes on store.
// The curves mirror post_render.rs (`soft_threshold`, `tone`, `vibrance`).

#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

struct Post {
    // xy: 1 / the pass's source size, z: the scatter (up passes).
    texel: vec4<f32>,
    // Threshold, knee, sky weight, bloom strength / the levels' weight sum (0: no bloom).
    bloom: vec4<f32>,
    // Contrast, shoulder, vibrance, grade on (1) / off (0).
    grade: vec4<f32>,
    // x: CAS sharpness (0: off), y: mid grey, zw: 1 / the first bloom level's size.
    sharp: vec4<f32>,
}

@group(0) @binding(0) var<uniform> post: Post;
#ifdef MULTISAMPLED
@group(0) @binding(1) var depth: texture_depth_multisampled_2d;
#else
@group(0) @binding(1) var depth: texture_depth_2d;
#endif
@group(0) @binding(2) var source: texture_2d<f32>;
@group(0) @binding(3) var linear: sampler;
@group(0) @binding(4) var bloom_tex: texture_2d<f32>;

const LUMA: vec3<f32> = vec3<f32>(0.2126, 0.7152, 0.0722);

fn soft_threshold(l: f32) -> f32 {
    let t = post.bloom.x;
    let k = post.bloom.y;
    var s = clamp(l - t + k, 0.0, 2.0 * k);
    s = s * s / (4.0 * k + 1e-5);
    return max(s, l - t) / max(l, 1e-5);
}

// A frame pixel's bloom input (rgb) and its Karis weight (w), the sky's weighted down. The threshold is on the
// largest channel, so saturated lights (a red lamp) count as bright as white ones.
fn bright(p: vec2<i32>) -> vec4<f32> {
    let q = min(p, vec2<i32>(textureDimensions(source)) - 1);
    let c = textureLoad(source, q, 0).rgb;
    var w = soft_threshold(max(c.r, max(c.g, c.b)));
    if (textureLoad(depth, q, 0) <= 0.0) { w *= post.bloom.z; }
    let b = c * w;
    let karis = 1.0 / (1.0 + dot(b, LUMA));
    return vec4<f32>(b * karis, karis);
}

@fragment
fn prefilter(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let p = vec2<i32>(floor(in.position.xy)) * 2;
    let s = bright(p) + bright(p + vec2<i32>(1, 0)) + bright(p + vec2<i32>(0, 1)) + bright(p + vec2<i32>(1, 1));
    return vec4<f32>(s.rgb / s.w, 1.0);
}

fn tap(uv: vec2<f32>, dx: f32, dy: f32) -> vec3<f32> {
    return textureSampleLevel(source, linear, uv + vec2<f32>(dx, dy) * post.texel.xy, 0.0).rgb;
}

// The 13-tap downsample: four overlapping 2×2 boxes around the centre and the inner box.
@fragment
fn down(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let uv = in.uv;
    let outer = tap(uv, -2.0, -2.0) + tap(uv, 2.0, -2.0) + tap(uv, -2.0, 2.0) + tap(uv, 2.0, 2.0);
    let edges = tap(uv, 0.0, -2.0) + tap(uv, -2.0, 0.0) + tap(uv, 2.0, 0.0) + tap(uv, 0.0, 2.0);
    let inner = tap(uv, -1.0, -1.0) + tap(uv, 1.0, -1.0) + tap(uv, -1.0, 1.0) + tap(uv, 1.0, 1.0);
    let c = tap(uv, 0.0, 0.0) * 0.125 + outer * 0.03125 + edges * 0.0625 + inner * 0.125;
    return vec4<f32>(c, 1.0);
}

fn tent(tex: texture_2d<f32>, uv: vec2<f32>, texel: vec2<f32>) -> vec3<f32> {
    let t = texel;
    var s = textureSampleLevel(tex, linear, uv, 0.0).rgb * 4.0;
    s += (textureSampleLevel(tex, linear, uv + vec2<f32>(-t.x, 0.0), 0.0).rgb
        + textureSampleLevel(tex, linear, uv + vec2<f32>(t.x, 0.0), 0.0).rgb
        + textureSampleLevel(tex, linear, uv + vec2<f32>(0.0, -t.y), 0.0).rgb
        + textureSampleLevel(tex, linear, uv + vec2<f32>(0.0, t.y), 0.0).rgb) * 2.0;
    s += textureSampleLevel(tex, linear, uv - t, 0.0).rgb
        + textureSampleLevel(tex, linear, uv + t, 0.0).rgb
        + textureSampleLevel(tex, linear, uv + vec2<f32>(-t.x, t.y), 0.0).rgb
        + textureSampleLevel(tex, linear, uv + vec2<f32>(t.x, -t.y), 0.0).rgb;
    return s / 16.0;
}

// Added onto the level above (additive blend), × the scatter (texel.z): each wider level weighs less.
@fragment
fn up(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    return vec4<f32>(tent(source, in.uv, post.texel.xy) * post.texel.z, 1.0);
}

fn load(p: vec2<i32>) -> vec3<f32> {
    return textureLoad(source, clamp(p, vec2<i32>(0), vec2<i32>(textureDimensions(source)) - 1), 0).rgb;
}

// Contrast-adaptive sharpening (the cross): a negative lobe of 1/8 .. 1/5 by the sharpness, scaled down where the
// cross's minimum nears 0 or its maximum nears 1.
fn cas(p: vec2<i32>) -> vec3<f32> {
    let e = load(p);
    if (post.sharp.x <= 0.0) { return e; }
    let b = load(p + vec2<i32>(0, -1));
    let d = load(p + vec2<i32>(-1, 0));
    let f = load(p + vec2<i32>(1, 0));
    let h = load(p + vec2<i32>(0, 1));
    let mn = min(min(min(b, d), min(f, h)), e);
    let mx = max(max(max(b, d), max(f, h)), e);
    let amp = sqrt(clamp(min(mn, 1.0 - mx) / max(mx, vec3<f32>(1e-5)), vec3<f32>(0.0), vec3<f32>(1.0)));
    let w = amp * (-1.0 / mix(8.0, 5.0, post.sharp.x));
    return clamp(((b + d + f + h) * w + e) / (1.0 + 4.0 * w), vec3<f32>(0.0), vec3<f32>(1.0));
}

fn tone(x: vec3<f32>) -> vec3<f32> {
    let mid = post.sharp.y;
    let s = post.grade.y;
    let c = mid * pow(max(x, vec3<f32>(0.0)) / mid, vec3<f32>(post.grade.x));
    let over = s + (1.0 - s) * (1.0 - exp(-(c - s) / (1.0 - s)));
    return select(c, over, c > vec3<f32>(s));
}

fn vibrance(c: vec3<f32>) -> vec3<f32> {
    let mx = max(c.r, max(c.g, c.b));
    let mn = min(c.r, min(c.g, c.b));
    let sat = (mx - mn) / max(mx, 1e-5);
    let l = dot(c, LUMA);
    return max(vec3<f32>(l) + (c - vec3<f32>(l)) * (1.0 + post.grade.z * (1.0 - sat)), vec3<f32>(0.0));
}

@fragment
fn composite(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let p = vec2<i32>(floor(in.position.xy));
    var c = cas(p);
    if (post.bloom.w > 0.0) { c += tent(bloom_tex, in.uv, post.sharp.zw) * post.bloom.w; }
    if (post.grade.w > 0.0) { c = vibrance(tone(c)); }
    return vec4<f32>(c, textureLoad(source, p, 0).a);
}
