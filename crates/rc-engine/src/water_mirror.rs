//! **Port-only (Water Enhanced, crate::graphics; docs/plan/graphics_options.md "Water"): the planar reflection.**
//! The screen-space reflection of `water.wgsl` can only show what is on screen; on flat water the world is drawn a
//! second time, mirrored about the surface, and the water reads that picture instead.
//!
//! **The plane** ([`choose_plane`]). Every frame with the option on, the flat regions of the Enhanced water
//! ([`flat_region`]: the clear strips at their z blend and the drawn clear ripple patches, crate::water_render
//! `WaterState::flat_surfaces`; the Enhanced seas and translucent pools block by block, [`flat_blocks`],
//! crate::sea_water `SeaWaterDraw::planes`) that the camera is above ([`MIN_ABOVE`]) and whose rectangle is partly in
//! the view frustum ([`visible_part`]: clipped by the near side and the four side planes, so a rectangle behind the eye
//! or beside the view does not count); each scores the screen area of its part in view ([`screen_area`]) times the
//! share of that part seen ([`seen_share`]: of a few points of it, [`sight_points`], those with a clear line to the eye
//! through the level's collision mesh, `rc_game::collision_query::coll_line`, its water faces left out; beyond
//! [`SEEN_REACH`] untested), the regions of one height adding up, and the plane covering the most of the screen wins
//! ([`choose_plane`], every [`CHOOSE_EVERY`] frames, about 0.5 ms; the drawn one stays until another covers [`STICK`]
//! times as much): on Rilgar's start beach the bay at 59.5 keeps it over the ripple pools at 61.2, which
//! keep the screen-space reflection (G-REN-111). A change of plane fades ([`fade`]): the drawn one goes out over
//! [`FADE_SECS`] to the screen-space reflection, then the new one comes in. None: the mirror cameras are inactive
//! (nothing drawn, no cost).
//!
//! **The mirror view.** Two cameras draw into [`WaterMirror::image`] at half the main view's size, without
//! multisampling: the sky layer (crate::sky_render's [`SKY_LAYER`], cleared to the level background, depth cleared)
//! then the world (no clear, the sky's depth loaded), as the main and sky cameras do. Their `Transform` is the main
//! camera's (the systems that read the first `Camera3d`'s transform or projection tangents see the same values as for
//! the sky camera); after propagation their `GlobalTransform` becomes [`mirror_view`]: the camera reflected about the
//! plane with its x axis turned back, a proper rotation, so the world is drawn with its own winding (no cull flip)
//! and the picture comes out mirrored left to right: the water reads it at (−x, y). The world camera's projection is
//! the main one with an oblique near plane on the water ([`GameProjection::clip`], [`crate::game_camera::oblique_near`], [`CLIP_BIAS`]
//! under the surface), so nothing under the water is drawn; the sky's has none (it is at infinity). Neither takes
//! the sun's shadow cascades ([`no_sun_cascades`]).
//!
//! **What it draws** ([`keep_world_draws`], after Bevy's visibility check, on the mirror cameras only): the sky shells,
//! the tfrags, the ties and the mobys (Ratchet among them; their metal pass too), without the effect mobys
//! (crate::display_blend `DisplayEffect`). Left out: the sky's stars, the shrubs and billboards, the water and seas,
//! the particles, the draw-callback effects. What the game's own culls hid for the main camera (occlusion, the tie
//! and moby frustum culls) is missing from it too (G-REN-110).
//!
//! **The water** (`water.wgsl` `planar`, bindings 11..13 of crate::water_render::WaterMaterial and
//! crate::sea_water::SeaWaterMaterial): `mirror` = (the weight while drawn, [`fade`]; the plane's height); on pixels
//! within 0.5..1.0 of the plane (Novalis's river strips lie 38.7..39.5) the mirror picture at the pixel, bent by the
//! wave normal and faded out where the bent sample leaves the picture, takes the place of the screen-space reflection
//! by that weight (the same Fresnel weight); every other water pixel keeps the screen-space one.
//!
//! `RC_WATER_MIRROR=0`: no mirror view (the screen-space reflection everywhere); `debug`: also shows the mirror
//! picture in the frame's top-right corner (flipped back, as it reads in the water). Original: no camera is spawned
//! and the shader's Original branch reads none of it.

use crate::fly_cam::FlyCam;
use crate::game_camera::{game_eye, GameProjection};
use crate::graphics::GraphicsSettings;
use crate::sky_render::SKY_LAYER;
use crate::tfrag_render::game_to_bevy;
use bevy::asset::RenderAssetUsages;
use bevy::camera::CameraProjection;
use bevy::camera::visibility::{RenderLayers, VisibilitySystems, VisibleEntities};
use bevy::camera::{Camera3dDepthLoadOp, CameraOutputMode, CameraUpdateSystems, ClearColorConfig, RenderTarget};
use bevy::core_pipeline::tonemapping::{DebandDither, Tonemapping};
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::light::{Cascades, SimulationLightSystems};
use bevy::math::Affine3A;
use bevy::prelude::*;
use bevy::render::render_resource::{BlendState, Extent3d, TextureDimension, TextureFormat, TextureUsages};
use bevy::render::storage::ShaderBuffer;
use bevy::render::view::Msaa;
use bevy::transform::TransformSystems;
use std::any::TypeId;

/// The mirror picture's format (the main view's: the water samples it as it samples the scene copy).
const FORMAT: TextureFormat = TextureFormat::Rgba8UnormSrgb;
/// How far the camera must be above a plane for it to be mirrored (game units).
pub const MIN_ABOVE: f32 = 0.1;
/// The points of a flat region lie this close to its height (game units; [`flat_region`]).
pub const FLAT_TOL: f32 = 0.25;
/// The oblique near plane lies this far under the surface (game units), so what stands in the water meets its image.
pub const CLIP_BIAS: f32 = 0.02;
/// Camera orders: before every camera of the frame (the menu layer −2, the canvases −4).
const ORDER_SKY: isize = -12;
const ORDER_WORLD: isize = -11;

/// A flat water region: its height and its rectangle (game units).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlatWater {
    pub z: f32,
    pub min: Vec2,
    pub max: Vec2,
}

/// The flat part of a surface's points (game x, y, z): the median height and the rectangle of the points within
/// [`FLAT_TOL`] of it (a sea's drop at its edge, a sloped strip's ends fall out). None: fewer than three such points.
pub fn flat_region(points: impl IntoIterator<Item = [f32; 3]>) -> Option<FlatWater> {
    let pts: Vec<[f32; 3]> = points.into_iter().filter(|p| p.iter().all(|v| v.is_finite())).collect();
    if pts.len() < 3 { return None; }
    let mut zs: Vec<f32> = pts.iter().map(|p| p[2]).collect();
    zs.sort_by(f32::total_cmp);
    let z = zs[zs.len() / 2];
    let (mut min, mut max, mut n) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN), 0);
    for p in pts.iter().filter(|p| (p[2] - z).abs() <= FLAT_TOL) {
        min = min.min(Vec2::new(p[0], p[1]));
        max = max.max(Vec2::new(p[0], p[1]));
        n += 1;
    }
    (n >= 3).then_some(FlatWater { z, min, max })
}

/// The flat regions of one surface's points (game x, y, z) taken `block` points at a time (a liquid grid's block: one
/// strip, crate::sea_water): the surface's median height ([`flat_region`]) and, per block, the rectangle of its points
/// within [`FLAT_TOL`] of it (a block with fewer than three such points falls out).
pub fn flat_blocks(points: &[[f32; 3]], block: usize) -> Vec<FlatWater> {
    let Some(all) = flat_region(points.iter().copied()) else { return Vec::new() };
    points
        .chunks(block.max(1))
        .filter_map(|c| {
            let (mut min, mut max, mut n) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN), 0);
            for p in c.iter().filter(|p| p.iter().all(|v| v.is_finite()) && (p[2] - all.z).abs() <= FLAT_TOL) {
                min = min.min(Vec2::new(p[0], p[1]));
                max = max.max(Vec2::new(p[0], p[1]));
                n += 1;
            }
            (n >= 3).then_some(FlatWater { z: all.z, min, max })
        })
        .collect()
}

/// Clip w under which a point counts as behind the eye (the frustum's near side of [`visible_part`]).
const NEAR_W: f32 = 1e-3;

/// The part of a region's rectangle inside the view frustum (`clip_from_world`, Bevy): the four corners clipped by the
/// near side (w ≥ [`NEAR_W`]) and the four side planes, one after the other (Sutherland–Hodgman); game points, a convex
/// polygon on the region's plane. Empty: the rectangle is out of view or behind the eye.
pub fn visible_part(clip_from_world: &Mat4, f: &FlatWater) -> Vec<Vec3> {
    let mut poly: Vec<(Vec3, Vec4)> = [(f.min.x, f.min.y), (f.max.x, f.min.y), (f.max.x, f.max.y), (f.min.x, f.max.y)]
        .iter()
        .map(|&(x, y)| (Vec3::new(x, y, f.z), *clip_from_world * game_to_bevy([x, y, f.z]).extend(1.0)))
        .collect();
    let sides: [fn(Vec4) -> f32; 5] = [|c| c.w - NEAR_W, |c| c.w + c.x, |c| c.w - c.x, |c| c.w + c.y, |c| c.w - c.y];
    for side in sides {
        let mut out = Vec::with_capacity(poly.len() + 1);
        for (k, &(p, c)) in poly.iter().enumerate() {
            let (q, d) = poly[(k + 1) % poly.len()];
            let (sa, sb) = (side(c), side(d));
            if sa >= 0.0 { out.push((p, c)); }
            if (sa >= 0.0) != (sb >= 0.0) {
                let t = sa / (sa - sb);
                out.push((p.lerp(q, t), c.lerp(d, t)));
            }
        }
        poly = out;
        if poly.is_empty() { break; }
    }
    poly.into_iter().map(|(p, _)| p).collect()
}

/// The points of a region's part in view ([`visible_part`]) tested for a line of sight ([`seen_share`]): its centre, and its
/// corners, edge midpoints and the corners' midpoints to the centre pulled [`SEEN_INSET`] toward it, [`SEEN_LIFT`] up.
pub fn sight_points(part: &[Vec3]) -> Vec<Vec3> {
    if part.is_empty() { return Vec::new(); }
    let c = part.iter().copied().sum::<Vec3>() / part.len() as f32;
    let lift = Vec3::new(0.0, 0.0, SEEN_LIFT);
    let mut out = vec![c + lift];
    for (k, &p) in part.iter().enumerate() {
        let mid = p.lerp(part[(k + 1) % part.len()], 0.5);
        for q in [p, mid, p.lerp(c, 0.5)] { out.push(q.lerp(c, SEEN_INSET) + lift); }
    }
    out
}

/// The share of a region's part in view ([`visible_part`]) seen from `eye`: of its [`sight_points`], those beyond
/// [`SEEN_REACH`] and those with nothing between them and the eye (`blocked(eye, point)`; crate::water_mirror `place`
/// asks the level's collision mesh, its water faces left out).
pub fn seen_share(eye: Vec3, part: &[Vec3], blocked: &mut impl FnMut(Vec3, Vec3) -> bool) -> f32 {
    let pts = sight_points(part);
    if pts.is_empty() { return 0.0; }
    pts.iter().filter(|p| p.distance(eye) > SEEN_REACH || !blocked(eye, **p)).count() as f32 / pts.len() as f32
}

/// Sight points farther than this from the eye (game units) count as seen untested ([`seen_share`]: far water is a thin
/// band of the screen, and the long lines are the costly ones).
pub const SEEN_REACH: f32 = 160.0;
/// The plane choice runs every this many frames ([`choose_plane`]'s lines of sight; the [`fade`] between planes is
/// longer).
pub const CHOOSE_EVERY: u32 = 4;

/// The screen area of a part in view ([`visible_part`]: in front of the eye), in NDC (the whole view is 4).
pub fn screen_area(clip_from_world: &Mat4, part: &[Vec3]) -> f32 {
    let ndc: Vec<Vec2> = part
        .iter()
        .map(|p| {
            let c = *clip_from_world * game_to_bevy(p.to_array()).extend(1.0);
            Vec2::new(c.x, c.y) / c.w.max(NEAR_W)
        })
        .collect();
    let n = ndc.len();
    (0..n).map(|k| ndc[k].perp_dot(ndc[(k + 1) % n])).sum::<f32>().abs() * 0.5
}

/// A sight point's pull toward the part's centre ([`sight_points`]: off the frustum's edges, where the ground meets the
/// water).
pub const SEEN_INSET: f32 = 0.15;
/// A sight point's lift over its plane (game units; [`sight_points`]): above the water's own collision faces.
pub const SEEN_LIFT: f32 = 0.15;
/// How much more of the screen another plane must cover to take over from the drawn one ([`choose_plane`]).
pub const STICK: f32 = 1.5;

/// The plane to mirror (game z) and the planes' scores. Each region the eye (game) is [`MIN_ABOVE`] over whose
/// rectangle is partly in the view frustum ([`visible_part`]) scores the screen area of that part ([`screen_area`])
/// times the share of it seen ([`seen_share`]); the regions of one height ([`SAME_PLANE`]: a sea's blocks) add up. The
/// plane covering the most of the screen wins (Rilgar's start: the bay over the beach's ripple pools at 61.2, which
/// keep the screen-space reflection), but the `drawn` one stays until another covers [`STICK`] times as much. Nothing
/// seen at all (lines cut by an invisible wall): by area alone.
pub fn choose_plane(eye: Vec3, clip_from_world: &Mat4, regions: &[FlatWater], drawn: Option<f32>, mut blocked: impl FnMut(Vec3, Vec3) -> bool) -> (Option<f32>, Vec<(f32, f32)>) {
    // (z, area, area seen) per height.
    let mut planes: Vec<(f32, f32, f32)> = Vec::new();
    for f in regions.iter().filter(|f| eye.z > f.z + MIN_ABOVE) {
        let part = visible_part(clip_from_world, f);
        if part.is_empty() { continue; }
        let area = screen_area(clip_from_world, &part);
        let seen = area * seen_share(eye, &part, &mut blocked);
        match planes.iter_mut().find(|p| (p.0 - f.z).abs() <= SAME_PLANE) {
            Some(p) => {
                p.1 += area;
                p.2 += seen;
            }
            None => planes.push((f.z, area, seen)),
        }
    }
    let by_seen = planes.iter().any(|p| p.2 > 0.0);
    let scores: Vec<(f32, f32)> = planes.iter().map(|p| (p.0, if by_seen { p.2 } else { p.1 })).collect();
    let best = scores.iter().copied().max_by(|a, b| a.1.total_cmp(&b.1));
    let kept = drawn.and_then(|d| scores.iter().copied().find(|p| (p.0 - d).abs() <= SAME_PLANE));
    let choice = match (best, kept) {
        (Some(b), Some(k)) if b.1 <= k.1 * STICK => Some(k.0),
        (b, _) => b.map(|b| b.0),
    };
    (choice, scores)
}

/// Two choices this close (game units) are the same plane ([`fade`]: a sea's height moves with its swell; the shader
/// reads the mirror within 0.5 of its plane at full weight).
pub const SAME_PLANE: f32 = 0.5;
/// The mirror's fade in and out (seconds; [`fade`]).
pub const FADE_SECS: f32 = 0.2;

/// The plane drawn and its weight after a frame of `dt` seconds (`water.wgsl` `mirror.x`), given the drawn one and this
/// frame's choice ([`choose_plane`]): the same plane ([`SAME_PLANE`]) follows the choice and fades in; another choice
/// (or none) fades the drawn one out over [`FADE_SECS`] (the water's screen-space reflection takes over) and then
/// takes its place, so a change of plane is never a hard cut.
pub fn fade(drawn: Option<f32>, weight: f32, want: Option<f32>, dt: f32) -> (Option<f32>, f32) {
    let step = dt.max(0.0) / FADE_SECS;
    match (drawn, want) {
        (Some(d), Some(w)) if (d - w).abs() <= SAME_PLANE => (Some(w), (weight + step).min(1.0)),
        (Some(d), _) if weight - step > 0.0 => (Some(d), weight - step),
        (Some(_), None) | (None, None) => (None, 0.0),
        (_, Some(w)) => (Some(w), step.min(1.0)),
    }
}

/// The mirror view's world-from-view for a camera's `g` and the plane y = `h` (Bevy): R·G·F, R the reflection about
/// the plane and F the view's x turned back (det +1: a proper rotation; the picture is mirrored left to right).
pub fn mirror_view(g: Affine3A, h: f32) -> Affine3A {
    let r = Vec3A::new(1.0, -1.0, 1.0);
    let m = g.matrix3;
    let t = g.translation;
    Affine3A { matrix3: bevy::math::Mat3A::from_cols(-(m.x_axis * r), m.y_axis * r, m.z_axis * r), translation: Vec3A::new(t.x, 2.0 * h - t.y, t.z) }
}

/// The plane y = `h` (kept above) in the view space of `world_from_view`.
pub fn view_plane(world_from_view: Affine3A, h: f32) -> Vec4 { Mat4::from(world_from_view).transpose() * Vec4::new(0.0, 1.0, 0.0, -h) }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Off,
    On,
    Debug,
}

/// The mirror picture, the water's `mirror` buffer and the cameras.
#[derive(Resource)]
pub struct WaterMirror {
    pub image: Handle<Image>,
    /// `(the mirror's weight while drawn, [`fade`], else 0; the plane's Bevy y, 0, 0)` (`water.wgsl` `mirror`).
    pub params: Handle<ShaderBuffer>,
    size: UVec2,
    mode: Mode,
    /// [sky, world], spawned on the first frame that mirrors.
    cams: Option<[Entity; 2]>,
    inset: Option<Entity>,
    /// This frame's mirror world-from-view ([`pin`]).
    view: Option<Affine3A>,
    regions: Vec<FlatWater>,
    /// The plane drawn and its weight ([`fade`]).
    drawn: Option<f32>,
    weight: f32,
    /// Frames counted for [`CHOOSE_EVERY`]; the last choice and the region count it saw.
    age: u32,
    chosen: (Option<f32>, usize),
    /// `debug`: the region count and plane last printed.
    logged: (usize, Option<f32>),
}

/// Both mirror cameras.
#[derive(Component)]
struct MirrorCam;
/// The world mirror camera ([`keep_world_draws`]).
#[derive(Component)]
struct MirrorWorld;

pub struct WaterMirrorPlugin;

impl Plugin for WaterMirrorPlugin {
    fn build(&self, app: &mut App) {
        let mode = match std::env::var("RC_WATER_MIRROR").ok().as_deref().map(str::trim) {
            Some("0") => Mode::Off,
            Some("debug") => Mode::Debug,
            _ => Mode::On,
        };
        let image = app.world_mut().resource_mut::<Assets<Image>>().add(target_image(UVec2::ONE));
        let params = app.world_mut().resource_mut::<Assets<ShaderBuffer>>().add(ShaderBuffer::new(&[0u8; 16], RenderAssetUsages::default()));
        app.insert_resource(WaterMirror { image, params, size: UVec2::ONE, mode, cams: None, inset: None, view: None, regions: Vec::new(), drawn: None, weight: 0.0, age: 0, chosen: (None, 0), logged: (0, None) }).add_systems(
            PostUpdate,
            (
                place
                    .after(crate::water_render::draw)
                    .after(crate::sea_water::draw)
                    .after(crate::sky_render::follow_main_camera)
                    .before(CameraUpdateSystems)
                    .before(TransformSystems::Propagate),
                pin.after(TransformSystems::Propagate).before(VisibilitySystems::UpdateFrusta),
                keep_world_draws.after(VisibilitySystems::CheckVisibility).before(VisibilitySystems::MarkNewlyHiddenEntitiesInvisible),
                no_sun_cascades
                    .after(SimulationLightSystems::UpdateDirectionalLightCascades)
                    .before(SimulationLightSystems::UpdateLightFrusta),
            ),
        );
    }
}

/// The sun's cascades emptied for the mirror cameras: bevy_light `build_directional_light_cascades` makes them for
/// every active camera, and the mirror reads none (crate::sun_shadow's receiver and the passes after it run on
/// `ShadowCamera` views only), so it skips three shadow views queueing every caster. The entry stays: bevy_pbr
/// `prepare_lights` unwraps it.
fn no_sun_cascades(cams: Query<Entity, With<MirrorCam>>, mut lights: Query<&mut Cascades>) {
    for mut lc in &mut lights {
        for e in &cams {
            if lc.cascades.get(&e).is_some_and(|c| !c.is_empty()) {
                lc.cascades.insert(e, Vec::new());
            }
        }
    }
}

/// The mirror's render target (bilinear, clamped: the water reads it between texels).
fn target_image(size: UVec2) -> Image {
    let mut img = Image::new_uninit(Extent3d { width: size.x, height: size.y, depth_or_array_layers: 1 }, TextureDimension::D2, FORMAT, RenderAssetUsages::default());
    img.texture_descriptor.usage = TextureUsages::TEXTURE_BINDING | TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_SRC;
    img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::ClampToEdge,
        address_mode_v: ImageAddressMode::ClampToEdge,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        ..default()
    });
    img
}

fn spawn_cameras(commands: &mut Commands, image: &Handle<Image>, t: Transform, p: GameProjection) -> [Entity; 2] {
    let base = |order: isize| {
        (
            Camera { order, is_active: false, ..default() },
            RenderTarget::Image(image.clone().into()),
            Projection::custom(p),
            Msaa::Off,
            Tonemapping::None,
            DebandDither::Disabled,
            t,
            MirrorCam,
            crate::level_switch::KeepAcrossLevels,
        )
    };
    let sky = commands.spawn((Camera3d { depth_load_op: Camera3dDepthLoadOp::Clear(0.0), ..default() }, base(ORDER_SKY), RenderLayers::layer(SKY_LAYER), Name::new("water mirror sky camera"))).id();
    let world = commands.spawn((Camera3d::default(), base(ORDER_WORLD), MirrorWorld, Name::new("water mirror world camera"))).id();
    [sky, world]
}

type MainCam<'w, 's> = Query<'w, 's, (&'static Transform, &'static Projection, &'static Camera), (With<FlyCam>, Without<MirrorCam>)>;
type MirrorCams<'w, 's> = Query<'w, 's, (&'static mut Transform, &'static mut Projection, &'static mut Camera, &'static mut Camera3d, Has<MirrorWorld>), (With<MirrorCam>, Without<FlyCam>)>;

/// The plane of this frame, the target's size, the cameras' state and the water's `mirror` buffer.
#[allow(clippy::too_many_arguments)]
fn place(
    mut commands: Commands,
    g: Res<GraphicsSettings>,
    mut mirror: ResMut<WaterMirror>,
    water: Option<Res<crate::water_render::WaterState>>,
    sea: Option<Res<crate::sea_water::SeaWaterDraw>>,
    play: Option<Res<crate::gameplay::Play>>,
    level: Option<Res<crate::Level>>,
    time: Res<Time<Real>>,
    main: MainCam,
    mut cams: MirrorCams,
    mut images: ResMut<Assets<Image>>,
    mut buffers: ResMut<Assets<ShaderBuffer>>,
    mut water_mats: ResMut<Assets<crate::water_render::WaterMaterial>>,
    sea_mats: Option<ResMut<Assets<crate::sea_water::SeaWaterMaterial>>>,
    mut inset: Query<&mut Visibility>,
) {
    let m = &mut *mirror;
    let Some((t, proj, cam)) = main.iter().next() else { return };
    let base = match proj {
        Projection::Custom(c) => c.get::<GameProjection>().copied().unwrap_or_default(),
        _ => GameProjection::default(),
    };
    let base = GameProjection { clip: Vec4::ZERO, ..base };
    let want = m.mode != Mode::Off && g.water.on();
    m.regions.clear();
    if want {
        if let Some(w) = water.as_deref() { w.flat_surfaces(play.as_deref().and_then(|p| p.svc.water.sim.as_ref()), &mut m.regions); }
        if let Some(s) = sea.as_deref() { m.regions.extend_from_slice(&s.planes); }
    }
    let g_main = GlobalTransform::from(*t).affine();
    let clip_from_world = base.get_clip_from_view() * Mat4::from(g_main.inverse());
    // Every face but the water's (surface 0, as the hero's capsule leaves it out) hides what is behind it.
    use rc_game::collision_query::{coll_line, QueryFlags};
    let coll = level.as_deref().and_then(|l| l.0.collision.as_ref());
    let blocked = |a: Vec3, b: Vec3| coll.is_some_and(|c| coll_line(c, a.to_array(), b.to_array(), QueryFlags::TWO_SIDED | QueryFlags::exclude_surface(0)).is_some());
    let t0 = std::time::Instant::now();
    m.age = m.age.wrapping_add(1);
    let due = m.age.is_multiple_of(CHOOSE_EVERY) || m.regions.len() != m.chosen.1;
    // Between choices the chosen plane at this frame's height (a sea's moves with its swell).
    let now = |z: f32| m.regions.iter().find(|f| (f.z - z).abs() <= SAME_PLANE).map(|f| f.z);
    let (choice, scores) = match () {
        _ if m.regions.is_empty() => (None, Vec::new()),
        _ if due => choose_plane(game_eye(t), &clip_from_world, &m.regions, m.drawn, blocked),
        _ => (m.chosen.0.and_then(now), Vec::new()),
    };
    if due { m.chosen = (choice, m.regions.len()); }
    let cost = t0.elapsed().as_secs_f64() * 1e3;
    (m.drawn, m.weight) = fade(m.drawn, m.weight, choice, time.delta_secs());
    let plane = m.drawn;
    if m.mode == Mode::Debug && (m.regions.len() != m.logged.0 || choice.map(|z| z.round()) != m.logged.1.map(|z| z.round())) {
        m.logged = (m.regions.len(), choice);
        let list: Vec<String> = m.regions.iter().take(8).map(|f| format!("z {:.2} x {:.1}..{:.1} y {:.1}..{:.1}", f.z, f.min.x, f.max.x, f.min.y, f.max.y)).collect();
        let s: Vec<String> = scores.iter().map(|(z, s)| format!("z {z:.2}: {s:.3}")).collect();
        println!("water mirror: plane {choice:?} (scores {}; {cost:.2} ms; drawn {plane:?} at {:.2}) of {} flat regions [{}]", s.join(", "), m.weight, m.regions.len(), list.join("; "));
    }

    // The target: half the main view, 1×1 while nothing mirrors (every water material re-binds on a resize).
    let size = match plane {
        Some(_) => cam.physical_viewport_size().map_or(UVec2::ONE, |s| ((s + 1) / 2).max(UVec2::ONE)),
        None if want => m.size,
        None => UVec2::ONE,
    };
    if size != m.size {
        if let Some(mut img) = images.get_mut(&m.image) { img.resize(Extent3d { width: size.x, height: size.y, depth_or_array_layers: 1 }); }
        m.size = size;
        let ids: Vec<_> = water_mats.ids().collect();
        for id in ids { water_mats.get_mut(id); }
        if let Some(mut sea_mats) = sea_mats {
            let ids: Vec<_> = sea_mats.ids().collect();
            for id in ids { sea_mats.get_mut(id); }
        }
    }

    if plane.is_some() && m.cams.is_none() { m.cams = Some(spawn_cameras(&mut commands, &m.image, *t, base)); }
    m.view = plane.map(|z| mirror_view(g_main, z));
    let clip = m.view.zip(plane).map_or(Vec4::ZERO, |(v, z)| view_plane(v, z - CLIP_BIAS));
    for (mut ct, mut cp, mut cc, mut c3, world) in &mut cams {
        if *ct != *t { *ct = *t; }
        let want_p = GameProjection { clip: if world { clip } else { Vec4::ZERO }, ..base };
        let same = matches!(&*cp, Projection::Custom(c) if c.get::<GameProjection>() == Some(&want_p));
        if !same { *cp = Projection::custom(want_p); }
        let active = plane.is_some();
        if cc.is_active != active { cc.is_active = active; }
        // crate::sky_render `main_camera_loads` sets every non-sky camera to load: the mirror sky clears again.
        let (clear, load) = if world { (ClearColorConfig::None, Camera3dDepthLoadOp::Load) } else { (ClearColorConfig::Default, Camera3dDepthLoadOp::Clear(0.0)) };
        if !matches!((&cc.clear_color, &clear), (ClearColorConfig::None, ClearColorConfig::None) | (ClearColorConfig::Default, ClearColorConfig::Default)) { cc.clear_color = clear; }
        let output = if world { CameraOutputMode::Write { blend_state: Some(BlendState::REPLACE), clear_color: ClearColorConfig::None } } else { CameraOutputMode::Skip };
        if matches!(cc.output_mode, CameraOutputMode::Skip) != matches!(output, CameraOutputMode::Skip) { cc.output_mode = output; }
        if matches!(c3.depth_load_op, Camera3dDepthLoadOp::Load) != world { c3.depth_load_op = load; }
    }

    let words = [if plane.is_some() { m.weight } else { 0.0 }, plane.unwrap_or(0.0), 0.0, 0.0];
    let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
    crate::asset_write::set_buffer(&mut buffers, &m.params, &bytes);

    if m.mode == Mode::Debug {
        let show = if plane.is_some() { Visibility::Inherited } else { Visibility::Hidden };
        match m.inset {
            None if plane.is_some() => {
                let node = Node { position_type: PositionType::Absolute, right: Val::Percent(1.0), top: Val::Percent(1.0), width: Val::Percent(33.0), height: Val::Percent(33.0), ..default() };
                let img = ImageNode { image: m.image.clone(), flip_x: true, ..default() };
                m.inset = Some(commands.spawn((node, img, GlobalZIndex(100), crate::level_switch::KeepAcrossLevels, Name::new("water mirror debug inset"))).id());
            }
            Some(e) => {
                if let Ok(mut v) = inset.get_mut(e) {
                    if *v != show { *v = show; }
                }
            }
            None => {}
        }
    }
}

/// The mirror cameras' `GlobalTransform` after propagation (their `Transform` stays the main camera's).
fn pin(mirror: Res<WaterMirror>, mut cams: Query<&mut GlobalTransform, With<MirrorCam>>) {
    let Some(v) = mirror.view else { return };
    for mut gt in &mut cams { *gt = GlobalTransform::from(v); }
}

type WorldDraw = Or<(
    With<MeshMaterial3d<crate::tfrag_render::TfragMaterial>>,
    With<MeshMaterial3d<crate::tie_render::TieMaterial>>,
    With<MeshMaterial3d<crate::moby_render::MobyMaterial>>,
    With<MeshMaterial3d<crate::moby_render::MobyMetalMaterial>>,
)>;

/// What the mirror cameras draw (module doc): the world camera keeps the tfrag, tie and moby draws but the effects,
/// the sky camera the sky shells.
fn keep_world_draws(
    mut cams: Query<(&Camera, &mut VisibleEntities, Has<MirrorWorld>), With<MirrorCam>>,
    world: Query<(), (WorldDraw, Without<crate::display_blend::DisplayEffect>)>,
    sky: Query<(), With<MeshMaterial3d<crate::sky_render::SkyMaterial>>>,
) {
    for (cam, mut ve, is_world) in &mut cams {
        if !cam.is_active { continue; }
        let list = ve.get_mut(TypeId::of::<Mesh3d>());
        if is_world { list.retain(|e| world.contains(*e)); } else { list.retain(|e| sky.contains(*e)); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game_camera::GameProjection;
    use bevy::camera::CameraProjection;

    fn flat(z: f32, min: [f32; 2], max: [f32; 2]) -> FlatWater { FlatWater { z, min: Vec2::from(min), max: Vec2::from(max) } }

    /// Nothing between the eye and any point.
    fn clear(_: Vec3, _: Vec3) -> bool { false }

    #[test]
    fn the_plane_seen_over_most_of_the_screen_wins() {
        // Rilgar's start: a patch at 61.2 near the eye, the bay at 59.5 beyond it.
        let g = cam([340.0, 110.0, 65.0], [340.0, 150.0, 58.0]);
        let clip = GameProjection::default().get_clip_from_view() * Mat4::from(g.inverse());
        let eye = Vec3::new(340.0, 110.0, 65.0);
        let patch = flat(61.2, [332.0, 112.0], [348.0, 128.0]);
        let bay = flat(59.5, [300.0, 140.0], [380.0, 300.0]);
        let area = |f: &FlatWater| screen_area(&clip, &visible_part(&clip, f));
        let (a_patch, a_bay) = (area(&patch), area(&bay));
        assert!(a_patch > a_bay && a_bay > 0.0 && a_patch < 4.0, "{a_patch} {a_bay}");
        assert_eq!(choose_plane(eye, &clip, &[bay, patch], None, clear).0, Some(61.2));
        // The sand at 62.9 over the patch's rectangle hides every point of it, and the bay's nearest points.
        let sand = |a: Vec3, b: Vec3| {
            let t = (62.9 - a.z) / (b.z - a.z);
            let p = a.lerp(b, t);
            (0.0..=1.0).contains(&t) && (332.0..=348.0).contains(&p.x) && (112.0..=128.0).contains(&p.y)
        };
        let mut s = sand;
        assert_eq!(seen_share(eye, &visible_part(&clip, &patch), &mut s), 0.0);
        let bay_seen = seen_share(eye, &visible_part(&clip, &bay), &mut s);
        assert!(bay_seen > 0.3 && bay_seen < 1.0, "{bay_seen}");
        let (choice, scores) = choose_plane(eye, &clip, &[bay, patch], None, sand);
        assert_eq!(choice, Some(59.5));
        assert_eq!(scores.len(), 2);
        // Nothing seen at all within SEEN_REACH (a bay that ends inside it): by area alone.
        let near_bay = flat(59.5, [300.0, 140.0], [380.0, 250.0]);
        assert!(visible_part(&clip, &near_bay).iter().all(|p| p.distance(eye) < SEEN_REACH));
        assert_eq!(choose_plane(eye, &clip, &[near_bay, patch], None, |_, _| true).0, Some(61.2));
        // The far points of the long bay count as seen untested.
        assert!(seen_share(eye, &visible_part(&clip, &bay), &mut |_, _| true) > 0.0);
        let pts = sight_points(&visible_part(&clip, &patch));
        assert!(pts.len() > 4 && pts.iter().all(|p| (p.z - 61.2 - SEEN_LIFT).abs() < 1e-4));
    }

    #[test]
    fn the_drawn_plane_stays_until_another_covers_much_more() {
        let g = cam([0.0, 0.0, 10.0], [0.0, 20.0, 5.0]);
        let clip = GameProjection::default().get_clip_from_view() * Mat4::from(g.inverse());
        let eye = Vec3::new(0.0, 0.0, 10.0);
        // Two blocks of one sea at 2.0 (2.03: its swell) side by side, a pool at 4.0 a little bigger than either.
        let (left, right) = (flat(2.0, [-6.0, 12.0], [0.0, 20.0]), flat(2.03, [0.0, 12.0], [6.0, 20.0]));
        let pool = flat(4.0, [-4.0, 9.0], [4.0, 15.0]);
        let area = |f: &FlatWater| screen_area(&clip, &visible_part(&clip, f));
        let (a_l, a_r, a_p) = (area(&left), area(&right), area(&pool));
        assert!(a_p > a_l && a_p > a_r && a_p < a_l + a_r, "{a_l} {a_r} {a_p}");
        // The sea's blocks add up.
        let (choice, scores) = choose_plane(eye, &clip, &[left, pool, right], None, clear);
        assert_eq!(choice, Some(2.0));
        assert_eq!(scores.len(), 2);
        // The pool alone against the left block: it wins unless the block is drawn and it covers less than STICK times.
        assert_eq!(choose_plane(eye, &clip, &[left, pool], None, clear).0, Some(4.0));
        let kept = choose_plane(eye, &clip, &[left, pool], Some(2.01), clear).0;
        assert_eq!(kept, if a_p > a_l * STICK { Some(4.0) } else { Some(2.0) });
        assert_eq!(choose_plane(eye, &clip, &[pool], Some(2.0), clear).0, Some(4.0), "a drawn plane out of view gives way");
    }

    #[test]
    fn a_change_of_plane_fades_out_then_in() {
        let dt = FADE_SECS * 0.3;
        let near = |got: (Option<f32>, f32), d: Option<f32>, w: f32| got.0 == d && (got.1 - w).abs() < 1e-5;
        // In from nothing, to full.
        let (mut d, mut w) = fade(None, 0.0, Some(59.5), dt);
        assert!(near((d, w), Some(59.5), 0.3));
        for _ in 0..5 { (d, w) = fade(d, w, Some(59.49), dt); }
        assert_eq!((d, w), (Some(59.49), 1.0), "the same plane follows its swell");
        // Another plane: the drawn one fades out first, then the new one comes in.
        let mut steps = Vec::new();
        for _ in 0..8 {
            (d, w) = fade(d, w, Some(61.2), dt);
            steps.push((d, w));
        }
        assert!(near(steps[0], Some(59.49), 0.7) && near(steps[1], Some(59.49), 0.4) && near(steps[2], Some(59.49), 0.1));
        assert!(near(steps[3], Some(61.2), 0.3) && near(steps[4], Some(61.2), 0.6));
        assert_eq!(steps[7], (Some(61.2), 1.0));
        // Out to nothing.
        for _ in 0..4 { (d, w) = fade(d, w, None, dt); }
        assert_eq!((d, w), (None, 0.0));
        // Back to the drawn plane mid-fade: it comes back in without a switch.
        let (d2, w2) = fade(Some(1.0), 0.5, Some(3.0), dt);
        assert!(near(fade(d2, w2, Some(1.0), dt), Some(1.0), 0.5));
    }

    /// A camera at game `eye` looking at game `target` (Bevy world-from-view).
    fn cam(eye: [f32; 3], target: [f32; 3]) -> Affine3A {
        GlobalTransform::from(Transform::from_translation(game_to_bevy(eye)).looking_at(game_to_bevy(target), Vec3::Y)).affine()
    }

    #[test]
    fn flat_region_takes_the_median_height_and_drops_the_drop() {
        let mut pts: Vec<[f32; 3]> = (0..10).map(|k| [k as f32, (k % 3) as f32, 59.5]).collect();
        // A wall dropping at the sea's edge.
        pts.extend([[20.0, 0.0, 40.0], [20.0, 5.0, 40.0], [21.0, 2.0, 45.0]]);
        let f = flat_region(pts).unwrap();
        assert_eq!(f.z, 59.5);
        assert_eq!((f.min, f.max), (Vec2::new(0.0, 0.0), Vec2::new(9.0, 2.0)));
        assert!(flat_region([[0.0, 0.0, 1.0], [1.0, 0.0, 1.0]]).is_none());
        assert!(flat_region([[0.0, 0.0, 1.0], [1.0, 0.0, 3.0], [2.0, 0.0, 5.0]]).is_none());
    }

    #[test]
    fn plane_choice_is_the_nearest_on_screen_plane_below_the_eye() {
        let g = cam([0.0, 0.0, 10.0], [0.0, 20.0, 5.0]);
        let clip = GameProjection::default().get_clip_from_view() * Mat4::from(g.inverse());
        let eye = Vec3::new(0.0, 0.0, 10.0);
        let ahead_far = flat(2.0, [-5.0, 30.0], [5.0, 40.0]);
        let ahead_near = flat(4.0, [-5.0, 8.0], [5.0, 16.0]);
        let behind = flat(6.0, [-5.0, -40.0], [5.0, -30.0]);
        let above = flat(12.0, [-50.0, -50.0], [50.0, 50.0]);
        assert!(!visible_part(&clip, &ahead_far).is_empty() && !visible_part(&clip, &ahead_near).is_empty());
        assert!(visible_part(&clip, &behind).is_empty());
        assert_eq!(choose_plane(eye, &clip, &[ahead_far, behind, above, ahead_near], None, clear).0, Some(4.0));
        assert_eq!(choose_plane(eye, &clip, &[behind, above], None, clear).0, None);
        // Just under the eye is not enough ([`MIN_ABOVE`]).
        assert_eq!(choose_plane(eye, &clip, &[flat(9.95, [-5.0, 8.0], [5.0, 16.0])], None, clear).0, None);
        // A plane the camera stands over counts even when its corners are all around the view.
        assert_eq!(choose_plane(eye, &clip, &[flat(1.0, [-500.0, -500.0], [500.0, 500.0])], None, clear).0, Some(1.0));
    }

    #[test]
    fn plane_choice_skips_a_nearer_region_out_of_view() {
        // Looking along +y from 10 over the origin, 60° wide: a region right under and beside the eye but out of the
        // frustum, one off to the side (its corners outside different side planes, so no single plane culls them
        // all), and a far one in view.
        let g = cam([0.0, 0.0, 10.0], [0.0, 20.0, 9.0]);
        let clip = GameProjection::default().get_clip_from_view() * Mat4::from(g.inverse());
        let eye = Vec3::new(0.0, 0.0, 10.0);
        let under = flat(5.0, [-3.0, -6.0], [3.0, -1.0]);
        let side = flat(6.0, [30.0, -40.0], [300.0, 3.0]);
        let ahead = flat(2.0, [-5.0, 60.0], [5.0, 70.0]);
        assert!(visible_part(&clip, &under).is_empty());
        assert!(visible_part(&clip, &side).is_empty(), "{:?}", visible_part(&clip, &side));
        assert!(!visible_part(&clip, &ahead).is_empty());
        assert_eq!(choose_plane(eye, &clip, &[under, side, ahead], None, clear).0, Some(2.0));
        // Partly in view: only the part in view counts, and its area is a share of the screen.
        let long = flat(3.0, [-2.0, -5.0], [2.0, 200.0]);
        let part = visible_part(&clip, &long);
        assert!(!part.is_empty() && part.iter().all(|p| p.y > 0.0 && p.z == 3.0));
        let a = screen_area(&clip, &part);
        assert!(a > 0.0 && a < 4.0, "{a}");
        // From over a plane larger than the view, looking a little down: about the screen under the horizon.
        let floor = screen_area(&clip, &visible_part(&clip, &flat(1.0, [-5000.0, -5000.0], [5000.0, 5000.0])));
        assert!((1.8..=2.6).contains(&floor), "{floor}");
        assert_eq!(screen_area(&clip, &[]), 0.0);
    }

    #[test]
    fn flat_blocks_split_a_surface_by_block_at_its_height() {
        // Two blocks of four points at 59.5 far apart, a third block on the drop (40) and a broken point.
        let mut pts = vec![[0.0, 0.0, 59.5], [2.0, 0.0, 59.5], [0.0, 2.0, 59.5], [2.0, 2.0, 59.5]];
        pts.extend([[100.0, 50.0, 59.5], [103.0, 50.0, 59.5], [100.0, 52.0, 59.5], [f32::NAN, 0.0, 59.5]]);
        pts.extend([[200.0, 0.0, 40.0], [201.0, 0.0, 40.0], [200.0, 1.0, 40.0], [201.0, 1.0, 40.0]]);
        let b = flat_blocks(&pts, 4);
        assert_eq!(b, [flat(59.5, [0.0, 0.0], [2.0, 2.0]), flat(59.5, [100.0, 50.0], [103.0, 52.0])]);
        assert!(flat_blocks(&pts[..2], 4).is_empty());
    }

    #[test]
    fn mirror_view_is_the_reflection_with_x_turned_back() {
        let h = 3.0;
        let g = cam([4.0, -2.0, 9.0], [10.0, 15.0, 1.0]);
        let m = mirror_view(g, h);
        assert!((m.matrix3.determinant() - 1.0).abs() < 1e-5, "a proper rotation");
        let reflect = |p: Vec3| Vec3::new(p.x, 2.0 * h - p.y, p.z);
        for p in [Vec3::new(1.0, 5.0, -3.0), Vec3::new(-7.0, 3.5, 12.0), Vec3::new(0.0, 0.0, 0.0)] {
            let a = m.inverse().transform_point3(p);
            let b = g.inverse().transform_point3(reflect(p));
            assert!((a - Vec3::new(-b.x, b.y, b.z)).length() < 1e-4, "{a} vs {b}");
        }
        // The mirrored eye is under the plane by as much as the eye is over it.
        assert!((m.translation.y - (2.0 * h - g.translation.y)).abs() < 1e-6);
    }

    #[test]
    fn oblique_near_clips_at_the_water_and_keeps_the_view() {
        let h = 2.0;
        let g = cam([0.0, 0.0, 8.0], [0.0, 25.0, 4.0]);
        let mv = mirror_view(g, h);
        let base = GameProjection::default();
        let p = GameProjection { clip: view_plane(mv, h), ..base };
        let (m0, m) = (base.get_clip_from_view(), p.get_clip_from_view());
        assert_eq!((m.row(0), m.row(1), m.row(3)), (m0.row(0), m0.row(1), m0.row(3)), "x, y and w unchanged");
        let view_from_world = mv.inverse();
        let ndc = |w: Vec3| {
            let c = m * view_from_world.transform_point3(w).extend(1.0);
            (c.truncate() / c.w, c)
        };
        // Points on the mirror camera's axis (from (0, 0, −4) toward (0, 25, 0)) past the water: inside the depth
        // range, nearer = larger depth.
        let mut last = f32::MAX;
        for d in [40.0f32, 50.0, 80.0, 150.0, 200.0] {
            let w = game_to_bevy([0.0, d, -4.0 + d * 4.0 / 25.0]);
            let (n, c) = ndc(w);
            assert!(c.w > 0.0 && n.z > 0.0 && n.z <= 1.0, "depth {d}: {n}");
            assert!(n.x.abs() <= 1.0 && n.y.abs() <= 1.0, "depth {d}: {n}");
            assert!(n.z < last, "reverse Z along the view: {d}");
            last = n.z;
        }
        // On the plane: the near plane (depth 1); under it: clipped (z > w).
        let (n, _) = ndc(game_to_bevy([0.0, 10.0, h]));
        assert!((n.z - 1.0).abs() < 1e-3, "{n}");
        for d in [5.0, 10.0, 30.0] {
            let (_, c) = ndc(game_to_bevy([0.0, d, h - 0.5]));
            assert!(c.z > c.w, "under the water at {d} is clipped");
        }
        // No plane, no change.
        assert_eq!(GameProjection { clip: Vec4::ZERO, ..base }.get_clip_from_view(), m0);
    }
}
