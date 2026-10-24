//! **Port-only (Water Enhanced, crate::graphics; docs/plan/graphics_options.md "Water"): the clear seas.** With the
//! option on, crate::sea_render hands over the opaque draws of the liquid ports ([`liquid_port`]: the liquid grid
//! module's users, 854's grid set and 1901's FIX 0x7f strips) whose image is clear water (crate::water_render::clear_rgba
//! by the average colour: lava, mud and sludge stay Original) and whose surface the camera is above ([`from_above`]: a
//! camera under it keeps the Original surface). Their effect layers (FIX < 0x61, 1901's additive pass) and the
//! effect-only seas (the ocean 1111, the pool overlays) stay Original, drawn over the Enhanced surface as before.
//!
//! Each draw is the strip water's Enhanced path (`water.wgsl` kind 0; this module's one `frame` record holds w = 1, no
//! scroll and no wobble, so the vertex shader passes the strip's points, ST and RGBA through) instead of the opaque
//! pass's world surface:
//! * after the scene copy (crate::scene_copy), so the bed and the mobys under the surface are in the copied colour and
//!   depth (refraction, absorption toward the water's colour, caustics, the reflection's trace);
//! * first in Transparent3d ([`SEA_BAND`], before crate::moby_render::CASTER_BAND's late caster draws) and writing Z
//!   (`GsPass::LateOpaque`), as the game's FIX 0x80 strip does: what is drawn later (the shadow casters, the translucent
//!   moby parts, the effects, the particles) is cut by the surface as in Original;
//! * with the far fade ([`FAR_FADE`], `look.misc.yz`): the depth behind distant water is thin and the bent bed seen
//!   through it flickered from frame to frame, so from the fade's start to its end the bed, the shore and the foam give
//!   way to the water's own colour (the original's opaque sea) under the reflection and the glint;
//! * the contact rings of crate::water_render (its buffer while the level has strip or ripple water, else none);
//! * the planar reflection (crate::water_mirror): each flat group's plane ([`SeaWaterDraw`]`::planes`, Rilgar's 879
//!   grid at one height) is a candidate for the mirror view, read on the surface instead of the screen-space trace.

use crate::fx_draw::PrimBuf;
use crate::game_camera::TfragFog;
use crate::gs_state::GsPass;
use crate::water_render::{WaterLook, WaterParams, ATTRIBUTE_WATER_TAG, ATTRIBUTE_WATER_UV, ATTRIBUTE_WATER_Z1};
use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::mesh::{Indices, MeshVertexBufferLayoutRef, PrimitiveTopology, VertexAttributeValues};
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, RenderPipelineDescriptor, SpecializedMeshPipelineError};
use bevy::render::storage::ShaderBuffer;
use bevy::shader::ShaderRef;
use rc_game::water::sea::SeaKind;

const SHADER_PATH: &str = "shaders/water.wgsl";
/// Transparent3d sort bias: before the casters' late draws (crate::moby_render::CASTER_BAND, −30000; the bands are
/// further apart than any view depth).
pub const SEA_BAND: f32 = -40000.0;
/// The far fade's start and end, view depth in game units (the grid draws its blocks out to 400, its fog runs 9.8 →
/// 254).
pub const FAR_FADE: [f32; 2] = [40.0, 110.0];
const _: () = assert!(FAR_FADE[0] > 0.0 && FAR_FADE[0] < FAR_FADE[1] && FAR_FADE[1] < 400.0);
/// The deep water's share of the water's colour (`look.misc.w`; the strip water's 0.55 left the deep seas far darker
/// than their opaque Original surface).
pub const DEEP: f32 = 0.85;
/// `water.wgsl`'s `frame` record (crate::water_render `FRAME_RECORD`): w = 1, no scroll, no wobble.
const FRAME_RECORD: usize = 80;

/// Whether a sea port's opaque draws are liquid surfaces the Enhanced look may take (module doc); the rest (window glass,
/// reflections, two-texture strips, the effect-only seas) are not.
pub fn liquid_port(kind: SeaKind) -> bool { matches!(kind, SeaKind::Grid(_) | SeaKind::GridSet | SeaKind::Hoven) }

/// Whether the camera (game z) is above every point of a draw (a camera under the surface keeps the Original one).
pub fn from_above(prims: &PrimBuf, cam_z: f32) -> bool {
    // Bevy y = game z.
    prims.parts().0.iter().all(|p| cam_z >= p[1])
}

/// One Enhanced draw of this tick: the image (as crate::sea_render resolved it), the fog and the primitives.
pub struct SeaGroup {
    pub image: Handle<Image>,
    pub fog: TfragFog,
    pub prims: PrimBuf,
}

/// The bindings of crate::water_render::WaterMaterial (`water.wgsl`), drawn first in Transparent3d with Z (module doc).
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct SeaWaterMaterial {
    #[texture(0)]
    #[sampler(1)]
    pub texture: Handle<Image>,
    #[uniform(2)]
    pub fog: TfragFog,
    #[uniform(3)]
    pub params: WaterParams,
    #[storage(4, read_only, visibility(vertex))]
    pub frame: Handle<ShaderBuffer>,
    /// Unread by kind 0.
    #[storage(5, read_only, visibility(vertex))]
    pub ripple: Handle<ShaderBuffer>,
    #[texture(6, sample_type = "float", filterable = false)]
    pub scene_depth: Handle<Image>,
    #[texture(7)]
    #[sampler(8)]
    pub scene_color: Handle<Image>,
    #[uniform(9)]
    pub look: WaterLook,
    #[storage(10, read_only, visibility(fragment))]
    pub rings: Handle<ShaderBuffer>,
    #[storage(11, read_only, visibility(fragment))]
    pub mirror: Handle<ShaderBuffer>,
    #[texture(12)]
    #[sampler(13)]
    pub mirror_color: Handle<Image>,
    pub order: f32,
}

impl Material for SeaWaterMaterial {
    fn vertex_shader() -> ShaderRef { SHADER_PATH.into() }
    fn fragment_shader() -> ShaderRef { SHADER_PATH.into() }
    fn alpha_mode(&self) -> AlphaMode { AlphaMode::Blend }
    fn depth_bias(&self) -> f32 { self.order }
    fn enable_prepass() -> bool { false }
    fn enable_shadows() -> bool { false }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = None;
        GsPass::LateOpaque.specialize(descriptor);
        descriptor.vertex.buffers = vec![layout.0.get_layout(&[
            Mesh::ATTRIBUTE_POSITION.at_shader_location(0),
            ATTRIBUTE_WATER_Z1.at_shader_location(1),
            ATTRIBUTE_WATER_UV.at_shader_location(2),
            ATTRIBUTE_WATER_TAG.at_shader_location(3),
        ])?];
        Ok(())
    }
}

/// The water layout of a draw's primitives: game-space points (z1 = z), (s, t, s, t), (RGBA, 0).
fn water_mesh(prims: &PrimBuf, mesh: &mut Mesh) {
    let (pos, uv, color, idx) = prims.parts();
    // Bevy (x, y, z) = game (x, z, −y); the RGBA bytes were stored / 128 (both exact).
    let game: Vec<[f32; 3]> = pos.iter().map(|p| [p[0], -p[2], p[1]]).collect();
    let z1: Vec<f32> = game.iter().map(|p| p[2]).collect();
    let uv4: Vec<[f32; 4]> = uv.iter().map(|t| [t[0], t[1], t[0], t[1]]).collect();
    let tag: Vec<[u32; 2]> = color.iter().map(|c| [c.iter().enumerate().fold(0u32, |a, (k, x)| a | ((x * 128.0).round() as u32 & 0xff) << (8 * k)), 0]).collect();
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, game);
    mesh.insert_attribute(ATTRIBUTE_WATER_Z1, z1);
    mesh.insert_attribute(ATTRIBUTE_WATER_UV, uv4);
    mesh.insert_attribute(ATTRIBUTE_WATER_TAG, VertexAttributeValues::Uint32x2(tag));
    mesh.insert_indices(Indices::U32(idx.to_vec()));
}

type Slot = (Entity, Handle<Mesh>, Handle<SeaWaterMaterial>, bool);

/// The Enhanced sea draws: this tick's groups (crate::sea_render fills `pending` once per tick), the slots, the buffers.
#[derive(Resource, Default)]
pub struct SeaWaterDraw {
    pub pending: Option<Vec<SeaGroup>>,
    /// The flat regions of the last tick's groups (crate::water_mirror's plane choice).
    pub planes: Vec<crate::water_mirror::FlatWater>,
    slots: Vec<Option<Slot>>,
    /// The `frame` record and an empty buffer (the unread `ripple`, the `rings` without strip water).
    buffers: Option<(Handle<ShaderBuffer>, Handle<ShaderBuffer>)>,
    copy_size: UVec2,
}

/// The material plugin and the draw (from crate::sea_render's plugin, after its draw).
pub fn build(app: &mut App) {
    app.add_plugins(MaterialPlugin::<SeaWaterMaterial>::default())
        .init_resource::<SeaWaterDraw>()
        .add_systems(crate::level_switch::LevelUnload, crate::level_switch::reset::<SeaWaterDraw>);
}

/// This tick's groups into the slots; every frame, the scene copy's size and the key light into the materials.
#[allow(clippy::too_many_arguments)]
pub fn draw(
    mut commands: Commands,
    state: ResMut<SeaWaterDraw>,
    copy: Res<crate::scene_copy::SceneCopy>,
    mirror: Res<crate::water_mirror::WaterMirror>,
    key: Option<Res<crate::sun_shadow::KeyLight>>,
    water: Option<Res<crate::water_render::WaterState>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<SeaWaterMaterial>>,
    mut buffers: ResMut<Assets<ShaderBuffer>>,
    mut vis: Query<&mut Visibility>,
) {
    let st = state.into_inner();
    let look = WaterLook { misc: Vec4::new(1.0, FAR_FADE[0], FAR_FADE[1], DEEP), sun: key.map_or(Vec3::Y, |k| k.to_light).extend(0.0) };
    // A resized copy re-binds (Bevy keeps a material's bind group until it changes), a moved key light re-uploads.
    let resized = copy.size != st.copy_size;
    st.copy_size = copy.size;
    for (_, _, mat, _) in st.slots.iter().flatten() {
        let moved = materials.get(mat).is_some_and(|m| m.look.sun.distance(look.sun) > 0.01);
        if resized || moved {
            if let Some(mut m) = materials.get_mut(mat) { m.look = look; }
        }
    }
    let Some(groups) = st.pending.take() else { return };
    let (frame, empty) = st
        .buffers
        .get_or_insert_with(|| {
            let mut rec = [0u8; FRAME_RECORD];
            rec[..4].copy_from_slice(&1.0f32.to_le_bytes());
            (buffers.add(ShaderBuffer::new(&rec, RenderAssetUsages::default())), buffers.add(ShaderBuffer::new(&[0u8; 16 * 17], RenderAssetUsages::default())))
        })
        .clone();
    let rings = water.map_or_else(|| empty.clone(), |w| w.rings_buffer());
    // Bevy (x, y, z) = game (x, z, −y).
    st.planes = groups.iter().filter_map(|g| crate::water_mirror::flat_region(g.prims.parts().0.iter().map(|p| [p[0], -p[2], p[1]]))).collect();
    let n = groups.len();
    for (k, g) in groups.into_iter().enumerate() {
        if st.slots.len() <= k { st.slots.push(None); }
        if g.prims.is_empty() {
            hide(&mut vis, &mut st.slots[k]);
            continue;
        }
        if let Some((e, mesh, mat, shown)) = &mut st.slots[k] {
            if let Some(mut m) = meshes.get_mut(&*mesh) { water_mesh(&g.prims, &mut m); }
            let need = materials.get(&*mat).is_none_or(|m| m.texture != g.image || m.fog != g.fog || m.rings != rings);
            if need {
                if let Some(mut m) = materials.get_mut(&*mat) {
                    m.texture = g.image;
                    m.fog = g.fog;
                    m.rings = rings.clone();
                }
            }
            if !*shown {
                *shown = true;
                if let Ok(mut v) = vis.get_mut(*e) { *v = Visibility::Inherited; }
            }
            continue;
        }
        let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
        water_mesh(&g.prims, &mut mesh);
        let mesh = meshes.add(mesh);
        let mat = materials.add(SeaWaterMaterial {
            texture: g.image,
            fog: g.fog,
            params: WaterParams { misc: Vec4::new(1.0, 0.0, 0.0, 0.0), ids: UVec4::ZERO },
            frame: frame.clone(),
            ripple: empty.clone(),
            scene_depth: copy.depth.clone(),
            scene_color: copy.color.clone(),
            look,
            rings: rings.clone(),
            mirror: mirror.params.clone(),
            mirror_color: mirror.image.clone(),
            order: SEA_BAND + k as f32,
        });
        let e = commands.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(mat.clone()), Transform::IDENTITY, NoFrustumCulling, Visibility::Inherited, Name::new(format!("sea water {k}")))).id();
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
    use rc_game::water::sea::{self as gs, PORTS};

    /// `water.wgsl`'s far fade: `smoothstep(start, end, view depth)`.
    fn far_fade(d: f32) -> f32 {
        let t = ((d - FAR_FADE[0]) / (FAR_FADE[1] - FAR_FADE[0])).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    }

    #[test]
    fn liquid_ports_are_the_grids_854_and_1901() {
        let liquid: Vec<&str> = PORTS.iter().filter(|p| liquid_port(p.kind)).map(|p| p.name).collect();
        assert!(liquid.contains(&"879 sea") && liquid.contains(&"1018 sea") && liquid.contains(&"854 Aridia liquids"));
        assert!(liquid_port(PORTS[gs::HOVEN].kind));
        assert!(!liquid_port(PORTS[gs::OCEAN].kind) && !liquid_port(PORTS[gs::ENV_OVERLAY].kind));
        assert!(PORTS.iter().filter(|p| matches!(p.kind, SeaKind::Reflect(_) | SeaKind::Pool(_) | SeaKind::TwoTex(_))).all(|p| !liquid_port(p.kind)));
    }

    #[test]
    fn from_above_needs_the_camera_over_every_point() {
        let mut b = PrimBuf::default();
        b.strip([([0.0, 0.0, 59.5], [0.0; 2], 0x80ff_ffff), ([1.0, 0.0, 59.5], [0.0; 2], 0), ([0.0, 1.0, 61.5], [0.0; 2], 0)]);
        assert!(from_above(&b, 61.5) && from_above(&b, 80.0));
        assert!(!from_above(&b, 60.0) && !from_above(&b, 40.0));
    }

    #[test]
    fn water_mesh_gives_back_the_game_points_and_bytes() {
        let mut b = PrimBuf::default();
        b.strip([([10.0, -20.0, 59.75], [0.5, 2.0], 0x8012_34ff), ([11.0, -20.0, 59.75], [1.5, 2.0], 0x0000_0001), ([10.0, -21.0, 59.75], [0.5, 3.0], 0xffff_ffff)]);
        let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
        water_mesh(&b, &mut m);
        let Some(VertexAttributeValues::Float32x3(p)) = m.attribute(Mesh::ATTRIBUTE_POSITION) else { panic!("positions") };
        assert_eq!(p[0], [10.0, -20.0, 59.75]);
        let Some(VertexAttributeValues::Uint32x2(t)) = m.attribute(ATTRIBUTE_WATER_TAG) else { panic!("tags") };
        assert_eq!(t.iter().map(|t| t[0]).collect::<Vec<_>>(), [0x8012_34ff, 0x0000_0001, 0xffff_ffff]);
        let Some(VertexAttributeValues::Float32x4(uv)) = m.attribute(ATTRIBUTE_WATER_UV) else { panic!("uv") };
        assert_eq!(uv[1], [1.5, 2.0, 1.5, 2.0]);
    }

    #[test]
    fn far_fade_runs_from_clear_to_opaque_inside_the_draw_distance() {
        assert_eq!(far_fade(0.0), 0.0);
        assert_eq!(far_fade(FAR_FADE[0]), 0.0);
        assert_eq!(far_fade(FAR_FADE[1]), 1.0);
        assert_eq!(far_fade(300.0), 1.0);
        let mid = far_fade((FAR_FADE[0] + FAR_FADE[1]) * 0.5);
        assert!((mid - 0.5).abs() < 1e-6);
        let mut last = 0.0;
        for k in 0..=100 {
            let f = far_fade(k as f32 * 1.5);
            assert!(f >= last);
            last = f;
        }
    }
}
