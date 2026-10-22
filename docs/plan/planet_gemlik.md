# Gemlik (level 13): the batch C checklist

Batch C's fifteenth planet, done 2026-10-03 after Hoven. Before this pass Gemlik ran ported code for its help
director 558, the story director 1353, the flying biters 63, the asteroids 212 / 1412, the explosive tanks 1261, the
wave gate 1271, the rotators, the gun turrets 29, the watchers 1577, the breakable props 1805, the base battle (the
ship 69, Qwark's ship 388 and its parts, the shots and missiles, the floating pickups 224 / 228) and the shared
crates, bolts and checkpoints. The census of 2026-10-03 left these classes without a port; this page tracks them.

## Classes without a port (census 2026-10-03)

| class | what | status |
|---|---|---|
| 6 | the state triggers (cuboid → mobys' and groups' states) | **ported** (`units::gemlik_trigger`) |
| 21, 244, 1270 | the lifts with their camera ride; the tipping lift | **ported** (`units::gemlik_lift`) |
| 101 (+ 1233) | the missile drones and their missiles | **ported** (`units::gemlik_launcher`) |
| 111 | the space fighters of the base battle | **ported** (`units::gemlik_fighter`) |
| 170 (+ 1632) | the base towers (hit from above, the staged blow-up) and their hit proxies | **ported** (`units::gemlik_tower`) |
| 231 | the target switches (group, countdown) | **ported** (`units::gemlik_switch`) |
| 404, 405 | the tower fields | **ported** (`units::gemlik_field`) |
| 667, 674, 677, 680 | the state relay and its watched fields | **ported** (`units::gemlik_relay`) |
| 1262 | the stomper robots and their shockwave ring | **ported** (`units::gemlik_robot`) |
| 1403, 1558 | the tracker and the scene thrusters | **ported** (`units::gemlik_small`) |

## Shared-system changes in this pass

- `Spring` 0x270780 vs `0x270830`: the Hoven seeker mine now uses the right one (`hero::physics::spring`).

## Notes

- [L] The tower blasts' velocity argument and the fields' texture scroll have no renderer input yet.
- [L] The robot ring's TEST change (alpha reference for the z write) has no counterpart.
- 1558's actor is the word 0x16cce0, the actor slot table 0x16ccd8 + 4·k (level 01's 0x16ce58, 0x180 lower on
  level13) at k = 2: the scene's third actor (checked 2026-10-21, G-TRI-018).

## Open

- The user's QA: docs/test-requests/2026-10-03-gemlik.md.
