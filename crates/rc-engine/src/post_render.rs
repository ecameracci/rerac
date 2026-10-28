//! **Port-only: the image post-process of Lighting Enhanced** (gated on crate::graphics
//! `GraphicsSettings::lighting.enhanced()`, the render world's extracted copy, as crate::sun_rays): bloom, a filmic
//! grade and contrast-adaptive sharpening of the 3D picture (`assets/shaders/post.wgsl`).
//!
//! Standard techniques, on the world camera after PostProcess (so after the sun's rays, crate::sun_rays) and before
//! the screen tints, the mirror flip, the AA blit (crate::aa_blit) and the UI pass: the HUD, menus, text and movies
//! (the UI pass), the underwater tint (crate::fog_state) and the fades (crate::scene_render) are drawn after it and
//! untouched; the menu's snapshot and screenshots copy the finished frame, the effect included. Every colour is linear
//! light (the frame's `Rgba8UnormSrgb` view decodes on load and encodes on store, crate::display):
//! 1. **Prefilter** (half resolution): each 2×2 block of the frame, each pixel weighted by a soft threshold on its
//!    brightness ([`soft_threshold`]: [`THRESHOLD`], knee [`KNEE`]; the largest channel, not the luminance, so a
//!    saturated red lamp counts as bright as a white one) and the sky's (depth 0) by [`SKY_WEIGHT`] more, averaged
//!    with Karis weights 1/(1 + luma) so single bright pixels do not flicker.
//! 2. **Down:** the 13-tap filter of each level into the next half size ([`mip_sizes`], [`LEVELS`] levels).
//! 3. **Up:** each level adds a 3×3 tent of the one below it × [`SCATTER`] (additive blend), back to the first: level
//!    i weighs `SCATTER^i`, so the glow stays near its light and the wide levels add no haze.
//! 4. **Composite** (one pass into the frame): CAS of the frame ([`SHARPEN`]: the cross's min / max set a negative
//!    lobe, weaker where the local contrast is high), the bloom added (a tent of the first level ÷ the weights' sum
//!    [`level_weights`] × [`BLOOM`]), then the grade: [`tone`] (a power of [`CONTRAST`] around [`MID_GREY`], an exponential shoulder from
//!    [`SHOULDER`] toward 1: no hard white clip) and [`vibrance`] (dull colours pushed from their luminance by up to
//!    [`VIBRANCE`], saturated ones barely).
//!
//! Switches, read at start ([`PostTuning::from_env`], docs/workflows/dev-switches.md): `RC_POST=0` none of it;
//! `RC_POST_BLOOM` (strength, 0 off), `RC_POST_THRESHOLD`, `RC_POST_SKY`, `RC_POST_SCATTER`, `RC_POST_GRADE=0` (no grade),
//! `RC_POST_CONTRAST`, `RC_POST_VIBRANCE` (at most [`VIBRANCE_CAP`]), `RC_POST_SHARPEN` (CAS sharpness 0..1, 0 off).
//! With Lighting Original or Simple nothing runs.

use crate::fx_view::{depth_defs, depth_entry, Fullscreen, Pipelines};
use crate::graphics::GraphicsSettings;
use crate::shadow_render::ShadowCamera;
use bevy::core_pipeline::{Core3d, Core3dSystems, FullscreenShader};
use bevy::prelude::*;
use bevy::render::camera::ExtractedCamera;
use bevy::render::render_resource::binding_types::{sampler, texture_2d, uniform_buffer};
use bevy::render::render_resource::{
    AddressMode, BindGroup, BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, BlendComponent, BlendFactor,
    BlendOperation, BlendState, Extent3d, FilterMode, LoadOp, Operations, PipelineCache, RenderPassColorAttachment,
    RenderPassDescriptor, RenderPipeline, Sampler, SamplerBindingType, SamplerDescriptor, ShaderStages, ShaderType, StoreOp,
    TextureDescriptor, TextureDimension, TextureFormat, TextureSampleType, TextureUsages, TextureView,
};
use bevy::render::renderer::{RenderContext, RenderDevice, ViewQuery};
use bevy::render::texture::{CachedTexture, TextureCache};
use bevy::render::view::{ViewDepthTexture, ViewTarget};
use bevy::render::{Render, RenderApp, RenderStartup, RenderSystems};
use bevy::shader::Shader;

const SHADER_PATH: &str = "shaders/post.wgsl";
const BLOOM_FORMAT: TextureFormat = TextureFormat::Rgba16Float;
/// The bloom's levels: half size, then four more halvings (1/2 .. 1/32 of the frame).
pub const LEVELS: usize = 5;
/// The brightness (the largest linear channel) above which the bloom takes the excess: `b − THRESHOLD`.
pub const THRESHOLD: f32 = 0.7;
/// The soft knee below [`THRESHOLD`]: from `THRESHOLD − KNEE` the contribution rises quadratically.
pub const KNEE: f32 = 0.25;
/// The sky's pixels count at this fraction (a bright sky would wash the picture out).
pub const SKY_WEIGHT: f32 = 0.25;
/// The bloom's strength on the levels' weighted mean ([`level_weights`]).
pub const BLOOM: f32 = 2.5;
/// Each wider level's weight relative to the one above it: the glow stays near its light, no haze.
pub const SCATTER: f32 = 0.6;
/// The contrast power around [`MID_GREY`].
pub const CONTRAST: f32 = 1.06;
pub const MID_GREY: f32 = 0.18;
/// Where the shoulder starts (linear); above it values approach 1 and never clip.
pub const SHOULDER: f32 = 0.8;
/// The vibrance of a grey-ish colour (none at full saturation).
pub const VIBRANCE: f32 = 0.2;
pub const VIBRANCE_CAP: f32 = 0.5;
/// The CAS sharpness (AMD's 0..1: a negative lobe of 1/8 .. 1/5).
pub const SHARPEN: f32 = 0.2;

/// The effect's values (the constants, or the `RC_POST*` switches over them).
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct PostTuning {
    pub on: bool,
    pub bloom: f32,
    pub threshold: f32,
    pub sky: f32,
    pub scatter: f32,
    pub grade: bool,
    pub contrast: f32,
    pub vibrance: f32,
    pub sharpen: f32,
}

impl Default for PostTuning {
    fn default() -> Self {
        PostTuning {
            on: true,
            bloom: BLOOM,
            threshold: THRESHOLD,
            sky: SKY_WEIGHT,
            scatter: SCATTER,
            grade: true,
            contrast: CONTRAST,
            vibrance: VIBRANCE,
            sharpen: SHARPEN,
        }
    }
}

impl PostTuning {
    /// The defaults with the switches (module docs) from `var` over them; unparsable values are ignored.
    pub fn from_env(var: impl Fn(&str) -> Option<String>) -> Self {
        let mut t = PostTuning::default();
        let num = |k: &str| var(k).and_then(|v| v.trim().parse::<f32>().ok()).filter(|v| v.is_finite());
        if var("RC_POST").is_some_and(|v| v.trim() == "0") { t.on = false; }
        if var("RC_POST_GRADE").is_some_and(|v| v.trim() == "0") { t.grade = false; }
        if let Some(v) = num("RC_POST_BLOOM") { t.bloom = v.max(0.0); }
        if let Some(v) = num("RC_POST_THRESHOLD") { t.threshold = v.max(0.0); }
        if let Some(v) = num("RC_POST_SKY") { t.sky = v.clamp(0.0, 1.0); }
        if let Some(v) = num("RC_POST_SCATTER") { t.scatter = v.clamp(0.0, 1.0); }
        if let Some(v) = num("RC_POST_CONTRAST") { t.contrast = v.clamp(0.5, 2.0); }
        if let Some(v) = num("RC_POST_VIBRANCE") { t.vibrance = v.clamp(0.0, VIBRANCE_CAP); }
        if let Some(v) = num("RC_POST_SHARPEN") { t.sharpen = v.clamp(0.0, 1.0); }
        t
    }
}

/// The bloom's contribution factor (× the colour) for a luminance `l`: 0 below `t − knee`, quadratic through the
/// knee, `(l − t) / l` above it (`post.wgsl` `soft_threshold`).
#[cfg_attr(not(test), allow(dead_code))]
pub fn soft_threshold(l: f32, t: f32, knee: f32) -> f32 {
    let s = (l - t + knee).clamp(0.0, 2.0 * knee);
    let s = s * s / (4.0 * knee + 1e-5);
    s.max(l - t) / l.max(1e-5)
}

/// The grade's curve on one linear channel: `mid·(x/mid)^contrast`, then above `shoulder` an exponential approach to
/// 1 with slope 1 where it starts (`post.wgsl` `tone`).
#[cfg_attr(not(test), allow(dead_code))]
pub fn tone(x: f32, contrast: f32, shoulder: f32) -> f32 {
    let c = MID_GREY * (x.max(0.0) / MID_GREY).powf(contrast);
    if c <= shoulder { return c; }
    shoulder + (1.0 - shoulder) * (1.0 - (-(c - shoulder) / (1.0 - shoulder)).exp())
}

/// Vibrance: the colour pushed from its luminance by `amount × (1 − saturation)` (`post.wgsl` `vibrance`).
#[cfg_attr(not(test), allow(dead_code))]
pub fn vibrance(c: Vec3, amount: f32) -> Vec3 {
    let (mx, mn) = (c.max_element(), c.min_element());
    let sat = (mx - mn) / mx.max(1e-5);
    let l = c.dot(LUMA);
    (Vec3::splat(l) + (c - Vec3::splat(l)) * (1.0 + amount * (1.0 - sat))).max(Vec3::ZERO)
}

#[cfg_attr(not(test), allow(dead_code))]
const LUMA: Vec3 = Vec3::new(0.2126, 0.7152, 0.0722);

/// The bloom levels' sizes for a frame of `size`: `levels` halvings (rounded up, at least 1).
pub fn mip_sizes(size: UVec2, levels: usize) -> Vec<UVec2> {
    let mut s = size;
    (0..levels).map(|_| { s = ((s + UVec2::ONE) / 2).max(UVec2::ONE); s }).collect()
}

/// The sum of the levels' weights in the bloom (the up passes: level i counts `scatter^i`), which the strength is
/// divided by.
pub fn level_weights(scatter: f32, levels: usize) -> f32 { (0..levels).map(|i| scatter.powi(i as i32)).sum() }

/// `post.wgsl`'s `Post`.
#[derive(Clone, Copy, Debug, Default, ShaderType)]
struct PostUniform {
    /// xy: 1 / the pass's source size, z: the scatter (up passes).
    texel: Vec4,
    /// Threshold, knee, sky weight, bloom strength ÷ [`level_weights`] (0: no bloom).
    bloom: Vec4,
    /// Contrast, shoulder, vibrance, grade on (1) / off (0).
    grade: Vec4,
    /// x: CAS sharpness (0: off), y: mid grey, zw: 1 / the first bloom level's size.
    sharp: Vec4,
}

/// The view's bloom levels this frame (only while the effect is on).
#[derive(Component)]
struct BloomLevels(Vec<CachedTexture>);

pub struct PostPlugin;

impl Plugin for PostPlugin {
    fn build(&self, app: &mut App) {
        let tuning = PostTuning::from_env(|k| std::env::var(k).ok());
        if tuning != PostTuning::default() { println!("post: {tuning:?} (RC_POST* switches; Lighting Enhanced)"); }
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else { return };
        render_app
            .insert_resource(tuning)
            .add_systems(RenderStartup, init_pipelines)
            .add_systems(Render, prepare.in_set(RenderSystems::PrepareResources))
            .add_systems(
                Core3d,
                post_pass
                    .after(Core3dSystems::PostProcess)
                    .before(crate::gs_post::pass::<crate::screen_tint::ScreenTint>)
                    .before(crate::gs_post::pass::<crate::mirror_render::MirrorFlip>)
                    .before(crate::gs_post::pass::<crate::aa_blit::BlitAcross>)
                    .before(bevy::ui_render::ui_pass),
            );
    }
}

/// The passes' stages (the pipeline key's first part).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Stage {
    Prefilter,
    Down,
    Up,
    Composite,
}

#[derive(Resource)]
struct PostPipelines {
    prefilter_layout: [BindGroupLayoutDescriptor; 2],
    chain_layout: BindGroupLayoutDescriptor,
    composite_layout: BindGroupLayoutDescriptor,
    shader: Handle<Shader>,
    linear: Sampler,
    /// (stage, depth multisampled, target format).
    pipes: Pipelines<(Stage, bool, TextureFormat)>,
}

/// `dst + src`: the up passes add onto the level.
const ADD: BlendState = BlendState {
    color: BlendComponent { src_factor: BlendFactor::One, dst_factor: BlendFactor::One, operation: BlendOperation::Add },
    alpha: BlendComponent { src_factor: BlendFactor::One, dst_factor: BlendFactor::One, operation: BlendOperation::Add },
};

fn init_pipelines(mut commands: Commands, assets: Res<AssetServer>, fullscreen: Res<FullscreenShader>, device: Res<RenderDevice>) {
    let filterable = || texture_2d(TextureSampleType::Float { filterable: true });
    let prefilter = |ms: bool| {
        BindGroupLayoutDescriptor::new(
            "post prefilter",
            &BindGroupLayoutEntries::with_indices(ShaderStages::FRAGMENT, ((0, uniform_buffer::<PostUniform>(false)), (1, depth_entry(ms)), (2, filterable()))),
        )
    };
    let chain = BindGroupLayoutDescriptor::new(
        "post bloom chain",
        &BindGroupLayoutEntries::with_indices(
            ShaderStages::FRAGMENT,
            ((0, uniform_buffer::<PostUniform>(false)), (2, filterable()), (3, sampler(SamplerBindingType::Filtering))),
        ),
    );
    let composite = BindGroupLayoutDescriptor::new(
        "post composite",
        &BindGroupLayoutEntries::with_indices(
            ShaderStages::FRAGMENT,
            ((0, uniform_buffer::<PostUniform>(false)), (2, filterable()), (3, sampler(SamplerBindingType::Filtering)), (4, filterable())),
        ),
    );
    let linear = device.create_sampler(&SamplerDescriptor {
        label: Some("post"),
        address_mode_u: AddressMode::ClampToEdge,
        address_mode_v: AddressMode::ClampToEdge,
        mag_filter: FilterMode::Linear,
        min_filter: FilterMode::Linear,
        ..default()
    });
    commands.insert_resource(PostPipelines {
        prefilter_layout: [prefilter(false), prefilter(true)],
        chain_layout: chain,
        composite_layout: composite,
        shader: assets.load(SHADER_PATH),
        linear,
        pipes: Pipelines::new(&fullscreen),
    });
}

fn prepare(
    mut commands: Commands,
    gfx: Option<Res<GraphicsSettings>>,
    tuning: Res<PostTuning>,
    device: Res<RenderDevice>,
    mut textures: ResMut<TextureCache>,
    views: Query<(Entity, &ExtractedCamera), With<ShadowCamera>>,
) {
    let on = tuning.on && gfx.is_some_and(|g| g.lighting.enhanced());
    for (e, cam) in &views {
        let Some(size) = cam.physical_target_size.filter(|_| on) else {
            commands.entity(e).remove::<BloomLevels>();
            continue;
        };
        let levels = mip_sizes(size, LEVELS)
            .into_iter()
            .map(|s| {
                textures.get(
                    &device,
                    TextureDescriptor {
                        label: Some("post bloom level"),
                        size: Extent3d { width: s.x, height: s.y, depth_or_array_layers: 1 },
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: TextureDimension::D2,
                        format: BLOOM_FORMAT,
                        usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
                        view_formats: &[],
                    },
                )
            })
            .collect();
        commands.entity(e).insert(BloomLevels(levels));
    }
}

fn post_pass(
    view: ViewQuery<(&ViewTarget, &ViewDepthTexture, &BloomLevels), With<ShadowCamera>>,
    tuning: Res<PostTuning>,
    pipes: Option<Res<PostPipelines>>,
    cache: Res<PipelineCache>,
    mut ctx: RenderContext,
) {
    let Some(pipes) = pipes else { return };
    let (target, depth, levels) = view.into_inner();
    let levels = &levels.0;
    let format = target.main_texture_format();
    let ms = depth.texture.sample_count() > 1;
    let fullscreen = |label, layout: &BindGroupLayoutDescriptor, entry, defs, format, blend| Fullscreen {
        label,
        layout: layout.clone(),
        shader: pipes.shader.clone(),
        entry,
        defs,
        format,
        blend,
        samples: 1,
    };
    let pre = pipes.pipes.get(&cache, (Stage::Prefilter, ms, BLOOM_FORMAT), || {
        fullscreen("post prefilter", &pipes.prefilter_layout[ms as usize], "prefilter", depth_defs(ms), BLOOM_FORMAT, None)
    });
    let down = pipes.pipes.get(&cache, (Stage::Down, false, BLOOM_FORMAT), || fullscreen("post down", &pipes.chain_layout, "down", vec![], BLOOM_FORMAT, None));
    let up = pipes.pipes.get(&cache, (Stage::Up, false, BLOOM_FORMAT), || fullscreen("post up", &pipes.chain_layout, "up", vec![], BLOOM_FORMAT, Some(ADD)));
    let comp = pipes.pipes.get(&cache, (Stage::Composite, false, format), || fullscreen("post composite", &pipes.composite_layout, "composite", vec![], format, None));
    let (Some(pre), Some(down), Some(up), Some(comp)) = (pre, down, up, comp) else { return };

    let bloom = tuning.bloom > 0.0;
    let size = |t: &CachedTexture| Vec2::new(t.texture.width() as f32, t.texture.height() as f32);
    let first = size(&levels[0]).recip();
    let base = PostUniform {
        texel: Vec4::ZERO,
        bloom: Vec4::new(tuning.threshold, KNEE, tuning.sky, if bloom { tuning.bloom / level_weights(tuning.scatter, LEVELS) } else { 0.0 }),
        grade: Vec4::new(tuning.contrast, SHOULDER, tuning.vibrance, tuning.grade as u32 as f32),
        sharp: Vec4::new(tuning.sharpen, MID_GREY, first.x, first.y),
    };
    let device = ctx.render_device().clone();
    let uniform = |texel: Vec2| crate::fx_view::uniform_buffer(&device, "post", &PostUniform { texel: texel.extend(tuning.scatter).extend(0.0), ..base });
    let chain_group = |src: &TextureView, texel: Vec2| {
        let buf = uniform(texel);
        device.create_bind_group(
            "post bloom chain",
            &cache.get_bind_group_layout(&pipes.chain_layout),
            &BindGroupEntries::with_indices(((0, buf.as_entire_binding()), (2, src), (3, &pipes.linear))),
        )
    };
    let post = target.post_process_write();
    let (source, destination) = (post.source, post.destination);
    let enc = ctx.command_encoder();
    let mut draw = |label, view: &TextureView, load: bool, pipe: &RenderPipeline, group: &BindGroup| {
        let load = if load { LoadOp::Load } else { LoadOp::Clear(Default::default()) };
        let mut rp = enc.begin_render_pass(&RenderPassDescriptor {
            label: Some(label),
            color_attachments: &[Some(RenderPassColorAttachment { view, depth_slice: None, resolve_target: None, ops: Operations { load, store: StoreOp::Store } })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        rp.set_pipeline(pipe);
        rp.set_bind_group(0, group, &[]);
        rp.draw(0..3, 0..1);
    };
    if bloom {
        let buf = uniform(Vec2::ZERO);
        let group = device.create_bind_group(
            "post prefilter",
            &cache.get_bind_group_layout(&pipes.prefilter_layout[ms as usize]),
            &BindGroupEntries::with_indices(((0, buf.as_entire_binding()), (1, depth.view()), (2, source))),
        );
        draw("post prefilter", &levels[0].default_view, false, pre, &group);
        for i in 1..levels.len() {
            let group = chain_group(&levels[i - 1].default_view, size(&levels[i - 1]).recip());
            draw("post down", &levels[i].default_view, false, down, &group);
        }
        for i in (0..levels.len() - 1).rev() {
            let group = chain_group(&levels[i + 1].default_view, size(&levels[i + 1]).recip());
            draw("post up", &levels[i].default_view, true, up, &group);
        }
    }
    let buf = uniform(Vec2::ZERO);
    let group = device.create_bind_group(
        "post composite",
        &cache.get_bind_group_layout(&pipes.composite_layout),
        &BindGroupEntries::with_indices(((0, buf.as_entire_binding()), (2, source), (3, &pipes.linear), (4, &levels[0].default_view))),
    );
    draw("post composite", destination, false, comp, &group);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn threshold_is_soft_and_continuous() {
        let f = |l| soft_threshold(l, THRESHOLD, KNEE);
        assert_eq!(f(0.0), 0.0);
        assert_eq!(f(THRESHOLD - KNEE - 0.01), 0.0);
        // Above the knee: the excess over the threshold, as a factor on the colour.
        assert!((f(1.0) - (1.0 - THRESHOLD)).abs() < 1e-4, "{}", f(1.0));
        assert!((f(2.0) * 2.0 - (2.0 - THRESHOLD)).abs() < 1e-4);
        // Rising, and continuous where the knee meets the line.
        let mut last = 0.0;
        for i in 0..=400 {
            let l = i as f32 / 200.0;
            let c = f(l) * l;
            assert!(c >= last - 1e-5 && c - last < 0.02, "l {l}: {c} after {last}");
            last = c;
        }
    }

    #[test]
    fn curve_keeps_mid_grey_and_never_clips() {
        let t = |x| tone(x, CONTRAST, SHOULDER);
        assert!((t(MID_GREY) - MID_GREY).abs() < 1e-6);
        assert_eq!(t(0.0), 0.0);
        // Gentle: shadows a little darker, the frame's white a little below 1, bloom-lit values still below 1.
        assert!(t(0.05) < 0.05 && t(0.05) > 0.045, "{}", t(0.05));
        assert!(t(1.0) > 0.95 && t(1.0) < 1.0, "{}", t(1.0));
        assert!(t(4.0) <= 1.0 && t(4.0) > 0.999);
        let mut last = -1.0;
        for i in 0..=1000 {
            let y = t(i as f32 / 250.0);
            assert!(y >= last && y <= 1.0, "{i}: {y}");
            assert!(i > 250 || y > last, "{i}: rising below the frame's white");
            last = y;
        }
        // No kink where the shoulder starts.
        let x0 = MID_GREY * (SHOULDER / MID_GREY).powf(1.0 / CONTRAST);
        let slope = |a: f32, b: f32| (t(b) - t(a)) / (b - a);
        assert!((slope(x0 - 1e-3, x0) - slope(x0, x0 + 1e-3)).abs() < 0.02);
    }

    #[test]
    fn vibrance_lifts_dull_colours_more() {
        let grey = Vec3::splat(0.4);
        assert_eq!(vibrance(grey, VIBRANCE), grey);
        let dull = Vec3::new(0.45, 0.4, 0.35);
        let vivid = Vec3::new(0.9, 0.1, 0.05);
        let spread = |c: Vec3| c.max_element() - c.min_element();
        let gain = |c: Vec3| spread(vibrance(c, VIBRANCE)) / spread(c);
        assert!(gain(dull) > 1.15 && gain(dull) <= 1.0 + VIBRANCE, "{}", gain(dull));
        assert!(gain(vivid) < 1.03, "{}", gain(vivid));
        // The luminance is kept.
        assert!((vibrance(dull, VIBRANCE).dot(LUMA) - dull.dot(LUMA)).abs() < 1e-5);
    }

    #[test]
    fn mip_chain_halves() {
        assert_eq!(
            mip_sizes(UVec2::new(1920, 1080), LEVELS),
            [UVec2::new(960, 540), UVec2::new(480, 270), UVec2::new(240, 135), UVec2::new(120, 68), UVec2::new(60, 34)]
        );
        assert_eq!(mip_sizes(UVec2::new(1024, 768), LEVELS)[4], UVec2::new(32, 24));
        assert_eq!(mip_sizes(UVec2::new(3, 1), 3), [UVec2::new(2, 1), UVec2::ONE, UVec2::ONE]);
    }

    #[test]
    fn levels_weigh_less_as_they_widen() {
        assert!((level_weights(SCATTER, LEVELS) - (1.0 + 0.6 + 0.36 + 0.216 + 0.1296)).abs() < 1e-5);
        assert_eq!(level_weights(1.0, LEVELS), LEVELS as f32);
        assert_eq!(level_weights(0.0, LEVELS), 1.0);
    }

    #[test]
    fn switches() {
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |k: &str| pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| v.to_string())
        };
        assert_eq!(PostTuning::from_env(env(&[])), PostTuning::default());
        let t = PostTuning::from_env(env(&[("RC_POST", "0"), ("RC_POST_BLOOM", "0"), ("RC_POST_VIBRANCE", "3"), ("RC_POST_SHARPEN", "x")]));
        assert!(!t.on);
        assert_eq!((t.bloom, t.vibrance, t.sharpen), (0.0, VIBRANCE_CAP, SHARPEN));
        assert!(!PostTuning::from_env(env(&[("RC_POST_GRADE", "0")])).grade);
        assert_eq!(PostTuning::from_env(env(&[("RC_POST_SCATTER", "2")])).scatter, 1.0);
    }
}
