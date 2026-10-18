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
//! **Mobys are not occluded** ([`AoMobys`]): the radius is about a character's head, so the occlusion would shade
//! their bodies with their own heads and arms over the game's baked lighting. The apply pass leaves a pixel alone
//! inside the bounding sphere of one of the [`MOBYS`] nearest drawn mobys (+0x31; spheres up to [`MOBY_MAX_RADIUS`],
//! so props as large as a building keep the occlusion) and above that moby's feet, fading over the sphere's outer
//! fifth and the first 0.15 units over the feet. Their depth still occludes the world around them.

use crate::fly_cam::FlyCam;
use crate::fx_view::{self, depth_defs, depth_entry, Fullscreen, FxView, Pipelines};
use crate::gameplay::Play;
use crate::graphics::GraphicsSettings;
use crate::shadow_render::{ShadowCamera, ShadowPassSet};
use crate::sun_shadow::SunShadowSet;
use crate::tfrag_render::game_to_bevy;
use bevy::core_pipeline::core_3d::{main_opaque_pass_3d, main_transparent_pass_3d};
use bevy::core_pipeline::{Core3d, Core3dSystems, FullscreenShader};
use bevy::prelude::*;
use bevy::render::camera::ExtractedCamera;
use bevy::render::extract_resource::{ExtractResource, ExtractResourcePlugin};
use bevy::render::render_resource::binding_types::{texture_2d, uniform_buffer};
use bevy::render::render_resource::{
    BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, Extent3d, LoadOp, Operations, PipelineCache,
    RenderPassColorAttachment, RenderPassDescriptor, ShaderStages, ShaderType, StoreOp, TextureDescriptor, TextureDimension,
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
/// Mobys left unoccluded at most (the nearest).
const MOBYS: usize = 32;
/// Mobys with a larger bounding sphere (world units) are occluded like the world.
const MOBY_MAX_RADIUS: f32 = 3.0;

/// `rerac::ao::AoMobys`: the mobys the apply pass leaves alone (module docs), this frame.
#[derive(Resource, Clone, Copy, Default, ShaderType, ExtractResource)]
struct AoMobys {
    /// Bounding sphere (Bevy world centre, radius).
    spheres: [Vec4; MOBYS],
    /// The feet height (Bevy y: the moby's position z) of moby i at `feet[i / 4][i % 4]`.
    feet: [Vec4; MOBYS / 4],
    count: u32,
}

/// The render-world set of the passes.
#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AoSet;

/// The view's occlusion target this frame (only while the option is on).
#[derive(Component)]
struct AoTarget(CachedTexture);

pub struct AoPlugin;

impl Plugin for AoPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AoMobys>().add_plugins(ExtractResourcePlugin::<AoMobys>::default()).add_systems(PostUpdate, collect_mobys);
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

/// The drawn mobys nearest the camera for [`AoMobys`].
fn collect_mobys(play: Option<Res<Play>>, g: Option<Res<GraphicsSettings>>, cams: Query<&Transform, With<FlyCam>>, mut out: ResMut<AoMobys>) {
    let mut v = AoMobys::default();
    if let (Some(p), Some(eye), true) = (play, cams.iter().next().map(|t| t.translation), g.is_some_and(|g| g.ao.on())) {        let mut near: Vec<(f32, Vec4, f32)> = p
            .game
            .mobys
            .mobys
            .iter()
            .filter(|m| m.visible != 0)
            .filter_map(|m| {
                let r = m.bsphere[3] / 1024.0;
                if !(r > 0.0 && r <= MOBY_MAX_RADIUS) { return None; }
                let c = game_to_bevy([m.bsphere[0] / 1024.0, m.bsphere[1] / 1024.0, m.bsphere[2] / 1024.0]);
                let d = c.distance(eye) - r;
                (d < MAX_DISTANCE).then_some((d, c.extend(r), m.position[2]))
            })
            .collect();
        near.sort_by(|a, b| a.0.total_cmp(&b.0));
        for (i, &(_, s, feet)) in near.iter().take(MOBYS).enumerate() {
            v.spheres[i] = s;
            v.feet[i / 4][i % 4] = feet;
        }
        v.count = near.len().min(MOBYS) as u32;
    }
    *out = v;
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
                    uniform_buffer::<AoMobys>(false),
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

fn ao_pass(
    view: ViewQuery<(&ExtractedCamera, &ExtractedView, &ViewTarget, &ViewDepthTexture, &AoTarget), With<ShadowCamera>>,
    pipes: Option<Res<AoPipelines>>,
    mobys: Option<Res<AoMobys>>,
    cache: Res<PipelineCache>,
    mut ctx: RenderContext,
) {
    let Some(pipes) = pipes else { return };
    let mobys = mobys.map_or_else(AoMobys::default, |m| *m);
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
    let moby_buf = fx_view::uniform_buffer(&device, "ao mobys", &mobys);
    let occ_group = device.create_bind_group(
        "ao occlusion",
        &cache.get_bind_group_layout(&pipes.occlusion_layout[ms as usize]),
        &BindGroupEntries::sequential((buf.as_entire_binding(), depth.view())),
    );
    let apply_group = device.create_bind_group(
        "ao apply",
        &cache.get_bind_group_layout(&pipes.apply_layout[ms as usize]),
        &BindGroupEntries::sequential((buf.as_entire_binding(), depth.view(), &ao.0.default_view, moby_buf.as_entire_binding())),
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
