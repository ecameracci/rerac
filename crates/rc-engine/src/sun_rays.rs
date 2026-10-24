//! **Port-only: the sun's rays of Lighting Enhanced** (gated on crate::graphics `GraphicsSettings::lighting.enhanced()`,
//! the gate of every advanced lighting effect; with crate::sun_shadow): light shafts from the sky around the key light.
//!
//! Standard screen-space radial blur (`assets/shaders/sun_rays.wgsl`), after the 3D passes and before the screen tints
//! and the anti-aliasing blit:
//! 1. **Mask** (half resolution): the sky's pixels (depth 0) near the sun's screen position, weighted by their
//!    brightness; geometry in front of the sky masks it, which makes the shafts.
//! 2. **Composite:** [`TAPS`] samples of the mask along the line to the sun, each fainter, added to the frame.
//!
//! The sun is the key light ([`KeyLight`], crate::sun_shadow). The rays fade as it leaves the screen or nears the
//! horizon and are skipped while it is behind the camera. With Lighting Original or Simple nothing runs.

use crate::fx_view::{depth_defs, depth_entry, Fullscreen, FxView, Pipelines};
use crate::graphics::GraphicsSettings;
use crate::shadow_render::ShadowCamera;
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
/// The shafts' brightness.
const INTENSITY: f32 = 0.6;

/// The view's half-resolution mask this frame (only while the option is on).
#[derive(Component)]
struct RaysMask(CachedTexture);

pub struct SunRaysPlugin;

impl Plugin for SunRaysPlugin {
    fn build(&self, app: &mut App) {
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else { return };
        render_app
            .add_systems(RenderStartup, init_pipelines)
            .add_systems(Render, prepare.in_set(RenderSystems::PrepareResources))
            .add_systems(Core3d, rays_pass.in_set(Core3dSystems::PostProcess));
    }
}

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

fn prepare(
    mut commands: Commands,
    gfx: Option<Res<GraphicsSettings>>,
    device: Res<RenderDevice>,
    mut textures: ResMut<TextureCache>,
    views: Query<(Entity, &ExtractedCamera), With<ShadowCamera>>,
) {
    let on = gfx.is_some_and(|g| g.lighting.enhanced());
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

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn rays_pass(
    view: ViewQuery<(&ExtractedCamera, &ExtractedView, &ViewTarget, &ViewDepthTexture, &RaysMask), With<ShadowCamera>>,
    key: Option<Res<KeyLight>>,
    pipes: Option<Res<RaysPipelines>>,
    cache: Res<PipelineCache>,
    mut ctx: RenderContext,
) {
    let (Some(pipes), Some(key)) = (pipes, key) else { return };
    let (cam, ev, target, depth, mask) = view.into_inner();
    let (Some(mut fx), Some(size)) = (FxView::of(cam, ev), cam.physical_target_size) else { return };
    let Some((sun, k)) = sun_on_screen(&fx, size, key.to_light) else { return };
    let aspect = size.x as f32 / size.y.max(1) as f32;
    fx.params = Vec4::new(sun.x, sun.y, INTENSITY * k, aspect);
    fx.extra = Vec4::new(size.x as f32, size.y as f32, TAPS as f32, 0.0);
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
        &BindGroupEntries::with_indices(((0, buf.as_entire_binding()), (2, source), (3, &pipes.linear), (4, &mask.0.default_view))),
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
}
