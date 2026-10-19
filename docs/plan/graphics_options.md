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
| Ambient occlusion | Original / On | the baked vertex colours only | built (`ao_render.rs`, `ao.wgsl`): a spiral SSAO from the depth buffer after the opaque pass, a depth-aware 4Ã—4 blur, multiplied into the frame before the transparent pass; faded out by 150 units; the 32 nearest drawn mobys are left unoccluded inside their bounding spheres, above their feet and in front of the sphere's centre (the wall behind a climbing Ratchet keeps its occlusion). Off: the pass is not added (no `AoTarget` on the view) |
| Particles | Original / Soft | sprites cut where they cross the world | built (`particle_render.rs`, `particle.wgsl`): the blend alpha fades over 1.5 units in front of the depth copy. Original: fade distance 0, the factor is 1 |
| Shadows | On / Off / Dynamic | On: the game's shadows, cast by characters and enemies (the classes with a shadow block) | On / Off exist; Dynamic built (`sun_shadow.rs`, `sun_shadow.wgsl`, `sun_rays.rs`, `sun_rays.wgsl`): in place of the game's shadows, a sun light along the level's key light (`ShadowGame::sun_light`: light set 0's direction A, fixed for the level, raised to at least 60Â° on the same bearing) with a cascaded shadow map (3 cascades to 150 units) that every tfrag (its LOD-0 list), tie (LOD 0 unmorphed, the ones culled for the camera too: `tie_lod` hides none while the sun casts), shrub (cut out by its texture's alpha) and moby casts into through its shader's `PREPASS_PIPELINE` branch (the GS alpha test kept, non-Z-writing passes skipped), received by a full-screen pass over the depth buffer (3Ã—3 PCF, multiplied in before the transparent pass); and the same sun's rays, the bright sky around its screen point blurred radially at half resolution (40 taps) and added in PostProcess before the AA blit, skipped with the sun behind the camera, faded at the screen edge and the horizon. Not part of the preset |
| Water | Original / Enhanced | the level-code water as the game draws it | built (`water_render.rs`, `water.wgsl`): on clear FX textures (`clear_water`, by average colour; lava and sludge stay Original; the log names each texture's verdict) the surface passes (kinds 0 and 2) draw the whole water from the scene copy, opaque: travelling sine-wave normals, the bed bent by them (refraction) and absorbed over the thickness toward the texture's colour, caustics on the bed, a Fresnel screen-space reflection (fog colour where the ray leaves the screen), a sun glint and shore foam; the second layers (kinds 1 and 3) draw nothing. Original: the shader's Original branch returns what it did before |

Enhanced: Anti-aliasing 4x (the best of 4x / 2x the GPU offers), Detail Far, Textures Smooth, HUD High resolution
(Original until High resolution exists), Ambient occlusion On, Water Enhanced, Particles Soft.

## Notes

* The HUD's per-sprite filtering is the GS's and stays exact in every mode (`hud.wgsl`); only the composite to the
  frame changes. At a 416p frame both modes are identical (1:1).
* The interlace half-line offsets (XYOFFSET) are not part of Anti-aliasing Original: on a progressive screen they would
  only show as jitter. Nor is the blit's in-place read (it copies the draw buffer onto itself 32 pixels at a time; which
  texels the GS texture cache still holds at a strip's left edge is not modelled).
* The anti-aliasing row offers 2x / 4x / 8x only where the GPU can multisample the world's attachments (M-series: no 8x);
  the old `msaa = N` key is read once when `anti_aliasing` is missing (N > 1 keeps that multisampling, else Original).
* Persisted keys in the port settings file: `anti_aliasing` (`original`, `off`, `2x`, `4x`, `8x`), `hud`, `textures`,
  `detail`, `ambient_occlusion`, `water`, `particles` (values: the lower-case names), `shadows`
  (`on`, `off`, `dynamic`). `RC_GFX_ANTI_ALIASING`, `RC_MSAA=0|2|4|8`, `RC_GFX_HUD`, `RC_GFX_TEXTURES`,
  `RC_GFX_DETAIL`, `RC_GFX_AMBIENT_OCCLUSION`, `RC_GFX_WATER`, `RC_GFX_PARTICLES`,
  `RC_GFX_PRESET=original|enhanced` and `RC_SHADOWS=0|1|dynamic` override at start. A `sun_rays` key left by an
  earlier build is ignored (the rays are part of Shadows Dynamic).
* The shared pieces: `fx_view.rs` / `fx_view.wgsl` (the view uniform, declared in the module at group 0 binding 0
  because naga_oil cannot pass a uniform's struct to an imported function by value, and the full-screen pipeline
  helpers; identifiers in that module must not end in a digit, which naga_oil renames),
  `scene_copy.rs` (the depth, and for Enhanced water the colour, copied after the opaque pass into images the
  water and particle materials sample; 1×1 while neither option is on). The main camera's depth is
  `TEXTURE_BINDING` for these passes.
* Ambient occlusion, Water, Particles and Dynamic shadows were written from the standard techniques
  (Alchemy-style SSAO, cascaded shadow maps with a deferred receiver, radial-blur light shafts, soft particles,
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
* The Port Options page (fourteen rows) keeps the Camera sub-page's frame and scrolls, four rows at a time (port.rs `VISIBLE`).
