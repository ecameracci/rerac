# Graphics options (port-only)

Decided 2026-10-10 with the user. Code: `rc-engine/src/graphics.rs` (the settings), `rc-game/src/menus/pause/port.rs`
(the Port Options rows), `rc-engine/src/menu_render.rs` (`sync_graphics_rows` / `read_graphics_rows`).

## The rule

Every option controls exactly one thing, and its **first value is always Original**: what the PS2 showed on a TV.
The other values are modern alternatives. A **Preset** row sets them all: Original (every option Original, the
default) or Enhanced; it shows Custom when the options match no preset (never chosen with ✕). The preset is not
stored; it is read back from the options. Options never change game state: the game logic keeps its own decisions
(LOD choice, the moby "drawn" flags, culling), the options only change what is drawn.

## Rows (in this order on the page)

| Row | Values | Original means | Status |
|---|---|---|---|
| Preset | Original / Enhanced / (Custom) | every row Original | done |
| Fullscreen, Resolution, Gameplay aspect, Cutscene aspect, FPS limit | | | exist (not part of the preset: they depend on the screen) |
| Anti-aliasing | Original / Off / 2x / 4x / 8x | the game's two full-screen bilinear copies, drawn at the frame's resolution: the AA blit A (`PutAABlitPacket_A` 0x223200, before the HUD: U = X across, 416 → 415 lines down) and the display copy B (`PutAABlitPacket_B` 0x223260, every frame: U = X across, 416 → 448 lines). Off = neither; 2x..8x = multisampling | done (`aa_blit.rs`, `aa_blit.wgsl`: four one-axis draws, each frame pixel reads the box of one game pixel a GS bilinear tap is; at 416 lines the vertical is the GS's result; checked against an independent reference at 416p, within 3 levels over four 8-bit draws). Off byte-identical to the earlier build |
| Detail distance | Original / Far / Farther / Maximum | the game's LOD distances (×1, ×2, ×4; Maximum ×1000, past every draw distance) | done: tfrag (`trunc(D·1024)` thresholds and the morph constants), tie (class near/mid/far), moby (`lod_trans << 10`, `moby_lod::set_lod_scale`; drawing only: every joint is evaluated whatever LOD is drawn), shrub mesh/billboard (F' = F × scale; within 24 units of D the class draws as one without a billboard: `shrub_render::detail_fade`, shrub.wgsl, shrub_billboard.wgsl; the factor rides in `tex_mode.y`). Draw distances are never scaled. Original byte-identical to the earlier build |
| Texture filtering | Original / Smooth / Sharp | the GS mip rule: one level from depth, `round(log2(z/32)+K)`; Smooth = the same level unrounded, two levels blended; Sharp = the GPU's footprint level + anisotropic | done (tfrag, tie, shrub, shrub billboard: `sample_world`; the mode rides in the shared fog buffer's third vec4 `tex_mode`; samplers linear between levels + anisotropy 16, an integer level reads one level: Original verified byte-identical to the earlier build on a Novalis walk) |
| HUD | Original / Sharp pixels / High resolution | the 512×416 screen through the display copy B (half a pixel across, like the world) and scaled up with bilinear filtering, as the TV and an emulator's display showed it; Sharp pixels = nearest (the port's earlier look); High resolution = drawn at the frame's resolution | Original / Sharp pixels done |
| Ambient occlusion | Original / On | the baked vertex colours only | built (`ao_render.rs`, `ao.wgsl`): a spiral SSAO from the depth buffer after the opaque pass, a depth-aware 4×4 blur, multiplied into the frame before the transparent pass; faded out by 150 units; the 32 nearest drawn mobys are left unoccluded inside their bounding spheres, above their feet and in front of the sphere's centre (the wall behind a climbing Ratchet keeps its occlusion). Off: the pass is not added (no `AoTarget` on the view) |
| Particles | Original / Soft | sprites cut where they cross the world | built (`particle_render.rs`, `particle.wgsl`): the blend alpha fades over 1.5 units in front of the depth copy. Original: fade distance 0, the factor is 1 |
| Lighting | Original / Simple / Enhanced | the game's shadows, cast by characters and enemies (the classes with a shadow block, `shadow_render.rs`) | done (2026-10-23; it replaced the Shadows row On / Off / Dynamic: Original is the old On, Simple the old Dynamic without the rays, Enhanced the old Dynamic). Simple (`sun_shadow.rs`, `sun_shadow.wgsl`): in place of the game's shadows, a sun light along the level's key light (`ShadowGame::sun_light`: light set 0's direction A, or its light B where A lights from below and B is the brighter, as on Drek's fleet (`shadow_render::key_light`), fixed for the level, raised to at least 60° on the same bearing) with a cascaded shadow map (3 cascades to 150 units) that every tfrag (its LOD-0 list), tie (LOD 0 unmorphed, the ones culled for the camera too: `tie_lod` hides none while the sun casts), shrub (cut out by its texture's alpha) and moby casts into through its shader's `PREPASS_PIPELINE` branch (the GS alpha test kept, non-Z-writing passes skipped), received by a full-screen pass over the depth buffer (3×3 PCF, multiplied in before the transparent pass). Enhanced: Simple's shadows and every advanced lighting effect: the same sun's rays (`sun_rays.rs`, `sun_rays.wgsl`), the bright sky around its screen point blurred radially at half resolution (40 taps) and added in PostProcess before the AA blit, skipped with the sun behind the camera, faded at the screen edge and the horizon; and its volumetric light (2026-10-24, `volumetric_light.rs`, `volumetric_light.wgsl`), shafts in the air with the sun on or off screen: at half resolution each pixel's view ray is marched from the camera to the depth-buffer surface (at most 48 units, 12 toward the sky) in 24 steps packed toward the camera from an interleaved-gradient-noise start, each step testing the sun's cascades (one comparison tap); lit steps add a uniform thin medium's in-scattering (σ 0.006 per unit, extinction along the ray) weighted by a Henyey-Greenstein phase (g 0.5, forward); a 4×4 depth-aware blur upsamples it and adds it to the frame in PostProcess before the rays. Its colour is the key light's (light set 0's colour A) pulled 35 % toward the fog colour, its strength follows that light's brightness (none below a luminance of 0.15, full from 0.6) and fades at the horizon. **The shafts follow the shadow light** (the key light raised to 60°), so they match the shadows on the ground: on Kerwan's first tunnel the raised sun does not reach inside, so the shafts are in the opening (cut by the arch and the hanging vines), not in the tunnel itself. **The gate:** a new lighting effect (volumetric light, global illumination) runs only while `GraphicsSettings::lighting.enhanced()` (`Lighting::enhanced`; `Lighting::sun_shadows` / `game_shadows` pick the shadows), in the main world or the render world's extracted copy. Not part of the preset (a preset keeps it; the Preset row ignores it) |
| Water | Original / Enhanced | the level-code water as the game draws it | built (`water_render.rs`, `water.wgsl`): on clear FX textures (`clear_water`, by average colour; lava and sludge stay Original; the log names each texture's verdict) the surface passes (kinds 0 and 2) draw the whole water from the scene copy, opaque: travelling sine-wave normals, the bed bent by them (refraction) and absorbed over the thickness toward the texture's colour, caustics on the bed, a Fresnel screen-space reflection (fog colour where the ray leaves the screen), a sun glint and shore foam; the second layers (kinds 1 and 3) draw nothing. Port-only addition: a shallow sheet flowing down every flow chute (class 679's splines, `chute_water.rs`; kind 4: the slope's normal tilted by a height field carried at the chute's speed, white streaks along the flow; rows over strip water left out; a row stops at the chute's walls and at a step up, so it never spreads onto the floor a chute is sunk in, and rows on open ground are left out), which the game leaves bare. Original: the shader's Original branch returns what it did before; kind 4 draws nothing |

Enhanced: Anti-aliasing 4x (the best of 4x / 2x the GPU offers), Detail Far, Textures Smooth, HUD High resolution
(Original until High resolution exists), Ambient occlusion On, Water Enhanced, Particles Soft. Neither preset touches
Lighting, as it did not touch the Shadows row before it (`GraphicsSettings::with_preset` keeps it, `preset` ignores it).

## Notes

* The HUD's per-sprite filtering is the GS's and stays exact in every mode (`hud.wgsl`); only the composite to the
  frame changes. At a 416p frame both modes are identical (1:1).
* The interlace half-line offsets (XYOFFSET) are not part of Anti-aliasing Original: on a progressive screen they would
  only show as jitter. Nor is the blit's in-place read (it copies the draw buffer onto itself 32 pixels at a time; which
  texels the GS texture cache still holds at a strip's left edge is not modelled).
* The anti-aliasing row offers 2x / 4x / 8x only where the GPU can multisample the world's attachments (M-series: no 8x);
  the old `msaa = N` key is read once when `anti_aliasing` is missing (N > 1 keeps that multisampling, else Original).
* Persisted keys in the port settings file: `anti_aliasing` (`original`, `off`, `2x`, `4x`, `8x`), `hud`, `textures`,
  `detail`, `ambient_occlusion`, `water`, `particles` (values: the lower-case names), `lighting`
  (`original`, `simple`, `enhanced`). `RC_GFX_ANTI_ALIASING`, `RC_MSAA=0|2|4|8`, `RC_GFX_HUD`, `RC_GFX_TEXTURES`,
  `RC_GFX_DETAIL`, `RC_GFX_AMBIENT_OCCLUSION`, `RC_GFX_WATER`, `RC_GFX_PARTICLES`, `RC_GFX_LIGHTING`,
  `RC_GFX_PRESET=original|enhanced` (Lighting still from the file) override at start; the older `RC_SHADOWS=1|dynamic`
  sets Lighting Original / Enhanced (under `RC_GFX_LIGHTING`), and `RC_SHADOWS=0` is a developer switch for no
  shadows at all (`ShadowSettings::off`; the row has no Off). Migration: the Shadows row's `shadows` key is read once
  when `lighting` is missing: `on` and `off` → Original, `dynamic` → Enhanced; the next save writes `lighting`. A
  `sun_rays` key left by an earlier build is ignored (the rays are part of Lighting Enhanced).
* The shared pieces: `fx_view.rs` / `fx_view.wgsl` (the view uniform, declared in the module at group 0 binding 0
  because naga_oil cannot pass a uniform's struct to an imported function by value, and the full-screen pipeline
  helpers; identifiers in that module must not end in a digit, which naga_oil renames),
  `scene_copy.rs` (the depth, and for Enhanced water the colour, copied after the opaque pass into images the
  water and particle materials sample; 1×1 while neither option is on). The main camera's depth is
  `TEXTURE_BINDING` for these passes.
* Ambient occlusion, Water, Particles and Dynamic shadows were written from the standard techniques
  (Alchemy-style SSAO, cascaded shadow maps with a deferred receiver, radial-blur light shafts, ray-marched
  in-scattering through the shadow map with a Henyey-Greenstein phase, soft particles,
  sine-wave normals, refraction, noise caustics, Fresnel with screen-space reflections). The feature list came from
  rac-rs's README, and its look from a screenshot; none of its code (GPL-3.0) was read or used.
* First play test (Novalis, 8x MSAA): Ambient occlusion, Dynamic shadows and the sun rays did not compile (the
  naga_oil limits above), and the first Water Enhanced was too faint; all fixed since. Every pipeline now compiles
  with 8x and without multisampling. Not yet checked on screen since: the look of each option, and Original
  byte-identical to the earlier build (offscreen captures come back blank in a locked session).
* Second play test: Dynamic shadows darkened Ratchet and the whole ground evenly, with no visible shadow shapes. The
  shadow map was right (read back with `RC_SUN_DUMP`), but the Novalis key light is only 33° high, so the giant
  trees' canopies (ties) covered the spawn area. The sun now keeps the key light's bearing at 60° or more; checked
  with `RC_SUN_DEBUG` at the spawn (Ratchet, plants and tree bases cast onto lit ground).
* Lighting checked 2026-10-23 (Novalis, `RC_SCENE=0`, `RC_SCREENSHOT_FRAME=400`, against the build before it): at the
  spawn view Original is byte-identical to Shadows On, and Simple and Enhanced to Dynamic (the sun is off screen there,
  so no rays); on a fly view toward the sun (`RC_PLAY_FLY=1`, `RC_SUN_DIR`) Enhanced is byte-identical to Dynamic and
  Simple differs from it only by the rays (about a third of the pixels, the sky brighter toward the sun).
* The Port Options page (fourteen rows) keeps the Camera sub-page's frame and scrolls, four rows at a time (port.rs `VISIBLE`).

## Light shafts per level

Lighting Enhanced's light shafts (the sun rays, `sun_rays.rs`, and the volumetric light, `volumetric_light.rs`) run
only on the levels of `graphics::SHAFT_LEVELS` (`LightShafts`, `shafts_on_level`), and never while the title world or a
level transition's cards, movies and flight show (the space scenes: `Travel::in_transition`, `TitleWorldRt::shown`).
From the 2026-10-26 play test: on levels without a sun lighting open air the shafts only veil or streak the view.
`RC_LIGHT_SHAFTS=1|0` forces them on or off everywhere (testing).

| Level | Shafts | Why |
|---|---|---|
| 00 Veldin | on | desert daylight |
| 01 Novalis | on | daylight |
| 02 Aridia | off | dust haze, no sun to cast them |
| 03 Kerwan | on | daylight through the tunnels |
| 04 Eudora | on | daylight in the forest |
| 05 Rilgar | off | night |
| 06 Nebula G34 | off | indoor station, no visible sun |
| 07 Umbris | off | dark swamp, no visible sun |
| 08 Batalia | off | rain |
| 09 Gaspar | on | daylight |
| 10 Orxon | off | night |
| 11 Pokitaru | on | daylight |
| 12 Hoven | off | night |
| 13 Gemlik | on | the sun in its sky |
| 14 Oltanis | off | overcast, no visible sun |
| 15 Quartu | off | night, indoor factory |
| 16 Kalebo III | on | daylight |
| 17 Drek's fleet | on | the sun in its sky |
| 18 Veldin | on | daylight |
