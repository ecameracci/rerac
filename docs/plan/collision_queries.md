# Collision queries (RAC1 NTSC, level01 addresses)

How the game queries the level collision mesh (format: `docs/formats/collision_rac1.md`). All addresses are level01.elf
unless marked "boot" (SCUS_971.99). Source files per bordplate/rac1-decomp: `cmecpu.cpp` / `collproc.cpp`. All the query
kernels are hand-written EE+VU0-macro asm (lqc2/vopmula/pextlh/psraw…). This doc works from the Ghidra disassembly, not the decompiler.

## 0. Globals and init
| Addr | What |
|---|---|
| `FUN_00255740` (init) | `blk=core+collision`: `*(0x1742c0) = blk + blk[0]` (mesh ptr), only if `blk[0]!=0`. If `blk[1]!=0`: `gp-0x75e8 = hero = blk+blk[1]`, and for each of `hero[0]` groups: `group.data (+0xc) += hero` (relative to the hero-section start) |
| `FUN_00255958` (level init) | `CollOutput+4 = 0x174380` (moby-mesh transform cache, 8×0x800), `+8 = 0x174340` (8×{key,stamp}), `+0xc=+0x10=+0x14=0` |
| `0x19bc60` | Moby grid (2-D, x/y only): 64×64 × 4-byte cells {u16 start, s8 count} over 16-unit squares; list of u16 moby indices at `0x19bc60+0x4000+start*32`; moby = `*(gp-0x6c28) + idx*0x100` |

**CollOutput** (level01 `0x1742c0`; boot `0x194100`, Lombyte name `CollOutput`), written by every query on a hit:

| Off | Type | Meaning |
|---|---|---|
| 0x00 | ptr | world mesh (the root, not the block) |
| 0x04/0x08/0x0c | ptr/ptr/s32 | moby-mesh transform cache: buffer, key table, round-robin index `&7` |
| 0x10 | s32 | query stamp: each query does `if(<0) return 0; ++`. Stamp goes into bits 32-63 of the flags reg and into `moby+0x9c` so each moby is tested once per query |
| 0x14 | s32 | hit-log ring index (`&0x3f`), log at `0x178580`, 64×0x40 |
| 0x18 | Moby* | hit moby, **0 for world-mesh and hero-group hits** |
| 0x1c | s32 | `0x1000 | face.type` for a triangle hit; for a moby primitive it's `-(primitive record ptr)` (negative) |
| 0x20 | vec4 | hit point (line) / closest point (sphere, capsule), world units |
| 0x30 | vec4 | sphere/capsule only: resolved centre, pushed out to touch |
| 0x40 | vec4 | face normal, **unnormalised** raw cross product in (1/1024)² units. Primitive hit: point − centre |
| 0x50/0x60/0x70 | vec4 | triangle v0, v1, v2 (world units) |
Return value (v0) = 1 on hit, else 0. The line query does not store the hit `t`.

## 1. Query functions
| Addr | Proto (inferred from regs) | Kind |
|---|---|---|
| `0x211870 CollLine_Fix` (boot 0x1efa68) | `(vec4* a, vec4* b, u32 flags, Moby* ignore, void* hitlog)` | segment a→b, nearest hit: world mesh, then mobys (grid) |
| `0x212960` | `(float r /*f12*/, vec4* c, u32 flags, Moby* ignore)` | sphere: closest face within r, world+mobys |
| `0x2135a0` | `(float r, float h, vec4* c, flags, ignore)` | vertical capsule c.z…c.z+h, radius r (hero body) |
| `0x214468` | `(float r, vec4* c, flags, ?)` | sphere vs mobys only (grid `trunc(p)>>4`) |
| `0x214d70` | `(float r, vec4* c)` | sphere vs **hero groups** (gp-0x75e8). Writes the same CollOutput with moby=0, type=0x1000 |
| `0x2117d0` | regs: t8=mesh, v1=z, v0=y, at=x → at=leaf word or 0 | cell lookup (§2) |
| `0x2151d8` (boot `0x1f0b58`) | `s32 CollType()` | `t=+0x1c; (t>=0 && (t&0x1f)!=0x1f) ? t&0x1f : -1` |
| `0x215208` | `CollSoundClass()` | `(t>=0 && (t&0x60)!=0x60) ? (t&0x60)>>5 : 0` |

Hero movement `0x233940`: up to 8 passes of `capsule(0x2135a0, flags 0x24 or 0xd24)` + `0x214d70`, each pass copying +0x30→pos, +0x40→normal,
+0x20→contact, +0x18→standing moby. Water-state uses sphere `0x212960`. Ground probe `0x232dc0`: `CollLine_Fix(..., flags 2)`.

**Flags** (`s3`, same in line/sphere/capsule): `0x1` skip world mesh (mobys only; also bit 0|0x8 → only mobys with `moby+0x34 & 0x4000`);
`0x2` skip moby primitives; `0x4` selects which moby sub-mask (u16 `blob+0/+2`, prim `+2` mask bit 1 vs 2); `0x10` two-sided faces;
`0x20` **exclude** faces with `(type&0x1f) == (flags>>8)&0x1f`; `0x80` exclude faces with type bit 7.
**Confidence: high** for offsets/flags (read directly from the asm). Medium for the capsule/moby-only prototypes (inferred from register use).

## 2. Cell lookup, traversal, vertex decode
- **Bounds:** min(a,b) ≥ 0 and max(a,b) − 1024 < 0 per axis, else return 0 (the check reads VU0 MAC sign flags `&0xe0` via `cfc2`; inferred MAC). The world is [0,1024)³.
- **Cell (line):** `c = (u32)vftoi0(p*1024.0) >> 12` (vftoi0 truncates; p ≥ 0 so this is floor(p/4)). **Sphere/capsule:** `vftoi0(p) >> 2` on the AABB c±r (capsule z: c−r…c+h+r).
- **Lookup `0x2117d0`** (all fields read `lhu`, so the "s16 base" is really used **unsigned**): `i=z-root.base` (32-bit signed sub); miss if `i<0 || count-i<=0`;
  `slab = mesh + root.zoff[i]*4` (zoff==0 → miss); `i=y-slab.base`… `row = mesh + slab.rowoff[i]` (0 → miss); `i=x-row.base`… returns `row.leafword[i]` (0 = empty).
- **Leaf size byte = DMA qword count.** The leaf goes to scratchpad over DMA ch.9 (toSPR, 0x1000D400): `QWC = word&0xff, MADR = mesh + (word>>8), SADR = 0/0x1000`,
  double-buffered (0x70000000 / 0x70001000). Checked across all 19 levels: `byte == ceil((4+4V+4F+Q)/16)` (0 mismatches). If the word is 0, the buffer's first dword is zeroed, so V=0 → the cell is skipped.
- **Line DDA** (`0x2119b0-0x211b14`): per axis, crossings of planes `k*4` for the k between the start and end cells. `t = (k*4096 - a*1024)/((b-a)*1024)`, and the float
  bits are clamped to [0,0x3f800000] with pminw/pmaxw. List end marker 0x50000000. The 3 sorted lists are merged into {t, packed cell x|y<<8|z<<16} at 0x70003800 (ties: x, then y, then z)
  and end with t=1.0. Cells are visited in order. **Early out:** once a hit exists, stop when `best_t <= t_enter(cell)`. Faces duplicated across cells need no dedup,
  because a hit is taken only if `best_t - t > 0` (strict), and the initial best is 1.0.
- **Sphere/capsule cells:** every cell of the AABB (loop z outer, y, x inner) is gathered if `|clamp(c, cellmin, cellmin+4) - c|² - 0.99951·r² < 0`
  (1/1024 units, const 0x3f7fdf3b). The capsule uses the same test at its base centre only, which looks like it ignores h (observed, low confidence).
  **Correction (§6):** the gather test uses the *unshrunk* `r²` (`vf14.y = -r²` is taken before `vf24.x *= 0.99951`); 0.99951 only scales the initial best.
  The capsule question is settled: gathering tests only the base sphere (high confidence), see §6.
- **Vertex decode** (the block at `0x211c98` inside `CollLine_Fix` 0x211870; identical block at `0x212c48` inside the sphere query 0x212960; neither is a function start): `hx=(w&0x3ff)<<6; hy=(w&0xffc00)<<12` (in the upper half); `hz=(w>>20)<<36`, i.e. halfwords
  {x<<6, y<<6, z<<4}. Then `pextlh`+`psraw 16` sign-extends them and `vitof0` converts. The result is in 1/1024 units relative to the cell centre `cell<<12 | 2048` (= cell*4+2).
  ⇒ **x = s10/16, y = s10/16, z = s12/64 — this confirms the format doc.** Each vertex also stores an outcode per axis (bit0: v < segmin−1, bit1: v > segmax+1, 16-bit `psubh` in 1/1024 units).
  A tri is rejected if the AND of its 3 outcodes ≠ 0. Note: segmin/max are truncated to 16 bits (`ppach`), so a sphere with r ≳ 30 would wrap (edge case).
- **Face decode:** `pextlb` with 0x020202 then `pextlh` with 0x0700, `<<4` ⇒ vertex address `0x70002000 + idx*16`. Type = byte 3.
- **Quad split** (the block at `0x211f48` inside `CollLine_Fix` 0x211870; same block at `0x212f68` inside 0x212960): after the triangle pass, the first `quad_count` face records are rewritten **in the scratch copy**: `f.v1=f.v2; f.v2=quad_v3[i]`, then
  run again. ⇒ quad (v0,v1,v2,v3) = tris **(v0,v1,v2) + (v0,v2,v3)**, diagonal v0–v2, same type byte. Moby meshes skip this (triangles only).
- **Line vs tri:** `N = (v2−v0)×(v1−v0)`; `t = ((v0−a)·N)/((b−a)·N)`. Default one-sided: needs `(v0−a)·N < 0` and `(v0−b)·N > 0` (a on the +N side).
  With 0x10, only opposite signs are needed. The edge tests `((P−v0)×(v1−v0))·N`, `((v2−v0)×(P−v0))·N` and `((P−v1)×(v2−v1))·N` must all be ≥ 0 (inclusive). Dots are summed `(x+y)+z` on VU0 (PS2 float: no denormals, truncating rounding).
- **Sphere vs tri:** centre must be on the front side (`(c−v0)·N ≥ 0`) unless 0x10. The centre is projected onto the plane. If the first failing edge test is edge e, the closest point is
  the clamped point on e (only one edge is tried). Accept if `d² < best`, then `best = 0.99951·d²`. Push-out: `+0x30 = hit + (c−hit)·r/√best`. Capsule: same, with z clamped to [0,h].
  **Correction (§6):** the accept test is `best − d² ≥ +0` (sign bit clear), i.e. `d² ≤ best`, non-strict.
**Confidence: high** (instruction-level). The MAC-flag reading of `cfc2 vc1` is inferred from the logic.

## 3. Surface type (face byte 3)
- **bits 0-4** = surface id, via `0x2151d8`: 0x1f = "none/default" → −1. Hit is negative (moby primitive) → −1.
- **bits 5-6** = footstep sound class, via `0x215208` (3 → 0). The ground probe `0x232dc0` stores it at `0x14063d` (default 3). `0x227e48 → 0x2a1898`: sound = `tbl_0x1bdca0[level] + class*4 + foot*2 + variant + base`.
- **bit 7** = ignored by queries passing flag 0x80.
- id **0 = water surface** (inferred): the ground probe records the level (`0x13f640` via `0x26ed38`) and re-casts with flags `0x24`, which excludes id 0, to find the floor. The hero capsule also always uses 0x24, so the hero passes through it.
  `CollType` reads +0x1c only, never +0x18, so a **moby** triangle of type 0 (`0x1000 | 0`: the patch managers' pools, `water::managers`) is water to its callers as a world face is. **In the port (2026-10-20, issue #36):** `CollOutput::surface_id` is that getter; the bomb's hit test (L01 0x2c3990, L07 0x2d72d0) had added "no hit moby" to it and now does not (`classes::bomb::fly`, hero_gameplay.md §13 "the bomb in moby water").
- id **0xd**: level stored in `0x13f640` (same slot as water), then re-cast from 0.01 below. Hero capsule uses `0xd24` (excludes 0xd) when `0x1413d4==0x7f`. Liquid-like; only in level 12.
- id **0xb**: z stored in `0x13f644`, and the ground snap is skipped.
- Wall/ledge checks reject ids 9/0xc or 10/0xc, then test slope `atan < 0.349/1.309 rad` (`0x22c9a0`, `0x22d090`, `0x22d838`).
- Moby/projectile "hit ground-like surface" switches use the set {0,1,3,8,0xb,0xc,0xd} (`0x2d48e8`, `0x2d5b30`, `0x2d9f90`, `0x2ddce0`). `0x2e2170` uses {0,1,3,4,7,0xb,0xd}; `0x2efc60` uses {4,6,7,8,0xd}.
- **No per-type table was found.** Semantics are spread across `==` compares and switches. Values used on disc (all levels): 0-5, 7-0xe, 0x1f, plus bit-5/6/7 variants (0x3f, 0x5f, 0x7f, 0x9f, 0x4c…).
**Confidence:** high for the bit split (0x1f/0x60/0x80). Medium for water (0) and footsteps. Low for 0xb/0xd names.

## 4. Mobys, ties, hero groups
- **Ties:** no tie path exists in any query. Tie collision is presumably baked into the level mesh (inference).
- **Mobys:** `moby+0x94 = class+0x10` (`InitMobyInstance 0x263488`). Line/sphere visit the 16-unit moby grid cells covered by the traversal (line: `cell>>2` of each visited cell, consecutive dupes removed).
  A moby is skipped if it is `ignore`, its stamp matches (`+0x9c`), or it fails the bsphere test (`moby+0x00`, compared in the ×1024 space). The blob is transformed by `moby+0x10` pos (×1024), rows `+0xc0/d0/e0`, and scale `+0x2c`.
  Blob: `+0 u16,+2 u16` (flag-4 selected; nonzero → use 8-entry transform cache, `0x267fc0`), `+4` part1 bytes, `+8` face bytes, `+0xc` vertex bytes.
  Layout: `+0x10` part1 = 0x20-byte **primitives** (s8 kind 1-4, s16 mask at +2; kind 1 = sphere at +0x10; others reference 2 vertices at +4/+6, i.e. capsule/cylinder-like. `moby+0x98` = per-prim disable bits),
  then vertices (4×s16, 8 B), then faces (same 4-byte records, no quads). Primitive hits return a negative +0x1c.
- **Hero groups** (`0x214d70`): bsphere `u16 x,y,z,r` compared at ×64 (0x42800000). Test is inclusive: `d² ≤ (r·64+R)²`. Vertices are **u16, zero-extended** (pextlh with zero, no psraw), absolute /64.
  Tri count read as **byte** (`lbu +8`), vertex count `lhu +0xa`. Output scale 1/64 (0x3c800000), type 0.
**Confidence:** medium (read, but primitive kinds 2-4 not decoded).

## 5. Leaf size byte
Used: it is the DMA QWC for the scratchpad upload (§2), which bounds a leaf to < 0x1000 bytes (one 4 KB half of the double buffer). Vertex scratch space is 0x70002000-0x70003000
(≤256 verts × 16 B). DDA lists are at 0x70003000/3200/3400, the cell list at 0x70003800. **Confidence: high.**

## 6. Port (`crates/rc-game`, `rc_game::collision_query`)
Re-read instruction by instruction from the level01 disassembly (2026-09-27). `coll_line` = `0x211870`, `coll_sphere` = `0x212960`, `coll_capsule` = `0x2135a0`;
`coll_sphere_mobys` (`0x214468`) and `coll_sphere_hero_groups` (`0x214d70`) are ported too (§7; they were stubs before 2026-09-26). The **boot ELF has only the line kernel** (`0x1efa68`,
instruction-identical to level01's apart from the global addresses: CollOutput `0x194100`, moby table `gp-0x6ce8`, moby grid `0x1b76e0` (level01 0x19bc60 = boot − 0x1ba80, not the usual boot − 0x1fe40 of the other globals here; both addresses read from the two line kernels), hit log `0x1983c0`),
the lookup `0x1ef9c8` and `CollType` `0x1f0b58`; sphere and capsule exist only in the overlays.

**Arithmetic model.** All three kernels run in x1024 space *relative to the centre of the cell being tested* (`centre = cell<<12 | 2048`, converted with `vitof0`).
Vertices are the halfwords `(x<<6, y<<6, z<<4)` (exact integers), so edges `v−v0` are exact; everything involving the query point rounds. The port runs every VU0
op in the game's order on PS2 float bits (`rc_game::ps2v` over `rc_formats::tfrag_light::ps2`: round toward zero, no denormals, adder guard-bit rule; **not**
modelled: the multiplier's rare last-bit deviations, and the VU `vdiv`/`vsqrt` are assumed to be the truncated exact result like the model's). So results are
bit-exact to the extent of that model; nothing uses IEEE `f32` arithmetic. Integer parts (DDA list merge, 16-bit outcodes, cell mapping) are exact, wrap-around
included. Sign tests read the float sign bit (`bltz`/`bgez` after `qmfc2`), so **`-0.0` counts as negative** and `+0.0` as non-negative everywhere below.

**Constants.** `0x44800000` = 1024 (world→x1024), `0x3a800000` = 2⁻¹⁰ (output scale, exact), `0x3f800000` = 1.0 (initial line best, DDA clamp, t terminator),
`0x50000000` (DDA list end marker), `0x3f7fdf3b` = 0.99951 (sphere/capsule best shrink), `0x45000000`/`0x45800000` = 2048/4096 (half/full cell),
`0x10101` → `(1,1,1,0)` (line outcode box margin), `0x8000` halfword masks (outcode sign bits), `0x0700_02xx` (face→scratch vertex address, `<<4`). There is **no
epsilon** anywhere in the triangle tests: all comparisons are against ±0 via the sign bit.

**Line (`CollLine_Fix`), in order.**
1. Stamp check; `a,b ×1024`; world box: sign flags of `min(a,b) − 0` must be clear and those of `max(a,b) − 1024` all set (else return 0). *Game bug:* the
   min-side exit happens before `s6` is cleared, so the function then returns the caller's `s6` (returns 1 with garbage output if non-zero). The port returns `None`.
2. Cells `s = vftoi0(a×1024)>>12`, `e = …b…`. If `s == e` and the x1024 integer points are equal: return 0 (zero length; the moby pass is skipped too).
3. DDA per axis (x, y, z): `q = 1/d` (`vdiv Q, vf0w`), planes `s+1..e` (or `s, s−1, …, e+1`), `t = (itof(k<<12) − a) · q`, bits clamped to `[0, 0x3f800000]` as
   signed ints (negatives → +0). Note the reciprocal truncates: e.g. `(4096−1024)·(1/3072) = 0.99999994`, so a segment ending exactly on a plane may or may not
   visit the next cell. Merge on the float bits, ties x, then y, then z; entry 0 = `(0, s)`, then `(1.0, 0)`.
4. Walk: stop at an entry with `t_enter == 1.0` exactly; once a hit exists stop when `best − t_enter ≤ 0` (integer compare of the bits).
5. Per cell: sub-segment box `[min, max] ± 1` of `vftoi0(a + d·t_enter)` and `vftoi0(a + d·t_next)` (absolute x1024 integers minus the centre, truncated to
   16 bits); per vertex and axis, bit 0 = `(v − min) <s16> < 0`, bit 1 = `(max − v) <s16> < 0` (`psubh`, wrapping). A triangle is skipped when the AND of its
   three outcode words is non-zero, i.e. its box misses the part of the segment inside this cell — so a triangle stored in a cell is only found through the
   cells whose sub-segment box overlaps it (a hit can still lie outside the cell's t-range, which is why the strict "closer" rule matters).
6. Faces `0..(face_count & 0xff)` (read with `lbu`; no retail leaf on Novalis has > 255 faces) as `(v0,v1,v2)`, then the first `quad_count` as `(v0,v2,v3)`.
   Filters: outcode AND, `0x80 & type`, `0x20` + id. Then `e1=v1−v0`, `e2=v2−v0`, `N = e2×e1` (`vopmula/vopmsub`: each lane `a.y·b.z − b.y·a.z`, products
   rounded first), `dN = d·N`, `s0 = (v0−a)·N`, `s1 = (v0−b)·N` (dots `(x+y)+1·z`), `Q = s0/dN`. One-sided: `s0` sign set and `s1 > +0` (as integer);
   always: `s0 ^ s1` sign set. `P = (a−v0) + d·Q`; edge dots `((P−v0)×e1)·N`, `(e2×(P−v0))·N`, `((P−v0−e1)×(e2−e1))·N` all sign-clear; `best − Q > +0`.
   Accept: `best = Q`, point `= (P + (v0+centre))·2⁻¹⁰`, normal `N` raw, tri `(v0+centre, e1+v0+centre, e2+v0+centre)·2⁻¹⁰`.
7. The moby pass (§7), then output; `+0x18` = 0 for a world hit because the moby pass starts with `s5 = 0`.

**Sphere, in order.** Stamp; `lo = c−r`, `hi = c+r` (world units); fail if a `lo − 0` sign flag is set, `r ≤ 0` (as integer) or any `hi − 1024` is non-negative.
Cell range `vftoi0(lo)>>2 .. vftoi0(hi)>>2`; loop z, y, x; keep a cell if `((−r² + dx²) + dy²) + dz² < 0` with `d = clamp(c, cellmin, cellmin+4096) − c`
(x1024, **unshrunk r²**) and it exists in the tree (≤ 127 non-empty cells fit the scratch list at 0x3800; not enforced by the port). No early out: all
gathered cells are tested. Outcode box: `vftoi0(lo×1024 − centre)`, `vftoi0(hi×1024 − centre)` truncated to 16 bits, **no ±1 margin** (and it wraps for
`r ≳ 30`). Per triangle: `w = (c−centre) − v0`, `N`, `s = w·N`, `nn = N·N`, `Q = s/nn`; one-sided: `s` sign clear. `P = w + (−N)·Q`, `d² = |N·Q|²`
(`(x+y)+1·z`); reject if `best − d²` is negative; edge dots (lane-y order `(y+x)+1·z`, same value); first failing edge in the order v0v1, v2v0, v1v2 →
`P = A + (B−A)·clamp((P−A)·(B−A)/|B−A|², 0, 1)` and `d² = |P − w|²`, reject if `best − d²` negative. Accept: **`d² ≤ best`** (non-strict; so with `d² = 0`
the last zero-distance face wins), then `best = 0.99951·d²`. Output: `Q = vrsqrt(r×1024, best) = r/√best` (with the *shrunk* best, so the push-out is
`1/√0.99951 = 1.000245×` the radius), `+0x30 = ((c − hit)·Q)·2⁻¹⁰ + hit·2⁻¹⁰`.

**Capsule (hero body), in order.** Sphere setup with `hi.z = (c.z + r) + h` and `h ≤ 0` also failing. **Height question settled (high confidence):** the
enumerated AABB spans `c.z − r … c.z + r + h`, but every cell is kept only if it is within `r` of the *base centre* `c` (same test as the sphere, `vf31` = base),
so faces stored only in cells that the upper part of the capsule reaches are never seen; the outcode box does include `h`. Per triangle: `bot = (c−centre)−v0`,
`top = bot + (0,0,h)`, `sb = bot·N`, `st = top·N`; one-sided: reject only if `sb` and `st` are both negative. Axis point `A`:
- `|N.z| < 1.0` (integer compare, i.e. a vertical face): `A.z` = height of v0, clamped;
- else if `bot.z`, `bot.z − e1.z`, `bot.z − e2.z` all sign-clear (base at/above all three vertices): `A = bot`, offset `N·(sb/nn)`;
- else if `top.z`, `top.z − e1.z`, `top.z − e2.z` all negative (top below all three): `A = top`, offset `N·(st/nn)`;
- else `X = (bot.x, bot.y, bot.z − h·(sb/(h·N.z)))` (axis ∩ plane), moved to the closest point of the first failing edge (lane-y dots); then
  `A = (bot.x, bot.y, bot.z + clamp(X.z − bot.z, 0, h))`, offset `N·((A·N)/nn)` (also for the vertical case).
Then as the sphere with `P = A − offset`, `d² = |offset|²`, edge fallback distance measured from the (unchanged) axis point `A`. Output push-out:
`k = clamp(hit.z − c.z, 0, h)` (x1024), `v = c − hit`, `v.z += k`, `v ·= r/√best`, `v.z −= k`, `+0x30 = v·2⁻¹⁰ + hit·2⁻¹⁰` (the pushed *base*).

**Scratchpad limits the port does not reproduce** (only reachable with extreme queries): the per-axis DDA lists hold 128 crossings (x overflows into y's,
y into z's), the line cell list 255 entries, the sphere/capsule cell list 127 cells.

**Verification.** Unit tests on hand-built cells (`crates/rc-game/src/collision_query/tests.rs`: hit/miss, one/two-sided, exclusion flags, quad split,
inclusive edges, strictly-closer across two cells, early out, DDA tie order, sphere/capsule push-out, capsule height culling). Disc test
(`crates/rc-game/tests/world/novalis_collision.rs`, skipped without `extracted/`): 1000 seeded vertical rays over the Novalis tfrag box hit 60.7 % (the box
includes void); rays from 1 unit above each of the 983 moby instances hit 949 (96.5 %), median drop below the instance 0.000; the class-0 instance
(Ratchet's spawn, index 0, (162.53, 136.39, 60.5)) is 0.500 above the landing pad (type 0x1f). Not yet compared against a PCSX2 trace.

## 7. Moby collision in the port

Read from the level01 disassembly (2026-09-26): `CollLine_Fix` 0x211fd0..0x21295c, sphere 0x212fb8..0x21350c, capsule
0x213df8..0x2143c4, `coll_sphere_mobys` 0x214468, hero groups 0x214d70, `UpdateMobyGrids` 0x265900 (callers `MobyBuildMatrix`
0x265bd8, `DeleteMoby` 0x2636c0), the pose fill 0x267fc0. Port: `rc_formats::moby_collision` (blob), `rc_game::collision_query`
(`coll_line_m` / `coll_sphere_m` / `coll_capsule_m`, `coll_sphere_mobys`, `coll_sphere_hero_groups`) and its `mobys`
submodule (grid, pose cache, moby pass). Same PS2 float model and operation order as §6. **Confidence: high** (instruction-level)
except where marked.

**Blob** (class +0x10 → `moby+0x94`): `u16 joints[2]` (+0: posed for flag-0x4 queries, +2: for the others), `s32` primitive /
face / vertex section bytes, then 0x20-byte primitives, 8-byte vertices (s16 x, y, z, pad; model units), 4-byte faces
(`v0 v1 v2 type`, no quads). Primitive: `s8 kind` +0, byte +1 (4 on every retail record, unread), `s16 mask` +2 (bit 0: flag-0x4
queries, bit 1: the others, **bit 15 ends the list** — the kernels stop after the record with a negative mask, not at the
section size); `moby+0x98` bit i disables primitive i. Kinds: **1** sphere (centre / radius at +0x10); **2** sphere on a joint
(joint `lw +4`, radius +0xc, offset +0x10 added to the joint); **3** vertical cylinder (base centre / radius +0x10, height f32 +4,
along world z, not rotated); **4** capsule between joints `lh +4` / `lh +6`, radius +0xc (any other kind value takes this path;
none on the disc). Disc (19 levels): 1064 blobs, kinds 1-4 = 464 / 31 / 350 / 302, 23369 vertices, 38359 faces, 104 posed
classes; every joint primitive's selected joint count is non-zero and above its joints. Class 825 (level 3) has 257 vertices,
one past the 256-slot scratch area (the kernels overwrite the start of the DDA lists; harmless). Test:
`crates/rc-formats/tests/formats/moby_collision_disc.rs` (counts pinned; no C++ oracle — the format has no other reader).

**Transforms.** Mesh vertices: `((r0·s)·x + (r1·s)·y) + (r2·s)·z` (rows `+0xc0/d0/e0` pre-scaled by `+0x2c`), relative to
`pos·1024`; the triangle tests are the world ones in that frame (`record` adds `pos·1024`). Primitive points: `(r0·p.x + r1·p.y) +
r2·p.z + pos·1024` with `p = q·s` (the scale on the vector, not the rows); radius `R·s`. Joint positions: the translation rows
of the posed joint matrices, model units, from 0x267fc0 (count = the blob's u16) into 0x70000000 and the 8-entry cache
(CollOutput +4 buffers 8×0x800, +8 keys, +0xc round robin; key `moby+0xa8`; a hit reuses the cached pose, so a mode-4 moby that
is never rebuilt keeps its cached pose). **Medium confidence:** the port takes the joint translations from
`rc_formats::moby_anim::joint_translations` (the `fun_002109b8` evaluator); 0x267fc0 is a third evaluator variant that was not
instruction-compared. Only kinds 2 / 4 read it (posed classes on Novalis: 186, 203, 459, 604, 1900).

**Grid** (0x19bc60, 64×64 × 16 units, x/y): cell `{u16 block, s8 count, u8 capacity}` at `+x·4 + y·0x100`, indices (`moby+0xac`)
at `0x19fc60 + block·32`. `MobyBuildMatrix` (every rebuild, state < 0x80, `+0x94 ≠ 0`) computes the rectangle bytes
`{x0,y0,x1,y1}` = `(vftoi0(bs.xy) ∓ vftoi0(bs.r)) >> 14` into `+0xa0` unless equal to the old one or `x0|y0` has bit 6/7 (off the
grid: the old registration stays). `UpdateMobyGrids(moby, new)`: old cells outside `new` drop the moby (the cell's last entry
takes its slot), new cells outside `old` append it; x inner, y outer; signed-byte `psubsb` containment. `DeleteMoby` passes
0x80807f7f (none); `InitMobyInstance` leaves the same. The loader registers every instance in array order through its
`MobyBuildMatrix`. Storage (capacity doubling/halving, allocator 0x265820 / 0x265880) keeps the order and is not modelled.
**No per-tick rebuild:** only matrix rebuilds, deletes and the load move entries.

**Candidates.** Line: `(x>>2, y>>2)` of the DDA list (entries up to the first `t = 1.0`, consecutive duplicates dropped — the
whole segment, even past the world walk's early out). Sphere / capsule: `vftoi0(lo·1024 .. hi·1024) >> 14`, y outer, x inner;
`coll_sphere_mobys`: `vftoi0(lo .. hi) >> 4`. Skip: `ignore`, `+0x94 = 0`, stamp `+0x9c` (written only after the bounding-sphere
test passes), and the bounding sphere `+0x00` (x1024; `MobyBuildMatrix`):
- line: `t = clamp((bs − a)·(d/|d|²), 0, best)`; reject `((|a + d·t − bs|²ₓ − R²) + y) + z > +0`. Flag `0x1 | 0x8` → only
  `mode & 0x4000` mobys (line only);
- sphere: `((dx² − (r+R)²) + dy²) + dz² > +0`;
- capsule: the same in x/y, plus `bs.z + (r+R) − c.z > 0` and `bs.z − (r+R) − c.z − h < 0` (a cylinder test; the upper test's
  64-bit `blez` also reads a stale lane when the value is exactly +0 — the port treats +0 as a reject).

**Per moby:** with a non-zero joint count (and not flag 0x2 with an empty mesh, which skips the moby first) the pose is fetched;
then the mesh (outcodes: float sign tests, no margin, against the line's whole min/max box, the sphere's `c ± r`, the capsule's
`c ± r` with top + h), then unless flag 0x2 (sphere-mobys ignores 0x2) the primitives in order. Each primitive becomes a sphere
`(C, R)` at its point nearest the query: kind 3 clamps `c.z` into `[base, base + h]`; kind 4 (sphere) the segment point nearest
`c`; kind 4 (capsule query) the 2-D nearest point, or the base z clamped to the segment when `|e.xy|² ≤ 1`.
- sphere / capsule: reject `(r+R)² − |C − c|² < 0`; point `P = C + (c − C)·(1/|c − C|)·min(|c − C|, R)`; accept `best − |P − c|² ≥ 0`,
  then **`best = d²` (not shrunk)**; `+0x40 = P − C`; `+0x50..0x70` keep the previous hit's triangle. Capsule: `c` is the axis point
  `(c.x, c.y, c.z + clamp(C.z − c.z, 0, h))`;
- line: kind 3 first intersects the infinite cylinder in x/y (`|d.xy|²` clamped to ≥ 2⁻⁶; closest-approach reject, entry root) and
  clamps the axis point's z; kind 4 intersects the infinite capsule line (parallel when every `|e×d|²` lane < 1: use `a`), backs
  off the entry, clamps to the segment; then all kinds: closest approach reject, entry root
  `t = (2w·d − √max((2w·d)² − (4|d|²)(|w|² − R²), 0))/(2|d|²)` clamped to [0, 1], accept `best − t > +0`, `+0x40 = hit − C`.
- `coll_sphere_mobys`: best fixed at `(r·1024)²` (unshrunk); the first passing triangle, else the first passing primitive, lists
  the moby. Output: the list (the kernel returns its length), `+0x18` = first, `+0x1c = 0x3f`, `+0x20 = (0,0,0,1)`; with a
  template every listed moby with `mode & 0x4000` gets a hit record (+0 = (0,0,0,1), +0x38 = 0) unless its current record's
  damage bits are larger (integer compare). `CollLine_Fix` with a template (the wrench sweep `FUN_0026ebe8`) writes the same
  record for its hit moby with +0 = the hit point and +0x38 = the primitive index (−1 for a triangle).
- Output: `+0x18` = moby of the last accepted hit, `+0x1c` = `0x1000 | type` (moby triangle) or `-(record address)` (port:
  `-(0x10 + 0x20·i)`, `CollOutput::primitive = Some(i)`).

**Hero groups** (0x214d70): no stamp, bounds or radius checks; groups culled by `((dy² − (r+R)²) + dx²) + dz²` at x64 (kept
when negative, or +0 with `dx² = +0`); vertices u16 zero-extended absolute x64, 16-bit outcodes against `vftoi0(c·64 ∓ r·64)`;
faces `0..lbu(+8)`; sphere triangle test always one-sided, no filters, initial best `(r·64)²` unshrunk, then `0.99951·d²`;
output ×1/64, moby 0, `kind 0x1000 | pad` (0x1000).

**Who calls what (ignore argument).** The hero passes Ratchet (`0x1413d0`) to every line / sphere / capsule query (0x233940,
0x232dc0, 0x236a68, 0x232978); the port's `hero::physics` reads `Env::mobys` / `Env::hero_moby`, which `tick::Game` fills
from `TickHooks::world` (`tick::MobySystem::scene`: `Services::hero_scene` snapshots the table's collision fields after the
moby loop, shares the grid / blobs / pose cache). After the write-back, HeroSyncMoby's `MobyBuildMatrix(Ratchet)` rebuilds his
bounding sphere and grid cells (`MobySystem::build_matrix`). The follow camera's queries read a second snapshot taken after
that (`CamInput::mobys`), with the game's `a2` per query (Ratchet, none, or the unported camera moby; `follow_camera.rs`). Bolts: init `CollLine_Fix(…, 0x22, 0, 0)`, fall `0x212960(r, pos, 0x22, bolt)`. The crate probe
`w.line(…, 2)`. A bolt landing on a moby with `mode & 0x20` and pvar +8 would attach (`FUN_00275290` / `FUN_00275528`, +0x5c):
not ported (no such moby on Novalis's crates).

**Stand-ins removed:** `CrateBoxes` / `MobyCollider` (inferred unit crate tops) and the bounding-sphere `sphere_mobys`
(`services.rs`). **Not modelled:** the query stamp wrap, list-storage exhaustion, cells beyond the 64×64 table, the camera moby
0x3ef (`Camera_handleCollWithHero`).

**Ratchet's cylinders** (2026-10-19, G-HERO-022). HeroSyncMoby (L01 0x229f20 on foot, 0x22a110 in a body), after its
`MobyBuildMatrix` and unless 0x13f51c is set or +0x94 is 0, calls `FUN_00263cb8(moby, i, point, r, h)` for records 0 and 1 of
his class blob (on foot 0x22a034 / 0x22a0b0, body 0x22a1f0 / 0x22a23c): with `k = 1024 / scale`, record +0x10..+0x18 = the
point relative to the moby through the transposed +0xc0 rows ×k, +0x1c = r·k, +0x04 = h·k (kind 3: base, radius, height). The
point is `0x248d80(base)` (the gravity step 0x248b68 by −base: up from the feet); record 0's base is the capsule bottom
0x13f574, record 1's radius 0x13f584 + 0.05 (+ 0.15 in group 4, = bottom in the water `0x22dea8`); height = top 0x13f570 − base,
at least 0.05 on foot (the body copy has neither floor nor the group rules). On Novalis record 0 has mask 0x0001 (flag-0x4
queries) and record 1 mask 0x8002. `FUN_00263cb8` per level: 00 0x24eed8, 01 0x263cb8, 02 0x251548, 03 0x23da48, 04 0x241c10,
05 0x279140, 06 0x25db98, 07 0x277900, 08 0x2593d8, 09 0x26e860, 10 0x241950, 11 0x274d08, 12 0x267c18, 13 0x25ab60, 14
0x2573d8, 15 0x23e540, 16 0x24a5b0, 17 0x249690, 18 0x2511c0. Port: `Hero::sync_cylinders` (from `Hero::write_back`) stores the
words on his moby (`Moby::prim_writes`: only his moby uses the class) and `mobys::active_prims` substitutes them for records
0 / 1. Native `f32`.

**Verification.** Unit tests (`collision_query/tests.rs`): grid registration order / swap-removal / off-grid / delete; a box
mesh hit by a line (vs a nearer world floor, ignore, flag 0x9, one-sidedness), blocking a capsule and a sphere; a sphere
primitive by line / sphere / capsule (entry point, unshrunk best, push-out), masks, `coll_sphere_mobys`; a joint capsule
(side, cap, sphere) and the pose cache round robin; hero groups; records 0 / 1 replaced by `prim_writes`; the cylinder words
per hero mode (`hero::tests::write_back_rewrites_the_collision_cylinders`). Disc (`novalis_line_hits_ratchets_cylinders`:
Ratchet's records 0 / 1 are cylinders, a line across him enters at his capsule radius, record 1 / with flag 0x4 record 0). Disc (`crates/rc-game/tests/world/moby_collision_novalis.rs`): after
120 ticks the 344 stack has settled; a line down onto crate 344 (ignoring 342 on it) hits its top face at z 66.390625 = 344.z + 1
= where 342 rests (kind 0x107f); world-only the same line reaches the ground; a bolt dropped 1.5 above crate 342 settles on it
(rest z 67.390625 = the crate top; world-only it falls through to 65.08). `moby_update_novalis`: 82 stacked crates, as with the
stand-in. Engine (frame-exact, `RC_SCENE=0`): the §12 crate-break script now hits at **tick 340** (was 341; not traced to the query, presumably the
wrench's line sweep meeting the crate's side faces, which the stand-in did not have), the crate breaks at 341 (22 dynamic mobys by 344), bolts reach
18 by tick 439 (was 10 at 430; presumably dropped bolts now rest on the lower crate, within pickup range). Appending
`360-383:stick 1 -0.04,381-388:press CROSS` makes Ratchet jump onto crate 410 and idle on its top at z 41.0000. Two runs give
identical traces and PNGs.
