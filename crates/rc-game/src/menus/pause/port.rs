//! **Port-only** (not in the game): the "Port Options" page, for settings the PS2 never had (the graphics options and
//! their preset, anti-aliasing, detail and draw distance, ambient occlusion, water, particles, foliage, the lighting (the
//! game's shadows, sun shadows, or sun shadows with the advanced lighting effects),
//! the gameplay and cutscene aspects, the render resolution, fullscreen, the FPS limit, frame smoothing and, last, the gameplay rows
//! Strafing and Quick Select, the ring's hold of the world, the mouse look's direction and speed and, last, the Controller
//! page: the controller's vibration, its strength, R2 as the fire button, the trigger effects, the button icons and the
//! lightbar).
//! Spec: docs/plan/menus.md "Port-only settings"; the graphics rows: docs/plan/graphics_options.md (every row's first
//! value is the game's own look).
//!
//! It is built entirely from the game's own machinery so it looks and behaves like the Options sub-pages:
//! a page record whose frame-moby seqs, filler widgets and "✕ Toggle / △ Exit" hint list are those of a
//! game sub-page ([`MODEL_FEW`] Subtitles for ≤ 2 rows, [`MODEL_MANY`] Camera for more), a title label with
//! the sub-pages' flags (0xf), and a list drawn and driven exactly like the camera list (update 0x294cc0 /
//! draw 0x294e68: label left at 12 in yellow / light blue, value right-aligned at w − 12, rows h/(n+1),
//! Up/Down without wrap with sound 1, ✕ cycles the value with sound 0, generic Start/Select/R3 and △ keys).
//! Transitions, panels, fonts, colours and sounds come from [`PageMenu`] unchanged.
//!
//! Entry: one item "Port Options" (action 3 → [`PAGE`]) inserted into the Options list 0x1b4f78 right before
//! Quit Game, so Quit Game stays the last (and, with the list's wrap, the Up-from-top) entry as on the disc;
//! the description label W1 (flags 0x90, table 0x1b4fc8) gets a patched copy of its id table with the port
//! description at the same index ([`Label::table`]). [`PageMenu::install_port_page`] does all of it; without
//! the call every record is exactly as read from the overlay. The Options picture (`options_ss` by the list cursor)
//! skips the inserted entry ([`picture_index`]): Quit Game keeps Quark, and Port Options shows the engine's own
//! picture ([`PageMenu::port_picture`]).
//!
//! The main menu (the front end's Options 0x1b8d08) gets the same page as [`FRONT_PAGE`]: the entry is appended to
//! both of the front end's Options lists (NTSC Language / Sound, PAL + Video; `saves::front_options_enter` installs
//! them), and the page takes the frames of that Options' Sound sub-page 0x1b8e38 (the front-end sub-pages' title is
//! W2, not W0). Both pages share the title and the list widgets, so the values and the cursor are one.
//!
//! The main menu's Options also ends with a Quit Game the disc's has not ([`FRONT_QUIT_ACTION`]): ✕ on it closes the
//! app at once, no confirmation (`MenuOut::quit`; the engine's front-end frame).
//!
//! **Scrolling** (port-only, beyond the camera list's look): the list shows the rows its stretched panel holds
//! ([`LIST_ROWS`], [`ROW_STEP`]); the game's arrow (icon 0xe99e frame 6, the Epilogue's page arrows) marks rows hidden
//! above and below.
//!
//! Records live at addresses no EE pointer can hold (0x7f00_xxxx), and the text ids are negative (the disc's
//! ids are 0..=21500), so nothing here can collide with or be mistaken for game data.
//!
//! **Adding an option**: one [`Entry`] in [`ENTRIES`] (a [`Setting`] variant, a label id and its value ids in
//! [`text`]); the engine maps the [`Setting`] to its resource and persistence. The engine can restrict a row to
//! some of its values ([`PageMenu::set_port_choices`], e.g. the sample counts the GPU supports): ✕ then skips
//! the others.
//!
//! **The Gadgetron Movement System** (GMS, the controllers in the game's own lore): the "Controller" row ([`Setting::
//! Controller`], value "GMS") does not cycle; ✕ opens a page of its own ([`GMS_PAGE`], from the main menu
//! [`GMS_PAGE_FRONT`]) with the same frames, its title "Gadgetron Movement System" and a list of the connected
//! controllers ([`GmsList`]): each row a device's name and its status (Active, Available, Locked, Off). The engine fills
//! the rows before every menu tick ([`PageMenu::set_gms_rows`]) and reads the row ✕ was pressed on after it
//! ([`PageMenu::take_gms_pick`]); △ goes back to Port Options.

use super::super::{text as print, MenuAssets, MenuDraw, MenuInput, MenuSound, Overlay};
use super::options::{keys, right};
use super::{Data, Item, MenuOut, Page, PageMenu, Widget, LIGHT_BLUE, YELLOW};
use crate::pad::button;
use rc_formats::font::Font;

/// The page and its two own widgets (title label, settings list).
pub const PAGE: u32 = 0x7f00_0000;
/// The main menu's page (parent: the front end's Options).
pub const FRONT_PAGE: u32 = 0x7f00_0010;
/// The list action of the main menu's Quit Game: ✕ quits (`PageMenu`'s list update), a value no disc item has (0..=11).
pub const FRONT_QUIT_ACTION: i16 = 0x7f00;
pub const TITLE_W: u32 = 0x7f00_0100;
pub const LIST_W: u32 = 0x7f00_0200;
/// The list's callbacks (dispatched like the game's by address).
pub const UPDATE: u32 = 0x7f00_1000;
pub const DRAW: u32 = 0x7f00_1004;
/// The page kind (no game code keys on it).
pub const KIND: i32 = 0x7f;
/// The Gadgetron Movement System page (from the pause menu's Port Options), its main-menu copy, its title and list
/// widgets and the list's callbacks.
/// Past the Mods page's records (super::mods: page 0x7f00_0020, callbacks 0x7f00_1008 / 0x7f00_100c).
pub const GMS_PAGE: u32 = 0x7f00_0040;
pub const GMS_PAGE_FRONT: u32 = 0x7f00_0050;
pub const GMS_TITLE_W: u32 = 0x7f00_0300;
pub const GMS_LIST_W: u32 = 0x7f00_0400;
pub const GMS_UPDATE: u32 = 0x7f00_1010;
pub const GMS_DRAW: u32 = 0x7f00_1014;

/// Game records the page is attached to / modelled on (level01).
pub const OPTIONS: u32 = 0x1b4dd0;
pub const OPTIONS_LIST: u32 = 0x1b4f78;
pub const OPTIONS_LABEL: u32 = 0x1b4fe8;
pub const QUIT_PAGE: u32 = 0x1b6060;
/// Subtitles 0x1b5788 (seqs 73..77: the one/two-row sub-page frames) and Camera 0x1b5ee0 (118..122, three rows). Only an
/// Options sub-page's frames will do: a page's frame animations start from its parent's layout, and △ plays them
/// backwards to it (another page's, e.g. Cheats', end on their own parent's layout under the Options page).
pub const MODEL_FEW: u32 = 0x1b5788;
pub const MODEL_MANY: u32 = 0x1b5ee0;
/// The front end's Options item whose page the main menu's page is modelled on: Sound (20258, page 0x1b8e38; its
/// list panel is the front-end sub-pages' widest).
pub const FRONT_MODEL_LABEL: i16 = 20258;
/// The rows the list shows at once before its panel has a rect; the list scrolls ([`PortList::top`]).
pub const VISIBLE: usize = 4;
/// The list panel (slot 3 on both pages) stretched to these screen rows, from below the title (y 26..68) to above
/// the hint list (318..388): the model pages' panels are 165..248 (Camera) and 118..267 (the front end's Sound).
pub const LIST_SLOT: usize = 3;
pub const LIST_ROWS: [i32; 2] = [82, 302];
/// The least row spacing: the panel shows `h / ROW_STEP − 1` rows (the camera list's `h/(n+1)` spacing at ≥ this).
pub const ROW_STEP: i32 = 20;

/// The frame mobys' stretch for page `page` ([`super::frame::FrameMobys::stretch`]): the list panel on the port pages.
pub fn panel_stretch(page: u32) -> [Option<[i32; 2]>; super::frame::SLOTS] {
    let mut s = [None; super::frame::SLOTS];
    if [PAGE, FRONT_PAGE, GMS_PAGE, GMS_PAGE_FRONT, super::mods::PAGE].contains(&page) { s[LIST_SLOT] = Some(LIST_ROWS); }
    s
}

/// The rows of `n` a list panel `h` pixels high shows.
fn visible(h: i32, n: usize) -> usize { if h <= 0 { VISIBLE.min(n) } else { ((h / ROW_STEP - 1).max(1) as usize).min(n) } }

/// The port's own strings (not in the game's text table). Ids are negative; [`MenuAssets::msg`] resolves them.
pub mod text {
    pub const ENTRY: i32 = -0x100;
    pub const TITLE: i32 = -0x101;
    pub const DESCRIPTION: i32 = -0x102;
    pub const ANTI_ALIASING: i32 = -0x110;
    pub const X2: i32 = -0x111;
    pub const X4: i32 = -0x112;
    pub const X8: i32 = -0x113;
    pub const LIGHTING: i32 = -0x120;
    pub const RESOLUTION: i32 = -0x130;
    pub const RES_WINDOW: i32 = -0x131;
    pub const RES_416: i32 = -0x132;
    pub const RES_720: i32 = -0x133;
    pub const RES_1080: i32 = -0x134;
    pub const RES_1440: i32 = -0x135;
    pub const RES_2160: i32 = -0x136;
    pub const FULLSCREEN: i32 = -0x140;
    pub const ASPECT: i32 = -0x150;
    pub const ASPECT_4_3: i32 = -0x151;
    pub const ASPECT_16_9: i32 = -0x152;
    pub const ASPECT_16_10: i32 = -0x153;
    pub const CUTSCENE_ASPECT: i32 = -0x154;
    pub const ASPECT_FILL: i32 = -0x155;
    pub const PRESET: i32 = -0x160;
    pub const ORIGINAL: i32 = -0x161;
    pub const ENHANCED: i32 = -0x162;
    pub const CUSTOM: i32 = -0x163;
    pub const HUD: i32 = -0x170;
    pub const SHARP_PIXELS: i32 = -0x171;
    pub const TEXTURES: i32 = -0x180;
    pub const SMOOTH: i32 = -0x181;
    pub const SHARP: i32 = -0x182;
    pub const DETAIL: i32 = -0x190;
    pub const FAR: i32 = -0x191;
    pub const FARTHER: i32 = -0x192;
    pub const MAXIMUM: i32 = -0x193;
    pub const DRAW_DISTANCE: i32 = -0x194;
    pub const FPS_LIMIT: i32 = -0x1a0;
    pub const FPS_30: i32 = -0x1a1;
    pub const FPS_60: i32 = -0x1a2;
    pub const FPS_120: i32 = -0x1a3;
    pub const UNLIMITED: i32 = -0x1a5;
    pub const SIMPLE: i32 = -0x121;
    pub const FRAME_SMOOTHING: i32 = -0x1a6;
    pub const AMBIENT_OCCLUSION: i32 = -0x1b0;
    pub const WATER: i32 = -0x1b1;
    pub const PARTICLES: i32 = -0x1b2;
    pub const SOFT: i32 = -0x1b3;
    pub const FOLIAGE: i32 = -0x1b4;
    pub const MODERATE: i32 = -0x1b5;
    pub const LUSH: i32 = -0x1b6;
    pub const STRAFING: i32 = -0x1c0;
    pub const CLASSIC: i32 = -0x1c1;
    pub const MODERN: i32 = -0x1c2;
    pub const QUICK_SELECT: i32 = -0x1c3;
    pub const MOUSE_X: i32 = -0x1d0;
    pub const MOUSE_Y: i32 = -0x1d1;
    pub const MOUSE_SPEED: i32 = -0x1d2;
    pub const NORMAL: i32 = -0x1d3;
    pub const INVERTED: i32 = -0x1d4;
    /// "1".."10" (the mouse sensitivity's values).
    pub const LEVELS: [i32; 10] = [-0x1e1, -0x1e2, -0x1e3, -0x1e4, -0x1e5, -0x1e6, -0x1e7, -0x1e8, -0x1e9, -0x1ea];
    /// The main menu's Mods page (super::super::mods).
    pub const MODS: i32 = -0x200;
    pub const MODS_NONE: i32 = -0x201;
    pub const MODS_WHERE: i32 = -0x202;
    /// "On" / "Off", capitalised like the page's other values (the game's own 20314 / 20315, the Subtitles /
    /// HelpDesk toggle values, are lower case).
    pub const ON: i32 = -0x103;
    pub const OFF: i32 = -0x104;
    /// Quit Game's third line (`options::quit_draw`): "□ Main Menu" with the font's □ glyph 0x13, as 21067 "△ No" /
    /// 21071 "○ Yes" start with △ 0x12 / ○ 0x11 (no disc message says "Main Menu").
    pub const MAIN_MENU: i32 = -0x105;
    /// The Controller page (the Gadgetron Movement System).
    pub const VIBRATION: i32 = -0x300;
    pub const VIBRATION_STRENGTH: i32 = -0x301;
    pub const LOW: i32 = -0x302;
    pub const MEDIUM: i32 = -0x303;
    pub const HIGH: i32 = -0x304;
    pub const FIRE_BUTTON: i32 = -0x310;
    pub const CIRCLE: i32 = -0x311;
    pub const R2: i32 = -0x312;
    pub const TRIGGER_EFFECTS: i32 = -0x313;
    pub const CONTROLLER: i32 = -0x320;
    pub const GMS: i32 = -0x321;
    pub const GMS_TITLE: i32 = -0x322;
    pub const ACTIVE: i32 = -0x323;
    pub const AVAILABLE: i32 = -0x324;
    pub const LOCKED: i32 = -0x325;
    pub const GMS_OFF: i32 = -0x326;
    /// The Gadgetron Movement System's HelpDesk note (rc-engine's gms), also its Help Log entry's text.
    pub const GMS_HELP: i32 = -0x327;
    pub const BUTTON_ICONS: i32 = -0x330;
    pub const PS5: i32 = -0x331;
    pub const XBOX_ONE: i32 = -0x332;
    pub const AUTO: i32 = -0x333;
    pub const LIGHTBAR: i32 = -0x340;
    pub const LIGHTBAR_DEFAULT: i32 = -0x341;
    pub const ORANGE: i32 = -0x342;
    pub const GREEN: i32 = -0x343;
    pub const BLUE: i32 = -0x344;
    pub const HEALTH: i32 = -0x345;

    /// The Gadgetron Movement System note (roxas997's words, from the ReRAC Discord).
    pub const GMS_HELP_TEXT: &[u8] = b"The Gadgetron Movement System is what is used to move Ratchet and Clank on their intergalactic adventure. \
If you like, you may change your options on the Port Options screen, under Controller.";

    pub fn get(id: i32) -> Option<&'static [u8]> {
        Some(match id {
            ENTRY | TITLE => b"Port Options",
            DESCRIPTION => b"Settings of this port that the original game does not have",
            ON => b"On",
            OFF => b"Off",
            MAIN_MENU => b"\x13 Main Menu",
            ANTI_ALIASING => b"Anti-aliasing",
            X2 => b"2x",
            X4 => b"4x",
            X8 => b"8x",
            LIGHTING => b"Lighting",
            RESOLUTION => b"Resolution",
            RES_WINDOW => b"Window",
            RES_416 => b"416p",
            RES_720 => b"720p",
            RES_1080 => b"1080p",
            RES_1440 => b"1440p",
            RES_2160 => b"2160p",
            FULLSCREEN => b"Fullscreen",
            ASPECT => b"Gameplay aspect",
            CUTSCENE_ASPECT => b"Cutscene aspect",
            ASPECT_4_3 => b"4:3",
            ASPECT_16_9 => b"16:9",
            ASPECT_16_10 => b"16:10",
            ASPECT_FILL => b"Fill",
            PRESET => b"Preset",
            ORIGINAL => b"Original",
            ENHANCED => b"Enhanced",
            CUSTOM => b"Custom",
            HUD => b"HUD",
            SHARP_PIXELS => b"Sharp pixels",
            TEXTURES => b"Texture filtering",
            SMOOTH => b"Smooth",
            SHARP => b"Sharp",
            DETAIL => b"Detail distance",
            FAR => b"Far",
            FARTHER => b"Farther",
            MAXIMUM => b"Maximum",
            DRAW_DISTANCE => b"Draw distance",
            FPS_LIMIT => b"FPS limit",
            FPS_30 => b"30",
            FPS_60 => b"60",
            FPS_120 => b"120",
            UNLIMITED => b"Unlimited",
            SIMPLE => b"Simple",
            FRAME_SMOOTHING => b"Frame smoothing",
            AMBIENT_OCCLUSION => b"Ambient occlusion",
            WATER => b"Water",
            PARTICLES => b"Particles",
            SOFT => b"Soft",
            FOLIAGE => b"Foliage",
            MODERATE => b"Moderate",
            LUSH => b"Lush",
            STRAFING => b"Strafing",
            QUICK_SELECT => b"Quick Select",
            MOUSE_X => b"Mouse left / right",
            MOUSE_Y => b"Mouse up / down",
            MOUSE_SPEED => b"Mouse sensitivity",
            NORMAL => b"Normal",
            INVERTED => b"Inverted",
            _ if (LEVELS[9]..=LEVELS[0]).contains(&id) => {
                const N: [&[u8]; 10] = [b"1", b"2", b"3", b"4", b"5", b"6", b"7", b"8", b"9", b"10"];
                N[(LEVELS[0] - id) as usize]
            }
            CLASSIC => b"Classic",
            MODERN => b"Modern",
            MODS => b"Mods",
            MODS_NONE => b"No mods found",
            MODS_WHERE => b"Put mods in the mods folder beside the game",
            VIBRATION => b"Vibration",
            VIBRATION_STRENGTH => b"Vibration strength",
            LOW => b"Low",
            MEDIUM => b"Medium",
            HIGH => b"High",
            FIRE_BUTTON => b"Fire button",
            CIRCLE => b"Circle",
            R2 => b"R2",
            TRIGGER_EFFECTS => b"Trigger effects",
            CONTROLLER => b"Controller",
            GMS => b"GMS",
            GMS_TITLE => b"Gadgetron Movement System",
            ACTIVE => b"Active",
            AVAILABLE => b"Available",
            LOCKED => b"Locked",
            GMS_OFF => b"Off",
            GMS_HELP => GMS_HELP_TEXT,
            BUTTON_ICONS => b"Button icons",
            PS5 => b"PS5",
            XBOX_ONE => b"Xbox One",
            AUTO => b"Auto",
            LIGHTBAR => b"Lightbar",
            LIGHTBAR_DEFAULT => b"Default",
            ORANGE => b"Orange",
            GREEN => b"Green",
            BLUE => b"Blue",
            HEALTH => b"Health",
            _ => return None,
        })
    }
}

/// A port setting (the engine owns the value's meaning and its persistence).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Setting {
    /// The graphics preset: value 0 Original, 1 Enhanced, 2 Custom (shown when the rows match no preset; ✕ skips it,
    /// the engine restricts the row).
    Preset,
    /// Anti-aliasing: value 0 Original (the game's own softening passes), 1 off, 2 2x, 3 4x, 4 8x multisampling.
    AntiAliasing,
    /// The lighting (docs/plan/graphics_options.md, docs/plan/shadows.md): value 0 Original (the game's shadows, the
    /// default), 1 Simple (every object casts a shadow from the level's key light, in place of the game's), 2 Enhanced
    /// (Simple's shadows and every advanced lighting effect, e.g. the sun's rays).
    Lighting,
    /// Ambient occlusion: value 0 Original (none), 1 on.
    AmbientOcclusion,
    /// The level-code water: value 0 Original, 1 Enhanced.
    Water,
    /// Particle sprites against the world: value 0 Original (hard cut), 1 Soft, 2 Enhanced (Soft, bright effects
    /// light the world around them, the stars glow and twinkle, the rain and snow are restyled).
    Particles,
    /// Extra plants on the grassy ground: value 0 Original (the level's own only), 1 Moderate, 2 Lush.
    Foliage,
    /// The render resolution (the engine's game frame): value 0 fits the window, 1..5 a height of 416 (the PS2's lines), 720, 1080, 1440 or 2160 (the width
    /// follows the Aspect row).
    Resolution,
    /// The window: value 0 windowed, 1 fullscreen.
    Fullscreen,
    /// The gameplay view's aspect: value 0 the TV's 4:3, 1 16:10, 2 16:9, 3 the window's own (Fill: the window's shape,
    /// never narrower than 4:3 — a narrower window is letterboxed, not squashed; the 3D view widens; the 2D screen stays
    /// 4:3, centred; the HUD's side slots move out to the view's edges).
    Aspect,
    /// The cutscenes' aspect (the story scenes, the letterboxed cutaways, the ship's take-off / landing), the same
    /// values. The frame takes the wider of the two; the narrower view is pillarboxed in it.
    CutsceneAspect,
    /// How the game's 2D screen reaches the frame: value 0 Original (scaled smoothly, as on a TV), 1 Sharp pixels.
    Hud,
    /// How world textures pick their detail level: value 0 Original (the PS2's rule), 1 Smooth, 2 Sharp.
    Textures,
    /// How far away models keep their full detail: value 0 Original (the game's distances), 1 Far (×2), 2 Farther (×4),
    /// 3 Maximum (always).
    Detail,
    /// How far the world is drawn: value 0 Original (the game's draw distances), 1 Far (×1.5), 2 Farther (×2),
    /// 3 Maximum (×4).
    DrawDistance,
    /// The most frames drawn a second: value 0 30, 1 60 (the default, the game's tick rate), 2 120, 3 unlimited
    /// (no vsync). The game still ticks at 60 Hz.
    FrameLimit,
    /// Frames between two ticks: value 0 Off (the game's: each tick's picture repeated), 1 On (the display's rate, each
    /// frame interpolated between the last two ticks; the game state is the same).
    FrameSmoothing,
    /// L2 / R2 on foot: value 0 Classic (the game's look stance and crouch), 1 Modern (Ratchet & Clank 3's strafe,
    /// `crate::hero::strafe::Strafing`).
    Strafing,
    /// The quick-select ring (docs/plan/quick_select_pause.md): value 0 Classic (the game's: the world runs under the
    /// ring), 1 Modern (the world holds while △ keeps the ring open: `crate::tick::quick_select_hold`).
    QuickSelect,
    /// The mouse's camera turn (the engine's mouse look): value 0 Normal, 1 Inverted.
    MouseX,
    /// The mouse's camera tilt: value 0 Normal, 1 Inverted.
    MouseY,
    /// The mouse's speed: values 0..9 shown as 1..10.
    MouseSpeed,
    /// Controller rumble (the engine's haptics; the game has none): value 0 on, 1 off.
    Vibration,
    /// The rumble's strength: value 0 Low, 1 Medium, 2 High.
    VibrationStrength,
    /// The pad's fire button: value 0 ○ (the game's), 1 R2 (read as ○; R1 still crouches).
    FireButton,
    /// R2's adaptive-trigger feel per weapon (DualSense, with R2 as the fire button): value 0 on, 1 off.
    TriggerEffects,
    /// Not a value: ✕ opens the Gadgetron Movement System page (module docs).
    Controller,
    /// The button icons in text (the engine's): value 0 Original (the disc's), 1 PS5, 2 Xbox One, 3 Auto (the
    /// controller last used).
    ButtonIcons,
    /// The DualSense lightbar (the engine's): value 0 Default (the pad's own), 1 off, 2 Orange, 3 Green, 4 Blue, 5
    /// Health.
    Lightbar,
}

/// One row: the setting, its label and the ids of its values (✕ cycles through them).
#[derive(Clone, Copy, Debug)]
pub struct Entry {
    pub setting: Setting,
    pub label: i32,
    pub values: &'static [i32],
}

/// The rows, in order.
pub const ENTRIES: &[Entry] = &[
    Entry { setting: Setting::Preset, label: text::PRESET, values: &[text::ORIGINAL, text::ENHANCED, text::CUSTOM] },
    Entry { setting: Setting::Fullscreen, label: text::FULLSCREEN, values: &[text::OFF, text::ON] },
    Entry { setting: Setting::Resolution, label: text::RESOLUTION, values: &[text::RES_WINDOW, text::RES_416, text::RES_720, text::RES_1080, text::RES_1440, text::RES_2160] },
    Entry { setting: Setting::Aspect, label: text::ASPECT, values: &[text::ASPECT_4_3, text::ASPECT_16_10, text::ASPECT_16_9, text::ASPECT_FILL] },
    Entry { setting: Setting::CutsceneAspect, label: text::CUTSCENE_ASPECT, values: &[text::ASPECT_4_3, text::ASPECT_16_10, text::ASPECT_16_9, text::ASPECT_FILL] },
    Entry { setting: Setting::FrameLimit, label: text::FPS_LIMIT, values: &[text::FPS_30, text::FPS_60, text::FPS_120, text::UNLIMITED] },
    Entry { setting: Setting::FrameSmoothing, label: text::FRAME_SMOOTHING, values: &[text::OFF, text::ON] },
    Entry { setting: Setting::AntiAliasing, label: text::ANTI_ALIASING, values: &[text::ORIGINAL, text::OFF, text::X2, text::X4, text::X8] },
    Entry { setting: Setting::Detail, label: text::DETAIL, values: &[text::ORIGINAL, text::FAR, text::FARTHER, text::MAXIMUM] },
    Entry { setting: Setting::DrawDistance, label: text::DRAW_DISTANCE, values: &[text::ORIGINAL, text::FAR, text::FARTHER, text::MAXIMUM] },
    Entry { setting: Setting::Textures, label: text::TEXTURES, values: &[text::ORIGINAL, text::SMOOTH, text::SHARP] },
    Entry { setting: Setting::Hud, label: text::HUD, values: &[text::ORIGINAL, text::SHARP_PIXELS] },
    Entry { setting: Setting::AmbientOcclusion, label: text::AMBIENT_OCCLUSION, values: &[text::ORIGINAL, text::ON] },
    Entry { setting: Setting::Particles, label: text::PARTICLES, values: &[text::ORIGINAL, text::SOFT, text::ENHANCED] },
    Entry { setting: Setting::Lighting, label: text::LIGHTING, values: &[text::ORIGINAL, text::SIMPLE, text::ENHANCED] },
    Entry { setting: Setting::Water, label: text::WATER, values: &[text::ORIGINAL, text::ENHANCED] },
    Entry { setting: Setting::Foliage, label: text::FOLIAGE, values: &[text::ORIGINAL, text::MODERATE, text::LUSH] },
    Entry { setting: Setting::Strafing, label: text::STRAFING, values: &[text::CLASSIC, text::MODERN] },
    Entry { setting: Setting::QuickSelect, label: text::QUICK_SELECT, values: &[text::CLASSIC, text::MODERN] },
    Entry { setting: Setting::MouseX, label: text::MOUSE_X, values: &[text::NORMAL, text::INVERTED] },
    Entry { setting: Setting::MouseY, label: text::MOUSE_Y, values: &[text::NORMAL, text::INVERTED] },
    Entry { setting: Setting::MouseSpeed, label: text::MOUSE_SPEED, values: &text::LEVELS },
    Entry { setting: Setting::Controller, label: text::CONTROLLER, values: &[] },
    // The Controller page's rows ([`PORT_ROWS`] on): listed there above the connected controllers.
    Entry { setting: Setting::Vibration, label: text::VIBRATION, values: &[text::ON, text::OFF] },
    Entry { setting: Setting::VibrationStrength, label: text::VIBRATION_STRENGTH, values: &[text::LOW, text::MEDIUM, text::HIGH] },
    Entry { setting: Setting::FireButton, label: text::FIRE_BUTTON, values: &[text::CIRCLE, text::R2] },
    Entry { setting: Setting::TriggerEffects, label: text::TRIGGER_EFFECTS, values: &[text::ON, text::OFF] },
    Entry { setting: Setting::ButtonIcons, label: text::BUTTON_ICONS, values: &[text::ORIGINAL, text::PS5, text::XBOX_ONE, text::AUTO] },
    Entry { setting: Setting::Lightbar, label: text::LIGHTBAR, values: &[text::LIGHTBAR_DEFAULT, text::OFF, text::ORANGE, text::GREEN, text::BLUE, text::HEALTH] },
];

/// The Port Options page shows [`ENTRIES`]`[..PORT_ROWS]` (the last one the Controller row); the Controller page the
/// rest, then the connected controllers. Every row's value lives in the Port Options list ([`PortList::values`], by
/// [`ENTRIES`] index), whichever page shows it.
pub const PORT_ROWS: usize = 23;

/// The list's run-time state: cursor, each row's value index and each row's selectable values (bit k = value k;
/// a missing entry = all).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PortList {
    pub cursor: i32,
    /// The first row shown ([`VISIBLE`] at a time; the cursor stays among them).
    pub top: i32,
    pub values: Vec<u8>,
    pub choices: Vec<u32>,
}

/// A GMS row's status ([`GmsRow::status`]).
pub mod gms_status {
    /// The controller last used, input from all of them counting.
    pub const ACTIVE: u8 = 0;
    pub const AVAILABLE: u8 = 1;
    /// The only controller read (the player picked it).
    pub const LOCKED: u8 = 2;
    /// Ignored while another one is locked.
    pub const OFF: u8 = 3;
}

/// One connected controller on the GMS page: its name (the device's own, or the port's) and its status.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GmsRow {
    pub name: Vec<u8>,
    pub status: u8,
}

/// The GMS list's run-time state: cursor, first row shown, the rows the engine filled, and the row ✕ was pressed on
/// this tick (taken by the engine).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GmsList {
    pub cursor: i32,
    pub top: i32,
    pub rows: Vec<GmsRow>,
    pub pick: Option<usize>,
}

/// The value after `v` among `n` values that `choices` allows (wrapping); `v` when no other is allowed.
fn next_choice(v: u8, n: usize, choices: u32) -> u8 {
    (1..=n).map(|k| (v as usize + k) % n.max(1)).find(|&i| choices & (1 << i) != 0).unwrap_or(v as usize) as u8
}

fn list(m: &PageMenu) -> Option<&PortList> {
    match m.widgets.get(&LIST_W).map(|w| &w.data) {
        Some(Data::Port(p)) => Some(p),
        _ => None,
    }
}

fn list_mut(m: &mut PageMenu, w: u32) -> Option<&mut PortList> {
    match m.widgets.get_mut(&w).map(|w| &mut w.data) {
        Some(Data::Port(p)) => Some(p),
        _ => None,
    }
}

fn gms_mut(m: &mut PageMenu, w: u32) -> Option<&mut GmsList> {
    match m.widgets.get_mut(&w).map(|w| &mut w.data) {
        Some(Data::Gms(g)) => Some(g),
        _ => None,
    }
}

/// The row of a setting in [`ENTRIES`].
pub fn row_of(s: Setting) -> Option<usize> { ENTRIES.iter().position(|e| e.setting == s) }

/// The Options picture index of the port's own entries ([`PageMenu::port_picture`]); past every `options_ss` entry.
pub const PICTURE: i32 = 0x7f00;
/// [`ImageSrc::Pixels`](super::super::ImageSrc::Pixels) key of [`PICTURE`].
pub const PICTURE_KEY: u64 = 0x7f00_0000_0000_0001;

/// Adds `it` to a front-end Options list before its Quit Game ([`FRONT_QUIT_ACTION`], kept last), else at the end.
pub fn front_options_insert(list: &mut Vec<Item>, it: Item) {
    let at = list.iter().position(|i| i.action == FRONT_QUIT_ACTION).unwrap_or(list.len());
    list.insert(at, it);
}

/// An Options entry the port inserted (Port Options, Mods): not in the disc list, so it has no `options_ss` picture.
pub fn is_port_item(it: &Item) -> bool { it.label == text::ENTRY as i16 || it.label == text::MODS as i16 }

/// The picture an Options list shows for its cursor: [`PICTURE`] on a port entry, else the disc entry's own (the
/// cursor less the port entries above it, so Quit Game keeps its picture below Port Options).
pub fn picture_index(items: &[Item], cursor: i32) -> i32 {
    let at = cursor.max(0) as usize;
    if items.get(at).is_some_and(is_port_item) { return PICTURE; }
    cursor - items.iter().take(at).filter(|it| is_port_item(it)).count() as i32
}

/// A list widget of the port pages (the callbacks dispatch on `update` / `draw`).
fn list_widget(addr: u32, update: u32, draw: u32, data: Data) -> Widget {
    Widget { addr, update, draw, enter: 0, leave: 0, dflags: 0, moby: 0, rect: [0; 4], raw: [0; 8], data }
}

/// A page modelled on `model` (its frames and fillers) with its title label (widget `title_slot`) replaced by `title`
/// and its list (widget 3) by `list`. The GMS pages use their Port Options page's model too, so their frames play
/// from that page's layout and back.
fn sub_page(addr: u32, model: &Page, parent: u32, (title_slot, title): (usize, u32), list: u32) -> Page {
    let mut widgets = model.widgets;
    widgets[title_slot] = title;
    widgets[3] = list;
    Page { addr, seqs: model.seqs, parent, kind: KIND, focus: list, widgets, pending: 0 }
}

impl PageMenu {
    /// Adds the pages, their widgets and the Options entries of the pause menu and of the main menu (module docs).
    /// `ov` is read for the description label's id table. False when a page could not be added (the pause page:
    /// nothing changed; the main menu's: the pause page stays).
    pub fn install_port_page(&mut self, ov: &Overlay) -> bool { self.install_pause_port_page(ov) && self.install_front_port_page() }

    fn install_pause_port_page(&mut self, ov: &Overlay) -> bool {
        if self.pages.contains_key(&PAGE) { return true; }
        let ad = self.addrs.clone();
        let model = if ENTRIES.len() <= 2 { ad.port_model_few } else { ad.port_model_many };
        let (Some(model), true) = (self.pages.get(&model).cloned(), self.pages.contains_key(&ad.port_options)) else { return false };
        let Some(Data::List(opts)) = self.widgets.get(&ad.port_options_list).map(|w| &w.data) else { return false };
        let n = opts.items.len();
        let at = opts.items.iter().position(|it| it.action == 3 && it.arg == ad.port_quit).unwrap_or(n);
        // The title: the model's title label (flags 0xf, large, centred) with the port title.
        let Some(mut title) = self.widgets.get(&model.widgets[0]).cloned() else { return false };
        let Data::Label(l) = &mut title.data else { return false };
        l.id = text::TITLE as u32;
        title.addr = TITLE_W;
        // The GMS page's title: the same label with its own text, in the regular font (flag 8 off): "Gadgetron Movement
        // System" in the large one wraps to two lines the box cannot hold, which turns the label into a scroller; 0x2000
        // keeps it still.
        let mut gms_title = title.clone();
        if let Data::Label(l) = &mut gms_title.data {
            l.id = text::GMS_TITLE as u32;
            l.flags = (l.flags & !8) | 0x2000;
        }
        gms_title.addr = GMS_TITLE_W;
        let settings = list_widget(
            LIST_W,
            UPDATE,
            DRAW,
            Data::Port(PortList { cursor: 0, top: 0, values: vec![0; ENTRIES.len()], choices: vec![u32::MAX; ENTRIES.len()] }),
        );
        let gms = list_widget(GMS_LIST_W, GMS_UPDATE, GMS_DRAW, Data::Gms(GmsList::default()));
        let page = sub_page(PAGE, &model, ad.port_options, (0, TITLE_W), LIST_W);
        let gms_page = sub_page(GMS_PAGE, &model, PAGE, (0, GMS_TITLE_W), GMS_LIST_W);
        // The Options description label: the disc table (one id per item, variant 0) with ours inserted.
        if let Some(Data::Label(l)) = self.widgets.get_mut(&ad.port_options_label).map(|w| &mut w.data) {
            let mut t: Vec<u32> = (0..n as u32).map(|k| ov.u32(l.id.wrapping_add(k.wrapping_mul(l.stride) & !3)).unwrap_or(0)).collect();
            t.insert(at, text::DESCRIPTION as u32);
            l.table = Some(t);
        }
        if let Some(Data::List(opts)) = self.widgets.get_mut(&ad.port_options_list).map(|w| &mut w.data) {
            opts.items.insert(at, Item { label: text::ENTRY as i16, action: 3, arg: PAGE, sublabel: 0, hl: 0 });
        }
        self.widgets.insert(TITLE_W, title);
        self.widgets.insert(LIST_W, settings);
        self.widgets.insert(GMS_TITLE_W, gms_title);
        self.widgets.insert(GMS_LIST_W, gms);
        self.pages.insert(PAGE, page);
        self.pages.insert(GMS_PAGE, gms_page);
        true
    }

    /// The main menu's page: the front end's Sound sub-page with the shared title (W2) and list (W3), parent the front
    /// end's Options; the entry last in both of its lists, then Quit Game ([`PageMenu::install_front_quit_entry`]).
    /// Needs the pause page's widgets.
    fn install_front_port_page(&mut self) -> bool {
        if self.pages.contains_key(&FRONT_PAGE) { return true; }
        if !self.widgets.contains_key(&TITLE_W) || !self.widgets.contains_key(&LIST_W) { return false; }
        let Some(model) = self.saves.front_options[0].iter().find(|it| it.label == FRONT_MODEL_LABEL && it.action == 3).and_then(|it| self.pages.get(&it.arg)).cloned() else { return false };
        if model.parent == 0 { return false; }
        let page = sub_page(FRONT_PAGE, &model, model.parent, (2, TITLE_W), LIST_W);
        let gms_page = sub_page(GMS_PAGE_FRONT, &model, FRONT_PAGE, (2, GMS_TITLE_W), GMS_LIST_W);
        for list in &mut self.saves.front_options {
            list.push(Item { label: text::ENTRY as i16, action: 3, arg: FRONT_PAGE, sublabel: 0, hl: 0 });
        }
        self.pages.insert(FRONT_PAGE, page);
        self.pages.insert(GMS_PAGE_FRONT, gms_page);
        self.install_front_quit_entry();
        true
    }

    /// The main menu's Quit Game (port-only; the disc's main menu has none): the pause Options' Quit Game label with
    /// [`FRONT_QUIT_ACTION`] (no page: ✕ closes the app), last in both front-end Options lists (the later port entries
    /// go before it, [`front_options_insert`]). False when the pause Options' Quit Game is missing (nothing changed).
    pub fn install_front_quit_entry(&mut self) -> bool {
        if self.saves.front_options.iter().flatten().any(|it| it.action == FRONT_QUIT_ACTION) { return true; }
        let Some(Data::List(opts)) = self.widgets.get(&self.addrs.port_options_list).map(|w| &w.data) else { return false };
        let Some(label) = opts.items.iter().find(|it| it.action == 3 && it.arg == self.addrs.port_quit).map(|it| it.label) else { return false };
        for list in &mut self.saves.front_options {
            list.push(Item { label, action: FRONT_QUIT_ACTION, arg: 0, sublabel: 0, hl: 0 });
        }
        true
    }

    /// The value index of a port setting (None without the page).
    pub fn port_value(&self, s: Setting) -> Option<u8> { list(self)?.values.get(row_of(s)?).copied() }

    /// Sets a port setting's value index (from the engine's current value), clamped to the row's values.
    pub fn set_port_value(&mut self, s: Setting, v: u8) {
        let Some(r) = row_of(s) else { return };
        let max = ENTRIES[r].values.len().saturating_sub(1) as u8;
        if let Some(slot) = list_mut(self, LIST_W).and_then(|p| p.values.get_mut(r)) { *slot = v.min(max); }
    }

    /// Restricts a port setting to some of its values (bit k = value index k; ✕ skips the others). Value 0 stays
    /// selectable whatever the mask, so a row always has a choice.
    pub fn set_port_choices(&mut self, s: Setting, mask: u32) {
        let Some(r) = row_of(s) else { return };
        if let Some(p) = list_mut(self, LIST_W) {
            if p.choices.len() < ENTRIES.len() { p.choices.resize(ENTRIES.len(), u32::MAX); }
            p.choices[r] = mask | 1;
        }
    }

    /// The GMS page's rows (the connected controllers, from the engine), the cursor kept on the list.
    pub fn set_gms_rows(&mut self, rows: Vec<GmsRow>) {
        if let Some(g) = gms_mut(self, GMS_LIST_W) {
            let n = (GMS_SETTINGS + rows.len()) as i32;
            g.cursor = g.cursor.clamp(0, n - 1);
            g.top = g.top.clamp(0, (n - VISIBLE as i32).max(0));
            g.rows = rows;
        }
    }

    /// The GMS row ✕ was pressed on since the last call.
    pub fn take_gms_pick(&mut self) -> Option<usize> { gms_mut(self, GMS_LIST_W)?.pick.take() }
}

/// The list update, as the camera list 0x294cc0.
pub fn update(m: &mut PageMenu, w: u32, inp: &MenuInput, out: &mut MenuOut) -> i32 {
    if !m.is_focus(w) { return 0; }
    if let Some(r) = keys(m, inp, 1) { return r; }
    let shown = visible(m.widgets.get(&w).map_or(0, |wd| wd.rect[3]), PORT_ROWS) as i32;
    let Some(p) = list_mut(m, w) else { return 0 };
    let start = p.cursor;
    if inp.pressed_u & button::UP != 0 && p.cursor != 0 { p.cursor -= 1; }
    if inp.pressed_u & button::DOWN != 0 && (p.cursor as usize + 1) < PORT_ROWS { p.cursor += 1; }
    if p.cursor != start { out.sounds.push(MenuSound::Cursor); }
    p.top = p.top.clamp(p.cursor - shown + 1, p.cursor).max(0);
    let r = p.cursor as usize;
    let opens_gms = ENTRIES.get(r).is_some_and(|e| e.setting == Setting::Controller);
    if inp.pressed_u & button::CROSS != 0 && !opens_gms {
        let choices = p.choices.get(r).copied().unwrap_or(u32::MAX);
        if let (Some(e), Some(v)) = (ENTRIES.get(r), p.values.get_mut(r)) {
            *v = next_choice(*v, e.values.len(), choices);
            out.sounds.push(MenuSound::Confirm);
        }
    }
    if inp.pressed_u & button::CROSS != 0 && opens_gms {
        // The Controller row opens the GMS page (the main menu's copy from the main menu's Port Options).
        m.target = if m.current == FRONT_PAGE { GMS_PAGE_FRONT } else { GMS_PAGE };
        out.sounds.push(MenuSound::Confirm);
    }
    0
}

/// The list draw, as the camera list 0x294e68 (panel-local).
pub fn draw(m: &mut PageMenu, w: u32, a: &MenuAssets, out: &mut Vec<MenuDraw>) -> u32 {
    let Some(wd) = m.widgets.get(&w) else { return 1 };
    let [_, _, ww, wh] = wd.rect;
    let Data::Port(p) = &wd.data else { return 1 };
    // The camera list's spacing for the rows shown (h/(n+1)).
    let shown = visible(wh, PORT_ROWS);
    let step = wh / (shown as i32 + 1);
    let mut y = step - 8;
    let top = (p.top.max(0) as usize).min(PORT_ROWS - shown);
    for (i, e) in ENTRIES.iter().enumerate().take(PORT_ROWS).skip(top).take(shown) {
        let col = if i as i32 == p.cursor { YELLOW } else { LIGHT_BLUE };
        print(out, Font::Regular, 0xc, y, col, a.msg(e.label));
        value(out, a, ww, y, e, p.values.get(i).copied().unwrap_or(0));
        y += step;
    }
    arrows(out, a, ww, wh, step, scroll_marks(top, shown));
    2
}

/// A row's value, right-aligned; nothing for a row without values (the Controller row opens a page).
fn value(out: &mut Vec<MenuDraw>, a: &MenuAssets, ww: i32, y: i32, e: &Entry, v: u8) {
    if let Some(&id) = e.values.get(v as usize) { right(out, a, ww - 0xc, y, LIGHT_BLUE, a.msg(id)); }
}

/// Rows hidden above / below: the Epilogue's arrow (`media::epilogue_arrows`: icon 0xe99e frame 6, 0x20 × 0x10
/// texels), half its size, centred between the panel's edge and the first / last row shown.
fn arrows(out: &mut Vec<MenuDraw>, a: &MenuAssets, ww: i32, wh: i32, step: i32, (more_above, more_below): (bool, bool)) {
    let frame = a.frame(0xe99e, 6);
    let (cx, gap) = (ww as f32 / 2.0, (step - 8) as f32 / 2.0);
    // At angle 0 the arrow points right (`rotated`: a quarter clockwise); ±π/2 turn it to point up / down.
    let half = std::f32::consts::FRAC_PI_2;
    if more_above { out.push(super::map_page::rotated(frame, cx, gap.max(ARROW_W), ARROW_W, ARROW_H, half, 0x20, 0x10)); }
    if more_below { out.push(super::map_page::rotated(frame, cx, (wh as f32 - gap).min(wh as f32 - ARROW_W), ARROW_W, ARROW_H, -half, 0x20, 0x10)); }
}

/// The Controller page's settings rows ([`ENTRIES`]`[PORT_ROWS..]`), before its controllers.
const GMS_SETTINGS: usize = ENTRIES.len() - PORT_ROWS;

/// The Controller page's list update: Up / Down over its settings and then its controllers; ✕ on a setting cycles
/// its value (kept in the Port Options list, where the engine reads it), on a controller reports it to the engine
/// ([`GmsList::pick`]), which locks or frees it.
pub fn gms_update(m: &mut PageMenu, w: u32, inp: &MenuInput, out: &mut MenuOut) -> i32 {
    if !m.is_focus(w) { return 0; }
    if let Some(r) = keys(m, inp, 1) { return r; }
    let h = m.widgets.get(&w).map_or(0, |wd| wd.rect[3]);
    let Some(g) = gms_mut(m, w) else { return 0 };
    let total = GMS_SETTINGS + g.rows.len();
    let start = g.cursor;
    if inp.pressed_u & button::UP != 0 && g.cursor != 0 { g.cursor -= 1; }
    if inp.pressed_u & button::DOWN != 0 && (g.cursor as usize + 1) < total { g.cursor += 1; }
    if g.cursor != start { out.sounds.push(MenuSound::Cursor); }
    g.top = g.top.clamp(g.cursor - visible(h, total) as i32 + 1, g.cursor).max(0);
    if inp.pressed_u & button::CROSS == 0 { return 0; }
    let row = g.cursor as usize;
    if row >= GMS_SETTINGS {
        g.pick = Some(row - GMS_SETTINGS);
        out.sounds.push(MenuSound::Confirm);
        return 0;
    }
    let r = PORT_ROWS + row;
    if let Some(p) = list_mut(m, LIST_W) {
        let choices = p.choices.get(r).copied().unwrap_or(u32::MAX);
        if let Some(v) = p.values.get_mut(r) {
            *v = next_choice(*v, ENTRIES[r].values.len(), choices);
            out.sounds.push(MenuSound::Confirm);
        }
    }
    0
}

/// The Controller page's list draw: its settings (label left, value right), then each controller (name left, status
/// right), as the Port Options rows.
pub fn gms_draw(m: &mut PageMenu, w: u32, a: &MenuAssets, out: &mut Vec<MenuDraw>) -> u32 {
    let values = list(m).map(|p| p.values.clone()).unwrap_or_default();
    let Some(wd) = m.widgets.get(&w) else { return 1 };
    let [_, _, ww, wh] = wd.rect;
    let Data::Gms(g) = &wd.data else { return 1 };
    let n = GMS_SETTINGS + g.rows.len();
    let shown = visible(wh, n);
    let step = wh / (shown.max(1) as i32 + 1);
    let mut y = step - 8;
    let top = (g.top.max(0) as usize).min(n - shown);
    for i in (0..n).skip(top).take(shown) {
        let col = if i as i32 == g.cursor { YELLOW } else { LIGHT_BLUE };
        if let Some(e) = ENTRIES.get(PORT_ROWS + i).filter(|_| i < GMS_SETTINGS) {
            print(out, Font::Regular, 0xc, y, col, a.msg(e.label));
            value(out, a, ww, y, e, values.get(PORT_ROWS + i).copied().unwrap_or(0));
        } else if let Some(row) = g.rows.get(i - GMS_SETTINGS) {
            print(out, Font::Regular, 0xc, y, col, &row.name);
            let status = match row.status {
                gms_status::ACTIVE => text::ACTIVE,
                gms_status::LOCKED => text::LOCKED,
                gms_status::OFF => text::GMS_OFF,
                _ => text::AVAILABLE,
            };
            right(out, a, ww - 0xc, y, LIGHT_BLUE, a.msg(status));
        }
        y += step;
    }
    arrows(out, a, ww, wh, step, (top > 0, top + shown < n));
    2
}

/// The scroll arrows' half sizes (pixels; [`super::map_page::rotated`]'s `w` and `h`).
const ARROW_W: f32 = 6.0;
const ARROW_H: f32 = 10.0;

/// Whether Port Options rows are hidden above and below when `shown` rows from `top` are drawn.
pub fn scroll_marks(top: usize, shown: usize) -> (bool, bool) { (top > 0, top + shown < PORT_ROWS) }

#[cfg(test)]
mod tests {
    use super::{next_choice, scroll_marks, Setting, ENTRIES, PORT_ROWS, VISIBLE};

    #[test]
    fn scroll_marks_show_hidden_rows() {
        assert_eq!(scroll_marks(0, VISIBLE), (false, true));
        assert_eq!(scroll_marks(1, VISIBLE), (true, true));
        assert_eq!(scroll_marks(PORT_ROWS - VISIBLE, VISIBLE), (true, false));
    }

    #[test]
    fn controller_rows_are_on_the_controller_page() {
        // Port Options ends with the Controller row (no value: it opens a page); the controller settings follow.
        assert_eq!((ENTRIES[PORT_ROWS - 1].setting, ENTRIES[PORT_ROWS - 1].values.len()), (Setting::Controller, 0));
        let gms: Vec<Setting> = ENTRIES[PORT_ROWS..].iter().map(|e| e.setting).collect();
        assert_eq!(gms, vec![Setting::Vibration, Setting::VibrationStrength, Setting::FireButton, Setting::TriggerEffects, Setting::ButtonIcons, Setting::Lightbar]);
    }

    #[test]
    fn cycle_skips_unsupported_values() {
        // Off / 2x / 4x / 8x with 8x unsupported: 4x → Off.
        assert_eq!(next_choice(0, 4, 0b0111), 1);
        assert_eq!(next_choice(2, 4, 0b0111), 0);
        // WebGPU-only (off / 4x): off → 4x → off.
        assert_eq!(next_choice(0, 4, 0b0101), 2);
        assert_eq!(next_choice(2, 4, 0b0101), 0);
        // All allowed: plain wrap; nothing else allowed: stays.
        assert_eq!(next_choice(3, 4, u32::MAX), 0);
        assert_eq!(next_choice(0, 4, 0b0001), 0);
    }
}
