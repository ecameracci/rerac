//! **Port-only: Ratchet's fur** (the Fur option, docs/plan/graphics_options.md "Fur"). Shell-layer fur: [`LAYERS`]
//! copies of his furry triangles, each pushed out along the skinned normal by its share of [`LENGTH`], drawn blended
//! over him with a strand mask whose strands thin and end towards the tips, the alpha tapering with them, and the
//! outer shells bent by a little wind (`moby_fur.wgsl`). The shells are skinned and lit exactly as his own draw: the
//! same `MobyInst` record, joint palette, normal table and `MobyLod` buffers, tagged with his record (`MeshTag`), so they
//! follow every animation, the cutscene actors and the first-person hide (children of his first high-LOD entity,
//! registered by crate::moby_render `ExtraMobys::spawn_split`).
//!
//! Which triangles: those of the textures in [`FUR_TEXTURES`] (by class and texture slot, the same on every level)
//! whose texels read as his fur ([`is_fur_texel`]): his body atlas also holds his gloves, trousers, boots and belt.
//! Built from the class mesh his draw uses (whatever crate::moby_render builds for it), once per class and record set.
//!
//! Original spawns nothing: no shell entity, mesh asset or material exists ([`follow`] spawns them while the option is
//! on and despawns them when it goes off), so the frame is the game's to the byte.
//! `RC_FUR_LOG=1` prints each class's texture slots and the furry triangles found.

use crate::graphics::GraphicsSettings;
use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::mesh::{Indices, MeshTag, MeshVertexAttribute, MeshVertexBufferLayoutRef, PrimitiveTopology, VertexAttributeValues, VertexFormat};
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError};
use bevy::render::storage::ShaderBuffer;
use bevy::shader::ShaderRef;
use rc_formats::texture::Texture;
use std::collections::HashMap;

const SHADER_PATH: &str = "shaders/moby_fur.wgsl";

/// The furry textures: (class, the class's texture slot, what it is).
pub const FUR_TEXTURES: &[(i32, i32, &str)] = &[(0, 0, "Ratchet's body atlas: fur, ears, tail, face; gloves, trousers, boots")];

/// Shells over the skin.
pub const LAYERS: u32 = 10;
/// The outermost shell's distance from the skin (world units; Ratchet stands about 1.5 tall).
pub const LENGTH: f32 = 0.022;
/// Strands per texture unit (his atlas is 256 texels across).
pub const STRANDS: f32 = 420.0;

/// Per vertex: the shell's height, 1/[`LAYERS`]..=1.
pub const ATTRIBUTE_FUR_HEIGHT: MeshVertexAttribute = MeshVertexAttribute::new("FurHeight", 0x4655_5231, VertexFormat::Float32);

/// `slot` of class `class` is listed in [`FUR_TEXTURES`].
pub fn is_fur_slot(class: i32, slot: i32) -> bool { FUR_TEXTURES.iter().any(|e| e.0 == class && e.1 == slot) }

/// `RC_FUR_LOG=1`.
pub fn log_on() -> bool { std::env::var("RC_FUR_LOG").is_ok_and(|v| v.trim() == "1") }

/// A texel of his fur: yellow to orange, saturated and bright (the trousers are green, the gloves and boots a dark
/// brown, the belt grey); the darker stripes in the fur fail it but sit inside furry triangles ([`is_fur_triangle`]).
pub fn is_fur_texel(rgb: [u8; 3]) -> bool {
    let [r, g, b] = rgb.map(|c| c as f32 / 255.0);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    if max < 0.45 || max - min < 0.35 * max { return false; }
    let d = max - min;
    let h = if max == r { 60.0 * ((g - b) / d).rem_euclid(6.0) } else if max == g { 60.0 * ((b - r) / d + 2.0) } else { 60.0 * ((r - g) / d + 4.0) };
    (22.0..=62.0).contains(&h)
}

/// The texel at `uv` (repeating) of `t`'s level 0.
fn texel(t: &Texture, uv: [f32; 2]) -> [u8; 3] {
    let x = ((uv[0].rem_euclid(1.0) * t.width as f32) as u32).min(t.width - 1);
    let y = ((uv[1].rem_euclid(1.0) * t.height as f32) as u32).min(t.height - 1);
    let i = ((y * t.width + x) * 4) as usize;
    [t.rgba[i], t.rgba[i + 1], t.rgba[i + 2]]
}

/// A triangle with UVs `uv` on `t` is furry: at least 60 % of 15 points spread over it (barycentric, corners
/// included) read as fur.
pub fn is_fur_triangle(t: &Texture, uv: [[f32; 2]; 3]) -> bool {
    let mut n = 0;
    let mut fur = 0;
    for i in 0..=4u32 {
        for j in 0..=(4 - i) {
            let (a, b) = (i as f32 / 4.0, j as f32 / 4.0);
            let c = 1.0 - a - b;
            let p = [0, 1].map(|k| uv[0][k] * a + uv[1][k] * b + uv[2][k] * c);
            n += 1;
            if is_fur_texel(texel(t, p)) { fur += 1; }
        }
    }
    fur * 10 >= n * 6
}

/// The shells of `mesh` (a crate::moby_render part: position, UV, skin, vertex id): its furry triangles ([`is_fur_triangle`]
/// on `texture`) [`LAYERS`] times, each copy's vertices carrying its height. None when no triangle is furry.
pub fn shell_mesh(mesh: &Mesh, texture: &Texture, skin: &MeshVertexAttribute) -> Option<(Mesh, usize)> {
    let Some(VertexAttributeValues::Float32x3(pos)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else { return None };
    let Some(VertexAttributeValues::Float32x2(uv)) = mesh.attribute(Mesh::ATTRIBUTE_UV_0) else { return None };
    let Some(VertexAttributeValues::Uint32x4(sk)) = mesh.attribute(skin.id) else { return None };
    let idx: Vec<u32> = mesh.indices()?.iter().map(|i| i as u32).collect();
    let tris: Vec<[u32; 3]> = idx.as_chunks::<3>().0.iter().copied().filter(|t| is_fur_triangle(texture, t.map(|i| uv[i as usize]))).collect();
    if tris.is_empty() { return None; }
    // The furry triangles' vertices, once per shell.
    let mut remap: HashMap<u32, u32> = HashMap::new();
    let mut used = Vec::new();
    for &i in tris.iter().flatten() { remap.entry(i).or_insert_with(|| { used.push(i); (used.len() - 1) as u32 }); }
    let (mut p, mut u, mut s, mut h, mut out) = (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for layer in 1..=LAYERS {
        let base = p.len() as u32;
        for &i in &used {
            p.push(pos[i as usize]);
            u.push(uv[i as usize]);
            s.push(sk[i as usize]);
            h.push(layer as f32 / LAYERS as f32);
        }
        out.extend(tris.iter().flatten().map(|i| base + remap[i]));
    }
    let n = tris.len();
    let m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, p)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, u)
        .with_inserted_attribute(*skin, VertexAttributeValues::Uint32x4(s))
        .with_inserted_attribute(ATTRIBUTE_FUR_HEIGHT, h)
        .with_inserted_indices(Indices::U32(out));
    Some((m, n))
}

/// `x` = 1 while the Fur option is on, `y` = [`LENGTH`], `z` = [`STRANDS`], `w` = [`LAYERS`].
#[derive(Clone, Copy, Debug, Default, ShaderType)]
pub struct FurParams {
    pub misc: Vec4,
}

/// The shells' material: the part's texture and the record set's buffers (crate::moby_render `MobyMaterial`'s).
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct FurMaterial {
    #[texture(0)]
    #[sampler(1)]
    pub texture: Handle<Image>,
    /// The shared fog buffer (crate::game_camera::fog_buffer).
    #[storage(2, read_only)]
    pub fog: Handle<ShaderBuffer>,
    #[storage(3, read_only, visibility(vertex))]
    pub instances: Handle<ShaderBuffer>,
    #[storage(4, read_only, visibility(vertex))]
    pub palette: Handle<ShaderBuffer>,
    #[storage(5, read_only, visibility(vertex))]
    pub normal_table: Handle<ShaderBuffer>,
    #[storage(6, read_only, visibility(vertex))]
    pub lods: Handle<ShaderBuffer>,
    #[uniform(7)]
    pub params: FurParams,
}

impl Material for FurMaterial {
    fn vertex_shader() -> ShaderRef { SHADER_PATH.into() }
    fn fragment_shader() -> ShaderRef { SHADER_PATH.into() }
    fn alpha_mode(&self) -> AlphaMode { AlphaMode::Blend }
    fn enable_prepass() -> bool { false }
    fn enable_shadows() -> bool { false }
    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = None;
        descriptor.vertex.buffers = vec![layout.0.get_layout(&[
            Mesh::ATTRIBUTE_POSITION.at_shader_location(0),
            Mesh::ATTRIBUTE_UV_0.at_shader_location(1),
            crate::moby_render::ATTRIBUTE_MOBY_SKIN.at_shader_location(2),
            ATTRIBUTE_FUR_HEIGHT.at_shader_location(3),
        ])?];
        Ok(())
    }
}

/// A shell entity waiting for its material ([`attach`]): the part's image and the record set's buffers.
#[derive(Component, Clone)]
pub struct FurShell {
    pub texture: Handle<Image>,
    pub instances: Handle<ShaderBuffer>,
    pub palette: Handle<ShaderBuffer>,
    pub normal_table: Handle<ShaderBuffer>,
    pub lods: Handle<ShaderBuffer>,
}

/// Where a moby's shells go: their mesh and material inputs, its record and the draw entity they hang under (a child
/// of his first high-LOD entity, so every hide of his draw hides them too).
#[derive(Clone)]
struct Source {
    mesh: Mesh,
    /// The mesh as an asset once spawned.
    handle: Option<Handle<Mesh>>,
    shell: FurShell,
    tag: u32,
    parent: Entity,
    /// The shell entity while the option is on.
    spawned: Option<Entity>,
}

/// Every registered shell ([`register`]): spawned only while the Fur option is on ([`follow`]), so with Original no
/// entity of the fur exists and the frame is the game's.
#[derive(Resource, Default)]
struct Sources(Vec<Source>);

/// Registers the shells of `mesh` for record `tag` under `parent` (one of the moby's draw entities); [`follow`] spawns
/// and despawns them with the option.
pub fn register(commands: &mut Commands, mesh: Mesh, shell: FurShell, tag: u32, parent: Entity) {
    commands.queue(move |world: &mut World| {
        world.get_resource_or_init::<Sources>().0.push(Source { mesh, handle: None, shell, tag, parent, spawned: None });
    });
}

/// The shells follow the Fur option and their parents: spawned (as children, identity transform) while it is on,
/// despawned when it is off or their parent is gone.
fn follow(mut commands: Commands, g: Res<GraphicsSettings>, mut sources: ResMut<Sources>, mut meshes: ResMut<Assets<Mesh>>, alive: Query<(), With<Visibility>>) {
    let on = g.fur.on();
    sources.0.retain_mut(|s| {
        let parent_alive = alive.get(s.parent).is_ok();
        match (on && parent_alive, s.spawned) {
            (true, None) => {
                let e = commands
                    .spawn((Mesh3d(s.handle.get_or_insert_with(|| meshes.add(s.mesh.clone())).clone()), s.shell.clone(), Transform::IDENTITY, MeshTag(s.tag), NoFrustumCulling, Visibility::Inherited, Name::new("fur")))
                    .id();
                commands.entity(s.parent).add_child(e);
                s.spawned = Some(e);
            }
            (false, Some(e)) => {
                if let Ok(mut c) = commands.get_entity(e) { c.despawn(); }
                s.spawned = None;
            }
            _ => {}
        }
        parent_alive
    });
}

fn params(g: &GraphicsSettings) -> FurParams {
    FurParams { misc: Vec4::new(if g.fur.on() { 1.0 } else { 0.0 }, LENGTH, STRANDS, LAYERS as f32) }
}

/// [`attach`]'s materials by (image, record set).
type MadeMaterials = HashMap<(AssetId<Image>, AssetId<ShaderBuffer>), Handle<FurMaterial>>;

/// Gives each new shell its material, one per (image, record set).
fn attach(
    mut commands: Commands,
    q: Query<(Entity, &FurShell), Without<MeshMaterial3d<FurMaterial>>>,
    g: Res<GraphicsSettings>,
    mut materials: ResMut<Assets<FurMaterial>>,
    mut made: Local<MadeMaterials>,
) {
    for (e, s) in &q {
        let m = made
            .entry((s.texture.id(), s.instances.id()))
            .or_insert_with(|| {
                materials.add(FurMaterial {
                    texture: s.texture.clone(),
                    fog: crate::game_camera::fog_buffer(),
                    instances: s.instances.clone(),
                    palette: s.palette.clone(),
                    normal_table: s.normal_table.clone(),
                    lods: s.lods.clone(),
                    params: params(&g),
                })
            })
            .clone();
        commands.entity(e).insert(MeshMaterial3d(m));
    }
    made.retain(|_, h| materials.contains(h.id()));
}

/// The Fur option into every shell material.
fn sync(g: Res<GraphicsSettings>, mut materials: ResMut<Assets<FurMaterial>>) {
    if !g.is_changed() { return; }
    let want = params(&g).misc;
    let stale: Vec<_> = materials.iter().filter(|(_, m)| m.params.misc != want).map(|(id, _)| id).collect();
    for id in stale { if let Some(mut m) = materials.get_mut(id) { m.params.misc = want; } }
}

pub struct FurPlugin;

impl Plugin for FurPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<FurMaterial>::default()).init_resource::<Sources>().add_systems(PostUpdate, (follow, attach, sync).chain());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fur_texels_by_colour() {
        // His fur (yellow, orange-tan), its dark stripes, his green trousers, the dark brown gloves, the grey belt, white.
        assert!(is_fur_texel([222, 170, 62]) && is_fur_texel([214, 150, 70]));
        assert!(!is_fur_texel([110, 70, 30]) && !is_fur_texel([40, 90, 40]) && !is_fur_texel([70, 45, 30]));
        assert!(!is_fur_texel([150, 150, 150]) && !is_fur_texel([250, 250, 245]));
    }

    #[test]
    fn furry_triangles_and_shells() {
        // A 2×1 texture: fur on the left half, trousers on the right.
        let t = Texture { width: 2, height: 1, rgba: vec![222, 170, 62, 255, 40, 90, 40, 255] };
        assert!(is_fur_triangle(&t, [[0.0, 0.0], [0.4, 0.0], [0.0, 0.9]]));
        assert!(!is_fur_triangle(&t, [[0.6, 0.0], [0.99, 0.0], [0.6, 0.9]]));
        let skin = crate::moby_render::ATTRIBUTE_MOBY_SKIN;
        let mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::MAIN_WORLD)
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0f32; 3]; 6])
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0f32, 0.0], [0.4, 0.0], [0.0, 0.9], [0.6, 0.0], [0.99, 0.0], [0.6, 0.9]])
            .with_inserted_attribute(skin, VertexAttributeValues::Uint32x4(vec![[0; 4]; 6]))
            .with_inserted_indices(Indices::U32(vec![0, 1, 2, 3, 4, 5]));
        let (shells, tris) = shell_mesh(&mesh, &t, &skin).unwrap();
        assert_eq!((tris, shells.count_vertices(), shells.indices().unwrap().len()), (1, 3 * LAYERS as usize, 3 * LAYERS as usize));
        let Some(VertexAttributeValues::Float32(h)) = shells.attribute(ATTRIBUTE_FUR_HEIGHT) else { panic!() };
        assert_eq!((h[0], h[h.len() - 1]), (1.0 / LAYERS as f32, 1.0));
        assert!(is_fur_slot(0, 0) && !is_fur_slot(601, 0));
    }
}
