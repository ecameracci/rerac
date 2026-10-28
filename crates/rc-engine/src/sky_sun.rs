//! **Port-only: the sun the sky paints**, where Lighting Enhanced's rays come from (crate::sun_rays) and the bearing of
//! the sun shadows' light where it exists (crate::sun_shadow).
//!
//! At level load [`detect`] draws the level's sky shells (crate::sky_render: shell and face order, each shell kind's
//! GS colour and blend, every shell at its first tick's SkyM) into a [`W`]×[`H`] latitude–longitude map of linear
//! luminance, then looks for the sun ([`find_sun`]): the brightest point (over a ~2° neighbourhood) no lower than
//! [`MIN_LAT`], bright in absolute terms and against the sky's median, in a daylight sky, whose bright surround is
//! one compact spot. The shell that adds most of the light there carries the sun, so [`follow`] turns the direction
//! with that shell's SkyM (its entity's rotation) every frame. A sky without such a spot (night, space, storms) has
//! no sun: [`SkySun::to_sun`] is `None` and no rays are drawn. Neither does a space sky (the flight between planets,
//! the title world: the sky camera off [`SKY_LAYER`]).
//!
//! Detection runs whatever the Lighting option (it can change in play) and takes ~20–35 ms of the load.
//!
//! Dev switch: `RC_SUN_RAYS_LOG=1` prints each level's verdict and figures, and an `RC_CAM` value facing the sun from
//! the spawn view (often blocked by the level: raise the eye); `2` also writes the map as
//! `work/fx/sky_sun_<level>.png` (display colours, the found sun marked red, each shell's own layer below). The
//! ignored test `survey_every_level` prints the same lines for levels 0–18 from `extracted/`.

use crate::sky_render::{LevelSky, ShellRotation, SkyCamera, SkyShellEntity, SKY_LAYER};
use crate::tfrag_render::game_to_bevy;
use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;
use bevy::render::extract_resource::{ExtractResource, ExtractResourcePlugin};
use rc_formats::texture::Texture;
use std::f32::consts::{FRAC_PI_2, PI, TAU};

/// Map columns (longitude) and rows (latitude): 1° cells.
pub const W: usize = 360;
pub const H: usize = 180;
/// The lowest sky looked at (radians above the horizon): the domes' pale band under the horizon (Novalis, Kerwan,
/// Eudora, Pokitaru, Kalebo III) is haze, not sun.
const MIN_LAT: f32 = 3.0 * PI / 180.0;
/// The sun's peak: at least this linear luminance, and this far above the sky's median.
const MIN_PEAK: f32 = 0.3;
const MIN_CONTRAST: f32 = 0.15;
/// The bright spot (at least [`SPOT_LEVEL`] of the peak's rise above its row): within [`SPOT_RADIUS`] of the peak,
/// at most [`MAX_SPOT_AREA`] steradians, and at least [`MIN_SPOT_SHARE`] of all such sky above [`MIN_LAT`].
const SPOT_LEVEL: f32 = 0.7;
const SPOT_RADIUS: f32 = 40.0 * PI / 180.0;
const MAX_SPOT_AREA: f32 = 1.2;
const MIN_SPOT_SHARE: f32 = 0.55;
/// The least of the sun shell's light the whole sky shows at the sun.
const MIN_SHOWN: f32 = 0.8;

/// One sky face for the map: positions (Bevy axes, the shell's SkyM applied), the GS vertex RGBA (0x80 = 1.0)
/// and, textured, ST and the decoded texture (`texture::scale_alpha`'d alpha, crate::sky_render::sky_image).
#[derive(Clone, Copy)]
pub struct Face<'a> {
    pub pos: [Vec3; 3],
    pub rgba: [[u8; 4]; 3],
    pub st: [[f32; 2]; 3],
    pub texture: Option<&'a Texture>,
}

/// The sky as drawn: linear luminance and display colour per cell.
pub struct SkyMap {
    pub lum: Vec<f32>,
    pub rgb: Vec<[f32; 3]>,
    /// Each shell's own layer: premultiplied display colour and coverage.
    pub layers: Vec<Vec<[f32; 4]>>,
}

/// A found sun: direction (world, the map's SkyM), the shell carrying it, its peak and the sky's median luminance
/// (linear), the bright spot's angular radius.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sun {
    pub dir: Vec3,
    pub shell: usize,
    pub peak: f32,
    pub median: f32,
    pub radius: f32,
}

impl Sun {
    /// The rays' strength, 0.5..1: how far the peak stands above the sky.
    pub fn strength(&self) -> f32 { (0.5 + (self.peak - self.median)).clamp(0.5, 1.0) }
}

/// What [`find_sun`] measured on the shell it chose (for the log), and the sun if the sky has one.
#[derive(Clone, Copy, Debug)]
pub struct Verdict {
    pub shell: usize,
    pub peak_dir: Vec3,
    pub peak: f32,
    pub median: f32,
    pub spot_area: f32,
    pub spot_share: f32,
    pub sun: Option<Sun>,
    /// Why there is none.
    pub reason: &'static str,
}

/// The centre direction of cell (column `i`, row `j`): row 0 at the zenith, column 0 at longitude −π.
pub fn cell_dir(i: usize, j: usize) -> Vec3 {
    let lon = (i as f32 + 0.5) / W as f32 * TAU - PI;
    let lat = FRAC_PI_2 - (j as f32 + 0.5) / H as f32 * PI;
    Vec3::new(lat.cos() * lon.cos(), lat.sin(), lat.cos() * lon.sin())
}

fn lat_lon(d: Vec3) -> (f32, f32) { (d.y.clamp(-1.0, 1.0).asin(), d.z.atan2(d.x)) }

fn srgb_to_linear(c: f32) -> f32 { if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) } }

fn luma(c: [f32; 3]) -> f32 { 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2] }

/// The face's colour and GS alpha (As / 0x80, at most 1) at barycentric `w`, as sky.wgsl computes them.
fn shade(f: &Face, w: [f32; 3]) -> ([f32; 3], f32) {
    let lerp = |k: usize| w[0] * f.rgba[0][k] as f32 + w[1] * f.rgba[1][k] as f32 + w[2] * f.rgba[2][k] as f32;
    let va = lerp(3);
    match f.texture {
        Some(t) => {
            let s = w[0] * f.st[0][0] + w[1] * f.st[1][0] + w[2] * f.st[2][0];
            let u = w[0] * f.st[0][1] + w[1] * f.st[1][1] + w[2] * f.st[2][1];
            let x = ((s.clamp(0.0, 1.0) * t.width as f32) as u32).min(t.width.saturating_sub(1));
            let y = ((u.clamp(0.0, 1.0) * t.height as f32) as u32).min(t.height.saturating_sub(1));
            let px = &t.rgba[((y * t.width + x) * 4) as usize..][..4];
            let ta = if px[3] == 0xff { 0x80 } else { px[3] / 2 } as f32;
            ([px[0] as f32 / 255.0, px[1] as f32 / 255.0, px[2] as f32 / 255.0], (ta * va / 128.0).min(128.0) / 128.0)
        }
        None => ([lerp(0) / 255.0, lerp(1) / 255.0, lerp(2) / 255.0], (va / 128.0).min(1.0)),
    }
}

/// Blends one face over a shell's premultiplied layer (display colour, coverage), in the cells whose centre it covers.
fn raster(f: &Face, layer: &mut [[f32; 4]]) {
    let [p0, p1, p2] = f.pos;
    let (n0, n1, n2) = (p1.cross(p2), p2.cross(p0), p0.cross(p1));
    let det = p0.dot(n0);
    if det.abs() < 1e-9 { return; }
    let inside = |d: Vec3| -> Option<[f32; 3]> {
        let w = [d.dot(n0) / det, d.dot(n1) / det, d.dot(n2) / det];
        (w.iter().all(|&x| x >= 0.0) && d.dot(p0 + p1 + p2) > 0.0).then(|| {
            let s = w[0] + w[1] + w[2];
            [w[0] / s, w[1] / s, w[2] / s]
        })
    };
    // Bounds from points along the edges (great-circle edges bulge in latitude past the corners).
    let (lat0, lon0) = lat_lon(f.pos[0].normalize_or_zero());
    let (mut lat_lo, mut lat_hi, mut lon_lo, mut lon_hi) = (lat0, lat0, 0.0f32, 0.0f32);
    for e in 0..3 {
        for k in 0..8 {
            let q = f.pos[e].lerp(f.pos[(e + 1) % 3], k as f32 / 8.0).normalize_or_zero();
            let (lat, lon) = lat_lon(q);
            let dl = (lon - lon0 + PI).rem_euclid(TAU) - PI;
            (lat_lo, lat_hi, lon_lo, lon_hi) = (lat_lo.min(lat), lat_hi.max(lat), lon_lo.min(dl), lon_hi.max(dl));
        }
    }
    let mut full_lon = lon_hi - lon_lo > PI;
    if inside(Vec3::Y).is_some() { lat_hi = FRAC_PI_2; full_lon = true; }
    if inside(Vec3::NEG_Y).is_some() { lat_lo = -FRAC_PI_2; full_lon = true; }
    let row = |lat: f32| ((FRAC_PI_2 - lat) / PI * H as f32).floor() as i64;
    let (j0, j1) = ((row(lat_hi) - 1).max(0) as usize, (row(lat_lo) + 1).min(H as i64 - 1) as usize);
    let col = |lon: f32| ((lon + PI) / TAU * W as f32).floor() as i64;
    let (i0, i1) = if full_lon { (0, W as i64 - 1) } else { (col(lon0 + lon_lo) - 1, col(lon0 + lon_hi) + 1) };
    for j in j0..=j1 {
        for i in i0..=i1 {
            let i = i.rem_euclid(W as i64) as usize;
            let Some(w) = inside(cell_dir(i, j)) else { continue };
            let (c, a) = shade(f, w);
            let l = &mut layer[j * W + i];
            for k in 0..3 { l[k] = c[k] * a + l[k] * (1.0 - a); }
            l[3] = a + l[3] * (1.0 - a);
        }
    }
}

/// Draws the shells in order over black: each shell's faces into its own layer, the layers blended in turn.
pub fn render(shells: &[Vec<Face>]) -> SkyMap {
    let n = W * H;
    let layers: Vec<Vec<[f32; 4]>> = shells
        .iter()
        .map(|faces| {
            let mut layer = vec![[0.0; 4]; n];
            for f in faces { raster(f, &mut layer); }
            layer
        })
        .collect();
    let mut rgb = vec![[0.0f32; 3]; n];
    for layer in &layers {
        for (f, c) in rgb.iter_mut().zip(layer) {
            for k in 0..3 { f[k] = c[k] + f[k] * (1.0 - c[3]); }
        }
    }
    let lum = rgb.iter().map(|c| luma([srgb_to_linear(c[0]), srgb_to_linear(c[1]), srgb_to_linear(c[2])])).collect();
    SkyMap { lum, rgb, layers }
}

/// The rows at or above [`MIN_LAT`].
fn sky_rows() -> usize { (((FRAC_PI_2 - MIN_LAT) / PI * H as f32) as usize).min(H) }

/// Mean of the 3×3 cells around (`i`, `j`), so a lone bright texel (a painted star) does not make a peak.
fn smoothed(lum: &[f32], i: usize, j: usize) -> f32 {
    let mut s = 0.0;
    for dj in [j.saturating_sub(1), j, (j + 1).min(H - 1)] {
        for di in [W - 1, 0, 1] { s += lum[dj * W + (i + di) % W]; }
    }
    s / 9.0
}

/// A shell layer's own linear luminance (its premultiplied colour over black).
fn layer_lum(layer: &[[f32; 4]]) -> Vec<f32> {
    layer.iter().map(|c| luma([srgb_to_linear(c[0]), srgb_to_linear(c[1]), srgb_to_linear(c[2])])).collect()
}

/// Each row's median luminance (the sky at that elevation, all round).
fn row_medians(lum: &[f32]) -> Vec<f32> {
    lum.chunks(W)
        .map(|r| {
            let mut r = r.to_vec();
            r.sort_by(f32::total_cmp);
            r[W / 2]
        })
        .collect()
}

/// The cells above [`MIN_LAT`] whose excess is at least `threshold` within `radius` of `peak`: their area (sr), their
/// share of all such cells' area, and their excess-weighted direction sum.
fn spot_extent(excess: &[f32], peak: Vec3, threshold: f32, radius: f32) -> (f32, f32, Vec3) {
    let area = |j: usize| (FRAC_PI_2 - (j as f32 + 0.5) / H as f32 * PI).cos() * (TAU / W as f32) * (PI / H as f32);
    let (mut spot, mut bright, mut sum) = (0.0f32, 0.0f32, Vec3::ZERO);
    for j in 0..sky_rows() {
        for i in 0..W {
            let l = excess[j * W + i];
            if l < threshold { continue; }
            bright += area(j);
            let d = cell_dir(i, j);
            if d.angle_between(peak) > radius { continue; }
            spot += area(j);
            sum += d * (l - threshold + 1e-3) * area(j);
        }
    }
    (spot, if bright > 0.0 { spot / bright } else { 0.0 }, sum)
}

/// One luminance map's brightest spot above [`MIN_LAT`] and why it is no sun ("" when it is one). A spot is measured
/// by how far it stands above its own row's median, so a band as bright all round (the horizon haze, a cloud layer
/// at one height) is no spot.
fn spot(lum: &[f32]) -> (Verdict, &'static str) {
    let rows = sky_rows();
    let medians = row_medians(lum);
    let excess: Vec<f32> = lum.iter().enumerate().map(|(k, &l)| (l - medians[k / W]).max(0.0)).collect();
    let mut peak = (0.0f32, 0usize, 0usize);
    for j in 0..rows {
        for i in 0..W {
            let s = smoothed(&excess, i, j);
            if s > peak.0 { peak = (s, i, j); }
        }
    }
    let (rise, peak_lum, median) = (peak.0, smoothed(lum, peak.1, peak.2), medians[peak.2]);
    let peak_dir = cell_dir(peak.1, peak.2);
    let (spot_area, spot_share, sum) = spot_extent(&excess, peak_dir, SPOT_LEVEL * rise, SPOT_RADIUS);
    let reason = if peak_lum < MIN_PEAK {
        "no bright point"
    } else if rise < MIN_CONTRAST {
        "no contrast"
    } else if spot_area > MAX_SPOT_AREA || spot_share < MIN_SPOT_SHARE {
        "bright area not compact"
    } else {
        ""
    };
    let sun = Sun { dir: sum.normalize_or(peak_dir), shell: 0, peak: peak_lum, median, radius: (spot_area / PI).sqrt() };
    (Verdict { shell: 0, peak_dir, peak: peak_lum, median, spot_area, spot_share, sun: reason.is_empty().then_some(sun), reason }, reason)
}

/// The map's sun, if the sky paints one (module doc): the brightest spot that passes in any one shell's own layer and
/// that the shells drawn after it do not hide (the whole sky keeps at least [`MIN_SHOWN`] of the layer's light there).
pub fn find_sun(map: &SkyMap) -> Verdict {
    let mut best: Option<Verdict> = None;
    let mut best_lum = Vec::new();
    for (s, layer) in map.layers.iter().enumerate() {
        let lum = layer_lum(layer);
        let (mut v, _) = spot(&lum);
        v.shell = s;
        if let Some(sun) = v.sun.as_mut() { sun.shell = s; }
        let better = match &best {
            None => true,
            Some(b) => (v.sun.is_some(), v.peak) > (b.sun.is_some(), b.peak),
        };
        if better { (best, best_lum) = (Some(v), lum); }
    }
    let mut v = best.unwrap_or_else(|| spot(&map.lum).0);
    if let Some(sun) = v.sun {
        let (lat, lon) = lat_lon(sun.dir);
        let i = (((lon + PI) / TAU * W as f32) as usize).min(W - 1);
        let j = (((FRAC_PI_2 - lat) / PI * H as f32) as usize).min(H - 1);
        if smoothed(&map.lum, i, j) < MIN_SHOWN * smoothed(&best_lum, i, j) { (v.sun, v.reason) = (None, "hidden by the shells above"); }
    }
    v
}

/// The level sky's faces, each shell at its SkyM on the first tick (crate::sky_render::spawn_sky).
pub fn faces<'a>(ls: &'a LevelSky, rotations: &[ShellRotation]) -> Vec<Vec<Face<'a>>> {
    ls.sky
        .shells
        .iter()
        .enumerate()
        .map(|(si, shell)| {
            let rot = rotations.iter().find(|r| r.shell == si).map_or(Quat::IDENTITY, |r| r.rotation(1));
            let mut out = Vec::new();
            for c in &shell.clusters {
                let v = rc_formats::sky::sky_gs_vertices(shell, c);
                for t in v.as_chunks::<3>().0 {
                    let texture = if shell.textured() {
                        let Some(tex) = ls.textures.iter().find(|x| x.index == t[0].texture as usize) else { continue };
                        Some(&tex.texture)
                    } else {
                        None
                    };
                    out.push(Face {
                        pos: t.each_ref().map(|v| rot * game_to_bevy(v.position.map(|x| x as f32))),
                        rgba: t.each_ref().map(|v| v.rgba),
                        st: t.each_ref().map(|v| v.st),
                        texture,
                    });
                }
            }
            out
        })
        .collect()
}

/// The level's sky sun: its direction in the frame of the shell carrying it (before SkyM).
#[derive(Resource, Clone, Copy)]
struct LevelSun {
    sun: Sun,
    local: Vec3,
}

/// Toward the sky's sun (world, unit) this frame, `None` when the sky shows no sun; the rest as [`Sun`]'s.
#[derive(Resource, Clone, Copy, Default, PartialEq, ExtractResource)]
pub struct SkySun {
    pub to_sun: Option<Vec3>,
    pub strength: f32,
    pub radius: f32,
    pub peak: f32,
    pub median: f32,
}

pub struct SkySunPlugin;

impl Plugin for SkySunPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SkySun>()
            .add_plugins(ExtractResourcePlugin::<SkySun>::default())
            .add_systems(crate::level_switch::LevelStartup, detect)
            .add_systems(crate::level_switch::LevelUnload, crate::level_switch::remove::<LevelSun>)
            .add_systems(PostUpdate, follow.in_set(SkySunSet).before(bevy::transform::TransformSystems::Propagate));
    }
}

/// The `PostUpdate` set that writes [`SkySun`].
#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SkySunSet;

fn log_level() -> u32 { std::env::var("RC_SUN_RAYS_LOG").ok().and_then(|v| v.trim().parse().ok()).unwrap_or(0) }

/// [`LevelSun`] from the level's sky.
fn detect(mut commands: Commands, level: Res<crate::Level>) {
    let Some(ls) = &level.0.sky else { return };
    let start = std::time::Instant::now();
    let rotations = crate::sky_render::level_rotations(ls.level);
    let map = render(&faces(ls, &rotations));
    let v = find_sun(&map);
    let log = log_level();
    if let Some(s) = v.sun {
        let rot = rotations.iter().find(|r| r.shell == s.shell).map_or(Quat::IDENTITY, |r| r.rotation(1));
        commands.insert_resource(LevelSun { sun: s, local: rot.inverse() * s.dir });
    }
    if log == 0 { return; }
    println!("{} ({} ms)", describe(ls.level, &v, &rotations), start.elapsed().as_millis());
    // A view from the spawn toward the sun (game axes: Bevy (x, y, z) is game (x, −z, y)).
    if let (Some(s), Some((_, eye))) = (v.sun, crate::fly_cam::spawn_view(&level.0)) {
        let t = eye + Vec3::new(s.dir.x, -s.dir.z, s.dir.y) * 50.0;
        println!("sky sun: facing it from the spawn: RC_CAM={:.1},{:.1},{:.1},{:.1},{:.1},{:.1}", eye.x, eye.y, eye.z, t.x, t.y, t.z);
    }
    if log >= 2 { dump(&map, v.sun.map(|s| s.dir), ls.level, std::path::Path::new("work/fx")); }
}

/// The `RC_SUN_RAYS_LOG` line of a level's verdict.
fn describe(level: u32, v: &Verdict, rotations: &[ShellRotation]) -> String {
    let deg = |d: Vec3| {
        let (lat, lon) = lat_lon(d);
        format!("elevation {:.1}° bearing {:.1}°", lat.to_degrees(), lon.to_degrees())
    };
    match v.sun {
        Some(s) => format!(
            "sky sun: level {level} found on shell {}{}: dir ({:.3}, {:.3}, {:.3}) {}, radius {:.1}°, peak {:.2} median {:.3}, strength {:.2}",
            s.shell, if rotations.iter().any(|r| r.shell == s.shell) { " (turning)" } else { "" },
            s.dir.x, s.dir.y, s.dir.z, deg(s.dir), s.radius.to_degrees(), s.peak, s.median, s.strength()
        ),
        None => format!(
            "sky sun: level {level} none ({}): brightest on shell {}: {:.2} at {}, median {:.3}, spot {:.2} sr ({:.0}% of the bright sky)",
            v.reason, v.shell, v.peak, deg(v.peak_dir), v.median, v.spot_area, v.spot_share * 100.0
        ),
    }
}

/// `RC_SUN_RAYS_LOG=2`: the map as `<dir>/sky_sun_<level>.png`, each shell's own layer (over black) below it.
fn dump(map: &SkyMap, sun: Option<Vec3>, level: u32, dir: &std::path::Path) {
    let byte = |x: f32| (x.clamp(0.0, 1.0) * 255.0) as u8;
    let mut px: Vec<u8> = map.rgb.iter().flat_map(|c| [byte(c[0]), byte(c[1]), byte(c[2]), 255]).collect();
    if let Some(d) = sun {
        for j in 0..H {
            for i in 0..W {
                let a = cell_dir(i, j).angle_between(d).to_degrees();
                if (2.5..3.5).contains(&a) { px[(j * W + i) * 4..][..3].copy_from_slice(&[255, 0, 0]); }
            }
        }
    }
    for layer in &map.layers { px.extend(layer.iter().flat_map(|c| [byte(c[0]), byte(c[1]), byte(c[2]), 255])); }
    let img = Image::new(
        bevy::render::render_resource::Extent3d { width: W as u32, height: (H * (1 + map.layers.len())) as u32, depth_or_array_layers: 1 },
        bevy::render::render_resource::TextureDimension::D2,
        px,
        bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
        bevy::asset::RenderAssetUsages::MAIN_WORLD,
    );
    let _ = std::fs::create_dir_all(dir);
    let path = dir.join(format!("sky_sun_{level:02}.png"));
    match img.try_into_dynamic().map_err(|e| e.to_string()).and_then(|i| i.to_rgb8().save(&path).map_err(|e| e.to_string())) {
        Ok(()) => println!("sky sun: map written to {}", path.display()),
        Err(e) => eprintln!("sky sun: cannot write {}: {e}", path.display()),
    }
}

/// [`SkySun`] this frame: the level sun turned with its shell, while the sky camera shows the level sky
/// (crate::sun_shadow's light follows it: [`SkySunSet`]).
fn follow(
    sun: Option<Res<LevelSun>>,
    shells: Query<(&SkyShellEntity, &Transform)>,
    cams: Query<&RenderLayers, With<SkyCamera>>,
    mut out: ResMut<SkySun>,
) {
    let level_sky = cams.iter().next().is_some_and(|l| *l == RenderLayers::layer(SKY_LAYER));
    let want = match sun.filter(|_| level_sky) {
        Some(l) => {
            let s = l.sun;
            let rot = shells.iter().find(|(e, _)| e.shell == s.shell).map_or(Quat::IDENTITY, |(_, t)| t.rotation);
            SkySun { to_sun: Some((rot * l.local).normalize()), strength: s.strength(), radius: s.radius, peak: s.peak, median: s.median }
        }
        None => SkySun::default(),
    };
    if *out != want { *out = want; }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An octahedron's eight faces (a whole sphere) in one gouraud colour.
    fn dome(rgba: [u8; 4]) -> Vec<Face<'static>> {
        let ax = [Vec3::X, Vec3::Z, Vec3::NEG_X, Vec3::NEG_Z];
        let mut out = Vec::new();
        for k in 0..4 {
            for pole in [Vec3::Y, Vec3::NEG_Y] {
                out.push(Face { pos: [ax[k] * 1000.0, ax[(k + 1) % 4] * 1000.0, pole * 1000.0], rgba: [rgba; 3], st: [[0.0; 2]; 3], texture: None });
            }
        }
        out
    }

    /// A textured quad facing `dir` of half-size `half` (sky units at distance 1000), its texture a white disc.
    fn sun_quad(dir: Vec3, half: f32, tex: &Texture) -> Vec<Face<'_>> {
        let right = dir.cross(Vec3::Y).normalize();
        let up = right.cross(dir);
        let c = |x: f32, y: f32| dir * 1000.0 + (right * x + up * y) * half;
        let (a, b, cc, d) = (c(-1.0, -1.0), c(1.0, -1.0), c(1.0, 1.0), c(-1.0, 1.0));
        let st = |x: f32, y: f32| [(x + 1.0) / 2.0, (y + 1.0) / 2.0];
        let v = [0x80, 0x80, 0x80, 0x80];
        vec![
            Face { pos: [a, b, cc], rgba: [v; 3], st: [st(-1.0, -1.0), st(1.0, -1.0), st(1.0, 1.0)], texture: Some(tex) },
            Face { pos: [a, cc, d], rgba: [v; 3], st: [st(-1.0, -1.0), st(1.0, 1.0), st(-1.0, 1.0)], texture: Some(tex) },
        ]
    }

    fn disc_texture() -> Texture {
        let n = 32u32;
        let mut rgba = Vec::new();
        for y in 0..n {
            for x in 0..n {
                let r = ((x as f32 + 0.5 - 16.0).powi(2) + (y as f32 + 0.5 - 16.0).powi(2)).sqrt() / 16.0;
                rgba.extend_from_slice(&[255, 250, 230, if r < 1.0 { 0xff } else { 0 }]);
            }
        }
        Texture { width: n, height: n, rgba }
    }

    /// The level's sky from the game data, None without it.
    fn level_sky(level: u32) -> Option<LevelSky> {
        let root = crate::level_load::extracted_root();
        let index = crate::disc_source::level_file(&root, level, "core_index.bin").ok()?;
        let data = rc_data::level_core_data(&root, level).ok()?;
        let core = rc_formats::level::parse_level_core(&index, data.len()).ok()?;
        crate::sky_render::load(&core, &data, level).ok().flatten()
    }

    /// Survey (`--ignored --nocapture`): every level's verdict, the maps into `<repo>/work/fx/`.
    #[test]
    #[ignore]
    fn survey_every_level() {
        let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../work/fx");
        for level in 0..19 {
            let Some(ls) = level_sky(level) else { println!("level {level}: no data"); continue };
            let rotations = crate::sky_render::level_rotations(level);
            let map = render(&faces(&ls, &rotations));
            let v = find_sun(&map);
            println!("{}", describe(level, &v, &rotations));
            for (s, layer) in map.layers.iter().enumerate() {
                let lum = layer_lum(layer);
                let (v, reason) = spot(&lum);
                let (lat, lon) = lat_lon(v.peak_dir);
                let m = row_medians(&lum);
                let excess: Vec<f32> = lum.iter().enumerate().map(|(k, &l)| (l - m[k / W]).max(0.0)).collect();
                let rise = (0..sky_rows()).flat_map(|j| (0..W).map(move |i| (i, j))).map(|(i, j)| smoothed(&excess, i, j)).fold(0.0, f32::max);
                let shares: Vec<String> = [(0.5, 40.0), (0.7, 40.0), (0.5, 60.0), (0.7, 60.0)]
                    .iter()
                    .map(|&(t, r): &(f32, f32)| format!("{:.0}", spot_extent(&excess, v.peak_dir, t * rise, r.to_radians()).1 * 100.0))
                    .collect();
                println!(
                    "  shell {s}: peak {:.2} rise {rise:.2} at ({:.1}°, {:.1}°) row median {:.3} spot {:.2} sr {:.0}% [shares {}] {reason}",
                    v.peak, lat.to_degrees(), lon.to_degrees(), v.median, v.spot_area, v.spot_share * 100.0, shares.join("/")
                );
            }
            dump(&map, v.sun.map(|s| s.dir), level, &out);
        }
    }

    #[test]
    fn cell_dir_round_trips() {
        for (i, j) in [(0, 0), (90, 45), (359, 179), (200, 90)] {
            let (lat, lon) = lat_lon(cell_dir(i, j));
            let ii = ((lon + PI) / TAU * W as f32).floor() as usize;
            let jj = ((FRAC_PI_2 - lat) / PI * H as f32).floor() as usize;
            assert_eq!((ii, jj), (i, j));
        }
    }

    #[test]
    fn dome_covers_the_sphere() {
        let map = render(&[dome([0x40, 0x60, 0x90, 0x80])]);
        assert!(map.rgb.iter().all(|c| (c[2] - 0x90 as f32 / 255.0).abs() < 1e-4), "holes in the dome");
        let v = find_sun(&map);
        assert!(v.sun.is_none());
        assert_eq!(v.reason, "no bright point");
    }

    #[test]
    fn finds_a_painted_sun_on_its_shell() {
        let tex = disc_texture();
        let dir = Vec3::new(0.6, 0.5, -0.4).normalize();
        let map = render(&[dome([0x50, 0x80, 0xd0, 0x80]), sun_quad(dir, 120.0, &tex)]);
        let v = find_sun(&map);
        let s = v.sun.unwrap_or_else(|| panic!("{v:?}"));
        assert!(s.dir.angle_between(dir) < 1.5f32.to_radians(), "{:?} vs {dir}", s.dir);
        assert_eq!(s.shell, 1);
        assert!(s.radius > 3f32.to_radians() && s.radius < 10f32.to_radians(), "{}", s.radius.to_degrees());
        assert!(s.strength() > 0.5);
    }

    #[test]
    fn a_sun_below_the_horizon_or_in_a_night_sky_is_none() {
        let tex = disc_texture();
        let low = Vec3::new(0.6, -0.4, -0.4).normalize();
        let v = find_sun(&render(&[dome([0x50, 0x80, 0xd0, 0x80]), sun_quad(low, 120.0, &tex)]));
        assert!(v.sun.is_none(), "{v:?}");
        // A night sky's dim moon.
        let v = find_sun(&render(&[dome([0x02, 0x02, 0x08, 0x80]), dim_disc(Vec3::new(0.0, 0.6, -0.8), &tex)]));
        assert_eq!(v.reason, "no bright point", "{v:?}");
    }

    /// The disc at half alpha in grey (0x80 alpha, vertex alpha 0x40).
    fn dim_disc(dir: Vec3, tex: &Texture) -> Vec<Face<'_>> {
        sun_quad(dir, 120.0, tex).into_iter().map(|mut f| { f.rgba = [[0x80, 0x80, 0x80, 0x30]; 3]; f }).collect()
    }

    /// A sun glow painted in a dome's vertex colours (no disc), with white cloud bands on another shell: the glow is
    /// the sun.
    #[test]
    fn finds_a_dome_glow_under_brighter_clouds() {
        let glow = Vec3::new(0.0, 0.45, -1.0).normalize();
        let mut sky = dome([0x48, 0x78, 0xc0, 0x80]);
        // A fan of faces around the glow: bright centre, the dome's blue at the rim.
        let right = glow.cross(Vec3::Y).normalize();
        let up = right.cross(glow);
        for k in 0..12 {
            let a = |k: usize| (k as f32 / 12.0) * TAU;
            let rim = |t: f32| (glow + (right * t.cos() + up * t.sin()) * 0.45) * 1000.0;
            sky.push(Face {
                pos: [glow * 1000.0, rim(a(k)), rim(a(k + 1))],
                rgba: [[0xff, 0xff, 0xf0, 0x80], [0x48, 0x78, 0xc0, 0x80], [0x48, 0x78, 0xc0, 0x80]],
                st: [[0.0; 2]; 3],
                texture: None,
            });
        }
        let mut clouds = Vec::new();
        for k in 0..36 {
            let at = |t: f32, y: f32| Vec3::new(t.cos(), y, t.sin()) * 1000.0;
            let (t0, t1) = (k as f32 * TAU / 36.0, (k as f32 + 0.6) * TAU / 36.0);
            let white = [[0xff, 0xff, 0xff, 0x80]; 3];
            clouds.push(Face { pos: [at(t0, 0.02), at(t1, 0.02), at(t1, 0.2)], rgba: white, st: [[0.0; 2]; 3], texture: None });
            clouds.push(Face { pos: [at(t0, 0.02), at(t1, 0.2), at(t0, 0.2)], rgba: white, st: [[0.0; 2]; 3], texture: None });
        }
        let v = find_sun(&render(&[sky, clouds]));
        let s = v.sun.unwrap_or_else(|| panic!("{v:?}"));
        assert_eq!(s.shell, 0);
        assert!(s.dir.angle_between(glow) < 4f32.to_radians(), "{:?} vs {glow}", s.dir);
    }

    #[test]
    fn an_overcast_sky_has_no_sun() {
        let tex = disc_texture();
        let dir = Vec3::new(0.0, 0.6, -0.8).normalize();
        // A near-white dome with a slightly brighter disc on it.
        let v = find_sun(&render(&[dome([0xf0, 0xf0, 0xf0, 0x80]), dim_disc(dir, &tex)]));
        assert!(v.sun.is_none(), "{v:?}");
        // A sun under an opaque grey shell drawn after it.
        let v = find_sun(&render(&[dome([0x50, 0x80, 0xd0, 0x80]), sun_quad(dir, 120.0, &tex), dome([0x70, 0x70, 0x78, 0x80])]));
        assert_eq!(v.reason, "hidden by the shells above", "{v:?}");
    }

    /// A sun on a turning shell: found at its first tick's SkyM, kept in the shell's frame ([`LevelSun::local`]) and
    /// turned with a later SkyM ([`follow`]), it lands where the sky drawn at that later tick shows it.
    #[test]
    fn a_turning_shell_carries_its_sun() {
        let tex = disc_texture();
        let local = Vec3::new(0.3, 0.5, -0.8).normalize();
        let r = ShellRotation { shell: 1, mul: 1, period: 0x8000, ry: 0.2 };
        let at = |counter: u64| {
            let rot = r.rotation(counter);
            let faces: Vec<Face> = sun_quad(local, 120.0, &tex).into_iter().map(|mut f| { f.pos = f.pos.map(|p| rot * p); f }).collect();
            (rot, find_sun(&render(&[dome([0x50, 0x80, 0xd0, 0x80]), faces])).sun.unwrap_or_else(|| panic!("tick {counter}")))
        };
        let (rot1, first) = at(1);
        assert_eq!(first.shell, 1);
        let kept = rot1.inverse() * first.dir;
        assert!(kept.angle_between(local) < 1.5f32.to_radians(), "{kept} vs {local}");
        let (rot2, later) = at(0x2000);
        assert!(first.dir.angle_between(later.dir) > 30f32.to_radians(), "the shell barely turned");
        assert!((rot2 * kept).angle_between(later.dir) < 1.5f32.to_radians(), "{} vs {}", rot2 * kept, later.dir);
    }

    #[test]
    fn a_covered_shell_does_not_own_the_sun() {
        let tex = disc_texture();
        let dir = Vec3::new(0.0, 0.7, -0.7).normalize();
        // The sun drawn on shell 1, a thin haze (shell 2) over everything.
        let map = render(&[dome([0x50, 0x80, 0xd0, 0x80]), sun_quad(dir, 150.0, &tex), dome([0xa0, 0xa0, 0xa0, 0x18])]);
        let v = find_sun(&map);
        assert_eq!(v.sun.map(|s| s.shell), Some(1), "{v:?}");
    }
}
