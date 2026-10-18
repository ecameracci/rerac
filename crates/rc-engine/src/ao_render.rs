//! **Port-only: Ambient occlusion On** (crate::graphics): screen-space ambient occlusion on the opaque world.
//!
//! Standard depth-only SSAO (`assets/shaders/ao.wgsl`), after the opaque world and its shadows, before the
//! transparent pass and the scene copy:
//! 1. **Occlusion:** per pixel, the view-space position and normal rebuilt from the depth buffer (crate::fx_view);
//!    [`SAMPLES`] taps on a spiral of [`RADIUS`] world units projected to the screen, rotated per pixel; each tap
//!    above the surface's tangent plane occludes by its height over the plane and its distance. Into an R8 target.
//! 2. **Apply:** a 4×4 depth-aware blur of that target, multiplied into the frame (`mix(1, ao, STRENGTH)`).
//!
//! The sky (depth 0) is never occluded, and the occlusion fades out between 100 and [`MAX_DISTANCE`] world units,
//! where a pixel spans more than the radius. With the option at Original neither pass runs.

use crate::fx_view::{self, depth_defs, depth_entry, Fullscreen, FxView, Pipelines};
use crate::graphics::GraphicsSettings;
use crate::shadow_render::{ShadowCamera, ShadowPassSet};
use crate::sun_shadow::SunShadowSet;
use bevy::core_pipeline::core_3d::{main_opaque_pass_3d, main_transparent_pass_3d};
use bevy::core_pipeline::{Core3d, Core3dSystems, FullscreenShader};
use bevy::prelude::*;
use bevy::render::camera::ExtractedCamera;
use bevy::render::render_resource::binding_types::{texture_2d, uniform_buffer};
use bevy::render::render_resource::{
    BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, Extent3d, LoadOp, Operations, PipelineCache,
    RenderPassColorAttachment, RenderPassDescriptor, ShaderStages, StoreOp, TextureDescriptor, TextureDimension, TextureFormat,
    TextureSampleType, TextureUsages,
};
use bevy::render::renderer::{RenderContext, RenderDevice, ViewQuery};
use bevy::render::texture::{CachedTexture, TextureCache};
use bevy::render::view::{ExtractedView, ViewDepthTexture, ViewTarget};
use bevy::render::{Render, RenderApp, RenderStartup, RenderSystems};
use bevy::shader::Shader;

const SHADER_PATH: &str = "shaders/ao.wgsl";
const AO_FORMAT: TextureFormat = TextureFormat::R8Unorm;
/// Taps per pixel.
const SAMPLES: u32 = 12;
/// The occlusion radius (world units: a metre is about one).
const RADIUS: f32 = 0.8;
/// How much of the occlusion darkens the frame.
const STRENGTH: f32 = 0.85;
/// No occlusion beyond this distance (world units).
pub const MAX_DISTANCE: f32 = 150.0;

/// The render-world set of the passes.
#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AoSet;

/// The view's occlusion target this frame (only while the option is on).
#[derive(Component)]
struct AoTarget(CachedTexture);

pub struct AoPlugin;

impl Plugin for AoPlugin {
    fn build(&self, app: &mut App) {
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else { return };
        render_app.add_systems(RenderStartup, init_pipelines).add_systems(Render, prepare.in_set(RenderSystems::PrepareResources)).add_systems(
            Core3d,
            ao_pass
                .after(main_opaque_pass_3d)
                .after(ShadowPassSet)
                .after(SunShadowSet)
                .before(main_transparent_pass_3d)
                .in_set(Core3dSystems::MainPass)
                .in_set(AoSet),
        );
    }
}

#[derive(Resource)]
struct AoPipelines {
    /// (occlusion, apply) × (single-sampled depth, multisampled).
    occlusion_layout: [BindGroupLayoutDescriptor; 2],
    apply_layout: [BindGroupLayoutDescriptor; 2],
    shader: Handle<Shader>,
    /// (apply, depth multisampled, target samples, target format).
    pipes: Pipelines<(bool, bool, u32, TextureFormat)>,
}

fn init_pipelines(mut commands: Commands, assets: Res<AssetServer>, fullscreen: Res<FullscreenShader>) {
    let occlusion = |ms: bool| {
        BindGroupLayoutDescriptor::new("ao occlusion", &BindGroupLayoutEntries::sequential(ShaderStages::FRAGMENT, (uniform_buffer::<FxView>(false), depth_entry(ms))))
    };
    let apply = |ms: bool| {
        BindGroupLayoutDescriptor::new(
            "ao apply",
            &BindGroupLayoutEntries::sequential(
                ShaderStages::FRAGMENT,
                (uniform_buffer::<FxView>(false), depth_entry(ms), texture_2d(TextureSampleType::Float { filterable: false })),
            ),
        )
    };
    commands.insert_resource(AoPipelines {
        occlusion_layout: [occlusion(false), occlusion(true)],
        apply_layout: [apply(false), apply(true)],
        shader: assets.load(SHADER_PATH),
        pipes: Pipelines::new(&fullscreen),
    });
}

fn prepare(
    mut commands: Commands,
    g: Option<Res<GraphicsSettings>>,
    device: Res<RenderDevice>,
    mut textures: ResMut<TextureCache>,
    views: Query<(Entity, &ExtractedCamera), With<ShadowCamera>>,
) {
    let on = g.is_some_and(|g| g.ao.on());
    for (e, cam) in &views {
        let size = cam.physical_target_size.filter(|_| on);
        let Some(size) = size else {
            commands.entity(e).remove::<AoTarget>();
            continue;
        };
        let t = textures.get(
            &device,
            TextureDescriptor {
                label: Some("ao"),
                size: Extent3d { width: size.x.max(1), height: size.y.max(1), depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: TextureDimension::D2,
                format: AO_FORMAT,
                usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
        );
        commands.entity(e).insert(AoTarget(t));
    }
}

fn ao_pass(
    view: ViewQuery<(&ExtractedCamera, &ExtractedView, &ViewTarget, &ViewDepthTexture, &AoTarget), With<ShadowCamera>>,
    pipes: Option<Res<AoPipelines>>,
    cache: Res<PipelineCache>,
    mut ctx: RenderContext,
) {
    let Some(pipes) = pipes else { return };
    let (cam, ev, target, depth, ao) = view.into_inner();
    let Some(mut fx) = FxView::of(cam, ev) else { return };
    fx.params = Vec4::new(RADIUS, SAMPLES as f32, STRENGTH, MAX_DISTANCE);
    let (samples, format) = fx_view::target_of(target);
    let ms = depth.texture.sample_count() > 1;
    let occlusion = pipes.pipes.get(&cache, (false, ms, 1, AO_FORMAT), || Fullscreen {
        label: "ao occlusion",
        layout: pipes.occlusion_layout[ms as usize].clone(),
        shader: pipes.shader.clone(),
        entry: "occlusion",
        defs: depth_defs(ms),
        format: AO_FORMAT,
        blend: None,
        samples: 1,
    });
    let apply = pipes.pipes.get(&cache, (true, ms, samples, format), || Fullscreen {
        label: "ao apply",
        layout: pipes.apply_layout[ms as usize].clone(),
        shader: pipes.shader.clone(),
        entry: "apply",
        defs: depth_defs(ms),
        format,
        blend: Some(fx_view::MULTIPLY),
        samples,
    });
    let (Some(occlusion), Some(apply)) = (occlusion, apply) else { return };
    let device = ctx.render_device().clone();
    let buf = fx.buffer(&device, "ao view");
    let occ_group = device.create_bind_group(
        "ao occlusion",
        &cache.get_bind_group_layout(&pipes.occlusion_layout[ms as usize]),
        &BindGroupEntries::sequential((buf.as_entire_binding(), depth.view())),
    );
    let apply_group = device.create_bind_group(
        "ao apply",
        &cache.get_bind_group_layout(&pipes.apply_layout[ms as usize]),
        &BindGroupEntries::sequential((buf.as_entire_binding(), depth.view(), &ao.0.default_view)),
    );
    {
        let mut pass = ctx.begin_tracked_render_pass(RenderPassDescriptor {
            label: Some("ao occlusion"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: &ao.0.default_view,
                depth_slice: None,
                resolve_target: None,
                ops: Operations { load: LoadOp::Clear(Default::default()), store: StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        if let Some(v) = &cam.viewport { pass.set_camera_viewport(v); }
        pass.set_render_pipeline(occlusion);
        pass.set_bind_group(0, &occ_group, &[]);
        pass.draw(0..3, 0..1);
    }
    let mut pass = ctx.begin_tracked_render_pass(RenderPassDescriptor {
        label: Some("ao apply"),
        color_attachments: &[Some(target.get_color_attachment())],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    if let Some(v) = &cam.viewport { pass.set_camera_viewport(v); }
    pass.set_render_pipeline(apply);
    pass.set_bind_group(0, &apply_group, &[]);
    pass.draw(0..3, 0..1);
}
