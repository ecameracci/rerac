# PCSX2 trace harness (`rc-trace`)

Purpose: ground truth for every computation we port. We read the PS2's EE memory from PCSX2
and compare it byte for byte with what our code computes from the same disc data. The first
target is the tfrag lighting pass (`LightTfrags`, docs/plan/tfrag_lighting.md): after a level
loads, every tfrag's RGBA block in EE RAM holds the game's lit colours.

Code: `tools/trace` (dev tool, never ships; workspace member, package `rc-trace`; README `tools/trace/README.md`,
workflow `docs/workflows/pcsx2.md`; depends on `rc-formats`, `rc-game`, `anyhow`, and the pure-Rust
`ruzstd` 0.8 and `miniz_oxide` 0.8 decoders. These are the same versions and a subset of the
features Bevy already uses, so the crate adds no lockfile packages and does not change the
engine's feature set). Nothing disc-derived is committed: dumps, reports and CSVs go under `work/trace/`
(git-ignored; `RC_WORK` overrides `work/`), kept savestates under `~/PS2/ratchet1/savestates/` and recordings under
`~/PS2/ratchet1/traces/` (the user's personal folder; `RC_PERSONAL` overrides it). The tool never writes into
`extracted/`. Findings reach tests only as distilled, numbers-only fixtures (`distill-spawn`, §6).

## 1. What exists on this machine (2026-09-26)

* PCSX2 **v2.8.2** (`/Applications/PCSX2.app`, Homebrew cask `pcsx2`), set up with the user's BIOS.
* Disc image: `~/PS2/ratchet1/Ratchet & Clank (USA) (En,Fr,De,Es,It).iso` (SCUS-97199).
* First savestate: `sstates/SCUS-97199 (CE4933D0).01.p2s` = Novalis after the arrival scene, at the spawn
  (kept as `~/PS2/ratchet1/savestates/novalis_spawn.p2s`; dumps in `work/trace/novalis_spawn*`). Results:
  docs/plan/trace_results_novalis.md.

## 2. PCSX2 v2.8.2 interfaces used (read from the v2.8.2 sources)

**Savestate `.p2s`** (`pcsx2/SaveState.cpp`). A ZIP archive written by libzip.
* `PCSX2 Savestate Version.id` is always stored uncompressed: `u32 save_version` (0x9A590000
  in v2.8.2; the high 16 bits must match for PCSX2 to load it), then `char build[32]`.
* Component entries: `eeMemory.bin` (EE main RAM, 32 MiB; 128 MiB with the "extended RAM"
  option), `iopMemory.bin`, `eeHwRegs.bin`, `iopHwRegs.bin`, `Scratchpad.bin` (16 KiB),
  `vu0Memory.bin`, `vu1Memory.bin`, `vu0MicroMem.bin`, `vu1MicroMem.bin`, `SPU2.bin`, `USB.bin`,
  `PAD.bin`, `GS.bin`, `Achievements.bin`, `PCSX2 Internal Structures.dat`, `Screenshot.png`.
* Compression is Zstandard (ZIP method 93) by default. Settings can switch it to Deflate (8)
  or none (0). The reader supports all three, ZIP64, and checks every entry's CRC-32.
* Location: `~/Library/Application Support/PCSX2/sstates/SCUS-97199 (<CRC>).<slot>.p2s`.
* Default hotkeys (`pcsx2/SIO/Pad/Pad.cpp`): **F1** saves to the current slot, **F3** loads it,
  F2 / Shift+F2 change the slot, Space pauses. On a Mac keyboard you may need Fn+F1. The
  menu route is System > Save State.

**PINE IPC** (`pcsx2/PINE.cpp`). Reads live EE memory, so no savestate is needed.
* Off by default. To enable it, turn on Tools > Show Advanced Settings, then tick
  Settings > Advanced > PINE Settings > Enable (slot 28011). Or set `EnablePINE = true` under
  `[EmuCore]` in `~/Library/Application Support/PCSX2/inis/PCSX2.ini`.
* Socket: `$TMPDIR/pcsx2.sock` on macOS. `.<slot>` is appended when the slot is not 28011.
* Framing: a message is `u32 len` followed by commands (`u8 op`, args). The reply is `u32 len`,
  `u8 status`, then the results. Requests must be under 650000 bytes and replies under 450000,
  so `rc-trace` sends 50000 `MsgRead64` per message. A full 32 MiB dump takes 84 round trips.
* It also supports `MsgSaveState` (`rc-trace pine-savestate N`), plus version, title, id and status.
* PINE reads while the game runs. For the tfrag test that is harmless: with no point lights,
  the per-frame relight writes the same bytes as the load-time pass. You can still pause first
  (Space).

## 3. Where the tfrag data sits in EE RAM (level01 = Novalis)

Each level is its own executable (`extracted/levels/NN/overlay.bin`) with its own data
layout. The addresses below come from the **level01** export (`work/decomp/level01.elf/`).
Boot-ELF addresses in other docs (e.g. `0x19bdc0`, `0x18cd00`) are *not* valid while a level
runs.

* `InitMemSlots__Fv` (0x252128) sets `DAT_00174294 = 2*DAT_001611cc + 0x320000 + 0x94000`. That
  global is the base of the decompressed level core data, and `DAT_00174284 = 0x320000` is the
  core-index copy. `DAT_001611cc` depends on the load path (it is set to 0x30000, 0x60000 or
  0x100000 and decremented by the space and cutscene loaders). So **the base is not a
  constant**. The harness reads the global and uses it only as a cross-check.
* `FUN_00258128` decompresses core data to that base (`fun_0020b618(wad + 0x50, base)`), then
  calls `fun_002040e0(base + core_index[2], ...)` (level01 0x255470) on the tfrags block. That
  call rewrites every header's `data` (0x10) into the **absolute pointer** `table + data`.
  `LightTfrags` then overwrites `data + rgba_ofs` (`rgba_size` qwords) with the lit colours.
* Light banks: directional at `0x180340` (16 x 0x40), point at `0x180740` (8 x 0x20).

The **locator** does not depend on any of these globals, so it works for every level. It:
1. Searches RAM for tfrag 0's bounding sphere (header bytes 0x00-0x10).
2. Takes the candidate where the most headers equal the disc on the load-invariant bytes:
   0x00-0x10, 0x14-0x2b, 0x2c, 0x2e-0x34, 0x38-0x3a and 0x3c-0x40. It excludes `data` (relocated),
   `occl_index_stash` 0x2b and `flags` 0x2d (writers unknown), the light bytes 0x34-0x37, and
   `occl_index` 0x3a-0x3b: the first real savestate showed the loader renumbers it (disc 0 for every
   Novalis tfrag, RAM a per-tfrag value), which made the old locator miss the table.
3. Checks that each header's RAM `data` field is the absolute pointer `table + disc data`.
4. Cross-checks the level global (level01 `0x174294` + `LevelCoreHeader.tfrags`).
5. Finds the directional bank by searching for the gameplay file's light sets. The point bank
   is assumed to follow it at +0x400 (verified for level01, inferred for other levels).

The report also counts, per volatile header byte, how many tfrags changed at run time. That
is free information about unknown writers such as `dir_lights_upd`.

## 4. Producing the input (user steps)

1. Start PCSX2. In the setup wizard, point it at your console's BIOS dump and add
   `~/PS2/ratchet1/` as a game directory.
2. Boot *Ratchet & Clank*. Start a new game and play the short Veldin opening until the ship
   lands on Novalis. There is no retail level select. Once you are there, save in-game to a
   memory card so later sessions start on Novalis.
3. Once you have control on Novalis, press **F1** (Fn+F1), or use System > Save State. Any
   moment after the level has loaded works.
4. Run the comparison:
   ```
   PATH=/opt/homebrew/opt/rustup/bin:$PATH cargo run -p rc-trace -- compare-tfrag-light --state latest --level 01
   ```
   `latest` means the newest `SCUS-97199*.p2s` in PCSX2's `sstates` folder. You can also give
   a path, or the name of a kept savestate (`--state novalis_spawn`).
   **With PINE enabled** you can skip steps 3-4: keep the game running on Novalis and run
   `cargo run -p rc-trace -- compare-tfrag-light --pine`.
5. Optionally, keep the state for regression runs: `cargo run -p rc-trace -- save-state <name>` copies the newest
   one to `~/PS2/ratchet1/savestates/<name>.p2s`.

## 5. What the tool reports

`compare-tfrag-light` prints:
* Where the table was found, how many headers match, whether the pointers were relocated,
  whether the level global agrees, and whether the light bank in RAM equals the disc's. If
  the bank differs, the comparison runs a second time with the RAM bank, which separates
  errors in parsing from errors in the maths.
* Totals: tfrags that match fully, and vertices and bytes that are equal. These are split by
  vertex class:
  * `single-set`: mul/add/max only.
  * `blended`: adds `rsqrt`.
  * `point-lit`: adds `div`/`sqrt`. It uses the tfrag's live header 0x36 and the RAM point bank.
* A "RAM == disc placeholder" count. It should be near zero if the pass ran; our own count is
  printed as the baseline.
* A histogram of `ours - RAM` per channel (-8..8, plus a "far" bucket), the first N
  mismatches with their light records, and every mismatch as a CSV in
  `work/trace/level01_tfrag_light_mismatches.csv`.
* The exit status: 0 = every lit byte equal, 2 = mismatches, 1 = error.

A 1-ULP multiplier deviation (tfrag_lighting.md §6) would show up as isolated ±1 differences.
A wrong operation order or rounding mode would show up as a spread histogram. Differences
only in `blended` point at the `rsqrt`/`div` model.

Only entries `0..vert_count` are compared. The padding entries up to `rgba_size*4` hold
scratchpad leftovers in the game, so they are only counted.

`compare-novalis-spawn <source>` (a savestate on Novalis after the arrival, at the spawn) runs every check one
state allows, against the port (`rc_game`, headless: `port_sim.rs` mirrors rc-engine's `gameplay.rs` wiring
without Bevy) and prints a summary table; exit 0/2/1 as above. Sections:
* **a. game state**: every save chunk (47 global + 20 × 11 per level, from the boot ELF's descriptor tables) at its
  RAM address vs `rc_game::game_state` after new game → Veldin → transition → Novalis start; play-dependent chunks
  (bolts, times, landmarks, help, Veldin kills, moby pack…) are listed with both values but not counted.
* **b. hero**: ~60 hero-block fields (0x13f350..0x141600) and Ratchet's moby, bit for bit.
* **c. camera**: 0x167240 pos, 0x167250 Euler, 0x167450.. rows, 0x16cf70.
* **d. fog**: 0x15f444.. vs the level settings + the fog zone at the camera; the underwater look 0x161200.
* **e. rand + tick counter**: 0x12f4d8 as a draw count from `srand(1234)` (`port_sim::lcg_distance`, exact, by
  bit lifting), the port's load-pass and per-tick draws; 0x15f5cc vs `Game::counter`.
* **f. moby table**: array base from Ratchet's moby pointer 0x1413d0 − index·0x100; RAM slot j paired with port slot
  j (the port applies the loader's spawn test, so both create the same slots; each pair is checked for equal class
  +0xa6 and spawn id +0xb2); per-field and per-class tallies, every mismatch in `work/trace/novalis_spawn_mobys.csv`; the slots after the statics.
* **g. tie / shrub palettes** (also alone: `compare-tie-shrub-light`): ties via `*0x160fc0` (0x20-byte records,
  +0x10 → 0x1c0 record, +0x40 lit RGBA), shrubs via `*0x160494` / `*0x16049c` / `*0x1604a0`; every record is
  checked by content (ambient copy, matrix) before comparing; lit with the RAM bank and each record's live
  selector (+0x1c) and point list (+0x1e).

The port timeline comes from the RAM: ticks = 0x15f5cc − 1; the first `ticks − 0x15f5c8` run in game mode 2 (the
arrival scene); a ✕ press held `--press-ticks` (default 2) ending at the tick the idle counter 0x160ff0 points at
(`--no-jump` disables it). Diagnostic knobs: `--ticks N`, `--cutscene-ticks N`, `--load-pre-draws N` (extra
`rand()` draws right before the load pass, on top of HeroInit's own fidget draw, which the port always makes),
`--load-emitters-visible` (the load pass's class-27 emitters see the camera and spawn one particle each: the pre-fix
port; by default they are culled by the game's unbuilt view, as in the game). `port-load-pass --out F.csv` dumps the
port's statics right after its load pass (no savestate needed). The loader's spawn test is ported
(`rc_formats::moby_spawn::spawn_test` / `loader_spawns`), so `port_sim` creates the same 929 slots as the RAM.
Not modelled in `port_sim` (expected to differ): the arrival scene (actors, camera), hand items, +0x31 beyond a
draw-distance stand-in, the unported classes. Regression: `tests/trace/novalis_spawn.rs` compares the port with the
committed, numbers-only fixture `tools/trace/tests/fixtures/novalis_spawn.tsv` (needs only `extracted/`; skipped
without it). `distill-spawn <source>` rewrites the fixture from a savestate (`--state novalis_spawn`) or its dump,
after checking that the fixture checks and the savestate checks give the same tallies on it.

Other commands, for future comparisons:

| Command | Use |
| --- | --- |
| `info --state F` | savestate version and entry list |
| `dump-ee <src> [--out F] [--entry NAME]` | write EE RAM (or any savestate entry, e.g. `vu1Memory.bin`, `Scratchpad.bin`); default `work/trace/<name>_ee.bin` |
| `find <src> --bytes HEX [--align N]` | locate a byte pattern in EE RAM |
| `read <src> --addr HEX --len N` | hex-dump EE memory |
| `pine-info`, `pine-savestate N` | PINE status / trigger a savestate |
| `synth --level NN [--unlit] --out F` | synthetic "loaded level" EE image (our lit bytes; see §6) |
| `compare-novalis-spawn <src> [...]` | the Novalis spawn checks a–g (above) |
| `compare-tie-shrub-light <src>` | tie + shrub lit colours only (level 01 globals) |
| `port-load-pass --out F [--load-pre-draws N] [--load-emitters-visible]` | the port's statics after its load pass |
| `save-state NAME [--from F.p2s] [--force]` | keep the newest PCSX2 savestate as `~/PS2/ratchet1/savestates/NAME.p2s` |
| `distill-spawn <src> [--out F]` | the Novalis spawn facts as the committed test fixture (§5, docs/workflows/pcsx2.md) |

`<src>` is `--state F|dir|latest|NAME` (NAME = `~/PS2/ratchet1/savestates/NAME.p2s`), `--ee raw.bin` or `--pine [slot]`. Output defaults go to `work/trace/`.

## 6. Verification done without a real savestate

* `zip` unit tests: stored, deflate and zstd entries round-trip; the CRC-32 known value
  matches; a corrupted byte is detected.
* The parser also read a savestate-shaped zip written by an independent implementation
  (Python 3.14 `zipfile`, zstd level 3 + deflate).
* `ee` tests:
  * address mirrors (0x0/0x2/0x3/kseg0/kseg1), and the scratchpad is rejected;
  * pattern search with alignment;
  * a v2.8.2-shaped savestate: the version id is parsed, `latest`/directory resolution
    works, and the RAM is read back.
* `pine` test: a fake Unix-socket server checks the framing and the 50000-per-message batching
  (120000 reads = 3 messages).
* `tests/trace/synthetic.rs`, on the real Novalis data: the tfrags block is placed at an arbitrary
  base, with relocated `data` pointers, the light bank and the level global, as the game does.
  * With our lit RGBA written in: 1004/1004 tfrags and 51331/51331 vertices match
    (129 blended).
  * One corrupted byte gives exactly 1 mismatch, at the right tfrag/vertex/channel, with d = +1.
  * The unlit disc placeholders give 1814/51331 vertices equal, and all 51331 are flagged as
    "RAM == placeholder".
  * With the level global zeroed, the search alone still finds the table.
  * The same image packed as a zstd `.p2s` matches through the savestate path.

## 7. Extending to other passes

The pattern: load disc inputs with `rc-formats`, find the game's copy in RAM by content, not
by a hard-coded address, run our port, compare, and bucket the differences by code path. Add
a module next to `tfrag_light_cmp.rs` and a subcommand.

* **Tie / shrub lighting** (`LightTies` level01 0x2ab218, shrub pass `FUN_0029e7e8`): same
  bank and idioms. Locate tie instances and shrub instances by their disc bytes, then compare
  their colour arrays once ported.
* **Moby skinning and lighting** (docs/plan/moby_skinning_lighting.md): the results live in
  VU1 data memory or the scratchpad, not in EE RAM. Use `dump-ee --entry vu1Memory.bin` or
  `Scratchpad.bin` from a savestate. The VU1 double buffers change every frame, so the state
  must be taken while paused, and the camera and moby state must be read from the same image.
* **VU1 constant blocks and camera** (docs/plan/game_camera_fog.md: tfrag block `0x1de750`,
  view matrices `0x186f40`/`0x186f80`, projection `0x18cdc0`, camera `0x187080`): these are
  **boot-ELF addresses**. For a level, take the level ELF's address from the matching function
  in `work/decomp/levelNN.elf/` (the Lombyte "exact-boot-match" comments pair the functions),
  or find it by content with `find`. Then read the camera from RAM, run our view-context code
  and compare the block. The same block also appears in `vu1Memory.bin` at qw 656..670.
* **Fixed test points.** Keep savestates for the scenes you test against in
  `~/PS2/ratchet1/savestates/` (`save-state <name>`). Record in this doc the level, slot, and what is on screen, so
  comparisons are repeatable. A test uses a scene only through a distilled fixture (like `distill-spawn`).

## 8. Triage

Policy: docs/plan/decisions.md, "Native-first fidelity policy (2026-09-27)". The port must behave like the original;
bit-exactness is a diagnostic tool, not the goal. So a mismatch this harness reports is not automatically a bug. Sort
every mismatch (or cluster of mismatches from one code path) into one of two kinds before acting on it:

1. **Misunderstanding of the game.** Wrong formula, constant, field, offset, operation order or timing (wrong tick,
   missing frame lag, wrong RNG draw order). Typical signatures: large or "far" differences, a spread histogram, whole
   tfrags or whole vertex classes wrong, values shifted by a constant or by a whole tick, or a difference that grows
   over time. **Action:** find the cause in the decomp and fix it natively. Note the fix in the system's doc.
2. **Pure hardware-arithmetic difference.** Last-bit float rounding (FMAC truncation, `rsqrt`/`div` ULPs, missing
   denormals), GS blend byte differences, SPU2 sound-chip interpolation. Typical signatures: isolated ±1 differences
   (for a colour byte) or 1-ULP differences (for a float), no growth over time, and (where a PS2 float model of the
   pass exists) the difference disappears when that model is used instead of IEEE floats. **Action:** accept it with a documented tolerance, unless the
   difference is noticeable in play (visible, audible, or changes a gameplay outcome such as a trigger tick or a
   collision result). Then reproduce the *result* natively, not the PS2 mechanism, and record it under "Result-level
   reproductions" in docs/plan/hardware_fidelity_layers.md.

If a mismatch cannot be classified, treat it as kind 1 until shown otherwise: a real misunderstanding hidden inside a
"±1" bucket is the expensive mistake. While the existing fidelity layers are in place (Phase 1), a kind-2 difference in
code that uses the PS2 float model is still worth a look: it may point at an operation-order error.

**Recording a tolerance.** Add one row to the "Tolerances" table in docs/plan/hardware_fidelity_layers.md:

* **Item**: the compared quantity and the command that measures it (e.g. tfrag lit RGBA, `compare-tfrag-light --level 01`).
* **Tolerance**: a bound the harness can check, e.g. max |d| per channel and the share of samples allowed to differ
  (take them from the histogram in §5), or a float bound in ULPs or absolute units.
* **Why acceptable**: the hardware cause and why it is not noticeable in play.
* **Measured**: date, savestate (level, slot, what is on screen; kept in `~/PS2/ratchet1/savestates/`) and the observed numbers.

When a comparison command gains a tolerance, give it an option to apply the bound, so the exit status (0/2) reports
"within tolerance" rather than "bit-equal". Keep the exact counts in the output too: they stay the diagnostic signal.

## 9. Open items

* First real run done (2026-09-26, trace_results_novalis.md): tfrag lighting 51331/51331 vertices bit-exact
  (tfrag_lighting.md §8's float-model question: the model holds on retail data), shrubs 28992/28992, ties
  96511/96512 (one ±1, proposed tolerance there). ~~The tolerance rows proposed in that doc still need to go into
  docs/plan/hardware_fidelity_layers.md "Tolerances".~~ **Done 2026-10-19**: all are in hardware_fidelity_layers.md
  "Tolerances" (copied 2026-09-27); applying them in the commands (§8) is G-TOOL-024.
* Only level01 has known global addresses (`known_addrs`, the tie/shrub/moby globals). The tfrag check relies on
  the search for other levels; the others need their level's addresses.
* Header bytes changed at run time on Novalis: 0x3a/0x3b only (0x2b/0x2d/0x35 unchanged at this point).
* A second savestate a few seconds later (pad untouched) would pin the per-tick `rand` count in gameplay.
* `rc-trace` depends on `rc-game` (path crate, no external dependency): a mid-edit break in `rc-game` stops the
  whole binary from building.
