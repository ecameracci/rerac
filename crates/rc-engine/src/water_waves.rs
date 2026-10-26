//! **Port-only (Water Enhanced, crate::graphics; docs/plan/graphics_options.md "Water"): the sea swell.** A sum of
//! travelling sine waves ([`WAVES`]) over Bevy world xz and the frame time, drawn only: the game's water height
//! (collision, swimming, floats, the Rilgar fish) is untouched, so the swell stays under [`MAX_HEIGHT`].
//!
//! * The grid seas (crate::sea_water: Rilgar 879, Batalia 327) are displaced in `water.wgsl`'s vertex shader. Their
//!   cells are 30 units (the block strips of `rc_formats::sea::LiquidGrid`), so [`tessellate`] splits the triangles
//!   near the camera (within [`SWELL_FADE`]'s end plus [`TESS_MARGIN`]) into a fixed number of parts per edge
//!   ([`SPACING`]; one count per draw, so neighbours share their edge points); the height is 0 beyond the fade
//!   ([`far_fade`]), where the untouched far triangles meet them. Every vertex (the far ones too) carries its shore
//!   damping ([`ShoreMap`]) in its tag; the shader scales the height by it, the far fade and the dev scale
//!   ([`scale`]), and the fragment shader adds the analytic slope ([`grad`]) times the damping and the scale to the
//!   wave normal, so the reflection, the Fresnel term, the refraction and the glint follow the displaced surface. The
//!   slope carries on past the height's fade, each wave dropping out from [`WAVE_LOD`] to twice that many wavelengths
//!   ([`wave_weight`]); crests are lightened, troughs darkened by [`CREST`].
//! * The strip and ripple water (crate::water_render: Novalis's rivers and pools, the ripple patches) keep their
//!   geometry (7–16-unit strip edges; the patches' own heights) and take the slope only, damped per pixel by the
//!   vertical depth of the bed behind the surface (the scene copy).
//! * The ocean 1111 (Pokitaru) gets a surface of its own (crate::ocean_water: world tiles split once, displaced in the
//!   vertex shader like the grid seas).
//!
//! **Shore damping** ([`ShoreMap`]): the level collision probed down vertical lines ([`PROBE`]) at the points of a
//! [`LATTICE`]-unit lattice, cached; a point's depth is the water's z minus the first surface met (land, a pier or a
//! bridge above the water: depth ≤ 0; else the bed from [`SKIP`] under the surface, past the swim surface; nothing:
//! deep). Each lattice node takes the shallowest depth within [`SHORE_RADIUS`] nodes through [`shore_damp`], and a
//! point the bilinear blend of its four nodes: the swell dies within about ten units of land and over the shelves.
//!
//! [`height`] and [`grad`] are the shader's swell on the CPU, for its tests.
//!
//! `RC_WATER_WAVES=0` turns the swell off, `RC_WATER_WAVES_SCALE=<k>` scales its amplitude (docs/workflows/dev-switches.md).

use std::collections::HashMap;

/// Per wave: Bevy xz direction (unit), wavenumber 2π/λ (λ = 22, 14, 9, 6 units), amplitude (slopes about 0.03–0.04
/// each). `water.wgsl`'s `swell_wave` holds the same values.
pub const WAVES: [[f32; 4]; 4] = [
    [0.8, 0.6, 0.2856, 0.11],
    [0.98058, -0.19612, 0.4488, 0.08],
    [0.45099, 0.89254, 0.69813, 0.06],
    [0.92918, 0.36961, 1.05, 0.04],
];
/// Phase offsets (radians), so the crests do not meet at the origin.
#[cfg_attr(not(test), allow(dead_code))]
pub const PHASE: [f32; 4] = [0.0, 1.7, 3.4, 5.1];
/// Angular speed `ω = SPEED·√(G·k)` (deep water, slowed: the λ = 22 swell runs 3.5 units/s).
#[cfg_attr(not(test), allow(dead_code))]
pub const SPEED: f32 = 0.6;
#[cfg_attr(not(test), allow(dead_code))]
pub const G: f32 = 9.8;
/// The swell's peak (the amplitudes' sum).
pub const MAX_HEIGHT: f32 = 0.3;
const _: () = assert!(WAVES[0][3] + WAVES[1][3] + WAVES[2][3] + WAVES[3][3] <= MAX_HEIGHT);
/// The displacement's far fade: whole to the first horizontal distance from the camera, none from the second (game
/// units; the split reaches no farther). The slope carries on, each wave to [`WAVE_LOD`]·2 of its wavelengths.
pub const SWELL_FADE: [f32; 2] = [50.0, 100.0];
#[cfg_attr(not(test), allow(dead_code))]
pub const WAVE_LOD: f32 = 8.0;
/// Beyond the fade's end, the camera's travel between two ticks (the meshes are built per tick, drawn per frame).
pub const TESS_MARGIN: f32 = 8.0;
/// The target edge length of the split triangles, and the most parts per edge.
pub const SPACING: f32 = 2.0;
pub const MAX_SPLIT: u32 = 24;
/// The shore lattice (game units), the nodes a node's depth is the shallowest of (radius), the depth ramp.
pub const LATTICE: f32 = 5.0;
pub const SHORE_RADIUS: i32 = 2;
pub const SHORE_DEPTH: [f32; 2] = [0.5, 8.0];
/// The probe lines: from this far above the water down to it (land, piers), and from [`SKIP`] below it to this far
/// below (the bed).
pub const PROBE: [f32; 2] = [30.0, 20.0];
/// The bed's probe starts this far under the water: past the swim surface the collision holds just under it (Rilgar's
/// whole sea: 59.4375 under the drawn 59.75).
pub const SKIP: f32 = 1.0;
/// The water's colour under the reflection, lightened on the crests and darkened in the troughs: `1 + CREST·h/MAX_HEIGHT`
/// (h the drawn height; `water.wgsl`).
#[cfg_attr(not(test), allow(dead_code))]
pub const CREST: f32 = 0.35;

/// The amplitude scale: 0 with `RC_WATER_WAVES=0`, else `RC_WATER_WAVES_SCALE` (default 1).
pub fn scale() -> f32 {
    static S: std::sync::OnceLock<f32> = std::sync::OnceLock::new();
    *S.get_or_init(|| {
        if std::env::var("RC_WATER_WAVES").is_ok_and(|v| v.trim() == "0") { return 0.0; }
        std::env::var("RC_WATER_WAVES_SCALE").ok().and_then(|v| v.trim().parse::<f32>().ok()).filter(|k| k.is_finite() && *k >= 0.0).unwrap_or(1.0)
    })
}

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg_attr(not(test), allow(dead_code))]
fn phase(w: &[f32; 4], ph: f32, xz: [f32; 2], t: f32) -> f32 { (w[0] * xz[0] + w[1] * xz[1]) * w[2] - SPEED * (G * w[2]).sqrt() * t + ph }

/// A wave's share at a horizontal distance `d` from the camera: whole to [`WAVE_LOD`] wavelengths, none from twice
/// that (the short waves would alias in the distance; the long swell carries on).
#[cfg_attr(not(test), allow(dead_code))]
pub fn wave_weight(w: &[f32; 4], d: f32) -> f32 {
    let l = std::f32::consts::TAU / w[2];
    1.0 - smoothstep(WAVE_LOD * l, 2.0 * WAVE_LOD * l, d)
}

/// The swell's height at Bevy world `xz`, time `t` (s), seen from `d` away ([`wave_weight`]).
#[cfg_attr(not(test), allow(dead_code))]
pub fn height(xz: [f32; 2], t: f32, d: f32) -> f32 { WAVES.iter().zip(PHASE).map(|(w, ph)| wave_weight(w, d) * w[3] * phase(w, ph, xz, t).sin()).sum() }

/// [`height`]'s gradient (d/dx, d/dz) at a fixed `d`; the surface normal is `(−gx, 1, −gz)` normalised.
#[cfg_attr(not(test), allow(dead_code))]
pub fn grad(xz: [f32; 2], t: f32, d: f32) -> [f32; 2] {
    WAVES.iter().zip(PHASE).fold([0.0; 2], |g, (w, ph)| {
        let c = wave_weight(w, d) * w[3] * w[2] * phase(w, ph, xz, t).cos();
        [g[0] + w[0] * c, g[1] + w[1] * c]
    })
}

/// The swell's share at a depth below the water (≤ 0: land).
pub fn shore_damp(depth: f32) -> f32 { smoothstep(SHORE_DEPTH[0], SHORE_DEPTH[1], depth) }

/// The swell's share at a horizontal distance from the camera.
#[cfg_attr(not(test), allow(dead_code))]
pub fn far_fade(dist: f32) -> f32 { 1.0 - smoothstep(SWELL_FADE[0], SWELL_FADE[1], dist) }

/// The shore damping of one water surface (module doc): the probed bed heights and the node values, by lattice node.
#[derive(Default)]
pub struct ShoreMap {
    bed: HashMap<(i32, i32), f32>,
    node: HashMap<(i32, i32), f32>,
    /// Probes made, of which met a surface above the water / under it.
    pub stats: [u32; 3],
}

impl ShoreMap {
    fn bed(&mut self, k: (i32, i32), z: f32, probe: &mut dyn FnMut([f32; 3], [f32; 3]) -> Option<f32>) -> f32 {
        if let Some(&b) = self.bed.get(&k) { return b; }
        let (x, y) = (k.0 as f32 * LATTICE, k.1 as f32 * LATTICE);
        self.stats[0] += 1;
        let b = if let Some(h) = probe([x, y, z + PROBE[0]], [x, y, z]) {
            self.stats[1] += 1;
            h
        } else if let Some(h) = probe([x, y, z - SKIP], [x, y, z - PROBE[1]]) {
            self.stats[2] += 1;
            h
        } else {
            z - PROBE[1]
        };
        self.bed.insert(k, b);
        b
    }

    fn node(&mut self, k: (i32, i32), z: f32, probe: &mut dyn FnMut([f32; 3], [f32; 3]) -> Option<f32>) -> f32 {
        if let Some(&d) = self.node.get(&k) { return d; }
        let r = SHORE_RADIUS;
        let mut depth = f32::MAX;
        for dj in -r..=r {
            for di in -r..=r {
                if di * di + dj * dj > r * r { continue; }
                depth = depth.min(z - self.bed((k.0 + di, k.1 + dj), z, probe));
            }
        }
        let d = shore_damp(depth);
        self.node.insert(k, d);
        d
    }

    /// The damping at game `(x, y)` of water at height `z`; `probe(top, bottom)` gives the z of the first surface on
    /// the line, if any.
    pub fn damp(&mut self, x: f32, y: f32, z: f32, probe: &mut dyn FnMut([f32; 3], [f32; 3]) -> Option<f32>) -> f32 {
        let (fx, fy) = (x / LATTICE, y / LATTICE);
        let (i, j) = (fx.floor(), fy.floor());
        let (tx, ty) = (fx - i, fy - j);
        let (i, j) = (i as i32, j as i32);
        let a = self.node((i, j), z, probe);
        let b = self.node((i + 1, j), z, probe);
        let c = self.node((i, j + 1), z, probe);
        let d = self.node((i + 1, j + 1), z, probe);
        (a * (1.0 - tx) + b * tx) * (1.0 - ty) + (c * (1.0 - tx) + d * tx) * ty
    }
}

/// [`ShoreMap::damp`]'s probe on the level collision: the z of the first surface (either side) on the line, clipped to
/// the collision's world (`rc_game::collision_query`: 0 ≤ x, y, z < 1024; Batalia's sea lies 15 above its floor;
/// nothing outside it in x, y).
pub fn probe_line(coll: Option<&rc_formats::collision::Collision>, a: [f32; 3], b: [f32; 3]) -> Option<f32> {
    if !(0.0..1024.0).contains(&a[0]) || !(0.0..1024.0).contains(&a[1]) { return None; }
    let clip = |p: [f32; 3]| [p[0], p[1], p[2].clamp(0.0, 1023.0)];
    rc_game::collision_query::coll_line(coll?, clip(a), clip(b), rc_game::collision_query::QueryFlags::TWO_SIDED).map(|h| h.point[2])
}

/// Triangles split near the camera ([`tessellate`]): game-space points, (s, t), RGBA / 128, the triangle list, and
/// per point whether it belongs to a split triangle.
#[derive(Default, Debug)]
pub struct Tess {
    pub pos: Vec<[f32; 3]>,
    pub uv: Vec<[f32; 2]>,
    pub color: Vec<[f32; 4]>,
    pub idx: Vec<u32>,
    pub near: Vec<bool>,
}

/// Splits every triangle (game-space `pos`) that comes within `reach` of the camera's game `(x, y)` into `n²` parts,
/// `n` = its longest edge over [`SPACING`] (the largest over the draw, at most [`MAX_SPLIT`]); the others and the
/// degenerate strip joins near the camera are kept / dropped as they are.
pub fn tessellate(pos: &[[f32; 3]], uv: &[[f32; 2]], color: &[[f32; 4]], idx: &[u32], cam: [f32; 2], reach: f32) -> Tess {
    let d2 = |a: [f32; 3], b: [f32; 3]| (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2);
    let tri = |t: &[u32]| [pos[t[0] as usize], pos[t[1] as usize], pos[t[2] as usize]];
    let near = |p: [[f32; 3]; 3]| {
        let c = [(p[0][0] + p[1][0] + p[2][0]) / 3.0, (p[0][1] + p[1][1] + p[2][1]) / 3.0, 0.0];
        let r = p.iter().map(|v| d2(*v, c)).fold(0.0f32, f32::max).sqrt();
        d2(c, [cam[0], cam[1], 0.0]).sqrt() - r < reach
    };
    let area2 = |p: [[f32; 3]; 3]| ((p[1][0] - p[0][0]) * (p[2][1] - p[0][1]) - (p[2][0] - p[0][0]) * (p[1][1] - p[0][1])).abs();
    let mut edge = 0.0f32;
    for t in idx.as_chunks::<3>().0 {
        let p = tri(t);
        if near(p) && area2(p) > 1e-6 { edge = edge.max(d2(p[0], p[1]).max(d2(p[1], p[2])).max(d2(p[2], p[0])).sqrt()); }
    }
    let n = ((edge / SPACING).ceil() as u32).clamp(1, MAX_SPLIT);
    let mut out = Tess::default();
    let mut far: HashMap<u32, u32> = HashMap::new();
    for t in idx.as_chunks::<3>().0 {
        let p = tri(t);
        if !near(p) {
            for &v in t {
                let k = *far.entry(v).or_insert_with(|| {
                    out.pos.push(pos[v as usize]);
                    out.uv.push(uv[v as usize]);
                    out.color.push(color[v as usize]);
                    out.near.push(false);
                    out.pos.len() as u32 - 1
                });
                out.idx.push(k);
            }
            continue;
        }
        if area2(p) <= 1e-6 { continue; }
        let base = out.pos.len() as u32;
        let (a, b, c) = (t[0] as usize, t[1] as usize, t[2] as usize);
        let lerp3 = |u: f32, v: f32, x: [f32; 3], y: [f32; 3], z: [f32; 3]| std::array::from_fn::<f32, 3, _>(|k| x[k] * (1.0 - u - v) + y[k] * u + z[k] * v);
        // Row j (toward c) holds n − j + 1 points (toward b).
        for j in 0..=n {
            for i in 0..=n - j {
                let (u, v) = (i as f32 / n as f32, j as f32 / n as f32);
                out.pos.push(lerp3(u, v, pos[a], pos[b], pos[c]));
                out.uv.push(std::array::from_fn(|k| uv[a][k] * (1.0 - u - v) + uv[b][k] * u + uv[c][k] * v));
                out.color.push(std::array::from_fn(|k| color[a][k] * (1.0 - u - v) + color[b][k] * u + color[c][k] * v));
                out.near.push(true);
            }
        }
        let row = |j: u32| base + (0..j).map(|r| n + 1 - r).sum::<u32>();
        for j in 0..n {
            for i in 0..n - j {
                let (p00, p10, p01) = (row(j) + i, row(j) + i + 1, row(j + 1) + i);
                out.idx.extend([p00, p10, p01]);
                if i + j + 1 < n { out.idx.extend([p10, row(j + 1) + i + 1, p01]); }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waves_are_unit_directions_and_small() {
        for w in WAVES { assert!(((w[0] * w[0] + w[1] * w[1]).sqrt() - 1.0).abs() < 1e-4, "{w:?}"); }
        let mut peak = 0.0f32;
        for i in 0..200 {
            for j in 0..200 { peak = peak.max(height([i as f32 * 1.3 - 130.0, j as f32 * 0.7 - 70.0], i as f32 * 0.05, 0.0).abs()); }
        }
        assert!(peak <= MAX_HEIGHT && peak > 0.15, "{peak}");
    }

    #[test]
    fn grad_matches_a_finite_difference_of_height() {
        let h = 1e-2;
        for &(x, z, t, d) in &[(0.0, 0.0, 0.0, 0.0), (13.7, -42.1, 3.3, 30.0), (-250.0, 180.0, 77.7, 90.0), (300.5, 61.25, 1500.0, 200.0)] {
            let g = grad([x, z], t, d);
            let dx = (height([x + h, z], t, d) - height([x - h, z], t, d)) / (2.0 * h);
            let dz = (height([x, z + h], t, d) - height([x, z - h], t, d)) / (2.0 * h);
            assert!((g[0] - dx).abs() < 2e-3 && (g[1] - dz).abs() < 2e-3, "{g:?} vs ({dx}, {dz}) at {x}, {z}, {t}, {d}");
        }
    }

    #[test]
    fn the_short_waves_drop_out_first_with_distance() {
        let l = |w: &[f32; 4]| std::f32::consts::TAU / w[2];
        for w in &WAVES {
            assert_eq!(wave_weight(w, 0.0), 1.0);
            assert_eq!(wave_weight(w, WAVE_LOD * l(w)), 1.0);
            assert_eq!(wave_weight(w, 2.0 * WAVE_LOD * l(w)), 0.0);
        }
        // Every wave whole where the displacement is whole; the λ = 22 swell still on at 300.
        assert!(WAVES.iter().all(|w| wave_weight(w, SWELL_FADE[0] - 3.0) == 1.0));
        assert!(wave_weight(&WAVES[0], 300.0) > 0.0 && wave_weight(&WAVES[3], 100.0) == 0.0);
        assert!(WAVE_LOD * l(&WAVES[3]) >= SWELL_FADE[0] - 3.0);
    }

    #[test]
    fn the_swell_travels() {
        let a = height([10.0, 5.0], 0.0, 0.0);
        let b = height([10.0, 5.0], 1.0, 0.0);
        assert!((a - b).abs() > 1e-3);
    }

    #[test]
    fn shore_damp_ramps_from_land_to_open_water() {
        assert_eq!(shore_damp(-3.0), 0.0);
        assert_eq!(shore_damp(0.0), 0.0);
        assert_eq!(shore_damp(SHORE_DEPTH[0]), 0.0);
        assert_eq!(shore_damp(SHORE_DEPTH[1]), 1.0);
        assert_eq!(shore_damp(PROBE[1]), 1.0);
        let mut last = 0.0;
        for k in 0..100 {
            let d = shore_damp(k as f32 * 0.1);
            assert!(d >= last);
            last = d;
        }
        assert_eq!(far_fade(0.0), 1.0);
        assert_eq!(far_fade(SWELL_FADE[0]), 1.0);
        assert_eq!(far_fade(SWELL_FADE[1]), 0.0);
    }

    #[test]
    fn shore_map_calms_the_water_near_land() {
        // Land (z 61) for x < 0, a bed sloping down from the shore to 20 under the water (z 60) at x = 50, open beyond,
        // and a swim surface 0.3 under the water over all of it; the probe gives the highest surface on the line.
        let mut probe = |a: [f32; 3], b: [f32; 3]| -> Option<f32> {
            let ground = if a[0] < 0.0 { Some(61.0) } else if a[0] < 50.0 { Some(60.0 - a[0] * 0.4) } else { None };
            let swim = (a[0] >= 0.0).then_some(59.7);
            [ground, swim].into_iter().flatten().filter(|z| (b[2]..=a[2]).contains(z)).reduce(f32::max)
        };
        let mut m = ShoreMap::default();
        assert_eq!(m.damp(-20.0, 3.0, 60.0, &mut probe), 0.0);
        assert_eq!(m.damp(4.0, 3.0, 60.0, &mut probe), 0.0, "within the radius of land");
        let mid = m.damp(25.0, 3.0, 60.0, &mut probe);
        assert!(mid > 0.0 && mid < 1.0, "{mid}");
        assert_eq!(m.damp(120.0, 3.0, 60.0, &mut probe), 1.0, "the swim surface is not the bed");
        let mut last = 0.0;
        for k in 0..60 {
            let d = m.damp(k as f32 * 2.0, -7.0, 60.0, &mut probe);
            assert!(d + 1e-6 >= last, "{k}");
            last = d;
        }
        assert!(m.stats[1] > 0 && m.stats[2] > 0 && m.stats[0] > m.stats[1] + m.stats[2]);
    }

    #[test]
    fn tessellate_splits_only_near_triangles_and_keeps_the_edges_shared() {
        // Two 30-unit right triangles sharing the hypotenuse near the camera, one far away.
        let pos = [[0.0, 0.0, 60.0], [30.0, 0.0, 60.0], [0.0, 30.0, 60.0], [30.0, 30.0, 60.0], [1000.0, 0.0, 60.0], [1030.0, 0.0, 60.0], [1000.0, 30.0, 60.0]];
        let uv = pos.map(|p| [p[0] / 30.0, p[1] / 30.0]);
        let color = [[1.0; 4]; 7];
        let idx = [0, 1, 2, 1, 2, 3, 4, 5, 6];
        let t = tessellate(&pos, &uv, &color, &idx, [10.0, 10.0], 100.0);
        let n = (30.0f32 * 2.0f32.sqrt() / SPACING).ceil() as usize;
        assert_eq!(t.idx.len(), 3 * (2 * n * n + 1));
        assert_eq!(t.near.iter().filter(|x| !**x).count(), 3);
        // Every split triangle has the area of an n-th part, and the points on the shared edge match.
        let total: f32 = t.idx.as_chunks::<3>().0.iter().map(|k| {
            let p = [t.pos[k[0] as usize], t.pos[k[1] as usize], t.pos[k[2] as usize]];
            ((p[1][0] - p[0][0]) * (p[2][1] - p[0][1]) - (p[2][0] - p[0][0]) * (p[1][1] - p[0][1])).abs() * 0.5
        }).sum();
        assert!((total - (900.0 + 450.0)).abs() < 0.5, "{total}");
        let on_edge = |p: &[f32; 3]| (p[0] + p[1] - 30.0).abs() < 1e-3;
        let m = (n + 1) * (n + 2) / 2;
        let key = |p: &[f32; 3]| ((p[0] * 100.0).round() as i32, (p[1] * 100.0).round() as i32);
        let mut a: Vec<(i32, i32)> = t.pos[..m].iter().filter(|p| on_edge(p)).map(key).collect();
        let mut b: Vec<(i32, i32)> = t.pos[m..2 * m].iter().filter(|p| on_edge(p)).map(key).collect();
        a.sort_unstable();
        b.sort_unstable();
        assert_eq!(a, b);
        assert_eq!(a.len(), n + 1);
        // The UVs follow the points.
        for (p, st) in t.pos.iter().zip(&t.uv) { assert!((st[0] - p[0] / 30.0).abs() < 1e-4 && (st[1] - p[1] / 30.0).abs() < 1e-4); }
    }

    #[test]
    fn the_shader_holds_the_same_swell() {
        let src = include_str!("../assets/shaders/water.wgsl");
        for w in WAVES { assert!(src.contains(&format!("vec4<f32>({:?}, {:?}, {:?}, {:?})", w[0], w[1], w[2], w[3])), "{w:?}"); }
        assert!(src.contains(&format!("vec2<f32>({:?}, {:?})", SWELL_FADE[0], SWELL_FADE[1])));
        assert!(src.contains(&format!("vec2<f32>({:?}, {:?})", SHORE_DEPTH[0], SHORE_DEPTH[1])));
        assert!(src.contains(&format!("- {SPEED:?} * sqrt({G:?} * w.z) * t + f32(i) * {:?}", PHASE[1])) && PHASE.iter().enumerate().all(|(i, p)| (p - i as f32 * PHASE[1]).abs() < 1e-6));
        assert!(src.contains(&format!("0x{:x}u", crate::sea_water::SWELL_TAG)));
        assert!(src.contains(&format!("(1.0 + {CREST:?} * clamp(swell * swell_height(wp.xz, t_now, swell_d) / {MAX_HEIGHT:?}")));
        assert!(src.contains(&format!("const WAVE_LOD: f32 = {WAVE_LOD:?};")));
    }

    #[test]
    fn scale_defaults_to_one() {
        if std::env::var("RC_WATER_WAVES").is_err() && std::env::var("RC_WATER_WAVES_SCALE").is_err() { assert_eq!(scale(), 1.0); }
    }
}
