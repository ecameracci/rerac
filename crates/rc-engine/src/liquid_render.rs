//! **Port-only (Water Enhanced, crate::graphics; docs/plan/graphics_options.md "Water"): the other liquids.** The
//! liquid-port surfaces crate::sea_render judges (crate::sea_water::surface_fix, `judged_rgb`: the image × the vertex
//! colours) get a kind ([`classify`]): clear water goes to crate::sea_water as before; lava, mud and sludge are drawn
//! here (`liquid.wgsl`) instead of their Original draw; anything doubtful stays Original. Ice is never judged by colour:
//! Hoven's liquid 1901 (grey 57 under green vertex colours) is ice by its port ([`hoven_scroll`]).
//! crate::water_render's ripple patches that are not clear water take sludge only
//! ([`patch_verdict`]: Gemlik's acid pools) and draw it in `water.wgsl`. The looks live in `liquid_look.wgsl`
//! (`rerac::liquid_look`, [`keep_look_module`]). Each keeps the draw's own colour (the texture as the game modulates
//! and fogs it) and adds a procedural surface over it:
//! * **lava**: a dark crust of that colour with bright, emissive cracks (the zero lines of two gradient-noise layers in
//!   the draw's ST, so they ride the original scroll; the second layer drifts at the second pass's relative speed,
//!   [`flow_velocity`]), and a slow glow pulse. Gaspar's lava flows draw two passes (FIX 0x80, then 0x60 scrolled
//!   twice as fast, rc_game::water::sea `gaspar_extras`): the look takes both, so the 0x60 pass is not drawn. A liquid
//!   grid's blocks each restart the ST (crate::sea_render::grid_prims_into): its layers lie on the world plane at the
//!   grid's ST scale instead ([`LiquidGroup::grid`], [`st_per_unit`]), so they neither repeat block by block nor
//!   break at the block edges (Umbris 460, Gaspar 317);
//! * **mud** (Aridia's quicksand 854, by its port: [`quicksand_kind`]): thick and slow, the game's texture and colour
//!   kept at about their brightness: a gentle swell in the normals (no waves, a few hundredths of a unit over 4 to 11
//!   units), slow bubbles in one 3.5-unit cell in four that swell for eight to thirteen seconds, pop and leave a
//!   settling ring, wet and drier patches, a drier crust along the banks ([`shore_map`]: the collision's faces that are not
//!   the quicksand at its height), shaded by the key light (crate::sun_shadow::KeyLight) relative to the flat surface,
//!   a soft wet sheen and a dull sky reflection (the fog colour, brightened) at grazing angles only;
//! * **sludge**: murky, a weak emissive tint of its colour, slow ripples and bubbles that swell and pop;
//! * **ice** (Hoven's slush): flat and still. The game scrolls its two layers (FX 0x2f opaque, FX 0x30 added over it
//!   at `fix2`) against each other, which reads as rolling waves; here every layer drifts at [`ICE_DRIFT`] of its
//!   speed. The opaque layers keep the game's own darker green-blue colour and pattern, with only a faint sky sheen
//!   and a sharp glint (`ICE`); the added pass, the white streaks, stays the game's draw over them.
//!
//! A translucent grid is a fog or cloud layer, never a liquid ([`layer_verdict`]: Oltanis's 1260, mud-brown at FIX
//! 0x40, is the haze at the bottom of the world), so it stays Original whatever its colour.
//!
//! Drawn as opaque world surfaces (the main pass, Z written), with the level's or the grid's fog, seen from either
//! side: so they keep the ambient occlusion and the shadows (the game's and the sun's, both applied between the opaque
//! and the transparent passes) that Ratchet and the mobys cast on the quicksand, which a draw after the scene copy
//! (a screen-space reflection, crate::sea_water) would lose. Not in
//! the planar reflection (crate::water_mirror): none of them mirrors. Drawing only: collision, damage and the game
//! state are untouched, no random numbers are drawn.
//!
//! `RC_LIQUID_FX=0`: no kind but water is drawn here (they stay Original); `RC_LIQUID_FX_KIND=water|lava|mud|sludge|
//! ice|original` forces that kind on every liquid surface the Enhanced path may take (tuning).

use crate::fx_draw::PrimBuf;
use crate::game_camera::TfragFog;
use crate::gs_state::GsPass;
use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::mesh::{MeshVertexBufferLayoutRef, PrimitiveTopology};
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError};
use bevy::shader::ShaderRef;
use std::collections::HashMap;

const SHADER_PATH: &str = "shaders/liquid.wgsl";
const LOOK_PATH: &str = "shaders/liquid_look.wgsl";

/// A liquid surface's kind (module doc).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LiquidKind {
    Water,
    Lava,
    Mud,
    Sludge,
    Ice,
    /// Kept Original (an unknown or doubtful colour: Kerwan's grey cloud floor, the dark liquids).
    Original,
}

impl LiquidKind {
    pub fn name(self) -> &'static str {
        match self {
            LiquidKind::Water => "water",
            LiquidKind::Lava => "lava",
            LiquidKind::Mud => "mud",
            LiquidKind::Sludge => "sludge",
            LiquidKind::Ice => "ice",
            LiquidKind::Original => "original",
        }
    }

    pub fn parse(s: &str) -> Option<LiquidKind> {
        [LiquidKind::Water, LiquidKind::Lava, LiquidKind::Mud, LiquidKind::Sludge, LiquidKind::Ice, LiquidKind::Original].into_iter().find(|k| k.name() == s.trim().to_ascii_lowercase())
    }

    /// Whether this module draws the kind (`liquid.wgsl` `params.misc.x`: 1 lava, 2 mud, 3 sludge, 4 ice).
    pub fn drawn_here(self) -> bool { matches!(self, LiquidKind::Lava | LiquidKind::Mud | LiquidKind::Sludge | LiquidKind::Ice) }

    fn shader_id(self) -> f32 {
        match self {
            LiquidKind::Lava => 1.0,
            LiquidKind::Mud => 2.0,
            LiquidKind::Sludge => 3.0,
            LiquidKind::Ice => 4.0,
            _ => 0.0,
        }
    }
}

/// The kind of a liquid surface by its judged colour (crate::sea_render `judged_rgb`). Water is
/// crate::sea_water::clear_liquid's verdict, unchanged; then lava (red over a little green, no blue: Gaspar 132, 23, 0
/// and Umbris 100, 17, 0), mud (a mid brown: Oltanis 101, 79, 58), sludge (green or purple over the rest, and a dark
/// olive with almost no blue: Umbris's swamp 1018, 16, 14, 7); grey (Kerwan's 994, 140), the other dark liquids
/// (Aridia's quicksand 854, 25, 23, 15: below 40 on every channel, mud by its port instead, [`quicksand_kind`]) and
/// anything else stay Original. Ice is never judged by colour (a pale ice reads as water or grey): Hoven's liquid is
/// ice by its port (crate::sea_render).
pub fn classify(rgb: [u8; 3]) -> LiquidKind {
    if crate::sea_water::clear_liquid(rgb) { return LiquidKind::Water; }
    let [r, g, b] = rgb.map(f32::from);
    let (hi, lo) = (r.max(g).max(b), r.min(g).min(b));
    if hi < 40.0 {
        return if hi >= 8.0 && r >= g && g >= 1.8 * b { LiquidKind::Sludge } else { LiquidKind::Original };
    }
    let sat = (hi - lo) / hi;
    if r == hi && r >= 3.0 * g && r >= 3.0 * b { return LiquidKind::Lava; }
    if g >= 1.15 * r && g >= 1.15 * b { return LiquidKind::Sludge; }
    if r >= 1.2 * g && b >= 1.2 * g { return LiquidKind::Sludge; }
    if r >= g && g >= b && (0.25..=0.75).contains(&sat) { return LiquidKind::Mud; }
    LiquidKind::Original
}

/// A translucent liquid grid (`grid`, its FIX below 0x80) is a fog or cloud layer under the level, not a liquid
/// (Oltanis's 1260 at FIX 0x40, the haze at the bottom of the world; Kerwan's 994 cloud floor at 0x20): no look of
/// this module, it stays Original. Clear water keeps crate::sea_water's verdict.
pub fn layer_verdict(kind: LiquidKind, grid: bool, fix: u8) -> LiquidKind {
    if grid && fix < 0x80 && kind.drawn_here() { LiquidKind::Original } else { kind }
}

/// `RC_LIQUID_FX=0`: only water goes Enhanced.
pub fn enabled() -> bool { !std::env::var("RC_LIQUID_FX").is_ok_and(|v| v.trim() == "0") }

/// `RC_LIQUID_FX_KIND`: the kind forced on every liquid surface (None: by colour).
pub fn forced() -> Option<LiquidKind> { std::env::var("RC_LIQUID_FX_KIND").ok().and_then(|v| LiquidKind::parse(&v)) }

/// The verdict with the switches: the forced kind, else [`classify`]; with `RC_LIQUID_FX=0` every kind this module
/// draws is Original.
pub fn verdict(rgb: [u8; 3], enabled: bool, forced: Option<LiquidKind>) -> LiquidKind {
    let k = forced.unwrap_or_else(|| classify(rgb));
    if !enabled && k.drawn_here() { LiquidKind::Original } else { k }
}

/// The kind of 854's grids (rc_game::water::sea `aridia_ref`), Aridia's quicksand: mud by its port, its judged colour
/// (25, 23, 15) being too dark for [`classify`]'s rules (they keep the dark liquids Original). Under its grids at z 25
/// lie 3,681 of the level's 3,779 faces of [`QUICKSAND_SURFACE`], the only level that has any. The switches as in
/// [`verdict`].
pub fn quicksand_kind() -> LiquidKind { verdict_of(LiquidKind::Mud, enabled(), forced()) }

fn verdict_of(k: LiquidKind, enabled: bool, forced: Option<LiquidKind>) -> LiquidKind {
    let k = forced.unwrap_or(k);
    if !enabled && k.drawn_here() { LiquidKind::Original } else { k }
}

/// The collision's sinking liquid (rc_game::hero::surface: Ratchet sinks into it, state 0x68).
pub const QUICKSAND_SURFACE: u8 = 3;
/// The banks' reach ([`shore_map`]): a face counts as bank where it lies from `x` below to `y` above the quicksand's
/// height (the grass sloping out of it; deeper ground is under the quicksand, higher ground a cliff behind the bank);
/// `z` = the drier rim's width (game units) from the bank, `w` = texels per game unit.
pub const SHORE: Vec4 = Vec4::new(0.3, 4.0, 3.0, 1.0);

/// The banks around a level's quicksand ([`shore_map`]): `texels` (row-major from `origin`, game x then y, one per
/// [`SHORE`]`.w` units) 255 on a bank, falling to 0 at [`SHORE`]`.z` from it; `z` the quicksand's height.
#[derive(Debug, Clone, PartialEq)]
pub struct ShoreMap {
    pub origin: [f32; 2],
    pub size: [u32; 2],
    pub texels: Vec<u8>,
    pub z: f32,
}

impl ShoreMap {
    /// `liquid.wgsl`'s `params.shore`: the origin (game x, y) and the texel scale over the size.
    pub fn params(&self) -> Vec4 {
        Vec4::new(self.origin[0], self.origin[1], SHORE.w / self.size[0] as f32, SHORE.w / self.size[1] as f32)
    }
}

/// The level collision's triangles (game space) with their surface ids (a quad as two triangles, `v0 v1 v2` and
/// `v0 v2 v3`).
pub fn collision_tris(c: &rc_formats::collision::Collision) -> Vec<([[f32; 3]; 3], u8)> {
    let mut out = Vec::new();
    for cell in &c.cells {
        for k in 0..cell.faces.len() {
            let i = cell.face_indices(k).map(|v| cell.vertices.get(v as usize).copied());
            let s = cell.faces[k].surface & 0x1f;
            let (Some(a), Some(b), Some(d), Some(e)) = (i[0], i[1], i[2], i[3]) else { continue };
            out.push(([a, b, d], s));
            if cell.is_quad(k) { out.push(([a, d, e], s)); }
        }
    }
    out
}

/// Where the quicksand meets its banks, from the collision (`tris`, [`collision_tris`]): its height is the commonest
/// of its faces' (Aridia 25); over their box (grown by the rim) each texel takes the highest face within [`SHORE`]'s
/// reach of that height, a bank when it is not quicksand, and keeps `1 − d / rim` of the nearest bank texel `d` away.
/// None without quicksand faces.
pub fn shore_map(tris: &[([[f32; 3]; 3], u8)]) -> Option<ShoreMap> {
    let cz = |t: &[[f32; 3]; 3]| (t[0][2] + t[1][2] + t[2][2]) / 3.0;
    let mut heights: HashMap<i32, usize> = HashMap::new();
    for (t, s) in tris {
        if *s == QUICKSAND_SURFACE { *heights.entry((cz(t) * 16.0).round() as i32).or_default() += 1; }
    }
    let z = heights.into_iter().max_by_key(|&(h, n)| (n, h))?.0 as f32 / 16.0;
    let (lo_z, hi_z) = (z - SHORE.x, z + SHORE.y);
    let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
    for (t, _) in tris.iter().filter(|(t, s)| *s == QUICKSAND_SURFACE && (cz(t) - z).abs() < 1.0) {
        for p in t {
            for k in 0..2 { (lo[k], hi[k]) = (lo[k].min(p[k]), hi[k].max(p[k])); }
        }
    }
    let rim = SHORE.z;
    let step = 1.0 / SHORE.w;
    let origin = [(lo[0] - rim - 1.0).floor(), (lo[1] - rim - 1.0).floor()];
    let size = [0, 1].map(|k| ((hi[k] + rim + 1.0 - origin[k]) / step).ceil().max(1.0) as u32);
    let (w, h) = (size[0] as usize, size[1] as usize);
    // The highest face's z over each texel centre and whether it is a bank.
    let mut top: Vec<(f32, bool)> = vec![(f32::MIN, false); w * h];
    for (t, s) in tris {
        let (zmin, zmax) = (t[0][2].min(t[1][2]).min(t[2][2]), t[0][2].max(t[1][2]).max(t[2][2]));
        if zmax < lo_z || zmin > hi_z { continue; }
        let [a, b, c] = *t;
        let det = (b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1]);
        if det.abs() < 1e-6 { continue; }
        let tx = |x: f32, k: usize| ((x - origin[k]) / step - 0.5).floor();
        let (x0, x1) = (tx(a[0].min(b[0]).min(c[0]), 0).max(0.0) as usize, (tx(a[0].max(b[0]).max(c[0]), 0) + 1.0).clamp(0.0, w as f32 - 1.0) as usize);
        let (y0, y1) = (tx(a[1].min(b[1]).min(c[1]), 1).max(0.0) as usize, (tx(a[1].max(b[1]).max(c[1]), 1) + 1.0).clamp(0.0, h as f32 - 1.0) as usize);
        if x0 > x1 || y0 > y1 { continue; }
        for j in y0..=y1 {
            for i in x0..=x1 {
                let p = [origin[0] + (i as f32 + 0.5) * step, origin[1] + (j as f32 + 0.5) * step];
                let u = ((p[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (p[1] - a[1])) / det;
                let v = ((b[0] - a[0]) * (p[1] - a[1]) - (p[0] - a[0]) * (b[1] - a[1])) / det;
                if u < -1e-4 || v < -1e-4 || u + v > 1.0 + 1e-4 { continue; }
                let pz = a[2] + u * (b[2] - a[2]) + v * (c[2] - a[2]);
                let e = &mut top[j * w + i];
                if (lo_z..=hi_z).contains(&pz) && pz > e.0 { *e = (pz, *s != QUICKSAND_SURFACE); }
            }
        }
    }
    let r = (rim / step).ceil() as i32;
    let mut texels = vec![0u8; w * h];
    for j in 0..h as i32 {
        for i in 0..w as i32 {
            let mut d2 = f32::MAX;
            for dj in -r..=r {
                for di in -r..=r {
                    let (x, y) = (i + di, j + dj);
                    if x < 0 || y < 0 || x >= w as i32 || y >= h as i32 || !top[y as usize * w + x as usize].1 { continue; }
                    d2 = d2.min((di * di + dj * dj) as f32);
                }
            }
            let v = (1.0 - d2.sqrt() * step / rim).clamp(0.0, 1.0);
            texels[j as usize * w + i as usize] = (v * 255.0).round() as u8;
        }
    }
    Some(ShoreMap { origin, size, texels, z })
}

/// The lava flows' ST speed of a scroll rule, per second (rc_game::water::sea::MeshSt::Flow: t + k·f with f = 1 −
/// (counter & 0x7ff)/2048, so t falls k/2048 a tick at 60 ticks a second; the jump by k every 2048 ticks is seamless
/// on a repeating texture). Zero for the other rules.
pub fn flow_velocity(st: &rc_game::water::sea::MeshSt) -> Vec2 {
    match *st {
        rc_game::water::sea::MeshSt::Flow { k, .. } => Vec2::new(0.0, -k * 60.0 / 2048.0),
        _ => Vec2::ZERO,
    }
}

/// The second noise layer's drift against the first (`params.flow`: xy per second, z the offset): a second flow pass
/// against the first, else a slow drift of its own (a grid's ST does not scroll: its image animates instead); w = the
/// first layer's own ST speed ([`flow_velocity`]'s t; `liquid_look.wgsl` `liquid_frame` carries lava's fine detail
/// with it).
pub fn layer_flow(first: Option<&rc_game::water::sea::MeshSt>, second: Option<&rc_game::water::sea::MeshSt>) -> Vec4 {
    use rc_game::water::sea::MeshSt;
    let own = first.map_or(0.0, |a| flow_velocity(a).y);
    match (first, second) {
        (Some(a @ MeshSt::Flow { add: a0, .. }), Some(b @ MeshSt::Flow { add: a1, .. })) => (flow_velocity(b) - flow_velocity(a)).extend(a1 - a0).extend(own),
        _ => DRIFT.with_w(own),
    }
}

/// Lava's fine detail (`liquid_look.wgsl` LAVA_FINE): its cracks' cells per world unit, the period of its two flow
/// phases (seconds), the grain's frequency over the cracks', the fastest flow it follows (world units per second).
/// The shader holds it; this copy is the tests' check of it.
#[cfg(test)]
const LAVA_FINE: Vec4 = Vec4::new(0.85, 3.0, 3.7, 6.0);

/// [`layer_flow`] without a second pass: ST per second and offset.
pub const DRIFT: Vec4 = Vec4::new(0.011, -0.017, 0.5, 0.0);

/// The ice look (`liquid_look.wgsl` ICE): the sky sheen's share of the Fresnel term, the glint's strength. The shader
/// holds it; this copy is the tests'.
#[cfg(test)]
const ICE: Vec2 = Vec2::new(0.35, 0.3);

/// Ice's drift: the share of the game's scroll speed its layers keep (module doc).
pub const ICE_DRIFT: f32 = 0.05;

/// Whether Hoven's liquid 1901 is drawn as ice (Water Enhanced with the switches: `RC_LIQUID_FX=0` or a forced kind
/// other than ice keep it Original).
pub fn hoven_ice(water_on: bool) -> bool { water_on && enabled() && forced().is_none_or(|k| k == LiquidKind::Ice) }

/// An ice layer's ST scroll at tick `counter`: the game's speed (ST per second, rc_game::water::sea `HovenData`) times
/// [`ICE_DRIFT`], wrapped to one tile (the textures repeat).
pub fn hoven_scroll(speed: [f32; 2], counter: u64) -> [f32; 2] {
    let t = (counter % (60 * 60 * 60)) as f32 / 60.0;
    speed.map(|v| (v * ICE_DRIFT * t).rem_euclid(1.0))
}

/// A flat draw's ST per world unit (the square root of the ratio of the ST area to the world area of its first
/// triangle that has both; 0: none). A liquid grid's ST is the same scale everywhere ([`LiquidGroup::grid`]).
pub fn st_per_unit(prims: &PrimBuf) -> f32 {
    let (pos, uv, _, idx) = prims.parts();
    for t in idx.as_chunks::<3>().0 {
        let [a, b, c] = t.map(|i| i as usize);
        let (Some(pa), Some(pb), Some(pc)) = (pos.get(a), pos.get(b), pos.get(c)) else { continue };
        // Bevy xz: the world plane.
        let w = ((pb[0] - pa[0]) * (pc[2] - pa[2]) - (pc[0] - pa[0]) * (pb[2] - pa[2])).abs();
        let s = ((uv[b][0] - uv[a][0]) * (uv[c][1] - uv[a][1]) - (uv[c][0] - uv[a][0]) * (uv[b][1] - uv[a][1])).abs();
        if w > 1e-4 && s > 1e-8 { return (s / w).sqrt(); }
    }
    0.0
}

/// One surface of this tick: the image (as crate::sea_render resolved it), the fog, the primitives, the kind and the
/// second layer's flow ([`layer_flow`]).
pub struct LiquidGroup {
    pub image: Handle<Image>,
    pub fog: TfragFog,
    pub prims: PrimBuf,
    pub kind: LiquidKind,
    pub flow: Vec4,
    /// A liquid grid's own draw: its ST per world unit ([`st_per_unit`]; the look's layers on the world plane); 0: in
    /// the draw's ST.
    pub grid: f32,
}

/// `liquid.wgsl`'s static uniform.
#[derive(Clone, Copy, Debug, Default, PartialEq, ShaderType)]
pub struct LiquidParams {
    /// x = the kind (1 lava, 2 mud, 3 sludge, 4 ice), y = [`LiquidGroup::grid`].
    pub misc: Vec4,
    /// [`layer_flow`].
    pub flow: Vec4,
    /// xyz = toward the key light (world).
    pub sun: Vec4,
    /// Mud's banks ([`ShoreMap::params`]; 0: none).
    pub shore: Vec4,
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct LiquidMaterial {
    #[texture(0)]
    #[sampler(1)]
    pub texture: Handle<Image>,
    #[uniform(2)]
    pub fog: TfragFog,
    #[uniform(3)]
    pub params: LiquidParams,
    /// [`ShoreMap`]'s texels (R8), a blank texel without one.
    #[texture(4)]
    #[sampler(5)]
    pub shore: Handle<Image>,
}

impl Material for LiquidMaterial {
    fn vertex_shader() -> ShaderRef { SHADER_PATH.into() }
    fn fragment_shader() -> ShaderRef { SHADER_PATH.into() }
    fn alpha_mode(&self) -> AlphaMode { AlphaMode::Opaque }
    fn enable_prepass() -> bool { false }
    fn enable_shadows() -> bool { false }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = None;
        GsPass::Opaque.specialize(descriptor);
        descriptor.vertex.buffers = vec![layout.0.get_layout(&[
            Mesh::ATTRIBUTE_POSITION.at_shader_location(0),
            Mesh::ATTRIBUTE_UV_0.at_shader_location(1),
            Mesh::ATTRIBUTE_COLOR.at_shader_location(2),
        ])?];
        Ok(())
    }
}

type Slot = (Entity, Handle<Mesh>, Handle<LiquidMaterial>, bool);

/// The surfaces of this tick (crate::sea_render fills `pending` once per tick) and the slots.
#[derive(Resource, Default)]
pub struct LiquidDraw {
    pub pending: Option<Vec<LiquidGroup>>,
    slots: Vec<Option<Slot>>,
    /// The level's [`ShoreMap`] as an image and its params, made with its first mud draw (None inside: no quicksand).
    shore: Option<Option<(Handle<Image>, Vec4)>>,
    blank: Option<Handle<Image>>,
}

/// An R8 image of `texels` (`w` × `h`), filtered, clamped at its edges.
fn r8_image(w: u32, h: u32, texels: Vec<u8>) -> Image {
    use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
    let mut img = Image::new(Extent3d { width: w, height: h, depth_or_array_layers: 1 }, TextureDimension::D2, texels, TextureFormat::R8Unorm, RenderAssetUsages::RENDER_WORLD);
    img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::ClampToEdge,
        address_mode_v: ImageAddressMode::ClampToEdge,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        ..default()
    });
    img
}

/// Keeps the `rerac::liquid_look` WGSL module loaded (`liquid.wgsl` and `water.wgsl` import it by its path).
#[derive(Resource)]
struct LiquidLookShader(#[allow(dead_code)] Handle<Shader>);

/// Loads `liquid_look.wgsl` once (from [`build`] and crate::water_render's plugin, either of which may be off).
pub fn keep_look_module(app: &mut App) {
    if app.world().contains_resource::<LiquidLookShader>() { return; }
    let look = app.world().resource::<AssetServer>().load(LOOK_PATH);
    app.insert_resource(LiquidLookShader(look));
}

/// The kind of a level-code water texture that is not clear water (crate::water_render's ripple patches and strips,
/// whose clear-water test stays its own): sludge by [`classify`] of its average colour (Gemlik's acid, FX 46 / 45),
/// else Original (Gemlik's orange FX 54 / 53 are unidentified: no mud or lava is known there). The switches as in
/// [`verdict`]; a forced water is Original (crate::water_render's water look keeps its own test).
pub fn patch_verdict(rgb: [u8; 3], enabled: bool, forced: Option<LiquidKind>) -> LiquidKind {
    let k = forced.unwrap_or(match classify(rgb) { LiquidKind::Sludge => LiquidKind::Sludge, _ => LiquidKind::Original });
    if !enabled || !k.drawn_here() { LiquidKind::Original } else { k }
}

/// [`patch_verdict`] of FX texture `fx` (None: absent) as `water.wgsl`'s `look.misc.y` (0: Original), logged.
pub fn patch_kind(fx: i32, t: Option<&rc_formats::texture::Texture>) -> f32 {
    let Some(t) = t else { return 0.0 };
    let n = (t.rgba.len() / 4).max(1) as f32;
    let sum = t.rgba.as_chunks::<4>().0.iter().fold([0.0f32; 3], |a, p| [a[0] + p[0] as f32, a[1] + p[1] as f32, a[2] + p[2] as f32]);
    let avg = crate::sea_render::judged_rgb(sum.map(|s| s / n), [1.0; 3]);
    let kind = patch_verdict(avg, enabled(), forced());
    println!("water: fx texture {fx} as another liquid (average {avg:?}): {} ({})", kind.name(), if kind.drawn_here() { "Water Enhanced's liquid look" } else { "kept Original" });
    kind.shader_id()
}

/// The material plugin and the draw (from crate::sea_render's plugin, after its draw).
pub fn build(app: &mut App) {
    keep_look_module(app);
    app.add_plugins(MaterialPlugin::<LiquidMaterial>::default())
        .init_resource::<LiquidDraw>()
        .add_systems(crate::level_switch::LevelUnload, crate::level_switch::reset::<LiquidDraw>);
}

/// This tick's surfaces into the slots; every frame, the key light into the materials.
#[allow(clippy::too_many_arguments)]
pub fn draw(
    mut commands: Commands,
    state: ResMut<LiquidDraw>,
    key: Option<Res<crate::sun_shadow::KeyLight>>,
    level: Res<crate::Level>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<LiquidMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut vis: Query<&mut Visibility>,
) {
    let st = state.into_inner();
    let sun = key.map_or(Vec3::Y, |k| k.to_light).extend(0.0);
    for (_, _, mat, _) in st.slots.iter().flatten() {
        if materials.get(mat).is_some_and(|m| m.params.sun.distance(sun) > 0.01) {
            if let Some(mut m) = materials.get_mut(mat) { m.params.sun = sun; }
        }
    }
    let Some(groups) = st.pending.take() else { return };
    let blank = st.blank.get_or_insert_with(|| images.add(r8_image(1, 1, vec![0]))).clone();
    if st.shore.is_none() && groups.iter().any(|g| g.kind == LiquidKind::Mud) {
        let t = std::time::Instant::now();
        let map = level.0.collision.as_ref().and_then(|c| shore_map(&collision_tris(c)));
        if let Some(m) = &map {
            let banks = m.texels.iter().filter(|&&v| v == 255).count();
            println!("liquid: mud banks {}×{} texels from game ({}, {}) at z {}: {banks} on a bank ({:.1} ms)", m.size[0], m.size[1], m.origin[0], m.origin[1], m.z, t.elapsed().as_secs_f32() * 1e3);
        }
        st.shore = Some(map.map(|m| (images.add(r8_image(m.size[0], m.size[1], m.texels.clone())), m.params())));
    }
    let n = groups.len();
    for (k, g) in groups.into_iter().enumerate() {
        if st.slots.len() <= k { st.slots.push(None); }
        if g.prims.is_empty() {
            hide(&mut vis, &mut st.slots[k]);
            continue;
        }
        let (shore_img, shore) = match (&st.shore, g.kind) {
            (Some(Some((img, p))), LiquidKind::Mud) => (img.clone(), *p),
            _ => (blank.clone(), Vec4::ZERO),
        };
        let params = LiquidParams { misc: Vec4::new(g.kind.shader_id(), g.grid, 0.0, 0.0), flow: g.flow, sun, shore };
        if let Some((e, mesh, mat, shown)) = &mut st.slots[k] {
            g.prims.update(&mut meshes, mesh);
            if materials.get(&*mat).is_none_or(|m| m.texture != g.image || m.fog != g.fog || m.params != params || m.shore != shore_img) {
                if let Some(mut m) = materials.get_mut(&*mat) {
                    m.texture = g.image;
                    m.fog = g.fog;
                    m.params = params;
                    m.shore = shore_img;
                }
            }
            if !*shown {
                *shown = true;
                if let Ok(mut v) = vis.get_mut(*e) { *v = Visibility::Inherited; }
            }
            continue;
        }
        let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
        g.prims.write(&mut mesh);
        let mesh = meshes.add(mesh);
        let mat = materials.add(LiquidMaterial { texture: g.image, fog: g.fog, params, shore: shore_img });
        let e = commands.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(mat.clone()), Transform::IDENTITY, NoFrustumCulling, Visibility::Inherited, Name::new(format!("liquid {k}")))).id();
        st.slots[k] = Some((e, mesh, mat, true));
    }
    for s in st.slots.iter_mut().skip(n) { hide(&mut vis, s); }
}

fn hide(vis: &mut Query<&mut Visibility>, s: &mut Option<Slot>) {
    let Some((e, _, _, shown)) = s else { return };
    if !*shown { return; }
    *shown = false;
    if let Ok(mut v) = vis.get_mut(*e) { *v = Visibility::Hidden; }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rc_game::water::sea::MeshSt;

    #[test]
    fn the_judged_colours_of_the_levels_get_their_kinds() {
        // The colours the engine logs (crate::sea_render `judged_rgb`), level by level.
        let cases = [
            ([1, 4, 9], LiquidKind::Water),        // Rilgar 879
            ([14, 15, 27], LiquidKind::Water),     // Batalia 327
            ([132, 23, 0], LiquidKind::Lava),      // Gaspar 317 and its flows
            ([100, 17, 0], LiquidKind::Lava),      // Umbris 460
            ([101, 79, 58], LiquidKind::Mud),      // Oltanis 1260
            ([140, 140, 140], LiquidKind::Original), // Kerwan 994, the cloud floor
            ([16, 14, 7], LiquidKind::Sludge),     // Umbris 1018, the swamp
            ([25, 23, 15], LiquidKind::Original),  // Aridia 854, the quicksand
            ([57, 57, 57], LiquidKind::Original),  // a grey FX texture
        ];
        for (c, k) in cases { assert_eq!(classify(c), k, "{c:?}"); }
        assert_eq!(classify([40, 90, 30]), LiquidKind::Sludge);
        assert_eq!(classify([90, 30, 100]), LiquidKind::Sludge);
        // Water stays crate::sea_water's verdict.
        for c in [[0u8, 0, 0], [10, 10, 12], [30, 60, 90], [200, 210, 255], [255, 0, 0]] {
            assert_eq!(classify(c) == LiquidKind::Water, crate::sea_water::clear_liquid(c), "{c:?}");
        }
    }

    #[test]
    fn the_quicksand_is_mud_by_its_port() {
        // Too dark for the colour rules, mud by 854's port; the switches as for any kind.
        assert_eq!(classify([25, 23, 15]), LiquidKind::Original);
        assert_eq!(verdict_of(LiquidKind::Mud, true, None), LiquidKind::Mud);
        assert_eq!(verdict_of(LiquidKind::Mud, false, None), LiquidKind::Original);
        assert_eq!(verdict_of(LiquidKind::Mud, true, Some(LiquidKind::Lava)), LiquidKind::Lava);
        assert_eq!(verdict_of(LiquidKind::Mud, true, Some(LiquidKind::Water)), LiquidKind::Water);
        assert_eq!(layer_verdict(LiquidKind::Mud, true, 0x80), LiquidKind::Mud);
    }

    /// A flat square of triangles over x, y in [x0, x1] × [y0, y1] at height z.
    fn square(x0: f32, y0: f32, x1: f32, y1: f32, z: f32, s: u8) -> [([[f32; 3]; 3], u8); 2] {
        [([[x0, y0, z], [x1, y0, z], [x1, y1, z]], s), ([[x0, y0, z], [x1, y1, z], [x0, y1, z]], s)]
    }

    #[test]
    fn the_shore_map_marks_the_banks_and_fades_over_the_rim() {
        assert_eq!(shore_map(&square(0.0, 0.0, 10.0, 10.0, 25.0, 0x8)), None);
        // Quicksand over x 100..140 at z 25; a bank rising out of it over x 130..140 (at 25.5); a deep floor under
        // the whole (z 20) and a cliff top far above (z 40) at x 100..110 count as neither.
        let mut tris = Vec::new();
        tris.extend(square(100.0, 100.0, 140.0, 120.0, 25.0, QUICKSAND_SURFACE));
        tris.extend(square(130.0, 100.0, 140.0, 120.0, 25.5, 0x8));
        tris.extend(square(90.0, 90.0, 150.0, 130.0, 20.0, 0x8));
        tris.extend(square(100.0, 100.0, 110.0, 120.0, 40.0, 0x8));
        let m = shore_map(&tris).expect("a quicksand");
        assert_eq!(m.z, 25.0);
        let at = |x: f32, y: f32| {
            let (i, j) = (((x - m.origin[0]) * SHORE.w) as usize, ((y - m.origin[1]) * SHORE.w) as usize);
            m.texels[j * m.size[0] as usize + i] as f32 / 255.0
        };
        assert_eq!(at(135.5, 110.5), 1.0, "on the bank");
        assert!((at(128.5, 110.5) - (1.0 - 2.0 / SHORE.z)).abs() < 0.01, "two units out: {}", at(128.5, 110.5));
        assert_eq!(at(120.5, 110.5), 0.0, "open quicksand");
        assert_eq!(at(104.5, 110.5), 0.0, "under the cliff: no bank");
        // The params map game (x, y) to the image.
        let p = m.params();
        assert!(((135.5 - p.x) * p.z * m.size[0] as f32 - (135.5 - m.origin[0])).abs() < 1e-3);
        assert!(m.origin[0] <= 100.0 - SHORE.z && m.origin[0] + m.size[0] as f32 >= 140.0 + SHORE.z);
    }

    #[test]
    fn a_translucent_grid_is_a_fog_layer() {
        // Oltanis 1260: mud by colour, FIX 0x40.
        assert_eq!(layer_verdict(classify([101, 79, 58]), true, 0x40), LiquidKind::Original);
        assert_eq!(layer_verdict(LiquidKind::Mud, true, 0x80), LiquidKind::Mud);
        assert_eq!(layer_verdict(LiquidKind::Lava, false, 0x60), LiquidKind::Lava);
        assert_eq!(layer_verdict(LiquidKind::Water, true, 0x20), LiquidKind::Water);
    }

    #[test]
    fn the_switches_force_or_drop_the_kinds() {
        assert_eq!(verdict([132, 23, 0], true, None), LiquidKind::Lava);
        assert_eq!(verdict([132, 23, 0], false, None), LiquidKind::Original);
        assert_eq!(verdict([1, 4, 9], false, None), LiquidKind::Water);
        assert_eq!(verdict([140, 140, 140], true, Some(LiquidKind::Mud)), LiquidKind::Mud);
        assert_eq!(verdict([1, 4, 9], true, Some(LiquidKind::Sludge)), LiquidKind::Sludge);
        for k in [LiquidKind::Water, LiquidKind::Lava, LiquidKind::Mud, LiquidKind::Sludge, LiquidKind::Ice, LiquidKind::Original] {
            assert_eq!(LiquidKind::parse(k.name()), Some(k));
        }
        assert_eq!(LiquidKind::parse(" Lava "), Some(LiquidKind::Lava));
        assert_eq!(LiquidKind::parse("magma"), None);
        assert!(LiquidKind::Ice.drawn_here() && !LiquidKind::Water.drawn_here() && LiquidKind::Sludge.drawn_here());
        // No colour is ice: Hoven's liquid is, by its port.
        for c in [[57u8, 57, 57], [200, 220, 240], [240, 240, 240]] { assert_ne!(classify(c), LiquidKind::Ice, "{c:?}"); }
        assert!(!hoven_ice(false));
    }

    #[test]
    fn a_grid_gets_its_st_scale_and_ice_drifts_slowly() {
        // A strip of 2-unit cells at ST 0.5 a cell (Bevy points from game ones): 0.25 ST per unit.
        let mut b = PrimBuf::default();
        b.strip([([10.0, 20.0, 35.0], [0.0, 0.0], 0x8080_8080), ([12.0, 20.0, 35.0], [0.5, 0.0], 0x8080_8080), ([10.0, 22.0, 35.0], [0.0, 0.5], 0x8080_8080), ([12.0, 22.0, 35.0], [0.5, 0.5], 0x8080_8080)]);
        assert!((st_per_unit(&b) - 0.25).abs() < 1e-6, "{}", st_per_unit(&b));
        assert_eq!(st_per_unit(&PrimBuf::default()), 0.0);
        // The ice's scroll: ICE_DRIFT of the speed, wrapped; none at tick 0.
        assert_eq!(hoven_scroll([0.3, -0.2], 0), [0.0, 0.0]);
        let s = hoven_scroll([0.3, -0.2], 600);
        assert!((s[0] - 0.3 * ICE_DRIFT * 10.0).abs() < 1e-5 && (s[1] - (1.0 - 0.2 * ICE_DRIFT * 10.0)).abs() < 1e-5, "{s:?}");
        assert!(hoven_scroll([5.0, 7.0], 123_457).iter().all(|v| (0.0..1.0).contains(v)));
        let look = include_str!("../assets/shaders/liquid_look.wgsl");
        let line = format!("const ICE: vec2<f32> = vec2<f32>({:?}, {:?});", ICE.x, ICE.y);
        assert!(look.contains(&line), "{line}");
    }

    #[test]
    fn the_ripple_patches_take_sludge_only() {
        // Gemlik's acid (FX 46 / 45), its orange patches (FX 54 / 53), an empty patch (FX 0).
        assert_eq!(patch_verdict([54, 182, 0], true, None), LiquidKind::Sludge);
        assert_eq!(patch_verdict([52, 182, 6], true, None), LiquidKind::Sludge);
        assert_eq!(patch_verdict([245, 155, 98], true, None), LiquidKind::Original);
        assert_eq!(patch_verdict([205, 115, 7], true, None), LiquidKind::Original);
        assert_eq!(patch_verdict([4, 4, 4], true, None), LiquidKind::Original);
        assert_eq!(patch_verdict([54, 182, 0], false, None), LiquidKind::Original);
        assert_eq!(patch_verdict([245, 155, 98], true, Some(LiquidKind::Lava)), LiquidKind::Lava);
        assert_eq!(patch_verdict([54, 182, 0], true, Some(LiquidKind::Water)), LiquidKind::Original);
    }

    #[test]
    fn the_flow_follows_the_lava_scroll() {
        use rc_game::water::sea::flow_offset;
        let (a, b) = (MeshSt::Flow { k: 1.0, add: 0.0 }, MeshSt::Flow { k: 2.0, add: 0.5 });
        // Over one second (60 ticks, no wrap) the ST moves by k·(f(60) − f(0)), as the velocity says.
        let d = (flow_offset(60) - flow_offset(0)) * 2.0;
        assert!((flow_velocity(&b).y - d).abs() < 1e-6, "{} vs {d}", flow_velocity(&b).y);
        assert_eq!(flow_velocity(&b).x, 0.0);
        assert_eq!(flow_velocity(&MeshSt::Stored { scale: 1.0 }), Vec2::ZERO);
        // The second pass against the first: one more k, half a tile apart.
        let f = layer_flow(Some(&a), Some(&b));
        assert!((f.y - flow_velocity(&a).y).abs() < 1e-7 && f.x == 0.0 && f.z == 0.5);
        // w: the first layer's own speed (the fine detail rides it); a still grid has none.
        assert_eq!(f.w, flow_velocity(&a).y);
        assert_eq!(layer_flow(Some(&a), None), DRIFT.with_w(flow_velocity(&a).y));
        assert_eq!(layer_flow(None, None), DRIFT);
        assert_eq!(layer_flow(Some(&MeshSt::Stored { scale: 1.0 }), None), DRIFT);
    }

    #[test]
    fn the_shaders_keep_the_fine_lava_constants() {
        let look = include_str!("../assets/shaders/liquid_look.wgsl");
        let v = LAVA_FINE;
        let line = format!("const LAVA_FINE: vec4<f32> = vec4<f32>({:?}, {:?}, {:?}, {:?});", v.x, v.y, v.z, v.w);
        assert!(look.contains(&line), "{line}");
        // The fine detail's frequency stays under the noise's own cells at the far fade (two pixels).
        assert!(LAVA_FINE.x > 0.3 && LAVA_FINE.x < 2.0 && LAVA_FINE.y > 1.0);
    }
}
