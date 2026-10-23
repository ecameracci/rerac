# Orchestration handbook

How this project has been run since 2026-09-26, written so another model (Opus 5.5) can take over the orchestrator role without loss. The orchestrator does no engineering itself: it writes briefs, dispatches Opus 5.5 subagents, relays their reports to the user, and keeps the dependency graph moving.

## 1. The standing rules (from the user)

- The goal is a **faithful reimplementation** in Rust/Bevy of the user's own copy of Ratchet & Clank (2002, PS2, SCUS_971.99): identical behaviour, not a bit-exact C++ port and not emulation. Decompilation (Ghidra + Lombyte) is the behaviour reference; PCSX2 traces are the diagnostic truth test (behaviour is the goal, bit-exactness is not: §3.6); loaders are regression-tested against committed snapshot hashes (the C++ reference extractor that first verified them was retired on 2026-09-27, decisions.md).
- Nothing disc-derived is ever committed. `extracted/`, `work/`, `target/`, `dist/` are git-ignored; committed verification tables hold only sizes, counts and hashes (`crates/rc-extract/data/*.tsv`, `crates/rc-formats/data/loader_snapshots.tsv`). No disc bytes in source or tests; tests read `extracted/` or the ISO at runtime and skip when absent. Nothing has been committed yet (the user never asked; don't commit unless asked).
- **Build times are sacred.** Never change the Bevy version, `[profile.*]`, `.cargo/config.toml`, workspace dependency versions or Bevy features. Engine builds only via `cargo dev` / `cargo dev-build` (dynamic linking, ~1 s incremental). Warn before anything that could invalidate the Bevy cache. Adding a workspace path crate is fine; adding an external dependency needs a reason and a note.
- The orchestrator model must not do the work. **Every** code change, build, test, doc edit or investigation goes to an Opus 5.5 subagent (`model: "opus"`, `subagent_type: "general-purpose"`, `run_in_background: true`). Never spawn a Fable subagent. The orchestrator may: write briefs, read reports, send follow-up messages to agents, save its own memory notes, send screenshots to the user, and answer the user's questions.
- Wrench (`~/Globals/wrench`) is GPL: orientation only, never copy code. OpenGOAL (`~/Globals/jak-project`) and its vendored VU disassembler are ISC and may be mirrored with attribution.

## 2. Repository map (what exists)

- `crates/rc-formats`: all loaders, golden-tested on all 19 levels against committed snapshot hashes (`data/loader_snapshots.tsv`, generated while byte-identical to the retired C++ oracle) (wad, toc, level core, textures, tfrag, tie, shrub, sky, moby, gadget, collision, occlusion, particle/FX textures, moby animation, lighting passes, disc/ISO reader, gameplay sections, fog zones, hud/strings/sound-bank/scene in flight). PS2 float model in `tfrag_light::ps2`.
- `crates/rc-game`: pure gameplay logic, no Bevy: collision kernels, rng, particles, hero controller + pad + follow camera + tick, fog zones, sky stars, water sim, moby runtime; scheduler/menus/game-state/audio/scene player in flight.
- `crates/rc-engine`: Bevy app. One module per renderer (`tfrag_*`, `tie_*`, `shrub_*`, `moby_*`, `sky_*`, `particle_render`, `water_render`, `hud_*`, …), `gs_state.rs` (GS alpha/depth rules), `determinism.rs` (frame-exact capture), `occlusion.rs`, `fog_state.rs`, `disc_source.rs`. Env switches are listed in `docs/workflows/dev-switches.md`.
- `crates/rc-extract`: `rerac-extract` (Tier 0 archive checked against `data/scus_971_99.tsv`, Tier 1 cache, Tier 2 exports). The C++ oracle (`src/core`, `tools/extract`) was retired on 2026-09-27; a new loader gets a golden test in `crates/rc-formats/tests/formats/golden.rs` that records snapshot rows (`tests/formats/snapshot/mod.rs`).
- `docs/formats/*`: verified format docs. `docs/plan/*`: one investigation doc per system (player_controller, moby_update_catalogue, particles, hud_text, audio, world_animation, occlusion_culling, collision_queries, game_state, menus, cutscenes_transitions, moby_animation, moby_skinning_lighting, tfrag_lighting, tie_lighting, shrub_lighting, sky_render_notes, game_camera_fog, level_sweep, trace_harness). `docs/plan/roadmap.md` has a dated status block at the top.
- `tools/` (dev only, never ships; each tool has a README): `tools/trace` (package `rc-trace`: PCSX2 savestate/PINE
  comparison harness, `docs/plan/trace_harness.md`), `tools/ghidra/scripts` + `tools/ghidra/names` (Ghidra scripts and
  name tables, `names/*.csv`, `clusters.tsv`), `tools/package` (release packaging), `tools/repo-checks` (layout guard
  tests), `tools/xtask` (`cargo xtask <command>`: `regen-data` rebuilds `extracted/` from `RC_ISO`, `package`).
- `docs/workflows/`: one page per dev workflow, one command per task (ghidra, pcsx2, game-data, release, launcher).
- `extracted/` (git-ignored): game data only, what `rerac-extract` writes. `work/` (git-ignored): generated dev output
  (`work/decomp/<program>/` decompiler export, `work/trace/`, `work/ghidra-import/`, `work/vu/`, `work/exports/`,
  `work/captures/`). `~/PS2/ratchet1/` (outside the repo): the user's ISO, `savestates/`, `traces/` (recordings); the
  Ghidra project is `~/ratchet1.gpr` + `~/ratchet1.rep` (target home `~/PS2/ratchet1/ghidra/`). Full layout:
  `docs/plan/repo_reorg.md`.

## 3. The working method

### 3.1 Two kinds of brief
1. **Investigation (docs-only).** Read-only; produces `docs/plan/<system>.md` with addresses, struct offsets, math, confidence lines per finding, unknowns, and a "Port plan". Always precedes a port of a system nobody has reversed yet.
2. **Port.** Implements exactly what a doc says, with tests, and appends an "In the port" section to that doc. Ports never guess: when the doc is ambiguous the agent reads the disassembly (Ghidra MCP) and pins it.

Loaders follow a fixed recipe: verify the format against the decomp and the spec, add a golden test that records every parsed section for levels 0–18 into the snapshot table (`RC_SNAPSHOT_WRITE=1 cargo xtask test-job --test formats --filter golden::`, rows for that test only), check invariants independent of the loader, prove the test can fail by breaking one thing and restoring it, report totals. Changing an existing loader's output means regenerating its rows, with the reason stated.

Renderers follow a fixed recipe: establish the GS state and math from the decomp, implement, verify with `RC_SCREENSHOT_FRAME=N` captures the agent LOOKS at with Read, check determinism (two runs byte-identical), report fps with `RC_NOVSYNC=1`.

### 3.2 The brief template
Every brief carries, in this order:
- One paragraph of context: what exists, where (file paths, doc names, the exact API names the agent will call, copied from the previous agent's report).
- **Product or tooling, and which folder**: "Product work in `crates/<crate>`" or "Tooling work in `tools/<tool>`" (plus `docs/` as needed). Product work never adds a dependency on `tools/`; tooling work writes its output only to `work/`.
- **Hard constraints** block: cargo path (`export PATH="/opt/homebrew/opt/rustup/bin:$PATH"`); `cargo dev` only; no Bevy/profile/dep/feature changes; **file ownership** (explicit list of files it may create/edit; "minimal insertions, re-read right before each edit" for `main.rs`, `level_load.rs`, `lib.rs`; explicit "do NOT touch" list naming the files other running agents own); no commits; no disc bytes; targeted tests green and clippy clean for its files (test policy below); Ghidra MCP URL and API gotchas (`decompile_function` takes `address`; pass the program per call; disassembly for asm/VU0 code); Lombyte path.
  Every brief also carries these standard lines verbatim:
  - "Use standard floats and native mechanisms; do not add hardware modelling. If a PS2 arithmetic effect is noticeable in play, reproduce the result, not the mechanism, and record it in hardware_fidelity_layers.md."
  - "Do not spawn sub-agents."
  - "Only one cargo or engine process at a time: never start a build, test or engine run while another is still running, and never run two in parallel from one command."
  - "Run the engine with `RC_SCENE=0`."
  - "Every port-only option (one the original game does not have) goes on the native-looking 'Port Options' page under Options."
  - "Add every item you leave unfinished to `docs/plan/gaps.md` (new IDs in the right section) and mark the gaps you close there (struck through, with the date and your doc § or file); your report lists both."
  - "Tests (docs/workflows/testing.md §2.1): `cargo xtask test-job <areas> [shared]` (areas: <named in the brief>): the unit tests, the integration binaries of your areas, and with `shared` (if you touched shared code) the NO_IDLE digest compared with the baseline and `all_levels_smoke`; take the baseline with `cargo xtask digest-baseline` before your first edit if there is none. One binary: `cargo xtask test-job --test <binary>`, one module or test: add `--filter <module>::[<test_fn>]` (`--exact` for a full path); one crate's unit tests: `cargo xtask test-quick <crate>`. Then clippy on your files (`cargo clippy-all`), and `cargo check-all` once at the end. Never run `cargo test` or `cargo nextest` directly; always go through `cargo xtask test-*`."
- **Test policy (2026-09-29): see `docs/workflows/testing.md`** (tiers, the area → tests map, the test audit). Two
  tiers. Per job: the unit tests, the integration groups of the areas the brief names ("areas: weapons, classes"),
  the shared-code guard set (NO_IDLE hero digest and `all_levels_smoke`) when the job touched shared code, and one
  final `cargo check-all`; the first three are one command, `cargo xtask test-job <areas> [shared]`. Full suite (`cargo xtask test-full`: nextest when
  installed, else `cargo test --workspace --features rc-engine/dev --no-fail-fast`; it also compares the NO_IDLE digest with the baseline and fails on a mismatch): only on merge to main, or for a big
  shared-code commit; the coordinator runs it through the commit agent and writes the result to
  `work/test-results/latest.txt` (git-ignored) with the time, HEAD and `git status --short`. The digest baseline is
  rewritten only by `cargo xtask digest-baseline`, when the user has accepted an intended hero change. Failures go back to the agent that owns the files.
- **Gap register.** `docs/plan/gaps.md` is the one list of known gaps. Every report and doc update adds the agent's
  unfinished items there (new IDs) and strikes the ones it closes.
  File gaps at the system level: name the missing shared system, and list the specific object or level only as a
  consumer of it.
- **Post-port fixes.** `docs/plan/post_port_fixes.md` lists the original game's own bugs that the port keeps on
  purpose, to fix once the port matches the original. A faithful port of a bug goes there, not into the code.
- **Read first**: the docs and files, with the specific facts the agent must not re-derive.
- **Deliverables**: numbered, concrete, each with its test or verification.
- **Report back**: the exact list of things the orchestrator needs (numbers, addresses, screenshot paths, verified vs inferred, left undone, files changed). The report is all the orchestrator sees; agents' transcripts are never read (they would flood context).

### 3.3 Concurrency and file ownership
- **At most 3 agents run at once** (user rule, 2026-09-27; supersedes the earlier ~10–12). Queue the rest and dispatch as reports arrive.
- **One cargo/engine process at a time** across all agents (user rule, 2026-09-27). Agents must not overlap builds, tests or engine runs; say so in every brief (§3.2).
- Disjoint file ownership is what makes it safe. Shared files (`main.rs`, `level_load.rs`, both `lib.rs`) are "insert one line, re-read first". When two agents must touch one file, sequence them: dispatch the second when the first reports.
- A finished agent's files become free. Before dispatching, list which running agents own which files (the orchestrator keeps this in its head; if it's lost, `ls -lt` recent mtimes plus the last reports tell you).
- When an agent reports something another running agent needs (an API change, a finding, a bug it can't fix), forward it with `SendMessage` to that agent's id and tell it exactly what to do; don't wait for it to finish.
- **Agents must not spawn sub-agents** (user rule, 2026-09-27; every brief says so). If a stray sub-agent hand-back still arrives as a message, save it verbatim to the scratchpad (`/private/tmp/claude-501/.../scratchpad/<topic>_reports/`) and point the next brief at it.

### 3.4 Dependency discipline
After every report, privately list: (a) reports in flight, (b) items each report unblocks, (c) items that depend on nothing. Dispatch every independent item immediately in one turn. Never dispatch two agents that would define the same struct or rewrite the same renderer.

### 3.5 Relaying to the user
Lead with the outcome. Tables only for numbers that matter. Send screenshots with `SendUserFile` when a renderer lands. Distinguish verified (disassembly/data) from inferred, and say what is left. Flag doc corrections agents made. Keep it short.

### 3.6 Fidelity policy the agents follow
Source: docs/plan/decisions.md, "Native-first fidelity policy (2026-09-27)". The port must *behave* like the original (what the player sees, hears and feels) using our own native systems; bit-exactness is a diagnostic tool, not the goal. Phase 1 is in force:
- **No new hardware modelling.** New code uses standard IEEE floats (`f32`/`f64`) and native Bevy mechanisms by default. Do not extend the PS2 float model (`tfrag_light::ps2`, `rc_game::ps2v::Pf`) to new code, and add no new entries to `docs/plan/hardware_fidelity_layers.md`.
- **Reproduce the result, not the mechanism**, when a hardware difference would be noticeable in play (e.g. the water ripple steps every 9 ticks on the PS2 because of float truncation: the port steps every 9 ticks). Record each one in `hardware_fidelity_layers.md` ("Result-level reproductions").
- Match the game's logic: formulas, constants, fields, operation order and timing. Getting these wrong is a bug, whatever the arithmetic.
- Game bugs/quirks: reproduce them natively when they change what the player sees, hears or feels (e.g. the dropped-joint scale rule); do not model memory leftovers (stale bytes, uninitialised stack words) mechanically, reproduce the resulting value only if it matters in play. Document each.
- **PCSX2 mismatches are triaged** (`docs/plan/trace_harness.md`, "Triage"): misunderstandings of the game get fixed natively; pure hardware-arithmetic differences (last-bit float rounding, GS blend bytes, sound-chip interpolation) are accepted with a tolerance recorded in `hardware_fidelity_layers.md` ("Tolerances"), unless noticeable in play.
- **Existing layers stay for now** (they serve diagnosis). Code that already uses them keeps working as is; agents editing such code keep it consistent rather than half-converting it. The strict native pass (Phase 2) replaces them later in the order given in `hardware_fidelity_layers.md`, PS2 float model last. Existing layers the port currently relies on:
  - Shared RNG stream: `srand(1234)` at level init, then load pass, then per tick mobys (slot order) → hero → particles → camera → counter, then render (stars). Every consumer draws in that order.
  - 60 Hz logic tick (`GameTicks`), catch-up rule `ticks_for_frame`; deterministic mode = exactly one tick per frame.
  - GS state per pass lives in `gs_state.rs` (`GsPass`); alpha test AREF 0x60 (0x20/0x0c for shrubs) with RGB_ONLY fail = two draws.
- Blending happens in linear light. The raw-display-byte framebuffer for exact GS blending (§4.2 item 1) is **on hold**: it would be a new hardware layer.

## 4. State on 2026-09-27 (hand-over point)

> A snapshot of 2026-09-27, kept for the record: most agents and queue items below are done. The backlog is
> gaps.md, the order priority_order.md.

### 4.1 In flight (report expected; what each unblocks)
| Agent | Owns | Unblocks |
|---|---|---|
| HUD port (hud/strings/font loaders, 2D pass, text, HUD state) | `rc-formats/{hud,strings,font}.rs`, `rc-engine/{hud_render,text_render}.rs`, `hud.wgsl` | menus draw path, subtitles, banners |
| Audio port (bank, VAG, voices, music, mixer) | `rc-formats/{sound_bank,vag}.rs`, `rc-game/audio*`, `rc-engine/audio_out.rs` | scene speech, moby sound triggers |
| Water surfaces (strips, ripples, foam) | `rc-formats/water.rs`, `rc-game/water.rs`, `rc-engine/water_render.rs` | underwater hooks in `fog_state.rs`, other levels' water |
| Moby low LOD + chrome/glass + untextured (−1) fix | `rc-engine/moby_render.rs`, `moby_lod.rs`, `moby_metal.wgsl` | frees `moby_render.rs` |
| Ghidra name consolidation | `tools/ghidra/names/*`, Ghidra project | nothing; improves later work |
| Game state + save format | `rc-formats/save_game.rs`, `rc-game/game_state.rs` | menus options, quick-select slots, bolts counter |
| Gameplay wiring (playable Novalis) | `rc-engine/{gameplay,input_map,play_camera}.rs` | everything gameplay-side on screen; defines pad bytes, `Mode`, tick driver |
| Moby update scheduler + bolts/crates/grass | `rc-game/moby_update/*` | `TickHooks.mobys` fill-in; more classes |
| Menus (doc consolidation + ring + pause + options + planet select) | `docs/plan/menus.md`, `rc-game/menus/*`, `rc-engine/menu_render.rs` | vendor screen, ship menu flow |
| Scene player + Novalis arrival scene 5 | `rc-formats/scene.rs`, `rc-game/scene_player.rs`, `rc-engine/scene_render.rs`, toc/disc scene naming | take-off/landing scenes, transitions |

Join points these agents were told to coordinate on: pad bytes and `Mode` (wiring ↔ menus ↔ scene), `Mode::advances_tick()` (menus → wiring), `TickHooks.mobys` (scheduler → wiring), speech VAG request hook (scene → audio), ripple water height + zone (water → `fog_state.rs`), HUD draw order before the underwater tint (fog → HUD).

### 4.2 Queued (dependent)
1. ~~Raw-byte framebuffer for exact GS blending + the AA blit~~ **On hold** under the native-first policy (decisions.md 2026-09-27): it adds a hardware layer. Revisit only if a blend difference is noticeable in play, and then fix the result natively.
2. Wrench hit delivery → crates/enemies (`deliver_hit`), more moby classes from the catalogue (critters 577, amoeboids 572/865/866, path enemy 459, lifts/doors, vendor 11, infobot 750, gold bolt 1134), spawn conditions and inactive pools on other levels.
3. Vendor screen and ship menu flow (after menus + game state).
4. Other levels' water classes; lava/goo.
5. Particle types beyond 6 (driven by class ports), render kinds 1–3.
6. Take-off/landing scenes, title cards, FMV playback (MPEG-2 + SShd audio; defer decoder).
7. Tie second-list AREF 0x40 and the clip-list programs (minor).
8. Hero light cross-fade in fog zones (hook exists in `rc_game::fog_zones::hero_light`).
9. Disc reader: serve global lumps from the ISO (`spaceships` currently read from `extracted/`); game-state agent is adding `Disc::global_lump`.
10. Memory-card image import into `rc-trace`.

### 4.3 Standing rules added on 2026-09-27
These apply to every dispatch from now on (the brief lines are in §3.2):
- Native-first fidelity policy, Phase 1 (§3.6; decisions.md).
- At most 3 concurrent agents.
- Agents spawn no sub-agents.
- One cargo/engine process at a time.
- Engine runs use `RC_SCENE=0`.
- Every port-only option goes on the native-looking "Port Options" page under Options.
- Every brief states product (`crates/`) or tooling (`tools/`) and which folder it touches (§3.2).
- Tools write generated output only to `work/` (never `extracted/`, never the repo tree outside `work/`); the one
  exception is a deliberate, committed fixture (e.g. `rc-trace distill-spawn`).
- No test may depend on personal files (ISO copies, savestates, recordings, EE dumps). PCSX2 findings reach tests
  only as distilled, numbers-only fixtures (docs/workflows/pcsx2.md). `tools/repo-checks` enforces the product side.
- Scratch and debug captures go in the agent's scratchpad subfolder or `work/captures/`, never in the repo tree or
  `extracted/`.
- Agents read decompiled code from `work/decomp/<program>/` (`index.tsv` + one `.c` per function).
- Paths agents use: product `crates/`; tools `tools/trace`, `tools/ghidra/{scripts,names}`, `tools/package`,
  `tools/repo-checks`, `tools/xtask` (`cargo xtask help`; agents never run `regen-data` on the real `extracted/`
  unless the brief says so: use `--data-dir` on a scratch folder); game data `extracted/` (`RC_EXTRACTED`); dev output `work/{decomp,trace,ghidra-import,vu,exports,captures}/`
  (`RC_WORK`); personal `~/PS2/ratchet1/{savestates,traces}/` (`RC_PERSONAL`, read-only for agents unless the brief
  says otherwise); the Ghidra project `~/ratchet1.gpr` + `.rep` (only through the Ghidra MCP, only when the brief
  allows it); workflows `docs/workflows/`.

### 4.4 User-gated
- **PCSX2 comparison** (mismatches are triaged per `trace_harness.md` "Triage"): the user must install the BIOS in PCSX2 2.8.2, play to Novalis, press F1 (Fn+F1) for a savestate, then run `cargo run -p rc-trace -- compare-tfrag-light --state latest --level 01` (or enable PINE and use `--pine`). First targets: tfrag lit RGBA, then hero position per tick, moby colours, scene-5 trigger tick, fog values.
- Commits: only when the user asks.

## 5. Gotchas collected
- Ghidra MCP: `import_file` with a `language` param forces the raw loader; `rename_function` returns `{status:"success"}`; `set_comment` uses `type`; `delete_file` uses `filePath`; don't leave the active program switched; the GUI holds programs. Some level overlays time out on `open_program`; use identical copies in another level.
- Lombyte names are mostly right but several are wrong (listed in `tools/ghidra/names/doc_names.csv` notes and `docs/plan/menus.md`). Never override a Lombyte name in Ghidra; add a plate comment.
- Each level overlay is its own executable: boot-ELF addresses are not valid in-level; docs say which program an address belongs to.
- Screenshot harness: `RC_SCREENSHOT_FRAME=N` (offscreen capture; exit 2/3 on failure). The default camera frames Ratchet's start (`fly_cam::spawn_view`, G-TOOL-015, 2026-10-19; leftover G-TOOL-025); `RC_CAM` sets another view. `RC_OCCL=0` for free-fly.
- Agents occasionally hit transient compile errors from another agent's mid-edit; they should wait and retry, not edit the other file.
- The scratchpad is shared; agents have overwritten each other's helper scripts. Tell agents to use a subfolder named after their task.
- Keep the user's machine awake for long overnight runs (`request_keep_awake` with `session_idle`).

## 6. Memory
The orchestrator's memory lives in the Claude Code project memory folder for this checkout (`~/.claude/projects/<checkout path with each "/" as "-">/memory/`) (index `MEMORY.md`): the orchestrator-only rule, build-time sensitivity, Ghidra gotchas, user preferences, and a dated status note. Update the status note at hand-over points.
