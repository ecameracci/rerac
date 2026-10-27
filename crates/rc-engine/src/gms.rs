//! **Port-only**: the Gadgetron Movement System (GMS), the game's lore name for its controllers: Port Options →
//! Controller opens a page listing every input device (`rc_game::menus::pause::port`, the GMS page) and lets the player
//! lock the game to one of them.
//!
//! **Devices** ([`Gms::devices`], rebuilt every frame): the keyboard and mouse (always), a DualSense read from its HID
//! reports (crate::dualsense) and each of Bevy's gamepads under its own name (an Xbox pad, a PDP pad, a DualShock 4;
//! Valve's devices by their ids, [`valve_name`]: Steam Deck, Steam Controller, Steam Input; a
//! DualSense's Bevy gamepad is left out while the pad is read directly, as crate::input_map::Pads does). The device that
//! last gave input (a key or button pressed, a stick pushed past half way, R2 pulled) is the **Active** one.
//!
//! **Lock** ([`Gms::lock`]): ✕ on a row reads only that device among the pads (Locked, the other pads Off); the
//! keyboard and mouse stay a fallback, so a lock to a pad that reads nothing can still be freed. ✕ on it again goes
//! back to all of them. While the locked device is unplugged the lock does not apply. The lock lasts for the run
//! (devices are not identified across runs). Pads with the same name are numbered ("Steam Input 1", "Steam Input 2":
//! Steam makes one virtual pad per controller it wraps).
//!
//! **The HelpDesk note** (`port::text::GMS_HELP_TEXT`): in a new game on Veldin (level 0 loaded with under
//! [`NEW_GAME_PLAY_TIME`] played) the help box shows what the Gadgetron Movement System is and where its options are,
//! as a text-only message (no voice: the game's lines are recordings) added to the level text under [`HELP_MESSAGE`]
//! and requested without a help record (`rc_game::help::Help::request_port`), so nothing of it enters the save; a
//! reload after a death plays on from the play time it showed at ([`Gms::shown_at`]) and does not show it again. Once
//! it has shown, the Help Log lists it too (`PageMenu::gms_in_log`, never in the saved log); the port settings file
//! keeps `gms_help = shown` for that.

use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use rc_game::menus::pause::port::{gms_status, GmsRow};

use crate::dualsense::DualSense;
use crate::gameplay::{Persistent, Play};

/// The note's id in the level text (positive, as the help box takes only those; past the disc's ids 0..=21500).
pub const HELP_MESSAGE: i32 = 0x7f00_0001;
/// Game ticks into Veldin before the note may show (past the arrival's own help).
const HELP_AFTER_TICKS: u64 = 600;
/// A Veldin load with less play time than this (the save's play time, game ticks: 5 minutes) is a new game.
pub const NEW_GAME_PLAY_TIME: i32 = 5 * 60 * 60;
/// `RC_GMS_TRACE=1`: the note's conditions every 120 ticks.
static TRACE: std::sync::LazyLock<bool> = std::sync::LazyLock::new(|| std::env::var("RC_GMS_TRACE").is_ok_and(|v| v.trim() == "1"));
/// The longest device name shown (the row's status needs the rest of the width).
const NAME_MAX: usize = 24;

/// One input device.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DeviceKey {
    Keyboard,
    /// The DualSense read from its HID reports.
    DualSense,
    /// One of Bevy's gamepads.
    Gamepad(Entity),
}

/// The devices, the one last used, the lock and the note's state.
#[derive(Resource, Debug, Default)]
pub struct Gms {
    pub devices: Vec<(DeviceKey, Vec<u8>)>,
    pub active: Option<DeviceKey>,
    pub lock: Option<DeviceKey>,
    /// The note has shown at least once: the Help Log lists it.
    pub in_log: bool,
    /// The note waits to show in this level (a new game's Veldin).
    pub help_due: bool,
    /// The game tick last seen: a lower one is a new level load.
    pub seen_counter: Option<u64>,
    /// The save's play time when the note last showed: a death's reload plays on from it (no note again), a new game
    /// starts below it.
    pub shown_at: Option<i32>,
    /// No note at all (deterministic runs).
    pub help_off: bool,
}

impl Gms {
    /// The lock while its device is connected.
    fn effective_lock(&self) -> Option<DeviceKey> { self.lock.filter(|l| self.devices.iter().any(|(k, _)| k == l)) }

    /// Whether input from `k` reaches the game. The keyboard always does: a lock to a pad that reads nothing (one of
    /// Steam's virtual pads for another controller) must not leave the game, and the page that frees it, without input.
    pub fn allows(&self, k: DeviceKey) -> bool { k == DeviceKey::Keyboard || self.effective_lock().is_none_or(|l| l == k) }

    /// The GMS page's rows: each device and its status.
    pub fn rows(&self) -> Vec<GmsRow> {
        let lock = self.effective_lock();
        self.devices
            .iter()
            .map(|(k, name)| GmsRow {
                name: name.clone(),
                status: match lock {
                    Some(l) if l == *k => gms_status::LOCKED,
                    Some(_) if *k != DeviceKey::Keyboard => gms_status::OFF,
                    Some(_) if self.active == Some(*k) => gms_status::ACTIVE,
                    Some(_) => gms_status::AVAILABLE,
                    None if self.active == Some(*k) => gms_status::ACTIVE,
                    None => gms_status::AVAILABLE,
                },
            })
            .collect()
    }

    /// ✕ on row `i`: lock that device, or free it when it is the one locked.
    pub fn pick(&mut self, i: usize) {
        let Some((k, _)) = self.devices.get(i) else { return };
        self.lock = if self.effective_lock() == Some(*k) { None } else { Some(*k) };
    }
}

/// Valve's USB vendor id.
const VALVE: u16 = 0x28de;

/// A Valve device's name by its product id, over the OS's (which is often generic): the Steam Deck's controls
/// (0x1205), the Steam Controller wired or through its dongle (0x1102, 0x1142), Steam Input's virtual pad (0x11ff).
fn valve_name(vendor: Option<u16>, product: Option<u16>) -> Option<&'static str> {
    if vendor != Some(VALVE) { return None; }
    match product? {
        0x1205 => Some("Steam Deck"),
        0x1102 | 0x1142 => Some("Steam Controller"),
        0x11ff => Some("Steam Input"),
        _ => None,
    }
}

/// A name as the row shows it: ASCII, cut at a word to [`NAME_MAX`] with "...".
fn row_name(name: &str) -> Vec<u8> { row_name_in(name, NAME_MAX) }

fn row_name_in(name: &str, max: usize) -> Vec<u8> {
    let ascii: String = name.chars().map(|c| if c.is_ascii() && !c.is_ascii_control() { c } else { ' ' }).collect();
    let ascii = ascii.trim();
    if ascii.len() <= max { return ascii.as_bytes().to_vec(); }
    let cut = &ascii[..max - 3];
    let cut = if ascii.as_bytes()[max - 3] == b' ' { cut } else { cut.rsplit_once(' ').map_or(cut, |(a, _)| a) };
    format!("{}...", cut.trim_end()).into_bytes()
}

/// The rows' names with a number after each name that more than one device has (in list order, from 1), the name cut
/// to leave room for it.
fn numbered(labels: &[String]) -> Vec<Vec<u8>> {
    labels
        .iter()
        .enumerate()
        .map(|(i, l)| {
            if labels.iter().filter(|o| *o == l).count() < 2 { return row_name(l); }
            let suffix = format!(" {}", labels[..=i].iter().filter(|o| *o == l).count());
            let mut name = row_name_in(l, NAME_MAX - suffix.len());
            name.extend_from_slice(suffix.as_bytes());
            name
        })
        .collect()
}

/// Each frame, before the pads are sampled: the devices, and the one that gave input.
pub fn track(
    mut gms: ResMut<Gms>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    gamepads: Query<(Entity, &Gamepad, Option<&Name>)>,
    dualsense: Option<Res<DualSense>>,
    mut ds_held: Local<u32>,
) {
    let ds = dualsense.as_ref().and_then(|d| d.get());
    let mut devices = vec![(DeviceKey::Keyboard, "Keyboard and mouse".to_string())];
    let mut used = (keys.get_just_pressed().next().is_some() || mouse.get_just_pressed().next().is_some()).then_some(DeviceKey::Keyboard);
    if let Some(p) = ds {
        devices.push((DeviceKey::DualSense, "DualSense".to_string()));
        let pressed = p.buttons & !*ds_held != 0;
        if pressed || p.left.length() > 0.5 || p.right.length() > 0.5 || p.r2 > 0.3 { used = Some(DeviceKey::DualSense); }
        *ds_held = p.buttons;
    }
    // In connection order, so the numbers of same-named pads stay put.
    let mut pads: Vec<_> = gamepads.iter().filter(|(_, g, _)| !(ds.is_some() && crate::dualsense::is_dualsense(g))).collect();
    pads.sort_by_key(|(e, ..)| *e);
    for (e, g, name) in pads {
        let label = valve_name(g.vendor_id(), g.product_id()).map(str::to_string).or(name.map(|n| n.as_str().to_string()));
        devices.push((DeviceKey::Gamepad(e), label.unwrap_or_else(|| format!("Controller {}", devices.len()))));
        if g.get_just_pressed().next().is_some() || g.left_stick().length() > 0.5 || g.right_stick().length() > 0.5 {
            used = Some(DeviceKey::Gamepad(e));
        }
    }
    if let Some(k) = used { gms.active = Some(k); }
    if gms.active.is_some_and(|a| !devices.iter().any(|(k, _)| *k == a)) { gms.active = None; }
    let labels: Vec<String> = devices.iter().map(|(_, l)| l.clone()).collect();
    gms.devices = devices.into_iter().map(|(k, _)| k).zip(numbered(&labels)).collect();
}

/// The keyboard's input as the game reads it (always, [`Gms::allows`]: the fallback under any lock).
pub fn keyboard<'a>(
    gms: Option<&Gms>,
    keys: &'a ButtonInput<KeyCode>,
    mouse: &'a ButtonInput<MouseButton>,
    motion: &'a AccumulatedMouseMotion,
) -> (&'a ButtonInput<KeyCode>, &'a ButtonInput<MouseButton>, &'a AccumulatedMouseMotion) {
    static NO_KEYS: std::sync::LazyLock<ButtonInput<KeyCode>> = std::sync::LazyLock::new(ButtonInput::default);
    static NO_MOUSE: std::sync::LazyLock<ButtonInput<MouseButton>> = std::sync::LazyLock::new(ButtonInput::default);
    static NO_MOTION: std::sync::LazyLock<AccumulatedMouseMotion> = std::sync::LazyLock::new(AccumulatedMouseMotion::default);
    if gms.is_none_or(|g| g.allows(DeviceKey::Keyboard)) { (keys, mouse, motion) } else { (&NO_KEYS, &NO_MOUSE, &NO_MOTION) }
}

/// The note in the level text (crate::gameplay's help setup).
pub fn add_help_message(messages: &mut Vec<rc_formats::strings::Message>) {
    if messages.iter().any(|m| m.id == HELP_MESSAGE) { return; }
    messages.push(rc_formats::strings::Message { id: HELP_MESSAGE, text: rc_game::menus::pause::port::text::GMS_HELP_TEXT.to_vec(), help_audio: -1 });
}

/// A new game's Veldin: request the note while the help box is free and enabled; once the box shows it, it is done
/// for this level and listed in the Help Log from then on.
fn help_note(mut gms: ResMut<Gms>, play: Option<ResMut<Play>>, gs: Option<Res<Persistent>>, mode: Option<Res<crate::menu_render::MenuMode>>) {
    if gms.help_off { return; }
    let Some(mut play) = play else { return };
    // A level load (the tick count starts again): due on a new game's Veldin.
    let counter = play.game.counter;
    let play_time = gs.as_ref().map_or(i32::MAX, |g| g.0.global.play_time);
    if gms.seen_counter.is_none_or(|c| counter < c) {
        gms.help_due = play.svc.level == 0 && play_time < NEW_GAME_PLAY_TIME && gms.shown_at.is_none_or(|t| play_time < t);
        if *TRACE { println!("gms: level {} loaded, play time {play_time}: note due {}", play.svc.level, gms.help_due); }
    }
    gms.seen_counter = Some(counter);
    if !gms.help_due { return; }
    let help = &mut play.svc.help;
    let shown = help.bx.state != 0 && help.bx.index.and_then(|i| help.text.messages.get(i)).is_some_and(|m| m.id == HELP_MESSAGE);
    if shown {
        gms.help_due = false;
        gms.shown_at = Some(play_time);
        if !gms.in_log {
            gms.in_log = true;
            crate::render_settings::save_key("gms_help", "shown");
        }
        println!("gms: the HelpDesk note shown");
        return;
    }
    let ticking = mode.as_ref().is_none_or(|m| m.state.mode.advances_tick());
    if *TRACE && play.game.counter % 120 == 0 {
        let h = &play.svc.help;
        println!(
            "gms: tick {}: level {}, ticking {ticking}, held {:#x}, help enabled {}, idle {}, box state {}, note in text {}",
            play.game.counter, play.svc.level, play.game.pad.held, h.bx.enabled, h.idle(), h.bx.state, h.text.messages.iter().any(|m| m.id == HELP_MESSAGE)
        );
    }
    if play.svc.level != 0 || !ticking || play.game.counter < HELP_AFTER_TICKS { return; }
    let help = &mut play.svc.help;
    if help.bx.enabled && help.idle() && help.text.messages.iter().any(|m| m.id == HELP_MESSAGE) { help.request_port(HELP_MESSAGE); }
}

pub struct GmsPlugin;

impl Plugin for GmsPlugin {
    fn build(&self, app: &mut App) {
        let in_log = crate::render_settings::load_key("gms_help").is_some_and(|v| v.trim() == "shown");
        app.insert_resource(Gms { in_log, help_off: crate::determinism::deterministic(), ..default() })
            .add_systems(PreUpdate, track.after(bevy::input::InputSystems).before(crate::input_map::sample_system))
            .add_systems(Update, help_note);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gms() -> Gms {
        let pad = DeviceKey::Gamepad(Entity::PLACEHOLDER);
        Gms { devices: vec![(DeviceKey::Keyboard, b"Keyboard and mouse".to_vec()), (DeviceKey::DualSense, b"DualSense".to_vec()), (pad, b"Xbox Controller".to_vec())], active: Some(DeviceKey::DualSense), ..default() }
    }

    #[test]
    fn rows_lock_and_free() {
        let mut g = gms();
        let st = |g: &Gms| g.rows().iter().map(|r| r.status).collect::<Vec<_>>();
        assert_eq!(st(&g), vec![gms_status::AVAILABLE, gms_status::ACTIVE, gms_status::AVAILABLE]);
        assert!(g.allows(DeviceKey::Keyboard) && g.allows(DeviceKey::DualSense));
        // ✕ on the Xbox pad: only it among the pads; the keyboard stays a fallback.
        g.pick(2);
        assert_eq!(st(&g), vec![gms_status::AVAILABLE, gms_status::OFF, gms_status::LOCKED]);
        assert!(g.allows(DeviceKey::Keyboard) && !g.allows(DeviceKey::DualSense));
        g.active = Some(DeviceKey::Keyboard);
        assert_eq!(st(&g)[0], gms_status::ACTIVE);
        // Unplugged: the lock does not apply.
        let saved = g.devices.pop();
        assert!(g.allows(DeviceKey::DualSense));
        g.devices.extend(saved);
        // ✕ on it again: all of them.
        g.pick(2);
        assert_eq!(g.lock, None);
        assert!(g.allows(DeviceKey::Keyboard));
    }

    #[test]
    fn names_fit_the_row() {
        assert_eq!(row_name("Xbox Controller"), b"Xbox Controller");
        assert_eq!(row_name("PDP Wired Controller for Xbox Series X"), b"PDP Wired Controller...");
        assert_eq!(row_name("Wireless Gamepad Controller XYZ"), b"Wireless Gamepad...");
        assert!(row_name("PDP Wired Controller for Xbox Series X").len() <= NAME_MAX);
    }

    #[test]
    fn same_names_are_numbered() {
        let l = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let n = numbered(&l(&["Keyboard and mouse", "Steam Input", "Xbox Controller", "Steam Input"]));
        assert_eq!(n, vec![b"Keyboard and mouse".to_vec(), b"Steam Input 1".to_vec(), b"Xbox Controller".to_vec(), b"Steam Input 2".to_vec()]);
        let long = numbered(&l(&["PDP Wired Controller for Xbox Series X"; 2]));
        assert_eq!(long[1], b"PDP Wired... 2");
        assert!(long.iter().all(|x| x.len() <= NAME_MAX));
    }

    #[test]
    fn valve_devices_by_id() {
        assert_eq!(valve_name(Some(VALVE), Some(0x1205)), Some("Steam Deck"));
        assert_eq!(valve_name(Some(VALVE), Some(0x1142)), Some("Steam Controller"));
        assert_eq!(valve_name(Some(VALVE), Some(0x11ff)), Some("Steam Input"));
        assert_eq!(valve_name(Some(VALVE), Some(0x1234)), None);
        assert_eq!(valve_name(Some(0x045e), Some(0x1205)), None);
        assert_eq!(valve_name(None, None), None);
    }

    #[test]
    fn the_note_is_added_once() {
        let mut m = Vec::new();
        add_help_message(&mut m);
        add_help_message(&mut m);
        assert_eq!(m.len(), 1);
        assert_eq!((m[0].id, m[0].help_audio), (HELP_MESSAGE, -1));
    }
}
