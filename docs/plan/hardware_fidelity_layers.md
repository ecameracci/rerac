# Hardware fidelity layers (and how to go native later)

ReRAC is a rewrite, not an emulator: no MIPS/VU interpreter, no DMA/VIF/GS/IOP model, no BIOS, and none of the game's original code runs. To make
output identical to the PS2, some parts of the port still reproduce PS2 hardware or Sony-library behaviour on purpose. This file lists those
**fidelity layers**: what each one reproduces, what it buys, what a native replacement would be, and what switching would cost. It covers (A)
hardware-like models, (B) SDK/library reproductions, (D) timing and resolution conventions, and lists (C) format decoding separately, because reading
the disc needs it and nothing here emulates anything. The last section lists game bugs the port reproduces on purpose. That is game-logic fidelity,
not hardware fidelity, but a "native" port might drop those too. Survey date 2026-09-27. Policy source: docs/plan/decisions.md, "Native-first
fidelity policy (2026-09-27)" (see "Policy" below); the agent-facing rules are in docs/plan/orchestration.md §3.6.

## Policy

The port must behave like the original (what the player sees, hears and feels) using our own native systems.
Bit-exactness is a diagnostic tool, not the goal. Full text: docs/plan/decisions.md, "Native-first fidelity policy
(2026-09-27)".

- **Phase 1 (now): no new layers.** Nothing may be added to this catalogue. New code uses standard IEEE floats and
  native Bevy mechanisms. The layers listed below stay for now because they serve diagnosis (bit-exact PCSX2
  comparison).
- **Noticeable effects are reproduced by result, not mechanism.** When a PS2 arithmetic effect is noticeable in play,
  the port reproduces the result natively (example: the water ripple steps every 9 ticks on the PS2 because of float
  truncation; the port steps every 9 ticks and does not model the float unit). Record each one under "Result-level
  reproductions" below.
- **PCSX2 mismatches that are pure hardware arithmetic** (last-bit float rounding, GS blend byte differences, sound-chip
  interpolation) are accepted with a documented tolerance, recorded under "Tolerances" below. Mismatches that come from
  misunderstanding the game are bugs and get fixed natively (triage: docs/plan/trace_harness.md, "Triage").
- **Phase 2 (later): strict native pass.** Replace each layer in the order of "Suggested order if going native" at the
  end of this file, keeping only behaviour-level fixes where differences are noticeable. The PS2 float model (A1) goes
  last, after the PCSX2 checks have served their purpose.

### Result-level reproductions

Native reproductions of PS2 effects that are noticeable in play. One row each.

| Effect (as the player notices it) | PS2 cause | Native reproduction (file) | Evidence |
|---|---|---|---|
| *(e.g. water ripple advances every 9 ticks)* | *(e.g. f32 truncation in the ripple phase accumulator)* | *(e.g. integer tick counter, `rc-game/src/water.rs`)* | *(e.g. PCSX2 trace, doc §)* |
| The Comet-Strike's catch: when the wrench is nearly back, Ratchet's anim jumps from the throw loop to the catch frames in one tick | `0x247d18` sets the loop exit 0x13fe08; the next key step of Ratchet's advance sets the rate 0x13fde4 to `0x15f708` = `0x7f800000`, which the PS2 FPU treats as the largest finite value (2¹²⁸, no infinity): the advance after it steps onto the key at once, and `(speed·2¹²⁸ − 1)/2¹²⁸` leaves t = speed. IEEE would give ∞ and NaN | the exit's next advance completes one key step and continues with `t = speed · rate` of the new key; no infinite rate is stored (`rc-game/src/hero/anim.rs` `RatchetAnim::jump`, test `anim::tests::loop_exit_jumps_to_the_key`) | disassembly of 0x247d48 (0x247ed0 `lwc1 f23, 0x15f708`) and the ELF data word; not trace-checked |
| Moby shadows darken the ground by 25 % with a hard edge, and never the casters | `ShadowResolve` 0x29b580: ALPHA_1 0x2000000064 on the frame buffer's display bytes (`Cd − ⌈Cd/4⌉`), gated by DATE on the destination-alpha counter the volumes built by texture feedback (TEX0 = the frame buffer, MODULATE 0x82 / 0x7f) | a native count target (R16Float, +1 / −1 per face, depth-tested against the scene) and one fixed multiply blend `dst·0.53` on the sRGB target (0.75^≈2.2 in linear light); casters drawn after the pass (`rc-engine/src/shadow_render.rs`, `moby_render.rs` `caster_pass`) | the multiply is within ±2 of the GS byte on 244 of 256 display bytes and −3 on 12 dark ones (12..40); test `shadow_render::tests::factor_matches_the_gs_bytes`. Not PCSX2-compared per pixel (docs/plan/shadows.md) |
| Explosion light on tfrags, ties and shrubs steps in whole colour bytes and saturates (255 tfrags, 243 ties / shrubs) | `LightTfrags` / `LightTies` / `LightShrubs` add each light on the float grid `65536 + c/128` (truncating VU0 FMAC), i.e. `floor(128·colour·d)` per light, then clamp | IEEE f32 in the world vertex shaders with the game's operation order, `floor(128·colour·d)` per light and the same clamps; the PS2 last-bit rounding is not modelled (`assets/shaders/world_lights.wgsl`, `rc-engine/src/world_lights.rs`; tfrag_lighting.md §9) | same formulas as the bit-exact CPU passes (`tfrag_light::light_tfrag`, `tie_light::light_regs`, `shrub_light::light_regs`, with points); expected ≤ 1 byte from them; not PCSX2-compared |
| Additive and translucent effects glow like the PS2's: dim additive puffs stack into saturated white-hot cores (the arrival crash's orb trail, the Bomb Glove's fire, the fire fields), translucent shells tint dark ground less | The GS blends on the frame buffer's display-encoded bytes: ALPHA 0x48 `Cd + (Cs·As >> 7)` and 0x44 `Cd + ((Cs − Cd)·As >> 7)`, clamped to 0..255 per draw, with MODULATE `Ct·Cv >> 7` letting vertex colours above 0x80 brighten up to 2× | a render-target choice, one mechanism for every effect entity (`DisplayEffect`: the particles, the effect mobys `MobyBlend::Translucent` / `Additive`, the soft edge of the mobys' glow packets `GsPass::EffectLowAlpha`, a gold weapon's metal (`ExtraMobys::set_gold`: its gold map's alpha 60..0x80 makes the linear-light blend visibly wash out the texture under it), the draw-callback effects of `fx_draw.rs` incl. the fire fields, the nanotech glow and the glow quad `0x2781d0`): after Bevy's transparent pass their `Transparent3d` items are drawn into an `Rgba8Unorm` copy of the frame that holds the display bytes themselves, with ordinary hardware blending (`One, OneMinusSrcAlpha` on the premultiplied GS terms), depth-tested against the scene, then converted back (exact per byte both ways); the world keeps linear-light blending (`rc-engine/src/display_blend.rs`, `assets/shaders/display_blend.wgsl`, `effect_blit.wgsl`) | stacking and order as the GS (hardware rounding of `Cs·As/128` instead of truncation: ±1 level; As > 0x80 clamps in the 0x44 destination factor; effects drawn after all translucent world draws). Screenshots vs PCSX2 (2026-09-27, particles.md "Display-space blending"): crash orb trail, Bomb Glove explosion, nanotech crate; flyer trails unchanged. Cost within the frame-rate noise (≈118 fps before and after at the flyer view) |
| A moby's animation loop sound (engines, jetpacks) that could not start is retried every 4th tick, each moby on its own phase | `MobyAnimAdvance` 0x265260 runs the loop refresh of a moby without a voice slot only when `(moby address >> 8 & 3) == (tick & 3)`: the phase comes from the moby's RAM address (array at 0x1e9a480 on Novalis, stride 0x100) | the phase is the moby's table index `& 3` (`rc-game/src/moby_update/anim_sound.rs` `advance`); equal to the address rule while `(base >> 8) & 3 = 0` (Novalis: 0x1e9a4 & 3 = 0; other levels assumed) | Novalis array base from `trace_results_novalis.md` §f; not trace-checked per moby |
| Blarg flyers' (660) per-segment path length, which sets their first-guess step along each spline segment (5 % too fast or too slow otherwise until the arc-length correction) | `FUN_0028bb90` samples the Hermite segment with `t += 0.05` while `t ≤ 1.0`: the PS2's truncating adds reach t = 0.99999946 on the 20th sample and take it; IEEE round-to-nearest reaches 1.0000001 after 19 and drops the last chord | 20 samples at `t = 0.05·k`, k = 1..=20 (`rc-game/src/moby_update/classes/flyer.rs` `arc_lengths`) | `compare-novalis-spawn` §f2: all 10 flyer splines (count, z, per-point arc lengths) within 2.7e-4 of RAM (2026-09-27, `SCUS-97199 (CE4933D0).01.p2s`) |
| Opaque world surfaces stay solid: no see-through or flickering walls, rocks, door frames, trees (the frame buffer's alpha is never shown) | The GS writes As into the frame buffer's alpha and the CRTC never displays it. On ties in the LOD morph range VU1 program 13507 blends all four lanes of a fat vertex, alpha included, as `avg·w + c0·z` with truncating FMACs, so an 0x80 slot alpha comes out 0x80 or 0x7f with the last bits of the morph weights (w + z = 256 or 256 + 2⁻¹⁷); k changes every frame while the camera moves or settles, so it toggles. On the GS an 0x7f As on an opaque draw is a 1/128 blend with the pixel under it (≤ 1 level) | the frame alpha is ignored at display: the world camera writes its finished main texture (sky + world) to the target with `BlendState::REPLACE` (`rc-engine/src/sky_render.rs` `main_camera_loads`). Bevy's default composites a second camera on the same target with alpha blending over the first one's earlier output (the sky pass alone), so every world pixel with alpha < 1 let that stale image through: the see-through flicker of 2026-09-27. The fat alpha lane stays VU-exact (`tie.wgsl` `vu_fat_lane`); the opaque pass does not blend the 1/128 | fixed camera, level-start door, tie 461 k stepped by 1e-5: 11,498 px change by ≥ 6 levels before, 2 after (the ±1 colour-lane step remains, as on the VU). Patch-flicker blocks (8×8, ≥ 50 % A-B-A): level-start door settle 4,103 → 0 (1 tick/frame) and 10,254 → 118 (`RC_DUMP_TICKS=1,0,2,1,1,1,0,2`); jungle door pan 7,066 → 1,929 (the rest is texture shimmer in motion). User A/B in play: `RC_TIE_LOD=0` removed it, pinning k on one tie removed it on that tie (2026-09-28) |
| Caves, pipes and rooms ring with the level's reverb: a studio-sized tail (about 0.9 s for studio B, 2.2 s for studio C, a metallic ring in pipes) that is darker than the dry sound, at the zone's depth | libsd's SPU2 reverb: a work area in sound RAM run by the SPU's fixed reverb engine at half rate (24 kHz) with the preset's register values (same / cross reflection loops with a wall gain and an IIR, four comb taps, two all-passes); the effect volume EVOL = the game's depth | a native Moorer-style stereo reverb on `f32` (four damped feedback combs per channel into two all-passes, input low-passed at ≈ 11 kHz for the half-rate band limit), one character per libsd type: loop times, decay time, damping and diffusion derived once from libsd v3.03's preset table as results; wet = `depth / 0x8000` × the reverb of the voices flagged "to reverb" (`rc-game/src/audio/reverb.rs` `ReverbFx`, `preset`) | unit tests `audio::reverb::tests` (each preset decays by 20–50 dB over half its RT60; the glide; the echo delay); `tests/ui/reverb_conformance.rs` (a wet signal inside a box on 13 levels, none away from the zones); Novalis cave WAVs (audio.md "Reverb zones and level sounds"). Not PCSX2-compared by ear |

### Tolerances

Accepted differences from PCSX2 comparisons (kind 2 in the triage). One row per compared quantity. The first
comparison and the evidence behind each row are in `docs/plan/trace_results_novalis.md` (class H); the savestate is
`SCUS-97199 (CE4933D0).01.p2s`, Novalis spawn after the arrival scene (kept as `~/PS2/ratchet1/savestates/novalis_spawn.p2s`; the regression test
reads its distilled facts, `tools/trace/tests/fixtures/novalis_spawn.tsv`).

| Item (quantity compared, command) | Tolerance | Why acceptable (not noticeable in play) | Measured (date, savestate) |
|---|---|---|---|
| Hero rotation `rot.x`, `rot.y` (hero block euler), `compare-novalis-spawn` §b | ±0 equal: compare with float `==` (−0.0 == +0.0) | sign of zero only; same angle, same matrix | 2026-09-27, `SCUS-97199 (CE4933D0).01.p2s` Novalis spawn |
| Hero rotation-matrix entries built from cos(yaw) (`rows[0].x`, `rows[1].y`), `compare-novalis-spawn` §b | 2 ULP (or 1e-6 absolute) | last bit of the trig polynomial (RAM −0.40455654, port −0.40455657); orientation error ~1e-7 rad | 2026-09-27, `SCUS-97199 (CE4933D0).01.p2s` Novalis spawn |
| Hero height above the ground probe hit (0x13f62c), `compare-novalis-spawn` §b | 1e-5 units | 3.8e-6 u, a few ULP at the probe hit distance; position and ground z are bit-equal | 2026-09-27, `SCUS-97199 (CE4933D0).01.p2s` Novalis spawn |
| Hero ground slope / pitch / roll angles, `compare-novalis-spawn` §b | 1e-6 rad | atan2/asin residuals on a flat normal (z = 0.9999999): 2.38e-7 vs 1.79e-7 rad; both "flat" to every consumer | 2026-09-27, `SCUS-97199 (CE4933D0).01.p2s` Novalis spawn |
| Follow camera position and orientation (0x167240 pos, 0x167250 euler, rows 0x167450..), `compare-novalis-spawn` §c | 3e-4 units position; 6e-5 rad angles / row entries | measured 2.5e-4 u and 5.4e-5 rad: sub-pixel. The port sim now freezes the camera during mode 2 (1 + 783 updates, as the game; trace_results_novalis.md "Open reads resolved" e): unchanged to the last digit, so the update count is not the cause. Tightened from 1e-3 u / 1e-4 rad (2026-09-27). **Still provisional** pending the idle-fidget port (countdown / re-arm 0x241e00): re-check then and tighten or drop | 2026-09-27, `SCUS-97199 (CE4933D0).01.p2s` Novalis spawn |
| Crate z (class 500/501/502/505/511 moby +0x18 after the ground snap / stacking), `compare-novalis-spawn` §f | 2 ULP (≈2e-5 at z ≈ 75) | 23 crates differ by 1 ULP (75.51146 vs 75.51147); rotation and stacking equal | 2026-09-27, `SCUS-97199 (CE4933D0).01.p2s` Novalis spawn |
| Fire-field (760) element scroll and the 809 curtain scroll `0x161374` after 1072 ticks, the port run from the first savestate's RAM element state (scratch harness; `docs/plan/trace_results_novalis.md` "World props and effects") | 2e-3 (element scroll), 1e-3 (curtain scroll) | an accumulator of ~0.01 steps: last-bit rounding of the PS2 FPU vs IEEE over 1072 adds (measured 1.4e-3 / 3.6e-4); the texture offset error is far below a texel. Timers, positions and respawn counts are exact | 2026-09-27, `novalis_spawn` → `novalis_idle` |
| Ratchet's posed shadow list (0x1acc00: 21 records, points and radii), `crates/rc-game/tests/world/shadow_volume_novalis.rs` (distilled fixture) | 1e-4 units on points, 1e-6 on radii, all 21 records | measured 3.05e-5 units / 1.5e-8: float order of the joint chain and the two matrix products (the 10 records under the runtime joint-modifier list +0x64, applied natively since 2026-09-28: 1.53e-5) | 2026-09-27 / 2026-09-28, `novalis_idle` |
| Shadow directions 0 / 1 (0x1af000 / 0x1af010) and Ratchet's slab (+0x84 / +0x88), same test | 2e-6 (directions), 1e-5 units (slab) | `std` sin / cos / atan2 for the VU0 polynomials and `FastArcTan` | 2026-09-27, `novalis_idle` |
| Tie vertex palette lit colours (RGBA per palette entry), `compare-tie-shrub-light` (also `compare-novalis-spawn` §g) | ±1 per channel | 96511/96512 entries equal; tie 435 slot 14 r and b −1 (one shared scalar 1 ULP off with two channels on a byte boundary); a 1/255 step is invisible | 2026-09-27, `SCUS-97199 (CE4933D0).01.p2s` Novalis spawn |

### Open decisions

Layers whose fate the user has deferred. One row each.

| Layer (where) | Status | Options |
|---|---|---|
| Additive particles blended in display bytes (flyer trails, sparkles, TNT sparks; `particle_render.rs` + `particle.wgsl`) | **Decided 2026-09-27: display-space blending for additive/glow effects** (user: "Let's go with the PS2-style blending glow on effects") | Implemented generally for every effect (particles, effect mobys, draw-callback effects) as one mechanism, recorded under "Result-level reproductions" (`rc-engine/src/display_blend.rs`) |

## Summary

| # | Item | Cat | Where (main) | Native alternative | Size |
|---|---|---|---|---|---|
| A1 | PS2 float model (FMAC/COP1: truncation, no denormals/NaN/Inf, adder guard bits) | A | `rc_formats::tfrag_light::ps2`, `rc_game::ps2v` (`Pf`) + ~40 modules | IEEE `f32` (or `f64`) | L |
| A2 | VU/GS integer semantics inside WGSL (ftoi0, s16 wrap, 1/128 grid, VU FMAC in `tie.wgsl`) | A | `moby.wgsl`, `moby_metal.wgsl`, `tie.wgsl`, `tfrag.wgsl`, `shrub*.wgsl` | plain float shading | M |
| A3 | GS colour units: raw display-encoded texels, 0x80 = 1.0, MODULATE `>>7`, no tonemapping | A | every world shader; `main.rs` camera | sRGB textures, linear shading, 1.0 = white | M |
| A4 | GS pixel-test/blend table: AREF + RGB_ONLY two-draw split, GEQUAL, ALPHA_1 | A | `rc-engine/src/gs_state.rs` | Bevy `AlphaMode::Mask/Blend` | M |
| A5 | Integer display-byte blends for full-screen overlays | A | `underwater_tint.wgsl`, `fade.wgsl`, `menu_snapshot.wgsl` | ordinary alpha-blended quad | S |
| A6 | GS fog byte, screen-linear colour/fog, affine sky ST (Q = 1), flat sprites | A | `tfrag.wgsl`, `sky.wgsl`, `particle.wgsl`, `game_camera.rs` (`TfragFog`) | perspective-correct interpolation, standard fog | S |
| A7 | GS mip rule `round(log2(z/32) + K)`, nearest mip filter | A | `tfrag/tie/shrub/shrub_billboard.wgsl`, `*_render.rs` samplers | hardware derivative LOD + trilinear | S |
| A8 | HUD 2D pass: 512×416 offscreen, manual bilinear on raw GS bytes, 12.4 UV | A | `hud_render.rs`, `hud.wgsl`, `text_render.rs` | Bevy UI/sprites at window resolution | M |
| A9 | Software SPU2: ADPCM voices, 4-tap Gaussian, ADSR state machine, 48 voices, mix clamp | A | `rc-game/src/audio.rs` (`Spu`, `SpuVoice`, `Adsr`, `GAUSS`) | per-sound Bevy audio with pre-decoded PCM | L |
| B1 | newlib `rand` and one shared stream in game order | B | `rc-game/src/rng.rs`; `IopRng` in `audio/grain_vm.rs` | any PRNG, independent streams | S/M |
| B2 | libpad2 18-byte pad block as the only input path | B | `rc-engine/src/input_map.rs` → `rc-game/src/pad.rs` | Bevy `ButtonInput`/`Gamepad` into the hero directly | S |
| B3 | 989snd: grain VM, LFOs, `MakeVolume`, IRX pan table, voice manager, stream player | B | `rc-game/src/audio/{grain_vm,voices,music}.rs` | Bevy audio entities with volume/pan/pitch | L |
| B4 | libsd `sceSdNote2Pitch` / `PS1Note2Pitch` tables | B | `rc-formats/src/vag.rs` | `2^(semitones/12)` playback-rate factor | S |
| B5 | Game/SDK trig: VU0 28259 sine, `sceVu0ECosSin`, FPU sine/asin, `FastArcTan`, boot normal table | B | `moby_light::vu0_sin_cos`, `scene_player.rs`, `services.rs`, `pad.rs`, `follow_camera.rs` | `f32::sin/cos/atan2/asin` | M |
| D1 | 60 Hz tick, RCNT1 catch-up (at most 1 extra), deterministic mode | D | `rc-game/src/tick.rs`, `rc-engine/src/{gameplay,determinism}.rs` | variable-dt or Bevy fixed timestep with unlimited catch-up | M |
| D2 | Audio clock tied to ticks: 800 samples/tick, 240 Hz IOP tick, one-frame RPC latency | D | `audio.rs` (`AudioSystem::tick`), `rc-engine/src/audio_out.rs` | audio device clock, events fired immediately | M |
| D3 | 512×416 draw buffer: GS projection, letterbox, Z = depth/2^24 mapping | D | `rc-engine/src/game_camera.rs` (`GameProjection`, `letterbox`) | Bevy perspective at window aspect | M |
| D4 | Frame-buffer pixel conventions: 12.4 corners, sprite offsets in 512×416 px, ×1.0625 y | D | `hud_render.rs`, `particle_render.rs`, `sky_stars.rs`, `particle/sky_stars.wgsl` | world-space billboards, resolution-independent UI | S |
| D5 | Menu frame snapshot (`DownloadFrameBuffer`) with GS darken | D | `menu_render.rs`, `menu_snapshot.wgsl` | live world behind a blurred/dimmed UI layer | M |
| D6 | Game-order frame lags (fog, previous-frame camera, sky clear rule, scene vsync frames) | D | `fog_state.rs`, `particle_render.rs`, `sky_render.rs`, `scene_player.rs` | same-frame values, per-frame clear | S |
| C | Disc/format decoding (ISO, TOC, WAD, VIF, CLUT, gs_ram, ADPCM, banks, saves) | C | `rc-formats` | none needed: required to read the assets | – |

Counts: A = 9, B = 5, D = 6, C = 1 grouped entry (about 12 formats, listed below). Game quirks: 9.

## A. Hardware-like models

**A1 PS2 float model.** *What:* EE COP1 and VU FMAC arithmetic on raw `u32` bit patterns: round toward zero, exponent 0 = zero, exponent 255 is a
finite number, overflow clamps to ±0x7fffffff, the adder pre-truncates the smaller operand, `div`/`sqrt`/`rsqrt` truncate. The rare last-bit
deviations of the multiplier are not modelled. *Where:* `crates/rc-formats/src/tfrag_light.rs` `pub mod ps2` (`mul`, `add`, `div`, `sqrt`, `itof12`);
`crates/rc-game/src/ps2v.rs` (VU0 macro helpers `vmadds`, `cross`, `dot_x/dot_y`, `ftoi0`, `itof0`; `Pf` with FPU operators and `c.lt.s`-style
compares). Users: lighting (`tfrag_light`, `tie_light`, `shrub_light`, `moby_light`), `moby_anim` (`advance` 0x20d580, `evaluate` 0x20e0e0),
`moby_spawn`, `collision_query` (`CollLine_Fix` 0x211870, sphere 0x212960, capsule 0x2135a0), `hero*`, `follow_camera` (0x314e00), `pad`, `fog_zones`
(0x26bbc0, `fun_001ee4b0`), `water` (ripple sim), `particles` + `type06`, `sky_stars`, `scene_player`, `menus/{quick_select,pause/frame}`,
`moby_update/*`, `audio/voices` (distance/pan), `rng` helpers. Engine: `tie_lod.rs` (TieProc k and colour weights), `water_render.rs`,
`play_camera.rs`. *Why:* colours are 1/128-grid floats (`0x47800000 + c`) and positions go through `ftoi0`, so one last bit changes an output byte.
Gameplay compares and timers drift too. Concrete case: the ripple step period is 9 ticks under truncation and 8 under IEEE (`rc-game/src/water.rs`
doc). Collision hits and hero trajectories would diverge over a few hundred ticks, which would make any PCSX2 trace diff fail. *Native:* `f32`
arithmetic in the same op order (or `f64` with the order ignored). `Pf` wraps every value, so it can become an `f32` newtype with the same operators
in one file. The `tfrag_light::ps2` users are raw `u32` code and need rewriting. *Cost:* the golden values in unit tests (`ps2v`, `tfrag_light`,
`pad`, `water`, `follow_camera`, `hero/*`, `rng`) and `rc-game/tests/{hero_novalis,moby_update_novalis,novalis_collision}.rs` would have to become
tolerances. The `moby_anim_golden` numbers and the `rc-trace` tfrag-light truth test would stop being exact. Lighting bytes shift by ±1 on some
vertices, and gameplay trajectories drift slowly. The change is mechanical but touches about 45 files. **L.**

**A2 VU/GS integer semantics in WGSL.** *What:* the shaders reproduce the VU1/VU0 conversions: `ftoi0` truncation, the `ppach` s16 wrap of skinned
positions, the `⌊128·Σ⌋` 1/128-grid colour sum and s16 saturation `>>7` (`moby.wgsl` VU0 104691), `⌊32·C·d⌋` metal colour and `ftoi12 >> 1` sphere-map
ST (`moby_metal.wgsl` 0x111/0x121/0x12d), the VU FMAC `vu_mul`/`vu_add` of the fat-vertex colour blend (`tie.wgsl`, VU1 13507, CPU twin
`tie_lod::vu_fat_color`), `ftoi0` of the morphed colour (`tfrag.wgsl` L17/L18), and fixed-point fades such as `min(trunc((D−z)·4096), 0x8000) >> 8`
(`shrub.wgsl`, `shrub_billboard.wgsl`). *Why:* vertex bytes equal the VU output. The GPU moby path matches the bit-exact CPU pass on all 315,913
Novalis vertices (bind pose, `RC_MOBY_LIGHT_CHECK=1`, `rc-engine/src/moby_light.rs`). Without it, colours shift by 1–2 LSB and fades step differently.
*Native:* float lighting and blends (`mix`, `smoothstep`) with no truncation. *Cost:* drop the `RC_MOBY_LIGHT_CHECK` equality and the tie colour
identity. Each shader can change on its own. The morph and fade distances stay the same, only the rounding changes. **M.**

**A3 GS colour units and display-byte space.** *What:* textures are `Rgba8Unorm` holding the raw GS bytes (display encoded, texel alpha 0..0x80 as
byte/255). Vertex colours are `rgba/128` (0x80 = 1.0). MODULATE is `min(⌊Ct·Cf/128⌋, 255)` in display space, and each shader then applies
`srgb_to_linear` so that Bevy's sRGB target stores exactly the GS byte. `Tonemapping::None` + `DebandDither::Disabled` on every camera (`main.rs`,
`sky_render.rs`, `hud_render.rs`, `menu_render.rs`). *Where:* all world shaders (`tfrag`, `tie`, `shrub`, `shrub_billboard`, `moby`, `moby_metal`,
`sky`, `sky_stars`, `particle`, `water`). *Why:* the stored pixel equals the PS2 frame buffer byte wherever blending is not involved, and the 2×
overbright range of 0x80 = 1.0 is kept. *Native:* `Rgba8UnormSrgb` textures, linear-light shading with 1.0 = white (vertex colour ×2 folded into the
data), and optionally tonemapping. *Cost:* every screenshot comparison changes slightly in mid-tones. Pixels above 1.0 would then need HDR or a clamp.
The change touches every material, but each one is a local edit. **M.**

**A4 GS pixel-test/blend pass table.** *What:* per-pass TEST_1/ALPHA_1 from the game (tfrag/tie/moby 0x5360b AREF 0x60, shrub lists 0x5320b / 0x530cb,
billboard 0x53001, sky 0x30000 / 0x3180b, HUD 0x5380b; ALPHA_1 0x8000000044). With AFAIL = RGB_ONLY each batch is drawn twice: A = As ≥ AREF, blended
**with** Z write; B = As < AREF, blended without Z. The depth test is GEQUAL on reverse Z (`GsPass`, `draws`, `specialize`, `GS_ATEST_*` shader defs).
Particles pick ALPHA 0x44/0x48 per record (`particle_render.rs`), and water bakes FIX into the alpha (`water.wgsl`). *Why:* cut-out foliage and fading
mobys occlude exactly like the GS: semi-transparent texels still write colour but not depth. *Native:* one `AlphaMode::Mask(0.75)` (0x60/0x80) or
`Blend` draw per material. Everything else in this row is a pipeline-state choice. *Cost:* fringes around grass/foliage cards change, and objects
behind low-alpha texels sort differently. `gs_state.rs` tests go. `RC_GS_ALPHA=0` already shows the simpler mapping. **M** (it is a table, but every
renderer references it).

**A5 Integer display-byte blends for overlays.** *What:* full-screen passes read the pixel, re-encode it to the GS byte, compute `((Cs − Cd)·As >> 7)
+ Cd` in integers and write the result back: underwater tint (`DrawDebugProfiler` 0x21a1b8, `fog_state.rs` `UnderwaterTint`), mode-2 fade
(`fade.wgsl`, `scene_render.rs`), menu darken 0x30 (`menu_snapshot.wgsl`). *Why:* these overlays cover the whole screen, so the difference between
byte and linear blending would show everywhere. *Native:* a normal alpha-blended quad (linear light). *Cost:* tint and fade become slightly lighter or
darker in mid-tones. The change is contained in three shaders. **S.** (The world passes already blend in linear light. Exact byte blending for them was
queued in orchestration.md §4.2 and is on hold: under the Phase 1 policy it would be a new layer.)

**A6 GS fog, interpolation, affine sky.** *What:* the fog value is the VU F lane `trunc(clamp(depth·slope + off, far, near))` as a 0..255 byte
(`game_camera.rs` `TfragFog`/`vu_constants`, per-instance F for ties/shrubs), blended RGB-only as `FOGCOL + (C − FOGCOL)·F/255`. Colour and fog use
`@interpolate(linear)` (screen-linear, as the GS interpolates), only ST is perspective-correct. Sky ST uses Q = 1 (fully affine, `sky.wgsl`). Sprites
and the HUD are flat (IIP 0). *Why:* the gradients across large triangles and the texture swim on sky shells match the PS2. *Native:*
perspective-correct varyings and Bevy's `DistanceFog`. *Cost:* colour/fog gradients on large tfrag polygons and sky texture placement shift. Each
change is one attribute qualifier or line. **S.**

**A7 GS mip rule.** *What:* level = `clamp(⌊log2(z/32) + K + 0.5⌋, 0, MXL)` from the per-pixel camera depth and the ad-gif TEX1 K, with
`mipmap_filter: Nearest`. Mips 2/3 come from `gs_ram`. *Where:* `tfrag.wgsl`, `tie.wgsl`, `shrub.wgsl`, `shrub_billboard.wgsl`; samplers in
`tfrag_render.rs`, `tie_render.rs`, `shrub_render.rs`, `shrub_billboard.rs`, `sky_render.rs`. *Why:* texture sharpness steps at the game's distances
regardless of window resolution or angle. *Native:* `textureSample` with derivative LOD, trilinear or anisotropic. *Cost:* textures get sharper at 2×
resolution and blurrier at grazing angles. The shader edit is small. **S.**

**A8 HUD 2D pass.** *What:* HUD sprites, glyphs and rectangles render in submission order into a 512×416 `Rgba16Float` target. Texturing is a
hand-written bilinear filter on an atlas of raw GS bytes: UV quantised to 1/16 texel (12.4), weights k/16, result truncated, clamped to each
sub-texture. Then MODULATE `>>7`, blending in display bytes premultiplied, and a **nearest** 2× composite in the UI pass (`hud_render.rs`, `hud.wgsl`;
text from `text_render.rs`, `FontPrint` 0x21ccf0). *Why:* glyph edges and orb glows are pixel-identical to the GS at 512×416. *Native:* Bevy
UI/sprites with the GPU sampler at window resolution (sharper text), or a 2× scale of the same layout. *Cost:* HUD screenshots stop matching. The
layout and state machines (`rc_game::hud`) stay as they are. **M.**

**A9 Software SPU2.** *What:* 48 voices that decode SPU ADPCM on the fly (`SpuVoice::key_on`), a pitch counter `+= min(pitch, 0x3fff)` with 12
fraction bits, the 512-entry Gaussian 4-tap interpolation (`GAUSS`, `gauss_interpolate`), the ADSR state machine (`Adsr`, psx-spx/PCSX2 step rule),
per-voice VOLL/VOLR `>>15`, master 0x3fff and one final 16-bit clamp (a single clamp is inferred; the hardware clamps per core), output at 48 kHz
(`rc-game/src/audio.rs` `Spu::mix`). Noise and PMON are not modelled; the reverb is a native effect, not the SPU's engine ("Result-level reproductions"). *Why:* timbre, the 44.1→48 kHz resampling (music streams run at
`rate_to_pitch`), envelope shapes and clipping match the console. *Native:* decode each sample once to PCM (already possible via `vag::decode`) and
play it through Bevy audio or a mixer crate with its own resampler, using ADSR as a simple gain curve or dropping it. *Cost:* the `RC_AUDIO_WAV`
output stops being bit-reproducible, and the audio unit tests in `audio.rs` go. Voice-count limits and the Gaussian softening disappear. Contained in
`rc-game/src/audio*` + `audio_out.rs`. **L** (together with B3).

## B. SDK / library reproductions

**B1 newlib `rand` and the shared stream.** *What:* `s = s·0x41c64e6d + 0x3039`, `rand = s & 0x7fffffff` (boot 0x1160d8), `srand(1234)` at level init
(0x255958), and helpers `randi`/`randf`/`rand_angle`/`rand_vec` on the PS2 float model (`rc-game/src/rng.rs`). One stream is consumed in game order:
load pass → per tick mobys (slot order) → hero → particles → camera → counter → render (sky stars). `audio/grain_vm.rs` `IopRng` is the same LCG
seeded 1 for 989snd (inferred, not reversed). *Why:* particle bursts, crate debris, bolt scatter, ripple drops and star fields match the PS2 only if
every earlier draw happened in the same order. *Native:* any PRNG (`rand`/`fastrand`), one stream per system. *Cost:* the `moby_update_novalis` and
particle tests need new expectations. Randomness stops matching traces, although the game logic is unchanged. Swapping the generator is **S**. Also
dropping the ordering discipline is **M**, since it frees the engine's system ordering.

**B2 libpad2 pad bytes.** *What:* keyboard, mouse and gamepad are encoded into the 18 bytes `sceScfPad2Read` returns (active-low buttons, 4 axis bytes
with 127/128 centre, 12 pressures; `input_map.rs` `sample`, `raw_axis`, `Script`). `pad.rs` then decodes them exactly as `UpdatePad` 0x27bb30 /
`ProcessPadInput` 0x27bd90 do: dead zone `|b − 127| < 48`, scale `/76`, `vsqrt` length, `FastArcTan` angle, 30-entry history, flick. The dead zone and
scale are **game code**, not libpad2. Only the byte format is SDK. *Why:* stick response, walk/run thresholds and flicks match a DualShock exactly,
and `RC_PLAY_SCRIPT` replays are pad-exact. *Native:* feed Bevy axis values into `PadState` as floats (skip the byte round trip). Optionally drop the
48/76 dead zone in favour of Bevy's gamepad settings. *Cost:* the analog stick gets the full 8-bit precision back, which the byte format quantised.
The `pad.rs` byte tests need adapting. `PadInput::axis_byte` exists already. Contained. **S.**

**B3 989snd.** *What:* the IOP sound library as in OpenGOAL's 989snd port with RAC1's v1 banks: block-sound handlers, the grain VM at 240 Hz (grains
1, 4, 20–44), LFOs (`trunc(32767·cos)` table), `MakeVolume`, `AdjustVolToGroup` (squared), the pan table read as the IRX has it (entry 1 = (0x3ffe,
0xb6)), the SPU voice allocator with group ranges and priority/age stealing (an *unverified proposal*, docs/plan/audio.md), the stream player (a Start
part queued into a Loop part, `MakeVolume(127, …) >> 1`), and the EE callbacks `PAN_RESET`/`PAN_DONT_CHANGE`. *Where:* `rc-game/src/audio/grain_vm.rs`
(`Snd989`), `audio/voices.rs` (`VoiceManager`, `pan_table`, `make_volume`), `audio/music.rs` (IOP side). *Why:* ambience scripts (random delays, pitch
bends, markers), volume curves and voice limits sound like the console. *Native:* keep the EE side (`sound_update` 0x2a0638 slots,
distance/pan/doppler, the music state machine: all game logic) and replace the IOP side with Bevy `AudioPlayer` entities. Grain scripts become a small
scheduler, and pan uses equal-power panning. *Cost:* the ambience still has to run its scripts (the grain VM is data-driven, so a native scheduler
would re-implement most of it). Volume curves change audibly. Tests in `grain_vm.rs`/`voices.rs`/`music.rs` go. **L.**

**B4 libsd note → pitch.** *What:* `sceSdNote2Pitch` from the 140-entry table (`trunc(0x8000·2^(k/12))`, `trunc(0x8000·2^(k/1536))`, checked against
the RAC1 libsd IRX) and 989snd's `PS1Note2Pitch` (`rc-formats/src/vag.rs` `note_to_pitch`, `ps1_note_to_pitch`, `rate_to_pitch`). *Why:* the pitch
words are integer-exact, which rules out small detuning. *Native:* `speed = 2^((note − centre)/12 + fine/1536)` as `f32`. *Cost:* a sub-cent pitch
difference, and `vag.rs` tests. Only matters if A9/B3 go native. **S.**

**B5 Game/SDK trig and matrix routines.** *What:* several distinct implementations coexist, each bit-reproduced: the VU0 28259 9th-order sine
polynomial with π folding (`rc_formats::moby_light::vu0_sin_cos`, used as `fast_sin`/`fast_cos` 0x2216f8/0x221710 by hero, rng, water, sky stars,
audio, spawn rotations); `sceVu0ECosSin` 0x125240 (cos polynomial + `√(1 − cos²)`) and `sceVu0RotMatrixX/Y/Z` in `scene_player.rs`; the FPU sine
`fun_001fa070` 0x2219a0 (`moby_update/services.rs` `fpu_sin`); `FastArcTan` 0x2217c0 with its octant table (`pad.rs` `fast_arctan`); the FPU `asin`
polynomial 0x221728 and quaternion rotation 0x274ac8 (`follow_camera.rs`); the boot-ELF `(cos, sin)` normal table used by lighting instead of `sin()`
(`tfrag_light::NormalTable`, 296 of 512 entries differ from IEEE). *Why:* camera angles, stick sectors, spawn matrices and lighting normals match the
PS2 to the last bit. *Native:* `f32::sin_cos`, `atan2`, `asin`, `glam` rotations. The normal table is disc data and can stay. *Cost:* same as A1
(depends on it). Last-bit angle drift feeds camera and hero trajectories. Each helper is one function, but they are called from many places. **M.**

## C. Format decoding (not emulation; required to read the assets)

Kept as is in any native plan: `iso9660.rs` (2048/2352-byte sectors), `disc.rs` + `toc.rs` (sector-addressed TOC in the ELF), `wad.rs` (Insomniac
LZ77), `vif.rs` (VIF1 unpack command lists for tfrags) and the tie GS-packet walk, `texture.rs` (8-bit indexed + CSM1 CLUT unswizzle, `gs_ram`
addressing for CLUTs, mips 2/3 and billboards), `tfrag_light::pext5` (5:5:5:1 colours), moby packed vertices/skin tables (`moby.rs`), `vag.rs` SPU
ADPCM decode (rounded PCSX2 variant plus the OpenGOAL variant for comparison), `sound_bank.rs` (989snd SBlk v1), `particle_tex.rs`,
`hud.rs`/`font.rs`/`strings.rs`, overlay/boot-ELF table readers (`water.rs`, pause menus, star tables, glyph tables, `NormalTable::from_elf`),
`save_game.rs` (memory-card sections, CRC-16 0x8320/0x1f45). A native port could convert these once into modern assets (PNG/KTX2, glTF, OGG). The
decoders themselves stay.

**Movies (PSS, 2026-09-27).** `pss.rs` (program-stream demux, `SShd` SPU-ADPCM audio through `vag.rs`) and `crates/rc-video` (MPEG-2 video,
IEEE 1180 float IDCT, BT.601 colour) replace the IPU / sceMpeg / 989snd movie path with standard decoding: not hardware modelling (no IPU
fixed-point CSC constants, no SPU Gaussian filter on the movie voices, 44.1 kHz streams resampled linearly). Timing is the game's (one
picture per field pair at the 60 Hz tick rate, audio-clocked; D1's 60 vs 59.94 Hz applies as everywhere). docs/formats/pss.md.

## D. Timing and resolution conventions

**D1 60 Hz tick and catch-up.** *What:* game logic runs in `FixedUpdate` at exactly 60 Hz (`TICK_HZ = 60.0`, not NTSC's 59.94). Per rendered frame,
the main loop's rule (`entry` 0x259c40) allows **at most one** extra tick when RCNT1 counted more than 0x2580 (one field). The port converts
wall-clock delta to RCNT1 counts (`gameplay.rs` `set_budget`, `tick.rs` `ticks_for_frame`). `GameTicks` = `floor(60·t)` otherwise. `RC_DETERMINISTIC`
/ `RC_SCREENSHOT_FRAME` force exactly one tick per update (`determinism.rs`). *Why:* the game is tuned per tick, and at 30 fps it runs two ticks per
frame like the PS2. Below 30 fps it slows down instead of skipping. Deterministic captures are a function of the frame number. *Native:* Bevy's fixed
timestep with unlimited catch-up, or variable dt with interpolation for 120+ Hz rendering. *Cost:* physics tuned per tick needs dt-scaling to run
above 60 Hz, which is a large gameplay change. Keeping the 60 Hz fixed step and only dropping the catch-up cap is **S**. Interpolated high-refresh
rendering is **M**. Deterministic mode must stay for tests.

**D2 Audio clock tied to game ticks.** *What:* each tick renders exactly 800 samples (48000/60) with the 989snd tick every 200 samples. EE→IOP
commands run at the start of the frame's samples and callbacks reach the EE a frame later (RPC round trip, `AudioSystem::tick`). `audio_out.rs`
streams these samples through a ring buffer (50 ms prebuffer, 0.25 s cap, silence on underrun). *Why:* the audio is a pure function of the tick
sequence (`RC_AUDIO_WAV` is reproducible), and the command latency matches the game's callback timing. *Native:* the device clock drives mixing and
sound events play immediately. *Cost:* when the frame rate drops, audio decouples from game time (it no longer stutters), and the one-frame latency
vanishes, which changes the music state machine's callback order. The music code relies on it, so it would need review. **M.**

**D3 512×416 draw buffer.** *What:* `GameProjection` reproduces `InitViewContext`/`UpdateViewContext`: tan 0.63 horizontal, ×0.775 vertical (NTSC),
pixel centre 2048, near 32 / far 745472 integer units, GS Z = `(Z_SCALE·d + Z_OFFSET)` mapped to depth = Z/2^24 (reverse, GEQUAL). `letterbox` keeps
the 512:416 viewport (`game_camera.rs`). Moby metal adds the game's `1000·Q` Z bias (`moby_metal.wgsl`). The sky writes Z = 0. The 3D already renders
at window resolution. *Why:* FOV, framing and depth ties are identical to the PS2. *Native:* Bevy `PerspectiveProjection` at the window aspect
(widescreen = wider horizontal FOV), standard reverse-Z. *Cost:* culling rules that use the game frustum (`tfrag_lod`, `tie_lod`, `moby_lod`,
`particle_render`, `shrub` fades) would need the new frustum or a wider one. The HUD layout (512×416) would need anchoring. Z-fighting between
coplanar batches may change. **M.**

**D4 Frame-buffer pixel conventions.** *What:* 2D corners are sent as `X = x·16 + OFX − 8` (12.4 fixed point, landing on pixel `x − 0.5`;
`hud_render.rs`). Particle and star sprite corners are offsets in 512×416 pixels added in clip space (`x/256`, `−y/208`) with the particle VU1
program's y factor 1.0625 (`particle_render.rs`, `sky_stars.rs`, `particle.wgsl`, `sky_stars.wgsl`). Menu rectangles/lines use the packets' pixel −1
corner offset (`menu_render.rs`). *Why:* sprite sizes, aspect and HUD pixel alignment match at any window size. *Native:* world-space billboards with
a square aspect, and resolution-independent UI anchoring. *Cost:* sprites change aspect slightly (6 %) and HUD pixel snapping is lost. Small and
local. **S.**

**D5 Menu frame snapshot.** *What:* the pause menu does not render the live world. It shows a copy of the last gameplay frame (`DownloadFrameBuffer`
0x2b4c88), taken on the GPU at the end of the render graph with the black 0x30 darken applied in GS bytes. A `copy` pass re-presents the window frame
(`menu_render.rs` "Snapshot", `menu_snapshot.wgsl`). *Why:* the frozen, darkened backdrop is byte-identical, including the HUD baked into it.
*Native:* keep rendering the (paused) world under a dimmed/blurred UI layer. *Cost:* removes the most fragile render-graph code (offscreen redirect
for window targets). The backdrop would no longer contain the HUD unless the HUD is drawn too. **M.**

**D6 Game-order frame lags.** *What:* the drawn fog lags the camera by one update (`UpdateFog` copies at the end of the render; `fog_state.rs`).
Particle emitters' `FastBSphereCheck` and the particle cull read the previous frame's camera (`particle_render.rs`). The sky clears only on the first
frame except on levels 05/07/10/13/14/15, and the gouraud dome otherwise acts as the clear (`sky_render.rs` `level_zeroes_clear_flag`). Stars generate
in the render after the tick. Scene fades spell out the game's blocking vsync waits as frames (`scene_player.rs`). *Why:* first-frame and transition
behaviour, plus RNG order (B1), match the PS2. *Native:* same-frame camera, a clear every frame, and fades as timed tweens. *Cost:* one-frame
differences at cuts and zone boundaries, all visible only in frame-exact comparisons. **S.**

## Already native (for reference; nothing to undo)

GPU clipping replaces the VU1 guard-band and clip programs. Bevy phases replace GS packet order (documented in `gs_state.rs`). The 3D renders at
window resolution. The world blends in linear light. Several CPU replays run in `f32`, not the PS2 model: `tfrag_lod.rs` culls, `moby_lod.rs`,
`shrub_render.rs` rules, `particle_render.rs` pass 1, `moby_spawn.rs` ground probe, the sky shell rotation angle (`sky_render.rs`
`ShellRotation::theta` in `f64` + `Quat`), and the particle cull's `1/cos(atan t)` computed as `√(1 + t²)` (`rc-game/src/particles.rs`). The moby
lighting on the GPU is `f32` plus A2's grid emulation. The hero's ledges and wall jump (`rc-game/src/hero/ledge.rs`, package
P3: probes A / B / C, states 0x11 and 0x18..0x1c) compute in `f32` with `std` trig and store back into the `Pf` hero block; no
PS2 effect was noticeable enough to reproduce there (the probes' 0.03 / 0.07 edge steps and the collision kernels decide the
hang point, not the float model).
The hero's damage / death / stance states (`rc-game/src/hero/damage.rs`, `stance.rs`, package P2) use the ported `Pf`
primitives for the shared steps (clamps, approach, gravity) and standard `f32` for the new formulas: the knockback
vector, 0x77's tumble about the body point (`std` `atan2` for the game's `FastArcTan` in the Euler extraction
`0x2721f0`); no PS2 effect there was noticeable enough to reproduce.
The effect mobys and particles (2026-09-28): translucent and additive mobys (moby+0x23 below 0x80, mode bit 0x200)
are drawn with native blend modes (alpha blend or additive, depth tested, no depth write, sorted; `moby_render.rs`
`MobyBlend`), not with the GS's per-moby TEST_1 / ALPHA_1 words: the result the game produces (a flash's pixels, all
below the Z-writing AREF 0x60, blend over the scene without occluding it) is reproduced, not the register split. The
moby additive mode added in linear light and only the particles' additive group blended in display bytes (the then
deferred layer). Since 2026-09-27 every effect (both groups of particles, the translucent and additive effect mobys,
the draw-callback effects) blends in display bytes through one mechanism (`display_blend.rs`; "Result-level
reproductions"), and the particles size and cull with the view's real field of view (scenes narrow it). The new particle types 2, 4, 8, 15, 52 (`rc-game/src/particles/type*.rs`), the point-light bank
(`rc-game/src/point_lights.rs`) and the moby point-light merge (`moby_render.rs` `point_light_merge`) are standard `f32`.
Type 2's packed velocities (8 bits per axis, `0x276380`) are the game's storage format, not a hardware effect, and are
kept (the blobs move in the game's quantised steps). No layer was added.
The effect particles of 2026-09-27 (types 16, 19 / 55, 22, 26, 35, 45 / 66, 46, 64), the splash class 775, the
nanotech glow and the renderer's lines / ribbons (kinds 2 / 3) are standard `f32` too (type 22's VU0 colour tween is
done in floats; the overlay tables are read through the functions' relocations); no layer was added.
The thrown wrench's rotation (`rc-game/src/hero/comet.rs` `turn_local` / `launch_euler`: `0x277380`, the throw
`0x236da0`) reuses the existing `Pf` rows ↔ Euler helpers (`services::euler_rows` / `mat4_mul` / `rows_euler`) and
adds no layer; no PS2 effect there is noticeable (the game's round trip through Euler each tick is kept).
The spline follower (`rc-game/src/spline.rs`: the level00 grind-path library 0x25d7a0 / 0x25d808 / 0x25df68 /
0x25da70 / 0x25dcd8, used by the hero's boots and the flow class 679) and the boots (`rc-game/src/hero/boots.rs`,
package P5: the grind, grind jumps, rail switch, grind wrench / hurt, the Magneboots and the cable) run in `f32` with
`std` sqrt / trig; the random draws are the game's, in its order. The gravity-mode-1 frame is reproduced at result
level: TurnTo in the hero's own frame (0x2323d8) and the floor alignment (0x236358 / 0x236098, which the game builds
from two Euler rotations about the cross axis) are one axis-angle (Rodrigues) rotation of the rows by the game's spring
step, converted back with the ported `MatrixToEuler` (`services::rows_euler`). No PS2 effect there was noticeable
enough to reproduce (the rails' w words are the exact chords on every level, `tests/hero/hero_boots_grind.rs`).
The Swingshot (`rc-game/src/hero/swingshot.rs`, package P6: the target searches, 0x24..0x26, 0x2c, 0x2d, the hand
item's hook update; `moby_update/classes/swing_target.rs`, classes 758 / 803) runs in `f32` with `std` sqrt / trig /
`atan2` (for `FastArcTan` / `fast_sin` / `fast_cos`), the ported `turn_spring` (0x270b58) for the body lean, and the
game's draw order (the target glint's `randi(0xff)`). The swing's body lean rotates the target direction into the
hero's yaw frame with a plain 2-D rotation (the game builds `EulerToMatrix(0, 0, yaw)`, transposes it and multiplies:
the same numbers up to rounding). The rope (`0x2dba30`) is a Bevy mesh rebuilt each tick from the same quads
(`rc-engine/src/moby_attach.rs` `rope_quads`) with a `StandardMaterial` (unlit, alpha-blended, the effect texture's GS
alpha doubled), not a GS packet. No PS2 effect was noticeable enough to reproduce (the swing's period matches
`π·√(L / g)` in `tests/hero/hero_swingshot_levels.rs`).

## Suggested order if going native (cheapest, least visible first)

1. **B4, B1 (generator only), D4, A7.** These are pure local swaps with little visible change. Update the tests.
2. **B2.** Feed floats into `PadState`, then decide whether to keep the game's 48/76 dead zone (it is game
   feel, not SDK).
3. **A6, A5.** These are single-shader edits. Check fog and tint by eye.
4. **A3 + A2 together.** Move to sRGB textures and linear shading, and drop the grid truncation. Moby/tie colours
   shift by LSBs.
5. **A4.** Collapse the two-draw split into Mask/Blend per material. Watch the foliage edges.
6. **D6, D5, A8.** Presentation: live paused world, a native HUD at window resolution.
7. **A9 + B3 + D2.** Replace the SPU2/989snd back end with Bevy audio. Keep the EE `sound_update` and music logic.
8. **A1 + B5.** Switch `Pf`/`ps2v` to an `f32` newtype, then the raw-`u32` lighting/animation code. Do this
   last: it ends bit-exact trace comparison (`rc-trace`) for everything downstream, so finish the PCSX2
   verification first (decisions.md "Native-first fidelity policy (2026-09-27)", Phase 2).
9. **D3, D1.** Widescreen and high-refresh rendering only after everything above, since culling and gameplay
   tuning depend on them.

The weapons and first person (`rc-game/src/hero/{comet,weapons}.rs`, the first-person camera and the switch blend in
`follow_camera.rs`, `moby_update/classes/bomb.rs`) are native `f32` with `std` trig (quaternion slerp for the blend,
Rodrigues rotations for the first-person view); only the state code of 0x15 / 0x23 keeps the hero block's `Pf` at its
boundary, as the other hero states do. The one reproduced PS2 effect is the loop exit's rate (the row above).

The hero polish (`rc-game/src/hero/fx.rs`, the camera shake in `follow_camera.rs`, particle types 25 / 34 / 47 / 60
in `rc-game/src/particles/`) is native: the new particle updates run in `f32` with `std` trig (the existing types 6 /
11 / 13 / 53 keep the PS2 model; type 53's record fill is shared, unchanged); the camera shake keeps the camera's `Pf`
steps (it lives inside the `Pf` follow camera: `fast_cos`, `FastNormalizeAngle`, `FastVecNormalize`). No PS2 effect
there was noticeable enough to reproduce.

## Reproduced game quirks (game-logic fidelity, not hardware)

- **Point-light list capacity.** `CreatePointLight` 0x252a28 lists at most 0x200 instances per light, ties first,
  then tfrags, then shrubs, and attaches with the radius at that moment plus 8 (re-attached only after moving 8
  units): the arrival crash's radius-60 lights fill the list with 512 ties, so the grass hill (tfrags) and the shrubs
  next to the explosion stay unlit, as in the original (`rc-game/src/point_lights.rs` `WorldLightLists`,
  tfrag_lighting.md §9).

- **Post-scale list re-append bug** (the "dropped-joint scale rule"). the walk keeps A records only up to the first
  joint also in B, then the B records after it, so the dropped joints get no post-scale (`rc-formats/src/moby_anim.rs` `post_scale_list`;
  moby_animation.md §6, 167 key pairs on the disc). Class joint counts larger than the frame's read the following
  bytes (`MobySequence` parsing), and the evaluator emulates the SPR joint records.
- **Particle allocator stale bytes.** `CreatePart` clears only the first 0x20 bytes of the 0x40 record
  (`rc-game/src/particles.rs` ~l.139, test `create_clears_only_the_first_half`). Type 6 never writes +0x3c..0x3f and
  stores a stack qw whose w is uninitialised (`particles/type06.rs`).
- **Shrub VU1 6-vertex colour-fetch quirk.** Vertex 3 of a 6-vertex packet reads its colour from address
  `n + 2·vi13`, i.e. from the other buffer's slot (`rc-formats/src/shrub_light.rs` `vu1_colour_address`,
  `vu1_shrub_data`; `shrub_render.rs`, modelled for batch slot 0 only).
- **Cumulative cable-grab jitter.** `0x2a7e20` (the cable grab's sparkle burst) writes each pair's `randf_sym(0, 0.15)`
  jitter into its argument, the hand point 0x13f930, so the four pairs drift from each other and the stored point moves
  (the next physics tick recomputes it) (`rc-game/src/hero/fx.rs` `sparkle_burst`).
- **Type-47 spin from stale bytes.** `PartType47Spawn` never writes +0x30, the spin its update adds to the rotation:
  a puff turns at whatever the record's previous occupant left there (`particles/type47.rs`).
- **C `%` on negatives.** The quick-select ring's `(sel + dir) % n` reaches entry −1 and reads the word before
  the table (`menus/quick_select.rs` `QsTables::entry_m1_icon`). `Rng::randi` uses MIPS `div` remainder semantics
  (`wrapping_rem`).
- **Audio stack-leftover velocity.** A sound slot without an owner uses the stack variable of the last listener
  step as its velocity, minus the listener velocity once per such slot (`audio/voices.rs` ~l.364). EE pans −1/−2
  arrive as `PAN_RESET`/`PAN_DONT_CHANGE`.
- **Sound remap out-of-range reads.** Def index −1 reads the u16 before the map, and the per-class copy walks
  past its list (`rc-formats/src/sound_bank.rs`; audio.md "In the port").
- **Moby shadow rules** (`rc-game/src/shadows.rs`, `shadows/volume.rs`; docs/plan/shadows.md): the fourth hero probe
  repeats the third (0x17c780); the circles step by 6.28/n, not 2π/n; the scene actors' second slab probe only reaches
  ~14 % of the way down (a horizontal and a 3-D normalisation mixed); a negative capsule segment count pushes that end
  **out**; the posed list's 8,064-byte budget (0x1acc00..0x1aeb80: a caster starting past it is dropped, the one before
  may run over); the capsule outline does not test B's disc containing A's (the `asin` gets |x| > 1, the outline
  twists). Not reproduced: the GS guard-band face drop (hardware).
- **Shimmy yaw extrapolation.** The ledge shimmy (0x1a / 0x1b physics) turns toward `y1 + (y1 − y2)/2` of the two
  probed wall yaws (`fast_subtract_rotations(y1, y2)·0.5` added to y1, as the instructions order it), not their
  midpoint; on a straight ledge y1 = y2 (`rc-game/src/hero/ledge.rs` `shimmy_physics`).
- **Hero damage rules** (P2, `rc-game/src/hero/damage.rs`): `HeroTakeDamage` takes `min(n, 1)` (a hit's damage
  above 1 still costs one point; a negative one gives health back); the hit intake applies its knockback even when
  SetState refuses the hurt state; a push away from the attacker keeps only the direction's xy (z = the fixed up
  speed); 0x77's death heights are per level (3: z < 5, 6: z < 50, 16: z < 77 or a capsule hit while rising).
- **Platform carry and surfaces** (P1, `rc-game/src/hero/{platform,surface}.rs`): the carry drops the attachment
  when a tick's correction jumps by more than 0.1 from last tick's applied platform step 0x13f490 (so a platform
  that starts at more than 0.1/tick lets go for one tick, then re-attaches), airborne decays the stored correction
  by `k·air_ticks` from the stored value (linear, the two `ticks(120)` calls are dead), `FUN_002753b0` adds
  `ClampLen(Δ, 1)` when Ratchet's class slot is below the carrier's (update-order patch), and with block flag
  bit 2 the local point creeps by the displacement every tick; in the ledge states the hang point is stored in
  0x13f680, which the game shares with the knockback magnitude / pitch / yaw (the port keeps it apart:
  `Carry::hang_local`, the knockback is never live while hanging). The slippery floor's capsule is top 0.8 /
  bottom 0.9 (the bottom above the top). Level 1's ground probe (the port's) re-cast the surface-0xd hit from
  the end point instead of the start point (a port bug, fixed: the re-cast starts 0.01 below the liquid's top).
- **Not reproduced: carry Euler round trips.** The game rebuilds the rider's yaw every carried tick through
  `MatrixToEuler(EulerToMatrix(rot)·M)` (VU sine, `FastArcTan`), which moves it by a few ULP; the port leaves the
  yaw untouched when the carrier does not rotate (every carrier on the disc) and composes in `f64` otherwise.
- **Swingshot rules** (P6, `rc-game/src/hero/swingshot.rs`): the pull search keeps the last target in 0x13fcb4 when
  nothing qualifies (only 0x13fcb8 drops), the swing search clears 0x13fce0; 0x25 re-issues `SetAnim(16, 10)` every tick
  of its last 22 (a blend restarted each tick, as the game does); the hooked flag 0x13fcec is only cleared by the next
  swing's entry; SetState(0x2c) writes the record's +0x14 (+0x10 or +0x0c) into the target's pvars, which nothing
  reads (not reproduced).
- **Low-LOD joint rule.** Low-LOD packets skin with only class[9] palette slots (slot 0 = identity when 0)
  (`rc-engine/src/moby_lod.rs`).
- **Not reproduced (documented only).** The fog-zone lookup's extra stale slot past the last circle
  (`fog_zones.rs` `lookup`), the TNT crate's stale stack bytes +0x18..+0x1b (0 here, `services.rs`), a stale save word
  (`game_state.rs` ~l.406), and the shrub sway phase base from heap-address low bits (still open).

