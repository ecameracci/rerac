//! **Port-only: the volumetric light of Lighting Enhanced** (gated on crate::graphics
//! `GraphicsSettings::lighting.enhanced()`): shafts of sunlight in the air, cut by whatever shadows the sun, also with
//! the sun off screen (crate::sun_rays needs it on screen).
//!
//! Standard in-scattering through the sun's shadow map (`assets/shaders/volumetric_light.wgsl`), after the 3D passes
//! and before crate::sun_rays, the screen tints and the anti-aliasing blit:
//! 1. **March** (half resolution): each pixel's view ray from the camera to the depth-buffer surface or the open sea
//!    ([`SeaLevel`]; none from under it), at most
//!    [`LENGTH`] units ([`SKY_LENGTH`] toward the sky, so it does not wash out), in [`STEPS`] steps packed toward the
//!    camera ([`step`]) from a per-pixel dithered start that moves every frame ([`jitter`]: interleaved gradient
//!    noise plus the golden ratio per frame, so no fixed screen pattern survives step 2). Each step looks the sun's
//!    cascades up (crate::sun_shadow `SunCascades`, one comparison tap; lit where the occluder is Ratchet,
//!    [`VolumetricLight::hero`]): a lit step adds `σ·T·dt` of a uniform thin medium ([`DENSITY`] σ, extinction
//!    `T = exp(−σt)`), weighted by the shadow around it ([`Shafts`]: in full
//!    within [`EDGE`] of a long shadow along the ray, [`OPEN`] in lit air far from one, so a shaded space with a lit
//!    opening gets bold beams and open daylight stays clear), and the sum by the Henyey-Greenstein phase ([`phase`],
//!    [`G`] blended [`ISO`] with an isotropic phase, [`phase_mix`]: forward scattering, strongest looking toward the
//!    light, without the blown-out peak). Into an RGBA16F target, the sample's view distance in alpha.
//! 2. **Temporal** (half resolution): the march blended ([`BLEND`]) into the last frame's result reprojected by the
//!    sample's surface ([`VolHistory`], two history targets in turn), each history texel kept only while it saw the
//!    same surface ([`same_surface`]) and clamped to the march's 3×3 neighbourhood; on a cut ([`CUT`], a resize, the
//!    shafts just turned on) or a disocclusion, the depth-weighted 3×3 mean. It runs per rendered frame from the
//!    extracted views, so interpolated frames (frame smoothing) reproject as any other.
//! 3. **Composite** (full resolution): a 4×4 depth-aware (bilateral) blur of the accumulated target, upsampled, less
//!    [`FLATTEN`] of the light it shares with its surroundings ([`flatten`]: the depth-weighted mean of a disc of
//!    [`FLAT_RADIUS`] of the target's height), added to the frame: light that is the same all around (lit air seen
//!    from a shaded spot, a level's whole distance) is a grey veil, not a shaft, so only what stands out from its
//!    surroundings (beams through an opening, cut by its edges) shows in full, under a soft maximum ([`soft_cap`],
//!    [`KNEE`]). The scene itself is not dimmed by the medium (the game's fog already does that).
//!
//! The colour is the key light's ([`VolumetricLight::tint`]: light set 0's light A, `ShadowGame::sun_color`) pulled
//! [`FOG_MIX`] of the way toward the frame's fog colour (crate::game_camera `GameFog`), so it sits in the level's
//! palette; its strength follows that light's brightness ([`strength`]: levels without a real sun get little or none)
//! and fades out as the light nears the horizon. The shafts follow the shadow light (under Enhanced the key light's
//! own elevation, at least 30°: crate::sun_shadow `min_elevation`), so they match the shadows on the ground.
//! Nothing runs without the sun's cascades (`RC_SHADOWS=0`), under Lighting Original or Simple, or where
//! crate::graphics [`LightShafts`] turns the shafts off (its per-level table, the space scenes).
//!
//! Tuned on Kerwan's first tunnel (`RC_LEVEL=3 RC_HERO_AT=264,116,54.5,1.5708`; its 35° key light does not reach
//! inside, so the shafts are in the opening, cut by the arch and the vines; with `RC_SUN_DIR=0.4,-0.5,0.75` bold beams
//! cross the tunnel's air), Kerwan's spawn (sunlit floor, shadowed walls: clear) and Novalis' spawn (open daylight).
//!
//! The flattening was tuned on Kerwan's spawn, Rilgar's bay and Hoven's start (the camera in shade under a lit
//! level: without it a grey veil over the whole distance), keeping the beams of Kerwan's first tunnel.
//!
//! Dev switches: `RC_VOLUMETRIC=0` off; a number scales the strength (`2`: twice as bright).
//! `RC_VOLUMETRIC_TUNE=steps,density,g,length,sky,open,edge,shade,after_lit,flatten,flat_radius,iso,knee,blend` overrides the
//! constants (an empty field keeps one); `RC_VOLUMETRIC_DEBUG=<gain>` shows the shafts alone, times the gain.

use crate::fx_view::{depth_defs, depth_entry, Fullscreen, FxView, Pipelines};
use crate::game_camera::GameFog;
use crate::gameplay::Play;
use crate::graphics::{GraphicsSettings, LightShafts};
use crate::shadow_render::{ShadowCamera, ShadowGame};
use crate::sun_shadow::{KeyLight, SunCascades};
use crate::tfrag_render::game_to_bevy;
use bevy::core_pipeline::{Core3d, Core3dSystems, FullscreenShader};
use bevy::pbr::{ExtractedDirectionalLight, ViewShadowBindings};
use bevy::prelude::*;
use bevy::render::camera::ExtractedCamera;
use bevy::render::extract_resource::{ExtractResource, ExtractResourcePlugin};
use bevy::render::render_resource::binding_types::{sampler, texture_2d, texture_2d_array, uniform_buffer};
use bevy::render::render_resource::{
    BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, CompareFunction, Extent3d, FilterMode, LoadOp,
    Operations, PipelineCache, RenderPassColorAttachment, RenderPassDescriptor, Sampler, SamplerBindingType, SamplerDescriptor,
    ShaderStages, ShaderType, StoreOp, TextureDescriptor, TextureDimension, TextureFormat, TextureSampleType, TextureUsages,
};
use bevy::render::renderer::{RenderContext, RenderDevice, ViewQuery};
use bevy::render::texture::{CachedTexture, TextureCache};
use bevy::render::view::{ExtractedView, ViewDepthTexture, ViewTarget};
use bevy::render::{Render, RenderApp, RenderStartup, RenderSystems};
use bevy::shader::Shader;
use std::f32::consts::PI;
use std::sync::OnceLock;

const SHADER_PATH: &str = "shaders/volumetric_light.wgsl";
const SCATTER_FORMAT: TextureFormat = TextureFormat::Rgba16Float;
/// Steps along each ray.
const STEPS: u32 = 24;
/// The longest ray (world units), well inside the shadow range (crate::sun_shadow `MAX_DISTANCE`).
const LENGTH: f32 = 48.0;
/// The ray toward the sky (no surface).
const SKY_LENGTH: f32 = 12.0;
/// The medium's scattering (and extinction) coefficient σ, per world unit.
const DENSITY: f32 = 0.02;
/// The Henyey-Greenstein asymmetry: > 0 scatters forward (mild, so the shafts also show seen from the side).
const G: f32 = 0.3;
/// The in-scattered light's brightness at full strength.
const INTENSITY: f32 = 2.0;
/// [`Shafts`]' weight of lit air far from shadow; the distance (world units) over which it rises to 1 near one; the
/// length along the ray (world units) from which a shadow counts in full; the weight of a shadow behind lit air while
/// the camera is in sunlight.
const OPEN: f32 = 0.05;
const EDGE: f32 = 4.0;
const SHADE: f32 = 4.0;
const AFTER_LIT: f32 = 0.25;
/// [`flatten`]'s share of the light common to the surroundings taken out, and the surroundings' radius (a fraction
/// of the march target's height).
const FLATTEN: f32 = 0.9;
const FLAT_RADIUS: f32 = 0.06;
/// How far the key light's hue is pulled toward the fog colour.
const FOG_MIX: f32 = 0.35;
/// The key light's luminance (1.0 = 128 on the colour byte) from which the shafts start, and at which they are full.
const DIM: f32 = 0.15;
const BRIGHT: f32 = 0.6;
/// The light where no game runs (the title world): white, at full brightness.
const DEFAULT_COLOR: Vec3 = Vec3::ONE;
/// The share of [`phase_mix`] that is isotropic: at full Henyey-Greenstein the forward peak blew out the view toward
/// the light (Gemlik).
const ISO: f32 = 0.5;
/// [`soft_cap`]'s knee: the most luminance the shafts add, approached smoothly.
const KNEE: f32 = 0.25;
/// This frame's weight in the temporal accumulation (about `1 / BLEND` frames averaged).
const BLEND: f32 = 0.1;
/// A camera move in one frame (world units) past which the history is dropped (a cut).
const CUT: f32 = 4.0;
/// The golden ratio's fraction: [`jitter`]'s per-frame step.
const GOLDEN: f32 = 0.618_034;

/// The shafts' colour and strength this frame (0: nothing runs).
#[derive(Resource, Clone, Copy, Default, PartialEq, ExtractResource)]
pub struct VolumetricLight {
    /// Linear RGB, brightest channel 1.
    pub tint: Vec3,
    pub strength: f32,
    /// Ratchet's bounding sphere (world centre, radius; 0: none): his shadow does not shade the air, which looking
    /// down along the sun read as a beam from his body (its long shadow column bounds the lit air beside it,
    /// [`Shafts`]; Gaspar marker 3 of session 2026-10-30_15-54-38).
    pub hero: Vec4,
}

/// The open sea's surface (Bevy y; crate::sea_render: the ocean 1111's sea z while its callback is registered). The
/// ocean writes no depth, so without it the march would run on under the surface to the bed and lay the shadows of
/// the islands above as dark jagged edges in the water (G-REN-162): the rays stop at it.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, ExtractResource)]
pub struct SeaLevel(pub Option<f32>);

/// The view's half-resolution targets this frame (only while the shafts show): the march, and the two history
/// textures the temporal pass ping-pongs (`history[cur]` written from `history[1 − cur]`).
#[derive(Component)]
struct ScatterTarget {
    march: CachedTexture,
    history: [CachedTexture; 2],
    cur: usize,
    frame: u32,
    /// The last frame's view, while its history may be reprojected ([`history_valid`]).
    prev: Option<PrevView>,
}

/// A view as the history saw it.
#[derive(Clone, Copy)]
struct PrevView {
    clip_from_world: Mat4,
    eye: Vec3,
    size: UVec2,
}

/// The render world's history bookkeeping: the last frame's view (None while the shafts are off) and a frame count.
#[derive(Resource, Default)]
struct VolHistory {
    prev: Option<PrevView>,
    frame: u32,
}

pub struct VolumetricLightPlugin;

impl Plugin for VolumetricLightPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<VolumetricLight>()
            .init_resource::<SeaLevel>()
            .add_plugins((ExtractResourcePlugin::<VolumetricLight>::default(), ExtractResourcePlugin::<SeaLevel>::default()))
            .add_systems(Last, update);
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else { return };
        render_app
            .init_resource::<VolHistory>()
            .add_systems(RenderStartup, init_pipelines)
            .add_systems(Render, prepare.in_set(RenderSystems::PrepareResources))
            .add_systems(Core3d, volumetric_pass.in_set(Core3dSystems::PostProcess).before(crate::sun_rays::SunRaysSet));
    }
}

/// Henyey-Greenstein phase function (per steradian: integrates to 1 over the sphere) of the angle between the
/// view ray and the direction toward the light. Mirrored in the shader.
#[cfg_attr(not(test), allow(dead_code))]
pub fn phase(cos_theta: f32, g: f32) -> f32 {
    let d = (1.0 + g * g - 2.0 * g * cos_theta).max(1e-4);
    (1.0 - g * g) / (4.0 * PI * d * d.sqrt())
}

/// [`phase`] blended with the isotropic `1/4π` by `iso` (0: Henyey-Greenstein alone): still integrates to 1, a lower
/// forward peak. Mirrored in the shader.
#[cfg_attr(not(test), allow(dead_code))]
pub fn phase_mix(cos_theta: f32, g: f32, iso: f32) -> f32 { iso / (4.0 * PI) + (1.0 - iso) * phase(cos_theta, g) }

/// The added light under a soft maximum: `l / (1 + Y(l) / knee)` (`Y` the luminance), so the shafts never add more
/// than `knee` and faint ones pass nearly unchanged. Mirrored in the shader's `composite`.
#[cfg_attr(not(test), allow(dead_code))]
pub fn soft_cap(light: Vec3, knee: f32) -> Vec3 { light / (1.0 + light.dot(LUMA) / knee.max(1e-4)) }

const LUMA: Vec3 = Vec3::new(0.2126, 0.7152, 0.0722);

/// A pixel's march start offset on `frame`: its fixed interleaved-gradient value `base` (0..1) moved on by the golden
/// ratio each frame, so every pixel runs through well-spread offsets the temporal pass averages (no fixed pattern).
/// Mirrored in the shader's `march`.
#[cfg_attr(not(test), allow(dead_code))]
pub fn jitter(base: f32, frame: u32) -> f32 { (base + GOLDEN * (frame % 4096) as f32).fract() }

/// Whether last frame's history (`prev`) can be reprojected into a view of `size` with its eye at `eye`: the same
/// target size and no cut.
fn history_valid(prev: Option<&PrevView>, eye: Vec3, size: UVec2) -> bool {
    prev.is_some_and(|p| p.size == size && p.eye.distance(eye) < CUT)
}

/// Whether a history texel that stored the view distance `stored` shows the surface now `expected` away from the
/// previous eye (else it is a different surface: disocclusion, a moving object). Mirrored in the shader's `temporal`.
#[cfg_attr(not(test), allow(dead_code))]
pub fn same_surface(stored: f32, expected: f32) -> bool { (stored - expected).abs() <= 0.05 * expected + 0.25 }

/// Step `i` of `n` along a ray of `length`, its start dithered by `jitter` (0..1): (distance, length of the step).
/// `t = length·u²` with `u = (i + jitter) / n` packs the steps toward the camera, where shafts are thin and near;
/// the step lengths sum to `length` at `jitter` 0.5. Mirrored in the shader.
#[cfg_attr(not(test), allow(dead_code))]
pub fn step(i: u32, n: u32, jitter: f32, length: f32) -> (f32, f32) {
    let u = (i as f32 + jitter) / n as f32;
    (length * u * u, length * 2.0 * u / n as f32)
}

/// A pixel's in-scattered `light` less `share` of what it has in common with the mean of its surroundings `around`
/// (per channel): a uniform veil keeps `1 − share` of itself, a beam brighter than around it its excess in full.
/// Mirrored in the shader's `composite`.
#[cfg_attr(not(test), allow(dead_code))]
pub fn flatten(light: Vec3, around: Vec3, share: f32) -> Vec3 { light - share * light.min(around) }

/// [`Shafts`]' shape: see there.
#[derive(Clone, Copy)]
#[cfg_attr(not(test), allow(dead_code))]
pub struct ShaftShape {
    pub open: f32,
    pub edge: f32,
    pub shade: f32,
    pub after_lit: f32,
}

impl Default for ShaftShape {
    fn default() -> Self { ShaftShape { open: OPEN, edge: EDGE, shade: SHADE, after_lit: AFTER_LIT } }
}

/// A lit run along the ray: its start, its last step, `Σ s`, `Σ s·exp(−d/edge)`, `Σ s·exp(d/edge)` (`d` from its
/// start), the weight of the shadow before it.
#[derive(Clone, Copy)]
#[cfg_attr(not(test), allow(dead_code))]
struct Run(f32, f32, f32, f32, f32, f32);

/// One ray's in-scattering, weighted by the shadow around each lit step (mirrored in the shader's `march`). A step
/// weighs `open` in lit air far from shadow (open daylight, the sky), rising to 1 within about `edge` units along
/// the ray of a shadow bounding its lit run: `1 − (1 − wb·a)(1 − wa·b)` with `a`, `b` = `exp(−d/edge)` to the run's
/// start and end. A bounding shadow weighs by its length along the ray (`smoothstep(shade/4, shade, length)`: a
/// character's thin shadow does not light up the air around it), the one after a run (lit air in front of a shadowed
/// wall) also by `after_lit` while the camera is in sunlight, so open daylight stays clear and a shaded space (the
/// ray starting in a long shadow) gets bold shafts at its lit opening and across its beams. Steps less than half lit
/// (penumbra) count in full. A run's exponentials are summed separably (`a·b` is its constant
/// `exp(−length/edge)`); a run ended by a shadow waits for that shadow's length.
#[derive(Default)]
#[cfg_attr(not(test), allow(dead_code))]
pub struct Shafts {
    shape: ShaftShape,
    total: f32,
    /// The camera's shade (the weight of a shadow the ray starts in), once known.
    cam: Option<f32>,
    /// The current shadow's length, while the ray is in one.
    shadow: Option<f32>,
    /// The weight of the last shadow (for the next run).
    before: f32,
    run: Option<Run>,
    /// A run ended by the current shadow.
    pending: Option<Run>,
}

#[cfg_attr(not(test), allow(dead_code))]
impl Shafts {
    pub fn new(mut shape: ShaftShape) -> Self {
        shape.edge = shape.edge.max(1.0);
        shape.shade = shape.shade.max(1e-3);
        Shafts { shape, ..default() }
    }

    /// A step at distance `t`, `dt` long, scattering `s` (`σ·T·dt`) with sun visibility `vis` (0..1).
    pub fn step(&mut self, t: f32, dt: f32, s: f32, vis: f32) {
        let s = s * vis;
        if vis >= 0.5 {
            self.end_shadow();
            if self.cam.is_none() { self.cam = Some(0.0); }
            let Run(start, _, sum, a, b, before) = self.run.unwrap_or(Run(t, t, 0.0, 0.0, 0.0, self.before));
            let d = ((t - start) / self.shape.edge).min(60.0);
            self.run = Some(Run(start, t, sum + s, a + s * (-d).exp(), b + s * d.exp(), before));
        } else {
            if let Some(r) = self.run.take() { self.pending = Some(r); }
            self.shadow = Some(self.shadow.unwrap_or(0.0) + dt);
            self.total += s;
        }
    }

    fn end_shadow(&mut self) {
        let Some(len) = self.shadow.take() else { return };
        let shade = self.shape.shade;
        let w = smoothstep(0.25 * shade, shade, len);
        if self.cam.is_none() { self.cam = Some(w); }
        if let Some(r) = self.pending.take() { self.close(r, w); }
        self.before = w;
    }

    fn close(&mut self, Run(start, end, sum, a, b, before): Run, after: f32) {
        let ShaftShape { open, edge, after_lit, .. } = self.shape;
        let after = after * (after_lit + (1.0 - after_lit) * self.cam.unwrap_or(0.0));
        let ab = (-((end - start) / edge).min(60.0)).exp();
        let near = before * a + after * ab * b - before * after * ab * sum;
        self.total += open * sum + (1.0 - open) * near;
    }

    /// The ray's weighted in-scattering (the surface or the ray's end bounds a last lit run).
    pub fn finish(mut self) -> f32 {
        self.end_shadow();
        if let Some(r) = self.run.take() { self.close(r, 0.0); }
        self.total
    }
}

/// The shafts' strength for a key light of colour `color` (1.0 = 128 on the colour byte) whose direction toward it
/// rises `to_light_y` (unit vector's y): none from a dim light or one at or below the horizon.
pub fn strength(color: Vec3, to_light_y: f32) -> f32 {
    let lum = color.dot(Vec3::new(0.2126, 0.7152, 0.0722));
    smoothstep(DIM, BRIGHT, lum) * smoothstep(0.0, 0.15, to_light_y)
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The key light's hue pulled toward the fog's (display-encoded 0..1 RGB), linear, brightest channel 1.
fn tint(color: Vec3, fog: Vec3) -> Vec3 {
    let hue = |c: Vec3| c / c.max_element().max(1e-4);
    let fog = Vec3::new(fog.x.powf(2.2), fog.y.powf(2.2), fog.z.powf(2.2));
    let base = if color.max_element() > 1e-4 { hue(color) } else { Vec3::ONE };
    let fog = if fog.max_element() > 1e-4 { hue(fog) } else { base };
    hue(base.lerp(fog, FOG_MIX))
}

/// `RC_VOLUMETRIC`: 0 off, else the strength's scale (default 1).
fn env_scale() -> f32 {
    static SCALE: OnceLock<f32> = OnceLock::new();
    *SCALE.get_or_init(|| std::env::var("RC_VOLUMETRIC").ok().and_then(|v| v.trim().parse::<f32>().ok()).map_or(1.0, |v| v.max(0.0)))
}

/// The march's steps, density, phase g, longest ray, sky ray, [`ShaftShape`] (open, edge, shade, after_lit),
/// [`flatten`]'s share and radius, [`ISO`], [`KNEE`] and [`BLEND`]: the constants, or `RC_VOLUMETRIC_TUNE=` up to
/// fourteen numbers (an empty field keeps the constant).
fn tune() -> [f32; 14] {
    static TUNE: OnceLock<[f32; 14]> = OnceLock::new();
    *TUNE.get_or_init(|| {
        let mut v = [STEPS as f32, DENSITY, G, LENGTH, SKY_LENGTH, OPEN, EDGE, SHADE, AFTER_LIT, FLATTEN, FLAT_RADIUS, ISO, KNEE, BLEND];
        if let Ok(s) = std::env::var("RC_VOLUMETRIC_TUNE") {
            for (slot, f) in v.iter_mut().zip(s.split(',')) {
                if let Ok(x) = f.trim().parse::<f32>() { *slot = x; }
            }
        }
        v
    })
}

/// `RC_VOLUMETRIC_DEBUG=<gain>`: the composite shows the shafts alone, times the gain (0: off).
fn debug_gain() -> f32 {
    static GAIN: OnceLock<f32> = OnceLock::new();
    *GAIN.get_or_init(|| std::env::var("RC_VOLUMETRIC_DEBUG").ok().and_then(|v| v.trim().parse::<f32>().ok()).unwrap_or(0.0))
}

/// The frame's [`VolumetricLight`] from the key light, the fog and the option.
#[allow(clippy::too_many_arguments)]
fn update(
    gfx: Res<GraphicsSettings>,
    key: Res<KeyLight>,
    game: Option<Res<ShadowGame>>,
    play: Option<Res<Play>>,
    fog: Option<Res<GameFog>>,
    shafts: Res<LightShafts>,
    mut out: ResMut<VolumetricLight>,
    mut logged: Local<Option<Vec3>>,
) {
    let color = game.filter(|_| play.is_some()).map_or(DEFAULT_COLOR, |g| g.sun_color());
    let fog = fog.map_or(Vec3::ONE, |f| Vec3::from_array(f.fog.color.map(|c| c as f32 / 255.0)));
    let on = gfx.lighting.enhanced() && key.sun_shadows && shafts.0;
    let hero = play.as_ref().and_then(|p| p.game.mobys.mobys.get(p.hero_id)).filter(|m| m.visible != 0 && m.bsphere[3] > 0.0);
    let hero = hero.map_or(Vec4::ZERO, |m| game_to_bevy([m.bsphere[0], m.bsphere[1], m.bsphere[2]].map(|v| v / 1024.0)).extend(m.bsphere[3] / 1024.0));
    let want = VolumetricLight { tint: tint(color, fog), strength: if on { INTENSITY * strength(color, key.to_light.y) * env_scale() } else { 0.0 }, hero };
    if on && *logged != Some(color) {
        *logged = Some(color);
        println!("volumetric light: key light colour {:.3?}, strength {:.3}, tint {:.3?} (RC_VOLUMETRIC=0 off)", color.to_array(), want.strength, want.tint.to_array());
    }
    if *out != want { *out = want; }
}

/// `rerac::volumetric_light::VolParams`.
#[derive(Clone, Copy, Default, ShaderType)]
struct VolParams {
    /// Tint × strength (linear RGB); w: the cascade count.
    tint: Vec4,
    /// Steps, density σ, phase g, the longest ray.
    march: Vec4,
    /// The sky ray, the debug gain (`RC_VOLUMETRIC_DEBUG`), the target's width and height.
    extra: Vec4,
    /// [`ShaftShape`]: open, edge, shade, after_lit.
    shape: Vec4,
    /// [`SeaLevel`]: its y, 1 while known.
    sea: Vec4,
    /// [`VolumetricLight::hero`].
    hero: Vec4,
    /// [`flatten`]'s share, the surroundings' radius in target pixels.
    flat: Vec4,
    /// The history's view (the temporal pass's reprojection).
    prev_clip_from_world: Mat4,
    /// The history's eye (world); w: 1 while it is valid ([`history_valid`]).
    prev_eye: Vec4,
    /// This frame's [`BLEND`], the frame index ([`jitter`]), [`ISO`], [`KNEE`].
    temporal: Vec4,
}

/// The passes: the march, the temporal accumulation, the composite.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Stage {
    March,
    Temporal,
    Composite,
}

#[derive(Resource)]
struct VolPipelines {
    /// (march, composite) × (single-sampled depth, multisampled).
    march_layout: [BindGroupLayoutDescriptor; 2],
    composite_layout: [BindGroupLayoutDescriptor; 2],
    temporal_layout: BindGroupLayoutDescriptor,
    shader: Handle<Shader>,
    compare: Sampler,
    /// (stage, depth multisampled, target format).
    pipes: Pipelines<(Stage, bool, TextureFormat)>,
}

fn init_pipelines(mut commands: Commands, assets: Res<AssetServer>, fullscreen: Res<FullscreenShader>, device: Res<RenderDevice>) {
    let unfiltered = || texture_2d(TextureSampleType::Float { filterable: false });
    let march = |ms: bool| {
        BindGroupLayoutDescriptor::new(
            "volumetric light march",
            &BindGroupLayoutEntries::with_indices(
                ShaderStages::FRAGMENT,
                (
                    (0, uniform_buffer::<FxView>(false)),
                    (1, depth_entry(ms)),
                    (2, uniform_buffer::<SunCascades>(false)),
                    (3, texture_2d_array(TextureSampleType::Depth)),
                    (4, sampler(SamplerBindingType::Comparison)),
                    (7, uniform_buffer::<VolParams>(false)),
                ),
            ),
        )
    };
    let composite = |ms: bool| {
        BindGroupLayoutDescriptor::new(
            "volumetric light composite",
            &BindGroupLayoutEntries::with_indices(
                ShaderStages::FRAGMENT,
                ((0, uniform_buffer::<FxView>(false)), (1, depth_entry(ms)), (5, unfiltered()), (6, unfiltered()), (7, uniform_buffer::<VolParams>(false))),
            ),
        )
    };
    let temporal = BindGroupLayoutDescriptor::new(
        "volumetric light temporal",
        &BindGroupLayoutEntries::with_indices(
            ShaderStages::FRAGMENT,
            ((0, uniform_buffer::<FxView>(false)), (6, unfiltered()), (7, uniform_buffer::<VolParams>(false)), (8, unfiltered())),
        ),
    );
    let compare = device.create_sampler(&SamplerDescriptor {
        label: Some("volumetric light compare"),
        mag_filter: FilterMode::Linear,
        min_filter: FilterMode::Linear,
        compare: Some(CompareFunction::GreaterEqual),
        ..default()
    });
    commands.insert_resource(VolPipelines {
        march_layout: [march(false), march(true)],
        composite_layout: [composite(false), composite(true)],
        temporal_layout: temporal,
        shader: assets.load(SHADER_PATH),
        compare,
        pipes: Pipelines::new(&fullscreen),
    });
}

fn prepare(
    mut commands: Commands,
    gfx: Option<Res<GraphicsSettings>>,
    vol: Option<Res<VolumetricLight>>,
    device: Res<RenderDevice>,
    mut textures: ResMut<TextureCache>,
    mut history: ResMut<VolHistory>,
    views: Query<(Entity, &ExtractedCamera, &ExtractedView), With<ShadowCamera>>,
) {
    let on = gfx.is_some_and(|g| g.lighting.enhanced()) && vol.is_some_and(|v| v.strength > 0.0);
    for (e, cam, ev) in &views {
        let (Some(size), Some(fx)) = (cam.physical_target_size.filter(|_| on), FxView::of(cam, ev)) else {
            commands.entity(e).remove::<ScatterTarget>();
            history.prev = None;
            continue;
        };
        // Distinct labels: the cache hands each its own texture back every frame (the history persists).
        let mut get = |label: &'static str| {
            textures.get(
                &device,
                TextureDescriptor {
                    label: Some(label),
                    size: Extent3d { width: size.x.div_ceil(2).max(1), height: size.y.div_ceil(2).max(1), depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: TextureDimension::D2,
                    format: SCATTER_FORMAT,
                    usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                },
            )
        };
        let (march, h0, h1) = (get("volumetric light"), get("volumetric light history 0"), get("volumetric light history 1"));
        let now = PrevView { clip_from_world: fx.clip_from_view * fx.view_from_world, eye: fx.world_from_view.w_axis.truncate(), size };
        let prev = history.prev.filter(|p| history_valid(Some(p), now.eye, size));
        let frame = history.frame;
        commands.entity(e).insert(ScatterTarget { march, history: [h0, h1], cur: (frame & 1) as usize, frame, prev });
        history.prev = Some(now);
        history.frame = frame.wrapping_add(1);
    }
}

/// The main camera's view with the sun's shadow maps and the march target.
type VolView = (&'static ExtractedCamera, &'static ExtractedView, &'static ViewTarget, &'static ViewDepthTexture, &'static ViewShadowBindings, &'static ScatterTarget);

fn volumetric_pass(
    view: ViewQuery<VolView, With<ShadowCamera>>,
    lights: Query<&ExtractedDirectionalLight>,
    vol: Option<Res<VolumetricLight>>,
    sea: Option<Res<SeaLevel>>,
    pipes: Option<Res<VolPipelines>>,
    cache: Res<PipelineCache>,
    mut ctx: RenderContext,
) {
    let (Some(pipes), Some(vol)) = (pipes, vol) else { return };
    let entity = view.entity();
    let (cam, ev, target, depth, shadows, scatter) = view.into_inner();
    let Some(light) = lights.iter().find(|l| l.shadow_maps_enabled) else { return };
    let Some((sun, count)) = SunCascades::of([light], entity) else { return };
    let (Some(mut fx), Some(size)) = (FxView::of(cam, ev), cam.physical_target_size) else { return };
    fx.extra = (-light.transform.forward().as_vec3()).extend(0.0);
    let [steps, density, g, length, sky, open, edge, shade, after_lit, flat, flat_radius, iso, knee, blend] = tune();
    let prev = scatter.prev;
    let params = VolParams {
        tint: (vol.tint * vol.strength).extend(count as f32),
        march: Vec4::new(steps.round().max(1.0), density, g, length),
        extra: Vec4::new(sky, debug_gain(), size.x as f32, size.y as f32),
        shape: Vec4::new(open, edge.max(1.0), shade.max(1e-3), after_lit),
        sea: sea.and_then(|s| s.0).map_or(Vec4::ZERO, |y| Vec4::new(y, 1.0, 0.0, 0.0)),
        hero: vol.hero,
        flat: Vec4::new(flat.clamp(0.0, 1.0), flat_radius.max(0.0) * size.y.div_ceil(2) as f32, 0.0, 0.0),
        prev_clip_from_world: prev.map_or(Mat4::IDENTITY, |p| p.clip_from_world),
        prev_eye: prev.map_or(Vec4::ZERO, |p| p.eye.extend(1.0)),
        temporal: Vec4::new(blend.clamp(0.01, 1.0), (scatter.frame % 4096) as f32, iso.clamp(0.0, 1.0), knee.max(1e-4)),
    };
    let format = target.main_texture_format();
    let ms = depth.texture.sample_count() > 1;
    let march_pipe = pipes.pipes.get(&cache, (Stage::March, ms, SCATTER_FORMAT), || Fullscreen {
        label: "volumetric light march",
        layout: pipes.march_layout[ms as usize].clone(),
        shader: pipes.shader.clone(),
        entry: "march",
        defs: depth_defs(ms),
        format: SCATTER_FORMAT,
        blend: None,
        samples: 1,
    });
    let temporal_pipe = pipes.pipes.get(&cache, (Stage::Temporal, false, SCATTER_FORMAT), || Fullscreen {
        label: "volumetric light temporal",
        layout: pipes.temporal_layout.clone(),
        shader: pipes.shader.clone(),
        entry: "temporal",
        defs: vec![],
        format: SCATTER_FORMAT,
        blend: None,
        samples: 1,
    });
    let composite_pipe = pipes.pipes.get(&cache, (Stage::Composite, ms, format), || Fullscreen {
        label: "volumetric light composite",
        layout: pipes.composite_layout[ms as usize].clone(),
        shader: pipes.shader.clone(),
        entry: "composite",
        defs: depth_defs(ms),
        format,
        blend: None,
        samples: 1,
    });
    let (Some(march_pipe), Some(temporal_pipe), Some(composite_pipe)) = (march_pipe, temporal_pipe, composite_pipe) else { return };
    let device = ctx.render_device().clone();
    let fx_buf = fx.buffer(&device, "volumetric light view");
    let sun_buf = crate::fx_view::uniform_buffer(&device, "volumetric light cascades", &sun);
    let params_buf = crate::fx_view::uniform_buffer(&device, "volumetric light params", &params);
    let (accum, last) = (&scatter.history[scatter.cur].default_view, &scatter.history[1 - scatter.cur].default_view);
    let march_group = device.create_bind_group(
        "volumetric light march",
        &cache.get_bind_group_layout(&pipes.march_layout[ms as usize]),
        &BindGroupEntries::with_indices((
            (0, fx_buf.as_entire_binding()),
            (1, depth.view()),
            (2, sun_buf.as_entire_binding()),
            (3, &shadows.directional_light_depth_texture_view),
            (4, &pipes.compare),
            (7, params_buf.as_entire_binding()),
        )),
    );
    let temporal_group = device.create_bind_group(
        "volumetric light temporal",
        &cache.get_bind_group_layout(&pipes.temporal_layout),
        &BindGroupEntries::with_indices((
            (0, fx_buf.as_entire_binding()),
            (6, &scatter.march.default_view),
            (7, params_buf.as_entire_binding()),
            (8, last),
        )),
    );
    for (label, view, pipe, group) in [("volumetric light march", &scatter.march.default_view, march_pipe, &march_group), ("volumetric light temporal", accum, temporal_pipe, &temporal_group)] {
        let mut pass = ctx.begin_tracked_render_pass(RenderPassDescriptor {
            label: Some(label),
            color_attachments: &[Some(RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: Operations { load: LoadOp::Clear(Default::default()), store: StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_render_pipeline(pipe);
        pass.set_bind_group(0, group, &[]);
        pass.draw(0..3, 0..1);
    }
    let post = target.post_process_write();
    let composite_group = device.create_bind_group(
        "volumetric light composite",
        &cache.get_bind_group_layout(&pipes.composite_layout[ms as usize]),
        &BindGroupEntries::with_indices((
            (0, fx_buf.as_entire_binding()),
            (1, depth.view()),
            (5, post.source),
            (6, accum),
            (7, params_buf.as_entire_binding()),
        )),
    );
    let mut pass = ctx.begin_tracked_render_pass(RenderPassDescriptor {
        label: Some("volumetric light composite"),
        color_attachments: &[Some(RenderPassColorAttachment { view: post.destination, depth_slice: None, resolve_target: None, ops: Operations::default() })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    pass.set_render_pipeline(composite_pipe);
    pass.set_bind_group(0, &composite_group, &[]);
    pass.draw(0..3, 0..1);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_integrates_to_one_and_scatters_forward() {
        for g in [0.0, G, 0.8] {
            let n = 4000;
            let sum: f32 = (0..n).map(|k| {
                let mu = -1.0 + 2.0 * (k as f32 + 0.5) / n as f32;
                phase(mu, g) * 2.0 * PI * (2.0 / n as f32)
            }).sum();
            assert!((sum - 1.0).abs() < 0.01, "g {g}: {sum}");
        }
        assert!((phase(0.3, 0.0) - 1.0 / (4.0 * PI)).abs() < 1e-6);
        assert!(phase(1.0, G) > phase(0.0, G) && phase(0.0, G) > phase(-1.0, G));
    }

    #[test]
    fn steps_cover_the_ray_toward_the_camera() {
        let (n, len) = (STEPS, 50.0);
        let total: f32 = (0..n).map(|i| step(i, n, 0.5, len).1).sum();
        assert!((total - len).abs() < 1e-3, "{total}");
        let ts: Vec<f32> = (0..n).map(|i| step(i, n, 0.99, len).0).collect();
        assert!(ts.windows(2).all(|w| w[1] > w[0]) && *ts.last().unwrap() < len);
        // Denser near the camera: the first quarter of the steps covers well under a quarter of the ray.
        assert!(step(n / 4, n, 0.0, len).0 < len / 10.0);
    }

    /// A ray of `(t, dt, vis)` steps, each scattering `S` where lit.
    fn ray(steps: &[(f32, f32, f32)]) -> f32 {
        let mut r = Shafts::new(ShaftShape::default());
        for &(t, dt, vis) in steps { r.step(t, dt, S, vis); }
        r.finish()
    }

    const S: f32 = 0.01;

    #[test]
    fn shafts_stand_out_next_to_long_shadows() {
        // Uniformly lit air (open daylight, the sky): the open weight only.
        let lit: Vec<_> = (0..24).map(|i| (i as f32 * 2.0, 2.0, 1.0)).collect();
        assert!((ray(&lit) - OPEN * 24.0 * S).abs() < 1e-6);
        // In shadow throughout: nothing; a penumbra step counts in full.
        assert_eq!(ray(&[(0.0, 1.0, 0.0), (1.0, 1.0, 0.0)]), 0.0);
        assert!((ray(&[(0.0, 1.0, 0.0), (1.0, 1.0, 0.3)]) - 0.3 * S).abs() < 1e-7);
        // A thin beam between long shadows (inside a tunnel): nearly in full.
        let beam = ray(&[(0.0, SHADE, 0.0), (SHADE, 0.5, 1.0), (SHADE + 0.5, 0.5, 1.0), (SHADE + 1.0, SHADE, 0.0)]);
        assert!(beam > 0.95 * 2.0 * S && beam <= 2.0 * S + 1e-6, "{beam}");
        // The same lit air beside a character's thin shadow: the open weight.
        let thin = ray(&[(0.0, 0.5, 1.0), (0.5, 0.2, 0.0), (0.7, 0.5, 1.0), (1.2, 0.5, 1.0)]);
        assert!((thin - OPEN * 3.0 * S).abs() < 1e-6, "{thin}");
        // Out of a long shadow into open light: in full at the edge, near the open weight five edges past it.
        let out = ray(&[(0.0, SHADE, 0.0), (SHADE, 1.0, 1.0), (SHADE + 5.0 * EDGE, 1.0, 1.0)]);
        let far = OPEN + (1.0 - OPEN) * (-5.0f32).exp();
        assert!((out - S * (1.0 + far)).abs() < 1e-6, "{out}");
        // A wide lit run between long shadows: each step by its nearer edge, never above 1.
        let wide = ray(&[(0.0, SHADE, 0.0), (1.0, 1.0, 1.0), (1.0 + EDGE, 1.0, 1.0), (1.0 + 2.0 * EDGE, 1.0, 1.0), (2.0 + 2.0 * EDGE, SHADE, 0.0)]);
        let mid = 1.0 - (1.0 - (-1.0f32).exp()).powi(2);
        assert!((wide - S * (2.0 + OPEN + (1.0 - OPEN) * mid)).abs() < 1e-6, "{wide}");
    }

    #[test]
    fn shadow_behind_lit_air_counts_less_from_sunlight() {
        // Lit air in front of a shadowed wall: from a sunlit camera by `AFTER_LIT`, from a shaded one in full.
        let from_sun = ray(&[(0.0, 1.0, 1.0), (5.0 * EDGE, 1.0, 1.0), (5.0 * EDGE + 1.0, SHADE, 0.0)]);
        let w = OPEN + (1.0 - OPEN) * AFTER_LIT;
        assert!((from_sun - S * (w + OPEN + (1.0 - OPEN) * AFTER_LIT * (-5.0f32).exp())).abs() < 1e-6, "{from_sun}");
        let from_shade = ray(&[(0.0, SHADE, 0.0), (SHADE, 1.0, 1.0), (SHADE + 5.0 * EDGE, 1.0, 1.0), (SHADE + 5.0 * EDGE + 1.0, SHADE, 0.0)]);
        assert!((from_shade - 2.0 * S).abs() < 1e-6, "{from_shade}");
        assert!(from_shade > 1.5 * from_sun);
    }

    #[test]
    fn flatten_keeps_what_stands_out() {
        let veil = Vec3::splat(0.1);
        assert!((flatten(veil, veil, FLATTEN) - veil * (1.0 - FLATTEN)).length() < 1e-6);
        // A beam over a fainter surround keeps its excess; the darker gap beside it keeps only its own remainder.
        let (beam, around) = (Vec3::splat(0.3), Vec3::splat(0.1));
        assert!((flatten(beam, around, FLATTEN) - (beam - FLATTEN * around)).length() < 1e-6);
        assert!((flatten(around, beam, FLATTEN) - around * (1.0 - FLATTEN)).length() < 1e-6);
        assert_eq!(flatten(beam, around, 0.0), beam);
        assert!(flatten(Vec3::ZERO, beam, FLATTEN).length() == 0.0);
    }

    #[test]
    fn strength_follows_the_key_light() {
        let up = 0.87;
        assert_eq!(strength(Vec3::splat(0.05), up), 0.0);
        assert_eq!(strength(Vec3::ONE, up), 1.0);
        assert!(strength(Vec3::splat(0.3), up) > 0.0 && strength(Vec3::splat(0.3), up) < 1.0);
        assert_eq!(strength(Vec3::ONE, 0.0), 0.0);
        assert_eq!(strength(Vec3::ONE, -0.5), 0.0);
    }

    #[test]
    fn the_mixed_phase_integrates_to_one_with_a_lower_peak() {
        let n = 4000;
        let sum: f32 = (0..n).map(|k| phase_mix(-1.0 + 2.0 * (k as f32 + 0.5) / n as f32, G, ISO) * 2.0 * PI * (2.0 / n as f32)).sum();
        assert!((sum - 1.0).abs() < 0.01, "{sum}");
        assert!(phase_mix(1.0, G, ISO) < 0.75 * phase(1.0, G));
        assert!(phase_mix(1.0, G, ISO) / phase_mix(-1.0, G, ISO) < 0.5 * phase(1.0, G) / phase(-1.0, G));
        assert_eq!(phase_mix(0.4, G, 0.0), phase(0.4, G));
    }

    #[test]
    fn the_soft_cap_bounds_what_the_shafts_add() {
        let faint = Vec3::splat(0.01);
        assert!((soft_cap(faint, KNEE) - faint).length() < 0.001);
        for l in [0.5, 2.0, 50.0] {
            let y = soft_cap(Vec3::new(l, 0.8 * l, 0.5 * l), KNEE).dot(LUMA);
            assert!(y < KNEE, "{l}: {y}");
        }
        // Monotonic: a brighter shaft stays brighter.
        assert!(soft_cap(Vec3::splat(0.4), KNEE).x > soft_cap(Vec3::splat(0.2), KNEE).x);
    }

    #[test]
    fn the_jitter_spreads_over_frames() {
        // Any pixel's offsets over 16 frames leave no gap wider than 2/16 of the step.
        for base in [0.0, 0.37, 0.91] {
            let mut v: Vec<f32> = (0..16).map(|f| jitter(base, f)).collect();
            assert!(v.iter().all(|x| (0.0..1.0).contains(x)));
            v.sort_by(f32::total_cmp);
            let gaps = v.windows(2).map(|w| w[1] - w[0]).chain([1.0 + v[0] - v[15]]);
            assert!(gaps.fold(0.0f32, f32::max) < 2.0 / 16.0, "{base}: {v:?}");
        }
        assert_ne!(jitter(0.5, 1), jitter(0.5, 2));
    }

    #[test]
    fn the_history_is_dropped_on_a_cut_or_a_resize() {
        let p = PrevView { clip_from_world: Mat4::IDENTITY, eye: Vec3::ZERO, size: UVec2::new(1600, 900) };
        assert!(history_valid(Some(&p), Vec3::new(0.5, 0.0, 0.2), p.size));
        assert!(!history_valid(Some(&p), Vec3::new(CUT + 1.0, 0.0, 0.0), p.size));
        assert!(!history_valid(Some(&p), Vec3::ZERO, UVec2::new(1280, 720)));
        assert!(!history_valid(None, Vec3::ZERO, p.size));
        // Its texels: the same surface within 5% (plus a quarter unit), not another one.
        assert!(same_surface(20.4, 20.0) && same_surface(1.1, 1.0));
        assert!(!same_surface(30.0, 20.0) && !same_surface(10_000.0, 20.0));
    }

    #[test]
    fn tint_keeps_the_light_hue_toward_the_fog() {
        let warm = tint(Vec3::new(1.0, 0.8, 0.5), Vec3::new(0.5, 0.6, 0.8));
        assert!((warm.max_element() - 1.0).abs() < 1e-5);
        assert!(warm.x > warm.z, "{warm}");
        assert_eq!(tint(Vec3::ZERO, Vec3::ZERO), Vec3::ONE);
    }
}
