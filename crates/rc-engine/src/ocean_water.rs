//! **Port-only (Water Enhanced, crate::graphics; docs/plan/graphics_options.md "Water"): Pokitaru's ocean.** The ocean
//! 1111 (crate::sea_render: a mixed band and two additive layers over the bed, no surface of its own) has nothing for
//! crate::sea_water to take, so with the option on and the camera above the sea this module draws a surface in place of
//! the band and the layers (the wall, drawn while the camera is high, stays Original), through `water.wgsl`'s Enhanced
//! path (kind 0, crate::sea_water::SeaWaterMaterial: refraction of the bed, absorption, caustics, the screen-space
//! reflection and its Fresnel term, the glint, the shore foam; the planar reflection, crate::water_mirror, through
//! [`OceanWater`]`::plane`, so the islands and the tower reflect at grazing views):
//! * a flat mesh at the ocean's resting z (`base_z`) in [`TILE`]-unit world tiles, each made once and kept: those
//!   within the swell's reach of the camera split to [`FINE`] units, the others to [`COARSE`] out to the band's reach.
//!   The mesh is re-joined only when that set changes; the vertex shader displaces it by the swell (crate::water_waves,
//!   times [`SWELL`]) and lifts it by the game's bob (`sea_z − base_z`: the `frame` record's z blend, z1 = z + 1);
//! * per fine point, the swell's shore damping from the level collision (crate::water_waves::ShoreMap; the coarse tiles,
//!   beyond the displacement, are open water);
//! * one colour, [`OPEN`]: the Original's open water. The only bed the art draws is the sand shelves and the islands'
//!   skirts; between them the scene copy holds the clear colour (no depth), which the shader reads as deep water, so
//!   the open sea is [`OPEN`] (times [`DEEP`]) and the shelves show through it, turning turquoise with the path
//!   through the water (the collision holds too little of the bed for a depth map: most probes meet nothing);
//! * `water.wgsl`'s ocean look (`params.misc.w` = 1, its OCEAN_* constants): an absorption that keeps the sand plain
//!   to a few units down ([`ABSORB`]), the sky in the scene copy (else [`SKY`]) for a reflection that meets nothing
//!   (the grid seas take the fog colour, on Pokitaru a navy that darkened the whole sea), softer ripples and crests, a
//!   lower Fresnel floor, a smaller and softer glint and a faint foam line; the bed kept farther out ([`FAR_FADE`]).
//!
//! A camera under the sea keeps the Original layers. The game's sea z, its collision and swimming are untouched.

use crate::game_camera::TfragFog;
use crate::sea_water::{SeaWaterMaterial, SEA_BAND, SWELL_TAG};
use crate::water_render::{WaterLook, WaterParams, ATTRIBUTE_WATER_TAG, ATTRIBUTE_WATER_UV, ATTRIBUTE_WATER_Z1};
use crate::water_waves::ShoreMap;
use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::render::storage::ShaderBuffer;
use std::collections::HashMap;

/// World tile side, and the split of the near and far tiles (game units).
pub const TILE: f32 = 64.0;
pub const FINE: f32 = 2.0;
pub const COARSE: f32 = 16.0;
/// A tile is fine while its nearest point is within this of the camera (the displacement's end plus a tick's travel).
pub const FINE_REACH: f32 = crate::water_waves::SWELL_FADE[1] + crate::water_waves::TESS_MARGIN;
/// The swell's scale over crate::water_waves::scale (open water; the shore damping calms it as on the grid seas).
pub const SWELL: f32 = 1.0;
/// The water's colour (RGB bytes): the Original's open water as drawn (the band at its 0x60 alpha over the GS clear
/// colour, plus the layers), sampled from Original frames over the lagoon, taken back out of the level fog, its red
/// lowered for the sky the surface reflects on top.
pub const OPEN: [u8; 3] = [0x18, 0xbc, 0xb0];
/// A reflected ray that meets nothing and leaves the screen or the sky: the sky's colour above the horizon (RGB bytes,
/// `params.ids.w`; the grid seas take the fog colour, Pokitaru's navy).
pub const SKY: [u8; 3] = [0x84, 0xbc, 0xe0];
/// `params.misc.x` (the FIX, which the Enhanced path reads only for the absorption): the ocean's rate
/// (`water.wgsl` OCEAN_ABSORB) times this, the sand clear to a few units down and turquoise beyond.
pub const ABSORB: f32 = 0.15;
/// The far fade's start and end (view depth; crate::sea_water::FAR_FADE for the grid seas), the deep share of the
/// colour (crate::sea_water::DEEP): with no bed behind (the open water shows the clear colour), the colour times this.
pub const FAR_FADE: [f32; 2] = [120.0, 300.0];
pub const DEEP: f32 = 0.92;
/// Before the grid seas' draws (crate::sea_water: `SEA_BAND + k`).
const ORDER: f32 = SEA_BAND - 1.0;
/// `water.wgsl`'s `frame` record (crate::water_render `FRAME_RECORD`).
const FRAME_RECORD: usize = 80;

/// The ocean of this tick (crate::sea_render), when Enhanced draws it.
#[derive(Clone, Copy, Debug)]
pub struct OceanFrame {
    /// The camera (game).
    pub cam: [f32; 3],
    /// The resting sea z and this tick's (with the bob).
    pub base_z: f32,
    pub sea_z: f32,
    /// The band's reach (pvar `REACH`): the mesh's extent around the camera.
    pub reach: f32,
    pub fog: TfragFog,
}

/// RGB bytes as the vertex colour / `params.ids.w` word (R low; the alpha 0x80, unread).
pub fn rgba(c: [u8; 3]) -> u32 { (0..3).fold(0x8000_0000, |a, k| a | (c[k] as u32) << (8 * k)) }

/// One tile's points (game), tags and triangles.
#[derive(Default)]
struct Tile {
    pos: Vec<[f32; 3]>,
    tag: Vec<[u32; 2]>,
    idx: Vec<u32>,
}

/// Tile `(i, j)` at water `z`, split to [`FINE`] units with the shore damping from the lattice, else to [`COARSE`]
/// undamped.
fn tile(i: i32, j: i32, fine: bool, z: f32, shore: &mut ShoreMap, probe: &mut dyn FnMut([f32; 3], [f32; 3]) -> Option<f32>) -> Tile {
    let mut t = Tile::default();
    let s = if fine { FINE } else { COARSE };
    let n = (TILE / s) as u32;
    for b in 0..=n {
        for a in 0..=n {
            let (x, y) = (i as f32 * TILE + a as f32 * s, j as f32 * TILE + b as f32 * s);
            let damp = if fine { shore.damp(x, y, z, probe) } else { 1.0 };
            t.pos.push([x, y, z]);
            t.tag.push([rgba(OPEN), SWELL_TAG | ((damp * 65535.0).round() as u32) << 8]);
        }
    }
    let at = |a: u32, b: u32| b * (n + 1) + a;
    for b in 0..n {
        for a in 0..n {
            t.idx.extend([at(a, b), at(a + 1, b), at(a, b + 1), at(a + 1, b), at(a + 1, b + 1), at(a, b + 1)]);
        }
    }
    t
}

/// The tiles around the camera (game xy) within `reach`, each with whether it is fine.
pub fn tile_set(cam: [f32; 2], reach: f32) -> Vec<(i32, i32, bool)> {
    let r = (reach / TILE).ceil() as i32 + 1;
    let (ci, cj) = ((cam[0] / TILE).floor() as i32, (cam[1] / TILE).floor() as i32);
    let mut out = Vec::new();
    for j in cj - r..=cj + r {
        for i in ci - r..=ci + r {
            let (x0, y0) = (i as f32 * TILE, j as f32 * TILE);
            let dx = (x0 - cam[0]).max(cam[0] - x0 - TILE).max(0.0);
            let dy = (y0 - cam[1]).max(cam[1] - y0 - TILE).max(0.0);
            let d = (dx * dx + dy * dy).sqrt();
            if d < reach { out.push((i, j, d < FINE_REACH)); }
        }
    }
    out
}

/// Entity, mesh, material, `frame` record.
type Slot = (Entity, Handle<Mesh>, Handle<SeaWaterMaterial>, Handle<ShaderBuffer>);

/// The Enhanced ocean's entity, mesh, material, `frame` record, tiles and shore damping.
#[derive(Resource, Default)]
pub struct OceanWater {
    /// This tick's ocean (crate::sea_render, once per tick): `Some(None)` when Enhanced draws none.
    pub pending: Option<Option<OceanFrame>>,
    /// The drawn surface as a flat region for the planar reflection (crate::water_mirror's plane choice): this tick's
    /// sea z (the bob in it; the swell stays within the mirror's 0.5) over the band's reach around the camera.
    pub plane: Option<crate::water_mirror::FlatWater>,
    draw: Option<Slot>,
    shown: bool,
    tiles: HashMap<(i32, i32, bool), Tile>,
    set: Vec<(i32, i32, bool)>,
    /// The water z the tiles were made for.
    made_for: Option<u32>,
    shore: ShoreMap,
    copy_size: UVec2,
    reported: bool,
}

/// The resource (from crate::sea_render's plugin; the draw runs after its own).
pub fn build(app: &mut App) {
    app.init_resource::<OceanWater>().add_systems(crate::level_switch::LevelUnload, crate::level_switch::reset::<OceanWater>);
}

fn image(images: &mut Assets<Image>) -> Handle<Image> {
    // 0x80: 1.0 under MODULATE, so the vertex colour is the water's colour.
    let mut img = Image::new(Extent3d { width: 1, height: 1, depth_or_array_layers: 1 }, TextureDimension::D2, vec![0x80; 4], TextureFormat::Rgba8Unorm, RenderAssetUsages::RENDER_WORLD);
    img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor { address_mode_u: ImageAddressMode::Repeat, address_mode_v: ImageAddressMode::Repeat, ..default() });
    images.add(img)
}

/// This tick's ocean into the mesh (re-joined when the tile set changes), the bob into the `frame` record; every
/// frame, the scene copy's size and the key light into the material.
#[allow(clippy::too_many_arguments)]
pub fn draw(
    mut commands: Commands,
    state: ResMut<OceanWater>,
    copy: Res<crate::scene_copy::SceneCopy>,
    mirror: Res<crate::water_mirror::WaterMirror>,
    key: Option<Res<crate::sun_shadow::KeyLight>>,
    water: Option<Res<crate::water_render::WaterState>>,
    level: Res<crate::Level>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<SeaWaterMaterial>>,
    mut buffers: ResMut<Assets<ShaderBuffer>>,
    mut vis: Query<&mut Visibility>,
) {
    let st = state.into_inner();
    let look = WaterLook {
        misc: Vec4::new(1.0, FAR_FADE[0], FAR_FADE[1], DEEP),
        sun: key.map_or(Vec3::Y, |k| k.to_light).extend(crate::water_waves::scale() * SWELL),
    };
    let resized = copy.size != st.copy_size;
    st.copy_size = copy.size;
    if let Some((_, _, mat, _)) = &st.draw {
        let moved = materials.get(mat).is_some_and(|m| m.look.sun.distance(look.sun) > 0.01);
        if resized || moved {
            if let Some(mut m) = materials.get_mut(mat) { m.look = look; }
        }
    }
    let Some(pending) = st.pending.take() else { return };
    st.plane = pending.map(|o| crate::water_mirror::FlatWater { z: o.sea_z, min: Vec2::new(o.cam[0] - o.reach, o.cam[1] - o.reach), max: Vec2::new(o.cam[0] + o.reach, o.cam[1] + o.reach) });
    let Some(o) = pending else {
        if st.shown {
            st.shown = false;
            if let Some((e, ..)) = &st.draw { if let Ok(mut v) = vis.get_mut(*e) { *v = Visibility::Hidden; } }
        }
        return;
    };
    if st.made_for != Some(o.base_z.to_bits()) {
        st.made_for = Some(o.base_z.to_bits());
        st.tiles.clear();
        st.set.clear();
        println!("ocean water: Enhanced surface at z {} (reach {})", o.base_z, o.reach);
    }
    let set = tile_set([o.cam[0], o.cam[1]], o.reach);
    let rebuild = set != st.set;
    if rebuild {
        let coll = level.0.collision.as_ref();
        let mut probe = |a, b| crate::water_waves::probe_line(coll, a, b);
        let t0 = std::time::Instant::now();
        let mut made = 0;
        for &(i, j, fine) in &set {
            if st.tiles.contains_key(&(i, j, fine)) { continue; }
            let t = tile(i, j, fine, o.base_z, &mut st.shore, &mut probe);
            st.tiles.insert((i, j, fine), t);
            made += 1;
        }
        let (mut pos, mut tag, mut idx) = (Vec::new(), Vec::new(), Vec::new());
        for k in &set {
            let t = &st.tiles[k];
            let base = pos.len() as u32;
            pos.extend_from_slice(&t.pos);
            tag.extend_from_slice(&t.tag);
            idx.extend(t.idx.iter().map(|i| i + base));
        }
        st.set = set;
        if !st.reported || made > 4 {
            st.reported = true;
            let [n, above, under] = st.shore.stats;
            println!("ocean water: {made} tiles made, {} points / {} triangles joined in {:.1} ms; shore lattice {n} probes ({above} land, {under} bed)", pos.len(), idx.len() / 3, t0.elapsed().as_secs_f64() * 1e3);
        }
        let z1: Vec<f32> = pos.iter().map(|p| p[2] + 1.0).collect();
        let uv = vec![[0.0f32; 4]; pos.len()];
        let fill = |m: &mut Mesh| {
            m.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
            m.insert_attribute(ATTRIBUTE_WATER_Z1, z1);
            m.insert_attribute(ATTRIBUTE_WATER_UV, uv);
            m.insert_attribute(ATTRIBUTE_WATER_TAG, VertexAttributeValues::Uint32x2(tag));
            m.insert_indices(Indices::U32(idx));
        };
        match &st.draw {
            Some((_, mesh, ..)) => {
                if let Some(mut m) = meshes.get_mut(mesh) { fill(&mut m); }
            }
            None => {
                let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
                fill(&mut m);
                let mesh = meshes.add(m);
                let frame = buffers.add(ShaderBuffer::new(&[0u8; FRAME_RECORD], RenderAssetUsages::default()));
                let rings = water.as_ref().map_or_else(|| buffers.add(ShaderBuffer::new(&[0u8; 16 * 17], RenderAssetUsages::default())), |w| w.rings_buffer());
                let mat = materials.add(SeaWaterMaterial {
                    texture: image(&mut images),
                    fog: o.fog,
                    params: WaterParams { misc: Vec4::new(ABSORB, 0.0, 0.0, 1.0), ids: UVec4::new(0, 0, 0, rgba(SKY)) },
                    frame: frame.clone(),
                    ripple: rings.clone(),
                    scene_depth: copy.depth.clone(),
                    scene_color: copy.color.clone(),
                    look,
                    rings,
                    mirror: mirror.params.clone(),
                    mirror_color: mirror.image.clone(),
                    order: ORDER,
                });
                let e = commands.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(mat.clone()), Transform::IDENTITY, NoFrustumCulling, Visibility::Inherited, Name::new("ocean water"))).id();
                st.draw = Some((e, mesh, mat, frame));
                st.shown = true;
            }
        }
    }
    let Some((e, _, mat, frame)) = &st.draw else { return };
    // The bob: z = z0·w + z1·(1 − w) = base + (1 − w).
    let mut rec = [0u8; FRAME_RECORD];
    rec[..4].copy_from_slice(&(1.0 - (o.sea_z - o.base_z)).to_le_bytes());
    crate::asset_write::set_buffer(&mut buffers, frame, &rec);
    if materials.get(mat).is_some_and(|m| m.fog != o.fog) {
        if let Some(mut m) = materials.get_mut(mat) { m.fog = o.fog; }
    }
    if !st.shown {
        st.shown = true;
        if let Ok(mut v) = vis.get_mut(*e) { *v = Visibility::Inherited; }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgba_packs_r_low() {
        assert_eq!(rgba([0x3c, 0xb8, 0xae]), 0x80ae_b83c);
        let c = rgba(OPEN);
        assert!((c >> 8) & 0xff > c & 0xff && (c >> 16) & 0xff > c & 0xff, "turquoise: green and blue over red");
    }

    #[test]
    fn the_fine_tiles_cover_the_displaced_water() {
        let cam = [500.3, 371.9];
        let set = tile_set(cam, 750.0);
        let at = |x: f32, y: f32| set.iter().find(|t| t.0 == (x / TILE).floor() as i32 && t.1 == (y / TILE).floor() as i32).copied();
        for k in 0..64 {
            let a = k as f32 * std::f32::consts::TAU / 64.0;
            for r in [0.0, 30.0, 60.0, crate::water_waves::SWELL_FADE[1] + 4.0] {
                let p = (cam[0] + r * a.cos(), cam[1] + r * a.sin());
                assert!(at(p.0, p.1).is_some_and(|t| t.2), "{p:?}");
            }
            let far = (cam[0] + 700.0 * a.cos(), cam[1] + 700.0 * a.sin());
            assert!(at(far.0, far.1).is_some_and(|t| !t.2), "{far:?}");
        }
        assert!(set.iter().all(|&(i, j, _)| ((i as f32 + 0.5) * TILE - cam[0]).hypot((j as f32 + 0.5) * TILE - cam[1]) < 750.0 + TILE));
    }

    #[test]
    fn tiles_share_their_edges() {
        let mut shore = ShoreMap::default();
        let mut probe = |_: [f32; 3], _: [f32; 3]| None;
        let a = tile(0, 0, true, 222.0, &mut shore, &mut probe);
        let b = tile(1, 0, true, 222.0, &mut shore, &mut probe);
        let edge = |t: &Tile, x: f32| { let mut v: Vec<[i32; 2]> = t.pos.iter().filter(|p| p[0] == x).map(|p| [p[0] as i32, (p[1] * 8.0) as i32]).collect(); v.sort_unstable(); v };
        assert_eq!(edge(&a, TILE), edge(&b, TILE));
        assert_eq!(a.idx.len(), 32 * 32 * 6);
        assert!(a.tag.iter().all(|t| t[1] == SWELL_TAG | 65535 << 8), "open water: undamped");
    }
}
