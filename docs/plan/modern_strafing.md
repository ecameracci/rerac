# Strafing = Modern (port-only)

**Port-only.** R&C1 has no strafe on foot: L1 / L2 are the look stance (state 1) and R1 / R2 the crouch (state 4). The
Port Options row **Strafing** (last row of the page, on the pause menu's and the main menu's page alike; values
**Classic**, the default and the game's own controls, and **Modern**) gives L2 / R2 Ratchet & Clank 3's strafe:
Ratchet's facing holds the camera's heading while he moves along the stick, ✕ sideways or back is a strafe flip, and his
legs turn toward the motion. Code: `crates/rc-game/src/hero/strafe.rs`; the setting: `crates/rc-engine/src/strafing.rs`
(key `strafing = classic|modern` in the port settings file). Linked from player_controller.md §16.

**Credit.** The rules and numbers are those of rac1-native-game by Daino Selvatico (MIT), docs/advanced.md and
src/game/enhance/advanced.cpp, whose numbers come from a study of UYA's hero code. Only their rules and numbers are
taken; the code is this port's own (native Rust on the port's hero block). Their R&C 3 movement physics, their Modern
aiming, the quick-select pause, the fades and the updater are not ported.

## 1. Rules

* **Pad remap** (gameplay only: game mode 0 and not the scene frame). L2 (0x1) or R2 (0x2) held is taken from the
  game's pad: the game never sees them, so L1 keeps the look stance and R1 the crouch. The remap leaves the pad alone
  where the game reads L2 / R2 for its own moves: the Thruster-Pack hover 0x81 (its strafe and camera), the Hoverboard
  (groups 0x15 / 0x16, the tricks) and the bodies (mode ≠ 0).
* **Strafing** = the button held, Ratchet on foot (mode 0, gravity mode 0) in a strafe state: on the ground 0, 2, 3,
  0x73; the jump family's 7, 9..=0x12, 0x1c, 0x3c, 0x69 (their list also has 0x49 / 0x4c / 0x5e, the Clank and Giant
  Clank bodies' states, which the mode test leaves out; 0xc is in the range but not ported: no caller is known).
* **Facing**: every `TurnTo` turns toward the camera's yaw (0x167258 as the last camera update left it) instead of the
  state's target, on a stiffer spring: k = 0.06, d = 0.45, at most 14 rad/s; within 0.6 rad the yaw snaps onto it
  (yaw velocity 0). A tick on which no state turned him (standing still) turns him after the state's physics.
* **Motion**: `SetPlanarVel` runs along the stick's heading (`target_yaw`) instead of the facing; with the stick
  centred the last heading is kept, so a stop slides on along the motion. The speed is R&C1's own two-step speed
  (0.9 / 5.7 u/s; their AIM_SPEED is 1).
* **No running long jump**: ✕ while strafing picks the jump 7, never the running jump 9.
* **Strafe flip**: ✕ in the run's jump branch while strafing with the last tick's stick past 0.9 and not forward
  (`StickSector` ≠ 2: 0 left, 1 right, 3 back) is the flip 0xb, before the game's own flip test. After its entry
  (R&C1's side / back flip animations 0x1c + sector) the flip runs on the stick's heading only (facing 0x13f78c =
  direction 0x13f788, no side speed 0x13f7a4), starts at 5 u/s (0x13f7a8) and leaves the ground the next tick
  (0x13f770 = 1); its forward speed then approaches max(5.5 · tilt, 2) u/s at 47 u/s² (R&C1: 4.2 u/s at 8 u/s²), tilt
  = min(1, (max(|x|, |y|) + √(x² + y²)) / 2) of the stick. A later 0xb (R1's crouch flip) is the game's.
* **Legs** (display only): while strafing in 2 / 0x73 above 0.5 u/s, with d = the stick's heading − the facing: past
  2 rad the run is a backpedal (its frames shown reversed, the legs turned onto the reverse line d ∓ π); past 0.3 rad
  the legs turn by d; else 0. The turn eases at 0.15 rad a tick. It turns Ratchet's evaluated joint matrices 1 (pelvis)
  and 92..=110 (legs, tail) about the moby's vertical axis (111 joints only). The legs straighten at once out of
  gameplay or off foot (a scene, the scene frame, a body, the hover, the board).
* **Camera**: while strafing the follow camera does what the game does for the Thruster-Pack hover's strafe (0x81
  with L2 / R2): the leash 0x314118 off, the look from the smoothed target, the horizontal spring 0.04 / 0.2. Without
  the leash his motion does not swing the view round him (a camera-facing strafe would otherwise circle); the right
  stick still turns the camera, and Ratchet with it. The hold is re-armed every tick and ends with the strafe: no lock
  after the release.

## 2. Hooks

| Rule | Where |
|---|---|
| Option, persistence | `rc-game/src/menus/pause/port.rs` (`Setting::Strafing`, row last); `rc-engine/src/menu_render.rs` (`PortSettings::rows_in` / `rows_out`); `rc-engine/src/strafing.rs` (`StrafingSetting`, `apply` into `GameOptions::strafing` before each tick); `rc-engine/src/gameplay.rs` `tick` keeps it when it rebuilds the options |
| Pad remap | `rc-game/src/tick.rs` `Game::tick_with_hero_sounds` (`strafe::remap`, `strafe::remaps`) |
| Facing | `rc-game/src/hero/physics.rs` `Hero::turn_to` → `strafe::turn`; `Hero::state_physics` (`strafe::before_physics` / `after_physics`) |
| Motion | `rc-game/src/hero/physics.rs` `Hero::set_planar_vel` → `strafe::heading` |
| No long jump | `rc-game/src/hero/common.rs` `Hero::jump_or_running_jump` |
| Flip | `rc-game/src/hero/walk.rs` `Hero::tr_walk` (`strafe::flip_wanted`, `strafe::flip_entered`); `rc-game/src/hero/physics.rs` `Hero::long_jump_air` (`strafe::flip_speed`) |
| Camera | `rc-game/src/follow_camera.rs` `Camera::update_type0` (the 0x81 hold, `strafe::active`) |
| Legs | `rc-game/src/hero.rs` `hero_update_with_sounds` (`strafe::legs_update`, mode 0); `rc-engine/src/gameplay.rs` `hero_display_state` (`strafe::display_state`, used by `publish_anim` and `upload`) and `upload` (`strafe::twist_rows` on the palette) |

**Classic is bit-identical.** Every hook is gated by `Strafe::held`, which only the remap sets and only with Modern;
the hero's strafe block sits after the hero digest's cut (`owned`), and the NO_IDLE digest guard
(`cargo xtask test-job shared`) passes against the baseline taken before the first edit.

## 3. Deviations from rac1-native-game

* **The motion with the stick centred** keeps the last stick heading; theirs falls back to the facing (a strafing stop
  turned toward the camera's heading).
* **The flip's UYA numbers** (5 u/s start, the 1-tick takeoff, max(5.5 · tilt, 2) at 47 u/s²) are theirs only with
  Movement = RAC 3; this port has no such option, so Modern always uses them (the flip is the strafe's own move).
* **The camera**: they hold the view on their own aim yaw and ease it back over 60 frames after the release (fix
  c794a49). The port uses the game's own strafe hold (the hover's), which has no state of its own, so the release needs
  no easing.
* **The remap's scope**: theirs takes L2 / R2 in every gameplay frame; here the Thruster-Pack hover, the Hoverboard and
  the bodies keep them (they read them for their own moves).
* **The backpedal** is a display state the engine draws (`strafe::display_state`), never written into the game's
  animation state, so there is nothing to restore (their restore had the stale-write bug fixed in f0eb96f). The clip's
  frame count is the sequence's own (`AnimView::frame_count_b`; their byte moby +0x7e is the trigger count in this
  port's layout).

## 4. Checks

* Unit tests (`rc-game` `hero::strafe::tests`): the remap and its scope, the strafe states, the facing / motion rule,
  the flip speed rule, the legs' target, the backpedal display, the twist's joints, the setting's values.
* Scripted hero test `hero::hero_novalis::novalis_modern_strafe` (Novalis, `extracted/`): Modern, R2 + stick right
  for 90 ticks: 6.2 units to the camera's right (−1.60 rad from its heading), the yaw within 0.002 rad of the camera's,
  the camera's yaw unchanged, the legs at −π/2, states 0 / 2 only; ✕ then is the flip 0xb, sideways; R2 + forward ✕ is
  a jump 7 (Classic's same run without R2: 9); Classic R2 + right is the crouch 4; Modern without L2 / R2 equals
  Classic tick for tick.
* Engine (`RC_LEVEL=01 RC_SCENE=0`, `RC_SETTINGS_FILE` with `strafing = modern`,
  `RC_PLAY_SCRIPT="60-220:stick 1 0,60-220:press R2"`, `RC_SCREENSHOT_FRAME=150`): `work/captures/strafe/` (not
  committed): Ratchet's back to the camera, legs and tail turned to his right, mid-stride.

Open: G-HERO-044 (docs/plan/gaps.md).
