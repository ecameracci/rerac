# Shadows: what the original draws, and the native port

**Status: built (2026-09-27), packages S1–S5; see "As built" at the end.** The research below was checked against
the disassembly and PCSX2 RAM while building; corrections are marked *(corrected)*. First written as research
(2026-09-27). Addresses are **level01.elf** unless marked "boot"; gp = 0x166c00. Decompiled C is in
`work/decomp/level01.elf/`. Tags: **[H]** read in code or data and cross-checked, **[M]** read in code but the decompile hides a detail (VU0 broadcast
fields, shift signedness), **[L]** inferred. Names in *italics* are suggested names for unnamed functions. They are not applied in Ghidra, which was
used read-only.

## 0. Answer in one paragraph

R&C1 has **one** dynamic shadow system, and nothing else is dynamic. Selected **characters** cast a shadow: Ratchet, enemies, critters and NPCs.
Crates, bolts, pickups, props and the level itself do not. Each casting class carries its own **shadow proxy** in the class data: a few
capsules and spheres attached to skeleton joints. Every frame the game projects each proxy's outline along a shadow direction and extrudes it
into a **shadow volume**. It clips the volume to a thin **height slab** around the ground under the caster, which it finds with collision probes.
It then draws the volume with a **z-pass stencil** that the GS builds in the frame buffer's destination alpha. Every pixel inside a volume is
darkened to **75 %** of its colour, with hard edges. Casters are drawn after this pass, so they never receive shadows, their own or anyone else's.
All static shading, such as the darkness under trees, roofs and overhangs, is **baked** into the tfrag, tie and shrub vertex colours. The port
already reproduces those. A faithful native version is a small screen-space **shadow-volume pass** driven by the game's own data and rules. It is
one general system, not one per object. Bevy's built-in shadow maps would look clearly different (§5).

## 1. Shadow types in the original

| Type | Exists? | Where |
|---|---|---|
| Blob / drop shadow (textured quad or decal) | **No.** No code path draws a shadow sprite. The FX texture named `lame_shadow` exists only in the later games' Wrench lists, not RAC1's (docs/formats/textures_rac1.md). | – |
| Projected silhouette of the character's **proxy shapes** as a **stencil shadow volume** | **Yes, the only dynamic shadow** | §2, §3 |
| Shadows from dynamic lights (point lights) | **No.** Point lights only add Lambert terms (`UpdateAllPointLights` 0x2528a8, docs/plan/render_pipeline.md §6). | – |
| Baked shade in tfrag / tie / shrub colours | **Yes, static.** It lives in the per-vertex base colours and the per-vertex or per-instance light-set choice. | §4 |
| Ratchet's own shadow | Yes, the same system, with its own direction (index 1), its own ground probe and a state gate. | §3.2 |
| Ratchet dimming in covered areas | Not a shadow. It is a light-set cross-fade by environment zone (`HeroEnvLighting` 0x26be04, docs/plan/world_animation.md). The roadmap lists it as not yet ported. | §4 |

## 2. Which objects cast: class data, not per-object code

- **Class header byte 0x0f** (`shadow` in docs/formats/moby_rac1.md) gives the size of the class's shadow block in quadwords. The block sits just
  before the skeleton pointer (class +0x14). `InitMobyInstance` 0x263488 (boot 0x20c5f0) checks it: when class+0xf ≠ 0 it sets
  **mode |= 0x400**, **+0x7f = 0x18** (shadow range byte, 24), +0x84 = +0x88 = 0 (ground slab, empty) and **+0xbd = 0** (direction index). The
  port already mirrors the init (`rc_game::moby_runtime` `mode::CLASS_F`, `b7f`, `bbd`); the slab fields do not exist yet. **[H]**
- **Shadow block format.** This was not known before; it was read from the consumer, fun_00227740, and checked on the data. It is a list of records,
  each with a common header: `u8 type`, `u8 last` (1 marks the final record), `u16 size`.
  - `type 0` **sphere**, 0x20 bytes: `+4 s32 joint`, `+8 s32 segments`, `+0x10 vec4 offset` (xyz in joint space, w = radius, model units).
  - `type 1` **capsule** (tapered), 0x30 bytes: `+4 u16 joint A`, `+6 u16 joint B`, `+8 s32 segments A`, `+0xc s32 segments B`,
    `+0x10 vec4 A`, `+0x20 vec4 B` (w = radius at each end).
    - A segment count of **0** gives a flat end (two points).
    - A **negative** count −k moves that end **away from** the other one by k·16/4096 of the segment and makes it
      flat *(corrected: 0x29bb98 is `A += (A − B)·(k·16/4096)`; checked in RAM)*.

  All-levels check (scratch script over `extracted/cache/v1/wad/levels/*/core_data.lump`): 154 class entries in 19 levels, 79 distinct classes.
  1,713 records: 1,668 capsules (0x30) and 45 spheres (0x20). Every block ends exactly at its `last` record, and every joint is below the class
  joint count. At most 21 records per class (Ratchet). Segment counts: 4, 5 and 6 dominate, the range is 0 to 16, and 152 are −10. **[H]**
- **Casters on Novalis** (level 01): class 0 Ratchet (21 prims), 10 (12 thin capsules; the class the scene loop special-cases), 459 robot trooper,
  572 / 865 / 866 amoeboids, 577 critters, 634, 774 talking NPC, 811. Other levels add their own characters, for example 580, 612, 786, 788, 914 and 1422.
  **No crate, bolt, pickup or prop class has a shadow block.** **[H]**
- **Per instance** the shadow is live only when:
  - mode 0x400 is set;
  - the slab is non-empty (+0x88 > 0), filled each frame by the class's update code (§3.2);
  - the shadow's projected sphere passes the view cull;
  - +0x7f ≠ 0, which cutscenes and Ratchet's timers use to switch it off.
- **mode 0x800** also defers a moby, but without a shadow. Example: `CreateMoby(0x509)` with `|= 0x806` in `GameStateUpdate` 0x2a4080. **[H]**

## 3. How it is drawn

### 3.1 Frame order

1. **Setup.** `DrawMobys` 0x264e18 (boot 0x20d460) calls `MobyProc(…, param_4 = 1)` 0x26a7a0 (boot 0x211808). With param_4 set, every moby
   with mode & 0xc00 is deferred into a scratch list at 0x70003000.
2. **Deferral and cull.** For a deferred moby without 0x800 and with +0x88 > 0, MobyProc does the following:
   - It moves the moby's bounding sphere along the shadow direction (table 0x1af000, index +0xbd) to the middle of the slab.
   - It culls that sphere with the normal moby cull.
   - It records `{moby, size = min(((+0x7f·1024 − depth) >> 1), 0x1000), guard-band flag}`.

   When the first pass ends, it writes a placeholder tag (pointer at gp−0x6c00 = 0x160000) and then draws the deferred mobys. The list is copied to
   **0x1aca80**; the count is at 0x15fffc (gp−0x6c04). Casters are therefore drawn **after** the placeholder. **[H]** (The shift signedness is
   **[M]**; see §7.)
3. **Frame position.** `DrawMobysCleanUp` 0x264d68 (after `ProcessMobyAnimData`) calls *SpliceShadowChain* `fun_0020d060` 0x264a18 (boot 0x20d060).
   If the count is non-zero, it builds the shadow sub-chain and links it into the placeholder. The GS order in the frame is:

   sky → tfrag → tie → shrub → billboards → non-deferred mobys → **shadows** → deferred mobys (the casters) → draw callbacks → particles →
   AA blit → HUD.

   **No VU1 program is used.** The EE transforms the shapes with VU0 macro code; the trig uses VU0 micro calls `vcallms 0xc80/0xc90`. It then
   sends GIF packets through VIF1 DIRECT (`0x50000000|n`). **[H]**

### 3.2 Direction and ground slab (game logic)

- **Direction table 0x1af000** (4 × vec4; `+0xbd` selects the entry).
  - **Index 0 (everyone but Ratchet)** is set each tick by *UpdateShadowDir* `fun_0020cfd0` 0x264988, called from `GameStateUpdate`,
    `InLevelFrameUpdate` and `CutsceneModeUpdate`: `(0.14·cos φ, 0.14·sin φ, −0.99)`. φ = `FastArcTan` of the hero moby's first directional light
    (`fun_0020d510` 0x265130 returns the light bank's `dir_a`, cross-faded by +0x3a).

    The result is **almost straight down**, leaning about 8° along the key light's horizontal direction. **[H]**
  - **Index 1 (Ratchet)** is set by *UpdateHeroShadow* `FUN_0022a260`, called from `HeroUpdateAlt` 0x228000 and from 0x228870. Ghidra labels
    0x228870 "PatchShrubGifs", but it is a hero update. The direction is `(cos p·cos φ, cos p·sin φ, −sin p)`.
    - The pitch p moves by at most **0.052 rad per tick** towards **0.97 rad** (56° below horizontal; the shadow falls clearly to the side, away
      from the light).
    - The target is **1.5 rad** (86°, almost under him) while air ticks 0x13f65e ≠ 0 or in group 4 (jumping). In the air, the shadow slides
      under him within about 10 ticks.
    - The shadow is off (mode &= ~0x400) in hero groups **0x12** (water surface), **0x15**, **0x16** (Hoverboard) and in state **0x7d**; otherwise
      mode 0x400 is set and +0xbd = 1. **[H]**
- **Slab** (+0x84 = lo, +0x88 = hi; hi ≤ 0 means no shadow). The class code chooses one of these:

  | Function | Used by | Rule |
  |---|---|---|
  | *ShadowSetGround* `FUN_0026eff8` | `PathEnemyUpdate` 0x2e6bf0 | `[z − 0.2, z + 0.2]` around a given z |
  | *ShadowProbeDown* `FUN_0026f020` | `AmoeboidUpdate`, `GroundCritterUpdate`, `TalkingNpcUpdate` | `CollLine_Fix` from pos.z + 0.5 to max(pos.z − 16, 0.5), flags 0x22; hit → `[hit − 0.2, hit + 0.2]`, miss → 0 |
  | *ShadowProbeAlongDir* `FUN_0026f0e0` | cutscene / scene actors (0x2a4080 loop over 0x16ce58, `CutsceneModeUpdate`, `VendorModeUpdate`) | *(corrected)* d = direction 0 scaled to a horizontal length of 1 (0x221460). Probe 1: **vertical**, 8 units down from `c − d.xy·0.75r`; a miss = no shadow. Probe 2: from `c + (d.xy·0.75r, r/2)` along d rescaled to a 3-D length of `(z − hit₁)/(−d.z)`, so it only reaches ~14 % of the way down (a game quirk: two normalisations mixed). `lo = min − 0.25`, `hi = min(max + 0.25, lo + 4)` |
  | *UpdateHeroShadow* `FUN_0022a260` | Ratchet | four probes along his direction from the bounding-sphere centre `c + (cos φ·0.7, sin φ·(±0.5), 0.4)·r` (table 0x17c780: (0.7, −0.5), (0.7, 0.5), (−0.7, −0.5), (−0.7, −0.5); the 4th repeats the 3rd, a data quirk), each reaching 0.5 below the seed height. The seed is the first of ground z 0x13f628 (≥ 1), water level 0x13f640 (≥ 1) or liquid level 0x13f644 (> 1); none: `lo = hi = 0`. *(corrected)* min and max start **at the seed**: `lo = min(seed, hits) − 0.12`, `hi = min(max(seed, hits) + 0.24, lo + 3)` |

  **[H]** (the probe arithmetic). The ±0.7/±0.5 offsets use `cos φ·x` and `sin φ·y` rather than a rotation, as written. **[H]**
- **Size by distance** (the `size` word): `min(((+0x7f << 10) − ftoi0(d − r')) >> 1, 0x1000)`, d = view depth of the shadow sphere
  (×1024), r' its radius: full size up to 16 units, then linear down towards 0 at **24** units (+0x7f = 0x18; amoeboids 0x16, the
  talking NPC 0x1a). The proxy is **scaled towards the joint frames and the moby origin**; it does not fade in alpha. **[H]** (§7.1:
  resolved.)

### 3.3 Geometry (the proxy volume)

1. *BuildShadowList* `fun_00227740` 0x29b948 (boot 0x227740) walks the list at 0x1aca80. For each moby it poses the joints with
   `moby_coll_xform_cached` (boot 0x20fa90; level01 0x267fc0, docs/plan/collision_queries.md). Each record point is transformed as
   `P = pos + R·((J·p)·s/1024·size/4096)`, with R the rotation rows +0xc0..+0xe0 and s the scale +0x2c *(corrected: size is 4.12, 0x1000 = 1)*.

   Output goes to **0x1acc00..0x1aeb80** (8,064 bytes): per moby a header `{prim count, guard, lo, hi, dir}` followed by the posed prims. The
   loop stops when the cursor passes the end, so only about 7 Ratchet-sized or about 25 amoeboid-sized casters fit. **[H]** (That the radius
   travels in w and is scaled by s·size is **[M]**.)
2. *DrawShadows* `fun_00227548` 0x29b750 (boot 0x227548) handles each prim in turn:
   - It sets the direction with *ShadowSetDir* `fun_00226fb8`: the unit dir, `dir·1000` and a perpendicular.
   - It builds the outline perpendicular to dir:
     - A **sphere** (*ShadowSphereOutline* `fun_00227d80`) becomes an n-gon of radius r.
     - A **capsule** (*ShadowCapsuleOutline* `fun_00227ed0`) becomes a half-arc around each end. The arcs are joined by tangents at
       `asin((rA − rB)/|AB⊥|)`. If one sphere contains the other, it falls back to a full circle. Flat ends are allowed.
     - Each outline point P is paired with P + 1000·dir.
   - *ShadowClipToSlab* `fun_00228520` cuts every pair to **z = hi** (top ring) and **z = lo** (bottom ring). The result is a closed prism: the
     proxy's silhouette slid along dir, as thick as the slab. **[H]**
   - *ShadowProjectAndClassify* `fun_00227a08` projects the prism to the screen. With the guard flag set, it drops faces that have a vertex outside
     the clip box. It classifies each side quad by the sign of its screen-space area (2 = one facing, 1 = the other) and does the same for the top
     and bottom caps. **[H]**

### 3.4 GS state: the destination-alpha stencil

1. **Clear.** *ShadowAlphaClear* `fun_002271d0` draws full-screen 32-px sprites with ALPHA_1 FIX = 0 (so RGB = Cd) and RGBAQ 0x7f808080. Every
   pixel's **dest alpha becomes 0x7f**. TEST_1 = 0x31001: alpha test NEVER, AFAIL FB_ONLY, Z test ALWAYS, no Z write.
2. **State block** 0x1bc3d0 (NTSC) / 0x1bc440 (PAL):
   - FRAME_1 = `0x00ffffff_00080070`: **FBMSK = RGB, so only alpha is written**.
   - TEX0_1 = `0x6_64020e00`: TBP 0xe00 = **the frame buffer itself**, 512×512, CT32, TCC = RGBA, MODULATE.
   - TEX1_1 = 1, CLAMP_1 = 0, ALPHA_1 = 0x80_00000044.

   Each face gets ST = its own screen position / 512, so it reads the destination pixel's alpha. **[H]**
3. **Per prim**, two passes (*ShadowDrawPass* `fun_00227140` → *ShadowEmitFaces* `fun_00228598`) with block 0x1bc370. TEST_1 = 0x51001: alpha
   test NEVER, AFAIL FB_ONLY, **Z test GEQUAL**, no Z write. Side quads are drawn as TRISTRIPs (PRIM 0x54), caps as TRIFANs (0x55).
   - Class 2 faces use vertex alpha **0x82**: `A = ⌊A·0x82/128⌋`, so 0x7f → 0x80.
   - Class 1 faces use vertex alpha **0x7f**: `A = ⌊A·0x7f/128⌋`, so 0x80 → 0x7f.

   The colours are at 0x160420 / 0x160424. This is a **multiplicative up/down counter** in destination alpha. For the counts that occur, bit 7
   ends up set exactly when (visible faces of one orientation) − (visible faces of the other) > 0. That is the classic z-pass stencil test
   ("the surface lies inside the volume"). **[H]** (checked by hand for up to 4 nested volumes).
4. **Resolve.** *ShadowResolve* `fun_00227378` restores FRAME_1 (FBMSK 0) and sets ALPHA_1 = `0x20_00000064`, i.e. `(0 − Cd)·0x20/128 + Cd` =
   **0.75·Cd**. TEST_1 = 0x3d801 enables **DATE/DATM = 1**, so only pixels with dest alpha bit 7 pass. It draws full-screen sprites in black.
   Then it restores TEST_1 0x5360b and ALPHA_1 0x80_00000044. **[H]**

### 3.5 What it looks like (no screenshots)

- **Shape.** A hard-edged silhouette made of rounded capsules and discs. It moves with the skeleton: legs scissor, the head and ears bob. It has
  no fingers, weapons or fine detail.
- **Darkness.** Uniformly 25 % darker. Overlapping shadows do not get darker; the result is binary. There is no penumbra.
- **Direction.**
  - Ratchet's shadow falls clearly to one side of him, away from the level's key light.
  - When he jumps it slides under him within about 10 ticks, then swings back out after landing.
  - Enemies' shadows sit almost straight under them.
- **Wraps the ground.** The volume darkens whatever lies inside the slab: the ground, small bumps, the base of a wall and the lower band of a
  crate or other non-casting moby standing there. It never reaches a floor far below the probed ground or anything above hi.
- **Distance.** Shadows shrink to nothing between 16 and 24 units from the camera.
- **Casters.** A caster is never darkened, not even its feet.
- **Ratchet.** He has no shadow while swimming on the surface, on the Hoverboard or in the level-16 group.

## 4. What is already baked

- **Tfrag.** Per-vertex base colours (555) plus per-vertex directional light sets, with no occlusion test. Some vertices select an empty set: set
  15 on 46 Novalis vertices has no direct light. The port reproduces this at load (`rc_formats::tfrag_light`, docs/plan/tfrag_lighting.md).
- **Tie.** Per-instance ambient RGBA5551 per light slot plus directional light sets. Reproduced in `tie_light.rs` (docs/plan/tie_lighting.md;
  compared with PCSX2 to ±1).
- **Shrub.** Instance colour plus light sets (docs/plan/shrub_lighting.md).
- The darkening under trees, roofs, bridges and overhangs is the artists' baked vertex colour and light-set choice. The engine has no
  run-time occlusion. The port therefore **already shows all static shade** it will ever need. Adding any shadow on world geometry beyond §3
  would darken twice and would not be faithful. Nothing of the moby shadow volume is baked.
- **Related, not a shadow.** Ratchet's light cross-fade by environment zone (`HeroEnvLighting` 0x26be04) dims him under cover. It is listed as
  not ported (roadmap, fog zones entry) and is worth scheduling next to this work (decision 6).

## 5. Classification under the hardware policy

| Mechanism | Visible result | Kind | Native way |
|---|---|---|---|
| Class shadow blocks, mode 0x400, +0x7f / +0xbd, per-class slab probes, directions 0 / 1, hero gate and pitch smoothing, size-by-distance, 8,064-byte budget, list order | Which characters have a shadow, where it falls and how big it is | **Game logic and data** | Port as is, in IEEE floats (no PS2 float model; policy phase 1) |
| Proxy outline (n-gon / tangent capsule), extrusion along dir, slab clip | The silhouette shape and how it wraps the ground | Game geometry | Port as is (CPU mesh per frame) |
| Z-pass volume counting in destination alpha by texture feedback (TEX0 = frame buffer, MODULATE 0x82/0x7f, FBMSK, DATE) | "Inside the volume" test | **Hardware trick** | A native count target: +1 / −1 per face, depth-tested against the scene. Exact integer counting. Same result for every count the GS counter gets right. |
| Resolve 0.75·Cd with DATE | 25 % darker, hard edge | Result | Full-screen multiply where count > 0. The factor is set so that the display byte is ×0.75 (§6.2). Record under "Result-level reproductions". |
| Guard-band face drop (param_3), 512-px strips, NTSC/PAL block pair | None intended; occasional missing slivers at the screen edge | Hardware | Not reproduced; the GPU clips |
| Frame order (casters after the resolve) | Casters are never darkened | Game rule | Draw the casters after the shadow pass (§6.3) |

**Would Bevy's built-in shadow maps match?** No, and they would look clearly wrong:

1. **Every mesh casts.** Trees, ties, walls and terrain would throw large dark shadows on top of shade the artists already baked.
2. **One light direction.** A single directional light cannot reproduce Ratchet's 56° side-cast shadow and the enemies' vertical ones at the
   same time.
3. **Detail and softness.** The shadows would have full-mesh detail (weapon, hands, hair) with soft PCF edges, cascade shimmer and aliasing. The
   original has smooth hard-edged capsules.
4. **Self-shadowing.** Casters would shadow themselves.
5. **Lighting model.** The darkness would follow Bevy's lighting model, not a flat 25 %.
6. **Integration.** Shadow maps only affect `StandardMaterial` / PBR lighting. Every custom shader here (tfrag, tie, shrub, moby) would need
   shadow-map sampling added.

A version restricted to proxy casters and a slab receiver test would need two light views and per-material receiver code. It would still be
resolution-limited, so it is more work than the volume pass for a less exact result. A blob decal (one dark disc) is cheaper but loses the
animated silhouette and the direction. It is not recommended.

**Recommendation.** A native screen-space shadow-volume pass driven by the game's own data and rules (§6). It is one general system: any class
with a shadow block, including other levels' characters, works with no per-class work. The only per-class code is the one-line slab probe each
update already calls in the game.

## 6. Native implementation plan

### 6.1 Data flow

```
rc-formats   moby_shadow.rs      class shadow block → Vec<ShadowPrim>
rc-game      shadows.rs          per tick: directions 0/1, hero gate/pitch, slab probes (+0x84/+0x88), casters list
rc-engine    shadow_volume.rs    per frame (CPU): pose joints → outlines → prisms clipped to [lo, hi], size by depth, byte budget
rc-engine    shadow_render.rs    per view (GPU): count target (+1/−1, depth-tested) → multiply resolve; casters drawn after
```

### 6.2 GPU pass (Bevy 0.19)

- **Scheduling.** Two render-world systems in `Core3d`, ordered `.after(main_opaque_pass_3d).before(main_transparent_pass_3d)`. Those two passes
  are chained in `Core3dSystems::MainPass`, in bevy_core_pipeline-0.19.1 `core_3d/mod.rs`.
- **Count target.** An `R16Float` target, cleared to 0, with MSAA matching the main view. `Rgba8`/integer formats cannot blend, and R16Float
  counts integers exactly.
  - Volumes are drawn with the **main depth texture as a read-only depth attachment**: `GreaterEqual` (reverse-Z = GS GEQUAL), no depth write,
    no culling.
  - The fragment writes `front_facing ? +1 : −1` with additive blending.
  - Bevy's depth format is fixed at `Depth32Float` (`CORE_3D_DEPTH_FORMAT`), which has no stencil. That is why a count target is used instead
    of a stencil buffer; no Bevy changes are needed.
- **Resolve.** A full-screen triangle into the main colour target that discards where count ≤ 0.5. It is a multiply blend (`dst·k`).
  - The port stores display bytes through `srgb_to_linear` (layer A3). The display byte ×0.75 is therefore a **linear factor of about 0.53**:
    within 2/255 on dark tones and exact at the gamma-2.2 limit.
  - The alternative is to read the colour and write `srgb_to_linear(0.75·linear_to_srgb(c))` exactly. That needs a copy of the target; see
    decision 2.
  - With MSAA, the pass runs per sample (`sample_index`, `textureLoad` of the MSAA count target).
- **Toggles.** The pass runs under Lighting Original (graphics_options.md); `RC_SHADOWS=0` turns it off; `RC_SHADOW_DEBUG=1` shows the count target. The pass is skipped when the list is empty.

### 6.3 Casters after the resolve

The game draws deferred mobys (0x400 with a live shadow, and 0x800) after the resolve. The port should move those draws from
Opaque3d/AlphaMask3d to the late phase: `Transparent3d` with Z write, as `GsPass::LateTested` already does for the metal pass.

- The metal pass of a caster must still land after its base. It needs a sort bias or its own late ordering.
- The alternative is a caster mask (depth or ID of casters) that the resolve skips. It keeps the phases but costs an extra target (decision 3).

### 6.4 Packages

At most 3 at once (the concurrency cap). Ownership lists exclude files other running agents hold; re-check `git status` before dispatch.

**S1: shadow block loader** (rc-formats, S). No dependencies.
- *Files:* `crates/rc-formats/src/moby_shadow.rs` (new), `crates/rc-formats/src/lib.rs` (one `mod` line), `crates/rc-formats/tests/moby_shadow_golden.rs`
  (new), loader snapshot rows for this test only in `crates/rc-formats/data/loader_snapshots.tsv`.
- *Tests:* golden over levels 0–18.
- *Acceptance:*
  - 154 class entries and 1,713 records (1,668 capsules, 45 spheres).
  - Invariants: sizes 0x20/0x30, every block ends on `last`, joints below the joint count, counts in {−10, 0, 4..16}.
  - Proof that the test can fail (break one thing, restore it).

**S2: game-side shadow state** (rc-game, M). Depends on S1 only for the class-has-shadow bit (already in `moby_runtime`).
- *Files:* `crates/rc-game/src/shadows.rs` (new): *UpdateShadowDir*, *UpdateHeroShadow* (gate, pitch smoothing, four probes), *ShadowSetGround*,
  *ShadowProbeDown*, *ShadowProbeAlongDir*, and the direction table as a resource. One call line in each of `moby_runtime.rs` (fields +0x84 / +0x88),
  the hero post-update, `moby_update/classes/{critter,amoeboid,path_enemy}.rs`, and the scene/cutscene actor update.
  - These files are currently being edited by other agents, so dispatch S2 after they land.
- *Tests:*
  - Unit tests of each probe rule on synthetic collision.
  - A Novalis disc test: at spawn, Ratchet's slab lies around his ground z; each critter's slab lies around its ground.
  - PCSX2 compare at the Novalis spawn savestate (rc-trace): hero moby +0x84 / +0x88 / +0xbd, table 0x1af000..0x1af01f, and critters' +0x84 / +0x88.
- *Acceptance:* the trace values match within the existing float tolerances (record them in hardware_fidelity_layers.md "Tolerances").

**S3: volume builder** (rc-engine CPU, M). Depends on S1 and S2.
- *Files:* `crates/rc-engine/src/shadow_volume.rs` (new, pure functions and unit tests), wired from a new system in `shadow_render.rs` (S4).
- *Content:*
  - Posed joint matrices from the evaluator used by `moby_anim`, cross-checked against the 0x267fc0 variant.
  - Outlines exactly as `fun_00227d80` / `fun_00227ed0`: n-gon; tangent-joined arcs; containment fallback; flat and negative ends.
  - Slab clip, size-by-depth, the 8,064-byte budget with the game's list order, per-caster direction.
- *Tests:* closed prisms with consistent winding; top ring at z = hi and bottom ring at z = lo; outline radius; tangent angle; each end case.
- *Acceptance:* a PCSX2 dump of **0x1acc00..0x1aeb80** at the spawn savestate agrees with the port's posed prim list (points, radii, lo, hi, dir)
  to 1e-3 units. That also settles the [M] items in §7.

**S4: GPU pass** (rc-engine render, M). Depends on S3.
- *Files:* `crates/rc-engine/src/shadow_render.rs` (new), `crates/rc-engine/assets/shaders/shadow_volume.wgsl` and `shadow_resolve.wgsl` (new),
  `main.rs` (one plugin line, minimal insertion).
- *Tests:*
  - `RC_SCREENSHOT_FRAME` captures at the Novalis spawn, looked at with Read.
  - With `RC_SHADOW_DEBUG=1`, the mask matches the expected silhouette.
  - Two runs are byte-identical.
  - `RC_NOVSYNC=1` fps within 2 % of the run without shadows.
  - MSAA 1× and 4×.
- *Acceptance:*
  - Ratchet's shadow falls to the side, opposite the light; it goes under him in a jump; it is ×0.75 display bytes (sample pixels in and out).
  - Enemy and critter shadows sit almost under them.
  - It wraps the ground and wall bases within the slab.
  - Adding the policy row: a "Result-level reproductions" row for the 0.75 darkening.

**S5: caster ordering** (rc-engine, S/M). Depends on S4.
- *Files:* `crates/rc-engine/src/moby_render.rs`, `crates/rc-engine/src/gs_state.rs` (a late opaque variant if needed). Both are owned by other
  agents now, so dispatch this after they land.
- *Tests:* a screenshot where Ratchet stands next to a crate: his feet are not darkened and the crate's base band is. Metal still lands on casters.
- *Acceptance:* no caster pixel darkened; no change in non-caster draw order (compare screenshots with shadows off).

**S6: verification and names** (docs and tools, S). Optional; needs the user's PCSX2 steps.
- *Content:*
  - A PCSX2 screenshot vs port screenshot at matched cameras (Novalis spawn, a jump, a critter).
  - The suggested names above added to `tools/ghidra/names/` with plate comments (Lombyte names untouched).
  - Update this doc's "Status" block.

## 7. Risks and open points

1. **Size beyond 24 units. Resolved.** The shift is logical (`srl` at 0x26b838), but it cannot see a negative value: the shadow sphere's cull
   just before it (0x26b7a0..0x26b7e0) drops the caster when `(+0x7f << 10) − d < 0` (the moby draw-distance test with +0x7f as the
   distance), and the size uses `d − r'`, so the difference is ≥ r'/2 > 0. Beyond 24 units the shadow is **culled**; at the cull it is about
   r'/8 units across, then vanishes. **[H]**
2. **Radius path. Resolved.** The joint and row transforms write xyz only (`vmaddw.xyz`), the scale `vmulx.xyzw` all four lanes: radius =
   `w·moby+0x2c·(1/1024)·size/4096`. RAM (Novalis idle savestate, Ratchet's 21 records): the port's radii equal the game's to 1.5e-8. **[H]**
3. **Joint matrices. Resolved.** `x·J0 + y·J1 + z·J2 + J3` with the full posed joint frames (`vmulax.xyz`… with rows 0..3 of each 0x40-byte
   record). The port's `moby_anim::evaluate_chains` (the joints' frames in model space) matches the game's list to **3.1e-5 units** on the 11
   records whose joints are not under the moby's runtime joint-modifier list (+0x64: the idle joint records and the head / torso
   manipulators, which neither the port's evaluator nor its drawn pose applies); the 10 records under it (head, ears, arms) differ by up to
   0.18 units, exactly as the port's drawn Ratchet differs from the game's. Test: `crates/rc-game/tests/world/shadow_volume_novalis.rs`. **[H]**
4. **Face orientation.** The game classifies faces by the sign of the screen area with GS y-down; Bevy uses `front_facing`. With exact ±1 counting
   the sign only has to be consistent, so either convention works, but a wrong global sign gives inverted shadows. Test with the debug view.
5. **Blended world fragments.** The port draws the RGB-only halves of world alpha tests and billboard pass 2 in Transparent3d, after the resolve.
   The game darkens them because it draws them before the shadows. Faint unshadowed fringes can appear on grass cards or fading shrubs inside a
   shadow. Acceptable, or fixed later by moving those draws (decision 3).
6. **Z-pass limits** (same as the game). The count goes wrong when the camera is inside a volume, which is unlikely with a slab ≤ 4 units thick.
   The near-plane handling differs from the GS guard-band drop, but only at the screen edge.
7. **Darkening tolerance.** The linear-factor multiply is within 2/255 of the GS byte on dark tones. Record it as a tolerance, or use the exact
   read-back (decision 2).
8. **Ownership and dependencies.** S2 and S5 touch files that other agents are editing now. Classes whose update is not ported yet (on Novalis 459 and 774, for example) get no slab,
   and so no shadow, until their updates are ported. That matches "not ported yet"; it is not a new bug.
9. **Cost.** One R16F full-screen clear and one resolve per frame, plus a few hundred triangles. Expected to be under 0.3 ms; measure in S4.

## 8. Decisions for the user

1. **Approach.** Recommended: native screen-space shadow volumes driven by the game's shadow data (§6). Not recommended: Bevy shadow maps (wrong
   look, §5) or blob decals (loses the silhouette and direction).
2. **Darkening accuracy.**
   - (a) A fixed multiply blend calibrated to the display byte ×0.75, within 2/255 as a tolerance. **Recommended.**
   - (b) Exact per-pixel display-byte math with a colour copy.
3. **Caster ordering.**
   - (a) Draw deferred mobys in the late phase after the resolve. **Recommended:** it matches the game's list, and no extra target is needed.
   - (b) A caster mask that the resolve skips.
   - Either way, accept risk 5 for now.
4. **Game-rule quirks.** Recommended: reproduce the 8,064-byte shadow budget, the fourth hero probe's repeated offset and the `cos·x, sin·y`
   offsets. They are game behaviour, cheap and deterministic. Recommended: drop the GS guard-band face drop (hardware).
5. **Enhancements.** Recommended: none (no smoothing, no extra segments, no soft edges). A settings entry "Shadows: on/off" is fine; on by default.
6. **Related work.** Recommended: schedule the hero light cross-fade (`HeroEnvLighting` 0x26be04) as its own small package after S2. It is the
   other thing players read as "Ratchet in shade".

## 9. Address index

| Address (level01) | Boot | Role |
|---|---|---|
| 0x263488 `InitMobyInstance` | 0x20c5f0 | class+0xf → mode 0x400, +0x7f = 0x18, +0x84/+0x88 = 0, +0xbd = 0 |
| 0x264988 *UpdateShadowDir* | 0x20cfd0 | direction 0 from the hero light |
| 0x22a260 *UpdateHeroShadow* | – | Ratchet gate, direction 1, 4-probe slab |
| 0x26eff8 / 0x26f020 / 0x26f0e0 | – | slab from z / vertical probe / probe along dir |
| 0x26a7a0 `MobyProc` | 0x211808 | deferral (mode & 0xc00 with param_4), shadow cull and size, list 0x1aca80, count 0x15fffc |
| 0x264e18 `DrawMobys`, 0x264d68 `DrawMobysCleanUp` | 0x20d460, 0x20d3b0 | frame placement |
| 0x264a18 *SpliceShadowChain* | 0x20d060 | chain splice at the 0x160000 slot |
| 0x29b948 *BuildShadowList* | 0x227740 | posed prims → 0x1acc00..0x1aeb80 |
| 0x29b750 *DrawShadows* | 0x227548 | per prim: outline, clip, project, 2 passes; resolve |
| 0x29b3d8 *ShadowAlphaClear*, 0x29b580 *ShadowResolve* | 0x2271d0, 0x227378 | dest alpha 0x7f; 0.75·Cd where alpha bit 7 |
| 0x29b1c0 *ShadowSetDir*, 0x29bf88 / 0x29c0d8 outlines, 0x29c728 slab clip, 0x29bc10 project and classify, 0x29b348 / 0x29c7a0 passes | 0x226fb8, 0x227d80 / 0x227ed0, 0x228520, 0x227a08, 0x227140 / 0x228598 | geometry and GIF packets |
| 0x1af000 (4 × vec4) | – | shadow directions (0 = everyone, 1 = Ratchet) |
| 0x1bc310 (4 × 3 qw), 0x1bc3d0 / 0x1bc440 (7 qw) | – | TEST_1 variants; alpha-only FRAME + frame-buffer TEX0 (NTSC / PAL) |
| 0x160420 / 0x160424 | – | face colours, alpha 0x7f / 0x82 |
| 0x17c780 (4 × vec4) | – | hero probe offsets |

## As built (2026-09-27)

Every decision took the recommended option (the user's "go for it"): native screen-space volumes from the game's data
and rules, a fixed multiply blend, casters drawn after the pass, the game's quirks kept (budget, repeated hero probe,
6.28 circles), the guard-band drop dropped, no enhancements, a Port Options switch (on by default).

| Package | Where | What |
|---|---|---|
| S1 loader | `crates/rc-formats/src/moby_shadow.rs`, `tests/formats/moby_shadow_disc.rs` | `ShadowBlock::of_class` / `parse_level`; all 19 levels: 154 entries, 79 classes, 1,668 capsules, 45 spheres, ≤ 21 records, counts {−10, 0, 4..16}, joints < joint count; the test was seen failing on a broken record size |
| S2 game side | `crates/rc-game/src/shadows.rs`; one call line each in `moby_update/classes/{critter,amoeboid,talking_npc,path_enemy}.rs`; moby +0x84 / +0x88 in `moby_runtime.rs` | `update_shadow_dir`, `light_dir` (`fun_0020d510`), `update_hero_shadow`, `set_ground`, `probe_down`, `probe_along_dir` (ready, not wired: see below) |
| S3 volumes | `crates/rc-game/src/shadows/volume.rs`, `tests/world/shadow_volume_novalis.rs` + `tests/fixtures/shadow_ratchet_novalis_idle.tsv` (numbers only) | posing, `ShadowSetDir`, sphere / capsule outlines, slab prisms, size by distance, budget |
| S4 GPU pass | `crates/rc-engine/src/shadow_render.rs`, `assets/shaders/shadow_{volume,resolve}.wgsl`, `main.rs` (plugin line) | per tick (after `GameTick`): Ratchet's shadow and direction 0; per frame (`Last`): MobyProc's shadow deferral, cull, size, budget and the volumes; render world: `Core3d` system between `main_opaque_pass_3d` and `main_transparent_pass_3d` |
| S5 casters | `crates/rc-engine/src/moby_render.rs` (`caster_pass`, `CASTER_BAND`), `gs_state.rs` (`GsPass::LateOpaque`) | a class with a shadow block has its Z-writing draws at the start of Transparent3d (after the shadow pass), its metal next; everything else unchanged |
| Setting | `crates/rc-engine/src/graphics.rs` (`Lighting`), `crates/rc-game/src/menus/pause/port.rs` (row "Lighting": Original / Simple / Enhanced, 2026-10-23; it replaced the row "Shadows" On / Off / Dynamic), `menu_render.rs`, `render_settings.rs` (`load_key` / `save_key`) | these shadows under Lighting Original (the default; `Lighting::game_shadows`); `lighting = original/simple/enhanced` in the port settings file (an older `shadows = on/off` reads as Original, `dynamic` as Enhanced); `RC_GFX_LIGHTING` and the older `RC_SHADOWS=1/dynamic` override at start; `RC_SHADOWS=0` (developer switch) draws no shadows at all. Simple and Enhanced (port-only) replace these shadows with a sun shadow map every object casts, Enhanced adds the sun's rays and its volumetric light through the same shadow map: `sun_shadow.rs`, `sun_rays.rs`, `volumetric_light.rs`, docs/plan/graphics_options.md. The sun is the level's key light on its bearing, raised to at least 60° under Simple and to at least 30° under Enhanced (`sun_shadow::min_elevation`, 2026-10-24; `RC_SUN_FLOOR`), so Enhanced's shadows and shafts keep the key light's own slant (Novalis 32.8°, Kerwan 35°) |

**The pass in native terms.** The prisms (Bevy axes) are drawn into an `R16Float` target (cleared to 0, the view's
size and sample count) with the scene depth attached read only, `GreaterEqual` (reverse Z), no culling, additive
blending: +1 for a front face, −1 for a back one. A visible surface inside a volume ends with count 1 (z-pass). A
full-screen triangle then multiplies the colour target by 0.53 (blend Zero / Src) where the count is > 0.5; with MSAA
both targets are multisampled and the resolve runs per sample. The depth format has no stencil, hence the count
target. `RC_SHADOW_DEBUG=1` shows the count (red inside, blue negative: none seen).

**Checked.** Unit tests (directions, the hero gate / pitch / probes, the slab rules, posing, outlines, closed and
outward prisms, size, budget, the 0.53 factor against all 256 GS bytes); RAM (Novalis idle savestate, distilled):
the posed list to 3.1e-5 units off the modifier list, radii to 1.5e-8, directions 0 / 1 to 2e-6, the slab exact.
Engine (Novalis, `RC_SCENE=0`): Ratchet's shadow falls to his left-behind, away from the key light, hard-edged, his
feet not darkened; in a jump it slides under him (pitch 0.97 → 1.5 in 11 ticks) and shows the spread limbs; critters,
amoeboids and troopers cast almost straight down; the size shrinks between 16 and 24 units (0x9ae at 20, 0x3b7 at
23); at a ledge the shadow is cut where the ground leaves the slab; once airborne past the probes' reach he has no
seed and no shadow; RC_SHADOWS=0 and `shadows = off` give identical images, and on / off differ only inside the
shadow; two runs give identical PNGs; MSAA 4× works. Frame rate with / without shadows within noise (≈127 fps both,
`RC_NOVSYNC=1`, dev build); the frame's CPU collection ≈ 70 µs.

**Scenes.** The game keeps shadows on in scenes: its scene loop calls *ShadowProbeAlongDir* on every actor with
+0x7f ≠ 0 (direction 0). In the port the hero moby is hidden during a scene (and in the vendor), so his gameplay
shadow is off, as in the game; the scene actors themselves (extra mobys of `scene_render.rs`) cast **no shadow yet**:
wiring `probe_along_dir` needs their records (position, sphere, pose) from `scene_render.rs`.

**Scene actors (2026-09-27, flicker fix).** The actors now cast (their table mobys, `scene_render.rs`). A one-frame
pop every 96 frames (each streamed chunk switch; on Novalis' arrival frames 301, 397, 493, …, the ear / head shadow
jumping while Ratchet lies still) came from the order: the pose was written into the actor mobys at the *start* of the
next frame, so the shadow used the last frame's pose (+0x50..0x54) while the drawn actor used this frame's, and at a
chunk switch the old chunk's frame index was evaluated on the new chunk's sequence. `CutsceneModeUpdate` writes the
pose, `MobyBuildMatrix`es and calls *ShadowProbeAlongDir* **after** `MobyUpdateLoop` and before the draw; the port now
does the same (`scene_render::actor_mobys`, `FixedUpdate` after `GameTick`; the actor probe in `shadow_tick` runs on
every scene frame after it), so shadow and model share one pose every frame and the moby loop still reads the last
frame's. Checked: every frame of Novalis scene 5 (240..560) and level 2 scene 1 (20..420) against a shadows-off run, no single-frame pops left; two runs identical; gameplay shots unchanged.

**Not done / known differences.**
* ~~Casters are chosen by class for the late draw~~ (2026-10-05): the late draw follows `MobyProc`'s deferral per moby
  (the casters `shadow_render::collect` kept, or mode 0x800; `moby_render::CasterTwin`): the driven statics, the dynamic
  slots and Ratchet. [L] read one frame late; the metal passes, Clank / the wrench / the packs and the scene actors
  keep the class rule.
* ~~Risk 5~~ (2026-10-05): the RGB-only halves of the world's alpha tests and billboard pass 2 are drawn right after the
  opaque pass, before the shadow pass, as the game draws them with the world (crate::pre_shadow), so shadows darken them.
* ~~The joint-modifier list (+0x64) is not in the port's pose~~: ported 2026-09-28 (hero_gameplay.md §7); all 21
  records of Ratchet's posed list match RAM to 3.05e-5, and the shadows pose every caster with its `Moby::joint_mods`
  (Ratchet also with his weapon arm layers).
* `HeroEnvLighting` 0x26be04 (Ratchet's light cross-fade in covered zones; it also turns direction 0 / 1 with his
  light word) is not ported: decision 6's separate package.
* Pixel comparison against a PCSX2 screenshot (S6) not done.
