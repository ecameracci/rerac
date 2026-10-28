//! **Port-only: the graphics options** (docs/plan/graphics_options.md). Every option controls one thing, and its first
//! value is always `Original`: what the PS2 showed on a TV. The others are modern alternatives. A preset sets them all
//! at once; it is not stored, it is read back from the options (Custom when they match no preset).
//!
//! | Option | Values | Consumer |
//! |---|---|---|
//! | [`AntiAliasing`] | Original (the game's own softening passes: the AA blit and the display copy), Off, 2x / 4x / 8x multisampling | crate::aa_blit, crate::render_settings (the samples) |
//! | [`Hud`] | Original (the 512×416 HUD scaled up smoothly, as the TV showed it), Sharp pixels | crate::hud_render composite |
//! | [`Textures`] | Original (the GS mip rule, one level per pixel), Smooth (blended levels), Sharp (the GPU's own choice, anisotropic) | the world shaders, through crate::game_camera's shared fog buffer |
//! | [`Detail`] | Original (the game's LOD distances), Far (×2), Farther (×4), Maximum (the full model whenever it is drawn) | crate::tfrag_lod, crate::tie_lod, crate::moby_render, crate::shrub_render (drawing only: the game logic keeps its own decisions) |
//! | [`DrawDistance`] | Original (the game's draw distances), Far (×1.5), Farther (×2), Maximum (×4), the new end faded | crate::tfrag_lod, crate::tie_lod, crate::shrub_render, crate::game_camera (the far plane); not mobys or particles (their drawn flags are game state) |
//! | [`AmbientOcclusion`] | Original (none: the shade is the artists' baked vertex colours), On (screen-space occlusion from the depth buffer) | crate::ao_render |
//! | [`Water`] | Original (the GS blend of the water textures), Enhanced (clear by depth, reflections, a sun glint, foam at the shore; under water: absorption, caustics, light shafts) | crate::water_render, crate::sea_water (the seas), crate::scene_copy, crate::underwater_render |
//! | [`Particles`] | Original (sprites cut where they meet the world), Soft (faded by their depth in front of it), Enhanced (Soft, bright effects light the world, glowing stars, restyled rain and snow) | crate::particle_render, crate::scene_copy, crate::particle_lights, crate::sky_stars, crate::flight_render, crate::weather_render |
//! | [`Foliage`] | Original (the level's own plants only), Moderate, Lush (grass clumps on the grassy ground and more of the level's small plants) | crate::foliage |
//! | [`Lighting`] | Original (the game's shadows), Simple (sun shadows in their place), Enhanced (Simple's shadows and every advanced lighting effect) | crate::shadow_render, crate::sun_shadow, crate::tie_lod ([`Lighting::game_shadows`], [`Lighting::sun_shadows`]); crate::sun_rays, crate::gi_render, crate::post_render ([`Lighting::enhanced`]; the rays also per level, [`LightShafts`]) |
//! | [`FrameSmoothing`] | Off (one picture per 60 Hz tick), On (the display's rate, interpolated between ticks); not in the presets | crate::frame_smooth, crate::frame_pace |
//!
//! **The lighting gate:** a later lighting effect (volumetric light, global illumination) runs only while
//! `GraphicsSettings::lighting.enhanced()` (main world, or the render world's extracted copy), as crate::sun_rays
//! does. Lighting is not part of a [`Preset`]: a preset keeps it, and [`GraphicsSettings::preset`] ignores it.
//!
//! Persisted in the port settings file (`crate::render_settings::save_key`); `RC_HUD`-style switches are not used here
//! (`RC_HUD=0` already hides the HUD): `RC_GFX_ANTI_ALIASING`, `RC_GFX_HUD`, `RC_GFX_TEXTURES`, `RC_GFX_DETAIL`,
//! `RC_GFX_DRAW_DISTANCE`, `RC_GFX_AMBIENT_OCCLUSION`, `RC_GFX_WATER`, `RC_GFX_PARTICLES`, `RC_GFX_FOLIAGE`, `RC_GFX_LIGHTING`,
//! `RC_GFX_FRAME_SMOOTHING` (the keys below), `RC_MSAA=0|2|4|8` (Anti-aliasing Off or that multisampling),
//! `RC_SHADOWS=1|dynamic` (Lighting Original / Enhanced, the older switch; `0`: crate::shadow_render) and
//! `RC_GFX_PRESET=original|enhanced` override the file at start.

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
/// How smoke and other sprites meet the world, and whether bright effects light it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Particles {
    #[default]
    Original,
    /// Faded by their distance in front of the world behind them (crate::particle_render).
    Soft,
    /// Soft, and the bright effects (flashes, fireballs, additive sprites) light the opaque world around them
    /// (crate::particle_lights); the stars glow and the rain and snow are restyled ([`Particles::atmosphere`]).
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
    (true, "Veldin: daylight"),
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

/// A preset: a value for every option but [`Lighting`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preset {
    Original,
    Enhanced,
    /// The options match no preset (shown, never chosen).
    Custom,
}

impl Preset {
    pub const ALL: [Preset; 3] = [Preset::Original, Preset::Enhanced, Preset::Custom];
    pub fn index(self) -> u8 { Self::ALL.iter().position(|&p| p == self).unwrap_or(0) as u8 }
    pub fn from_index(i: u8) -> Self { Self::ALL.get(i as usize).copied().unwrap_or(Preset::Custom) }
}

/// The graphics options (module docs; the render world has a copy for the port's own passes).
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq, Default, bevy::render::extract_resource::ExtractResource)]
pub struct GraphicsSettings {
    pub aa: AntiAliasing,
    pub hud: Hud,
    pub textures: Textures,
    pub detail: Detail,
    /// Not part of the presets yet: every preset keeps it.
    pub draw: DrawDistance,
    pub ao: AmbientOcclusion,
    pub water: Water,
    pub particles: Particles,
    /// Not in the presets (module docs).
    pub lighting: Lighting,
    pub foliage: Foliage,
}

impl GraphicsSettings {
    /// The options of a preset (`Custom` keeps `self`); Lighting and Draw distance stay `self`'s.
    pub fn with_preset(self, p: Preset) -> Self {
        match p {
            Preset::Original => GraphicsSettings { lighting: self.lighting, draw: self.draw, ..GraphicsSettings::default() },
            // 4x: the best count every GPU offers (crate::render_settings clamps a count a device lacks).
            Preset::Enhanced => GraphicsSettings {
                aa: AntiAliasing::X4,
                hud: Hud::Original,
                textures: Textures::Smooth,
                detail: Detail::Far,
                draw: self.draw,
                ao: AmbientOcclusion::On,
                water: Water::Enhanced,
                particles: Particles::Soft,
                lighting: self.lighting,
                foliage: Foliage::Moderate,
            },
            Preset::Custom => self,
        }
    }

    /// The preset these options are (whatever the Lighting and the Draw distance).
    pub fn preset(&self) -> Preset {
        [Preset::Original, Preset::Enhanced].into_iter().find(|&p| self.with_preset(p) == *self).unwrap_or(Preset::Custom)
    }

    /// The settings file's values, then the `RC_GFX_*` switches over them.
    pub fn startup() -> Self {
        let mut s = GraphicsSettings::default();
        if let Ok(p) = std::env::var("RC_GFX_PRESET") {
            match p.trim() {
                "enhanced" => s = s.with_preset(Preset::Enhanced),
                _ => s = s.with_preset(Preset::Original),
            }
        } else {
            s.aa = file_value(legacy_msaa());
            s.hud = file_value(s.hud);
            s.textures = file_value(s.textures);
            s.detail = file_value(s.detail);
            s.draw = file_value(s.draw);
            s.ao = file_value(s.ao);
            s.water = file_value(s.water);
            s.particles = file_value(s.particles);
            s.foliage = file_value(s.foliage);
        }
        s.lighting = file_lighting(crate::render_settings::load_key);
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
        s
    }

    /// Writes every option into the port settings file.
    pub fn save(&self) {
        save(self.aa);
        save(self.hud);
        save(self.textures);
        save(self.detail);
        save(self.draw);
        save(self.ao);
        save(self.water);
        save(self.particles);
        save(self.lighting);
        save(self.foliage);
    }
}

/// The file's `lighting`; without it, the Shadows row's older `shadows` key ([`Lighting::from_shadows`]); else
/// Original.
fn file_lighting(load: impl Fn(&str) -> Option<String>) -> Lighting {
    match load(Lighting::KEY) {
        Some(v) => Lighting::parse(&v).unwrap_or_default(),
        None => load("shadows").map_or(Lighting::Original, |v| Lighting::from_shadows(&v)),
    }
}

/// Before the Anti-aliasing row had Original, the file kept the sample count (`msaa = 0|2|4|8`): a count above 1 stays
/// that multisampling; off (the old default) becomes Original.
fn legacy_msaa() -> AntiAliasing {
    match crate::render_settings::load_key("msaa").and_then(|v| v.trim().parse::<u32>().ok()) {
        Some(n) if n > 1 => AntiAliasing::from_samples(n),
        _ => AntiAliasing::Original,
    }
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

/// The Draw distance factor onto the main camera's far plane (crate::game_camera `GameProjection::draw_scale`); the
/// tfrag, tie and shrub systems and the shaders read the resource or the shared fog buffer.
fn apply_draw_distance(g: Res<GraphicsSettings>, mut cams: Query<&mut Projection, With<crate::fly_cam::FlyCam>>) {
    let s = g.draw.scale();
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
    fn presets_round_trip() {
        let o = GraphicsSettings::default();
        assert_eq!(o.preset(), Preset::Original);
        let e = o.with_preset(Preset::Enhanced);
        assert_eq!(e.preset(), Preset::Enhanced);
        assert_eq!((o.foliage, e.foliage), (Foliage::Original, Foliage::Moderate));
        let c = GraphicsSettings { hud: Hud::SharpPixels, ..o };
        assert_eq!(c.preset(), Preset::Custom);
        assert_eq!(c.with_preset(Preset::Custom), c);
        assert_eq!(GraphicsSettings { foliage: Foliage::Lush, ..e }.preset(), Preset::Custom);
        // Draw distance is not part of the presets: either preset keeps it and is still recognised.
        let d = GraphicsSettings { draw: DrawDistance::Farther, ..o };
        assert_eq!(d.preset(), Preset::Original);
        assert_eq!(d.with_preset(Preset::Enhanced).draw, DrawDistance::Farther);
        assert_eq!(GraphicsSettings { draw: DrawDistance::Maximum, ..e }.preset(), Preset::Enhanced);
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
        assert!(!AmbientOcclusion::Original.on() && Water::Enhanced.on());
        assert_eq!(Particles::parse("enhanced"), Some(Particles::Enhanced));
        assert!(Particles::Enhanced.on() && Particles::Enhanced.lights() && !Particles::Soft.lights());
        assert!(Particles::Enhanced.atmosphere() && !Particles::Soft.atmosphere() && !Particles::Original.atmosphere());
        assert_eq!(GraphicsSettings { particles: Particles::Enhanced, ..GraphicsSettings::default().with_preset(Preset::Enhanced) }.preset(), Preset::Custom);
        assert_eq!(Detail::from_index(9), Detail::Original);
        assert_eq!(AntiAliasing::from_samples(0), AntiAliasing::Off);
        assert_eq!(AntiAliasing::from_samples(4).samples(), 4);
    }

    #[test]
    fn lighting_reads_the_older_shadows_key() {
        let file = |pairs: &'static [(&'static str, &'static str)]| {
            move |k: &str| pairs.iter().find(|p| p.0 == k).map(|p| p.1.to_string())
        };
        assert_eq!(file_lighting(file(&[])), Lighting::Original);
        assert_eq!(file_lighting(file(&[("shadows", "on")])), Lighting::Original);
        assert_eq!(file_lighting(file(&[("shadows", "off")])), Lighting::Original);
        assert_eq!(file_lighting(file(&[("shadows", "dynamic")])), Lighting::Enhanced);
        assert_eq!(file_lighting(file(&[("shadows", "dynamic"), ("lighting", "simple")])), Lighting::Simple);
        assert_eq!(file_lighting(file(&[("lighting", "bogus")])), Lighting::Original);
        assert_eq!((Lighting::from_shadows("1"), Lighting::from_shadows("0"), Lighting::from_shadows(" dynamic")), (Lighting::Original, Lighting::Original, Lighting::Enhanced));
    }

    #[test]
    fn lighting_gates_and_stays_out_of_the_presets() {
        let g = |l: Lighting| (l.game_shadows(), l.sun_shadows(), l.enhanced());
        assert_eq!(g(Lighting::Original), (true, false, false));
        assert_eq!(g(Lighting::Simple), (false, true, false));
        assert_eq!(g(Lighting::Enhanced), (false, true, true));
        let s = GraphicsSettings { lighting: Lighting::Enhanced, ..GraphicsSettings::default() };
        assert_eq!(s.preset(), Preset::Original);
        assert_eq!(s.with_preset(Preset::Enhanced).lighting, Lighting::Enhanced);
        assert_eq!(s.with_preset(Preset::Enhanced).with_preset(Preset::Original), s);
    }

    #[test]
    fn shafts_follow_the_level_table() {
        // The play-test's list: Aridia, Rilgar, Nebula G34, Umbris, Batalia, Orxon, Hoven, Oltanis, Quartu.
        let off: Vec<u32> = (0..SHAFT_LEVELS.len() as u32).filter(|&l| !shafts_on_level(l)).collect();
        assert_eq!(off, [2, 5, 6, 7, 8, 10, 12, 14, 15]);
        assert!(shafts_on_level(1) && shafts_on_level(3) && shafts_on_level(13) && shafts_on_level(16));
        assert!(!shafts_on_level(19) && !shafts_on_level(u32::MAX));
        assert!(SHAFT_LEVELS.iter().all(|l| !l.1.is_empty()));
    }
}
