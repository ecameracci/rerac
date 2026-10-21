# Priority order: every known gap, by importance and by dependency

Written 2026-09-29, docs only (nothing built or run). Inputs: every open row of `gaps.md` (struck rows skipped; merged
IDs resolve to their homes, gaps.md "Merged IDs"), `class_census.md` and `work/census/*.tsv` (the re-run of 2026-09-28),
the coordinator's memory notes (read-only) and the plan docs named per edge. Items that exist only in memory get an
`M-` name. `gaps.md` stays the backlog; this page only orders it.

## 1. Method

**Edges.** Four kinds, from the item to its prerequisite:

| kind | meaning |
|---|---|
| **depends** | cannot start without the prerequisite |
| **based-on** | builds on its data or structures |
| **reuses** | calls its code |
| **easier** | much cheaper once the prerequisite is done |

Evidence codes:

| code | source |
|---|---|
| `gaps` | the row's own "Depends on" / "Needs" column in gaps.md |
| `cen` | class_census.md's main table or its unique-blocker table |
| `u_s` | co-requirement counts computed for this page from `work/census/unit_systems.tsv` (units needing system X that also need Y; "sole" = X is the only non-conditional system the unit still lacks; conditional = cheat, Suck Cannon interface, save) |
| `LG` | level_generalisation.md |
| `LS` | level_scripting.md |
| `mem:<file>` | a memory note |

Inferred edges are marked **[L]**.

**Importance score** (0–11) = V + R + D + F. Effort (S / M / L) is kept apart and only breaks ties (S first).

| part | 0 | 1 | 2 | 3 |
|---|---|---|---|---|
| **V** player-visible | not seen | minor, side by side only | seen in places | core play or progression on many levels |
| **R** reach (census created instances, else levels) | < 10 inst or 1 level | 10–99 or 2–4 levels | 100–499 or 5–9 levels | ≥ 500 or ≥ 10 levels |
| **D** dependents in this graph | none | 1–2 | 3–4 | ≥ 5 |
| **F** fidelity risk | none | known deviation | wrong behaviour on several levels | — |

**Order.**
- The order is topological: a wave holds only items whose prerequisites are all in earlier waves.
- Within a wave, items are ranked by score.
- A wave has at most **3 lanes**, one per concurrent agent. The lanes don't share files; file areas are marked [L]
  where they are inferred.
- A lane is a queue: its items run one after another.
- Quick wins go into the lane of the item they unblock.
- Deferred items are not scheduled (§5).

**Census caveat.**
- The census numbers below are the 2026-09-28 re-run (4,030 created instances in 417 units; cheap 197 / 1,308)
  unless a row says "2026-09-29".
- **W0 re-ran it on 2026-09-29** (class_census.md "Re-run 2026-09-29"): the family is unchanged (4,030 / 417), the
  stale tags are fixed (help, the splash and burn sparks, the Suck Cannon interface, the move-collide, the walk-to
  states, the ammo read, `PromptRelease`), and the census now sees direct reads of engine state (branches on the hero
  state / body, the ship moby, the cheat flag) plus two overrides (the fighters, the 838 / 855 emitters). Verdicts:
  cheap **208 / 1,177**, missing 163 / 2,594, partly 40 / 251, unknown 6 / 8; **ready now 243 units / 1,653**
  (cheap + conditional-only + a dead hero-state branch only). By system (units / instances): cheat 48 / 1,497
  (conditional), paths 31 / 1,108, hero states and bodies 68 / 880 (of which the branch tests: 14 / 169 on state
  0x32, 25 / 456 on the body), animation 42 / 422, creature copies 22 / 363, effect mobys 21 / 276, light flicker
  6 / 256, platforms 7 / 181, ship mode 2 / 112, particle types 14 / 90, break variant 5 / 87, camera 29 / 74, blob
  shadows 7 / 70, save bytes 3 / 45, level sound 4 / 41, save 38 / 39 (conditional), voice handoff 2 / 33, sprite
  helper 8 / 20. Sole blockers: paths 648 (17 units), hero states 297 (31), effect mobys 114 (12), creature 84 (5).
- What the re-run freed (`u_s` predicted about 40 units): the 21 help-only directors are cheap; of the 11 enemy
  units, 568 is cheap and the other ten are conditional on the cheat (and most also branch on the Giant Clank body:
  a dead branch to file); the 8 save-only directors are conditional. Not freed: the fighters (G-LVL-009) and
  838 / 855 (G-AUD-010), now overrides.

## 2. Dependency graph and scores

One row per item. Scores are V R D F = total; E = effort; W = wave (§4). "—" = no prerequisite in the sources.

### Housekeeping, measurement and memory items

| item | prerequisites (kind; evidence) | unblocks | V R D F = S | E | W |
|---|---|---|---|---|---|
| ~~**M-COMMIT**~~ commit the explosion audit: 12 files uncommitted (`bomb.rs`, `creature/fx.rs`, `react.rs`, `debris.rs`, `explosive_tank.rs` …; git status; mem:project-next-queue 2026-09-29) | **done**: commit 3646133 | — | easier: G-ENM-009, G-ENM-010, G-CLS-027. They edit `creature/` and `classes/`, and would otherwise mix into an unverified batch [L]. | 0 0 2 1 = 3 | S | 0 |
| **G-TOOL-017** census follow-ups: re-run and re-tag | — | easier: G-CLS-027 (unit ids renumber per run, `cen`) and every class wave. **W0 part done 2026-09-29** (re-tag, data census, 30 functions named; 13 untagged left). | 0 3 3 1 = 7 | S | ~~0~~ done; then after W2 / W3 / W4 |
| ~~**G-TRI-015**~~ census still tags the Suck Cannon interface | **done 2026-09-29** (re-tagged has: react.rs) | 568 cheap; the other ten enemy units conditional on the cheat | — | — | ~~0~~ done |
| **G-TOOL-013** `rc-trace` depends on `rc-game` | — | easier: G-TOOL-017 re-runs while class lanes edit `rc-game` mid-wave [L] | 0 0 1 0 = 1 | M [L] | 0 |
| ~~**DOC-STALE**~~ gaps.md "Stale notes in other docs" (12 entries) | **done 2026-09-29** (every entry fixed in its source doc / comment) | — | — | — | ~~0~~ done |
| ~~**G-TOOL-014**~~ review items and digest baselines | **done 2026-09-29** (NO_IDLE `596306d7…`, full `42ec9908…` recorded in gaps.md) | — | — | — | ~~0~~ done |
| ~~**G-TOOL-004**~~ tolerance rows into hardware_fidelity_layers.md | **done 2026-10-19** (all already in "Tolerances"; the commands applying them → G-TOOL-024) | — | — | — | ~~0~~ done |
| **G-TOOL-010** user steps. Move the `.mov` files out: `repo-checks` fails on them. Also: Ghidra project, the `extracted/` regenerate, Ghidra save. | The regenerate step is **easier** after G-EXT-006: the 19 `overlay.elf` references vanish on regenerate (mem:project-dev-tooling-reorg). | green commit runs | user | S | 0 (user) |
| **M-QS2** the Quick Select page's second-run frame is missing (mem:project-next-queue 2026-09-28) | — | — | 0 0 0 0 = 0 | S | 6 |
| **M-PINE** the user enables PINE in PCSX2 (hero_feel_pass.md §1, §5) | — | depends: G-HERO-001 [deferred]; live traces for G-TOOL-003 [L] | user | S | user |
| ~~M-PACK~~ Clank pack-swap check | **resolved 2026-09-29**: the user saw in PCSX2 that the original snaps like the port. No gaps row carries it (G-TOOL-018 covers only the plume and the after-images). | — | — | — | — |

### Hero (HERO) and level generalisation

| item | prerequisites (kind; evidence) | unblocks | V R D F = S | E | W |
|---|---|---|---|---|---|
| **G-LVL-002** level-branch leftovers: H1 (the hero's done; the engine's: 0x2aba68, 0x2a4080, 0x255958, 0x28bf50), ~~**H2**~~ (**done 2026-09-29**, W1 lane 3: level_generalisation.md H2; `PASS_SURFACE` and Kerwan's cable reach ported), C4, C5, the level-13 branches | ~~easier G-TOOL-008 X1~~ (done). The landing part **depends** G-LVL-001 (gaps). The engine H1 branches **depend** G-HERO-005 (level 0xf, Giant Clank), G-LVL-009 (8 / 0xc, state 0x32), G-LVL-001 (0x509) (gaps, H2 2026-09-29). | based-on: G-HERO-002 (`cen` herostate row: "level copies … H2"). Easier: G-HERO-001 [L]. | 2 3 2 2 = 9 | M | 1 (H1 / H2); C4 / C5 in 4 |
| **G-TOOL-008** Ghidra tooling (~~X1~~ done 2026-09-29: `rc-trace overlay-diff`; then the VU disassembler, merged scripts, struct types) | — | easier: G-LVL-002 (X1, done). **reuses** by every later "does level N differ" question: G-HERO-031, G-HERO-002, G-LVL-002 C4 (docs/workflows/ghidra.md) | 0 0 1 1 = 2 | S (X1) / M | 1 (X1), 8 |
| ~~**G-HERO-002**~~ scripted hero states (**closed 2026-09-29**: 0x1f / 0x32 / 0x78 ported, hero_states.md "Scripted control"; the level `SetState` copies re-tagged has; idle per body → G-HERO-005, 0x38..0x3a → G-WPN-006, the vehicle word → G-LVL-009) | ~~based-on G-LVL-002 H2~~ (H2 done 2026-09-29: the level `SetState` copies are case sets of the superset, the shared cases unchanged; what is left here is the scripted states and the idle per body, **based-on** G-HERO-005) | depends: G-REN-008 (Gemlik 0x32, gaps) and G-CLS-003 (615 U86 herostate, `cen`). based-on: G-LVL-009 (state 0x32). 2026-09-29: `herostate` 68 units / 880 instances with G-HERO-005's bodies, sole blocker of 31 / 297 (`cen`; 14 units / 169 only branch on state 0x32). **Edges added 2026-09-29:** **enables** the class ports it unblocked (440 turret, 587, 336, 347, 1150 / 1151, 1178, 1772, 263–267, 361, 388: G-CLS-027; ported with it 1039 and 438); G-WPN-006 **absorbs** 0x38..0x3a; G-HERO-005 **absorbs** the per-body idle; G-LVL-009 **absorbs** the vehicle record 0x140940 (readers: 1039, 212, 1271, 1772, Gemlik's water pause); G-SAV-007 **gains** 438's turret skill point. | done | M | 2 |
| **G-HERO-031** superset audit: the port against level 00's copies of the 31 hero functions it took from level 01 (2026-09-29) | **reuses** G-TOOL-008's `overlay-diff` (the L00 column) | easier: G-HERO-001 [L] (feel on the surface-7 / Magneboots levels) | 2 2 1 1 = 6 | M | 3 |
| **G-HERO-027** the level camera system: camera records, regions, collision grid — **2026-09-30 (W3 lane 3, player_controller.md §15): the system and class 17 done; the grid is empty on every level (nothing to port)**; **2026-10-01: classes 7 (Swingshot), 23 (placed view), 18 (moby focus) done**; left: the camera classes 1, 3, 8, 14, 19..21 (22: giant Clank, G-HERO-005) | — | the Kerwan cables (done); every level's camera volumes (row). **Reuses** `moby_update::triggers` (the shapes) and `level_overlay::Relocation` (`CameraPorts`); **writes** the follow camera through its setters (shared with `0x3111d8`, the first-person turn; next: the mobys of G-HERO-026). The class-7 camera **depends** on the collision push `0x20f2a8` (G-TRI-004's function, now with a consumer) and co-needs G-HERO-021 (the Swingshot). The Novalis digest harness loads no records (a re-baseline is a human decision). 2026-10-01: the camera classes **read** the moby table and groups through `CamWorld` (fed by the tick; `MobySystem::group`) and the Swingshot targets' hint calls (`CinematicCall::LookHint`); the camera blocks **depend** on the loader's moby-link fixups (`cameras::remap_moby_links`, engine load) | 3 3 1 2 = 9 | L | 3 (classes: 5) |
| **G-HERO-026** follow and script camera settings from mobys — **2026-09-30: the setters ported, the census focus point is the ported `0x316e28`; camera census 29 units / 74 → 3 / 3 (`0x313b48`)** | reuses G-HERO-027 (done: the setters) | depends: G-CLS-003 (gaps). Co-needed by 14 help units and 17 herostate units (`u_s`). The moby → camera call channel is **needed by** 701 (lane-2 file `props.rs`) and the census units. The focus moby / look-at hint **reads** the Swingshot targets (`swing_target.rs`). | 2 2 2 1 = 7 | M | 3 |
| **G-HERO-009** joint-modifier producers; NPC look-at `0x2777d8` (40 units) | **Mostly done 2026-09-30 (W3 lane 1)**: the shared system `moby_update::manip` (moby_animation.md §9) **used by** the vendor 11 / logo 1143, the NPC 774, 823, 481, the Visibomb fins (G-WPN-004) and **reused by** Clank's eyelids (G-REN-005, the same node shape); it **unblocks** U540 347 and U197 447 / 920 (cheap now) and **eases** G-SAV-006's big-head manipulators (`0x278720`: attach + scale on a record, the same calls). The NPCs behind the cheat / save stay there. Census `anim` 42 units / 422 → 4 / 65. Left: the hero rows in gaps.md. | 42 units / 422 instances; sole blocker of 15 units / 90 (`u_s`). reuses: G-REN-005 (`0x2278c0`), G-WPN-004 fins (`0x221e38`, WPN-004 row), G-UI-006 logo draw (HERO-009 row). | 2 2 3 1 = 8 | M | 3 |
| **G-HERO-005** other bodies: Clank, Giant Clank (the Hologuise body 3 goes with G-WPN-006); Clank's health; the Giant Clank energy branch; the L15 / L18 missile of states 0x5d / 0x5f (G-TRI-016, resolved) | **based-on** G-HERO-002: the body switch `0x231348` and idle per body sit in the same census system (`cen`). The circular pair with G-WPN-006 is resolved (§7): nothing here depends on the gadget. | Clank on 8 levels, Giant Clank on 8 (row). depends: G-WPN-006's Hologuise row (the body switch). 2026-09-29: 25 census units / 456 instances branch on the body (`cen`). | 3 3 2 1 = 9 | L | 4 |
| **G-HERO-029** gravity-frame modes 1 / 2 | — | depends: G-HERO-008 (gaps) | 2 1 1 1 = 5 | M | 5 |
| **G-HERO-008** Hoverboard | **depends** G-HERO-029 (gaps) | depends: G-LVL-007 (gaps) | 3 1 1 0 = 5 | M | 5 |
| **G-HERO-025** hand-item slot leftovers | **depends** G-WPN-006 (gaps) | — | 2 1 0 1 = 4 | S | 5 |
| **G-HERO-016** hero feedback call sites | — | — | 2 2 0 1 = 5 | S | 7 |
| ~~**G-HERO-022**~~ moby collision leftovers | **done 2026-10-19** (gaps.md G-HERO-022; leftovers G-HERO-045) | — | — | — | ~~7~~ done |
| **G-HERO-018** surface-reaction leftovers | — | — | 1 1 0 1 = 3 | S | 7 |
| **G-HERO-020** rail leftovers | **depends** the arming classes (gaps → G-CLS-001) | — | 2 1 0 0 = 3 | M | 7 |
| **G-HERO-021** look-stance aiming beams | — | — | 2 1 0 0 = 3 | S | 7 |
| ~~**G-HERO-028**~~ tick-counter parity | **done 2026-10-19** (player_controller.md §0 "The tick counter in the hero update") | — | — | — | ~~7~~ done |
| **G-HERO-030** after-image leftovers | — | — | 1 0 0 0 = 1 | S | 7 |

### Weapons and gadgets (WPN)

| item | prerequisites (kind; evidence) | unblocks | V R D F = S | E | W |
|---|---|---|---|---|---|
| **G-WPN-006** utility gadgets: Hydrodisplacer, Trespasser, Metal Detector, Hologuise, PDA | **depends** G-UI-011 element 7 and G-HERO-005 body 3 (gaps) | depends: G-CLS-003, G-HERO-025, G-UI-006 PDA (gaps). The Hologuise row of G-HERO-005. | 3 2 3 0 = 8 | M | 5 |
| **G-WPN-008** target readers left: the wrench 0x13 / 0x14 / 0x15 search, `0x30d308`, platform previews | The previews are **based-on** G-CLS-024 [L] (0x13f64c moving platforms). | — | 2 1 0 1 = 4 | S | 2 (wrench); 7 (previews) |
| **G-WPN-011** weapon arm-layer leftovers | — | — | 2 1 0 1 = 4 | M | 7 |
| ~~**G-WPN-004** leftovers~~ (done 2026-09-30, W3 lane 1: the rebound 0x21 **reuses** 0x7a's physics and transitions and **depends on** the targets' records `targeting::record` (flag 2); the fins **reuse** `manip`) | The fins **reuse** G-HERO-009 (`0x221e38`, row). | — | 1 1 0 1 = 3 | S | 3 |
| **G-WPN-002** leftovers: the Gadgets page's Drone Device path, class 0x4d6, the held mine's frame; census `0x249530` | — | Kerwan's director 1342 (`cen`: "Drone Device path … 1342 (L03)") | 1 0 1 0 = 2 | S | 2 |
| **G-WPN-009** gold weapons | **depends** G-SAV-002 [deferred] (gaps) | — | 2 1 0 0 = 3 | M | blocked (§5) |

### Enemies and moby classes (ENM, CLS)

| item | prerequisites (kind; evidence) | unblocks | V R D F = S | E | W |
|---|---|---|---|---|---|
| **G-CLS-001** umbrella: the unported class family, 4,030 created in 417 units (re-run) | **based-on** every census system (`cen`) | the rows below | 3 3 3 1 = 10 | — | 1–7 |
| **G-CLS-025** path helpers not linked on level 01 | **Mostly done 2026-09-29 (W1 lane 1)**: `rc-game/src/path.rs` (4 of 6 jobs; `0x277260` and `0x265b38` left), units 1564 / 257 / 1667–1671 ported (203 instances). Census after: `path` blocks 1 unit / 2 (947) | Was 31 units / 1,108 (`cen`). The freed units now sit under their other blockers: **G-SAV-006** (827, 749, 1048, 1262 …), **G-REN-024** (580), **G-ENM-009** (238, 294, 1906: creature calls in their own code), **G-CLS-028** + **G-CLS-026** (level 03's nine-class family 246: its private explosion `0x24ce98` and exhaust moby 235 — **not cheap** although the census says so), the draw callbacks (857, 1212 / 1213) → G-ENM-001 / 010 | 3 3 3 1 = 10 (open part: 1 1 1 0 = 3) | S left | 1 (done) |
| **G-CLS-028** level-private explosion variants (level03 `0x24ce98`) | **based-on** the beam-explosion family (`fx::beam_explosion`, ported); **depends** G-CLS-026 for the family's exhaust moby 235 (`0x2bad40`) | level 03's air-traffic family 75 / 115–120 / 132 / 795 (246 instances; `classes::flyer`'s path driver is reused) | 3 2 1 1 = 7 | M | 2 |
| ~~**G-ENM-009**~~ creature-layer copies | **done 2026-09-29 (W1 lane 2; creatures.md §9)**: two of the three census functions were level copies of ported code (`0x274b78`, `0x26de80`); the wander `0x261630` and the joint hit `0x2599e8` / `0x26e830` are ported (`walker::wander`, `attack::joint_hit`); units 749 (106) and 1023 (40) ported | Census after: `creature` blocks no unit. Freed → G-ENM-001 / 010 (only the cheat left, conditional): 340 (04: 28), 333 (08: 8), 217 (04: 5), 578 (03: 3). New **reuses** edges: 340 → `walker::wander` (+ its level04 Suck Cannon table: **reuses** `react::tables_from_overlays` / `OTHER_REFS`); 857 (06, 10) and 238 (12) → `walker::wander`; the 12 census units that call `0x26e830` → `attack::sphere_hit`; 749 → `floor_switch::press` (shared with 830) | 3 2 2 1 = 8 (open part: 0) | M | 1 (done) |
| **G-HERO-033** the hero capsule's contact moby (0x13f58c / 0x13f590) | **blocks** 568's contact blast (18) and 621's (08) (both read the word; the port's hero never fills it). **reuses** the capsule resolve already ported (the moby pass just has to store its moby) | W2 lane 1 (2026-09-29): creatures.md §10 | 1 1 0 0 = 2 | S | W4 (`hero/**`) |
| ~~**G-PRT-007**~~ particle type 68 (**done 2026-09-29**: `particles::type68`) | — | 63's flight ribbon lives and draws | 1 1 0 0 = 2 | S | W6 (particles) |
| **W2 lane 1 part 1** (2026-09-29, done) | 1271 (13) **depends** 63 (its group 20 is 63's ground pack); 29 (13) **depends** its rider 36 and shot 1238 (created by code, unported); 568 (18) **depends** the boss 1422 (its only thrower, `0x2d5cf8`; unported, G-ENM-001); 193 **reuses** `path::toward` (`0x294cb0` = L15 `0x265b38`; also 63's walkways) and **stops at** G-HERO-005; 193 / 63 / 1445 / 252 / 568 **reuse** the reaction tables (matched by slots 0 / 2 / 3 now, the classes' own sequence tables through the record's +0x70: `react::SEQ_TABLES`); 252 **reuses** the unit draw callbacks (`UnitGlow`, `UnitQuads` + `register_with_matrix`) | creatures.md §10 | — | — | done |
| **W3 enemy units** (2026-09-29, done) | 1196 (10) **wakes** 1202's moby groups (the path nodes' w → its +0x160 mobys' groups, state 1 → 2); 1202 **depends** 857 (Clank's gadgebots: its other target class, unported) and **stops at** G-HERO-005 (body 1), G-HERO-033 (the capsule contact range), G-SAV-007 (0x13d419); 1196 **reads** the Orxon help director 1344's level word (G-SAV-007: 0x13d418); 1196 / 1199 **share** one targeting half (`orxon_flyers::targeting`, the cuboid lane as the data row); 29 **creates** 36 and 1238; 1238 **reads** the drones' slots (0x141346 / 0x141370, `classes::drone`) and **reuses** the Blaster shot's impact sparks (`blaster_shot::impact_sparks`) and type 51; 1202 **reuses** the tank's scorch (`explosive_tank::scorch`); `turn::spring` now returns the game's value (1196 / 1199's rise) | creatures.md §11 | — | — | done |
| **G-CLS-001·L** per-level logic classes: help, story and mission directors, cutaway stagers (LS §3; gaps top-15 item 5) | **Part 1** (29 units: 21 help-only + 8 save-only, `u_s`) **reuses** G-UI-017 (ported) and is **easier** after the G-TOOL-017 re-tag. **Part 2 is based-on** G-HERO-026 (14 units co-need it), G-HERO-002 (14) and G-HERO-009 (7, `u_s`). Both parts are **based-on** G-SAV-010 (78 flag classes, LS §3 "f"). `memcard_Save` and `OpenShipMenu` stay logged: G-SAV-002 and G-LVL-001 are deferred. **2026-09-29 (W2 lane 2, LS §8)**: part 1's help directors ported (11: 1413, 1324, 1342, 1343, 1347, 1348, 1349, 1000, 1344, 422, 558) **reusing** G-UI-017 and G-SAV-010 (flag writes) and `units/hints.rs`; their skill points are **based-on** G-SAV-007 [deferred], 1347's pad activation on G-SAV-003, 558's airless flag on G-HERO-032. The 8 save-only ones **depend** G-CLS-029 (new: direct item / bolt / health writes) besides G-SAV-002 (logged); 439 / 1469 on G-HERO-002 / 005. | depends: G-UI-001 (gaps). Consumers of G-UI-017 and G-SAV-010; G-LVL-002 C5. The writers of G-SAV-009 [deferred]. | 3 3 2 1 = 9 | L | 2 (part 1), 4 (part 2) |
| **G-UI-017** help-message consumers (the system is ported) | **depends** G-CLS-001 (gaps) | the help hints on every level (2026-09-29: no census unit is blocked by the system itself any more) | 3 3 1 0 = 7 | — | with G-CLS-001·L |
| **G-ENM-010** hit readers without a class port (2,874 instances) | **depends** G-ENM-001, G-CLS-001 (gaps). Ready now (2026-09-29): 568 is cheap; 193, 252, 63, 1202, 52, 1445, 1199, 29, 1196, 1271 need only the cheat (conditional) and, most of them, branch on the Giant Clank body (a dead branch to file under G-HERO-005) (`cen`). | — | 3 3 0 2 = 8 | L | 2 |
| **G-ENM-001** enemy and creature classes | **based-on** G-ENM-009 and G-CLS-025 (`cen`). **depends** G-PRT-001 (type 74) and G-CLS-018 (gaps). The Sonic Summoner **reuses** G-REN-023 and G-REN-025 (row). | depends: G-ENM-010 (gaps) | 3 3 1 1 = 8 | L | 2 (ready units), 7 (Summoner) |
| **G-CLS-027** cheap wins round 2 (mem:project-next-queue "cheap wins round 2") | **easier** G-TOOL-017 (done). reuses `units::PORTS` (parts A / B). The fighters (→ G-LVL-009) and 838 / 855 (→ G-AUD-010) are census overrides now, out of the list. **Round 2 done 2026-09-29** (all levels, largest first: 13 unit rows / 253 created; `cen` "In the port: cheap wins round 2"). New edges: 408 **reuses** the drones 77's release (one family) and is **called by** the guards 44 (`set_off`, unported); 123 / 99 lasers **reuse** `line_hit_in`; 1038 **reuses** the gold bolt's item glow (third consumer); 127… **reuse** `linked_mover::link_state`; 1172 **reuses** the resolver, flash and `SetDeathBits`; 408's tint → **G-REN-030**; 468 / 469 → **G-AUD-011**; 133 → G-HERO-008; 933 → G-HERO-033; 1143 / 823 → G-HERO-009; 1378 → G-HERO-032; 939 → its created 938 (G-CLS-001). | 2026-09-29: 208 units / 1,177 cheap; **243 / 1,653 ready** with the conditional and dead-branch units (`cen` "Cheap wins") | 3 3 0 0 = 6 | L | 2 (levels 00–08), 4 (09–18) |
| ~~**G-CLS-018**~~ (done 2026-10-01; left: G-CLS-030 the death reload) spawn conditions and inactive pools | — | depends: G-ENM-001 (gaps) | 2 3 1 2 = 8 | M | 4 |
| ~~**G-CLS-023**~~ (done 2026-10-01) moby group commands and re-creation | — | depends: G-CLS-010, G-REN-008 (gaps) | 3 2 2 1 = 8 | M | 4 |
| ~~**G-CLS-024**~~ moby platforms and riders | **done 2026-09-30 (W3 lane 2; triggers.md §5b)**: the moby side of the carry (`triggers::carried` / `to_local` / `from_local` / `carry_riders` with the turn, `record_children` / `place_children`); **reuses** `hero::platform::Carrier`; consumers bolts, crates, `BoltBurst` flag 4, the bolt colour, the infobot ride, units 707 / 734, 1210, 812, 1381, 1584. New edges: the weapon riders (bombs, mines, decoys, Doom bots) **reuse** it (open, G-CLS-024 consumers); the follow camera's `CameraCollLineFix` **reuses** it; 1381 **places** 1584 (the Veldin carriers) and **is driven by** the boss 1422; `hero/platform.rs` holds a copy of the same algebra (merge: lane 1) | depends: G-CLS-009 (gaps). 1246 (126) together with G-REN-024 (`u_s`). based-on: G-WPN-008 previews [L]. Sole blocker of 3 units / 36. | 2 2 2 1 = 7 | M | 3 |
| ~~**G-CLS-026**~~ effect-moby spawners | **done 2026-09-30 (W3 lane 2; creatures.md §12)**: not a system: the 9-level cluster is `BreakFxB` (ported; **reuses** `fx::break_piece_with`, now with velocity / spin / sphere); the rest are family code of their callers. Consumer 1511 ported; the freed units go to G-CLS-027 round 3 | 2026-09-29: 21 units / 276 (three L08 / L09 spawners newly tagged); sole blocker of 12 / 114 (`cen`) | 2 2 1 0 = 5 | M | 3 |
| ~~**G-CLS-010**~~ (done 2026-10-01, with Pokitaru's 318) teleporter pad 1135 (8 levels) | **depends** G-CLS-023 (gaps) | — | 3 2 0 0 = 5 | M | 4 |
| **G-CLS-019** moby runtime leftovers | — | — | 1 3 0 1 = 5 | M | 4 |
| **G-CLS-003** gadget puzzle classes: Trespasser 615, Hydrodisplacer pads 341 | **depends** G-WPN-006, G-HERO-026, G-REN-008 (gaps). **based-on** G-REN-024 and G-HERO-002 (615 in the light and herostate lists, `cen`). | progression on 7 + 5 levels | 3 1 0 0 = 4 | M | 6 |
| **G-CLS-005** Novalis world classes: 613, 695, 1504, 1848 | 695 **reuses** move-collide `0x26d270` and `SetWaterLevel` (ported: gaps ENM-009 row, LG W4) | — | 2 0 0 1 = 3 | M | 5 |
| ~~**G-CLS-009**~~ multi-hit crates | **closed 2026-09-30**: dead code (no level's class table has 503–510); no consumer | — | 2 1 0 0 = 3 | S | 3 |
| **G-CLS-014** breakables outside the general system, break variant `0x251f08` | reuses breakables (ported) | 5 units / 87 | 2 1 0 0 = 3 | M | 6 |
| **G-CLS-015** flyer path driver family | **based-on** G-PRT-001 type 10 (PRT row). The skill-point part **depends** G-SAV-007 [deferred] (gaps). | — | 2 1 0 0 = 3 | M | 7 |
| ~~**G-CLS-011** bolt-crank consumers (Eudora, Batalia)~~ closed 2026-10-20 (all ported) | reuses the bolt crank (ported) | — | 2 1 0 0 = 3 | S | 7 |
| **G-CLS-013** talk-table kinds 4 / 5 | — | — | 2 0 0 0 = 2 | S | 7 |
| ~~**G-ENM-005**~~ creature fx helpers: census residue | **resolved 2026-09-29**: the splash and burn-spark tags are has; no unit is blocked | — | — | — | — |
| ~~**G-LVL-008**~~ level height grid `0x278020` (**done 2026-09-29**: `HeightGrid`) | — | 1400 (3 instances), which still **depends** G-PRT-001 (type 01, `SpawnImpactSparks`) | 1 0 0 0 = 1 | S | 2 |

### Rendering and particles (REN, PRT)

| item | prerequisites (kind; evidence) | unblocks | V R D F = S | E | W |
|---|---|---|---|---|---|
| **G-PRT-001** particle types (**2026-09-29: 28 ported / rows, 19 left**: 40, 58, 71, 42 + 15 with no spawner) | — | the classes' particle side is ready: G-CLS-015 (type 10), G-ENM-001 (74), 1400 (0, 1, 73, `SpawnImpactSparks`; G-LVL-008's consumer), U412, U397, U134, U436, U33, U439, U458, U34, U203, U371, U254 (61). **depends** G-PRT-008 for 40 / 58 (U323, U254). The Morph-o-Ray's type-78 call **depends** a hero seam (`hero::fx::PartSpawn` + the gravity hook) | 2 1 2 1 = 6 | M | 6 |
| **G-PRT-008** particle-delivered hits (new) | — (a queue drained into `World::deliver_hit` after `UpdateParts`) | types 40 and 58 → U323 (702, L10), U254 (1106, L07) | 1 1 1 0 = 3 | S | 7 |
| **G-REN-004** hero light cross-fade `0x26be04` | — | — | 2 3 0 1 = 6 | S | 7 |
| ~~**G-REN-024**~~ point-light flicker `0x25fb60` | **closed 2026-09-30 (creatures.md §12)**: it is the creature hit flash L01 0x2723f8 (ported), not a light; its consumers 580 / 612 / 1231 / 1246 / 668 are class ports (G-CLS-027 round 3; the cheat is their only other blocker, 668 is cheap) | 6 units / 256; with platforms 1246, with paths 580 (`u_s`). based-on: G-CLS-003 (615). | 2 2 1 0 = 5 | S | 3 |
| **G-REN-005** attachment leftovers (Clank's pulse and antenna glow; **done 2026-09-30, W3 lane 1**, left: the antenna's red dot quad `0x229440`, Clank as the hero → G-HERO-005) | reuses G-HERO-009 (`0x2278c0`, both rows) | — | 2 2 0 1 = 5 | M | 3 |
| **G-REN-008** water-manager leftovers | **depends** G-CLS-023, G-LVL-009 (the vehicle word 0x140940; ~~G-HERO-002~~ done 2026-09-29) (gaps) | depends: G-CLS-003 (Hydrodisplacer pads) | 2 1 1 1 = 5 | M | 5 |
| **G-REN-027** underwater test for draw-callback liquids | easier G-REN-026 [L] | — | 2 2 0 1 = 5 | M | 5 |
| **G-REN-025** blob shadows | — | 7 units / 70. reuses: G-ENM-001 (Summoner); mines, drones, the Visibomb missile (row). | 2 1 1 1 = 5 | M | 6 |
| **G-REN-012** tie mirror pass | — | — | 2 2 0 1 = 5 | M | 7 |
| **G-REN-013** GS / VU render-rule leftovers | overlaps G-TOOL-016 [L] | — | 1 3 0 1 = 5 | L | 7 |
| **G-REN-019** shadow RGB-only alpha-test halves | — | — | 1 3 0 1 = 5 | M | 7 |
| **G-PRT-005** particle renderer deviations | — | — | 1 3 0 1 = 5 | M | 7 |
| **G-REN-026** liquid meshes: 09 lava, 02, 12's 293, 14, 15 | reuses `sea_render` | easier: G-REN-027 [L] | 2 2 0 0 = 4 | M | 5 |
| **G-REN-023** glow-quad callers | reuses `fx_draw::glow_quad` | reuses: G-ENM-001 (Summoner) | 2 1 1 0 = 4 | S | 6 |
| **G-REN-018** moby render leftovers | — | — | 1 2 0 1 = 4 | M | 7 |
| **G-REN-020** draw-callback leftovers, `DrawSpriteHelper_A` | — 2026-09-29: the census part closed (`DrawSpriteHelper_A` = VU1 set-up, re-tagged has); its 4 units / 16 are G-CLS-027 now. | sole blocker of 8 units / 20 (`u_s`) | 1 1 0 1 = 3 | S | 2 |
| **G-REN-028** display-blend draw order | — | — | 1 1 0 1 = 3 | M | 7 |
| **G-PRT-002** type-6 collision branch | — | — | 1 1 0 1 = 3 | S | 7 |
| **G-REN-029** render-distance globals | — | — | 1 0 0 1 = 2 | S | 7 |

### Audio, cutscenes, UI, saves, levels

| item | prerequisites (kind; evidence) | unblocks | V R D F = S | E | W |
|---|---|---|---|---|---|
| **G-UI-011** HUD slot leftovers | The race timer **depends** G-LVL-007, the boss meters the boss classes (gaps; circular with G-LVL-007, §7). | depends: G-WPN-006 (element 7), G-LVL-007 (gaps) | 2 2 2 0 = 6 | M | 5 (race timer in 6) |
| **G-UI-002** pause-page leftovers | The save pages **depend** G-SAV-002; the Cheats page is the page of G-SAV-006 (gaps). | depends: G-CUT-003, G-SAV-006 (gaps) | 2 1 2 1 = 6 | M | 6 |
| **G-SAV-010** global-flag class write channel: move the consumers | reuses `interact::set_global_flag` | based-on: G-CLS-001·L (78 classes); G-SAV-003's crank flags | 2 2 1 1 = 6 | S | 2 |
| **G-UI-001** missions page; zone-flag writers | **depends** G-CLS-001·L (gaps) | — | 2 2 0 1 = 5 | M | 6 |
| **G-LVL-007** races | **depends** G-HERO-008, G-UI-011 (gaps) | — | 3 1 0 0 = 4 | M | 6 |
| ~~**G-AUD-010**~~ voice handoff between mobys (**done 2026-09-29**: `SoundSink::hand_over`; 838 ported) | — | depends: 838 / 855 (33 instances; census overrides `X:838` / `X:855` since 2026-09-29) | 2 1 1 0 = 4 | S | 2 |
| **G-AUD-005** music player leftovers | — | — | 2 1 0 1 = 4 | S | 7 |
| **G-UI-006** vendor leftovers | The PDA **depends** G-WPN-006 (gaps); the logo **reuses** G-HERO-009. | — | 2 1 0 1 = 4 | M | 7 |
| **G-UI-010** screen static in linear light | — | — | 1 2 0 1 = 4 | S | 6 |
| **G-AUD-007** sound fidelity; the level-04 slot sound `0x27eca0` | — 2026-09-29: `0x27eca0` is `PlayLevelSoundAtMoby`'s L04 copy (has): the 466 family is cheap; its skill point **based-on** G-SAV-007. | sole blocker of U145 (38 instances, `u_s`) | 1 1 0 1 = 3 | M | 2 (`0x27eca0`), 7 |
| **G-AUD-009** one dialogue-player model | — | — | 1 1 0 1 = 3 | M | 7 |
| **G-CUT-005** scene-player leftovers | — | — | 1 1 0 1 = 3 | M | 7 |
| **G-CUT-003** front-end movies | **depends** G-UI-002, G-SAV-001 [deferred] (gaps) | depends: G-UI-004 | 2 1 1 0 = 4 | M | blocked |
| **G-CUT-004** other scene triggers | **depends** G-LVL-001 [deferred], G-UI-002 (gaps) | — | 2 1 0 0 = 3 | M | blocked |
| **G-UI-004** menu post-actions, leaving the level | **depends** G-LVL-001 [deferred], G-CUT-003 (gaps) | — | 2 1 0 0 = 3 | M | blocked |
| **G-LVL-009** ship combat (fighters 1843 / 1319: 112, census overrides; the asteroids' wake 130; 14 more units only branch on state 0x32) | **depends** G-LVL-001 [deferred] (gaps); **based-on** G-HERO-002 (state 0x32, row) | — | 3 2 0 0 = 5 | M | blocked |

### Launcher, extractor, tooling (non-deferred)

| item | prerequisites | unblocks | V R D F = S | E | W |
|---|---|---|---|---|---|
| **G-EXT-006** `export --what code`, `verify --strict` | — | easier: G-TOOL-010's regenerate and G-TOOL-008's re-import (mem:project-dev-tooling-reorg) | 0 0 1 0 = 1 | M | 8 |
| **G-TOOL-009** test-harness follow-ups | — | — | 0 0 0 1 = 1 | S | 8 |
| **G-TOOL-011** memory-card import | reuses the save codec (ported) [L] | — | 0 0 0 0 = 0 | M | 8 |
| **G-TOOL-015** dev default camera | — | — | 0 0 0 0 = 0 | S | 8 |
| **G-EXT-002** uncached parser decompressions | — | — | 0 0 0 0 = 0 | S | 8 |
| **G-EXT-005** Stage 1 leftovers | — | — | 0 0 0 0 = 0 | S | 8 |
| **G-LCH-002** Stop button; **G-LCH-003** Windows / Linux; **G-LCH-006** art; **G-LCH-007** `--game` / `--profile` / logs | LCH-007 makes G-MOD-003 profiles **easier** [L] (mods.md §4 q5) | — | 1 / 0 / 1 / 0 | S / M / S / S | 8 (launcher repo) |
| **G-TOOL-003, G-TOOL-005, G-TOOL-007, G-TOOL-012, G-TOOL-018** PCSX2 comparisons: **user-gated** | depend on the user's captures or savestates (gaps). G-TOOL-018 = the Thruster plume brightness and the after-images, still open. | feed G-TOOL-016 [deferred] | 0–1 | — | user, any time |
| **G-TOOL-006** the `rand` gap | depends on a second savestate (gaps) | — | 0 1 0 1 = 2 | M | user-gated |

## 3. Importance ranking (score, then effort)

| score | items |
|---|---|
| 11 | ~~G-HERO-002~~ (closed 2026-09-29; `herostate` 786 → 487 created instances, the rest the bodies G-HERO-005) |
| 10 | G-CLS-025 (G-CLS-001 umbrella) |
| 9 | G-LVL-002, G-HERO-027, G-HERO-005, G-CLS-001·L |
| 8 | ~~G-ENM-009~~ (done 2026-09-29), G-HERO-009, G-WPN-006, G-ENM-010, G-ENM-001, G-CLS-018, G-CLS-023 |
| 7 | G-TOOL-017, G-HERO-026, G-CLS-024, G-UI-017 |
| 6 | G-CLS-027, G-PRT-001, G-REN-004, G-UI-011, G-UI-002, G-SAV-010 |
| 5 | G-HERO-029, G-HERO-008, G-HERO-016, G-CLS-026, G-CLS-010, G-CLS-019, G-REN-024, G-REN-005, G-REN-008, G-REN-027, G-REN-025, G-REN-012, G-REN-013, G-REN-019, G-PRT-005, G-UI-001, G-LVL-009 (blocked) |
| ≤ 4 | the rest (§2) |

## 4. The ordered plan

Each wave is ≤ 3 lanes; lanes are queues and don't share files. After W2, W3 and W4, re-run the census
(G-TOOL-017) before the next class wave.

| wave | lane (file area [L]) | items, in order | why here |
|---|---|---|---|
| **W0** housekeeping — **done 2026-09-29** except the user steps and G-TOOL-013 (G-TOOL-004 done 2026-10-19) | A: git | ~~M-COMMIT~~ (3646133); then the user's G-TOOL-010 steps (open). Hold the `extracted/` regenerate until G-EXT-006, or accept losing the ELF reference. | Nothing else should build on 12 uncommitted files. |
| | B: `tools/trace`, `tools/ghidra/names`, class_census.md, gaps.md | ~~G-TOOL-017 re-run with the re-tag: G-TRI-015, help, `0x2ff768`, `0x271258`~~ (done, plus the data census and 30 named functions) → G-TOOL-013 (open: M effort, not started) | Every class wave's numbers and order depend on it; freed 21 help directors, 568 and the pushables 695; found the dead hero-state branches (§1). |
| | C: `docs/plan/*` except the two above | ~~DOC-STALE → G-TOOL-014~~ (done) → ~~G-TOOL-004~~ (done 2026-10-19) → the read-only triage checks ~~G-TRI-004 / 005 / 006 / 007 / 008 / 011~~ (closed, gaps.md) / 014 (open: 0x248920 not read) | Cheap, and it removes false leads before the ports. |
| **W1** the three biggest shared blockers | 1: new path module + its first consumers [L] | ~~**G-CLS-025**~~ (done 2026-09-29: `path.rs`, units 1564, 257, 1667–1671; left: `0x277260` for 947, `0x265b38`). L03's nine-class family 246 moved to **G-CLS-028** (+ G-CLS-026): its private code is an explosion and an effect moby, not paths | 1,108 instances; sole blocker of 692 (after: `path` blocks 2) |
| | 2: `moby_update/creature/` | ~~**G-ENM-009**~~ (done 2026-09-29: the wander and the joint hit ported, 749 and 1023 ported; 340, 333, 217, 578 freed for W2 lane 1) | 539 instances; pairs with paths for 141 more |
| | 3: `tools/ghidra`, then `hero/**` | ~~G-TOOL-008 X1 → **G-LVL-002** H2 diff + H1 branch transcription → port the differing hero functions~~ (done 2026-09-29: `rc-trace overlay-diff`; H2 found 2 per-level differences, both ported; the hero H1 branches were already in; new: G-HERO-031) | Ratchet's own code may differ on 10 levels (F2). Prerequisite of G-HERO-002. |
| **W2** class ports on W1 + hero states | 1: `classes/` (enemies) | **G-ENM-010 / G-ENM-001**: first the ready enemy units (~~568~~ cheap; ~~193, 252, 63~~, 1202, ~~52, 1445~~, 1199, 29, 1196, ~~1271~~: the cheat branch filed under G-SAV-006, the Giant Clank body branch under G-HERO-005), then the units W1 freed (creature + path). **Part 1 done 2026-09-29** (creatures.md §10): 568, 193, 252, 63, 52, 1445, 1271 (319 created instances); left 1202 / 1199 / 1196 (10) and 29 (13: + its rider 36 and shot 1238); new edges: 568 → the boss 1422 (its only thrower) and → G-HERO-033 (the capsule contact); 63 → G-PRT-007 (particle type 68); 193 → G-HERO-005 (the Giant Clank pack) | Enemies that ignore weapons on 18 levels (F2) |
| | 2: `classes/` (directors, `units/`), `audio/voices`, `fx_draw` | **G-CLS-001·L part 1** (the 21 help-only directors, now cheap, + the 8 save-only ones, conditional) + **G-SAV-010** consumers + Kerwan's 1342 (cheap: `0x249530` is the ammo read, ported) → quick wins G-AUD-010 (frees 838 / 855), G-AUD-007 `0x27eca0` (38), G-REN-020 (20), G-LVL-008 (3) → **G-CLS-027** round 2, levels 00–08 (pool 2026-09-29: 243 ready units / 1,653; **round 2 taken 2026-09-29 over all levels, largest first: 253 created ported, 223 cheap units / 1,365 left**) | The story and help flow of every level; quick wins next to what they free |
| | 3: `hero/**` | ~~**G-HERO-002**~~ (done 2026-09-29) → G-WPN-008 wrench aim search (S) | Needs H2; the largest shared blocker after the paths |
| **W3** second-tier shared systems | 1: hero joint modifiers, `moby_attach` [L] | **G-HERO-009** → G-REN-005 (Clank's pulse, same `0x2278c0`) → G-WPN-004 (fins reuse `0x221e38`; the rebound) | 422 instances; three reuse edges |
| | 2: moby services, `classes/` of the users [L] | ~~G-REN-024~~ (not a system: the ported hit flash) → ~~**G-CLS-024**~~ (done: triggers.md §5b; units 707 / 734, 1210, 812, 1381) → ~~G-CLS-009~~ (dead code) → ~~**G-CLS-026**~~ (not a system: `BreakFxB`; 1511 ported) (done 2026-09-30) | 256 + 181 + 265 instances |
| | 3: `follow_camera/**` | **G-HERO-027** → **G-HERO-026** | Every level's camera; co-needed by the directors |
| **W4** content on W2–W3 | 1: `hero/**` | **G-HERO-005** (Clank, Giant Clank; the Hologuise body waits for W5) → G-TRI-016 check | 8 + 8 levels |
| | 2: `classes/` | **G-CLS-001·L part 2** (the camera / herostate co-needers) + G-LVL-002 C4 / C5 → **G-CLS-027** round 3 (what round 2 left: the deferred large units 466-family, 857, 471, 1355, 99, and the appendix below 10 instances) | Needs W2's hero states and W3's camera |
| | 3: scheduler, spawn, `teleporter.rs`, `enemy_spawner.rs` [L] | **G-CLS-018** → **G-CLS-023** → G-CLS-010 → G-CLS-019 | Dormant pools on every level; spawners, teleporters, water raise and lower |
| **W5** gadgets, water, hoverboard | 1: `hud.rs`, `hero/gadgets.rs`, `menus/vendor` [L] | **G-UI-011** (element 7, oxygen, bolt alert, boss meters) → **G-WPN-006** (+ the Hologuise on W4's body switch) → G-HERO-025 | Gadgets gate progression |
| | 2: `water/**`, `sea_render` [L] | **G-REN-008** (needs G-CLS-023, G-HERO-002) → G-REN-026 → G-REN-027 → G-CLS-005 | Water leftovers; prerequisite of the Hydrodisplacer pads |
| | 3: hero physics (`ledge.rs`, `swim.rs`, packs) [L] | **G-HERO-029** → **G-HERO-008** | The Hoverboard, for the races |
| **W6** puzzles, races, particles, map | 1: `classes/` (puzzles, races), HUD race lines | **G-CLS-003** → **G-LVL-007** (+ G-UI-011's race timer) | All their prerequisites are now met |
| | 2: `particles/**`, fx draws [L] | ~~**G-PRT-001** (sole 78; types 10, 74, 5, 78)~~ (done 2026-09-29 but for 40 / 58 / 71: G-PRT-008) → G-REN-025 → G-REN-023 → G-CLS-014 → G-ENM-005 residue | Unblocks the flyers and the Summoner (W7) |
| | 3: `menus/**`, `map/**` | **G-UI-002** (non-save parts) → **G-UI-001** (needs W4's directors) → G-UI-010 → M-QS2 | |
| **W7** polish (no blockers left) | 1: `hero/**` | G-HERO-016 → G-WPN-011 → G-HERO-022 → G-HERO-018 → G-HERO-021 → G-WPN-008 previews → G-HERO-020 → G-HERO-028 → G-HERO-030 | Scores 5 … 1 |
| | 2: `rc-engine` render | G-REN-004 → G-REN-012 → G-REN-019 → G-PRT-005 → G-REN-013 → G-REN-018 → G-REN-028 → G-PRT-002 → G-REN-029 | |
| | 3: `classes/`, `audio/**`, scenes, vendor | G-CLS-015 + the Summoner part of G-ENM-001 (need W6's particles) → G-AUD-005 → G-UI-006 → G-AUD-009 → G-AUD-007 rest → G-CUT-005 → G-CLS-011 → G-CLS-013 | |
| **W8** tooling and launcher | 1: `crates/rc-extract`, `tools/**` | G-EXT-006 → G-TOOL-008 rest → G-TOOL-009 → G-TOOL-011 → G-TOOL-015 | G-EXT-006 before the user's `extracted/` regenerate |
| | 2: `rc-data` / loaders | G-EXT-002 → G-EXT-005 | |
| | 3: `rerac-launcher` repo | G-LCH-002 → G-LCH-007 → G-LCH-006 → G-LCH-003 | A separate repo: no file overlap, so it can fill a free slot in any wave |

## 5. Deferred items (not scheduled)

⚑ marks a deferred item that blocks a lot of non-deferred work (worth reconsidering).

| item | would slot | depends on | blocks |
|---|---|---|---|
| ⚑ **G-SAV-002** save / load | Any time (no prerequisite); naturally with W4, whose story directors call `memcard_Save` (38 units / 39 instances, conditional; `cen`) | — | Depends: G-WPN-009 (gold weapons), G-SAV-003 / 004 / 005, **G-LVL-001**, G-UI-002's save pages (gaps). easier: G-TOOL-011 [L]. With G-SAV-001: G-CUT-003 → G-UI-004. |
| ⚑ **G-LVL-001** planet travel, space scenes, transition movies | After G-SAV-002 (gaps) and W4 (the level-exit classes, `OpenShipMenu`: 6 units, `cen`) | G-SAV-002 | Depends: **G-LVL-009** (242 instances: fighters 112 + the asteroid wake 130), G-CUT-004, G-UI-004, G-EXT-004, G-LVL-002's landing part (gaps). Every level except the start is reachable only by a debug boot (gaps top 15). |
| **G-SAV-001** main menu and title flow | With G-SAV-002 | — | Depends: G-CUT-003 (gaps) |
| **G-SAV-003** save-chunk writers | After G-SAV-002 | G-SAV-002 | the death / visit bytes of 44 / 78 (45 instances, `cen` "death") |
| **G-SAV-004** challenge mode | After G-SAV-002 | G-SAV-002 | the even gold-weapon offers, teleporter 961's gate |
| **G-SAV-005** nanotech upgrades | After G-SAV-002 | G-SAV-002 | — |
| **G-SAV-006** cheats | After W6 (G-UI-002) | G-UI-002 (gaps) | Nothing in normal play. 45 units / 1,454 instances call the manipulator only with the cheat on (`cen`), so port those classes now and file the branch. |
| **G-SAV-007** skill points | After W7 (G-CLS-015) | — | G-CLS-015's skill-point part (gaps); Kerwan 1342's cuboid |
| **G-SAV-009** statistics | After W4 (the directors write the records, LS §3) | — | nothing visible |
| **G-HERO-001** hero feel pass | After W2: G-LVL-002 H2 and G-HERO-002 leave `hero/**` settled [L] | M-PINE + the user's recording (gaps) | — |
| **G-TOOL-001** profiling session | After W7 (render and particle changes settled) [L] | the user plays | Depends: G-EXT-003 → G-MOD-005 (gaps). G-TOOL-002's fixes. |
| **G-TOOL-002** performance fixes | After G-TOOL-001 | user decisions (profile changes, the `hud_render.rs` edit) | — |
| **G-TOOL-016** native Phase 2, and **G-REN-017** (on hold) | Last, after the PCSX2 checks (decisions.md) | G-TOOL-003 / 005 / 007 | — |
| **G-MOD-001, G-MOD-002, G-MOD-003, G-MOD-004, G-MOD-005, G-MOD-006, G-MOD-007** mods | Last (mods.md: "last stage"). Chain 001 → 002 → 003 / 004, 001 → 006, G-EXT-003 → 005 (gaps). | the user's six answers (G-MOD-001) | — |
| **G-EXT-003** Tier 1 v2+; **G-EXT-004** the unknown lumps | After G-TOOL-001; after G-LVL-001 (gaps) | the deferred items | G-MOD-005 |
| **G-LCH-001** Official feed; **G-LCH-004** signing; **G-LCH-005** other discs; **G-EXT-001** strip / LTO / deployment target | User-gated: repo public, Apple ID, "deferred, not dropped", a decision | the user | — |
| **M-GSPOST** rename `rc-engine/src/gs_post.rs` (the user: "rename later"; mem:project-next-queue 2026-09-28) | Any time, S | — | — |
| **M-REORG** dev tooling reorg | **Already done** (d41ec5e, mem:project-dev-tooling-reorg); its follow-ups are G-TOOL-008 (W1 / W8), G-TOOL-009 and G-EXT-006 (W8) | — | — |

**Blocked by deferred items** (not deferred themselves): G-WPN-009, G-CUT-003, G-CUT-004, G-UI-004, G-LVL-009,
G-EXT-003, G-EXT-004, and the census's level-exit (6) and save (39, conditional) calls.

**Reconsider:**
- **G-SAV-002** blocks 6 rows directly and 5 more through G-LVL-001.
- **G-LVL-001** holds all story flow between levels and ship combat.

Both lead the gaps.md top 15.

## 6. Top 20, in the order to do them

1. **M-COMMIT**: 12 uncommitted files in `creature/` and `classes/`, which the next three lanes edit.
2. ~~**G-TOOL-017 + G-TRI-015**~~: done 2026-09-29 (class_census.md "Re-run 2026-09-29").
3. ~~**G-CLS-025** path helpers: the largest real blocker (1,108 instances; sole 692)~~ done 2026-09-29 (W1 lane 1; census: `path` blocks 2). Next in its place: **G-CLS-028** (L03's air traffic 246: its own explosion + G-CLS-026's moby 235).
4. ~~**G-ENM-009** creature copies: 363 instances (2026-09-29; sole blocker of 84)~~ done 2026-09-29 (W1 lane 2): 146 instances ported, 44 freed (340, 333, 217, 578; cheat-conditional).
5. ~~**G-LVL-002 H2** (with X1): the hero code may differ on 10 levels; prerequisite of G-HERO-002.~~ Done 2026-09-29 (W1 lane 3). Follow-up: **G-HERO-031** superset audit (S–M, the list is generated).
6. **G-ENM-010 / G-ENM-001**: 11 enemy units / 407 instances are ready now (568 cheap, ten conditional on the cheat with a
   dead Giant Clank branch); enemies ignore weapons on 18 levels.
7. **G-CLS-001·L part 1 + G-SAV-010**: 29 directors are ready once help counts as ported; the story and hints of
   every level.
8. ~~**G-HERO-002** scripted states~~: done 2026-09-29 (0x1f / 0x32 / 0x78; 1039 and 438 ported; the unblocked units go to G-CLS-027).
9. **G-HERO-027 + G-HERO-026** camera: every level's camera volumes; co-needed by the directors.
10. **G-HERO-009** joint modifiers: 422 instances; reused by Clank's pulse, the Visibomb fins and the vendor logo.
11. **G-REN-024 + G-CLS-024**: 256 + 181 instances; together they free 1246 (126).
    Ported 2026-10-01 (creatures.md §13): 1246, 580 / 668, 612, 1231 / 1297; the 1246 boarders now wait on the boats 1075 (U363, a class port).
12. **G-CLS-026** effect-moby spawners: 276 instances (sole blocker of 114).
13. **G-HERO-005** Clank and Giant Clank: sections on 8 + 8 levels.
14. **G-CLS-027** cheap wins round 2: 1,177 instances of class-only work, 1,653 with the conditional and dead-branch units.
15. **G-CLS-001·L part 2**: the directors that need the camera and hero states.
16. **G-CLS-018 + G-CLS-023 (+ G-CLS-010)**: dormant pools, spawners, teleporters on 8 levels.
17. **G-UI-011** HUD slots: prerequisite of the gadgets and the races.
18. **G-WPN-006** utility gadgets: they gate progression; prerequisite of the puzzles.
19. **G-HERO-029 → G-HERO-008** Hoverboard: prerequisite of the races.
20. **G-CLS-003** Trespasser and Hydrodisplacer puzzles: the progression gates on 7 + 5 levels.

## 7. Needs triage, and contradictions found

**Too unclear to place:**

| item | source | where it would go if confirmed |
|---|---|---|
| G-TRI-001 `slab_allocator` use-after-free log lines | gaps | its own bug hunt; before W7's render lane |
| ~~G-TRI-004 script-camera push; G-TRI-005 `unknown_74`; G-TRI-006 shadow probe; G-TRI-007 `ripple_z`; G-TRI-008 ISO lumps; G-TRI-011 fog-zone camera~~ | checked 2026-09-29: closed in gaps.md (each with its finding) | — |
| G-TRI-009 Oltanis instances without geometry; G-TRI-010 mpegs 0, 1, 64–69; G-TRI-012 texture `type` enum; G-TRI-013 unknowns lists | gaps | no consumer yet (no-speculative-ports rule) |
| G-TRI-014 pad vibration | gaps | still open: the boot ELF links libpad's vibration (`sce_vib_*` at 0x1250d8…); `0x248920` not read |
| ~~G-TRI-016 the L15 / L18 missile from level hero code~~ | resolved 2026-09-29: Giant Clank's states 0x5d / 0x5f → a consumer of G-HERO-005 (W4) | — |
| G-PRT-004 ribbon / line callers | gaps ("names none") | no consumer named: hold |
| M-SPHERES `class_spheres` from the class header at load | mem:project-next-queue 2026-09-26 (creatures leftover); the tests fill them by hand (`all_levels_smoke.rs:191`) | the moby system (W4 lane 3) if the engine doesn't |
| M-APPICON the launcher's app icon ("amateurish" first attempt) | mem:project-launcher-and-mods | W8 lane 3, if still open (the launcher has `assets-src/icon/gen.py`) |
| M-WATER-EXACT whether to chase byte-exact water after the perf rewrite | mem:project-randcre-status-2026-09-26 | a user decision; G-TOOL-016 territory |
| ~~M-249530 `0x249530` "Drone Device path" (census, Kerwan 1342) vs G-WPN-002's Gadgets-page path `0x28f260`~~ | resolved 2026-09-29: `0x249530` is the ammo read (`hero/weapons.rs`, ported); the census note was wrong; 1342 is cheap | — |

**Contradictions (all resolved 2026-09-29 unless marked):**
- **Circular "Depends on" pairs in gaps.md:**
  - G-HERO-005 ↔ G-WPN-006, over the Hologuise only. Resolved: the body switch is hero code and depends on nothing in
    the gadget; the Hologuise body 3 has no consumer without the gadget, so it is ported with G-WPN-006 (W5), which
    depends on G-HERO-005's body switch. Bodies 1 / 2 stay in W4.
  - G-UI-011 ↔ G-LVL-007, over the race timer only. Resolved: the slot machinery is ported; the race timer's only
    consumer is the races, so it is ported with G-LVL-007 (W6); G-UI-011 no longer depends on G-LVL-007.
- **Stale census tags.** Fixed: help, the splash and burn sparks and the Suck Cannon interface are tagged has against
  the code (class_census.md "Re-run 2026-09-29").
- **Cheap list vs gaps.** Fixed: the census now reads engine state directly (the hero state / body compares, the ship
  moby, the cheat flag) and carries overrides for what it still cannot see (the fighters, 838 / 855).
- **Mixed counts in gaps.md.** Fixed: the rows and the top 15 both use the 2026-09-29 run.
- **The dev tooling reorg.** MEMORY.md indexes it as "deferred", but its note says done (d41ec5e, 2026-09-27): for
  the coordinator's memory index (not a repo file).
- **Digest baseline.** Re-derived: NO_IDLE `596306d7baa53abb8107b2d0ccfc5369`, full `42ec9908f7c83181a599154cb4749b40`
  (gaps.md G-TOOL-014); the commit that moved it off `94746b95` is not bisected (hero_gameplay.md §12 already
  reports `596306d7`).
- **Locked Goodies pages.** They are pause pages: G-UI-002; menus.md §12 fixed.
- **The `.mov` files.** G-TOOL-010 is count-agnostic now.
- **The in-game map.** Removed from gaps.md's [deferred] marker list.
- **The pack swap** (M-PACK): not a gap; no doc implied it was missing (checked 2026-09-29).
