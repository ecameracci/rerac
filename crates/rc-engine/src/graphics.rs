//! **Port-only: the graphics options** (docs/plan/graphics_options.md). Every option controls one thing, and its first
//! value is always `Original`: what the PS2 showed on a TV. The others are modern alternatives. A [`Preset`] (Original,
//! Low, Medium, High) sets them all at once ([`GraphicsSettings::with_preset`], one line per option); the row shows
//! Custom when the options match no preset ([`GraphicsSettings::preset`]). The preset is also stored (`preset`): an
//! option the file lacks (one added since it was written) takes the stored preset's value ([`GraphicsSettings::from_file`]).
//!
//! | Option | Values | Consumer |
//! |---|---|---|
//! | [`AntiAliasing`] | Original (the game's own softening passes: the AA blit and the display copy), Off, 2x / 4x / 8x multisampling | crate::aa_blit, crate::render_settings (the samples) |
//! | [`Hud`] | Original (the 512×416 HUD scaled up smoothly, as the TV showed it), Sharp pixels | crate::hud_render composite |
//! | [`Textures`] | Original (the GS mip rule, one level per pixel), Smooth (blended levels), Sharp (the GPU's own choice, anisotropic) | the world shaders, through crate::game_camera's shared fog buffer |
//! | [`Detail`] | Original (the game's LOD distances), Far (×2), Farther (×4), Maximum (the full model whenever it is drawn) | crate::tfrag_lod, crate::tie_lod, crate::moby_render, crate::shrub_render (drawing only: the game logic keeps its own decisions) |
//! | [`DrawDistance`] | Original (the game's draw distances), Far (×1.5), Farther (×2), Maximum (×4), the new end faded | crate::tfrag_lod, crate::tie_lod, crate::shrub_render, crate::moby_lod (their drawn flag stays the game's), crate::game_camera (the far plane); not particles (their drawn flag is game state) |
//! | [`AmbientOcclusion`] | Original (none: the shade is the artists' baked vertex colours), On (screen-space occlusion from the depth buffer) | crate::ao_render |
//! | [`Water`] | Original (the GS blend of the water textures), Enhanced (clear by depth, reflections, a sun glint, foam at the shore; under water: absorption, caustics, light shafts) | crate::water_render, crate::sea_water (the seas), crate::scene_copy, crate::underwater_render |
//! | [`Particles`] | Original (sprites cut where they meet the world), Soft (faded by their depth in front of it), Enhanced (Soft, bright effects light the world, glowing stars, restyled rain and snow) | crate::particle_render, crate::scene_copy, crate::particle_lights, crate::sky_stars, crate::flight_render, crate::weather_render |
//! | [`Foliage`] | Original (the level's own plants only), Moderate, Lush (grass clumps on the grassy ground and more of the level's small plants) | crate::foliage |
//! | [`Fur`] | Original (Ratchet as the game draws him), On (shell-layer fur on his furry parts) | crate::fur |
//! | [`Lighting`] | Original (the game's shadows), Simple (sun shadows in their place), Enhanced (Simple's shadows and every advanced lighting effect) | crate::shadow_render, crate::sun_shadow, crate::tie_lod ([`Lighting::game_shadows`], [`Lighting::sun_shadows`]); crate::sun_rays, crate::gi_render, crate::post_render ([`Lighting::enhanced`]; the rays also per level, [`LightShafts`]) |
//! | [`Upscaler`] | Off (the 3D drawn at the frame's resolution), Ultra quality / Quality / Balanced / Performance (drawn at 1/1.3 .. 1/2 of it per axis and upscaled FSR 1-style; the HUD and menus at the frame's) | crate::upscale |
//! | [`FrameSmoothing`] | Off (one picture per 60 Hz tick), On (the display's rate, interpolated between ticks); not in the presets | crate::frame_smooth, crate::frame_pace |
//!
//! **The lighting gate:** a later lighting effect (volumetric light, global illumination) runs only while
//! `GraphicsSettings::lighting.enhanced()` (main world, or the render world's extracted copy), as crate::sun_rays
//! does.
//!
//! Persisted in the port settings file (`crate::render_settings::save_key`); `RC_HUD`-style switches are not used here
//! (`RC_HUD=0` already hides the HUD): `RC_GFX_ANTI_ALIASING`, `RC_GFX_HUD`, `RC_GFX_TEXTURES`, `RC_GFX_DETAIL`,
//! `RC_GFX_DRAW_DISTANCE`, `RC_GFX_AMBIENT_OCCLUSION`, `RC_GFX_WATER`, `RC_GFX_PARTICLES`, `RC_GFX_FOLIAGE`, `RC_GFX_FUR`,
//! `RC_GFX_LIGHTING`, `RC_GFX_UPSCALER`, `RC_GFX_FRAME_SMOOTHING` (the keys below), `RC_MSAA=0|2|4|8` (Anti-aliasing Off or that
//! multisampling), `RC_SHADOWS=1|dynamic` (Lighting Original / Enhanced, the older switch; `0`: crate::shadow_render)
//! and `RC_GFX_PRESET=original|low|medium|high` (the file's options ignored; `enhanced`, the older preset, is Medium)
//! override the file at start.

use bevy::prelude::*;

/// One option: its values in the Port Options row's order (the first is `Original`) and their settings-file keys.
pub trait GfxOption: Copy + PartialEq + Sized + 'static {
    const ALL: &'static [Self];
    /// The settings-file key of the option.
    const KEY: &'static str;
    fn key(self) -> &'static str;
    fn index(self) -> u8 { Self::ALL.iter().position(|&v| v == self).unwrap_or(0) as u8 }
    fn from_index(i: u8) -> Self { Self::ALL.get(i as usize).copied().unwrap_or(Self::ALL[0]) }
    fn parse(v: &str) -> Option<Self> { Self::ALL.iter().copied().find(|a| a.key() == v.trim()) }
}

/// How the world's edges are smoothed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum AntiAliasing {
    /// The game's two full-screen bilinear copies, scaled to the frame (crate::aa_blit).
    #[default]
    Original,
    /// Neither: one sample per pixel, unfiltered.
    Off,
    X2,
    X4,
    X8,
}

impl AntiAliasing {
    /// The world cameras' samples per pixel.
    pub fn samples(self) -> u32 {
        match self {
            AntiAliasing::Original | AntiAliasing::Off => 1,
            AntiAliasing::X2 => 2,
            AntiAliasing::X4 => 4,
            AntiAliasing::X8 => 8,
        }
    }

    /// Multisampling at `n` samples (1 or less: Off).
    pub fn from_samples(n: u32) -> Self {
        match n {
            2 => AntiAliasing::X2,
            4 => AntiAliasing::X4,
            8 => AntiAliasing::X8,
            _ => AntiAliasing::Off,
        }
    }
}

impl GfxOption for AntiAliasing {
    const ALL: &'static [Self] = &[AntiAliasing::Original, AntiAliasing::Off, AntiAliasing::X2, AntiAliasing::X4, AntiAliasing::X8];
    const KEY: &'static str = "anti_aliasing";
    fn key(self) -> &'static str {
        match self {
            AntiAliasing::Original => "original",
            AntiAliasing::Off => "off",
            AntiAliasing::X2 => "2x",
            AntiAliasing::X4 => "4x",
            AntiAliasing::X8 => "8x",
        }
    }
}

/// How the game's 2D screen (HUD, menus, text) reaches the frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Hud {
    /// The 512×416 image scaled to the frame with bilinear filtering, as the TV (and an emulator's display) showed it.
    #[default]
    Original,
    /// Each game pixel a hard square (integer-looking pixel doubling).
    SharpPixels,
}

impl GfxOption for Hud {
    const ALL: &'static [Self] = &[Hud::Original, Hud::SharpPixels];
    const KEY: &'static str = "hud";
    fn key(self) -> &'static str {
        match self {
            Hud::Original => "original",
            Hud::SharpPixels => "sharp",
        }
    }
}

/// How world textures pick their detail level.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Textures {
    /// The GS rule: one whole level from the depth (`round(log2(z / 32) + K)`), bilinear inside it.
    #[default]
    Original,
    /// The same rule without rounding: the two nearest levels blended (no visible switch line).
    Smooth,
    /// The GPU's own level from the screen footprint, with anisotropic filtering.
    Sharp,
}

impl GfxOption for Textures {
    const ALL: &'static [Self] = &[Textures::Original, Textures::Smooth, Textures::Sharp];
    const KEY: &'static str = "textures";
    fn key(self) -> &'static str {
        match self {
            Textures::Original => "original",
            Textures::Smooth => "smooth",
            Textures::Sharp => "sharp",
        }
    }
}

/// How far away models keep their full detail.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Detail {
    #[default]
    Original,
    Far,
    Farther,
    Maximum,
}

impl Detail {
    /// The factor on the game's LOD distances. Maximum: far past every draw distance (the largest is 720 units), finite
    /// so the morph ramps stay defined.
    pub fn lod_scale(self) -> f32 {
        match self {
            Detail::Original => 1.0,
            Detail::Far => 2.0,
            Detail::Farther => 4.0,
            Detail::Maximum => 1000.0,
        }
    }
}

impl GfxOption for Detail {
    const ALL: &'static [Self] = &[Detail::Original, Detail::Far, Detail::Farther, Detail::Maximum];
    const KEY: &'static str = "detail";
    fn key(self) -> &'static str {
        match self {
            Detail::Original => "original",
            Detail::Far => "far",
            Detail::Farther => "farther",
            Detail::Maximum => "maximum",
        }
    }
}

/// How far the world is drawn (docs/plan/graphics_options.md "Draw distance").
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum DrawDistance {
    #[default]
    Original,
    Far,
    Farther,
    Maximum,
}

impl DrawDistance {
    /// The factor on the game's draw distances (tfrag 500, tie ≤ 720, shrub ≤ 500 units) and the far plane (728):
    /// ×1.5 puts every tfrag pixel the game could draw (to the far plane) inside the new 750; ×4 covers the
    /// longest views across a level.
    pub fn scale(self) -> f32 {
        match self {
            DrawDistance::Original => 1.0,
            DrawDistance::Far => 1.5,
            DrawDistance::Farther => 2.0,
            DrawDistance::Maximum => 4.0,
        }
    }
}

impl GfxOption for DrawDistance {
    const ALL: &'static [Self] = &[DrawDistance::Original, DrawDistance::Far, DrawDistance::Farther, DrawDistance::Maximum];
    const KEY: &'static str = "draw_distance";
    fn key(self) -> &'static str {
        match self {
            DrawDistance::Original => "original",
            DrawDistance::Far => "far",
            DrawDistance::Farther => "farther",
            DrawDistance::Maximum => "maximum",
        }
    }
}

/// The fade at the end of a draw range under Draw distance above Original (Original keeps the game's hard cut): the
/// visibility 0..=128 at view depth `z` of a range ending at `end` (world units), over its last 16 units × `scale` (the
/// band the game fades mobys and shrub billboards over), in the moby fade's steps. crate::tie_lod (per instance) and
/// tfrag.wgsl (per pixel) use it; the shaders dither by it ([`end_fade_shown`]).
pub fn end_fade(z: f32, end: f32, scale: f32) -> u32 {
    let steps = (end - z) * (8.0 / scale);
    if steps <= 0.0 { 0 } else { (steps as u32).min(128) }
}

/// The 4×4 ordered dither of the end fade (tie.wgsl / tfrag.wgsl `end_fade_shown`): pixel (x, y) is drawn at
/// visibility `vis` when `vis > 8·B(x, y) + 4`, B the 4×4 Bayer index (0..15, the bits of x ⊕ y and y interleaved), so
/// 0 draws none and 128 every pixel.
#[cfg_attr(not(test), allow(dead_code))] // CPU reference for the shaders, exercised by the tests
pub fn end_fade_shown(vis: u32, x: u32, y: u32) -> bool {
    let e = x ^ y;
    let b = (e & 1) << 3 | (y & 1) << 2 | (e & 2) | (y >> 1) & 1;
    vis > 8 * b + 4
}

/// Two-valued options: `Original`, then the one modern alternative.
macro_rules! two_valued {
    ($(#[$doc:meta])* $name:ident, $key:literal, $(#[$alt_doc:meta])* $alt:ident = $alt_key:literal) => {
        $(#[$doc])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
        pub enum $name {
            #[default]
            Original,
            $(#[$alt_doc])*
            $alt,
        }

        impl GfxOption for $name {
            const ALL: &'static [Self] = &[$name::Original, $name::$alt];
            const KEY: &'static str = $key;
            fn key(self) -> &'static str {
                match self {
                    $name::Original => "original",
                    $name::$alt => $alt_key,
                }
            }
        }

        impl $name {
            pub fn on(self) -> bool { self != $name::Original }
        }
    };
}

two_valued!(
    /// Shade where surfaces meet: Original is the baked vertex colours only.
    AmbientOcclusion, "ambient_occlusion",
    /// Screen-space occlusion from the depth buffer (crate::ao_render).
    On = "on"
);
two_valued!(
    /// The level-code water's look.
    Water, "water",
    /// Clear by depth, with reflections, a sun glint and foam at the shore (crate::water_render).
    Enhanced = "enhanced"
);
two_valued!(
    /// Ratchet's fur: Original is his model as the game draws it.
    Fur, "fur",
    /// Shell-layer fur on his furry parts (crate::fur).
    On = "on"
);
/// How smoke and other sprites meet the world, and whether bright effects light it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Particles {
    #[default]
    Original,
    /// Faded by their distance in front of the world behind them (crate::particle_render).
    Soft,
    /// Soft, and the bright effects (flashes, fireballs, additive sprites) light the opaque world around them
    /// (crate::particle_lights); the stars glow and the rain and snow are restyled ([`Particles::atmosphere`]); the
    /// ground fire rings (Gemlik's stompers, Blarg's fire waves) burn as flame curtains (crate::fx_draw::flame_curtain).
    Enhanced,
}

impl GfxOption for Particles {
    const ALL: &'static [Self] = &[Particles::Original, Particles::Soft, Particles::Enhanced];
    const KEY: &'static str = "particles";
    fn key(self) -> &'static str {
        match self {
            Particles::Original => "original",
            Particles::Soft => "soft",
            Particles::Enhanced => "enhanced",
        }
    }
}

impl Particles {
    /// The soft fade (Soft and Enhanced).
    pub fn on(self) -> bool { self != Particles::Original }
    /// The effect lights (Enhanced).
    pub fn lights(self) -> bool { self == Particles::Enhanced }
    /// The glowing, twinkling stars and the restyled rain and snow (Enhanced: crate::sky_stars, crate::flight_render,
    /// crate::particle_render's weather styles).
    pub fn atmosphere(self) -> bool { self == Particles::Enhanced }
}

/// The shadows and the port's lighting effects (the Port Options "Lighting" row; module docs).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Lighting {
    /// The game's shadow volumes under the classes with a shadow block (crate::shadow_render).
    #[default]
    Original,
    /// Cascaded shadows from the level's key light in their place (crate::sun_shadow).
    Simple,
    /// Simple's shadows and every advanced lighting effect (crate::sun_rays, crate::post_render, and the effects gated on [`Lighting::enhanced`]).
    Enhanced,
}

impl Lighting {
    /// The game's shadow volumes are drawn.
    pub fn game_shadows(self) -> bool { self == Lighting::Original }
    /// The sun casts shadows (in place of the game's).
    pub fn sun_shadows(self) -> bool { self != Lighting::Original }
    /// The advanced lighting effects run: the gate of every effect beyond the sun shadows.
    pub fn enhanced(self) -> bool { self == Lighting::Enhanced }

    /// The older `shadows` key and `RC_SHADOWS` value: `dynamic` is Enhanced; `on`, `off`, `1`, `0` and the rest are
    /// Original (the row has no Off: `RC_SHADOWS=0` stays a developer switch, crate::shadow_render).
    pub fn from_shadows(v: &str) -> Self { if v.trim() == "dynamic" { Lighting::Enhanced } else { Lighting::Original } }
}

impl GfxOption for Lighting {
    const ALL: &'static [Self] = &[Lighting::Original, Lighting::Simple, Lighting::Enhanced];
    const KEY: &'static str = "lighting";
    fn key(self) -> &'static str {
        match self {
            Lighting::Original => "original",
            Lighting::Simple => "simple",
            Lighting::Enhanced => "enhanced",
        }
    }
}

/// Extra plants on the level's grassy ground (crate::foliage).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Foliage {
    /// The level's own shrubs only (no foliage pass).
    #[default]
    Original,
    Moderate,
    /// Denser, taller and drawn farther than Moderate.
    Lush,
}

impl GfxOption for Foliage {
    const ALL: &'static [Self] = &[Foliage::Original, Foliage::Moderate, Foliage::Lush];
    const KEY: &'static str = "foliage";
    fn key(self) -> &'static str {
        match self {
            Foliage::Original => "original",
            Foliage::Moderate => "moderate",
            Foliage::Lush => "lush",
        }
    }
}

/// The 3D picture drawn below the frame's resolution and upscaled to it (crate::upscale); the HUD and menus stay at
/// the frame's. FSR 1's modes and per-axis factors (AMD's `ffx_fsr1.h` documentation: 1.3, 1.5, 1.7, 2.0).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Upscaler {
    /// The game's: the 3D drawn at the frame's resolution.
    #[default]
    Off,
    UltraQuality,
    Quality,
    Balanced,
    Performance,
}

impl Upscaler {
    /// The frame's size ÷ the 3D picture's, per axis (1: no upscaling).
    pub fn factor(self) -> f32 {
        match self {
            Upscaler::Off => 1.0,
            Upscaler::UltraQuality => 1.3,
            Upscaler::Quality => 1.5,
            Upscaler::Balanced => 1.7,
            Upscaler::Performance => 2.0,
        }
    }

    pub fn on(self) -> bool { self != Upscaler::Off }

    /// The 3D picture's size for a frame of `frame`: each axis ÷ [`Self::factor`], rounded, at least 1.
    pub fn render_size(self, frame: UVec2) -> UVec2 { (frame.as_vec2() / self.factor()).round().as_uvec2().max(UVec2::ONE) }
}

impl GfxOption for Upscaler {
    const ALL: &'static [Self] = &[Upscaler::Off, Upscaler::UltraQuality, Upscaler::Quality, Upscaler::Balanced, Upscaler::Performance];
    const KEY: &'static str = "upscaler";
    fn key(self) -> &'static str {
        match self {
            Upscaler::Off => "off",
            Upscaler::UltraQuality => "ultra_quality",
            Upscaler::Quality => "quality",
            Upscaler::Balanced => "balanced",
            Upscaler::Performance => "performance",
        }
    }
}

/// Frames drawn between two 60 Hz ticks (crate::frame_smooth). Not part of the presets (like the FPS limit, it depends
/// on the screen): its own resource, not a [`GraphicsSettings`] field.
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FrameSmoothing {
    /// The game's: one picture per tick, repeated while no tick runs.
    #[default]
    Off,
    /// The display's rate, each frame interpolated between the last two ticks.
    On,
}

impl GfxOption for FrameSmoothing {
    const ALL: &'static [Self] = &[FrameSmoothing::Off, FrameSmoothing::On];
    const KEY: &'static str = "frame_smoothing";
    fn key(self) -> &'static str {
        match self {
            FrameSmoothing::Off => "off",
            FrameSmoothing::On => "on",
        }
    }
}

/// Lighting Enhanced's light shafts (crate::sun_rays, crate::volumetric_light) by level index: on where a sun lights
/// open air, off where they only veil or streak the view (the play-test of 2026-10-26). docs/plan/graphics_options.md.
pub const SHAFT_LEVELS: [(bool, &str); 19] = [
    (true, "Veldin: desert daylight"),
    (true, "Novalis: daylight"),
    (false, "Aridia: dust haze, no sun to cast them"),
    (true, "Kerwan: daylight through the tunnels"),
    (true, "Eudora: daylight in the forest"),
    (false, "Rilgar: night"),
    (false, "Nebula G34: indoor station, no visible sun"),
    (false, "Umbris: dark swamp, no visible sun"),
    (false, "Batalia: rain"),
    (true, "Gaspar: daylight"),
    (false, "Orxon: night"),
    (true, "Pokitaru: daylight"),
    (false, "Hoven: night"),
    (true, "Gemlik: the sun in its sky"),
    (false, "Oltanis: overcast, no visible sun"),
    (false, "Quartu: night, indoor factory"),
    (true, "Kalebo III: daylight"),
    (true, "Drek's fleet: the sun in its sky"),
    (false, "Veldin finale: dark red sky, no visible sun"),
];

/// [`SHAFT_LEVELS`]' choice for a level (none past the table).
pub fn shafts_on_level(level: u32) -> bool { SHAFT_LEVELS.get(level as usize).is_some_and(|l| l.0) }

/// This frame's gate of the light shafts (main world, and the render world's copy): [`shafts_on_level`] for the
/// loaded level, off while the title world or a level transition's cards, movies and flight show (the space
/// scenes); `RC_LIGHT_SHAFTS=1|0` forces them on or off. The effects also need [`Lighting::enhanced`].
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq, bevy::render::extract_resource::ExtractResource)]
pub struct LightShafts(pub bool);

/// `RC_LIGHT_SHAFTS`: Some(on) to force, None for the table.
fn env_shafts() -> Option<bool> {
    static FORCE: std::sync::OnceLock<Option<bool>> = std::sync::OnceLock::new();
    *FORCE.get_or_init(|| match std::env::var("RC_LIGHT_SHAFTS").as_deref().map(str::trim) {
        Ok("1") => Some(true),
        Ok("0") => Some(false),
        _ => None,
    })
}

fn update_shafts(
    travel: Option<Res<crate::travel_render::Travel>>,
    title: Option<Res<crate::title_world::TitleWorldRt>>,
    mut out: ResMut<LightShafts>,
    mut logged: Local<Option<(u32, bool)>>,
) {
    let level = crate::level_load::level_index();
    let space = travel.is_some_and(|t| t.in_transition()) || title.is_some_and(|t| t.shown());
    let on = env_shafts().unwrap_or(!space && shafts_on_level(level));
    if *logged != Some((level, on)) {
        *logged = Some((level, on));
        let why = SHAFT_LEVELS.get(level as usize).map_or("", |l| l.1);
        println!("graphics: light shafts {} (level {level:02} {why}{}; RC_LIGHT_SHAFTS=0|1 forces)", if on { "on" } else { "off" }, if space { ", space scene" } else { "" });
    }
    if out.0 != on { out.0 = on; }
}

impl FrameSmoothing {
    pub fn on(self) -> bool { self == FrameSmoothing::On }
    /// The settings file's value, then `RC_GFX_FRAME_SMOOTHING` over it.
    pub fn startup() -> Self { env_value(file_value(FrameSmoothing::Off)) }
    pub fn save(self) { save(self); }
}

/// A preset: a value for every option in [`GraphicsSettings`] (not the screen's rows: resolution, aspects, FPS limit,
/// frame smoothing). Its values: [`GraphicsSettings::with_preset`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preset {
    /// Every option Original: what the PS2 showed.
    Original,
    /// A cleaner picture for a weak GPU: no multisampling, smooth textures, soft particles, the 3D upscaled (Balanced).
    Low,
    /// The modern options that cost little: 4x, Far distances, ambient occlusion, Enhanced water, Simple lighting,
    /// Moderate foliage.
    Medium,
    /// Every option at its best short of Maximum distances: Lighting Enhanced, Enhanced particles, Lush foliage.
    High,
    /// The options match no preset (shown, never chosen).
    Custom,
}

impl Preset {
    /// The row's values, in order.
    pub const ALL: [Preset; 5] = [Preset::Original, Preset::Low, Preset::Medium, Preset::High, Preset::Custom];
    /// The presets that can be chosen (all but Custom), the columns of [`GraphicsSettings::with_preset`].
    pub const CHOSEN: [Preset; 4] = [Preset::Original, Preset::Low, Preset::Medium, Preset::High];
    /// The settings-file key.
    pub const KEY: &'static str = "preset";
    pub fn index(self) -> u8 { Self::ALL.iter().position(|&p| p == self).unwrap_or(0) as u8 }
    pub fn from_index(i: u8) -> Self { Self::ALL.get(i as usize).copied().unwrap_or(Preset::Custom) }
    pub fn key(self) -> &'static str {
        match self {
            Preset::Original => "original",
            Preset::Low => "low",
            Preset::Medium => "medium",
            Preset::High => "high",
            Preset::Custom => "custom",
        }
    }
    /// A settings-file / `RC_GFX_PRESET` value; `enhanced` (the preset before Low / Medium / High) is Medium.
    pub fn parse(v: &str) -> Option<Self> {
        match v.trim() {
            "enhanced" => Some(Preset::Medium),
            v => Self::ALL.into_iter().find(|p| p.key() == v),
        }
    }
}

/// The value for preset `p` out of an option's line `[Original, Low, Medium, High]`.
fn pick<T: Copy>(p: Preset, line: [T; 4]) -> T { line[Preset::CHOSEN.iter().position(|&c| c == p).unwrap_or(0)] }

/// The graphics options (module docs; the render world has a copy for the port's own passes).
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq, Default, bevy::render::extract_resource::ExtractResource)]
pub struct GraphicsSettings {
    pub aa: AntiAliasing,
    pub hud: Hud,
    pub textures: Textures,
    pub detail: Detail,
    pub draw: DrawDistance,
    pub ao: AmbientOcclusion,
    pub water: Water,
    pub particles: Particles,
    pub lighting: Lighting,
    pub foliage: Foliage,
    pub upscaler: Upscaler,
    pub fur: Fur,
}

impl GraphicsSettings {
    /// The options of a preset (`Custom` keeps `self`): one line per option, its values for Original, Low, Medium and
    /// High. **Adding an option:** its line here (the literal has no `..`, so a missing line does not build), its key in
    /// [`Self::keys`] and [`Self::from_file`], `startup`'s switch and its row (crate::menu_render).
    pub fn with_preset(self, p: Preset) -> Self {
        if p == Preset::Custom { return self; }
        GraphicsSettings {
            // 4x: the best count every GPU offers (crate::render_settings clamps a count a device lacks).
            aa: pick(p, [AntiAliasing::Original, AntiAliasing::Off, AntiAliasing::X4, AntiAliasing::X4]),
            hud: pick(p, [Hud::Original; 4]),
            textures: pick(p, [Textures::Original, Textures::Smooth, Textures::Smooth, Textures::Sharp]),
            detail: pick(p, [Detail::Original, Detail::Original, Detail::Far, Detail::Farther]),
            draw: pick(p, [DrawDistance::Original, DrawDistance::Original, DrawDistance::Far, DrawDistance::Farther]),
            ao: pick(p, [AmbientOcclusion::Original, AmbientOcclusion::Original, AmbientOcclusion::On, AmbientOcclusion::On]),
            water: pick(p, [Water::Original, Water::Original, Water::Enhanced, Water::Enhanced]),
            particles: pick(p, [Particles::Original, Particles::Soft, Particles::Soft, Particles::Enhanced]),
            lighting: pick(p, [Lighting::Original, Lighting::Original, Lighting::Simple, Lighting::Enhanced]),
            foliage: pick(p, [Foliage::Original, Foliage::Original, Foliage::Moderate, Foliage::Lush]),
            upscaler: pick(p, [Upscaler::Off, Upscaler::Balanced, Upscaler::Off, Upscaler::Off]),
            fur: pick(p, [Fur::Original, Fur::Original, Fur::Original, Fur::On]),
        }
    }

    /// The preset these options are (Custom: none).
    pub fn preset(&self) -> Preset { Preset::CHOSEN.into_iter().find(|&p| self.with_preset(p) == *self).unwrap_or(Preset::Custom) }

    /// Every settings-file key and value: the preset's, then each option's.
    pub fn keys(&self) -> [(&'static str, &'static str); 13] {
        fn kv<T: GfxOption>(v: T) -> (&'static str, &'static str) { (T::KEY, v.key()) }
        [
            (Preset::KEY, self.preset().key()),
            kv(self.aa),
            kv(self.hud),
            kv(self.textures),
            kv(self.detail),
            kv(self.draw),
            kv(self.ao),
            kv(self.water),
            kv(self.particles),
            kv(self.lighting),
            kv(self.foliage),
            kv(self.upscaler),
            kv(self.fur),
        ]
    }

    /// The options from a settings file (`load`: a key's value): the stored preset's (Original without one, or for
    /// Custom), each key the file has over them; the older `msaa` and `shadows` keys where the new ones are missing.
    pub fn from_file(load: impl Fn(&str) -> Option<String>) -> Self {
        fn val<T: GfxOption>(load: &impl Fn(&str) -> Option<String>, d: T) -> T { load(T::KEY).as_deref().and_then(T::parse).unwrap_or(d) }
        let base = GraphicsSettings::default().with_preset(load(Preset::KEY).as_deref().and_then(Preset::parse).unwrap_or(Preset::Original));
        GraphicsSettings {
            aa: val(&load, legacy_msaa(&load).unwrap_or(base.aa)),
            hud: val(&load, base.hud),
            textures: val(&load, base.textures),
            detail: val(&load, base.detail),
            draw: val(&load, base.draw),
            ao: val(&load, base.ao),
            water: val(&load, base.water),
            particles: val(&load, base.particles),
            lighting: file_lighting(&load).unwrap_or(base.lighting),
            foliage: val(&load, base.foliage),
            upscaler: val(&load, base.upscaler),
            fur: val(&load, base.fur),
        }
    }

    /// The settings file's values ([`Self::from_file`]), or `RC_GFX_PRESET`'s, then the `RC_GFX_*` switches over them.
    pub fn startup() -> Self {
        let mut s = match std::env::var("RC_GFX_PRESET") {
            Ok(p) => GraphicsSettings::default().with_preset(Preset::parse(&p).unwrap_or(Preset::Original)),
            Err(_) => GraphicsSettings::from_file(crate::render_settings::load_key),
        };
        if let Ok(v) = std::env::var("RC_SHADOWS") { s.lighting = Lighting::from_shadows(&v); }
        s.lighting = env_value(s.lighting);
        s.aa = env_value(s.aa);
        // The older switch: `RC_MSAA=0|2|4|8` (0 = Off).
        if let Ok(v) = std::env::var("RC_MSAA") { s.aa = AntiAliasing::from_samples(v.trim().parse().unwrap_or(0)); }
        s.hud = env_value(s.hud);
        s.textures = env_value(s.textures);
        s.detail = env_value(s.detail);
        s.draw = env_value(s.draw);
        s.ao = env_value(s.ao);
        s.water = env_value(s.water);
        s.particles = env_value(s.particles);
        s.foliage = env_value(s.foliage);
        s.upscaler = env_value(s.upscaler);
        s.fur = env_value(s.fur);
        s
    }

    /// Writes the preset and every option into the port settings file ([`Self::keys`]).
    pub fn save(&self) {
        for (k, v) in self.keys() { crate::render_settings::save_key(k, v); }
    }
}

/// The file's `lighting`; without it, the Shadows row's older `shadows` key ([`Lighting::from_shadows`]); None without
/// either.
fn file_lighting(load: &impl Fn(&str) -> Option<String>) -> Option<Lighting> {
    match load(Lighting::KEY) {
        Some(v) => Some(Lighting::parse(&v).unwrap_or_default()),
        None => load("shadows").map(|v| Lighting::from_shadows(&v)),
    }
}

/// Before the Anti-aliasing row had Original, the file kept the sample count (`msaa = 0|2|4|8`): a count above 1 stays
/// that multisampling; off (the old default) becomes Original. None without the key.
fn legacy_msaa(load: &impl Fn(&str) -> Option<String>) -> Option<AntiAliasing> {
    Some(match load("msaa")?.trim().parse::<u32>() {
        Ok(n) if n > 1 => AntiAliasing::from_samples(n),
        _ => AntiAliasing::Original,
    })
}

fn file_value<T: GfxOption>(d: T) -> T { crate::render_settings::load_key(T::KEY).as_deref().and_then(T::parse).unwrap_or(d) }

fn env_value<T: GfxOption>(d: T) -> T {
    std::env::var(format!("RC_GFX_{}", T::KEY.to_ascii_uppercase())).ok().as_deref().and_then(T::parse).unwrap_or(d)
}

fn save<T: GfxOption>(v: T) { crate::render_settings::save_key(T::KEY, v.key()); }

pub struct GraphicsPlugin;

impl Plugin for GraphicsPlugin {
    fn build(&self, app: &mut App) {
        let s = GraphicsSettings::startup();
        if s != GraphicsSettings::default() { println!("graphics: {s:?} ({:?})", s.preset()); }
        app.insert_resource(s)
            .init_resource::<LightShafts>()
            .add_plugins(bevy::render::extract_resource::ExtractResourcePlugin::<GraphicsSettings>::default())
            .add_plugins(bevy::render::extract_resource::ExtractResourcePlugin::<LightShafts>::default())
            .add_systems(PreUpdate, (apply_detail, apply_samples, apply_draw_distance))
            .add_systems(PostUpdate, update_shafts);
    }
}

/// The Draw distance factor onto the main camera's far plane (crate::game_camera `GameProjection::draw_scale`) and the
/// moby draw distance (crate::moby_lod, a static); the
/// tfrag, tie and shrub systems and the shaders read the resource or the shared fog buffer.
fn apply_draw_distance(g: Res<GraphicsSettings>, mut cams: Query<&mut Projection, With<crate::fly_cam::FlyCam>>) {
    let s = g.draw.scale();
    crate::moby_lod::set_draw_scale(s);
    for mut p in &mut cams {
        if crate::game_camera::draw_scale(&p) != Some(s) { crate::game_camera::set_draw_scale(&mut p, s); }
    }
}

/// The Anti-aliasing option's sample count into crate::render_settings (which puts it on the world cameras), clamped to
/// what the GPU offers.
fn apply_samples(g: Res<GraphicsSettings>, render: Option<ResMut<crate::render_settings::RenderSettings>>, supported: Option<Res<crate::render_settings::SupportedMsaa>>) {
    let Some(mut render) = render else { return };
    if !g.is_changed() { return; }
    let want = crate::render_settings::msaa_from_samples(g.aa.samples());
    let want = supported.map_or(want, |s| s.clamp(want));
    if render.msaa != want { render.msaa = want; }
}

/// The Detail distance factor into the moby LOD pick (crate::moby_lod reads it from a static; tfrag, tie and shrub
/// systems and the shaders read the resource or the shared fog buffer).
fn apply_detail(g: Res<GraphicsSettings>) {
    if g.is_changed() { crate::moby_lod::set_lod_scale(g.detail.lod_scale()); }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_are_recognised_and_one_change_is_custom() {
        let o = GraphicsSettings::default();
        assert_eq!(o.preset(), Preset::Original);
        assert_eq!(o.with_preset(Preset::Original), o);
        for p in Preset::CHOSEN {
            let s = o.with_preset(p);
            assert_eq!(s.preset(), p, "{s:?}");
            // From any other options too: a preset sets every option.
            assert_eq!(GraphicsSettings { hud: Hud::SharpPixels, draw: DrawDistance::Maximum, ..o }.with_preset(p), s);
        }
        // The presets differ, so none is mistaken for another.
        for (i, a) in Preset::CHOSEN.into_iter().enumerate() {
            for b in &Preset::CHOSEN[i + 1..] { assert_ne!(o.with_preset(a), o.with_preset(*b), "{a:?} {b:?}"); }
        }
        let m = o.with_preset(Preset::Medium);
        assert_eq!((m.aa, m.lighting, m.foliage, m.upscaler), (AntiAliasing::X4, Lighting::Simple, Foliage::Moderate, Upscaler::Off));
        let h = o.with_preset(Preset::High);
        assert_eq!((h.lighting, h.particles, h.foliage), (Lighting::Enhanced, Particles::Enhanced, Foliage::Lush));
        assert_eq!(o.with_preset(Preset::Low).upscaler, Upscaler::Balanced);
        // Fur: On on High only.
        assert_eq!(Preset::CHOSEN.map(|p| o.with_preset(p).fur), [Fur::Original, Fur::Original, Fur::Original, Fur::On]);
        // Any one option changed from a preset makes it Custom; Custom keeps the options.
        for p in Preset::CHOSEN {
            let s = o.with_preset(p);
            let changed = [
                GraphicsSettings { hud: Hud::SharpPixels, ..s },
                GraphicsSettings { draw: DrawDistance::Maximum, ..s },
                GraphicsSettings { detail: Detail::Maximum, ..s },
                GraphicsSettings { aa: AntiAliasing::X8, ..s },
                GraphicsSettings { upscaler: if s.upscaler.on() { Upscaler::Off } else { Upscaler::Performance }, ..s },
                GraphicsSettings { fur: if s.fur == Fur::On { Fur::Original } else { Fur::On }, ..s },
                GraphicsSettings { lighting: if s.lighting.enhanced() { Lighting::Original } else { Lighting::Enhanced }, ..s },
            ];
            for c in changed {
                assert_eq!(c.preset(), Preset::Custom, "{p:?}: {c:?}");
                assert_eq!(c.with_preset(Preset::Custom), c);
            }
        }
    }

    /// `text` as a settings file: the keys' values (crate::render_settings' own reader).
    fn reader(text: &str) -> impl Fn(&str) -> Option<String> + '_ { move |k: &str| crate::render_settings::read_key(text, k).map(str::to_string) }

    /// `s` written into a settings file holding `text` (crate::render_settings' own writer, as `save` does).
    fn written(text: &str, s: &GraphicsSettings) -> String {
        s.keys().into_iter().fold(text.to_string(), |t, (k, v)| crate::render_settings::write_key(&t, k, v))
    }

    #[test]
    fn presets_round_trip_through_the_settings_file() {
        let o = GraphicsSettings::default();
        let custom = [
            GraphicsSettings { hud: Hud::SharpPixels, ..o.with_preset(Preset::High) },
            GraphicsSettings { upscaler: Upscaler::Performance, lighting: Lighting::Enhanced, ..o },
            GraphicsSettings { upscaler: Upscaler::UltraQuality, ..o.with_preset(Preset::Medium) },
        ];
        // Unknown keys and comments around them are kept, as by the real file.
        let other = "# ReRAC port settings\nresolution = 1080\nfps_limit = 60\n";
        for s in Preset::CHOSEN.map(|p| o.with_preset(p)).into_iter().chain(custom) {
            let text = written(other, &s);
            assert_eq!(GraphicsSettings::from_file(reader(&text)), s, "{text}");
            assert_eq!(crate::render_settings::read_key(&text, "preset"), Some(s.preset().key()));
            assert_eq!(crate::render_settings::read_key(&text, "resolution"), Some("1080"));
            // Saved again over itself: the same file.
            assert_eq!(written(&text, &s), text);
        }
        // A preset chosen over a Custom file: every key follows.
        let text = written(&written(other, &custom[0]), &o.with_preset(Preset::Low));
        assert_eq!(GraphicsSettings::from_file(reader(&text)), o.with_preset(Preset::Low));
        assert_eq!(GraphicsSettings::from_file(reader(&text)).preset(), Preset::Low);
    }

    #[test]
    fn a_stored_preset_fills_the_options_the_file_lacks() {
        // An option added after the file was written takes the stored preset's value.
        let high = GraphicsSettings::default().with_preset(Preset::High);
        let text: String = written("", &high).lines().filter(|l| !l.starts_with("foliage") && !l.starts_with("upscaler") && !l.starts_with("fur")).map(|l| format!("{l}\n")).collect();
        assert_eq!(GraphicsSettings::from_file(reader(&text)), high);
        // The options in the file win over the stored preset (a hand edit): Custom.
        let edited = text.replace("water = enhanced", "water = original");
        let s = GraphicsSettings::from_file(reader(&edited));
        assert_eq!((s.water, s.preset()), (Water::Original, Preset::Custom));
        // A file from before the presets were stored: its options as they are, the rest Original.
        let old = "anti_aliasing = 4x\ntextures = smooth\ndetail = far\nambient_occlusion = on\nwater = enhanced\nparticles = soft\nfoliage = moderate\nlighting = enhanced\n";
        let s = GraphicsSettings::from_file(reader(old));
        assert_eq!((s.aa, s.lighting, s.upscaler, s.draw, s.preset()), (AntiAliasing::X4, Lighting::Enhanced, Upscaler::Off, DrawDistance::Original, Preset::Custom));
        // The older names: `preset = enhanced` is Medium; no file at all is Original.
        assert_eq!(GraphicsSettings::from_file(reader("preset = enhanced\n")).preset(), Preset::Medium);
        assert_eq!(GraphicsSettings::from_file(reader("preset = custom\n")), GraphicsSettings::default());
        assert_eq!(GraphicsSettings::from_file(|_: &str| None), GraphicsSettings::default());
        assert_eq!(Preset::ALL.map(|p| Preset::parse(p.key())), Preset::ALL.map(Some));
        assert_eq!(Preset::ALL.map(|p| Preset::from_index(p.index())), Preset::ALL);
    }

    #[test]
    fn upscaler_modes() {
        assert_eq!(Upscaler::ALL.iter().map(|u| u.factor()).collect::<Vec<_>>(), [1.0, 1.3, 1.5, 1.7, 2.0]);
        assert_eq!(Upscaler::ALL.iter().map(|u| u.key()).collect::<Vec<_>>(), ["off", "ultra_quality", "quality", "balanced", "performance"]);
        for &u in Upscaler::ALL { assert_eq!(Upscaler::parse(u.key()), Some(u)); }
        let hd = UVec2::new(1920, 1080);
        assert_eq!(Upscaler::Off.render_size(hd), hd);
        assert_eq!(Upscaler::Quality.render_size(hd), UVec2::new(1280, 720));
        assert_eq!(Upscaler::Performance.render_size(hd), UVec2::new(960, 540));
        assert_eq!(Upscaler::UltraQuality.render_size(UVec2::new(3440, 1440)), UVec2::new(2646, 1108));
        assert_eq!(Upscaler::Performance.render_size(UVec2::ONE), UVec2::ONE);
    }

    #[test]
    fn draw_distance_factors() {
        assert_eq!(DrawDistance::ALL.iter().map(|d| d.scale()).collect::<Vec<_>>(), [1.0, 1.5, 2.0, 4.0]);
        assert_eq!(DrawDistance::KEY, "draw_distance");
        assert_eq!(DrawDistance::default(), DrawDistance::Original);
        // Far reaches past the far plane the game could draw a tfrag pixel to (728 units).
        assert!(500.0 * DrawDistance::Far.scale() > crate::game_camera::FAR / crate::game_camera::UNITS);
    }

    #[test]
    fn end_fade_band_and_dither() {
        // ×1: the last 16 units; ×4: the last 64.
        assert_eq!((end_fade(484.0, 500.0, 1.0), end_fade(492.0, 500.0, 1.0), end_fade(500.0, 500.0, 1.0), end_fade(501.0, 500.0, 1.0)), (128, 64, 0, 0));
        assert_eq!((end_fade(1936.0, 2000.0, 4.0), end_fade(1968.0, 2000.0, 4.0), end_fade(10.0, 2000.0, 4.0)), (128, 64, 128));
        // The dither draws every pixel at 128, none at 0, and vis / 128 of a 4×4 block between.
        let block = |v: u32| (0..16).filter(|&i| end_fade_shown(v, i % 4, i / 4)).count();
        // The Bayer order: one more pixel of the block for every 8 steps, the classic matrix's.
        let order: Vec<u32> = (0..16).map(|i| (0..=128).find(|&v| end_fade_shown(v, i % 4, i / 4)).unwrap()).collect();
        assert_eq!(order, [0, 8, 2, 10, 12, 4, 14, 6, 3, 11, 1, 9, 15, 7, 13, 5].map(|b| 8 * b + 5));
        assert_eq!((block(128), block(0), block(64), block(4), block(5)), (16, 0, 8, 0, 1));
        assert!((0..=128).all(|v| block(v) as u32 == (v + 3) / 8));
    }

    #[test]
    fn foliage_round_trips() {
        let keys: Vec<&str> = Foliage::ALL.iter().map(|v| v.key()).collect();
        assert_eq!(keys, ["original", "moderate", "lush"]);
        for &v in Foliage::ALL {
            assert_eq!(Foliage::parse(v.key()), Some(v));
            assert_eq!(Foliage::from_index(v.index()), v);
        }
        assert_eq!(Foliage::KEY, "foliage");
        assert_eq!(Foliage::parse(" lush "), Some(Foliage::Lush));
        assert_eq!(Foliage::parse("dense"), None);
        assert_eq!(Foliage::from_index(7), Foliage::Original);
    }

    #[test]
    fn keys_parse_back() {
        for &v in AntiAliasing::ALL { assert_eq!(AntiAliasing::parse(v.key()), Some(v)); }
        for &v in Hud::ALL { assert_eq!(Hud::parse(v.key()), Some(v)); }
        for &v in Textures::ALL { assert_eq!(Textures::parse(v.key()), Some(v)); }
        for &v in Detail::ALL { assert_eq!(Detail::parse(v.key()), Some(v)); }
        for &v in DrawDistance::ALL { assert_eq!(DrawDistance::parse(v.key()), Some(v)); }
        assert_eq!(DrawDistance::parse(" far "), Some(DrawDistance::Far));
        assert_eq!(DrawDistance::from_index(DrawDistance::Maximum.index()), DrawDistance::Maximum);
        for &v in AmbientOcclusion::ALL { assert_eq!(AmbientOcclusion::parse(v.key()), Some(v)); }
        for &v in Water::ALL { assert_eq!(Water::parse(v.key()), Some(v)); }
        for &v in Particles::ALL { assert_eq!(Particles::parse(v.key()), Some(v)); }
        for &v in Lighting::ALL { assert_eq!(Lighting::parse(v.key()), Some(v)); }
        assert_eq!(Lighting::ALL.iter().map(|v| v.key()).collect::<Vec<_>>(), ["original", "simple", "enhanced"]);
        for &v in FrameSmoothing::ALL { assert_eq!(FrameSmoothing::parse(v.key()), Some(v)); }
        assert_eq!(FrameSmoothing::ALL[0], FrameSmoothing::default());
        for &v in Foliage::ALL { assert_eq!(Foliage::parse(v.key()), Some(v)); }
        for &v in Fur::ALL { assert_eq!(Fur::parse(v.key()), Some(v)); }
        assert_eq!((Fur::KEY, Fur::On.key(), Fur::default()), ("fur", "on", Fur::Original));
        assert!(!AmbientOcclusion::Original.on() && Water::Enhanced.on());
        assert_eq!(Particles::parse("enhanced"), Some(Particles::Enhanced));
        assert!(Particles::Enhanced.on() && Particles::Enhanced.lights() && !Particles::Soft.lights());
        assert!(Particles::Enhanced.atmosphere() && !Particles::Soft.atmosphere() && !Particles::Original.atmosphere());
        assert_eq!(GraphicsSettings { particles: Particles::Original, ..GraphicsSettings::default().with_preset(Preset::Medium) }.preset(), Preset::Custom);
        assert_eq!(Detail::from_index(9), Detail::Original);
        assert_eq!(AntiAliasing::from_samples(0), AntiAliasing::Off);
        assert_eq!(AntiAliasing::from_samples(4).samples(), 4);
    }

    #[test]
    fn lighting_reads_the_older_shadows_key() {
        let file = |pairs: &'static [(&'static str, &'static str)]| {
            move |k: &str| pairs.iter().find(|p| p.0 == k).map(|p| p.1.to_string())
        };
        assert_eq!(file_lighting(&file(&[])), None);
        assert_eq!(file_lighting(&file(&[("shadows", "on")])), Some(Lighting::Original));
        assert_eq!(file_lighting(&file(&[("shadows", "off")])), Some(Lighting::Original));
        assert_eq!(file_lighting(&file(&[("shadows", "dynamic")])), Some(Lighting::Enhanced));
        assert_eq!(file_lighting(&file(&[("shadows", "dynamic"), ("lighting", "simple")])), Some(Lighting::Simple));
        assert_eq!(file_lighting(&file(&[("lighting", "bogus")])), Some(Lighting::Original));
        assert_eq!((Lighting::from_shadows("1"), Lighting::from_shadows("0"), Lighting::from_shadows(" dynamic")), (Lighting::Original, Lighting::Original, Lighting::Enhanced));
    }

    #[test]
    fn lighting_gates() {
        let g = |l: Lighting| (l.game_shadows(), l.sun_shadows(), l.enhanced());
        assert_eq!(g(Lighting::Original), (true, false, false));
        assert_eq!(g(Lighting::Simple), (false, true, false));
        assert_eq!(g(Lighting::Enhanced), (false, true, true));
    }

    #[test]
    fn shafts_follow_the_level_table() {
        // The play-test's list: Aridia, Rilgar, Nebula G34, Umbris, Batalia, Orxon, Hoven, Oltanis, Quartu; and the
        // Veldin finale (play session 2026-11-07_08-30-02).
        let off: Vec<u32> = (0..SHAFT_LEVELS.len() as u32).filter(|&l| !shafts_on_level(l)).collect();
        assert_eq!(off, [2, 5, 6, 7, 8, 10, 12, 14, 15, 18]);
        assert!(shafts_on_level(1) && shafts_on_level(3) && shafts_on_level(13) && shafts_on_level(16));
        assert!(!shafts_on_level(19) && !shafts_on_level(u32::MAX));
        assert!(SHAFT_LEVELS.iter().all(|l| !l.1.is_empty()));
    }
}
