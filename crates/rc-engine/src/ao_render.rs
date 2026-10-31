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
//! inside the box of one of the [`MOBYS`] nearest drawn mobys (+0x31; bounding spheres up to [`MOBY_MAX_RADIUS`], so
//! props as large as a building keep the occlusion): its class's bind-pose bounds (the high LOD's packed vertex
//! positions, [`ClassBoxes`]) placed as crate::moby_render draws it (rows +0xc0, scale +0x2c, position +0x10), padded
//! by [`PAD`] on an animated class (limbs leave the bind pose; not under the feet), fading over the [`FADE`] units
//! outside each face and inside the bottom one (its feet), so the wall it hangs against and the floor it stands on keep
//! their occlusion up to its contact, under an overhanging edge too. The box does not depend on the view: the
//! half-sphere in front of the bounding sphere's centre it replaces shaded the far faces and the corners of a crate,
//! moving with the camera (Orxon marker 4 of session 2026-10-30_15-54-38). Their depth still occludes the world around
//! them.

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
/// The mask's fade outside each face of a moby's box, inside its bottom face (world units).
const FADE: f32 = 0.05;
/// An animated class's box grows by this on every face but the bottom one (world units).
const PAD: f32 = 0.15;

/// `rerac::ao::AoMobys`: the mobys the apply pass leaves alone (module docs), this frame.
#[derive(Resource, Clone, Copy, Default, ShaderType, ExtractResource)]
struct AoMobys {
    /// Bevy world → the moby's box as [-1, 1]³.
    boxes: [Mat4; MOBYS],
    /// The fade's width per box axis, as a share of the half extent (≤ 0.5).
    fade: [Vec4; MOBYS],
    count: u32,
}

/// Per class (`o_class`): its bind-pose bounds in packed vertex units (min, max) and whether it animates. Rebuilt with
/// the level.
#[derive(Default)]
struct ClassBoxes {
    of: std::collections::HashMap<i32, (Vec3, Vec3, bool)>,
}

impl ClassBoxes {
    fn new(level: &crate::level_load::LoadedLevel) -> Self {
        let mut of = std::collections::HashMap::new();
        for c in &level.mobys.classes {
            let mut lo = Vec3::splat(f32::MAX);
            let mut hi = Vec3::splat(f32::MIN);
            for v in c.class.high_lod.iter().flat_map(|s| &s.vertices) {
                let p = Vec3::from(v.packed_position().map(|x| x as f32));
                lo = lo.min(p);
                hi = hi.max(p);
            }
            if lo.x <= hi.x { of.insert(c.o_class, (lo, hi, c.class.header.joint_count > 1)); }
        }
        ClassBoxes { of }
    }
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
fn collect_mobys(
    play: Option<Res<Play>>,
    level: Option<Res<crate::Level>>,
    g: Option<Res<GraphicsSettings>>,
    cams: Query<&Transform, With<FlyCam>>,
    mut boxes: Local<ClassBoxes>,
    mut out: ResMut<AoMobys>,
) {
    let mut v = AoMobys::default();
    if let Some(l) = level.as_ref().filter(|l| l.is_changed()) { *boxes = ClassBoxes::new(&l.0); }
    if let (Some(p), Some(eye), true) = (play, cams.iter().next().map(|t| t.translation), g.is_some_and(|g| g.ao.on())) {
        let mut near: Vec<(f32, Mat4, Vec4)> = p
            .game
            .mobys
            .mobys
            .iter()
            // MobyProc's skips (deleted, mode bit 1): Ratchet's moby stays `visible` while hidden (in the Clank
            // sections it stands at Clank's feet and its box lifted the grass around him, session 2026-10-30_23-50-16).
            .filter(|m| m.visible != 0 && !m.is_deleted() && m.mode & rc_game::moby_runtime::mode::HIDDEN == 0)
            .filter_map(|m| {
                let r = m.bsphere[3] / 1024.0;
                if !(r > 0.0 && r <= MOBY_MAX_RADIUS) { return None; }
                let c = game_to_bevy([m.bsphere[0] / 1024.0, m.bsphere[1] / 1024.0, m.bsphere[2] / 1024.0]);
                let d = c.distance(eye) - r;
                if d >= MAX_DISTANCE { return None; }
                let &(lo, hi, animated) = boxes.of.get(&(m.o_class as i32))?;
                // The model matrix of the moby's draw (crate::moby_render `spawn_mobys`, `MobyOcclusion::drive`).
                let rows = Mat3::from_cols(Vec3::from_slice(&m.rows[0]), Vec3::from_slice(&m.rows[1]), Vec3::from_slice(&m.rows[2]));
                let model = Mat4::from_mat3_translation(crate::moby_render::axes() * rows * (m.scale / 1024.0), game_to_bevy([m.position[0], m.position[1], m.position[2]]));
                // World units per packed unit on each box axis; the pad in packed units, none under the feet.
                let k = Vec3::new(model.col(0).truncate().length(), model.col(1).truncate().length(), model.col(2).truncate().length());
                if k.min_element() <= 1e-9 { return None; }
                let pad = if animated { Vec3::splat(PAD) / k } else { Vec3::ZERO };
                let (lo, hi) = (lo - pad.with_z(0.0), hi + pad);
                let half_w = k * (hi - lo) * 0.5;
                if half_w.min_element() <= 1e-4 { return None; }
                let box_from_unit = model * Mat4::from_scale_rotation_translation((hi - lo) * 0.5, Quat::IDENTITY, (lo + hi) * 0.5);
                Some((d, box_from_unit.inverse(), (Vec3::splat(FADE) / half_w).min(Vec3::splat(0.5)).extend(0.0)))
            })
            .collect();
        near.sort_by(|a, b| a.0.total_cmp(&b.0));
        for (i, &(_, b, f)) in near.iter().take(MOBYS).enumerate() {
            v.boxes[i] = b;
            v.fade[i] = f;
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
