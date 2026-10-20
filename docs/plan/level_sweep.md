# All-level smoke sweep (2026-09-26)

A read-only pass of all 19 levels through `cargo dev` (Bevy dev build, Apple M4, window 1024×832, surface
2048×1664). Only Novalis (01) had been looked at on screen before this pass. Nothing was fixed. This page
lists what was seen.

## How it was run

- `RC_LEVEL=NN RC_SCREENSHOT_FRAME=120 RC_SCREENSHOT=NN_default.png cargo dev`: the default camera frames the tfrag
  bounding spheres (main.rs `setup`; since 2026-10-19 it frames the level's spawn, H3).
- `NN_low.png`: `RC_OCCL=0`. `RC_CAM` = P + (0, −30, 15) looking at P, with P the ship spawn (level settings
  +0x2c) where the engine created one (01, 14, 15), else the tfrag sphere centroid. No log reports a class-0
  (hero) moby instance. Levels 05, 07, 08, 15 and 18 put that camera inside rock, so they
  also got `NN_low2.png` from P + (0, −90, 60).
- fps: `RC_NOVSYNC=1`, wall-clock capture after 16 s, levels 00/01/10/18 only.
- Source: the disc image (`source: iso …`) on every run.
- Revision: the tree was being edited by other agents during the sweep, and there are no commits yet, so the
  revision is given by file mtimes. Default shots 12:41–12:42: newest sources `rc-formats/src/gameplay.rs`
  12:39:39 and `rc-engine/src/particle_render.rs` 12:37:30. Every later run (low shots, fps, diagnostics,
  12:45–12:57): the same sources plus `particle_render.rs` 12:43:10. `rc-formats/src/moby_anim.rs` changed at
  12:57:49, after the last run. Every `cargo dev` built without errors, so no build-failure retries were
  needed.
- Artefacts (logs, PNGs, crops, contact sheets):
  the sweep session's scratchpad folder `sweep/`
  (`NN.log`, `NN_default.png`, `NN_low.png`, `NN_low2.png`, `NN_fps.log`, `diag/`). They are outside the repo and
  are not kept.

Level names: `docs/formats/disc_layout.md` has no name table. The names below follow the conventional RAC1
level order. Each one matches what is on screen, except 02, where the match is uncertain (see W3).

## Results

Load = `level_load::load_level` total (the WAD decompress share in brackets). Build = the later per-layer log
times summed (tie/moby/shrub parse, lighting and mesh build, plus tfrag mesh build and upload). Counts:
tfrags / LOD-0 triangles; ties placed (classes); shrubs placed; mobys placed / in the gameplay file; textures;
occlusion cells. Sky = shells / gouraud shells, clear policy. Every run exited 0 and no panic, backtrace or
`error` line appeared in any log.

| NN | Name | Exit | Load ms | Build ms | Tfrags / tris | Ties | Shrubs | Mobys | Tex | Occl cells | Sky | fps (novsync) | Anomalies (see issue ids) |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 00 | Veldin (Kyzil Plateau) | 0 | 65.8 (37.7) | 63 | 460 / 24,520 | 1114 (63) | 1697 | 286/296 | 415 | 3251 | 5/1 first | ~105 | M1 (no ship; plausible for the opening) |
| 01 | Novalis | 0 | 78.9 (41.7) | 109 | 1004 / 61,919 | 1508 (95) | 1208 | 919/984 | 619 | 6039 | 5/1 first | ~135 | none; ship drawn |
| 02 | Aridia (?) | 0 | 76.8 (34.3) | 95 | 695 / 41,743 | 3751 (105) | 1199 | 740/775 | 440 | 7809 | 4/1 first | – | W3 very dark, green ground, pure-black voids; M1 |
| 03 | Kerwan | 0 | 75.7 (40.0) | 111 | 1006 / 61,624 | 5105 (127) | 975 | 994/1029 | 438 | 16445 | 3/1 first | – | none seen; M1 |
| 04 | Eudora | 0 | 69.6 (36.7) | 94 | 709 / 43,962 | 1249 (79) | 835 | 504/536 | 484 | 5003 | 4/1 first | – | none seen; M1 |
| 05 | Rilgar | 0 | 86.4 (51.2) | 103 | 847 / 63,344 | 1855 (112) | 1216 | 1397/1431 | 506 | 15929 | 2/0 every | – | M2 (6,616 untextured moby tris skipped); H2 capture stall; no stars (known) |
| 06 | Blarg Station | 0 | 99.7 (52.2) | 125 | 2144 / 115,784 | 1290 (97) | 519 | 705/766 | 587 | 5886 | 3/1 first | – | none seen (the textured starfield shell draws); M1 |
| 07 | Umbris | 0 | 67.3 (34.1) | 71 | 1027 / 42,860 | 1150 (64) | 817 | 608/649 | 386 | 9011 | 3/0 every | – | very dark (probably correct); T1 1241 tie slots at the clamp; M1 |
| 08 | Batalia | 0 | 67.7 (36.9) | 80 | 454 / 27,548 | 3388 (109) | 1067 | 795/847 | 419 | 8982 | 3/1 first | – | flat (13,13,19) sky (verify); M1 |
| 09 | Gaspar | 0 | 77.0 (45.0) | 84 | 444 / 31,532 | 2153 (69) | 3511 | 871/908 | 397 | 23817 | 5/1 first | – | W2 flat orange under the world, dark-maroon lava bed; M1 |
| 10 | Orxon | 0 | 75.7 (43.0) | 90 | 892 / 51,460 | 1883 (127) | 1637 | 801/872 | 425 | 8398 | 5/0 every | ~131 | W4 lime clear colour in gaps and under the sky band; M1 |
| 11 | Pokitaru | 0 | 78.8 (41.7) | 91 | 1170 / 52,900 | 1331 (43) | 1307 | 688/720 | 437 | 26726 | 5/1 first | – | **L1 no sea surface**: default frame ≈ 90 % pure blue; fish float over the seabed; M1 |
| 12 | Hoven | 0 | 81.2 (40.0) | 120 | 1108 / 80,160 | 4604 (101) | 1756 | 659/695 | 566 | 6976 | 5/1 first | – | **L1 no sea surface**: icebergs on a flat (83,107,135) void; M3 grid of 10 parked helicopters; M1 |
| 13 | Gemlik Base | 0 | 92.4 (50.7) | 105 | 1096 / 70,444 | 2040 (120) | 976 | 922/970 | 501 | 11451 | 3/1 every | – | none seen; default camera close to a hull; no stars (known); M1 |
| 14 | Oltanis | 0* | 83.6 (52.8) | 82 | 961 / 52,748 | 1773 (80) | 556 | 432/552 | 402 | 17557 | 3/1 every | – | dark (probably correct); 120 mobys without class geometry; ship drawn. *first attempt: H1 |
| 15 | Quartu | 0 | 89.3 (44.4) | 122 | 2119 / 78,946 | 2501 (112) | 1095 | 865/911 | 581 | 8629 | 2/0 every | – | none seen; ship spawn inside rock at the low offset; no stars (known) |
| 16 | Kalebo III | 0 | 93.3 (43.4) | 125 | 1769 / 64,466 | 3078 (124) | 1622 | 1534/1569 | 467 | 18796 | 5/1 first | – | H3 default camera buried in terrain (frame all brown); M2 (9,118 untextured moby tris skipped); M1 |
| 17 | Drek's Fleet | 0 | 76.1 (38.6) | 89 | 967 / 63,144 | 1157 (85) | 511 | 689/734 | 419 | 12133 | 3/1 first | – | T1 1536 tie slots at the clamp; fighter grid formation (probably real); M1 |
| 18 | Veldin (finale) | 0 | 88.6 (43.1) | 131 | 1144 / 61,350 | 3782 (92) | 3058 | 953/991 | 615 | 17271 | 7/1 first | ~89 | very dark, flat (10,10,10) sky (verify); M1 |

- Wall time per run is 3.1–3.5 s, from launch to frame-120 capture and exit, cached build.
- fps is the steady state (the 2nd and 3rd 5-s windows; the first window includes pipeline compilation):
  00 97.9/105.9/105.2, 01 126.1/134.2/136.4, 10 128.5/133.3/131.1, 18 83.5/88.7/89.8.
- Things that looked right on all 19 levels:
  - no magenta, white or black textures;
  - no vertex explosions or spikes;
  - no mirrored or floating props, apart from the parked pools in M3;
  - the "every frame" clear policy printed by `sky_render` matches `sky_render_notes.md` exactly (05, 07, 10, 13,
    14, 15).

## Issues, by owner, most severe first

### Crashes / load failures
None. The loader and every renderer ran on all 19 levels.

### L — missing layer

**L1 (fixed 2026-09-28, see Resolutions). The open sea is not drawn on 11 Pokitaru and 12 Hoven.**
- **11:** the default camera looks at the open sea, and ~90 % of the frame is the sky dome's lower hemisphere.
  - A column read down the middle of `diag/11_back.png` goes (7,91,122) → (0,50,170) → (0,5,240) pure blue at
    the bottom of the frame. This is the gouraud nadir colour, not fog (0,86,127) or the background
    (43,181,202).
  - `11_low.png` (eye at z 225, below the waterline) shows the sand seabed with piranha mobys swimming in clear
    air, and no water surface or tint.
  - Ties and shrubs off (`RC_NO_TIES=1 RC_NO_SHRUBS=1`) leave the default frame unchanged, so the blue comes from
    neither of them.
- **12:** in `12_default.png` / `12_low.png`, icebergs and platforms stand on a flat (83,107,135) void, with no
  water plane or shoreline.
- **Other levels to check for the same gap:**
  - 09 (W2): a flat orange (249,143,28 family) under the world.
  - 02 (W3): pure-black voids.
- **What to find out:** no renderer in `rc-engine` draws water. Find which object holds the sea: a moby class in
  the untextured / env pass (M2), a tfrag with animated UVs, or a dedicated water path.

### W — wrong look

**W1 (sky / fog, verify). The sky's lower hemisphere shows through wherever the world has no floor.**
- This is correct GS behaviour when there is really nothing below the world. It becomes the visible symptom of
  L1 when a water or floor layer is missing.
- Keep it in mind when judging 02, 08, 09 and 18 against reference footage.

**W2 (09 Gaspar, verify).**
- Below the rock spires, a saturated flat orange fills the frame (`09_default.png` left third and upper band).
- The only drawn floor, in `09_low.png`, is a dark-maroon lava bed with red mushrooms on it. It is not bright.
- **Possible causes:**
  - a missing emissive/animated lava layer (same family as L1);
  - the lava tfrag's colour after `LightTfrags`.

**W3 (02, verify against reference).**
- The whole level is very dark (frame mean (10,15,18)).
- The ground reads as flat green with little texture detail (`02_crop_terrain.png`).
- The gaps between terrain tongues are pure (0,0,0).
- Level 02 is conventionally Aridia, a sand desert. If that is right, the green ground points at tfrag
  lighting or texture palette (tfrag_light / textures).
- Log facts: background (15,25,75), FOGCOL (0,0,10), sky 4 shells with 1 gouraud dome, first-frame clear.

**W4 (10 Orxon, probably faithful, verify).**
- Bright lime triangles (117,170,25) = the GS clear colour appear:
  - between distant fogged mountains, e.g. `diag/10_crop_tl.png`;
  - in a band below the textured cloud shell (`diag/10_tilt_s.png`).
- They are still there with `RC_OCCL=0`, so occlusion is not the cause.
- Level 10 clears every frame and has no gouraud shell, so the game should show the clear colour at the world
  edge too. The contrast with the fog colour (60,100,0) makes them read as holes.
- Only a reference capture can decide this.

**W5 (08 Batalia, 18 Veldin finale, verify).**
- The sky is a flat uniform colour: 08 (13,13,19), 18 (10,10,10), with no gradient and no cloud texture visible
  in the default or low2 frames.
- Both levels have textured shells (08 has 2 textures, 18 has 7), so check that they draw and are not fully
  transparent.

### moby / moby_spawn

**M1 (moby_spawn). The ship is created on only 3 of 19 levels.**
- 16 levels log `moby spawn: the level places a ship but class 531 is not loaded; none created`: 00, 02–13, 16,
  17, 18.
- The engine only tries `SHIP_CLASSES[0]` = 531 (`rc-formats/src/moby_spawn.rs:28`).
- Check which of 531/532/533 each level's class table contains, and which index `DoSpaceTransition` implies per
  level.
- 00 (the Veldin opening, before the ship is built) probably correctly has none.

**M2 (moby_render). Many untextured moby triangles are skipped.**
- Log: `skipped 0 chrome + 0 glass + N untextured`. N per level:

  | Level | 16 | 05 | 14 | 11 | 10 | 08 | 12 | 17 | 06 | 09 | 02 | 18 | 13 | 03 | 15 | 07 | 01 | 00 | 04 |
  |---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
  | N | 9118 | 6616 | 1732 | 1482 | 1314 | 1312 | 1091 | 982 | 890 | 776 | 732 | 696 | 496 | 356 | 306 | 159 | 144 | 95 | 0 |

- These are real geometry the game draws, e.g. glows, beams or water. They are a candidate for part of L1 and
  for missing effects on 16 and 05.

**M3 (moby_spawn, 12 Hoven).**
- A 5×2 grid of identical helicopters sits in mid-air over the missing sea, all overlapping (`diag/12_crop_heli.png`).
- It looks like a parked enemy pool that the game keeps hidden or inactive until triggered. Check its
  spawn/visibility rule.
- **Resolved 2026-10-01** (gaps.md G-CLS-018): not a pool. They are class 336 (11 placed, census U392, unported),
  path flyers whose state 0 moves them onto their spline (pvar +0x74, point +0xa0); without a port they stay where
  they were placed.
- The fighter grids on 17 look like intended formations. Lower priority.

**M4 (moby, info). Moby positions outside the tfrag sphere bounds.**
- `N instance positions outside the tfrag sphere bounds`: 03 41, 05 10, 06 9, 09 21, 11 46, 13 87, 16 90,
  18 67.
- None was seen on screen as misplaced. Info only.

**M5 (moby, info).** 14 Oltanis has 120 of 552 instances without class geometry. Other levels have 10–71.

### tie

**T1 (tie_light, verify). Many lit slots at the 243 clamp.**
- Log: `N lit slots at the 243 clamp`. Novalis has 0, while 17 has 1536, 07 1241, 06 454, 00 206, 05 111,
  09 129, 02 38, 04 20, 15 3.
- No over-bright ties were obvious on screen. Confirm that the clamp is the game's (`LightTies`) and not an
  overflow.

### tfrag / shrub / occlusion
Nothing specific found. There were no missing tfrags, and every tfrag, tie and moby maps to an occlusion cell
(`mapped tfrags N/N, ties N/N`). The W4 gaps are not occlusion.

### harness / level_load (main.rs, determinism.rs)

**H1 (determinism). A run can exit 0 without saving the screenshot.**
- The first level-14 run logged `Monitor removed` → `No windows are open, exiting` after 1.2 s, exited 0 and
  saved no PNG (`14_attempt1_monitor_removed.log`). A retry was fine.
- A capture run that exits before the capture should return non-zero.

**H2 (determinism). The blank-frame retry is unbounded and silently breaks frame-exactness.**
- `05_low2` logged `determinism: frame N came back blank …` 2,814 times and finally saved **frame 8567**, not
  120. It took 144 s, with `fps: 60.0` logged throughout.
- The window was probably not being presented (focus or occlusion), since the eventual image has content.
- The retry needs a cap and a non-zero exit, or at least the captured frame number in the file name or exit
  status.

**H3 (main.rs camera). The default framing puts the camera inside or against geometry.**
- 16: the whole frame is one brown terrain texture.
- 13: the frame is a hull close-up.
- A spawn-based default (ship or hero start) would make first looks at new levels useful.
- **Fixed 2026-10-19 (G-TOOL-015; `rc-engine/src/fly_cam.rs` `spawn_view`, called from main.rs `setup_camera`).**
  Without `RC_CAM` the fly camera starts 20 units behind and 10 above Ratchet's start (the placed class-0 instance,
  the one crate::gameplay `setup` takes) along its yaw, looking 2 units above it; a level without one falls back to
  this sweep's ship framing (level settings +0x2c, P + (0, −30, 15) looking at P), then to the tfrag-sphere framing.
  `RC_CAM` still overrides. Checked with `RC_PLAY=0` captures at frame 120 (`work/captures/g-tool-015/`): 16 shows
  Ratchet on the start platform with the city behind, 01 Ratchet on the landing meadow, 13 Gemlik's landing deck
  with Ratchet hidden behind a girder. Leftovers (no line-of-sight test; the boot level only): G-TOOL-025.

## Expected and known gaps (not counted above)
- Star sprites: 00, 02, 05, 06, 07, 13, 15, 17 (`sky_sprite_proc`, not ported). 06 and 17 still show stars,
  which come from their textured starfield shells.
- Water/lava animation (scrolling textures), particles beyond Novalis's class-27 emitters, HUD, the hero
  (the logs report no class-0 instance), and the moby metal/glass passes (0 chrome/glass triangles counted on
  all 19 levels, as on Novalis in moby_render_notes.md).
- `tfrag_render` INFO lines `tfrag tex N SamplerKey … draws [OpaqueTested …, ColorOnlyLowAlpha …]` on every
  level: diagnostic, not warnings.

## Resolutions (2026-09-26, second pass)

**M1 fixed (`rc-formats/src/moby_spawn.rs`, `rc-engine/src/moby_spawn.rs`).**
- **Which ship.** `DoSpaceTransition` (L01 0x2a68f8 = boot 0x231ff0) sets the ship index 0x13e056 before it loads
  destination planet L:
  - 0, raised to 1 if planet 8 is unlocked (0x13dd40[8]) or L > 7;
  - raised to 2 if planet 14 is unlocked (0x13dd40[14]) or L > 13.
  - A new game starts from the zeroed save, so the index is 0.
  - First visits in story order: 0 on 00–07, 1 on 08–13, 2 on 14–18 (`moby_spawn::first_visit_ship`). `RC_SHIP=0|1|2`
    overrides it.
- **Where the class comes from.** The loader streams TOC `spaceships` entry `ship + 1` (TOC in RAM at 0x137b80;
  `FUN_00257dc8` case 1). Entry k holds class 530 + k. `FUN_00253a18` then registers the ship class and a second
  class (`0x160558[ship]` = 535/536/537) only when the level core lacks them. Each class gets one texture:
  - ship: a 256×256 PSMT8 PIF, 4 mips, descriptor 0x15fc00;
  - second class: 128×128, descriptor 0x15fc10.
- **Core copies.** 531 is in the cores of 01, 14 and 15; 532 in 07 and 10; 533 and 537 in 13. On first visits
  only 01 (531) and 10 (532) use the core copy. Every other level now loads the class and its texture from
  `global/spaceships/NNN.bin` (`moby_spawn::parse_spaceship`).
  - That file is read via `disc_source::read`, which does not serve global files from the ISO. It therefore comes
    from `extracted/`.
- **Level 13 special case.** The loader's branch for level 13 with ship index 2 reuses the placed 533 instance at
  (467, 585, 317). It only fires on a revisit; a first visit creates 532.
- **Levels with a ship.** Level-settings +0x2c > 0 on all 19 levels, so the loader creates a ship on every level.
  - **00 is included:** its settings put the ship at (20, 20, 20), yaw 0. That is a parked copy of 531 in a
    corner of the map, far from the play area; the ship you see on Veldin is the gameplay-placed class-530
    instance at (132.2, 118.6, 31.3).
- **Checked on screen** (frame 120, camera at P + (−14, −14, 7)): 00, 02, 05, 08, 12, 13, 16, 18.
  - Each shows the ship of its era on its landing pad in sequence 1 (landed): 531 green, 532 orange, 533 purple.
  - On 00 the ship hangs at (20, 20, 20) in a canyon corner.

**H1/H2 fixed (`rc-engine/src/determinism.rs`).**
- **Cause of H2.** An occluded macOS window (covered, on another Space, or behind a full-screen app) gets no
  drawable: wgpu-hal Metal returns `SurfaceError::Occluded`. Bevy then draws nothing, and every window
  screenshot is all zeros.
  - Reproduced here: 2,105 blank retries, frame 6617 saved.
  - `WindowLevel::AlwaysOnTop` did not help (still 1,721 frames occluded).
- **H2 fix: offscreen capture.** With `RC_SCREENSHOT_FRAME`, every camera that targets the primary window
  renders to an offscreen `Bgra8UnormSrgb` image. The image is kept at the window's physical size, and that
  image is what gets captured.
  - It is pixel-identical to a window capture of the same frame (checked on 05, frame 1721).
  - `RC_CAPTURE_WINDOW=1` restores the window capture.
- **Blank retries are capped.** After 30 frames the image is saved anyway with a warning. Any capture that is
  not of frame N exits with status 3.
- **Cause of H1.** Bevy 0.19 links each window to its monitor entity (`OnMonitor` → `HasWindows`, `linked_spawn`).
  A transient "Monitor removed" therefore despawns the window, and the app exits 0.
- **H1 fix.** The link is removed as soon as it is inserted. If the app exits before the capture is saved, the
  process exits with status 2.
- **Test.** 10 consecutive `RC_LEVEL=05 RC_SCREENSHOT_FRAME=120` runs gave frame 120 each time, with 0 blank
  retries and identical PNGs (same md5).
  - The app took about 3 s from renderer init to exit. Total wall time was 3.6–8.1 s; the spread is cargo lock
    waits and rebuilds caused by other agents.

**W3 (02): not a bug, the level is a night scene.**
- The lighting inputs are read correctly:
  - gameplay +0x04 → section 0x7b360, count 1 (≤ 12, no cap);
  - the level02 loader (`FUN_00243490`) copies `count << 6` bytes to its bank exactly as the port reads them;
  - every select is 0 or the 0xf sentinel (1,057 vertices, which are lit by base colour only, as in the game).
- **Set 0:** A colour (0.37, 0.57, 0.72) dir (−0.89, −0.32, −0.34), B colour (0.04, 0.09, 0.11). This is a blue
  moonlight with a dim back light.
- **Mean vertex colours:**

  | | Base | Lit |
  |---|---|---|
  | 02 | (18.6, 33.6, 32.2) | (31.6, 54.7, 59.0) |
  | 01 | (40.8, 41.8, 42.4) | (75.0, 76.3, 65.5) |

- **Why the ground looks green.** Sand textures (tan) × a cyan vertex colour under GS MODULATE.
- **Sky data.** The level's own sky textures are a starfield, a nebula, star flares and two planets on a dark
  purple horizon. Also: background (15, 25, 75), FOGCOL (0, 0, 10), star sprites.
  - A horizon shot (`RC_CAM=191.4,140,45,260,200,50`) shows the starry sky with a large planet over moonlit dunes.
- No change to `tfrag_light.rs`.
- The pure-black gaps are the sky's lower hemisphere below terrain that has nothing under it (W1). Whether the
  game has a layer there is the L1 question.

**L1 fixed: the seas are draw-callback surfaces of level classes (docs/plan/world_animation.md §3.3).**
- No tfrag, tie, shrub, sky or moby geometry holds the sea: the placed controller mobys have no class geometry and draw
  the sea from a draw callback. Three mechanisms, each ported once and found by code identity (`rc_game::water::sea`,
  `rc-engine` `sea_render.rs`, `ClassUpdate::Sea(i)`):
  - the **liquid grid module** (level05 `0x2ce098` … `0x2ce830`): 03 (994), **05 (879, the open sea around Rilgar)**,
    07 (460, 1018), 08 (327), **09 (317, the lava bed: W2)**, 14 (1260, a translucent FIX 0x40 sludge);
  - the **camera-following ocean 1111** (level11 `0x30b358` / `0x30a908`): **11** and 16 (the same code);
  - the **Hoven liquid 1901** (level12 `0x30bff0` / `0x30be68`, static strips): **12**.
- The grids and 1901 draw on the draw list drained after the ties and before the shrubs (`0x16e100`), which level01
  never fills; the port now has it (`DrawCallbacks::ties`).
- Height: 05's grid is at 59.5 ± 0.25 (the collision water 59.44); 11's ocean at 223 ± 0.25 (collision 223); 12's open-sea
  strips at 24.92 / 25.99 (the 19 ripple patches at 25.5).
- Shots (`RC_CAM`, frame 120; before = `RC_SEA=0`, identical to the pre-change build: `nosea_11_*` md5 = the original
  `before_11_*`): 05 default / over `400,40,95,300,160,58` / under `330,70,55,330,160,66`; 11 `470,380,245,400,300,223`,
  default, under `470,380,216,400,300,226`; 12 `300,280,70,360,450,25`; 09 `300,100,90,300,300,25`; 14
  `300,200,80,300,350,16`; 07 `300,200,90,300,350,38`. Two runs of each are byte-identical; Novalis (no sea port) is
  byte-identical with and without the sea path.
- Not ported (listed in world_animation.md §3.3): 02's seven liquid meshes (`0x2a48a0`), 09's lava-flow meshes
  (`0x21e8c0`, `0x2c1978`), 12's 293 strips (`0x2bc210`, also on 14), 08's splash.

**W2 resolved:** the maroon bed was the tfrag under the lava; the lava itself is 317's liquid grid (z 25, 49×49 cells of
15, an animated FX 44..59 image, fog colour c8 64 14), now drawn. The lava streams are not ported yet (above).

**M2: the game draws texture −1 triangles.** See `docs/plan/moby_untextured.md`.
- At load, the ad-gif blocks with TEX0 index −1 are rewritten to a reserved 8×8 all-0x80 PSMCT32 texture at GS
  block 0x3ffb (`fun_00203338` / `fun_00202d78`).
- MODULATE with that texture gives plain Gouraud: colour = lit vertex colour, alpha = vertex alpha.
- Renderer change: bind a 1×1 (0x80, 0x80, 0x80, 0x80) image for −1 parts and draw them through the normal moby
  path. Keep −2/−3 skipped.
