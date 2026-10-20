# Moby update functions: mechanism and Novalis catalogue

How a moby gets its per-class behaviour, what every class placed on Novalis (level 01) does, and which of
them leave sequence 0. Addresses are **level01.elf** (the in-game engine; "boot" = SCUS_971.99). Sources:
Ghidra C exports in `work/decomp/level01.elf/`, the class table read from the level 01 overlay (then via the C++ `overlay.elf`, now
`overlay.bin`), instance and pvar data from the decompressed `levels/01/gameplay_ntsc.bin`, class blobs from the
level 01 core blocks `moby_class/NNNN`. Twelve update functions are only referenced by pointer and are not defined in
Ghidra (0x2bb128, 0x2cbda8, 0x2df448, 0x2e30b0, 0x2ece90, 0x2eda00, 0x2f3568, 0x2f6128, 0x300080,
0x309c98, 0x30a6c8, 0x30d200). The three that are placed on Novalis (0x2bb128, 0x2f6128, 0x309c98) were
read from raw disassembly. The same is true in every level ELF.

## 1. Mechanism

**Class → update table (per level, in the overlay).** ELF segment `0x20bb00` (0x924 bytes) holds
`struct { s32 o_class; void (*update)(Moby*); const void* vtbl; } tbl[]`, terminated by `o_class = −1`
(update 0). Novalis has 184 entries, all for classes in the level's core class list (212 classes; 28 have
no entry). 120 distinct non-null functions; several classes share one (bolts 13–16, crates 500/501/502/505/511,
grass 724/725, items 304/1456–1465, 13 FX classes → 0x30cd18, …). Two classes have explicit `update = 0`
(483, 634). `vtbl` → class header +0x2c. Default 0x20c3ac = 6 fn ptrs {0x3040b8, 0x3040c0, 0x3040c8,
0x3040d0, 0x3040f8, 0x3040d8}; 0x3040d8 is a 7-instruction wrapper that calls `DeleteMoby` 0x2636c0 (not DeleteMoby itself). 270, 572, 577 and 866 have their own. Callers 0x3028c8,
0x302a50, 0x304100 and 0x3051a8 are in the suck-cannon code (0x303000), so this is the "can be sucked /
on release" interface. Inferred.

**Registration.** When a class is loaded (slot `0x15ffc0`), `FUN_00279218(o_class)` (the level copy of
boot `fun_00212d68`) looks it up. It writes `update` → `0x197b00[slot]` and `vtbl` → `class+0x2c`.
Lookup tables: `0x198040[o_class]` = u8 slot, `0x197780[slot]` = class header.

**Instance init.** `InitMobyInstance` 0x263488 (boot 0x20c5f0): `moby+0x74 = 0x197b00[slot]`. **If it is
0, mode (+0x34) |= 2 ("no update")**. The instance record fields go to the moby as listed in
moby_render_notes.md §1. The loader then (§4) turns `pvar index` (+0x78) into a pointer.

**Per-frame call.** The in-level frame `FUN_002aba68` runs, in this order:

1. `FUN_00263300` (free-slot bookkeeping).
2. `FUN_00279470` → `FUN_002793d8`, the moby loop.
3. `FUN_002a1a18`, level callback list (0x13f2e4, stride 0x90, fn at +4).
4. `FUN_002b9a68` (level ambience).
5. `FUN_00228870` (`HeroUpdate`), the **hero** update.
6. `UpdateParts`, the camera, `sound_update`, lights.

The moby loop walks the list built by `fun_0020d868` (0x265548). For each moby with state `+0x20` ≥ 0:

1. `MobyAnimAdvance` 0x265260, unless mode & 0x40.
2. `(*moby+0x74)(moby)`: the **only argument is the moby pointer**. Everything else goes through
   moby+0x78 (pvars) and globals.
3. `fun_0020def8` 0x265bd8 rebuilds the matrix, unless mode & 4.

Within a class, order is by moby index. Group-activated mobys come after the directly-active ones of the
same class. **Level load** calls `FUN_002792d0` once (from `FUN_00258128`). It runs the same three steps
on every moby that has state ≥ 0 and no mode & 2, with **no distance gate**. So every init branch has run
before the first frame is drawn.

**Activity gate** (`fun_0020d868`). A moby is skipped if state `+0x20` < 0 (0xfd/0xfe = deleted,
0xff = end of array) or mode & 2. Otherwise it is active if any of these holds:

- `+0x31 != 0`. MobyProc sets this byte when it draws the moby and clears it when the moby is occluded
  (occlusion_culling.md).
- update distance `+0x30 == 0xff`.
- `|pos − 0x167240|² ≤ (+0x30)²`, where 0x167240 is the **camera position** written by the camera code.

If an active moby has group `+0x21` ≥ 0, the whole group is activated: every member in
`0x1abcc0[group]` (u16 moby-index list, bit 15 = last; 0x70 groups) is added. Active mobys with
mode & 0x1000 are also collected into the NULL-terminated array `0x1abe80`. The hero code scans that
array for targets (0x22c080, range byte at pvar-header +0x38).

**Mode bits seen in updates** (+0x34): 1 = hidden (MobyProc skips `& 0x81`; occlusion_culling.md),
2 = no update, 4 = keep matrix, 0x40 = no anim advance, 0x41 = "hide and freeze" idiom,
0x20 = pvar starts with a header pointer (read by `FUN_002711f8`/`FUN_00275290`), 0x100 = keep rotation
rows, 0x1000 = targetable, 0x8000 = mirror. Other fields that updates use: +0x20 state byte (each
class's own state machine; 0 = init), +0x53 target sequence, +0x70 bit 1 = sequence finished, +0x90 glow
colour, +0xa4 hit-message slot (set to 0xff after reading), +0xb0 mission byte index, +0xb2 spawn id
(persistence bit), +0xbc scratch byte (also an external command from other mobys).

## 2. Pvars

Gameplay header pointers:

- **0x54**: pvar table `{s32 offset, s32 size}[]`
- **0x58**: pvar data
- **0x50**: moby-link fixups `{s32 pvar_index, s32 byte_offset}[]`, terminated by < 0
- **0x5c**: pointer fixups (same shape)
- **0x48**: groups

The loader (`FUN_00255958`, after the instance loop) copies each moby's pvar block into the level heap and
sets `moby+0x78` to the copy, or 0 for index −1. It overwrites the table entry's offset with the copy's
address, so the fixups can find it. It does the same for cameras (`0x15ef50`, stride 0x20, +0x1c) and for
the level callback list (+8).

- **Moby-link fixup:** the s32 at `copy + byte_offset` is an *instance index*. It is replaced by
  `0x1acc00[index]`, the runtime moby index (−1 if that instance did not spawn). Updates reach the moby as
  `0x15ffd8 + i·0x100`.
- **Pointer fixup:** it adds the copy's base to a self-relative offset.

Novalis pvar sizes include 576 bytes (459), 624 (577), 688 (572 family), 416 (666) and 208 (750). Pvars
hold tuning values, cuboid indices (`FUN_00274820`, table 0x1600ec, 0x80 each), path indices (spline
table 0x1b0930), linked mobys and per-instance state. **A port must load pvars as raw bytes plus the two
fixup lists. The layout is per class.** Verified (loader read); the per-class layouts are only partly known.

## 3. Novalis catalogue (85 placed classes, 983 instances)

`#` = instances. `fn` = update function (level01). `cl` = cluster hash/programs from
`tools/ghidra/names/clusters.tsv`. **/19 means shared by every level** (engine-wide gameplay); /1 means
Novalis-only. "no geo" = no class blob, so there is nothing to draw. Seq = sequences the update selects.
Summary sources: my own reading for the top classes; the rest from read-only sub-agent passes over the C
(confidence noted in §4 or marked "med").

| o_class | # | fn | cl | name | behaviour | seq |
|---|---|---|---|---|---|---|
| 0 | 1 | — (hero code) | — | Ratchet | no table entry. Init `FUN_00226b70` sets mode \|= 2; driven by `FUN_00228870` (§5) | hero anim API |
| 11 | 1 | 0x2bb128 (undef) | — | Gadgetron vendor | pulses glow. Spawns child 1143 (hologram) and scales it by distance. Hero close and facing → help msg 21475; △ → `FUN_002ae1a0` (vendor menu), state 3 | 1 on approach (blend 10) |
| 13 (14–16) | 281 | 0x2bb758 | e7025b68/19 | bolt (value 1/5/20/50) | init: random spin, scale 0.65/0.7/0.75/0.6, state 3 (placed) or 1 (dropped: gravity plus sphere coll). Idle bob/spin; magnet to hero; collect → `FUN_002bc4f0` (bolts += value, HUD), marks spawn id collected, DeleteMoby | none (spins rows) |
| 27 | 10 | 0x2bd100 | 82acdd09/1 | particle emitter (no geo) | pvar-driven burst every N frames (`FUN_0027e900`) | — |
| 66, 310, 687, 742, 747, 780 | 13 | — | — | static props | no update fn (mode 2); seq 0 loop is right | 0 |
| 280 | 2 | 0x2e0c68 | dd9dd80d/3 | bolt crank | no pvar: spins. Else a 6-state jump table (0x20ae00) driven by hero state 0x3b (cranking, inferred). pvar f32[0] = 0..1 progress read by 641/665 | — |
| 304 | 10 | 0x2e1ac0 | bf70eee5/2 | item offer | pvar+0x40 item: dialog `FUN_0027b480`/`0027b028` → item owned, bolts −= price (0x1c4544), memcard_Save. **Ported** (challenge gate, states 0/2/4; the dialog is not): "In the port: Blarg flyers …" | — |
| 1456–1465 | 10 | 0x2e1ac0 | bf70eee5/2 | item-linked props | DeleteMoby once pvar item is owned. **Ported** (same section) | — |
| 459 | 25 | 0x2e6bf0 | f540458c/1 | path enemy | follows spline pvar+0x164; HP pvar+0x20; hit mask 0x210000 → knockback states; can grab the hero (hero state 0x1f). **Hidden** (0x41) at init if > 0.5 above ground | 0 idle, 4, 5, 8, 10, 11 (hit) |
| 500 | 199 | 0x2ea178 | 1cc860fc/19 | crate | hit mask 0x1830000 → `FUN_002eb918` (break FX) + `FUN_002eb498` (bolt drop), hide | none |
| 501 | 12 | 0x2ea178 | 1cc860fc/19 | nanotech crate (was "ammo crate": corrected, hero_gameplay.md §1.1) | as 500, no drop of its own; init spawns the nanotech cluster 806 (`FUN_00300528`) whose orbs heal 1 HP | none |
| 502 | 4 | 0x2ea178 | 1cc860fc/19 | reinforced crate | as 500; breaks only on damage flag 0x20000 | none |
| 505 | 1 | 0x2ea178 | 1cc860fc/19 | TNT crate | hit → state 5: 3 s blink/beep (sound 1) → explode | none |
| 511 | 24 | 0x2ea178 | 1cc860fc/19 | ammo crate (was "nanotech crate": corrected) | as 500 (+0xc6 = 100): ammo pickups (0x2db028) | none |
| 572 | 5 | 0x2edca0 | c1be19d3/3 | Amoeboid large (inferred) | HP 3; wanders toward pvar target, LOS test `FUN_00276fe8`; death: sound 4, seq 5 | **1 walk at start**; 0, 2, 3, 5 |
| 865 / 866 | 10 / 20 | 0x2edca0 | c1be19d3/3 | Amoeboid medium / small | same fn, scale 0.7 / 0.49, HP 1. **Hidden, speed 0, z −20** (state 0xc) until a parent splits | 1, 0, 2, 3, 5 |
| 577 | 68 | 0x2efc60 | 98445d80/1 | ground critter (suckable) | hit mask 0x330000; wander/turn/charge states 1–0x11; random mirror (0x8000); death state 99 | **4 at init if pvar+0x20a ≠ 0 (18 of 68)**; else 0 and state 0xe (z+6) |
| 604 | 1 | 0x2f2b68 | 96286e20/10 | commanded NPC/prop | 8-state anim driven by +0xbc commands from other code | 0→1→2→3/6→4→5→6→0 (blend 3–10) |
| 605 | 20 | 0x2f2eb8 | 3d8d96b2/18 | nearest-area marker (no geo) | hidden; picks the nearest of its areas to the hero into 0x141390 | — |
| 613 / 679 | 4 / 2 | 0x2f3120 / 0x2f6328 | fdae9be8/1, 78bc85ff/4 | water currents (no geo) | pushes the swimming hero (state 0x10–0x12) along a spline | — |
| 641 / 665 | 1 / 4 | 0x2f4348 / 0x2f4710 | 388482b0/1, 57bd9eb1/1 | crank-driven rotator / slider | rot X or XY lerp from the linked 280's progress; loop sound | — |
| 660 | 10 | 0x2f4428 | 4197de5e/1 | Blarg flyer (shootable) | shared flyer driver `FUN_002f5168` (path); scales/fades by distance and altitude; hit 0x800000 → explode; moves its class-27 exhaust emitter. **Ported** (flight; the kill effects are not) | — |
| 666 | 2 | 0x2f4960 | 23d6456b/1 | dropship | trigger cuboid → fly path, drop ≤ 8 children (0x1ab/0x154/0x1cb) | **5 every frame when pvar+0x170 = −1 (1 of 2)**; 2 at drop, 0 after |
| 676 | 1 | 0x2f6128 (undef) | — | custom-draw mesh | registers draw cb 0x2f60f8 (`FUN_002b96e0(8, 0x1e2fc0)`) | — |
| 678 | 1 | 0x2f6180 | ed91b009/1 | custom-draw surfaces (no geo) | bounds for 2 vertex sets, draw cb `FUN_002b96e0(2, 0x1e3380)` | — |
| 688 | 2 | 0x2f7728 | 7751f2bf/1 | gunship (cutscene) | cuboid → cutscene (hero state 0x72, camera), fires 686 projectiles at cuboid points, persistent done bit | — |
| 695 | 9 | 0x2f8268 | 87aee26b/1 | floating pushable | spring to home; pushed by hero; z follows water height | — |
| 700 | 6 | 0x2f8c58 | 37ec743a/1 | impact smoke emitter (no geo) | smoke while pvar timer > 0 (armed by 688) | — |
| 701 | 3 | 0x2f9080 | 88cc9af9/1 | collapsing platform | hero on it → rumble, camera shake → falls 20; persistent | — |
| 703 / 715 | 2 / 5 | 0x2f95c0 | 4bf3ffc5/1 | elevator | vertical ping-pong, 2 s pauses, carries riders `FUN_002755f8` | — |
| 704 | 3 | 0x2f9810 | a2813ecd/1 | breakable rock | hit 0x10000 → 200 smoke + 40 debris, death bits, delete | — |
| 705 | 5 | 0x2f9bf0 | 5f63cd50/1 | oscillating spinner | yaw spin that ramps and reverses | — |
| 709–711 | 3 | 0x2f9d80 | 87a8f896/1 | breakable by 686 | only projectile class 686 breaks it; persistent | — |
| 724 / 725 | 89 / 36 | 0x2fa720 | 575c8bad/4 | grass / plant | hero < 1 unit and speed > 2·dt → seq 1 (blend 6); when it finishes → seq 0 | 0, 1 |
| 726 | 1 | 0x2b9eb0 | 7e8874a8/4 | path platform / lift | moves along spline pvar+0xb4 between ends when the hero rides or calls it; loop sound 0; snaps to a path end at init | — |
| 729 | 1 | 0x2fa800 | 5c0fb729/1 | big breakable wall | hit 0x800000 → 500 smoke + 100 debris, persistent | — |
| 730 / 790 | 1 / 1 | 0x2fad68 | 2c0aee80/1 | mission NPC (descends) | **hidden** until cuboid; drops in, dialog streams, waits for 3 linked kills, unlocks item, save | 1 on landing, 2 (blend 20) |
| 737 | 5 | 0x2fb5b0 | 5c26590d/6 | camera trigger (no geo) | cuboid → scripted camera move | — |
| 746 | 2 | 0x2fb8a8 | 194dc9ba/1 | hinged bridge | lowers when commanded (+0xbc = 2) or mission done | — |
| 750 | 1 | 0x2fbf80 | 2dc87352/14 | infobot / item pickup | **hidden** (pvar[0] = 0) until activated; flies a path to the hero; pickup → item unlock, dialog, save | — |
| 751 | 1 | 0x2fd0e8 | fb028eaf/1 | water surface FX manager (no geo) | 7 camera zones: ripple sim, draw cb, spawns 787 | — |
| 754 | 12 | 0x2fd9a0 | 98a37245/1 | breakable pot | hit → shards 1817 + 1816, delete | — |
| 760 | 4 | 0x2fdbc0 | aa93bf0e/3 | waterfall foam / mist (no geo) | FX 46/47 quads in a cuboid, per-element V scroll + 809 global, draw cb list 2 (world_animation.md §3) | — |
| 761 | 1 | 0x2feb58 | 916c4f0f/1 | rapids strip water (no geo) | edits its 4 strips once; 64-tick z bob; draw cb 0x2feb28 (world_animation.md §1) | — |
| 768 / 769 | 4 / 4 | 0x2fed68 | 99bfce39/1 | sliding door halves | open when hero is within pvar radius; ±2 along yaw | — |
| 774 | 1 | 0x2ff118 | b2740d48/2 | talking NPC ("Water Pump Worker") | dialog, head look-at (manipulators) | — |
| 778 | 1 | 0x2ff860 | 86bb284b/1 | breakable prop | hit → debris, spawns 779 | — |
| 805 | 3 | 0x300220 | a42e3b21/18 | checkpoint trigger (no geo) | cuboid rising edge → set respawn point | — |
| 809 | 1 | 0x2ba658 | 2909db84/3 | texture-scroll controller (no geo) | update dist 0xff; scrolls a global −8..0 | — |
| 815 | 6 | 0x3021b8 | dfc02b88/1 | enemy spawner | spawns pvar class while its group is empty | — |
| 832 | 1 | 0x302648 | b12211ca/16 | RC-vehicle range limiter (no geo) | static overlay, then ends control of 172 | — |
| 1042 | 12 | 0x307c48 | 0614849b/1 | shootable | delete on hit | — |
| 1134 | 3 | 0x307ca0 | a37bacdb/17 | gold bolt | deleted if collected (0x14bec0); bob/spin; pickup cutscene (hero anim 0x82) | 1 at pickup, 0 then hide |
| 1135 | 2 | 0x308bd8 | 3945bbf7/8 | teleporter pad (pair) | 3 child 315 arms; △ on pad → teleport to linked pad; hidden in a first playthrough when pvar+0x3c ≠ 0. **Ported** (gate, arms, idle; the teleport is not) | — |
| 1225 | 1 | 0x309c98 (undef) | — | dark water strips (no geo) | dists 0xff, draw cb 0x309bf8 (`FUN_002b96e0(3, 0x202d80)`, then the 64-tick z blend for the next frame) | — |
| 1341 | 1 | 0x30acb8 | 29f554f2/1 | help-hint director (no geo) | tutorial messages by timers, cuboids, bolts | — |
| 1504 | 1 | 0x30b618 | 7a2089d7/2 | wandering point light (no geo) | allocates a light slot near the camera | — |
| 1546 | 1 | 0x30c190 | 4067f411/1 | cutscene FX driver (no geo) | effects keyed to cutscene id/frame | — |
| 1813 | 4 | 0x30d0f0 | e025ab3c/1 | breakable crate variant | hit → debris 1815/1816, delete | — |
| 1818 | 1 | 0x30df40 | c095e3bb/10 | controllable critter ("mouse" strings) | cuboid + LOS → hero state 0x1f, moves, returns home | 0 (init), 1 walk, 2 |
| 1848 | 1 | 0x30f208 | bbc6005e/1 | UV scroller (no geo) | 2 globals += dt·0.025, draw cb | — |

**Spawn-only classes with update fns** (not placed; created by code), 104 classes, not read:

- item/weapon classes from the item records at `0x179f40` (0x4c stride; o_class at +0x10, i.e. `0x179f50 + 0x4c·id`): 71, 849, 192, 157,
  208, 163, 175, 168, 176, 190, 180, 177, 229, 185, 1251, 454, 562, 188, 585, 433, 1289, 1290, 618
- bolt values 14–16
- ship 530/531 (the table fn is replaced, §5)
- the 0x2c5218 group (149, 348–364), the 0x2db028 group (204, 213–226, 1006, 1438–1449) and the
  0x30cd18 group (1736–1817), which look like FX/debris
- 607 (Clank pack, §5), 806 (nanotech cluster of crate 501: `classes/pickup.rs`), 1143 (vendor hologram), 1816/1817 (shards)

## 4. Top classes

- **Ratchet (0) / Clank (601).** See §5.
- **Bolts 13–16 (281 placed).** States: 0 init → 3 idle (placed) or 1 fall (spawned from crates:
  gravity 10.8 × `0x15ed70` per tick, sphere collision `0x212960` flags 0x22) → 2 bounce → 3 → 4 fly-to-hero → collect.
  Value is by class (1/5/20/50). Collecting writes `0x1bbb04[spawn id]`, so a collected placed bolt does
  not respawn. Bolts are hidden while `0x15f5c4 == 2` (cutscene). **No sequence change:** spin comes from
  rotating the rows (mode 0x100). Seq 0 is right. High.
- **Crates 500–511 (240 placed).** No sequences. Hit query `FUN_0026f320(m, 0x1830000)`. Break →
  `FUN_002eb918` (FX/debris) + `FUN_002eb498` (spawns N bolt mobys with random velocity; the count and
  type come from pvar +0xc4/+0xcb). The crate is then deleted, or, when pvar+0xc8 is set, hidden
  (`mode |= 1`, glow 0) in state 6 with a respawn timer (planet 0x12). The multi-hit classes 0x1fb–0x1fd swap `o_class`/class pointer in place, stepping toward 0x1fe (none on
  Novalis). High.
- **Amoeboids 572/865/866 (35).** Big ones (pvar+0x230 = 1) start in state 1. That state blends to
  **seq 1 over 5 ticks** on the first frame, then walks: states 1/2 steer toward the target (+0x200)
  when `FUN_00276fe8` gives line of sight, 1-in-4 per frame. Seq 0 on stopping, 3 on attack, 2 on
  land/hit, 5 on death (sound 4). The 30 small/medium ones start **hidden, z −20, speed 0**, and wait to
  be unhidden by a splitting parent. Our renderer draws them. Medium (the split logic is not traced).
- **Critter 577 (68).** Init seeds a jump pattern (0x26d930). With pvar+0x20a ≠ 0 it goes to state 1
  and **seq 4 with blend 0**, an instant switch. Otherwise it is lifted 6 units, goes to state 0xe and
  stays on seq 0. States 1–3 wander, turning randomly every 30–90 ticks (seq 2/3/5). 4–6 charge and bite
  (seq 2), with hero distance thresholds of 2/8 units. 0x11 retreats (seq 1). Hits (mask 0x330000)
  knock it back or kill it (state 99 → delete). Suckable. Medium.
- **Enemy 459 (25).** Rides spline pvar+0x164 (from 0x1b0930). Hit types 1–6 pick the knockback
  (seq 11 for heavy hits). Can grab the hero (`FUN_0023cf98(0x1f)`). Init forces seq 0 and hides
  airborne instances. Medium.
- **Grass 724/725 (125).** The whole update is 36 lines (0x2fa720): see the table. The cheapest visible
  win. High.
- **Vendor 11.** Glow `+0x90 = (sin(t)·48 + 96)·0x010101 | 0x80000000`. Child 1143 is created at init
  and attached with 2 manipulators. Hero within range, facing (< 90°), not in state 0x1d/0x32 → help
  message 21475. △ (pad pressed bit 0x10 at 0x13cae4) → vendor. Seq 1 (blend 10) when the hero is near.
  Medium (the class and message identity are inferred).
- **Ship 531.** Not a placed instance. The loader creates it with `CreateMoby(0x160548[ship])` at the
  level's ship position (level-settings +0x2c). It sets `+0x74 = FUN_002a1c40` (the engine-shared ship
  update), draw 0xff, update 16, and **`fun_00212ed8(ship, 1, 0)`: seq 1 by hard cut**.
  `FUN_002a1c40`: pulsing glow, engine draw callbacks (0x2a70a8; 0x2a2130 when the camera is within 32),
  hero within 4 → help 0x53e4, △ → ship menu `FUN_00279070(2)`. High.
- **Platform 726.** Path lift: see the table. Uses the spawn-id save bits (0x14c190/0x1ba950) to
  remember that it was activated. Medium.
- **Mission NPC 730/790, infobot 750.** Hidden at spawn. They appear on cuboid triggers and run dialog
  streams (`FUN_002ac330`/`FUN_002acf50`). They unlock items (`FUN_002756d0`), set the mission byte
  (`FUN_00265080`) and save. Medium-high.

## 5. Hero, Clank and ship

- **Ratchet = o_class 0.** No table entry. `FUN_00226b70` (hero init) zeroes the hero block
  0x13f350..0x141660 and finds the moby with `+0xa6 == 0` (→ `0x1413d0`, also 0x13fdd8). It ground-snaps
  that moby, sets mode |= 2, so the generic loop never runs it, and copies the position to 0x13f3d0.
- **Per-frame update:** `FUN_00228870`. It bails out (fade, respawn) outside x/y 2..1022. With hero mode
  `0x1413f4 == 0` it calls ~30 sub-updates in a fixed order: 0x247d48, 0x231d18, 0x22cd48, 0x22d090,
  **0x242930**, 0x236860 … 0x22a260. Otherwise `FUN_00228000`.
- **State machine:** `FUN_00242930`, 1851 lines. `switch (0x1413d4)` over states 0..0x82, with 72
  distinct case labels. 0x77 is forced when z < the level kill height.
- **State and animation API:**
  - set-state `FUN_0023cf98(state, force)` (1620 lines, its own switch)
  - animation `FUN_00247a90(f32 blend, seq, frame)`: a hero-only blend with the same snapshot scheme as
    `fun_00212f90`, rate in 0x13fde0/4. Sequences come from `ratchet_seq`. It swaps seq 0x31↔0x32 when
    `0x15edb5` is set (mirror).
- **Hero globals used by mobys:** 0x13f3d0 pos, 0x13f440/0x13f450 velocity, 0x13f3e8 yaw, 0x1413d4
  state, 0x1413dc sub-state (inferred name), 0x13f64c standing-on moby, 0x1415f8 "can interact"
  (inferred name).
- **Clank = o_class 601** (18 seqs/33 joints). Created by `FUN_0022f3c0` with `CreateMoby(item[1].class)`
  (0x179f9c) → `0x1404d4`, and the pack moby `CreateMoby(item[pack].class)` → `0x1404d0`. Pack =
  equipped (0x141430) / 0x14166c / 2, or 3 if 0x15ed94 is set. Classes: 2 → 607, 3 → 608, 4 → 609.
  601/608/609 have no table fn (mode 2). 607's fn 0x2f30c0 only sets seq 6 in hero state 8. The hero
  code drives their sequences directly: e.g. `fun_00212f90(clank, 1, 0, ticks(7))` when a sequence ends
  in state 0, and blend 12 when `0x17ad58` > 0.61 or `0x17ad54` > 0.44. Classes 1/2 (20 joints, translation
  records for joints 52–86) have no update. Their use is not traced.
- The player-controller reversal is a separate task. Entry points: `FUN_00228870`, `FUN_00242930`,
  `FUN_0023cf98`, `FUN_00247a90`, movement `0x233940` (collision_queries.md).

## 6. Sequence at spawn (why "everything plays seq 0" is wrong)

Visible on the first frames on Novalis:

- **531 ship: seq 1** (hard cut).
- **577: seq 4** on the 18 instances with pvar+0x20a ≠ 0.
- **572: seq 1** (walk, 5-tick blend).
- **666: seq 5** on the instance at (206.4, 246.0, 40.0) (pvar+0x170 = −1). The other is hidden.

Classes that force seq 0 at init: 459, 604, 1818.

Mobys that change sequence later, on a condition:

| class | new seq | condition |
|---|---|---|
| 11 | 1 | hero near |
| 724/725 | 1 | hero runs through |
| 730/790 | 1, 2 | landing / mission already done (then 2 at once) |
| 1134 | 1 | pickup |
| 604 | 1–6 | on command |
| 607 | 6 | hero state 8 |
| 1818 | 1, 2 | moving |
| 459/572/577 | various | combat |

**Hidden at spawn (the renderer should not draw them):** 865/866 (30, also 20 units lower); 730, 790,
750; 666 (the triggered one); airborne 459s; 1135 with pvar+0x3c ≠ 0 while 0x15ee20 = 0 (instance 961, every
tick, see "Not modelled" below). Also, but none on Novalis: 726 when pvar+0xcc ≠ −1 and the hero is outside that
cuboid.

Crates, bolts and pots use no sequences, and 50 of the 68 577s stay on seq 0 at spawn. So the
remaining seq-0 drawing is correct for every other placed class.

## 7. Engine services the updates call (API surface)

| Service | level01 addr (boot) | Notes |
|---|---|---|
| seq blend / cut / flags | 0x26c660 `fun_00212f90`, 0x26c5a8 `fun_00212ed8`, 0x26c7a8 `fun_002130d8` | moby_animation.md §4. Idiom: `if (m+0x53 != s) blend(m, s, 0, ticks(n))`; poll `m+0x70 & 2` |
| anim state / frame count | 0x263718 `update_moby_animation_state`, 0x263920 | 0x263920 = current key time |
| create / delete / init | 0x263390 `CreateMoby(o_class)`, 0x2636c0 `DeleteMoby` (state 0xfd/0xfe), 0x263488 | CreateMoby takes a free slot after the static mobys |
| manipulators (+0x64 list) | 0x264370 `AttachManipulator(m, joint, node)`, 0x2643e8 Detach | fills the joint-modifier list of moby_animation.md §6.4 |
| hit / damage message | 0x26f320 `(m, mask, keep)` → record in 0x178580[m+0xa4] (0x40 each; +0x20 attacker, +0x24 flags, +0x2c damage) | masks: bolts/crates 0x1830000, 577 0x330000, 459 0x210000 |
| collision | 0x211870 `CollLine_Fix`, 0x212960 sphere, 0x2135a0 capsule, 0x214468 sphere-vs-mobys, 0x26e618 ground height | collision_queries.md |
| cuboid / path / group | 0x274820 point-in-cuboid (0x1600ec), 0x272e28/0x2726c8 spline sample/project (0x1b0930), 0x26e008 group count, 0x277fb8 hero-on-moby, 0x2755f8 carry riders | |
| hero control | 0x23cf98 set state, 0x247a90 set anim, 0x2368e0 teleport, 0x236738 | |
| sound | 0x2a1618 `fun_0022da68(idx, flags, m)` play class sound → voice, 0x2a12f0 voice alive?, 0x2a1348 release_voice_slot; 0x2ac330 / 0x2acf50 dialog streams | |
| particles / FX | 0x27e900 spawn particle, 0x281f30 smoke, 0x282060 sprite, 0x2f8530 debris moby, 0x273310 beam/explosion, 0x2787a0/0x278ad8/0x278e20 break FX | |
| custom draw | 0x21afe0 / 0x21b198 `(fn, arg)` per-frame draw callbacks (64 each, drained in the draw phase), 0x2b96e0 draw static strips | used by ship, 676, 678, 751, 760, 761, 1225, 1848 |
| lights | 0x252750 alloc / 0x252850 free point light | |
| timers / RNG / math | 0x220e30 `ticks(n)` = n·scale + 0.5 (PAL/NTSC), 0x220e78/0x220ea8 FastDecTimer (0 running, 2 on expiry, 1 idle), 0x26c9c8 rand float, 0x26c970 rand int, 0x26c930 rand mod, 0x26ca90 rand angle, 0x221188/0x2211b8/0x221210 vec add/sub/scale, 0x221360/0x221398 distances, 0x2217c0 atan2, 0x221ff8/0x222100 angle add/diff, 0x26cef0/0x26d058 spring turn, 0x270728 approach | fRam0015ed6c = dt |
| HUD / save / mission | 0x278f58 try_set_help_message, 0x225818 help queue, 0x2789e0 HUD text, 0x261448 memcard_Save, 0x265080 mission done, 0x26c250 death bits (0x14c190 + level·0x100, 0x1ba950), 0x1bbb04 per-spawn-id flag, 0x2756d0 unlock item | |
| camera | 0x316ef8 / 0x317070 camera script, 0x167240 camera pos | |

## 8. Port plan

1. **Formats:** `rc_formats::gameplay` gets the pvar table (0x54/0x58), the two fixup lists (0x50, 0x5c)
   and groups (0x48). `rc_formats::level_overlay::update_table(elf)` reads the class table: the segment at
   0x20bb00 on L01, found per level by the ELF segment of size < 0x1000. It is kept for cross-checks and
   documentation only; the port does not call PS2 code.
2. **Runtime moby** (`rc-game`): a `Moby` struct mirroring the 0x100-byte layout that updates touch
   (state, mode, dists, group, anim state, pvar bytes, +0xbc scratch, glow, spawn id). Pvars stay
   `Vec<u8>` plus typed per-class views (`bytemuck`), with moby links as `Option<MobyId>`.
3. **Dispatch:** `fn update(o_class) -> Option<fn(&mut World, MobyId)>`. It is a `match` on o_class
   mirroring the level's table (per level, since the same class id can map to different code), with
   shared fns shared (e.g. `crate_update` for 500–511). A trait object per class is unnecessary: the
   game's own model is a plain function pointer plus a per-class pvar layout. Classes without an entry
   get mode 2 exactly as the game does.
4. **Scheduler:** port `fun_0020d868` exactly: camera-distance gate, the +0x31 visible flag, 0xff,
   group activation, class-slot order, the 0x1000 target list. Then the loop: advance → update →
   matrix. Also the load-time pass `FUN_002792d0`, and the frame order hero-after-mobys of `FUN_002aba68`.
   Order matters for RNG and hit messages.
5. **Services** (`World` methods, §7): anim blend/cut, create/delete (slot reuse and state 0xfd/0xfe
   semantics), hit messages, collision (already ported kernels), cuboids/splines, sound handles
   (stubbed), particle and draw callbacks (as render events), save bits.
6. **Order of work:**
   1. hidden-at-spawn plus the four spawn sequences above (small render fix)
   2. grass 724/725, bolts, crates, elevators/doors/cranks (simple, many instances)
   3. ship, vendor
   4. enemies 577, 572 family, 459
   5. NPC/mission scripting
   6. hero (separate agent)
7. **Verification:** the PCSX2 trace harness (trace_harness.md) should record per-moby state
   {+0x20, +0x34, +0x52/+0x53, +0x54, pos} per frame, so each ported update can be diffed frame by frame.

## 9. Confidence and unknowns

- **Verified:** table layout and registration (`FUN_00279218` plus raw table); InitMobyInstance +0x74 /
  mode 2; the update loop, the active-list gate and group activation; pvar loading and fixups; hero
  identification (`+0xa6 == 0`); ship spawn with seq 1; the pvar values quoted for
  577/572/865/866/666/750/726/1135.
- **Inferred:** vtbl = suck-cannon interface; 0x167240 = camera position; class names (vendor, Amoeboid,
  critter, infobot, etc.) come from behaviour, not from strings. Clank = 601 comes from the item table
  and the hero code.
- **Unknown:** the 9 other undefined functions (all spawn-only on Novalis); the behaviour of the 104
  spawn-only classes; the amoeboid split code; which instances of 459 are airborne at spawn;
  `FUN_0025e7b0` (instance `unknown_74` hook); classes 1/2; what the level callback list 0x13f2e4 holds.

## In the port (2026-09-26): state after the load pass

Code: `crates/rc-formats/src/moby_spawn.rs` (pure rules, `initial_state`, `ship_state`,
`SpawnState::anim_after_load`), `crates/rc-formats/src/gameplay.rs` (`parse_pvars`, `parse_pvar_fixups`,
`MobyInstance::pvar`, `parse_splines`, `ship_placement`), `crates/rc-formats/src/moby_anim.rs` (`hard_cut`,
`set_sequence`, `snapshot`, `evaluate_with_snapshot`), `crates/rc-engine/src/moby_spawn.rs` (applies it),
`crates/rc-engine/src/moby_anim.rs` (snapshots, first-update changes). `RC_SPAWN_RULES=0` restores the old
behaviour (sequence 0 from the spawn state, nothing hidden or moved, no ship).

**Pvars (§2), checked in the loader.** Blocks are indexed by pvar index. Moby-link fixups replace an
instance index ≥ 0 with `(s16)0x1acc00[index]`; the port spawns every instance, so the index is kept. Pointer
fixups add the heap address of the copy; the port keeps the value relative to the block. Both lists are
applied only to blocks the loader copied: the table offset must be ≥ the gameplay base, which is true only
after the copy. There is also a third, shared-data list at header 0x4c, `{size, count, pad[2], data,
{u16 pvar, u16 offset, s32 data_offset}[count]}`, run before the spline copy. It is empty on Novalis and is
not applied.

**Order of events.** Loader: `InitMobyInstance` per instance, then the ship. `FUN_002792d0`: for each moby,
`MobyAnimAdvance` and then the class update, with no gate. So after load every moby has already been
advanced once (the port adds that advance; Ratchet is excluded: hero init sets mode 2 and class 0 has no
sequence 0). Every init change is made after that advance. `fun_00212f90` with 0 ticks gives
rate = 1/0 = +MAX, so the next advance lands on the target key with t = the target's rate. `hard_cut`
(0x26c5a8) clamps B to count − 1 rather than wrapping (§4 of moby_animation.md says "wrap"; the code clamps).

**Snapshot (`fun_0020ede8`).** `fun_00212f90` passes the flags `slot | 0x300`, which skip **both** pose-layer
lists (moby_animation.md §4 says "layers included"). The snapshot does its own decode and interpolation:
* **Lists.** It builds one scale list with −4 markers. That list is a true union, except that a joint repeated
  in B is appended again.
* **Interpolation order.** The loops are pipelined, so the next operands are loaded before the current store.
* **Quaternion blend.** It nlerps A·u and B·t with the sign test on the scaled products.
* **Encoding.** Quaternions go through `vftoi15` and are clamped. Scale goes through `vftoi15 >> 3`
  (logical shift) and is written only when it is not 0x1000; its flag is `(w & 0x80) ^ 0x80`, where
  w = (flag byte >> 6) − 1. A translation is written only when its `vftoi0` differs from the rest value.
  Rate and time are 0.

**Rules pinned from the decompilation** (the table `class | fn | rule` is in the `initial_state` doc):
* **459 (0x2e6bf0 case 0).** The moby first moves to point 0 of spline pvar+0x134, or of spline pvar+0x164
  when pvar+0x134 < 0. In the pvar+0x164 case it also moves 20 units up when pvar+0x1b4 = 0. Then
  `FUN_0026e618(0.5, pos)` (a line from z + 0.5 down to z = 0.01, world mesh only, returning 0 on a miss)
  decides "airborne": z − ground > 0.5 sets `mode |= 0x41` and +0x94 = 0. On Novalis 21 of 25 are hidden:
  the 19 raised ones (margins 19.4–21.3 units) and 325–327, which sit 30 units above the ground at their
  spline start. The 4 with pvar+0x1b4 = 1 (322, 328–330) start exactly on the ground and stay visible.
* **865/866 (0x2edca0 case 0).** These are both moved and hidden. With pvar+0x258 = 0 and pvar+0x230 = 0,
  the init sets `mode = mode & ~0x1000 | 1`, speed 0, +0x31 = 0 and z −= 20. It then enters state 0xc
  (pvar+0x25c = −1, true for all 30 on Novalis), which re-sets mode bit 1 every frame, or state 0xd (a
  cuboid trigger adds 20 back). The port hides them and also applies the move. The big 572s (pvar+0x230 = 1)
  do nothing visible in the load pass; they blend to sequence 1 in their *next* update. That blend is
  `fun_00212f90(1, 0, 5)`, raw 5 ticks, not `ticks(5)`. If the 1-in-4 line-of-sight branch succeeds it is
  `ticks(10)` instead; the port uses 5. Because the first frame's advance has made t > 0.025, the blend
  starts from a snapshot.
* **577.** When pvar+0x20a = 0 the moby is lifted 6 units into state 0xe. That state is a hover: states
  0x10/0x11 keep it at ground + 6, so the move is real and is reproduced (50 of 68).
* **Ship.** The ship is created at level settings +0x2c/+0x30/+0x34, with yaw (moby+0x48) from +0x38, when
  x > 0. Its class is `0x160548[ship]` = {531, 532, 533} in every level overlay, with index 0 on a new game
  (`DoSpaceTransition` 0x2a68f8). It gets update `FUN_002a1c40`, draw distance 0xff and update distance
  0x10, then mode &= ~2 and `fun_00212ed8(ship, 1, 0)`. Its render fields are the `InitMobyInstance`
  defaults: light word 0, ambient 0x40/0x40/0x40, occlusion 0x7f80. The occlusion resolution skips
  0x7f80, so the ship is always visible. Novalis: (167.23, 125.85, 61.15), yaw π/2, on the landing pad.
  Sequence 1 of class 531 has a single key (the landed pose with the canopy open), so it does not animate.

**Novalis counts** (984 = 983 placed + the ship):
* **Hidden: 55.** 459 ×21, 865 ×10, 866 ×20, 666 ×1, 730, 790, 750.
* **Moved: 108.** 459 ×25, 577 ×50, 865/866 ×30, 730/790 (+30 z, hidden), 1818 (+0.02 z, onto its house,
  instance 647).
* **Sequence changes.** The ship is cut to sequence 1. In the load pass, 577 ×18 go to sequence 4 and
  666 ×1 to sequence 5, both with blend 0. At the first update, 572 ×5 blend to sequence 1 over 5 ticks.
* **Rendering.** 113 renderer entities are tagged `SpawnHidden`.

**Engine.** Hiding is a `SpawnHidden` component plus a PostUpdate system that runs after
`moby_render::update_moby_occlusion`: that system only writes `Visibility` when its own result changes, so
the hide wins without editing the renderer. Hidden instances are never evaluated. The first-update change is
applied on the first tick the moby is active by `fun_0020d868`: drawn last frame (the port reads
occlusion-visible; +0x31 is 0 before the first frame), update distance 0xff, or within the update distance
of the camera. Group activation is not modelled. The ground probe in the engine is an f32 copy of
`CollLine_Fix`'s one-sided segment test, not the bit-exact `rc-game` kernel; every Novalis decision has a
margin of at least 0.5 units.

**Not modelled:** the random mirror of 577; later frames (every update function); the
0x4c shared-data pvar fixups; the 726 hidden rule (726 pvar+0xcc = −1 on Novalis). The 1135 rule is ported (below).
Spawn conditions are ported: the loader's spawn test (0x256890..0x256a58) is
`rc_formats::moby_spawn::spawn_test` / `loader_spawns` (Novalis first arrival: 929 of 983 records created; see
`trace_results_novalis.md` "Fixes applied" #1).

**1135 challenge-mode pad (correction, 2026-09-27).** `TeleporterPadUpdate` 0x308bd8 has a second hide rule
that does apply on Novalis: when pvar+0x3c ≠ 0 and the times-completed count 0x15ee20 (gp−0x7de0) is 0, every
tick sets +0x94 = 0 and mode |= 0x41 and returns before the state switch (state stays 0, no 315 arms are
created). Instance 961 (slot 907, at (158.97, 139.31, 60), left of the spawn) has pvar+0x3c = 1: RAM mode 0x51,
+0x31 = 0 at the spawn savestate; the other pad, 962, has pvar+0x3c = 0 and is shown. Same gate as
`ItemOfferUpdate` 0x2e1ac0 (deleted when 0x15ee20 = 0). The port draws 961: **M**, port the 1135 update (at least
this gate) in `rc-game` `moby_update`. **Done** (2026-09-27): "In the port: Blarg flyers, teleporter pads, gold-weapon
offers".

## In the port (2026-09-27): the scheduler, bolts, crates and grass

Code: `crates/rc-game/src/moby_update.rs` (module root and unit tests), `moby_update/scheduler.rs`,
`moby_update/services.rs`, `moby_update/classes/{mod,bolt,crate_,grass}.rs`; disc test
`crates/rc-game/tests/classes/moby_update_novalis.rs`. Addresses are level01. Everything runs on the PS2 float model
(`Pf`, VU0 lanes in the instructions' ACC order) and draws from the one shared `rand` stream.

### Scheduler (read from the disassembly)

* **Active list** `fun_0020d868` 0x265548. The walk goes from index 0 to the first state 0xff and skips
  state ≥ 0x80 and mode & 2. A moby is active if `+0x31 != 0`, or `+0x30 == 0xff`, or the sign bit of
  `((d·d − dx²) − dy²) − dz²` is clear (VU0 `vmula`/`vmsuba`, d = `(f32)+0x30`, `dx..` = pos − camera
  0x167240). An active moby with group ≥ 0 only flags its group. After the walk, every member of each
  flagged group (0..0x70, list order) is added; only its state is checked, not mode 2 or distance.
  Additions go to 224 class-slot lists, which are chained in slot order. **Run order: class slot, then
  direct actives by index, then group members.** Mode-0x1000 additions form the target list 0x1abe80.
* **Loop** `FUN_002793d8`. The list is built before any update runs. A moby deleted earlier in the loop
  is skipped; a moby created during the loop waits a tick. For each moby: `MobyAnimAdvance` (unless
  mode 0x40), then `(*+0x74)(m)` if it is non-null, then `fun_0020def8` (unless mode 4).
  `fun_0020def8` (0x265bd8) does four things:
  - rows from Euler, or the kept rows with mode 0x100; row 1 is negated with mode 0x8000, also on kept rows;
  - the sequence sphere at `pos·1024`, lerped as `((B·t) + A) − A·t`;
  - the `+0xa8` change counter.
  The moby-grid re-insert is not modelled. `UpdateMobysPartB` 0x279470 is this loop plus a profiler
  branch.
* **Load pass** `FUN_002792d0`. Every moby up to the first 0xff with state < 0x80 and no mode 2, in
  **array order**, with no distance gate and the target list cleared.
* **Dispatch.** `moby+0x74` holds the level-table address. `scheduler::ported(addr)` maps it to the Rust
  port and `port_update_fn(o_class)` gives it for the table builder. A class without a port gets `None`,
  so `InitMobyInstance` sets mode 2, exactly as for a class with no table entry. Such mobys are still
  advanced and rebuilt when their sequence 0 has ≥ 2 frames (which clears mode 2), or when a group adds
  them.
* **Ports outside `moby_update`.** Examples are water 751 in `water.rs` and the class-27 emitters in
  `particles::type06`. They plug in through `services::ExternalUpdates` (`update_fn(o_class)`,
  `update(addr, id, table, rng, camera, counter)`), and `scheduler::update_fn_for` includes them. They then
  run at their place in the order with the shared RNG.
* **Class slots.** The order of the level core's class list (`MobyClassesRelocate` 0x2549d0, counter
  0x15ffc0). The start at 0 is inferred.
* **Loader** (`scheduler::load_static_mobys`, `InitLevelRenderGlobals` 0x255958):
  - +0xb1 = 0xfe with no spawn flags, else 0xff. `FUN_0029ab50` for flag 0x10 is not ported.
  - +0xb4 = +0xb6 = record +0x10; +0xb0 = record +0x04.
  - scale = class·instance.
  - the instance mode bits are also OR'd into the class's +0x44, so later instances and `CreateMoby`
    inherit them.
  - `MobyBuildMatrix`, then ambient and light.
  - Dynamic slots = the section's spawnable count.
* **`0x15ffbc` is a count, not a flag** (verified, 0x263300). It counts reusable dynamic slots plus every
  slot after the end marker; `CreateMoby` decrements it. It is `MobyTable::free_slots`: recounted by
  `MobyTable::free_slot_pass` (the tick's first call, and at the start of the load pass) and decremented by
  `MobyTable::create`.
* **A slot without a class header** (a class with no blob in the core, e.g. the class-27 emitters and 751):
  `InitMobyInstance` (boot 0x20c5f0, read) takes +0x74 from the slot's update-table entry *before* testing
  the header, so the update still runs; with no header it only adds mode 5. `ClassInfo::no_header`.
* **Load-pass split with `rc_formats::moby_spawn`.** `initial_state` covers the unported classes' init
  branches (459, 572/865/866, 577, 666, 730/790, 750, 1818); in this table those classes are mode 2 and
  never run. The scheduler's load pass runs the ported classes' state-0 branches plus the anim advance of
  every non-mode-2 moby. Per moby, the engine must take the anim, position and mode from exactly one
  source: the table for ported classes, `SpawnState::anim_after_load` for the rest.
* **+0x31** is written by MobyProc every frame: 1 when drawn, 0 on every skip path. The scheduler only
  reads it; the engine must write it back after drawing. Bolts also set it to 1 in their own update.

### Engine wiring (rc-engine `gameplay.rs`, 2026-09-26)

* Load: class table (slot = index in the core's class list; headerless slots included), `load_static_mobys`
  over the instances before the loader's ship, `MobyTable::new(statics, spawnable count)`, the ship via
  `CreateMoby`, `Game::new` (hero init, `srand(1234)`), services (level, splines, groups), `load_pass` at
  counter 0 with the game camera, the level collision, the moby collision (class blobs and the loader's grid
  registrations, `Services::build_grid`; collision_queries.md §7), the particles and the external updates
  (class 27 `type06::emitter_update` 0x2bd100, 751 `WaterState::ripple_init` / `ripple_update` 0x2fd0e8).
  `rc_formats::moby_spawn` stays the source for the unported classes only.
* Tick: `TickHooks.mobys` runs `Scheduler::tick` on `Game.rng`; the particle hook runs `UpdateParts` on the
  same `Game.rng` (the TNT sparks' draws, type 11, are on the game stream, no longer on `ParticleSim::rng`,
  which only the `RC_PLAY=0` loop uses) with the tick's camera as 0x167240, then `Glints::update`.
  One stream: load pass → mobys (incl. 751 drops, class-27 spawns) → hero → particles → camera → counter → sky
  stars (render phase). `TickHooks.world` (`services::SharedServices`) gives the hero and the camera the moby
  collision and runs `MobyBuildMatrix` on Ratchet after his write-back (grid re-registration).
* Rendering: static mobys of a ported class are driven from the table (`MobyOcclusion::drive`,
  `MobyAnim::drive`); mobys in the dynamic slots are drawn as extra instances (one record / palette range
  per slot). +0x31 is written back after drawing (1 drawn, 0 on every skip).
* Sounds are queued in `Services::sounds` and counted (no `SoundSink`: `rc_game::audio` has no moby-sound
  entry and its own stream). Glints and type-53 sparkles are simulated but not drawn.

### Bolts 13–16 (`BoltUpdate` 0x2bb758)

* **State 0 (placed).**
  - Sets state 3, +0x73 = 0x14, mode 0x100, update/draw distance 0x20.
  - `randi(2) == 0` → spin direction bit.
  - Probe `CollLine_Fix(pos + up·0.2, pos − up·3, 0x22)`. It becomes the rest position and rest Euler
    `(0, atan(n.z, |n.xy|), atan(n.x, n.y))`, unless pvar+0x54.
  - Draws `rand_angle` ×2, then glint wait `rand_range(0, ticks(600))`.
  - Scale = class·0.65 / 0.7 / 0.75 / 0.6.
  - **4 draws.**
* **Dropped** (`BoltSpawn` 0x2bcdb8):
  - Starts in state 1 with update/draw distance 0x40.
  - Spin quaternion about `(src − bolt) × vel` at `randf(0, 720)`·π/180·dt per tick.
  - Draws: `rand_range`, `randi(2)` (≠ 0 sets the bit), `rand_angle` ×2, `randf`: **5 draws**, plus 1 with
    flags 0x12.
  - When `CreateMoby` fails, `0x15ed98 += value`.
* **State 1 (fall).** Gravity 10.8·dt², then a sphere query of radius 0.15 / 0.23 / 0.23 / 0.25, flags 0x22:
  - bounce by reflection;
  - fly at once when within the pickup radii;
  - settle (state 2, `randf(30, 40)` hop ticks) when both slopes are < 45°;
  - 2 below its spawn z and below the hero: fly with a `randf(±15°)` bend.
* **State 2 (settle).** nlerp the spin quaternion toward the rest pose and lerp toward rest + 0.25 z, with
  a hop `−((t − ½)² − ¼)·n·0.025`. At the end: state 3 with `randf(10, 20)` hop ticks.
* **State 3 (idle).**
  - Pose = rest + rest_up·k1 + row0·k2.
  - Spin x += k / 0.04, 0.03, 0.025, 0.0175 per tick and z −= 0.0125, with the tilt constant.
  - The hop finishes.
  - A glint (16-entry table 0x16eec0, offset `0x1d7850 + class·0x10`) every `rand_range(300, 600)` ticks:
    `rand_angle` when a slot is free.
  - Pickup test: xy distance < `0x1415d8` and |dz| < `0x1415dc`. These are 2.125 / 1.25 from hero init;
    each hero tick writes 3.0 / 1.75, or 12 / 4.5 with the bolt grabber 0x13d4e2. The mobys run first, so
    tick 0 uses the init values.
  - A dynamic bolt (strictly after the first dynamic slot) more than 48 away, with free slots < 200, is
    deleted when not drawn.
* **Fly** (`FUN_002bcb90` start, state 4).
  - Sound 0 (flags 0x20) at most every `ticks(3)`. A bolt started inside that window gets a random bend
    (`randf(±30°)`, `randf(0, 30°)`) instead.
  - From idle: velocity along the rest up axis, `randf(3, 7)·dt`.
  - Speed approaches 48·dt by 16·dt² per tick, with drag 10·dt².
  - When below the hero's ground point: 7·dt up, clamped 50·dt².
  - Pushed by the hero's displacement when he moves faster than 8·dt.
  - Moves by the speed toward the body point `0x13f420 = pos + R·(0, 0, 0.7)`, bent in hero space.
  - **Collected** when the distance is ≤ the speed.
* **`CollectBolt` 0x2bc4f0.**
  - `0x15ed98 += 1 / 5 / 20 / 50`, `0x13df38[level]`, the spawner's `0x14d592` s16, and `0x15ee2c` for
    challenge bolts.
  - HUD refresh.
  - Two type-53 sparkles: `rand_vec(0.7·dt, dt)`, `randi(2)`, `randf(0.4, 0.5)`: **5 draws**.
  - A placed bolt writes `0x1baea4[spawn_id]` and (mission 0xff, or the mission rule) `0x1bbb04[spawn_id]`
    = mission + 2, then `DeleteMoby`.
* The **catalogue's §4 bolt line** is right about states 0–4. The fell-through rule and the free-slot
  despawn were added here.

### Crates 500 / 501 / 502 / 505 / 511 (`CrateUpdate` 0x2ea178)

* **Init** (`FUN_002eac18`).
  - Each class-500 crate of the group (or the lone crate), in list order, draws `randi(2)`; ≠ 0 turns it
    by π/2 when x and y rotation are 0.
  - A probe from z + 0.5 down to z = 0.1 (flags 2) either snaps the crate onto the ground (Novalis:
    up to 0.30 up, e.g. crates 342–345) or stacks it on a crate of its group
    (`pos = below.pos + below.row2`, links at pvar+0xa0 / +0xa4).
  - Then 511 sets pvar+0xc6 = 100, and 501 spawns its icon moby 806.
* **Stack physics** (`FUN_002ec388`). Once per tick per stack, bottom to top:
  - The hero inside a crate's footprint (|x|, |y| < 0.55, below its origin, above −(capsule top + radius))
    breaks it.
  - Gravity 9.8·dt², clamped ±0.3, until the crate rests on the ground probe or the crate below.
  - A landing faster than dt/2 lights a TNT fuse.
* **Hit** (mask 0x1830000; exactly 0x1000000 = a touch).
  - Damage > 0 breaks the crate; 502 needs flag 0x20000.
  - TNT goes to state 5: fuse 1 with 0x20000, else `trunc(randf(5, 15))`.
  - A touch (hero capsule / standing moby) starts the full fuse.
* **TNT fuse** (state 5, `moby+0xbc`).
  - `ticks(60)·3 − 1 = 179`.
  - Beeps (sound 1, flags 0x20) at 177, 117, 57 and 13.
  - Red ambient (200, 64, 64) when `bc % 60 > 40` or `bc < 15`.
  - Explodes on the **179th** state-5 update.
* **Blast** (state 3, TNT). `ticks(20)` + 1 updates with radius `k/(20·scale)`:
  - `coll_sphere_mobys(2r, …, flags 1, damage 2)` for the hero;
  - `(3r, flags 0x30000)` for everything else.
  After the blast: delete, or hide with respawn timer `rand_range(30, 60)` when pvar+0xc8. Respawn happens
  out of view (`FastBSphereCheck`), onto its group's crate below or the ground.
* **`CrateBreakFx` 0x2eb918.**
  - Sound 0, rate-limited to one per 2 ticks.
  - Unlinks the crate from its stack.
  - 6 type-13 dust particles (5 draws each).
  - Debris mobys: n / c = 4/4, 2/2, 2/1 or 1/0 by free slots > 200 / 150 / 100. Class 357 for 500/511,
    348 for 501, 354 for 502, 363 for 505; chunks use class + 1. Each big one draws 9; each chunk 16 or 17.
  - Hides the crate: state 3, collision off.
  - TNT adds up to 10 type-11 sparks (5 draws each, more with frame load) and 2 or 5 flash mobys (class
    0x70, 3 draws each).
* **Drops.** **Correction to §4:** `CrateDropBolts` 0x2eb498 never creates bolts itself.
  - It calls `SetDeathBits(m, 0x100, pvar+0xc0)` 0x26c250, which sets the death bits:
    `0x14c190 + level·0x100` and `0x1ba950` (bit per spawn id), plus `0x1baea4` / `0x1bbb04`.
  - `SetDeathBits` returns when +0xb1 = 0xfe (no spawn flags).
  - Otherwise it calls `BoltBurst(n ± spread)` 0x275988, with n = +0xb4 and spread = n/4 (n ≥ 8) or
    1 (n ≥ 2). When +0xb1 ≥ 0, n is at least 1 and uses +0xb6 minus what the spawner already gave.
  - Flags: 1 | 4 (fl 0x100); 2 = fly to the hero at once (free slots < 50, hero flag 0x141402, hero state
    0x10 within 4); 8 = at most 3 coins (free < 100).
  - `BoltBurst` draws `N = randi(hi − lo + 1) + lo` (doubled in 0x15eea0 / 0x15ee20; challenge limiter
    0x15ee28 / 0x15ee2c). It splits N into 50 / 20 / 5 / 1 coins; with flags 8 it picks the closer of
    "3×50 broken down" and "3×1 built up".
  - Each coin: 0.5 above the crate, `rand_angle`, `randf(0, 3)·dt` ×2, `randf(3.7, 6)·dt` up: **4 draws**,
    then `BoltSpawn`'s 5.
  - A dropper with a path (pvar+0xc0 ≥ 0) aims the landing at the path (quadratic fall time, `ClampToPath`
    0x276820).
  - (Correction 2026-09-28, hero_gameplay.md: 511 is the **ammo** crate, 501 the **nanotech** crate.)
  - pvar+0xc6 ≠ 0 (511) selects ammo pickups instead: type pvar+0xcb. Re-pick among owned items via
    `0x26bff8`; count `randi(5) ≠ 0 ? 1 : 2`; each pickup draws `rand_angle` and `randf` ×2.
  - Class 501 only sets the death bits.
* **Novalis.**
  - 240 crates: 82 stacked on another crate (resting on its collision blob, collision_queries.md §7), 204 with a bolt drop, none
    with a path.
  - Multi-hit classes 0x1fb–0x1fd are not ported: no level's class table has them (G-CLS-009, dead code). Crates on moving platforms: ported 2026-09-30 (triggers.md §5b).

### Grass 724 / 725 (`GrassUpdate` 0x2fa720)

* Blends to sequence 1 over `ticks(6)` when all of these hold:
  - `VecDistance(hero.pos, pos) < 1.0` (3-D);
  - `2·dt < |hero 0x13f450|`;
  - seq B is not already 1.
* Blends back to 0 when flag bit 1 (wrap) is set on sequence 1. That happens on the advance into the last
  key interval.
* No RNG.

### Services (`services.rs`)

* Vector and quaternion library: 0x221ef8 quat→rows, 0x221c98 / 0x221ce8 matrix products, 0x221d78
  quat product, 0x221db8 nlerp, 0x221e38 half-angle axis quats (negated sine), 0x26ee30 Euler→quat,
  0x2721f0 rows→Euler (with the FPU-polynomial Ry of 0x2219a0), 0x221570 reflect, 0x2212d0 cross (= b × a),
  0x272090 axis-angle.
* Hit records 0x178580:
  - `World::get_hit` = 0x26f320.
  - `World::deliver_hit` = `FUN_0026e968`: next slot `(slot + 1) & 0x3f`; skipped when the pending record
    has a larger damage.
  - `sphere_mobys` = 0x214468 `coll_sphere_mobys` (grid walk, bounding sphere, class blob triangles and
    primitives; collision_queries.md §7); the damage test compares float bits as integers.
* Sounds go through `SoundSink` with the shared RNG (for pitch-bend draws such as crate 502's) and are
  recorded in `Services::sounds`.
* Particle spawners 13 / 11 / 53 write records in the game's order and count spawns, including unported
  types. `World::part11` is `particles::type11::spawn` (the one `PartType11Spawn`, shared with the sparks'
  split; unit test `part11_is_type11_spawn_draw_for_draw`).
* Debris and flash mobys.
* Glints; save bits; counters; `MissionState` and `Inventory` traits (defaults: no missions, empty
  inventory).
* Line / sphere probes (`World::line`, `coll_line`, `coll_sphere`) run the game's kernels with the moby pass:
  the grid 0x19bc60 (kept by `MobyBuildMatrix` / `DeleteMoby`), the class collision blobs, the pose cache
  (collision_queries.md §7). The `CrateBoxes` / `MobyCollider` stand-in is gone.

### Tests

* Unit tests (`moby_update::tests`):
  - active rule: distance boundary, visible, 0xff, groups, slot order, targets;
  - bolt flight: straight path within ±1 tick of an f64 model; 4 + 2 + 5 draws;
  - crate break: bolt classes from an independent 3-coin model; 1 + 9 + 1 + 9·coins draws;
  - TNT fuse: 179 updates, beeps 177/117/57/13, 21 blast updates;
  - grass: trigger and return.
* Novalis (`tests/classes/moby_update_novalis.rs`):
  - The level table's addresses for the 11 ported classes match the port.
  - The load pass runs 788 mobys. For 300 ticks, the active count per tick equals an independent f64
    re-computation of the rule: 134 per tick, with a draw-distance stand-in for +0x31.
  - Every idle bolt's switch to flying matches the pickup test.
  - From the spawn no bolt is in reach.
  - Standing on instance 11: bolts 11 and 10 are collected, bolt counter 2.
  - The RNG after 300 ticks is deterministic across runs.

### Not ported / open

* Hit delivery from the wrench and weapons (they would call `deliver_hit`).
* Moby collision: ported (collision_queries.md §7), with Ratchet's per-tick cylinders (`FUN_00263cb8` from
  HeroSyncMoby, `Hero::sync_cylinders`; collision_queries.md "Ratchet's cylinders"; 2026-10-19). Not modelled
  there: the camera moby 0x3ef.
* The bomb's hit-test pass-through (0x2c3b7c, 2026-10-19): a moby whose +0x74 equals 0x160770 (written by
  level03's air traffic 0x29dba8 and the flyer driver's level-3 tail 0x294b1c; −1 at load on every level) does not
  explode it unless class 0x336 (`bomb::passes_through`, `Services::pass_update`). Leftovers G-HERO-045.
* ~~Multi-hit crates.~~ (dead code: on no level's class table, G-CLS-009.)
* Platforms.
* Snapshot sphere of `fun_0020def8`.
* Mission and inventory state.
* `FUN_0029ab50`.
* Frame-load throttle inputs.
* Every other class (ported since: 660, 1135, 304 / 1456–1465, next section).

## In the port (2026-09-27): Blarg flyers, teleporter pads, gold-weapon offers

Code: `crates/rc-game/src/moby_update/classes/{flyer,teleporter,item_offer}.rs` (registry `classes/mod.rs`), the
live-owner emitter `rc_game::particles::type06::emitter_update_live`, `World::joint_point` and
`Services::{joint_lists, unported}` (`services.rs`). Written on standard `f32` (and `std` trig), not the PS2 float
model; the rand draws are the game's, in its order. Level01 addresses. Checked against the Novalis spawn savestate
by `compare-novalis-spawn` §f2 (trace_results_novalis.md "Blarg flyers and the challenge gate").

### Blarg flyers 660 (`BlargFlyerUpdate` 0x2f4428 + `FlyerPathDriver` 0x2f5168)

Full description in the module doc of `flyer.rs` (pvar map, states). In short:
* **Update:** hit 0x800000 → the kill; else the driver, mode |= 0x1000, pvar+0x2c = 100, +0x2b = 1; distance factor
  `f = clamp(1 − (d − 20)·0.5/180, 0.5, 1)` (d = |pos − camera 0x167240|, gp−0x510c/−0x5108/−0x5104 = 0.5/200/20)
  on the base scale (+0x170) and speed (+0x174) saved in state 0; the altitude fade alpha +0x23 (z ≥ 175: 0,
  125..175: `trunc((175 − z)·128/50)`, ≤ 125: 0x80).
* **State 0** (load pass): the spline `0x1b0930[pvar+0x74]` loses its closing point when it is within 0.5 of the
  first (the shared spline's count−−), `FUN_0028b410` (per point w = chord, z through +0.5 −0.5), the nearest point
  `FUN_0028b510` when pvar+0x60 < 1, the first segment, the rotation from the tangent, the bob phase, the arc
  lengths `FUN_0028bb90` (per point w; 20 samples, see hardware_fidelity_layers.md "Result-level reproductions"),
  the emitters placed, update distance 0xff, state 1 (3 with a start delay). Draws only for offsets > 500 and a
  −1 bob phase: none on Novalis.
* **State 1:** Hermite flight over Kochanek–Bartels tangents (`FUN_0028b8c8`, `FUN_0028ba80`) at the speed with the
  arc-length correction; heading = the step, yaw eased by `1/pvar+0x110`, pitch and roll through `SpringTurn`
  0x26cef0 when seen (`FastBSphereCheck(draw distance, bsphere/1024)`, 0x275690), else (0, 0, heading); position =
  last tick's curve point. For each set flag bit k (bits 1–4) the linked emitter `pvar+0x13c + 4k` (runtime index,
  spawn remap) goes to joint list j (`FUN_002645a8`: the partial pose of the list's last joint, scale/1024, rows,
  + position) with P+0x80 = unit(joint − position), P+0x70 |= 0x1000.
* **Emitters:** `type06::emitter_update_live` copies the moby's position, rotation, P+0x70 and P+0x80..0x8b into the
  particle owner before 0x2bd100 runs, so the cull, the spawn point and the velocity follow the flyer (engine and
  `port_sim`). The Novalis flyers' exhaust texture is part_defs[4] frame 0, the darkest smoke frame, drawn
  additive with vertex colour 0x2c: next to invisible in the engine (particles verified along the flight path in
  `port_sim`; not verified on screen).
* **Not ported, counted in `FxStats::unported`:** the kill effects (skill point 0x13d408[0], level sound 1, banner
  0x53d6, `SpawnBeamExplosion`, `FUN_0030be70`; the port does state 0x65 and the `DeleteMoby`s), group
  synchronisation, flag-bit-0 type-10 sparks (the draws are made), the level-3/9 combat block, class sounds (not
  660), debug lines. Engine: alpha 0 hides a driven static moby; the partial fade is not rendered.

### Teleporter pads 1135 (`TeleporterPadUpdate` 0x308bd8)

* **Challenge gate:** pvar+0x3c ≠ 0 and times completed 0x15ee20 = 0 → no collision, mode |= 0x41, return before
  the states (`Services::counters.times_completed`, from `GameState::global.completes`). Instance 961 on Novalis.
* **State 0:** update distance 0xff; three `CreateMoby(315)` arms (draw distance 0x40, +0x31 = 1, the hero moby's
  light word and ambient, the pad's mode / position / rotation, yaw + k·2π/3); state 6 (pvar+0x38 ≠ 0 or the spawn
  id collected / killed) or 5 with `FUN_003097e8` (hidden); arm placement `FUN_003092d0` (z − 0.4, rotation y −20°,
  offset along row 0 by `spread·2.4 − 1.2`, rotation y `tilt·(−π/3) − 20°`, `MobyBuildMatrix`).
* **State 6 → 1; state 1** idles until the pvar+0x08 mission is done. Not ported (counted): the activation
  cuboid (pvar+0x04), the group command, the on-pad prompt and △, the teleport states 2/3/4/7/8.

### Gold-weapon offers 304 and props 1456–1465 (`ItemOfferUpdate` 0x2e1ac0)

* **Challenge gate:** pvar+0x40 = k ≠ −1, k even, times completed = 0 → `DeleteMoby` (the load pass deletes the
  five even offers and their props on a first playthrough).
* State 0: 304 → 3 when the gold weapon `0x13e520[0x1deb10[k]]` is owned, else 1 (the help registration
  `FUN_0027b480` is not ported: the success branch is taken, as the savestate shows, and counted); props → 2.
  State 2 deletes a prop once its weapon is owned. State 1 (the buy prompt) and state 4's memory-card save: not
  ported (counted).


## In the port (2026-09-27): debris pieces and explosion flashes (`classes/debris.rs`)

One module for the breakables' effect mobys; every breakable calls the same spawners with its own arguments.
Before this port the pieces and flashes were created with no update (mode 2) and hung in the air.

* **Registry, from the level tables.** Every level table read (01, 05, 06, 07, 09, 11, 12; table at 0x20bb00
  in 01, 0x218c80 in 05, 0x201b80 in 06, 0x211d00 in 07, 0x209380 in 09, 0x21b900 in 11, 0x20cb80 in 12) maps
  the same classes to its copy of each function:
  - debris update (01: 0x2c5218): 149, 348, 349, 354–359, 363, 364;
  - flash update (01: 0x2c22a8): 112 (0x70), 1192.
  The crate chunk classes of 503 / 504 / 506–510 (351/352, 360/361, 366/367, 369/370) have no entry anywhere:
  spawned without an update, as in the game. Unit test `level_table_lists_the_shared_classes` checks the
  level01 table against the port's lists.
* **`DebrisSpawn` 0x2c5080** (`debris::spawn`, moved from `services.rs`; its only level01 caller is
  `CrateBreakFx`). Pvars: 0 parent, 4 spawn z, 8 radius = `randf(a, b)/4`, 0xc s16 bounces, 0xe s16 timer
  `ticks(20)`, 0x10 velocity, 0x20/0x24/0x28 spin (x = 0). 6 draws.
* **`DebrisUpdate` 0x2c5218** (`debris::update`), every tick: `v.z −= 20·dt²`, `pos += v`, `rot += spin`.
  - State 0: 3 below the spawn z → state 1. Else, once the timer has run out (`FastDecTimer` ≠ 0) and bounces
    are left, `coll_sphere(radius, pos, 2, none)`; with `v · n < 0` (n = CollOutput +0x40, 0x174300): pos =
    pushed centre (+0x30, 0x1742f0), bounces −1, `v = reflect(v, n)·0.75`, spin (0, `randf_sym(0, 360)°·dt` ×2):
    2 draws. No bounce left: the piece falls through the floor.
  - State 1: scale ×0.9 (`gp−0x5790`), alpha −4 (`gp−0x578c`); deleted when alpha < 4 (33 updates).
  - Constants (level01 .sdata, gp = 0x166c00): `gp−0x57b8` 0.0, `gp−0x57b4` 360.0, `gp−0x5790` 0.9,
    `gp−0x578c` 4.
* **`FlashSpawn` 0x2c20e0** (`debris::flash_spawn`, moved from `services.rs`) / **`FlashUpdate` 0x2c22a8**
  (`debris::flash_update`): scale `(T − timer)·size/T`, Euler += π·dt each axis, `FastDecTimer(timer)`:
  deleted when it runs out, else in the second half alpha = `timer·a₀/(T/2)`. No draws in the update.
* Standard `f32` arithmetic; no new fidelity layer and no result-level reproduction needed.
* **Tests** (`debris::tests`): registry and level table; a piece thrown up bounces once on a floor within ±1
  tick of an f64 model (z per tick within 1e-4 before the bounce), 2 draws on the bounce and none otherwise,
  falls through, fades 33 updates, deleted; a piece with no bounces falls through (fade start = model);
  spawn draws and spin from the stream; the flash grows, fades in its second half and is deleted when its
  timer runs out; two runs identical.
* **Engine** (`RC_PLAY_SCRIPT="0-5:rstick 1 0,20-226:stick 0 -1,250-310:rstick 1 0,330-331:press SQUARE"`,
  `RC_SCENE=0`): the crate breaks at trace tick 341 (15 dynamic mobys: debris + bolts); frame 360 pieces in
  flight, frame 400 none in view; live dynamic mobys back to the pre-break 3 (free slots 243) at tick 466.
  Two frame-400 runs: identical PNG (md5) and identical per-tick traces.
* Not ported here: the other breakables' own effect code (704 rock, 729 wall, 778 prop use other spawners,
  e.g. 0x2f8530); they should call these spawners only where the game does.
