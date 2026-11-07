# Water Enhanced surfaces, by level

Every water and liquid surface that Water Enhanced (crate::graphics `Water`) redraws, the system that builds it and
where to tune it. The full history and the reasons behind each choice are in graphics_options.md's Water row; this
page is the index. When you add a surface, add its row here; when you tune a look, use the shared points first so
every surface changes together.

## Which surfaces Enhanced takes

Hardcoded per level: `SURFACE_LIFT`, `TFRAG_LIQUIDS`, the wave meshes' `DEFS`, the pools' meshes, `SURFACE_COLOURS`
and the sea and manager ports. Everything else is judged at the level load from the texture's colour
(`water_render::clear_rgba`, `liquid_render::patch_kind`, `sea_render::judged_rgb`), and the log says which way each
went:

* `water: fx texture N is clear water` or `not clear water`, and `water: fx texture N as another liquid … : <kind>`
* `sea: … : <kind>`
* `water: draw order …; strips …; ripple patches …`, `water: chute sheets …`, `water: wave meshes …`,
  `water: pool meshes …`

To see a level's surfaces, play it and grep the log for `water:`, `sea:` and `liquid:`.

| Level | Surface | System | Enhanced kind | Built by |
|---|---|---|---|---|
| 01 Novalis | river and waterfall strips 676 / 678 / 761 / 1225 | water module strips (kinds 0, 1) | water | `water_render.rs` `setup` |
| 01 Novalis | 751 ripple patches | ripple patches (kinds 2, 3) | water | `water_render.rs` `setup` |
| 02 Aridia | quicksand 854 | liquid look | mud | `liquid_render` `quicksand_kind`, `draw` |
| 05 Rilgar | bay sea grid 879 | sea grid | water | `sea_water.rs` `draw` |
| 05 Rilgar | 831 sewer flood patches (lifted 0.25, `SURFACE_LIFT`) | ripple patches | water | `water_render.rs` `setup` |
| 05 Rilgar | flow chutes (class 679) | chute sheet (kind 4) | water | `chute_water.rs` |
| 07 Umbris | lava grid 460 | liquid look | lava | `liquid_render` `draw` |
| 07 Umbris | swamp 1018 | liquid look | sludge | `liquid_render` `draw` |
| 07 Umbris | Hydrodisplacer pool patches 902, 941..971 | ripple patches | by the log | `water_render.rs` `setup` |
| 08 Batalia | sea grid 327 | sea grid | water | `sea_water.rs` `draw` |
| 08 Batalia | flow chutes | chute sheet | water | `chute_water.rs` |
| 09 Gaspar | lava grid 317 and its flows | liquid look | lava | `liquid_render` `draw` |
| 10 Orxon | goo (tfrag texture 12) | tfrag look `goo_look` | sludge | `liquid_render` `TFRAG_LIQUIDS`, `tfrag.wgsl` |
| 11 Pokitaru | ocean 1111 | ocean | water | `ocean_water.rs` `draw` |
| 11 Pokitaru | 1158 tide patches | ripple patches | by the log | `water_render.rs` `setup` |
| 12 Hoven | 1901 | liquid look | ice | `liquid_render` `hoven_ice` |
| 12 Hoven | rising-water patches | ripple patches | by the log | `water_render.rs` `setup` |
| 13 Gemlik | acid patches 1263 / 1393 (FX 46 / 45) | ripple patches, liquid look | sludge | `liquid_render` `patch_kind` |
| 13 Gemlik | orange patches (FX 54 / 53) | ripple patches, liquid look | lava by `patch_lava` (G-REN-132 says Original: to check) | `liquid_render` `patch_kind` |
| 15 Quartu | tanks: wave meshes 1408 / 1409 / 1565 / 1567 | wave mesh sheet (kind 0, tank) | water | `water_render.rs` `WaveSheet`, `units::wave_mesh` |
| 15 Quartu | green pools (tfrag texture 10) | tfrag look `swirl_look` | sludge | `TFRAG_LIQUIDS` |
| 16 Kalebo III | ocean 1111 | ocean | water (not checked on screen) | `ocean_water.rs` `draw` |
| 17 Drek's fleet | wave meshes 1405 / 1406 / 1407 / 1426 | wave mesh sheet (kind 0, tank) | water | `water_render.rs` `WaveSheet` |
| 17 Drek's fleet | green liquid (tfrag texture 57) | tfrag look `swirl_look` | sludge | `TFRAG_LIQUIDS` |
| 18 Veldin | Hydrodisplacer pools 1402 (three meshes, raised and lowered) | pool sheet (kind 0, tank) | water | `water_render.rs` `PoolSheet`, `units::veldin_pool` |

Kept Original on purpose: Novalis's 1848 overlay, Kerwan's 994 cloud floor (its fountains are not found,
G-REN-115), Batalia's 600 / 1649 and Pokitaru's 1903 / 1919 overlays, Hoven's 293 and Oltanis's 1418 two-texture
strips and Oltanis's 1260 haze (G-REN-039), Quartu's dry chute. 00 Veldin, 04 Eudora and 06 Nebula G34 have no
liquid Enhanced takes.

The planar reflection (`water_mirror.rs`, fed by `WaterState::flat_surfaces`, the sea grids and
`OceanWater::plane`) and the underwater pass (`underwater_render.rs`) apply to the water rows above.

## Tuning points

Shared by every surface of a system:

* The clear-water test: `water_render::clear_rgba`. `sea_water::clear_liquid` and `liquid_render::classify` use
  it too.
* The water look: `assets/shaders/water.wgsl` `fragment`. It covers the strips, patches, seas, wave and pool sheets,
  chutes and the ocean (the ocean adds its own `OCEAN_*` overrides). It holds:
  * deep share, absorption, tint mix, crest;
  * Fresnel, the screen-space reflection, sun glint, reflection cap;
  * foam, caustics, refraction bend.
* The swell: `water_waves.rs` (`WAVES`, `SPEED`, `MAX_HEIGHT`, `SWELL_FADE`, `SHORE_DEPTH`, `CREST`). It is mirrored
  in `water.wgsl`, so edit both together.
* The liquid looks: `assets/shaders/liquid_look.wgsl` (`DRIFT`, `LAVA_FINE`, `ICE`, `MUD_CELL`). These are mirrored
  in `liquid_render.rs`'s test copies.
* The switches: `RC_WATER_WAVES_SCALE`, `RC_LIQUID_FX`, `RC_LIQUID_FX_KIND` (forces one kind everywhere, for tuning),
  `RC_WATER_MIRROR`.

Tuning for one system:

* Seas: `sea_water.rs` `FAR_FADE`, `DEEP`, `CLEAR_PATH`.
* Ocean: `ocean_water.rs` (`SWELL`, `OPEN`, `SKY`, `ABSORB`, `FAR_FADE`, `DEEP`) and `water.wgsl` `OCEAN_*`.
* Tanks:
  * `water_render.rs` `WAVE_RGBA` (wave meshes) and `POOL_RGBA` (Veldin's pools) set the tint.
  * `params.misc.w` = −1 turns off shore foam.
* Per level: `water_render.rs` `SURFACE_LIFT`, `CHUTE_FX`; `liquid_render.rs` `TFRAG_LIQUIDS`;
  `underwater_render.rs` `SURFACE_COLOURS`.
* Chutes: `chute_water.rs`. Mirror: `water_mirror.rs`.

Open water gaps: G-REN-039, G-REN-110, G-REN-111, G-REN-115, G-REN-130, G-REN-132, G-REN-133, G-REN-210, G-REN-224
(docs/plan/gaps.md).
