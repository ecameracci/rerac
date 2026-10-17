# Mods (design notes, last stage)

Status: **Stage 3a texture overrides are built** (§2.1b); the rest is design. Mods are the last piece (user, 2026-09-26). They must be real mods: assets,
functionality and gameplay. This page records how OpenGOAL does it, our options, a recommendation and open
questions. The data tiers and the launcher it builds on are in `docs/plan/launcher_extractor.md`.

## 1. How OpenGOAL does it
Sources: `~/Globals/jak-project` (ISC); `github.com/open-goal/launcher` (ISC) `src-tauri/src/commands/features/
{mods,texture_packs}.rs` and `schemas/`; jakmods.dev FAQ; `OpenGOAL-Mods/OG-Mod-Base`. Ideas only.

- **A mod is a fork of the whole `jak-project`.** Modders change GOAL source (`goal_src/`), PC assets
  (`game/assets/`, `custom_assets/`) and decompiler config, then build their own `gk` (runtime), `goalc` (compiler)
  and `extractor`.
  - **There is no scripting layer and no Lua.** GOAL itself, a compiled Lisp, is the modding language. The REPL
    (`goalc` listener) is used for live development.
  - The FAQ is explicit: each mod is a standalone copy, so **two mods cannot be combined**.
- **Distribution.** Per-platform GitHub release bundles (binaries + data folder). Some mods ship only some OSes.
- **Mod sources.** A **mod source** is a JSON index the user adds by URL (schema `mod-source/v1`):
  `{schemaVersion, sourceName, lastUpdated, mods: {id: {displayName, description, authors, tags, supportedGames,
  versions: [{version, publishedDate, assets: {windows|linux|macos: url}}], coverArtUrl, …}}, texturePacks: {…}}`.
- **Install.** Each mod lives in `features/<game>/mods/<source>/<mod>/`, with its own `_settings` and saves.
  - The launcher **reuses the base game's already extracted `iso_data`**, then runs that mod's own `extractor
    --decompile` and `--compile` (`extract_iso_for_mod_install`, `decompile_for_mod_install`,
    `compile_for_mod_install`). No disc is needed.
  - "Enable" means "launch this install". There is no layering.
- **Texture packs** are the one composable mod type:
  - A zip with `custom_assets/<game>/texture_replacements/**.png` (paths mirror the game's texture pages), plus
    optional `cover.png` and `metadata.json` (schema `texture-packs/v1`: name, version, authors, supportedGames,
    tags).
  - The launcher keeps an **ordered enabled list**. It clears the replacement dir and copies packs so that the
    **first in the list wins**, then re-runs the decompiler, which bakes the PNGs into the converted data. Changing
    packs means a rebuild, not a live toggle.
  - Documented limits: no sky, eye or animated textures; PNGs need alpha.
- **Custom content without a fork of the engine logic.** `custom_assets/<game>/levels/<name>/<name>.jsonc` + `.glb`
  → `build-custom-level` (background mesh, collision, actor placements as res-lump tags). `build-actor` builds
  actors from glb; `merc_replacements/*.glb` replace models. These run inside the build, so they still live in a
  jak-project tree.

Takeaways:
- The composable parts are assets that are baked at rebuild time, with an ordered list.
- Functionality mods are forks and do not compose.
- The disc is extracted once, and every mod install reuses the extraction.

## 2. Options for us

### 2.1 Assets (textures, models, audio, text)
- **Override layer in `rc-data`** (the VFS, P3.1). Each mod provides files at stable asset paths in Tier-2-style
  formats: `textures/levels/01/tfrag/0123.png`, `audio/levels/01/music/003.wav`, `text/en.json`,
  `models/mobys/0577.glb`.
- Profiles hold an **ordered** list of enabled mods; the first match wins, like OpenGOAL.
- Importers bake the overrides into a **per-profile Tier 1 overlay cache**. The base cache is never modified.
- Toggling a mod rebuilds only the affected levels' overlay sections, in seconds, with no disc.
- Renderer and mixer needs:
  - an RGBA texture path next to the paletted one;
  - a PCM sample kind next to ADPCM;
  - a mesh import path (glTF → our moby/tie layout) for new models. This is the hard part, as with OpenGOAL's merc
    replacements: skins, LODs and GS state must be synthesised.

### 2.1a From Tier 2 exports to overrides (the exports are built, 2026-09-26)
`rerac-extract export` (docs/plan/launcher_extractor.md §5.5) writes the formats and the paths an asset mod
will ship. The rule: **a mod overrides an asset by shipping a file at the same relative path as its export**, and
the importer uses the export's sidecar (or extras) to rebuild what the standard format cannot hold.

| Export | Override | What the importer restores from the sidecar / extras |
|---|---|---|
| `textures/levels/NN/<table>/<key>.png` (indexed PNG) | same path, indexed or RGBA PNG | CLUT as stored (`clut_csm1`: CSM1 order, raw alpha 0..0x80), table entry (`ty` = mip count, gs_ram blocks), GS TEX0 format. An indexed PNG with an unchanged palette re-imports byte-exactly; an RGBA PNG goes through the renderer's RGBA path (quantising back to 256 colours is optional). Mips: `.mipK.png` if shipped, else generated. |
| `audio/levels/NN/sound_bank/NNN.wav` | same path, WAV | Sample rate, loop (`smpl` chunk or sidecar); the bank index (`sound_bank.json`) keeps which sounds and tones play it. Mods add PCM samples as a second sample kind (not re-encoded to ADPCM). |
| `audio/levels/NN/{music,speech}/*.wav` | same path | Rate and loop from the sidecar / `smpl`; the level's music table in `music.json`. |
| `text/levels/NN/<lang>.json`, `text/global/all_text/<lang>.json` | keyed patch by message `id` | `text` uses the `\xNN` escapes of `strings::display` for control codes and accented letters; `help_audio` is kept. |
| `models/levels/NN/mobys/CCCC.gltf` | same path (glTF/GLB) | Joint palette convention (identity inverse binds, one node per palette joint, extra `static` joint), class header, texture slots (`class_textures`), skeleton rows and parents, sequence extras (rate override, triggers). New skins/LODs/GS state are synthesised (the hard part, §2.1). |
| `levels/NN/level.gltf` (tfrag, ties, shrubs) | later: per-class tie/shrub meshes, tfrag chunks | `_PS2_*` attributes (raw RGBA, tie light/morph slots, morph deltas), LOD scenes and distances, ad-gif registers, instance colours. |
| `levels/NN/*.json` (mobys, ties, shrubs, volumes, paths, …) | keyed JSON patches (§2.2) | Every stored field is exported, so a patch names an instance by `index` (and `uid` for ties) and changes fields. |
| `levels/NN/collision.gltf` | later | Surface byte per primitive; the 4-unit cell grid is rebuilt by the importer. |

The exports are a starting point for mod authors: they are complete for the textures, audio, text and tables,
and lossy only where §5.5 of the extractor plan says so (lighting not baked, LOD morphs not animated).

### 2.1b What is built (texture overrides, 2026-10)
- **Mods folder** (`crates/rc-engine/src/mods.rs`): `mods/<folder>/mod.toml` + `assets/` beside the executable
  (`RC_MODS_DIR` overrides it). The settings file's `mods = <id>, …` holds the enabled set; the main menu's
  Options → Mods page toggles it. The first mod in folder-name order that has a file wins.
- **HUD frames and FX textures** (fonts, icons, the logo; `crates/rc-engine/src/hud_render.rs`): a PNG at the
  export's path at a whole scale (1..=8). Applied live: a toggle rebuilds the HUD atlas.
- **World, moby, shrub, billboard and sky textures** (`crates/rc-engine/src/mod_textures.rs`):
  `assets/textures/hash/<key>.dds|png`, `key` = XXH3-64 of the decoded RGBA8 (alpha 0x80 → 0xff) in 16 hex digits
  + `_<w>x<h>`. The same key holds in every level. DDS (BC3 or 32-bit, PCSX2 pack alpha 0..0x80) or PNG (export
  alpha), at a power-of-two scale up to 16. The world shaders shift the GS mip level by the replacement's texel
  density at the frame's height. Applied at the next level load.
- **Not yet:** streamed pictures (`hud_images`: maps, backgrounds), environment / metal maps, particles, audio,
  text, models, data patches.
- **A first step, simpler than §2.1 and §3:** overrides are read from the mod folders at load time in `rc-engine`,
  with no `rc-data` overlay or per-profile cache; one enabled set, in folder-name order, not ordered profiles; the
  in-game Mods page, no launcher tab; `mod.toml` reads only `id`, `name`, `version`, `description`. World textures
  are keyed by content hash rather than §2.1a's export paths, so one file covers a texture in every level and PCSX2
  packs convert; the export does not write those names yet. The rest of the design stands for the later stages.
- **PCSX2 packs** are keyed by GS texture and CLUT hash, not by our keys; converting one means matching each pack
  file to a decoded texture by CLUT hash, size and content (a one-off tool, kept out of the repo with the pack).

### 2.2 Data (gameplay content)
- Level content as editable data exported to JSON (Tier 2) and merged back as patches: moby instances and pvars,
  paths, volumes, cameras, level settings, item/vendor tables, planet list, text.
- ELF/overlay tables are read by virtual address today. For data mods, each table that mods should touch gets
  **lifted** into typed Tier 1 data once, per system; the game code then reads the typed data.
- Patches should be **keyed edits** (by instance uid or table key), not whole-file replacements, so several mods
  compose. Conflicts are reported by the launcher.

### 2.3 Functionality (new behaviour)
| Option | Power | Composability | Safety / determinism | Build and runtime cost | Notes |
|---|---|---|---|---|---|
| **Forks** of our Rust tree (OpenGOAL model) | Total | None | n/a | Full build per mod; ships binaries | Always possible for the few who want it; our repo is private today |
| **Compiled Rust dylib plugins** | High | Medium | Unsafe: no stable Rust ABI, same compiler required, crashes take the game down | Low | Not recommended |
| **Lua** (mlua, vendored Lua 5.4 or **Luau**) | High through an API | Good (per-hook registration) | Sandboxable (Luau is designed for it); deterministic if no time/IO and our RNG only | C build once (~seconds); fast enough for per-tick class updates | Widely known to modders; MIT |
| **Rhai** | Medium | Good | Safe, pure Rust | Slower at run time; moderate compile | Rust-like syntax, small community |
| **WASM** (wasmi interpreter / wasmtime JIT) | High, any language | Good | Strong sandbox; deterministic apart from NaN bits | wasmi light; **wasmtime heavy to compile** | Best isolation; more friction for modders |

**Hook points** our native code already has, or will have:
- the moby class registry (`rc_game::moby_update::classes::for_class(o_class)` → `ClassUpdate`), which gains "mod
  class" entries for new or overridden `o_class` ids;
- item and gadget definitions (`save_game::ItemTables`, `hero::items`);
- spawn rules (`moby_spawn`);
- HUD and text strings;
- menus and planet list;
- sound triggers;
- level list and transitions.

Scripts would see a narrow API (world queries, spawn and kill, animation, sound, particles, the shared RNG,
pad state). They never see internals.

**Determinism.** Scripts run inside the 60 Hz tick at their slot's position in the update order and draw from the
same RNG stream. PCSX2 comparisons, golden tests and `RC_DETERMINISTIC` runs force mods off.

## 3. Recommendation
1. **Stage 3a:** asset override layers with ordered profiles, plus the launcher Mods tab. Install from a zip or
   folder; a mod-source JSON index is optional later. Package: `mod.toml` (id, name, version, authors, game `rc1`,
   supported builds, load-after hints) + `assets/` + `data/` + `scripts/`.
2. **Stage 3b:** data mods as keyed JSON patches, which requires lifting the relevant ELF/overlay tables.
3. **Stage 3c:** scripting for functionality. **Lean: Luau via `mlua`** (sandbox, speed, modder familiarity). It
   lives in a separate `rc-mods` crate behind an engine feature that the dev loop does not enable, so build times
   are untouched. WASM (wasmi) is the fallback if isolation matters more than ease.
4. **Forks** remain possible for total conversions, as with OpenGOAL, but they are not the supported path.
5. **"On the fly":** enable and disable in the launcher, applied at the next launch by an incremental overlay-cache
   rebuild. In-game live reload of asset mods is a later extra (Bevy asset hot reload).

## 4. Open questions for the user
1. Scripting language: Luau/Lua, Rhai, WASM, or none (data mods only)? **Lean Luau.** It is a new dependency and
   needs approval when Stage 3c starts.
2. Must mods compose, several at once with load order? That is our recommendation, unlike OpenGOAL. Or is one
   active mod at a time enough?
3. Distribution: local zips only, or mod-source JSON indexes like OpenGOAL (network access in the launcher)?
4. May mods override game logic of existing classes (replace a native `ClassUpdate`), or only add new classes and
   data?
5. Saves: separate save slots per mod profile (OpenGOAL keeps `_settings` per mod)? Recommended.
6. New-level mods (custom levels from glTF, like OpenGOAL's `build-custom-level`): in scope for v1 of mods, or
   later?
