//! **Port-only: the sun's rays of Lighting Enhanced** (gated on crate::graphics `GraphicsSettings::lighting.enhanced()`,
//! the gate of every advanced lighting effect; with crate::sun_shadow): light shafts from the sun the level's sky
//! paints.
//!
//! Standard screen-space radial blur (`assets/shaders/sun_rays.wgsl`), after the 3D passes and before the screen tints
//! and the anti-aliasing blit:
//! 1. **Mask** (half resolution): the sky's pixels (depth 0) around the sun's screen position (a falloff of
//!    [`SPOT_SCALE`] × the sun spot's radius), keyed on their luminance between the sky's median and the sun's peak,
//!    so the sun disc and its glow make the mask and the plain sky adds nothing; geometry in front of the sky masks
//!    it, which makes the shafts.
//! 2. **Composite:** [`TAPS`] samples of the mask along the line to the sun, each fainter, soft-capped under [`KNEE`]
//!    and added to the frame.
//!
//! The sun is the sky's ([`SkySun`], crate::sky_sun: found in the shells at level load, turning with its shell),
//! not the level's key light (rarely where the sky shows its sun); the shadows' light (crate::sun_shadow) takes this
//! sun's bearing, raised to 60°. The rays' strength follows the sun's brightness; they fade as it leaves the screen or nears the horizon
//! and are skipped while it is behind the camera, and on a sky without a sun. With Lighting Original or Simple nothing
//! runs, nor under `RC_SHADOWS=0` ([`KeyLight::sun_shadows`] false: no sun at all), nor on the levels and in the
//! space scenes crate::graphics [`LightShafts`] turns the shafts off (`SHAFT_LEVELS`).

use crate::fx_view::{depth_defs, depth_entry, Fullscreen, FxView, Pipelines};
use crate::graphics::{GraphicsSettings, LightShafts};
use crate::shadow_render::ShadowCamera;
use crate::sky_sun::{SkySun, SkySunPlugin};
use crate::sun_shadow::KeyLight;
use bevy::core_pipeline::{Core3d, Core3dSystems, FullscreenShader};
use bevy::prelude::*;
use bevy::render::camera::ExtractedCamera;
use bevy::render::render_resource::binding_types::{sampler, texture_2d, uniform_buffer};
use bevy::render::render_resource::{
    AddressMode, BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, Extent3d, FilterMode, LoadOp, Operations,
    PipelineCache, RenderPassColorAttachment, RenderPassDescriptor, Sampler, SamplerBindingType, SamplerDescriptor, ShaderStages,
    StoreOp, TextureDescriptor, TextureDimension, TextureFormat, TextureSampleType, TextureUsages,
};
use bevy::render::renderer::{RenderContext, RenderDevice, ViewQuery};
use bevy::render::texture::{CachedTexture, TextureCache};
use bevy::render::view::{ExtractedView, ViewDepthTexture, ViewTarget};
use bevy::render::{Render, RenderApp, RenderStartup, RenderSystems};
use bevy::shader::Shader;

const SHADER_PATH: &str = "shaders/sun_rays.wgsl";
const MASK_FORMAT: TextureFormat = TextureFormat::Rgba16Float;
/// Samples along each ray.
const TAPS: u32 = 40;
/// The shafts' brightness at full strength.
const INTENSITY: f32 = 0.6;
/// The soft cap's knee (sun_rays.wgsl `composite`): the rays add `l / (1 + Y(l) / KNEE)`, under `KNEE` in luminance,
/// so a sun filling the sky (Gemlik) tints the frame instead of washing it out (2026-10-26 play test).
const KNEE: f32 = 0.06;
const LUMA: Vec3 = Vec3::new(0.2126, 0.7152, 0.0722);
/// The mask's falloff radius, in sun spot radii (crate::sky_sun `Sun::radius`), and its range (target UV heights):
/// a dome's wide glow (Novalis: 21°) would otherwise let every bright cloud on screen into the shafts.
const SPOT_SCALE: f32 = 2.0;
const MIN_SPOT: f32 = 0.08;
const MAX_SPOT: f32 = 0.3;

/// The view's half-resolution mask this frame (only while the option is on).
#[derive(Component)]
struct RaysMask(CachedTexture);

pub struct SunRaysPlugin;

impl Plugin for SunRaysPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(SkySunPlugin);
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else { return };
        render_app
            .add_systems(RenderStartup, init_pipelines)
            .add_systems(Render, prepare.in_set(RenderSystems::PrepareResources))
            .add_systems(Core3d, rays_pass.in_set(Core3dSystems::PostProcess).in_set(SunRaysSet));
    }
}

/// The render-world set of the rays pass (crate::volumetric_light runs before it).
#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SunRaysSet;

#[derive(Resource)]
struct RaysPipelines {
    mask_layout: [BindGroupLayoutDescriptor; 2],
    composite_layout: BindGroupLayoutDescriptor,
    shader: Handle<Shader>,
    linear: Sampler,
    /// (composite, depth multisampled, target format).
    pipes: Pipelines<(bool, bool, TextureFormat)>,
}

fn init_pipelines(mut commands: Commands, assets: Res<AssetServer>, fullscreen: Res<FullscreenShader>, device: Res<RenderDevice>) {
    let filterable = || texture_2d(TextureSampleType::Float { filterable: true });
    let mask = |ms: bool| {
        BindGroupLayoutDescriptor::new(
            "sun rays mask",
            &BindGroupLayoutEntries::with_indices(
                ShaderStages::FRAGMENT,
                ((0, uniform_buffer::<FxView>(false)), (1, depth_entry(ms)), (2, filterable()), (3, sampler(SamplerBindingType::Filtering))),
            ),
        )
    };
    let composite = BindGroupLayoutDescriptor::new(
        "sun rays composite",
        &BindGroupLayoutEntries::with_indices(
            ShaderStages::FRAGMENT,
            ((0, uniform_buffer::<FxView>(false)), (2, filterable()), (3, sampler(SamplerBindingType::Filtering)), (4, filterable())),
        ),
    );
    let linear = device.create_sampler(&SamplerDescriptor {
        label: Some("sun rays"),
        address_mode_u: AddressMode::ClampToEdge,
        address_mode_v: AddressMode::ClampToEdge,
        mag_filter: FilterMode::Linear,
        min_filter: FilterMode::Linear,
        ..default()
    });
    commands.insert_resource(RaysPipelines {
        mask_layout: [mask(false), mask(true)],
        composite_layout: composite,
        shader: assets.load(SHADER_PATH),
        linear,
        pipes: Pipelines::new(&fullscreen),
    });
}

#[allow(clippy::too_many_arguments)]
fn prepare(
    mut commands: Commands,
    gfx: Option<Res<GraphicsSettings>>,
    shafts: Option<Res<LightShafts>>,
    key: Option<Res<KeyLight>>,
    sun: Option<Res<SkySun>>,
    device: Res<RenderDevice>,
    mut textures: ResMut<TextureCache>,
    views: Query<(Entity, &ExtractedCamera), With<ShadowCamera>>,
) {
    let on = gfx.is_some_and(|g| g.lighting.enhanced()) && shafts.is_some_and(|s| s.0) && sun.is_some_and(|s| s.to_sun.is_some());
    let on = on && key.is_some_and(|k| k.sun_shadows);
    for (e, cam) in &views {
        let Some(size) = cam.physical_target_size.filter(|_| on) else {
            commands.entity(e).remove::<RaysMask>();
            continue;
        };
        let t = textures.get(
            &device,
            TextureDescriptor {
                label: Some("sun rays mask"),
                size: Extent3d { width: (size.x / 2).max(1), height: (size.y / 2).max(1), depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: TextureDimension::D2,
                format: MASK_FORMAT,
                usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
        );
        commands.entity(e).insert(RaysMask(t));
    }
}

/// The sun's position (target UV) and how strongly its rays show (0: none), for a view.
fn sun_on_screen(fx: &FxView, target: UVec2, to_light: Vec3) -> Option<(Vec2, f32)> {
    let v = fx.view_from_world * to_light.extend(0.0);
    if v.z >= -0.05 { return None; }
    let clip = fx.clip_from_view * v;
    let ndc = clip.truncate().truncate() / clip.w;
    let on_screen = 1.0 - smoothstep(1.0, 1.6, ndc.x.abs().max(ndc.y.abs()));
    let above = smoothstep(0.0, 0.15, to_light.y);
    let k = on_screen * above;
    if k <= 0.0 { return None; }
    let px = fx.viewport.truncate().truncate() + Vec2::new(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5) * fx.viewport.zw();
    Some((px / target.as_vec2().max(Vec2::ONE), k))
}

/// What the rays add, soft-capped under `knee` in luminance. Mirrored in the shader's `composite`.
#[cfg_attr(not(test), allow(dead_code))]
fn soft_cap(light: Vec3, knee: f32) -> Vec3 { light / (1.0 + light.dot(LUMA) / knee) }

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn rays_pass(
    view: ViewQuery<(&ExtractedCamera, &ExtractedView, &ViewTarget, &ViewDepthTexture, &RaysMask), With<ShadowCamera>>,
    sky: Option<Res<SkySun>>,
    pipes: Option<Res<RaysPipelines>>,
    cache: Res<PipelineCache>,
    mut ctx: RenderContext,
) {
    let (Some(pipes), Some(sky)) = (pipes, sky) else { return };
    let Some(to_sun) = sky.to_sun else { return };
    let (cam, ev, target, depth, mask) = view.into_inner();
    let (Some(mut fx), Some(size)) = (FxView::of(cam, ev), cam.physical_target_size) else { return };
    let Some((sun, k)) = sun_on_screen(&fx, size, to_sun) else { return };
    let aspect = size.x as f32 / size.y.max(1) as f32;
    fx.params = Vec4::new(sun.x, sun.y, INTENSITY * sky.strength * k, aspect);
    let spot = (sky.radius.tan() * fx.clip_from_view.y_axis.y * 0.5 * fx.viewport.w / size.y.max(1) as f32 * SPOT_SCALE).clamp(MIN_SPOT, MAX_SPOT);
    let (lo, hi) = (sky.median + 0.25 * (sky.peak - sky.median), sky.median + 0.75 * (sky.peak - sky.median));
    fx.extra = Vec4::new(TAPS as f32, spot, lo, hi);
    let format = target.main_texture_format();
    let ms = depth.texture.sample_count() > 1;
    let mask_pipe = pipes.pipes.get(&cache, (false, ms, MASK_FORMAT), || Fullscreen {
        label: "sun rays mask",
        layout: pipes.mask_layout[ms as usize].clone(),
        shader: pipes.shader.clone(),
        entry: "mask",
        defs: depth_defs(ms),
        format: MASK_FORMAT,
        blend: None,
        samples: 1,
    });
    let composite_pipe = pipes.pipes.get(&cache, (true, false, format), || Fullscreen {
        label: "sun rays composite",
        layout: pipes.composite_layout.clone(),
        shader: pipes.shader.clone(),
        entry: "composite",
        defs: vec![],
        format,
        blend: None,
        samples: 1,
    });
    let (Some(mask_pipe), Some(composite_pipe)) = (mask_pipe, composite_pipe) else { return };
    let device = ctx.render_device().clone();
    let buf = fx.buffer(&device, "sun rays view");
    let composite_buf = FxView { extra: Vec4::new(TAPS as f32, KNEE, 0.0, 0.0), ..fx }.buffer(&device, "sun rays composite");
    let post = target.post_process_write();
    let (source, destination) = (post.source, post.destination);
    let mask_group = device.create_bind_group(
        "sun rays mask",
        &cache.get_bind_group_layout(&pipes.mask_layout[ms as usize]),
        &BindGroupEntries::with_indices(((0, buf.as_entire_binding()), (1, depth.view()), (2, source), (3, &pipes.linear))),
    );
    let composite_group = device.create_bind_group(
        "sun rays composite",
        &cache.get_bind_group_layout(&pipes.composite_layout),
        &BindGroupEntries::with_indices(((0, composite_buf.as_entire_binding()), (2, source), (3, &pipes.linear), (4, &mask.0.default_view))),
    );
    {
        let mut pass = ctx.begin_tracked_render_pass(RenderPassDescriptor {
            label: Some("sun rays mask"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: &mask.0.default_view,
                depth_slice: None,
                resolve_target: None,
                ops: Operations { load: LoadOp::Clear(Default::default()), store: StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_render_pipeline(mask_pipe);
        pass.set_bind_group(0, &mask_group, &[]);
        pass.draw(0..3, 0..1);
    }
    let mut pass = ctx.begin_tracked_render_pass(RenderPassDescriptor {
        label: Some("sun rays composite"),
        color_attachments: &[Some(RenderPassColorAttachment { view: destination, depth_slice: None, resolve_target: None, ops: Operations::default() })],
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

    fn view_looking(dir: Vec3) -> FxView {
        let world_from_view = Transform::IDENTITY.looking_to(dir, Vec3::Y).to_matrix();
        let clip_from_view = Mat4::perspective_infinite_reverse_rh(1.0, 16.0 / 9.0, 0.1);
        FxView {
            clip_from_view,
            view_from_clip: clip_from_view.inverse(),
            world_from_view,
            view_from_world: world_from_view.inverse(),
            viewport: Vec4::new(0.0, 0.0, 1600.0, 900.0),
            ..default()
        }
    }

    #[test]
    fn sun_ahead_is_centred_and_behind_is_skipped() {
        let up_ahead = Vec3::new(0.0, 0.5, -1.0).normalize();
        let (uv, k) = sun_on_screen(&view_looking(up_ahead), UVec2::new(1600, 900), up_ahead).unwrap();
        assert!((uv - Vec2::splat(0.5)).length() < 1e-4, "{uv}");
        assert!(k > 0.99);
        assert!(sun_on_screen(&view_looking(-up_ahead), UVec2::new(1600, 900), up_ahead).is_none());
        // Below the horizon: no rays.
        let low = Vec3::new(0.0, -0.2, -1.0).normalize();
        assert!(sun_on_screen(&view_looking(low), UVec2::new(1600, 900), low).is_none());
    }

    #[test]
    fn rays_fade_past_the_screen_edge_and_near_the_horizon() {
        let ahead = Vec3::new(0.0, 0.5, -1.0).normalize();
        let view = view_looking(ahead);
        let k = |to_sun: Vec3| sun_on_screen(&view, UVec2::new(1600, 900), to_sun).map_or(0.0, |(_, k)| k);
        // At NDC x = `ndc` on the view's centre row: just past the right edge it fades, its point off the target;
        // past 1.6, none.
        let tan_x = 1.0 / view.clip_from_view.x_axis.x;
        let side = |ndc: f32| (view.world_from_view * Vec4::new(ndc * tan_x, 0.0, -1.0, 0.0)).truncate().normalize();
        let (uv, edge) = sun_on_screen(&view, UVec2::new(1600, 900), side(1.2)).unwrap();
        assert!(uv.x > 1.0 && edge > 0.0 && edge < 1.0, "{uv} {edge}");
        assert!(k(side(0.9)) > 0.99);
        assert_eq!(k(side(2.0)), 0.0);
        // A sun 3° up fades by the horizon smoothstep (0..0.15 of its height).
        let low = Vec3::new(0.0, 3f32.to_radians().sin(), -1.0).normalize();
        let k_low = sun_on_screen(&view_looking(low), UVec2::new(1600, 900), low).unwrap().1;
        assert!(k_low > 0.2 && k_low < 0.35, "{k_low}");
    }

    #[test]
    fn the_soft_cap_keeps_faint_rays_and_bounds_bright_ones() {
        let faint = Vec3::splat(0.005);
        assert!(soft_cap(faint, KNEE).dot(LUMA) > 0.9 * faint.dot(LUMA));
        for y in [0.5, 2.0, 50.0] {
            let capped = soft_cap(Vec3::new(1.0, 0.6, 0.2) * y, KNEE).dot(LUMA);
            assert!(capped < KNEE, "{y}: {capped}");
        }
    }
}
