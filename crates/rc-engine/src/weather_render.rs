//! **Port-only: Particles Enhanced's rain and snow** (docs/plan/graphics_options.md, Particles row). The weather stays
//! the game's: the emitter class 1400 (`rc_game::moby_update::classes::units::weather`, levels 08, 12, 14) spawns the
//! drops around the camera only where the level height grid lets them fall, types 0 (`type00`, the rain streak) and 73
//! (`type73`, the flake) move and die as in the game, and `PartProc`'s cull, fade and draw flags run unchanged
//! (crate::particle_render). Only the draw differs: with [`crate::graphics::Particles::atmosphere`] crate::particle_render
//! hands this module the live records of the two types instead of drawing them, and they are drawn here, after every
//! particle, in one instanced draw:
//!
//! * rain: a thin streak, the [`STREAK_SHARE`] of the record's ribbon behind its head (end 2), at least the drop's
//!   travel over [`BLUR_TICKS`] ticks (`type00`'s velocity +0x30 plus the camera step, the motion blur), about one pixel wide at
//!   any distance with the game's wide far drops' coverage kept as brightness, faded toward the tail, tinted by the
//!   level's fog colour (the scene's light), soft where it meets the world;
//! * snow: a round soft flake of the record's size varied ±30 % per flake, fluttering sideways (a visual offset of
//!   at most `weather.wgsl`'s `FLUTTER` units, from the record's pool index and the tick count: no game state, no `rand`), sharp
//!   at mid range and blurred larger and fainter near the camera and far away (a depth-of-field look); and
//!   [`FLAKE_COPIES`] − 1 more flakes per record, each at its own hashed offset of up to `weather.wgsl`'s
//!   `EXTRA_SPREAD` from it (2.2 units across, 2.6 up and down), sized for its own depth, 0.6..0.9 of the size and a
//!   little fainter: the snowfall about four times as thick, only where and while the game's flakes fall (they move,
//!   fade and die with their record; nothing of the game's is added or read but the record).
//!
//! Splashes (type 1) keep the game's look. Original and Soft never reach this module (no entity, no resource).
//!
//! Sprite buffer (`particle_render::SpriteBuffer`'s layout; `weather.wgsl`): header (count, ticks, fog RGBA, 0);
//! rain `p` = (tail, half-width), `q` = (head, 0), `c` = (RGBA tail, 1, RGBA head, index); snow `p` = (centre, a.x),
//! `q` = (a.y, b.x, b.y, 0) in game pixels as the particle sprites, `c` = (RGBA, 2, 0, index).

use crate::graphics::GraphicsSettings;
use crate::particle_render::{quad_mesh, SpriteBuffer, ATTRIBUTE_SPRITE_CORNER};
use crate::tfrag_render::game_to_bevy;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::ecs::system::SystemParam;
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, BlendComponent, BlendFactor, BlendOperation, BlendState, RenderPipelineDescriptor, SpecializedMeshPipelineError,
};
use bevy::render::storage::ShaderBuffer;
use bevy::shader::ShaderRef;
use rc_game::moby_update::classes::units::weather;
use rc_game::particles::{rec, type00, type73, POOL_RECORDS};

const SHADER_PATH: &str = "shaders/weather.wgsl";

/// Style codes in `c.y`.
pub const STYLE_RAIN: u32 = 1;
pub const STYLE_SNOW: u32 = 2;
/// A streak covers the drop's travel over this many ticks (at least the game's own length).
pub const BLUR_TICKS: f32 = 0.5;
/// The share of the game's ribbon drawn, from its head (the game's streak texture shows a short dash of it).
pub const STREAK_SHARE: f32 = 0.4;
/// After every particle draw (crate::particle_render's four slots).
const DRAW_BIAS: f32 = 1.0e6 + 10.0;
/// Quads per sprite (`weather.wgsl`'s `COPIES`): a flake record draws its own flake and this many − 1 more around it.
pub const FLAKE_COPIES: u32 = 4;

/// This frame's weather sprites (filled by crate::particle_render's `build_sprites` while Particles is Enhanced).
#[derive(Resource)]
pub(crate) struct WeatherSprites {
    pub sprites: SpriteBuffer,
    buffer: Handle<ShaderBuffer>,
}

#[derive(Component)]
struct WeatherDraw;

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct WeatherMaterial {
    #[storage(0, read_only, visibility(vertex, fragment))]
    pub sprites: Handle<ShaderBuffer>,
    /// The opaque scene's depth (crate::scene_copy; Enhanced keeps the copy on).
    #[texture(1, sample_type = "float", filterable = false)]
    pub scene_depth: Handle<Image>,
}

/// Premultiplied: rgb = C·A, alpha = A (over).
const PREMULTIPLIED: BlendState = BlendState {
    color: BlendComponent { src_factor: BlendFactor::One, dst_factor: BlendFactor::OneMinusSrcAlpha, operation: BlendOperation::Add },
    alpha: BlendComponent { src_factor: BlendFactor::Zero, dst_factor: BlendFactor::One, operation: BlendOperation::Add },
};

impl Material for WeatherMaterial {
    fn vertex_shader() -> ShaderRef { SHADER_PATH.into() }
    fn fragment_shader() -> ShaderRef { SHADER_PATH.into() }
    fn alpha_mode(&self) -> AlphaMode { AlphaMode::Blend }
    fn depth_bias(&self) -> f32 { DRAW_BIAS }
    fn enable_prepass() -> bool { false }
    fn enable_shadows() -> bool { false }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = None;
        if let Some(ds) = descriptor.depth_stencil.as_mut() { ds.depth_write_enabled = Some(false); }
        if let Some(frag) = descriptor.fragment.as_mut() {
            for t in frag.targets.iter_mut().flatten() { t.blend = Some(PREMULTIPLIED); }
        }
        descriptor.vertex.buffers = vec![layout.0.get_layout(&[ATTRIBUTE_SPRITE_CORNER.at_shader_location(0)])?];
        Ok(())
    }
}

pub struct WeatherRenderPlugin;

impl Plugin for WeatherRenderPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<WeatherMaterial>::default())
            .add_systems(crate::level_switch::LevelUnload, (despawn, crate::level_switch::remove::<WeatherSprites>))
            .add_systems(PostUpdate, (ensure.before(crate::particle_render::BuildSprites), upload.after(crate::particle_render::BuildSprites)));
    }
}

/// The levels whose class 1400 makes weather (`weather::REFERENCE_LEVEL`, `HOVEN_LEVEL`, `OLTANIS_LEVEL`).
pub fn weather_level(level: u32) -> bool { matches!(level, weather::REFERENCE_LEVEL | weather::HOVEN_LEVEL | weather::OLTANIS_LEVEL) }

/// What `particle_render::build_sprites` needs: the option and this frame's list.
#[derive(SystemParam)]
pub(crate) struct WeatherParams<'w> {
    g: Res<'w, GraphicsSettings>,
    sprites: Option<ResMut<'w, WeatherSprites>>,
}

impl WeatherParams<'_> {
    /// This frame's list, emptied, with the header (`ticks`: the particle ticks so far, the flutter's clock; the fog
    /// colour, the streaks' and flakes' light); `None` (and nothing diverted) unless Enhanced on a weather level.
    pub(crate) fn frame(&mut self, level: u32, ticks: u64, fog: Option<&crate::game_camera::GameFog>) -> Option<&mut WeatherSprites> {
        let on = self.g.particles.atmosphere() && weather_level(level);
        let w = self.sprites.as_deref_mut()?;
        w.sprites.clear();
        if !on { return None; }
        let [r, g, b] = fog.map_or([0x80; 3], |f| f.fog.color);
        w.sprites.set_header(ticks as u32, r as u32 | (g as u32) << 8 | (b as u32) << 16 | 0xff << 24);
        Some(w)
    }
}

/// Takes a live type-0 ribbon or type-73 sprite record (pool index `index`) into the weather list: `alpha` is
/// `PartProc`'s fade (the ribbon's m, 0..0x1000; the sprite's faded alpha byte), `cz` its camera depth, `size_px`
/// the sprites' size scale, `wind` the weather's camera step. False: not a weather record (drawn as before).
pub(crate) fn push(w: &mut WeatherSprites, r: &[u8; 64], index: usize, alpha: u32, cz: f32, size_px: f32, wind: [f32; 4]) -> bool {
    let kind = r[1] & 3;
    if r[0] == type00::TYPE && kind == 3 {
        let v = rec::v4(r, 0x30);
        let d = Vec3::new(v[0] + wind[0], v[1] + wind[1], v[2] + wind[2]);
        let (tail, head) = rain_streak(Vec3::from_array(rec::pos(r)), Vec3::from_array(rec::v3(r, 0x20)), d);
        let fade = |c: u32| (c & 0x00ff_ffff) | (((c >> 24) * alpha) >> 12).min(0xff) << 24;
        let (t, h) = (game_to_bevy(tail.to_array()), game_to_bevy(head.to_array()));
        w.sprites.push_raw([t.x, t.y, t.z, rec::ff(r, 0x1c)], [h.x, h.y, h.z, 0.0], [fade(rec::u32(r, 4)), STYLE_RAIN, fade(rec::u32(r, 0xc)), index as u32]);
        return true;
    }
    if r[0] == type73::TYPE && kind == 0 {
        let q = f32::from_bits(rec::u32(r, 0xc)) * size_px / (cz + 0.5);
        let theta = r[8] as f32 * (std::f32::consts::TAU / 256.0);
        let (s, c) = (theta.sin() * q, theta.cos() * q);
        let p = game_to_bevy(rec::pos(r));
        let rgba = (rec::u32(r, 4) & 0x00ff_ffff) | alpha.min(0xff) << 24;
        w.sprites.push_raw([p.x, p.y, p.z, c], [1.0625 * s, -s, 1.0625 * c, 0.0], [rgba, STYLE_SNOW, 0, index as u32]);
        return true;
    }
    false
}

/// The weather draw, spawned the first time Particles Enhanced runs on a weather level.
#[allow(clippy::too_many_arguments)]
fn ensure(
    mut commands: Commands,
    g: Res<GraphicsSettings>,
    have: Option<Res<WeatherSprites>>,
    sim: Option<Res<crate::particle_render::ParticleSim>>,
    copy: Res<crate::scene_copy::SceneCopy>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<WeatherMaterial>>,
    mut buffers: ResMut<Assets<ShaderBuffer>>,
) {
    if have.is_some() || !g.particles.atmosphere() || !sim.as_ref().is_some_and(|s| weather_level(s.sys.level)) { return; }
    let depth = copy.depth.clone();
    let mut sprites = SpriteBuffer::new(POOL_RECORDS as u32);
    let buffer = buffers.add(sprites.asset());
    let mat = materials.add(WeatherMaterial { sprites: buffer.clone(), scene_depth: depth });
    println!("weather: Particles Enhanced draws level {:02}'s rain and snow", sim.map_or(0, |s| s.sys.level));
    commands.spawn((
        Mesh3d(meshes.add(quad_mesh(POOL_RECORDS as u32 * FLAKE_COPIES))),
        MeshMaterial3d(mat),
        Transform::IDENTITY,
        NoFrustumCulling,
        Visibility::Hidden,
        WeatherDraw,
        Name::new("weather (Particles Enhanced)"),
    ));
    commands.insert_resource(WeatherSprites { sprites, buffer });
}

fn despawn(mut commands: Commands, draws: Query<Entity, With<WeatherDraw>>) {
    for e in &draws { commands.entity(e).despawn(); }
}

/// The frame's list to the GPU; the draw hidden while it is empty.
fn upload(w: Option<ResMut<WeatherSprites>>, mut buffers: ResMut<Assets<ShaderBuffer>>, mut draws: Query<&mut Visibility, With<WeatherDraw>>) {
    let Some(mut w) = w else { return };
    let n = w.sprites.len();
    for mut v in &mut draws {
        let want = if n == 0 { Visibility::Hidden } else { Visibility::Inherited };
        if *v != want { *v = want; }
    }
    if n == 0 { return; }
    let WeatherSprites { sprites, buffer } = &mut *w;
    crate::asset_write::set_buffer(&mut buffers, buffer, &sprites.contents());
}

/// A rain record's streak: (tail, head) from end 1 / end 2 (`type00`: end 2 = end 1 + unit(vel)·length, ahead of the
/// drop) and the travel `d` per tick (velocity + camera step): the head's [`STREAK_SHARE`] of the ribbon, at least
/// [`BLUR_TICKS`] ticks of travel long.
pub fn rain_streak(end1: Vec3, end2: Vec3, d: Vec3) -> (Vec3, Vec3) {
    let seg = end2 - end1;
    let len = seg.length();
    let dir = if len > 1e-6 { seg / len } else { d.normalize_or(Vec3::NEG_Z) };
    let want = (len * STREAK_SHARE).max(d.length() * BLUR_TICKS);
    (end2 - dir * want, end2)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shader draws as many quads per sprite as the mesh holds.
    #[test]
    fn flake_copies_match_the_shader() {
        assert!(include_str!("../assets/shaders/weather.wgsl").contains(&format!("const COPIES: u32 = {FLAKE_COPIES}u;")));
    }

    /// A streak keeps its head, points along the drop and covers at least the blur's travel.
    #[test]
    fn streak_covers_the_travel() {
        let (t, h) = rain_streak(Vec3::new(1.0, 1.0, 5.0), Vec3::new(1.0, 1.0, 4.99), Vec3::new(0.0, 0.0, -0.3));
        assert_eq!(h, Vec3::new(1.0, 1.0, 4.99));
        assert!(((h - t).length() - 0.15).abs() < 1e-5 && t.z > h.z);
        let (t, _) = rain_streak(Vec3::new(0.0, 0.0, 5.0), Vec3::new(0.0, 0.0, 2.0), Vec3::new(0.0, 0.0, -0.3));
        assert!((t - Vec3::new(0.0, 0.0, 3.2)).length() < 1e-5, "the head share of the game's longer streak");
    }
}
