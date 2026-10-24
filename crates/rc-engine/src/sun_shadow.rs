//! **Port-only: the sun shadows of Lighting Simple and Enhanced** (crate::graphics `Lighting::sun_shadows`; off under
//! `RC_SHADOWS=0`, crate::shadow_render `ShadowSettings::sun`): every opaque object casts a shadow along the level's
//! key light, in place of the game's shadow volumes under mobys.
//!
//! Standard cascaded shadow mapping:
//! - **Casters:** a Bevy `DirectionalLight` with shadow maps along the level's key light (light set 0's direction A,
//!   or B over a dim under-light A: `ShadowGame::sun_light`, fixed for the level; a fixed high sun where there is no
//!   game), raised on the same bearing to at least [`MIN_ELEVATION`] (60°) under Simple, [`ENHANCED_FLOOR`] (30°)
//!   under Enhanced ([`min_elevation`]: Enhanced keeps most levels' own elevation, so its shafts slant in under arches;
//!   the canopies' long shadows are accepted there). Bevy renders its
//!   cascades from the materials' shadow halves (the shaders' `PREPASS_PIPELINE` branch,
//!   gs_state::specialize_shadow): tfrags and ties at their full detail whatever the camera's LOD and cull (tfrags
//!   their LOD-0 list, ties LOD 0 unmorphed and never hidden from Bevy while the sun casts: crate::tie_lod), so the
//!   world's shadows do not change with the view; shrubs (cut out by their texture's alpha) and mobys as drawn. The
//!   metal pass and every blended or effect draw cast nothing. Under Enhanced, crate::sun_rays draws rays from the sky's own
//!   sun (crate::sky_sun), not from this light, and crate::volumetric_light its shafts through these cascades.
//! - **Receiver:** the port's shaders compute no Bevy lighting, so after the opaque world a full-screen pass
//!   (`assets/shaders/sun_shadow.wgsl`) rebuilds each pixel's world position and normal from the depth buffer,
//!   looks it up in its cascade (normal-offset bias, 3×3 PCF on a comparison sampler) and multiplies the frame by
//!   `1 − STRENGTH` where shadowed. Only surfaces turned toward the light are darkened (the game's baked lighting
//!   already shades the others), and the shadows fade out toward [`MAX_DISTANCE`].
//!
//! Limits: the game culls mobys outside the camera's view (and those its occlusion data hides) before they are
//! drawn, so such a moby casts nothing, and a shrub casts only within its draw distance of the camera; the blended
//! draws (water, particles, effects) are not shadowed.
//!
//! Dev switches: `RC_SUN_DIR=x,y,z` (the light's travel direction, world), `RC_SUN_FLOOR=<degrees>` (Enhanced's
//! floor), `RC_SUN_CASTERS` ([`casts`]),
//! `RC_SUN_DEBUG=1` (the receiver replaces the frame: cascade 0/1/2/3 red/green/blue/yellow, dark where shadowed;
//! `2` also colours the skipped pixels: no depth blue, too far green, facing away magenta), `RC_SUN_DUMP=<n>`
//! (the module `dump`).

use crate::fx_view::{self, depth_defs, depth_entry, Fullscreen, FxView, Pipelines};
use crate::gameplay::Play;
use crate::graphics::Lighting;
use crate::shadow_render::{ShadowCamera, ShadowGame, ShadowPassSet, ShadowSettings};
use bevy::core_pipeline::core_3d::{main_opaque_pass_3d, main_transparent_pass_3d};
use bevy::core_pipeline::{Core3d, Core3dSystems, FullscreenShader};
use bevy::light::CascadeShadowConfigBuilder;
use bevy::pbr::{ExtractedDirectionalLight, ViewShadowBindings};
use bevy::prelude::*;
use bevy::render::camera::ExtractedCamera;
use bevy::render::extract_resource::{ExtractResource, ExtractResourcePlugin};
use bevy::render::render_resource::binding_types::{sampler, texture_2d_array, uniform_buffer};
use bevy::render::render_resource::{
    BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, CompareFunction, FilterMode, RenderPassDescriptor,
    Sampler, SamplerBindingType, SamplerDescriptor, ShaderStages, ShaderType, TextureFormat, TextureSampleType,
};
use bevy::render::renderer::{RenderContext, RenderDevice, ViewQuery};
use bevy::render::view::{ExtractedView, ViewDepthTexture, ViewTarget};
use bevy::render::{RenderApp, RenderStartup};
use bevy::shader::Shader;
use std::sync::OnceLock;

const SHADER_PATH: &str = "shaders/sun_shadow.wgsl";
/// The darkening of a shadowed, light-facing pixel (linear light).
const STRENGTH: f32 = 0.45;
/// Beyond this distance from the camera (world units) nothing is shadowed; from 80 % of it the shadows fade.
pub const MAX_DISTANCE: f32 = 150.0;
const CASCADES: usize = 3;
/// The sun's lowest elevation under Lighting Simple (radians). The hero's key light is often low (Novalis spawn: 33°)
/// and set for the characters' shading; a sun that low lays the giant trees' canopies over whole areas.
const MIN_ELEVATION: f32 = 60.0 * std::f32::consts::PI / 180.0;
/// The sun's lowest elevation under Lighting Enhanced (radians; `RC_SUN_FLOOR=<degrees>`): the key light's own
/// elevation down to this, so crate::volumetric_light's shafts slant in under arches and into tunnels.
pub const ENHANCED_FLOOR: f32 = 30.0 * std::f32::consts::PI / 180.0;
/// The light where no game runs (the title world, `RC_PLAY=0`): high, from behind the default camera's left.
const DEFAULT_DIR: Vec3 = Vec3::new(0.35, -0.85, 0.4);

/// Whether a renderer's draws cast sun shadows: all of them, or those named in `RC_SUN_CASTERS` (a comma list of
/// `tfrag`, `tie`, `shrub`, `moby`).
pub fn casts(renderer: &str) -> bool {
    std::env::var("RC_SUN_CASTERS").map_or(true, |v| v.split(',').any(|r| r.trim() == renderer))
}

/// The sun light entity (one while the sun casts shadows).
#[derive(Component)]
struct Sun;

/// Toward the key light (world, unit; crate::sun_rays reads it too), every frame whatever the options.
#[derive(Resource, Clone, Copy, ExtractResource)]
pub struct KeyLight {
    pub to_light: Vec3,
    /// The sun casts shadows (crate::shadow_render `ShadowSettings::sun`).
    pub sun_shadows: bool,
}

impl Default for KeyLight {
    fn default() -> Self { KeyLight { to_light: -DEFAULT_DIR.normalize(), sun_shadows: false } }
}

/// The render-world set of the receiver pass (after the game's shadow volumes, before the ambient occlusion).
#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SunShadowSet;

pub struct SunShadowPlugin;

impl Plugin for SunShadowPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<KeyLight>()
            .add_plugins(ExtractResourcePlugin::<KeyLight>::default())
            .add_systems(PostUpdate, follow.before(bevy::transform::TransformSystems::Propagate));
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else { return };
        render_app.add_systems(RenderStartup, init_pipelines).add_systems(
            Core3d,
            receiver_pass
                .after(main_opaque_pass_3d)
                .after(ShadowPassSet)
                .before(main_transparent_pass_3d)
                .in_set(Core3dSystems::MainPass)
                .in_set(SunShadowSet),
        );
    }
}

/// The sun's lowest elevation (radians) under `lighting`: [`ENHANCED_FLOOR`] (or `floor_deg`, `RC_SUN_FLOOR`) under
/// Enhanced, [`MIN_ELEVATION`] otherwise.
pub fn min_elevation(lighting: Lighting, floor_deg: Option<f32>) -> f32 {
    if !lighting.enhanced() { return MIN_ELEVATION; }
    floor_deg.map_or(ENHANCED_FLOOR, |d| d.clamp(0.0, 90.0).to_radians())
}

/// `RC_SUN_FLOOR=<degrees>`: Enhanced's [`ENHANCED_FLOOR`] in its place.
fn env_floor() -> Option<f32> {
    static FLOOR: OnceLock<Option<f32>> = OnceLock::new();
    *FLOOR.get_or_init(|| std::env::var("RC_SUN_FLOOR").ok().and_then(|v| v.trim().parse().ok()))
}

/// A light's elevation above the horizon (degrees) from its travel direction.
fn elevation_deg(dir: Vec3) -> f32 { (-dir.y).clamp(-1.0, 1.0).asin().to_degrees() }

/// A light travelling `dir`, at least `min` (radians) above the horizon (same compass bearing).
fn raised(dir: Vec3, min: f32) -> Vec3 {
    let flat = Vec2::new(dir.x, dir.z);
    if -dir.y >= min.sin() || flat.length() < 1e-4 { return dir; }
    let (s, c) = min.sin_cos();
    let f = flat.normalize() * c;
    Vec3::new(f.x, -s, f.y)
}

/// The key light, and the sun along it while it casts shadows.
fn follow(
    mut commands: Commands,
    (settings, gfx): (Res<ShadowSettings>, Res<crate::graphics::GraphicsSettings>),
    game: Option<Res<ShadowGame>>,
    play: Option<Res<Play>>,
    mut key: ResMut<KeyLight>,
    mut suns: Query<(Entity, &mut Transform), With<Sun>>,
    mut logged: Local<Option<(Vec3, Lighting)>>,
) {
    let key_dir = game.filter(|_| play.is_some()).and_then(|g| g.sun_light()).unwrap_or(DEFAULT_DIR.normalize());
    let mut dir = raised(key_dir, min_elevation(gfx.lighting, env_floor()));
    if gfx.lighting.sun_shadows() && *logged != Some((key_dir, gfx.lighting)) {
        *logged = Some((key_dir, gfx.lighting));
        println!("sun shadow: key light elevation {:.1} deg, shadow light {:.1} deg ({:?}; RC_SUN_FLOOR)", elevation_deg(key_dir), elevation_deg(dir), gfx.lighting);
    }
    if let Some(d) = std::env::var("RC_SUN_DIR").ok().and_then(|v| {
        let n: Vec<f32> = v.split(',').filter_map(|s| s.trim().parse().ok()).collect();
        (n.len() == 3).then(|| Vec3::new(n[0], n[1], n[2]).normalize())
    }) {
        dir = d;
    }
    if key.to_light != -dir { key.to_light = -dir; }
    let sun = settings.sun(gfx.lighting);
    if key.sun_shadows != sun { key.sun_shadows = sun; }
    if !sun {
        for (e, _) in &suns { commands.entity(e).despawn(); }
        return;
    }
    let up = if dir.y.abs() > 0.99 { Vec3::X } else { Vec3::Y };
    let want = Transform::IDENTITY.looking_to(dir, up);
    if let Some((_, mut t)) = suns.iter_mut().next() {
        if t.rotation != want.rotation { *t = want; }
        return;
    }
    let cascades = CascadeShadowConfigBuilder {
        num_cascades: CASCADES,
        minimum_distance: 0.05,
        first_cascade_far_bound: 15.0,
        maximum_distance: MAX_DISTANCE,
        overlap_proportion: 0.2,
    }
    .build();
    commands.spawn((
        Name::new("Sun (Lighting Simple / Enhanced)"),
        Sun,
        DirectionalLight { shadow_maps_enabled: true, illuminance: 0.0, ..default() },
        cascades,
        want,
        crate::level_switch::KeepAcrossLevels,
    ));
}

/// `rerac::sun_shadow::SunCascades` (crate::volumetric_light binds it too).
#[derive(Clone, Copy, Default, ShaderType)]
pub struct SunCascades {
    clip_from_world: [Mat4; 4],
    /// The cascades' far bounds (view distance), 0 past the last.
    far: Vec4,
    /// World size of a shadow-map texel per cascade.
    texel: Vec4,
}

impl SunCascades {
    /// The sun's cascades for a view and their count (None while the sun casts no shadows there).
    pub fn of<'a>(lights: impl IntoIterator<Item = &'a ExtractedDirectionalLight>, view: Entity) -> Option<(Self, usize)> {
        let light = lights.into_iter().find(|l| l.shadow_maps_enabled)?;
        let cascades = light.cascades.get(&view)?;
        let mut sun = SunCascades::default();
        for (i, (c, bound)) in cascades.iter().zip(&light.cascade_shadow_config.bounds).take(4).enumerate() {
            sun.clip_from_world[i] = c.clip_from_world;
            sun.far[i] = *bound;
            sun.texel[i] = c.texel_size;
        }
        Some((sun, cascades.len().min(4)))
    }
}

#[derive(Resource)]
struct SunPipelines {
    layout: [BindGroupLayoutDescriptor; 2],
    shader: Handle<Shader>,
    compare: Sampler,
    pipes: Pipelines<(bool, u32, TextureFormat, bool)>,
}

fn init_pipelines(mut commands: Commands, assets: Res<AssetServer>, fullscreen: Res<FullscreenShader>, device: Res<RenderDevice>) {
    let layout = |ms: bool, label: &'static str| {
        BindGroupLayoutDescriptor::new(
            label,
            &BindGroupLayoutEntries::sequential(
                ShaderStages::FRAGMENT,
                (
                    uniform_buffer::<FxView>(false),
                    uniform_buffer::<SunCascades>(false),
                    depth_entry(ms),
                    texture_2d_array(TextureSampleType::Depth),
                    sampler(SamplerBindingType::Comparison),
                ),
            ),
        )
    };
    let compare = device.create_sampler(&SamplerDescriptor {
        label: Some("sun shadow compare"),
        mag_filter: FilterMode::Linear,
        min_filter: FilterMode::Linear,
        compare: Some(CompareFunction::GreaterEqual),
        ..default()
    });
    commands.insert_resource(SunPipelines {
        layout: [layout(false, "sun shadow"), layout(true, "sun shadow (multisampled)")],
        shader: assets.load(SHADER_PATH),
        compare,
        pipes: Pipelines::new(&fullscreen),
    });
}

#[allow(clippy::too_many_arguments)]
fn receiver_pass(
    view: ViewQuery<(&ExtractedCamera, &ExtractedView, &ViewTarget, &ViewDepthTexture, &ViewShadowBindings), With<ShadowCamera>>,
    lights: Query<&ExtractedDirectionalLight>,
    pipes: Option<Res<SunPipelines>>,
    cache: Res<bevy::render::render_resource::PipelineCache>,
    mut ctx: RenderContext,
) {
    let Some(pipes) = pipes else { return };
    let entity = view.entity();
    let (cam, ev, target, depth, shadows) = view.into_inner();
    let Some(light) = lights.iter().find(|l| l.shadow_maps_enabled) else { return };
    let Some((sun, count)) = SunCascades::of([light], entity) else { return };
    let Some(mut fx) = FxView::of(cam, ev) else { return };
    let to_light = -light.transform.forward().as_vec3();
    fx.params = Vec4::new(STRENGTH, count as f32, MAX_DISTANCE, 0.8 * MAX_DISTANCE);
    let debug = std::env::var("RC_SUN_DEBUG").ok().and_then(|v| v.trim().parse::<f32>().ok()).unwrap_or(0.0);
    fx.extra = to_light.extend(debug);

    let (samples, format) = fx_view::target_of(target);
    let ms = depth.texture.sample_count() > 1;
    let Some(pipe) = pipes.pipes.get(&cache, (ms, samples, format, debug > 0.5), || Fullscreen {
        label: "sun shadow",
        layout: pipes.layout[ms as usize].clone(),
        shader: pipes.shader.clone(),
        entry: "fragment",
        defs: depth_defs(ms),
        format,
        blend: if debug > 0.5 { None } else { Some(fx_view::MULTIPLY) },
        samples,
    }) else {
        return;
    };
    let device = ctx.render_device().clone();
    let fx_buf = fx.buffer(&device, "sun shadow view");
    let sun_buf = fx_view::uniform_buffer(&device, "sun shadow cascades", &sun);
    let group = device.create_bind_group(
        "sun shadow",
        &cache.get_bind_group_layout(&pipes.layout[ms as usize]),
        &BindGroupEntries::sequential((
            fx_buf.as_entire_binding(),
            sun_buf.as_entire_binding(),
            depth.view(),
            &shadows.directional_light_depth_texture_view,
            &pipes.compare,
        )),
    );
    let mut pass = ctx.begin_tracked_render_pass(RenderPassDescriptor {
        label: Some("sun shadow"),
        color_attachments: &[Some(target.get_color_attachment())],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    if let Some(v) = &cam.viewport { pass.set_camera_viewport(v); }
    pass.set_render_pipeline(pipe);
    pass.set_bind_group(0, &group, &[]);
    pass.draw(0..3, 0..1);
    drop(pass);
    dump::step(&mut ctx, &shadows.directional_light_depth_texture, &shadows.directional_light_depth_texture_view, target, &sun, ev.world_from_view.translation());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn low_key_light_is_raised_on_its_bearing() {
        let novalis = Vec3::new(-0.7774237, -0.54172873, -0.3195972);
        let r = raised(novalis, MIN_ELEVATION);
        assert!((r.length() - 1.0).abs() < 1e-5);
        assert!((-r.y - MIN_ELEVATION.sin()).abs() < 1e-5);
        assert!(Vec2::new(r.x, r.z).normalize().dot(Vec2::new(novalis.x, novalis.z).normalize()) > 0.9999);
        let high = Vec3::new(0.2, -1.0, 0.15).normalize();
        assert_eq!(raised(high, MIN_ELEVATION), high);
    }

    #[test]
    fn enhanced_keeps_the_key_light_down_to_its_floor() {
        assert_eq!(min_elevation(Lighting::Original, None), MIN_ELEVATION);
        assert_eq!(min_elevation(Lighting::Simple, Some(10.0)), MIN_ELEVATION);
        assert_eq!(min_elevation(Lighting::Enhanced, None), ENHANCED_FLOOR);
        assert!((min_elevation(Lighting::Enhanced, Some(45.0)) - 45f32.to_radians()).abs() < 1e-6);
        // Novalis' 33° key light is kept under Enhanced; a 20° one is raised to the floor.
        let novalis = Vec3::new(-0.7774237, -0.54172873, -0.3195972);
        assert_eq!(raised(novalis, min_elevation(Lighting::Enhanced, None)), novalis);
        assert!((elevation_deg(novalis) - 32.8).abs() < 0.1, "{}", elevation_deg(novalis));
        let low = Vec3::new(0.94, -0.342, 0.0);
        assert!((elevation_deg(raised(low, ENHANCED_FLOOR)) - 30.0).abs() < 1e-3);
    }
}

/// `RC_SUN_DUMP=<n>`: on the receiver's n-th frame, read the shadow map back (each 4x4 block's nearest
/// depth) and write each cascade as `work/fx/sun_cascade_<i>.pgm`, with its value range and what cascade
/// 0 holds under the camera, and the frame after the receiver as `work/fx/sun_frame.ppm` (run with `RC_MSAA=0`).
/// GPU readback, so it works where screenshots come back blank (a locked remote session).
mod dump {
    use super::SunCascades;
    use bevy::prelude::*;
    use bevy::render::render_resource::{
        BindGroupEntry, Buffer, BufferDescriptor, BufferUsages, ComputePassDescriptor, MapMode, RawComputePipelineDescriptor,
        ShaderModuleDescriptor, ShaderSource, Texture, TextureView,
    };
    use bevy::render::renderer::RenderContext;
    use bevy::render::view::ViewTarget;
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
    use std::sync::{Arc, Mutex};

    const N: u32 = 512;

    struct Pending {
        buf: Buffer,
        layers: u32,
        mapped: Arc<AtomicBool>,
        sun: SunCascades,
        eye: Vec3,
    }

    static FRAME: AtomicU32 = AtomicU32::new(0);
    static PENDING: Mutex<Option<Pending>> = Mutex::new(None);
    static SCREEN: Mutex<Option<(Buffer, UVec2, Arc<AtomicBool>)>> = Mutex::new(None);

    fn compute(ctx: &mut RenderContext, src: String, view: &TextureView, out: &Buffer, groups: UVec3) {
        let device = ctx.render_device().clone();
        let raw = device.wgpu_device();
        let module = raw.create_shader_module(ShaderModuleDescriptor { label: Some("sun dump"), source: ShaderSource::Wgsl(src.into()) });
        let pipe = raw.create_compute_pipeline(&RawComputePipelineDescriptor {
            label: Some("sun dump"),
            layout: None,
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        let layout = pipe.get_bind_group_layout(0);
        let bind = raw.create_bind_group(&bevy::render::render_resource::BindGroupDescriptor {
            label: Some("sun dump"),
            layout: &layout,
            entries: &[
                BindGroupEntry { binding: 0, resource: bevy::render::render_resource::BindingResource::TextureView(view) },
                BindGroupEntry { binding: 1, resource: out.as_entire_binding() },
            ],
        });
        let mut pass = ctx.command_encoder().begin_compute_pass(&ComputePassDescriptor { label: Some("sun dump"), timestamp_writes: None });
        pass.set_pipeline(&pipe);
        pass.set_bind_group(0, &bind, &[]);
        pass.dispatch_workgroups(groups.x, groups.y, groups.z);
    }

    /// The frame so far (the main texture, without MSAA) as `work/fx/sun_frame.ppm`.
    fn screen(ctx: &mut RenderContext, target: &ViewTarget) {
        let tex = target.main_texture();
        let size = UVec2::new(tex.width(), tex.height());
        let bytes = (size.x * size.y * 4) as u64;
        let device = ctx.render_device().clone();
        let storage = device.create_buffer(&BufferDescriptor { label: Some("sun frame"), size: bytes, usage: BufferUsages::STORAGE | BufferUsages::COPY_SRC, mapped_at_creation: false });
        let buf = device.create_buffer(&BufferDescriptor { label: Some("sun frame read"), size: bytes, usage: BufferUsages::COPY_DST | BufferUsages::MAP_READ, mapped_at_creation: false });
        let src = format!(
            "@group(0) @binding(0) var frame: texture_2d<f32>;
             @group(0) @binding(1) var<storage, read_write> out: array<u32>;
             @compute @workgroup_size(8, 8, 1) fn main(@builtin(global_invocation_id) id: vec3<u32>) {{
                 if (id.x >= {w}u || id.y >= {h}u) {{ return; }}
                 out[id.y * {w}u + id.x] = pack4x8unorm(clamp(textureLoad(frame, id.xy, 0), vec4<f32>(0.0), vec4<f32>(1.0)));
             }}",
            w = size.x,
            h = size.y
        );
        compute(ctx, src, target.main_texture_view(), &storage, UVec3::new(size.x.div_ceil(8), size.y.div_ceil(8), 1));
        ctx.command_encoder().copy_buffer_to_buffer(&storage, 0, &buf, 0, bytes);
        *SCREEN.lock().unwrap() = Some((buf, size, Arc::new(AtomicBool::new(false))));
    }

    fn screen_finish(map: bool) {
        let mut guard = SCREEN.lock().unwrap();
        let Some((buf, size, mapped)) = guard.as_ref() else { return };
        if map {
            let flag = mapped.clone();
            buf.slice(..).map_async(MapMode::Read, move |r| { if r.is_ok() { flag.store(true, Ordering::Release); } });
            return;
        }
        if !mapped.load(Ordering::Acquire) { return };
        let data = buf.slice(..).get_mapped_range();
        let mut ppm = format!("P6 {} {} 255\n", size.x, size.y).into_bytes();
        for px in data.as_chunks::<4>().0 { ppm.extend_from_slice(&px[..3]); }
        let _ = std::fs::write("work/fx/sun_frame.ppm", ppm);
        println!("sun dump: frame {size:?} written");
        drop(data);
        buf.unmap();
        *guard = None;
    }

    pub fn step(ctx: &mut RenderContext, tex: &Texture, view: &TextureView, target: &ViewTarget, sun: &SunCascades, eye: Vec3) {
        let Some(want) = std::env::var("RC_SUN_DUMP").ok().and_then(|v| v.trim().parse::<u32>().ok()) else { return };
        let n = FRAME.fetch_add(1, Ordering::Relaxed);
        if n == want {
            let _ = std::fs::create_dir_all("work/fx");
            let layers = tex.depth_or_array_layers();
            let block = tex.width() / N;
            let src = format!(
                "@group(0) @binding(0) var map: texture_depth_2d_array;
                 @group(0) @binding(1) var<storage, read_write> out: array<f32>;
                 @compute @workgroup_size(8, 8, 1) fn main(@builtin(global_invocation_id) id: vec3<u32>) {{
                     var m = 0.0;
                     for (var j = 0u; j < {block}u; j++) {{ for (var i = 0u; i < {block}u; i++) {{
                         m = max(m, textureLoad(map, vec2<u32>(id.x * {block}u + i, id.y * {block}u + j), i32(id.z), 0));
                     }} }}
                     out[(id.z * {N}u + id.y) * {N}u + id.x] = m;
                 }}"
            );
            let device = ctx.render_device().clone();
            let bytes = (N * N * layers * 4) as u64;
            let storage = device.create_buffer(&BufferDescriptor { label: Some("sun dump"), size: bytes, usage: BufferUsages::STORAGE | BufferUsages::COPY_SRC, mapped_at_creation: false });
            let buf = device.create_buffer(&BufferDescriptor { label: Some("sun dump read"), size: bytes, usage: BufferUsages::COPY_DST | BufferUsages::MAP_READ, mapped_at_creation: false });
            compute(ctx, src, view, &storage, UVec3::new(N / 8, N / 8, layers));
            ctx.command_encoder().copy_buffer_to_buffer(&storage, 0, &buf, 0, bytes);
            screen(ctx, target);
            println!("sun dump: reading {}x{}x{layers} at frame {n}", tex.width(), tex.height());
            *PENDING.lock().unwrap() = Some(Pending { buf, layers, mapped: Arc::new(AtomicBool::new(false)), sun: *sun, eye });
        } else if n == want + 2 {
            screen_finish(true);
            if let Some(p) = PENDING.lock().unwrap().as_ref() {
                let flag = p.mapped.clone();
                p.buf.slice(..).map_async(MapMode::Read, move |r| { if r.is_ok() { flag.store(true, Ordering::Release); } });
            }
        } else if n > want + 2 {
            screen_finish(false);
            let mut guard = PENDING.lock().unwrap();
            let Some(p) = guard.as_ref() else { return };
            if !p.mapped.load(Ordering::Acquire) { return };
            let data = p.buf.slice(..).get_mapped_range();
            let at = |x: u32, y: u32, l: u32| -> f32 {
                let o = (((l * N + y) * N + x) * 4) as usize;
                f32::from_le_bytes([data[o], data[o + 1], data[o + 2], data[o + 3]])
            };
            for l in 0..p.layers {
                let (mut lo, mut hi, mut set, mut near) = (f32::MAX, f32::MIN, 0u32, 0u32);
                for y in 0..N {
                    for x in 0..N {
                        let v = at(x, y, l);
                        if v > 0.0 {
                            lo = lo.min(v);
                            set += 1;
                        }
                        if v >= 0.999 {
                            near += 1;
                        }
                        hi = hi.max(v);
                    }
                }
                let pc = |k: u32| 100.0 * k as f32 / (N * N) as f32;
                println!("sun dump: cascade {l}: set depth {lo} .. {hi}, {:.2}% blocks set, {:.2}% at the near plane, far bound {}, texel {}", pc(set), pc(near), p.sun.far[l as usize % 4], p.sun.texel[l as usize % 4]);
                let mut pgm = format!("P5 {N} {N} 255\n").into_bytes();
                let scale = if hi > lo { 1.0 / (hi - lo) } else { 0.0 };
                for y in 0..N { for x in 0..N { let v = at(x, y, l); pgm.push(if v <= 0.0 { 0 } else { (40.0 + 215.0 * (v - lo) * scale) as u8 }); } }
                let _ = std::fs::write(format!("work/fx/sun_cascade_{l}.pgm"), pgm);
            }
            let c = p.sun.clip_from_world[0] * p.eye.extend(1.0);
            let ndc = c.truncate() / c.w;
            let uv = Vec2::new(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
            let px = (uv * N as f32).as_uvec2().min(UVec2::splat(N - 1));
            println!("sun dump: camera {:?} -> cascade 0 uv {uv:?} depth {}, map there {}", p.eye, ndc.z, at(px.x, px.y, 0));
            drop(data);
            p.buf.unmap();
            *guard = None;
        }
    }
}
