// Water Enhanced's other liquids (crate::liquid_render: the kinds and every rule): the look of lava (1), mud (2) and
// sludge (3) over a draw's own colour, shared by liquid.wgsl (the seas' surfaces) and water.wgsl (the ripple
// patches). Linear light, before the fog.

#define_import_path rerac::liquid_look

// The second noise layer's drift without a second pass (crate::liquid_render::DRIFT): ST per second, offset.
const DRIFT: vec4<f32> = vec4<f32>(0.011, -0.017, 0.5, 0.0);

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + 0.055) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}

fn hash2(p: vec2<f32>) -> vec2<f32> {
    let q = vec2<f32>(dot(p, vec2<f32>(127.1, 311.7)), dot(p, vec2<f32>(269.5, 183.3)));
    return fract(sin(q) * 43758.5453) * 2.0 - 1.0;
}

// Gradient noise, about −0.7..0.7.
fn gnoise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = dot(hash2(i), f);
    let b = dot(hash2(i + vec2<f32>(1.0, 0.0)), f - vec2<f32>(1.0, 0.0));
    let c = dot(hash2(i + vec2<f32>(0.0, 1.0)), f - vec2<f32>(0.0, 1.0));
    let d = dot(hash2(i + vec2<f32>(1.0, 1.0)), f - vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

fn fbm(p: vec2<f32>) -> f32 {
    return gnoise(p) + 0.5 * gnoise(p * 2.03 + 7.1) + 0.25 * gnoise(p * 4.07 - 3.3);
}

// A gradient per lattice point from fract arithmetic only, for world coordinates (a sine hash loses its precision at
// the levels' hundreds of units and shows as blocks).
fn whash2(p: vec2<f32>) -> vec2<f32> {
    var q = fract(vec3<f32>(p.xyx) * vec3<f32>(0.1031, 0.1030, 0.0973));
    q += dot(q, q.yzx + 33.33);
    return fract((q.xx + q.yz) * q.zy) * 2.0 - 1.0;
}

// `gnoise` on `whash2`.
fn wnoise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = dot(whash2(i), f);
    let b = dot(whash2(i + vec2<f32>(1.0, 0.0)), f - vec2<f32>(1.0, 0.0));
    let c = dot(whash2(i + vec2<f32>(0.0, 1.0)), f - vec2<f32>(0.0, 1.0));
    let d = dot(whash2(i + vec2<f32>(1.0, 1.0)), f - vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

// What lava's fine detail needs of the surface (`liquid_frame`): the world normal, the world velocity of the draw's
// scroll, a pixel's footprint in world units and in ST.
struct LiquidFrame {
    n: vec3<f32>,
    vel: vec3<f32>,
    foot: f32,
    st_foot: f32,
}

// A flat, still frame without footprints (water.wgsl's patches: never lava).
fn still_frame() -> LiquidFrame {
    return LiquidFrame(vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(0.0), 0.0, 0.0);
}

// The frame from the screen derivatives of the world position and the ST (call in uniform control flow); `scroll` the
// draw's ST per second (crate::liquid_render::layer_flow `w`): a feature at a fixed ST moves by −(∂world/∂ST)·scroll.
fn liquid_frame(world: vec3<f32>, uv: vec2<f32>, scroll: vec2<f32>) -> LiquidFrame {
    let wx = dpdx(world);
    let wy = dpdy(world);
    let ux = dpdx(uv);
    let uy = dpdy(uv);
    let c = cross(wx, wy);
    let n = select(vec3<f32>(0.0, 1.0, 0.0), c / max(length(c), 1e-12), length(c) > 1e-12);
    let det = ux.x * uy.y - uy.x * ux.y;
    var vel = vec3<f32>(0.0);
    if (abs(det) > 1e-12) {
        let tu = (wx * uy.y - wy * ux.y) / det;
        let tv = (wy * ux.x - wx * uy.x) / det;
        vel = -(tu * scroll.x + tv * scroll.y);
        vel *= min(1.0, LAVA_FINE.w / max(length(vel), 1e-6));
    }
    return LiquidFrame(n, vel, max(length(wx), length(wy)), max(length(ux), length(uy)));
}

// Lava's fine detail (crate::liquid_render::LAVA_FINE): x = cells per world unit of its cracks, y = the period (s) of
// its two flow phases, z = the grain's frequency over the cracks', w = the fastest flow followed (world units / s).
const LAVA_FINE: vec4<f32> = vec4<f32>(0.85, 3.0, 3.7, 6.0);

// One sample of the fine detail on a plane `p` (world units): x = the thin cracks' ridge (1 on a crack), y = the
// crust's grain (about −1..1).
fn lava_fine_at(p: vec2<f32>) -> vec2<f32> {
    let w = vec2<f32>(wnoise(p * 0.23), wnoise(p * 0.23 + vec2<f32>(5.2, 1.3)));
    let q = p * LAVA_FINE.x + w * 1.3;
    let r = 1.0 - abs(wnoise(q) * 1.5 + wnoise(q * 2.13 + vec2<f32>(3.7, -1.9)) * 0.55);
    let g = q * LAVA_FINE.z;
    return vec2<f32>(r, wnoise(g) * 1.2 + wnoise(g * 2.3 + vec2<f32>(1.3, 4.1)) * 0.6);
}

// `lava_fine_at` on the planes the surface faces (weights by its normal), at the world point `p`.
fn lava_fine_planes(p: vec3<f32>, n: vec3<f32>) -> vec2<f32> {
    var a = abs(n);
    a = a * a * a * a;
    a = a / max(a.x + a.y + a.z, 1e-6);
    var s = vec2<f32>(0.0);
    if (a.y > 0.02) { s += lava_fine_at(p.xz) * a.y; }
    if (a.z > 0.02) { s += lava_fine_at(p.xy + vec2<f32>(17.0, 0.0)) * a.z; }
    if (a.x > 0.02) { s += lava_fine_at(p.zy + vec2<f32>(0.0, 23.0)) * a.x; }
    return s / max(select(0.0, a.y, a.y > 0.02) + select(0.0, a.z, a.z > 0.02) + select(0.0, a.x, a.x > 0.02), 1e-6);
}

// Lava's fine detail at `world` (x the thin cracks' ridge, y the grain, z its weight: 0 once a crack cell is under about
// two pixels). A moving surface carries it with its scroll in two phases of LAVA_FINE.y seconds, blended, so it never
// stretches; a still one keeps it in place.
fn lava_fine(world: vec3<f32>, f: LiquidFrame, t: f32) -> vec3<f32> {
    let keep = 1.0 - smoothstep(0.25, 0.6, f.foot * LAVA_FINE.x);
    if (keep <= 0.0) { return vec3<f32>(0.0); }
    if (dot(f.vel, f.vel) < 1e-4) { return vec3<f32>(lava_fine_planes(world, f.n), keep); }
    let period = LAVA_FINE.y;
    let ph = fract(vec2<f32>(t / period, t / period + 0.5));
    let cycle = floor(vec2<f32>(t / period, t / period + 0.5));
    let a = lava_fine_planes(world - f.vel * (ph.x - 0.5) * period + vec3<f32>(cycle.x * 7.31, 0.0, cycle.x * 3.17), f.n);
    let b = lava_fine_planes(world - f.vel * (ph.y - 0.5) * period + vec3<f32>(cycle.y * 5.13, 0.0, cycle.y * 8.71) + 31.0, f.n);
    let wa = 1.0 - abs(ph.x * 2.0 - 1.0);
    return vec3<f32>(a * wa + b * (1.0 - wa), keep);
}

// Mud: slow bulges over world xz (height in world units) that rise and settle.
fn mud_height(xz: vec2<f32>, t: f32) -> f32 {
    let a = gnoise(xz * 0.11 + vec2<f32>(t * 0.013, -t * 0.009));
    let b = gnoise(xz * 0.29 + vec2<f32>(-t * 0.021, t * 0.017) + 4.7);
    let breathe = 0.7 + 0.3 * sin(t * 0.35 + gnoise(xz * 0.05) * 6.0);
    return (a * 1.4 + b * 0.5) * breathe;
}

// Sludge: slow ripples and bubbles in cells of 2.5 units, about one cell in three (a bubble swells, pops into a ring
// and rests).
fn sludge_height(xz: vec2<f32>, t: f32) -> f32 {
    var h = gnoise(xz * 0.2 + vec2<f32>(t * 0.03, t * 0.02)) * 0.12;
    let cell = 2.5;
    let c = floor(xz / cell);
    for (var j = -1; j <= 1; j += 1) {
        for (var i = -1; i <= 1; i += 1) {
            let id = c + vec2<f32>(f32(i), f32(j));
            let r = hash2(id);
            let centre = (id + 0.5 + r * 0.35) * cell;
            let period = 5.0 + r.x * 2.5;
            let age = fract(t / period + r.y * 0.5 + 0.5);
            let size = 0.15 + 0.3 * fract(r.x * 7.3 + 0.5);
            let rad = size * smoothstep(0.0, 0.5, age);
            let d = length(xz - centre);
            let bump = max(1.0 - d * d / max(rad * rad, 1e-4), 0.0);
            let live = 1.0 - step(0.5, age);
            let ring_r = (age - 0.5) * 2.5;
            let ring = exp(-pow((d - ring_r) * 6.0, 2.0)) * (1.0 - age) * step(0.5, age);
            let some = step(0.33, fract(r.y * 13.7 + 0.5));
            h += (bump * size * 0.3 * live + ring * 0.02) * some;
        }
    }
    return h;
}

// The look of `kind` in linear light: `rgb` the draw's MODULATE colour (display bytes / 255), `soft` the same at low
// contrast (mud; else `rgb`), `uv` its ST, `world` the Bevy position, `slope` an extra surface slope over world xz (the
// ripple module's), `flow` the second lava layer (crate::liquid_render::layer_flow), `sun` toward the key light, `sky`
// the reflected sky, `to_eye` toward the camera, `t` the frame time, `frame` the surface's (`liquid_frame`; lava's fine
// detail).
fn liquid_look(kind: u32, rgb: vec3<f32>, soft: vec3<f32>, uv: vec2<f32>, world: vec3<f32>, slope: vec2<f32>, flow: vec4<f32>, sun: vec3<f32>, sky: vec3<f32>, to_eye: vec3<f32>, t: f32, frame: LiquidFrame) -> vec3<f32> {
    let base = srgb_to_linear(rgb);
    let sun_up = smoothstep(0.0, 0.15, sun.y);
    if (kind == 1u) {
        // Lava: two noise layers in ST (the first rides the draw's scroll, the second drifts against it); the zero
        // lines of their sum are the cracks. Over them, world-space detail (`lava_fine`): their edges roughened, thin
        // cracks through the crust and a grain, each faded out before it would alias.
        let uv1 = uv * 1.6;
        let uv2 = uv * 2.9 + flow.xy * t * 2.9 + flow.z;
        let warp = vec2<f32>(gnoise(uv1 * 0.7 + vec2<f32>(t * 0.05, 0.0)), gnoise(uv1 * 0.7 + vec2<f32>(3.1, -t * 0.04)));
        let n1 = fbm(uv1 + warp * 0.5);
        let n2 = fbm(uv2 - warp * 0.35);
        let fine = lava_fine(world, frame, t);
        let ridge = 1.0 - abs(n1 * 0.75 + n2 * 0.5) + fine.y * 0.03 * fine.z;
        // The edges a pixel wide (the ridge's change over a pixel: about four per unit of ST, the ST's footprint).
        let aa = clamp(frame.st_foot * 1.6 * 4.0, 0.012, 0.08);
        let crack = smoothstep(0.88 - aa, 0.88 + aa, ridge);
        let core = smoothstep(0.97 - aa, 0.97 + aa * 0.5, ridge);
        let fa = clamp(frame.foot * LAVA_FINE.x * 2.0, 0.02, 0.25);
        let thin = smoothstep(0.92 - fa, 0.92 + fa * 0.5, fine.x) * fine.z * smoothstep(0.35, 0.8, ridge);
        // The texture's bright parts run hotter.
        let lum = dot(rgb, vec3<f32>(0.5, 0.4, 0.1));
        let heat = clamp(lum * 1.7, 0.45, 1.1);
        let pulse = 0.85 + 0.15 * sin(t * 1.3 + n1 * 5.0);
        let grain = (0.8 + 0.4 * gnoise(uv * 11.0)) * (1.0 + fine.y * 0.3 * fine.z);
        let crust = base * mix(0.1, 0.35, smoothstep(0.4, 0.85, ridge)) * max(grain, 0.2);
        // The cracks: the draw's colour pushed toward orange and yellow (linear), never white.
        let hot = clamp(base * 2.4 + vec3<f32>(0.3, 0.07, 0.0), vec3<f32>(0.0), vec3<f32>(1.0, 0.36, 0.06)) * heat;
        var col = mix(crust, hot * pulse, max(crack, thin * 0.55));
        col = mix(col, vec3<f32>(1.0, 0.55, 0.12), core * 0.6 * heat);
        // The glow around the cracks.
        return col + base * 0.5 * smoothstep(0.6, 0.88, ridge) * pulse;
    }
    // Mud and sludge: a height field over world xz, its normal, the key light, a sharp highlight and a sky Fresnel.
    let xz = world.xz;
    let e = 0.06;
    var h0 = 0.0;
    var hx = 0.0;
    var hz = 0.0;
    if (kind == 2u) {
        h0 = mud_height(xz, t);
        hx = mud_height(xz + vec2<f32>(e, 0.0), t);
        hz = mud_height(xz + vec2<f32>(0.0, e), t);
    } else {
        h0 = sludge_height(xz, t);
        hx = sludge_height(xz + vec2<f32>(e, 0.0), t);
        hz = sludge_height(xz + vec2<f32>(0.0, e), t);
    }
    let g = vec2<f32>((hx - h0) / e, (hz - h0) / e) + slope;
    let n = normalize(vec3<f32>(-g.x, 1.0, -g.y));
    let diff = max(dot(n, sun), 0.0);
    let half_v = normalize(sun + to_eye);
    let cos_v = max(dot(n, to_eye), 0.0);
    let fres = 0.04 + 0.96 * pow(1.0 - cos_v, 5.0);
    if (kind == 2u) {
        // Thick and dark: the low-contrast colour, shaded by the bulges, glossy.
        let body = srgb_to_linear(soft) * (0.3 + 0.2 * diff * sun_up + 0.06 * h0);
        let spec = pow(max(dot(n, half_v), 0.0), 220.0) * 2.0 * sun_up;
        let sheen = pow(max(dot(n, half_v), 0.0), 18.0) * 0.08 * sun_up;
        return mix(body, sky * 0.35, clamp(fres * 0.6, 0.0, 0.4)) + vec3<f32>(1.0, 0.95, 0.85) * (spec + sheen);
    }
    // Sludge: murky (about its Original brightness), a weak glow of its own colour, a soft highlight.
    let murk = base * (0.6 + 0.3 * diff * sun_up);
    let glow = base * (0.25 + 0.1 * sin(t * 0.8 + h0 * 4.0));
    let spec = pow(max(dot(n, half_v), 0.0), 24.0) * 0.06 * sun_up;
    return mix(murk + glow, sky * 0.3, clamp(fres * 0.5, 0.0, 0.3)) + vec3<f32>(0.9, 1.0, 0.85) * spec;
}
