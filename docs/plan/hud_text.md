# HUD and text rendering (RAC1, NTSC `SCUS_971.99`, Novalis = level 01)

Addresses are **level01.elf** (the overlay carries the engine; boot equivalents in parentheses where Lombyte matched).
gp = 0x166c00, so `gp−0x73f8` = 0x15f808 etc. "Tick" = one game frame; every duration below goes through
`ScaleTicks(n) = (int)(n * f + 0.5)` (`0x220e30`, boot 0x1f96f8), `f` = float at 0x15ed68 (rate factor: 1.0 on
NTSC, 0x3f555555 = 5/6 on PAL, both written by `SetTimeBase` 0x276258 = boot `set_time_base` 0x214970). Confidence: **[H]** read from code and data, **[M]** read from code with lost arguments or
one inference, **[L]** plausible, unverified.

## 1. Assets

### 1.1 Where they are
* **Per-level HUD**: level header `+0x20 hud_header` (raw) and `+0x28 hud_banks[5]` (each WAD-compressed); already in
  `LevelFiles.hud_header` / `hud_banks`. Levels 00-04, 08, 09, 11, 12, 14, 17 (Novalis included) carry byte-identical
  header + banks, equal to the global copy (`extracted/global/hud_header.bin`, `hud_banks/`); 05, 06, 07, 10, 13, 15, 16, 18
  differ (extra level icons). [H]
* **Fonts** are not in the HUD banks: they are **FX textures 1, 2, 3** (`core_index` `fx_textures`, 256×128 PSMT8 each)
  fetched by `GetEffectTex__Fii(n)` (0x21ae98, boot 0x1f44b8). FX 4 = the 64×64 Gadgetron "G" used by the help box.
  `rc_formats::particle_tex::parse_particle_textures` already decodes them (`fx_textures[1..=4]`). [H]
* **Strings**: per-level `gameplay_ntsc` (+0x10.. language table) and the global `all_text` lump (§5). [H]
* `debug_font`, `hud_seqs`, help/option/mission screen PIFs: not used by in-game HUD or text (menus/front end). [M]

### 1.2 `hud_header` layout (4164 bytes for the standard set)
All offsets relative to the header; the game copies it to the HUD heap (`LoadHudBanks__Fv` 0x253e28, boot 0x202a98).

| Off | Type | Meaning |
|---|---|---|
| 0x00 | u16, u16 | 57 = icon entries incl. terminator, 326 = frame entries (0x01460039) |
| 0x04 | u32 | → **icon table** A (8 B each, ends with id 0xffff) |
| 0x08 | u32 | → **frame table** B (4 B each) |
| 0x0c | u32 | → **palette table** C (8 B each) |
| 0x10 | u32 | → **texture table** D (8 B each) |
| 0x14 | u32[5] | cumulative palette count per bank (bank i owns C[prev..this]) — std: 0,0x25,0x25,0x25,0x2a |
| 0x34 | u32[5] | cumulative texture count per bank — std: 0x5c,0xeb,0xeb,0xeb,0xec |
| 0x54 | u32[5] | decompressed size of each bank (0 = absent) — std: 0x20000, 0x4fd00, 0, 0, 0x11400 |
| 0x74 | u32[5] | runtime: bank base pointer (0 on disc) |
| 0x94 | u32[5] | runtime: stash handle (banks 0, 2..4) |

* Icon A: `{u16 icon_id, u16 frame_count, u16 first_frame, u8 anim_mode?, u8 ticks_per_frame}`. Lookup =
  linear scan for the id (`Hud_GetIconIndex__Fi` 0x24a4e0); `GetIconFrame__Fii(id, k)` (0x24fe10) returns
  `first_frame + k` if `k < frame_count` and both the palette and texture are resident, else frame 0. [H]
* Frame B: `{s16 palette_index, s16 texture_index}`. [H]
* Palette C: `{u32 offset | 0x80000000 (bit31 = not yet linked), u16 runtime CBP, u16 0}` — 256×RGBA32 (0x400 B) at
  `offset` in its bank. [H]
* Texture D: `{u32 offset | 0x80000000, u16 runtime TBP, u8 log2 w, u8 log2 h}` — 8-bit indexed, linear rows,
  `w*h` bytes at `offset` in its bank. [H]
* `LinkHudBank__FiPc` (0x24a848) clears bit 31 and adds the bank base for bank *i*'s C and D ranges. [H]

**Bank roles (standard set)**: bank 0 = 92 textures, no palettes (pixels only; palettes live in bank 1), decompressed to
`DAT_00174288+0x60000` and sent once as **PSMT8H** into the free top byte of the 24-bit Z buffer
(`Hud_SendResidentBank__FiPcb` 0x24ab28: TBP from 0x1b0000>>8, each texture advancing `4·w·h` bytes, one texel per
32-bit word).
Bank 1 = palettes 0-36 (0x0..0x9400) + textures 92-234, heap-resident, uploaded on demand. Bank 4 = palettes 37-41 +
texture 235 (256×256 galaxy), streamed. Banks 2, 3 empty. [H]

**Decoding**: identical to level textures — `texture::decode_indexed8(pixels, w, h, palette)` (CSM1 `clut_index`
swizzle + `scale_alpha`) gives correct images for all 326 frames (checked by rendering the whole set). [H]
`GetFrameTex__Fi` (0x24fec0) builds TEX0: TBW = `1<<max(lw−6,0)`, PSM = 0x1b (PSMT8H) if TBP < 0x2800 else 0x13
(PSMT8), TCC = 1, TFX = modulate, CPSM = CT32, CSM1, CLD = 4. Non-resident frames queue an upload in the per-frame
list at 0x16d200 (≤ 64 entries, flushed by `DoGifPaging`). [H]

### 1.3 Icon ids that matter (standard set; frames 32×32 unless noted)
| id | frames | content |
|---|---|---|
| 30006 (0x7536) | 32 | nanotech orb: 0-29 spin, 30 empty shell, 31 glow ring |
| 30031 (0x754f) | 30 | spinning bolt |
| 30080 (0x7580) | 2 | backing bar: 0 = middle (stretched), 1 = end cap |
| 30020 | 4 | △ ○ × × buttons; 30003 / 30015 = orange / blue, red long bars (boss/vehicle meters) |
| 600xx | 1-5 | per-item icons (60000 + item id), several variants incl. "AMMO" |
| 59900-59905 | 10-21 | weapon/gadget/item icons for menus; 59802 misc HUD glyphs; 59801 map tiles |

## 2. The 2D draw path (PATH2 direct, no VU1 program) [H]

Every 2D primitive is a small packet appended at `DAT_001611c0` in the main VIF1 chain: DMA `CNT` tag + VIF
`DIRECT n` + a GIFtag in **REGLIST** mode. No VU1 microprogram is involved (the sprite program 0x10e818 is for
3D billboards/particles).

| Function | GIF regs | PRIM | Use |
|---|---|---|---|
| `HudSprite` 0x2500e0 (boot 0x1ffc30) (frame,x,y,w,h,a) | TEX0 PRIM RGBAQ UV XYZ2 UV XYZ2 | 0x156 SPRITE+TME+ABE+FST | whole HUD texture stretched to w×h |
| 0x250468 same args | TEX0 PRIM RGBAQ (UV XYZ2)×4 | 0x154 TRISTRIP | same, rotated 180° (right end caps) |
| 0x2506c8 same args | as above | 0x154 | same, rotated 90° |
| 0x2502c8 (frame,x,y,w,h,a) | as HudSprite | 0x156 | UV = (w,h) texels, 1:1 sub-rect |
| `HudFrame` 0x24fd28 (slot,frame,x,y,flags,a) | → HudSprite | | adds slot offset (+0x48,+0x4a); flags 1 = centre, 2 = half size, 4 = double |
| `DrawTexturedQuad` 0x21be90 (x,y,w,h,u,v,tw,th,rgba,tex0) | TEX0 PRIM RGBAQ (UV XYZ2)×4 | 0x154 | glyphs, FX textures |
| `DrawRectOverlay` 0x21bce0 (top,bottom,left,right,rgba) | PRIM RGBAQ + 4×XYZ2 | 0x144 untextured TRISTRIP+ABE | frames, fades |

* **Units**: integer pixels in the **512×416** draw buffer (NTSC; PAL 512×448), origin top-left.
  `X = x*16 + OFX − 8`, `Y = y*16 + OFY − 8` with `OFX = (2048 − W/2)*16`, `OFY = (2048 − H/2)*16`
  (`InitViewContext__Fv` 0x219448, `SetPalMode__Fi` 0x219cd0 writes the same into XYOFFSET_1), i.e. window
  pixel = `x − 0.5`. UVs are texel·16 (FST), full texture `(0,0)..(tw*16,th*16)`, no half-texel bias.
  Z = 0xfffff0 for everything (HUD `DAT_0017e964`, glyphs and rects the same constant). No interlace/field
  handling in HUD code: it draws into the full-height frame after the AA pass.
* **Colour**: HUD sprites RGBAQ = `alpha<<24 | 0x7f7f7f` (RGB 127/128, modulate); alpha 0..0x80 = 0..1.0 (values >0x80
  occur for the orb glow and are not clamped). Text uses caller RGBA (R in the low byte).
* **GS state during HUD** (frame render 0x21a1b8, HUD gated by mask bit 0x80): AA blit `FUN_00223200` (38 A+D regs at
  0x151900: TEST=0x30000, CLAMP_1=5, TEX1_1=0x100000261 bilinear) → `fun_00233c28` (0x2b4bc0) replays 0x1c2970:
  **TEST_1 = 0x5380b** (ATE, GEQUAL 0x80, AFAIL = RGB_ONLY; ZTE, ZTST GEQUAL), **ALPHA_1 = 0x44**
  ((Cs−Cd)·As/128 + Cd). HUD packets set no TEX1/CLAMP, so they inherit bilinear + clamp from the AA packet. [M]
  Note: `render_pipeline.md` lists HUD before the AA blit; the code order is AA blit **then** HUD.
* **Layering / order**: HUD slots 0..12 in index order (`HudDraw` 0x24fb50), then the banner, then the help box
  (0x2266c0), then `fun_001f4d98` (screen fade), then `DoGifPaging` (texture uploads spliced earlier in the chain).
  Painter's order; later draws on top. HUD is skipped while `DAT_0017e988` or `DAT_0015f404` is set.

## 3. Text

### 3.1 Fonts and glyph tables [H]
| font | fn (x,y,rgba,str,len) | FX tex | glyph table |
|---|---|---|---|
| regular | 0x21cf70 (Lombyte "font_print_large"), right 0x21d3f8, centre 0x21d5a8 | 1 | 0x1c35d0 |
| small | 0x21cff0, right 0x21d488, centre 0x21d640, window 0x21e0a8 | 2 | 0x1c3970 |
| large | 0x21d070, right 0x21d518, centre 0x21d6d8 | 3 | 0x1c3d10 |

Glyph table: 232 entries × `{u8 u, u8 v, s8 y_off, s8 advance}` indexed by the byte value (data in the overlay's
`.data`; dumped at the time from the C++ `overlay.elf`, now read from `overlay.bin` with `font::read_overlay`). Atlas cells are 16×16 in a 256×128 texture. `len = -1` = to NUL.

### 3.2 `FontPrint` 0x21ccf0 (boot 0x1f62b0) per byte `c` [H]
1. If `DAT_0015f460 == 0`, colour slot 0 (`0x16ccb8[0]`) = caller colour.
2. `c` in 0x08..0x0f: colour code. Only if `DAT_0015f45c != 0`: colour = `(colour & 0xff000000) | (table[c−8] & 0xffffff)`,
   table 0x16ccb8 = {caller, 0x70/0x70/0xE0 blue, 0x40/0xA0/0x40 green, 0xA0/0x60/0xA0 purple, **0xC0/0x80/0x40
   orange** (0x0c, used for all highlighted names), black×3} (R/G/B). Otherwise the byte is skipped.
3. Else if `advance == 0`: skipped (no draw, no advance).
4. `c` in 0x80..0xa7 (accented letter): first draw the accent glyph `e = table[c+0x40]` as a 16×16 quad at
   `(x + e.advance, y + e.y_off)`, UV `(e.u,e.v)`.
5. `c < 0x20` (0x10-0x19 pad icons): 24×16 quad at `(x, y + y_off)`, colour turned grey: `avg=(R+G+B)/3`, keeps alpha.
   `c > 0x20`: 16×16 quad at `(x, y + y_off)`. Space is advance only.
6. `x += advance`. Stops at NUL or `len` bytes. No kerning, no built-in shadow: callers draw a shadow copy
   at (+1,+1) in black (alpha tweened) before the text.

`measure_text_width` (0x21cc40) = sum of non-zero advances (colour codes/icons included as their advance).
Right-align: `x − width`; centre: `x − (width>>1)`.

### 3.3 `FontPrintWindow` 0x21db48 (word wrap) [H]
Window struct (shorts, `FontSetWindow` 0x21e120): `[0] y_min [1] y_max [2] x_min [3] x_max [4] x_anchor
[5] y_start [6] out max_width [7] out height [8] line_height [9] flags [10],[11] x/y sub-pixel (1/16) [12..] unused`.
Flags: 1 = centre each line on `x_anchor`, 2 = centre the block vertically on `y_start`, 4 = measure only,
8 = float-position path (`0x21d0f0`).
* Scissor set to `x_min..x_max−1`, `y_min..y_max−1`, reset to full screen after; `DAT_0015f460` cleared at exit.
* Wrap width `W = 2*min(x_max−x_anchor, x_anchor−x_min)` if flag 1, else `x_max − x_anchor`.
* Greedy break: lines end at byte 0x00 or **0x01 (newline)**; break opportunities at space and any byte < 0x10;
  a word longer than the line is split. Trailing break char is dropped. Colour code active at each line start is
  remembered and passed as that line's colour (`0x16ccb8[idx]`).
* **Balancing**: count lines `n0` at W. Repeat: if lines < 2 stop; if the last line's width ≥ W/3 stop;
  else `W −= 16` and re-wrap; if the line count ever exceeds `n0`, re-wrap at the original W and stop.
* Lines drawn at `y = y_start` (or `y_start − n*lh/2` with flag 2), step `line_height`; a line is skipped if
  `y + lh < y_min` or `y > y_max`.

### 3.4 Message boxes [H unless noted]
* **Frame** `DrawUIFrame(top,bottom,left,right,alpha)` 0x21c958: filled rect RGB (4,4,4) alpha `a` (0x60 default) with
  rounded ends: extra columns `left−2..left` rows `top+1..bottom−1`, `left−3..left−2` rows `top+2..bottom−2`,
  `left−4..left−3` rows `top+4..bottom−4`, mirrored on the right. No 9-slice textures.
* **Bar frame** `0x251ab0(x,y,w,h,a)`: 30080 cap (32 wide) + middle stretched `w−64` + rotated cap.
* **Help / Infobot / tutorial box** (`Help_Update` 0x225bd0, draw 0x2266c0). Open with `Help_Request(msg_id, slot)`
  0x225818 (refused if a box, voice line or stream is active, or the slot's shown-counter at 0x141968+8·slot is −1).
  Size (0x225a98): measure with small font, window {y 240..480, x 44..468, anchor 256, y 360, lh 16, flags 7}; box
  centre (256, H−60), half-size (maxw/2+10, h/2+5), moved up to `H − (h/2+17)` if it would pass `H−12`.
  States (`DAT_00179890`, tick `DAT_00179894`):
  1 open: frame half-size `t*4+8`, 6 ticks → 2 prompt: 64×64 frame + FX-4 logo, ≥24 ticks (waits for the voice clip)
  → 3 grow 32→full over 8 ticks → 4 text fade-in, alpha `t*0x20` over 4 ticks → 5 hold ≥ 420 ticks or until the voice
  line (`entry.audio + 30000`) ends; △ (pad bit 0x10) skips → 6 fade-out `(4−t)*0x20`, 4 ticks → 7 shrink 8 ticks → 0.
  Text: small font, colour 0xffa888 (R 0x88 G 0xA8 B 0xFF) with the alpha above, window `x 44..468, y 240..480`,
  anchor x 256, y = box centre, lh 16, flags lost in decompile (3 inferred [M]). **No typewriter reveal** exists.
* **Banner** (`ShowBanner(msg_id, ticks=180)` 0x2789e0, with printf arg 0x278a50): drawn by 0x24fb50 via 0x251b88 —
  large font centred at (256, y=100), colour 0xf0f0f0, alpha `DAT_0015f644` ramps ±0x80/ScaleTicks(8) per tick while the
  countdown `DAT_0015f640` is non-zero / after it hits 0; shadow at (+1,+1) black; bar frame behind, height 32,
  `x = textleft−32`, `y−8`, alpha `min(a,0x50)`. A second line (0x1795e8) shows while countdown > 1000.
  Used for "Skill Point", Infobot/planet ("coordinates") messages 1009+, etc. [H]

## 4. HUD elements

### 4.1 Slot machinery [H]
13 slots × 0x90 at 0x17e0d0. `HudShow(slot|flags, icon_id, init, update, draw, data_ptr, max)` 0x24ad98
(Lombyte "queue_animation_update"): re-arms only if any argument changed. Fields: +0x04 flags, +0x08 max,
+0x0c data ptr, +0x10/14/18 init/update/draw, +0x40 icon index/anim, +0x48/4a draw offset, **+0x50/54 anchor x,y
(static .data, never written)**, +0x58/5c size, +0x60 anchor bits, +0x70 slide counter, +0x71 alpha counter,
+0x74/78 shown/target value, +0x7c visible timer.
Update loop 0x24f880: if flag 0x10 or the global show-all 0x17e95c, timer = max(timer, 10); timer−−; ramp state.
Each element's update ramps **slide** (+0x70) 0→8 then **alpha** (+0x71) 0→8 one step per tick while timer ≥ 5, and
the reverse (alpha first) when timer < 5. Fractions `s = slide/8`, `f = alpha/8`.
Static anchors (NTSC): slot 0 (20,15) TL · 1 (256,32) TC · 2 (492,15) TR · 3 (20,208) · 4 (492,208) · 5 (20,376) ·
6 (256,376) · 7 (492,376) · 12 (142,50). Element y below uses its own constant (NTSC value; PAL in brackets).

### 4.2 Elements on Novalis
| Slot | Element | Sprites and geometry (NTSC) | Timing / condition | Code |
|---|---|---|---|---|
| 1 | **Health (nanotech)** | bar: cap 30080:1 at `256−hw−32`, middle 30080:0 width `2hw`, cap rotated at `256+hw`, y 16, 32 tall, alpha `20·s` [M]; `hw = 48s` (4 or 8 orbs), 32s (5). Second bar (5 and 8 orbs, `hw2` 16s / 48s) 32 below the first, behind the second orb row (port-only: the disc's 0x15f80c is 16, half over the gap between the rows). Orbs 30006 32×32 centred at `(256+32+ox, 32+oy)`, ox,oy from 0x15f7f8 {−80,−48,−16,16}×0 (4), 0x17e8e0 (5: 3+2 rows), 0x17e8f8 (8: 4+4, row 2 at +32); filled orb = spin frame `(t%60)/2` (+6 per orb, mod 30) + shell 30 + glow 31 (34×34 at +14,−17, alpha `(pulse>>4)·f`); empty = frame 30; alpha `128f`. y 24 on PAL | shown 120 ticks after HP (0x1415f8) changes, always while HP = 1; init shows 210 ticks; max HP 0x15eda0 | show 0x24a498 (called on damage 0x226fa8), update 0x24e238, draw 0x24e418 |
| 2 | **Bolt counter** | bar: cap rotated at x−12, middle `x−28−w` width `w+16`, cap at `x−60−w`, y 18 [10], alpha `0.7·128s`, `w = 13·digits·s`; bolt 30031 frame `(t%60)/2` at (x−32, y) alpha 0x80; digits `"%d"` of 0x15ed98 regular font **right-aligned at x−32, y+8**, colour tween(f, 0x00e08060→0x80e08060) = R 0x60 G 0x80 B 0xE0, shadow black at +1,+1; group separator every 3 digits: `'` at (x−30−13k, y+21) (reads as a comma) or `.` at (x−29−13k, y+10) for German/Italian | 90 ticks after the bolt count changes | show 0x24e8d0-registered (many callers), update 0x24e908, draw 0x24ea00 |
| 0 | **Weapon + ammo** | cap at x−28, middle at x+4, cap rotated after it, y 18 [10], bar alpha `0.7·128s`, length `70s` (95 if max ≥ 100) [M: code uses `x+len` as the middle width]; icon `(60000+item)` frame 3 at (x, y); text `"%d/%d"` (ammo/max) regular, right-aligned at `x+25+70|95`, y+8, colour tween to 0x80e08060 (0x80202080 = red when ammo 0), shadow | persistent (flag 0x10) while the held item has ammo (item table 0x1c4538 stride 0x18, max ammo at +6); switching re-inits (150 ticks) | 0x24f9c0, init 0x24f368, update 0x2519c0, draw 0x24f3b0 |
| 7 | nearby-bolt alert | bar + spinning bolt at (x−32, H−50 [H−42]), large "!" pulsing red 0x800000ff | option flag 0x13d4db and a bolt within 20 units [L] | 0x227d90, draw 0x24eed8 |
| 3 | quick-select ring | icon 59700 quadrants, item icons, names | d-pad/QuickSelect open | 0x242930, draw 0x24d938 |
| 4 | boss/vehicle meters (30003, 30015) | — | level scripts | 0x2406b0, 0x2d2450, 0x303000 |
| 12 | race timer ("Best Time/Score") | — | challenges | 0x278eb8, draw 0x24c898 |

Default Novalis play: nothing is on screen until something changes; ammo appears when a weapon with ammo is held,
bolts on pickup (90 ticks), health on damage (120 ticks), plus help boxes and banners. No level-start planet-name
banner was found in HUD code (the landing cutscene may carry one) [L].

## 5. Strings [H]
* Blocks: `{u32 count, u32 size, entries[count] × {s32 text_offset (from block start), s32 id, s32 audio (help_audio
  index, −1 none; voice = audio+30000 in the sound system), s32 0}, NUL-terminated strings}`.
* `gameplay_ntsc/pal` header +0x10 + 4·lang → block (level text: 1521 English entries in level 01); copied to the level
  heap and relocated (0x255958). `all_text` (global): 8 offsets at +0x00 (En 1762 entries, 1 empty, Fr, De, Es, It,
  6 stub, 7 empty); streamed and swapped in for menus by 0x290d40/0x290cd0.
* Lookup `msg_string__Fi(id)` 0x2259e8: linear search of the 16-byte entries (table `DAT_0015f660`, count 0x1798bc),
  fallback "Paradox! This message does not exist".
* Language `0x15ed88`: 0 En, 1 unused, 2 Fr, 3 De, 4 Es, 5 It; set in `InitOnce` from the PS2 system language
  (EN→0, FR→2, ES→4, DE→3, IT→5, other→0) and by the options menu (0x28e600).
* Encoding: ASCII for 0x20-0x7a (as the glyph table), plus 0x01 newline, 0x08 colour reset, 0x09-0x0c colours
  (English uses 0x0c only), 0x10 × 0x11 ○ 0x12 △ 0x13 □, 0x14 L1 0x15 R1 0x16 L2 0x17 R2 [M: atlas order],
  0x18/0x19 sticks, 0x80-0xa7 accented A a E e I i O o U u in groups of 4 (+0 = acute, from Fr/Es strings; +1..+3
  inferred grave/circumflex/diaeresis [M]), 0xa8-0xb0 extra letters (ñ, ç, ß, ™ …) [L].
* Novalis ids: help/tutorial 1000-1008 (Infobot/planets, long jump, look-around \x14, bolt crank \x13, map, swimming),
  0-5 (Bomb Glove, Nanotech pickup, HelpDesk welcome, Hyper-Strike, Comet-Strike), 20004-20016 (weapon tips);
  planet banners 1009+ ("Infobot for Planet Aridia acquired"); level script 0x30acb8 issues most of them.

## 6. Unknowns
* Exact runtime values of the gp HUD constants (read here from `.data`, e.g. health bar alpha 20, row offset 16)
  (the rate factor 0x15ed68 is settled: 1.0 NTSC / 5/6 PAL from `SetTimeBase`); confirm with a PCSX2 memory dump.
* TEX1/CLAMP inherited by HUD when the AA pass is disabled (`DAT_0016a4b8 == 0`).
* The weapon bar middle width (`x + len`) looks like a bug or a lost register; verify on screen.
* `FontPrintWindow` flags used by the help box draw (stack arg lost); vendor screen text/layout (separate code,
  not covered); slot 3/4/12 details; accent order +1..+3; glyphs 0xa8-0xb0.

## 7. Port plan
**Formats (`rc-formats`)**
* `hud.rs`: parse `hud_header` (tables A-D, per-bank ranges), decompress banks, decode every frame with
  `texture::decode_indexed8`. Golden: frame count 326, icon count 56 (+terminator), and hashes of decoded RGBA for
  the standard set (levels 01 and one of 05/06/07); cross-check against the global copy.
* `font.rs`: glyph tables (3 × 232 × 4 B) read from the overlay `.data` at 0x1c35d0/0x1c3970/0x1c3d10 (keep as a
  golden `.bin` too) + FX textures 1-3 from `particle_tex`. Golden: `measure_text_width` of all English strings equals
  a Python re-implementation over the dumped table.
* `text.rs`: text blocks from `gameplay_*` and `all_text`, per language; golden: entry counts per language
  (1521/0/1498/1497/1498/… for level 01) and a hash of the id→bytes map.

**Engine (`rc-engine`)**
* One 2D pass after the 3D frame (after the AA resolve), orthographic projection mapping window pixel `(x−0.5,
  y−0.5)` of the 512×416 (PAL 512×448) game viewport, letter/pillar-boxed into the window; no depth test.
* Batch sprites/quads/rects in submission order (painter's). Blend `(Cs−Cd)·As/128 + Cd` with alpha in 0..0x80
  (allow >0x80); texture modulate `tex·rgb/128` (RGB 0x7f for HUD), texture alpha scaled as PS2 (`a*2` capped);
  bilinear + clamp to match the AA-state inheritance (make it a toggle). Alpha test GEQUAL 0x80 with RGB_ONLY only
  matters for destination alpha, ignore it in the port.
* Scissor rectangles for `FontPrintWindow`.

**Game (`rc-game`)**
* HUD slot system: 13 slots, static anchors, `HudShow`/update/draw callbacks, 8-step slide then alpha ramps,
  timers via `ScaleTicks`; health, bolts, weapon/ammo first, then banner and help box state machine (states 0-8,
  voice-line hold), string lookup by id and language; unit-test the ramps and the wrap/balance algorithm against
  hand-checked strings, then compare screenshots with PCSX2 at the same tick.

## In the port (2026-09-27)

**Formats** (`crates/rc-formats`; first verified against the C++ oracle's `rc_extract hud` dump, retired 2026-09-27):
* `hud.rs`: `parse_header` / `parse_hud(header, banks)` → icons, frames, palettes, textures, banks; `icon_index`,
  `icon_frame` (= `GetIconFrame` with every bank resident), `frame_size`, `decode_frame` (scaled alpha, as every
  level texture) and `decode_frame_raw` (GS alpha, what the renderer samples). **Correction to §1.2**: every
  per-bank array is `u32[8]` (5 used, 3 zero), at +0x14 / +0x34 / +0x54 / +0x74 / +0x94; the header is 0xb4
  bytes and the icon table follows it.
* `font.rs`: overlay section splitter (the `ratchet-executable` rule of `level.cpp`), `find_glyph_tables`,
  `parse_glyph_tables`, `measure_text_width`, `COLOUR_TABLE` (0x16ccb8). **The glyph tables move per overlay**
  (level 00: 0x1c3150/0x1c34f0/0x1c3890 … 12 distinct address triples over 19 levels) but their 3 × 928 bytes are
  identical everywhere; they are found as the game reaches them: a `jal GetEffectTex` with `li a0, n` in its
  delay slot, then `lui t2, hi` and a `jal FontPrint` with `addiu t2, t2, lo` in its delay slot (Rust: flexible
  window; C++: the exact 11-instruction wrapper; both must agree). The colour table's 7 fixed words are present
  in every overlay.
* `strings.rs`: `parse_strings(gameplay, lang)` (+0x10 + 4·lang), `find_index` / `lookup` (first match,
  "Paradox!" fallback), `display` (control codes as `\xNN`).
* Golden `hud_fonts_and_strings_for_every_level`: 19 levels, 6242 frames (11,147,008 texels), all
  tables, glyph tables + addresses, 28,416 English messages (byte-identical to C++ when the committed snapshot hashes
  were generated); the 11 standard levels
  (00–04, 08, 09, 11, 12, 14, 17) equal the global `hud_header` / `hud_banks`, the other 8 do not; a flipped
  texel byte and a changed glyph advance both reach the snapshot.

**Game** (`crates/rc-game/src/hud.rs`): `HudState::tick(Inputs{hp, max_hp, bolts, weapon, lang}) -> Vec<Draw>`.
Slot machine as §4.1 plus what the code adds: `queue_animation_update` zeroes the slot's timer and ramps when a
new request arrives, the request is applied when +0x6c reaches −6 (level start: all slots at −6); health init
(0x24e1b8) sets timer `ScaleTicks(180)+30` = 210 and draw offset **(32, 0)** (so the orbs centre at
`256 + 32 + ox`), bolts / weapon init (0x24e8d0 / 0x24f368) `ScaleTicks(120)+30` = 150 (so the *first* bolt
pickup shows 150 ticks, later changes 90). Health glow/spin shorts (0x15fab0) and the spin tick ported; health
bar alpha 20·s (so the bar is faint); orb glow alpha `(p1 >> 4)·f` reaches 255. Weapon rule: shown while the
held item's item-table (0x1c4538, stride 0x18) s16 at +0 is non-zero and the player state is 0; the wrench
(item 8) has 0 → nothing (the `Inputs::weapon = None` case). The weapon update clamps its timer to 5 each tick
and flag 0x10 bumps it to 10, so it stays up until released (`FUN_0024b090` clears the flag). Weapon middle bar
width is `x + len` (x = 20) and the right cap follows it: consistent geometry, not a visible glitch. Bolts use
`FastTweenColor` (0x2221a8, `a·(1−t) + b·t` truncated), shadows at +1,+1 for digits and +2,+2 for separators.
Banner: shadow, then the stretchable bar frame, then the text (0x251b88). Help box (§3.4) confirmed: sizing
window flags **7**, draw flags **3**, prompt logo alpha 0x7e (RGB 0x80), grow-state logo alpha `(8−t)·16`.
`FontPrintWindow` layout (`hud::text::layout`) follows the disassembly: the colour slot and the "last line
width" are **not reset between balancing passes** (only before the first), so a string whose last colour code
is not 0x08 starts line 0 in that colour after a re-wrap. Tests: ramp timings, 90-tick bolt visibility, health
at 1 HP, separators for 1234567 En/De, wrench/ammo slot, help state timings (open 0, prompt 6, grow 30, text 38,
hold 42, fade 462, shrink 466, idle 474), wrap/balance.

**Engine** (`hud_render.rs`, `text_render.rs`, `assets/shaders/hud.wgsl`, `gs_state::GsPass::Hud`): the 2D
primitives render in order into an offscreen **512×416** `Rgba16Float` target (own `Camera2d`, order −10) with the
exact mapping (corner at pixel x − 0.5 → integer target coordinate), texel UVs, hand-made bilinear + clamp on an
atlas of raw GS bytes (1/16-texel sample positions, truncated weights [M]), MODULATE `>> 7`, blend `(Cs−Cd)·As+Cd`
without clamping; then a Bevy UI `UiMaterial` node fills the main camera's letterboxed viewport and samples it
**nearest**. Native resolution + integer upscale was chosen because the GS filters at 512×416: sprites stretched by
the game (bar middles, 34×34 glow) blur exactly as on the PS2 and every texel becomes a crisp 2×2 block at
1024×832, as on a PS2 frame shown on a 2× display. The composite runs in the UI pass, after all 3D and before the
underwater tint (`fog_state`), the game's order (HUD in the `0x15f3f4 & 0x80` pass, tint and fades in `& 0x40`).
Differences: the composite mixes in linear light (exact at HUD coverage 0 or 1), As > 0x80 over the 3D scene is
clamped (exact over opaque HUD pixels), no per-blend COLCLAMP inside the HUD target, no texture paging emulation.
`RC_HUD_DEMO=1`, `RC_HUD_TEXT`, `RC_HUD_TEXT_WINDOW=1`, `RC_HUD_HELP=<id>`, `RC_LANG`, `RC_HUD=0` (hud_render.rs).

**Not done** (~~vendor screen~~, ~~quick-select ring (slot 3)~~: done, interaction.md §10 / menus.md §2; the rest is G-UI-011 and G-TOOL-007): boss/vehicle meters (slot 4), race timer (slot 12),
nearby-bolt alert (slot 7), `FontPrintWindow` float path (flag 8) (help voice lines, stream waits and the first-input
gate: done, "Help system" below), `all_text` menus, PAL layout, PCSX2 pixel comparison.

## Help system (2026-09-28)

**A system** (docs/plan/menus.md §10): one copy of `Help_Request` / `Help_Update` / the log / the records in every
overlay; the callers are each their owner's own code. Port: `crates/rc-game/src/help.rs` (`Help` in `Services::help`),
drawn by the HUD from `Help::bx` (`HudState::set_help`; `crate::hud` keeps only the draw-owned 0x1798a8 / 0x1798ac),
run by `crate::gameplay::help_frame` after the tick (`InLevelFrameUpdate` order: moby loop → hero → `Help_Update`).
Records / log mirrored from `GameState` before the tick (`Help::sync_in`) and written back after it (`sync_out`).

State block (0x179890): +0 state, +4 t, +0x20 index (0x1798b0), +0x24 request (0x1798b4), +0x28 rec (0x1798b8), +0x30
enabled (0x1798c0), +0x34 first-input count, +0x38 hold (s16), +0x3a reopened (s16), +0x3c reopen timer. All overlay
.bss: zero on every level load, so help is off until `ScaleTicks(120)` after the level's first direction input
(held 0x13cae0 & 0xf000, stick directions included) — the port no longer starts it enabled.

**Coverage: `Help_Request` 0x225818** [H]

| address | what | port |
|---|---|---|
| 0x225818 state ≠ 0 / request ≠ −1 | refused | `help::Help::request` |
| 0x151720 ≠ 0 / 0x1516ec ≠ −1 | refused while the dialogue player is busy or a line is requested | `request` (`Voice::busy / request`) |
| `help[rec].count` (s16) = −1 | refused ("never again") | `request` (`Records::help_count`, out of range refused) |
| store msg / rec, `FUN_00226a70(msg)` | request, record, log | `request` → `log_append` |

**Coverage: `Help_Update` 0x225bd0** (disassembly; the decompile drops 12 blocks) [H]

| address / case | what | port |
|---|---|---|
| 0x225bf0..0x225c50 | first-input gate: count from the first 0xf000 held, enabled at `ScaleTicks(120)` | `Help::update` |
| 0x225c5c | mode 0x15f5c4 ≠ 0 or disabled: state 0, t 0, request −1 | `update` |
| case 0 | request → `Help_FindIndex` (0x225978) → `Help_ComputeSize` | `update` → `compute_size` |
| cases 1–5, 7 | `force_help_message(5, 0)` | `HelpOut::force_prompt` → `help::tick` → `Interact::force_prompt(5, 0)` |
| case 1 | the voice line request `0x1516ec = audio + 30000` when the player is free; △ → bump, state 7, t = 8 − t; t ≥ 6 → 2 | `update` |
| case 2 | △ → bump, 7; t ≥ `ScaleTicks(24)` → 3 unless the line is this message's and not yet buffered (0x15172a ≠ 3) | `update` |
| case 3 | △ → bump, 7; t ≥ 8 → 4 | `update` |
| case 4 | △ → bump, 6, t = 4 − t; t ≥ 4 → `continue_audio_stream_if_ready` 0x279e78 (its line buffered) → 5 | `update` (`VoiceCmd::Play`) |
| case 5 | hold until t ≥ `ScaleTicks(420)` and its line no longer plays, or △ → bump, 6 | `update` |
| case 6 | text option 0x15ee1d on: 4 ticks (△ skips) → 7 | `update` |
| case 7 | its line still running → stop (0x15172a = 5); t ≥ 8 → 8 if held, else 0 (index −1) | `update` |
| case 8 | held: wait; else reopen after `FastDecTimer(0x1798cc)` (`ComputeSize`, +0x3a = 1), or bump (index ≥ 0) / time + mask only (index < 0), → 0 | `update` |
| the bump (inline ×6) | `count++` unless 0xffff, `time = max(time, ScaleTicks(play)/600)`, `mask |= 1 << level \| 0x80000000` | `help::bump` (= `hero::melee::bump_record`), `help::touch` |
| `Help_ComputeSize` 0x225a98 | state 1, `PlayLevelSoundAtMoby(0, 1, 0)` when text or voice option on, small-font measure (flags 7), box at (256, H − 60) moved up past H − 12 | `compute_size`; the sound → `HelpOut::sounds` → `AudioSystem::play_level_sound_at_moby` |
| `FUN_002258b0` | kill: stop its line, state / t / index / request cleared (no bump) | `Help::kill`; callers `cinematic::{start_scene, start_movie}`, the talker's scene / movie hand-off, `OpenVendorMenu` (vendor 11), `FUN_002aea70` (item offer); ship take-off not ported (G-LVL-001) |
| `FUN_00225a28` / `FUN_00225a88` / `FUN_00225790` | suspend (close the box, reopen timer 60, idle → 8) / resume | `Help::suspend` / `resume`; caller the Visibomb's launch 0x2c8cc0 / end 0x2cb788: **not wired** (the Visibomb is another agent's; G-UI-017 consumer) |
| `FUN_00226a70` + `fun_001fecc8` | the log: the message's index in 0x1798d0 appended, or moved to the end when logged (entry 0 = the welcome, length starts at 1) | `Help::log_append`; table `help::load_log_ids` (relocated per level; identical on all 19) |
| draw 0x2266c0 / `Help_DrawPrompt` 0x2265d8 | as §3.4; nothing unless the text option is on | `HudState::draw_help` |
| music_Update 0x27a688 (dialogue part) + `fun_002156d8` | request → start stream `help_audio[lang·150 + n]` (volume 0 with the voice option off) when free, else stop the running one; states 1 → 3 buffered → 4 playing → 7 | `Help::voice_frame` (every frame, the scene / vendor frames too), `VoiceCmd::{Load, Play, Stop}` → `gameplay::help_frame` (reads the VAG, its length from the header [L: stream latency not modelled; ready on the next tick]) |

**Coverage: the callers.**

| caller | what | port |
|---|---|---|
| Novalis director 1341 `0x30acb8` (U82) | the 9 rules of `units::help_director` (records 0x40..0x44, 0x51, 0x72, 1, 2, 4..6; move records 9, 11..13, 22, 23, 29; planet bits; cuboids; cranks 280; clusters 806 with `FastBSphereCheck` 0 / 1 / −1) | `moby_update::classes::units::help_director` (records bumps M[13], M[22], M[23]; H[0x44] := 0xffff) |
| hero `0x228498` | Hydro-Pack hint 20014 (rec 0x78: owned[4], `FUN_0022dea8` in water); QuickSelect hint 20006 (rec 0x50) every 128 ticks with > 8 weapons | `help::hero_hints` (engine after the hero's normal update) |
| Pyrocitor `0x2cde98` | the hold counter +0x50, move record 10 on a long fire, taps 0x141404 on short ones, 20004 (rec 0x4e) after 3 | `hero::pyrocitor::hold_stats` → `HeroHelp::out` |
| Tesla Claw `0x2ce448` | 20016 (rec 0x81) after 3 short shots (0x141405) in group ok; move record 26 counted (> ticks(70)) / touched on every shot | `hero::tesla::update` → `HeroHelp::out` |
| first-person look (`SetState` 1, `HeroStatePhysics`) | gp 0x15f688 timer 60 → move record 18 bumped, 0x15f68c + 1 (read by the director) | `hero::stance::{entry, first_person_turn}` |
| class 1290 `0x30a6d0` (L09 U307) | 9000 (rec 0x34) with its item gift | not ported (G-UI-017 consumer) |
| `mode_freezeInit(5)` | logs 20011 (save notice) | not ported (the save notice dialog, G-SAV-002) |
| the other 17 levels' directors / NPCs (38 census units) | their own rules | not ported: G-UI-017 consumers (level_scripting.md §3) |

Tests: `help::tests` (gate and log, the log's move-to-end, the state timings and the close bump, △ skip + prompt,
first-input gate and mode, voice wait / play / stop / kill, suspend / reopen, hero hints), `hud::tests::help_box_draws_from_the_help_system`,
`units::help_director::tests`, `tests/ui/help_novalis.rs` (the log table on all 19 levels; the director's look and map
hints in its real cuboids; a whole Infobot box on the level text with its voice line). Frame: Ratchet placed in the
director's look cuboid (`RC_HERO_AT=240.7,175.4,96 RC_PLAY_SCRIPT="2-6:stick 0 -0.3" RC_SCREENSHOT_FRAME=215`): the
gate opens at tick 124, 1004 "To enter look-around mode, press and hold L1." with voice line 30064; two runs identical.
`RC_HUD_HELP=<id>` moved to `crate::gameplay` (forces the request at the first tick, skipping the gate).
