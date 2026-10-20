# Umbris (level 07): the batch C checklist

Batch C's tenth planet, done 2026-10-03 after Blarg. Before this pass Umbris ran ported code for its story director
436, the sinking floats 1080, the rocking floats 1069, the lobbing turrets 1041, the linked panels and platforms, and
the shared crates, bolts and checkpoints. The census of 2026-10-03 left these classes without a port; this page tracks
them.

## Classes without a port (census 2026-10-03)

| class | what | status |
|---|---|---|
| 1059 | the swamp beasts: cruising, swallowing Ratchet in the water, wading and biting | **ported** (`units::umbris_swamp`) |
| 1126, 880 | the pop-up turrets and their shots: the sweep, the bursts, down into the ground when hit | **ported** (`units::umbris_turret`) |
| 1110, 1112, 871 | the floating mines: lone mines, the chain on its leader's path, the leader that revives them | **ported** (`units::umbris_mines`) |
| 38, 1474 | the path lifts: the ride, Ratchet held, the vanish and return | **ported** (`units::umbris_lift`) |
| 529, 1789, 1552, 1133, 1142, 1113–1116 | the parked ship, scene jets and dust, the swinging part, the ammo drop, the beast's walls | **ported** (`units::umbris_small`) |
| 1128 | the held linked mover | **ported** (`linked_mover::held_update`) |
| 1106 | the Snagglebeast: the arena walk between its platforms, the tongue grab and the ammo shake, the stomp's rings, the spit, the beam, the fire sweep, the shimmer, the falls, the boss meter, the death | **ported** (`units::umbris_beast`) |
| 1046, 1049 | the beast's shockwave rings and spit globs | **ported** (`units::umbris_beast_fx`, with the tongue, beam, shimmer and fire-line draws) |

## Shared-system changes in this pass

- **Particle type 58** (the ground fire, `particles::type58`) and **the particle hit queue** (`Particles::hits`,
  delivered by `scheduler::part_hits` at the next tick's start: G-PRT-008).
- **`linked_mover::run_with`**: the held variant (1128).

## Notes

- The beast's turn back to the arena reads joint list 10's world heading where the game turns by joint 10's pose
  Euler z (`0x210850` / `0x285d50`) [L].
- The shimmer's GS blend (1.25·Cd + Cs) is drawn additive; the beam's second fan is drawn in the strip's colour (the
  game draws it from a stack copy taken before the colour is set) [L].
- The headless runs reach the arrival scene but not the lair: the beast's fight is checked in the user's QA only.
- **In the port (2026-10-20, issue #36):** the Bomb Glove's bomb no longer goes off at Ratchet's feet in the drained
  Hydrodisplacer pools (the patch mobys 902 / 941 … 971 of `water::managers`). Groups 0 and 1 drain to 0.5 above
  their floor, so the bomb leaves the glove (0.49 up) under the pool moby's water face; the game's hit test takes a
  moby's type-0 face for water (`CollType`, level07 0x21b3c0) and lets a rising bomb through, the port had not
  (`classes::bomb::fly`, hero_gameplay.md §13; tests `weapons::bomb_water_umbris`).

## Open

- The user's QA: docs/test-requests/2026-10-03-umbris.md.
