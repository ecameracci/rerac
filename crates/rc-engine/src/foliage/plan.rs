//! The foliage placement (crate::foliage): pure, once per level and option value, on a worker thread. Game axes
//! (Z up), world units. Nothing here touches the game state or its RNG stream: every random draw comes from a
//! splitmix64 hash of the level's own seed and the triangle / plant index.
//!
//! * **Grass**: candidate points over every LOD-0 tfrag triangle (`rc_formats::tfrag::tfrag_triangles(t, 0)`),
//!   area-weighted at [`Density::clumps`] per square unit, on every level but the stations ([`NO_FOLIAGE`]). A point
//!   is kept when its triangle faces up ([`faces_up`]), the ground there is grass ground ([`grass_ground`]: a turf
//!   texture, [`is_turf`]: enough grass texels and an even grain, whose level-0 texel average under the point reads as
//!   grass ([`is_grass`]) both on its own and times the baked vertex colour, the GS MODULATE of tfrag.wgsl, opaque:
//!   the baked light alone turns sand, paving and grey metal green on Aridia, Blarg, Pokitaru or Oltanis), no other
//!   opaque tfrag that is not grass ground lies just over it (a path or a decal: [`Grid::covered`]), and a `coll_line`
//!   from [`RAY_ABOVE`] above to [`RAY_BELOW`] below first meets walkable collision within [`GROUND_TOLERANCE`] of the
//!   drawn surface ([`ground_hit`], [`ground_verdict`], [`walkable`]). A kept
//!   point is a blade clump, or with the chances [`Density::ferns`] / [`Density::flowers`] a fern or a flower cluster
//!   ([`Kind`]).
//! * **Plant copies**: around each placed instance of the level's small plant shrub classes (crate::foliage
//!   `plant_classes`), [`Density::copies`] more at a random yaw and scale on a ring, each on walkable collision
//!   whose drawn surface there is grass ground.

use crate::graphics::Foliage;
use rc_formats::collision::Collision;
use rc_game::collision_query::{coll_line, QueryFlags};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// The steepest drawn ground grass grows on (from the vertical).
pub const MAX_SLOPE_DEG: f32 = 35.0;
/// The collision check's segment, from this far above the drawn point ...
pub const RAY_ABOVE: f32 = 1.4;
/// ... to this far below it.
pub const RAY_BELOW: f32 = 0.5;
/// The most the collision ground may lie above or below the drawn surface.
pub const GROUND_TOLERANCE: f32 = 0.3;
/// The flattest collision face a point may stand on (normal z).
pub const MIN_GROUND_NZ: f32 = 0.6;
/// A tfrag this far over the drawn point (or 0.03 under it, a coplanar layer) hides the grass when it is opaque and
/// not green.
pub const COVER_ABOVE: f32 = 0.6;
/// Texel alpha (texture::scale_alpha: 0xff = GS 0x80) and vertex alpha (GS, 0x80 = 1) at or above the tfrag
/// pass's AREF 0x60: the pixel writes Z (tfrag_render.rs `alpha_mode`).
pub const OPAQUE_TEXEL: u8 = 0xc0;
pub const OPAQUE_VERTEX: u8 = 0x60;
/// The least share of a texture's opaque texels that read as grass on their own for it to grow any ([`TexStats`]):
/// the turf textures are 0.16 (Novalis' grass-and-dirt edges) to 1; the grey floors, sand, paving and glass that only
/// read green under a level's green-tinted vertex light are 0.
pub const MIN_GRASS_SHARE: f32 = 0.15;
/// The most a turf texture's luma spreads ([`TexStats::luma_sd`]): turf is an even fine noise (3 to 34, the mixed
/// grass-and-dirt edges the highest); the teal glowing panels of Blarg, Gemlik, Quartu and Veldin's finale, whose
/// darker texels pass as blue-green, are 46 to 64.
pub const MAX_LUMA_SD: f32 = 40.0;
/// Collision surfaces nothing grows on (rc_game::hero::surface): 0 water, 1 hot floor, 2 magnetic, 3 sinking
/// liquid, 4 sinking floor, 7 slippery, 8 / 0xc pits, 0xb liquid, 0xd deadly liquid, 0xe shallow water.
pub const BARREN_SURFACES: [i32; 11] = [0, 1, 2, 3, 4, 7, 8, 0xb, 0xc, 0xd, 0xe];
/// The width of the shrink band in rank units (crate::foliage, foliage.wgsl): an instance shrinks from full size to
/// nothing while the density at its distance falls over this much past its rank.
pub const SHRINK_BAND: f32 = 0.15;
/// The horizontal cell of the cover grid (world units).
const GRID_CELL: f32 = 4.0;
/// A flower cluster's height over the clump height it replaces (its heads show just over the blades).
pub const FLOWER_RISE: f32 = 1.2;
/// A fern's height and spread (frond reach) ranges, world units.
pub const FERN_HEIGHT: [f32; 2] = [0.22, 0.34];
pub const FERN_SPREAD: [f32; 2] = [0.4, 0.7];
/// The share of flower clusters with yellow heads (the others white).
pub const YELLOW_FLOWERS: f32 = 0.4;

/// The levels that grow nothing whatever their textures (rc_game::game_state `LEVEL_COUNT` = 19; the travel and
/// space scenes between planets are movies, no level): space stations and ships, where any green is lit metal,
/// glowing panels or a planter.
pub const NO_FOLIAGE: [(u32, &str); 3] = [
    (6, "Blarg Station, Nebula G34: a space station"),
    (13, "Gemlik Base, Oltanis Orbit: a space station"),
    (17, "Drek's Fleet: space ships"),
];

/// Why level `level` grows nothing ([`NO_FOLIAGE`]), None where it may.
pub fn no_foliage(level: u32) -> Option<&'static str> { NO_FOLIAGE.iter().find(|e| e.0 == level).map(|e| e.1) }

/// One option value's numbers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Density {
    /// Grass clump candidates per square unit of drawn ground.
    pub clumps: f32,
    /// Clump height range (world units).
    pub height: [f32; 2],
    /// Clump width factor (1 = the template's 0.15-unit radius).
    pub width: f32,
    /// Every instance drawn up to this distance from the camera; then fewer, none from `end`.
    pub full: f32,
    pub end: f32,
    /// Extra copies per placed plant shrub (the fraction: one more with that chance).
    pub copies: f32,
    /// The ring the copies land on around their plant (world units).
    pub ring: [f32; 2],
    /// The chance a kept grass point is a fern, and a flower cluster.
    pub ferns: f32,
    pub flowers: f32,
}

/// The numbers of an option value; None for Original (no foliage pass). Both keep the blades about ankle-high on
/// Ratchet (his knee is about 0.35 units up); Lush is denser, reaches further and is a little taller. Lush's clump discs
/// (32 a square unit, each about 0.35 units across) cover the ground about three times over, a carpet at Kerwan's
/// and Novalis' spawns; Moderate's (14, 0.31 across) a little over once.
pub fn density(f: Foliage) -> Option<Density> {
    match f {
        Foliage::Original => None,
        Foliage::Moderate => Some(Density {
            clumps: 14.0,
            height: [0.09, 0.18],
            width: 1.1,
            full: 14.0,
            end: 30.0,
            copies: 1.5,
            ring: [0.9, 3.0],
            ferns: 0.003,
            flowers: 0.006,
        }),
        Foliage::Lush => Some(Density {
            clumps: 32.0,
            height: [0.12, 0.24],
            width: 1.25,
            full: 22.0,
            end: 48.0,
            copies: 3.5,
            ring: [0.8, 4.0],
            ferns: 0.003,
            flowers: 0.006,
        }),
    }
}

/// What a grass point grows (crate::foliage's template per kind).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Kind {
    Blades,
    Flower { yellow: bool },
    Fern,
}

/// The size factor of an instance of rank `rank` (0..1) at distance `d`: with the density f = `(end − d) / (end − full)`
/// (clamped 0..1) stretched to f·(1 + [`SHRINK_BAND`]), 1 while that is a band above the rank, a smoothstep down to 0
/// where it reaches the rank, so every rank is whole within `full`, none is left at `end` and the far field thins one
/// instance at a time without popping. The reference for foliage.wgsl `vertex`'s `s`.
#[cfg(test)]
pub fn thin(rank: f32, d: f32, full: f32, end: f32) -> f32 {
    let f = ((end - d) / (end - full).max(1e-3)).clamp(0.0, 1.0) * (1.0 + SHRINK_BAND);
    let t = ((f - rank) / SHRINK_BAND).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// A level-0 tfrag texture as decoded (`texture::decode_tfrag_mip_levels`): display-encoded RGB, alpha 0..0xff.
pub struct Tex {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Tex {
    /// The texel at `uv` (nearest, wrapped: tfrag UVs repeat).
    fn texel(&self, uv: [f32; 2], dx: i32, dy: i32) -> [u8; 4] {
        let (w, h) = (self.width.max(1) as i32, self.height.max(1) as i32);
        let x = ((uv[0] * w as f32).floor() as i32 + dx).rem_euclid(w);
        let y = ((uv[1] * h as f32).floor() as i32 + dy).rem_euclid(h);
        let i = (y * w + x) as usize * 4;
        self.rgba.get(i..i + 4).map_or([0; 4], |p| [p[0], p[1], p[2], p[3]])
    }

    /// The centre texel's alpha and the RGB average of the 5×5 texels around `uv` (about what the GS mip rule shows
    /// a few units away, and steadier than one texel).
    fn sample(&self, uv: [f32; 2]) -> ([u8; 3], u8) {
        let mut s = [0u32; 3];
        for dy in -2..=2 {
            for dx in -2..=2 {
                let t = self.texel(uv, dx, dy);
                for k in 0..3 { s[k] += t[k] as u32; }
            }
        }
        (s.map(|v| (v / 25) as u8), self.texel(uv, 0, 0)[3])
    }
}

/// A texture's whole-image numbers: size, the opaque texels' mean colour, the share of them that read as grass on
/// their own ([`is_grass`]) and the spread (standard deviation) of their luma.
#[derive(Clone, Copy, Debug)]
pub struct TexStats {
    pub width: u32,
    pub height: u32,
    pub mean: [u8; 3],
    pub grass_share: f32,
    pub luma_sd: f32,
}

pub fn texture_stats(t: &Tex) -> TexStats {
    let (mut s, mut n, mut g, mut l1, mut l2) = ([0f64; 3], 0f64, 0f64, 0f64, 0f64);
    for p in t.rgba.as_chunks::<4>().0.iter().filter(|p| p[3] >= OPAQUE_TEXEL) {
        for k in 0..3 { s[k] += p[k] as f64; }
        let l = 0.299 * p[0] as f64 + 0.587 * p[1] as f64 + 0.114 * p[2] as f64;
        (l1, l2, n) = (l1 + l, l2 + l * l, n + 1.0);
        g += is_grass([p[0], p[1], p[2]]) as u8 as f64;
    }
    let n1 = n.max(1.0);
    let m = l1 / n1;
    TexStats { width: t.width, height: t.height, mean: s.map(|v| (v / n1) as u8), grass_share: (g / n1) as f32, luma_sd: (l2 / n1 - m * m).max(0.0).sqrt() as f32 }
}

/// One LOD-0 tfrag triangle: world positions, UVs (S, T / 4096), the lit vertex colours (0x80 = 1.0) and its texture
/// (index into [`PlanInput::textures`]).
#[derive(Clone, Copy, Debug)]
pub struct Tri {
    pub p: [[f32; 3]; 3],
    pub uv: [[f32; 2]; 3],
    pub col: [[u8; 4]; 3],
    pub tex: u32,
}

impl Tri {
    pub fn normal(&self) -> [f32; 3] {
        let [a, b, c] = self.p;
        cross(sub(b, a), sub(c, a))
    }

    fn area(&self) -> f32 { len(self.normal()) * 0.5 }

    fn at(&self, w: [f32; 3]) -> ([f32; 3], [f32; 2], [f32; 4]) {
        let p = std::array::from_fn(|k| self.p[0][k] * w[0] + self.p[1][k] * w[1] + self.p[2][k] * w[2]);
        let uv = std::array::from_fn(|k| self.uv[0][k] * w[0] + self.uv[1][k] * w[1] + self.uv[2][k] * w[2]);
        let col = std::array::from_fn(|k| self.col[0][k] as f32 * w[0] + self.col[1][k] as f32 * w[1] + self.col[2][k] as f32 * w[2]);
        (p, uv, col)
    }

    /// The barycentric weights of `(x, y)` in the triangle seen from above, None outside it or for an edge-on one.
    fn weights_xy(&self, x: f32, y: f32) -> Option<[f32; 3]> {
        let [a, b, c] = self.p;
        let d = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1]);
        if d.abs() < 1e-6 { return None; }
        let w0 = ((b[1] - c[1]) * (x - c[0]) + (c[0] - b[0]) * (y - c[1])) / d;
        let w1 = ((c[1] - a[1]) * (x - c[0]) + (a[0] - c[0]) * (y - c[1])) / d;
        let w2 = 1.0 - w0 - w1;
        const E: f32 = -1e-4;
        (w0 >= E && w1 >= E && w2 >= E).then_some([w0, w1, w2])
    }
}

/// A placed plant shrub the copies grow around: its class (`LevelShrubs::classes`), its instance (for the palette),
/// origin and the instance matrix's class → world columns.
#[derive(Clone, Copy, Debug)]
pub struct PlantSource {
    pub class: usize,
    pub instance: u32,
    pub origin: [f32; 3],
    pub lin: [[f32; 3]; 3],
}

/// Everything the placement reads, copied out of the loaded level once (crate::foliage `build_input`).
pub struct PlanInput {
    /// The per-level seed's source (the level index).
    pub level: u32,
    pub tris: Vec<Tri>,
    pub textures: Vec<Option<Tex>>,
    pub collision: Option<Collision>,
    pub plants: Vec<PlantSource>,
}

/// A grass point: its kind, root on the drawn ground, yaw, height, width factor (a fern: its spread), rank (0..1, the
/// distance thinning), the root and tip colours (display-encoded) and its sway / blade seed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grass {
    pub kind: Kind,
    pub pos: [f32; 3],
    pub yaw: f32,
    pub height: f32,
    pub width: f32,
    pub rank: f32,
    pub root: [u8; 3],
    pub tip: [u8; 3],
    pub seed: u32,
    /// The tfrag triangle it grows on and the collision surface under it (crate::foliage `trace`).
    pub tri: u32,
    pub surface: i32,
}

/// A copy of [`PlantSource`] `src` at `pos` turned by `yaw` about Z and scaled by `scale`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlantCopy {
    pub src: usize,
    pub pos: [f32; 3],
    pub yaw: f32,
    pub scale: f32,
    pub rank: f32,
    pub seed: u32,
}

/// Why candidates were dropped, for the log line.
#[derive(Clone, Copy, Debug, Default)]
pub struct PlanStats {
    pub candidates: usize,
    pub steep: usize,
    pub not_grass: usize,
    /// Of `not_grass`: on a texture that is not turf ([`is_turf`]), and on a texel green only as drawn (lit).
    pub non_grass_texture: usize,
    pub tinted: usize,
    pub covered: usize,
    pub no_ground: usize,
    pub off_surface: usize,
    pub barren: usize,
    pub grass: usize,
    /// Of `grass`: the flower clusters and the ferns.
    pub flowers: usize,
    pub ferns: usize,
    pub copy_tries: usize,
    pub copies: usize,
    /// The level grows nothing ([`NO_FOLIAGE`]): why.
    pub no_foliage: Option<&'static str>,
    pub time: Duration,
}

impl PlanStats {
    fn add(&mut self, o: &PlanStats) {
        self.candidates += o.candidates;
        self.steep += o.steep;
        self.not_grass += o.not_grass;
        self.non_grass_texture += o.non_grass_texture;
        self.tinted += o.tinted;
        self.covered += o.covered;
        self.no_ground += o.no_ground;
        self.off_surface += o.off_surface;
        self.barren += o.barren;
        self.grass += o.grass;
        self.flowers += o.flowers;
        self.ferns += o.ferns;
    }
}

pub struct Plan {
    pub grass: Vec<Grass>,
    pub copies: Vec<PlantCopy>,
    pub stats: PlanStats,
}

/// The ground under a drawn point as the collision mesh has it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ground {
    Ok,
    /// Nothing within the segment.
    None,
    /// The first hit is too far above (something overhead) or below (the drawn point floats).
    OffSurface,
    /// Too steep, or a surface nothing grows on.
    Barren,
}

/// The ground under a drawn point at `visual_z` from its [`ground_hit`]: its z, surface id and unit normal z.
pub fn ground_verdict(visual_z: f32, hit: Option<(f32, i32, f32)>) -> Ground {
    match hit {
        None => Ground::None,
        Some((z, _, _)) if (z - visual_z).abs() > GROUND_TOLERANCE => Ground::OffSurface,
        Some((_, s, nz)) if nz < MIN_GROUND_NZ || !walkable(s) => Ground::Barren,
        Some(_) => Ground::Ok,
    }
}

/// The first collision hit down through `p` from `above` over it to `below` under it (rc_game::collision_query::coll_line,
/// world mesh, one-sided: faces seen from above): its z, surface id and unit normal z.
pub fn ground_hit(col: &Collision, p: [f32; 3], above: f32, below: f32) -> Option<(f32, i32, f32)> {
    coll_line(col, [p[0], p[1], p[2] + above], [p[0], p[1], p[2] - below], QueryFlags::NONE).map(|h| {
        let n = h.normal;
        (h.point[2], h.surface_id(), (n[2] / len(n).max(1e-12)).abs())
    })
}
/// A collision surface id plants grow on (not one of [`BARREN_SURFACES`]).
pub fn walkable(surface: i32) -> bool { !BARREN_SURFACES.contains(&surface) }

/// A triangle normal (any length, either winding: tfrag strips have none) within [`MAX_SLOPE_DEG`] of vertical.
pub fn faces_up(n: [f32; 3]) -> bool {
    let l = len(n);
    l > 0.0 && n[2].abs() / l >= MAX_SLOPE_DEG.to_radians().cos()
}

/// A display-encoded colour that reads as grass: green the strongest channel, a hue from yellow-green to
/// blue-green (70°..170°), saturated (≥ 0.18) and not black (value ≥ 0.08).
pub fn is_grass(rgb: [u8; 3]) -> bool {
    let [r, g, b] = rgb.map(|c| c as f32 / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    if g < max || max < 0.08 || max - min < 1e-4 { return false; }
    let sat = (max - min) / max;
    // Green is the maximum: hue = 60·((b − r) / (max − min) + 2).
    let hue = 60.0 * ((b - r) / (max - min) + 2.0);
    sat >= 0.18 && (70.0..=170.0).contains(&hue)
}

/// A turf texture: at least [`MIN_GRASS_SHARE`] of its texels read as grass and its luma spreads at most
/// [`MAX_LUMA_SD`].
pub fn is_turf(s: &TexStats) -> bool { s.grass_share >= MIN_GRASS_SHARE && s.luma_sd <= MAX_LUMA_SD }

/// Drawn ground grass grows on: a turf texture ([`is_turf`]) whose texels under the point read as grass on their own
/// (`raw`: the texture's colour, so sand or a grey floor lit green by the baked vertex colours does not pass) and as
/// drawn (`drawn`: × the vertex colour, so turf in shadow or tinted away from green does not).
pub fn grass_ground(raw: [u8; 3], drawn: [u8; 3], turf: bool) -> bool { turf && is_grass(raw) && is_grass(drawn) }

/// A drawn colour: the texel average × the vertex colour (GS MODULATE, 0x80 = 1.0), clamped.
pub fn modulate(tex: [u8; 3], col: [f32; 4]) -> [u8; 3] {
    std::array::from_fn(|k| (tex[k] as f32 * col[k] / 128.0).min(255.0) as u8)
}

/// Whether a drawn pixel writes Z (tfrag.wgsl: As = At·Af >> 7 at or above AREF 0x60).
pub fn opaque(texel_alpha: u8, vertex_alpha: f32) -> bool { texel_alpha >= OPAQUE_TEXEL && vertex_alpha >= OPAQUE_VERTEX as f32 }

/// The grass root colour (the ground's, a touch darker where the blades meet it) and tip colour (lighter, warmer).
pub fn blade_colours(ground: [u8; 3]) -> ([u8; 3], [u8; 3]) {
    let root = ground.map(|c| (c as f32 * 0.88) as u8);
    let warm = [22.0, 24.0, 0.0];
    let tip = std::array::from_fn(|k| (ground[k] as f32 * 1.45 + warm[k]).min(255.0) as u8);
    (root, tip)
}

/// A fern's root and frond-tip colours: the ground's green, darker and deeper at the root.
pub fn fern_colours(ground: [u8; 3]) -> ([u8; 3], [u8; 3]) {
    let root = ground.map(|c| (c as f32 * 0.5) as u8);
    let tip = [ground[0] as f32 * 0.8, ground[1] as f32 * 1.05 + 6.0, ground[2] as f32 * 0.85].map(|c| c.min(255.0) as u8);
    (root, tip)
}

/// The tfrags over a horizontal cell grid, for the cover test and the copies' drawn ground, and whether each texture
/// is turf ([`is_turf`]).
struct Grid {
    cells: HashMap<(i32, i32), Vec<u32>>,
    turf: Vec<bool>,
}

impl Grid {
    fn new(input: &PlanInput) -> Self {
        let turf = input.textures.iter().map(|t| t.as_ref().is_some_and(|t| is_turf(&texture_stats(t)))).collect();
        let mut cells: HashMap<(i32, i32), Vec<u32>> = HashMap::new();
        for (i, t) in input.tris.iter().enumerate() {
            let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
            for p in &t.p {
                for k in 0..2 { lo[k] = lo[k].min(p[k]); hi[k] = hi[k].max(p[k]); }
            }
            if !(lo[0].is_finite() && hi[0].is_finite()) || hi[0] - lo[0] > 256.0 || hi[1] - lo[1] > 256.0 { continue; }
            for cy in cell(lo[1])..=cell(hi[1]) {
                for cx in cell(lo[0])..=cell(hi[0]) { cells.entry((cx, cy)).or_default().push(i as u32); }
            }
        }
        Grid { cells, turf }
    }

    fn at(&self, x: f32, y: f32) -> &[u32] { self.cells.get(&(cell(x), cell(y))).map_or(&[], |v| v) }

    fn turf(&self, tex: u32) -> bool { self.turf.get(tex as usize).copied().unwrap_or(false) }

    /// Another opaque tfrag that is not grass ground ([`grass_ground`]) lies over `p` (from 0.03 under it to
    /// [`COVER_ABOVE`] over it).
    fn covered(&self, input: &PlanInput, skip: u32, p: [f32; 3]) -> bool {
        self.at(p[0], p[1]).iter().filter(|&&j| j != skip).any(|&j| {
            let t = &input.tris[j as usize];
            let Some(w) = t.weights_xy(p[0], p[1]) else { return false };
            let (q, uv, col) = t.at(w);
            let dz = q[2] - p[2];
            if !(-0.03..=COVER_ABOVE).contains(&dz) { return false; }
            let Some(Some(tex)) = input.textures.get(t.tex as usize) else { return false };
            let (rgb, a) = tex.sample(uv);
            opaque(a, col[3]) && !grass_ground(rgb, modulate(rgb, col), self.turf(t.tex))
        })
    }

    /// The drawn ground nearest `z` at `(x, y)` (within 0.4), when it is opaque: its z and whether it is grass ground.
    fn ground(&self, input: &PlanInput, x: f32, y: f32, z: f32) -> Option<(f32, bool)> {
        let mut best: Option<(f32, f32, bool)> = None;
        for &j in self.at(x, y) {
            let t = &input.tris[j as usize];
            let Some(w) = t.weights_xy(x, y) else { continue };
            let (q, uv, col) = t.at(w);
            let dz = (q[2] - z).abs();
            if dz > 0.4 || best.is_some_and(|b| b.0 <= dz) { continue; }
            let Some(Some(tex)) = input.textures.get(t.tex as usize) else { continue };
            let (rgb, a) = tex.sample(uv);
            if opaque(a, col[3]) { best = Some((dz, q[2], grass_ground(rgb, modulate(rgb, col), self.turf(t.tex)))); }
        }
        best.map(|b| (b.1, b.2))
    }
}

fn cell(v: f32) -> i32 { (v / GRID_CELL).floor() as i32 }

/// splitmix64.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self { Rng(seed) }
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
    /// Uniform in 0..1.
    pub fn f(&mut self) -> f32 { (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32 }
    pub fn range(&mut self, r: [f32; 2]) -> f32 { r[0] + (r[1] - r[0]) * self.f() }
}

/// The level's seed (its own stream: never the game's `srand(1234)` one).
pub fn level_seed(level: u32) -> u64 { 0x666f_6c69_6167_6500 ^ (level as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15) }

/// The whole placement (module docs). Deterministic: the triangles are split over threads, each triangle draws from
/// its own stream, and the results are joined in triangle order.
pub fn plan(input: &PlanInput, d: &Density) -> Plan {
    let t0 = Instant::now();
    if let Some(why) = no_foliage(input.level) {
        return Plan { grass: Vec::new(), copies: Vec::new(), stats: PlanStats { no_foliage: Some(why), time: t0.elapsed(), ..PlanStats::default() } };
    }
    let seed = level_seed(input.level);
    let grid = Grid::new(input);
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get()).clamp(1, 8);
    let chunk = input.tris.len().div_ceil(threads).max(1);
    let parts: Vec<(Vec<Grass>, PlanStats)> = std::thread::scope(|s| {
        let jobs: Vec<_> = (0..input.tris.len())
            .step_by(chunk)
            .map(|start| {
                let grid = &grid;
                s.spawn(move || grass_range(input, grid, d, seed, start..(start + chunk).min(input.tris.len())))
            })
            .collect();
        jobs.into_iter().map(|j| j.join().expect("foliage placement thread")).collect()
    });
    let mut stats = PlanStats::default();
    let mut grass = Vec::new();
    for (g, s) in parts {
        grass.extend(g);
        stats.add(&s);
    }
    let copies = plant_copies(input, &grid, d, seed, &mut stats);
    stats.time = t0.elapsed();
    Plan { grass, copies, stats }
}

fn grass_range(input: &PlanInput, grid: &Grid, d: &Density, seed: u64, range: std::ops::Range<usize>) -> (Vec<Grass>, PlanStats) {
    let mut out = Vec::new();
    let mut st = PlanStats::default();
    for ti in range {
        let t = &input.tris[ti];
        let mut rng = Rng::new(seed ^ (ti as u64).wrapping_mul(0xd1b5_4a32_d192_ed03));
        let want = t.area() * d.clumps;
        let n = want.floor() as usize + (rng.f() < want.fract()) as usize;
        if n == 0 { continue; }
        st.candidates += n;
        if !faces_up(t.normal()) { st.steep += n; continue; }
        let Some(Some(tex)) = input.textures.get(t.tex as usize) else { st.not_grass += n; continue };
        if !grid.turf(t.tex) { st.not_grass += n; st.non_grass_texture += n; continue; }
        for _ in 0..n {
            // Uniform on the triangle.
            let (r1, r2) = (rng.f().sqrt(), rng.f());
            let w = [1.0 - r1, r1 * (1.0 - r2), r1 * r2];
            let (p, uv, col) = t.at(w);
            let (yaw, height, width, rank, bseed) = (rng.f() * std::f32::consts::TAU, rng.range(d.height), d.width * rng.range([0.8, 1.2]), rng.f(), rng.next_u64() as u32);
            let (pick, size) = (rng.f(), rng.f());
            let (rgb, a) = tex.sample(uv);
            let ground = modulate(rgb, col);
            if !opaque(a, col[3]) || !grass_ground(rgb, ground, true) {
                st.not_grass += 1;
                if opaque(a, col[3]) && is_grass(ground) { st.tinted += 1; }
                continue;
            }
            if grid.covered(input, ti as u32, p) { st.covered += 1; continue; }
            let Some(c) = input.collision.as_ref() else { st.no_ground += 1; continue };
            let hit = ground_hit(c, p, RAY_ABOVE, RAY_BELOW);
            let surface = hit.map_or(-1, |h| h.1);
            match ground_verdict(p[2], hit) {
                Ground::Ok => {}
                Ground::None => { st.no_ground += 1; continue; }
                Ground::OffSurface => { st.off_surface += 1; continue; }
                Ground::Barren => { st.barren += 1; continue; }
            }
            let g = if pick < d.ferns {
                let (root, tip) = fern_colours(ground);
                let lerp = |r: [f32; 2]| r[0] + (r[1] - r[0]) * size;
                // Larger than a clump, so kept further out.
                Grass { kind: Kind::Fern, pos: p, yaw, height: lerp(FERN_HEIGHT), width: lerp(FERN_SPREAD), rank: rank * 0.6, root, tip, seed: bseed, tri: ti as u32, surface }
            } else {
                let (root, tip) = blade_colours(ground);
                let mut g = Grass { kind: Kind::Blades, pos: p, yaw, height, width, rank, root, tip, seed: bseed, tri: ti as u32, surface };
                if pick < d.ferns + d.flowers {
                    g.kind = Kind::Flower { yellow: size < YELLOW_FLOWERS };
                    (g.height, g.width) = (height * FLOWER_RISE, 1.0);
                }
                g
            };
            match g.kind {
                Kind::Flower { .. } => st.flowers += 1,
                Kind::Fern => st.ferns += 1,
                Kind::Blades => {}
            }
            out.push(g);
            st.grass += 1;
        }
    }
    (out, st)
}

fn plant_copies(input: &PlanInput, grid: &Grid, d: &Density, seed: u64, st: &mut PlanStats) -> Vec<PlantCopy> {
    let mut out = Vec::new();
    let Some(col) = input.collision.as_ref() else { return out };
    for (si, s) in input.plants.iter().enumerate() {
        let mut rng = Rng::new(seed ^ 0x706c_616e_7473 ^ (si as u64).wrapping_mul(0xa076_1d64_78bd_642f));
        let n = d.copies.floor() as usize + (rng.f() < d.copies.fract()) as usize;
        for _ in 0..n {
            st.copy_tries += 1;
            let (a, r) = (rng.f() * std::f32::consts::TAU, rng.range([d.ring[0] * d.ring[0], d.ring[1] * d.ring[1]]).sqrt());
            let (yaw, scale, rank, cseed) = (rng.f() * std::f32::consts::TAU, rng.range([0.75, 1.15]), rng.f(), rng.next_u64() as u32);
            let (x, y) = (s.origin[0] + r * a.cos(), s.origin[1] + r * a.sin());
            // The first ground within 2.5 units up or down of the plant's own height, walkable and flat enough.
            let Some(h) = ground_hit(col, [x, y, s.origin[2]], 2.5, 2.5) else { continue };
            if ground_verdict(h.0, Some(h)) != Ground::Ok { continue; }
            let Some((vz, true)) = grid.ground(input, x, y, h.0) else { continue };
            out.push(PlantCopy { src: si, pos: [x, y, vz], yaw, scale, rank, seed: cseed });
            st.copies += 1;
        }
    }
    out
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] { [a[0] - b[0], a[1] - b[1], a[2] - b[2]] }
fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] { [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]] }
fn len(a: [f32; 3]) -> f32 { (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt() }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grass_colours() {
        // Novalis-like greens, olive and blue-green.
        for c in [[60, 110, 40], [90, 120, 50], [40, 90, 70], [30, 60, 20]] { assert!(is_grass(c), "{c:?}"); }
        // Dirt, sand, rock, path, water blue, black, white, a yellow-dominant olive.
        for c in [[120, 90, 60], [200, 180, 140], [100, 100, 100], [150, 140, 120], [40, 80, 160], [5, 8, 4], [250, 255, 250], [130, 120, 40]] {
            assert!(!is_grass(c), "{c:?}");
        }
    }

    #[test]
    fn slope_limit() {
        assert!(faces_up([0.0, 0.0, 1.0]) && faces_up([0.0, 0.0, -3.0]));
        let s = MAX_SLOPE_DEG.to_radians();
        assert!(faces_up([(s - 0.01).sin(), 0.0, (s - 0.01).cos()]));
        assert!(!faces_up([(s + 0.01).sin(), 0.0, (s + 0.01).cos()]));
        assert!(!faces_up([1.0, 0.0, 0.0]) && !faces_up([0.0; 3]));
    }

    #[test]
    fn ground_rules() {
        assert_eq!(ground_verdict(5.0, None), Ground::None);
        assert_eq!(ground_verdict(5.0, Some((5.1, 0x10, 0.95))), Ground::Ok);
        assert_eq!(ground_verdict(5.0, Some((5.0, -1, 0.95))), Ground::Ok);
        assert_eq!(ground_verdict(5.0, Some((6.0, 0x10, 1.0))), Ground::OffSurface);
        assert_eq!(ground_verdict(5.0, Some((4.5, 0x10, 1.0))), Ground::OffSurface);
        assert_eq!(ground_verdict(5.0, Some((5.0, 0x10, 0.4))), Ground::Barren);
        for s in BARREN_SURFACES { assert_eq!(ground_verdict(5.0, Some((5.0, s, 1.0))), Ground::Barren, "surface {s}"); }
        assert!(walkable(5) && walkable(9) && walkable(0x10) && !walkable(0) && !walkable(0xd));
    }

    #[test]
    fn opacity_and_colours() {
        assert!(opaque(0xff, 128.0) && opaque(0xc0, 96.0));
        assert!(!opaque(0xbf, 128.0) && !opaque(0xff, 95.0));
        assert_eq!(modulate([100, 200, 255], [128.0, 64.0, 255.0, 128.0]), [100, 100, 255]);
        for f in [blade_colours, fern_colours] {
            let (root, tip) = f([60, 110, 40]);
            assert!(root.iter().zip(&[60, 110, 40]).all(|(r, g)| r <= g));
            assert!(tip.iter().zip(&root).all(|(t, r)| t > r) && is_grass(root) && is_grass(tip));
        }
    }

    #[test]
    fn density_tables() {
        assert!(density(Foliage::Original).is_none());
        let (m, l) = (density(Foliage::Moderate).unwrap(), density(Foliage::Lush).unwrap());
        assert!(l.clumps > 2.0 * m.clumps && l.height[1] > m.height[1] && l.full > m.full && l.end > m.end && l.copies > m.copies);
        // Lush's clump discs (the template's 0.14 radius × width) cover the ground about three times over: a carpet.
        assert!(l.clumps * std::f32::consts::PI * (0.14 * l.width).powi(2) > 3.0 && l.width > m.width);
        for d in [m, l] {
            assert!(d.full < d.end && d.height[0] < d.height[1] && d.ring[0] < d.ring[1]);
            // Below Ratchet's knee, flowers included; ferns and flowers a sparse share.
            assert!(d.height[1] * FLOWER_RISE < 0.3 && FERN_HEIGHT[1] < 0.35);
            assert!(d.ferns + d.flowers < 0.02 && d.ferns > 0.0 && d.flowers > 0.0);
            // Within the full distance everything is whole; at the end nothing is left; in between it only shrinks.
            assert_eq!(thin(0.999, d.full, d.full, d.end), 1.0);
            assert_eq!(thin(0.0, d.end, d.full, d.end), 0.0);
            let mut last = 1.0;
            for k in 0..=100 {
                let s = thin(0.4, d.full + (d.end - d.full) * k as f32 / 100.0, d.full, d.end);
                assert!(s <= last + 1e-6 && (0.0..=1.0).contains(&s));
                last = s;
            }
        }
    }

    #[test]
    fn barycentric_cover() {
        let t = Tri { p: [[0.0, 0.0, 1.0], [4.0, 0.0, 1.0], [0.0, 4.0, 1.0]], uv: [[0.0; 2]; 3], col: [[0x80; 4]; 3], tex: 0 };
        let w = t.weights_xy(1.0, 1.0).unwrap();
        assert!((w.iter().sum::<f32>() - 1.0).abs() < 1e-6 && (t.at(w).0[2] - 1.0).abs() < 1e-6);
        assert!(t.weights_xy(3.0, 3.0).is_none());
        // A grey opaque layer 0.2 over the point hides it; a green one or one 1 unit up does not.
        let grey = Tex { width: 1, height: 1, rgba: vec![110, 110, 110, 0xff] };
        let green = Tex { width: 1, height: 1, rgba: vec![60, 120, 40, 0xff] };
        let ground = Tri { p: [[0.0, 0.0, 0.8], [4.0, 0.0, 0.8], [0.0, 4.0, 0.8]], ..t };
        let high = Tri { p: [[0.0, 0.0, 2.0], [4.0, 0.0, 2.0], [0.0, 4.0, 2.0]], ..t };
        let input = |cover: Tri, tex: Tex| PlanInput { level: 1, tris: vec![ground, cover], textures: vec![Some(tex)], collision: None, plants: vec![] };
        let p = [1.0, 1.0, 0.8];
        let i = input(t, grey);
        assert!(Grid::new(&i).covered(&i, 0, p));
        let i = input(t, green);
        assert!(!Grid::new(&i).covered(&i, 0, p));
        let i = input(high, Tex { width: 1, height: 1, rgba: vec![110, 110, 110, 0xff] });
        assert!(!Grid::new(&i).covered(&i, 0, p));
    }

    #[test]
    fn grass_ground_reads_the_texture() {
        // The feedback's ground: (raw texture mean, mean vertex colour), green as drawn under the level's light.
        let lit = [
            ([152, 138, 84], [40, 68, 76, 128]),     // Aridia's sand under its blue-green night light (markers 2, 3)
            ([99, 101, 108], [42, 77, 31, 128]),     // Blarg's floor plates (6)
            ([74, 78, 95], [81, 80, 47, 128]),       // Gemlik's metal floor (21)
            ([196, 202, 53], [47, 69, 66, 128]),     // Rilgar's slime-yellow pipe rims (8, 9)
            ([199, 200, 200], [58, 65, 51, 128]),    // Batalia's concrete wall top (14)
            ([169, 171, 183], [66, 70, 51, 128]),    // Batalia's paving (15)
            ([80, 91, 95], [133, 133, 85, 128]),     // Pokitaru's platform (18)
            ([102, 112, 124], [57, 67, 58, 128]),    // Oltanis' wet pavement (22, 23)
            ([109, 158, 178], [73, 70, 51, 128]),    // Kalebo's glass-covered trough (25)
        ];
        for (raw, col) in lit {
            let drawn = modulate(raw, col.map(|c| c as f32));
            assert!(!is_grass(raw) && !grass_ground(raw, drawn, true), "{raw:?} drawn {drawn:?}");
        }
        // The lawns: Novalis' spawn, Kerwan's start, Pokitaru's islands, Batalia's grass, Aridia's green patches.
        for (raw, col) in [([66, 100, 35], [109, 100, 76]), ([62, 79, 31], [117, 110, 71]), ([53, 116, 19], [135, 135, 86]), ([113, 127, 72], [63, 68, 53]), ([61, 96, 30], [43, 72, 79])] {
            let drawn = modulate(raw, [col[0] as f32, col[1] as f32, col[2] as f32, 128.0]);
            assert!(grass_ground(raw, drawn, true), "{raw:?} drawn {drawn:?}");
            // A green texel on a texture that is not turf grows nothing.
            assert!(!grass_ground(raw, drawn, false));
        }
        let mixed = Tex { width: 4, height: 1, rgba: [[60u8, 110, 40, 0xff], [150, 135, 85, 0xff], [150, 135, 85, 0xff], [0, 0, 0, 0]].concat() };
        let s = texture_stats(&mixed);
        assert!((s.grass_share - 1.0 / 3.0).abs() < 1e-6 && s.mean == [120, 126, 70] && s.luma_sd > 0.0);
        let stats = |share: f32, sd: f32| TexStats { width: 1, height: 1, mean: [0; 3], grass_share: share, luma_sd: sd };
        // Turf (Novalis' edges, Kerwan's mixed ground); sand (0 grass texels); a teal glowing panel (Quartu's 93).
        assert!(is_turf(&stats(0.16, 20.0)) && is_turf(&stats(0.23, 33.8)) && is_turf(&stats(1.0, 3.0)));
        assert!(!is_turf(&stats(0.0, 5.6)) && !is_turf(&stats(0.38, 57.3)) && !is_turf(&stats(0.19, 46.4)));
    }

    #[test]
    fn station_levels() {
        for l in [6, 13, 17] { assert!(no_foliage(l).is_some(), "level {l}"); }
        for l in [1, 2, 3, 5, 8, 11, 14, 16] { assert!(no_foliage(l).is_none(), "level {l}"); }
        let green = Tex { width: 1, height: 1, rgba: vec![60, 120, 40, 0xff] };
        let flat = Tri { p: [[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [0.0, 10.0, 0.0]], uv: [[0.0; 2]; 3], col: [[0x80; 4]; 3], tex: 0 };
        let p = plan(&PlanInput { level: 6, tris: vec![flat], textures: vec![Some(green)], collision: None, plants: vec![] }, &density(Foliage::Lush).unwrap());
        assert!(p.grass.is_empty() && p.stats.candidates == 0 && p.stats.no_foliage.is_some());
    }

    #[test]
    fn placement_is_deterministic() {
        let green = || Tex { width: 2, height: 2, rgba: [60u8, 120, 40, 0xff].repeat(4) };
        let flat = Tri { p: [[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [0.0, 10.0, 0.0]], uv: [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]], col: [[0x80; 4]; 3], tex: 0 };
        let input = PlanInput { level: 4, tris: vec![flat; 3], textures: vec![Some(green())], collision: None, plants: vec![] };
        let d = density(Foliage::Moderate).unwrap();
        let (a, b) = (plan(&input, &d), plan(&input, &d));
        // No collision mesh: every grassy candidate is dropped at the ground check, the same ones each time.
        assert_eq!(a.stats.candidates, b.stats.candidates);
        assert!(a.stats.candidates > 100 && a.grass.is_empty() && a.stats.no_ground == a.stats.candidates);
        assert_ne!(level_seed(1), level_seed(4));
        let mut r = Rng::new(7);
        assert!((0..1000).map(|_| r.f()).all(|v| (0.0..1.0).contains(&v)));
    }
}
