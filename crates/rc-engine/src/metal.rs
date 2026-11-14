//! **Port-only: shiny metal** (Lighting Enhanced, docs/plan/graphics_options.md "Shiny metal"). The textures listed in
//! [`METAL`] draw with a reflection of the sky and a highlight from the key light (`metal_look.wgsl`) while
//! [`crate::graphics::Lighting::enhanced`]. The tfrag and tie materials of a listed texture take their `WORLD_METAL`
//! pipeline variant (crate::tfrag_render `TfragKey`, crate::tie_render `TieKey`); every other pipeline, and these in
//! Original or Simple, stays the shader it was. A moby texture's shine rides in its vertices (crate::moby_render
//! `SKIN_SHINE_SHIFT`) and moby.wgsl draws it while the environment's switch is on (the moby material is left exactly as
//! it was: even an unused field on it changed the Original frame).
//!
//! Which textures are metal is a curated list per level, as crate::foliage::plan's `NOT_TURF`: no colour rule tells grey
//! paint, stone or concrete from steel. Tfrag and tie textures are the level's table indices (`rerac-extract export`
//! names them `levels/NN/tfrag/NNN_*`, `levels/NN/tie/NNN_*`); a moby texture is (class, the class's own texture slot),
//! the same on every level that has the class ([`ANY_LEVEL`]).
//!
//! The environment ([`MetalEnv`]) rides in the shared fog buffer after the fog (crate::game_camera `FOG_PREFIX`): the
//! key light (crate::sun_shadow::KeyLight) and the sky probe (crate::sky_sun::SkyProbe). `RC_METAL=0` turns the look off,
//! `RC_METAL=all` (testing) draws every tfrag, tie and moby texture as metal, `RC_METAL_LOG=1` prints the listed
//! textures each level resolves.

use crate::graphics::GraphicsSettings;
use bevy::prelude::*;
use std::sync::atomic::{AtomicBool, Ordering};

/// How much a texture shines (`metal_look.wgsl`'s `k`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Shine {
    /// Painted or dirty metal: a soft sheen.
    Painted = 1,
    /// Bare steel and machinery.
    Bare = 2,
    /// Chrome and polished plating.
    Chrome = 3,
}

impl Shine {
    /// The shader's `k`.
    pub fn strength(self) -> f32 {
        match self {
            Shine::Painted => 0.35,
            Shine::Bare => 0.6,
            Shine::Chrome => 0.9,
        }
    }
}

/// A level texture: a tfrag one (`tfrag_lod.mips` index), a tie one (the tie texture table) or a moby class's texture
/// slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MetalTex {
    Tfrag(u32),
    Tie(usize),
    Moby { class: i32, slot: i32 },
}

/// A [`METAL`] set for every level that has the moby class.
pub const ANY_LEVEL: u32 = u32::MAX;

/// Which table a [`MetalSet`]'s ids index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Tfrag,
    Tie,
    /// The texture slots of a moby class.
    Moby(i32),
}

/// Textures of one level and table that shine alike.
pub struct MetalSet {
    pub level: u32,
    pub kind: Kind,
    pub shine: Shine,
    pub ids: &'static [u32],
    pub what: &'static str,
}

const fn set(level: u32, kind: Kind, shine: Shine, ids: &'static [u32], what: &'static str) -> MetalSet { MetalSet { level, kind, shine, ids, what } }

use Kind::{Moby, Tfrag, Tie};
use Shine::{Bare, Chrome, Painted};

/// The metal textures, chosen from the exported textures (`rerac-extract export`) and checked on screen. Left out:
/// mottled grey that may be stone or concrete, glowing panels and screens, wood, rock and plants.
pub const METAL: &[MetalSet] = &[
    // Every level: Ratchet's ship (rc_game::travel: classes 531 / 532 / 533, one hull texture from the spaceships file;
    // the canopy is the game's own glass pass).
    set(ANY_LEVEL, Moby(531), Bare, &[0], "Ratchet's ship: the hull"),
    set(ANY_LEVEL, Moby(532), Bare, &[0], "Ratchet's ship: the hull"),
    set(ANY_LEVEL, Moby(533), Bare, &[0], "Ratchet's ship: the hull"),
    // Robots (the class's texture slots, wherever the class appears; seen on Kerwan's moby textures 96..101).
    set(ANY_LEVEL, Moby(573), Bare, &[0], "robot class 573: its grey armour"),
    set(ANY_LEVEL, Moby(574), Bare, &[1], "robot class 574: its metal parts (slot 0 is the red body paint)"),
    set(ANY_LEVEL, Moby(578), Bare, &[0, 1], "robot class 578: its plated body and joints"),
    // 03 Kerwan: the city's steel panels, vents and grates, its brass trim, the yellow-painted plates.
    set(3, Tfrag, Bare, &[6, 7, 10, 11, 42, 43, 46, 47, 48, 49, 50, 53, 54, 55, 56, 57, 62, 64], "Kerwan: steel panels, vents, grates, tread plate"),
    set(3, Tfrag, Chrome, &[21, 32, 36, 37], "Kerwan: brass plates and grates"),
    set(3, Tfrag, Painted, &[31, 58, 59, 60, 61, 63, 65], "Kerwan: teal and yellow painted plates"),
    set(3, Tie, Chrome, &[0, 4, 10, 13, 24, 106], "Kerwan: brass trim, polished steel cylinders"),
    set(3, Tie, Bare, &[2, 3, 12, 25, 29, 32, 34, 36, 37, 38, 41, 45, 48, 51, 53, 55, 59, 61, 62, 63, 64, 65, 66, 67, 68, 71, 74, 87, 89, 90, 99, 107], "Kerwan: steel panels, pipes, vents, copper bands, metal crates"),
    set(3, Tie, Painted, &[8, 31, 49, 50, 69, 86, 88], "Kerwan: teal, blue and yellow painted metal"),
    // 13 Gemlik Base: a space station, metal throughout; its lights, screens and the asteroid rock left out.
    set(13, Tfrag, Chrome, &[1, 30, 59, 63, 74, 79, 87], "Gemlik: polished steel bands and cylinders"),
    set(13, Tfrag, Painted, &[0, 2, 3, 4, 8, 16, 18, 19, 20, 32, 33, 34, 48, 53, 56, 57, 69, 70, 75, 85], "Gemlik: white and lilac painted panels"),
    set(13, Tfrag, Bare, &[6, 7, 9, 10, 11, 12, 14, 17, 21, 22, 23, 24, 26, 27, 28, 29, 31, 35, 36, 37, 38, 40, 41, 42, 43, 44, 45, 46, 47, 49, 50, 51, 52, 55, 58, 61, 62, 64, 65, 66, 67, 72, 73, 76, 77, 80, 82, 83, 86, 88], "Gemlik: steel floors, walls, vents"),
    set(13, Tie, Chrome, &[0, 3, 13, 14, 16, 18, 34, 35, 44, 54, 56, 57, 58, 61, 111], "Gemlik: polished steel and gold bands"),
    set(13, Tie, Painted, &[1, 6, 9, 10, 11, 21, 23, 24, 26, 28, 29, 49, 59, 73, 74, 77, 83, 95, 112, 113, 119, 121, 125, 130, 131, 134], "Gemlik: white painted panels"),
    set(13, Tie, Bare, &[4, 5, 15, 17, 19, 20, 22, 25, 27, 31, 32, 33, 36, 37, 38, 39, 40, 41, 42, 43, 45, 46, 47, 50, 52, 53, 62, 63, 64, 65, 66, 68, 69, 70, 71, 72, 78, 79, 81, 84, 85, 86, 88, 89, 90, 93, 97, 98, 99, 100, 101, 102, 103, 107, 108, 109, 110, 115, 116, 117, 118, 120, 122, 123, 124, 126, 127, 128, 129, 133], "Gemlik: steel panels, pipes, vents, grilles"),
];

/// The shine of `tex` on `level` ([`METAL`]; every texture under `RC_METAL=all`).
pub fn shine(level: u32, tex: MetalTex) -> Option<Shine> {
    match env_mode() {
        Mode::Off => return None,
        Mode::All => return Some(Shine::Bare),
        Mode::Listed => {}
    }
    let (kind, id) = match tex {
        MetalTex::Tfrag(i) => (Tfrag, i),
        MetalTex::Tie(i) => (Tie, i as u32),
        MetalTex::Moby { class, slot } => (Moby(class), u32::try_from(slot).ok()?),
    };
    let e = METAL.iter().find(|e| (e.level == level || e.level == ANY_LEVEL) && e.kind == kind && e.ids.contains(&id))?;
    if log_on() { println!("metal: level {level:02} {tex:?}: {:?} ({})", e.shine, e.what); }
    Some(e.shine)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Listed,
    Off,
    All,
}

fn env_mode() -> Mode {
    static M: std::sync::OnceLock<Mode> = std::sync::OnceLock::new();
    *M.get_or_init(|| match std::env::var("RC_METAL").as_deref().map(str::trim) {
        Ok("0") => Mode::Off,
        Ok("all") => Mode::All,
        _ => Mode::Listed,
    })
}

/// `RC_METAL_LOG=1`.
pub fn log_on() -> bool { std::env::var("RC_METAL_LOG").is_ok_and(|v| v.trim() == "1") }

/// The look is on (Lighting Enhanced): read by the material builders for a new material ([`sync_metal`] updates the
/// existing ones).
static ON: AtomicBool = AtomicBool::new(false);

pub fn on() -> bool { ON.load(Ordering::Relaxed) }

/// Sky probe sectors around the horizon.
pub const SECTORS: usize = 8;
/// The probe's colours: [`SECTORS`] for each of the zenith, middle and horizon bands, then the ground.
pub const PROBE: usize = 3 * SECTORS + 1;

/// A sky probe: per band (zenith, middle, horizon) [`SECTORS`] display colours, sector k centred at longitude
/// (k + 0.5)·45° − 180° (crate::sky_sun `cell_dir`: Bevy x = cos, z = sin), then the ground.
pub type Probe = [Vec3; PROBE];

/// The environment `metal_look.wgsl` reads: 2 + [`PROBE`] vec4 after the fog in the shared fog buffer ([`write_env`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MetalEnv {
    /// Toward the key light (Bevy world); w = the highlight's strength.
    pub to_sun: Vec4,
    /// The highlight's colour (linear); w = 1 while the mobys draw their shine (Lighting Enhanced).
    pub sun: Vec4,
    /// The sky probe (display-encoded rgb).
    pub probe: Probe,
}

impl Default for MetalEnv {
    fn default() -> Self {
        let bands = [Vec3::new(0.45, 0.6, 0.85), Vec3::new(0.6, 0.7, 0.85), Vec3::new(0.75, 0.78, 0.8)];
        let mut probe = [Vec3::new(0.25, 0.24, 0.22); PROBE];
        for (b, c) in bands.iter().enumerate() { probe[b * SECTORS..(b + 1) * SECTORS].fill(*c); }
        MetalEnv { to_sun: Vec4::new(0.3, 0.8, 0.5, 1.0), sun: Vec4::new(1.0, 0.96, 0.88, 0.0), probe }
    }
}

impl MetalEnv {
    pub fn bytes(&self) -> Vec<u8> {
        let probe = self.probe.iter().map(|c| c.extend(0.0));
        [self.to_sun, self.sun].into_iter().chain(probe).flat_map(|v| v.to_array()).flat_map(|f| f.to_le_bytes()).collect()
    }
}

/// The sky probe from a sky map's display colours (crate::sky_sun::SkyMap: `rows` × `cols` cells, row 0 at the
/// zenith, column 0 at longitude −180°), averaged by cell area: the bands above 55°, 20..55° and 0..20° in [`SECTORS`]
/// sectors of longitude, and below −10° (the ground; a sky without shells there gives a dark copy of the horizon).
pub fn probe(rows: usize, cols: usize, rgb: &[[f32; 3]]) -> Probe {
    let mut sum = [Vec3::ZERO; PROBE];
    let mut w = [0.0f32; PROBE];
    for j in 0..rows {
        let lat = 90.0 - (j as f32 + 0.5) * 180.0 / rows as f32;
        let band = if lat > 55.0 { 0 } else if lat > 20.0 { 1 } else if lat >= 0.0 { 2 } else if lat < -10.0 { 3 } else { continue };
        let a = lat.to_radians().cos();
        for i in 0..cols {
            let k = if band == 3 { PROBE - 1 } else { band * SECTORS + i * SECTORS / cols };
            sum[k] += Vec3::from(rgb[j * cols + i]) * a;
            w[k] += a;
        }
    }
    let mut out = [Vec3::ZERO; PROBE];
    for k in 0..PROBE { if w[k] > 0.0 { out[k] = sum[k] / w[k]; } }
    if out[PROBE - 1].length() < 0.03 {
        let horizon = out[2 * SECTORS..3 * SECTORS].iter().sum::<Vec3>() / SECTORS as f32;
        out[PROBE - 1] = horizon * 0.35;
    }
    out
}

pub struct MetalPlugin;

/// Keeps `metal_look.wgsl` loaded (`#import rerac::metal_look`).
#[derive(Resource)]
struct LookShader(#[allow(dead_code)] Handle<Shader>);

impl Plugin for MetalPlugin {
    fn build(&self, app: &mut App) {
        let look = app.world().resource::<AssetServer>().load("shaders/metal_look.wgsl");
        app.insert_resource(LookShader(look))
            .add_systems(PostUpdate, (sync_metal, write_env.after(crate::sky_sun::SkySunSet).before(bevy::asset::AssetEventSystems)));
    }
}

/// [`ON`] from the Lighting option, and every listed material's pipeline variant with it.
fn sync_metal(
    g: Res<GraphicsSettings>,
    mut tfrags: ResMut<Assets<crate::tfrag_render::TfragMaterial>>,
    mut ties: ResMut<Assets<crate::tie_render::TieMaterial>>,
) {
    let want = g.lighting.enhanced() && env_mode() != Mode::Off;
    if ON.swap(want, Ordering::Relaxed) == want && !g.is_changed() { return; }
    let stale: Vec<_> = tfrags.iter().filter(|(_, m)| m.lod.misc.w != 0.0 && (m.lod.misc.w > 0.0) != want).map(|(id, _)| id).collect();
    for id in stale { if let Some(mut m) = tfrags.get_mut(id) { m.lod.misc.w = -m.lod.misc.w; } }
    let stale: Vec<_> = ties.iter().filter(|(_, m)| m.params.misc.w != 0.0 && (m.params.misc.w > 0.0) != want).map(|(id, _)| id).collect();
    for id in stale { if let Some(mut m) = ties.get_mut(id) { m.params.misc.w = -m.params.misc.w; } }
}

/// A listed material's signed shine for its uniform (tfrag `lod.misc.w`, tie `params.misc.w`): + on, − off, 0 none.
pub fn signed_strength(s: Option<Shine>) -> f32 {
    s.map_or(0.0, |s| if on() { s.strength() } else { -s.strength() })
}

/// The frame's [`MetalEnv`] from the key light and the sky probe, written after the fog in the shared fog buffer
/// (crate::game_camera `FOG_PREFIX`; only where it changed).
fn write_env(
    key: Option<Res<crate::sun_shadow::KeyLight>>,
    sky: Option<Res<crate::sky_sun::SkyProbe>>,
    fog: Res<crate::game_camera::GameFog>,
    g: Res<GraphicsSettings>,
    mut buffers: ResMut<Assets<bevy::render::storage::ShaderBuffer>>,
) {
    let mut e = MetalEnv::default();
    if let Some(k) = key { e.to_sun = k.to_light.normalize_or(Vec3::Y).extend(1.0); }
    // The mobys' switch (moby.wgsl reads the shine from the vertex, crate::moby_render `SKIN_SHINE_SHIFT`).
    e.sun.w = if g.lighting.enhanced() && env_mode() != Mode::Off { 1.0 } else { 0.0 };
    match sky.and_then(|s| s.0) {
        Some(p) => e.probe = p,
        None => {
            // No sky drawn: the fog colour at the horizon, a lighter zenith.
            let f = fog.uniform.color.truncate();
            let bands = [f.lerp(Vec3::splat(0.9), 0.35), f.lerp(Vec3::splat(0.8), 0.2), f];
            for (b, c) in bands.iter().enumerate() { e.probe[b * SECTORS..(b + 1) * SECTORS].fill(*c); }
            e.probe[PROBE - 1] = f * 0.35;
        }
    }
    crate::game_camera::write_fog_range(&mut buffers, &crate::game_camera::fog_buffer(), crate::game_camera::FOG_PREFIX, &e.bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_bands_by_height() {
        // 4 rows × 8 columns (one per sector): 67.5°, 22.5°, −22.5°, −67.5°: zenith, middle, (no horizon), ground.
        let row = |c: [f32; 3]| [c; 8];
        let mut rgb: Vec<[f32; 3]> = [row([1.0, 0.0, 0.0]), row([0.0, 1.0, 0.0]), row([0.0, 0.0, 1.0]), row([0.0, 0.0, 1.0])].concat();
        rgb[3] = [0.0, 0.0, 0.0]; // the zenith's sector 3 alone
        let p = probe(4, 8, &rgb);
        assert_eq!((p[0], p[3], p[SECTORS], p[2 * SECTORS], p[PROBE - 1]), (Vec3::X, Vec3::ZERO, Vec3::Y, Vec3::ZERO, Vec3::Z));
        // A sky without ground shells: a dark copy of the horizon.
        // 6 rows: 75°, 45°, 15° (the horizon, grey), then black below.
        let p = probe(6, 8, &[row([0.0; 3]), row([0.0; 3]), row([0.5; 3]), row([0.0; 3]), row([0.0; 3]), row([0.0; 3])].concat());
        assert_eq!((p[2 * SECTORS], p[PROBE - 1]), (Vec3::splat(0.5), Vec3::splat(0.5) * 0.35));
    }

    #[test]
    fn shine_levels_round_trip() {
        assert!(Shine::Painted.strength() < Shine::Bare.strength() && Shine::Bare.strength() < Shine::Chrome.strength());
        assert!(METAL.iter().all(|e| !e.what.is_empty() && !e.ids.is_empty()));
        // No texture in two sets of one level and table.
        for (i, a) in METAL.iter().enumerate() {
            for b in &METAL[i + 1..] {
                if a.level == b.level && a.kind == b.kind { assert!(a.ids.iter().all(|x| !b.ids.contains(x)), "{} / {}", a.what, b.what); }
            }
        }
    }

    #[test]
    fn env_bytes() { assert_eq!(MetalEnv::default().bytes().len(), 16 * (2 + PROBE)); }
}
