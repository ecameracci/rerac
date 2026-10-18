# Quick Select: Modern (port-only)

Not in the game. A Port Options row, **Quick Select**: **Classic** (the first value and the default: the game's, the
world runs under the ring) and **Modern** (the world holds still while △ keeps the quick-select ring open, so a weapon
can be picked in a fight without being hit). The rule is ported natively after rac1-native-game by Daino Selvatico,
MIT, docs/advanced.md 'Quick Select: Pause' (`advanced_world_paused`, src/game/enhance/advanced.cpp; its gate in
`gameplay_frame` 0x297f78, src/game/ovl/game_frame.cpp).

## 1. The rule

`rc_game::tick::quick_select_hold(modern, ring_open, pad_data)`, true when all three hold:

* the row is Modern;
* △ is held in this tick's pad block: PAD+0x1c0's bit 0x10 (`pad::button::TRIANGLE`). The mirror option swaps only
  ←/→ and the lock modes (the ring's PAD+0x1cc = 2) leave +0x1c0 alone, so it is the block's own bit (the unit test
  `tick::tests::the_triangle_bit_is_the_pads_unmirrored_held` checks it against `PadState::update`);
* the quick select is the open HUD element: slot 3 holds the ring (`QuickSelectUpdate` 0x24d238; theirs reads the
  slot's update +0x14 for L00 0x237ba0) or Clank's command page (L00 0x238b80), as the last HUD update left it
  (`QuickSelect::open`, `MenuMode::ring_open`).

So the △ press that opens the ring is a normal tick (the ring is not open yet), the world holds from the next tick to
the release, and the release tick runs: the ring's fade-out writes the hand request 0x141408 and the swap takes it on
the following ticks exactly as in Classic. A one-frame tap never holds. Menus are not touched: the rule is tested only
in the gameplay tick, and the ring only opens in mode 0.

## 2. The hooks

| Where | What |
|---|---|
| `rc-game/src/tick.rs` `quick_select_hold` | the rule (§1) |
| `rc-game/src/tick.rs` `Game::hold_tick` | the held tick: `UpdatePad` only (the ring and the pause tests read the pad), `cheat_toggled` cleared; no free-slot pass, moby loop, hero, particles, camera, sound step or counter |
| `rc-engine/src/gameplay.rs` `tick` | after the input (and the respawn / death reload, which never meet a held tick), the rule; held: `hold_tick`, `Play::qs_hold` / `qs_held`, the help's dialogue step (`help_frame` in the scene / vendor frames' form, after `Help::sync_in`), return. Skipped with it: the item-global and save syncs, `map_tick`, the play time 0x15eea4, the HUD weapon slot write |
| `rc-engine/src/gameplay.rs` `QuickSelectMode` | the setting: key `quick_select` (`classic` / `modern`) in the port settings file, `RC_QUICK_SELECT` at start (frame-exact runs read no file) |
| `rc-engine/src/menu_render.rs` `menu_frame` | the mode-0 frame test is (counter, `qs_held`), so the ring's frame runs on a held tick; `MenuMode::ring_open = qs.open` after the ring's update (false outside mode 0); the cursor pulse reads counter + `qs_held`; `PortSettings` syncs the row both ways (pause and main menu: one list) |
| `rc-game/src/menus/pause/port.rs` | the row (`Setting::QuickSelect`, last in `ENTRIES`; texts −0x1c0..−0x1c2) |
| `rc-engine/src/sky_stars.rs` `star_frame` | no star frame on a held tick: its `rand` draws stay off the game's stream |
| `rc-engine/src/moby_anim.rs` `tick` | no `MobyAnimAdvance` of the unported classes' instances on a held tick (now ordered after `GameTick`, which writes the flag; the order does not change Classic: the undriven instances read nothing the tick writes and the driven ones skip the advance) |
| `rc-engine/src/water_render.rs` `tick` | no 761 bob step on a held tick (ordered after `GameTick` likewise) |

The hero's `GameOptions` is rebuilt from the save every tick (`gameplay::tick`); the setting is not one of them (a
resource of its own), so the rebuild cannot lose it.

## 3. What runs during the hold

* **The ring**: its hero part `QuickSelect::hero` (the open test, the tap and double-tap timers) and its HUD update
  `QuickSelect::update` (the open animation, the lock timer, the stick / d-pad selection, the fade and the request on
  release) run every frame on the held tick's pad, as in Classic; the test checks its fields frame for frame against
  Classic's.
* **The pause tests** (0x2aba68: Start, Select / R3, the freeze dialogs), the save notice and the "use" system's
  after-tick (idempotent with no tick: its hand-offs are drained) run, as theirs run the pause / menu checks before
  the freeze.
* **The HUD** (its timers on the frame clock `GameTicks`), the lights and shadows (derived from the held state), the
  help's dialogue player step (a voice line keeps playing), the audio layer (`audio_out::run_audio` fills the ticks
  without a sound step with IOP-only frames: playing sounds go on, nothing new starts, no EE `sound_update`, no `rand`
  draw), and `elapsed_frame` 0x15ee24 (the game's frame counter, every frame in every mode).

Not run: everything `FUN_002ab960` and the gate bits 1 / 2 / 4 / 8 of theirs stop: the hero, the mobys (with their
sound emitters, wind and draw lists), the particles and spin timers, the camera, the counter 0x15f5cc and the play
time 0x15eea4; and here also the sky stars, the unported mobys' animation and the 761 bob, which the port runs
outside the gameplay tick. The `rand` stream is untouched, so the world resumes exactly where it held.

**Deviations from the reference port** (intended): the ring's hero part keeps its timers counting (theirs stops with
the hero update; here it is the ring's own code, which must run for the ring to work as in Classic); the sound step
does not run (theirs runs `sound_frame`; here its `rand` draws would move the world's stream).

## 4. Verified

* Unit tests: `tick::tests` (the rule: Classic, ring closed, △ released, pad lost, short block, other buttons; the △
  bit against `PadState::update` for both mirrors and the three lock modes).
* `hero_quick_select_pause` (hero binary, Novalis, the ring script `60-100:press TRIANGLE,70-100:stick 0 -1`):
  Classic: no hold, the counter every frame, Ratchet walking under the ring before its pad lock, a moby moving, the
  Bomb Glove swap 8 → 0 at frame 103 → 10 at 110. Modern: held exactly on frames 61..=100; the counter, `rand`,
  Ratchet's state / position, the camera, every moby's state / position and the particle count as on frame 60; the
  ring's fields and the request frame for frame Classic's; the same hand changes (103, 110); deterministic. A △ tap
  and the frames up to the ring's open are Classic's.
* Classic unchanged: the `shared` guard set (the NO_IDLE hero digest identical to the baseline taken before the
  first edit, the all-levels smoke).
* The engine, frame-exact (`RC_LEVEL=1 RC_FRONTEND=0 RC_SCENE=0`, the script above, `RC_DUMP_FRAMES=56..116`,
  `work/captures/qs_pause/{modern,classic}/`): under Modern Ratchet keeps his frame-62 pose and the sky's ship its
  place through frame 100 with the ring open on the Bomb Glove; under Classic he turns and walks; both open the ring
  on frame 60 and close it on 108 with the request 10; frame 116 shows the glove in his hand.

## 5. Left

G-UI-032 (gaps.md): the sky shells' rotation (`sky_render`, on `GameTicks`, not 0x15f5cc) keeps turning; the HUD's
`play_hero_sound` (`hud_render`: the oxygen meter's low-air beep) draws the game's `rand` if it fires during a hold.
