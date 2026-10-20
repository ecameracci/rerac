# Veldin's last level (level 18, Kyzil Plateau again): the batch C checklist

Veldin's second visit: the run through Drek's base to the last arenas and the boss 1422 (`units::veldin_boss`), the
scenes 3 → 4 → 5 after it, the slideshow and the ending movie. The first visit is [planet_veldin.md](planet_veldin.md).
Started 2026-10-02.

## Classes placed (level18 class table; census 2026-10-01)

Ported before this page (earlier batches): the vendor 11, bolts 13, the Hydrodisplacer pads 341, the props of 419,
crates 500 / 501 / 502 / 505 / 511, the rolling mines 568, the boss's pads 583 and countdown 586, the floaters 587,
buried bolts 605, the Trespasser locks 615, the cutaway director 644, horny toads 749, the floor switch 830, the swing
targets 758 / 803, checkpoints 805, 832, the barricades 885..936, the petal door 1021, the kill cuboid 1039, lamps 1060,
gold bolts 1134, the teleporter pads 1135, the divers 1355, the falling platforms 1381, the boss 1422, 1432, path
gliders 1564, 1584, the save before the boss 1750, Giant Clank's pad 1899, the hoppers 1906.

| class | placed | what | status |
|---|---|---|---|
| 582 | 1 | the rail chooser: four rails laid both ways, only the one starting nearer Ratchet grindable; the grind catch widened; two nanotech clusters | **ported 2026-10-02** (`units::veldin_rails`; the rails reach the hero through `HeroFields::rail_radius`) |
| 1392 | 2 | the fields beside the Trespasser locks (a hum, pulsing bands until the lock is solved) | **ported 2026-10-02** (`units::veldin_lock_field`) |
| 1402 | 3 | the Hydrodisplacer pools (two heights, the group carried, three surface meshes, flag 0x7e swaps the starts) | **ported 2026-10-02** (`units::veldin_pool`; the meshes read from the overlay at the level load) |
| 1434 / 1435 | 1 / 2 | the pieces that turn over once the boss's checkpoint is set | **ported 2026-10-02** (`units::veldin_turnover`) |
| 1799 | 1 | the boss's jet in the scenes 0..3 | **ported 2026-10-02** (`units::veldin_scene_jet`) |
| 1890 | 1 | the energy fan (gone once its switch group is thrown; also Kalebo III's 1443) | **ported 2026-10-02** (`units::energy_fan`) |
| 638 | 46 | the hover troopers (patrols, bursts of shots 49, knockback and death flight, skill point 0x1d after ten clean kills; also Kalebo III's) | **ported 2026-10-02** (`units::drop_trooper`) |
| 1356 | 10 | the dropships (fly in with up to four troopers, drop them, fly out or hover and fire homing shots 50; also Kalebo III's) | **ported 2026-10-02** (`units::dropship`) |
| 1454 | 3 | the tanks (a path, a tracking turret, shells 41 that home within 10, treads 331; six damage) | **ported 2026-10-02** (`units::veldin_tank`; the shell's trail shared with the boss's shell) |
| 1563 | 1 | the cutscene effects (the seat glows, scene 4's jets, blasts and the big blast, the piece's glow and beam, the white flash, scene 3's puffs, scene 5's trail, scene 1's Morph-o-Ray beam) | **ported 2026-10-02** (`units::veldin_finale_fx`; the beam through `hero::morph_ray::beam_step`, now shared) |

## Open

- Every placed class of level 18 now has a port; Kalebo III's 638, 1356 and 1443 run the same ports by code identity
  (their level checks for 18 fail there, as in the game).
- ~~Scene 1 sets the Morph-o-Ray's range word to 24 for the rest of the level (the hero's item would reach 24 after
  it); the port keeps the item at 8 [L].~~ **Closed 2026-10-19: not a leak.** The range is gp−0x54b4 = 0x16174c
  (8.0 in the overlay's data). 1563's update `0x2f88e8` (scene 1, ticks 446..502, actor 4) keeps it in $f20
  (0x2f956c), stores 24 in the delay slot of `jal 0x2bf908` (0x2f9578) and stores $f20 back right after the call
  (0x2f9580; $f20 is saved by the beam's prologue): only the scene's beam call sees 24. Readers (verified): the search
  0x2bec80, the lay 0x2bf834, the beam's line / length 0x2bf9e0 / 0x2bfa10, the pulses 0x2c02e8, and class 804's
  update 0x2e1da0 (unplaced on 18, no spawn of 0x324 in the overlay [L]; unported, G-WPN-021). The port was already right: the scene's
  beam runs on 1563's own `Beam` at 24, the hero's item at `hero::morph_ray::RANGE` 8
  (`units::veldin_finale_fx::morph_beam_from`, test `scene_one_beam_reaches_24_and_the_item_stays_at_8`).
- The grind-path cuts of the falling platforms (G-HERO-039); the rail chooser's radius channel is the hero side
  such a cut would also need.
- The pools' ripple patch word (`0x1db508 + n·0x1190`) has no reader on level 18: no ripple manager sets up the patch
  table there [L].
- A full play-through.
