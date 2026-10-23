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
