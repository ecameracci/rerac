// 3D clouds (crate::cloud_render): a cumulus slab above the camera, ray-marched at half resolution over the sky's
// pixels, then composited over the sky as `sky · (1 − α + ambient) + tint · sunlit` (a multiply draw, then an add).
// march: R the sunlit light, G the ambient share, A the opacity (premultiplied, RGBA16F).

#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput
#import rerac::fx_view::{view, fx_noise}

struct CloudParams {
    // Toward the sun (world), w: noise texels per world unit.
    sun: vec4<f32>,
    // The sun's tint (linear), w: coverage.
    tint: vec4<f32>,
    // Base, thickness (above the camera), the far distance, sigma.
    shape: vec4<f32>,
    // The noise offset (world x, z), steps, the ambient floor.
    march: vec4<f32>,
    // Light steps, the light step's share of the thickness.
    light: vec4<f32>,
}

#ifdef MULTISAMPLED
@group(0) @binding(1) var depth: texture_depth_multisampled_2d;
#else
@group(0) @binding(1) var depth: texture_depth_2d;
#endif
// march: the tiling noise (repeat); composite: the cloud target (clamp).
@group(0) @binding(2) var tex: texture_2d<f32>;
@group(0) @binding(3) var samp: sampler;
@group(0) @binding(4) var<uniform> cp: CloudParams;

const NOISE_TEXELS: f32 = 128.0;

fn is_sky(p: vec2<i32>) -> bool {
    let size = vec2<i32>(textureDimensions(depth));
    return textureLoad(depth, clamp(p, vec2<i32>(0), size - 1), 0) <= 0.0;
}

// The world-space view ray of a target pixel (camera-relative, unit).
fn view_dir(coord: vec2<f32>) -> vec3<f32> {
    let uv = (coord - view.viewport.xy) / view.viewport.zw;
    let ndc = vec2<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0);
    let v = view.view_from_clip * vec4<f32>(ndc, 1.0, 1.0);
    let dir_view = normalize(v.xyz / v.w);
    return normalize((view.world_from_view * vec4<f32>(dir_view, 0.0)).xyz);
}

// Density at a camera-relative point (0..1).
fn density(p: vec3<f32>) -> f32 {
    let h = (p.y - cp.shape.x) / cp.shape.y;
    if (h <= 0.0 || h >= 1.0) { return 0.0; }
    let xz = (p.xz + cp.march.xy) * (cp.sun.w / NOISE_TEXELS);
    let n = textureSampleLevel(tex, samp, xz, 0.0);
    let cov = cp.tint.w;
    // Clear sky: no cloud cell here (the detail fetches only inside the coverage).
    if (n.r <= 1.0 - cov) { return 0.0; }
    // Billows lean with height, so the tops are not the bases' copy.
    let b = textureSampleLevel(tex, samp, xz * 3.1 + vec2<f32>(h * 0.21, h * 0.13), 0.0);
    let f = textureSampleLevel(tex, samp, xz * 9.3 - vec2<f32>(h * 0.37, h * 0.11), 0.0);
    let base = saturate((n.r - (1.0 - cov)) / max(cov, 0.05));
    // The coverage gathered into cells (the billows), each cell domed.
    let shape = base * mix(0.35, 1.0, n.g * n.g) - 0.22 * (1.0 - b.a);
    // Flat bases, rounded tops: a cell is as tall as its shape.
    let top = sqrt(saturate(shape)) * 0.95;
    var d = saturate((top - h) * 6.0) * smoothstep(0.0, 0.06, h);
    // Fine erosion, strongest at the edges and the base.
    d -= (1.0 - f.a) * 0.45 * (1.0 - d) + f.b * 0.12;
    return saturate(d * 1.6);
}

fn hg(c: f32, g: f32) -> f32 {
    let g2 = g * g;
    return (1.0 - g2) / pow(max(1.0 + g2 - 2.0 * g * c, 1e-4), 1.5);
}

@fragment
fn march(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    // The four full-resolution pixels this one covers: skip when none is sky.
    let full = vec2<i32>(floor(in.position.xy)) * 2;
    if (!(is_sky(full) || is_sky(full + vec2<i32>(1, 0)) || is_sky(full + vec2<i32>(0, 1)) || is_sky(full + vec2<i32>(1, 1)))) {
        return vec4<f32>(0.0);
    }
    let coord = in.position.xy * 2.0;
    let dir = view_dir(coord);
    if (dir.y < 0.015) { return vec4<f32>(0.0); }
    let base = cp.shape.x;
    let thick = cp.shape.y;
    let far = cp.shape.z;
    let sigma = cp.shape.w;
    let t0 = base / dir.y;
    if (t0 >= far) { return vec4<f32>(0.0); }
    let t1 = min((base + thick) / dir.y, far);
    let steps = i32(cp.march.z);
    let dt = (t1 - t0) / f32(steps);
    var t = t0 + dt * fx_noise(in.position.xy);
    let sun = normalize(cp.sun.xyz);
    let cos_sun = dot(dir, sun);
    // Mostly forward (silver linings) with an isotropic floor; 1 when isotropic.
    let phase = 0.65 + 0.35 * min(hg(cos_sun, 0.55), 6.0);
    let lsteps = i32(cp.light.x);
    let lstep = cp.light.y * thick;
    var trans = 1.0;
    var lit = 0.0;
    var amb = 0.0;
    for (var i = 0; i < steps; i += 1) {
        let p = dir * t;
        let d = density(p);
        if (d > 0.002) {
            var od = 0.0;
            for (var j = 1; j <= lsteps; j += 1) {
                od += max(density(p + sun * (lstep * f32(j))), 0.0);
            }
            let to_sun = exp(-sigma * od * lstep);
            // Beer-powder: dark edges facing away from the sun.
            let powder = 1.0 - exp(-2.0 * sigma * d * 40.0);
            let ext = exp(-sigma * d * dt);
            let h = saturate((p.y - base) / thick);
            let share = trans * (1.0 - ext);
            lit += share * to_sun * phase * mix(0.6, 1.0, powder);
            amb += share * mix(cp.march.w, 1.0, h);
            trans *= ext;
            if (trans < 0.02) { break; }
        }
        t += dt;
    }
    // Thin out toward the horizon and with distance, where the sky shells paint their own far clouds.
    let fade = (1.0 - smoothstep(far * 0.4, far, t0)) * smoothstep(0.05, 0.18, dir.y);
    let alpha = (1.0 - trans) * fade;
    return vec4<f32>(lit * fade, amb * fade, 0.0, alpha);
}

fn cloud_at(in: FullscreenVertexOutput) -> vec4<f32> {
    if (!is_sky(vec2<i32>(floor(in.position.xy)))) { return vec4<f32>(0.0); }
    // The half-resolution target covers the whole target (texel k = full pixels 2k, 2k + 1).
    let half = vec2<f32>(textureDimensions(tex));
    return textureSampleLevel(tex, samp, in.position.xy * 0.5 / half, 0.0);
}

// Multiply: 1 − α + ambient (the sky's own colour through the clouds and in their shade).
@fragment
fn shade(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let c = cloud_at(in);
    return vec4<f32>(vec3<f32>(saturate(1.0 - c.a + c.g)), 1.0);
}

// Add: the sun's light on the lit parts.
@fragment
fn light(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let c = cloud_at(in);
    return vec4<f32>(cp.tint.rgb * c.r, 0.0);
}
