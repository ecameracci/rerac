//! **Port-only: Strafing = Modern** (the Port Options' "Strafing" row; docs/plan/modern_strafing.md). R&C1 has no
//! strafe on foot (L2 / R2 are the look stance and the crouch). With Modern, L2 or R2 held in gameplay is Ratchet &
//! Clank 3's strafe: his facing holds the camera's heading while he moves along the stick, ✕ sideways or back is the
//! strafe flip, the run's legs turn toward the motion (backwards: the run shown in reverse). The rules and numbers
//! are rac1-native-game's (Daino Selvatico, MIT: docs/advanced.md, src/game/enhance/advanced.cpp, from a study of
//! UYA's hero code); the code is this port's own.
//!
//! Hooks (every one is gated by [`Strafe::held`], which only [`remap`] sets, and only with Modern, so Classic runs
//! the game's code unchanged):
//! * the pad: `crate::tick::Game::tick_with_hero_sounds` ([`remap`], [`remaps`]);
//! * the facing: [`Hero::turn_to`](super::Hero::turn_to) ([`turn`]) and the stand-still turn after the state's
//!   physics ([`Hero::state_physics`](super::Hero::state_physics), [`after_physics`]);
//! * the motion: [`Hero::set_planar_vel`](super::Hero::set_planar_vel) ([`heading`]);
//! * no running long jump: `Hero::jump_or_running_jump` (hero/common.rs);
//! * the flip: `Hero::tr_walk` (hero/walk.rs, [`flip_wanted`]), the 0xb entry (hero/jump.rs, [`flip_entered`]) and
//!   `Hero::long_jump_air` (hero/physics.rs, [`flip_speed`]);
//! * the camera: `follow_camera::Camera::update_type0` holds the view as for the Thruster-Pack hover's strafe (0x81
//!   with L2 / R2: the leash off), released with the button;
//! * the legs: [`legs_update`] at the end of `hero_update_with_sounds`; the engine draws them (rc-engine
//!   gameplay.rs `hero_display_state` / `upload`: [`display_state`], [`twist_rows`]).

use super::physics::{turn_spring, DT, DT2, SCALE64};
use super::Hero;
use crate::pad::fast_diff_rots;
use crate::ps2v::Pf;
use rc_formats::moby_anim::{AnimState, Rows};

/// The Port Options row's values (Classic first: the game's own controls).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Strafing {
    #[default]
    Classic,
    Modern,
}

impl Strafing {
    /// In the row's value order.
    pub const ALL: [Strafing; 2] = [Strafing::Classic, Strafing::Modern];
    pub fn index(self) -> u8 { self as u8 }
    pub fn from_index(v: u8) -> Self { Self::ALL.get(v as usize).copied().unwrap_or_default() }
    /// The port settings file's value (`strafing = classic|modern`).
    pub fn key(self) -> &'static str { if self == Strafing::Modern { "modern" } else { "classic" } }
    pub fn parse(s: &str) -> Option<Self> { Self::ALL.into_iter().find(|v| v.key().eq_ignore_ascii_case(s.trim())) }
}

/// The facing snaps onto the camera's heading within this (rad).
pub const SNAP: f32 = 0.6;
/// The flip: ✕ with the last tick's stick past this, not forward.
pub const FLIP_TILT: f32 = 0.9;
/// The leg twist's ease (rad a tick), and the motion-from-facing bounds: none within 0.3, backpedal past 2.
pub const LEG_STEP: f32 = 0.15;
pub const LEG_MIN: f32 = 0.3;
pub const LEG_BACK: f32 = 2.0;
/// Ratchet's class: 111 joints; the twist turns the pelvis 1 and the legs and tail 92..=110.
pub const JOINTS: usize = 111;
pub const PELVIS: usize = 1;
pub const LEGS: std::ops::RangeInclusive<usize> = 92..=110;

/// The strafe's state on the hero (not in the game's block).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Strafe {
    /// L2 or R2 held this tick, taken from the game's pad by [`remap`] (never with Classic).
    pub held: bool,
    /// The heading the facing holds: the camera's yaw (`Env::cam_yaw`, the last rendered view).
    pub cam_yaw: Pf,
    /// A state's turn ran this tick ([`turn`]); else [`after_physics`] turns him (standing still).
    pub turned: bool,
    /// The motion's heading: the stick's (`target_yaw`) while it is pushed, kept when it is let go.
    pub heading: Pf,
    /// The 0xb under way is a strafe flip ([`flip_entered`]); cleared when 0xb ends.
    pub flip: bool,
    pub legs: Legs,
}

/// The run's legs while strafing, for the draw.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Legs {
    /// The lower body's turn (rad, eased by [`LEG_STEP`]).
    pub twist: f32,
    /// Moving backwards: the run's frames shown reversed ([`display_state`]).
    pub back: bool,
    /// The sequence and its frame count the reverse is for (the clip of the update that set `back`).
    pub seq: u8,
    pub frames: u8,
}

/// The states the strafe works in (rac1-native-game `aim_state`): on the ground 0, 2, 3, 0x73; the jump family's
/// 7, 9..=0x12, 0x1c, 0x3c, 0x49, 0x4c, 0x5e, 0x69 (0x49 / 0x4c / 0x5e are the bodies', which [`active`] leaves out;
/// 0xc is not ported: hero/registry.rs).
pub fn strafe_state(st: i32) -> bool {
    matches!(st, 0 | 2 | 3 | 0x73 | 7 | 0x1c | 0x3c | 0x49 | 0x4c | 0x5e | 0x69) || (9..=0x12).contains(&st)
}

/// Strafing now: L2 / R2 held, Ratchet on foot (mode 0, gravity mode 0) in a strafe state.
pub fn active(h: &Hero) -> bool { h.strafe.held && h.mode == 0 && h.gravity_mode == 0 && strafe_state(h.state) }

/// Whether the tick takes L2 / R2 from the game's pad: Ratchet on foot, except the moves that read them for their
/// own (the Thruster-Pack hover 0x81's strafe, `packs::hover_physics`, and its camera; the Hoverboard's tricks,
/// groups 0x15 / 0x16). The bodies (mode ≠ 0) keep the game's pad.
pub fn remaps(h: &Hero) -> bool { h.mode == 0 && h.state != 0x81 && !matches!(h.group, 0x15 | 0x16) }

/// The pad remap on the libpad2 bytes (byte 1 bits 0 / 1, active low): L2 / R2 released for the game; returns
/// the bytes and whether either was held.
pub fn remap(data: &[u8]) -> (Vec<u8>, bool) {
    let mut d = data.to_vec();
    let mut held = false;
    if let Some(b) = d.get_mut(1) {
        held = !*b & 3 != 0;
        *b |= 3;
    }
    (d, held)
}

/// `TurnTo` while strafing: toward the camera's heading on a stiffer spring (k 0.06, d 0.45, 14 rad/s), snapped
/// within [`SNAP`].
pub(super) fn turn(h: &mut Hero) {
    h.strafe.turned = true;
    let t = h.strafe.cam_yaw;
    let (mut yaw, mut v) = (h.rot[2], h.yaw_vel);
    if fast_diff_rots(t, yaw) < Pf::f(SNAP) {
        yaw = t;
        v = Pf::ZERO;
    }
    h.yaw_residual = turn_spring(t, SCALE64 * Pf::f(0.06), SCALE64 * Pf::f(0.45), DT * Pf::f(14.0), &mut yaw, &mut v, 0);
    h.rot[2] = yaw;
    h.yaw_vel = v;
}

/// Before the state's physics: no turn yet; a flip ends with its 0xb; outside the strafe the motion's heading is the
/// facing (so a stop that starts strafing slides on, not toward an old heading).
pub(super) fn before_physics(h: &mut Hero) {
    h.strafe.turned = false;
    if h.state != 0xb { h.strafe.flip = false; }
    if !active(h) { h.strafe.heading = h.rot[2]; }
}

/// After the state's physics: standing still (no state turned him), the turn to the camera here.
pub(super) fn after_physics(h: &mut Hero) {
    if active(h) && !h.strafe.turned { turn(h); }
}

/// `SetPlanarVel`'s heading while strafing: the stick's, the last one kept while it is centred.
pub(super) fn heading(h: &mut Hero) -> Pf {
    if Pf::ZERO < h.target_speed { h.strafe.heading = h.target_yaw; }
    h.strafe.heading
}

/// ✕ in the run's jump branch: a flip when strafing with the last tick's stick past [`FLIP_TILT`], not forward
/// (`StickSector` ≠ 2).
pub(super) fn flip_wanted(h: &mut Hero, c: &mut super::states::Ctx) -> bool {
    active(h) && Pf::f(FLIP_TILT) < h.stick_mag && h.stick_sector(c) != 2
}

/// The strafe flip's 0xb, after its entry: on the stick's heading only (facing 0x13f78c = direction 0x13f788, no side
/// speed 0x13f7a4), from 5 u/s (0x13f7a8) and off the ground the next tick (0x13f770 = 1).
pub(super) fn flip_entered(h: &mut Hero) {
    if h.state != 0xb { return; }
    h.jump.face_yaw = h.jump.dir_yaw;
    h.jump.side_speed = Pf::ZERO;
    h.jump.fwd_speed = DT * Pf::f(5.0);
    h.jump.takeoff = 1;
    h.strafe.flip = true;
}

/// R&C 3's stick tilt: the mean of the stick's square and round lengths, at most 1.
pub fn tilt(x: f32, y: f32) -> f32 { ((x.abs().max(y.abs()) + (x * x + y * y).sqrt()) * 0.5).min(1.0) }

/// The flip's forward speed (u/tick) and its step: `max(5.5·tilt, 2)` u/s, approached at 47 u/s².
pub fn flip_speed(stick: [f32; 2]) -> (f32, f32) {
    ((5.5 * tilt(stick[0], stick[1])).max(2.0) * DT.to_f32(), 47.0 * DT2.to_f32())
}

/// `a − b` wrapped into (−π, π].
fn angle_sub(a: f32, b: f32) -> f32 {
    let d = (a - b).rem_euclid(std::f32::consts::TAU);
    if d > std::f32::consts::PI { d - std::f32::consts::TAU } else { d }
}

/// The legs' turn target for the motion `d` from the facing: none within [`LEG_MIN`], `d` up to [`LEG_BACK`], beyond
/// it backwards (the reverse line, `d ∓ π`). Returns (target, backwards).
pub fn leg_target(d: f32) -> (f32, bool) {
    let a = d.abs();
    if LEG_BACK < a { (angle_sub(d, std::f32::consts::PI.copysign(d)), true) } else if LEG_MIN < a { (d, false) } else { (0.0, false) }
}

/// After the hero's update (mode 0): the run's legs (states 2 / 0x73 moving at more than 0.5 u/s while strafing),
/// eased; outside them the twist goes back at the same rate. `anim` is his animation after the update.
pub(super) fn legs_update(h: &mut Hero, anim: &super::anim::AnimView) {
    if !h.strafe.held && h.strafe.legs == Legs::default() { return; }
    let mut target = (0.0, false);
    if active(h) && matches!(h.state, 2 | 0x73) && (DT * Pf::f(0.5)) < h.eff_len_xy {
        target = leg_target(angle_sub(h.target_yaw.to_f32(), h.rot[2].to_f32()));
    }
    let l = &mut h.strafe.legs;
    l.twist = if l.twist < target.0 { (l.twist + LEG_STEP).min(target.0) } else { (l.twist - LEG_STEP).max(target.0) };
    l.back = target.1;
    l.seq = anim.seq_b;
    l.frames = anim.frame_count_b;
    if !h.strafe.held && l.twist == 0.0 { *l = Legs::default(); }
}

/// Out of gameplay (a scene, a menu, a body): the strafe released and the legs straight at once.
pub fn drop_pose(h: &mut Hero) {
    h.strafe.held = false;
    h.strafe.legs = Legs::default();
}

/// The animation state to draw: the run's frames reversed while backpedalling (only on the clip the legs were set
/// for and not while blending, so a scene or a state change in between shows the real frames).
pub fn display_state(l: &Legs, mut s: AnimState) -> AnimState {
    let n = l.frames;
    if l.back && n >= 2 && s.seq_a == s.seq_b && s.seq_b == l.seq {
        s.frame_a = n - 1 - s.frame_a % n;
        s.frame_b = n - 1 - s.frame_b % n;
    }
    s
}

/// The legs' twist on Ratchet's evaluated joint matrices: the pelvis, legs and tail turned by `a` about the moby's
/// vertical axis (every row's x, y). Nothing below 0.01 rad or on another skeleton.
pub fn twist_rows(rows: &mut [Rows], a: f32) {
    if a.abs() < 0.01 || rows.len() != JOINTS { return; }
    let (s, c) = a.sin_cos();
    for j in std::iter::once(PELVIS).chain(LEGS) {
        for r in rows[j].iter_mut() {
            let (x, y) = (r[0], r[1]);
            r[0] = x * c - y * s;
            r[1] = x * s + y * c;
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    /// A neutral libpad2 read (every button up, sticks centred).
    fn neutral() -> Vec<u8> { crate::pad::PadInput::neutral().bytes().to_vec() }

    fn pad(l2: bool, r2: bool) -> Vec<u8> {
        let mut d = neutral();
        if l2 { d[1] &= !1; }
        if r2 { d[1] &= !2; }
        d
    }

    #[test]
    fn remap_takes_l2_r2() {
        let n = neutral();
        assert_eq!(remap(&n), (n.clone(), false));
        for (l2, r2) in [(true, false), (false, true), (true, true)] {
            let (d, held) = remap(&pad(l2, r2));
            assert!(held);
            assert_eq!(d, n, "L2 / R2 not seen by the game");
        }
        // The other buttons pass: R1 (crouch, byte 1 bit 3) and L1 (look, bit 2).
        let mut b = neutral();
        b[1] &= !(4 | 8 | 2);
        let (d, held) = remap(&b);
        assert!(held);
        assert_eq!(d[1], b[1] | 2);
    }

    #[test]
    fn remap_scope() {
        let mut h = Hero::new();
        assert!(remaps(&h));
        h.state = 0x81;
        assert!(!remaps(&h), "the Thruster-Pack hover keeps its L2 / R2 strafe");
        h.state = 0;
        h.group = 0x15;
        assert!(!remaps(&h), "the Hoverboard keeps its tricks");
        h.group = 0;
        h.mode = 1;
        assert!(!remaps(&h), "a body keeps its pad");
    }

    #[test]
    fn strafe_states() {
        for st in [0, 2, 3, 0x73, 7, 9, 0xb, 0xe, 0x12, 0x1c, 0x3c, 0x49, 0x4c, 0x5e, 0x69] { assert!(strafe_state(st), "{st:#x}"); }
        for st in [1, 4, 6, 8, 0x13, 0x23, 0x81] { assert!(!strafe_state(st), "{st:#x}"); }
        let mut h = Hero::new();
        assert!(!active(&h), "not without the button");
        h.strafe.held = true;
        assert!(active(&h));
        h.state = 4;
        assert!(!active(&h), "the crouch");
    }

    #[test]
    fn facing_holds_the_camera_motion_follows_the_stick() {
        let mut h = Hero::new();
        h.strafe.held = true;
        h.strafe.cam_yaw = Pf::f(0.4);
        // Within the snap: on the camera's heading at once.
        h.rot[2] = Pf::f(0.0);
        turn(&mut h);
        assert_eq!((h.rot[2], h.yaw_vel, h.strafe.turned), (Pf::f(0.4), Pf::ZERO, true));
        // Beyond it: the spring turns him toward it, at most 14 rad/s.
        h.rot[2] = Pf::f(-1.5);
        turn(&mut h);
        let y = h.rot[2].to_f32();
        assert!(y > -1.5 && y <= -1.5 + 14.0 / 60.0 + 1e-6, "{y}");
        // The motion: the stick's heading, kept when it is let go.
        h.target_yaw = Pf::f(-1.2);
        h.target_speed = DT;
        assert_eq!(heading(&mut h), Pf::f(-1.2));
        h.target_yaw = h.rot[2];
        h.target_speed = Pf::ZERO;
        assert_eq!(heading(&mut h), Pf::f(-1.2));
        // Not strafing: the heading is the facing.
        h.strafe.held = false;
        before_physics(&mut h);
        assert_eq!(h.strafe.heading, h.rot[2]);
    }

    #[test]
    fn flip_speed_rule() {
        assert_eq!(tilt(0.0, 0.0), 0.0);
        assert_eq!(tilt(1.0, 0.0), 1.0);
        // The diagonal is full sooner than the round length: (0.6 + 0.8485) / 2.
        assert!((tilt(0.6, 0.6) - 0.724_26).abs() < 1e-4);
        assert_eq!(tilt(1.0, 1.0), 1.0);
        let (v, step) = flip_speed([1.0, 0.0]);
        assert!((v - 5.5 / 60.0).abs() < 1e-6 && (step - 47.0 / 3600.0).abs() < 1e-7);
        // At least 2 u/s.
        assert!((flip_speed([0.2, 0.0]).0 - 2.0 / 60.0).abs() < 1e-6);
    }

    #[test]
    fn legs_rule() {
        assert_eq!(leg_target(0.2), (0.0, false));
        assert_eq!(leg_target(1.5), (1.5, false));
        assert_eq!(leg_target(-1.5), (-1.5, false));
        let (t, back) = leg_target(3.0);
        assert!(back && (t - (3.0 - std::f32::consts::PI)).abs() < 1e-6);
        let (t, back) = leg_target(-2.5);
        assert!(back && (t - (-2.5 + std::f32::consts::PI)).abs() < 1e-6);
    }

    #[test]
    fn backpedal_display() {
        let s = AnimState { seq_a: 4, frame_a: 2, seq_b: 4, frame_b: 3, t: 0.5, speed: 1.0, rate: 1.0, flags: 0, trigger_count: 0, skip_advance: false };
        let l = Legs { twist: 0.0, back: true, seq: 4, frames: 10 };
        let r = display_state(&l, s);
        assert_eq!((r.frame_a, r.frame_b), (7, 6));
        // Another clip, a blend or no backpedal: the real frames.
        assert_eq!(display_state(&Legs { seq: 3, ..l }, s), s);
        let blend = AnimState { seq_a: 0xff, ..s };
        assert_eq!(display_state(&l, blend), blend);
        assert_eq!(display_state(&Legs { back: false, ..l }, s), s);
    }

    #[test]
    fn twist_turns_the_lower_body_only() {
        let id: Rows = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [1.0, 0.0, 0.0, 1.0]];
        let mut rows = vec![id; JOINTS];
        twist_rows(&mut rows, std::f32::consts::FRAC_PI_2);
        let near = |a: [f32; 4], b: [f32; 4]| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-6);
        for j in std::iter::once(PELVIS).chain(LEGS) {
            assert!(near(rows[j][0], [0.0, 1.0, 0.0, 0.0]) && near(rows[j][3], [0.0, 1.0, 0.0, 1.0]), "joint {j}");
        }
        assert_eq!((rows[0], rows[2], rows[91]), (id, id, id));
        let mut other = vec![id; 40];
        twist_rows(&mut other, 1.0);
        assert!(other.iter().all(|r| *r == id), "another skeleton");
    }

    #[test]
    fn setting_values() {
        assert_eq!(Strafing::default(), Strafing::Classic);
        for (k, v) in Strafing::ALL.into_iter().enumerate() {
            assert_eq!((v.index() as usize, Strafing::from_index(k as u8), Strafing::parse(v.key())), (k, v, Some(v)));
        }
        assert_eq!(Strafing::parse(" Modern "), Some(Strafing::Modern));
        assert_eq!(Strafing::from_index(7), Strafing::Classic);
    }
}
