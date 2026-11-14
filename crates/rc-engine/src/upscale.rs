//! **Port-only: the Upscaler option** (crate::graphics [`Upscaler`], docs/plan/graphics_options.md): the 3D picture
//! drawn below the frame's resolution and upscaled to it, FSR 1-style (`assets/shaders/upscale.wgsl`); the HUD,
//! menus, movies and fades stay at the frame's resolution.
//!
//! **Cameras.** With the option Off nothing here runs: the world cameras draw into the frame (crate::display) as
//! before. On, every camera that draws into the frame before [`COMPOSE_ORDER`] (the sky and main cameras; the HUD,
//! menu and canvas cameras have their own images) is pointed at [`Upscale::world`], an image of
//! [`Upscaler::render_size`] of the frame, and the compose camera ([`ComposeCamera`], a `Camera2d` drawing nothing of its
//! own: a `Camera3d` would be taken for the main camera by the systems that query "the" 3D camera) draws into the frame:
//! 1. **EASU** (edge-adaptive spatial upsampling) of the world image into a full-size `Rgba16Float` texture: per output
//!    pixel the 12 nearest source texels, a direction and an edge length from their luma over the 2×2 around the
//!    sample, and a Lanczos-2-like kernel stretched along the edge, clamped to the 2×2's range (no ringing).
//! 2. **RCAS** (robust contrast-adaptive sharpening) of that into the frame: a negative lobe from the cross's min / max,
//!    limited so it never clips; the strength [`SHARPNESS`] stops (`RC_UPSCALE_SHARPNESS`; `RC_UPSCALE_RCAS=0` none).
//!    Both work on gamma-2 values (√ of the linear colour), as AMD's notes ask.
//! 3. The UI pass (the HUD composite, the menu layer, movies: their nodes are moved to it, [`follow`]), then the passes
//!    the game draws after the HUD, moved from the main camera's render entity to the compose camera's
//!    ([`move_after_hud`]): the missile view overlay, the underwater tint and the fade, in the main camera's order.
//!
//! The menu snapshot and screenshots copy the frame, so they include the upscaled picture. Lighting Enhanced's own
//! sharpening (crate::post_render) runs on the world image before the upscale.
//!
//! **Licence.** EASU and RCAS follow AMD's FidelityFX Super Resolution 1.0 (`ffx_fsr1.h`, MIT licence, © 2021 Advanced
//! Micro Devices): the tap pattern, the direction / length analysis, the kernel's lobe and clip, the dering clamp and
//! RCAS's lobe limit (`FSR_RCAS_LIMIT`) are AMD's, rewritten in WGSL here (no AMD source is vendored).
//!
//! DLSS is not offered: Bevy's own (`bevy_anti_alias::dlss`) needs the `dlss` cargo feature and NVIDIA's SDK
//! (docs/plan/graphics_options.md "DLSS").

use crate::display::GameFrame;
use crate::fly_cam::FlyCam;
use crate::fx_view::{Fullscreen, Pipelines};
use crate::graphics::{GraphicsSettings, Upscaler};
use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::RenderLayers;
use bevy::camera::{ClearColorConfig, RenderTarget};
use bevy::core_pipeline::tonemapping::{DebandDither, Tonemapping};
use bevy::core_pipeline::{Core2d, Core2dSystems, FullscreenShader};
use bevy::prelude::*;
use bevy::render::camera::ExtractedCamera;
use bevy::render::extract_component::{ExtractComponent, ExtractComponentPlugin};
use bevy::render::extract_resource::{ExtractResource, ExtractResourcePlugin};
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_resource::binding_types::{texture_2d, uniform_buffer};
use bevy::render::render_resource::{
    BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, Extent3d, LoadOp, Operations, PipelineCache,
    RenderPassColorAttachment, RenderPassDescriptor, ShaderStages, ShaderType, StoreOp, TextureDescriptor, TextureDimension,
    TextureFormat, TextureSampleType, TextureUsages,
};
use bevy::render::diagnostic::RecordDiagnostics;
use bevy::render::renderer::{RenderContext, RenderDevice, ViewQuery};
use bevy::render::texture::{CachedTexture, GpuImage, TextureCache};
use bevy::render::view::{Msaa, ViewTarget};
use bevy::render::{Render, RenderApp, RenderStartup, RenderSystems};
use bevy::shader::Shader;
use bevy::ui::{IsDefaultUiCamera, UiTargetCamera};

const SHADER_PATH: &str = "shaders/upscale.wgsl";
/// The EASU result (gamma-2 values; RCAS reads it).
const MID_FORMAT: TextureFormat = TextureFormat::Rgba16Float;
/// The compose camera's order: after every camera that draws into the frame (the world: −1, 0), before the present
/// camera (crate::display, 1000). A camera on the frame below it is a world camera.
pub const COMPOSE_ORDER: isize = 100;
const _: () = assert!(COMPOSE_ORDER > 0 && COMPOSE_ORDER < 1000);
/// The compose camera's own layer (nothing draws in it).
const COMPOSE_LAYER: usize = 62;
/// RCAS's strength in stops (AMD's `FsrRcasCon`: 0 the strongest, each stop halves it); AMD's sample uses 0.2.
pub const SHARPNESS: f32 = 0.2;

/// The upscaler's state: the world image while it is on.
#[derive(Resource, Clone, Default, ExtractResource)]
pub struct Upscale {
    /// The image the world cameras draw into (None: Off).
    pub world: Option<Handle<Image>>,
    /// Its size.
    pub size: UVec2,
    /// RCAS's lobe scale (`exp2(−stops)`; 0: no sharpening).
    pub sharpen: f32,
}

/// The camera that upscales the world into the frame and draws the UI over it (module docs).
#[derive(Component, Clone, Copy, ExtractComponent)]
pub struct ComposeCamera;

/// On the main camera while its picture is upscaled: its after-HUD passes move to the compose camera.
#[derive(Component, Clone, Copy, ExtractComponent)]
pub struct UpscaledWorld;

/// On a camera this module pointed at the world image (pointed back at the frame when the option goes Off).
#[derive(Component)]
struct OnWorldImage;

/// RCAS's lobe scale from the switches (`RC_UPSCALE_SHARPNESS=<stops>`, `RC_UPSCALE_RCAS=0`).
fn sharpen_from_env(var: impl Fn(&str) -> Option<String>) -> f32 {
    if var("RC_UPSCALE_RCAS").is_some_and(|v| v.trim() == "0") { return 0.0; }
    let stops = var("RC_UPSCALE_SHARPNESS").and_then(|v| v.trim().parse::<f32>().ok()).filter(|v| v.is_finite()).unwrap_or(SHARPNESS);
    (-stops.max(0.0)).exp2()
}

pub struct UpscalePlugin;

impl Plugin for UpscalePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Upscale { sharpen: sharpen_from_env(|k| std::env::var(k).ok()), ..default() })
            .add_plugins((
                ExtractResourcePlugin::<Upscale>::default(),
                ExtractComponentPlugin::<ComposeCamera>::default(),
                ExtractComponentPlugin::<UpscaledWorld>::default(),
            ))
            .add_systems(Startup, spawn_compose)
            .add_systems(PostUpdate, follow.before(bevy::camera::CameraUpdateSystems));
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else { return };
        use crate::fog_state::UnderwaterTint;
        use crate::gs_post::pass;
        use crate::scene_render::SceneFade;
        use crate::visibomb_view::MissileViewOverlay;
        render_app
            .add_systems(RenderStartup, init_pipelines)
            .add_systems(Render, (move_after_hud.in_set(RenderSystems::PrepareViews), prepare.in_set(RenderSystems::PrepareResources)))
            .add_systems(
                Core2d,
                (
                    upscale_pass.after(Core2dSystems::PostProcess).before(bevy::ui_render::ui_pass),
                    // The main camera's after-HUD passes, in its order (crate::visibomb_view, crate::fog_state,
                    // crate::scene_render).
                    (pass::<MissileViewOverlay>, pass::<UnderwaterTint>, pass::<SceneFade>)
                        .chain()
                        .after(bevy::ui_render::ui_pass)
                        .before(bevy::core_pipeline::upscaling::upscaling),
                ),
            );
    }
}

fn spawn_compose(mut commands: Commands, frame: Res<GameFrame>) {
    commands.spawn((
        Camera2d,
        Camera { order: COMPOSE_ORDER, is_active: false, clear_color: ClearColorConfig::Custom(Color::BLACK), ..default() },
        RenderTarget::Image(frame.image.clone().into()),
        Msaa::Off,
        Tonemapping::None,
        DebandDither::Disabled,
        RenderLayers::layer(COMPOSE_LAYER),
        ComposeCamera,
        crate::level_switch::KeepAcrossLevels,
        Name::new("compose camera (upscaled world + UI → frame)"),
    ));
}

/// A new world image of `size`, the frame's format (crate::display).
fn world_image(size: UVec2) -> Image {
    let mut img = Image::new_target_texture(size.x, size.y, TextureFormat::Bgra8UnormSrgb, None);
    img.asset_usage = RenderAssetUsages::RENDER_WORLD;
    img.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    img
}

/// The option onto the cameras (module docs): the world image's size, which cameras draw into it, the compose camera,
/// the UI's camera.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn follow(
    mut commands: Commands,
    gfx: Res<GraphicsSettings>,
    frame: Res<GameFrame>,
    mut up: ResMut<Upscale>,
    mut images: ResMut<Assets<Image>>,
    mut cams: Query<(Entity, &mut Camera, &mut RenderTarget, Has<ComposeCamera>, Has<OnWorldImage>), Without<crate::display::PresentCamera>>,
    main: Query<(Entity, Has<IsDefaultUiCamera>, Has<UpscaledWorld>), With<FlyCam>>,
    mut nodes: Query<&mut UiTargetCamera>,
    mut logged: Local<Option<(Upscaler, UVec2)>>,
) {
    let mode = gfx.upscaler;
    let compose = cams.iter().find(|c| c.3).map(|c| c.0);
    let on = mode.on() && compose.is_some() && main.iter().next().is_some();
    let size = mode.render_size(frame.size);
    if *logged != Some((mode, frame.size)) {
        *logged = Some((mode, frame.size));
        if on { println!("upscale: {mode:?}: the 3D drawn at {}×{}, upscaled to the frame's {}×{}", size.x, size.y, frame.size.x, frame.size.y); }
    }
    // A fresh image per size (a target resized in place keeps a stale render-world size, crate::display::fresh_target).
    match (on, &up.world) {
        (true, Some(_)) if up.size == size => {}
        (true, _) => {
            up.world = Some(images.add(world_image(size)));
            up.size = size;
        }
        (false, Some(_)) => up.world = None,
        (false, None) => {}
    }
    let frame_target = RenderTarget::Image(frame.image.clone().into());
    let world_target = up.world.clone().map(|h| RenderTarget::Image(h.into()));
    for (e, mut cam, mut target, is_compose, moved) in &mut cams {
        if is_compose {
            if cam.is_active != on { cam.is_active = on; }
            continue;
        }
        let want = match (&world_target, moved) {
            (Some(w), true) => w,
            (Some(w), false) if target.as_image() == Some(&frame.image) && cam.order < COMPOSE_ORDER => {
                commands.entity(e).insert(OnWorldImage);
                w
            }
            (None, true) => {
                commands.entity(e).remove::<OnWorldImage>();
                &frame_target
            }
            _ => continue,
        };
        if target.as_image() != want.as_image() { *target = want.clone(); }
    }
    // The UI (and the after-HUD passes' marker) follow the picture's last camera.
    let (Some(compose), Some((main, main_default, marked))) = (compose, main.iter().next()) else { return };
    let (from, to) = if on { (main, compose) } else { (compose, main) };
    if on != marked {
        if on { commands.entity(main).insert(UpscaledWorld); } else { commands.entity(main).remove::<UpscaledWorld>(); }
    }
    if main_default == on {
        commands.entity(from).remove::<IsDefaultUiCamera>();
        commands.entity(to).insert(IsDefaultUiCamera);
    }
    for mut n in &mut nodes {
        if n.0 == from { n.0 = to; }
    }
}

/// The passes the game draws after the HUD, moved from the upscaled main camera's render entity to the compose
/// camera's (they are extracted onto the main camera every frame); the missile overlay's rectangle scaled to the frame.
#[allow(clippy::type_complexity)]
fn move_after_hud(
    mut commands: Commands,
    main: Query<(Entity, &ExtractedCamera, Option<&crate::fog_state::UnderwaterTint>, Option<&crate::scene_render::SceneFade>, Option<&crate::visibomb_view::MissileViewOverlay>), With<UpscaledWorld>>,
    compose: Query<(Entity, &ExtractedCamera), With<ComposeCamera>>,
) {
    let Some((c, ccam)) = compose.iter().next() else { return };
    let Some((m, mcam, tint, fade, overlay)) = main.iter().next() else {
        commands.entity(c).remove::<(crate::fog_state::UnderwaterTint, crate::scene_render::SceneFade, crate::visibomb_view::MissileViewOverlay)>();
        return;
    };
    let scale = match (ccam.physical_target_size, mcam.physical_target_size) {
        (Some(f), Some(w)) => f.as_vec2() / w.max(UVec2::ONE).as_vec2(),
        _ => Vec2::ONE,
    };
    let mut to = commands.entity(c);
    match tint {
        Some(t) => { to.insert(*t); }
        None => { to.remove::<crate::fog_state::UnderwaterTint>(); }
    }
    match fade {
        Some(f) => { to.insert(*f); }
        None => { to.remove::<crate::scene_render::SceneFade>(); }
    }
    match overlay {
        Some(o) => {
            let mut o = *o;
            o.vp *= scale.extend(scale.x).extend(scale.y);
            to.insert(o);
        }
        None => { to.remove::<crate::visibomb_view::MissileViewOverlay>(); }
    }
    commands.entity(m).remove::<(crate::fog_state::UnderwaterTint, crate::scene_render::SceneFade, crate::visibomb_view::MissileViewOverlay)>();
}

/// `upscale.wgsl`'s `Upscale`.
#[derive(Clone, Copy, Debug, Default, ShaderType)]
struct UpscaleUniform {
    /// xy: the world image's size, zw: the frame's.
    size: Vec4,
    /// x: RCAS's lobe scale (0: copy).
    sharp: Vec4,
}

#[derive(Resource)]
struct UpscalePipelines {
    layout: BindGroupLayoutDescriptor,
    shader: Handle<Shader>,
    /// (RCAS, target format).
    pipes: Pipelines<(bool, TextureFormat)>,
}

fn init_pipelines(mut commands: Commands, assets: Res<AssetServer>, fullscreen: Res<FullscreenShader>) {
    let layout = BindGroupLayoutDescriptor::new(
        "upscale",
        &BindGroupLayoutEntries::with_indices(
            ShaderStages::FRAGMENT,
            ((0, uniform_buffer::<UpscaleUniform>(false)), (1, texture_2d(TextureSampleType::Float { filterable: false }))),
        ),
    );
    commands.insert_resource(UpscalePipelines { layout, shader: assets.load(SHADER_PATH), pipes: Pipelines::new(&fullscreen) });
}

/// The compose view's EASU texture (the frame's size).
#[derive(Component)]
struct UpscaleMid(CachedTexture);

fn prepare(mut commands: Commands, device: Res<RenderDevice>, mut textures: ResMut<TextureCache>, views: Query<(Entity, &ExtractedCamera), With<ComposeCamera>>) {
    for (e, cam) in &views {
        let Some(size) = cam.physical_target_size else { continue };
        let tex = textures.get(
            &device,
            TextureDescriptor {
                label: Some("upscale easu"),
                size: Extent3d { width: size.x.max(1), height: size.y.max(1), depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: TextureDimension::D2,
                format: MID_FORMAT,
                usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
        );
        commands.entity(e).insert(UpscaleMid(tex));
    }
}

fn upscale_pass(
    view: ViewQuery<(&ViewTarget, &UpscaleMid), With<ComposeCamera>>,
    up: Option<Res<Upscale>>,
    images: Res<RenderAssets<GpuImage>>,
    pipes: Option<Res<UpscalePipelines>>,
    cache: Res<PipelineCache>,
    mut ctx: RenderContext,
) {
    let (target, mid) = view.into_inner();
    let (Some(up), Some(pipes)) = (up, pipes) else { return };
    let Some(src) = up.world.as_ref().and_then(|h| images.get(h)) else { return };
    let format = target.main_texture_format();
    let fullscreen = |label, entry, format| Fullscreen { label, layout: pipes.layout.clone(), shader: pipes.shader.clone(), entry, defs: vec![], format, blend: None, samples: 1 };
    let easu = pipes.pipes.get(&cache, (false, MID_FORMAT), || fullscreen("upscale easu", "easu", MID_FORMAT));
    let rcas = pipes.pipes.get(&cache, (true, format), || fullscreen("upscale rcas", "rcas", format));
    let (Some(easu), Some(rcas)) = (easu, rcas) else { return };
    let out = UVec2::new(mid.0.texture.width(), mid.0.texture.height());
    let uniform = UpscaleUniform {
        size: Vec4::new(src.texture.width() as f32, src.texture.height() as f32, out.x as f32, out.y as f32),
        sharp: Vec4::new(up.sharpen, 0.0, 0.0, 0.0),
    };
    let device = ctx.render_device().clone();
    let buf = crate::fx_view::uniform_buffer(&device, "upscale", &uniform);
    let layout = cache.get_bind_group_layout(&pipes.layout);
    let easu_group = device.create_bind_group("upscale easu", &layout, &BindGroupEntries::with_indices(((0, buf.as_entire_binding()), (1, &src.texture_view))));
    let rcas_group = device.create_bind_group("upscale rcas", &layout, &BindGroupEntries::with_indices(((0, buf.as_entire_binding()), (1, &mid.0.default_view))));
    // Timed under `RC_RENDER_DIAG=1` (render/upscale_easu, render/upscale_rcas).
    let diagnostics = ctx.diagnostic_recorder();
    let diagnostics = diagnostics.as_deref();
    let enc = ctx.command_encoder();
    for (label, view, pipe, group) in [("upscale_easu", &mid.0.default_view, easu, &easu_group), ("upscale_rcas", target.main_texture_view(), rcas, &rcas_group)] {
        let mut rp = enc.begin_render_pass(&RenderPassDescriptor {
            label: Some(label),
            color_attachments: &[Some(RenderPassColorAttachment { view, depth_slice: None, resolve_target: None, ops: Operations { load: LoadOp::Clear(Default::default()), store: StoreOp::Store } })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        let span = diagnostics.pass_span(&mut rp, label);
        rp.set_pipeline(pipe);
        rp.set_bind_group(0, group, &[]);
        rp.draw(0..3, 0..1);
        span.end(&mut rp);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sharpness_switches() {
        let env = |pairs: &'static [(&'static str, &'static str)]| move |k: &str| pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| v.to_string());
        assert!((sharpen_from_env(env(&[])) - (-SHARPNESS).exp2()).abs() < 1e-6);
        assert_eq!(sharpen_from_env(env(&[("RC_UPSCALE_SHARPNESS", "0")])), 1.0);
        assert_eq!(sharpen_from_env(env(&[("RC_UPSCALE_SHARPNESS", "2")])), 0.25);
        assert_eq!(sharpen_from_env(env(&[("RC_UPSCALE_SHARPNESS", "-3")])), 1.0);
        assert_eq!(sharpen_from_env(env(&[("RC_UPSCALE_RCAS", "0"), ("RC_UPSCALE_SHARPNESS", "0")])), 0.0);
    }
}
