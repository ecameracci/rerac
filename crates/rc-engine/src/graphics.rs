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
//! | [`AmbientOcclusion`] | Original (none: the shade is the artists' baked vertex colours), On (screen-space occlusion from the depth buffer) | crate::ao_render |
//! | [`Water`] | Original (the GS blend of the water textures), Enhanced (clear by depth, reflections, a sun glint, foam at the shore) | crate::water_render, crate::scene_copy |
//! | [`Particles`] | Original (sprites cut where they meet the world), Soft (faded by their depth in front of it) | crate::particle_render, crate::scene_copy |
//!
//! Persisted in the port settings file (`crate::render_settings::save_key`); `RC_HUD`-style switches are not used here
//! (`RC_HUD=0` already hides the HUD): `RC_GFX_ANTI_ALIASING`, `RC_GFX_HUD`, `RC_GFX_TEXTURES`, `RC_GFX_DETAIL`,
//! `RC_GFX_AMBIENT_OCCLUSION`, `RC_GFX_WATER`, `RC_GFX_PARTICLES` (the keys below),
//! `RC_MSAA=0|2|4|8` (Anti-aliasing Off or that multisampling) and `RC_GFX_PRESET=original|enhanced` override the file
//! at start.

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
    /// How smoke and other sprites meet the world.
    Particles, "particles",
    /// Faded by their distance in front of the world behind them (crate::particle_render).
    Soft = "soft"
);

/// A preset: a value for every option.
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
    pub ao: AmbientOcclusion,
    pub water: Water,
    pub particles: Particles,
}

impl GraphicsSettings {
    /// The options of a preset (`Custom` keeps `self`).
    pub fn with_preset(self, p: Preset) -> Self {
        match p {
            Preset::Original => GraphicsSettings::default(),
            // 4x: the best count every GPU offers (crate::render_settings clamps a count a device lacks).
            Preset::Enhanced => GraphicsSettings {
                aa: AntiAliasing::X4,
                hud: Hud::Original,
                textures: Textures::Smooth,
                detail: Detail::Far,
                ao: AmbientOcclusion::On,
                water: Water::Enhanced,
                particles: Particles::Soft,
            },
            Preset::Custom => self,
        }
    }

    /// The preset these options are.
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
            s.ao = file_value(s.ao);
            s.water = file_value(s.water);
            s.particles = file_value(s.particles);
        }
        s.aa = env_value(s.aa);
        // The older switch: `RC_MSAA=0|2|4|8` (0 = Off).
        if let Ok(v) = std::env::var("RC_MSAA") { s.aa = AntiAliasing::from_samples(v.trim().parse().unwrap_or(0)); }
        s.hud = env_value(s.hud);
        s.textures = env_value(s.textures);
        s.detail = env_value(s.detail);
        s.ao = env_value(s.ao);
        s.water = env_value(s.water);
        s.particles = env_value(s.particles);
        s
    }

    /// Writes every option into the port settings file.
    pub fn save(&self) {
        save(self.aa);
        save(self.hud);
        save(self.textures);
        save(self.detail);
        save(self.ao);
        save(self.water);
        save(self.particles);
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
            .add_plugins(bevy::render::extract_resource::ExtractResourcePlugin::<GraphicsSettings>::default())
            .add_systems(PreUpdate, (apply_detail, apply_samples));
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
        let c = GraphicsSettings { hud: Hud::SharpPixels, ..o };
        assert_eq!(c.preset(), Preset::Custom);
        assert_eq!(c.with_preset(Preset::Custom), c);
    }

    #[test]
    fn keys_parse_back() {
        for &v in AntiAliasing::ALL { assert_eq!(AntiAliasing::parse(v.key()), Some(v)); }
        for &v in Hud::ALL { assert_eq!(Hud::parse(v.key()), Some(v)); }
        for &v in Textures::ALL { assert_eq!(Textures::parse(v.key()), Some(v)); }
        for &v in Detail::ALL { assert_eq!(Detail::parse(v.key()), Some(v)); }
        for &v in AmbientOcclusion::ALL { assert_eq!(AmbientOcclusion::parse(v.key()), Some(v)); }
        for &v in Water::ALL { assert_eq!(Water::parse(v.key()), Some(v)); }
        for &v in Particles::ALL { assert_eq!(Particles::parse(v.key()), Some(v)); }
        assert!(!AmbientOcclusion::Original.on() && Water::Enhanced.on());
        assert_eq!(Detail::from_index(9), Detail::Original);
        assert_eq!(AntiAliasing::from_samples(0), AntiAliasing::Off);
        assert_eq!(AntiAliasing::from_samples(4).samples(), 4);
    }
}
