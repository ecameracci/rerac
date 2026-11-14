//! **Port-only: the 3D clouds of Lighting Enhanced** (gated on crate::graphics `GraphicsSettings::lighting.enhanced()`
//! and the per-level table [`CLOUD_LEVELS`]): a ray-marched layer of cumulus over the game's sky.
//!
//! Standard slab ray-marching through tiling noise (`assets/shaders/clouds.wgsl`), after the opaque world, its shadows,
//! the ambient occlusion and the bounce light, before the transparent pass (so water, effects and the HUD stay in
//! front):
//! 1. **March** (half resolution, only where a sky pixel (depth 0) is among the four it covers): the view ray through
//!    a slab [`BASE`]..[`BASE`] + [`THICKNESS`] units above the camera, to [`FAR`] at most, in the level's
//!    [`Clouds::steps`] steps (up to twice that on the long, grazing paths) from an interleaved-gradient-noise start. The fine detail fades out with distance, where it would alias. The density is a coverage map (a tiling fbm,
//!    [`noise_texture`] R, one tile per [`TILE`] units) eroded by billows (inverted Worley, G and A) and shaped by the
//!    height in the slab (flat bases, rounded tops where the coverage is high). Each step looks [`LIGHT_STEPS`] steps
//!    toward the sun for its self-shadow (Beer's law), with a Henyey-Greenstein phase (forward-scattering silver
//!    linings) and an ambient that brightens toward the tops. The layer drifts with a wind ([`WIND`]) and its noise
//!    coordinates are the camera's world position, so it has parallax as Ratchet moves. Into an RGBA16F target: R the
//!    sunlit light, G the ambient (sky-lit) share, A the opacity, all premultiplied.
//! 2. **Composite** (full resolution, sky pixels only; a 5-tap tent of the half-resolution target averages the
//!    jitter out, which would otherwise show as hatching): the frame becomes `sky · (1 − α + G) + tint · R`, drawn as a
//!    multiply then an add (8-bit target; no dual-source blending). The shaded parts are the sky's own colour, darker,
//!    and the lit parts add the sun's tint ([`Clouds::tint`]: the key light's hue pulled halfway to the fog's), so each
//!    level keeps its palette and mood.
//!
//! The sun is the sky's (crate::sky_sun `SkySun`, the disc the shells paint) or else the key light (crate::sun_shadow
//! `KeyLight`). The layer thins to nothing toward the horizon and with distance, where the game's sky shells paint
//! their own far clouds. Nothing runs under Lighting Original or Simple, on a level whose coverage is 0 (no open sky,
//! night, space, storms), in the space scenes (`Travel::in_transition`, `TitleWorldRt::shown`) or with `RC_CLOUDS=0`.
//!
//! Dev switches: `RC_CLOUDS=0` off, `RC_CLOUDS=<coverage>` forces that coverage on every level (0..1),
//! `RC_CLOUDS_STEPS=<n>` the march steps.

use crate::fx_view::{depth_defs, depth_entry, uniform_buffer, Fullscreen, FxView, Pipelines};
use crate::gi_render::GiSet;
use crate::graphics::GraphicsSettings;
use crate::shadow_render::{ShadowCamera, ShadowGame};
use bevy::core_pipeline::core_3d::{main_opaque_pass_3d, main_transparent_pass_3d};
use bevy::core_pipeline::{Core3d, Core3dSystems, FullscreenShader};
use bevy::prelude::*;
use bevy::render::camera::ExtractedCamera;
use bevy::render::diagnostic::RecordDiagnostics;
use bevy::render::extract_resource::{ExtractResource, ExtractResourcePlugin};
use bevy::render::render_resource::binding_types::{sampler, texture_2d, uniform_buffer as uniform_entry};
use bevy::render::render_resource::{
    AddressMode, BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, BlendComponent, BlendFactor, BlendOperation,
    BlendState, Extent3d, FilterMode, LoadOp, Operations, PipelineCache, RenderPassColorAttachment, RenderPassDescriptor, Sampler,
    SamplerBindingType, SamplerDescriptor, ShaderStages, ShaderType, StoreOp, TextureDataOrder, TextureDescriptor, TextureDimension,
    TextureFormat, TextureSampleType, TextureUsages, TextureView, TextureViewDescriptor,
};
use bevy::render::renderer::{RenderContext, RenderDevice, RenderQueue, ViewQuery};
use bevy::render::texture::{CachedTexture, TextureCache};
use bevy::render::view::{ExtractedView, ViewDepthTexture, ViewTarget};
use bevy::render::{Render, RenderApp, RenderStartup, RenderSystems};
use bevy::shader::Shader;

const SHADER_PATH: &str = "shaders/clouds.wgsl";
const CLOUD_FORMAT: TextureFormat = TextureFormat::Rgba16Float;
/// The noise texture's side (texels; it tiles).
const NOISE_SIZE: u32 = 128;
/// World units per noise tile.
const TILE: f32 = 1400.0;
/// The slab's base and thickness, above the camera (world units).
const BASE: f32 = 140.0;
const THICKNESS: f32 = 110.0;
/// The farthest the march starts (world units); the layer fades out before it.
const FAR: f32 = 5000.0;
/// Extinction per unit at density 1.
const SIGMA: f32 = 0.11;
/// Steps of the light march toward the sun, each this share of the thickness.
const LIGHT_STEPS: u32 = 2;
const LIGHT_STEP: f32 = 0.28;
/// The march steps per pixel.
const STEPS: u32 = 24;
/// The wind's drift (world units per second, Bevy x / z).
const WIND: Vec2 = Vec2::new(4.0, 1.5);
/// The sun's light on the clouds, and the floor of their ambient (at the base; 1 at the tops).
const SUN_GAIN: f32 = 0.85;
const AMBIENT_FLOOR: f32 = 0.65;

/// Which levels get clouds, how much of the sky they cover (0: none) and how bright the sun makes them (the mood),
/// and why. Tuned from before / after shots of each sky (docs/plan/graphics_options.md "Clouds per level").
pub const CLOUD_LEVELS: [(f32, f32, &str); 19] = [
    (0.0, 0.0, "Veldin: dusk, stars above and the sun on the horizon"),
    (0.65, 1.0, "Novalis: fair-weather cumulus"),
    (0.0, 0.0, "Aridia: dust haze"),
    (0.6, 1.0, "Kerwan: city daylight"),
    (0.55, 0.8, "Eudora: forest daylight under the canopy"),
    (0.0, 0.0, "Rilgar: night"),
    (0.0, 0.0, "Nebula G34: space station"),
    (0.0, 0.0, "Umbris: dark swamp"),
    (0.0, 0.0, "Batalia: storm"),
    (0.0, 0.0, "Gaspar: volcanic"),
    (0.0, 0.0, "Orxon: night, toxic"),
    (0.65, 1.0, "Pokitaru: tropical daylight"),
    (0.0, 0.0, "Hoven: night snow"),
    (0.0, 0.0, "Gemlik: airless moon"),
    (0.0, 0.0, "Oltanis: overcast"),
    (0.0, 0.0, "Quartu: night, indoor factory"),
    (0.55, 0.9, "Kalebo III: daylight"),
    (0.0, 0.0, "Drek's fleet: space"),
    (0.0, 0.0, "Veldin finale: dark red sky"),
];

/// [`CLOUD_LEVELS`]' coverage and sun gain for a level ((0, 0) past the table).
pub fn level_clouds(level: u32) -> (f32, f32) { CLOUD_LEVELS.get(level as usize).map_or((0.0, 0.0), |l| (l.0, l.1)) }

/// This frame's clouds (main world, and the render world's copy): coverage 0 draws nothing.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, ExtractResource)]
pub struct Clouds {
    pub coverage: f32,
    /// The sun's light on the lit sides (linear RGB, the key light's hue × the level's sun gain).
    pub tint: Vec3,
    /// Toward the sun (Bevy world, unit).
    pub to_sun: Vec3,
    /// The noise offset of the camera and the wind, wrapped to a tile (world units, Bevy x / z).
    pub offset: Vec2,
    pub steps: u32,
}

pub struct CloudPlugin;

impl Plugin for CloudPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Clouds>()
            .add_plugins(ExtractResourcePlugin::<Clouds>::default())
            .add_systems(PostUpdate, update.after(crate::sky_sun::SkySunSet).after(bevy::transform::TransformSystems::Propagate));
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else { return };
        render_app
            .add_systems(RenderStartup, init_pipelines)
            .add_systems(Render, prepare.in_set(RenderSystems::PrepareResources))
            .add_systems(
                Core3d,
                clouds_pass
                    .after(main_opaque_pass_3d)
                    .after(GiSet)
                    .before(main_transparent_pass_3d)
                    .in_set(Core3dSystems::MainPass),
            );
    }
}

/// `RC_CLOUDS`: Some(coverage) to force (0: off), None for the table.
fn env_coverage() -> Option<f32> {
    static FORCE: std::sync::OnceLock<Option<f32>> = std::sync::OnceLock::new();
    *FORCE.get_or_init(|| std::env::var("RC_CLOUDS").ok().and_then(|v| v.trim().parse::<f32>().ok()).map(|v| v.clamp(0.0, 1.0)))
}

fn env_steps() -> u32 {
    static STEPS_ENV: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
    *STEPS_ENV.get_or_init(|| std::env::var("RC_CLOUDS_STEPS").ok().and_then(|v| v.trim().parse().ok()).map_or(STEPS, |n: u32| n.clamp(4, 128)))
}

/// The sun's hue on the clouds: the key light's (1.0 = 128 on the colour byte) pulled halfway toward the fog's
/// (display-encoded 0..1), linear, brightest channel 1; white without either.
fn tint(key: Vec3, fog: Vec3) -> Vec3 {
    let hue = |c: Vec3| c / c.max_element().max(1e-4);
    let fog = fog.powf(2.2);
    let base = if key.max_element() > 1e-4 { hue(key) } else { Vec3::ONE };
    let fog = if fog.max_element() > 1e-4 { hue(fog) } else { base };
    hue(base.lerp(fog, 0.5).lerp(Vec3::ONE, 0.35))
}

#[allow(clippy::too_many_arguments)]
fn update(
    gfx: Res<GraphicsSettings>,
    sun: Option<Res<crate::sky_sun::SkySun>>,
    key: Option<Res<crate::sun_shadow::KeyLight>>,
    game: Option<Res<ShadowGame>>,
    fog: Option<Res<crate::game_camera::GameFog>>,
    travel: Option<Res<crate::travel_render::Travel>>,
    title: Option<Res<crate::title_world::TitleWorldRt>>,
    cams: Query<&GlobalTransform, With<crate::fly_cam::FlyCam>>,
    time: Res<Time>,
    mut out: ResMut<Clouds>,
    mut logged: Local<Option<(u32, bool)>>,
) {
    let level = crate::level_load::level_index();
    let space = travel.is_some_and(|t| t.in_transition()) || title.is_some_and(|t| t.shown());
    let (table, gain) = level_clouds(level);
    let coverage = if space || !gfx.lighting.enhanced() { 0.0 } else { env_coverage().unwrap_or(table) };
    let gain = if env_coverage().is_some() && gain == 0.0 { 1.0 } else { gain };
    if gfx.lighting.enhanced() && *logged != Some((level, coverage > 0.0)) {
        *logged = Some((level, coverage > 0.0));
        let why = CLOUD_LEVELS.get(level as usize).map_or("", |l| l.2);
        println!("graphics: clouds {} (level {level:02} {why}; RC_CLOUDS=0|<coverage> forces)", if coverage > 0.0 { format!("coverage {coverage:.2}") } else { "off".into() });
    }
    if coverage <= 0.0 {
        if out.coverage != 0.0 { *out = Clouds::default(); }
        return;
    }
    let to_sun = sun.and_then(|s| s.to_sun).or(key.map(|k| k.to_light)).unwrap_or(Vec3::Y).normalize_or(Vec3::Y);
    let key_colour = game.map_or(Vec3::ONE, |g| g.sun_color());
    let fog = fog.map_or(Vec3::ONE, |f| Vec3::from_array(f.fog.color.map(|c| c as f32 / 255.0)));
    let cam = cams.iter().next().map_or(Vec3::ZERO, |t| t.translation());
    let drift = cam.xz() + WIND * (time.elapsed_secs_wrapped() % 3600.0);
    *out = Clouds { coverage, tint: tint(key_colour, fog) * gain * SUN_GAIN, to_sun, offset: drift.rem_euclid(Vec2::splat(TILE)), steps: env_steps() };
}

/// `Clouds` in the shader (`clouds.wgsl` `CloudParams`).
#[derive(Clone, Copy, Default, ShaderType)]
struct CloudParams {
    /// Toward the sun (world), w: the noise texels per world unit.
    sun: Vec4,
    /// The sun's tint (linear), w: coverage.
    tint: Vec4,
    /// Base, thickness (above the camera), the far distance, σ.
    shape: Vec4,
    /// The noise offset (x, z), steps, the ambient floor.
    march: Vec4,
    /// Light steps, the light step's share of the thickness.
    light: Vec4,
}

/// The view's half-resolution cloud target this frame (only while there are clouds).
#[derive(Component)]
struct CloudTarget(CachedTexture);

#[derive(Resource)]
struct CloudPipelines {
    march_layout: [BindGroupLayoutDescriptor; 2],
    composite_layout: [BindGroupLayoutDescriptor; 2],
    shader: Handle<Shader>,
    noise: TextureView,
    repeat: Sampler,
    linear: Sampler,
    /// (entry point, depth multisampled, target samples, target format).
    pipes: Pipelines<(&'static str, bool, u32, TextureFormat)>,
}

/// `dst · src`, alpha kept (fx_view's MULTIPLY) and `dst + src`, alpha kept.
const ADD: BlendState = BlendState {
    color: BlendComponent { src_factor: BlendFactor::One, dst_factor: BlendFactor::One, operation: BlendOperation::Add },
    alpha: BlendComponent { src_factor: BlendFactor::Zero, dst_factor: BlendFactor::One, operation: BlendOperation::Add },
};

fn init_pipelines(mut commands: Commands, assets: Res<AssetServer>, fullscreen: Res<FullscreenShader>, device: Res<RenderDevice>, queue: Res<RenderQueue>) {
    let filterable = || texture_2d(TextureSampleType::Float { filterable: true });
    let layout = |label: &'static str, ms: bool| {
        BindGroupLayoutDescriptor::new(
            label,
            &BindGroupLayoutEntries::with_indices(
                ShaderStages::FRAGMENT,
                (
                    (0, uniform_entry::<FxView>(false)),
                    (1, depth_entry(ms)),
                    (2, filterable()),
                    (3, sampler(SamplerBindingType::Filtering)),
                    (4, uniform_entry::<CloudParams>(false)),
                ),
            ),
        )
    };
    let noise = device.create_texture_with_data(
        &queue,
        &TextureDescriptor {
            label: Some("cloud noise"),
            size: Extent3d { width: NOISE_SIZE, height: NOISE_SIZE, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8Unorm,
            usage: TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        },
        TextureDataOrder::LayerMajor,
        &noise_texture(NOISE_SIZE),
    );
    let mk_sampler = |label: &'static str, mode: AddressMode| {
        device.create_sampler(&SamplerDescriptor {
            label: Some(label),
            address_mode_u: mode,
            address_mode_v: mode,
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            ..default()
        })
    };
    commands.insert_resource(CloudPipelines {
        march_layout: [layout("clouds march", false), layout("clouds march", true)],
        composite_layout: [layout("clouds composite", false), layout("clouds composite", true)],
        shader: assets.load(SHADER_PATH),
        noise: noise.create_view(&TextureViewDescriptor::default()),
        repeat: mk_sampler("cloud noise", AddressMode::Repeat),
        linear: mk_sampler("clouds", AddressMode::ClampToEdge),
        pipes: Pipelines::new(&fullscreen),
    });
}

fn prepare(
    mut commands: Commands,
    clouds: Option<Res<Clouds>>,
    device: Res<RenderDevice>,
    mut textures: ResMut<TextureCache>,
    views: Query<(Entity, &ExtractedCamera), With<ShadowCamera>>,
) {
    let on = clouds.is_some_and(|c| c.coverage > 0.0);
    for (e, cam) in &views {
        let Some(size) = cam.physical_target_size.filter(|_| on) else {
            commands.entity(e).remove::<CloudTarget>();
            continue;
        };
        let t = textures.get(
            &device,
            TextureDescriptor {
                label: Some("clouds"),
                size: Extent3d { width: size.x.div_ceil(2).max(1), height: size.y.div_ceil(2).max(1), depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: TextureDimension::D2,
                format: CLOUD_FORMAT,
                usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
        );
        commands.entity(e).insert(CloudTarget(t));
    }
}

fn clouds_pass(
    view: ViewQuery<(&ExtractedCamera, &ExtractedView, &ViewTarget, &ViewDepthTexture, &CloudTarget), With<ShadowCamera>>,
    clouds: Option<Res<Clouds>>,
    pipes: Option<Res<CloudPipelines>>,
    cache: Res<PipelineCache>,
    mut ctx: RenderContext,
) {
    let (Some(pipes), Some(c)) = (pipes, clouds) else { return };
    if c.coverage <= 0.0 { return; }
    let (cam, ev, target, depth, cloud) = view.into_inner();
    let Some(fx) = FxView::of(cam, ev) else { return };
    let (samples, format) = crate::fx_view::target_of(target);
    let ms = depth.texture.sample_count() > 1;
    let pipe = |entry: &'static str, samples: u32, format: TextureFormat, blend: Option<BlendState>, layout: &BindGroupLayoutDescriptor| {
        pipes.pipes.get(&cache, (entry, ms, samples, format), || Fullscreen {
            label: entry,
            layout: layout.clone(),
            shader: pipes.shader.clone(),
            entry,
            defs: depth_defs(ms),
            format,
            blend,
            samples,
        })
    };
    let march = pipe("march", 1, CLOUD_FORMAT, None, &pipes.march_layout[ms as usize]);
    let shade = pipe("shade", samples, format, Some(crate::fx_view::MULTIPLY), &pipes.composite_layout[ms as usize]);
    let light = pipe("light", samples, format, Some(ADD), &pipes.composite_layout[ms as usize]);
    let (Some(march), Some(shade), Some(light)) = (march, shade, light) else { return };
    let params = CloudParams {
        sun: c.to_sun.extend(NOISE_SIZE as f32 / TILE),
        tint: c.tint.extend(c.coverage),
        shape: Vec4::new(BASE, THICKNESS, FAR, SIGMA),
        march: Vec4::new(c.offset.x, c.offset.y, c.steps as f32, AMBIENT_FLOOR),
        light: Vec4::new(LIGHT_STEPS as f32, LIGHT_STEP, 0.0, 0.0),
    };
    let device = ctx.render_device().clone();
    // `RC_RENDER_DIAG=1`: the march and the composite each get a GPU time (render/clouds_march, render/clouds_composite).
    let diagnostics = ctx.diagnostic_recorder();
    let diagnostics = diagnostics.as_deref();
    let view_buf = fx.buffer(&device, "clouds view");
    let param_buf = uniform_buffer(&device, "clouds params", &params);
    let march_group = device.create_bind_group(
        "clouds march",
        &cache.get_bind_group_layout(&pipes.march_layout[ms as usize]),
        &BindGroupEntries::with_indices((
            (0, view_buf.as_entire_binding()),
            (1, depth.view()),
            (2, &pipes.noise),
            (3, &pipes.repeat),
            (4, param_buf.as_entire_binding()),
        )),
    );
    let composite_group = device.create_bind_group(
        "clouds composite",
        &cache.get_bind_group_layout(&pipes.composite_layout[ms as usize]),
        &BindGroupEntries::with_indices((
            (0, view_buf.as_entire_binding()),
            (1, depth.view()),
            (2, &cloud.0.default_view),
            (3, &pipes.linear),
            (4, param_buf.as_entire_binding()),
        )),
    );
    {
        let mut pass = ctx.begin_tracked_render_pass(RenderPassDescriptor {
            label: Some("clouds march"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: &cloud.0.default_view,
                depth_slice: None,
                resolve_target: None,
                ops: Operations { load: LoadOp::Clear(Default::default()), store: StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        let span = diagnostics.pass_span(&mut pass, "clouds_march");
        pass.set_render_pipeline(march);
        pass.set_bind_group(0, &march_group, &[]);
        pass.draw(0..3, 0..1);
        span.end(&mut pass);
    }
    let mut pass = ctx.begin_tracked_render_pass(RenderPassDescriptor {
        label: Some("clouds composite"),
        color_attachments: &[Some(target.get_color_attachment())],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    if let Some(v) = &cam.viewport { pass.set_camera_viewport(v); }
    pass.set_bind_group(0, &composite_group, &[]);
    let span = diagnostics.pass_span(&mut pass, "clouds_composite");
    pass.set_render_pipeline(shade);
    pass.draw(0..3, 0..1);
    pass.set_render_pipeline(light);
    pass.draw(0..3, 0..1);
    span.end(&mut pass);
}

/// The clouds' tiling noise, `size`² RGBA8: R a 5-octave value-noise fbm (the coverage, 4 cells across), G and A
/// inverted Worley F1 (billows, 6 and 12 cells), B a 3-octave fbm (16 cells: fine erosion). Every channel tiles.
pub fn noise_texture(size: u32) -> Vec<u8> {
    let mut texels = Vec::with_capacity((size * size) as usize);
    for y in 0..size {
        for x in 0..size {
            let (u, v) = (x as f32 / size as f32, y as f32 / size as f32);
            texels.push([fbm(u, v, 4, 5, 11), 1.0 - worley(u, v, 6, 23), fbm(u, v, 16, 3, 37), 1.0 - worley(u, v, 12, 51)]);
        }
    }
    // Each channel stretched over 0..1 (an fbm's sum gathers around its middle).
    let (mut lo, mut hi) = ([f32::MAX; 4], [f32::MIN; 4]);
    for t in &texels {
        for c in 0..4 {
            lo[c] = lo[c].min(t[c]);
            hi[c] = hi[c].max(t[c]);
        }
    }
    texels.iter().flat_map(|t| std::array::from_fn::<u8, 4, _>(|c| ((t[c] - lo[c]) / (hi[c] - lo[c]).max(1e-6) * 255.0).round() as u8)).collect()
}

fn hash(x: u32, y: u32, seed: u32) -> u32 {
    let mut h = x.wrapping_mul(0x8da6_b343) ^ y.wrapping_mul(0xd816_3841) ^ seed.wrapping_mul(0xcb1a_b31f);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b_3c6d);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297a_2d39);
    h ^ (h >> 15)
}

fn unit(h: u32) -> f32 { (h >> 8) as f32 / (1u32 << 24) as f32 }

/// Tiling value noise of `cells` cells across the unit square (0..1).
fn value(u: f32, v: f32, cells: u32, seed: u32) -> f32 {
    let (fx, fy) = (u * cells as f32, v * cells as f32);
    let (ix, iy) = (fx.floor(), fy.floor());
    let (tx, ty) = (fx - ix, fy - iy);
    let q = |t: f32| t * t * t * (t * (t * 6.0 - 15.0) + 10.0);
    let (sx, sy) = (q(tx), q(ty));
    let c = |dx: u32, dy: u32| unit(hash((ix as u32 + dx) % cells, (iy as u32 + dy) % cells, seed));
    let top = c(0, 0) + (c(1, 0) - c(0, 0)) * sx;
    let bot = c(0, 1) + (c(1, 1) - c(0, 1)) * sx;
    top + (bot - top) * sy
}

/// Tiling fbm: `octaves` of [`value`] from `cells`, each twice as fine and half as strong, normalised to 0..1.
fn fbm(u: f32, v: f32, cells: u32, octaves: u32, seed: u32) -> f32 {
    let (mut sum, mut amp, mut norm) = (0.0, 1.0, 0.0);
    for o in 0..octaves {
        sum += value(u, v, cells << o, seed + o) * amp;
        norm += amp;
        amp *= 0.5;
    }
    sum / norm
}

/// Tiling Worley F1 (distance to the nearest feature point, one per cell), `cells` across, about 0..1.
fn worley(u: f32, v: f32, cells: u32, seed: u32) -> f32 {
    let (fx, fy) = (u * cells as f32, v * cells as f32);
    let (ix, iy) = (fx.floor() as i32, fy.floor() as i32);
    let n = cells as i32;
    let mut best = f32::MAX;
    for dy in -1..=1 {
        for dx in -1..=1 {
            let (cx, cy) = (ix + dx, iy + dy);
            let (wx, wy) = (cx.rem_euclid(n) as u32, cy.rem_euclid(n) as u32);
            let px = cx as f32 + unit(hash(wx, wy, seed));
            let py = cy as f32 + unit(hash(wx, wy, seed + 1));
            best = best.min((px - fx).powi(2) + (py - fy).powi(2));
        }
    }
    best.sqrt().min(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_noise_tiles_and_spans_its_range() {
        let n = 64;
        let t = noise_texture(n);
        let at = |x: u32, y: u32, c: usize| t[((y * n + x) * 4) as usize + c] as i32;
        for c in 0..4 {
            // Across the wrap the step is no larger than inside the tile.
            let inner = (0..n).map(|y| (at(n / 2, y, c) - at(n / 2 + 1, y, c)).abs()).max().unwrap();
            let wrap = (0..n).map(|y| (at(n - 1, y, c) - at(0, y, c)).abs()).max().unwrap();
            assert!(wrap <= inner.max(8) + 24, "channel {c}: wrap {wrap}, inner {inner}");
            let (lo, hi) = (0..n * n).map(|i| t[(i * 4) as usize + c]).fold((255u8, 0u8), |(a, b), v| (a.min(v), b.max(v)));
            assert!(lo < 80 && hi > 170, "channel {c}: {lo}..{hi}");
        }
    }

    #[test]
    fn clouds_only_on_open_daylight_skies() {
        let on: Vec<u32> = (0..CLOUD_LEVELS.len() as u32).filter(|&l| level_clouds(l).0 > 0.0).collect();
        assert_eq!(on, [1, 3, 4, 11, 16]);
        assert_eq!(level_clouds(19), (0.0, 0.0));
        assert!(CLOUD_LEVELS.iter().all(|l| !l.2.is_empty() && (0.0..=1.0).contains(&l.0)));
    }

    #[test]
    fn the_tint_keeps_the_light_hue() {
        let warm = tint(Vec3::new(1.0, 0.7, 0.4), Vec3::new(1.0, 0.8, 0.6));
        assert!((warm.max_element() - 1.0).abs() < 1e-5 && warm.x > warm.z);
        assert_eq!(tint(Vec3::ZERO, Vec3::ZERO), Vec3::ONE);
    }
}
