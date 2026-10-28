//! **Port-only: Particles Enhanced** (crate::graphics): the bright effects light the opaque world around them.
//!
//! **Sources** (main world, every frame, read only: the game state, its RNG and the particle simulation are never
//! touched): the live records of the particle pool (crate::particle_render: kinds 0, 1 and the ribbons 3; the
//! additive ones, ALPHA 0x48, by their whole colour once it is bright (dim grey additive smoke emits nothing), the
//! normal ones (0x44, e.g. the Pyrocitor's flame
//! `rc_game::particles::type12`) only by the part of their colour above 0x80, MODULATE's overbright), the explosion
//! flashes (classes 0x70 / 1192, `rc_game::moby_update::classes::debris::flash_update`: ambient colour, alpha, the
//! growing bounding sphere) and the Bomb Glove's fireballs (122, `classes::bomb::fireball_update`, a fire colour
//! tinted by the ambient). Each gives a colour, a power (peak channel × alpha × size, [`sprite_source`]) and a reach
//! (a few times its size); sources within [`MERGE`] of a brighter one merge ([`cluster`]: an explosion's sparks,
//! flashes and fireballs make one light), and a light's intensity saturates with its power ([`to_light`]). The
//! [`select`]ed lights (`RC_PARTICLE_LIGHTS_MAX`, default [`DEFAULT_MAX`], the highest `intensity · r² / max(d², r²)`
//! from the camera) are tracked from frame to frame ([`Tracker`]): a light fades in over [`FADE_IN`] and out over
//! [`FADE_OUT`] when its effect appears or ends or when it enters or leaves the cap, so the cap never pops.
//!
//! **Lighting** (render world): a full-screen pass after the opaque world, its shadows and the ambient occlusion,
//! before the scene copy and the transparent pass (`assets/shaders/particle_lights.wgsl`): each pixel's world position
//! and normal rebuilt from the depth buffer (crate::fx_view), and per light a smooth window `(1 − d²/r²)²`
//! ([`falloff`]) times a wrapped `n·l`; the frame is multiplied by `1 + L`. A deferred-style pass rather than a loop in
//! the world shaders: the port's tfrag, tie, shrub and moby shaders compute no Bevy lighting and each has its own
//! buffers (a new binding reorders their draws: crate::world_lights), while the frame already holds each surface's
//! texture × baked light, so one pass lights all of them alike and Original / Soft keep their shaders and draws. The
//! frame is 8-bit, so a blend's source is clamped to 1: the pass draws three times with `∛(1 + L) − 1` (up to ×8).
//!
//! Original and Soft: the source system returns at once and no pass is drawn; nothing new lives in either world but
//! systems and two statics (a resource is an entity: crate::world_lights). Enhanced without a light draws nothing.
//!
//! Limits: no shadows (a light reaches through walls within its radius), the translucent draws are not lit, and the
//! multiply leaves black surfaces black (the frame stands in for the albedo, so a dark corner brightens less than
//! one the level lights).
//!
//! Dev switches: `RC_PARTICLE_LIGHTS_MAX=<n>` (the cap, at most [`MAX_CAP`]), `RC_PARTICLE_LIGHTS_SCALE=<k>` (the
//! intensity factor, default 1), `RC_PARTICLE_LIGHTS_DEBUG=1` (each light drawn as its sphere's outline and centre;
//! `RC_PARTICLE_LIGHTS_DEBUG=2` also prints the lights once a second).

use crate::fly_cam::FlyCam;
use crate::fx_view::{self, depth_defs, depth_entry, Fullscreen, FxView, Pipelines};
use crate::graphics::GraphicsSettings;
use crate::shadow_render::{ShadowCamera, ShadowPassSet};
use crate::tfrag_render::game_to_bevy;
use bevy::core_pipeline::core_3d::{main_opaque_pass_3d, main_transparent_pass_3d};
use bevy::core_pipeline::{Core3d, Core3dSystems, FullscreenShader};
use bevy::prelude::*;
use bevy::render::camera::ExtractedCamera;
use bevy::render::render_resource::binding_types::uniform_buffer;
use bevy::render::render_resource::{
    BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, BlendComponent, BlendFactor, BlendOperation, BlendState,
    PipelineCache, RenderPassDescriptor, ShaderStages, ShaderType, TextureFormat,
};
use bevy::render::renderer::{RenderContext, ViewQuery};
use bevy::render::view::{ExtractedView, ViewDepthTexture, ViewTarget};
use bevy::render::{ExtractSchedule, RenderApp};
use bevy::shader::Shader;
use rc_game::moby_runtime::Moby;
use rc_game::moby_update::classes::{bomb, debris};
use rc_game::particles::{rec, PartPool};
use std::sync::Mutex;

const SHADER_PATH: &str = "shaders/particle_lights.wgsl";
/// Multiplying draws per frame; each returns `(1 + L)^(1/DRAWS) − 1` (particle_lights.wgsl).
const DRAWS: usize = 3;
/// Lights the shader takes: the cap and the ones still fading out.
pub const GPU_LIGHTS: usize = 64;
/// Lights fully on at most (`RC_PARTICLE_LIGHTS_MAX`).
pub const DEFAULT_MAX: usize = 32;
/// The largest cap the switch takes (the rest of [`GPU_LIGHTS`] is for the fading ones).
pub const MAX_CAP: usize = 48;
/// Seconds a light takes to come on, and to go off once its effect ends or it leaves the cap.
pub const FADE_IN: f32 = 0.05;
pub const FADE_OUT: f32 = 0.15;
/// Effects farther than this from the camera light nothing (world units).
const MAX_DISTANCE: f32 = 120.0;
/// Sources nearer than this to a brighter one merge into its light (world units).
pub const MERGE: f32 = 2.5;
/// Clusters kept per frame before the selection.
const MAX_CLUSTERS: usize = 256;
/// A sprite's reach in sprite radii (`size / 420000`, crate::particle_render), and every light's bounds.
const SPRITE_REACH: f32 = 6.0;
/// A flash's or fireball's reach in bounding-sphere radii, and their power against a sprite's.
const MOBY_REACH: f32 = 4.0;
const FLASH_GAIN: f32 = 4.0;
pub const MIN_RADIUS: f32 = 1.0;
pub const MAX_RADIUS: f32 = 10.0;
/// A light's intensity at its centre (the frame there is multiplied by 1 + it) as its power grows without bound,
/// and the power at which it reaches 63 % of that.
pub const PEAK: f32 = 5.0;
const POWER_REF: f32 = 4.0;
/// The fireballs' colour (their texture's orange; the moby's ambient tints it).
const FIRE: Vec3 = Vec3::new(1.0, 0.5, 0.15);

/// One effect's contribution before merging: position (Bevy world), reach, linear chroma (peak channel 1), power.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Source {
    pub pos: Vec3,
    pub radius: f32,
    pub colour: Vec3,
    pub power: f32,
}

/// A light: position (Bevy world), radius, linear colour × intensity.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Light {
    pub pos: Vec3,
    pub radius: f32,
    pub colour: Vec3,
}

impl Light {
    pub fn intensity(&self) -> f32 { self.colour.max_element() }
}

/// The smooth window of a light of radius `r` at distance `d`: 1 at the centre, 0 with a zero slope at `r` (the
/// shader's `light_window`).
#[cfg(test)]
pub fn falloff(d: f32, r: f32) -> f32 {
    if r <= 0.0 { return 0.0; }
    let x = (1.0 - (d * d) / (r * r)).max(0.0);
    x * x
}

/// A sprite's chroma and power from its GS colour bytes (`0x80` = 1.0 under MODULATE), alpha and radius (world
/// units): additive sprites from a peak channel of 0x40 (fully from 0x80: the flyers' grey 0x26..0x34 type-6 exhaust,
/// particles.md "Blarg flyer trails", is smoke, not light), normal ones only by the overbright part (None: emits nothing).
pub fn sprite_source(rgb: [u8; 3], alpha: u8, rad: f32, additive: bool) -> Option<(Vec3, f32)> {
    let c = Vec3::new(rgb[0] as f32, rgb[1] as f32, rgb[2] as f32) / 128.0;
    let peak = c.max_element();
    let weight = if additive { ((peak - 0.5) * 2.0).clamp(0.0, 1.0) } else { (peak - 1.0).clamp(0.0, 1.0) };
    let power = peak * (alpha as f32 / 128.0).min(2.0) * weight * rad.max(0.0);
    (peak > 0.0 && power > 1.0e-4).then(|| (linear_chroma(c / peak), power))
}

/// Display-referred chroma to the linear light the frame is blended in.
fn linear_chroma(c: Vec3) -> Vec3 { c.max(Vec3::ZERO).powf(2.2) }

/// Merges every source into the first brighter cluster whose seed is within [`MERGE`]: power summed, position and
/// colour power-weighted, the reach the farthest of its members'.
pub fn cluster(mut sources: Vec<Source>) -> Vec<Source> {
    sources.sort_by(|a, b| b.power.total_cmp(&a.power));
    // (seed, power, Σ power·pos, Σ power·colour, radius)
    let mut out: Vec<(Vec3, f32, Vec3, Vec3, f32)> = Vec::new();
    for s in sources {
        if let Some(c) = out.iter_mut().find(|c| c.0.distance_squared(s.pos) < MERGE * MERGE) {
            c.1 += s.power;
            c.2 += s.pos * s.power;
            c.3 += s.colour * s.power;
            c.4 = c.4.max(s.radius + c.0.distance(s.pos)).min(MAX_RADIUS);
        } else if out.len() < MAX_CLUSTERS {
            out.push((s.pos, s.power, s.pos * s.power, s.colour * s.power, s.radius));
        }
    }
    out.into_iter().map(|(_, p, ps, cs, r)| Source { pos: ps / p, radius: r, colour: cs / p, power: p }).collect()
}

/// A merged source's light: intensity `PEAK·(1 − e^(−power))`, the chroma scaled to it.
pub fn to_light(s: &Source) -> Light {
    let i = PEAK * (1.0 - (-s.power / POWER_REF).exp());
    let peak = s.colour.max_element();
    let chroma = if peak > 0.0 { s.colour / peak } else { Vec3::ZERO };
    Light { pos: s.pos, radius: s.radius.clamp(MIN_RADIUS, MAX_RADIUS), colour: chroma * i }
}

/// How much a light matters to the view from `eye`: its intensity, by its size over the distance (full inside it).
pub fn score(l: &Light, eye: Vec3) -> f32 {
    let r2 = l.radius * l.radius;
    l.intensity() * r2 / l.pos.distance_squared(eye).max(r2)
}

/// The `max` lights that matter most, in that order.
pub fn select(mut lights: Vec<Light>, eye: Vec3, max: usize) -> Vec<Light> {
    lights.sort_by(|a, b| score(b, eye).total_cmp(&score(a, eye)));
    lights.truncate(max);
    lights
}

/// A light the shader draws: the last values of its effect and how far it is on (0..1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tracked {
    pub light: Light,
    pub weight: f32,
    live: bool,
}

/// The lights from frame to frame: this frame's selection matched to the last frame's lights by position.
#[derive(Clone, Debug, Default)]
pub struct Tracker {
    pub lights: Vec<Tracked>,
}

impl Tracker {
    /// One frame of `dt` seconds with `wanted` selected: a match (the nearest within its half radius or [`MERGE`])
    /// takes the new values and comes on, the others fade out where they were; new ones start at one step of
    /// [`FADE_IN`]. At most [`GPU_LIGHTS`] (the dimmest fading ones go first).
    pub fn update(&mut self, wanted: &[Light], dt: f32) {
        for t in &mut self.lights { t.live = false; }
        let step_in = if FADE_IN > 0.0 { dt / FADE_IN } else { 1.0 };
        for w in wanted {
            let reach = (w.radius * 0.5).max(MERGE);
            let best = self
                .lights
                .iter()
                .enumerate()
                .filter(|(_, t)| !t.live)
                .map(|(i, t)| (i, t.light.pos.distance(w.pos)))
                .filter(|&(_, d)| d < reach)
                .min_by(|a, b| a.1.total_cmp(&b.1));
            match best {
                Some((i, _)) => {
                    let t = &mut self.lights[i];
                    t.light = *w;
                    t.weight = (t.weight + step_in).min(1.0);
                    t.live = true;
                }
                None => self.lights.push(Tracked { light: *w, weight: step_in.min(1.0), live: true }),
            }
        }
        let step_out = dt / FADE_OUT;
        for t in &mut self.lights {
            if !t.live { t.weight -= step_out; }
        }
        self.lights.retain(|t| t.weight > 0.0);
        if self.lights.len() > GPU_LIGHTS {
            self.lights.sort_by(|a, b| b.live.cmp(&a.live).then((b.weight * b.light.intensity()).total_cmp(&(a.weight * a.light.intensity()))));
            self.lights.truncate(GPU_LIGHTS);
        }
    }
}

/// This frame's lights as the shader takes them: `(position, radius)` and `(colour × weight, 0)`.
#[derive(Clone, Debug, Default)]
struct Frame {
    lights: Vec<[Vec4; 2]>,
    scale: f32,
    debug: bool,
}

/// Written by [`collect`] (main world), copied by [`extract`] while the main world waits, read by [`light_pass`].
static MAIN: Mutex<Frame> = Mutex::new(Frame { lights: Vec::new(), scale: 1.0, debug: false });
static RENDER: Mutex<Frame> = Mutex::new(Frame { lights: Vec::new(), scale: 1.0, debug: false });

fn frame_of(lock: &Mutex<Frame>) -> std::sync::MutexGuard<'_, Frame> { lock.lock().unwrap_or_else(|e| e.into_inner()) }

/// The render-world set of the pass.
#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ParticleLightsSet;

pub struct ParticleLightsPlugin;

impl Plugin for ParticleLightsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PostUpdate, collect.after(bevy::transform::TransformSystems::Propagate));
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else { return };
        render_app.add_systems(ExtractSchedule, extract).add_systems(
            Core3d,
            light_pass
                .after(main_opaque_pass_3d)
                .after(ShadowPassSet)
                .after(crate::sun_shadow::SunShadowSet)
                .after(crate::ao_render::AoSet)
                .before(crate::scene_copy::SceneCopySet)
                .before(main_transparent_pass_3d)
                .in_set(Core3dSystems::MainPass)
                .in_set(ParticleLightsSet),
        );
    }
}

/// [`collect`]'s state between frames (a `Local`: see the module docs).
struct Collect {
    tracker: Tracker,
    max: usize,
    scale: f32,
    debug: u8,
    printed: f64,
}

impl Default for Collect {
    fn default() -> Self {
        let env = |k: &str| std::env::var(k).ok();
        Collect {
            tracker: Tracker::default(),
            max: env("RC_PARTICLE_LIGHTS_MAX").and_then(|v| v.trim().parse().ok()).unwrap_or(DEFAULT_MAX).min(MAX_CAP),
            scale: env("RC_PARTICLE_LIGHTS_SCALE").and_then(|v| v.trim().parse().ok()).filter(|k: &f32| k.is_finite() && *k >= 0.0).unwrap_or(1.0),
            debug: env("RC_PARTICLE_LIGHTS_DEBUG").and_then(|v| v.trim().parse().ok()).unwrap_or(0),
            printed: 0.0,
        }
    }
}

/// The particle pool's sources: live records of kinds 0, 1 and 3 within [`MAX_DISTANCE`] of `eye` (Bevy world);
/// `by_type` sums their power per update type (byte 0; the debug print).
fn particle_sources(pool: &PartPool, eye: Vec3, out: &mut Vec<Source>, by_type: &mut [f32; 256]) {
    let hw = (pool.hw + 1).max(0) as usize;
    for r in pool.recs.iter().take(hw) {
        if r[1] & 0x80 != 0 { continue; }
        let kind = r[1] & 3;
        let additive = r[3] == 0x48;
        let (pos, rgba, alpha, rad) = match kind {
            0 | 1 => (game_to_bevy(rec::pos(r)), rec::u32(r, 4), r[7], f32::from_bits(rec::u32(r, 0xc)) / 420_000.0),
            3 => {
                let (a, b) = (game_to_bevy(rec::pos(r)), game_to_bevy(rec::v3(r, 0x20)));
                let c = rec::u32(r, 4);
                ((a + b) * 0.5, c, (c >> 24) as u8, rec::ff(r, 0x1c).abs().max(a.distance(b) * 0.25))
            }
            _ => continue,
        };
        if !pos.is_finite() || !rad.is_finite() || pos.distance_squared(eye) > MAX_DISTANCE * MAX_DISTANCE { continue; }
        let rgb = [rgba as u8, (rgba >> 8) as u8, (rgba >> 16) as u8];
        let Some((colour, power)) = sprite_source(rgb, alpha, rad, additive) else { continue };
        by_type[r[0] as usize] += power;
        out.push(Source { pos, radius: (rad * SPRITE_REACH).clamp(MIN_RADIUS, MAX_RADIUS), colour, power });
    }
}

/// The flashes' and fireballs' sources.
fn moby_sources(mobys: &[Moby], eye: Vec3, out: &mut Vec<Source>) {
    for m in mobys.iter().filter(|m| m.state < 0xfd) {
        let flash = debris::FLASH_CLASSES.contains(&m.o_class);
        if !flash && !bomb::FIREBALL_CLASSES.contains(&m.o_class) { continue; }
        let pos = game_to_bevy([m.position[0], m.position[1], m.position[2]]);
        let r = m.bsphere[3] / 1024.0;
        let rad = if r.is_finite() && r > 0.0 { r } else { m.scale.abs() };
        if !pos.is_finite() || !rad.is_finite() || pos.distance_squared(eye) > MAX_DISTANCE * MAX_DISTANCE { continue; }
        let tint = Vec3::new(m.ambient[0] as f32, m.ambient[1] as f32, m.ambient[2] as f32) / 128.0;
        let c = if flash { tint } else { FIRE * tint };
        let bytes = (c * 128.0).min(Vec3::splat(255.0));
        let Some((colour, power)) = sprite_source([bytes.x as u8, bytes.y as u8, bytes.z as u8], m.alpha, rad, true) else { continue };
        out.push(Source { pos, radius: (rad * MOBY_REACH).clamp(MIN_RADIUS, MAX_RADIUS), colour, power: power * FLASH_GAIN });
    }
}

/// This frame's lights into [`MAIN`] (Enhanced only).
fn collect(
    g: Res<GraphicsSettings>,
    time: Res<Time>,
    sim: Option<Res<crate::particle_render::ParticleSim>>,
    play: Option<Res<crate::gameplay::Play>>,
    cams: Query<&Transform, With<FlyCam>>,
    mut st: Local<Collect>,
) {
    if !g.particles.lights() {
        if !st.tracker.lights.is_empty() {
            st.tracker = Tracker::default();
            frame_of(&MAIN).lights.clear();
        }
        return;
    }
    let Some(eye) = cams.iter().next().map(|t| t.translation) else { return };
    let mut sources = Vec::new();
    let mut by_type = [0.0; 256];
    if let Some(sim) = &sim { particle_sources(&sim.sys.pool, eye, &mut sources, &mut by_type); }
    if let Some(p) = &play { moby_sources(&p.game.mobys.mobys, eye, &mut sources); }
    let lights = select(cluster(sources).iter().map(to_light).collect(), eye, st.max);
    st.tracker.update(&lights, time.delta_secs());
    let now = time.elapsed_secs_f64();
    if st.debug >= 2 && now - st.printed >= 1.0 {
        st.printed = now;
        let v: Vec<String> = st.tracker.lights.iter().map(|t| format!("({:.1} {:.1} {:.1}) r {:.1} i {:.2} w {:.2}", t.light.pos.x, t.light.pos.y, t.light.pos.z, t.light.radius, t.light.intensity(), t.weight)).collect();
        let mut types: Vec<(usize, f32)> = by_type.iter().copied().enumerate().filter(|t| t.1 > 0.0).collect();
        types.sort_by(|a, b| b.1.total_cmp(&a.1));
        let t: Vec<String> = types.iter().take(8).map(|(k, p)| format!("{k}:{p:.1}")).collect();
        println!("particle lights: {} selected, {} drawn, power by type [{}]: {}", lights.len(), v.len(), t.join(" "), v.join("; "));
    }
    let mut f = frame_of(&MAIN);
    f.lights.clear();
    f.lights.extend(st.tracker.lights.iter().map(|t| [t.light.pos.extend(t.light.radius), (t.light.colour * t.weight).extend(0.0)]));
    f.scale = st.scale;
    f.debug = st.debug > 0;
}

/// [`MAIN`] into [`RENDER`] (the main world is idle during the extraction).
fn extract() {
    let f = frame_of(&MAIN).clone();
    *frame_of(&RENDER) = f;
}

/// `ParticleLights` in the shader.
#[derive(Clone, Copy, ShaderType)]
struct LightBlock {
    pos_radius: [Vec4; GPU_LIGHTS],
    colour: [Vec4; GPU_LIGHTS],
}

/// `dst · (1 + src)` on colour, the destination alpha kept.
const LIGHT_BLEND: BlendState = BlendState {
    color: BlendComponent { src_factor: BlendFactor::Dst, dst_factor: BlendFactor::One, operation: BlendOperation::Add },
    alpha: BlendComponent { src_factor: BlendFactor::Zero, dst_factor: BlendFactor::One, operation: BlendOperation::Add },
};

struct LightPipes {
    layout: [BindGroupLayoutDescriptor; 2],
    shader: Handle<Shader>,
    /// (debug, depth multisampled, target samples, target format).
    pipes: Pipelines<(bool, bool, u32, TextureFormat)>,
}

impl LightPipes {
    fn new(assets: &AssetServer, fullscreen: &FullscreenShader) -> Self {
        let layout = |ms: bool| {
            BindGroupLayoutDescriptor::new(
                "particle lights",
                &BindGroupLayoutEntries::sequential(ShaderStages::FRAGMENT, (uniform_buffer::<FxView>(false), uniform_buffer::<LightBlock>(false), depth_entry(ms))),
            )
        };
        LightPipes { layout: [layout(false), layout(true)], shader: assets.load(SHADER_PATH), pipes: Pipelines::new(fullscreen) }
    }
}

/// The lights into the frame ([`DRAWS`] times: module docs), then with `RC_PARTICLE_LIGHTS_DEBUG` their spheres. The
/// pipelines are made on the first frame with a light (a `Local`: nothing exists in Original or Soft).
#[allow(clippy::too_many_arguments)]
fn light_pass(
    view: ViewQuery<(&ExtractedCamera, &ExtractedView, &ViewTarget, &ViewDepthTexture), With<ShadowCamera>>,
    g: Option<Res<GraphicsSettings>>,
    assets: Res<AssetServer>,
    fullscreen: Res<FullscreenShader>,
    cache: Res<PipelineCache>,
    mut pipes: Local<Option<LightPipes>>,
    mut ctx: RenderContext,
) {
    if !g.is_some_and(|g| g.particles.lights()) { return; }
    let frame = frame_of(&RENDER).clone();
    if frame.lights.is_empty() { return; }
    let pipes = pipes.get_or_insert_with(|| LightPipes::new(&assets, &fullscreen));
    let (cam, ev, target, depth) = view.into_inner();
    let Some(mut fx) = FxView::of(cam, ev) else { return };
    let count = frame.lights.len().min(GPU_LIGHTS);
    fx.params = Vec4::new(count as f32, frame.scale, 0.0, 0.0);
    let mut block = LightBlock { pos_radius: [Vec4::ZERO; GPU_LIGHTS], colour: [Vec4::ZERO; GPU_LIGHTS] };
    for (i, [p, c]) in frame.lights.iter().take(count).enumerate() {
        block.pos_radius[i] = *p;
        block.colour[i] = *c;
    }
    let (samples, format) = fx_view::target_of(target);
    let ms = depth.texture.sample_count() > 1;
    let make = |debug: bool| {
        let (layout, shader) = (pipes.layout[ms as usize].clone(), pipes.shader.clone());
        move || Fullscreen {
            label: if debug { "particle lights debug" } else { "particle lights" },
            layout,
            shader,
            entry: if debug { "debug" } else { "fragment" },
            defs: depth_defs(ms),
            format,
            blend: Some(if debug { BlendState::ALPHA_BLENDING } else { LIGHT_BLEND }),
            samples,
        }
    };
    let Some(light) = pipes.pipes.get(&cache, (false, ms, samples, format), make(false)) else { return };
    let debug = if frame.debug { pipes.pipes.get(&cache, (true, ms, samples, format), make(true)) } else { None };
    let device = ctx.render_device().clone();
    let fx_buf = fx.buffer(&device, "particle lights view");
    let light_buf = fx_view::uniform_buffer(&device, "particle lights", &block);
    let group = device.create_bind_group(
        "particle lights",
        &cache.get_bind_group_layout(&pipes.layout[ms as usize]),
        &BindGroupEntries::sequential((fx_buf.as_entire_binding(), light_buf.as_entire_binding(), depth.view())),
    );
    let mut pass = ctx.begin_tracked_render_pass(RenderPassDescriptor {
        label: Some("particle lights"),
        color_attachments: &[Some(target.get_color_attachment())],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    if let Some(v) = &cam.viewport { pass.set_camera_viewport(v); }
    pass.set_render_pipeline(light);
    pass.set_bind_group(0, &group, &[]);
    for _ in 0..DRAWS {
        pass.draw(0..3, 0..1);
    }
    if let Some(d) = debug {
        pass.set_render_pipeline(d);
        pass.draw(0..3, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn light(x: f32, r: f32, i: f32) -> Light { Light { pos: Vec3::new(x, 0.0, 0.0), radius: r, colour: Vec3::new(i, i * 0.5, 0.0) } }

    #[test]
    fn falloff_is_a_smooth_window() {
        assert_eq!(falloff(0.0, 4.0), 1.0);
        assert_eq!(falloff(4.0, 4.0), 0.0);
        assert_eq!(falloff(5.0, 4.0), 0.0);
        assert_eq!(falloff(1.0, 0.0), 0.0);
        let mut last = 1.0;
        for k in 1..=40 {
            let f = falloff(k as f32 * 0.1, 4.0);
            assert!(f < last && f >= 0.0);
            last = f;
        }
        // Zero slope at the edge: the last step is far smaller than the first ones in the middle.
        assert!(falloff(3.99, 4.0) < 1.0e-4);
    }

    #[test]
    fn sprite_colour_and_power() {
        // The explosions' orange spark 0x4f008fff: additive, peak 0xff = 2.0, alpha 0x4f, radius 0.95.
        let (c, p) = sprite_source([0xff, 0x8f, 0x00], 0x4f, 0.95, true).unwrap();
        assert!((c.x - 1.0).abs() < 1e-6 && c.y > 0.0 && c.y < 0.56 && c.z == 0.0, "{c:?}");
        assert!((p - (255.0 / 128.0) * (0x4f as f32 / 128.0) * 0.95).abs() < 1e-5, "{p}");
        // A normal-blend sprite counts only above 0x80 (the Pyrocitor's 0x1820c0c0 flame: half its colour).
        let (_, q) = sprite_source([0xc0, 0xc0, 0x20], 0x18, 0.5, false).unwrap();
        assert!((q - 1.5 * (0x18 as f32 / 128.0) * 0.5 * 0.5).abs() < 1e-5);
        assert!(sprite_source([0x60, 0x60, 0x60], 0x80, 1.0, false).is_none(), "grey smoke");
        assert!(sprite_source([0x34, 0x34, 0x34], 0xff, 0.7, true).is_none(), "the flyers' additive exhaust");
        let (_, half) = sprite_source([0x60, 0x30, 0x00], 0x80, 1.0, true).unwrap();
        assert!((half - 0.75 * 0.5).abs() < 1e-5, "half weight between 0x40 and 0x80: {half}");
        assert!(sprite_source([0xff, 0xff, 0xff], 0, 1.0, true).is_none(), "faded out");
        // The light: intensity saturating at PEAK, chroma kept.
        let l = to_light(&Source { pos: Vec3::ZERO, radius: 30.0, colour: c, power: 50.0 });
        assert!((l.intensity() - PEAK).abs() < 1e-3 && l.radius == MAX_RADIUS);
        let dim = to_light(&Source { pos: Vec3::ZERO, radius: 0.1, colour: c, power: 0.1 });
        assert!(dim.intensity() < 0.1 * PEAK && dim.radius == MIN_RADIUS);
    }

    #[test]
    fn clusters_merge_near_sources() {
        let s = |x: f32, p: f32| Source { pos: Vec3::new(x, 0.0, 0.0), radius: 2.0, colour: Vec3::X, power: p };
        let c = cluster(vec![s(0.0, 1.0), s(1.0, 3.0), s(10.0, 0.5)]);
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].power, 4.0);
        assert!((c[0].pos.x - 0.75).abs() < 1e-6, "power-weighted");
        assert!((c[0].radius - 3.0).abs() < 1e-6, "the far member's reach from the seed");
        assert_eq!(c[1].pos.x, 10.0);
    }

    #[test]
    fn selection_prefers_bright_and_near() {
        let eye = Vec3::ZERO;
        let near_dim = light(5.0, 4.0, 1.0);
        let far_bright = light(80.0, 4.0, 2.5);
        let near_bright = light(6.0, 4.0, 2.5);
        let s = select(vec![far_bright, near_dim, near_bright], eye, 2);
        assert_eq!(s, vec![near_bright, near_dim]);
        assert!(score(&light(2.0, 4.0, 1.0), eye) == 1.0, "inside its radius: its full intensity");
    }

    #[test]
    fn cap_fades_instead_of_popping() {
        let dt = 1.0 / 60.0;
        let mut t = Tracker::default();
        let a = light(0.0, 4.0, 2.0);
        let b = light(50.0, 4.0, 2.0);
        t.update(&[a], dt);
        assert_eq!(t.lights.len(), 1);
        let w0 = t.lights[0].weight;
        assert!(w0 > 0.0 && w0 < 1.0, "comes on over FADE_IN");
        for _ in 0..10 { t.update(&[a], dt); }
        assert_eq!(t.lights[0].weight, 1.0);
        // `a` leaves the cap for `b`: it fades out over FADE_OUT (never more than one step a frame), `b` comes on.
        let mut last = 1.0;
        let mut frames = 0;
        loop {
            t.update(&[b], dt);
            let wa = t.lights.iter().find(|l| l.light.pos.x == 0.0).map_or(0.0, |l| l.weight);
            assert!(last - wa <= dt / FADE_OUT + 1e-5, "a step of {}", last - wa);
            last = wa;
            frames += 1;
            if wa == 0.0 { break; }
            assert!(frames < 60);
        }
        assert!((frames as f32 - FADE_OUT / dt).abs() <= 1.0, "{frames} frames");
        assert_eq!(t.lights.len(), 1);
        assert_eq!(t.lights[0].weight, 1.0);
        // A moving effect keeps its light (matched within its half radius).
        t.update(&[light(51.0, 4.0, 1.0)], dt);
        assert_eq!((t.lights.len(), t.lights[0].weight, t.lights[0].light.pos.x), (1, 1.0, 51.0));
        // Never more than the shader takes.
        let many: Vec<Light> = (0..GPU_LIGHTS + 20).map(|k| light(k as f32 * 10.0, 2.0, 1.0)).collect();
        t.update(&many, dt);
        assert_eq!(t.lights.len(), GPU_LIGHTS);
    }
}
