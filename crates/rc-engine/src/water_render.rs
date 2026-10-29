//! Level-code water: strip meshes (classes 676, 678, 761, 1225) and the ripple patches of the level's ripple module
//! (751 on Novalis, the patch managers of levels 05 / 07 / 11 / 12 / 13), drawn in the game's frame position. Spec:
//! docs/plan/world_animation.md §1–§3; tables: `rc_formats::water`; state and the exact float arithmetic:
//! `rc_game::water` (the ripple module lives in the moby system, `Services::water`, `rc_game::water::world`).
//! `RC_WATER=0` disables the drawing; `RC_WATER_STATS=1` prints the ripple module's state once per second of ticks.
//!
//! **Frame position.** The game's water is drawn by per-frame draw callbacks that the moby updates register
//! on list 0x21afe0 (moby instance order: 676, 678, 751, 761, 1225 on Novalis); the list is drained after the
//! mobys and before the particles. Here every water draw is a Transparent3d item at the world origin with
//! `depth_bias = 5e5 + slot`, `slot` = the callback order (per class, per strip / patch, layer 1 then layer 2):
//! after every moby item (Opaque3d / AlphaMask3d run first, moby Transparent3d items have bias 0) and before
//! the particles (bias 1e6), and among themselves exactly in callback order (all share the origin distance).
//! The ripple patches are drawn on a tick whose moby loop registered `Callback::RipplePatches` (the manager's
//! callback: `FUN_002b91c8(table, n)`), at the manager class's place among the strip classes.
//!
//! **GS state → Bevy.** Both passes: `ALPHA = FIX << 32 | 0x64` = `Cd + (Cs − Cd)·FIX/128`, TEST 0x5360b with
//! vertex A = 0 (As = 0 fails, AFAIL RGB_ONLY: colour, never Z), ZTST GEQUAL, CLAMP 0 (repeat), TEX1 bilinear
//! without mips, MODULATE, FGE. The blend is `GsPass::BlendNoZ` (SrcAlpha / OneMinusSrcAlpha, no depth write,
//! GreaterEqual) with FIX/128 **baked into the fragment alpha** (not `BlendFactor::Constant`: Bevy's material
//! draw has no per-draw blend constant). The GPU mixes in linear light, the GS on display bytes (as for every
//! blended pass, gs_state.rs).
//!
//! **Strips.** One static mesh per strip (the GS tri-strip as a `TriangleStrip`), two materials (layer 1 / 2).
//! Per rendered frame (`FUN_002b96e0` runs in the draw callback, so once per frame on which a tick ran):
//! `ScrollState::tick`, then the layer offset, the eight wobble offsets `A·(sin θ, cos θ)` for this phase
//! (VU0 sine on the CPU, `rc_game::water::wobble_table`; each vertex carries its `hash & 7` from the raw bits
//! of x and y, computed at load) and the class's z blend go to the material uniforms. The z blend: 761 writes
//! `bob(t)` in its update (per tick, before the draw); 1225 writes it in its callback after drawing (one frame
//! late); 676/678 keep the stored 0 (z = z1). The FastBSphereCheck(400) cull is not reproduced (the GPU clips).
//!
//! **Ripples.** The managers run in the moby loop (`rc_game::water::managers`: zone activation or the per-patch
//! view test, random drops, the sim clock) on the game's one `rand` stream; with `RC_PLAY=0` (no moby loop) this
//! plugin's `FixedUpdate` runs Novalis's 751 on a stream of its own (`srand(1234)`) with the fly camera. Per
//! rendered frame, per patch with a mask: the four-corner frustum test (`FUN_002b91c8`; approximated with the
//! port's projection, the game tests clip codes against 0x167200), then `advance_uv` (only for drawn patches, as
//! in the game), the 17×17 vertex grid with grey and sphere-map UV from the drawn height buffer, and one mesh of the
//! active 4×4 sub-blocks (each the game's 46-vertex strip, degenerate joins dropped). Water pass (FX +0x18, FIX
//! +0x1d, the shared animated UV) then env pass (FX +0x14, FIX +0x1c, the sphere-map UV), per patch. The game
//! emits water/env per sub-block; sub-blocks do not overlap, so per patch is the same image. A patch's FX / FIX
//! are read every drawn frame (levels 11 / 12 set them when the patch's moby builds it, 07 / 13 copy the module's
//! FIX every tick).
//!
//! **Buffers.** The materials never change after setup (except the fog uniform, on a fog change, and a patch's
//! FX / FIX), so Bevy does not re-prepare them: the per-frame values live in two storage buffers every water
//! material binds, rewritten on each drawn frame. `frame` holds one [`FRAME_RECORD`] per callback slot (z blend w,
//! scroll, the eight wobble offsets); `ripple` one record per patch (the sub-block mask, the 17×17 positions,
//! sphere-map UVs and grey, then the 46 water UVs), which the patches' one static mesh (all 16 sub-blocks, each
//! vertex tagged with its sub-block, strip index and grid vertex) reads; sub-blocks outside the mask are dropped by
//! the vertex shader. The triangles, their order and their vertex values are those of a mesh of the active
//! sub-blocks.
//!
//! **Fire fields** (class 760, the flames and smoke on the bombed buildings; list 2 = after the particles: `LIST2_BIAS`). The moby system keeps the fields' state
//! (`rc_game::moby_update::classes::fire_field`: element life cycle, scroll, respawns on the game's stream) and this
//! tick's registrations (`Services::draw_callbacks`); [`draw_fire_fields`] rebuilds, once per tick, three meshes per
//! registered field from them, in `FastDrawQuadReal`'s form: the flames (FX P+0x54 + 40, ALPHA 0x48 additive),
//! the smoke elements (same FX, 0x44) and the six-quad smoke curtain (FX P+0x58 + 40, 0x44, when P+0x42 = 0), all
//! camera-facing about the moby (`fire_field_bases`), placed in the cuboid, with the per-corner alpha tables, fog on
//! (PRIM 0x7c), TEST 0x51001 (colour only, Z tested), CLAMP repeat, bilinear. The quad tables are read from the
//! overlay per level ([`FireFieldTables`]). Both equations blend on the frame's display bytes like every effect
//! (crate::display_blend; the shared draw-callback material crate::fx_draw::FxPrimMaterial, `fx_prim.wgsl`), As > 0x80 included (the curtain's A = 0xff).
//!
//! Elsewhere: 1848's env overlay is drawn by crate::sea_render (the shared liquid mesh path), the drips 787 are mobys
//! (`rc_game::moby_update::classes::units::drip`) drawn by the moby renderer.

use crate::game_camera::{game_eye, GameFog, GameProjection, TfragFog};
use crate::fx_draw::{FxPrimMaterial, FxPrimParams, PrimBuf};
use crate::graphics::GraphicsSettings;
use crate::gs_state::GsPass;
use anyhow::{Context, Result};
use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::camera::CameraProjection;
use bevy::mesh::{Indices, MeshVertexAttribute, MeshVertexBufferLayoutRef, PrimitiveTopology, VertexAttributeValues, VertexFormat};
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError};
use bevy::render::storage::ShaderBuffer;
use bevy::shader::ShaderRef;
use rc_formats::level_overlay::{LevelOverlay, Relocation};
use rc_formats::water::{self as wf, StripAnim, StripClass, StripDescriptor, SUB_STRIP_LEN};
use rc_game::moby_update::classes::draw_callbacks::Callback;
use rc_game::ps2v::Pf;
use rc_game::rng::{Rng, LEVEL_SEED};
use rc_game::water::world::LevelWaterData;
use rc_game::water::{self as ww, RippleSim, ScrollState, VG};
use std::path::Path;
use std::sync::Arc;

const SHADER_PATH: &str = "shaders/water.wgsl";
/// Transparent3d sort bias of the water draws (see the module doc).
const WATER_BIAS: f32 = 5.0e5;
/// The ripple patches' place in [`LevelWater::order`] (a manager class stands for it).
const RIPPLE_SLOT: u16 = u16::MAX;
/// The chute sheets' FX texture when the level's own water has no clear one ([`clear_water`]): Rilgar's ripple
/// water, then its env pass. Without either the level has no water to carry (Quartu: its one chute, from the
/// factory door at (311, 281, 49) down to the cave floor, is a dry metal slide) and its chutes get no sheet.
const CHUTE_FX: [i32; 2] = [42, 41];
/// **Port-only (Water Enhanced): drawn surface lifts.** (level, game box min, max, lift): a ripple patch whose
/// load-time centre lies in the box draws its water this much higher on Enhanced (`params.misc.w`, added in the
/// vertex shader to the height the game moves; the game's water height, the ripple module and Original are
/// untouched). Rilgar's sewer flood under the slide (831's 32 patches at z 15.2, all of them so no seam shows between
/// patches): the rims of the round grates sit just under its surface, where the shore foam outlined them.
const SURFACE_LIFT: [(u32, [f32; 3], [f32; 3], f32); 1] = [(5, [150.0, 240.0, 10.0], [290.0, 355.0, 25.0], 0.25)];

/// The [`SURFACE_LIFT`] of a patch centred at `c` on `level`.
fn surface_lift(level: u32, c: [f32; 3]) -> f32 {
    SURFACE_LIFT
        .iter()
        .find(|&&(lv, lo, hi, _)| lv == level && (0..3).all(|k| (lo[k]..=hi[k]).contains(&c[k])))
        .map_or(0.0, |&(.., l)| l)
}

pub const ATTRIBUTE_WATER_Z1: MeshVertexAttribute = MeshVertexAttribute::new("WaterZ1", 0x5741_5431, VertexFormat::Float32);
pub const ATTRIBUTE_WATER_UV: MeshVertexAttribute = MeshVertexAttribute::new("WaterUv", 0x5741_5432, VertexFormat::Float32x4);
pub const ATTRIBUTE_WATER_TAG: MeshVertexAttribute = MeshVertexAttribute::new("WaterTag", 0x5741_5433, VertexFormat::Uint32x2);

/// `RC_WATER=0` turns the water drawing off.
pub fn enabled() -> bool { !std::env::var("RC_WATER").is_ok_and(|v| v.trim() == "0") }

/// Level data for the water.
#[derive(Default)]
pub struct LevelWater {
    pub strips: Vec<StripClass>,
    /// The level's water data (`rc_game::water::world`; the moby system's `Services::water` is built from it, also
    /// with `RC_WATER=0`). None when the overlay could not be read.
    pub data: Option<LevelWaterData>,
    /// Water draw callbacks in moby instance order = draw-callback order: strip classes, and [`RIPPLE_SLOT`] for the
    /// ripple patches (the first instance of a manager class).
    pub order: Vec<u16>,
    /// The fire / smoke fields' quad tables (class 760, levels 00 / 01 / 14; see "Fire fields" below).
    pub fire_fields: Option<FireFieldTables>,
    /// The other draw callbacks' tables (crate::fx_draw: the nanotech glow).
    pub fx: crate::fx_draw::LevelFx,
    /// The flow chutes' splines (Water Enhanced's chute sheets: crate::chute_water).
    pub chutes: Vec<crate::chute_water::ChuteSpline>,
    /// The level index (for [`SURFACE_LIFT`]).
    pub level: u32,
}

/// Reads the level's water data (by code identity against the reference overlays: rc_game::water::world), the strip
/// tables and the draw order.
pub fn load(root: &Path, index: u32, gameplay: &[u8]) -> Result<LevelWater> {
    let ov_bytes = crate::disc_source::level_file(root, index, "overlay.bin")?;
    let target = LevelOverlay::parse(&ov_bytes).context("parsing the level overlay")?;
    let cache: std::cell::RefCell<std::collections::HashMap<u32, Option<Arc<LevelOverlay>>>> = Default::default();
    let reference = |l: u32| -> Option<Arc<LevelOverlay>> {
        cache
            .borrow_mut()
            .entry(l)
            .or_insert_with(|| crate::disc_source::level_file(root, l, "overlay.bin").ok().and_then(|b| LevelOverlay::parse(&b).ok()).map(Arc::new))
            .clone()
    };
    let data = match LevelWaterData::load(&ov_bytes, &target, &reference, gameplay) {
        Ok(d) => Some(d),
        Err(e) => {
            eprintln!("water: level water data not read ({e}): no water managers");
            None
        }
    };
    if !enabled() { return Ok(LevelWater { data, level: index, ..LevelWater::default() }); }
    let ov = wf::Overlay::parse(&ov_bytes).context("parsing the level overlay")?;
    let strips = match reference(1) {
        Some(r01) => wf::parse_strip_classes(&ov, &wf::strip_tables(&target, &Relocation::new(&r01, &target)))?,
        None => Vec::new(),
    };
    let fire_fields = FireFieldTables::parse(&ov, index)?;
    let fx = crate::fx_draw::LevelFx::parse(&ov, index);
    let managers = data.as_ref().map(|d| d.manager_classes.clone()).unwrap_or_default();
    let has_patches = data.as_ref().is_some_and(|d| d.patch_records().is_some());
    let instances = rc_formats::gameplay::parse_moby_instances(gameplay)?;
    let mut order = Vec::new();
    for m in &instances {
        let c = m.o_class as u16;
        let slot = if strips.iter().any(|s| s.class == c) { c } else if has_patches && managers.contains(&m.o_class) { RIPPLE_SLOT } else { continue };
        if !order.contains(&slot) { order.push(slot); }
    }
    let chutes = crate::chute_water::chutes(gameplay, index);
    Ok(LevelWater { strips, data, order, fire_fields, fx, chutes, level: index })
}

/// Static uniform of one water draw.
#[derive(Clone, Copy, Debug, Default, ShaderType)]
pub struct WaterParams {
    /// x = FIX / 128, y = kind (0/1 strip layer, 2 ripple water, 3 ripple env, 4 chute), z = an even absorption
    /// (Enhanced; crate::sea_water::clarity; 0 here) or a chute's flow speed, w = the Enhanced surface lift
    /// ([`SURFACE_LIFT`]; ripple water) or 1: Pokitaru's ocean (kind 0, crate::ocean_water).
    pub misc: Vec4,
    /// x = callback slot (record in `frame`), y = first word of the patch's `ripple` record, z = offset of its
    /// water UVs in the record (words), w = the ocean's sky colour (crate::ocean_water::SKY).
    pub ids: UVec4,
}

/// Bytes per callback slot in the `frame` buffer: (w, scroll x, scroll y, 0), then the eight wobble offsets
/// two per vec4 (index = hash & 7).
const FRAME_RECORD: usize = 80;
/// Words of a patch's `ripple` record: mask + 3 pad, per grid vertex (x, y, z, env u, env v, RGBA), then the
/// 46 water UVs.
const RIPPLE_HEAD: usize = 4;
const RIPPLE_VERTEX: usize = 6;
const RIPPLE_UV_AT: usize = RIPPLE_HEAD + VG * VG * RIPPLE_VERTEX;
const RIPPLE_WORDS: usize = RIPPLE_UV_AT + SUB_STRIP_LEN * 2;

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct WaterMaterial {
    #[texture(0)]
    #[sampler(1)]
    pub texture: Handle<Image>,
    #[uniform(2)]
    pub fog: TfragFog,
    #[uniform(3)]
    pub params: WaterParams,
    /// Per-frame values of every slot (see [`FRAME_RECORD`]).
    #[storage(4, read_only, visibility(vertex))]
    pub frame: Handle<ShaderBuffer>,
    /// Per-frame ripple patch data (see [`RIPPLE_WORDS`]).
    #[storage(5, read_only, visibility(vertex))]
    pub ripple: Handle<ShaderBuffer>,
    /// Water Enhanced: the opaque scene behind the water (crate::scene_copy).
    #[texture(6, sample_type = "float", filterable = false)]
    pub scene_depth: Handle<Image>,
    #[texture(7)]
    #[sampler(8)]
    pub scene_color: Handle<Image>,
    #[uniform(9)]
    pub look: WaterLook,
    /// Water Enhanced: the rings where Ratchet meets the water (see [`WaterRings`]).
    #[storage(10, read_only, visibility(fragment))]
    pub rings: Handle<ShaderBuffer>,
    /// Water Enhanced: the planar reflection's state and picture (crate::water_mirror).
    #[storage(11, read_only, visibility(fragment))]
    pub mirror: Handle<ShaderBuffer>,
    #[texture(12)]
    #[sampler(13)]
    pub mirror_color: Handle<Image>,
    /// Callback-order slot (Transparent3d position).
    pub slot: u32,
    /// The FX texture index (−1: none yet), for [`WaterLook`]'s clear-water test.
    pub fx: i32,
}

/// Water Enhanced (crate::graphics) of one draw.
#[derive(Clone, Copy, Debug, Default, PartialEq, ShaderType)]
pub struct WaterLook {
    /// x = 1: enhanced (the option is on and the texture is clear water: [`clear_water`]).
    pub misc: Vec4,
    /// xyz = toward the key light (world; crate::sun_shadow::KeyLight), for the glint; w = the swell's scale
    /// (crate::water_waves).
    pub sun: Vec4,
}

/// **Port-only (Water Enhanced): the rings where Ratchet meets the water.** crate::gameplay queues the contact points
/// ([`crate::gameplay::Play::water_contacts`]: the swim's splashes away from the ripple patches, which draw their own,
/// and his wake while wading or swimming at the surface); each becomes a wave packet the water shader adds to the
/// surface's slope for [`RING_LIFE`] seconds. Drawing only: nothing here reaches the game state.
#[derive(Default)]
struct WaterRings {
    /// (game x, game y, start time in `Time::elapsed_secs_wrapped` = the shader's `globals.time`, strength).
    live: Vec<[f32; 4]>,
}

/// The shader's ring count (`MAX_RINGS`) and a ring's lifetime (its `ring_grad` cut-off).
const MAX_RINGS: usize = 16;
const RING_LIFE: f32 = 3.0;
/// Bytes of the `rings` buffer: the count, then [`MAX_RINGS`] vec4s.
const RINGS_BYTES: usize = 16 * (1 + MAX_RINGS);

impl WaterRings {
    /// Adds this tick's contacts at `now`, drops the finished and the oldest beyond [`MAX_RINGS`], and writes the buffer.
    fn update(&mut self, contacts: impl IntoIterator<Item = [f32; 3]>, now: f32, bytes: &mut [u8]) {
        self.live.retain(|r| (0.0..RING_LIFE).contains(&(now - r[2])));
        self.live.extend(contacts.into_iter().map(|[x, y, s]| [x, y, now, s]));
        let over = self.live.len().saturating_sub(MAX_RINGS);
        self.live.drain(..over);
        bytes.fill(0);
        let mut w = bytes.as_chunks_mut::<4>().0.iter_mut();
        let mut put = |x: f32| if let Some(d) = w.next() { *d = x.to_le_bytes() };
        for x in [self.live.len() as f32, 0.0, 0.0, 0.0] { put(x); }
        // Game (x, y) -> Bevy (x, −z).
        for r in &self.live { for x in [r[0], -r[1], r[2], r[3]] { put(x); } }
    }
}

/// Whether an FX texture is clear water, by its average colour: blue over red and not far under green (lava, mud
/// and sludge are not).
fn clear_water(t: &rc_formats::texture::Texture) -> bool { clear_rgba(&t.rgba) }

/// [`clear_water`] of RGBA texels (crate::sea_render's liquid images too).
pub fn clear_rgba(rgba: &[u8]) -> bool {
    let n = (rgba.len() / 4).max(1) as f32;
    let sum = rgba.as_chunks::<4>().0.iter().fold([0.0f32; 3], |a, p| [a[0] + p[0] as f32, a[1] + p[1] as f32, a[2] + p[2] as f32]);
    let [r, g, b] = sum.map(|s| s / n);
    b >= 1.15 * r && b >= 0.8 * g
}

/// The Water option and the key light into every water material (only those whose look changes are touched).
fn follow_look(
    g: Res<GraphicsSettings>,
    key: Option<Res<crate::sun_shadow::KeyLight>>,
    level: Res<crate::Level>,
    state: Option<ResMut<WaterState>>,
    mut materials: ResMut<Assets<WaterMaterial>>,
) {
    let Some(mut st) = state else { return };
    let st = &mut *st;
    let tex = level.0.particles.textures.as_ref();
    // w: the swell's scale (crate::water_waves; the slope only on this water).
    let sun = key.map_or(Vec3::Y, |k| k.to_light).extend(crate::water_waves::scale());
    for h in &st.materials {
        let Some(m) = materials.get(h) else { continue };
        let clear = *st.clear_fx.entry(m.fx).or_insert_with(|| {
            let clear = usize::try_from(m.fx).ok().and_then(|i| tex?.fx_textures.get(i)?.as_ref()).is_some_and(clear_water);
            if m.fx >= 0 { println!("water: fx texture {} is {}", m.fx, if clear { "clear water (Enhanced applies)" } else { "not clear water (kept Original)" }); }
            clear
        });
        let other = if clear { 0.0 } else {
            *st.liquid_fx.entry(m.fx).or_insert_with(|| crate::liquid_render::patch_kind(m.fx, usize::try_from(m.fx).ok().and_then(|i| tex?.fx_textures.get(i)?.as_ref())))
        };
        let misc = if other > 0.0 { Vec4::new(2.0, other, 0.0, 0.0) } else { Vec4::new(clear as u32 as f32, 0.0, 0.0, 0.0) };
        let want = WaterLook { misc: if g.water.on() { misc } else { Vec4::ZERO }, sun };
        let moved = m.look.misc != want.misc || m.look.sun.distance(want.sun) > 0.01;
        if moved { if let Some(mut m) = materials.get_mut(h) { m.look = want; } }
    }
}

impl Material for WaterMaterial {
    fn vertex_shader() -> ShaderRef { SHADER_PATH.into() }
    fn fragment_shader() -> ShaderRef { SHADER_PATH.into() }
    fn alpha_mode(&self) -> AlphaMode { AlphaMode::Blend }
    fn depth_bias(&self) -> f32 { WATER_BIAS + self.slot as f32 }
    fn enable_prepass() -> bool { false }
    fn enable_shadows() -> bool { false }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = None;
        GsPass::BlendNoZ.specialize(descriptor);
        descriptor.vertex.buffers = vec![layout.0.get_layout(&[
            Mesh::ATTRIBUTE_POSITION.at_shader_location(0),
            ATTRIBUTE_WATER_Z1.at_shader_location(1),
            ATTRIBUTE_WATER_UV.at_shader_location(2),
            ATTRIBUTE_WATER_TAG.at_shader_location(3),
        ])?];
        Ok(())
    }
}

struct StripDraw {
    desc: StripDescriptor,
    scroll: ScrollState,
    /// Callback slots of layer 1 / 2.
    slots: [u32; 2],
}

struct ClassDraw {
    class: u16,
    anim: StripAnim,
    /// The class's z blend (+0x5c of every strip; they are always written together).
    w: Pf,
    strips: Vec<StripDraw>,
}

/// One patch's two draws (water, env) and the FX / FIX their materials hold.
struct PatchDraw {
    entities: [Entity; 2],
    mats: [Handle<WaterMaterial>; 2],
    /// FX (water, env) and FIX (water, env) the materials were made with.
    fx: [i32; 2],
    fix: [u8; 2],
    slots: [u32; 2],
    visible: bool,
    /// The water draw's [`SURFACE_LIFT`].
    lift: f32,
}

/// The live water drawing state.
#[derive(Resource)]
pub struct WaterState {
    classes: Vec<ClassDraw>,
    /// Position of the ripple patches in the callback order (between the strip classes).
    ripple_at: usize,
    /// None: a patch without its FX textures (not drawn).
    patches: Vec<Option<PatchDraw>>,
    /// The `RC_PLAY=0` ripple module (Novalis's 751 on a stream of its own, `srand(1234)`); None with the game tick
    /// (the module is the moby system's, crate::gameplay::Play).
    pub fallback: Option<RippleSim>,
    fallback_inputs: Option<Arc<LevelWaterData>>,
    fallback_rng: Rng,
    /// The frame counter 0x15f5cc as the updates see it.
    counter: u32,
    ticks: u64,
    drawn_ticks: u64,
    stats: bool,
    /// FX images by FX index, made on first use.
    fx_images: Vec<Option<Handle<Image>>>,
    /// Every water material (for a fog change) and the fog they hold.
    materials: Vec<Handle<WaterMaterial>>,
    fog: TfragFog,
    /// The `frame` / `ripple` buffers and their CPU copies.
    frame: Handle<ShaderBuffer>,
    frame_bytes: Vec<u8>,
    ripple_buf: Handle<ShaderBuffer>,
    ripple_bytes: Vec<u8>,
    /// The rings ([`WaterRings`]), their buffer and its CPU copy.
    rings: WaterRings,
    rings_buf: Handle<ShaderBuffer>,
    rings_bytes: Vec<u8>,
    /// [`clear_water`] by FX index, found on first use.
    clear_fx: std::collections::HashMap<i32, bool>,
    /// crate::liquid_render::patch_kind by FX index (not clear water), found on first use.
    liquid_fx: std::collections::HashMap<i32, f32>,
}

impl WaterState {
    /// The [`WaterRings`] buffer (crate::sea_water's seas share the rings).
    pub fn rings_buffer(&self) -> Handle<ShaderBuffer> { self.rings_buf.clone() }

    /// The flat regions of this frame's Enhanced water (crate::water_mirror): the clear strips at their z blend and
    /// the drawn clear ripple patches (`sim`: the moby system's module, else the `RC_PLAY=0` one).
    pub fn flat_surfaces(&self, sim: Option<&RippleSim>, out: &mut Vec<crate::water_mirror::FlatWater>) {
        let clear = |fx: i32| self.clear_fx.get(&fx).copied().unwrap_or(false);
        for c in &self.classes {
            let w = c.w.to_f32();
            for s in c.strips.iter().filter(|s| clear(s.desc.fx[0])) {
                out.extend(crate::water_mirror::flat_region(s.desc.vertices.iter().map(|v| [v[0], v[1], v[2] * w + v[3] * (1.0 - w)])));
            }
        }
        let Some(sim) = sim.or(self.fallback.as_ref()) else { return };
        for (pd, pa) in self.patches.iter().zip(&sim.patches) {
            if !pd.as_ref().is_some_and(|pd| pd.visible && clear(pd.fx[0])) { continue; }
            let [x, y, z] = pa.centre.map(Pf::to_f32);
            out.push(crate::water_mirror::FlatWater { z, min: Vec2::new(x - 8.0, y - 8.0), max: Vec2::new(x + 8.0, y + 8.0) });
        }
    }
}

pub struct WaterPlugin;

impl Plugin for WaterPlugin {
    fn build(&self, app: &mut App) {
        if !enabled() { return; }
        if !crate::determinism::deterministic() && !app.world().contains_resource::<Time<Fixed>>() {
            app.insert_resource(Time::<Fixed>::from_hz(crate::determinism::TICK_HZ));
        }
        crate::liquid_render::keep_look_module(app);
        app.add_plugins(MaterialPlugin::<WaterMaterial>::default())
            .add_systems(crate::level_switch::LevelStartup, (setup, fire_field_setup))
            .add_systems(crate::level_switch::LevelUnload, (crate::level_switch::remove::<WaterState>, crate::level_switch::remove::<FireFieldDraw>))
            .add_systems(FixedUpdate, tick.after(crate::gameplay::GameTick))
            .add_systems(PostUpdate, ((draw, follow_look).chain(), draw_fire_fields).before(bevy::asset::AssetEventSystems));
    }
}

type MainCamera<'w, 's> = Query<'w, 's, &'static Transform, (With<Camera3d>, Without<crate::sky_render::SkyCamera>)>;

fn strip_mesh(d: &StripDescriptor) -> Mesh {
    let pos: Vec<[f32; 3]> = d.vertices.iter().map(|v| [v[0], v[1], v[2]]).collect();
    let z1: Vec<f32> = d.vertices.iter().map(|v| v[3]).collect();
    let uv: Vec<[f32; 4]> = d.uvs.iter().map(|u| [u[0], u[1], u[0], u[1]]).collect();
    let tag: Vec<[u32; 2]> = d.vertices.iter().map(|v| [d.rgba, ww::wobble_hash(v[0], v[1]) & 7]).collect();
    Mesh::new(PrimitiveTopology::TriangleStrip, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
        .with_inserted_attribute(ATTRIBUTE_WATER_Z1, z1)
        .with_inserted_attribute(ATTRIBUTE_WATER_UV, uv)
        .with_inserted_attribute(ATTRIBUTE_WATER_TAG, VertexAttributeValues::Uint32x2(tag))
}

fn params(fix: u8, kind: u32, slot: u32, patch: usize) -> WaterParams { lifted_params(fix, kind, slot, patch, 0.0) }

/// [`params`] with a ripple water draw's [`SURFACE_LIFT`] (`misc.w`).
fn lifted_params(fix: u8, kind: u32, slot: u32, patch: usize, lift: f32) -> WaterParams {
    WaterParams {
        misc: Vec4::new(fix as f32 / 128.0, kind as f32, 0.0, lift),
        ids: UVec4::new(slot, (patch * RIPPLE_WORDS) as u32, RIPPLE_UV_AT as u32, 0),
    }
}

/// Writes one slot's `frame` record.
fn write_frame(bytes: &mut [u8], slot: u32, w: f32, scroll: [f32; 2], wobble: &[[f32; 2]; 8]) {
    let at = slot as usize * FRAME_RECORD;
    let Some(rec) = bytes.get_mut(at..at + FRAME_RECORD) else { return };
    let head = [w, scroll[0], scroll[1], 0.0];
    for (dst, v) in rec.as_chunks_mut::<4>().0.iter_mut().zip(head.into_iter().chain(wobble.iter().flatten().copied())) { *dst = v.to_le_bytes(); }
}

/// The static mesh every ripple patch draws: the 16 sub-blocks' GS strips as triangles without the degenerate
/// joins (`strip_order`: grid offsets of the 46 strip vertices). Vertex tag = (sub-block | strip index << 8,
/// grid vertex); the other attributes are unused (the vertex shader reads the patch's `ripple` record).
fn patch_mesh(order: &[u16]) -> Mesh {
    let (mut tag, mut idx) = (Vec::new(), Vec::new());
    for i in 0..16u32 {
        let base = ((i & 3) * 4 + (i >> 2) * 4 * VG as u32) as usize;
        let first = tag.len() as u32;
        for (k, &o) in order.iter().enumerate() { tag.push([i | (k as u32) << 8, (base + o as usize) as u32]); }
        // The GS strip's triangles (k, k+1, k+2), without the degenerate joins.
        for k in 0..order.len().saturating_sub(2) {
            let (a, b, c) = (order[k], order[k + 1], order[k + 2]);
            if a == b || b == c || a == c { continue; }
            idx.extend([first + k as u32, first + k as u32 + 1, first + k as u32 + 2]);
        }
    }
    let n = tag.len();
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0f32; 3]; n])
        .with_inserted_attribute(ATTRIBUTE_WATER_Z1, vec![0.0f32; n])
        .with_inserted_attribute(ATTRIBUTE_WATER_UV, vec![[0.0f32; 4]; n])
        .with_inserted_attribute(ATTRIBUTE_WATER_TAG, VertexAttributeValues::Uint32x2(tag))
        .with_inserted_indices(Indices::U32(idx))
}

/// Writes patch `p`'s `ripple` record: the mask, the 17×17 vertices and the 46 water UVs.
fn write_patch(bytes: &mut [u8], p: usize, mask: u16, v: &ww::PatchVerts, water_uv: &[[f32; 2]; SUB_STRIP_LEN]) {
    let at = p * RIPPLE_WORDS * 4;
    let Some(rec) = bytes.get_mut(at..at + RIPPLE_WORDS * 4) else { return };
    let mut w = rec.as_chunks_mut::<4>().0.iter_mut();
    let mut put = |x: u32| if let Some(d) = w.next() { *d = x.to_le_bytes() };
    for x in [mask as u32, 0, 0, 0] { put(x); }
    for g in 0..VG * VG {
        let (pos, uv) = (v.pos[g], v.env_uv[g]);
        for x in [pos[0], pos[1], pos[2], uv[0], uv[1]] { put(x.to_bits()); }
        put(v.rgba[g]);
    }
    for uv in water_uv { put(uv[0].to_bits()); put(uv[1].to_bits()); }
}

/// The FX image of index `i` (made once; None: no such FX texture).
fn fx_image(cache: &mut Vec<Option<Handle<Image>>>, images: &mut Assets<Image>, tex: &rc_formats::particle_tex::ParticleTextures, i: i32) -> Option<Handle<Image>> {
    let i = usize::try_from(i).ok()?;
    let t = tex.fx_textures.get(i)?.as_ref()?;
    if cache.len() <= i { cache.resize(i + 1, None); }
    if cache[i].is_none() { cache[i] = Some(crate::fx_draw::fx_image(images, t)); }
    cache[i].clone()
}

#[allow(clippy::too_many_arguments)]
fn setup(
    mut commands: Commands,
    level: Res<crate::Level>,
    fog: Option<Res<GameFog>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<WaterMaterial>>,
    mut buffers: ResMut<Assets<ShaderBuffer>>,
    copy: Res<crate::scene_copy::SceneCopy>,
    mirror: Res<crate::water_mirror::WaterMirror>,
) {
    let lw = &level.0.water;
    if lw.order.is_empty() && lw.chutes.is_empty() { return; }
    let Some(tex) = level.0.particles.textures.as_ref() else {
        eprintln!("water: no FX textures, not drawn");
        return;
    };
    let fog = fog.map(|f| f.uniform).unwrap_or_else(|| TfragFog::new(&level.0.fog));
    let mut fx_images: Vec<Option<Handle<Image>>> = Vec::new();

    let mut fallback_rng = Rng::new();
    fallback_rng.srand(LEVEL_SEED);
    let mut slot = 0u32;
    let mut classes = Vec::new();
    let mut patches = Vec::new();
    let mut ripple_at = usize::MAX;
    let mut patch_mesh_handle: Option<Handle<Mesh>> = None;
    let spawn = |commands: &mut Commands, mesh: Handle<Mesh>, mat: Handle<WaterMaterial>, vis: Visibility, name: String| {
        commands.spawn((Mesh3d(mesh), MeshMaterial3d(mat), Transform::IDENTITY, NoFrustumCulling, vis, Name::new(name))).id()
    };
    // The patch records as the manager's init hands them over (their FX and FIX; levels 11 / 12 set them later).
    let records = lw.data.as_ref().and_then(|d| d.patch_records()).map(<[_]>::to_vec).unwrap_or_default();
    let module = lw.data.as_ref().and_then(|d| d.module.clone());
    // The two per-frame buffers, sized for every slot / patch the setup below can create (a slot per strip
    // layer, two per patch). Contents are written once the layout is known.
    let n_slots: usize = lw.strips.iter().map(|c| c.strips.len() * 2).sum::<usize>() + records.len() * 2;
    let n_patches = records.len();
    let mut frame_bytes = vec![0u8; n_slots.max(1) * FRAME_RECORD];
    let ripple_bytes = vec![0u8; n_patches.max(1) * RIPPLE_WORDS * 4];
    let frame = buffers.add(ShaderBuffer::new(&frame_bytes, RenderAssetUsages::default()));
    let ripple_buf = buffers.add(ShaderBuffer::new(&ripple_bytes, RenderAssetUsages::default()));
    let rings_bytes = vec![0u8; RINGS_BYTES];
    let rings_buf = buffers.add(ShaderBuffer::new(&rings_bytes, RenderAssetUsages::default()));
    let mut all_mats = Vec::new();
    let mut new_mat = |materials: &mut Assets<WaterMaterial>, texture: Handle<Image>, params: WaterParams, slot: u32, fx: i32| {
        let h = materials.add(WaterMaterial {
            texture,
            fog,
            params,
            frame: frame.clone(),
            ripple: ripple_buf.clone(),
            scene_depth: copy.depth.clone(),
            scene_color: copy.color.clone(),
            look: WaterLook::default(),
            rings: rings_buf.clone(),
            mirror: mirror.params.clone(),
            mirror_color: mirror.image.clone(),
            slot,
            fx,
        });
        all_mats.push(h.clone());
        h
    };
    for &class in &lw.order {
        if class == RIPPLE_SLOT {
            let Some(m) = module.as_ref() else { continue };
            ripple_at = classes.len();
            let mesh = meshes.add(patch_mesh(&m.strip_order));
            for (i, p) in records.iter().enumerate() {
                let (Some(tw), Some(te)) = (fx_image(&mut fx_images, &mut images, tex, p.fx_water), fx_image(&mut fx_images, &mut images, tex, p.fx_env)) else {
                    // The FX a patch has at load: a patch built later by its moby (levels 11 / 12) gets textures then.
                    patches.push(None);
                    continue;
                };
                let lift = surface_lift(lw.level, p.centre);
                let mw = new_mat(&mut materials, tw, lifted_params(p.fix_water, 2, slot, i, lift), slot, p.fx_water);
                let me = new_mat(&mut materials, te, params(p.fix_env, 3, slot + 1, i), slot + 1, p.fx_env);
                let ew = spawn(&mut commands, mesh.clone(), mw.clone(), Visibility::Hidden, format!("ripple patch {i} water"));
                let ee = spawn(&mut commands, mesh.clone(), me.clone(), Visibility::Hidden, format!("ripple patch {i} env"));
                patches.push(Some(PatchDraw { entities: [ew, ee], mats: [mw, me], fx: [p.fx_water, p.fx_env], fix: [p.fix_water, p.fix_env], slots: [slot, slot + 1], visible: false, lift }));
                slot += 2;
            }
            patch_mesh_handle = Some(mesh);
            continue;
        }
        let Some(sc) = lw.strips.iter().find(|s| s.class == class) else { continue };
        let mut descs: Vec<StripDescriptor> = sc.strips.clone();
        let w = Pf::f(descs.first().map_or(0.0, |d| d.z_blend));
        if sc.anim == StripAnim::Bob761 {
            // Load pass: 761's first update runs its init.
            let mut verts: Vec<Vec<[f32; 4]>> = descs.iter().map(|d| d.vertices.clone()).collect();
            ww::init_761(&mut verts);
            for (d, v) in descs.iter_mut().zip(verts) { d.vertices = v; }
        }
        let mut strips = Vec::new();
        for (k, d) in descs.into_iter().enumerate() {
            let mesh = meshes.add(strip_mesh(&d));
            let mut slots = Vec::new();
            for layer in 0..2 {
                let Some(t) = fx_image(&mut fx_images, &mut images, tex, d.fx[layer]) else { continue };
                let m = new_mat(&mut materials, t, params(d.fix[layer], layer as u32, slot, 0), slot, d.fx[layer]);
                // Until the first draw: the class's z blend, no scroll, no wobble.
                write_frame(&mut frame_bytes, slot, w.to_f32(), [0.0; 2], &[[0.0; 2]; 8]);
                slots.push(slot);
                slot += 1;
                spawn(&mut commands, mesh.clone(), m, Visibility::Inherited, format!("water {class} strip {k} layer {}", layer + 1));
            }
            let Ok(slots) = <[u32; 2]>::try_from(slots) else { continue };
            strips.push(StripDraw { scroll: ScrollState::new(&d), desc: d, slots });
        }
        classes.push(ClassDraw { class, anim: sc.anim, w, strips });
    }
    // Patches without load-time FX (built later by their moby: levels 11 / 12): their slots follow every other
    // draw's; the textures and FIX are set on their first draw.
    if let Some(mesh) = patch_mesh_handle {
        for (i, p) in patches.iter_mut().enumerate() {
            if p.is_some() { continue; }
            let lift = records.get(i).map_or(0.0, |r| surface_lift(lw.level, r.centre));
            let mw = new_mat(&mut materials, Handle::default(), lifted_params(0, 2, slot, i, lift), slot, -1);
            let me = new_mat(&mut materials, Handle::default(), params(0, 3, slot + 1, i), slot + 1, -1);
            let ew = spawn(&mut commands, mesh.clone(), mw.clone(), Visibility::Hidden, format!("ripple patch {i} water"));
            let ee = spawn(&mut commands, mesh.clone(), me.clone(), Visibility::Hidden, format!("ripple patch {i} env"));
            *p = Some(PatchDraw { entities: [ew, ee], mats: [mw, me], fx: [-1, -1], fix: [0, 0], slots: [slot, slot + 1], visible: false, lift });
            slot += 2;
        }
    }
    // Water Enhanced's chute sheets (crate::chute_water), after every game draw. Kind 4 reads no `frame` record.
    let mut chute_rows = Vec::new();
    if let (false, Some(coll)) = (lw.chutes.is_empty(), level.0.collision.as_ref()) {
        let is_clear = |i: i32| usize::try_from(i).ok().and_then(|i| tex.fx_textures.get(i)?.as_ref()).is_some_and(clear_water);
        let fx = records.iter().map(|p| p.fx_water).chain(lw.strips.iter().flat_map(|c| c.strips.iter().map(|d| d.fx[0]))).chain(CHUTE_FX).find(|&i| is_clear(i));
        let boxes: Vec<[f32; 6]> = lw.strips.iter().flat_map(|c| &c.strips).map(|d| {
            d.vertices.iter().fold([f32::MAX, f32::MAX, f32::MAX, f32::MIN, f32::MIN, f32::MIN], |b, v| {
                [b[0].min(v[0]), b[1].min(v[1]), b[2].min(v[2].min(v[3])), b[3].max(v[0]), b[4].max(v[1]), b[5].max(v[2].max(v[3]))]
            })
        }).collect();
        // Water already there: a strip's box, or a water face (surface 0: the hero's ground probe `0x232dc0` with
        // flags 2) over the spline (from high enough to start above a river the spline runs under); the floor is the
        // probe's re-cast through the water (flags 0x24).
        use rc_game::collision_query::{coll_line, QueryFlags};
        let covered = |p: [f32; 3]| {
            boxes.iter().any(|b| b[0] - 1.0 <= p[0] && p[0] <= b[3] + 1.0 && b[1] - 1.0 <= p[1] && p[1] <= b[4] + 1.0 && b[2] - 3.0 <= p[2] && p[2] <= b[5] + 3.0)
                || coll_line(coll, [p[0], p[1], p[2] + 6.0], [p[0], p[1], p[2] - 4.0], QueryFlags(2)).is_some_and(|o| o.surface_id() == 0)
        };
        let floor = |a: [f32; 3], b: [f32; 3]| coll_line(coll, a, b, QueryFlags(0x24)).map(|o| o.point[2]);
        // No clear water of the level's own (Quartu): its chute is a dry metal slide, drawn bare.
        let found = fx.and_then(|i| Some((fx_image(&mut fx_images, &mut images, tex, i)?, i)));
        for (k, c) in lw.chutes.iter().enumerate() {
            let Some((img, fx)) = found.clone() else {
                chute_rows.push(0);
                continue;
            };
            let s = crate::chute_water::sheet(c, floor, covered);
            chute_rows.push(s.idx.len() / 6);
            if s.idx.is_empty() { continue; }
            let n = s.pos.len();
            let mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
                .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, s.pos.clone())
                .with_inserted_attribute(ATTRIBUTE_WATER_Z1, s.fade)
                .with_inserted_attribute(ATTRIBUTE_WATER_UV, s.uv)
                .with_inserted_attribute(ATTRIBUTE_WATER_TAG, VertexAttributeValues::Uint32x2(vec![[0x0080_8080, 0]; n]))
                .with_inserted_indices(Indices::U32(s.idx));
            let mut p = params(0x80, 4, slot, 0);
            p.misc.z = c.speed;
            let m = new_mat(&mut materials, img, p, slot, fx);
            spawn(&mut commands, meshes.add(mesh), m, Visibility::Inherited, format!("chute {k} water sheet"));
            slot += 1;
        }
        let starts: Vec<[i32; 3]> = lw.chutes.iter().map(|c| c.points[0].map(|x| x.round() as i32)).collect();
        println!("water: chute sheets (Water Enhanced) {} chutes from {starts:?}, quads {chute_rows:?}, fx {}", lw.chutes.len(), fx.map_or("none (no sheets)".to_string(), |i| i.to_string()));
    }
    let data = lw.data.as_ref();
    println!(
        "water: draw order {:?}; strips {:?}; ripple patches {} of {} (module {}, 751 zones {:?}, managers {:?}); {} draws",
        lw.order,
        classes.iter().map(|c| (c.class, c.strips.len())).collect::<Vec<_>>(),
        records.iter().filter(|p| p.centre != [0.0; 3]).count(),
        records.len(),
        module.is_some(),
        data.and_then(|d| d.z751.as_ref()).map(|(t, z)| (t.zones.iter().map(|z| (z.first_patch, z.patch_count)).collect::<Vec<_>>(), z.clone())),
        data.map(|d| d.manager_classes.clone()).unwrap_or_default(),
        slot
    );
    crate::asset_write::set_buffer(&mut buffers, &frame, &frame_bytes);
    commands.insert_resource(WaterState {
        classes,
        ripple_at,
        patches,
        fallback: None,
        fallback_inputs: lw.data.clone().filter(|d| d.z751.is_some()).map(Arc::new),
        fallback_rng,
        counter: 0,
        ticks: 0,
        drawn_ticks: 0,
        stats: std::env::var("RC_WATER_STATS").is_ok_and(|v| v.trim() == "1"),
        fx_images,
        materials: all_mats,
        fog,
        frame,
        frame_bytes,
        ripple_buf,
        ripple_bytes,
        rings: WaterRings::default(),
        rings_buf,
        rings_bytes,
        clear_fx: Default::default(),
        liquid_fx: Default::default(),
    });
}

/// One 60 Hz tick: the 761 z pulse, and without the game tick (`RC_PLAY=0`) Novalis's 751 on the plugin's stream. None
/// on a tick of the held world (`Play::qs_hold`: Quick Select: Modern, docs/plan/quick_select_pause.md).
fn tick(state: Option<ResMut<WaterState>>, play: Option<Res<crate::gameplay::Play>>, cams: MainCamera) {
    let Some(mut st) = state else { return };
    if play.as_ref().is_some_and(|p| p.qs_hold) { return; }
    let st = &mut *st;
    st.counter = st.counter.wrapping_add(1);
    let t = st.counter;
    for c in st.classes.iter_mut().filter(|c| c.anim == StripAnim::Bob761) { c.w = ww::bob(t); }
    if play.is_none() {
        if let Some(d) = st.fallback_inputs.clone() {
            let Some((tables, zones)) = d.z751.as_ref() else { return };
            let cam = cams.iter().next().map_or([0.0; 3], |t| game_eye(t).to_array());
            let mut rng = st.fallback_rng;
            match st.fallback.as_mut() {
                None => st.fallback = Some(RippleSim::new(tables, zones.clone(), d.light_xy, &mut rng)),
                Some(sim) => { sim.tick(cam, &d.cuboids, &mut rng); }
            }
            st.fallback_rng = rng;
        }
    }
    st.ticks += 1;
}

/// Clip-space outcode of a game-space point (bits: 1 left, 2 right, 4 bottom, 8 top, 0x20 behind).
fn outcode(clip_from_world: &Mat4, p: [f32; 3]) -> u32 {
    let c = *clip_from_world * crate::tfrag_render::game_to_bevy(p).extend(1.0);
    let mut o = 0;
    if c.x < -c.w { o |= 1; }
    if c.x > c.w { o |= 2; }
    if c.y < -c.w { o |= 4; }
    if c.y > c.w { o |= 8; }
    if c.w <= 0.0 { o |= 0x20; }
    o
}

/// The draw callbacks of this frame, in list order (runs only on frames that followed a tick).
#[allow(clippy::too_many_arguments)]
pub fn draw(
    state: Option<ResMut<WaterState>>,
    mut play: Option<ResMut<crate::gameplay::Play>>,
    cams: MainCamera,
    fog: Option<Res<GameFog>>,
    level: Res<crate::Level>,
    mut materials: ResMut<Assets<WaterMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut buffers: ResMut<Assets<ShaderBuffer>>,
    mut vis: Query<&mut Visibility>,
    frame: Res<crate::display::GameFrame>,
    time: Res<Time>,
) {
    let Some(mut st) = state else { return };
    let st = &mut *st;
    if st.ticks == st.drawn_ticks { return; }
    st.drawn_ticks = st.ticks;
    let contacts = play.as_deref_mut().map(|p| std::mem::take(&mut p.water_contacts)).unwrap_or_default();
    st.rings.update(contacts, time.elapsed_secs_wrapped(), &mut st.rings_bytes);
    crate::asset_write::set_buffer(&mut buffers, &st.rings_buf, &st.rings_bytes);
    let fog = fog.map(|f| f.uniform).unwrap_or_else(|| TfragFog::new(&level.0.fog));
    // The fog uniform is the only material field that changes: written only when the frame's fog differs.
    if fog != st.fog {
        st.fog = fog;
        for h in &st.materials {
            if let Some(mut m) = materials.get_mut(h) { m.fog = fog; }
        }
    }
    let Some(cam_t) = cams.iter().next() else { return };
    let cam = game_eye(cam_t).to_array();
    // The view's tangent as the game's camera writes it (0x16cf70: Gemlik's ship widens it), as the main camera's.
    let tan = play.as_deref().map_or(crate::game_camera::TAN_HALF_FOV_X, |p| p.svc.view_tan_x);
    // Through crate::display's Hor+ (16:9 widens x, as the main camera's projection).
    let (tan_x, tan_y) = crate::display::view_tans(tan, frame.ratio());
    let proj = GameProjection { tan_x, tan_y, ..GameProjection::default() };
    let clip_from_world = proj.get_clip_from_view() * cam_t.to_matrix().inverse();
    // The ripple module and whether its callback was registered this tick (the moby system's; else the fallback).
    let mut fallback = st.fallback.take();
    let (mut sim, registered) = match play.as_deref_mut() {
        Some(p) => {
            let reg = p.svc.draw_callbacks.list1.iter().any(|(c, _)| *c == Callback::RipplePatches);
            (p.svc.water.sim.as_mut(), reg)
        }
        None => (fallback.as_mut(), true),
    };

    let n = st.classes.len();
    for ci in 0..=n {
        if ci == st.ripple_at {
            let tex = level.0.particles.textures.as_ref();
            draw_ripples(st, sim.as_deref_mut().filter(|_| registered), tex, cam, &clip_from_world, &mut materials, &mut images, &mut buffers, &mut vis);
        }
        let Some(c) = st.classes.get_mut(ci) else { continue };
        let w = c.w;
        for s in &mut c.strips {
            s.scroll.tick(&s.desc);
            let table = ww::wobble_table(s.scroll.phase, s.desc.wobble_amp);
            for (layer, &slot) in s.slots.iter().enumerate() {
                let off = if layer == 0 { s.scroll.s1 } else { s.scroll.s2 };
                write_frame(&mut st.frame_bytes, slot, w.to_f32(), [off[0].to_f32(), off[1].to_f32()], &table);
            }
        }
        // 1225's callback writes the z blend after drawing (0x309bf8).
        if c.anim == StripAnim::BobAfterDraw1225 { c.w = ww::bob(st.counter); }
    }
    st.fallback = fallback;
    crate::asset_write::set_buffer(&mut buffers, &st.frame, &st.frame_bytes);
}

/// The ripple patches of this frame (`sim` None: not registered this tick, nothing drawn).
#[allow(clippy::too_many_arguments)]
fn draw_ripples(
    st: &mut WaterState,
    sim: Option<&mut RippleSim>,
    tex: Option<&rc_formats::particle_tex::ParticleTextures>,
    cam: [f32; 3],
    clip_from_world: &Mat4,
    materials: &mut Assets<WaterMaterial>,
    images: &mut Assets<Image>,
    buffers: &mut Assets<ShaderBuffer>,
    vis: &mut Query<&mut Visibility>,
) {
    let mut wrote = false;
    let mut active = 0;
    let n = st.patches.len();
    let mut sim = sim;
    for p in 0..n {
        let Some(pd) = st.patches[p].as_mut() else { continue };
        let mut show = false;
        if let Some(sim) = sim.as_deref_mut().filter(|s| p < s.patches.len()) {
            let pa = &sim.patches[p];
            show = pa.mask != 0;
            if show {
                active += 1;
                let [x, y, z] = pa.centre.map(Pf::to_f32);
                let codes = [[x - 8.0, y - 8.0], [x + 8.0, y - 8.0], [x - 8.0, y + 8.0], [x + 8.0, y + 8.0]].map(|[a, b]| outcode(clip_from_world, [a, b, z]));
                let all = codes.iter().fold(0x2f, |a, &c| a & c);
                show = all == 0;
            }
            if show {
                // The patch's FX / FIX as the record holds them now (a change re-makes the material's texture / FIX).
                let (fx, fix) = ([pa.rec.fx_water, pa.rec.fx_env], [pa.rec.fix_water, pa.rec.fix_env]);
                for k in 0..2 {
                    if fx[k] != pd.fx[k] || fix[k] != pd.fix[k] {
                        let img = tex.and_then(|t| fx_image(&mut st.fx_images, images, t, fx[k]));
                        if let (Some(img), Some(mut m)) = (img, materials.get_mut(&pd.mats[k])) {
                            m.texture = img;
                            m.params = lifted_params(fix[k], 2 + k as u32, pd.slots[k], p, if k == 0 { pd.lift } else { 0.0 });
                            m.fx = fx[k];
                            pd.fx[k] = fx[k];
                            pd.fix[k] = fix[k];
                        }
                    }
                }
                show = pd.fx[0] == fx[0] && pd.fx[1] == fx[1];
            }
            if show {
                let mask = sim.patches[p].mask;
                let uv = sim.advance_uv(p);
                let verts = sim.patch_verts(p, cam);
                write_patch(&mut st.ripple_bytes, p, mask, &verts, &uv);
                wrote = true;
            }
        }
        if show != pd.visible {
            pd.visible = show;
            for e in pd.entities {
                if let Ok(mut v) = vis.get_mut(e) { *v = if show { Visibility::Inherited } else { Visibility::Hidden }; }
            }
        }
    }
    if wrote {
        crate::asset_write::set_buffer(&mut *buffers, &st.ripple_buf, &st.ripple_bytes);
    }
    if st.stats && st.ticks.is_multiple_of(60) {
        if let Some(s) = sim.as_deref() { println!("water: tick {} active patches {active} clock {:?} render buffer {}", st.ticks, s.clock, s.render); }
    }
}

// ---------------------------------------------------------------------------------------------------
// Fire fields (class 760)

/// The 760 draw callback's tables: `(level, quad tables, flame alpha bytes gp−0x4f68, smoke alpha bytes gp−0x4f60)`
/// (level01 `0x2fe080` reads 0x1fa840..0x1fab40; the level 00 / 14 copies of the callback read the same bytes at
/// their own addresses; gp = 0x166c00 on every overlay).
const FIRE_FIELD_TABLES: [(u32, u32, u32, u32); 3] = [(0, 0x1e_11a0, 0x16_19a0, 0x16_19a8), (1, 0x1f_a840, 0x16_1c98, 0x16_1ca0), (14, 0x1e_04d0, 0x16_1d38, 0x16_1d40)];

/// Transparent3d sort bias of the list-2 callbacks (drained after the particles, bias 1e6).
const LIST2_BIAS: f32 = 2.0e6;

/// The quad tables of the fire / smoke fields (offsets from the level's table base).
#[derive(Clone, Debug, PartialEq)]
pub struct FireFieldTables {
    /// +0x000: the flame quad's corners (x, y, z, 1): y × width, z × height.
    pub flame: [[f32; 4]; 4],
    /// +0x040: the flame's (s, t); the element's scroll is subtracted from s.
    pub flame_st: [[f32; 2]; 4],
    /// gp−0x4f68: the flame's per-corner alpha factor (0 / 1: the top corners fade out).
    pub flame_alpha: [u8; 4],
    /// +0x060: the curtain's 12 points (y × P+0x48, z × P+0x4c).
    pub curtain: [[f32; 4]; 12],
    /// +0x120: the smoke element's 12 points (y × width, z × height).
    pub smoke: [[f32; 4]; 12],
    /// +0x1e0: six quads of four point indices (GS strip order); the curtain draws all six, a smoke element 2 and 3.
    pub quads: [[u32; 4]; 6],
    /// +0x240 / +0x2a0: per point (a, b): the curtain's s = b + the 809 scroll, t = a; a smoke element's s = b +
    /// its scroll, t = a.
    pub curtain_ab: [[f32; 2]; 12],
    pub smoke_ab: [[f32; 2]; 12],
    /// gp−0x4f60: per point, 0 / 1 (the curtain's alpha is 0xff × this, a smoke element's P+0x5c × this).
    pub point_alpha: [u8; 12],
}

impl FireFieldTables {
    /// The level's tables (None: no fire-field class on this level).
    pub fn parse(ov: &wf::Overlay, level: u32) -> Result<Option<FireFieldTables>> {
        let Some(&(_, base, a_flame, a_points)) = FIRE_FIELD_TABLES.iter().find(|t| t.0 == level) else { return Ok(None) };
        let f = |a: u32| -> Result<f32> { Ok(ov.f32(a)?) };
        let v4 = |a: u32| -> Result<[f32; 4]> { Ok([f(a)?, f(a + 4)?, f(a + 8)?, f(a + 12)?]) };
        let v2 = |a: u32| -> Result<[f32; 2]> { Ok([f(a)?, f(a + 4)?]) };
        let mut t = FireFieldTables {
            flame: [[0.0; 4]; 4],
            flame_st: [[0.0; 2]; 4],
            flame_alpha: [0; 4],
            curtain: [[0.0; 4]; 12],
            smoke: [[0.0; 4]; 12],
            quads: [[0; 4]; 6],
            curtain_ab: [[0.0; 2]; 12],
            smoke_ab: [[0.0; 2]; 12],
            point_alpha: [0; 12],
        };
        for k in 0..4 {
            t.flame[k] = v4(base + 16 * k as u32)?;
            t.flame_st[k] = v2(base + 0x40 + 8 * k as u32)?;
        }
        for k in 0..12 {
            t.curtain[k] = v4(base + 0x60 + 16 * k as u32)?;
            t.smoke[k] = v4(base + 0x120 + 16 * k as u32)?;
            t.curtain_ab[k] = v2(base + 0x240 + 8 * k as u32)?;
            t.smoke_ab[k] = v2(base + 0x2a0 + 8 * k as u32)?;
        }
        for (q, quad) in t.quads.iter_mut().enumerate() {
            for (k, v) in quad.iter_mut().enumerate() {
                let i = ov.u32(base + 0x1e0 + 16 * q as u32 + 4 * k as u32)?;
                if i >= 12 { anyhow::bail!("fire-field quad {q} point {k}: index {i} out of range"); }
                *v = i;
            }
        }
        t.flame_alpha.copy_from_slice(ov.read(a_flame, 4)?);
        t.point_alpha.copy_from_slice(ov.read(a_points, 12)?);
        Ok(Some(t))
    }
}

/// One registered field's three draw groups (entity, mesh, material).
/// One draw group's entity, mesh, material and FX image.
type FireFieldGroup = (Entity, Handle<Mesh>, Handle<FxPrimMaterial>, Handle<Image>);

struct FireFieldSlot {
    /// Per group, made when the group first has quads (Bevy's mesh allocator keeps no slab for an empty mesh, so a
    /// group's mesh is never empty).
    groups: [Option<FireFieldGroup>; 3],
    visible: [bool; 3],
}

/// The fire-field renderer's state: the tables, the FX images, the per-slot draws.
#[derive(Resource)]
pub struct FireFieldDraw {
    tables: FireFieldTables,
    /// FX images by FX index, made on first use.
    fx: Vec<Option<Handle<Image>>>,
    slots: Vec<FireFieldSlot>,
    fog: TfragFog,
    /// The tick counter the last draw used (redrawn once per tick, like the other callbacks).
    drawn: Option<u64>,
}

fn fire_field_setup(mut commands: Commands, level: Res<crate::Level>, fog: Option<Res<GameFog>>) {
    let Some(tables) = level.0.water.fire_fields.clone() else { return };
    let n = level.0.particles.textures.as_ref().map_or(0, |t| t.fx_textures.len());
    let fog = fog.map(|f| f.uniform).unwrap_or_else(|| TfragFog::new(&level.0.fog));
    commands.insert_resource(FireFieldDraw { tables, fx: vec![None; n], slots: Vec::new(), fog, drawn: None });
}

fn add3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] { [a[0] + b[0], a[1] + b[1], a[2] + b[2]] }
fn scale3(a: [f32; 3], s: f32) -> [f32; 3] { [a[0] * s, a[1] * s, a[2] * s] }
/// `FastVecCross(out, a, b)` 0x2212d0 (`vopmula` / `vopmsub`): b × a.
fn cross_ba(a: [f32; 3], b: [f32; 3]) -> [f32; 3] { [b[1] * a[2] - b[2] * a[1], b[2] * a[0] - b[0] * a[2], b[0] * a[1] - b[1] * a[0]] }
/// `FastVecNormalize(1, v, v)` 0x221410 (zero stays zero).
fn unit3(v: [f32; 3]) -> [f32; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if l == 0.0 { v } else { scale3(v, 1.0 / l) }
}

/// The callback's two camera-facing bases (0x2fe080 prologue): rows (F, R, U) with F = unit(camera − moby),
/// R = unit(ẑ × F), U = F × R; the mirrored one has −F, −R and the same U.
fn fire_field_bases(cam: [f32; 3], at: [f32; 3]) -> [[[f32; 3]; 3]; 2] {
    let up = [0.0, 0.0, 1.0];
    let f = unit3([cam[0] - at[0], cam[1] - at[1], cam[2] - at[2]]);
    let r = unit3(cross_ba(f, up));
    let u = cross_ba(r, f);
    let fm = scale3(f, -1.0);
    let rm = unit3(cross_ba(fm, up));
    let um = cross_ba(rm, fm);
    [[f, r, u], [fm, rm, um]]
}

/// `fun_001f9d20` with a basis and a translation: `F·x + R·y + U·z + t`.
fn basis_point(b: &[[f32; 3]; 3], t: [f32; 3], v: [f32; 3]) -> [f32; 3] {
    add3(add3(add3(scale3(b[0], v[0]), scale3(b[1], v[1])), scale3(b[2], v[2])), t)
}

/// The three groups of one registered field: flames (0x48), smoke elements (0x44), smoke curtain (0x44, if P+0x42 = 0).
fn fire_field_groups(t: &FireFieldTables, state: &rc_game::moby_update::classes::fire_field::FireFieldState, pv: &[u8], pos: [f32; 3], cub: &[[f32; 4]; 4], scroll_u: f32, cam: [f32; 3]) -> [PrimBuf; 3] {
    use rc_game::moby_update::classes::fire_field::{cuboid_point, drawn, FireFieldState};
    use rc_game::moby_update::services::pvar as p;
    let bases = fire_field_bases(cam, pos);
    let rgb = p::u32(pv, 0x38) & 0xff_ffff;
    let smoke_rgb = (pv[0x5f] as u32) << 16 | (pv[0x5e] as u32) << 8 | pv[0x5d] as u32;
    let smoke_a = pv[0x5c] as u32;
    let mut out: [PrimBuf; 3] = Default::default();
    for i in FireFieldState::range(pv) {
        // The element as this frame's callback leaves it (the PS2 steps it in the draw; the port's step runs at
        // the next tick's start, rc_game's draw_callbacks).
        let Some(e) = state.elems.get(i).map(|e| drawn(state, e)) else { break };
        let b = &bases[(e.mirror != 0) as usize];
        let tr = cuboid_point(cub, e.pos);
        let corner = |v: [f32; 4]| basis_point(b, tr, [v[0], v[1] * e.width, v[2] * e.height]);
        if e.kind == 0 {
            let a = (e.alpha as u32) << 24;
            let p4 = std::array::from_fn(|k| corner(t.flame[k]));
            let st = std::array::from_fn(|k| [t.flame_st[k][0] - e.scroll, t.flame_st[k][1]]);
            let c = std::array::from_fn(|k| a.wrapping_mul(t.flame_alpha[k] as u32) | rgb);
            out[0].quad(p4, st, c);
        } else {
            for q in &t.quads[2..4] {
                let p4 = q.map(|j| corner(t.smoke[j as usize]));
                let st = q.map(|j| [t.smoke_ab[j as usize][1] + e.scroll, t.smoke_ab[j as usize][0]]);
                let c = q.map(|j| (smoke_a << 24).wrapping_mul(t.point_alpha[j as usize] as u32) | smoke_rgb);
                out[1].quad(p4, st, c);
            }
        }
    }
    if p::i16(pv, 0x42) == 0 {
        let tr = [cub[3][0], cub[3][1], cub[3][2] - p::ff(pv, 0x50)];
        let (wd, ht) = (p::ff(pv, 0x48), p::ff(pv, 0x4c));
        for q in &t.quads {
            let p4 = q.map(|j| {
                let v = t.curtain[j as usize];
                basis_point(&bases[0], tr, [v[0], v[1] * wd, v[2] * ht])
            });
            let st = q.map(|j| [t.curtain_ab[j as usize][1] + scroll_u, t.curtain_ab[j as usize][0]]);
            let c = q.map(|j| (t.point_alpha[j as usize] as u32).wrapping_mul(0xff00_0000) | smoke_rgb);
            out[2].quad(p4, st, c);
        }
    }
    out
}

/// The list-2 draws of this tick: every registered 760 field (flames, smoke elements, curtain) in registration
/// order, from the moby system's state (crate::gameplay::Play).
#[allow(clippy::too_many_arguments)]
fn draw_fire_fields(
    mut commands: Commands,
    state: Option<ResMut<FireFieldDraw>>,
    play: Option<Res<crate::gameplay::Play>>,
    cams: MainCamera,
    fog: Option<Res<GameFog>>,
    level: Res<crate::Level>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<FxPrimMaterial>>,
    mut vis: Query<&mut Visibility>,
) {
    let Some(mut st) = state else { return };
    let st = &mut *st;
    let Some(cam_t) = cams.iter().next() else { return };
    let registered: Vec<(usize, rc_game::moby_runtime::MobyId)> = match play.as_deref() {
        Some(p) => p.svc.draw_callbacks.list2.iter().filter(|(cb, _)| *cb == rc_game::moby_update::classes::draw_callbacks::Callback::FireField760).map(|&(_, id)| id).enumerate().collect(),
        None => Vec::new(),
    };
    let counter = play.as_deref().map(|p| p.game.counter);
    let fog = fog.map(|f| f.uniform).unwrap_or_else(|| TfragFog::new(&level.0.fog));
    let fog_changed = fog != st.fog;
    st.fog = fog;
    if fog_changed {
        for (_, _, mat, _) in st.slots.iter().flat_map(|s| s.groups.iter().flatten()) {
            if let Some(mut mm) = materials.get_mut(mat) { mm.fog = fog; }
        }
    }
    if counter == st.drawn { return; }
    st.drawn = counter;
    let cam = game_eye(cam_t).to_array();
    let tex = level.0.particles.textures.as_ref();
    for (k, id) in registered.iter().copied() {
        let Some(p) = play.as_deref() else { break };
        let Some(m) = p.game.mobys.mobys.get(id) else { continue };
        let pv = &m.pvars;
        if pv.len() < 0x60 { continue; }
        use rc_game::moby_update::services::pvar as pvr;
        let Some(cub) = usize::try_from(pvr::i32(pv, 4)).ok().and_then(|c| p.svc.volumes.cuboids.get(c)).map(|s| s.matrix) else { continue };
        let groups = fire_field_groups(&st.tables, &p.svc.fire_fields, pv, [m.position[0], m.position[1], m.position[2]], &cub, p.svc.fire_fields.scroll_u, cam);
        let fx_of = |g: usize| pvr::i32(pv, if g == 2 { 0x58 } else { 0x54 }) + 0x28;
        while st.slots.len() <= k { st.slots.push(FireFieldSlot { groups: [None, None, None], visible: [false; 3] }); }
        for (g, q) in groups.into_iter().enumerate() {
            // The FX image of this group (made once per FX index).
            let fx = usize::try_from(fx_of(g)).ok();
            let img = fx.and_then(|i| {
                let t = tex?.fx_textures.get(i)?.as_ref()?;
                if st.fx.len() <= i { st.fx.resize(i + 1, None); }
                if st.fx[i].is_none() { st.fx[i] = Some(crate::fx_draw::fx_image(&mut images, t)); }
                st.fx[i].clone()
            });
            let slot_no = k as u32;
            let slot = &mut st.slots[k];
            let show = !q.is_empty() && img.is_some();
            if show {
                let img = img.expect("checked");
                match &mut slot.groups[g] {
                    Some((_, mesh, mat, cur)) => {
                        q.update(&mut meshes, mesh);
                        if *cur != img {
                            if let Some(mut mm) = materials.get_mut(&*mat) { mm.texture = img.clone(); }
                            *cur = img;
                        }
                    }
                    None => {
                        let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
                        q.write(&mut m);
                        let mesh = meshes.add(m);
                        let params = FxPrimParams::blend(g == 0);
                        let mat = materials.add(FxPrimMaterial { texture: img.clone(), fog, params, order: LIST2_BIAS + (slot_no * 3 + g as u32) as f32 });
                        let e = commands
                            .spawn((Mesh3d(mesh.clone()), MeshMaterial3d(mat.clone()), Transform::IDENTITY, NoFrustumCulling, Visibility::Inherited, crate::display_blend::DisplayEffect, Name::new(format!("fire field {slot_no} group {g}"))))
                            .id();
                        slot.groups[g] = Some((e, mesh, mat, img));
                        slot.visible[g] = true;
                        continue;
                    }
                }
            }
            if show != slot.visible[g] {
                slot.visible[g] = show;
                if let Some((e, ..)) = &slot.groups[g] {
                    if let Ok(mut v) = vis.get_mut(*e) { *v = if show { Visibility::Inherited } else { Visibility::Hidden }; }
                }
            }
        }
    }
    // Slots nobody registered this tick draw nothing.
    for slot in st.slots.iter_mut().skip(registered.len()) {
        for g in 0..3 {
            if slot.visible[g] {
                slot.visible[g] = false;
                if let Some((e, ..)) = &slot.groups[g] {
                    if let Ok(mut v) = vis.get_mut(*e) { *v = Visibility::Hidden; }
                }
            }
        }
    }
}
