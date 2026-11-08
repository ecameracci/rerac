// The Upscaler option (crate::upscale): FSR 1-style EASU (edge-adaptive spatial upsampling) of the world image into a
// frame-sized texture, then RCAS (robust contrast-adaptive sharpening) into the frame.
//
// Follows AMD FidelityFX Super Resolution 1.0 (ffx_fsr1.h, MIT licence, Copyright (c) 2021 Advanced Micro Devices,
// Inc.): `FsrEasuF` / `FsrEasuSetF` / `FsrEasuTapF` and `FsrRcasF`, rewritten in WGSL (full precision, no packed
// math, texel loads instead of gathers). Both stages work on gamma-2 values (sqrt of the linear colour the sRGB views
// give), as AMD asks for perceptual input; RCAS squares its result back to linear for the frame's sRGB view.

#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

struct Upscale {
    // xy: the source's size (EASU: the world image), zw: the output's (the frame).
    size: vec4<f32>,
    // x: RCAS's lobe scale (exp2(-stops); 0: a plain copy).
    sharp: vec4<f32>,
}

@group(0) @binding(0) var<uniform> u: Upscale;
@group(0) @binding(1) var src: texture_2d<f32>;

// ---- EASU ----

// A world texel, clamped to the image, as gamma-2.
fn tap(p: vec2<i32>) -> vec3<f32> {
    let m = vec2<i32>(u.size.xy) - vec2(1);
    return sqrt(max(textureLoad(src, clamp(p, vec2(0), m), 0).rgb, vec3(0.0)));
}

// AMD's luma approximation (`FsrEasuL`'s weights: B/2 + R/2 + G).
fn luma(c: vec3<f32>) -> f32 { return c.b * 0.5 + (c.r * 0.5 + c.g); }

// `FsrEasuSetF`: one bilinear corner's direction (xy) and edge length (z), weighted by `w`. The cross around the corner
// c: a above, b left, d right, e below.
fn easu_set(w: f32, la: f32, lb: f32, lc: f32, ld: f32, le: f32) -> vec3<f32> {
    let dir_x = ld - lb;
    var len_x = clamp(abs(dir_x) / max(max(abs(ld - lc), abs(lc - lb)), 1.0 / 32768.0), 0.0, 1.0);
    len_x *= len_x;
    let dir_y = le - la;
    var len_y = clamp(abs(dir_y) / max(max(abs(le - lc), abs(lc - la)), 1.0 / 32768.0), 0.0, 1.0);
    len_y *= len_y;
    return vec3(dir_x, dir_y, len_x + len_y) * w;
}

// `FsrEasuTapF`: a texel's weight at `off` from the sample, in the frame rotated to the edge and scaled by `len`; the
// kernel is (25/16·(2/5·x² − 1)² − (25/16 − 1))·(lob·x² − 1)², x² clipped at `clp`.
fn easu_weight(off: vec2<f32>, dir: vec2<f32>, len: vec2<f32>, lob: f32, clp: f32) -> f32 {
    let v = vec2(off.x * dir.x + off.y * dir.y, off.x * -dir.y + off.y * dir.x) * len;
    let dist_sq = min(dot(v, v), clp);
    var wb = 2.0 / 5.0 * dist_sq - 1.0;
    var wa = lob * dist_sq - 1.0;
    wb *= wb;
    wa *= wa;
    wb = 25.0 / 16.0 * wb - (25.0 / 16.0 - 1.0);
    return wb * wa;
}

@fragment
fn easu(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    // The output pixel's centre in source texels (AMD's con0 / con1), split into the texel f and the fraction.
    let pp_full = in.position.xy * (u.size.xy / u.size.zw) - vec2(0.5);
    let fp = floor(pp_full);
    let pp = pp_full - fp;
    let i = vec2<i32>(fp);
    //    b c
    //  e f g h
    //  i j k l
    //    n o
    let b = tap(i + vec2(0, -1));
    let c = tap(i + vec2(1, -1));
    let e = tap(i + vec2(-1, 0));
    let f = tap(i);
    let g = tap(i + vec2(1, 0));
    let h = tap(i + vec2(2, 0));
    let ii = tap(i + vec2(-1, 1));
    let j = tap(i + vec2(0, 1));
    let k = tap(i + vec2(1, 1));
    let l = tap(i + vec2(2, 1));
    let n = tap(i + vec2(0, 2));
    let o = tap(i + vec2(1, 2));
    let bl = luma(b);
    let cl = luma(c);
    let el = luma(e);
    let fl = luma(f);
    let gl = luma(g);
    let hl = luma(h);
    let il = luma(ii);
    let jl = luma(j);
    let kl = luma(k);
    let ll = luma(l);
    let nl = luma(n);
    let ol = luma(o);
    // Direction and length over the 2×2 f g / j k, each corner by its bilinear weight.
    let acc = easu_set((1.0 - pp.x) * (1.0 - pp.y), bl, el, fl, gl, jl)
        + easu_set(pp.x * (1.0 - pp.y), cl, fl, gl, hl, kl)
        + easu_set((1.0 - pp.x) * pp.y, fl, il, jl, kl, nl)
        + easu_set(pp.x * pp.y, gl, jl, kl, ll, ol);
    var dir = acc.xy;
    let dir_sq = dot(dir, dir);
    let flat_dir = dir_sq < 1.0 / 32768.0;
    dir = select(dir * inverseSqrt(max(dir_sq, 1e-20)), vec2(1.0, dir.y), flat_dir);
    var len = acc.z * 0.5;
    len *= len;
    // Stretched along an edge (1 → √2 diagonally), shrunk across it; the negative lobe from 1/2 (no edge) down to
    // 1/4 − 0.04 (an edge), and the kernel clipped where the lobe ends.
    let stretch = dot(dir, dir) / max(abs(dir.x), abs(dir.y));
    let len_xy = vec2(1.0 + (stretch - 1.0) * len, 1.0 - 0.5 * len);
    let lob = 0.5 + ((1.0 / 4.0 - 0.04) - 0.5) * len;
    let clp = 1.0 / lob;
    var ac = vec3(0.0);
    var aw = 0.0;
    var w: f32;
    w = easu_weight(vec2(0.0, -1.0) - pp, dir, len_xy, lob, clp); ac += b * w; aw += w;
    w = easu_weight(vec2(1.0, -1.0) - pp, dir, len_xy, lob, clp); ac += c * w; aw += w;
    w = easu_weight(vec2(-1.0, 1.0) - pp, dir, len_xy, lob, clp); ac += ii * w; aw += w;
    w = easu_weight(vec2(0.0, 1.0) - pp, dir, len_xy, lob, clp); ac += j * w; aw += w;
    w = easu_weight(vec2(0.0, 0.0) - pp, dir, len_xy, lob, clp); ac += f * w; aw += w;
    w = easu_weight(vec2(-1.0, 0.0) - pp, dir, len_xy, lob, clp); ac += e * w; aw += w;
    w = easu_weight(vec2(1.0, 1.0) - pp, dir, len_xy, lob, clp); ac += k * w; aw += w;
    w = easu_weight(vec2(2.0, 1.0) - pp, dir, len_xy, lob, clp); ac += l * w; aw += w;
    w = easu_weight(vec2(2.0, 0.0) - pp, dir, len_xy, lob, clp); ac += h * w; aw += w;
    w = easu_weight(vec2(1.0, 0.0) - pp, dir, len_xy, lob, clp); ac += g * w; aw += w;
    w = easu_weight(vec2(1.0, 2.0) - pp, dir, len_xy, lob, clp); ac += o * w; aw += w;
    w = easu_weight(vec2(0.0, 2.0) - pp, dir, len_xy, lob, clp); ac += n * w; aw += w;
    // Dering: within the 2×2's range.
    let mn = min(min(f, g), min(j, k));
    let mx = max(max(f, g), max(j, k));
    return vec4(min(mx, max(mn, ac / max(aw, 1e-5))), 1.0);
}

// ---- RCAS ----

// An EASU texel (gamma-2), clamped to the frame.
fn mid(p: vec2<i32>) -> vec3<f32> {
    let m = vec2<i32>(u.size.zw) - vec2(1);
    return textureLoad(src, clamp(p, vec2(0), m), 0).rgb;
}

// AMD's `FSR_RCAS_LIMIT`: the strongest lobe, kept short of where the cross would clip.
const RCAS_LIMIT: f32 = 0.25 - 1.0 / 16.0;

@fragment
fn rcas(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let p = vec2<i32>(in.position.xy);
    //    b
    //  d e f
    //    h
    let b = mid(p + vec2(0, -1));
    let d = mid(p + vec2(-1, 0));
    let e = mid(p);
    let f = mid(p + vec2(1, 0));
    let h = mid(p + vec2(0, 1));
    let mn_cross = min(min(b, d), min(f, h));
    let mx_cross = max(max(b, d), max(f, h));
    // The lobe that would just reach 0 (hit_min) or 1 (hit_max) per channel; the largest of the safe ones.
    let hit_min = min(mn_cross, e) / max(4.0 * mx_cross, vec3(1.0 / 256.0));
    let hit_max = (vec3(1.0) - max(mx_cross, e)) / min(4.0 * mn_cross - vec3(4.0), vec3(-1.0 / 256.0));
    let lobe_rgb = max(-hit_min, hit_max);
    let lobe = max(-RCAS_LIMIT, min(max(lobe_rgb.r, max(lobe_rgb.g, lobe_rgb.b)), 0.0)) * u.sharp.x;
    let c = (lobe * (b + d + f + h) + e) / (4.0 * lobe + 1.0);
    let lin = c * c;
    return vec4(lin, 1.0);
}
