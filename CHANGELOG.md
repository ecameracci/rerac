# Changelog

## Unreleased

- Brings in upstream ReRAC's fixes through its v0.3.7: the game keeps its pace on screens where vsync does not hold it, the pause menu no longer leaves stray panels on a slow frame, In-Level Movies no longer show a character twice, holding Circle with the Swingshot no longer leaves Ratchet stuck, the reticles are solid in 16:9, level textures are drawn at the PS2's scale, the Bomb Glove aims from Ratchet's hand and its bombs sink in water, Oltanis' lightning, Gemlik's lasers, Novalis' water flowers, Quartu's typed monitor, the teleporters' arrival sound, the hoverboard's boost meter with a weapon held, the Visibomb's scanlines, bombs and mines on moving platforms, the Epilogue's picture on every planet, and more.
- Brings in upstream ReRAC's fixes through its v0.3.8: coming back to Gemlik after the escape, the ship is waiting again; gold weapons take Ratchet's warm tint in hand, in the gold upgrade's demo and on the menu; mines, decoys and Doom bots thrown from a moving platform (the Metropolis train) keep its speed and ride along with it; Kalebo III's breakroom chickens peck, cluck and run from Ratchet; first-person wrench throws follow Ratchet's tilt on the Magneboots; Clank and the pack shrink with Ratchet into the Hologuise; the game remembers the language you pick, and the first start follows the computer's language when the game has it.
- With a completed save, ordinary moves no longer toggle cheats ("Cheat Enabled" / "Cheat Disabled") on planets other than Novalis: the cheat move sequences are read from every level's own data, so only the game's sequences turn cheats on, as in the original. The Cheats page's list and the pause menu's debug codes read the right tables on every level too.
- New keyboard and mouse layout: WASD moves, the mouse and the arrow keys turn the camera, Space jumps, left click swings the wrench (□), right click fires (○), middle click opens the quick select (△), Ctrl crouches (R1), Shift is L1, Esc pauses (Start) and Tab opens the map (Select); Alt walks. The number row 1–8 (1 the top slot, then clockwise) and the numpad (8 up, 9 up-right, …) pick a quick-select weapon or gadget directly. The cursor is locked while playing and free in menus. The debug fly camera moved from Tab to F7. Port Options has Mouse left / right and Mouse up / down (Normal / Inverted) and Mouse sensitivity (1–10); in first person the mouse's left / right turns the view the same way; the mouse turns the camera from the slightest movement and at the same speed whatever the frame rate.
- Water on Enhanced: the chutes Ratchet slides down (Rilgar's slide to the rising-water area, and the chutes on Batalia) are covered by a shallow sheet of flowing water. The original game draws them bare, so Water on Original is unchanged. Quartu's metal slide from the factory door down to the cave floor is dry and stays bare.
- Water on Enhanced: the mirror reflections show the buildings, the ground and the sky again, not just the ships and characters (Rilgar's bay, Novalis's river). Pokitaru's bay now mirrors the islands, the palms and the tower too when seen at a low angle. The reflection in Novalis's river pool holds still instead of flickering. Looking steeply down, the water shows mostly what is under it, as before.
- Water on Enhanced: shallow pools with a flat floor, such as the one at the bottom of Rilgar's water slide, are no longer covered in white foam that made them look opaque. Foam now only forms where the floor rises toward the surface: along shores, walls and the edges of platforms.
- Water on Enhanced: in Rilgar's flooded sewer at the bottom of the water slide, the round grates just under the surface are no longer outlined in white foam. The water there is drawn a little higher (it still rises and falls with the game's water, which is unchanged for Ratchet and everything else).
- Water on Enhanced: water no longer turns into an opaque deep-blue sheet when the window or render size changes after the level loads (for example at startup, or on a resolution or aspect change). The floor and platforms of Rilgar's slide pool and the bed of Novalis's cave pool now show through again, with refraction and caustics.
- Port Options: the Shadows row is now Lighting. Original draws the game's own shadows, as on the PS2. Simple draws shadows from the sun in their place, cast by everything (the level, plants, characters and objects). Enhanced adds the sun's rays, and will hold the other advanced lighting effects. Shadows Off is gone; a saved Off becomes Original and a saved Dynamic becomes Enhanced. The presets leave Lighting as it is. Lighting Enhanced's light shafts are off on the planets without a sun lighting open air (Aridia, Rilgar, Nebula G34, Umbris, Batalia, Orxon, Hoven, Oltanis, Quartu) and in the space scenes between planets. On Drek's fleet the sun shadows now fall away from the main light, no longer toward it (they followed the dim light from below).
- Lighting Enhanced: the sun's rays now stream from the sun painted in each planet's sky (Veldin's sunset, the bright sky over Novalis and Eudora, Gemlik's red sun) and turn with it; skies without a sun (night, space, storms) have no rays. They are gentler where a big sun fills the view (Gemlik): a warm tint, no longer an orange wash over everything. Where the sky paints a sun disc (Veldin's sunset, Gemlik's red sun), the sun shadows (Lighting Simple and Enhanced) now fall away from it too: on Gemlik they no longer run across the red sun's light.
- Port Options: Lighting Enhanced adds global illumination: light bounces once between nearby surfaces, so a coloured wall tints the ground beside it and lit ground brightens the shaded side of what stands over it.
- Port Options: Lighting Enhanced also polishes the 3D picture: lamps, glows and explosions get a soft bloom, a gentle film-like grade adds a little contrast and colour without clipping the whites, and a light sharpening brings out detail. The HUD, menus and movies are unchanged.
- Port Options has a new graphics row, Draw distance, below Detail distance: Far, Farther and Maximum draw the level's scenery, buildings and plants farther away (×1.5, ×2, ×4), and fade it out at the new limit instead of popping. Characters, objects and particles keep the original distance. Original still looks like the PS2 game; the presets leave the row as it is.
- New Port Options row Frame smoothing (Off / On, after FPS limit; Off by default): on a display faster than 60 Hz, On draws every frame the display can show, with the camera, Ratchet, every object's movement and animation, and the particles placed between the game's 60 ticks a second instead of repeating each tick's picture. The game itself runs exactly as with Off; the picture is up to one tick (16.7 ms) behind. The HUD, menus, shadows, water, sky spin and texture animations still move 60 times a second. Cuts, respawns and level changes are drawn without blending across them.
- Port Options → Foliage (Original, Moderate, Lush) adds a carpet of short grass, a scattering of small white and yellow flowers, low ferns and more of the level's own small plants on grassy ground only (never on sand, paving, metal floors or the space stations): it sways with the level's plants, bends away from Ratchet and the enemies and characters walking through it and springs back behind them, thins out smoothly in the distance and never changes the game itself. Lush is denser and reaches further than Moderate; Umbris' dry, olive ground gets only a sparse scattering. Original shows the level as it was; the Enhanced preset uses Moderate.
- Water on Enhanced now covers the seas too: Rilgar's city bay (with its fish swimming under the surface) and Batalia's sea get the clear, rippling water with reflections and foam at the shore, fading into the sea's own colour in the distance. Pokitaru's open ocean and the murky or lava liquids keep the original look; Water on Original is unchanged.
- Water on Enhanced now mirrors the world in flat water: Novalis's river and pools and Rilgar's bay reflect the buildings, the trees and Ratchet himself, rippling with the waves, including the scenery off screen above you (the reflection used to show only what was already on screen). The mirrored surface is the water that fills most of the view, counting only what the ground does not hide, so water behind you, beside the view or under the sand no longer takes the reflection away from the water you are looking at (on Rilgar's start beach the bay keeps it over the little pools), and a change to other water fades over a fifth of a second instead of switching at once. The buildings and scenery above the view stay in the reflection when the camera looks down at the water: Rilgar's lit windows no longer drop out of the bay, or flicker in and out as the camera moves. Water on Original is unchanged.
- Water on Enhanced now rolls: a gentle swell moves over the open seas of Rilgar, Batalia and Pokitaru, calming near the shore and in the distance, with the reflections and highlights following the waves; Novalis's rivers ripple with it too. It is drawn only, so swimming and floating objects behave as before. Pokitaru's ocean now has a real surface too: the original's turquoise, clear enough to see the sand, rocks and the boats' floats beneath, with soft reflections of the islands and sky, a gentle sun glint, and the swell rolling across it.
- Water on Enhanced now gives the other liquids their own looks: Gaspar's and Umbris's lava glows through slowly flowing cracks in a dark crust (Gaspar's lava falls flow along with them), with sharp crack edges, finer cracks and a grainy crust up close, and Umbris's swamp and Gemlik's acid pools are murky, faintly glowing sludge with slow ripples and bubbles. The lava's cracks no longer show straight seams where the lava's blocks meet, and Hoven's river is almost frozen: its own green-blue colour and white streaks, barely drifting instead of rolling waves, with a faint sheen and glint. The brown haze at the bottom of Oltanis stays the fog it is in the original, no longer a glossy liquid surface. The colours stay those of the original; Water on Original is unchanged.
- Water on Enhanced now gives Aridia's quicksand a look of its own: thick, slow mud whose broad, gentle swell shows as glossy bands of reflected sky and moonlight even from a distance, dark glossy wet patches and a lighter, matte, finely cracked crust along the banks, and bubbles across the flat that rise as glossy domes, pop and leave a ring that settles. It keeps the original's colour and texture and the shadows on it, and the night stays about as bright; Water on Original is unchanged.
- Water on Enhanced also changes the view under water: the water takes on its colour with distance (reds fade first), the light dims the deeper you go, rippling light patterns play on the floors, soft light shafts come down from the surface and the picture sways gently. It fades in and out as the camera goes under or comes up, and a camera at the surface shows the waterline. Under Pokitaru's ocean the view takes the turquoise of its surface instead of the game's dark blue, and the deep water beyond the shelves fades smoothly into that turquoise instead of turning almost black behind a hard edge. Original still looks like the PS2 game.
- Port Options → Particles has a third value, Enhanced: soft particles, and explosions, fire, muzzle flashes and sparks light the floor, walls and characters around them in their own colour, flaring up and dying away with the effect. Enhanced also makes the stars glow and twinkle softly (on the title screen, in the levels' skies and in space travel), and redraws the rain on Batalia and Oltanis as fine motion-blurred streaks and the snow on Hoven as soft drifting flakes, about four times as thick.
- Particles on Enhanced: mist, foam, smoke, dust, steam, rain and splashes no longer light their surroundings, so the river mist and foam at the foot of Novalis's waterfall no longer make a bright patch on and above the water; only effects that glow (fire, explosions, sparks, flashes, lamps) cast light. Glows that last, such as the green glow under Orxon's floor grates and its spark fountains, now hold a steady brightness and change it smoothly instead of flickering, and lights no longer pop as others come and go. The particles themselves are drawn as before, and Particles Original and Soft are unchanged.

## v0.4.3-alpha

- Blarg: after Clank hands the hero back to Ratchet (and the Hydrodisplacer cutscene), Clank's footsteps no longer keep playing at the Clank station. The same applies to Orxon's Clank section.
- Skipping a planet landing with Cross no longer makes Ratchet jump: he steps forward out of the landing spot first, as in the original.
- In airless areas (Orxon and Blarg outside the air curtains), the pause menu's Gadgets page shows the head items as unusable, so the O2 Mask can't be swapped out where Ratchet needs it, as in the original. Before, this only happened on Gemlik.
- Quartu: Drek's broadcast cutscenes show the typed yellow captions at the top left, as in the original, also when replayed from Goodies → In-Level Movies.
- Effects that were built mirrored now face the right way, as in the original: Umbris' beast tongue and its ground line, the light fixtures' glows, Qwark's tractor beam, the Tesla Claw's and the Tesla bolts' glows, Batalia's flame sheets, the Metal Detector's scan, Gaspar's meteor paths, the Gemlik fighters' shot spread, grenade bounces on Batalia and the roll of tumbling debris.
- Wrench swings and glove throws auto-aim as in the original: the ground combo, the jump attack, the Comet Strike and the glove throws turn Ratchet toward a nearby enemy in front of him, and the combo's steps close the distance to it.
- Suck Cannon shots that bounce off a slope, step or doorway lip keep flying straight, as in the original; before, the bounce could send them up toward the ceiling or off to the side.

## v0.4.2-alpha

- Options → Quit Game has a third choice, Square: Main Menu, which goes back to the title screen without the logos; the loading picture shows while the level reloads, instead of a black screen. Circle: Yes still closes the game.
- Quit Game in the main menu's Options closes the game.
- The memory card icon shows at the bottom left while the game autosaves: after choosing a planet in the ship, picking up a gold bolt and the other autosaves. Before, it never appeared. In wide aspect ratios it sits in the screen's corner.
- The Persuader, Map-o-Matic and Bolt Grabber stay on Ratchet in cutscenes and the ship's take-off and landing, and glow as in the original.
- Umbris: bombs thrown while wading in the drained pools fly as they should instead of going off at Ratchet's feet.
- The Glove of Doom and the Decoy Glove show their landing ring on the ground while held, like the Bomb Glove.
- The map's star marks your ship instead of sitting in the map's corner.
- Port Options: the rows are grouped (display, graphics, gameplay) and On / Off are capitalised. The FPS limit is 30, 60, 120 or Unlimited (a saved 144 becomes 120).
- Dynamic shadows no longer pop as the camera moves, and ambient occlusion no longer leaves a bright disc on the wall behind a climbing Ratchet.
- A `settings.toml` next to the game overrides the settings it lists.

## v0.4.1-alpha

- Port Options has two new rows. Strafing: Modern gives L2 / R2 Ratchet & Clank 3's strafe on foot, with a strafe flip on Cross. Quick Select: Modern holds the world still, dimmed, while Triangle keeps the quick-select ring open. Classic keeps both as in the original.
- Water on Enhanced: Ratchet makes ripples where he dives, swims and wades.
- Dynamic shadows no longer flicker going into the Novalis cave, and ambient occlusion no longer darkens Ratchet's face.
- The Swingshot's rope shows during the hook's whole flight, from the first swing.
- Smoother frames while swinging on the Swingshot.
- The README explains how to report a bug: F8 to mark it, and which files to send.

## v0.4.0-alpha

- Mods: texture mods in a `mods` folder next to the game, turned on and off from Options → Mods. They can replace the HUD, fonts and icons, and the world, character, enemy and sky textures.
- Port Options has three new graphics options: Ambient occlusion, Water (Enhanced: clear water with rippling reflections, light patterns on the riverbed and foam at the shore) and Particles (Soft: smoke no longer cuts into walls and floors). The Enhanced preset turns them all on. Shadows has a new Dynamic setting: everything (the level, plants, characters and objects) casts shadows from the sun, and the sun shines rays through the scenery. Original still looks like the PS2 game.
- Aspect ratio has a new Fill setting: the picture takes the window's own shape, so ultrawide monitors have no black bars. 16:10, 16:9 and Fill show more of the world at the sides instead of stretching it.
- Portable builds: the game keeps its data in a `data` folder next to it and asks for the .iso on the first start. Saves go in a `memcard` folder next to it, starting from a copy of the saves from earlier versions.
- Started on its own, the game writes a log for each run to a `logs` folder next to it, shows errors and crashes in a dialog, and on Windows no console window stays open while playing. `rerac.exe` has the ReRAC icon.
- Bug reports: every run records its input and starting state so it can be replayed. Press F8 the moment something goes wrong to mark it.
- Steam Controller on Windows: presses no longer get lost and buttons no longer stick under Steam Input.
- Start skips the logos at startup, and the pop between the logos and the title screen is gone.
- Veldin Orbit keeps its music after the fleet ship mission.
- Options: Quit Game shows Qwark's picture again, and Port Options has its own picture.
- The pause menu no longer shows leftover boxes behind it after watching a movie or scene from Goodies.
- Goodies: the Sketchbook, Epilogue, Making Of and Commercials descriptions show on every planet, instead of "Paradox! This message does not exist".
- The save confirm highlights the slot being overwritten.
- Cutscenes: Clank's antenna glow sits on his antenna instead of the ground beside him, and Ratchet wears his helmet and O2 mask where he should.
- Ratchet wears the Persuader, Map-o-Matic and Bolt Grabber once he has them.
- Jumping off ice keeps the slide's speed.
- Mine Glove: the mine shows in the glove, and a mine's explosion has its full cloud of smoke.
- Clank's rotor hums while he glides.

## v0.3.5-alpha

- Widescreen: gameplay and cutscenes each have their own aspect ratio setting, and movies fill the cutscene's frame.
- Port Options can be opened from the main menu, and its list is taller. The window opens at the saved resolution and aspect ratio, and Quit Game closes the game.
- Save slots show the time played, and the save date uses your local time.
- The play timer stops on the pause menu.
- The ammo counter comes back after a death.
- Fixed the purple or black screen on the first start, and the blue bars on the planet title cards while the text fades in.
- Windows builds are supported. On Linux, gamepads work behind a wireless keyboard receiver, and the 8BitDo Ultimate's X and Y no longer swap.

## v0.3.4-alpha

Fixes from playing v0.3.3.

- Cutscenes: characters are lit like the place they stand in, Ratchet's head is the right size, and Clank's eyes and antenna glow.
- Lamps, glowing mushrooms and other lights brighten Ratchet again, like in Novalis's caves, Aridia's factory and Al's shop.
- Ratchet raises his gun when standing close to a wall, and his weapons fire along it.
- Aiming with the wrench or the Swingshot shows the reticle.
- The Swingshot's claws fold out once and stay open.
- The Comet-Strike sparks when the wrench hits a wall or an enemy.
- The Bomb Glove's blast throws enemies away from it, and the gold Bomb Glove has its bigger, golden blast.
- Pressing Start on the Hoverboard asks to quit the race.
- Shop messages fit inside their box.
- Novalis: Drek's gunship destroys the bridge only once.
- Aridia: the lifts come for Ratchet from either end, and the shark and nest counters show during the race.
- Kerwan: Ratchet no longer falls off the taxis mid-ride.
- Blarg Station: the pad Clank uses carries Ratchet, the airlock doors stay in step after Clank's section, and the queen's screams and death play out as they should.

## v0.3.3-alpha

Fixes from playing v0.3.2.

- Gemlik Base: Qwark's shield drops again, he flies off when the fight starts, the camera follows his crash, and his health bar goes away afterwards.
- Gemlik Base: after the fight the new ship is waiting, closed, with its orange canopy, and Ratchet can board it.
- Veldin Orbit: the nebulae out past the fleet are back, the windows have their glass, and the two pools near the start have water.
- Quartu: the patrolling guard walks its route again, so the Hologuise gets Ratchet past the barrier.
- Hoven: Ratchet freezes in the icy water.
- Veldin: after dying, the pools drained with the Hydrodisplacer stay drained.
- Blarg Station: broken objects no longer leave oversized pieces behind.
- First person: only a thrown wrench shows in Ratchet's hand.
- The Drone Device works from the pause menu, and its drones show up next to Ratchet there.
- The map page on Veldin shows the right name.
- Cutscene dialogue plays in the chosen language.

## v0.3.2-alpha

Fixes from playing v0.3.1.

- Windows: the black window on some PCs is fixed.
- Gold weapons look gold, in Ratchet's hand, in the menus and in the cutscene that now plays after buying one.
- Ratchet gets darker indoors, like in Veldin's garage, and so do his weapon and Clank.
- Ratchet glares at enemies, and makes faces when hurt, holding his breath or pulling out a big weapon.
- Things that should stand on the ground no longer float: Qwark at Oltanis's Gadgetron stand, Umbris's Hydrodisplacer spots and more.
- Oltanis: the round tilting platforms are solid again.
- Kalebo III: the lifts go all the way up and down.
- Gemlik Base: Ratchet no longer dies outside the base during the space battle, and the asteroids drift.
- Umbris: the turrets fire from their guns.
- The map: Ratchet's arrow turns the right way, the markers are in the right places, and the map no longer goes blank after skipping a cutscene.
- The oxygen meter no longer shows up after drowning.
- Options: the pages keep their layout after leaving Port Options, which now scrolls.

## v0.3.1-alpha

Fixes from playing v0.3.0, smoother frame pacing, and new graphics options.

- Dying no longer empties every weapon.
- Gemlik Base: Ratchet wears his O2 Mask from the start.
- Clank and Giant Clank no longer get stuck in first person.
- The weapon counter no longer keeps the vendor's ammo icon, and each weapon shows its own ammo.
- Vendor: one weapon per press when browsing, and the selection frame shows again.
- The Comet-Strike works on Magneboots walls and ceilings.
- The Metal Detector's head points at what it scans.
- The boots hide Ratchet's feet.
- Gadgets and Weapons pages: Ratchet plays each item's animation on every planet, and the Heli-Pack spins and folds.
- Jet fighter missions: locking on to a target close to the ship no longer crashes the game.
- One game step per frame, at up to 60 frames a second: no more doubled or skipped frames, and less work for the computer.
- New graphics options under Options, Port Options: a preset, anti-aliasing, detail distance, texture filtering and HUD scaling. Every option starts at Original, the PS2's picture on a TV, so the game now looks soft like the original. Set Anti-aliasing to Off for the sharper look of earlier versions.
- The Port Options page fits all its rows.
- A Linux version (64-bit PCs), not tested yet: reports welcome.

## v0.3.0-alpha

Dying now works the way it does in the original, and a long list of fixes from play-testing Umbris, Orxon, Gemlik and the rest.

- Dying brings back enemies, crates and pickups the way the original does, and saves remember which enemies stay defeated and what they dropped.
- Lifts and gates you already opened stay open after loading a save.
- After dying, the ship can be boarded and has its glass, nanotech no longer gets stuck in the air or turns see-through, and leftover fog no longer lingers.
- Kerwan: the troopers and the info bot are on the train.
- Umbris, Gaspar, Orxon and the last two planets: breaking ammo crates no longer crashes the game, and the vendor sells ammo again.
- Umbris: the first landing no longer leaves the screen dark with Ratchet missing.
- The Snagglebeast: its fire sweep, suck beam, shockwave and shield work like the original, and rapid fire no longer lands every shot. A few other enemies had the same problem and are fixed too.
- Boss health bars no longer freeze after dying.
- Orxon: the gate at the start of Clank's section opens without dying first, dying there brings Clank back at the checkpoint, and Clank no longer gets stuck after the landing or the turret cutscene. The gadgetbots' glass bubbles show.
- Gemlik Base: Ratchet walks the curved Magneboots rails, and the robots' rolling fire wall shows and hurts.
- The Suck Cannon pulls in Eudora's robots, the crew on Drek's fleet and the hoppers on Veldin.
- Ratchet no longer holds the wrench on ledges or while swimming.
- Space travel: planets show their real colours, and the ship's canopy stays in place in every flight and cutscene.
- The planet preview in the ship's menu turns smoothly, and the "Enter Ship" prompt no longer shows behind the planet menu.
- Qwark's broadcast on Rilgar no longer takes on the TV screen's tint.
- Vendor: weapons face the right way, and the item bar fits the list.
- Ammo and other pickups no longer show their insides as they fade in.
- Ratchet, the crate and the ship stay on the title screen behind the main menu.

## v0.2.3-alpha

Mostly Blarg Station, plus fixes for Clank's sections and a few other planets.

- Blarg Station: the energy doors' beams cross the right way, Clank's bridge extends once he has the Hydrodisplacer, and freed gadgetbots no longer fly off and die. Their command voices play.
- Blarg Station: the big red button's cover opens once the room is clear, and the self-destruct countdown shows on screen.
- Blarg Station: the countdown voice lines play, and the escape, the shuttle flight and the launch tube have their explosion sounds.
- The queen's health bar on Blarg and the Umbris boss's health bar go away when the boss dies.
- Dying in a Clank section no longer brings you back as Ratchet, leaves Clank unhurtable or leaves a frozen Ratchet standing around.
- Clank: the pause menu greys out what he can't use, ledge climbs work, and his antenna glow and heli-pack stay in place when he turns.
- Ratchet's ship is solid again: you can no longer walk through it.
- The info bot movie from Drek's henchman on Eudora plays once.
- The quick select no longer opens during scripted scenes or when △ is used on a prompt.
- Info bot and other messages at the top of the screen no longer stay up too long.
- The weapon counter keeps its shape in widescreen.
- Oltanis's lightning thunders.

## v0.2.2-alpha

- Fixes a black screen after the opening movies of a new game, after loading a save and after flying to another planet.

## v0.2.1-alpha

Fixes from the first round of testing, and new display settings.

- Port Options has three new settings: Aspect ratio (4:3 as on a TV, or 16:10 and 16:9 widescreen), Resolution (Window, or 416p up to 2160p) and Fullscreen. Widescreen shows more of the world at the sides while the HUD and menus keep their shape. F11 also switches fullscreen.
- Fullscreen and maximised windows now have black bars, and the menus no longer break in them.
- Loading a save on Kerwan brings back the enemies of later missions, and Helga no longer offers the Swingshot again once you have it.
- Reaching the info bot on Kerwan's train no longer throws Ratchet off the map.
- The Qwark bot on Kerwan no longer repeats its cutscene.
- A thrown wrench brings back the bolts and ammo it hits, as in the original.
- The bomb glove no longer floats in the air during cutscenes, at the vendor or when boarding the ship, and Clank's antenna glow no longer floats in cutscenes.
- The ship's canopy shows during take-off and landing.
- The planet selection screen shows the button hints and your gold bolt count.
- Enemies and objects that could turn invisible while still active now show.

## v0.2.0-alpha

The first test build of ReRAC. It's an early alpha: every planet is in, from Veldin to Drek's fleet, but only the first few have been played through properly, so expect rough edges, missing details and the odd crash further on.

- Made for Macs with Apple Silicon. There's also a Windows build, but nobody has tried it yet, so it may not even start; please tell us how it goes. Linux comes later.
- You need your own copy of the game (the NTSC-U disc, SCUS-97199). No game files are included; the ReRAC launcher sets the game up from your disc.
- Veldin, Novalis, Aridia and Kerwan are complete. The other planets have all their enemies, machines and bosses, and are waiting for a full play-through.
- The main menu, saving and loading, flying between planets, the Gadgetron vendor, and cutscenes with their sound and music all work.
- If something goes wrong, please report it with the log the launcher keeps.
