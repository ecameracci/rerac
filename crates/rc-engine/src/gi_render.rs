//! **Port-only: the global illumination of Lighting Enhanced** (gated on crate::graphics
//! `GraphicsSettings::lighting.enhanced()`): one bounce of light between nearby surfaces, so a red wall tints the floor
//! beside it and a lit floor lightens the underside of an overhang.
//!
//! The game's world colours are baked vertex lighting times textures (no albedo buffer, only the lit colour), so the
//! bounce is the lit colour of the surfaces around a pixel. Standard screen-space one-bounce GI
//! (`assets/shaders/gi.wgsl`), after the opaque world, its shadows and the ambient occlusion (crate::ao_render), before
//! the scene copy (crate::scene_copy: the water's bed shows it) and the transparent pass (water and effects take none):
//! 1. **Cells** (one per [`CELL`]×[`CELL`] target pixels): the view normal and depth of each cell's centre pixel
//!    (crate::fx_view, from the depth buffer), then the cell's lit colour (linear: its pixels at that depth, averaged).
//! 2. **Bounce:** per cell, [`TAPS`] cells on a golden-angle spiral disc of [`RADIUS`] world units projected to the
//!    grid, denser near the centre, rotated per cell by a fixed noise (stable from frame to frame). Each tap stands
//!    for its ring's share of the disc, as world area at the sender (a cell's size at its depth over its slant to the
//!    view: at a third-person camera's grazing angles the ground near an object's foot is a thin strip on screen, and
//!    each of its cells covers much ground) and sends its colour by the disc-to-point form factor
//!    `A·cos θr·cos θs / (πd² + A)`; no visibility test.
//! 3. **Blur:** 7 cells across, then 7 down, weighted by depth and normal (the noise averages out without flicker).
//! 4. **Composite:** per pixel the four nearest cells (bilinear, depth-aware), × [`STRENGTH`], faded out between 100
//!    and [`MAX_DISTANCE`] units like the occlusion, × the gain that turns the receiver's lit colour into a
//!    reflectance (0.5 / its luminance, at most 3: the dark underside of an overhang takes more); blended
//!    `dst · (1 + bounce)` ([`BRIGHTEN`]), so the bounce takes the receiving surface's own colour. The sky takes none.
//!
//! With Lighting Original or Simple nothing runs (no [`GiTargets`] on the view). Dev switches, read at start:
//! `RC_GI=0` off; `RC_GI_STRENGTH=<f>` over [`STRENGTH`]; `RC_GI_DEBUG=1` the bounce light alone in place of the frame
//! (black where none; the draws after the pass still show over it); `RC_GI_CELL=2` half-resolution cells (default 4:
//! quarter).
//!
//! **Limits** (docs/plan/gaps.md G-REN-080, G-REN-081): only what is on screen sends, and nothing tests whether a
//! sender is hidden from the receiver (light can pass through a thin wall between them); the draws after the pass
//! (water, blended draws, effects, glows) neither send nor receive; mobys send and receive like the world.

use crate::ao_render::AoSet;
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
    BindGroup, BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, BlendComponent, BlendFactor, BlendOperation,
    BlendState, Extent3d, LoadOp, Operations, PipelineCache, RenderPassColorAttachment, RenderPassDescriptor, RenderPipeline,
    ShaderStages, StoreOp, TextureDescriptor, TextureDimension, TextureFormat, TextureSampleType, TextureUsages, TextureView,
};
use bevy::render::renderer::{RenderContext, RenderDevice, ViewQuery};
use bevy::render::texture::{CachedTexture, TextureCache};
use bevy::render::view::{ExtractedView, ViewDepthTexture, ViewTarget};
use bevy::render::{Render, RenderApp, RenderStartup, RenderSystems};
use bevy::shader::Shader;

const SHADER_PATH: &str = "shaders/gi.wgsl";
/// The cells' view normal (xyz) and depth (w; 0: none).
const GEOM_FORMAT: TextureFormat = TextureFormat::Rgba32Float;
/// The cells' lit colour and bounce light.
const LIGHT_FORMAT: TextureFormat = TextureFormat::Rgba16Float;
/// A cell's side in target pixels (quarter resolution).
const CELL: u32 = 4;
/// Spiral taps per cell.
const TAPS: u32 = 16;
/// The gather radius (world units: a metre is about one).
const RADIUS: f32 = 3.0;
/// The bounce light's scale (over the form factors' physical 1: visible but subtle on Novalis, Kerwan and Eudora).
const STRENGTH: f32 = 3.0;
/// No bounce beyond this distance (world units).
pub const MAX_DISTANCE: f32 = 150.0;

/// `dst · (1 + src)` on colour, the destination alpha kept: the bounce brightening the frame toward its own colour.
const BRIGHTEN: BlendState = BlendState {
    color: BlendComponent { src_factor: BlendFactor::Dst, dst_factor: BlendFactor::One, operation: BlendOperation::Add },
    alpha: BlendComponent { src_factor: BlendFactor::Zero, dst_factor: BlendFactor::One, operation: BlendOperation::Add },
};

/// The render-world set of the passes.
#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct GiSet;

/// The dev switches (module docs), read at start.
#[derive(Resource, Clone, Copy)]
struct GiConfig {
    on: bool,
    strength: f32,
    debug: bool,
    cell: u32,
}

impl GiConfig {
    fn from_env() -> Self {
        let var = |k: &str| std::env::var(k).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
        GiConfig {
            on: var("RC_GI").is_none_or(|v| v != "0"),
            strength: var("RC_GI_STRENGTH").and_then(|v| v.parse().ok()).unwrap_or(STRENGTH),
            debug: var("RC_GI_DEBUG").is_some_and(|v| v == "1"),
            cell: var("RC_GI_CELL").and_then(|v| v.parse().ok()).filter(|&c| (1..=8).contains(&c)).unwrap_or(CELL),
        }
    }
}

/// The view's cell images this frame (only while the option is on).
#[derive(Component)]
struct GiTargets {
    geom: CachedTexture,
    colour: CachedTexture,
    bounce: CachedTexture,
    blur: CachedTexture,
}

pub struct GiPlugin;

impl Plugin for GiPlugin {
    fn build(&self, app: &mut App) {
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else { return };
        render_app
            .insert_resource(GiConfig::from_env())
            .add_systems(RenderStartup, init_pipelines)
            .add_systems(Render, prepare.in_set(RenderSystems::PrepareResources))
            .add_systems(
                Core3d,
                gi_pass
                    .after(main_opaque_pass_3d)
                    .after(ShadowPassSet)
                    .after(SunShadowSet)
                    .after(AoSet)
                    .before(main_transparent_pass_3d)
                    .in_set(Core3dSystems::MainPass)
                    .in_set(GiSet),
            );
    }
}

#[derive(Resource)]
struct GiPipelines {
    /// Every pass: view, depth, colour input, cell geometry, bounce input; × (single-sampled depth, multisampled).
    layout: [BindGroupLayoutDescriptor; 2],
    shader: Handle<Shader>,
    /// (entry point, depth multisampled, target samples, target format, blended).
    pipes: Pipelines<(&'static str, bool, u32, TextureFormat, bool)>,
}

fn init_pipelines(mut commands: Commands, assets: Res<AssetServer>, fullscreen: Res<FullscreenShader>) {
    let tex = || texture_2d(TextureSampleType::Float { filterable: false });
    let layout = |ms: bool| {
        BindGroupLayoutDescriptor::new("gi", &BindGroupLayoutEntries::sequential(ShaderStages::FRAGMENT, (uniform_buffer::<FxView>(false), depth_entry(ms), tex(), tex(), tex())))
    };
    commands.insert_resource(GiPipelines { layout: [layout(false), layout(true)], shader: assets.load(SHADER_PATH), pipes: Pipelines::new(&fullscreen) });
}

/// The grid of a target: one cell per `cell`×`cell` pixels, the edges' partial cells included.
fn grid_size(target: UVec2, cell: u32) -> UVec2 { UVec2::new(target.x.div_ceil(cell), target.y.div_ceil(cell)).max(UVec2::ONE) }

fn prepare(
    mut commands: Commands,
    g: Option<Res<GraphicsSettings>>,
    cfg: Res<GiConfig>,
    device: Res<RenderDevice>,
    mut textures: ResMut<TextureCache>,
    views: Query<(Entity, &ExtractedCamera), With<ShadowCamera>>,
) {
    let on = cfg.on && g.is_some_and(|g| g.lighting.enhanced());
    for (e, cam) in &views {
        let Some(size) = cam.physical_target_size.filter(|_| on) else {
            commands.entity(e).remove::<GiTargets>();
            continue;
        };
        let grid = grid_size(size, cfg.cell);
        let mut image = |label: &'static str, format: TextureFormat| {
            textures.get(
                &device,
                TextureDescriptor {
                    label: Some(label),
                    size: Extent3d { width: grid.x, height: grid.y, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: TextureDimension::D2,
                    format,
                    usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                },
            )
        };
        let t = GiTargets {
            geom: image("gi geometry", GEOM_FORMAT),
            colour: image("gi colour", LIGHT_FORMAT),
            bounce: image("gi bounce", LIGHT_FORMAT),
            blur: image("gi blur", LIGHT_FORMAT),
        };
        commands.entity(e).insert(t);
    }
}

/// One full-screen draw into a cell image.
fn cell_pass(ctx: &mut RenderContext, label: &'static str, target: &TextureView, pipe: &RenderPipeline, group: &BindGroup) {
    let mut pass = ctx.begin_tracked_render_pass(RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[Some(RenderPassColorAttachment {
            view: target,
            depth_slice: None,
            resolve_target: None,
            ops: Operations { load: LoadOp::Clear(Default::default()), store: StoreOp::Store },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    pass.set_render_pipeline(pipe);
    pass.set_bind_group(0, group, &[]);
    pass.draw(0..3, 0..1);
}

fn gi_pass(
    view: ViewQuery<(&ExtractedCamera, &ExtractedView, &ViewTarget, &ViewDepthTexture, &GiTargets), With<ShadowCamera>>,
    pipes: Option<Res<GiPipelines>>,
    cfg: Res<GiConfig>,
    cache: Res<PipelineCache>,
    mut ctx: RenderContext,
) {
    let Some(pipes) = pipes else { return };
    let (cam, ev, target, depth, t) = view.into_inner();
    let Some(mut fx) = FxView::of(cam, ev) else { return };
    fx.params = Vec4::new(RADIUS, TAPS as f32, cfg.strength, MAX_DISTANCE);
    fx.extra = Vec4::new(cfg.cell as f32, if cfg.debug { 1.0 } else { 0.0 }, 0.0, 0.0);
    let (samples, format) = fx_view::target_of(target);
    let ms = depth.texture.sample_count() > 1;
    let pipe = |entry: &'static str, samples: u32, format: TextureFormat, blend: Option<BlendState>| {
        pipes.pipes.get(&cache, (entry, ms, samples, format, blend.is_some()), || Fullscreen {
            label: entry,
            layout: pipes.layout[ms as usize].clone(),
            shader: pipes.shader.clone(),
            entry,
            defs: depth_defs(ms),
            format,
            blend,
            samples,
        })
    };
    let (Some(geometry), Some(colour), Some(bounce), Some(blur_x), Some(blur_y), Some(composite)) = (
        pipe("geometry", 1, GEOM_FORMAT, None),
        pipe("colour", 1, LIGHT_FORMAT, None),
        pipe("bounce", 1, LIGHT_FORMAT, None),
        pipe("blur_x", 1, LIGHT_FORMAT, None),
        pipe("blur_y", 1, LIGHT_FORMAT, None),
        pipe("composite", samples, format, (!cfg.debug).then_some(BRIGHTEN)),
    ) else {
        return;
    };
    let device = ctx.render_device().clone();
    let buf = fx.buffer(&device, "gi view");
    let layout = cache.get_bind_group_layout(&pipes.layout[ms as usize]);
    // No pass binds the image it draws into.
    let group = |src: &TextureView, geom: &TextureView, aux: &TextureView| {
        device.create_bind_group("gi", &layout, &BindGroupEntries::sequential((buf.as_entire_binding(), depth.view(), src, geom, aux)))
    };
    let frame = target.main_texture_view();
    let (g, c, b, x) = (&t.geom.default_view, &t.colour.default_view, &t.bounce.default_view, &t.blur.default_view);
    cell_pass(&mut ctx, "gi geometry", g, geometry, &group(frame, c, b));
    cell_pass(&mut ctx, "gi colour", c, colour, &group(frame, g, b));
    cell_pass(&mut ctx, "gi bounce", b, bounce, &group(c, g, x));
    cell_pass(&mut ctx, "gi blur across", x, blur_x, &group(c, g, b));
    cell_pass(&mut ctx, "gi blur down", b, blur_y, &group(c, g, x));
    let composite_group = group(c, g, b);
    let mut pass = ctx.begin_tracked_render_pass(RenderPassDescriptor {
        label: Some("gi composite"),
        color_attachments: &[Some(target.get_color_attachment())],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    if let Some(v) = &cam.viewport { pass.set_camera_viewport(v); }
    pass.set_render_pipeline(composite);
    pass.set_bind_group(0, &composite_group, &[]);
    pass.draw(0..3, 0..1);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// gi.wgsl's `spiral`, `form_factor`, the bounce's tap area and the composite's fade, mirrored.
    mod model {
        use bevy::math::{Vec2, Vec3};
        use std::f32::consts::PI;

        const GOLDEN: f32 = 2.399_963_2;

        pub fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
            let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
            t * t * (3.0 - 2.0 * t)
        }

        pub fn spiral(i: u32, taps: u32, rot: f32) -> Vec2 {
            let t = (i as f32 + 0.5) / taps as f32;
            let a = rot + i as f32 * GOLDEN;
            Vec2::new(a.cos(), a.sin()) * t
        }

        /// The share of the disc's cells tap i stands for (2t / taps).
        pub fn share(i: u32, taps: u32) -> f32 { 2.0 * (i as f32 + 0.5) / (taps * taps) as f32 }

        pub fn form_factor(nr: Vec3, ns: Vec3, v: Vec3, area: f32, radius: f32) -> f32 {
            let d2 = v.length_squared();
            if d2 < 1e-8 { return 0.0; }
            let d = d2.sqrt();
            let dir = v / d;
            let cr = (nr.dot(dir) - 0.05).max(0.0);
            let cs = (-ns.dot(dir) - 0.05).max(0.0);
            area * cr * cs / (PI * d2 + area) * (1.0 - smoothstep(0.75 * radius, radius, d))
        }

        /// The world area tap i stands for: its share of a disc of `r_cells` cells, at a sender at view position `ps`
        /// with normal `ns` (`focal`: target pixels per unit at depth 1).
        pub fn tap_area(i: u32, r_cells: f32, taps: u32, cell: f32, focal: f32, ps: Vec3, ns: Vec3) -> f32 {
            let side = cell * -ps.z / focal;
            PI * r_cells * r_cells * share(i, taps) * side * side / ns.dot(ps.normalize()).abs().max(0.2)
        }

        pub fn fade(z: f32, far: f32) -> f32 { 1.0 - smoothstep(far * 0.66, far, z) }
    }
    use model::*;

    #[test]
    fn spiral_fills_the_disc_denser_near_the_centre() {
        let taps: Vec<Vec2> = (0..TAPS).map(|i| spiral(i, TAPS, 0.7)).collect();
        for (i, o) in taps.iter().enumerate() {
            let r = (i as f32 + 0.5) / TAPS as f32;
            assert!((o.length() - r).abs() < 1e-5 && o.length() < 1.0, "tap {i}: {o}");
        }
        // Half the taps within half the radius (a quarter of the area); their shares still cover the disc once,
        // each in proportion to its ring.
        assert_eq!(taps.iter().filter(|o| o.length() < 0.5).count() as u32, TAPS / 2);
        assert!(((0..TAPS).map(|i| share(i, TAPS)).sum::<f32>() - 1.0).abs() < 1e-5);
        let inner: f32 = (0..TAPS / 2).map(|i| share(i, TAPS)).sum();
        assert!((inner - 0.25).abs() < 1e-5, "{inner}");
        // Golden-angle steps: no two taps share a direction, and every quadrant is hit.
        for a in 0..taps.len() {
            for b in a + 1..taps.len() {
                assert!(taps[a].normalize().dot(taps[b].normalize()) < 0.999, "taps {a} and {b} line up");
            }
        }
        for (sx, sy) in [(1.0, 1.0), (-1.0, 1.0), (-1.0, -1.0), (1.0, -1.0)] {
            assert!(taps.iter().any(|o| o.x * sx > 0.0 && o.y * sy > 0.0), "quadrant {sx},{sy} empty");
        }
        // The rotation turns the whole pattern rigidly.
        let turned = spiral(5, TAPS, 0.7 + 1.0);
        let base = spiral(5, TAPS, 0.7);
        assert!((turned.length() - base.length()).abs() < 1e-6);
        assert!((base.normalize().dot(turned.normalize()) - 1f32.cos()).abs() < 1e-5);
    }

    #[test]
    fn form_factor_needs_surfaces_that_face_each_other() {
        let (up, a) = (Vec3::Y, 0.1);
        // A floor point and a patch of wall facing it (−x) 1 unit across and up: both cosines 1/√2.
        let wall = form_factor(up, -Vec3::X, Vec3::new(1.0, 1.0, 0.0), a, RADIUS);
        let expect = a * (0.5f32.sqrt() - 0.05).powi(2) / (std::f32::consts::PI * 2.0 + a);
        assert!((wall - expect).abs() < 1e-6, "{wall} {expect}");
        // The same floor: nothing (a flat surface does not light itself).
        assert_eq!(form_factor(up, up, Vec3::new(1.0, 0.0, 0.0), a, RADIUS), 0.0);
        // A wall turned away, or a sender behind the receiver: nothing.
        assert_eq!(form_factor(up, Vec3::X, Vec3::new(1.0, 1.0, 0.0), a, RADIUS), 0.0);
        assert_eq!(form_factor(up, -Vec3::X, Vec3::new(1.0, -1.0, 0.0), a, RADIUS), 0.0);
        // Falls off with the distance and stops at the radius; never more than all of the light, however large or near.
        let dir = Vec3::new(1.0, 1.0, 0.0).normalize();
        let near = form_factor(up, -Vec3::X, dir * 0.5, a, RADIUS);
        let mid = form_factor(up, -Vec3::X, dir * 2.0, a, RADIUS);
        assert!(near > mid && mid > 0.0, "{near} {mid}");
        assert_eq!(form_factor(up, -Vec3::X, dir * RADIUS, a, RADIUS), 0.0);
        assert_eq!(form_factor(up, -Vec3::X, Vec3::ZERO, a, RADIUS), 0.0);
        assert!(form_factor(up, -up, Vec3::Y * 1e-3, 1e6, RADIUS) <= 1.0);
    }

    #[test]
    fn taps_stand_for_more_ground_far_away_and_at_a_slant() {
        let (focal, cell, at) = (500.0, CELL as f32, Vec3::new(0.0, 0.0, -10.0));
        let facing = tap_area(3, 8.0, TAPS, cell, focal, at, Vec3::Z);
        // Twice as far: each cell covers four times the area.
        let far = tap_area(3, 8.0, TAPS, cell, focal, at * 2.0, Vec3::Z);
        assert!((far / facing - 4.0).abs() < 1e-4, "{}", far / facing);
        // Ground seen at a grazing angle: up to five times the area.
        let slant = tap_area(3, 8.0, TAPS, cell, focal, at, Vec3::new(0.0, 0.995, 0.1).normalize());
        assert!((slant / facing - 5.0).abs() < 1e-3, "{}", slant / facing);
        // The taps together stand for the disc's cells.
        let side = cell * 10.0 / focal;
        let all: f32 = (0..TAPS).map(|i| tap_area(i, 8.0, TAPS, cell, focal, at, Vec3::Z)).sum();
        assert!((all - std::f32::consts::PI * 64.0 * side * side).abs() < 1e-4, "{all}");
    }

    #[test]
    fn fade_spans_the_last_third_of_the_distance() {
        assert_eq!(fade(10.0, MAX_DISTANCE), 1.0);
        assert_eq!(fade(MAX_DISTANCE * 0.66, MAX_DISTANCE), 1.0);
        assert_eq!(fade(MAX_DISTANCE, MAX_DISTANCE), 0.0);
        let mut last = 1.0;
        for z in (90..=150).map(|z| z as f32) {
            let f = fade(z, MAX_DISTANCE);
            assert!(f <= last);
            last = f;
        }
    }

    #[test]
    fn grid_covers_the_target() {
        assert_eq!(grid_size(UVec2::new(1920, 1080), 4), UVec2::new(480, 270));
        assert_eq!(grid_size(UVec2::new(1921, 1083), 4), UVec2::new(481, 271));
        assert_eq!(grid_size(UVec2::new(1, 1), 4), UVec2::ONE);
        assert_eq!(grid_size(UVec2::new(1280, 720), 2), UVec2::new(640, 360));
    }
}
