//! **Port-only: Particles Enhanced** (crate::graphics): the bright effects light the opaque world around them.
//!
//! **Sources** (main world, every frame, read only: the game state, its RNG and the particle simulation are never
//! touched): the live records of the particle pool (crate::particle_render: kinds 0, 1 and the ribbons 3; the
//! additive ones, ALPHA 0x48, by their whole colour once it is bright (dim grey additive smoke emits nothing), the
//! normal ones (0x44, e.g. the Pyrocitor's flame
//! `rc_game::particles::type12`) only by the part of their colour above 0x80, MODULATE's overbright; never the
//! [`NON_EMISSIVE`] types, the weather, water, mist, smoke and dust, whose additive white is a veil, not a glow), the
//! explosion flashes (classes 0x70 / 1192, `rc_game::moby_update::classes::debris::flash_update`: ambient colour,
//! alpha, the growing bounding sphere) and the Bomb Glove's fireballs (122, `classes::bomb::fireball_update`, a fire
//! colour tinted by the ambient). Each gives a colour, a power (peak channel × alpha × size, [`sprite_source`]) and a
//! reach (a few times its size); a light's intensity saturates with its power ([`to_light`]).
//!
//! **Lights from frame to frame** ([`Tracker`]): a light lives on while sources keep coming within its reach: each
//! source joins the nearest tracked light that reaches it, the rest merge into new ones ([`cluster`]: sources within
//! [`MERGE`] of a brighter one: an explosion's sparks, flashes and fireballs make one light). A tracked light follows
//! its sources' power-weighted centre and colour through exponential filters ([`TAU_POS`], [`TAU_RISE`],
//! [`TAU_FALL`]): a fountain of random sparks or a fire whose alpha the game redraws every tick (Orxon's spark
//! fountains 1240, `units::orxon_sparks`) glows steadily instead of flickering, and a light whose sources end fades
//! with [`TAU_FALL`]. The lights shown (`RC_PARTICLE_LIGHTS_MAX`, default [`DEFAULT_MAX`]) are those with the highest
//! `intensity · r² / max(d², r²)` from the camera ([`score`]), the ones already shown counted [`HOLD`] times (so two
//! lights near the cap do not swap every frame); one fades in over [`FADE_IN`] when it enters the cap and out over
//! [`FADE_OUT`] when it leaves it.
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
//! `RC_PARTICLE_LIGHTS_DEBUG=2` also prints the lights once a second, each with its strongest source: `p<type>` a
//! particle type, `m<class>` a flash or fireball).

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
/// Seconds a light takes to come on when it enters the cap, and to go off when it leaves it.
pub const FADE_IN: f32 = 0.1;
pub const FADE_OUT: f32 = 0.15;
/// Time constants (seconds) of a tracked light's filters: its intensity rising and falling (an effect's onset stays
/// quick, its tick-to-tick noise and its end are slow), its centre and radius.
pub const TAU_RISE: f32 = 0.1;
pub const TAU_FALL: f32 = 0.4;
pub const TAU_POS: f32 = 0.05;
/// A shown light's score is counted this many times against the others' (the cap's hysteresis).
pub const HOLD: f32 = 1.5;
/// A tracked light whose filtered intensity falls below this is dropped.
const MIN_INTENSITY: f32 = 0.005;
/// Particle update types (record byte 0; the update table is the same code on every level, docs/plan/particles.md) that
/// never light anything: the weather's rain and flakes (0, 73), the rain and water splashes, drops, rings and bubbles
/// (1, 34, 35, 45, 46, 66), Novalis's waterfall mist and foam rings (56, 57, ripple manager 751), the smoke, steam and
/// dust puffs and trails (13, 16, 18, 22, 43, 44, 47, 51, 74, 77), the goo drips (52) and the flat decals (70).
pub const NON_EMISSIVE: [u8; 22] = [0, 1, 13, 16, 18, 22, 34, 35, 43, 44, 45, 46, 47, 51, 52, 56, 57, 66, 70, 73, 74, 77];
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

/// One effect's contribution before merging: position (Bevy world), reach, linear chroma (peak channel 1), power, and
/// what it is ([`Tag`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Source {
    pub pos: Vec3,
    pub radius: f32,
    pub colour: Vec3,
    pub power: f32,
    pub tag: Tag,
}

/// What a source is, for the debug print: a particle's update type or a moby's class.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tag {
    #[default]
    None,
    Particle(u8),
    Moby(i16),
}

impl std::fmt::Display for Tag {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Tag::None => write!(f, "-"),
            Tag::Particle(t) => write!(f, "p{t}"),
            Tag::Moby(c) => write!(f, "m{c}"),
        }
    }
}

/// A light: position (Bevy world), radius, linear colour × intensity, its strongest source's [`Tag`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Light {
    pub pos: Vec3,
    pub radius: f32,
    pub colour: Vec3,
    pub tag: Tag,
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

/// Sources summed into one: power, Σ power·position, Σ power·colour, the reach from the seed, the strongest member.
#[derive(Clone, Copy, Debug, Default)]
struct Acc {
    power: f32,
    pos: Vec3,
    colour: Vec3,
    radius: f32,
    top: f32,
    tag: Tag,
}

impl Acc {
    fn add(&mut self, s: &Source, seed: Vec3) {
        self.power += s.power;
        self.pos += s.pos * s.power;
        self.colour += s.colour * s.power;
        self.radius = self.radius.max(s.radius + seed.distance(s.pos)).min(MAX_RADIUS);
        if s.power > self.top { (self.top, self.tag) = (s.power, s.tag); }
    }

    fn source(&self) -> Option<Source> {
        (self.power > 0.0).then(|| Source { pos: self.pos / self.power, radius: self.radius, colour: self.colour / self.power, power: self.power, tag: self.tag })
    }
}

/// Merges every source into the first brighter cluster whose seed is within [`MERGE`]: power summed, position and
/// colour power-weighted, the reach the farthest of its members'.
pub fn cluster(mut sources: Vec<Source>) -> Vec<Source> {
    sources.sort_by(|a, b| b.power.total_cmp(&a.power));
    let mut out: Vec<(Vec3, Acc)> = Vec::new();
    for s in sources {
        if let Some(c) = out.iter_mut().find(|c| c.0.distance_squared(s.pos) < MERGE * MERGE) {
            c.1.add(&s, c.0);
        } else if out.len() < MAX_CLUSTERS {
            let mut a = Acc::default();
            a.add(&s, s.pos);
            out.push((s.pos, a));
        }
    }
    out.iter().filter_map(|c| c.1.source()).collect()
}

/// A merged source's light: intensity `PEAK·(1 − e^(−power))`, the chroma scaled to it.
pub fn to_light(s: &Source) -> Light {
    let i = PEAK * (1.0 - (-s.power / POWER_REF).exp());
    let peak = s.colour.max_element();
    let chroma = if peak > 0.0 { s.colour / peak } else { Vec3::ZERO };
    Light { pos: s.pos, radius: s.radius.clamp(MIN_RADIUS, MAX_RADIUS), colour: chroma * i, tag: s.tag }
}

/// How much a light matters to the view from `eye`: its intensity, by its size over the distance (full inside it).
pub fn score(l: &Light, eye: Vec3) -> f32 {
    let r2 = l.radius * l.radius;
    l.intensity() * r2 / l.pos.distance_squared(eye).max(r2)
}

/// The distance within which a tracked light takes a source: three quarters of its radius, at least [`MERGE`] (a
/// glow's short-lived blobs anywhere over Orxon's grates keep one light).
fn reach(l: &Light) -> f32 { (l.radius * 0.75).max(MERGE) }

/// The filter step `1 − e^(−dt/τ)`.
fn follow(dt: f32, tau: f32) -> f32 { if tau > 0.0 { 1.0 - (-dt / tau).exp() } else { 1.0 } }

/// A light the shader draws: its filtered values and how far it is on (0..1, the cap's fade).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tracked {
    pub light: Light,
    pub weight: f32,
    /// Shown this frame (within the cap).
    pub live: bool,
}

/// The lights from frame to frame (module docs, "Lights from frame to frame").
#[derive(Clone, Debug, Default)]
pub struct Tracker {
    pub lights: Vec<Tracked>,
}

impl Tracker {
    /// One frame of `dt` seconds: every source to the nearest tracked light within its [`reach`], the rest
    /// [`cluster`]ed into new candidates; each tracked light's filters stepped toward its sources' light (zero
    /// intensity without sources); then the `max` best [`score`]s from `eye` (tracked lights already shown × [`HOLD`])
    /// are shown, a new one starting at its sources' values and one step of [`FADE_IN`]. Returns how many are shown.
    /// At most [`GPU_LIGHTS`] are kept (the dimmest hidden ones go first).
    pub fn update(&mut self, sources: Vec<Source>, eye: Vec3, max: usize, dt: f32) -> usize {
        let mut acc = vec![Acc::default(); self.lights.len()];
        let mut free = Vec::new();
        for s in sources {
            let near = self
                .lights
                .iter()
                .enumerate()
                .map(|(i, t)| (i, t.light.pos.distance(s.pos)))
                .filter(|&(i, d)| d < reach(&self.lights[i].light))
                .min_by(|a, b| a.1.total_cmp(&b.1));
            match near {
                Some((i, _)) => acc[i].add(&s, self.lights[i].light.pos),
                None => free.push(s),
            }
        }
        let (k_rise, k_fall, k_pos) = (follow(dt, TAU_RISE), follow(dt, TAU_FALL), follow(dt, TAU_POS));
        for (t, a) in self.lights.iter_mut().zip(&acc) {
            let l = &mut t.light;
            match a.source() {
                Some(s) => {
                    let want = to_light(&s);
                    let k = if want.intensity() > l.intensity() { k_rise } else { k_fall };
                    l.colour += (want.colour - l.colour) * k;
                    l.pos += (want.pos - l.pos) * k_pos;
                    l.radius += (want.radius - l.radius) * k_pos;
                    l.tag = want.tag;
                }
                None => l.colour -= l.colour * k_fall,
            }
        }
        self.lights.retain(|t| t.light.intensity() >= MIN_INTENSITY);
        let fresh: Vec<Light> = cluster(free).iter().map(to_light).collect();
        // (score, index: a tracked light, or `lights.len() + k` the k-th fresh one)
        let n = self.lights.len();
        let mut ranked: Vec<(f32, usize)> = self
            .lights
            .iter()
            .enumerate()
            .map(|(i, t)| (score(&t.light, eye) * if t.live { HOLD } else { 1.0 }, i))
            .chain(fresh.iter().enumerate().map(|(k, l)| (score(l, eye), n + k)))
            .collect();
        ranked.sort_by(|a, b| b.0.total_cmp(&a.0));
        ranked.truncate(max);
        for t in &mut self.lights { t.live = false; }
        let step_in = if FADE_IN > 0.0 { dt / FADE_IN } else { 1.0 };
        for &(_, i) in &ranked {
            match i.checked_sub(n) {
                None => self.lights[i].live = true,
                Some(k) => self.lights.push(Tracked { light: fresh[k], weight: 0.0, live: true }),
            }
        }
        let step_out = dt / FADE_OUT;
        for t in &mut self.lights {
            t.weight = if t.live { (t.weight + step_in).min(1.0) } else { t.weight - step_out };
        }
        self.lights.retain(|t| t.weight > 0.0);
        if self.lights.len() > GPU_LIGHTS {
            self.lights.sort_by(|a, b| b.live.cmp(&a.live).then((b.weight * b.light.intensity()).total_cmp(&(a.weight * a.light.intensity()))));
            self.lights.truncate(GPU_LIGHTS);
        }
        ranked.len()
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
        if r[1] & 0x80 != 0 || NON_EMISSIVE.contains(&r[0]) { continue; }
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
        out.push(Source { pos, radius: (rad * SPRITE_REACH).clamp(MIN_RADIUS, MAX_RADIUS), colour, power, tag: Tag::Particle(r[0]) });
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
        out.push(Source { pos, radius: (rad * MOBY_REACH).clamp(MIN_RADIUS, MAX_RADIUS), colour, power: power * FLASH_GAIN, tag: Tag::Moby(m.o_class) });
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
    let max = st.max;
    let shown = st.tracker.update(sources, eye, max, time.delta_secs());
    let now = time.elapsed_secs_f64();
    if st.debug >= 2 && now - st.printed >= 1.0 {
        st.printed = now;
        let v: Vec<String> = st.tracker.lights.iter().map(|t| format!("{} ({:.1} {:.1} {:.1}) r {:.1} i {:.2} w {:.2}", t.light.tag, t.light.pos.x, t.light.pos.y, t.light.pos.z, t.light.radius, t.light.intensity(), t.weight)).collect();
        let mut types: Vec<(usize, f32)> = by_type.iter().copied().enumerate().filter(|t| t.1 > 0.0).collect();
        types.sort_by(|a, b| b.1.total_cmp(&a.1));
        let t: Vec<String> = types.iter().take(8).map(|(k, p)| format!("{k}:{p:.1}")).collect();
        println!("particle lights: {shown} shown, {} drawn, power by type [{}]: {}", v.len(), t.join(" "), v.join("; "));
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
        let l = to_light(&Source { pos: Vec3::ZERO, radius: 30.0, colour: c, power: 50.0, tag: Tag::None });
        assert!((l.intensity() - PEAK).abs() < 1e-3 && l.radius == MAX_RADIUS);
        let dim = to_light(&Source { pos: Vec3::ZERO, radius: 0.1, colour: c, power: 0.1, tag: Tag::None });
        assert!(dim.intensity() < 0.1 * PEAK && dim.radius == MIN_RADIUS);
    }

    #[test]
    fn veils_are_not_lights() {
        // Novalis's waterfall: the mist puffs 56 and foam rings 57 (additive white); the smoke, dust, rain and splashes.
        for t in [56, 57, 0, 1, 16, 22, 44, 45, 47, 73] { assert!(NON_EMISSIVE.contains(&t), "type {t}"); }
        // The effects that glow: explosions, sparks, flashes, fire, the emitters (6), Orxon's spark fountains (64).
        for t in [4, 5, 6, 8, 11, 12, 14, 15, 21, 25, 27, 53, 64] { assert!(!NON_EMISSIVE.contains(&t), "type {t}"); }
    }

    fn src(x: f32, p: f32) -> Source { Source { pos: Vec3::new(x, 0.0, 0.0), radius: 2.0, colour: Vec3::X, power: p, tag: Tag::Particle(4) } }

    #[test]
    fn clusters_merge_near_sources() {
        let c = cluster(vec![src(0.0, 1.0), src(1.0, 3.0), src(10.0, 0.5)]);
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].power, 4.0);
        assert!((c[0].pos.x - 0.75).abs() < 1e-6, "power-weighted");
        assert!((c[0].radius - 3.0).abs() < 1e-6, "the far member's reach from the seed");
        assert_eq!(c[1].pos.x, 10.0);
    }

    fn shown(t: &Tracker) -> Vec<f32> {
        let mut v: Vec<f32> = t.lights.iter().filter(|l| l.live).map(|l| l.light.pos.x).collect();
        v.sort_by(f32::total_cmp);
        v
    }

    #[test]
    fn selection_prefers_bright_and_near() {
        let eye = Vec3::ZERO;
        let mut t = Tracker::default();
        // Near and dim, far and bright, near and bright: the cap of 2 takes the near ones.
        let n = t.update(vec![src(5.0, 1.0), src(80.0, 8.0), src(20.0, 8.0)], eye, 2, 1.0 / 60.0);
        assert_eq!((n, shown(&t)), (2, vec![5.0, 20.0]));
        let l = Light { pos: Vec3::new(2.0, 0.0, 0.0), radius: 4.0, colour: Vec3::ONE, tag: Tag::None };
        assert!(score(&l, eye) == 1.0, "inside its radius: its full intensity");
    }

    #[test]
    fn noisy_sources_glow_steadily() {
        // A fountain of random sparks (Orxon's 1240): every frame a different handful within a unit of x = 0, each of
        // a random power; the light keeps one identity and its intensity moves far less than the raw one.
        let dt = 1.0 / 60.0;
        let mut t = Tracker::default();
        let mut seed = 0x1234_5678u32;
        let mut rand = move || { seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223); (seed >> 8) as f32 / (1u32 << 24) as f32 };
        let (mut raw, mut out) = (Vec::new(), Vec::new());
        for frame in 0..240 {
            let n = 1 + (rand() * 6.0) as usize;
            let sources: Vec<Source> = (0..n).map(|_| src(rand() * 2.0 - 1.0, 0.2 + rand() * 1.5)).collect();
            raw.push(to_light(&cluster(sources.clone())[0]).intensity());
            t.update(sources, Vec3::new(0.0, 0.0, 20.0), 8, dt);
            if frame >= 60 {
                assert_eq!(t.lights.len(), 1, "frame {frame}: one light");
                out.push(t.lights[0].light.intensity() * t.lights[0].weight);
            }
        }
        let step = |v: &[f32]| v.windows(2).map(|w| (w[1] - w[0]).abs()).sum::<f32>() / (v.len() - 1) as f32;
        let (s_raw, s_out) = (step(&raw[60..]), step(&out));
        assert!(s_out < 0.2 * s_raw, "frame-to-frame change {s_out} against the raw {s_raw}");
        assert!(out.iter().all(|&i| i > 0.3), "never drops out");
    }

    #[test]
    fn onset_is_quick_and_the_end_fades() {
        let dt = 1.0 / 60.0;
        let mut t = Tracker::default();
        t.update(vec![src(0.0, 20.0)], Vec3::ZERO, 8, dt);
        let first = t.lights[0].light.intensity();
        assert!(first > 0.9 * PEAK, "a new light starts at its sources' values: {first}");
        assert!(t.lights[0].weight > 0.0 && t.lights[0].weight < 1.0, "and comes on over FADE_IN");
        for _ in 0..(FADE_IN / dt).ceil() as usize { t.update(vec![src(0.0, 20.0)], Vec3::ZERO, 8, dt); }
        assert_eq!(t.lights[0].weight, 1.0);
        // The effect ends: the light falls with TAU_FALL, never more than one filter step a frame.
        let mut last = t.lights[0].light.intensity();
        let mut frames = 0;
        while let Some(l) = t.lights.first() {
            let i = l.light.intensity();
            assert!(last - i <= last * follow(dt, TAU_FALL) + 1e-5, "a step of {}", last - i);
            last = i;
            t.update(Vec::new(), Vec3::ZERO, 8, dt);
            frames += 1;
            assert!(frames < 600);
        }
        let expect = TAU_FALL * (PEAK / MIN_INTENSITY).ln() / dt;
        assert!((frames as f32 - expect).abs() < 0.05 * expect, "{frames} frames, {expect}");
    }

    #[test]
    fn cap_holds_and_fades_instead_of_popping() {
        let dt = 1.0 / 60.0;
        let eye = Vec3::ZERO;
        let mut t = Tracker::default();
        // Cap 1: `a` is shown; then `b`, which scores a little higher, does not take its place (HOLD).
        for _ in 0..10 { t.update(vec![src(5.0, 2.0)], eye, 1, dt); }
        let a_score = score(&to_light(&src(5.0, 2.0)), eye);
        let b_score = score(&to_light(&src(-5.5, 3.0)), eye);
        assert!(b_score > a_score && b_score < HOLD * a_score);
        for _ in 0..10 { t.update(vec![src(5.0, 2.0), src(-5.5, 3.0)], eye, 1, dt); }
        assert_eq!(shown(&t), vec![5.0]);
        let a = |t: &Tracker| t.lights.iter().find(|l| l.light.pos.x.round() == 5.0).map_or(0.0, |l| l.weight);
        assert_eq!(a(&t), 1.0);
        // `b` grows far brighter: it takes the cap, `a` fades out over FADE_OUT (one step a frame).
        let mut last = 1.0;
        let mut frames = 0;
        loop {
            t.update(vec![src(5.0, 2.0), src(-5.5, 40.0)], eye, 1, dt);
            let w = a(&t);
            assert!(last - w <= dt / FADE_OUT + 1e-5, "a step of {}", last - w);
            last = w;
            frames += 1;
            if w <= 0.0 { break; }
            assert!(frames < 60);
        }
        assert!((frames as f32 - FADE_OUT / dt).abs() <= 1.0, "{frames} frames");
        assert_eq!(shown(&t).len(), 1);
        // A moving effect keeps its light (its sources within its reach).
        let x0 = shown(&t)[0];
        for k in 1..=60 { t.update(vec![src(-5.5 - k as f32 * 0.5, 40.0)], eye, 1, dt); }
        assert_eq!(t.lights.len(), 1);
        assert!(shown(&t)[0] < x0 - 25.0, "followed it: {:?}", shown(&t));
        // Never more than the shader takes.
        let many: Vec<Source> = (0..GPU_LIGHTS as i32 + 20).map(|k| src(k as f32 * 10.0, 1.0)).collect();
        t.update(many, eye, MAX_CAP, dt);
        assert!(t.lights.len() <= GPU_LIGHTS);
    }
}

