//! **Port-only: Ratchet model detail Enhanced** (docs/plan/graphics_options.md). Ratchet's high-LOD mesh (class 0,
//! rc_game::hero::packs `RATCHET_CLASS`) refined once at load with curved PN triangles (Vlachos et al. 2001): every
//! triangle becomes `segments²` triangles on a cubic patch through its corners, so his silhouette and his per-vertex
//! lighting (moby.wgsl, Gouraud) read rounder. Done on the CPU when crate::moby_render builds the class's parts; no
//! per-frame cost. Original draws the game's mesh, byte-identical: the option only swaps the mesh assets' contents
//! ([`apply_model_detail`]).
//!
//! The steps ([`subdivide`]), over every part of the class at once (the parts are split by texture, so a texture
//! boundary is an edge two meshes share):
//! * vertices are welded by their packed position (exact: positions are the game's i16s), so UV seams and the packet
//!   duplicates meet;
//! * shading normals: the game's normals (azimuth / elevation, `normal_table`) at one position are averaged over the
//!   corners whose normals are within the crease angle; a split the model means (normals further apart) stays hard;
//! * geometry normals: the face normals (oriented by the game's normals, as the strips' winding is not normalised)
//!   averaged over the faces around a position joined by an edge whose dihedral is within the crease angle;
//! * an edge is curved only where every face on it has the same geometry normal at both ends, else straight; its points
//!   are computed once from the edge in welded order, so both sides land on the same bits (watertight);
//! * a new vertex takes its corners' UVs, RGBA multipliers and shading normals (linear), and their joint weights summed,
//!   the three largest kept and rounded to 256 (the shader's three-joint blend, /256); its CPU colour id is the
//!   nearest corner's.
//!
//! The original vertices keep their positions and skins, so the attachments (helmet, O2 mask, weapons, Clank, the
//! gadgets: joints of the same palette) sit where they did. The low LOD and the metal packets stay the game's.

use bevy::prelude::*;
use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

/// Ratchet's moby class (rc_game::hero::packs `RATCHET_CLASS`).
pub const HERO_CLASS: i32 = 0;
/// Segments per edge (`RC_SMOOTH_SEGMENTS`, 1..=4): 2 makes 4 triangles of each (3 looked the same on Ratchet, 2026-11-07).
const DEFAULT_SEGMENTS: u32 = 2;
/// Dihedral and normal angle (degrees) past which an edge or a normal split stays hard (`RC_SMOOTH_CREASE`).
const DEFAULT_CREASE: f32 = 60.0;

/// One mesh as crate::moby_render `build_parts` makes it: packed positions, GS-normalised UVs, the skin word
/// (`ATTRIBUTE_MOBY_SKIN`), the CPU colour id and the triangle list.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PartMesh {
    pub pos: Vec<[f32; 3]>,
    pub uv: Vec<[f32; 2]>,
    pub skin: Vec<[u32; 4]>,
    pub vid: Vec<u32>,
    pub idx: Vec<u32>,
}

/// The table's angle step (`NormalTable`: entry i is (cos, sin) of `i · 2π / 256`).
const STEP: f32 = std::f32::consts::TAU / 256.0;

/// The unit normal of skin word x's azimuth / elevation bytes (moby.wgsl: `(ca·ce, sa·ce, se)`).
pub fn decode_normal(w: u32) -> Vec3 {
    let (a, e) = ((w & 0xff) as f32 * STEP, ((w >> 8) & 0xff) as f32 * STEP);
    Vec3::new(a.cos() * e.cos(), a.sin() * e.cos(), e.sin())
}

/// The azimuth | elevation << 8 bytes nearest a unit normal.
pub fn encode_normal(n: Vec3) -> u32 {
    let az = (n.y.atan2(n.x) / STEP).round() as i32;
    let el = (n.z.clamp(-1.0, 1.0).asin() / STEP).round() as i32;
    az.rem_euclid(256) as u32 | (el.rem_euclid(256) as u32) << 8
}

/// Joint weights of skin words `w1` (joints) / `w2` (weights) with count `count`, as (joint, weight /256); count 0
/// (the identity) as joint 256.
fn joints_of(s: &[u32; 4]) -> ([(u16, f32); 3], usize) {
    let count = ((s[0] >> 16) & 0xff) as usize;
    let mut out = [(0u16, 0f32); 3];
    if count == 0 { out[0] = (256, 1.0); return (out, 1); }
    if count == 1 { out[0] = ((s[1] & 0xff) as u16, 1.0); return (out, 1); }
    for (k, o) in out.iter_mut().enumerate().take(count.min(3)) {
        *o = (((s[1] >> (8 * k)) & 0xff) as u16, ((s[2] >> (10 * k)) & 0x3ff) as f32 / 256.0);
    }
    (out, count.min(3))
}

/// The skin words y / z and the count of a blend of corners' joints (`(skin word, share)`): the three heaviest joints,
/// rounded to integers summing to 256 (largest remainders). The identity (count 0) only when nothing else is left.
fn blend_joints(corners: &[(&[u32; 4], f32)]) -> (u32, u32, u32) {
    let mut acc: Vec<(u16, f32)> = Vec::with_capacity(9);
    for (s, share) in corners {
        let (js, n) = joints_of(s);
        for &(j, w) in &js[..n] {
            match acc.iter_mut().find(|a| a.0 == j) {
                Some(a) => a.1 += w * share,
                None => acc.push((j, w * share)),
            }
        }
    }
    acc.retain(|a| a.1 > 0.0);
    if acc.iter().any(|a| a.0 != 256) { acc.retain(|a| a.0 != 256); }
    if acc.is_empty() || acc[0].0 == 256 { return (0, 0, 0); }
    acc.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    acc.truncate(3);
    let total: f32 = acc.iter().map(|a| a.1).sum();
    let raw: Vec<f32> = acc.iter().map(|a| a.1 / total * 256.0).collect();
    let mut q: Vec<u32> = raw.iter().map(|r| r.floor() as u32).collect();
    let mut left = 256 - q.iter().sum::<u32>();
    let mut order: Vec<usize> = (0..q.len()).collect();
    order.sort_by(|&a, &b| (raw[b] - raw[b].floor()).total_cmp(&(raw[a] - raw[a].floor())).then(a.cmp(&b)));
    for &k in order.iter().cycle() {
        if left == 0 { break; }
        q[k] += 1;
        left -= 1;
    }
    let kept: Vec<(u16, u32)> = acc.iter().zip(&q).filter(|(_, w)| **w > 0).map(|(a, w)| (a.0, *w)).collect();
    let count = kept.len() as u32;
    let (mut j, mut w) = (0u32, 0u32);
    for (k, (jj, ww)) in kept.iter().enumerate() {
        j |= (*jj as u32) << (8 * k);
        w |= ww << (10 * k);
    }
    if count == 1 { w = 256; }
    (count, j, w)
}

/// Union-find over a vertex's incidences.
fn find(p: &mut [usize], mut i: usize) -> usize {
    while p[i] != i { p[i] = p[p[i]]; i = p[i]; }
    i
}

struct Face {
    part: usize,
    local: [u32; 3],
    weld: [u32; 3],
    /// Unit, oriented by the game's normals; zero for a degenerate face (passed through).
    n: Vec3,
    angle: [f32; 3],
}

/// The cubic edge's inner control point near `p` toward `q` (PN triangles: `(2p + q − ((q − p)·n) n) / 3`), or
/// a third of the way when straight.
fn ctrl(p: Vec3, q: Vec3, n: Option<Vec3>) -> Vec3 {
    match n {
        Some(n) => (2.0 * p + q - (q - p).dot(n) * n) / 3.0,
        None => (2.0 * p + q) / 3.0,
    }
}

fn bezier(p0: Vec3, c0: Vec3, c1: Vec3, p1: Vec3, t: f32) -> Vec3 {
    let s = 1.0 - t;
    p0 * (s * s * s) + c0 * (3.0 * s * s * t) + c1 * (3.0 * s * t * t) + p1 * (t * t * t)
}

/// Refines `parts` (one class LOD's meshes) with PN triangles, `segments` per edge (1: unchanged), hard past
/// `crease_deg` (module docs). The parts come back in the same order, each with its own vertices.
pub fn subdivide(parts: &[PartMesh], segments: u32, crease_deg: f32) -> Vec<PartMesh> {
    let n = segments.clamp(1, 8);
    if n == 1 { return parts.to_vec(); }
    let crease = crease_deg.to_radians().cos();
    let p3 = |a: [f32; 3]| Vec3::from(a);

    // Weld by position.
    let mut weld_ix: HashMap<[u32; 3], u32> = HashMap::new();
    let mut weld_pos: Vec<Vec3> = Vec::new();
    let welds: Vec<Vec<u32>> = parts.iter().map(|m| m.pos.iter().map(|p| {
        *weld_ix.entry(p.map(f32::to_bits)).or_insert_with(|| { weld_pos.push(p3(*p)); weld_pos.len() as u32 - 1 })
    }).collect()).collect();
    let game_n: Vec<Vec<Vec3>> = parts.iter().map(|m| m.skin.iter().map(|s| decode_normal(s[0])).collect()).collect();

    // Faces, oriented by the game's normals.
    let mut faces = Vec::new();
    for (pi, m) in parts.iter().enumerate() {
        for t in m.idx.as_chunks::<3>().0 {
            let local = *t;
            let weld = local.map(|l| welds[pi][l as usize]);
            let p = weld.map(|w| weld_pos[w as usize]);
            let mut nf = (p[1] - p[0]).cross(p[2] - p[0]);
            let degenerate = weld[0] == weld[1] || weld[1] == weld[2] || weld[0] == weld[2] || nf.length_squared() <= f32::EPSILON;
            let mut angle = [0.0; 3];
            if degenerate { nf = Vec3::ZERO; } else {
                let g: Vec3 = local.iter().map(|&l| game_n[pi][l as usize]).sum();
                nf = nf.normalize();
                if nf.dot(g) < 0.0 { nf = -nf; }
                for k in 0..3 { angle[k] = (p[(k + 1) % 3] - p[k]).angle_between(p[(k + 2) % 3] - p[k]); }
            }
            faces.push(Face { part: pi, local, weld, n: nf, angle });
        }
    }

    // Incidences per welded vertex (non-degenerate faces).
    let mut inc: Vec<Vec<(u32, u8)>> = vec![Vec::new(); weld_pos.len()];
    for (fi, f) in faces.iter().enumerate() {
        if f.n == Vec3::ZERO { continue; }
        for k in 0..3 { inc[f.weld[k] as usize].push((fi as u32, k as u8)); }
    }
    // Per face corner: the shading normal and the geometry group (an index into `geo_n`).
    let mut shade: Vec<[Vec3; 3]> = vec![[Vec3::ZERO; 3]; faces.len()];
    let mut geo: Vec<[u32; 3]> = vec![[u32::MAX; 3]; faces.len()];
    let mut geo_n: Vec<Vec3> = Vec::new();
    for list in &inc {
        let k = list.len();
        let gn = |&(f, c): &(u32, u8)| { let f = &faces[f as usize]; game_n[f.part][f.local[c as usize] as usize] };
        let mut ps: Vec<usize> = (0..k).collect();
        for a in 0..k { for b in a + 1..k {
            if gn(&list[a]).dot(gn(&list[b])) >= crease { let (x, y) = (find(&mut ps, a), find(&mut ps, b)); ps[x] = y; }
        } }
        let mut pg: Vec<usize> = (0..k).collect();
        for a in 0..k { for b in a + 1..k {
            let (fa, fb) = (&faces[list[a].0 as usize], &faces[list[b].0 as usize]);
            let (ca, cb) = (list[a].1 as usize, list[b].1 as usize);
            let edge = (1..3).any(|i| { let w = fa.weld[(ca + i) % 3]; (1..3).any(|j| fb.weld[(cb + j) % 3] == w) });
            if edge && fa.n.dot(fb.n) >= crease && find(&mut ps, a) == find(&mut ps, b) {
                let (x, y) = (find(&mut pg, a), find(&mut pg, b)); pg[x] = y;
            }
        } }
        let mut s_sum: HashMap<usize, Vec3> = HashMap::new();
        let mut g_sum: HashMap<usize, Vec3> = HashMap::new();
        for (a, it) in list.iter().enumerate() {
            let f = &faces[it.0 as usize];
            let w = f.angle[it.1 as usize].max(1e-4);
            *s_sum.entry(find(&mut ps, a)).or_default() += gn(it) * w;
            *g_sum.entry(find(&mut pg, a)).or_default() += f.n * w;
        }
        let mut g_ix: HashMap<usize, u32> = HashMap::new();
        for (a, it) in list.iter().enumerate() {
            let f = &faces[it.0 as usize];
            let s = s_sum[&find(&mut ps, a)];
            shade[it.0 as usize][it.1 as usize] = s.try_normalize().unwrap_or_else(|| gn(it));
            let r = find(&mut pg, a);
            let gi = *g_ix.entry(r).or_insert_with(|| { geo_n.push(g_sum[&r].try_normalize().unwrap_or(f.n)); geo_n.len() as u32 - 1 });
            geo[it.0 as usize][it.1 as usize] = gi;
        }
    }

    // Edges: curved where every face on it has the same geometry groups at both ends.
    let mut edge_groups: HashMap<(u32, u32), Option<(u32, u32)>> = HashMap::new();
    for (fi, f) in faces.iter().enumerate() {
        if f.n == Vec3::ZERO { continue; }
        for k in 0..3 {
            let l = (k + 1) % 3;
            let (key, g) = if f.weld[k] < f.weld[l] { ((f.weld[k], f.weld[l]), (geo[fi][k], geo[fi][l])) } else { ((f.weld[l], f.weld[k]), (geo[fi][l], geo[fi][k])) };
            edge_groups.entry(key).and_modify(|e| if *e != Some(g) { *e = None }).or_insert(Some(g));
        }
    }
    // The edge's inner control points in welded order (lo end, hi end).
    let edge_ctrl = |lo: u32, hi: u32| -> (Vec3, Vec3) {
        let (a, b) = (weld_pos[lo as usize], weld_pos[hi as usize]);
        match edge_groups.get(&(lo, hi)).copied().flatten() {
            Some((ga, gb)) => (ctrl(a, b, Some(geo_n[ga as usize])), ctrl(b, a, Some(geo_n[gb as usize]))),
            None => (ctrl(a, b, None), ctrl(b, a, None)),
        }
    };

    // Per part local vertex: its shading normal (the first face corner's; every corner of one local vertex is in one
    // shading group, as their game normals are equal).
    let mut local_n: Vec<Vec<Option<Vec3>>> = parts.iter().map(|m| vec![None; m.pos.len()]).collect();
    for (fi, f) in faces.iter().enumerate() {
        if f.n == Vec3::ZERO { continue; }
        for k in 0..3 { local_n[f.part][f.local[k] as usize].get_or_insert(shade[fi][k]); }
    }

    let mut out: Vec<PartMesh> = parts.iter().map(|_| PartMesh::default()).collect();
    let mut corner_ix: Vec<HashMap<u32, u32>> = parts.iter().map(|_| HashMap::new()).collect();
    let mut edge_ix: Vec<HashMap<(u32, u32, u32), u32>> = parts.iter().map(|_| HashMap::new()).collect();
    // A new vertex from corners (local index, share) of part `pi`, at `pos`.
    let push = |o: &mut PartMesh, m: &PartMesh, ln: &[Option<Vec3>], corners: &[(u32, f32)], pos: Vec3| -> u32 {
        let normal = |l: u32| ln[l as usize].unwrap_or_else(|| decode_normal(m.skin[l as usize][0]));
        let mut uv = Vec2::ZERO;
        let mut nn = Vec3::ZERO;
        let mut mult = [0f32; 4];
        for &(l, s) in corners {
            uv += Vec2::from(m.uv[l as usize]) * s;
            nn += normal(l) * s;
            for (c, b) in mult.iter_mut().zip(m.skin[l as usize][3].to_le_bytes()) { *c += b as f32 * s; }
        }
        let near = corners.iter().max_by(|a, b| a.1.total_cmp(&b.1)).unwrap().0 as usize;
        let skins: Vec<(&[u32; 4], f32)> = corners.iter().map(|&(l, s)| (&m.skin[l as usize], s)).collect();
        let (count, j, w) = blend_joints(&skins);
        let flags = m.skin[near][0] & 0xff00_0000;
        let nrm = nn.try_normalize().unwrap_or_else(|| normal(near as u32));
        o.pos.push(pos.to_array());
        o.uv.push(uv.to_array());
        o.skin.push([encode_normal(nrm) | count << 16 | flags, j, w, u32::from_le_bytes(mult.map(|c| c.round().clamp(0.0, 255.0) as u8))]);
        o.vid.push(m.vid[near]);
        o.pos.len() as u32 - 1
    };

    for f in &faces {
        let (pi, m) = (f.part, &parts[f.part]);
        let o = &mut out[pi];
        let mut corner = |l: u32, o: &mut PartMesh| -> u32 {
            *corner_ix[pi].entry(l).or_insert_with(|| {
                let li = l as usize;
                o.pos.push(m.pos[li]);
                o.uv.push(m.uv[li]);
                let mut s = m.skin[li];
                if let Some(nn) = local_n[pi][li] { s[0] = (s[0] & !0xffff) | encode_normal(nn); }
                o.skin.push(s);
                o.vid.push(m.vid[li]);
                o.pos.len() as u32 - 1
            })
        };
        if f.n == Vec3::ZERO {
            for l in f.local { let v = corner(l, o); o.idx.push(v); }
            continue;
        }
        let pc = f.weld.map(|w| weld_pos[w as usize]);
        // Inner control points per directed corner pair: near[k][l] near corner k toward l.
        let mut near = [[Vec3::ZERO; 3]; 3];
        for k in 0..3 {
            let l = (k + 1) % 3;
            let (lo, hi) = if f.weld[k] < f.weld[l] { (k, l) } else { (l, k) };
            let (cl, ch) = edge_ctrl(f.weld[lo], f.weld[hi]);
            near[lo][hi] = cl;
            near[hi][lo] = ch;
        }
        let e = (near[0][1] + near[1][0] + near[1][2] + near[2][1] + near[2][0] + near[0][2]) / 6.0;
        let v = (pc[0] + pc[1] + pc[2]) / 3.0;
        let mid = e + (e - v) / 2.0;
        // Sample (i, j): shares (n−i−j, i, j)/n of corners 0, 1, 2.
        let mut grid = vec![0u32; ((n + 1) * (n + 2) / 2) as usize];
        let at = |i: u32, j: u32| (j * (n + 1) - j * (j.saturating_sub(1)) / 2 + i) as usize;
        for j in 0..=n {
            for i in 0..=n - j {
                let c = [n - i - j, i, j];
                let zeros: Vec<usize> = (0..3).filter(|&k| c[k] == 0).collect();
                let ix = if let Some(k) = (0..3).find(|&k| c[k] == n) {
                    corner(f.local[k], o)
                } else if zeros.len() == 1 {
                    // On the edge between the two other corners: canonical by welded order.
                    let z = zeros[0];
                    let (a, b) = ((z + 1) % 3, (z + 2) % 3);
                    let (lo, hi) = if f.weld[a] < f.weld[b] { (a, b) } else { (b, a) };
                    let s = c[hi];
                    let key = (f.local[lo], f.local[hi], s);
                    if let Some(&ix) = edge_ix[pi].get(&key) { ix } else {
                        let t = s as f32 / n as f32;
                        let (cl, ch) = edge_ctrl(f.weld[lo], f.weld[hi]);
                        let pos = bezier(pc[lo], cl, ch, pc[hi], t);
                        let ix = push(o, m, &local_n[pi], &[(f.local[lo], 1.0 - t), (f.local[hi], t)], pos);
                        edge_ix[pi].insert(key, ix);
                        ix
                    }
                } else {
                    let [u, vv, w] = c.map(|x| x as f32 / n as f32);
                    let pos = pc[0] * (u * u * u) + pc[1] * (vv * vv * vv) + pc[2] * (w * w * w)
                        + (near[0][1] * (u * u * vv) + near[1][0] * (u * vv * vv) + near[0][2] * (u * u * w)
                            + near[1][2] * (vv * vv * w) + near[2][0] * (u * w * w) + near[2][1] * (vv * w * w)) * 3.0
                        + mid * (6.0 * u * vv * w);
                    push(o, m, &local_n[pi], &[(f.local[0], u), (f.local[1], vv), (f.local[2], w)], pos)
                };
                grid[at(i, j)] = ix;
            }
        }
        for j in 0..n {
            for i in 0..n - j {
                o.idx.extend([grid[at(i, j)], grid[at(i + 1, j)], grid[at(i, j + 1)]]);
                if i + j + 1 < n { o.idx.extend([grid[at(i + 1, j)], grid[at(i + 1, j + 1)], grid[at(i, j + 1)]]); }
            }
        }
    }
    out
}

/// `RC_SMOOTH_SEGMENTS` / `RC_SMOOTH_CREASE` over the defaults.
pub fn params() -> (u32, f32) {
    let seg = std::env::var("RC_SMOOTH_SEGMENTS").ok().and_then(|v| v.trim().parse().ok()).unwrap_or(DEFAULT_SEGMENTS).clamp(1, 4);
    let crease = std::env::var("RC_SMOOTH_CREASE").ok().and_then(|v| v.trim().parse().ok()).unwrap_or(DEFAULT_CREASE);
    (seg, crease)
}

/// The last refinement: (input, segments, crease, output).
type Cached = (Vec<PartMesh>, u32, f32, Vec<PartMesh>);
static LAST: Mutex<Option<Cached>> = Mutex::new(None);

/// [`subdivide`] with [`params`], remembering the last result: a level builds Ratchet's parts several times (the level
/// instances, the gameplay hero, the scenes' actors), from the same class data.
pub fn subdivide_cached(parts: &[PartMesh]) -> (Vec<PartMesh>, bool) {
    let (segments, crease) = params();
    let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((_, _, _, o)) = last.as_ref().filter(|(i, s, c, _)| i.as_slice() == parts && *s == segments && *c == crease) {
        return (o.clone(), true);
    }
    let out = subdivide(parts, segments, crease);
    *last = Some((parts.to_vec(), segments, crease, out.clone()));
    (out, false)
}

/// The current choice (main world), read when a hero mesh is built so it starts as shown.
static SMOOTH: AtomicBool = AtomicBool::new(false);

/// Mesh assets with both builds: (asset, Original, Smooth, which one the asset holds).
type Entry = (AssetId<Mesh>, Mesh, Mesh, bool);
static REGISTRY: Mutex<Vec<Entry>> = Mutex::new(Vec::new());

/// Whether new hero meshes start smooth.
pub fn smooth_now() -> bool { SMOOTH.load(Ordering::Relaxed) }

/// Records a hero part's two builds (crate::moby_render), the asset holding the one [`smooth_now`] picked; drops
/// entries whose asset is gone.
pub fn register(meshes: &Assets<Mesh>, id: AssetId<Mesh>, original: Mesh, smooth: Mesh, shown: bool) {
    let mut r = REGISTRY.lock().unwrap_or_else(|e| e.into_inner());
    r.retain(|e| meshes.contains(e.0));
    r.push((id, original, smooth, shown));
}

/// The Ratchet model detail option onto the registered meshes: swaps the asset contents when it changes.
pub fn apply_model_detail(g: Res<crate::graphics::GraphicsSettings>, mut meshes: ResMut<Assets<Mesh>>) {
    let want = g.model.on();
    SMOOTH.store(want, Ordering::Relaxed);
    let mut r = REGISTRY.lock().unwrap_or_else(|e| e.into_inner());
    if r.iter().all(|e| e.3 == want) { return; }
    r.retain(|e| meshes.contains(e.0));
    for e in r.iter_mut().filter(|e| e.3 != want) {
        let mesh = if want { e.2.clone() } else { e.1.clone() };
        let _ = meshes.insert(e.0, mesh);
        e.3 = want;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// A skin word: normal `n`, one joint or a two-joint blend, multiplier 0x80.
    fn skin(n: Vec3, joints: &[(u8, u16)]) -> [u32; 4] {
        let (mut j, mut w) = (0u32, 0u32);
        for (k, (jj, ww)) in joints.iter().enumerate() { j |= (*jj as u32) << (8 * k); w |= (*ww as u32) << (10 * k); }
        [encode_normal(n) | (joints.len() as u32) << 16, j, w, 0x8080_8080]
    }

    /// An octahedron of radius 1000 (packed units) split into two parts along a UV seam (the upper and lower halves,
    /// each with its own copy of the equator's vertices and other UVs), some triangles wound the other way; the upper
    /// half skins to joint 1, the lower to joint 2, the equator 50/50.
    fn octahedron() -> Vec<PartMesh> {
        let r = 1000.0;
        let eq = [Vec3::X * r, Vec3::Y * r, -Vec3::X * r, -Vec3::Y * r];
        let mut parts = Vec::new();
        for (pole, joint, v) in [(Vec3::Z * r, 1u8, 0.0f32), (-Vec3::Z * r, 2, 1.0)] {
            let mut m = PartMesh::default();
            let mut add = |p: Vec3, js: &[(u8, u16)], u: f32| {
                m.pos.push(p.to_array());
                m.uv.push([u, v]);
                m.skin.push(skin(p.normalize(), js));
                m.vid.push(m.vid.len() as u32);
            };
            add(pole, &[(joint, 256)], 0.5);
            for (k, &p) in eq.iter().enumerate() { add(p, &[(1, 128), (2, 128)], k as f32 / 4.0); }
            for k in 0..4u32 {
                let (a, b) = (1 + k, 1 + (k + 1) % 4);
                if k % 2 == 0 { m.idx.extend([0, a, b]); } else { m.idx.extend([0, b, a]); }
            }
            parts.push(m);
        }
        parts
    }

    /// Edges of every output triangle by position: a closed surface has each exactly twice.
    fn open_edges(parts: &[PartMesh]) -> usize {
        let mut count: HashMap<([u32; 3], [u32; 3]), u32> = HashMap::new();
        for m in parts {
            for t in m.idx.as_chunks::<3>().0 {
                for k in 0..3 {
                    let (a, b) = (m.pos[t[k] as usize].map(f32::to_bits), m.pos[t[(k + 1) % 3] as usize].map(f32::to_bits));
                    *count.entry(if a < b { (a, b) } else { (b, a) }).or_default() += 1;
                }
            }
        }
        count.values().filter(|&&c| c != 2).count()
    }

    #[test]
    fn subdivision_is_watertight_across_seams() {
        let parts = octahedron();
        assert_eq!(open_edges(&parts), 0);
        for seg in 2..=4 {
            // A crease under the octahedron's 109.5° dihedral: every edge curved.
            let out = subdivide(&parts, seg, 120.0);
            assert_eq!(out.iter().map(|m| m.idx.len() / 3).sum::<usize>(), 8 * (seg * seg) as usize);
            assert_eq!(open_edges(&out), 0, "{seg} segments");
            // Curved: the sampled points bulge out of the flat faces (|x|+|y|+|z| = r there).
            let max_l1 = out.iter().flat_map(|m| &m.pos).map(|p| p[0].abs() + p[1].abs() + p[2].abs()).fold(0f32, f32::max);
            assert!(max_l1 > 1050.0, "{seg}: {max_l1}");
        }
    }

    #[test]
    fn hard_edges_stay_flat() {
        // A crease under the dihedral: every edge straight, every face flat (points on |x|+|y|+|z| = r).
        let out = subdivide(&octahedron(), 3, 60.0);
        assert_eq!(open_edges(&out), 0);
        for p in out.iter().flat_map(|m| &m.pos) {
            assert!((p[0].abs() + p[1].abs() + p[2].abs() - 1000.0).abs() < 0.5, "{p:?}");
        }
    }

    #[test]
    fn new_vertices_keep_skins_and_uvs() {
        let out = subdivide(&octahedron(), 3, 120.0);
        for (pi, m) in out.iter().enumerate() {
            assert_eq!((m.pos.len(), m.uv.len(), m.vid.len()), (m.skin.len(), m.skin.len(), m.skin.len()));
            for (k, s) in m.skin.iter().enumerate() {
                let count = (s[0] >> 16) & 0xff;
                assert!((1..=3).contains(&count), "{s:x?}");
                let ws: Vec<u32> = (0..count).map(|i| (s[2] >> (10 * i)) & 0x3ff).collect();
                assert_eq!(ws.iter().sum::<u32>(), 256, "{s:x?}");
                assert!(ws.iter().all(|&w| w > 0));
                // Only the two joints of the mesh.
                assert!((0..count).all(|i| matches!((s[1] >> (8 * i)) & 0xff, 1 | 2)));
                assert_eq!(s[3], 0x8080_8080);
                // UVs inside the part's range (its v is 0 or 1).
                assert_eq!(m.uv[k][1], pi as f32);
                // Shading normals: the sphere's, as the octahedron's are (within the table's step).
                let n = decode_normal(s[0]);
                assert!(n.dot(Vec3::from(m.pos[k]).normalize()) > 0.9, "{n} at {:?}", m.pos[k]);
            }
        }
    }

    #[test]
    fn joint_blend_keeps_three_and_sums() {
        let a = skin(Vec3::Z, &[(1, 100), (2, 100), (3, 56)]);
        let b = skin(Vec3::Z, &[(4, 200), (5, 56)]);
        let (count, j, w) = blend_joints(&[(&a, 0.5), (&b, 0.5)]);
        assert_eq!(count, 3);
        let ws: Vec<u32> = (0..3).map(|i| (w >> (10 * i)) & 0x3ff).collect();
        assert_eq!(ws.iter().sum::<u32>(), 256);
        // Heaviest first: 4 (100), then 1 and 2 (50 each).
        assert_eq!(j & 0xff, 4);
        let one = blend_joints(&[(&skin(Vec3::Z, &[(7, 256)]), 1.0)]);
        assert_eq!(one, (1, 7, 256));
    }

    #[test]
    fn normals_round_trip() {
        for az in [0u32, 17, 64, 200, 255] {
            for el in [0u32, 10, 60, 200, 250] {
                let w = az | el << 8;
                let n = decode_normal(w);
                assert!(decode_normal(encode_normal(n)).dot(n) > 0.9999, "{az} {el}");
            }
        }
    }
}
