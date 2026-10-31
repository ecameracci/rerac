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
//!
//! **Mobys are not occluded** ([`mask_buffer`]): the radius is about a character's head, so the occlusion would shade
//! their bodies with their own heads and arms over the game's baked lighting. Each moby fragment that passes its
//! alpha test (`assets/shaders/moby.wgsl`, its plain draws) keeps its depth's bits in a per-pixel buffer
//! (`atomicMax`: the nearest), and the apply pass leaves a pixel alone where that depth is the pixel's own. A moby
//! whose bounding sphere is over [`MOBY_MAX_RADIUS`] does not write it (crate::moby_lod `FLAG_AO_WORLD`): props as
//! large as a building keep the occlusion. Their depth still occludes the world around them. The world-space boxes
//! this replaces lifted the occlusion off the grass and plants standing inside one, a rectangle around Ratchet, Clank
//! and the plant mobys' bases (Orxon, session 2026-10-31_09-35-03).

use crate::fx_view::{self, depth_defs, depth_entry, Fullscreen, FxView, Pipelines};
use crate::graphics::GraphicsSettings;
use crate::shadow_render::{ShadowCamera, ShadowPassSet};
use crate::sun_shadow::SunShadowSet;
use bevy::core_pipeline::core_3d::{main_opaque_pass_3d, main_transparent_pass_3d};
use bevy::core_pipeline::{Core3d, Core3dSystems, FullscreenShader};
use bevy::prelude::*;
use bevy::render::camera::ExtractedCamera;
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_resource::binding_types::{storage_buffer_read_only_sized, texture_2d, uniform_buffer};
use bevy::render::storage::{GpuShaderBuffer, ShaderBuffer};
use bevy::render::render_resource::{
    BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, Extent3d, LoadOp, Operations, PipelineCache,
    RenderPassColorAttachment, RenderPassDescriptor, ShaderStages, StoreOp, TextureDescriptor, TextureDimension,
    TextureFormat, TextureSampleType, TextureUsages,
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
/// Mobys with a larger bounding sphere (world units) are occluded like the world.
pub const MOBY_MAX_RADIUS: f32 = 3.0;

/// The per-pixel moby depths ([module docs](self)): a `u32` per pixel of the largest camera viewport, in rows of the
/// view's width, 0 where no moby. Every moby material binds it (crate::moby_render `MobyMaterial::ao_mask`).
static MASK: std::sync::OnceLock<Handle<ShaderBuffer>> = std::sync::OnceLock::new();

/// The shared moby mask ([`MASK`]) for a new moby material.
pub fn mask_buffer() -> Handle<ShaderBuffer> { MASK.get().cloned().expect("AoPlugin creates the moby mask") }

/// The render-world set of the passes.
#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AoSet;

/// The view's occlusion target this frame (only while the option is on).
#[derive(Component)]
struct AoTarget(CachedTexture);

pub struct AoPlugin;

impl Plugin for AoPlugin {
    fn build(&self, app: &mut App) {
        let mask = app.world_mut().resource_mut::<Assets<ShaderBuffer>>().add(ShaderBuffer::with_size(4, default()));
        let _ = MASK.set(mask);
        app.add_systems(PostUpdate, grow_mask);
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else { return };
        render_app.add_systems(RenderStartup, init_pipelines).add_systems(Render, prepare.in_set(RenderSystems::PrepareResources)).add_systems(
            Core3d,
            (
                clear_mask.before(main_opaque_pass_3d).in_set(Core3dSystems::MainPass),
                ao_pass
                    .after(main_opaque_pass_3d)
                    .after(ShadowPassSet)
                    .after(SunShadowSet)
                    .before(main_transparent_pass_3d)
                    .in_set(Core3dSystems::MainPass)
                    .in_set(AoSet),
            ),
        );
    }
}

/// Grows [`MASK`] to the largest camera viewport. A moby material's bind group keeps the buffer it was made with, so
/// the materials are marked changed the frame after, once the render world has the new one.
fn grow_mask(
    cams: Query<&Camera, With<Camera3d>>,
    mut buffers: ResMut<Assets<ShaderBuffer>>,
    mut materials: ResMut<Assets<crate::moby_render::MobyMaterial>>,
    mut rebind: Local<bool>,
) {
    if std::mem::take(&mut *rebind) { materials.iter_mut().for_each(drop); }
    let need = cams.iter().filter_map(Camera::physical_viewport_size).map(|s| s.x as u64 * s.y as u64 * 4).max().unwrap_or(0);
    let h = mask_buffer();
    if buffers.get(&h).is_some_and(|b| b.buffer_description.size < need) {
        if let Some(mut b) = buffers.get_mut(&h) { b.resize(need); }
        *rebind = true;
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
                (
                    uniform_buffer::<FxView>(false),
                    depth_entry(ms),
                    texture_2d(TextureSampleType::Float { filterable: false }),
                    storage_buffer_read_only_sized(false, None),
                ),
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

/// Zeroes the view's part of [`MASK`] before the opaque pass draws the mobys into it.
fn clear_mask(view: ViewQuery<(&ExtractedCamera, &AoTarget), With<ShadowCamera>>, buffers: Res<RenderAssets<GpuShaderBuffer>>, mut ctx: RenderContext) {
    let (cam, _) = view.into_inner();
    let Some(b) = MASK.get().and_then(|h| buffers.get(h)) else { return };
    let n = cam.physical_viewport_size.map_or(0, |s| s.x as u64 * s.y as u64 * 4).min(b.buffer.size());
    if n > 0 { ctx.command_encoder().clear_buffer(&b.buffer, 0, Some(n)); }
}

fn ao_pass(
    view: ViewQuery<(&ExtractedCamera, &ExtractedView, &ViewTarget, &ViewDepthTexture, &AoTarget), With<ShadowCamera>>,
    pipes: Option<Res<AoPipelines>>,
    buffers: Res<RenderAssets<GpuShaderBuffer>>,
    cache: Res<PipelineCache>,
    mut ctx: RenderContext,
) {
    let Some(pipes) = pipes else { return };
    let Some(mask) = MASK.get().and_then(|h| buffers.get(h)) else { return };
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
        &BindGroupEntries::sequential((buf.as_entire_binding(), depth.view(), &ao.0.default_view, mask.buffer.as_entire_binding())),
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
