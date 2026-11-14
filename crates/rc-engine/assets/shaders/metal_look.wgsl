// Port-only (Lighting Enhanced): the shiny metal look of the textures crate::metal lists, for tfrag.wgsl's and
// tie.wgsl's `WORLD_METAL` variant and moby.wgsl's shine branch. Standard techniques: an environment reflection read
// from a sky probe (the level's sky averaged by height and bearing, crate::metal `probe`) along the reflected view ray,
// Schlick's Fresnel, the surface's own colour tinting what it reflects (a conductor; a painted coat reflects white),
// and a Blinn-Phong highlight from the key light.
// The baked vertex light stands in for shadowing: what the game darkened reflects and shines less.
//
// The environment rides in the shared fog buffer after the texture options (crate::game_camera `fog_bytes`, 27
// vec4, `env`): to the sun (xyz, Bevy world; w = highlight strength), the sun's colour (linear), then the sky
// probe: eight bearings each of the zenith, middle and horizon bands, and the ground (display-encoded rgb).
// Identifiers here must not end in a digit (naga_oil renames them).

#define_import_path rerac::metal_look

fn metal_to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + 0.055) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}

fn metal_to_display(c: vec3<f32>) -> vec3<f32> {
    let lo = c * 12.92;
    let hi = 1.055 * pow(max(c, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(hi, lo, c <= vec3<f32>(0.0031308));
}

// Sector colour `i` (wrapped) of band `b` of the probe `env` (display rgb).
fn metal_cell(env: array<vec4<f32>, 27>, b: i32, i: i32) -> vec3<f32> {
    // A local copy: a runtime index into a value array.
    var e = env;
    return e[2 + b * 8 + ((i % 8) + 8) % 8].rgb;
}

// Band `b` along bearing `az` (radians, atan2(z, x)): the two nearest sectors blended.
fn metal_band(env: array<vec4<f32>, 27>, b: i32, az: f32) -> vec3<f32> {
    let s = (az + 3.14159265) / 6.28318531 * 8.0 - 0.5;
    let i = i32(floor(s));
    return mix(metal_cell(env, b, i), metal_cell(env, b, i + 1), s - floor(s));
}

// The sky probe along direction `r` (Bevy world, y up), linear: three bands by height, the ground below.
fn metal_sky(r: vec3<f32>, env: array<vec4<f32>, 27>) -> vec3<f32> {
    let az = atan2(r.z, r.x);
    let low = metal_band(env, 2, az);
    var c = mix(low, metal_band(env, 1, az), smoothstep(0.0, 0.4, r.y));
    c = mix(c, metal_band(env, 0, az), smoothstep(0.4, 0.95, r.y));
    let below = mix(low, env[26].rgb, smoothstep(0.0, 0.35, -r.y));
    return metal_to_linear(select(c, below, r.y < 0.0));
}

// The geometric normal of a pixel from its world position's screen derivatives, facing `to_eye`.
fn metal_face_normal(world: vec3<f32>, to_eye: vec3<f32>) -> vec3<f32> {
    var n = normalize(cross(dpdx(world), dpdy(world)));
    if (dot(n, to_eye) < 0.0) { n = -n; }
    return n;
}

// `n` (interpolated, not zero) normalised and turned to face the eye; no derivatives, so it may sit in a branch.
fn metal_facing(n: vec3<f32>, to_eye: vec3<f32>) -> vec3<f32> {
    let s = normalize(n);
    return select(s, -s, dot(s, to_eye) < 0.0);
}

// The shader's `k` of a shine level (crate::metal `Shine`: 1 Painted, 2 Bare, 3 Chrome).
fn metal_strength(level: u32) -> f32 { return select(select(0.9, 0.6, level == 2u), 0.35, level == 1u); }

// `n` (interpolated, maybe zero) facing the eye, else the face normal.
fn metal_normal(n: vec3<f32>, world: vec3<f32>, to_eye: vec3<f32>) -> vec3<f32> {
    let face = metal_face_normal(world, to_eye);
    if (dot(n, n) < 0.25) { return face; }
    var s = normalize(n);
    if (dot(s, face) < 0.0) { s = -s; }
    return s;
}

// `rgb`: the pixel as the game shades it (display-encoded, texture × vertex colour, before fog); `lit`: the vertex
// colour alone (the baked light); `k`: the texture's shine (crate::metal `Shine`, 0..1). Returns display-encoded rgb.
fn metal_shade(
    rgb: vec3<f32>, lit: vec3<f32>, n: vec3<f32>, to_eye: vec3<f32>, k: f32,
    env: array<vec4<f32>, 27>,
) -> vec3<f32> {
    let base = metal_to_linear(rgb);
    // A conductor's reflectance at normal incidence follows its colour (F0, here √albedo: the art's dark greys are
    // still steel): coloured metal reflects in its colour. Painted metal (the lowest shine) is a dielectric coat: F0
    // 0.04, white, its colour kept whole. Schlick's term takes either toward white at grazing angles.
    let paint = 1.0 - smoothstep(0.4, 0.55, k);
    let f0 = mix(clamp(sqrt(base) * 1.1, vec3<f32>(0.0), vec3<f32>(1.0)), vec3<f32>(0.04), paint);
    let tint = mix(f0, vec3<f32>(1.0), paint);
    // The baked light's brightness (0x80 = 1): shadowed parts reflect and shine less.
    let light = clamp(dot(lit, vec3<f32>(0.299, 0.587, 0.114)), 0.0, 1.5);
    let shade = smoothstep(0.08, 0.7, light);
    let nv = clamp(dot(n, to_eye), 0.0, 1.0);
    let fresnel = f0 + (vec3<f32>(1.0) - f0) * (mix(0.5, 1.0, paint) * pow(1.0 - nv, 5.0));
    let r = reflect(-to_eye, n);
    let sky = metal_sky(r, env);
    // The game's colour stays mostly (it holds the art's own shading); the sky, an emitter brighter than its display
    // bytes, is added in the surface's reflectance.
    var c = base * (1.0 - 0.2 * k * (1.0 - paint)) + sky * fresnel * (1.6 * mix(k, 1.0, paint) * shade);
    // Blinn-Phong from the key light (normalised: (p + 8) / 8π), sharper on the shinier textures.
    let to_sun = env[0];
    let h = normalize(to_sun.xyz + to_eye);
    let p = mix(24.0, 90.0, k);
    let spec = pow(clamp(dot(n, h), 0.0, 1.0), p) * (p + 8.0) / 25.13 * step(0.0, dot(n, to_sun.xyz));
    c += env[1].rgb * tint * (spec * to_sun.w * k * shade * 0.4);
    return metal_to_display(c);
}
