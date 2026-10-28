//! **Port-only: the Foliage option** (crate::graphics `Foliage`, docs/plan/graphics_options.md): grass on the level's
//! grassy ground and more of its own small plants, drawn on top of the game's picture. Original adds nothing (no
//! entity, no buffer, no planning). Written from the standard GPU grass / scatter technique.
//!
//! * **Placement** ([`plan`], once per level and value, on a worker thread; frame-exact runs wait for it): grass
//!   points over the LOD-0 tfrag triangles where the ground is a turf texture that reads as grass both on its own and
//!   as drawn and the collision mesh agrees (walkable, nothing overhead), none on the space stations
//!   (`plan::NO_FOLIAGE`), each a short blade clump, a little often a flower cluster or a fern
//!   (`plan::Kind`), and copies of the level's small plant shrub classes ([`plant_classes`]: a world
//!   radius of at most [`MAX_PLANT_RADIUS`], swaying or green) around their placed instances. Its inputs are copied out
//!   of the level once ([`build_input`]) and kept for the level, so Moderate ↔ Lush only re-plans; the old plants stay
//!   until the new ones are ready.
//! * **Draw** (`assets/shaders/foliage.wgsl`): one record per plant in a storage buffer, grouped by [`CELL`]-unit
//!   world cells into chunks of [`GRASS_CAP`] clumps / [`FLOWER_CAP`] flower clusters / [`FERN_CAP`] ferns /
//!   [`COPY_CAP`] copies; a chunk is one entity whose mesh is the template (a clump of [`BLADES`] thin curved blades,
//!   [`FLOWER_HEADS`] stems with star heads, [`FRONDS`] serrated arching fronds, or a shrub class part from
//!   crate::shrub_render `class_geometry`) repeated once per slot, `MeshTag` = the chunk's first record, so Bevy draws
//!   every chunk of a template in one instanced draw. Bevy culls each chunk's box against the view ([`Aabb`]);
//!   [`cull_cells`] hides the chunks past the end distance. The shader thins with distance by each plant's rank and
//!   shrinks it to nothing before its cut-off (`plan::thin`), sways it with the shrubs' wind
//!   (`rc_formats::shrub::wind_sway`'s terms on the 60 Hz tick), bends it away from Ratchet and the [`MOVERS`] − 1
//!   drawn mobys nearest the camera ([`movers`]: each a capsule from a lagging tail to its feet, the footprint from
//!   its bounding sphere, so the plant springs back as it passes; visual only), and fogs it as the world shaders.
//!   Opaque and Z-writing (copies alpha-tested on their texture), so ambient occlusion and the Shadows Dynamic receiver,
//!   both read from the depth buffer, apply to it; it casts no sun shadow.
//! * **Colour**: grass from the ground's drawn colour under the clump (`plan::blade_colours`: the root matches the
//!   terrain, the tip lighter; ferns darker, `plan::fern_colours`); flower heads white or yellow round a yellow
//!   centre, dimmed with the ground; copies with their source instance's `LightShrubs` palette and class texture.
//!
//! `RC_GFX_FOLIAGE=original|moderate|lush` sets the option at start (crate::graphics); `RC_FOLIAGE_TRACE` prints
//! where the grass grew by tfrag texture ([`trace`]).

mod plan;

use crate::graphics::{Foliage, GraphicsSettings};
use crate::level_load::LoadedLevel;
use crate::tfrag_render::game_to_bevy;
use bevy::asset::RenderAssetUsages;
use bevy::camera::primitives::Aabb;
use bevy::camera::visibility::NoAutoAabb;
use bevy::mesh::{Indices, MeshTag, MeshVertexAttribute, MeshVertexBufferLayoutRef, PrimitiveTopology, VertexAttributeValues, VertexFormat};
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, Extent3d, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError, TextureDimension, TextureFormat};
use bevy::render::storage::ShaderBuffer;
use bevy::shader::ShaderRef;
use plan::{Density, Kind, Plan, PlanInput, PlantSource, Rng, Tex, Tri};
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

const SHADER_PATH: &str = "shaders/foliage.wgsl";
/// The world cell (game x, y) a chunk's plants share.
pub const CELL: f32 = 8.0;
/// Slots per chunk (the template mesh holds that many): blade clumps, flower clusters, ferns, plant copies.
pub const GRASS_CAP: usize = 64;
pub const FLOWER_CAP: usize = 16;
pub const FERN_CAP: usize = 4;
pub const COPY_CAP: usize = 16;
/// Blades per clump and rows per blade (a tapered strip ending in a point).
pub const BLADES: usize = 12;
const ROWS: usize = 2;
/// Heads per flower cluster; petals per head.
pub const FLOWER_HEADS: usize = 3;
const PETALS: usize = 5;
/// Fronds per fern and rows per frond (alternately wide and narrow: the leaflets).
pub const FRONDS: usize = 7;
const FROND_ROWS: usize = 12;
/// The largest plant shrub copied (world radius of the class's bounding sphere on its instance).
pub const MAX_PLANT_RADIUS: f32 = 1.75;
/// How far over its origin a plant shrub's bounding sphere may start (world units): a class drawn clear of its origin
/// (Orxon's green lamp heads, class 28, over the posts beside the ship's pad) does not grow from the ground, and its
/// copies on the grass float.
pub const MAX_PLANT_LIFT: f32 = 0.15;
/// One record (foliage.wgsl `FoliageInst`).
const RECORD: usize = 96;
/// The grass's wind: ShrubProc's mode 1 (the halved sway).
const GRASS_SWAY: u32 = 1;
/// Record kinds (foliage.wgsl `FoliageInst::info.w & 0xff`).
const KIND_BLADES: u32 = 0;
const KIND_COPY: u32 = 1;
const KIND_FLOWER: u32 = 2;
const KIND_FERN: u32 = 3;
/// The characters the grass bends away from (foliage.wgsl `FoliageFrame::movers`): Ratchet and the drawn mobys nearest
/// the camera, up to this many, with a bounding sphere of at most [`MOVER_MAX_RADIUS`].
pub const MOVERS: usize = 48;
const MOVER_MAX_RADIUS: f32 = 3.0;
/// The bend's footprint: this share of the bounding sphere's radius, clamped.
const FOOTPRINT: (f32, [f32; 2]) = (0.6, [0.3, 1.8]);
/// Ratchet's footprint (from his moby's sphere) is at least this.
const HERO_FOOTPRINT: f32 = 0.7;
/// The trail: each mover's tail follows it with this time constant (seconds) and at most this far behind; Ratchet's
/// is the longer one.
const TAIL_TAU: [f32; 2] = [0.25, 0.12];
const TAIL_MAX: [f32; 2] = [1.6, 0.8];
/// A mover that jumps further than this in one frame (a respawn, a reused slot) starts without a tail.
const TAIL_RESET: f32 = 4.0;

/// Per vertex: the template slot (the record is `MeshTag + slot`).
pub const ATTRIBUTE_FOLIAGE_SLOT: MeshVertexAttribute = MeshVertexAttribute::new("FoliageSlot", 0x464f_4c53, VertexFormat::Uint32);

/// x = kind (0 grass, 1 plant copy), y = MXL, z = near (32 / the mod texture's LOD scale; GS Q = n / z).
#[derive(Clone, Copy, Debug, Default, ShaderType)]
pub struct FoliageParams {
    pub misc: Vec4,
}

/// The foliage material: one for the grass, one per plant texture.
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct FoliageMaterial {
    #[texture(0)]
    #[sampler(1)]
    pub texture: Handle<Image>,
    /// The shared fog buffer (crate::game_camera::fog_buffer).
    #[storage(2, read_only)]
    pub fog: Handle<ShaderBuffer>,
    #[uniform(3)]
    pub params: FoliageParams,
    /// The records ([`RECORD`] bytes each).
    #[storage(4, read_only, visibility(vertex))]
    pub instances: Handle<ShaderBuffer>,
    /// Every shrub instance's 24 `LightShrubs` palette entries (crate::shrub_render `LevelShrubs::palettes`).
    #[storage(5, read_only, visibility(vertex))]
    pub palettes: Handle<ShaderBuffer>,
    /// The per-frame values ([`upload_frame`]).
    #[storage(6, read_only, visibility(vertex))]
    pub frame: Handle<ShaderBuffer>,
}

impl Material for FoliageMaterial {
    fn vertex_shader() -> ShaderRef { SHADER_PATH.into() }
    fn fragment_shader() -> ShaderRef { SHADER_PATH.into() }
    fn alpha_mode(&self) -> AlphaMode { if self.params.misc.x > 0.5 { AlphaMode::Mask(0.5) } else { AlphaMode::Opaque } }
    fn enable_prepass() -> bool { false }
    fn enable_shadows() -> bool { false }
    /// Blades and shrub strips are seen from both sides.
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
            crate::shrub_render::ATTRIBUTE_SHRUB_INFO.at_shader_location(2),
            ATTRIBUTE_FOLIAGE_SLOT.at_shader_location(3),
        ])?];
        Ok(())
    }
}

pub struct FoliagePlugin;

impl Plugin for FoliagePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<FoliageMaterial>::default())
            .init_resource::<FoliageState>()
            .add_systems(crate::level_switch::LevelUnload, crate::level_switch::reset::<FoliageState>)
            .add_systems(Update, (follow_option, cull_cells).chain())
            .add_systems(PostUpdate, upload_frame);
    }
}

/// A placement on its worker thread.
struct Job {
    mode: Foliage,
    handle: Mutex<Option<JoinHandle<Plan>>>,
}

/// The level's foliage: the placement's inputs, the job, what is drawn.
#[derive(Resource, Default)]
struct FoliageState {
    input: Option<Arc<PlanInput>>,
    /// Per [`PlantSource::class`]: its parts (template mesh with [`COPY_CAP`] slots, material key).
    plant_parts: HashMap<usize, Vec<PlantPart>>,
    plant_images: HashMap<(usize, u8), (Handle<Image>, u32, f32)>,
    /// The code-made templates: blades, flowers, ferns ([`Template`]).
    templates: [Option<Handle<Mesh>>; 3],
    white: Option<Handle<Image>>,
    palettes: Option<Handle<ShaderBuffer>>,
    frame: Option<Handle<ShaderBuffer>>,
    job: Option<Job>,
    shown: Option<Foliage>,
    density: Option<Density>,
    entities: Vec<Entity>,
    /// Per moby slot (Ratchet's too): its trail's tail (Bevy world), for [`upload_frame`].
    tails: HashMap<usize, Vec3>,
    last_tick: Option<u32>,
}

/// A code-made template: its index in [`FoliageState::templates`], slots, record kind.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Template {
    Blades,
    Flowers,
    Ferns,
}

impl Template {
    fn of(k: Kind) -> Self {
        match k {
            Kind::Blades => Template::Blades,
            Kind::Flower { .. } => Template::Flowers,
            Kind::Fern => Template::Ferns,
        }
    }
    fn cap(self) -> usize { [GRASS_CAP, FLOWER_CAP, FERN_CAP][self as usize] }
    fn mesh(self) -> Mesh {
        let (pos, uv, idx) = match self {
            Template::Blades => clump_geometry(),
            Template::Flowers => flower_geometry(),
            Template::Ferns => fern_geometry(),
        };
        repeated(&pos, &uv, &vec![0u32; pos.len()], &idx, self.cap())
    }
}

/// A chunk's cell, for [`cull_cells`]: its centre (Bevy x, z) and half size.
#[derive(Component, Clone, Copy)]
struct FoliageCell {
    centre: Vec2,
    half: f32,
}

#[derive(Clone)]
struct PlantPart {
    mesh: Handle<Mesh>,
    texture: usize,
    /// [`wrap_key`], for the image cache.
    wrap: u8,
    sampler: bevy::image::ImageSamplerDescriptor,
}

/// The level's small plant classes: a world radius of at most [`MAX_PLANT_RADIUS`] and a bounding sphere reaching
/// down to within [`MAX_PLANT_LIFT`] of the origin on every instance, not hidden (`mode_bits & 1`), with a mesh
/// (billboard F ≠ 0), and swaying (`mode_bits & 6`) or green on average.
fn plant_classes(level: &LoadedLevel) -> Vec<bool> {
    let s = &level.shrubs;
    let mut small = vec![true; s.classes.len()];
    for (ii, inst) in s.instances.iter().enumerate() {
        if let Some(ci) = s.class_of[ii] {
            let c = rc_formats::shrub_light::instance_centre(&s.classes[ci].class, inst);
            let (r, lift) = (c[3], c[2] - c[3] - inst.matrix[3][2]);
            if !(r > 0.0 && r <= MAX_PLANT_RADIUS) || lift > MAX_PLANT_LIFT { small[ci] = false; }
        }
    }
    s.classes
        .iter()
        .enumerate()
        .map(|(ci, c)| {
            let h = &c.class.header;
            if !small[ci] || h.mode_bits & 1 != 0 || crate::shrub_render::billboard_f(c) == Some(0.0) { return false; }
            let sway = h.mode_bits & 6 != 0;
            let green = class_colour(level, ci).is_some_and(plan::is_grass);
            sway || green
        })
        .collect()
}

/// The average opaque texel of a class's textures (level 0).
fn class_colour(level: &LoadedLevel, ci: usize) -> Option<[u8; 3]> {
    let c = &level.shrubs.classes[ci];
    let mut s = [0u64; 3];
    let mut n = 0u64;
    for d in c.class.packets.iter().flat_map(|p| &p.draws) {
        let Some(l) = c.texture_table_index(d.texture as u16).and_then(|t| level.shrubs.mips.get(&t)).and_then(|m| m.first()) else { continue };
        for px in l.rgba.as_chunks::<4>().0.iter().filter(|p| p[3] >= 0xc0) {
            for k in 0..3 { s[k] += px[k] as u64; }
            n += 1;
        }
    }
    (n > 0).then(|| s.map(|v| (v / n) as u8))
}

/// The placement's inputs, copied out of the loaded level (module docs).
fn build_input(level: &LoadedLevel) -> PlanInput {
    let mut tris = Vec::new();
    for t in &level.tfrags {
        let Ok(list) = rc_formats::tfrag::tfrag_triangles(t, 0) else { continue };
        for tri in list {
            let Ok(tex) = u32::try_from(t.texture_index(tri.ad_gif as usize)) else { continue };
            let vs = [tri.a, tri.b, tri.c].map(|v| v as usize);
            tris.push(Tri {
                p: vs.map(|v| t.world_position(v)),
                uv: vs.map(|v| { let e = t.vertex_info[v]; [e.s as f32 / 4096.0, e.t as f32 / 4096.0] }),
                col: vs.map(|v| t.rgba.get(t.position_index(v)).map_or([0x80; 4], |c| [c.r, c.g, c.b, c.a])),
                tex,
            });
        }
    }
    let textures = level
        .tfrag_lod
        .mips
        .iter()
        .map(|m| m.as_ref().and_then(|l| l.first()).map(|t| Tex { width: t.width, height: t.height, rgba: t.rgba.clone() }))
        .collect();
    let plant = plant_classes(level);
    let s = &level.shrubs;
    let plants = s
        .instances
        .iter()
        .enumerate()
        .filter_map(|(ii, inst)| {
            let ci = s.class_of[ii].filter(|&ci| plant[ci])?;
            let m = inst.matrix;
            Some(PlantSource { class: ci, instance: ii as u32, origin: [m[3][0], m[3][1], m[3][2]], lin: [0, 1, 2].map(|c| [m[c][0], m[c][1], m[c][2]]) })
        })
        .collect();
    PlanInput { level: crate::level_load::level_index(), tris, textures, collision: level.collision.clone(), plants }
}

/// The option into the drawn plants (module docs): Original despawns them; another value plans (once its job is done
/// the plants are swapped in).
#[allow(clippy::too_many_arguments)]
fn follow_option(
    mut commands: Commands,
    gfx: Res<GraphicsSettings>,
    level: Res<crate::Level>,
    det: Option<Res<crate::determinism::Deterministic>>,
    mut st: ResMut<FoliageState>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<FoliageMaterial>>,
    mut buffers: ResMut<Assets<ShaderBuffer>>,
) {
    let want = gfx.foliage;
    let Some(density) = plan::density(want) else {
        if st.shown.is_some() || st.job.is_some() {
            for e in st.entities.drain(..) { commands.entity(e).try_despawn(); }
            st.shown = None;
            st.job = None;
            println!("foliage: off");
        }
        return;
    };
    if st.job.as_ref().is_some_and(|j| j.mode != want) { st.job = None; }
    if st.job.is_none() {
        if st.shown == Some(want) { return; }
        if level.0.collision.is_none() { return; }
        let input = st.input.get_or_insert_with(|| Arc::new(build_input(&level.0))).clone();
        let handle = std::thread::Builder::new().name("foliage plan".into()).spawn(move || plan::plan(&input, &density));
        match handle {
            Ok(h) => st.job = Some(Job { mode: want, handle: Mutex::new(Some(h)) }),
            Err(e) => { eprintln!("foliage: cannot start the placement thread ({e})"); return; }
        }
    }
    let wait = det.is_some_and(|d| d.0);
    let done = {
        let job = st.job.as_ref().unwrap();
        let mut g = job.handle.lock().unwrap_or_else(|e| e.into_inner());
        if !wait && !g.as_ref().is_some_and(|h| h.is_finished()) { return; }
        g.take().map(|h| h.join())
    };
    st.job = None;
    let Some(Ok(plan)) = done else { eprintln!("foliage: the placement thread failed"); return };
    for e in st.entities.drain(..) { commands.entity(e).try_despawn(); }
    let n = spawn_plan(&mut commands, &level.0, &plan, &mut st, &mut meshes, &mut images, &mut materials, &mut buffers);
    let s = &plan.stats;
    if let Some(why) = s.no_foliage {
        println!("foliage: {want:?}: none on this level ({why})");
        st.shown = Some(want);
        st.density = Some(density);
        return;
    }
    println!(
        "foliage: {want:?}: {} grass points ({} flower clusters, {} ferns) of {} candidates ({} steep, {} not grass ({} on a texture \
         that is not, {} green only as lit), {} under a path or decal, {} no ground, {} off the collision surface, {} barren), {} plant \
         copies of {} tries ({} source plants), {n} chunks; placement {:.1} ms",
        s.grass, s.flowers, s.ferns, s.candidates, s.steep, s.not_grass, s.non_grass_texture, s.tinted, s.covered, s.no_ground, s.off_surface, s.barren, s.copies, s.copy_tries,
        st.input.as_ref().map_or(0, |i| i.plants.len()), s.time.as_secs_f64() * 1e3
    );
    if let Some(input) = st.input.as_ref().filter(|_| std::env::var_os("RC_FOLIAGE_TRACE").is_some()) { trace(input, &plan); }
    st.shown = Some(want);
    st.density = Some(density);
}

/// `RC_FOLIAGE_TRACE=1` (the whole level) or `x,y,r` (the points within r of game x, y): per tfrag texture, its
/// grass points and where they lie on average, the raw texture's mean colour, grass-texel and green-lean shares, the
/// points' mean vertex colour and drawn colour and their collision surfaces (dev: why a surface grows grass); with
/// `x,y,r` also every upward ground texture there (area, height, vertex colour, its turf verdicts: why it grows
/// nothing), the plant copies there and their sources.
fn trace(input: &PlanInput, plan: &Plan) {
    let probe: Option<[f32; 3]> = std::env::var("RC_FOLIAGE_TRACE").ok().and_then(|v| {
        let p: Vec<f32> = v.split(',').filter_map(|s| s.trim().parse().ok()).collect();
        (p.len() == 3).then(|| [p[0], p[1], p[2]])
    });
    trace_at(input, plan, None);
    if probe.is_some() { trace_at(input, plan, probe); }
    let Some(p) = probe else { return };
    // The ground there by texture: the upward faces whose centre lies within r (why it grows nothing).
    let mut ground: BTreeMap<u32, (f32, [f32; 4], f32)> = BTreeMap::new();
    for t in &input.tris {
        let c: [f32; 3] = std::array::from_fn(|k| (t.p[0][k] + t.p[1][k] + t.p[2][k]) / 3.0);
        if (c[0] - p[0]).hypot(c[1] - p[1]) > p[2] || !plan::faces_up(t.normal()) { continue; }
        let n = t.normal();
        let a = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt() * 0.5;
        let e = ground.entry(t.tex).or_default();
        e.0 += a;
        for v in &t.col { for (s, c) in e.1.iter_mut().zip(v) { *s += *c as f32 * a / 3.0; } }
        e.2 += c[2] * a;
    }
    for (tex, (area, col, z)) in &ground {
        let raw = input.textures.get(*tex as usize).and_then(|x| x.as_ref()).map(plan::texture_stats);
        let col = col.map(|v| v / area.max(1e-6));
        println!(
            "  ground tex {tex:4} {area:7.1} sq  z {:.1}  vtx {:?}  {}",
            z / area.max(1e-6),
            col.map(|v| v as u8),
            raw.map_or("-".into(), |s| format!(
                "{}x{} mean {:?} grass {:.2} lean {:.2} sd {:.1} turf {} muted turf {} drawn {:?}",
                s.width, s.height, s.mean, s.grass_share, s.lean_share, s.luma_sd, plan::is_turf(&s), plan::is_muted_turf(&s), plan::modulate(s.mean, col)
            ))
        );
    }
    for c in plan.copies.iter().filter(|c| (c.pos[0] - p[0]).hypot(c.pos[1] - p[1]) <= p[2]) {
        let s = &input.plants[c.src];
        println!("  copy of shrub class {} (instance {} at {:?}) at {:?}", s.class, s.instance, s.origin, c.pos);
    }
}

fn trace_at(input: &PlanInput, plan: &Plan, probe: Option<[f32; 3]>) {
    #[derive(Default)]
    struct T { n: usize, col: [f32; 4], drawn: [f32; 3], surf: BTreeMap<i32, usize>, nz: f32, pos: [f32; 3] }
    let mut by: BTreeMap<u32, T> = BTreeMap::new();
    for g in &plan.grass {
        if let Some(p) = probe { if (g.pos[0] - p[0]).hypot(g.pos[1] - p[1]) > p[2] { continue; } }
        let t = &input.tris[g.tri as usize];
        let e = by.entry(t.tex).or_default();
        e.n += 1;
        for v in &t.col { for (s, c) in e.col.iter_mut().zip(v) { *s += *c as f32 / 3.0; } }
        for (s, c) in e.drawn.iter_mut().zip(g.root) { *s += c as f32 / 0.88; }
        *e.surf.entry(g.surface).or_default() += 1;
        for (s, c) in e.pos.iter_mut().zip(g.pos) { *s += c; }
        let n = t.normal();
        e.nz += n[2].abs() / (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-9);
    }
    let mut list: Vec<_> = by.into_iter().collect();
    list.sort_by_key(|(_, t)| std::cmp::Reverse(t.n));
    println!("foliage trace{}: {} textures", probe.map_or(String::new(), |p| format!(" at {p:?}")), list.len());
    for (tex, t) in list.iter().take(40) {
        let n = t.n as f32;
        let raw = input.textures.get(*tex as usize).and_then(|x| x.as_ref()).map(plan::texture_stats);
        println!(
            "  tex {tex:4} {:5} pts  around {:?}  raw {}  vtx {:?}  drawn {:?}  nz {:.2}  surfaces {:?}",
            t.n,
            t.pos.map(|v| (v / n).round() as i32),
            raw.map_or("-".into(), |s| format!("{}x{} mean {:?} grass {:.2} lean {:.2} sd {:.1}", s.width, s.height, s.mean, s.grass_share, s.lean_share, s.luma_sd)),
            t.col.map(|v| (v / n) as u8),
            t.drawn.map(|v| (v / n) as u8),
            t.nz / n,
            t.surf
        );
    }
}

/// Little-endian record writer (foliage.wgsl `FoliageInst`).
fn record(out: &mut Vec<u8>, pos: Vec3, rank: f32, shape: [f32; 4], info: [u32; 4], lin: Mat3) {
    for v in [pos.x, pos.y, pos.z, rank].into_iter().chain(shape) { out.extend_from_slice(&v.to_le_bytes()); }
    for v in info { out.extend_from_slice(&v.to_le_bytes()); }
    for c in [lin.x_axis, lin.y_axis, lin.z_axis] {
        for v in [c.x, c.y, c.z, 0.0] { out.extend_from_slice(&v.to_le_bytes()); }
    }
}

fn rgba(c: [u8; 3]) -> u32 { u32::from_le_bytes([c[0], c[1], c[2], 0xff]) }

/// `game_to_bevy` as a 3×3 (columns = images of game x, y, z).
fn axes() -> Mat3 { Mat3::from_cols(Vec3::X, Vec3::NEG_Z, Vec3::Y) }

fn cell_of(p: [f32; 3]) -> (i32, i32) { ((p[0] / CELL).floor() as i32, (p[1] / CELL).floor() as i32) }

/// The chunk entities of a plan; returns their count.
#[allow(clippy::too_many_arguments)]
fn spawn_plan(
    commands: &mut Commands,
    level: &LoadedLevel,
    plan: &Plan,
    st: &mut FoliageState,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<FoliageMaterial>,
    buffers: &mut Assets<ShaderBuffer>,
) -> usize {
    let Some(input) = st.input.clone() else { return 0 };
    let white = st
        .white
        .get_or_insert_with(|| {
            images.add(Image::new_fill(Extent3d { width: 1, height: 1, depth_or_array_layers: 1 }, TextureDimension::D2, &[255, 255, 255, 0x80], TextureFormat::Rgba8Unorm, RenderAssetUsages::RENDER_WORLD))
        })
        .clone();
    let palettes = st
        .palettes
        .get_or_insert_with(|| {
            let mut b: Vec<u8> = level.shrubs.palettes.iter().flatten().flatten().copied().collect();
            if b.is_empty() { b = vec![0; 4]; }
            buffers.add(ShaderBuffer::new(&b, RenderAssetUsages::RENDER_WORLD))
        })
        .clone();
    let frame = st.frame.get_or_insert_with(|| buffers.add(ShaderBuffer::new(&[0u8; FRAME_BYTES], RenderAssetUsages::default()))).clone();

    // Chunks: (template, material key, cell, records), records written in cell order.
    struct Chunk { mesh: Handle<Mesh>, mat: Option<(usize, u8)>, cell: (i32, i32), base: u32, lo: Vec3, hi: Vec3 }
    let mut bytes = Vec::new();
    let mut chunks: Vec<Chunk> = Vec::new();
    let mut by_cell: BTreeMap<((i32, i32), Template), Vec<usize>> = BTreeMap::new();
    for (i, g) in plan.grass.iter().enumerate() { by_cell.entry((cell_of(g.pos), Template::of(g.kind))).or_default().push(i); }
    let a = axes();
    for (&(cell, t), list) in &by_cell {
        let mesh = st.templates[t as usize].get_or_insert_with(|| meshes.add(t.mesh())).clone();
        for part in list.chunks(t.cap()) {
            let base = (bytes.len() / RECORD) as u32;
            let (mut lo, mut hi) = (Vec3::MAX, Vec3::MIN);
            for &i in part {
                let g = &plan.grass[i];
                let p = game_to_bevy(g.pos);
                lo = lo.min(p);
                hi = hi.max(p);
                let (s, c) = g.yaw.sin_cos();
                let kind = match g.kind {
                    Kind::Blades => KIND_BLADES,
                    Kind::Flower { yellow } => KIND_FLOWER | (yellow as u32) << 16,
                    Kind::Fern => KIND_FERN,
                };
                record(&mut bytes, p, g.rank, [c, s, g.height, g.width], [rgba(g.root), rgba(g.tip), g.seed, kind | GRASS_SWAY << 8], Mat3::IDENTITY);
            }
            bytes.resize((base as usize + t.cap()) * RECORD, 0);
            // Ferns spread up to their width (under 1) past the root, clumps by their lean; 1 unit covers both.
            chunks.push(Chunk { mesh: mesh.clone(), mat: None, cell, base, lo: lo - Vec3::new(1.0, 0.0, 1.0), hi: hi + Vec3::ONE });
        }
    }
    let mut by_class: BTreeMap<((i32, i32), usize), Vec<usize>> = BTreeMap::new();
    for (i, c) in plan.copies.iter().enumerate() { by_class.entry((cell_of(c.pos), input.plants[c.src].class)).or_default().push(i); }
    for (&(cell, ci), list) in &by_class {
        let parts = st.plant_parts.entry(ci).or_insert_with(|| plant_parts(level, ci, meshes)).clone();
        if parts.is_empty() { continue; }
        let kind = KIND_COPY | ((level.shrubs.classes[ci].class.header.mode_bits as u32 & 6) >> 1) << 8;
        for part in list.chunks(COPY_CAP) {
            let base = (bytes.len() / RECORD) as u32;
            let (mut lo, mut hi) = (Vec3::MAX, Vec3::MIN);
            for &i in part {
                let c = &plan.copies[i];
                let src = &input.plants[c.src];
                let lin = Mat3::from_cols(Vec3::from(src.lin[0]), Vec3::from(src.lin[1]), Vec3::from(src.lin[2]));
                let lin = a * Mat3::from_rotation_z(c.yaw) * lin * c.scale;
                let p = game_to_bevy(c.pos);
                lo = lo.min(p);
                hi = hi.max(p);
                record(&mut bytes, p, c.rank, [1.0, 0.0, c.scale, 1.0], [src.instance, 0, c.seed, kind], lin);
            }
            bytes.resize((base as usize + COPY_CAP) * RECORD, 0);
            for pp in &parts {
                chunks.push(Chunk { mesh: pp.mesh.clone(), mat: Some((pp.texture, pp.wrap)), cell, base, lo: lo - Vec3::splat(MAX_PLANT_RADIUS), hi: hi + Vec3::splat(MAX_PLANT_RADIUS) });
            }
        }
    }
    if bytes.is_empty() { return 0; }
    let instances = buffers.add(ShaderBuffer::new(&bytes, RenderAssetUsages::RENDER_WORLD));
    let mut mats: HashMap<Option<(usize, u8)>, Handle<FoliageMaterial>> = HashMap::new();
    let fog = crate::game_camera::fog_buffer();
    for ch in &chunks {
        let mat = mats
            .entry(ch.mat)
            .or_insert_with(|| {
                let (texture, params) = match ch.mat {
                    None => (white.clone(), Vec4::new(0.0, 0.0, crate::game_camera::NEAR, 0.0)),
                    Some((tex, wrap)) => {
                        let sampler = st.plant_parts.values().flatten().find(|p| p.texture == tex && p.wrap == wrap).map(|p| p.sampler.clone()).unwrap_or_default();
                        let (img, mxl, lod_scale) = st.plant_images.entry((tex, wrap)).or_insert_with(|| plant_image(level, tex, sampler, images, &white)).clone();
                        (img, Vec4::new(1.0, mxl as f32, crate::game_camera::NEAR / lod_scale, 0.0))
                    }
                };
                materials.add(FoliageMaterial { texture, fog: fog.clone(), params: FoliageParams { misc: params }, instances: instances.clone(), palettes: palettes.clone(), frame: frame.clone() })
            })
            .clone();
        let centre = Vec2::new((ch.cell.0 as f32 + 0.5) * CELL, -(ch.cell.1 as f32 + 0.5) * CELL);
        let e = commands
            .spawn((
                Mesh3d(ch.mesh.clone()),
                MeshMaterial3d(mat),
                MeshTag(ch.base),
                Transform::IDENTITY,
                Aabb::from_min_max(ch.lo - Vec3::splat(0.5), ch.hi + Vec3::splat(0.5)),
                NoAutoAabb,
                FoliageCell { centre, half: CELL * 0.5 + MAX_PLANT_RADIUS },
                Visibility::Hidden,
                Name::new(format!("foliage cell {:?} {}", ch.cell, if ch.mat.is_some() { "plants" } else { "grass" })),
            ))
            .id();
        st.entities.push(e);
    }
    chunks.len()
}

/// A shrub texture's image (its mip chain, or a mod's replacement) with its MXL and LOD scale, as crate::shrub_render
/// builds them.
fn plant_image(level: &LoadedLevel, tex: usize, sampler: bevy::image::ImageSamplerDescriptor, images: &mut Assets<Image>, white: &Handle<Image>) -> (Handle<Image>, u32, f32) {
    let Some(levels) = level.shrubs.mips.get(&tex) else { return (white.clone(), 0, 1.0) };
    let mxl = levels.len() as u32 - 1;
    if let Some(hd) = crate::mod_textures::find(&levels[0], sampler.clone()) {
        let (img, mxl, lod_scale) = hd.into_parts(mxl);
        return (images.add(img), mxl, lod_scale);
    }
    (images.add(crate::shrub_render::mip_image(levels, sampler).0), mxl, 1.0)
}

/// A plant class's parts, each its geometry repeated [`COPY_CAP`] times.
fn plant_parts(level: &LoadedLevel, ci: usize, meshes: &mut Assets<Mesh>) -> Vec<PlantPart> {
    crate::shrub_render::class_geometry(&level.shrubs.classes[ci])
        .into_iter()
        .map(|g| {
            let sampler = g.sampler();
            PlantPart { mesh: meshes.add(repeated(&g.pos, &g.uv, &g.info, &g.idx, COPY_CAP)), texture: g.texture, wrap: wrap_key(&sampler), sampler }
        })
        .collect()
}

/// The CLAMP wrap of a shrub sampler as a key (bit 0 S clamps, bit 1 T clamps).
fn wrap_key(s: &bevy::image::ImageSamplerDescriptor) -> u8 {
    use bevy::image::ImageAddressMode::ClampToEdge;
    (s.address_mode_u == ClampToEdge) as u8 | ((s.address_mode_v == ClampToEdge) as u8) << 1
}

/// A template mesh repeated `n` times, each copy's vertices tagged with its slot.
fn repeated(pos: &[[f32; 3]], uv: &[[f32; 2]], info: &[u32], idx: &[u32], n: usize) -> Mesh {
    let v = pos.len() as u32;
    let mut p = Vec::with_capacity(pos.len() * n);
    let mut t = Vec::with_capacity(pos.len() * n);
    let mut f = Vec::with_capacity(pos.len() * n);
    let mut s = Vec::with_capacity(pos.len() * n);
    let mut i = Vec::with_capacity(idx.len() * n);
    for k in 0..n as u32 {
        p.extend_from_slice(pos);
        t.extend_from_slice(uv);
        f.extend_from_slice(info);
        s.extend(std::iter::repeat_n(k, pos.len()));
        i.extend(idx.iter().map(|&x| x + k * v));
    }
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, p)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, t)
        .with_inserted_attribute(crate::shrub_render::ATTRIBUTE_SHRUB_INFO, VertexAttributeValues::Uint32(f))
        .with_inserted_attribute(ATTRIBUTE_FOLIAGE_SLOT, VertexAttributeValues::Uint32(s))
        .with_inserted_indices(Indices::U32(i))
}

type Geometry = (Vec<[f32; 3]>, Vec<[f32; 2]>, Vec<u32>);

/// One grass clump at unit height and width (Bevy axes, y up; the record scales y by its height, x / z by its
/// width): [`BLADES`] thin tapered blades over a 0.14-unit disc, each leaning outward on a curve. UV x = height
/// fraction (the colour ramp), y = the blade's shade (0..1).
fn clump_geometry() -> Geometry {
    let (mut pos, mut uv, mut idx) = (Vec::new(), Vec::new(), Vec::new());
    let mut rng = Rng::new(0x67_7261_7373);
    for b in 0..BLADES {
        let a = (b as f32 + rng.f() * 0.8) / BLADES as f32 * std::f32::consts::TAU;
        let r = 0.01 + 0.13 * rng.f().sqrt();
        let base = Vec2::new(a.cos(), a.sin()) * r;
        let lean = Vec2::new(a.cos(), a.sin()) * rng.range([0.02, 0.08]) + Vec2::new(rng.f() - 0.5, rng.f() - 0.5) * 0.03;
        let face = rng.f() * std::f32::consts::PI;
        let across = Vec2::new(face.cos(), face.sin());
        let h = rng.range([0.5, 1.0]);
        let w = rng.range([0.014, 0.024]);
        let shade = rng.f();
        let first = pos.len() as u32;
        for row in 0..ROWS {
            let t = row as f32 / ROWS as f32;
            let c = base + lean * t * t;
            let hw = w * (1.0 - 0.55 * t) * 0.5;
            for side in [-1.0, 1.0] {
                let q = c + across * hw * side;
                pos.push([q.x, t * h, q.y]);
                uv.push([t * h, shade]);
            }
        }
        let tip = base + lean;
        pos.push([tip.x, h, tip.y]);
        uv.push([h, shade]);
        for row in 0..ROWS as u32 - 1 {
            let o = first + row * 2;
            idx.extend_from_slice(&[o, o + 1, o + 2, o + 1, o + 3, o + 2]);
        }
        let o = first + (ROWS as u32 - 1) * 2;
        idx.extend_from_slice(&[o, o + 1, o + 2]);
    }
    (pos, uv, idx)
}

/// One flower cluster at unit height (x / z in world units: the record's width is 1): [`FLOWER_HEADS`] thin stems
/// within 0.12 of the root, each topped by a cupped [`PETALS`]-petal star (radius 0.035..0.05) tilted up to 40°. UV
/// x = height fraction; y = the part: under 1 a stem (its shade), 2 a petal tip, 3 the head's centre.
fn flower_geometry() -> Geometry {
    let (mut pos, mut uv, mut idx) = (Vec::new(), Vec::new(), Vec::new());
    let mut rng = Rng::new(0x666c_6f77_6572);
    for f in 0..FLOWER_HEADS {
        let a = (f as f32 + rng.f() * 0.7) / FLOWER_HEADS as f32 * std::f32::consts::TAU;
        let base = Vec2::new(a.cos(), a.sin()) * rng.range([0.0, 0.12]);
        let h = rng.range([0.75, 1.0]);
        let top = base + Vec2::new(rng.f() - 0.5, rng.f() - 0.5) * 0.04;
        let face = rng.f() * std::f32::consts::PI;
        let across = Vec2::new(face.cos(), face.sin()) * 0.004;
        let shade = rng.f();
        // The stem: one quad, base to head.
        let o = pos.len() as u32;
        for (c, y) in [(base, 0.0), (top, h)] {
            for s in [-1.0, 1.0] {
                pos.push([c.x + across.x * s, y, c.y + across.y * s]);
                uv.push([y, shade]);
            }
        }
        idx.extend_from_slice(&[o, o + 1, o + 2, o + 1, o + 3, o + 2]);
        // The head: a centre, an inner ring and a star rim, cupped, tilted about a random horizontal axis.
        let radius = rng.range([0.05, 0.07]);
        let (tilt, axis) = (rng.range([0.1, 0.7]), rng.f() * std::f32::consts::TAU);
        let rot = Quat::from_axis_angle(Vec3::new(axis.cos(), 0.0, axis.sin()), tilt);
        let spin = rng.f() * std::f32::consts::TAU;
        let head = |p: Vec3| -> [f32; 3] {
            let q = rot * p;
            // y here is world-scaled by the record's height (about 0.2): undo it so the head keeps its shape.
            [top.x + q.x, h + q.y * 5.0, top.y + q.z]
        };
        let c0 = pos.len() as u32;
        pos.push(head(Vec3::ZERO));
        uv.push([h, 3.0]);
        let rim = 2 * PETALS;
        for k in 0..rim {
            let t = spin + k as f32 / rim as f32 * std::f32::consts::TAU;
            let d = Vec3::new(t.cos(), 0.0, t.sin());
            let r = if k % 2 == 0 { radius } else { radius * 0.45 };
            pos.push(head(d * radius * 0.3 + Vec3::Y * 0.002));
            uv.push([h, 3.0]);
            pos.push(head(d * r + Vec3::Y * (r / radius) * 0.012));
            uv.push([h, 2.0]);
        }
        for k in 0..rim as u32 {
            let (i0, i1) = (c0 + 1 + k * 2, c0 + 1 + ((k + 1) % rim as u32) * 2);
            idx.extend_from_slice(&[c0, i0, i1, i0, i0 + 1, i1, i1, i0 + 1, i1 + 1]);
        }
    }
    (pos, uv, idx)
}

/// One fern at unit height and spread: [`FRONDS`] fronds around the root, each rising and arching out to its tip
/// (reach 0.7..1, the peak at unit height a little past halfway), [`FROND_ROWS`] rows alternately wide and narrow
/// (the leaflets), tapering at both ends. UV x = the distance along the frond (the colour ramp), y = its shade.
fn fern_geometry() -> Geometry {
    let (mut pos, mut uv, mut idx) = (Vec::new(), Vec::new(), Vec::new());
    let mut rng = Rng::new(0x6665_726e);
    for f in 0..FRONDS {
        let a = (f as f32 + rng.f() * 0.6) / FRONDS as f32 * std::f32::consts::TAU;
        let dir = Vec2::new(a.cos(), a.sin());
        let across = Vec2::new(-dir.y, dir.x);
        let (reach, rise, w) = (rng.range([0.7, 1.0]), rng.range([0.7, 1.0]), rng.range([0.1, 0.14]));
        let shade = rng.f();
        let first = pos.len() as u32;
        for row in 0..FROND_ROWS {
            let t = row as f32 / (FROND_ROWS - 1) as f32;
            // y = 2.2 t − 2 t², normalised to its peak (0.605 at t = 0.55): it ends a third of the way up.
            let y = rise * (2.2 * t - 2.0 * t * t) / 0.605;
            let c = dir * reach * t;
            let env = (4.0 * t * (1.0 - t)).max(0.0).powf(0.6);
            let hw = w * env * if row % 2 == 1 { 1.0 } else { 0.3 };
            for side in [-1.0, 1.0] {
                // The leaflets droop a little to the sides.
                let q = c + across * hw * side;
                pos.push([q.x, y - hw * 0.4, q.y]);
                uv.push([t, shade]);
            }
        }
        for row in 0..FROND_ROWS as u32 - 1 {
            let o = first + row * 2;
            idx.extend_from_slice(&[o, o + 1, o + 2, o + 1, o + 3, o + 2]);
        }
    }
    (pos, uv, idx)
}

/// Hides the chunks whose cell lies past the end distance (horizontally) from the camera; Bevy's frustum test does the
/// rest.
fn cull_cells(
    st: Res<FoliageState>,
    cams: Query<&Transform, (With<Camera3d>, Without<crate::sky_render::SkyCamera>)>,
    mut q: Query<(&FoliageCell, &mut Visibility)>,
) {
    let (Some(d), Some(cam)) = (st.density, cams.iter().next()) else { return };
    let eye = Vec2::new(cam.translation.x, cam.translation.z);
    for (c, mut vis) in &mut q {
        let off = ((eye - c.centre).abs() - Vec2::splat(c.half)).max(Vec2::ZERO);
        vis.set_if_neq(if off.length() <= d.end + 2.0 { Visibility::Inherited } else { Visibility::Hidden });
    }
}

/// foliage.wgsl `FoliageFrame`.
const FRAME_BYTES: usize = 80 + 32 * MOVERS;

/// One character the grass bends away from (foliage.wgsl `Mover`), Bevy world: its feet now, its trail's tail, the
/// footprint radius and its lowest point (y).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Mover {
    now: Vec3,
    tail: Vec3,
    radius: f32,
    bottom: f32,
}

/// The footprint of a bounding sphere of radius `r` ([`FOOTPRINT`]).
fn footprint(r: f32) -> f32 { (r * FOOTPRINT.0).clamp(FOOTPRINT.1[0], FOOTPRINT.1[1]) }

/// A tail `tail` following `now` over `dt` seconds with time constant `tau`, never more than `max` behind; a jump
/// past [`TAIL_RESET`] starts over at `now`.
fn follow(tail: Vec3, now: Vec3, dt: f32, tau: f32, max: f32) -> Vec3 {
    if tail.distance(now) > TAIL_RESET { return now; }
    let t = tail + (now - tail) * (1.0 - (-dt / tau).exp());
    let back = t - now;
    if back.length() > max { now + back.normalize() * max } else { t }
}

/// The movers this frame: Ratchet first (his feet, his moby's sphere), then the drawn mobys (`visible`, not deleted,
/// a sphere of at most [`MOVER_MAX_RADIUS`]) nearest the camera within the end distance, as crate::ao_render
/// `collect_mobys`. Their tails advance by the ticks since the last frame (frame-exact runs repeat them).
fn movers(st: &mut FoliageState, game: &rc_game::tick::Game, eye: Vec3, end: f32, dt: f32) -> Vec<Mover> {
    let mobys = &game.mobys.mobys;
    let hero = game.hero_moby;
    let sphere = |m: &rc_game::moby_runtime::Moby| (game_to_bevy([m.bsphere[0], m.bsphere[1], m.bsphere[2]].map(|v| v / 1024.0)), m.bsphere[3] / 1024.0);
    let mut near: Vec<(f32, usize, Vec3, f32, f32)> = mobys
        .iter()
        .enumerate()
        .filter(|&(i, m)| i != hero && m.visible != 0 && m.state < 0xfd)
        .filter_map(|(i, m)| {
            let (c, r) = sphere(m);
            if !(r > 0.0 && r <= MOVER_MAX_RADIUS) { return None; }
            let feet = game_to_bevy([m.position[0], m.position[1], m.position[2]]);
            let d = c.distance(eye) - r;
            (d < end).then_some((d, i, Vec3::new(c.x, feet.y, c.z), footprint(r), (c.y - r).min(feet.y)))
        })
        .collect();
    near.sort_by(|a, b| a.0.total_cmp(&b.0));
    let hp = game_to_bevy(game.hero.position());
    let hr = mobys.get(hero).map_or(0.0, |m| sphere(m).1);
    let list = std::iter::once((hero, hp, footprint(hr).max(HERO_FOOTPRINT), hp.y, 0))
        .chain(near.into_iter().take(MOVERS - 1).map(|(_, i, p, r, b)| (i, p, r, b, 1)));
    let mut out = Vec::with_capacity(MOVERS);
    let mut tails = HashMap::with_capacity(MOVERS);
    for (i, now, radius, bottom, k) in list {
        let tail = st.tails.get(&i).map_or(now, |&t| follow(t, now, dt, TAIL_TAU[k], TAIL_MAX[k]));
        tails.insert(i, tail);
        out.push(Mover { now, tail, radius, bottom });
    }
    st.tails = tails;
    out
}

/// The frame's tick (the 60 Hz fixed clock, as crate::shrub_render `update_sway`), distances, camera and the movers
/// (with the box around them, for the shader's early out) into the frame buffer.
fn upload_frame(
    mut st: ResMut<FoliageState>,
    fixed: Res<Time<Fixed>>,
    play: Option<Res<crate::gameplay::Play>>,
    cams: Query<&Transform, (With<Camera3d>, Without<crate::sky_render::SkyCamera>)>,
    mut buffers: ResMut<Assets<ShaderBuffer>>,
) {
    let (Some(h), Some(d)) = (st.frame.clone(), st.density) else { return };
    let tick = (fixed.elapsed().as_nanos() / fixed.timestep().as_nanos().max(1)) as u32;
    let dt = st.last_tick.map_or(0, |t| tick.wrapping_sub(t)).min(30) as f32 / 60.0;
    st.last_tick = Some(tick);
    let eye = cams.iter().next().map_or(Vec3::ZERO, |c| c.translation);
    let movers = play.as_ref().map_or_else(Vec::new, |p| movers(&mut st, &p.game, eye, d.end, dt));
    let (mut lo, mut hi) = (Vec3::splat(1e9), Vec3::splat(-1e9));
    for m in &movers {
        let r = Vec3::new(m.radius, 0.0, m.radius);
        lo = lo.min(m.now.min(m.tail) - r);
        hi = hi.max(m.now.max(m.tail) + r);
        // foliage.wgsl `push`'s height window.
        (lo.y, hi.y) = ((m.bottom - 0.8).min(lo.y), (m.bottom + 0.6).max(hi.y));
    }
    let sway = !std::env::var("RC_SWAY").is_ok_and(|v| v.trim() == "0");
    let mut b = Vec::with_capacity(FRAME_BYTES);
    for v in [tick, sway as u32, movers.len() as u32, 0] { b.extend_from_slice(&v.to_le_bytes()); }
    for v in [d.full, d.end, plan::SHRINK_BAND, 0.0, eye.x, eye.y, eye.z, 0.0] { b.extend_from_slice(&v.to_le_bytes()); }
    for v in [lo.x, lo.y, lo.z, 0.0, hi.x, hi.y, hi.z, 0.0] { b.extend_from_slice(&v.to_le_bytes()); }
    for m in &movers {
        for v in [m.now.x, m.now.y, m.now.z, m.radius, m.tail.x, m.tail.y, m.tail.z, m.bottom] { b.extend_from_slice(&v.to_le_bytes()); }
    }
    b.resize(FRAME_BYTES, 0);
    crate::asset_write::set_buffer(&mut buffers, &h, &b);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clump_template() {
        let (pos, uv, idx) = clump_geometry();
        assert_eq!(pos.len(), BLADES * (ROWS * 2 + 1));
        assert_eq!(idx.len(), BLADES * ((ROWS - 1) * 6 + 3));
        assert!(idx.iter().all(|&i| (i as usize) < pos.len()));
        // Unit height, rooted at y = 0, within the clump's radius plus the lean.
        assert!(pos.iter().all(|p| (0.0..=1.0).contains(&p[1]) && p[0].hypot(p[2]) < 0.25));
        for (pos, uv, idx) in [flower_geometry(), fern_geometry()] {
            assert!(idx.len() % 3 == 0 && idx.iter().all(|&i| (i as usize) < pos.len()) && uv.len() == pos.len());
            assert!(pos.iter().all(|p| (-0.2..=1.4).contains(&p[1]) && p[0].hypot(p[2]) <= 1.05));
        }
        let (_, fuv, _) = flower_geometry();
        assert!(fuv.iter().any(|t| t[1] == 2.0) && fuv.iter().any(|t| t[1] == 3.0) && fuv.iter().any(|t| t[1] < 1.0));
        assert_eq!(Template::Ferns.mesh().count_vertices(), FRONDS * FROND_ROWS * 2 * FERN_CAP);
        assert!(uv.iter().zip(&pos).all(|(t, p)| (t[0] - p[1]).abs() < 1e-6));
        let m = repeated(&pos, &uv, &vec![0; pos.len()], &idx, 3);
        assert_eq!(m.count_vertices(), pos.len() * 3);
        let Some(VertexAttributeValues::Uint32(slots)) = m.attribute(ATTRIBUTE_FOLIAGE_SLOT) else { panic!("no slots") };
        assert_eq!((slots[0], slots[pos.len()], slots[pos.len() * 3 - 1]), (0, 1, 2));
    }

    #[test]
    fn record_layout() {
        let mut b = Vec::new();
        record(&mut b, Vec3::new(1.0, 2.0, 3.0), 0.5, [1.0, 0.0, 0.4, 1.0], [7, 8, 9, 1], Mat3::IDENTITY);
        assert_eq!(b.len(), RECORD);
        assert_eq!(f32::from_le_bytes(b[12..16].try_into().unwrap()), 0.5);
        assert_eq!(u32::from_le_bytes(b[44..48].try_into().unwrap()), 1);
        assert_eq!(f32::from_le_bytes(b[48..52].try_into().unwrap()), 1.0);
        assert_eq!(cell_of([7.9, -0.1, 0.0]), (0, -1));
        assert_eq!(axes() * Vec3::new(1.0, 2.0, 3.0), game_to_bevy([1.0, 2.0, 3.0]));
    }

    #[test]
    fn mover_tails() {
        // The shader's array matches MOVERS.
        assert!(include_str!("../assets/shaders/foliage.wgsl").contains(&format!("array<Mover, {MOVERS}>")));
        let (a, b) = (Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0));
        // Closes in on the feet, never overshoots, stays within its length, restarts on a jump.
        let t = follow(a, b, 1.0 / 60.0, 0.2, 2.0);
        assert!(t.x > 0.0 && t.x < 0.2);
        assert!(follow(a, b, 10.0, 0.2, 2.0).distance(b) < 1e-3);
        assert!((follow(a, Vec3::new(3.0, 0.0, 0.0), 1.0 / 60.0, 0.2, 0.8).x - 2.2).abs() < 1e-4);
        assert_eq!(follow(a, Vec3::new(9.0, 0.0, 0.0), 1.0 / 60.0, 0.2, 0.8), Vec3::new(9.0, 0.0, 0.0));
        assert_eq!(follow(b, b, 0.0, 0.2, 0.8), b);
        assert_eq!((footprint(0.1), footprint(1.0), footprint(10.0)), (FOOTPRINT.1[0], 0.6, FOOTPRINT.1[1]));
    }
}
