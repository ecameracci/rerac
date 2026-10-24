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
| Frame smoothing (after FPS limit) | Off / On | Off: the game's 60 Hz ticks, each tick's picture shown until the next (pacing unchanged) | built (`frame_smooth.rs`, the pacing in `frame_pace.rs`): On, when frames come faster than the ticks (a display above 60 Hz with FPS limit 60 or 120, Unlimited, `RC_NOVSYNC=1`, `RC_FRAME_SMOOTH_HZ`), the limiter follows the display instead of 60 and each frame draws the last two ticks blended by the time since the last tick (`acc / tick`): the camera (lerp + slerp), every moby's instance record and joint palette (Ratchet, his attachments, statics, dynamics, extras) and the particles' positions. The game state never changes: only the render inputs (`GlobalTransform`, the shader buffers) are overwritten in `Last` and put back in `First`. Not part of the preset; at or below 60 Hz On draws as Off (byte-identical) |
| Anti-aliasing | Original / Off / 2x / 4x / 8x | the game's two full-screen bilinear copies, drawn at the frame's resolution: the AA blit A (`PutAABlitPacket_A` 0x223200, before the HUD: U = X across, 416 → 415 lines down) and the display copy B (`PutAABlitPacket_B` 0x223260, every frame: U = X across, 416 → 448 lines). Off = neither; 2x..8x = multisampling | done (`aa_blit.rs`, `aa_blit.wgsl`: four one-axis draws, each frame pixel reads the box of one game pixel a GS bilinear tap is; at 416 lines the vertical is the GS's result; checked against an independent reference at 416p, within 3 levels over four 8-bit draws). Off byte-identical to the earlier build |
| Detail distance | Original / Far / Farther / Maximum | the game's LOD distances (×1, ×2, ×4; Maximum ×1000, past every draw distance) | done: tfrag (`trunc(D·1024)` thresholds and the morph constants), tie (class near/mid/far), moby (`lod_trans << 10`, `moby_lod::set_lod_scale`; drawing only: every joint is evaluated whatever LOD is drawn), shrub mesh/billboard (F' = F × scale; within 24 units of D the class draws as one without a billboard: `shrub_render::detail_fade`, shrub.wgsl, shrub_billboard.wgsl; the factor rides in `tex_mode.y`). Draw distances are the Draw distance row's. Original byte-identical to the earlier build |
| Draw distance | Original / Far / Farther / Maximum | the game's draw distances (×1, ×1.5, ×2, ×4; see "Draw distances" below) | done: tfrag (`tfrag_lod::DRAW_DISTANCE`), tie (per-instance distance), shrub (D and both fade bands), the main camera's far plane (`GameProjection::draw_scale`); above ×1 tfrags and ties fade out over the last 16·s units (4×4 ordered dither, tfrag.wgsl / tie.wgsl); the factor rides in `tex_mode.z`. Mobys, particles, the sky and the level fog are not scaled. Not part of the preset yet. Original byte-identical to the earlier build (8 views on Novalis, Kerwan, Rilgar, Pokitaru, 2026-10-24) |
| Texture filtering | Original / Smooth / Sharp | the GS mip rule: one level from depth, `round(log2(z/32)+K)`; Smooth = the same level unrounded, two levels blended; Sharp = the GPU's footprint level + anisotropic | done (tfrag, tie, shrub, shrub billboard: `sample_world`; the mode rides in the shared fog buffer's third vec4 `tex_mode`; samplers linear between levels + anisotropy 16, an integer level reads one level: Original verified byte-identical to the earlier build on a Novalis walk) |
| HUD | Original / Sharp pixels / High resolution | the 512×416 screen through the display copy B (half a pixel across, like the world) and scaled up with bilinear filtering, as the TV and an emulator's display showed it; Sharp pixels = nearest (the port's earlier look); High resolution = drawn at the frame's resolution | Original / Sharp pixels done |
| Ambient occlusion | Original / On | the baked vertex colours only | built (`ao_render.rs`, `ao.wgsl`): a spiral SSAO from the depth buffer after the opaque pass, a depth-aware 4×4 blur, multiplied into the frame before the transparent pass; faded out by 150 units; the 32 nearest drawn mobys are left unoccluded inside their bounding spheres, above their feet and in front of the sphere's centre (the wall behind a climbing Ratchet keeps its occlusion). Off: the pass is not added (no `AoTarget` on the view) |
| Particles | Original / Soft | sprites cut where they cross the world | built (`particle_render.rs`, `particle.wgsl`): the blend alpha fades over 1.5 units in front of the depth copy. Original: fade distance 0, the factor is 1 |
| Lighting | Original / Simple / Enhanced | the game's shadows, cast by characters and enemies (the classes with a shadow block, `shadow_render.rs`) | done (2026-10-23; it replaced the Shadows row On / Off / Dynamic: Original is the old On, Simple the old Dynamic without the rays, Enhanced the old Dynamic). Simple (`sun_shadow.rs`, `sun_shadow.wgsl`): in place of the game's shadows, a sun light along the level's key light (`ShadowGame::sun_light`: light set 0's direction A, or its light B where A lights from below and B is the brighter, as on Drek's fleet (`shadow_render::key_light`), fixed for the level, raised on the same bearing to at least 60° under Simple; under Enhanced it keeps its own elevation down to a 30° floor, `sun_shadow::min_elevation`, `ENHANCED_FLOOR`, `RC_SUN_FLOOR=<degrees>`, 2026-10-24: Novalis 32.8° and Kerwan 35° are kept) with a cascaded shadow map (3 cascades to 150 units) that every tfrag (its LOD-0 list), tie (LOD 0 unmorphed, the ones culled for the camera too: `tie_lod` hides none while the sun casts), shrub (cut out by its texture's alpha) and moby casts into through its shader's `PREPASS_PIPELINE` branch (the GS alpha test kept, non-Z-writing passes skipped), received by a full-screen pass over the depth buffer (3×3 PCF, multiplied in before the transparent pass). Enhanced: Simple's shadows and every advanced lighting effect: the sun's rays (`sun_rays.rs`, `sun_rays.wgsl`, `sky_sun.rs`; 2026-10-24) from the sun the sky paints, not the shadows' key light: at level load the sky shells are drawn into a 1° latitude–longitude luminance map (each shell at its first tick's SkyM) and the brightest compact spot at least 3° up in one shell's layer, bright and above its row's median and not hidden by the shells drawn after it, is the sun (Veldin's sunset disc, the bright patch of the Novalis / Eudora / Kerwan / Pokitaru / Kalebo III domes, Gemlik's red sun); it turns with its shell. The sky's pixels (depth 0) around its screen point (a falloff of twice the spot's radius, 0.08–0.3 of the screen height), keyed on their luminance between the sky's median and the sun's peak, are blurred radially at half resolution (40 taps) and added in PostProcess before the AA blit, at a strength from the sun's contrast (0.5–1); skipped with the sun behind the camera, faded at the screen edge and the horizon; none on a sky without a sun (night, space, storms: 12 of the 19 levels) or under `RC_SHADOWS=0`; and its volumetric light (2026-10-24, `volumetric_light.rs`, `volumetric_light.wgsl`), shafts in the air with the sun on or off screen: at half resolution each pixel's view ray is marched from the camera to the depth-buffer surface (at most 48 units, 12 toward the sky) in 24 steps packed toward the camera from an interleaved-gradient-noise start, each step testing the sun's cascades (one comparison tap); lit steps add a uniform thin medium's in-scattering (σ 0.02 per unit, extinction along the ray, brightness 2.0) weighted by a Henyey-Greenstein phase (g 0.3, mildly forward). **Shadow-contrast weighting** (2026-10-24, `volumetric_light::Shafts`, mirrored in the shader): each lit step counts in full within about 4 units along the ray of a long shadow bounding its lit run (`1 − (1 − a)(1 − b)`, `a`, `b` = `exp(−d/4)` to the run's ends), and 0.05 in lit air far from shadow; a bounding shadow counts by its length along the ray (`smoothstep(1, 4, length)`, so a character's thin shadow does not light the air around it), and a shadow behind lit air (a shadowed wall seen from the sun) only 0.25 while the camera is in sunlight. So a shaded space with a lit opening gets bold beams and open daylight and the sky stay clear. A 4×4 depth-aware blur upsamples it and adds it to the frame in PostProcess before the rays. Its colour is the key light's (light set 0's colour A) pulled 35 % toward the fog colour, its strength follows that light's brightness (none below a luminance of 0.15, full from 0.6) and fades at the horizon. **The shafts follow the shadow light**, so they match the shadows on the ground: on Kerwan's first tunnel the real 35° key light does not reach inside, so the shafts are in the opening (cut by the arch and the hanging vines); a lower sun from the side (`RC_SUN_DIR=0.4,-0.5,0.75`) sends bold beams across the tunnel's air, as in the user's reference; and global illumination (`gi_render.rs`, `gi.wgsl`, 2026-10-24): one bounce of the lit colour between nearby surfaces (the world has no albedo, only baked light × texture), screen-space after the ambient occlusion and before the scene copy: quarter-resolution cells (normal and depth from the depth buffer, the lit colour averaged), 16 taps per cell on a golden-angle spiral of 3 units (denser near the centre) with a fixed per-cell rotation, each standing for its share of the disc as world area at the sender (larger far away and at a slant) and sending by the disc-to-point form factor (no visibility test), a 7 + 7-cell depth- and normal-aware blur, a depth-aware upsample, × 3 and the receiver's gain (0.5 / its lit luminance, at most 3), blended `dst · (1 + bounce)` so the bounce takes the receiver's colour; none on the sky, faded out by 150 units like the occlusion; water and effects are drawn after it. Checked 2026-10-24 on Novalis, Kerwan and Eudora (`RC_GI_DEBUG=1` shows the bounce alone): Ratchet's boots warm the grass at his feet, the grass greens his legs, crates and the ground at their feet trade light; Original and Simple byte-identical to the build before it; no fps cost measured (G-REN-080, G-REN-081). And the image post-process (`post_render.rs`, `post.wgsl`, 2026-10-24) on the 3D picture after the rays and before the screen tints, the AA blit and the UI pass (the HUD, menus, movies, the underwater tint and the fades untouched): a bloom (a soft threshold on the brightest linear channel, so saturated lamps count, the sky's pixels at a quarter, Karis-averaged into half size, five half-size levels down with the 13-tap filter and back up with a 3×3 tent, each wider level weighted 0.6 of the one above so the glow stays near its light; in the 8-bit frame a light is at most white, so the threshold sits at 0.7 and the strength is 2.5), a filmic grade in linear light (a gentle contrast power around mid grey and an exponential shoulder toward white, no hard clip; vibrance that lifts dull colours more than saturated ones) and contrast-adaptive sharpening (CAS), light; each part has an `RC_POST*` switch (docs/workflows/dev-switches.md). **The gate:** a new lighting effect (volumetric light, global illumination) runs only while `GraphicsSettings::lighting.enhanced()` (`Lighting::enhanced`; `Lighting::sun_shadows` / `game_shadows` pick the shadows), in the main world or the render world's extracted copy. Not part of the preset (a preset keeps it; the Preset row ignores it) |
| Water | Original / Enhanced | the level-code water as the game draws it | built (`water_render.rs`, `water.wgsl`): on clear FX textures (`clear_water`, by average colour; lava and sludge stay Original; the log names each texture's verdict) the surface passes (kinds 0 and 2) draw the whole water from the scene copy, opaque: travelling sine-wave normals, the bed bent by them (refraction) and absorbed over the thickness toward the texture's colour, caustics on the bed, a Fresnel screen-space reflection (fog colour where the ray leaves the screen), a sun glint and shore foam; the second layers (kinds 1 and 3) draw nothing. Port-only addition: a shallow sheet flowing down every flow chute (class 679's splines, `chute_water.rs`; kind 4: the slope's normal tilted by a height field carried at the chute's speed, white streaks along the flow; rows over strip water left out; a row stops at the chute's walls and at a step up, so it never spreads onto the floor a chute is sunk in, and rows on open ground are left out), which the game leaves bare. Original: the shader's Original branch returns what it did before; kind 4 draws nothing |
| Foliage | Original / Moderate / Lush | the level's own plants only | built (`foliage.rs`, `foliage/plan.rs`, `foliage.wgsl`): planned once per level on a worker thread from a seed of the level number (never the game's RNG): points spread over the LOD-0 tfrag triangles by area, kept where the triangle faces up (≤ 35°), its drawn colour (texel average × baked vertex colour, MODULATE) is opaque and green, a `coll_line` ray finds walkable ground within 0.3 units (not the barren surfaces: water, lava, sinking, slippery, pits, deadly), nothing overhead within 0.6 units, and no opaque non-green tfrag lies over it (paths). Each point is a short clump (twelve thin curved blades, ankle-high on Ratchet, root colour = the ground's, lighter tip), or with a small chance (0.3 % / 0.6 %) a fern (seven serrated arching fronds, darker green, 0.22..0.34 high) or a cluster of three white or yellow star flowers just over the blades; plus copies of the level's small swaying or green shrub classes in a ring around their own instances (random yaw, scale 0.75..1.15, ground checked the same way). Drawn instanced in 8-unit cells (per-cell frustum culling, hidden past the end distance), thinned with distance by a per-plant rank and shrunk smoothly before its cut-off; the shrub wind sway (mode 1 for grass, the class's own for copies), the tfrag fog, alpha-tested opaque and depth-writing (AO and Shadows Dynamic receive on it). Bend (visual only): Ratchet and the 47 drawn mobys nearest the camera (bounding sphere ≤ 3 units) push the plants away within a footprint from their bounding sphere, each along a short trail (a tail lagging behind it, 1.6 units at most for Ratchet, 0.8 for the others) so the plants spring back smoothly after it passes. Moderate: 6 clumps per square unit, 0.09..0.18 high, to 30 units; Lush: 14, 0.12..0.24 high, to 48 units. Original: no plan, no entities. Changing Moderate ↔ Lush re-plans in the background without a reload |

Enhanced: Anti-aliasing 4x (the best of 4x / 2x the GPU offers), Detail Far, Textures Smooth, HUD High resolution
(Original until High resolution exists), Ambient occlusion On, Water Enhanced, Particles Soft, Foliage Moderate. Neither preset touches
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
  `detail`, `draw_distance`, `ambient_occlusion`, `water`, `particles`, `foliage` (values: the lower-case names), `lighting`
  (`original`, `simple`, `enhanced`), `frame_smoothing` (`off`, `on`). `RC_GFX_ANTI_ALIASING`, `RC_MSAA=0|2|4|8`, `RC_GFX_HUD`, `RC_GFX_TEXTURES`,
  `RC_GFX_DETAIL`, `RC_GFX_DRAW_DISTANCE`, `RC_GFX_AMBIENT_OCCLUSION`, `RC_GFX_WATER`, `RC_GFX_PARTICLES`, `RC_GFX_FOLIAGE`, `RC_GFX_LIGHTING`, `RC_GFX_FRAME_SMOOTHING`,
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
* Ambient occlusion, Water, Particles, Dynamic shadows and Foliage were written from the standard techniques
  sine-wave normals, refraction, noise caustics, Fresnel with screen-space reflections, area-weighted scattering
  with ray-cast ground checks, instanced grass with distance thinning and trail bending); so was Lighting Enhanced's
  post-process (the soft-knee threshold and 13-tap / tent mip-chain bloom, a power contrast with an exponential
  shoulder, vibrance, AMD's contrast-adaptive sharpening). The feature list came from
  (Alchemy-style SSAO, cascaded shadow maps with a deferred receiver, radial-blur light shafts, ray-marched
  in-scattering through the shadow map with a Henyey-Greenstein phase, soft particles,
  rac-rs's README, and its look from a screenshot; none of its code (GPL-3.0) was read or used.
* First play test (Novalis, 8x MSAA): Ambient occlusion, Dynamic shadows and the sun rays did not compile (the
  naga_oil limits above), and the first Water Enhanced was too faint; all fixed since. Every pipeline now compiles
  with 8x and without multisampling. Not yet checked on screen since: the look of each option, and Original
  byte-identical to the earlier build (offscreen captures come back blank in a locked session).
* Second play test: Dynamic shadows darkened Ratchet and the whole ground evenly, with no visible shadow shapes. The
  shadow map was right (read back with `RC_SUN_DUMP`), but the Novalis key light is only 33° high, so the giant
  trees' canopies (ties) covered the spawn area. The sun now keeps the key light's bearing at 60° or more; checked
  with `RC_SUN_DEBUG` at the spawn (Ratchet, plants and tree bases cast onto lit ground). Lighting Simple keeps that
  raise; since 2026-10-24 Enhanced keeps the key light's own elevation down to 30° (`sun_shadow::ENHANCED_FLOOR`,
  `RC_SUN_FLOOR`), so its volumetric shafts slant in under arches: the canopies' longer shadows are accepted there.
* Lighting checked 2026-10-23 (Novalis, `RC_SCENE=0`, `RC_SCREENSHOT_FRAME=400`, against the build before it): at the
  spawn view Original is byte-identical to Shadows On, and Simple and Enhanced to Dynamic (the sun is off screen there,
  so no rays); on a fly view toward the sun (`RC_PLAY_FLY=1`, `RC_SUN_DIR`) Enhanced is byte-identical to Dynamic and
  Simple differs from it only by the rays (about a third of the pixels, the sky brighter toward the sun).
* Lighting Enhanced retuned 2026-10-24 (`RC_SCENE=0`, `RC_SCREENSHOT_FRAME=400`): Original and Simple byte-identical
  to the build before it (Novalis spawn, Kerwan's first tunnel with the real and a test sun). Enhanced: the key light
  kept (Novalis 32.8°, Kerwan 35°; Simple 60°); Kerwan's tunnel shows shafts in its opening, and bold beams across its
  air with `RC_SUN_DIR=0.4,-0.5,0.75` (like the user's reference); Kerwan's spawn and Novalis' spawn stay clear, the
  sky untouched. Novalis' spawn foreground is then all in a canopy's shadow (G-REN-068). `RC_NOVSYNC=1`, one PC:
  Novalis Simple 52 fps, Enhanced 47 (shafts off 47, floor 60° 46), Kerwan's tunnel 37 / 35.
* Light shafts reworked 2026-10-26 (third play test: a strong dither, too intense at some angles). The volumetric
  light's march starts at an interleaved-gradient offset moved by the golden ratio each frame; a temporal pass at half
  resolution reprojects the previous result by the sample's depth (by direction for the sky), drops taps on another
  surface, clamps it to the 3×3 neighbourhood and blends 10 % of the new frame in; the 4×4 bilateral upsample stays.
  Its phase is half isotropic, half Henyey-Greenstein (g 0.3), and what it adds is soft-capped (`l / (1 + Y(l) / 0.25)`).
  It reprojects per rendered frame from that frame's view, so frame smoothing's interpolated frames (its own branch)
  should hold; not checked on screen with it (G-REN-200). Gemlik's haze and Kerwan's
  tunnel beams are smooth; the tunnel opening keeps its shafts, softer.
* Sky-sun rays checked 2026-10-24 (`RC_SCENE=0`, `RC_PLAY_FLY=1`, `RC_SCREENSHOT_FRAME=240`, views facing the sky's
  sun, against the build before it): Veldin, Novalis and Eudora show shafts from the sun's point cut by the
  buildings, rocks and trees; Original and Simple byte-identical; `RC_SHADOWS=0` no rays. `RC_SUN_RAYS_LOG=1` prints
  each level's verdict (crate::sky_sun).
* Sun rays capped 2026-10-26 (third play test: too intense at some angles): facing Gemlik's red sun, which fills the
  sky, the mask covers most of the view and the rays laid an orange wash over it (mean |Δ| 22 against the rays off);
  what they add is now soft-capped, `l / (1 + Y(l) / 0.06)` (`sun_rays::KNEE`): 12.0, the sun's face no longer
  blown out; what is left is a warm tint under the knee over the whole view (the foreground 9.6, against 10.9
  uncapped), as the line to the sun crosses sky almost everywhere there. They carry no noise (a fixed radial blur), so no dither: only the faint radial streaks of the
  sun's own texture.
* Sun shadows from the sky's sun 2026-10-27 (fourth play test, Gemlik marker 7: the shadows ran across the red sun
  in view): where crate::sky_sun finds a painted sun disc (radius at most 15°, `sun_shadow::MAX_SKY_SUN_RADIUS`:
  Veldin 6.6°, Gemlik 11.3°), the shadows' light (`sun_shadow::follow`, and so `KeyLight::to_light`) takes its
  bearing, still raised to 60° (30° under Enhanced, `sun_shadow::min_elevation`); elsewhere the key light as before. The domes' broad glows (Novalis, Kerwan, Eudora,
  Pokitaru, Kalebo III, 20–30°) are left out: low under the clouds, and on Eudora 140° of bearing from the key light.
  Gemlik's key light (set 0 light A) comes from bearing 180° (game axes) at 20°, its sky sun from 114.5° at 14°: 65°
  apart; Veldin's from −65° at 20°, its sunset from 1° at 8°: 66°. Only the port's sun light moves; the game's baked
  vertex lighting keeps the key light. Checked with `RC_SUN_DEBUG=1` facing Gemlik's sun: the shadows now run toward
  the camera, away from the sun (before: across the view); Lighting Original byte-identical on Gemlik and Drek's fleet.
* Frame smoothing (2026-10-25): what is drawn between ticks, and what stays at the tick rate (the HUD and menus,
  shadows, the sky's spin and stars, seas and water, shrub sway, texture scrolls and animations, effect quads), is
  listed in `frame_smooth.rs`'s module doc; left: G-REN-150..152. A tick's move above 8 units or 45° (camera, moby),
  4 units (a particle), a moby's changed palette or colour mode, a joint turned over 45° or shown / hidden, a level
  change are discontinuities: drawn as the tick, not blended. It adds latency: a frame shows the tick before the last
  plus the elapsed fraction, so up to one tick (16.7 ms, ~8.3 ms on average) after Off. Evidence (Novalis, frame-exact):
  Off and On at 60 Hz byte-identical to the earlier build; at 120 Hz (`RC_DUMP_TICKS=1,0`) the same play trace as Off
  with every frame distinct (the camera half a tick between); a death reload's 120-unit camera cut drawn as the tick.
* The Port Options page (twenty-two rows) keeps the Camera sub-page's frame and scrolls, four rows at a time (port.rs `VISIBLE`).

## Draw distances

What ends the view in the game, and what the Draw distance row (factor s) does with it (units: game units, 1024
integer units each; depth is along the view axis):

| Kind | The game's | At s > 1 |
|---|---|---|
| Tfrag | `TfragProc`: culled when the sphere's near edge is past 512000 (0x160ec0, 500 units); no fade (pop) | 500·s (`tfrag_lod::DRAW_DISTANCE`); faded per pixel by depth over the last 16·s units (tfrag.wgsl `end_fade`) |
| Tie | per instance, capped at 720 (0x160fe0), culled when the centre's depth passes it; no fade. The Visibomb view caps it at 144 | the distance ×s (the Visibomb's cap kept); faded per instance by the centre's depth over the last 16·s units (`tie_lod` x bits 16..23, tie.wgsl) |
| Shrub | D = max(dd, 16), F + 24 for billboard classes, capped at 500 (0x1604a4); meshes fade over the last 8 units, billboards over the last 16 | D ×s, both fade bands ×s (shrub.wgsl, shrub_billboard.wgsl, `shrub_render::cull_shows`) |
| Moby | MobyProc: cap 500 (0x15fff0), 16-unit fade | not scaled: the result is game state (G-REN-050) |
| Particle | 32·m per record, capped at 500 (64 underwater), 16-unit fade | not scaled: `FLAG_DRAWN` is the simulation's (G-REN-051) |
| Far plane | 745472 (728 units) | 728·s on the main camera (`GameProjection::draw_scale`); the sky's camera keeps its own |
| Sea / liquid grids | no far cap in the port | end at the far plane (G-REN-052) |
| Fog | the level's (Novalis F 255 → 102 over 0..240 units, Kerwan → 153, Rilgar → 64, Pokitaru → 153) | unchanged |

* The factors: ×1.5 is the first step that puts everything the game's far plane could show (728 units) inside the tfrag
  distance (750); ×2 and ×4 cover the views across the largest levels from their edges (Rilgar's bounding sphere is
  608 units across). At ×1 no fade is added: Original keeps the game's pop.
* Fog: kept as the level sets it. Scaled with s (near and far ×s, tried with a temporary switch), it took the haze off
  everything in front of the camera too (15–59 % of the pixels changed at ×4 on Novalis and Kerwan, the island no
  longer blended into the sky backdrop): that is a different look, not a longer view. With the level's fog the
  geometry the row adds sits at the level's far intensity, as the game's farthest geometry did.
* Checked 2026-10-24 from a fly camera (`RC_PLAY=0`) on Novalis, Kerwan, Rilgar and Pokitaru, one view from a level's
  edge across it and one from outside: Original is byte-identical to the earlier build in all 8; Far fills in the
  far city blocks on Kerwan, the island's far side on Novalis and Rilgar's far buildings (0.01–5.3 % of the pixels);
  Maximum adds little to Far there (at most 0.06 %). Nothing that was not meant to be seen showed in these views.
* fps (`RC_NOVSYNC=1`, Novalis from its edge, 20 s): about 52 / 53 / 58 / 52 for Original / Far / Farther / Maximum,
  equal within the noise.

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
