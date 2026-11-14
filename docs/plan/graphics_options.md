# Graphics options (port-only)

Decided 2026-10-10 with the user. Code: `rc-engine/src/graphics.rs` (the settings), `rc-game/src/menus/pause/port.rs`
(the Port Options rows), `rc-engine/src/menu_render.rs` (`sync_graphics_rows` / `read_graphics_rows`).

## The rule

Every option controls exactly one thing, and its **first value is always Original**: what the PS2 showed on a TV.
The other values are modern alternatives (the Upscaler and Frame smoothing say Off for it). A **Preset** row sets them
all: Original (every option Original, the default), Low, Medium or High ("Presets" below); it shows Custom when the
options match no preset (never chosen with ✕), so changing any one option after a preset shows Custom. The preset is
read back from the options, and also stored (`preset`) so that an option added later, missing from an older file,
takes the stored preset's value. Options never change game state: the game logic keeps its own decisions (LOD choice,
the moby "drawn" flags, culling), the options only change what is drawn.

## Presets

2026-11-07 (the 0.6.0 beta, matching the Low / Medium / High / Custom row of rac-rs's README; none of its code read):
the presets replaced Original / Enhanced, and now set Lighting and Draw distance too (Enhanced had kept both). The
table is `GraphicsSettings::with_preset` (`graphics.rs`), one line per option; **a new option** adds its line there
(the struct literal has no `..`, so a missing line does not build), its key in `keys` / `from_file`, its `RC_GFX_*`
switch in `startup` and its row in `menu_render.rs`.

| Option | Original | Low | Medium | High |
|---|---|---|---|---|
| Anti-aliasing | Original | Off | 4x | 4x |
| Upscaler | Off | Balanced | Off | Off |
| Detail distance | Original | Original | Far | Farther |
| Draw distance | Original | Original | Far | Farther |
| Texture filtering | Original | Smooth | Smooth | Sharp |
| HUD | Original | Original | High resolution | High resolution |
| Ambient occlusion | Original | Original | On | On |
| Particles | Original | Soft | Soft | Enhanced |
| Lighting | Original | Original | Simple | Enhanced |
| Water | Original | Original | Enhanced | Enhanced |
| Foliage | Original | Original | Moderate | Lush |

Not in the presets (they depend on the screen): Fullscreen, Resolution, the aspects, FPS limit, Frame smoothing.
Medium is the old Enhanced with Simple lighting and Far draw distance; `preset = enhanced` and
`RC_GFX_PRESET=enhanced` read as Medium. 4x is clamped on a GPU without it (`render_settings`), the option keeps 4x, so
the preset is still recognised. Loading (`GraphicsSettings::from_file`): the stored preset's values (Original without
one, or for `custom`), each option key in the file over them; a hand-edited option therefore reads as Custom. Tests:
`graphics::tests::presets_round_trip_through_the_settings_file` (each preset and three Custom sets written with the
settings file's own writer and read back; unknown keys kept; saving twice is the same file),
`a_stored_preset_fills_the_options_the_file_lacks`, `presets_are_recognised_and_one_change_is_custom`.

## Rows (in this order on the page)

| Row | Values | Original means | Status |
|---|---|---|---|
| Preset | Original / Low / Medium / High / (Custom) | every row Original | done ("Presets" above) |
| Fullscreen, Resolution, Gameplay aspect, Cutscene aspect, FPS limit | | | exist (not part of the preset: they depend on the screen) |
| Frame smoothing (after FPS limit) | Off / On | Off: the game's 60 Hz ticks, each tick's picture shown until the next (pacing unchanged) | built (`frame_smooth.rs`, the pacing in `frame_pace.rs`): On, when frames come faster than the ticks (a display above 60 Hz with FPS limit 60 or 120, Unlimited, `RC_NOVSYNC=1`, `RC_FRAME_SMOOTH_HZ`), the limiter follows the display instead of 60 and each frame draws the last two ticks blended by the time since the last tick (`acc / tick`): the camera (lerp + slerp), every moby's instance record and joint palette (Ratchet, his attachments, statics, dynamics, extras) and the particles' positions. The game state never changes: only the render inputs (`GlobalTransform`, the shader buffers) are overwritten in `Last` and put back in `First`. Not part of the preset; at or below 60 Hz On draws as Off (byte-identical) |
| Anti-aliasing | Original / Off / 2x / 4x / 8x | the game's two full-screen bilinear copies, drawn at the frame's resolution: the AA blit A (`PutAABlitPacket_A` 0x223200, before the HUD: U = X across, 416 → 415 lines down) and the display copy B (`PutAABlitPacket_B` 0x223260, every frame: U = X across, 416 → 448 lines). Off = neither; 2x..8x = multisampling | done (`aa_blit.rs`, `aa_blit.wgsl`: four one-axis draws, each frame pixel reads the box of one game pixel a GS bilinear tap is; at 416 lines the vertical is the GS's result; checked against an independent reference at 416p, within 3 levels over four 8-bit draws). Off byte-identical to the earlier build |
| Upscaler (after Anti-aliasing) | Off / Ultra quality / Quality / Balanced / Performance | Off: the 3D drawn at the frame's resolution (the Resolution row) | built (`upscale.rs`, `upscale.wgsl`, 2026-11-07): the world cameras (sky, main) draw into an image of the frame ÷ 1.3 / 1.5 / 1.7 / 2 per axis (FSR 1's modes; `Upscaler::render_size`), and a compose camera (`Camera2d`, order 100) draws it into the frame with an FSR 1-style EASU (12 taps, edge direction and length from luma, an anisotropic Lanczos-2-like kernel, clamped to the 2×2 range) and RCAS (0.2 stops, `RC_UPSCALE_SHARPNESS=<stops>`, `RC_UPSCALE_RCAS=0`), both in gamma-2, then the UI pass (HUD, menus, movies: their nodes and the default UI camera move to it) and the after-HUD passes (missile overlay, underwater tint, fade: moved from the main camera's render entity, `move_after_hud`; the fade moved to `gs_post` so it can run in Core2d), so the HUD and menus stay at the frame's resolution. Everything on the world camera (AA blit, Lighting Enhanced's post, the mirror flip, the tints) runs on the smaller image. The menu snapshot copies the frame. EASU and RCAS follow AMD's FSR 1.0 (`ffx_fsr1.h`, MIT licence, © 2021 AMD), rewritten in WGSL. Off: nothing changes (no compose camera active, the cameras keep the frame). Results: "Upscaler checks" below |
| Detail distance | Original / Far / Farther / Maximum | the game's LOD distances (×1, ×2, ×4; Maximum ×1000, past every draw distance) | done: tfrag (`trunc(D·1024)` thresholds and the morph constants), tie (class near/mid/far), moby (`lod_trans << 10`, `moby_lod::set_lod_scale`; drawing only: every joint is evaluated whatever LOD is drawn), shrub mesh/billboard (F' = F × scale; within 24 units of D the class draws as one without a billboard: `shrub_render::detail_fade`, shrub.wgsl, shrub_billboard.wgsl; the factor rides in `tex_mode.y`). Draw distances are the Draw distance row's. Original byte-identical to the earlier build |
| Draw distance | Original / Far / Farther / Maximum | the game's draw distances (×1, ×1.5, ×2, ×4; see "Draw distances" below) | done: tfrag (`tfrag_lod::DRAW_DISTANCE`), tie (per-instance distance), shrub (D and both fade bands), the main camera's far plane (`GameProjection::draw_scale`); above ×1 tfrags and ties fade out over the last 16·s units (4×4 ordered dither, tfrag.wgsl / tie.wgsl); the factor rides in `tex_mode.z`. Mobys, particles, the sky and the level fog are not scaled. Not part of the preset yet. Original byte-identical to the earlier build (8 views on Novalis, Kerwan, Rilgar, Pokitaru, 2026-10-24) |
| Texture filtering | Original / Smooth / Sharp | the GS mip rule: one level from depth, `round(log2(z/32)+K)`; Smooth = the same level unrounded, two levels blended; Sharp = the GPU's footprint level + anisotropic | done (tfrag, tie, shrub, shrub billboard: `sample_world`; the mode rides in the shared fog buffer's third vec4 `tex_mode`; samplers linear between levels + anisotropy 16, an integer level reads one level: Original verified byte-identical to the earlier build on a Novalis walk) |
| HUD | Original / Sharp pixels / High resolution | the 512×416 screen through the display copy B (half a pixel across, like the world) and scaled up with bilinear filtering, as the TV and an emulator's display showed it; Sharp pixels = nearest (the port's earlier look); High resolution = drawn at the frame's resolution | done (2026-11-07, "Sharp HUD" below): High resolution draws the HUD, text and menus into a layer the frame's own size (`hud_render::layer_size`, `LayerScale`), each primitive at the game's place and scissor, so edges, fades and rotated or scaled sprites are drawn at the frame's pixels instead of a 512×416 image stretched; textures drawn 1:1 or larger are magnified bicubic (`hud.wgsl` flag 16), held in the four nearest texels' range so no halo; the static layer (noise, scan lines) stays at 416 lines |
| Ambient occlusion | Original / On | the baked vertex colours only | built (`ao_render.rs`, `ao.wgsl`): a spiral SSAO from the depth buffer after the opaque pass, a depth-aware 4×4 blur, multiplied into the frame before the transparent pass; faded out by 150 units; the mobys' own pixels are left unoccluded (2026-10-31: each moby fragment keeps its depth in a per-pixel mask, `moby.wgsl`, and the pass skips a pixel whose depth is that one; mobys with a bounding sphere over 3 units are occluded like the world). It replaced the mobys' world boxes (2026-10-30), which lifted the occlusion off the grass and plants inside them, rectangles around Ratchet, Clank and plant mobys (Orxon, session 2026-10-31_09-35-03), and before them the half-sphere in front of the bounding sphere's centre, which shaded a crate's far faces as the camera turned (Orxon marker 4 of session 2026-10-30_15-54-38). Off: the pass is not added (no `AoTarget` on the view) |
| Particles | Original / Soft / Enhanced | sprites cut where they cross the world; effects light nothing but through the game's own light bank | built (`particle_render.rs`, `particle.wgsl`): the blend alpha fades over 1.5 units in front of the depth copy. Original: fade distance 0, the factor is 1. Enhanced (`particle_lights.rs`, `particle_lights.wgsl`): Soft, and the bright effects (additive and overbright particles, the explosion flashes and fireballs) become up to 32 point lights, faded in and out at the cap, applied to the opaque world by a full-screen pass from the depth buffer. The veils are never lights (`particle_lights::NON_EMISSIVE`: the weather, splashes, rings, drops and bubbles, Novalis's waterfall mist 56 and foam rings 57, the smoke, steam and dust puffs, goo drips, decals; 2026-10-28, Novalis marker 1 of the fifth play test: the 127 additive foam rings at the foot of the fall made five saturated lights on the river). A light keeps its identity while sources keep coming within its reach and follows them through exponential filters (intensity up 0.1 s, down 0.4 s, centre 0.05 s), and the ones shown count 1.5× against the others at the cap, so a fountain of random sparks or a glow the game redraws every tick holds a steady level (2026-10-28, Orxon marker 3: the type-2 goo blobs under the floor grates and the spark fountains 1240 flickered when each frame's random sparks were re-clustered and each light took that frame's power). Enhanced also restyles the atmosphere (`Particles::atmosphere`; meant for Lighting Enhanced once that option lands, G-REN-240): the sky stars (`sky_stars.rs`, `sky_stars.wgsl` `STAR_GLOW`: the title world's and the levels' star records) glow with a soft halo sized by brightness, twinkle smoothly per star (the game's per-frame colour noise of kinds 1 / 3 replaced by its mean and a hashed slow shimmer) with a slight warm / cool tint; the space flight's painted stars (`flight_render.rs`, `sky.wgsl` `SKY_STAR_GLOW`) get a bright-pass halo and twinkle; the rain and snow of levels 08, 12 and 14 (`weather_render.rs`, `weather.wgsl`: the game's type-0 and type-73 records, so its intensity, area and roof culling) are drawn as thin motion-blurred streaks tinted by the fog colour and soft round fluttering flakes with a depth-of-field blur, in one instanced draw; each flake record also draws three more flakes at hashed offsets of up to 2.2 units across and 2.6 up or down, sized for their own depth, 0.6..0.9 of the size and 0.85 of the opacity (`weather_render::FLAKE_COPIES`, `weather.wgsl` `EXTRA_SPREAD`), so the snow is about four times as thick but only where and while the game's own flakes fall (2026-10-27, Hoven marker 4 of the fourth play test). Not in the Enhanced preset (G-REN-142) |
| Lighting | Original / Simple / Enhanced | the game's shadows, cast by characters and enemies (the classes with a shadow block, `shadow_render.rs`) | done (2026-10-23; it replaced the Shadows row On / Off / Dynamic: Original is the old On, Simple the old Dynamic without the rays, Enhanced the old Dynamic). Simple (`sun_shadow.rs`, `sun_shadow.wgsl`): in place of the game's shadows, a sun light along the level's key light (`ShadowGame::sun_light`: light set 0's direction A, or its light B where A lights from below and B is the brighter, as on Drek's fleet (`shadow_render::key_light`), fixed for the level, raised on the same bearing to at least 60° under Simple; under Enhanced it keeps its own elevation down to a 30° floor, `sun_shadow::min_elevation`, `ENHANCED_FLOOR`, `RC_SUN_FLOOR=<degrees>`, 2026-10-24: Novalis 32.8° and Kerwan 35° are kept) with a cascaded shadow map (3 cascades to 150 units) that every tfrag (its LOD-0 list), tie (LOD 0 unmorphed, the ones culled for the camera too: `tie_lod` hides none while the sun casts), shrub (cut out by its texture's alpha) and moby casts into through its shader's `PREPASS_PIPELINE` branch (the GS alpha test kept, non-Z-writing passes skipped), received by a full-screen pass over the depth buffer (3×3 PCF, multiplied in before the transparent pass). Enhanced: Simple's shadows and every advanced lighting effect: the sun's rays (`sun_rays.rs`, `sun_rays.wgsl`, `sky_sun.rs`; 2026-10-24) from the sun the sky paints, not the shadows' key light: at level load the sky shells are drawn into a 1° latitude–longitude luminance map (each shell at its first tick's SkyM) and the brightest compact spot at least 3° up in one shell's layer, bright and above its row's median and not hidden by the shells drawn after it, is the sun (Veldin's sunset disc, the bright patch of the Novalis / Eudora / Kerwan / Pokitaru / Kalebo III domes, Gemlik's red sun); it turns with its shell. The sky's pixels (depth 0) around its screen point (a falloff of twice the spot's radius, 0.08–0.3 of the screen height), keyed on their luminance between the sky's median and the sun's peak, are blurred radially at half resolution (40 taps) and added in PostProcess before the AA blit, at a strength from the sun's contrast (0.5–1); skipped with the sun behind the camera, faded at the screen edge and the horizon; none on a sky without a sun (night, space, storms: 12 of the 19 levels) or under `RC_SHADOWS=0`; and its volumetric light (2026-10-24, `volumetric_light.rs`, `volumetric_light.wgsl`), shafts in the air with the sun on or off screen: at half resolution each pixel's view ray is marched from the camera to the depth-buffer surface (at most 48 units, 12 toward the sky) in 24 steps packed toward the camera from an interleaved-gradient-noise start, each step testing the sun's cascades (one comparison tap; 2026-10-30: Ratchet's own shadow does not shade the air: where the light reaches a step through his bounding sphere and the shadow map's occluder lies within it, `VolumetricLight::hero`, so looking down along the sun his shadow column no longer bounds a beam that seems to come from his body, Gaspar marker 3 of play session 2026-10-30_15-54-38; other mobys still cast into the air); lit steps add a uniform thin medium's in-scattering (σ 0.02 per unit, extinction along the ray, brightness 2.0) weighted by a Henyey-Greenstein phase (g 0.3, mildly forward). **Shadow-contrast weighting** (2026-10-24, `volumetric_light::Shafts`, mirrored in the shader): each lit step counts in full within about 4 units along the ray of a long shadow bounding its lit run (`1 − (1 − a)(1 − b)`, `a`, `b` = `exp(−d/4)` to the run's ends), and 0.05 in lit air far from shadow; a bounding shadow counts by its length along the ray (`smoothstep(1, 4, length)`, so a character's thin shadow does not light the air around it), and a shadow behind lit air (a shadowed wall seen from the sun) only 0.25 while the camera is in sunlight. So a shaded space with a lit opening gets bold beams and open daylight and the sky stay clear. A 4×4 depth-aware blur upsamples it and adds it to the frame in PostProcess before the rays. Its colour is the key light's (light set 0's colour A) pulled 35 % toward the fog colour, its strength follows that light's brightness (none below a luminance of 0.15, full from 0.6) and fades at the horizon. **The shafts follow the shadow light**, so they match the shadows on the ground: on Kerwan's first tunnel the real 35° key light does not reach inside, so the shafts are in the opening (cut by the arch and the hanging vines); a lower sun from the side (`RC_SUN_DIR=0.4,-0.5,0.75`) sends bold beams across the tunnel's air, as in the user's reference; and global illumination (`gi_render.rs`, `gi.wgsl`, 2026-10-24): one bounce of the lit colour between nearby surfaces (the world has no albedo, only baked light × texture), screen-space after the ambient occlusion and before the scene copy: quarter-resolution cells (normal and depth from the depth buffer, the lit colour averaged), 16 taps per cell on a golden-angle spiral of 3 units (denser near the centre) with a fixed per-cell rotation, each standing for its share of the disc as world area at the sender (larger far away and at a slant) and sending by the disc-to-point form factor (no visibility test), a 7 + 7-cell depth- and normal-aware blur, a depth-aware upsample, × 3 and the receiver's gain (0.5 / its lit luminance, at most 3), blended `dst · (1 + bounce)` so the bounce takes the receiver's colour; none on the sky, faded out by 150 units like the occlusion; water and effects are drawn after it. Checked 2026-10-24 on Novalis, Kerwan and Eudora (`RC_GI_DEBUG=1` shows the bounce alone): Ratchet's boots warm the grass at his feet, the grass greens his legs, crates and the ground at their feet trade light; Original and Simple byte-identical to the build before it; no fps cost measured (G-REN-080, G-REN-081). And the image post-process (`post_render.rs`, `post.wgsl`, 2026-10-24) on the 3D picture after the rays and before the screen tints, the AA blit and the UI pass (the HUD, menus, movies, the underwater tint and the fades untouched): a bloom (a soft threshold on the brightest linear channel, so saturated lamps count, the sky's pixels at a quarter, Karis-averaged into half size, five half-size levels down with the 13-tap filter and back up with a 3×3 tent, each wider level weighted 0.6 of the one above so the glow stays near its light; in the 8-bit frame a light is at most white, so the threshold sits at 0.7 and the strength is 2.5), a filmic grade in linear light (a gentle contrast power around mid grey and an exponential shoulder toward white, no hard clip; vibrance that lifts dull colours more than saturated ones) and contrast-adaptive sharpening (CAS), light; each part has an `RC_POST*` switch (docs/workflows/dev-switches.md). **The gate:** a new lighting effect (volumetric light, global illumination) runs only while `GraphicsSettings::lighting.enhanced()` (`Lighting::enhanced`; `Lighting::sun_shadows` / `game_shadows` pick the shadows), in the main world or the render world's extracted copy. Not part of the preset (a preset keeps it; the Preset row ignores it) |
| Water | Original / Enhanced | the level-code water as the game draws it | built (`water_render.rs`, `water.wgsl`): on clear FX textures (`clear_water`, by average colour; lava and sludge are not water, their looks below; the log names each texture's verdict) the surface passes (kinds 0 and 2) draw the whole water from the scene copy, opaque: travelling sine-wave normals, the bed bent by them (refraction) and absorbed over the thickness toward the texture's colour, caustics on the bed, a Fresnel screen-space reflection (fog colour where the ray leaves the screen), a sun glint and shore foam; the second layers (kinds 1 and 3) draw nothing. Quartu's and the Fleet's water (2026-11-03, session 2026-11-03_13-24-36 marker 2: Quartu's tanks drew the plain water) is not the water module's but the wave meshes' (1408, 1409, 1565, 1567 and 1405..1407, 1426, `units::wave_mesh`): their draw callbacks hand crate::fx_draw two passes of quads (FX 0x2d at alpha 0x40, ST counter-scrolled) over a floor where the level draws the room upside down as its reflection, so no water draw existed for Enhanced to take; FX 0x2d is clear water, so each strip is now also a kind-0 draw (`water_render::WaveSheet`: built once at the load from the moby's place, at rest, z blend 1, no scroll or wobble; its tint at vertex grey 0x80, twice the moby's ambient, as the game draws the strips twice: at 0x40 the deep water came out at half the Original's blue), shown on Enhanced only, when crate::fx_draw leaves the quads out (`takes_wave_meshes`), and in the planar reflection's flat regions; a tank (`params.misc.w` = −1) has no shore foam (its walls stand straight in the water, and the thin water against them drew a dotted white rim). Original pixel-identical at the marker. Veldin 2's Hydrodisplacer pools (2026-11-07, session 2026-11-07_13-10-24 marker 1: the pools drew the plain water) are the same case: class 1402's three meshes (`units::veldin_pool`, FX 0x2b quads, colour 0x50 grey, s scrolled) through crate::fx_draw, raised and lowered with the pool as the Hydrodisplacer drains and fills it; each mesh is now also a kind-0 tank draw (`water_render::PoolSheet`: z1 one unit under the mesh, so the frame record's z blend `1 + dz` follows the pool's height, its s scroll there too; tint 0x50 grey, the quads' colour drawn once), shown on Enhanced while a pool drew that mesh this tick, when crate::fx_draw leaves the quads out (`takes_veldin_pools`), and in the planar reflection's flat regions at the pool's height. Every Enhanced water and liquid surface by level, with its builder and tuning points: docs/plan/water_surfaces.md. Port-only addition: a shallow sheet flowing down every flow chute (class 679's splines, `chute_water.rs`; kind 4: the slope's normal tilted by a height field carried at the chute's speed, white streaks along the flow; rows over strip water left out; a row stops at the chute's walls and at a step up, so it never spreads onto the floor a chute is sunk in, and rows on open ground are left out), which the game leaves bare; only on levels with clear water of their own (2026-10-28, `CHUTE_FX`: Rilgar, Batalia; Quartu has none of the water module's (its tanks are the wave meshes above) and its one chute, from the factory door down to the cave floor, is a dry metal slide, so it stays bare). The seas (`sea_water.rs`, 2026-10-23): the opaque draws of the liquid-grid ports, 854's grid set and 1901 whose image is clear water (`clear_rgba`; on Rilgar 879 and Batalia 327 it is, Umbris's, Aridia's and Hoven's liquids are not water) are drawn by the same shader (kind 0) after the scene copy instead of in the opaque pass, first in Transparent3d and writing Z (the casters, translucent parts, effects and particles drawn later are cut by the surface as before); deep water absorbs toward 0.85 of the water's colour (`DEEP`; the rivers' 0.55 made the deep seas far darker than their opaque surface); from 40 to 110 units of view depth (`FAR_FADE`) the bed, the shore and the foam fade into the water's own colour (the depth behind distant water is thin and the refracted bed flickered), so the far sea is the Original surface under the reflection; a camera under the surface keeps the Original sea; the effect layers stay Original (G-REN-039). The verdict is on the image × the grid's vertex colours (`sea_render::judged_rgb`, logged), and a grid's own translucent draw (FIX < 0x61) qualifies like an opaque one (`sea_water::surface_fix`, 2026-10-24): drawn see-through, the bed keeping 1 − FIX/128 of its light over one unit of water as through the original's blend (`clarity`), with no far fade; none does today (Kerwan's 994 is a grey cloud floor under the city, Oltanis's 1260 mud), and Kerwan's fountains are not a grid (G-REN-115). Rilgar, release build, `RC_NOVSYNC=1` over the bay: Original ~72 fps, Enhanced ~63 (a loaded machine). The planar reflection (`water_mirror.rs`, 2026-10-24): of the flat Enhanced surfaces below the eye (the clear strips and ripple patches, the seas' grids block by block, Pokitaru's ocean at its sea z over the band's reach, 2026-10-28: `OceanWater::plane`; before it the ocean had the screen-space reflection only, which missed the islands and the tower at grazing views; the world camera's output replaces the target, `water_mirror::place`: from 2026-10-20's world alpha, `gs_state::WORLD_ALPHA`, Bevy's default alpha blend for a target's second camera had dropped the tfrags, ties and sky from every mirror; a plane of strips up to 0.5 apart, Novalis's river at 38.7..39.5, is drawn at the height of the strip scoring most and held at the nearest strip between choices, `water_mirror::nearest_plane`: the first strip within 0.5 had jumped it between 39.0 and 39.5 three frames in four, a flicker; 2026-10-29: the kept plane stays at the drawn strip's height, and a step over 0.1 in a frame fades, `water_mirror::FOLLOW`: the main channel's overlapping strips at 38.73, 38.99, 39.13 and 39.35 had traded the best place as the camera moved and the plane cut between them, 87 cuts on Novalis's `RC_WATER_MIRROR_SWEEP=90`, none after; the sweep of every level finds no other) whose rectangle, clipped by the view frustum, is partly in view, the height whose parts in view cover the most of the screen, each part's screen area times the share of it seen through the collision mesh (`choose_plane`, `seen_share`, every 4 frames; the drawn one stays until another covers 1.5 times as much; a change fades out and in over 0.2 s, `fade`, 2026-10-26), is mirrored: two cameras draw the sky, the tfrags, the ties and the mobys (Ratchet too) reflected about it into a half-size target, with an oblique near plane on the water (`game_camera::oblique_near`); on pixels within 0.5..1.0 of that height the water reads it, bent by the waves, in place of the screen-space reflection (the same Fresnel; the screen-space one elsewhere and where the bent sample leaves the target); shrubs, particles, effects, the sky's stars and the water itself are not mirrored, nor what occlusion and the moby culls hid for the main camera (G-REN-110); the ties and tfrags the main camera's frustum or distance tests cull are tested again reflected about the plane and drawn when that passes (2026-10-27, `tie_lod::mirrored`, `tfrag_lod`; G-REN-222), so the static world stays in the reflection under a camera pitched down (Rilgar's bay: about 170 ties and 10 tfrags more, the town's windows), the main view unchanged (they lie outside its frustum); one plane per frame, other heights keep the screen-space reflection (G-REN-111, G-REN-210); no flat Enhanced water on screen: the cameras are inactive. Rilgar's bay, dev build, `RC_NOVSYNC=1`, one batch (a loaded machine): Enhanced ~53 fps with the mirror, ~61 without (`RC_WATER_MIRROR=0`), Original ~59: about 2 ms a frame. After the frustum choice (same camera, dev build, `RC_NOVSYNC=1`, a loaded machine): Enhanced ~53 fps, Original ~62..70; Kerwan's start, where nothing is Enhanced and the mirror stays off: Original ~73, Enhanced ~68 (the frames are identical). `RC_WATER_MIRROR=0|debug` (dev-switches.md). Original: the shader's Original branch returns what it did before; kind 4 draws nothing; the seas' Original frames are pixel-identical to the earlier build. The swell (`water_waves.rs`, 2026-10-24): six travelling sine waves (λ 23, 15.3, 10.7, 8.3, 6.9, 6.05 units in six directions, so no regular lattice; 0.43 high at most; 2026-11-01: raised from four waves 0.3 high), drawn only (collision, swimming and floats keep the game's water height). The grid seas (Rilgar, Batalia) are split on the CPU within 108 units of the camera (2-unit spacing, one count per draw so edges stay shared) and displaced in the vertex shader; the Pokitaru ocean 1111 has its own surface (below); the strip and ripple water (Novalis) takes the slope only. Normals, reflection, Fresnel, refraction and caustics follow the analytic slope; crests are a little lighter. Shore damping: the level collision probed on a 5-unit lattice (land or a pier above the water: calm; the bed from 1 unit under the surface, past the swim surface Rilgar holds 0.31 under it), the shallowest depth within 10 units through 0.5 → 8 units; the strips damp per pixel by the bed's depth in the scene copy. Far fade: the height from 50 to 100 units, each wave's slope from 8 to 16 wavelengths. `RC_WATER_WAVES=0` / `RC_WATER_WAVES_SCALE`. Rilgar low over the bay, `RC_NOVSYNC=1`: Original ~68 fps, Enhanced ~60, Enhanced without the swell ~69 (G-REN-121). Original stays pixel-identical (Rilgar, Novalis), and with the mirror no camera is spawned in Original. The ripple waves of the normal fade out from 30 to 60 wavelengths from the camera (`RIPPLE_LOD`: the short ones sparkled into noise in the distance). Pokitaru's ocean (`ocean_water.rs`, 2026-10-25): the effect-only ocean 1111 (a mixed band and two additive layers over the bed) gets a surface of its own from above, in place of the band and the layers (the wall stays): 64-unit world tiles made once and kept (2-unit split within 108 units, 16 beyond, out to the band's 600), re-joined when the set changes, displaced in the vertex shader (the swell ×1.0) and lifted by the game's bob through the frame record; the swell's CPU split per tick is gone (G-REN-180). Its look (2026-10-25, `water.wgsl`'s `OCEAN_*`, `params.misc.w` = 1): one colour, the Original's open turquoise (`OPEN`, sampled from Original frames; the band's colour mixed toward a deep blue had gone dark teal); the only bed the art draws is the sand shelves (some 30 units down) and the islands' skirts, the clear colour between them reading as deep water; a light absorption, red first, over the bed's vertical depth plus 0.3 of the slant run, 0.35 of the water's colour scattered in at the surface and at least (0.12, 0.35, 0.35) of the bed always through (the Original's band shows it at a quarter), so the shelves, rocks and the boats' floats read through the water to 120 → 300 units; a reflection that meets nothing takes the sky in the scene copy along the ray, else a sky blue (the fog colour the grid seas use is Pokitaru's navy, which darkened the whole sea); reflection and glint on 0.4 of the slope, a hit tolerance of 1 step (4 tore the islands' reflection into rings), Fresnel 0.03 + (1 − cos)⁵ capped at 0.5; a gentler refraction bend (7 px), caustics faded out from 25 to 60 units, 0.6 of the ripple normal, crests 0.08, a smaller, softer glint, a faint foam line. Pokitaru over the islands, `RC_NOVSYNC=1`: Original ~82 fps, Enhanced ~84 (the mesh's 318 tiles are made once in ~11 ms). The other liquids (`liquid_render.rs`, `liquid.wgsl`, `liquid_look.wgsl`, 2026-10-24): each liquid-port surface gets a kind from its judged colour (`liquid_render::classify`: water as before, lava, mud, sludge, else Original; no colour is ice: Hoven's 1901 is ice by its port, below; its snow banks are level geometry, G-REN-130) and lava, mud and sludge are drawn opaque, Z written, from either side, keeping the draw's own colour: lava (Gaspar 317 and its flows, Umbris 460) a dark crust with bright orange cracks along two noise layers in the draw's ST, on a grid on the world plane at its ST scale (2026-10-27, `st_per_unit`: the ST restarts per 7 × 7-cell block, so in ST the pattern repeated per block and broke at its borders) (they ride the scroll; on Gaspar's two-pass flows the second layer drifts at the second pass's relative speed, that pass not drawn) and a glow around them, plus a finer layer in world space (2026-10-26, `lava_fine`: thin cracks and a crust grain, triplanar on slopes, carried along the scroll by a two-phase flow map from the ST's screen derivatives, faded out with the pixel's footprint; the crack edges anti-aliased by that footprint); mud (2026-10-28, Aridia marker 2 of the fifth play test: the quicksand looked flat and dull; Aridia's quicksand 854 by its port, `liquid_render::quicksand_kind`: its judged colour 25, 23, 15 is too dark for the colour rules; it is the level's collision surface 3, the sinking liquid, which no other level has) thick and slow, keeping the game's animated texture and colour: a slow swell in the normals only (slopes of about a tenth over 3..22 units, no waves, so it still shows 20..60 units away), bubbles in a little under half of the 5-unit cells, 0.45..0.9 units, that swell glossy for 6..10 s, pop and leave a settling ring (faded out by the pixel's footprint), dark glossy wet patches and a lighter, matte dry crust with a fine grain along the banks, finely cracked there (`liquid_render::shore_map`: at the level load the collision faces within 0.3 below to 4 above the quicksand's height that are not quicksand are its banks, rasterised at one texel a unit, fading over 3 units), shaded by the key light against the flat surface's light, a wet sheen bent by the swell: the sky (the fog colour, brightest at its horizon, so the swell shows as glossy bands) by Fresnel and the key light's sharp highlight; the wet patches' darkening pays for most of it (2026-10-28, later: the first, matte version was invisible at the marker; now, at marker 2, the mud's mean luma 23.2 → 27.8 and the frame's 26.7 → 28.1); the level's point lights are not in the highlight (G-REN-224); drawn in the opaque pass like the other looks, so the quicksand keeps its ambient occlusion and the shadows cast on it (the game's and the sun's, applied before the transparent pass): a screen-space reflection would need the draw after the scene copy (as the clear seas) and lose them; sludge (Umbris's swamp 1018, a dark olive) murky with a faint glow, slow ripples and bubbles. The ripple patches that are not clear water take sludge only (`liquid_render::patch_verdict`: Gemlik's acid pools, FX 46/45; its orange FX 54/53 stay Original, G-REN-132) in `water.wgsl` (`look.misc.x` = 2). Hoven's 1901 (2026-10-27, `hoven_ice`, G-REN-220) is ice: 2026-11-02 (the user: icy water that freezes Ratchet at once; a reference photo of broken pack ice; the Original's low-resolution texture must not show), no texture is drawn, only its mean (`liquid.wgsl`, both opaque groups on FX 0x2f's so they meet without a seam) and the game's added pass (the white streaks) is left out; the open water keeps the Original's teal pushed toward its emerald with a moving caustic web (two warped ridge layers flowing different ways, `ice_caustics`), a fine ripple, a sky sheen and a glint (pack-ice plates on it, after a reference photo, were tried and dropped; before: every layer drifting at 5 % of the game's scroll, the texture kept). A translucent grid (FIX below 0x80) is a fog or cloud layer and stays Original (2026-10-27, `layer_verdict`, G-REN-223: Oltanis's 1260, the brown haze at the bottom of the world). Kerwan's cloud floor 994 stays Original; none of the new looks is mirrored. Orxon's goo (2026-11-02, `liquid_render::TFRAG_LIQUIDS`: tfrag texture 12, a still texture the game never animates, under the whole level) is drawn by the tfrag pass with its own look on (`tfrag.wgsl` `goo_look`, the batch flagged in its LOD uniform's free lanes so the material layout and Original stay byte-identical): the texture is not drawn but its look is redrawn procedurally on the world plane around its mean colour (`liquid_render::pack_mean`) under the vertex colours, and made alive: olive-brown mottling that breathes, "eyes" (1.4-unit cells, most filled, yellow-orange centres with a thick dark rim and a turning neon crescent) that wander, swell over 3..6.5 s and pop into a neon ring and a ring wave, broken neon squiggles that drift, all carried by an uneven noise swell (three octaves, about 6, 2.5 and 1.1 units, drifting three ways: no regular crests) that shades it under a fixed light (Orxon is night) with a faint Fresnel sheen of the fog colour and a highlight on its high ground; the details widen to the pixel's footprint and fade only under a few pixels (eyes from 0.25 units a pixel, squiggles from 0.4: at first they faded from 0.05, flat a short way out from the landing pad) (2026-11-02, the user: match the Original's look and feel, made dynamic); `RC_LIQUID_FX=0` or a forced kind other than sludge keep it Original. The pattern is drawn on the texture's own ST mapped back to world units by each triangle's map (`tfrag_render::liquid_st_map`, a vertex attribute read only by the look's own pipeline variant `TFRAG_LIQUID`, `TfragKey`), not on the world position, which jumps across the goo mesh's overlapping pieces and drew thin lines of a shifted pattern (marker 1 of session 2026-11-02_12-34-52). Quartu's green pools (2026-11-06, tfrag texture 10, flat at z 28.97 and 32.85: the giant Clank arena and the yard round the factory dome; session 2026-11-06_10-48-49 markers 1 and 2) take a second look, `swirl_look` (`TfragLook::Swirl`, `lod.tex.w` = 2): the game's texture and vertex colours kept, carried by a slow curl-noise flow on the plane (eddies of about 8 and 3 units; two copies half a 7-second cycle apart, so it swirls without stretching), the texture laid on the world plane at one scale (`SWIRL_TILE`, 2.5 units a tile: the game's maps turn and shear it triangle by triangle, and moving it through each map drew seams, session 2026-11-06_15-02-26 marker 6); the Fleet's same green liquid (tfrag texture 57, under the laser grid, marker 5) takes it too, a finer copy of the texture flowing the other way for detail under its texels, a gentle swell shading it with a sheen of the fog colour (the user: the Original's look and feel, swirling and alive). Gaspar, dev build, `RC_NOVSYNC=1`, a loaded machine: Original ~63 fps, Enhanced ~68 (noise; no measurable cost). `RC_LIQUID_FX=0|RC_LIQUID_FX_KIND` (dev-switches.md). Original: byte-identical captures to the earlier build on Novalis, Aridia, Kerwan, Rilgar, Umbris, Batalia, Gaspar, Hoven, Gemlik and Oltanis. **Under water** (`underwater_render.rs`, `underwater.wgsl`): while the game's underwater flag is set, one full-screen pass on the 3D picture (PostProcess, after the sun rays; the game's fog swap and its tint sprite after the HUD stay, in the water's colour where its Enhanced surface has one, `underwater_render::SURFACE_COLOURS`: Pokitaru's ocean turquoise, the sprite at `TINT_SHARE` of its alpha, 2026-10-26; elsewhere the game's alternate fog colour): the in-water part of each view ray (cut at the water plane: a camera at the surface gets the waterline split) absorbed per channel (red first) toward the alternate fog colour, darker deeper (the in-scatter lit at the depth of the ray's first 30 units, `SCATTER_REACH`, at least 0.4 of the surface's light, dimmed by up to 30 % looking straight down, 2026-10-27: a far floor and the open sea, where no geometry is, end on the same colour, so Pokitaru's deep trench no longer turns near black behind a hard edge); the light on surfaces fading with their depth below the plane, half the water's colour filling in what a deep floor loses (`FLOOR_FILL`); two warped caustic layers projected along the sun bent into the water (Snell), on up-facing surfaces only (2026-11-06: the surfaces facing away get a softer triplanar pattern at `WALL_CAUSTICS` of the strength, so a flooded tube's far side is not bare, session 2026-11-06_15-02-26 marker 3), each faded once its cells shrink below a few pixels; 16-tap light shafts lit by a coarse layer of the same pattern; a gentle wobble; all fading in and out over 0.35 s when the flag flips. The plane: the first water face above the camera (the underwater test's line query), else a ripple patch above, else the last one found. Checked on Novalis, Rilgar and Gemlik (G-REN-100..103); Original never runs the pass |
| Foliage | Original / Moderate / Lush | the level's own plants only | built (`foliage.rs`, `foliage/plan.rs`, `foliage.wgsl`): planned once per level on a worker thread from a seed of the level number (never the game's RNG): points spread over the LOD-0 tfrag triangles and the ties' up-facing LOD-0 faces (2026-10-29: Batalia's turf-topped tie pads were bare; `foliage::build_input`) by area, none on the space stations (Blarg 6, Gemlik 13, Drek's Fleet 17: `plan::NO_FOLIAGE`) nor on the tfrag and tie textures `plan::NOT_TURF` lists (paint that passes as turf by colour: 2026-11-04, Kalebo III marker 2 of the play test: grass grew on the Gadgetron offices' mint desk tops, tfrag texture 40, hue 143°, and on the dark green-grey machine paint of the antenna caps by the sea, tie textures 52, 55, 58, 154°, where Rilgar's lawn is 139°; also tie texture 23, a teal glow panel), kept where the triangle faces up (≤ 35°), its texture is turf (at least 15 % of its texels green on their own and an even grain, luma spread ≤ 40: not sand, paving, grey metal, glass or the teal glowing panels) and the texel average under the point is opaque and green both on its own and as drawn (× baked vertex colour, MODULATE: the levels' tinted vertex light alone turned Aridia's sand, Pokitaru's plaza and Oltanis' pavement green; 2026-10-26, from the drawn colour alone: Oltanis, Hoven, Gaspar, Quartu and both Veldins now grow none, their ground has no turf texture; on Umbris, `plan::LEVEL_GROUND`, also muted turf: its grass is grey-green leaves the vertex light turns olive, none of it green on its own, so a texture counts when a quarter of its 5×5 texel averages lean green (green 3 over the mean of red and blue, over red, blue at most 12 over it: not its grey-purple mud, its orange guide lines or teal) and the average under the point does, and the level grows 0.3 of the option's clumps and plant copies, a sparse cover, Lush ~9.6 clumps a square unit, Moderate ~4.2; 2026-10-27, Umbris marker 6 of the fourth play test: the grass by the ship's pad grew none), a `coll_line` ray finds walkable ground within 0.3 units (not the barren surfaces: water, lava, sinking, slippery, pits, deadly), nothing overhead within 0.6 units, and no opaque tfrag that is not such ground lies over it (paths). Each point is a short clump (twelve thin curved blades, ankle-high on Ratchet, root colour = the ground's, lighter tip), or with a small chance (0.3 % / 0.6 %) a fern (seven serrated arching fronds, darker green, 0.22..0.34 high) or a cluster of three white or yellow star flowers just over the blades; plus copies of the level's small swaying or green shrub classes in a ring around their own instances (random yaw, scale 0.75..1.15, ground checked the same way), and no class drawn clear of its origin (an instance's bounding sphere starting over 0.15 units above it, `foliage::MAX_PLANT_LIFT`; 2026-10-27, Orxon marker 4 of the fourth play test: the green lamp heads on the posts beside the ship's pad, class 28, were copied onto the grass as floating lights). Drawn instanced in 8-unit cells (per-cell frustum culling, hidden past the end distance), thinned with distance by a per-plant rank and shrunk smoothly before its cut-off; the shrub wind sway (mode 1 for grass, the class's own for copies), the tfrag fog, alpha-tested opaque and depth-writing (AO and Shadows Dynamic receive on it). Bend (visual only): Ratchet and the 47 drawn mobys nearest the camera (bounding sphere ≤ 3 units) push the plants away within a footprint from their bounding sphere, each along a short trail (a tail lagging behind it, 1.6 units at most for Ratchet, 0.8 for the others) so the plants spring back smoothly after it passes. Moderate: 14 clumps per square unit (width ×1.1: the clump discs cover the ground about once), 0.09..0.18 high, to 30 units, 1.5 copies per plant; Lush: 32 (width ×1.25: about three times over, a carpet), 0.12..0.24 high, to 48 units, 3.5 copies (2026-10-25, from 6 / 14: Novalis Lush 434 k grass points, about 42 MB of records, +0.26 ms GPU a frame on an RTX A1000). Original: no plan, no entities. Changing Moderate ↔ Lush re-plans in the background without a reload |

The presets' values: "Presets" above.

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
  (`original`, `simple`, `enhanced`), `upscaler` (`off`, `ultra_quality`, `quality`, `balanced`, `performance`),
  `preset` (`original`, `low`, `medium`, `high`, `custom`), `frame_smoothing` (`off`, `on`). `RC_GFX_ANTI_ALIASING`, `RC_MSAA=0|2|4|8`, `RC_GFX_HUD`, `RC_GFX_TEXTURES`,
  `RC_GFX_DETAIL`, `RC_GFX_DRAW_DISTANCE`, `RC_GFX_AMBIENT_OCCLUSION`, `RC_GFX_WATER`, `RC_GFX_PARTICLES`, `RC_GFX_FOLIAGE`, `RC_GFX_LIGHTING`, `RC_GFX_UPSCALER`, `RC_GFX_FRAME_SMOOTHING`,
  `RC_GFX_PRESET=original|low|medium|high` (the file's options ignored) override at start; the older `RC_SHADOWS=1|dynamic`
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
* The Port Options page (twenty-four rows) keeps the Camera sub-page's frame and scrolls, four rows at a time (port.rs `VISIBLE`).

## Sharp HUD (the HUD row's High resolution)

From rac-rs's README feature list ("sharp HUD"; none of its code read). The HUD layer (`hud_render.rs`) has always
drawn the game's 2D primitives itself, in the game's 512×416 coordinates; Original renders them at 512×416 and the
composite stretches that image over the frame (bilinear, half a pixel left, as the TV showed it). High resolution:

* **Target.** The layer's target is the frame's size (`display::GameFrame::size`; the layer already spans the whole
  frame, the 512-wide screen in its middle and the wide-screen side margins `display::side_extra` around it, so the
  layout and the margins of commits 37097ba4 / 31aba112 are unchanged). Each primitive keeps the game's position;
  only the vertex shader's clip mapping covers more pixels. Scissors (`FontPrintWindow`'s windows, the menu panels)
  are converted to target pixels by centre coverage (`build_mesh`). A mod's whole-scale HUD replacements keep their
  own scale when it is larger (a 416-line frame).
* **Textures.** Fonts, icons and panels are drawn 1:1 by the game (16×16 glyph cells), so at 1440p and 4K every texel
  is magnified 3.5–5×. The GS bilinear tap at that density is the same blur the TV's stretch gave; High resolution
  samples them with Catmull-Rom (4×4 taps on the unquantised texel position) clamped to the range of the four nearest
  texels, which keeps glyph edges firm without the ring a bicubic leaves round a dark outline. NEAREST primitives
  (TEX1 = 1) stay nearest. A mod's or pack's larger replacement is sampled plain bilinear on its own texels, as before.
* **Composite.** The target is the frame's size, so the composite is pixel for pixel (`scaling.z`: plain bilinear at
  the texel centres, no half-pixel shift). The static layer (the screens' noise and scan lines) stays at 416 lines
  and is stretched bilinear, as those are the game's pixel-sized effects.
* **Upscaler contract** (0.6.0, with the upscaler's session): the HUD composite is drawn in the main camera's UI pass
  into `GameFrame`; the upscaler keeps `GameFrame` at the output size and lowers only the 3D cameras' resolution, so
  the HUD stays at full resolution. With it on, its compose camera (`upscale::ComposeCamera`, on `GameFrame`) takes
  over the UI pass and every `UiTargetCamera(FlyCam)` node, the HUD composite included; the layer is sized from
  `GameFrame::size`, never from the FlyCam's own (smaller) target. A frame smaller than the window (the Resolution row's 720p / 1080p) draws the HUD
  at the frame's lines, and the present camera scales the whole frame.
* **Look.** Same layout, colours and blending (the GS formulas per fragment are unchanged); text and icons are crisp
  where Original is soft. It is part of the Enhanced preset.
* Checked 2026-11-07 (4K frame, `RC_RESOLUTION=2160`): see "Checks" below.

## Texture packs (PCSX2 format)

From rac-rs's README feature list ("PCSX2 texture packs work as they are"; none of its code read). Code:
`crates/rc-formats/src/pcsx2.rs` (the names), `crates/rc-engine/src/texture_packs.rs` (the folder, the index, the
lookups), crate::mods (the toggles), crate::mod_textures (world loading). Player steps: the README.

**The scheme** (learned from PCSX2's source, GPL-3.0, read only for the scheme and nothing copied:
`GSTextureReplacements.cpp`, `GSTextureCache.cpp` `HashCacheKey::Create` / `HashTexture` / `HashTextureLevel`,
`GSClut.cpp` `Read32`; PCSX2 has no documentation page for it, only its 2022–2025 progress posts). A replacement is
`<tex>-<clut>-<bits>.dds|png` anywhere under `<serial>/replacements/`; `tex` is XXH3-64 of the texture as it lies in GS
memory (a level at least one block in each direction is hashed block by block, each block's 256 bytes in the GS's
own swizzled order, PSMT8's column layout; a smaller one row by row), `clut` XXH3-64 of the 256-entry CLUT expanded to
32-bit RGBA in index order, `bits` = PSM | TW << 6 | TH << 10 (TEXA bits only for 16 / 24-bit textures; bit 14, once
TCC, set in older packs and ignored). Hashes are `%llx` (no zero padding).

**Why every kind can match.** Every texture the port uploads is 8-bit indexed with a 32-bit CLUT (PSMT8 / CT32 /
CSM1): the tfrag, moby, tie and shrub tables, the shrub billboards, the sky, the particles, the FX textures (fonts,
logo) and the HUD frames. The game uploads their linear indices with PSMT8 image transfers, so GS memory holds them in
PSMT8's layout, which the port rebuilds (`pcsx2::t8_block_offset`, `t8_hash_bytes`) from the stored indices; the CLUT
is the stored one with CSM1 undone. No translation step: names are computed at the level load
(`texture_packs::register_level`, about … ms a level with a pack on, nothing with none).

**Verified** against the community pack "SCUS-97199 ratchet1 v2" (13 200 files, the one the HD Assets mod was made
from), over all 19 levels with a throwaway checker (kept in `work/`): the names the port computes are in the pack for

| Kind | Textures | CLUT in the pack | Name in the pack |
|---|---|---|---|
| tfrag | 1639 | 1600 | 1346 |
| moby | 3870 | 3594 | 3112 |
| tie | 2549 | 2504 | 1916 |
| shrub | 963 | 954 | 851 |
| shrub billboard | 83 | 83 | 82 |
| sky | 102 | 98 | 98 |
| particle | 948 | 811 | 810 |
| FX (fonts, logo) | 1057 | 635 | 607 |
| HUD frame | 6242 | 6065 | 2778 |

(counts per level, so shared textures count once per level). Where the CLUT is in the pack but the name is not, the pack
does not cover that texture, or (HUD) PCSX2 named the draw by a region (below).

**What cannot match exactly, and why:**
* **Region names** (`<tex>-<clut>-r<w>x<h>-<bits>`): PCSX2 hashes only the part of a texture a draw's CLAMP region
  (or a recent smaller upload) selects, so the name depends on each draw's GS state, not on the texture. The packs' HUD
  font glyphs and some HUD frames are named this way; they are skipped (as PCSX2 skips them when the region differs).
* **Mipmapped hashes**: with PCSX2's hardware mipmapping on, a mipmapped draw's name also hashes its lower levels and
  starts at the draw's first level. The port looks up both the level-0 name and the whole chain's
  (`texture_packs::key_chain`, tfrag and billboards); a name from a draw that started at level 1 or deeper cannot be
  matched. The community pack was dumped with mipmapping off (no chain name in it).
* **Non-power-of-two sizes**: PCSX2 hashes the whole `1 << TW` × `1 << TH` area, beyond the texture's own texels; no
  retail texture has such a size (none in the table above).
* **Streamed pictures** (planet maps, menu backgrounds, `hud_images`), environment / metal maps, the movies and the
  spaceship textures are not looked up yet.
* **BC7** files load for the world, mobys and the sky (sampled as they are) but not for the HUD and particles, whose
  layers are built on the CPU (BC1, BC2, BC3 and 32-bit only there).

**Files.** DDS: BC1 (decoded, opaque at 0x80), BC2, BC3, BC7, 32-bit; PNG: RGBA or RGB (opaque at 0x80); alpha raw
(0x80 = opaque), as PCSX2 dumps. A whole multiple of the original's size (1..16 for the world, 1..8 for the HUD and
particles); other sizes are skipped with a log line. Packs apply live to the HUD (its atlas is rebuilt on a toggle) and
from the next level load to the world, the mobys, the sky and the particles. `RC_TEXTURE_PACKS_DIR` overrides the
folder (and, set, enables packs in frame-exact runs, with the enabled set from `RC_SETTINGS_FILE`'s `mods = pack:<folder>`).

## Checks

2026-11-07, Windows, dev build (captures in the git-ignored `work/shots/`, the test pack in `work/texture_packs/`):

* **Texture packs.** A test pack generated from the port's own data (`make-pack` in the throwaway `work/texpack-verify`:
  every texture of Novalis and Kerwan named as PCSX2 names it, colours rotated R,G,B → B,R,G, at 2×, half PNG and half
  uncompressed DDS, raw alpha, in PCSX2's `SCUS-97199/replacements/` layout; 1097 files, nothing from it committed).
  Novalis (`RC_LEVEL=1`): 623 world textures registered (60 ms), 687 of 687 world / moby / sky lookups replaced, 375
  HUD textures, 41 particle layers; Kerwan (`RC_LEVEL=3`): 440 registered (67 ms), 544 of 545, 60 particle layers.
  Both levels' captures with the pack on show the rotated colours on the ground, plants, buildings, Ratchet, the sky and
  the HUD's health orbs; with it off they match the game.
* **Sharp HUD.** 4K frame (`RC_RESOLUTION=2160`, `RC_ASPECT=16:9`), Original against High resolution: the HUD
  (`RC_HUD_DEMO`), a `FontPrintWindow` text box (`RC_HUD_TEXT_WINDOW`) and the pause menu (a two-update Start press
  replayed with `RC_REPLAY`). Same layout and colours; High resolution's text and panel edges are crisp where
  Original's are soft. First try sampled past a glyph's cell (thin stray lines over some letters, the neighbouring
  glyphs in the font texture): the bicubic taps now stay inside the primitive's own texel rectangle
  (`ATTRIBUTE_UV_RECT`). The health orbs' art is aliased at 1×; High resolution shows its texel steps where Original
  blurs them.

## Draw distances

What ends the view in the game, and what the Draw distance row (factor s) does with it (units: game units, 1024
integer units each; depth is along the view axis):

| Kind | The game's | At s > 1 |
|---|---|---|
| Tfrag | `TfragProc`: culled when the sphere's near edge is past 512000 (0x160ec0, 500 units); no fade (pop) | 500·s (`tfrag_lod::DRAW_DISTANCE`); faded per pixel by depth over the last 16·s units (tfrag.wgsl `end_fade`) |
| Tie | per instance, capped at 720 (0x160fe0), culled when the centre's depth passes it; no fade. The Visibomb view caps it at 144 | the distance ×s (the Visibomb's cap kept); faded per instance by the centre's depth over the last 16·s units (`tie_lod` x bits 16..23, tie.wgsl) |
| Shrub | D = max(dd, 16), F + 24 for billboard classes, capped at 500 (0x1604a4); meshes fade over the last 8 units, billboards over the last 16 | D ×s, both fade bands ×s (shrub.wgsl, shrub_billboard.wgsl, `shrub_render::cull_shows`) |
| Moby | MobyProc: per moby +0x32 (the gameplay record's, 64 on the Veldin 2 props and tanks of the 2026-11-07 play tests), cap 500 (0x15fff0), 16-unit fade | ×s and the fade band ×s; +0x31 keeps the game's result, so a moby the game no longer updates out there holds its last pose (`moby_lod::moby_proc_drawn`, G-REN-050) |
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


## Upscaler checks (2026-11-07)

* **Screenshots** (Novalis spawn, `RC_SCENE=0 RC_LEVEL=1 RC_RESOLUTION=1080 RC_GFX_PRESET=medium RC_HUD_DEMO=1
  RC_HUD_TEXT=…`, frame 150), every mode at 1080p 4:3 (1440×1080: the 3D at 1108×831, 960×720, 847×635, 720×540) and
  1080p 16:9 (1920×1080: 1477×831, 1280×720, 1129×635, 960×540): the HUD's bolt counter and banner are identical
  in every mode (drawn at the frame's resolution); the world softens gradually, Performance visibly so on distant
  thin geometry (antennas, tree branches), with no ringing at edges. No ultrawide capture: the window's shape has no
  switch (`Aspect::Fill` follows the monitor), so 16:9 is the wide case.
* **Frame time** (RTX A1000 laptop GPU, Vulkan, dev build, `RC_NOVSYNC=1`, preset High; the CPU was ~85 % busy with
  other builds, so wall-clock fps at 1080p was CPU-bound noise). GPU-bound at a 2160p frame, fps over two interleaved
  rounds: Off 15–21 (median ~18, ~56 ms), Quality 22–29 (~28, ~36 ms), Performance 28–35 (~31, ~32 ms). The passes'
  own GPU time at 1080p (`RC_RENDER_DIAG=1`, `render/upscale_easu`, `render/upscale_rcas`): EASU 0.35–0.54 ms, RCAS
  0.15–0.17 ms; Bevy's opaque pass 0.91 ms (Off) → 0.79 / 0.59 / 0.53 ms (Ultra quality / Quality / Performance).
  The port's own full-screen passes (AO, GI, light shafts, water, post) are not timed by that switch; they scale
  with the 3D picture's pixels, which is where the 2160p gain comes from.
* **Off unchanged**: against the base commit's build, all-Original at frame 150 is byte-identical; a heavy set
  (4x, AO, Enhanced water / particles / lighting, Moderate foliage) PSNR 104.6 dB (a few pixels; frame 20 differs
  between two runs of the same build: start-up timing, not rendering).
* Not checked on screen: the fade, the underwater tint and the missile overlay on the compose camera (no frame of
  them was captured), the pause menu's snapshot and the Port Options rows under the upscaler (no input script), and
  a run-time switch of the option (the code follows it every frame; only start-up switches were run).
## DLSS (investigated 2026-11-07, not added)

DLSS is not offered: it would need what `AGENTS.md` forbids (a Bevy feature and a new dependency), NVIDIA's SDK at
build time and NVIDIA's licence terms, and most of the port's renderer reworked. What it would take:

* **Bevy's support.** Bevy 0.19 has DLSS in `bevy_anti_alias::dlss` behind the cargo feature `dlss` (off by default),
  which pulls in the crate `dlss_wgpu` (4.0.0 for Bevy 0.19 / wgpu 29: DLSS SDK 310.5.3) and Bevy's
  `raw_vulkan_init`. `DlssInitPlugin` must be added before `RenderPlugin`, a `DlssProjectId` resource inserted, and
  the `Dlss` component put on the camera; `DlssSuperResolutionSupported` says at run time whether the machine can.
* **Platforms.** NVIDIA RTX GPUs only, Windows and Linux, and only on wgpu's **Vulkan** backend (not DX12 or Metal):
  macOS, the main platform, never gets it, and Windows would have to run on Vulkan.
* **Build.** `dlss_wgpu` builds against the DLSS SDK, which it cannot redistribute: clone NVIDIA's DLSS repository
  at the matching tag, set `DLSS_SDK` to its absolute path, install the Vulkan SDK (`VULKAN_SDK`) and clang (for
  bindgen). Every developer and CI machine building with the feature needs all of that.
* **Shipping.** `nvngx_dlss.dll` (Windows) / `libnvidia-ngx-dlss.so.<version>` (Linux) from the SDK's `lib/*/rel`
  beside the executable, plus the SDK's copyright and licence text (its programming guide, section 9.5).
* **Licence.** The NVIDIA RTX SDK licence: the SDK "may not" be used so that it becomes subject to an open-source
  licence (section 4(e)); the port is ISC, permissive, but the SDK and DLLs must stay out of its source tree and
  release; NVIDIA must be notified before a commercial release; NVIDIA's marks and attribution go on the splash screen,
  the about box and the credits; use is limited to systems with NVIDIA GPUs. Whether a free, open-source port may ship
  it at all is a question for NVIDIA (nvidia-rtx-license-questions@nvidia.com); the licence has no open-source
  exemption.
* **The renderer.** Bevy's `Dlss` component requires `Hdr`, `DepthPrepass`, `MotionVectorPrepass`, `TemporalJitter`
  and `MipBias`, and renders the main passes at the lower resolution through `MainPassResolutionOverride`. The port
  draws in LDR with GS-exact byte blends, its world materials (tfrag, tie, shrub, moby, sky, water, particles, foliage)
  write no motion vectors, and its ~30 full-screen passes (AO, shadows, GI, light shafts, water, post, AA blit, …)
  read the whole target rather than the override's viewport. Each would need motion vectors, jitter and the override
  (or DLSS run on crate::upscale's separate world image, which still needs the vectors and the jitter).

Recommendation: keep FSR 1 (`crate::upscale`, every GPU and backend, no dependency). DLSS could come later as an
optional, separately built and separately licensed release (a cargo feature in the fork only, never in the default
build), after motion vectors exist for the world.
