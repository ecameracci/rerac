# Post-port fixes

The original game's own bugs and rough edges that ReRAC keeps on purpose. The port matches the original first
(`decisions.md`, native-first fidelity); these are fixed afterwards, each as a port-only option or change recorded in
`CHANGELOG.md`. Missing or wrong port behaviour is a gap (`gaps.md`), not an entry here.

**Adding an entry.** Next free ID (`PPF-NNN`, never reused), what the player sees, why it happens with the original's
addresses, where the port reproduces it, and a possible fix. Strike an entry (`~~…~~`) with "fixed YYYY-MM-DD, <PR or
file>" when it is fixed.

## Open

### PPF-001: Rilgar biters shot before their switch can never be locked onto

- **Seen:** Rilgar, the room north of the floor switch at (342, 213) (session 2026-10-23_09-41-08, marker 2). A robot
  shot before Ratchet steps on the switch chases him, but the Blaster (and every other lock-on) never targets it.
- **Why:** class 623 (level05 `0x301f48`) sets `mode` 0x1000 (targetable) only in its init (`0x30208c`, the biters
  with no trigger) and when its trigger fires in state 0xf (`0x30287c`); a biter waiting for its trigger starts
  untargetable (`0x3021b4`) but is drawn and can be hit. A hit's knockback (state 0xb) lands in the chase (4), which
  never passes 0xf again, so the bit stays off until it dies.
- **Port:** `rc-game` `rilgar_biter` (`waiting`, the `KNOCKED` arm of `update`), faithful.
- **Fix idea:** set the targetable bit when the knockback lands (0xb → 4).

### PPF-002: an Umbris swamp beast vanishes when its mouth brushes Ratchet outside a lunge

- **Seen:** Umbris, the swamp pool by (245, 316) (session 2026-10-24_08-37-39, before marker 2). The beast that chased
  Ratchet gives up when he jumps out of the water, its mouth touches him as he leaves, and it disappears for good.
- **Why:** class 1059 (level07 `0x30f5f0`) has three ways into the swallow (state 5): the chase's lunge (state 4)
  stores the lunge's start (+0x100) and clears the progress (+0x14c); idle (1) and cruise (2) go there on the mouth
  test (`0x30f4d0`, 1.5) alone. The swallow slides the body from +0x100 toward him, and +0x100 is 0 in all 14
  placements, so the first step puts it 2.8 % of the way from the world's origin (about (7, 9, 2)) and the world box
  test at the end of the update deletes it.
- **Port:** `rc-game` `umbris_swamp` (states 1 and 2 of `update`, `swallow`), faithful to the port's reading of
  `0x30f5f0`; to recheck in the decomp that the 1 / 2 → 5 paths really leave +0x100 alone.
- **Fix idea:** store the position in +0x100 and clear +0x14c on the 1 / 2 → 5 paths too, as the lunge does.

### PPF-003: Gaspar's cannon flickers as Ratchet walks up to it

- **Seen:** Gaspar, the cannon by (284, 344) (session 2026-10-30_15-54-38, marker 1). As Ratchet comes near, the
  cannon sinks and turns toward him and its whole body flashes on and off for about two-thirds of a second, then
  darkens again when he walks away.
- **Why:** class 1201 (level09 `0x304c80`, idle state 4) allocates a white seat light (colour 4, radius 0.4, at the
  seat + rows·(0, −0.3, −0.1)) once Ratchet is within 4 (xy) and blinks it for its first 40 ticks (radius 0 every
  other 4 ticks); `MobyProc` (0x212150) merges every point light within its radius of a moby's sphere centre into
  that moby's third directional light, so the seat light lights the whole cannon, not just the seat. Same under
  Lighting Original and Enhanced (measured over a 180-frame dump: 4-frame steps until frame 42, a drop when the
  light is freed).
- **Port:** `rc-game` `gaspar_cannon` (`idle`) and `rc-engine` moby_render `point_light_merge`, faithful.
- **Fix idea:** a port-only option to weaken the cannon's own merge of its seat light (or light only the vertices near
  the seat), keeping the blink on the seat and the ground.
