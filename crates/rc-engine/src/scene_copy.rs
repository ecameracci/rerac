//! **Port-only: the scene copy** for the materials that read the world behind them (crate::graphics Water Enhanced,
//! Particles Soft). The main view's depth and colour can not be read by a draw that renders into them, so right before
//! the transparent pass (after the opaque world, the shadows and the ambient occlusion) a full-screen pass copies them
//! into two images the materials bind: [`SceneCopy::depth`] (`R32Float`, the depth buffer's value, 0 = the sky) and
//! [`SceneCopy::color`] (the frame as drawn so far, `Rgba8UnormSrgb`; with MSAA the opaque pass's resolve). The
//! images are the main camera's target size, so a material reads them at its own fragment position.
//!
//! The copy runs only while an option needs it (the colour only for the water); otherwise the images stay 1×1 and
//! nothing is drawn, so Original frames are unchanged. A resize re-binds every [`WaterMaterial`] and
//! [`ParticleMaterial`] (Bevy keeps a material's bind group until the material changes).
//!
//! What the copy lacks: the shadow casters drawn after the shadow pass (crate::moby_render's late draws, only while
//! the game's shadow volumes are on), the blended draws and the effects.

use crate::ao_render::AoSet;
use crate::fx_view::{depth_defs, depth_entry};
use crate::graphics::GraphicsSettings;
use crate::particle_render::ParticleMaterial;
use crate::shadow_render::{ShadowCamera, ShadowPassSet};
use crate::sun_shadow::SunShadowSet;
use crate::water_render::WaterMaterial;
use bevy::asset::RenderAssetUsages;
use bevy::core_pipeline::core_3d::{main_opaque_pass_3d, main_transparent_pass_3d};
use bevy::core_pipeline::{Core3d, Core3dSystems, FullscreenShader};
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::render::extract_resource::{ExtractResource, ExtractResourcePlugin};
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_resource::binding_types::texture_2d;
use bevy::render::render_resource::{
    BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, CachedRenderPipelineId, ColorTargetState, ColorWrites,
    Extent3d, FragmentState, Operations, PipelineCache, RenderPassColorAttachment, RenderPassDescriptor, RenderPipelineDescriptor,
    ShaderStages, TextureDimension, TextureFormat, TextureSampleType, TextureUsages, VertexState,
};
use bevy::render::renderer::{RenderContext, ViewQuery};
use bevy::render::texture::GpuImage;
use bevy::render::view::{ViewDepthTexture, ViewTarget};
use bevy::render::{RenderApp, RenderStartup};
use bevy::shader::Shader;

const SHADER_PATH: &str = "shaders/scene_copy.wgsl";
pub const DEPTH_FORMAT: TextureFormat = TextureFormat::R32Float;
pub const COLOR_FORMAT: TextureFormat = TextureFormat::Rgba8UnormSrgb;

/// The copies and what is copied this frame.
#[derive(Resource, Clone, ExtractResource)]
pub struct SceneCopy {
    pub depth: Handle<Image>,
    pub color: Handle<Image>,
    /// The images' size (1×1 while nothing needs them).
    pub size: UVec2,
    pub want_depth: bool,
    pub want_color: bool,
}

/// The render-world set of the copy (the passes that must land in the copy run before it).
#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SceneCopySet;

pub struct SceneCopyPlugin;

impl Plugin for SceneCopyPlugin {
    fn build(&self, app: &mut App) {
        let mut images = app.world_mut().resource_mut::<Assets<Image>>();
        let depth = images.add(target_image(UVec2::ONE, DEPTH_FORMAT));
        let color = images.add(target_image(UVec2::ONE, COLOR_FORMAT));
        app.insert_resource(SceneCopy { depth, color, size: UVec2::ONE, want_depth: false, want_color: false })
            .add_plugins(ExtractResourcePlugin::<SceneCopy>::default())
            .add_systems(PostUpdate, follow);
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else { return };
        render_app
            .add_systems(RenderStartup, init_pipelines)
            .add_systems(
                Core3d,
                copy_pass
                    .after(main_opaque_pass_3d)
                    .after(ShadowPassSet)
                    .after(SunShadowSet)
                    .after(AoSet)
                    .before(main_transparent_pass_3d)
                    .in_set(Core3dSystems::MainPass)
                    .in_set(SceneCopySet),
            );
    }
}

/// A render-target image (no CPU data: the copy pass writes it).
fn target_image(size: UVec2, format: TextureFormat) -> Image {
    let mut img = Image::new_uninit(Extent3d { width: size.x, height: size.y, depth_or_array_layers: 1 }, TextureDimension::D2, format, RenderAssetUsages::default());
    img.texture_descriptor.usage = TextureUsages::TEXTURE_BINDING | TextureUsages::RENDER_ATTACHMENT;
    img
}

/// What the options need, and the images at the main camera's target size while they need anything.
fn follow(
    g: Res<GraphicsSettings>,
    mut copy: ResMut<SceneCopy>,
    cams: Query<&Camera, With<ShadowCamera>>,
    mut images: ResMut<Assets<Image>>,
    mut water: ResMut<Assets<WaterMaterial>>,
    mut particles: ResMut<Assets<ParticleMaterial>>,
) {
    let (want_depth, want_color) = (g.water.on() || g.particles.on(), g.water.on());
    if copy.want_depth != want_depth || copy.want_color != want_color {
        copy.want_depth = want_depth;
        copy.want_color = want_color;
    }
    let size = if want_depth { cams.iter().next().and_then(Camera::physical_target_size).unwrap_or(UVec2::ONE) } else { UVec2::ONE };
    if size == copy.size || size.x == 0 || size.y == 0 { return; }
    let extent = Extent3d { width: size.x, height: size.y, depth_or_array_layers: 1 };
    for h in [copy.depth.clone(), copy.color.clone()] {
        if let Some(mut img) = images.get_mut(&h) { img.resize(extent); }
    }
    copy.size = size;
    let ids: Vec<_> = water.ids().collect();
    for id in ids { water.get_mut(id); }
    let ids: Vec<_> = particles.ids().collect();
    for id in ids { particles.get_mut(id); }
}

#[derive(Resource)]
struct CopyPipelines {
    layout: [BindGroupLayoutDescriptor; 2],
    shader: Handle<Shader>,
    fullscreen: VertexState,
    ids: std::sync::Mutex<HashMap<(bool, bool), CachedRenderPipelineId>>,
}

fn init_pipelines(mut commands: Commands, assets: Res<AssetServer>, fullscreen: Res<FullscreenShader>) {
    let layout = |ms: bool, label: &'static str| {
        BindGroupLayoutDescriptor::new(
            label,
            &BindGroupLayoutEntries::sequential(ShaderStages::FRAGMENT, (depth_entry(ms), texture_2d(TextureSampleType::Float { filterable: false }))),
        )
    };
    commands.insert_resource(CopyPipelines {
        layout: [layout(false, "scene copy"), layout(true, "scene copy (multisampled)")],
        shader: assets.load(SHADER_PATH),
        fullscreen: fullscreen.to_vertex_state(),
        ids: Default::default(),
    });
}

impl CopyPipelines {
    /// The copy pipeline: depth always, colour when `color`.
    fn id(&self, cache: &PipelineCache, ms: bool, color: bool) -> CachedRenderPipelineId {
        let mut ids = self.ids.lock().unwrap_or_else(|e| e.into_inner());
        *ids.entry((ms, color)).or_insert_with(|| {
            let mut targets = vec![Some(ColorTargetState { format: DEPTH_FORMAT, blend: None, write_mask: ColorWrites::ALL })];
            let mut shader_defs = depth_defs(ms);
            if color {
                targets.push(Some(ColorTargetState { format: COLOR_FORMAT, blend: None, write_mask: ColorWrites::ALL }));
                shader_defs.push("COPY_COLOR".into());
            }
            cache.queue_render_pipeline(RenderPipelineDescriptor {
                label: Some("scene copy".into()),
                layout: vec![self.layout[ms as usize].clone()],
                vertex: self.fullscreen.clone(),
                fragment: Some(FragmentState { shader: self.shader.clone(), shader_defs, targets, ..default() }),
                ..default()
            })
        })
    }
}

fn copy_pass(
    view: ViewQuery<(&ViewTarget, &ViewDepthTexture), With<ShadowCamera>>,
    copy: Option<Res<SceneCopy>>,
    images: Res<RenderAssets<GpuImage>>,
    pipes: Option<Res<CopyPipelines>>,
    cache: Res<PipelineCache>,
    mut ctx: RenderContext,
) {
    let (Some(copy), Some(pipes)) = (copy, pipes) else { return };
    if !copy.want_depth { return; }
    let (target, depth) = view.into_inner();
    let (Some(d), Some(c)) = (images.get(&copy.depth), images.get(&copy.color)) else { return };
    let size = depth.texture.size();
    if d.texture.width() != size.width || d.texture.height() != size.height { return; }
    let color = copy.want_color && c.texture.width() == size.width && c.texture.height() == size.height;
    let ms = depth.texture.sample_count() > 1;
    let Some(pipe) = cache.get_render_pipeline(pipes.id(&cache, ms, color)) else { return };
    let device = ctx.render_device().clone();
    let group = device.create_bind_group(
        "scene copy",
        &cache.get_bind_group_layout(&pipes.layout[ms as usize]),
        &BindGroupEntries::sequential((depth.view(), target.main_texture_view())),
    );
    let mut attachments = vec![Some(RenderPassColorAttachment { view: &d.texture_view, depth_slice: None, resolve_target: None, ops: Operations::default() })];
    if color { attachments.push(Some(RenderPassColorAttachment { view: &c.texture_view, depth_slice: None, resolve_target: None, ops: Operations::default() })); }
    let mut pass = ctx.begin_tracked_render_pass(RenderPassDescriptor {
        label: Some("scene copy"),
        color_attachments: &attachments,
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    pass.set_render_pipeline(pipe);
    pass.set_bind_group(0, &group, &[]);
    pass.draw(0..3, 0..1);
}
