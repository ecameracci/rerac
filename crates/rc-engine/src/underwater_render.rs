//! **Port-only: the underwater look of Water Enhanced** (crate::graphics): with the camera under water, one
//! full-screen pass over the 3D picture (`assets/shaders/underwater.wgsl`), in PostProcess after the sun rays and
//! before the screen tints, the anti-aliasing blit, the HUD and the game's own tint sprite
//! (crate::fog_state `UnderwaterTint`, which stays as the game draws it).
//!
//! The game's underwater state is kept as it is: the flag 0x167494 and the fog swap of `UpdateFog`
//! (crate::fog_state, docs/plan/world_animation.md §6). Over it, per pixel (world position from the depth buffer,
//! crate::fx_view):
//! 1. **The in-water part of the view ray** ([`water_segment`]): from the eye to the scene, cut at the water plane
//!    ([`UnderwaterView::surface`]); a camera at the surface gets the waterline split from the plane itself.
//! 2. **Light from above** ([`transmittance`] of the pixel's depth below the plane with [`LIGHT_SIGMA`]); what a deep
//!    floor loses is filled by half the water's colour, so the floor fades toward it rather than to black.
//! 3. **Caustics** on the surfaces below the plane: two warped noise layers, sampled where the sun's direction bent
//!    into the water ([`refract_down`]) meets the plane, faded by depth, by facing, and where a layer's cells shrink
//!    below a pixel (no aliasing in the distance).
//! 4. **Light shafts:** [`SHAFT_TAPS`] taps along the in-water ray, each lit by a coarse layer of the same pattern
//!    projected up to the plane along the sun, so the shafts slant with the sun and drift with the caustics.
//! 5. **Absorption and in-scatter** along the in-water part ([`transmittance`] with [`ABSORB_SIGMA`]: red goes
//!    first) toward the water's colour, the alternate fog colour the game uses (`UnderwaterLook::fog`, 0x161204). The
//!    in-scatter is lit at the depth of the ray's first 30 units (the shader's `SCATTER_REACH`), so far shelves and the
//!    open sea (no geometry, depth 0) end on the same lit colour, not on the dark of the ray's far end; a ray looking
//!    down is dimmed smoothly by its slope (up to 30 %), so a trench still reads deeper.
//! 6. **A wobble** of the whole picture (a few pixels' sine offset of the read).
//!
//! **The water's colour.** Every level's water managers leave the Novalis overlay's navy in 0x161200.. (the alternate
//! fog and the 50 % tint sprite). Where the Enhanced surface has a colour of its own ([`SURFACE_COLOURS`]: Pokitaru's
//! turquoise ocean), the fog the frame is drawn with, the tint sprite and this pass take it instead ([`enhanced_look`],
//! read by crate::fog_state), so under the water reads as the surface does from above (G-REN-103's tint at
//! [`TINT_SHARE`] of its strength there). The game's look and its flag are unchanged; other levels keep the navy.
//!
//! The pass fades in and out over [`RAMP_SECS`] ([`ramp_step`]) when the flag flips, and runs only while that fade is
//! above 0; with Water Original it never runs, so those frames are unchanged.
//!
//! **The water plane** is found by [`probe_surface`]: the underwater test's own line query (`CollLine_Fix` flags
//! 0x12: the world mesh, then the moby meshes that carry the patch managers' water), cast upward from the camera,
//! stepping past non-water faces; the first water face (surface 0) gives `SetWaterLevel`'s height
//! (`WaterWorld::water_height`: the ripple patch, 982's plane, else the hit's z). The last height found is kept while
//! nothing is hit (a ceiling of more than [`PROBE_UP`] units of water, a forced flag); without any, the plane is
//! [`DEFAULT_DEPTH`] above the camera.
//!
//! Dev switches: `RC_UNDERWATER_FX=0` (off), `RC_UNDERWATER_FX_STRENGTH=<k>` (the absorption, light, caustics and
//! shafts scaled; default 1), `RC_UNDERWATER_FX_DEBUG=1|2|3|4` (the in-water length, the caustics, the shafts in grey;
//! the world normals).

use crate::fly_cam::FlyCam;
use crate::fog_state::{FogSet, FogState};
use crate::fx_view::{depth_defs, depth_entry, Fullscreen, FxView, Pipelines};
use crate::gameplay::Play;
use crate::graphics::GraphicsSettings;
use crate::shadow_render::ShadowCamera;
use crate::sun_shadow::KeyLight;
use bevy::core_pipeline::{Core3d, Core3dSystems, FullscreenShader};
use bevy::prelude::*;
use bevy::render::camera::ExtractedCamera;
use bevy::render::extract_resource::{ExtractResource, ExtractResourcePlugin};
use bevy::render::render_resource::binding_types::{sampler, texture_2d, uniform_buffer};
use bevy::render::render_resource::{
    AddressMode, BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, FilterMode, Operations, PipelineCache,
    RenderPassColorAttachment, RenderPassDescriptor, Sampler, SamplerBindingType, SamplerDescriptor, ShaderStages, ShaderType,
    TextureFormat, TextureSampleType,
};
use bevy::render::renderer::{RenderContext, RenderDevice, ViewQuery};
use bevy::render::view::{ExtractedView, ViewDepthTexture, ViewTarget};
use bevy::render::{RenderApp, RenderStartup};
use bevy::shader::Shader;
use rc_game::collision_query::{coll_line, coll_line_m, QueryFlags};
use rc_game::fog_zones::UnderwaterLook;

const SHADER_PATH: &str = "shaders/underwater.wgsl";
/// The fade in / out when the underwater flag flips (seconds).
pub const RAMP_SECS: f32 = 0.35;
/// Per world unit of water along the view ray: red goes first, then green.
pub const ABSORB_SIGMA: Vec3 = Vec3::new(0.06, 0.022, 0.012);
/// Per world unit of depth below the plane, for the light reaching a surface.
pub const LIGHT_SIGMA: Vec3 = Vec3::new(0.05, 0.022, 0.014);
/// The refractive index of water (the sun's direction bent at the plane).
pub const WATER_IOR: f32 = 1.33;
/// How far above the camera the plane is looked for (world units).
pub const PROBE_UP: f32 = 64.0;
/// The plane above the camera when no water face was ever found.
pub const DEFAULT_DEPTH: f32 = 4.0;
/// Taps of the light shafts along the in-water ray.
const SHAFT_TAPS: u32 = 16;
/// The wobble's amplitude (target pixels per 1,000 lines).
const WOBBLE_PX: f32 = 1.6;

/// The view's water state this frame (main world; copied to the render world).
#[derive(Resource, Clone, Copy, Debug, ExtractResource)]
pub struct UnderwaterView {
    /// The fade: 0 (nothing drawn) .. 1.
    pub amount: f32,
    /// The water plane (Bevy y = game z), None before any was found.
    pub surface: Option<f32>,
    /// The water's colour (linear): the alternate fog colour.
    pub water: Vec3,
    pub time: f32,
    pub strength: f32,
    pub debug: u32,
    enabled: bool,
}

impl Default for UnderwaterView {
    fn default() -> Self {
        let env = |k: &str| std::env::var(k).ok().map(|v| v.trim().to_string());
        UnderwaterView {
            amount: 0.0,
            surface: None,
            water: Vec3::new(0.009, 0.030, 0.279),
            time: 0.0,
            strength: env("RC_UNDERWATER_FX_STRENGTH").and_then(|v| v.parse().ok()).unwrap_or(1.0f32).max(0.0),
            debug: env("RC_UNDERWATER_FX_DEBUG").and_then(|v| v.parse().ok()).unwrap_or(0),
            enabled: env("RC_UNDERWATER_FX").as_deref() != Some("0"),
        }
    }
}

/// `rerac::underwater::Underwater` (the shader's uniform at binding 4).
#[derive(Clone, Copy, Default, ShaderType)]
struct UnderwaterUniform {
    /// Plane y, plane known (0 / 1), fade, time.
    plane: Vec4,
    /// Water colour (linear), strength.
    water: Vec4,
    /// Absorption per unit (r, g, b), wobble in target pixels.
    absorb: Vec4,
    /// Light falloff per unit of depth (r, g, b), shaft taps.
    light: Vec4,
    /// Toward the light inside the water (unit, up), debug mode.
    sun: Vec4,
}

pub struct UnderwaterPlugin;

impl Plugin for UnderwaterPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<UnderwaterView>()
            .add_plugins(ExtractResourcePlugin::<UnderwaterView>::default())
            .add_systems(PostUpdate, follow.after(FogSet));
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else { return };
        render_app
            .add_systems(RenderStartup, init_pipelines)
            .add_systems(Core3d, underwater_pass.after(crate::sun_rays::SunRaysSet).in_set(Core3dSystems::PostProcess));
    }
}

/// One frame of the fade toward `target` (0 or 1) over `secs`.
pub fn ramp_step(amount: f32, target: f32, dt: f32, secs: f32) -> f32 {
    let step = if secs > 0.0 { dt.max(0.0) / secs } else { 1.0 };
    if target > amount { (amount + step).min(target) } else { (amount - step).max(target) }
}

/// The part `(t0, t1)` of the ray `eye_y + t·dir_y`, `0 ≤ t ≤ t_scene`, below the plane `surface` (None: all of it).
/// The shader's `water_segment`.
#[cfg_attr(not(test), allow(dead_code))]
pub fn water_segment(eye_y: f32, dir_y: f32, t_scene: f32, surface: Option<f32>) -> (f32, f32) {
    let Some(h) = surface else { return (0.0, t_scene) };
    if eye_y <= h {
        let exit = if dir_y > 1e-5 { (h - eye_y) / dir_y } else { f32::INFINITY };
        (0.0, t_scene.min(exit))
    } else if dir_y < -1e-5 {
        let enter = (h - eye_y) / dir_y;
        (enter.min(t_scene), t_scene)
    } else {
        (0.0, 0.0)
    }
}

/// What is left of each channel after `d` units of water with `sigma` (Beer–Lambert; the shader's `transmittance`).
#[cfg_attr(not(test), allow(dead_code))]
pub fn transmittance(sigma: Vec3, d: f32) -> Vec3 { (-sigma * d.max(0.0)).exp() }

/// The light's travel direction inside the water: `to_light` (unit, above the plane) bent at the plane (Snell).
pub fn refract_down(to_light: Vec3) -> Vec3 {
    let i = -to_light.normalize();
    let n = Vec3::Y;
    let eta = 1.0 / WATER_IOR;
    let cos_i = -n.dot(i);
    let k = 1.0 - eta * eta * (1.0 - cos_i * cos_i);
    if k < 0.0 || cos_i <= 0.0 { return -Vec3::Y; }
    (eta * i + (eta * cos_i - k.sqrt()) * n).normalize()
}

/// The water plane above `eye` (game axes, z up): `cast(p0, p1)` → (hit, surface id) up from 0.75 below the eye to
/// [`PROBE_UP`] above, past up to 8 non-water faces; the first water face's `height(hit)`, else its z.
pub fn probe_surface(eye: [f32; 3], mut cast: impl FnMut([f32; 3], [f32; 3]) -> Option<([f32; 3], i32)>, mut height: impl FnMut([f32; 3]) -> Option<f32>) -> Option<f32> {
    let end = [eye[0], eye[1], eye[2] + PROBE_UP];
    let mut from = [eye[0], eye[1], eye[2] - 0.75];
    for _ in 0..8 {
        let (hit, surface) = cast(from, end)?;
        if surface == 0 { return Some(height(hit).unwrap_or(hit[2])); }
        from = [hit[0], hit[1], hit[2] + 0.01];
        if from[2] >= end[2] { return None; }
    }
    None
}

/// The water above `eye` where no face was hit: the first `height` (`SetWaterLevel` without its fallback) of the points
/// 1 unit apart up to [`PROBE_UP`] above it (a ripple patch answers within its 2-unit band).
pub fn scan_up(eye: [f32; 3], mut height: impl FnMut([f32; 3]) -> Option<f32>) -> Option<f32> {
    (0..=PROBE_UP as i32).find_map(|k| height([eye[0], eye[1], eye[2] + k as f32]))
}

/// The Enhanced water's own colour by level (RGB bytes) where its surface has one: Pokitaru (11), its ocean's open water
/// (crate::ocean_water `OPEN` on feat/water-waves, kept equal by hand). Every other level keeps the game's colours.
pub const SURFACE_COLOURS: &[(u32, [u8; 3])] = &[(11, [0x18, 0xbc, 0xb0])];
/// The share of the game's tint sprite strength kept under a [`SURFACE_COLOURS`] water ([`enhanced_look`]).
pub const TINT_SHARE: f32 = 0.6;

/// [`SURFACE_COLOURS`]' entry for a level.
pub fn surface_colour(level: u32) -> Option<[u8; 3]> { SURFACE_COLOURS.iter().find(|e| e.0 == level).map(|e| e.1) }

/// The underwater colours drawn with Water Enhanced (crate::fog_state: the alternate fog `UpdateFog` swaps in and the
/// tint sprite; this pass's water colour): the game's `look` with the level's [`surface_colour`] in place of the fog's
/// and the tint's colour, the tint's alpha times [`TINT_SHARE`]. The game's own look (0x161200..) is not written.
pub fn enhanced_look(look: &UnderwaterLook, colour: Option<[u8; 3]>) -> UnderwaterLook {
    let Some(c) = colour else { return *look };
    let mut out = *look;
    out.fog.color = c;
    out.tint = [c[0], c[1], c[2], (look.tint[3] as f32 * TINT_SHARE).round() as u8];
    out
}

fn srgb_to_linear(b: u8) -> f32 {
    let c = b as f32 / 255.0;
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

/// The fade, the plane and the water's colour, after the fog state's camera update.
fn follow(
    g: Res<GraphicsSettings>,
    state: Option<Res<FogState>>,
    play: Option<Res<Play>>,
    time: Res<Time>,
    cams: Query<&Transform, With<FlyCam>>,
    mut view: ResMut<UnderwaterView>,
) {
    let v = &mut *view;
    let flag = state.as_ref().is_some_and(|s| s.underwater_flag());
    let target = if v.enabled && g.water.on() && flag { 1.0 } else { 0.0 };
    let amount = ramp_step(v.amount, target, time.delta_secs().min(0.1), RAMP_SECS);
    if amount != v.amount { v.amount = amount; }
    if amount <= 0.0 { return; }
    v.time = time.elapsed_secs_wrapped();
    let Some(state) = state else { return };
    v.water = Vec3::from_array(enhanced_look(state.look(), surface_colour(crate::level_load::level_index())).fog.color.map(srgb_to_linear));
    if !flag { return; }
    let Some(eye) = cams.iter().next().map(|t| crate::game_camera::game_eye(t).to_array()) else { return };
    let found = state.mesh().and_then(|mesh| match play.as_deref() {
        Some(p) => {
            let src = p.svc.scene_parts(&p.game.mobys, &*p.classes);
            let scene = p.svc.scene(&src);
            probe_surface(
                eye,
                |a, b| coll_line_m(mesh, Some(&scene), a, b, QueryFlags(0x12), None).map(|h| (h.point, h.surface_id())),
                |q| p.svc.water.water_height(q),
            )
            .or_else(|| scan_up(eye, |q| p.svc.water.water_height(q)))
        }
        None => probe_surface(eye, |a, b| coll_line(mesh, a, b, QueryFlags(0x12)).map(|h| (h.point, h.surface_id())), |_| None),
    });
    let surface = found.or(v.surface).unwrap_or(eye[2] + DEFAULT_DEPTH);
    if v.surface.is_none_or(|h| (h - surface).abs() > 0.5) {
        println!("underwater fx: water plane z {surface:.2} ({}) at {eye:.2?}", if found.is_some() { "found" } else { "kept" });
    }
    if v.surface != Some(surface) { v.surface = Some(surface); }
}

#[derive(Resource)]
struct UnderwaterPipelines {
    layout: [BindGroupLayoutDescriptor; 2],
    shader: Handle<Shader>,
    linear: Sampler,
    /// (depth multisampled, target format).
    pipes: Pipelines<(bool, TextureFormat)>,
}

fn init_pipelines(mut commands: Commands, assets: Res<AssetServer>, fullscreen: Res<FullscreenShader>, device: Res<RenderDevice>) {
    let layout = |ms: bool| {
        BindGroupLayoutDescriptor::new(
            "underwater",
            &BindGroupLayoutEntries::sequential(
                ShaderStages::FRAGMENT,
                (
                    uniform_buffer::<FxView>(false),
                    depth_entry(ms),
                    texture_2d(TextureSampleType::Float { filterable: true }),
                    sampler(SamplerBindingType::Filtering),
                    uniform_buffer::<UnderwaterUniform>(false),
                ),
            ),
        )
    };
    let linear = device.create_sampler(&SamplerDescriptor {
        label: Some("underwater"),
        address_mode_u: AddressMode::ClampToEdge,
        address_mode_v: AddressMode::ClampToEdge,
        mag_filter: FilterMode::Linear,
        min_filter: FilterMode::Linear,
        ..default()
    });
    commands.insert_resource(UnderwaterPipelines { layout: [layout(false), layout(true)], shader: assets.load(SHADER_PATH), linear, pipes: Pipelines::new(&fullscreen) });
}

fn underwater_pass(
    view: ViewQuery<(&ExtractedCamera, &ExtractedView, &ViewTarget, &ViewDepthTexture), With<ShadowCamera>>,
    uw: Option<Res<UnderwaterView>>,
    key: Option<Res<KeyLight>>,
    g: Option<Res<GraphicsSettings>>,
    pipes: Option<Res<UnderwaterPipelines>>,
    cache: Res<PipelineCache>,
    mut ctx: RenderContext,
) {
    let (Some(uw), Some(pipes)) = (uw, pipes) else { return };
    if uw.amount <= 0.0 || !g.is_some_and(|g| g.water.on()) { return; }
    let (cam, ev, target, depth) = view.into_inner();
    let (Some(mut fx), Some(size)) = (FxView::of(cam, ev), cam.physical_target_size) else { return };
    fx.extra = Vec4::new(size.x as f32, size.y as f32, 0.0, 0.0);
    let to_light = key.map_or(Vec3::Y, |k| k.to_light);
    let sun_up = -refract_down(if to_light.y > 0.05 { to_light } else { Vec3::Y });
    let s = uw.strength;
    let u = UnderwaterUniform {
        plane: Vec4::new(uw.surface.unwrap_or(0.0), uw.surface.is_some() as u32 as f32, uw.amount, uw.time),
        water: uw.water.extend(s),
        absorb: (ABSORB_SIGMA * s).extend(WOBBLE_PX * size.y as f32 / 1000.0),
        light: (LIGHT_SIGMA * s).extend(SHAFT_TAPS as f32),
        sun: sun_up.extend(uw.debug as f32),
    };
    let format = target.main_texture_format();
    let ms = depth.texture.sample_count() > 1;
    let Some(pipe) = pipes.pipes.get(&cache, (ms, format), || Fullscreen {
        label: "underwater",
        layout: pipes.layout[ms as usize].clone(),
        shader: pipes.shader.clone(),
        entry: "fragment",
        defs: depth_defs(ms),
        format,
        blend: None,
        samples: 1,
    }) else {
        return;
    };
    let device = ctx.render_device().clone();
    let buf = fx.buffer(&device, "underwater view");
    let ubuf = crate::fx_view::uniform_buffer(&device, "underwater", &u);
    let post = target.post_process_write();
    let (source, destination) = (post.source, post.destination);
    let group = device.create_bind_group(
        "underwater",
        &cache.get_bind_group_layout(&pipes.layout[ms as usize]),
        &BindGroupEntries::sequential((buf.as_entire_binding(), depth.view(), source, &pipes.linear, ubuf.as_entire_binding())),
    );
    let mut pass = ctx.begin_tracked_render_pass(RenderPassDescriptor {
        label: Some("underwater"),
        color_attachments: &[Some(RenderPassColorAttachment { view: destination, depth_slice: None, resolve_target: None, ops: Operations::default() })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    pass.set_render_pipeline(pipe);
    pass.set_bind_group(0, &group, &[]);
    pass.draw(0..3, 0..1);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ramp_reaches_its_target_in_the_fade_time() {
        let mut a = 0.0;
        for _ in 0..20 { a = ramp_step(a, 1.0, RAMP_SECS / 20.0, RAMP_SECS); }
        assert!((a - 1.0).abs() < 1e-5, "{a}");
        // Halfway, then back down; never past the target.
        let half = ramp_step(0.0, 1.0, RAMP_SECS / 2.0, RAMP_SECS);
        assert!((half - 0.5).abs() < 1e-5);
        assert_eq!(ramp_step(half, 0.0, 10.0, RAMP_SECS), 0.0);
        assert_eq!(ramp_step(1.0, 1.0, 0.016, RAMP_SECS), 1.0);
        assert_eq!(ramp_step(0.2, 1.0, 0.016, 0.0), 1.0);
    }

    #[test]
    fn segment_is_cut_at_the_plane() {
        // Eye 2 below the plane: looking up at 30° (dir_y 0.5) leaves the water after 4 units.
        assert_eq!(water_segment(8.0, 0.5, 100.0, Some(10.0)), (0.0, 4.0));
        // A nearer hit ends it first; looking flat or down, the whole ray is in the water.
        assert_eq!(water_segment(8.0, 0.5, 3.0, Some(10.0)), (0.0, 3.0));
        assert_eq!(water_segment(8.0, 0.0, 50.0, Some(10.0)), (0.0, 50.0));
        assert_eq!(water_segment(8.0, -0.7, 50.0, Some(10.0)), (0.0, 50.0));
        // Eye above the plane (the fade out, the flag's 0.04 margin): only the part below it.
        assert_eq!(water_segment(11.0, -0.5, 10.0, Some(10.0)), (2.0, 10.0));
        assert_eq!(water_segment(11.0, -0.5, 1.0, Some(10.0)), (1.0, 1.0));
        assert_eq!(water_segment(11.0, 0.2, 10.0, Some(10.0)), (0.0, 0.0));
        // Waterline: just under the plane, rays above the horizon leave at once, rays below stay in.
        let (a, b) = water_segment(9.99, 0.1, 40.0, Some(10.0));
        assert!(b - a < 0.11);
        assert_eq!(water_segment(9.99, -0.1, 40.0, Some(10.0)), (0.0, 40.0));
        assert_eq!(water_segment(8.0, 0.5, 7.0, None), (0.0, 7.0));
    }

    #[test]
    fn red_is_absorbed_first() {
        let t = transmittance(ABSORB_SIGMA, 10.0);
        assert!(t.x < t.y && t.y < t.z, "{t}");
        assert_eq!(transmittance(ABSORB_SIGMA, 0.0), Vec3::ONE);
        assert_eq!(transmittance(ABSORB_SIGMA, -3.0), Vec3::ONE);
        let l = transmittance(LIGHT_SIGMA, 20.0);
        assert!(l.x < l.y && l.y < l.z && l.z < 1.0, "{l}");
    }

    #[test]
    fn sun_bends_toward_the_vertical() {
        let to_light = Vec3::new(0.8, 0.6, 0.0).normalize();
        let d = refract_down(to_light);
        assert!(d.y < 0.0 && (d.length() - 1.0).abs() < 1e-5);
        // Snell: sin(θt) = sin(θi) / n, travelling away from the light's side.
        let (sin_i, sin_t) = (to_light.x, -d.x);
        assert!((sin_t - sin_i / WATER_IOR).abs() < 1e-4, "{sin_t}");
        assert_eq!(refract_down(Vec3::Y), -Vec3::Y);
    }

    #[test]
    fn probe_steps_past_ceilings_to_the_water() {
        // A non-water face at z 12, the water at z 15 (its ripple height 15.2).
        let faces = [(12.0, 3), (15.0, 0)];
        let cast = |a: [f32; 3], b: [f32; 3]| faces.iter().find(|f| f.0 >= a[2] && f.0 <= b[2]).map(|f| ([a[0], a[1], f.0], f.1));
        assert_eq!(probe_surface([1.0, 2.0, 10.0], cast, |_| Some(15.2)), Some(15.2));
        assert_eq!(probe_surface([1.0, 2.0, 10.0], cast, |_| None), Some(15.0));
        // Nothing above: none.
        assert_eq!(probe_surface([1.0, 2.0, 20.0], cast, |_| None), None);
        // The water just below the eye (the flag's margin) is found.
        assert_eq!(probe_surface([1.0, 2.0, 15.5], cast, |_| None), Some(15.0));
    }

    #[test]
    fn pokitaru_takes_its_surface_colour_and_the_rest_keep_the_game_look() {
        let game = UnderwaterLook::default();
        let poki = enhanced_look(&game, surface_colour(11));
        assert_eq!(poki.fog.color, [0x18, 0xbc, 0xb0]);
        assert_eq!(poki.tint, [0x18, 0xbc, 0xb0, (0x40 as f32 * TINT_SHARE).round() as u8]);
        assert_eq!((poki.fog.near_dist, poki.fog.far_dist, poki.fog.far_intensity), (game.fog.near_dist, game.fog.far_dist, game.fog.far_intensity));
        let (g, b) = (poki.fog.color[1], poki.fog.color[2]);
        assert!(g > poki.fog.color[0] * 4 && b > poki.fog.color[0] * 4, "turquoise, not navy");
        for level in [1, 5, 9] {
            assert_eq!(surface_colour(level), None);
            assert_eq!(enhanced_look(&game, surface_colour(level)), game);
        }
    }

    #[test]
    fn scan_finds_the_patch_band_above() {
        // A patch at z 30 answering within its band 29..31.
        let patch = |q: [f32; 3]| (29.0..31.0).contains(&q[2]).then_some(30.2);
        assert_eq!(scan_up([0.0, 0.0, 20.5], patch), Some(30.2));
        assert_eq!(scan_up([0.0, 0.0, 40.0], patch), None);
    }
}
