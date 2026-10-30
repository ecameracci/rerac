# Launcher and full asset extractor (design plan)

Status: **decided 2026-09-26** (user's picks in §9; they override the original recommendations where they differ).
**Built:** Stage 1a, the Rust extractor `rerac-extract` (`crates/rc-extract`, §4.1), and Stage 1b, the engine
cut-over (§7: the engine reads only the data folder; `--data-dir`, `RC_DATA_DIR`, `--version-json`; settings under
`rerac`). **P1.5, the Tier 1 engine cache v1** (§5.4: `rerac-extract prepare`, `crates/rc-data`), is built too,
and so is **P1.7, the Tier 2 exports** (§5.5: `rerac-extract export`, PNG / WAV / glTF / JSON).
The launcher ↔ game interface is `docs/plan/launcher_contract.md`. Mods get a separate, shorter doc: `docs/plan/mods.md`.

**Names.** The project is **ReRAC** (renamed from randcrw on 2026-09-29). Everything user-facing says ReRAC, and the
technical identifier is `rerac` (binaries, data folders); the window title is "ReRAC". Nothing depends on the name of
the folder the repository is checked out in.

The requirement (user, verbatim): *"we need a system, just like openGOAL, where we have a launcher. In that launcher
we will have rc1 (and rc2 and 3 eventually). In the menu for rc1, the user needs to be prompted to browse to their
PERSONAL disc image copy, so that our app can EXTRACT everything and so that the disc image is NO LONGER NEEDED."*
Follow-up: the launcher and a full asset extractor come first. Engine-ready assets and "usable formats" are wanted.

## 1. Summary

- **Three data tiers.** OpenGOAL works this way too (§2.3).
  - **Tier 0, archive.** Every lump the ToC and the filesystem reference, byte-identical to the disc, in today's
    `extracted/` naming. It is the only tier that needs the disc, and it is written once. Its format must never
    change, so the disc is never needed again.
  - **Tier 1, engine cache.** Versioned and prepared for the engine: decompressed first, then baked products where
    they pay off. It is rebuilt from Tier 0 at any time, with no disc.
  - **Tier 2, exports.** Optional "usable formats" (PNG, WAV, glTF, JSON) for people and for mod authors. The
    engine never reads them.
- **The engine reads only a data directory.** The disc reader (`rc_formats::disc`, `iso9660`) becomes
  extractor-only. Today the engine opens the ISO itself (`crates/rc-engine/src/disc_source.rs`).
- **Launcher (decided: U3/U4).** A **Tauri** app in a **separate repo**, `rerac-launcher`, OpenGOAL-style: launcher
  and game builds are versioned separately, mods are separate game builds (forks), and every game build ships its own
  extractor. The launcher drives the game build only through `docs/plan/launcher_contract.md` (manifest, extractor CLI
  with JSON lines, runtime flags, folders). Official downloads stay off until the repo is public; a Development source
  uses local builds.
- **Disc verification (built).** Identify the disc by `SYSTEM.CNF` serial plus boot-ELF SHA-1, the way OpenGOAL uses
  serial plus ELF hash. Every file is checked against a committed size + SHA-1 table while it is extracted, and again by
  `verify`. Only NTSC-U SCUS_971.99 v1.00 is supported; other RC1 builds and the sequels are identified and refused
  (code 21), with a ready-to-paste build-DB row for unknown RC1 builds. The DB stays extensible (§4.1).
- **Disc image formats (U11):** plain 2048-byte `.iso` only for now; no `.bin`, CHD or CSO.
- **Build order.** Stage 1: Rust extractor, data directory, engine cut-over. Stage 2: launcher. Stage 3: mods.
  Packages, ownership and acceptance criteria are in §8.

Surprises found while surveying (details in §3):
1. **"ISO mode" is not ISO-only today.** `disc_source::IsoSource::read` serves only `toc.bin`, `boot/*` and the 11
   level-group members. Everything else silently falls back to `extracted/`: music, speech, scene regions,
   `global/spaceships`, `global/save_game`. On a machine with only the ISO, the engine loses level music, cutscenes,
   speech, the ship and the save template.
2. **No global-lump extraction in Rust** (fixed by Stage 1a: `Disc::global_lumps`, `Disc::archive_files`). Only the
   C++ `rc_extract unpack` wrote `extracted/global/*` (229 MiB without the movies, plus 2.8 GiB of movies).
3. **The game logic reads about 40 data tables by virtual address** from the boot ELF and the level overlay ELFs:
   menus, quick select, item tables, water constants, glyph tables, normal table, save chunk tables, planet names.
   The ELF and the 19 overlays must stay in the archive. They are RC1's equivalent of OpenGOAL's decompiled data.
4. **One level load decompresses `core_data` 5 to 8 times** (`level_load`, `fog_state`, `menu_render`, `gameplay`
   twice, `moby_attach`, `moby_spawn`), and `gameplay_ntsc` 4 times. Each `core_data` decompress takes about
   40 ms (level_sweep.md). A decompressed cache (Tier 1 v1) removes roughly 150–300 ms per load. **Measured (P1.5,
   §5.4):** 8 / 6 / 6 `core_data` and 4 `gameplay_ntsc` decompressions per load on levels 01 / 05 / 16 before; one
   cache read per lump after, and the load up to the window drops from 285 / 196 / 192 ms to 147 / 100 / 106 ms.
5. **The NTSC disc carries PAL copies** of every FMV (1,333 MiB) and every scene region (85 MiB). Tier 0 is about
   4.0 GiB, the same as the 4.2 GB ISO. Dropping PAL-only data gives about 2.6 GiB (decision U2).
6. **About 15 test sites hard-code `CARGO_MANIFEST_DIR/../../extracted`** in `rc-game` and `rc-trace`.

## 2. OpenGOAL findings

Sources: local checkout `~/Globals/jak-project` (commit 76e8eeca, 2026-09-22, **ISC**, `LICENSE`), and
`github.com/open-goal/launcher` (**ISC**, Tauri 2 + Svelte 5 + Rust backend). We take **ideas only; no code is
copied.**

### 2.1 Extractor (`decompiler/extractor/main.cpp`, `extractor_util.cpp`)
- One CLI, `extractor <iso|folder> [-e extract] [-v validate] [-d decompile] [-c compile] [-p play] [-g game]
  [--proj-path] [--extract-path]`. With no step flags it runs every step.
- **Extraction.** It unpacks the ISO filesystem into `iso_data/_temp/`. It hashes the extracted contents (XXH64
  over the files) and counts them. It finds the ELF (serial from the filename) and hashes the ELF.
  `validate()` then looks these up in a compiled-in DB: `{serial → {elf_hash → ISOMetadata{canonical_name,
  region, num_files, contents_hash set, decomp_config_version, game_name, flags}}}`.
  - Unknown serial or ELF hash: it logs a ready-to-paste DB row (`log_potential_new_db_entry`) and returns a
    specific error code.
  - Known build: it moves `_temp` to `iso_data/<game>/` and writes `buildinfo.json` (serial, ELF hash).
  - The error codes (`ExtractorErrorCode`) are an interface: the launcher maps them to messages
    (`error-code-metadata.json`).
- **Multi-game.** `-g jak1|jak2|jak3` picks the `data_subfolders` entry and the per-version decompiler config
  (`decompiler/config/<game>/<ver>/`). Jak 1 NTSC v1 ("black label") and v2 differ only by a flag
  (`FLAG_JAK1_BLACK_LABEL`). Region-specific config sits under `ntsc_v1/`, `pal/`, `jp/`.
- **Directory roles.** `iso_data/<game>` holds raw disc files and is kept permanently. `decompiler_out/<game>`
  holds human-readable dumps and exports. `out/<game>/` holds runtime data.

### 2.2 What they convert, and what they keep raw
- **Converted.** Background geometry (tfrag, tie, shrub, hfrag), merc models and collision become their own
  **`.fr3`** format (`common/custom_data/Tfrag3Data.h`), zstd-compressed, one file per level in `out/<game>/fr3`.
  `TFRAG3_VERSION = 43` is written at the start and end of the file. Loading a mismatched version asserts with a
  "re-extract" message (`TFrag3Data.cpp` ~l.587). The runtime `Loader.cpp` reads, zstd-decompresses,
  deserializes and uploads on a loader thread.
- **Kept raw and relinked.** Art groups and texture pages are copied as the original object files
  (`copy-go`/`copy-textures` in `goal_src/jak1/game.gp`) and relinked into DGO/CGO by their compiler.
- **Copied verbatim.** Audio and streams (`.VAG`, `.SBK`, `.MUS`, `.STR`, `VAGDIR.AYB`, `SAVEGAME.ICO`) are copied
  into `out/<game>/iso/` (`copy-iso-file`). The runtime's "fake ISO" (`game/overlord/common/fake_iso.cpp`) serves
  them by 8.3 name, and their IOP/989snd reimplementation decodes the PS2 formats at run time. **They do not
  transcode audio.**
- **Optional exports**, off by default and exposed as launcher settings: `rip_levels` (glb), `rip_collision` (obj),
  `save_texture_pngs`, `rip_streamed_audio`, `rip_music` (`decompiler/config/jak1/jak1_config.jsonc`). This is our
  Tier 2.
- **Replacements are baked at rebuild time.** `custom_assets/<game>/texture_replacements/**.png` and
  `merc_replacements/*.glb` are applied while the decompiler runs (`decompilation_process.cpp:290`,
  `extract_level.cpp:119`). Changing a texture pack means re-running the decompile step from `iso_data`, with no disc.
- **Custom content.** `custom_assets/jak1/levels/<name>/<name>.jsonc` + `.glb` go through `build-custom-level`;
  actors go through `build-actor` from glb.

### 2.3 Launcher (`open-goal/launcher`, `src-tauri/src/commands/*.rs`)
- Tauri 2. The frontend is Svelte and restricted. The Rust backend does all file and process work. Long jobs run
  as a sequential job list with progress events.
- **Install layout.**
  - `<install>/versions/official/<ver>/`: downloaded release, binaries plus `data/`.
  - `<install>/active/<game>/data/`: copied data dir, with `iso_data/<game>`, `decompiler_out`, `out`,
    `custom_assets`.
  - `<install>/features/<game>/mods/<source>/<mod>/`: one install per mod, with its own `_settings`.
  - `<install>/features/<game>/texture-packs/<pack>/`.
  - User settings and saves: `~/.config/OpenGOAL/<game>/` (`common/util/FileUtil.cpp get_user_config_dir`).
    Launcher logs go to per-OS app dirs.
- **Flow.** Pick ISO → `extractor <iso> --extract --validate --proj-path <data> [--game]` → `--decompile` → `--compile`
  → launch `gk -v --proj-path <data> --game <g> -- -boot -fakeiso` (`binaries.rs`).
- **Updates.** It pulls OpenGOAL releases from GitHub per OS and keeps an "active version". A version change copies
  `data/` and re-runs decompile and compile from the kept `iso_data`. **The ISO is never needed again after the
  first extraction.** Mods also reuse `active/<game>/data/iso_data` (`extract_iso_for_mod_install`,
  `decompile_for_mod_install`).
- **Texture packs.** Each is a zip with `custom_assets/<game>/texture_replacements/` plus optional `cover.png` and
  `metadata.json` (schema `schemas/texture-packs/v1`). The launcher keeps an **ordered enabled list**. It clears
  `active/<game>/data/custom_assets/<game>/texture_replacements` and copies the packs in reverse order, so the first
  pack in the list wins (`update_texture_pack_data`), then triggers a decompile.

### 2.4 What we adopt, and what we do differently
| Adopt | Ours |
|---|---|
| Raw disc data kept forever; runtime data rebuilt from it | Tier 0 archive → Tier 1 cache |
| Serial + ELF hash build DB, printed "new DB row" for unknown builds, numbered error codes | `rc_formats::build_db` + `ExtractError` codes (P1.1/P1.2) |
| Versioned converted format that refuses stale data | Tier 1 stamp with format version, converter version and archive manifest hash; the engine rebuilds or refuses |
| Optional exports as settings | Tier 2 `export` command and launcher toggles |
| Ordered overlay lists for replacements | Mod profiles (mods.md) |
| Audio stays in PS2 formats, decoded natively | VAG stays ADPCM; our `rc-game` decoder already exists |

| Differ | Why |
|---|---|
| No decompile/compile step. The game logic is native Rust, compiled into the binary | R&C is C++; decisions.md "No GOAL layer" |
| Launcher in Rust/egui, not Tauri/Svelte (recommended; U3) | No Node toolchain; one language; the core crate keeps Tauri possible |
| Hash manifest per lump, not only a whole-contents hash | Lets `verify` name the corrupt file and repair only that file |
| No downloaded game binaries per version (v1) | The repo is private; distribution is out of scope for now |

## 3. Current state (2026-09-26)

### 3.1 Readers
- `crates/rc-formats/src/iso9660.rs` reads `.iso` (2048-byte sectors) and raw `.bin` (2352-byte sectors, mode 1 or
  mode 2 form 1). It seeks only to the bytes it needs.
- `crates/rc-formats/src/disc.rs` is `Disc`. It reads the ToC at sector 1500, the level headers, the boot ELF via
  `SYSTEM.CNF`, and `level(id) → LevelFiles` (11 level-group members). It also provides `level_stream_lumps`
  (music, bindata, speech, scene), `read_lump`, `scene_region`, `scene_speech` and `save_game_lump`.
  `cargo run --release -p rc-trace -- disc-check` (check 1, by hand: no test reads the disc image) proves the
  level-group bytes identical to the Tier 0 archive. **Added by Stage 1a:** `RAC1_GLOBAL_FIELDS` + `Disc::global_lumps` (port of the retired C++ reader's
  `rac1_global_fields`/`global_lumps`, including the duplicate-name suffix rule) and `Disc::archive_files` (the whole Tier 0 file
  plan as byte ranges of the image). `IsoImage::volume_sectors` exposes the PVD size (truncation check). Build
  identification lives in `crates/rc-extract` (§4.1).
- `crates/rc-formats/src/toc.rs` has the level header, `SceneRecord`, `level_stream_lumps`, `probe_lump_size` and
  `global_sector_range`.
- The C++ `tools/extract` (`rc_extract info|ls|toc|unpack|<dump cmds>`) was the oracle until it was retired on
  2026-09-27 (decisions.md). Its `unpack` wrote the `extracted/` raw layout that Tier 0 keeps, plus a `.dec` next to
  every WAD lump; the golden tests now check committed snapshot hashes (`crates/rc-formats/data/loader_snapshots.tsv`)
  and `rerac-extract` the committed SHA-1 table.

### 3.2 Engine data access
*(As before Stage 1b; the cut-over is described in §7.)*
- Every engine read goes through `crates/rc-engine/src/disc_source.rs` with extractor-relative names:
  `read(root, rel)`, `level_file(root, NN, name)`, `read_path`. The root is `level_load::extracted_root()`:
  `RC_EXTRACTED`, else `<workspace>/extracted`.
- `RC_SOURCE=iso|extracted` and `RC_ISO` choose the source. The default ISO path is
  `~/PS2/ratchet1/Ratchet & Clank (USA) (En,Fr,De,Es,It).iso`. In ISO mode only `toc.bin`, `boot/*` and the level
  group come from the image (surprise 1).
- Call sites (16 files): `level_load`, `fog_state`, `sky_stars`, `water_render`, `menu_render`, `scene_render`
  (level_header, `scene/KK_ntsc`, `speech/KK_<lang>`, `global/spaceships`), `moby_light`, `tfrag_light` (boot
  ELF), `moby_attach`, `audio_out` (sound_bank, level_header, `music/NNN`), `moby_spawn` (`global/spaceships`),
  `moby_render`/`moby_lod` (tests), `hud_render`, `gameplay` (boot ELF, overlays of every level for the item tables,
  `global/save_game`), `shrub_render`.
- Settings: `render_settings::settings_path()`. It uses `RC_SETTINGS_FILE`, else
  `~/Library/Application Support/rerac/settings.toml` on macOS, `%APPDATA%\rerac\settings.toml` on Windows,
  and `$XDG_CONFIG_HOME/rerac/settings.toml` on Linux. It is disabled in deterministic mode.
- `rc-trace` resolves `RC_EXTRACTED`, else `<workspace>/extracted` (`tools/trace/src/lib.rs`), and reads
  files directly. The `rc-game` tests read `../../extracted/levels/01/...` directly and skip when absent.
- Other `RC_*` variables used by the engine are debug switches (see `docs/workflows/dev-switches.md`). Only `RC_ISO`,
  `RC_SOURCE` and `RC_EXTRACTED` concern data location.

### 3.3 The `extracted/` tree (dev machine)
| Part | Files | Size | Notes |
|---|---|---|---|
| Raw lumps (Tier 0 equivalent) | 2,937 | 4,014 MiB | levels 983, global 229, FMVs 2,801 |
| `.dec` copies, `*_dump.bin` goldens, png/obj previews, `core/`, `gameplay/`, `textures/` splits, `vu/` (written by the retired C++ extractor) | many | ~5 GiB | derived dev data; not part of Tier 0; nothing reads the C++-written files since 2026-09-27. Since the repo reorg (2026-09-27) the PCSX2 `traces/` moved out (dumps to `work/trace/`, savestates to `~/PS2/ratchet1/savestates/`), `overlay.elf` and `vu/` were copied to `work/ghidra-import/levelNN.elf` and `work/vu/`; regenerating `extracted/` (docs/workflows/game-data.md) drops the rest |

## 4. Disc inventory (NTSC-U SCUS_971.99 v1.00)

Sizes are from `extracted/`. "Loader" is the current Rust status.

| Data | Where on disc | Size | Loader (Rust) | Conversion needed | Engine-ready form (Tier 1) |
|---|---|---|---|---|---|
| Boot ELF `SCUS_971.99`, `SYSTEM.CNF` | ISO9660 FS | 1.35 MB | `Disc::boot_elf`; tables read by VA (normal table 0x165500, save chunks 0x1a04c0/0x1a07c0, vendor/item records 0x1dfd98/0x1dffb0) | no | Raw image; later, lifted typed tables |
| `IOPRP243.IMG`, `irx` lump | FS / ToC 0x12c0 | 0.3 MB (WAD) | none | no (IOP modules; not used natively) | Archive only |
| ToC | sector 1500 | 0x2960 B | `Disc::toc` | no | Raw |
| Level header ×19 | ToC level table | 0x2434 B each | `toc::parse_level_header` | no | Raw |
| Level overlay ELF ×19 (`overlay.bin`) | level data container | ~1.7 MB each | font glyph tables, menus, quick select, items (`ItemTables::load`), water tables, planet names | no | Raw image; later, lifted tables |
| `core_index` + `core_data` (textures, tfrag, tie, shrub, sky, mobys, anim, collision, occlusion, particle/FX) ×19 | data container | ~13 MB WAD / 23 MB decompressed each | all golden-tested | decompress | `.dec` (v1); baked GPU-ready products later (§5.3) |
| `gs_ram` ×19 | data container | ~0.8 MB | env maps, shrub/moby textures | no | Raw |
| `sound_bank` ×19, global `sound_bank` | data container / ToC 0x14e0 | ~1.6 MB / 0.15 MB | `sound_bank.rs`, `vag.rs` | no | Raw (ADPCM) |
| `hud_header` + `hud_bank_0..4` ×19, global HUD | data container / ToC 0x14f8, 0x1500 | ~2 MB | `hud.rs` | decompress | `.dec` |
| `gameplay_ntsc` / `gameplay_pal` ×19 | sector ranges | 0.5 MB WAD / 1.4 MB each | `gameplay.rs`, `moby_spawn.rs`, `volumes.rs` | decompress | `.dec` (NTSC); PAL archived |
| `occlusion` ×19 | sector range | ~0.5 MB | `occlusion.rs` | no | Raw |
| Music (15 VAG streams per level) | level header 0x148 | 3–10 MB per level | `audio.rs` music | no | Raw ADPCM |
| Speech (per scene × 5 languages) | scene records | 0–31 MB per level | `scene_render` speech | no | Raw ADPCM |
| Scenes (in-engine cutscenes, NTSC + PAL chunk WADs) | scene records | 0–17 MB per level | `scene.rs` | decompress per chunk | Decompressed chunk table |
| `bindata` (36 per level; only level 06 non-empty, 5.4 MB) | level header 0x28 | 5.4 MB | none (semantics unknown) | unknown | Archive only until understood |
| `ratchet_seqs` (28), `hud_seqs` (20) | ToC 0x18 / 0xf8 | 7.6 / 0.8 MB | Ratchet animations used by `rc-trace` | decompress | `.dec` |
| `spaceships` (4) | ToC 0x12c8 | 1.3 MB | `moby_spawn`, `scene_render` | no | Raw |
| `save_game` (memory-card icons + blank template) | ToC 0x10 | 92 KB | `save_game.rs` | no | Raw |
| `all_text` (all localised UI text) | ToC 0x1528 | 0.6 MB | `strings.rs` (level text in flight) | no | Raw; Tier 2 JSON |
| `debug_font` | ToC 0x08 | 20 KB | none | no | Raw |
| `vendor` + `vendor_audio` (37 VAG) | ToC 0x198 / 0x1a0 | 0.35 + 1.3 MB | none (vendor screen queued) | decompress | `.dec` + raw ADPCM |
| `frontbin` ("ratchet executable" overlay) | ToC 0x4f8 | 0.9 MB | none | no | Raw image |
| Menu images: `help_*`, `options_ss`, `mission_ss`, `planets`, `goodies_images`, `character_*`, `skill_images`, `epilogue_*`, `sketchbook`, `commercials`, `item_images` | ToC 0x2c8–0xeb8 | ~40 MB | none (menus/goodies) | decompress 8bpp textures | Decoded paletted textures; Tier 2 PNG |
| `space_plates`, `transition`, `space_audio` (36 VAG) | ToC 0x1388 / 0x13b8 / 0x13c0 | 3.6 / 1.0 / 8.1 MB | none (ship/space mode) | decompress | `.dec` + ADPCM |
| `help_audio` (900 VAG), `qwark_boss_audio` (240 VAG), `post_credits_audio` (18) | ToC 0x1ab8 / 0xf00 / 0x1618 | 122 / 15 / 4.3 MB | none | no | Raw ADPCM |
| `credits_images_ntsc` / `_pal` (raw RGBA) | ToC 0x16a8 / 0x1748 | 16 / 18 MB | none | no | Raw; Tier 2 PNG |
| `post_credits_helpdesk_girl_seq` | ToC 0x1610 | 92 KB | none | decompress | `.dec` |
| Unknown lumps `stuff2` (0x820), `anim_looking_thing_2` (0x12e8), `wad_14e0` (0x14e8), `things` (0x1530), `wad_things` (0x17e8) | ToC | 30 / 5 / 4 / 2 / 12 MB | none (cutscenes doc: 0x12e8/0x1530/transition are space scenes) | decompress where WAD | Archive; typed later |
| FMVs `mpegs[88]` (PSS: MPEG-2 512×416 30 fps + SShd ADPCM stereo 48 kHz) | ToC 0x17f8 | 2,801 MiB (PAL-only 1,333) | played natively at runtime (U10 decided: `rc-video`) | none: the raw PSS is what the engine reads | – |
| PS2 logo (sectors 0–11) | — | 24 KB | none | no | Not needed |

**Coverage.** Nobody has checked whether sectors exist that the ToC and the filesystem do not reference. P1.2
includes a coverage audit that reports unreferenced non-zero sector runs.

### 4.1 Tier 0 archive as built (Stage 1a, `crates/rc-extract`)

`rerac-extract` (binary of `crates/rc-extract`; CLI and codes in `docs/plan/launcher_contract.md`) writes the
Tier 0 archive **directly into the data folder** it is given (`--out`, the launcher's `games/rac1/data/`), in exactly
the raw layout the retired C++ `rc_extract unpack` wrote to `extracted/`:

```
<data>/extract-info.json          written last; its absence = incomplete folder
<data>/boot/SCUS_971.99           boot ELF (every ISO 9660 file goes to boot/<NAME>)
<data>/boot/SYSTEM.CNF, boot/IOPRP243.IMG
<data>/toc.bin                    0x2960 bytes at sector 1500
<data>/global/<field>.bin         count-1 fields (save_game, all_text, music, …)
<data>/global/<field>/NNN.bin     array fields (mpegs/, help_audio/, spaceships/, …)
<data>/levels/NN/<member>.bin     level_header, overlay, sound_bank, core_index, gs_ram, hud_header, hud_bank_0..4,
                                  core_data, gameplay_ntsc, gameplay_pal, occlusion
<data>/levels/NN/{bindata,music,speech,scene}/*.bin
```

- **Scope.** 2,937 files, 4,209,359,410 bytes (4,014.4 MiB) on SCUS_971.99. These are exactly the `.bin` raw lumps
  and boot files of `extracted/` and nothing else: no `.dec`, `core/`/`gameplay/` splits, `overlay.elf`/`.txt`,
  dumps or previews (all derived; Tier 1/2 or dev-only). The golden test proves both directions (same file set,
  byte-identical). Everything the port reads is in it: the boot ELF and the 19 overlays (`overlay.bin`), the save
  template (`global/save_game.bin`), the movies (`global/mpegs/`, raw PSS as on disc, U10), music, speech, scenes,
  `global/spaceships/`. Not archived: the PS2 logo sectors 0–11 (not needed) and any sectors neither the ToC nor
  the filesystem reference (the coverage audit is still open, §8 P1.2).
- **`--ntsc-only`** (U2 option) skips 213 PAL copies (1,445 MiB): every `levels/NN/gameplay_pal.bin`,
  `levels/NN/scene/KK_pal.bin`, `global/credits_images_pal/*` and the PAL FMVs `global/mpegs/` 021–039, 052–062,
  075–079, 084–087 (cutscenes_transitions.md §5). Result: 2,724 files, 2,569 MiB.
- **Build DB** (`crates/rc-extract/src/build_db.rs`): `TITLES` (every RC1/RC2/RC3/Deadlocked serial from
  disc_layout.md §2.6, so any R&C disc is named) and `BUILDS` (serial + boot-ELF SHA-1 + VER + region + ELF size +
  reference sector count + supported flag + Tier 0 table). Unknown serials fall back to the ELF string search
  (§2.6). An unknown RC1 build prints `new build DB row: Build { … }` to paste in.
- **Size/SHA-1 table** (U7): `crates/rc-extract/data/scus_971_99.tsv`, 2,937 rows `path<TAB>size<TAB>sha1`, hashes
  and sizes only (a unit test checks every line has that shape). Regenerate with `rerac-extract table --iso <image>
  --output crates/rc-extract/data/scus_971_99.tsv`; `cargo run --release -p rc-trace -- disc-check`
  (check 3) checks it. Cross-checked against `extracted/` with the system `shasum`.
- **Speed** (M-series, release build, 4 workers, std threads only): identify 3 ms; full extract 3.1–4.2 s warm
  cache, 7.9 s with a cold image cache (~500–1,300 MiB/s); `--ntsc-only` 3.2 s; `verify` 1.9 s.
- **Crash safety**: files are written as `*.partial` and renamed once hash-checked; `extract-info.json` is removed at
  start and written last; a re-run removes stale `.partial` files. The launcher cancels by killing the process.
- **Dependency-free**: in-crate SHA-1 (FIPS vectors tested) and flat JSON; free space via the platform C library
  (`statfs`/`statvfs`/`GetDiskFreeSpaceExW`). Only depends on `rc-formats`.

## 5. Formats: raw vs converted

### 5.1 Options
| | (a) Raw lumps, parse at run time (today) | (b) Convert everything once to engine-native formats (glTF/KTX2/Opus/video) | (c) Hybrid: raw archive + versioned engine cache + optional exports |
|---|---|---|---|
| Fidelity | Exact. Parsers are golden-tested | **Lossy by default.** glTF/KTX2 have no slot for CLUT palettes, PS2 0x80 alpha, the GS pass and AREF per batch, tfrag/tie LOD morph deltas, strip/ADC structure, the tie per-instance colour/light inputs, or the PS2-float lighting inputs. Opus loses ADPCM loop flags and bit-exact decode. Keeping them needs custom extensions: a second, unverified schema | Exact. Tier 1 serializes **the output of the same golden-tested parsers**, so there is no second parser |
| Golden tests vs C++ oracle | Direct | Broken. A new conversion layer needs its own verification | Tier 0 = C++ bytes; Tier 1 = round-trip tests `parse(Tier0) == load(Tier1)` |
| Dev loop | No extra step; repeated decompress (~40 ms each) | Converter must run after every loader change | Lazy per-level cache build in dev (first load only); no rebuild of Bevy |
| Load time | 70–90 ms load + repeat decompressions | Fast | Fast; improves step by step |
| Disk | 4.0 GiB | Smaller if lossy | 4.0 GiB + ~1 GiB cache (decompressed levels ~0.5 GiB) |
| Moddability | Poor (PS2 formats) | Good | Good: mods ship Tier-2-style files; importers bake them into Tier 1 (mods.md) |
| Re-extraction on converter change | n/a | Needs disc, or a kept raw copy | **Never needs the disc**: rebuild Tier 1 from Tier 0 |
| RC2/RC3 | Per-game readers | Per-game converters | Per-game Tier 0 readers; the Tier 1 container and tooling are shared |

### 5.2 Recommendation: (c), the hybrid
- **Tier 0 archive** = today's raw layout, byte-identical to the C++ `rc_extract unpack` `.bin` files (no `.dec`,
  no dumps; since the C++ side retired, checked against the committed SHA-1 table). WAD lumps stay compressed exactly as on disc. This keeps every current golden test valid, and it means
  the archive format has no reason to ever change.
- **Tier 1 cache** = `cache/v<N>/`, written by the extractor's `prepare` step and readable only through `rc-data`:
  - **v1 (Stage 1):** decompressed WADs (`core_data`, `gameplay_ntsc`, HUD banks, scene chunks, global WAD lumps)
    plus a stamp. The engine keeps using the existing parsers.
  - **v2+ (later, per renderer, only when profiling shows a load or parse cost):** baked products. Examples: parsed
    tfrag/tie/shrub/moby geometry as `bytemuck` Pod arrays in GPU-upload layout, with LOD morph data, GS pass/AREF
    per batch and PS2 colour inputs kept; textures decoded to RGBA8 with PS2 alpha kept in 0..0x80, plus the
    original indices and CLUT; audio indices over the raw ADPCM.
  - Every baked kind gets a round-trip test against the parser for all 19 levels.
  - **Container:** our own `RCPK` file (magic, container version, per-section `{name, kind, kind_version, offset,
    len, hash}`, 16-byte aligned so it can be memory-mapped for zero-copy). No new dependencies: `bytemuck` is
    already used. Optional per-section zstd via `ruzstd`, which the workspace already uses in `rc-trace`. Default
    off: disk is cheap and memory-mapped raw is faster.
  - **Staleness:** the stamp holds the cache version, per-kind converter versions and the Tier 0 manifest hash.
    Mismatches work like OpenGOAL's `TFRAG3_VERSION`, except the engine **rebuilds the affected level lazily** in
    dev builds instead of asserting, and the launcher prebuilds everything at install.
- **Tier 2 exports** (`rc-extract export …`, launcher toggles like OpenGOAL's `rip_*`): textures and menu images to
  PNG, VAG to WAV, `all_text`/level strings to UTF-8 JSON, level geometry (LOD0, vertex colours, textures) and later
  mobys (skin + animations) to glTF, gameplay instances to JSON, FMVs to MP4 if the user has ffmpeg. These are the
  "actually usable formats" the user asked for, and the formats mods ship in.

### 5.3 Per-kind notes that constrain conversion
- **Textures:** the renderer depends on the palette index/CLUT form, PS2 alpha (0x80 = 1.0), mip chains and GS
  TEX0/ALPHA state per batch (`gs_state.rs`). A mod replacement is RGBA; the renderer needs an RGBA path next to the
  paletted one (Stage 3).
- **Geometry:** tfrag/tie LOD morph, shrub billboards, and the moby skin palette and low-LOD joint rule must survive.
  glTF is export-only.
- **Lighting:** the load-time tfrag/tie/shrub/moby lighting uses the PS2 float model (`tfrag_light::ps2`). Baking
  its output into Tier 1 is allowed, because the output is deterministic data. Keep `RC_NO_LIGHT` working by storing
  both the stored and the lit colours.
- **Audio:** keep ADPCM. The loop and end flags live in the frames, the mixer decodes natively, and it is 3.5×
  smaller than PCM. Mods add PCM samples as a second sample kind; they are not re-encoded to ADPCM.
- **ELF/overlays:** keep them as images read by VA. Lifting each consumed table into typed Tier 1 data is a later,
  per-system job. It helps data mods (mods.md).
- **FMV:** decided (U10, 2026-09-27): no Tier 1 form. The engine demuxes the raw PSS itself at play time
  (`rc_formats::pss`) and decodes it natively (`rc-video`); nothing is converted at extraction.

### 5.4 Tier 1 as built (v1, P1.5, 2026-09-26)
- **Code.** `crates/rc-data` (no Bevy, no external crates; depends on `rc-formats` only) holds the cache format and
  the engine-side store. `rerac-extract prepare` (`crates/rc-extract/src/prepare.rs`) builds the cache with the
  same code. A crate rather than an engine module, so the extractor does not link the engine and the cache tests
  build in seconds.
- **Content.** Every WAD lump the engine used to decompress on each load: per level `core_data`, `gameplay_ntsc`,
  `hud_bank_0..4` (133 lumps, 438 MiB of decompressed data, 439 MiB on disk for the full disc). The bytes are exactly
  `rc_formats::wad::decompress` of the Tier 0 file: nothing is converted, so no PS2 detail can be lost; the
  golden-tested parsers still run on them. Not in v1: WAD streams decompressed inside parsers (63 gadget classes per
  load, about 5 ms, parsed once per level in memory by `rc_data::level_gadgets` since G-EXT-002; scene chunks once
  when a scene starts) and `gameplay_pal` (the NTSC game never reads it).
- **Layout** (a kind's files mirror their Tier 0 path under `<kind>/`; E2 exports can mirror the same way):
  ```
  <data>/cache/v1/stamp.toml
  <data>/cache/v1/wad/levels/NN/core_data.lump
  <data>/cache/v1/wad/levels/NN/gameplay_ntsc.lump
  <data>/cache/v1/wad/levels/NN/hud_bank_B.lump
  ```
- **Lump file:** the decompressed bytes from offset 0 (memory-mappable, usable by tools as is), then a 48-byte
  little-endian trailer: magic `RCWLUMP1`, kind version (u32), reserved (u32 0), payload length, payload XXH64,
  source length, source XXH64 (u64 each). XXH64 is in-crate (reference vectors tested); checking a 23 MiB core
  costs about 5 ms, against 40 ms to decompress it.
- **Stamp** (`stamp.toml`, `key = value` lines, no dependency): `cache_version = 1`, `game = "rac1"`,
  `tier0_manifest = "extract-info.json:xxh64:<hex>"` (or `"none"` for a development tree without that file),
  `kind.wad = 1`, and the informational `written_by`. Any mismatch of the first four makes the whole cache stale.
  The committed SHA-1 table was not used because the runtime does not carry it; `extract-info.json` names the disc,
  data format, extractor version and NTSC-only flag, and the table fixes the file contents for those.
- **Checks on every read:** trailer magic, kind version, payload length, payload XXH64 (corruption), Tier 0 source
  length (a swapped source). `prepare` also compares the source XXH64, since it reads the sources anyway.
- **Writes** are `<file>.<pid>.partial` + rename; `prepare` deletes leftover `.partial` files and other
  `cache/v<N>` folders.
- **Engine API** (`rc_data`): `level_core_data(root, NN)`, `level_gameplay(root, NN)`, `hud_bank(root, NN, B)` and
  `wad_lump(root, rel)` return `Arc<[u8]>`. Each lump is produced once per process (a mutex-guarded map), from
  memory, else the cache, else Tier 0; a lump built from Tier 0 is written to the cache with one log line
  (`rc-data: cached levels/01/core_data.bin (22.8 MiB; decompressed in 41 ms, written in 5 ms)`). A missing cache is
  created and a stale one emptied first (one line each); an unwritable folder switches to in-memory decompression
  (one line). `RC_CACHE=0` never touches the disk; `RC_PERF_LOG=1` prints every request; `rc_data::stats()` counts
  them. The engine builds lazily in every build type, not only in dev; the launcher prebuilds at install.
- **Call sites switched** (the decompress lines only): `level_load` (core data, gameplay), `tfrag_light`,
  `fog_state` (both), `moby_spawn` (gameplay, ground-probe core data), `hud_render` (banks), `menu_render`,
  `moby_attach::load_blobs` (twice per load: its own setup and `gameplay`), `gameplay` (collision blobs, joint
  lists). `LoadTimings::decompress` (the `WAD` figure in the load line) now measures the cache read.
- **Measured** (dev build, M-series; before/after built from the same tree, runs with `--data-dir` on a fresh
  extraction, `RC_SCENE=0`):

  | Level | `core_data` / `gameplay` decompressions per load | Load up to the window | Level loader (`load`) | Frame 60 reached |
  |---|---|---|---|---|
  | 01 | 8 / 4 → 0 / 0 (1 cache read each; 1 / 1 on a cold cache) | 285 → 147 ms | 77 → 36 ms | 2,120 → 1,751 ms |
  | 05 | 6 / 4 → 0 / 0 | 196 → 100 ms | 72 → 32 ms | 2,011 → 1,693 ms |
  | 16 | 6 / 4 → 0 / 0 | 192 → 106 ms | 73 → 36 ms | 1,949 → 1,710 ms |

  A cold cache (first start after deleting it) loads level 01 up to the window in 191 ms, writing its 5 lumps.
  `prepare` for all 19 levels takes 0.2 s (release, 4 workers); a full `extract` including it 4.7 s.
- **Fidelity checks:** `crates/rc-data/tests/data/roundtrip.rs` (all 133 lumps of the 19 levels: lazily built, re-read by
  a fresh store, `read_lump` and `prepare`'s check all equal `wad::decompress` of Tier 0, whose output the rc-formats golden test checks against the committed snapshot
  hashes);
  `tests/data/lifecycle.rs` (stale stamp, corrupt lump, unwritable cache, once per process); `RC_SCENE=0
  RC_SCREENSHOT_FRAME=300` on Novalis byte-identical before and after, with a warm and with a cold cache.
- **For v2+ (E2 and later):** add a kind (`kind.<name> = <version>` in the stamp, files under `<kind>/`), keep
  Tier 0 paths as the key, and add its round-trip test. Tier 2 exports are a separate tree (not under `cache/`).

### 5.5 Tier 2 as built (P1.7 / E2, 2026-09-26)
- **Command.** `rerac-extract export --out <data> [--to <dir>] [--what textures,audio,models,levels,collision,text|all]
  [--level NN] [--json]` (contract clarification 18). Reads only the Tier 0 archive, through the same golden-tested
  `rc-formats` loaders the engine uses; never the disc, never the Tier 1 cache. Default output `<data>/exports/`
  (kept outside `cache/`). The launcher's "Export assets…" runs it with a folder the user picks.
- **Code.** `crates/rc-extract/src/export/`: the encoders `png.rs` (CRC-32, Adler-32, zlib with one fixed-Huffman
  deflate block and greedy LZ77), `wav.rs` (RIFF PCM16 + `smpl` loop chunk), `gltf.rs` (accessor/buffer writer and a
  structural validator), `jsonv.rs` (nested JSON writer and reader); the exporters `textures.rs`, `audio.rs`,
  `geometry.rs` (level and collision glTF), `models.rs` (moby glTF), `tables.rs`, `text.rs`. No new dependency: the
  crate still depends on `rc-formats` and `rc-data` only. Jobs are (level, kind) pairs plus the global kinds, run on
  the extractor's worker pool (`--threads`).
- **Layout** (paths are the asset paths mod overrides will use, mods.md §2.1):
  ```
  <to>/export-info.json                        written last: format 1, kinds, levels, counts per folder, skipped
  <to>/textures/levels/NN/{tfrag,moby,tie,shrub}/<index>_<w>x<h>_t<ty>.png   + .json; tfrag mips .mipK.png
  <to>/textures/levels/NN/billboard/<o_class>_<w>x<h>.png                   + .json; mips .mipK.png
  <to>/textures/levels/NN/{sky,particle,fx,hud}/…png                         + .json; particle/part_defs.json, hud/icons.json
  <to>/textures/global/hud/…                                                 the global HUD set
  <to>/audio/levels/NN/sound_bank/NNN.wav + .json, audio/levels/NN/sound_bank.json (the bank index)
  <to>/audio/levels/NN/music/NNN.wav, speech/KK_<lang>.wav (+ .json), music.json (the level header's table)
  <to>/audio/global/sound_bank/…, music.wav, {help,vendor,space,qwark_boss,post_credits}_audio/NNN.wav
  <to>/levels/NN/level.gltf + level.bin         scenes: 0 level (LOD 0), 1 lod1, 2 lod2, 3 sky
  <to>/levels/NN/collision.gltf + .bin
  <to>/levels/NN/{mobys,ties,shrubs,volumes,paths,grind_paths,sound_instances,env_sample_points,fog_zones,level}.json
  <to>/models/levels/NN/mobys/CCCC.gltf + .bin  one per moby class, skinned, every sequence as an animation
  <to>/text/levels/NN/<lang>.json, text/global/all_text/<lang>.json
  ```
- **Textures → PNG.** 8-bit **indexed** PNGs: the pixel values are the stored PSMT8 indices; `PLTE`/`tRNS` hold the
  CLUT in linear order (CSM1 swizzle undone) with alpha scaled 0x80 → 255 (`a < 0x80 ? 2a : 255`, the loaders'
  `scale_alpha`), so any decoder shows exactly `decode_indexed8`. The export checks every texel against the loader's
  decode. The sidecar holds the CLUT as stored (`clut_csm1`, 1024 bytes hex: raw alpha, CSM1 order), the table
  entry (`data_offset`, `ty`, `palette`, `mipmap`, `pad`, or the billboard/sky/particle/HUD descriptor), the GS
  TEX0 format (PSMT8, CPSMCT32, CSM1, TCC 1, TW/TH) and the mip files. Accessors added to `rc-formats` so the
  indices come from the loaders' own slicing: `texture::{IndexedImage, entry_image, billboard_image,
  tfrag_mip_images, billboard_mip_images}`, `sky::sky_texture_image`, `particle_tex::{bank_texture_image,
  core_bank}`, `hud::Hud::frame_image` (the existing decoders now call them).
- **Audio → WAV.** Mono PCM16 decoded by `rc_formats::vag::decode` (the port's decoder, PCSX2 rounding). Bank
  samples: one WAV per distinct sample the tones play (`Bank::vags`), at 48 kHz for PS2-rate tones (negative centre
  note) and 44.1 kHz for PS1-style ones; VAG files at their header rate (44,100 music, 44,056 speech). Loops (the
  ADPCM loop-start + repeat flags, `SampleExtent::loop_points`) go into the WAV `smpl` chunk and the sidecar.
  `sound_bank.json` is the bank index: header, every sound (volume, pan, flags, instance limit) with its grains
  (type, delay, raw data, decoded tone parameters and the WAV the tone plays).
- **Levels → glTF 2.0** (`.gltf` + `.bin`, validated by `export::gltf::validate`, and imported by Blender 5.2 in
  the check below). Buffers keep game coordinates (Z up, world units); each scene's root node rotates Z up → Y up.
  - tfrag: one mesh per strip list (LOD 0 in scene 0, LOD 1 / 2 in scenes 1 / 2), one primitive per (texture,
    wrap, min filter) like the engine's batches; `COLOR_0` = the stored RGBA as display values (×2, clamped,
    sRGB-decoded), the stored bytes in `_PS2_RGBA`; TEX1 K values and the LOD distances (6L, 4L, 2L) in extras.
  - ties: one mesh per class and LOD, instanced by nodes with the instance matrix; NORMAL = the class normal of the
    vertex's light slot; `_PS2_TIE_SLOTS` (light slot, morph slots, fat) and `_PS2_MORPH_DELTA`; per-instance
    RGBA5551 ambient colours, uid, draw distance, occlusion index in node extras; ad-gif registers per primitive.
  - shrubs: one mesh per class, instanced; instance colour and light sets in extras, billboard record and texture
    in mesh extras. Mobys: an empty node per instance (position, R = Rz·Ry·Rx, scale) pointing at its model file.
  - sky: scene 3, one mesh per shell, camera-relative raw units; gouraud shells carry vertex colours.
  - materials: double-sided, `KHR_materials_unlit` (optional), base colour texture with the GS wrap/filter as the
    sampler, `alphaMode` MASK at AREF/0x80 when the texture has alpha; extras hold the GS pass (TEST_1, AREF,
    ALPHA_1 and the two-draw rule).
  - collision: world-space triangles (`collision_triangles`), one primitive per surface byte with surface id,
    sound class and high bit in extras.
- **Mobys → glTF**, skinned: stored pose (bind pose) mesh, high LOD + metal in scene 0, low LOD in scene 1;
  JOINTS_0 / WEIGHTS_0 from the resolved skin (weights /256). The glTF skin reproduces the game's result, not its
  mechanism: one joint node per palette entry `F_j = P_j·S_j` (`moby_anim::evaluate`) with identity inverse binds,
  so the rest pose is the stored pose, plus a `static` joint for lists the game draws with the identity. Every
  sequence is an animation: one key per keyframe at the time the game reaches it (1/rate ticks at 60 Hz), F_j
  decomposed into TRS (shear, if any, is dropped and reported: at most 1.2e-7 on Novalis), identity channels
  omitted. Ratchet (class 0) gets the level's `ratchet_seq` sequences. The class skeleton (the game's inverse bind
  matrices) and joint parents are in extras. Triangles are turned to face along their vertex normals (the game's
  strips alternate winding and the GS draws both sides).
- **JSON tables** from the gameplay file: moby instances (every field and the pvar block), tie and shrub instances,
  volumes, paths, grind paths, sound instances, env sample points, fog zones, ship placement. **Text**: every
  language block of each level and of `all_text`, each message as `strings::display` (lossless `\xNN` escapes)
  and as stored bytes.
- **What is lossy / not exported.** Load-time vertex lighting is not baked (tfrag uses the stored colours; tie and
  shrub colours stay per instance in extras); tfrag/tie LOD morphing is not animated (LODs are separate scenes, the
  morph deltas are attributes); PS2 colours above 0x80 clamp in `COLOR_0` (kept in `_PS2_RGBA`); a TRS shear in
  animations; ADPCM is decoded (the ADPCM bytes stay in Tier 0). Not exported yet: menu and goodies images,
  credits images and FMVs (no loader), the RAC1 gadget classes (compressed separately), moby collision, occlusion,
  scenes (cutscenes), sky shell rotations, `bindata`.
- **Check.** `RC_EXPORT_TEST_DIR=<dir> cargo xtask test-quick rc-extract --filter export_level_01 --ignored --nocapture` exports level 01, decodes
  every PNG/WAV/JSON with the test readers, validates every glTF (indices, bounds, min/max, skins, animations,
  image URIs), compares every texture with `parse_textures` and every bank WAV with `vag::decode_extent`
  (`RC_EXPORT_TEST_ALL=1`: every level and the global data). Level 01: 3,304 files, 168.6 MiB (audio 97.6,
  models 32.9, levels 27.8, textures 8.5, text 1.8) in 0.7 s; glTF: 515 meshes, 571,072 triangles, 81 skins, 543
  animations. Full disc: 60,015 files, 3.5 GiB in 10 s, nothing skipped. Blender 5.2 imports `level.gltf` and the
  moby models headless (textures, skin and animations render as expected).

## 6. Launcher options

| | A. Tauri app (OpenGOAL's choice) | B. Launcher screen inside the Bevy game binary | C. Separate Rust native GUI: egui/eframe (or iced/Slint) |
|---|---|---|---|
| Look and feel | Best: web UI, easy theming, rich layout | Consistent with the game, but bevy_ui widgets (text input, lists, scroll) are immature in 0.19; no game fonts or art exist before extraction | Good with a custom egui theme; iced looks nicer; Slint has the best declarative look |
| Toolchain | Node/yarn + Tauri CLI + ~400 crates; WebKitGTK on Linux | None new except a file dialog crate | eframe + rfd (~150 crates), pure Rust |
| Effect on game dev loop | None if separate workspace | **Touches rc-engine**: launcher edits rebuild the engine crate; new deps join the Bevy graph | None if separate workspace |
| Packaging | Built-in bundler (dmg/msi/AppImage) and updater | One binary | cargo-packager/cargo-bundle (needs approval) or a script |
| Multi-game RC1/2/3 | Natural (separate game binaries) | Awkward if RC2/RC3 become separate binaries | Natural |
| Mod management, restarts | Natural (spawns the game) | The game must restart itself | Natural |
| Licence | MIT/Apache | — | egui/eframe/rfd/iced MIT or Apache; **Slint GPLv3 / royalty-free with attribution / commercial** |

**Decided (user, 2026-09-26): A, Tauri, in a separate repo `rerac-launcher`** (overrides the original
recommendation C, egui in this repo).
- The launcher never links game code. It talks to a game build only through `docs/plan/launcher_contract.md`:
  `rerac-manifest.json`, the `rerac-extract` CLI (JSON lines, exit codes) and the runtime flags
  (`--version-json`, `--data-dir`/`RC_DATA_DIR`).
- Each downloadable game build (official or a mod fork) carries its runtime and its extractor, OpenGOAL-style, so a
  mod can change its own data format.
- The engine keeps a minimal "no game data found: run the launcher or `rerac-extract`" message and exit code.

### 6.1 Launcher behaviour
- **Library:** RC1, plus RC2/RC3 shown as "not yet supported". The picker still identifies an RC2/RC3 disc and
  says so.
- **RC1 page states:** Not installed → *Select disc image…* (file dialog, `.iso` only for now) → **Identify** (<1 s: PVD,
  `SYSTEM.CNF`, boot-ELF SHA-1 against the build DB).
  - Known, supported build: shows "Ratchet & Clank, NTSC-U v1.00 (SCUS-97199)" and the space needed.
  - Other RC1 builds (PAL SCES-50916, JP SCPS-15037, demos, Greatest Hits with a different ELF hash): "recognised,
    not supported yet", with serial and hash shown for a report.
  - Unknown build: refused, with the DB-row text to copy (OpenGOAL's pattern).
- **Extraction (as built):** `rerac-extract extract --iso <image> --out <root>/games/rac1/data --json`, with progress
  by bytes (`identify`, `copy`, `prepare`), an ETA and Cancel (= kill the process). There is no staging folder: the
  extractor is crash-safe in place (`*.partial` files, `extract-info.json` after the archive; contract clarification
  7). The last stage, `prepare`, builds Tier 1 (§5.4; contract clarification 17).
  - Measured (§4.1): 3–8 s for 4 GiB on the dev machine.
- **Done:** "Your disc image is no longer needed." Then Play, Verify files (re-hash against the manifest), Rebuild
  cache, Open data folder, Move data folder, Uninstall.
- **Stale cache** after an update: rebuilt automatically from Tier 0 with a progress bar; the disc is never needed.
- **Corrupt archive** (`verify` fails): name the files and offer re-extraction of only those lumps from a disc image.
- **Settings:** data location, export toggles (Tier 2), logs. **Game options stay in the game** (the in-game Port
  Options page rule); the launcher holds only install, data and mod settings.
- **Art:** we ship no game art. After extraction, the launcher may show images from the user's own disc (Tier 2 PNG
  of `planets`/`character_renders`). Before extraction it shows neutral art we make ourselves.

### 6.2 Data locations (per OS)
| | Game data (large) | Config (small) | Saves | Logs |
|---|---|---|---|---|
**Decided (contract):** one per-OS app data root named `rerac` (macOS `~/Library/Application Support/rerac/`;
the launcher picks the Windows/Linux equivalents), user-movable, owned by the launcher. The extractor and the
runtime never compute it; they receive paths (`--out`, `--data-dir`/`RC_DATA_DIR`).

```
<root>/versions/<source>/<version>/   one game build: rerac-manifest.json, runtime, extractor
<root>/games/rac1/data/               Tier 0 archive (§4.1) + extract-info.json
<root>/games/rac1/data/cache/v1/      Tier 1 engine cache (§5.4): stamp.toml, wad/levels/NN/*.lump
<root>/logs/, <root>/settings/
```
The engine's settings folder is still its own per-OS config folder `rerac/` (`render_settings::settings_path`); the
engine cut-over (P1.4) moves it under the data root's `settings/`.
**Dev:** `RC_DATA_DIR` or `--data` may point at a data root *or directly at an archive* (a directory containing
`toc.bin`). The workspace `extracted/` therefore stays the dev default, and its cache goes to
`extracted/cache/v<N>/`. `RC_EXTRACTED` stays as an alias for one transition period.

### 6.3 Multi-game structure
- `rc-install` keeps a game registry: `{id: rc1, name, supported builds, extractor fn, cache builder fn, game binary
  name}`.
- `rc-extract` exposes a `GameExtractor` trait: `identify`, `extract`, `prepare`, `export`.
- RC2/RC3 have their ToC at sector 1001 and wrapped ELFs from RC3 on (disc_layout.md §2.1, 2.6–2.7). Each needs its
  own Tier 0 reader. The container, manifest, launcher and data dir are shared.
- Whether RC2/RC3 run in the same engine binary is a later question.

### 6.4 Updates and packaging (later)
- **v1:** manual download.
- **v2:** a release feed check like OpenGOAL's GitHub releases. The repo is private, so this is decision U12.
- **Packaging:**
  - The launcher is packaged by its own repo (Tauri bundler). A game build is a folder with `rerac-manifest.json`,
    the runtime and `rerac-extract`; signing and notarisation need the user's Apple Developer ID (later, U13).
  - Windows zip or MSI; Linux tarball or AppImage.
  - Release builds only. No `[profile.*]` changes are proposed. LTO or strip for release would need user approval.

### 6.5 Packaging as built (D1, 2026-09-26)
- The runtime executable is `rerac` (`[[bin]]` in `crates/rc-engine/Cargo.toml`; the crate keeps its name, so
  `cargo dev`, `cargo dev-build` and `-p rc-engine` are unchanged).
- Shaders: `main.rs` `asset_dir()` uses `assets/` next to the symlink-resolved executable, then `../Resources/assets`
  (a macOS `.app`), else the compile-time repo path `crates/rc-engine/assets`. The dev loop is unchanged: there is no
  `assets/` in `target/*/`, so `cargo dev` still reads the repo's shaders directly (edits apply on restart, no
  rebuild). Embedding was rejected: it would rebuild the engine on every shader edit.
- `tools/package/package.sh [--no-build]`: one `cargo build --release --locked -p rc-engine -p rc-extract --bins`
  (no `--features dev`), then `dist/rerac-<version>-<os>-<arch>/` with `rerac`, `rerac-extract`,
  `assets/shaders/*.wgsl`, `rerac-manifest.json` (version from `crates/rc-engine/Cargo.toml`) and `README.txt`,
  zipped with `ditto` (macOS). An allow-list fails the package on any other file; the packaged
  `rerac --version-json` is checked against the manifest version. `dist/` is git-ignored.
- No `.app` for the runtime: the launcher starts it directly like OpenGOAL's `gk`, and a bundle would add an
  `Info.plist`, a second path layout and signing questions for no gain. The `../Resources/assets` lookup keeps the
  option open.
- Linux (`zip`) and Windows (Git Bash, `Compress-Archive`, `.exe` names in the manifest) branches are written but
  untested.

## 7. Removing the ISO dependency from the engine

**Done (Stage 1b, 2026-09-26).** As built, without a new `rc-data` crate:
- `crates/rc-engine/src/disc_source.rs` is data-folder-only: `IsoSource`, `RC_ISO`, `RC_SOURCE` and the ISO fallback
  are gone (setting either variable prints one "ignored" note). `read`/`level_file`/`read_path` keep their names, so
  the call sites are unchanged; `level_load::extracted_root()` returns `disc_source::data_root()`.
- Root order: `--data-dir <dir>` (or `=`), `RC_DATA_DIR`, then the development default (`RC_EXTRACTED`, else
  `<workspace>/extracted`). The folder must exist and hold `toc.bin`; for the first two `extract-info.json` must exist
  and carry `data_format` 1 (the development tree may lack it: one warning). Failures print one `error:` line and
  exit before any window: 3 = missing / not a data folder / incomplete extraction, 4 = `data_format` mismatch or
  unreadable `extract-info.json`, 2 = `--data-dir` without a value. Unknown arguments are ignored with a warning.
- `--version-json` prints `{"name":"rerac","version":"0.1.0","game":"rac1","data_format":1}` and exits first
  (about 10 ms, no window, no data access).
- Settings: `<per-OS config dir>/rerac/settings.toml`; when that file is missing and the pre-rename
  `…/randcrw/settings.toml` exists, the old file is copied once (and kept, never modified). Only the engine's own file is
  copied: on macOS the same folder is the launcher's data root, which the launcher migrates. `RC_SETTINGS_FILE` still overrides; deterministic runs still skip the file.
- Tests and tools: `rc_formats::test_data::root()` (`RC_EXTRACTED`, else `<workspace>/extracted`) replaces every
  hard-coded `../../extracted` in `rc-formats`, `rc-game` and `rc-trace` (`default_extracted`). Since 2026-09-27 the
  tests read only Tier 0 files (decompressed lumps, core blocks and gameplay sections come from the Rust loaders,
  `rc_formats::test_data`), so any `rerac-extract` data folder works as `RC_EXTRACTED`.
- Verified: a fresh `rerac-extract extract` (2,937 files, 4.2 s) then the engine with `--data-dir` on it,
  `RC_EXTRACTED` pointed at a bogus path and `RC_ISO` unset: Novalis renders (`RC_SCENE=0`), and the arrival scene
  plays with speech (0–25 s) and the level music after it (WAV capture + `RC_AUDIO_TRACE`).
- Not done from the plan below: the 19-level smoke run and the before/after deterministic capture; `rc_formats::disc`
  stays public in `rc-formats` (only `rc-extract` and golden tests use it).

The original plan:
- `disc_source.rs` keeps its function names (`read`, `level_file`, `read_path`), so the 16 call sites stay
  untouched. Its body becomes data-dir-only through `rc-data`.
- `level_load::extracted_root()` becomes `rc_data` root resolution.
- `RC_ISO`/`RC_SOURCE` are removed from the engine (it prints one line if they are set).
- `rc_formats::{disc, iso9660}` are used only by `rc-extract` and its tests.
- `rc-trace` and `rc-game` tests switch to `rc_data::test_root()` and still skip when absent.
- Acceptance: `grep -rn "rc_formats::disc\|iso9660" crates/rc-engine` is empty. The engine runs with the ISO moved
  away and with `extracted/` renamed to a launcher-style data dir. Deterministic captures are byte-identical before
  and after.

## 8. Staged build plan

Standing rules apply to every package: at most 3 agents, one cargo or engine process at a time, no sub-agents,
`RC_SCENE=0`, no Bevy/profile/dependency changes without approval, no commits, no disc bytes in the repo. New
workspace **path** crates are fine. Each package reports load times, file counts and the files it changed.

### Stage 1: Rust extractor, data dir, engine reads only the data dir
| Id | Package | Owns | Tests / acceptance | Depends |
|---|---|---|---|---|
| P1.1 | **Done (Stage 1a)**, with the build DB, SHA-1 and table in `crates/rc-extract` instead of `rc-formats`; no SHA-256 (not needed). Original scope: **Global lumps + build identification.** Port the RC1 global field table (`rac1_global_fields`: kind, count, names, duplicate-name suffix rule) and add `Disc::global_lumps()`/`read_global`. In-house `hash.rs` (SHA-1, SHA-256; no dependency). `build_db.rs`: RC1 builds from disc_layout.md §2.6, NTSC-U v1.00 ELF SHA-1 `72fd1de3…`, sequel serials for rejection. `identify(&IsoImage) → Identified{game, region, serial, version, elf_sha1, supported}` | `rc-formats/src/{toc.rs (append), disc.rs (append), hash.rs, build_db.rs, lib.rs (+2 lines)}`, `rc-formats/tests/disc_global.rs` | Every global lump byte-identical to `extracted/global/*` (all groups, including FMVs); FIPS test vectors for the hashes; `identify` on the real disc = supported; synthetic RC2 serial = "not RC1" | — |
| P1.2 | **Done (Stage 1a) except the `audit` subcommand**; no staging dir, `manifest.tsv` or `buildinfo.toml` (replaced by the committed table and `extract-info.json`, contract). Original scope: **`crates/rc-extract`** (lib + bin, no Bevy). `extract(iso, out, opts, progress, cancel) → Manifest` writes Tier 0: boot files, `toc.bin`, global lumps, the 19 level groups, streams. Parallel reader and writer threads (std only). Staging dir + atomic rename. `manifest.tsv` + `buildinfo.toml`. `ExtractError` codes. Subcommands `identify`, `extract`, `verify`, `audit` (sector coverage map) | `crates/rc-extract/**`, workspace `Cargo.toml` members (+1 line) | Synthetic mini-ISO end-to-end (`iso9660::tests::mini_iso`); real disc: every file byte-equal to the C++ `extracted/` `.bin` (2,937 files); a flipped byte fails `verify` and names the file; cancelling leaves no archive; wall time reported; audit report of unreferenced sectors | P1.1 |
| P1.3 | **`crates/rc-data`** (no Bevy). Root resolution (`--data`, `RC_DATA_DIR`, `RC_EXTRACTED` alias, per-OS default, workspace `extracted/` in dev), flat-archive detection, `read(rel)`, `level_file`, manifest and stamp checks, `test_root()` for tests | `crates/rc-data/**`, workspace members (+1 line) | Precedence unit tests; flat vs launcher layout; stale-stamp detection; missing-data error text | — (parallel with P1.1) |
| P1.4 | **Done (Stage 1b; §7)** except the 19-level smoke run and the deterministic before/after capture. Original scope: **Engine cut-over.** `disc_source.rs` becomes data-dir-only via `rc-data`; `extracted_root()` → `rc_data`; drop `RC_ISO`/`RC_SOURCE`; clear no-data message and exit code; README env table | `rc-engine/src/disc_source.rs`, `level_load.rs` (`extracted_root` only), `rc-engine/Cargo.toml` (+1 path dep), `README.md` (data section) | No `disc`/`iso9660` use in rc-engine; engine runs with the ISO moved away; music, speech, scenes, ship and save template load from the data dir; `RC_DETERMINISTIC` capture on 01 byte-identical before and after; 19-level smoke run | P1.3 |
| P1.5 | **Done (2026-09-26; §5.4)** with `stamp.toml`, lumps with an XXH64 trailer, and the lazy build in every build type; measured 01/05/16 and capture identity as in §5.4. Original scope: **Tier 1 v1 cache.** `rc-extract prepare` writes decompressed WADs + `stamp.toml`. `rc-data` serves `level_core_data(NN)`, `level_gameplay(NN)`, `hud_bank` as `Arc<[u8]>`, decompressed once per process and built lazily if the cache is missing or stale. Engine call sites switch from `wad::decompress(read(…))` to these helpers | `rc-extract/src/prepare.rs`, `rc-data` (cache module), engine: `level_load`, `fog_state`, `menu_render`, `gameplay`, `moby_attach`, `moby_spawn`, `tfrag_light`, `hud_render` (the decompress lines only; dispatch only when no other agent owns these files) | Load time before/after on 01, 05, 16; goldens unchanged; deterministic capture identical; stale stamp triggers a rebuild | P1.2, P1.4 |
| P1.6 | **Done (Stage 1b)** with `rc_formats::test_data::root()` instead of `rc_data::test_root()`. Original scope: **Test and tool migration.** `rc-trace` roots and the `rc-game` test paths use `rc_data::test_root()` | `tools/trace/src/{lib.rs, novalis_spawn.rs, tfrag_light_cmp.rs, tie_shrub_cmp.rs, port_sim.rs}`, the listed `rc-game` test modules (path lines only) | `cargo test --workspace` green; tests still skip without data | P1.3 (parallel with P1.4/P1.5) |
| P1.7 | **Built (§5.5).** **Tier 2 exports v1.** Textures and menu images → PNG (in-house stored-deflate PNG writer or the approved `png` crate); VAG → WAV via the existing decoder; text → JSON; tfrag/tie/shrub LOD0 → glTF (hand-written JSON + bin). `rc-extract export` | `rc-extract/src/export/**` | PNG count = texture count per level; WAV sample count = decoder output; glTF structure test | P1.2 (can move after Stage 2) |
| P1.8 | ~~**FMV demux** (lossless) into Tier 1; doc of the decoder options for U10~~ **Dropped** (U10 decided 2026-09-27: the engine demuxes at play time, `rc_formats::pss`) | `rc-extract/src/pss.rs`, `docs/plan/cutscenes_transitions.md` §5 append | Demuxed stream sizes add up to the PSS payload; SShd header fields as documented | P1.2 |

Stage 1 is done when a fresh machine with only the ISO can run `rc-extract extract` then `cargo dev`, and after
deleting the ISO everything the engine currently plays still works. The C++ `tools/extract` was the dump oracle until
the loaders were all ported; it was retired on 2026-09-27 (decisions.md) in favour of committed snapshot hashes.

### Stage 2: Launcher
Superseded in part by the Tauri decision: P2.1/P2.2/P2.4/P2.5 happen in the separate `rerac-launcher` repo against
`docs/plan/launcher_contract.md`; P2.3 (runtime `--data-dir`, `--version-json`, exit codes) stays here. The rows
below are the original plan.

| Id | Package | Owns | Tests / acceptance | Depends |
|---|---|---|---|---|
| P2.1 | **`crates/rc-install`** (UI-agnostic). Game registry; install state machine (NotInstalled, Identified, Extracting, Preparing, Ready, CacheStale, Corrupt); job thread with progress events; `launcher.toml`; move data dir; spawn the game with `--data`/`--game`/`--profile`; log capture | `crates/rc-install/**` | State transitions on a synthetic ISO; cancel and failure paths leave consistent state; spawn contract test with a stub binary | P1.2, P1.3 |
| P2.2 | **Launcher UI** (`launcher/`, separate workspace; eframe + rfd, pending approval): library, RC1 page, extraction progress, settings, logs, Mods tab placeholder | `launcher/**` | Screenshots of every state; manual end-to-end on macOS by the user with their disc | P2.1, U3/U4/U5 |
| P2.3 | **Partly done (Stage 1b)**: `--data-dir`, `--version-json`, exit codes 2/3/4 (§7); no `--game`/`--profile`, no log folder. Original scope: **Game binary contract.** `--data`, `--game rc1`, `--profile`; exit codes (no data, stale cache, corrupt); logs to `<data>/logs` | `rc-engine/src/main.rs` (minimal insertion), `rc-engine/src/disc_source.rs` | Launch from the launcher; each exit code shows the right launcher message | P1.4 |
| P2.4 | **Done for macOS (D1, 2026-09-26)**: `tools/package/package.sh` (§6.5); Linux/Windows branches written, untested; no `.app`, no signing. Original scope: **Packaging.** Release build script; macOS `.app` with both binaries (unsigned until U13), Windows zip, Linux tarball | `tools/package/**` | The `.app` runs on a clean user account | P2.2 |
| P2.5 | **Launcher polish.** Art from the user's own Tier 2 PNGs; RC2/RC3 identify-only pages | `launcher/**` | Screenshots | P1.7 |

### Stage 3: Mods (outline; see mods.md)
P3.1 VFS overlay in `rc-data` (per-path override with load order) → P3.2 mod package format, profiles, launcher Mods
tab → P3.3 importers (PNG → texture with an RGBA renderer path, WAV → PCM sample kind, text JSON) → P3.4 data mods
(gameplay instances, level settings, lifted ELF tables) → P3.5 scripting runtime (after decision M3).

## 9. Decisions (user, 2026-09-26)

| Id | Decision |
|---|---|
| U1 | Three tiers: **accepted**. |
| U2 | Extract everything, including the PAL copies; `--ntsc-only` optional: **accepted**. |
| U3 | Launcher: **Tauri** (overrides the egui recommendation). |
| U4 | **Separate repo** `rerac-launcher` (overrides "own workspace in this repo"). OpenGOAL structure: launcher and game builds versioned separately, mods are separate game builds (forks), the extractor ships in every game build. |
| U5 | Launcher dependencies are the launcher repo's business; **the extractor stays dependency-free**: accepted. |
| U6 | **SCUS_971.99 v1.00 only for now**; other builds recognised and refused; the build DB stays extensible (deferred, not dropped). |
| U7 | Committed size + SHA-1 table (hashes only): **accepted** (`crates/rc-extract/data/scus_971_99.tsv`). |
| U8 | Per-OS data folders named `rerac`, user-movable: **accepted** (contract "Folders"). |
| U9 | Engine loses ISO reading: **accepted** (later package, P1.4). |
| U10 | Movies archived losslessly as on disc: **accepted**. Decoder **decided 2026-09-27**: played natively from the original files at runtime (`rc-video` MPEG-2 decoder + `rc_formats::pss` + the VAG ADPCM decoder); no conversion at extraction, no ffmpeg in the product (decisions.md; docs/formats/pss.md). |
| U11 | **`.iso` only for now** (no `.bin`, CHD, CSO). |
| U12 | Manual updates: **accepted**. Official downloads stay off until the repo is public; a Development source uses local builds. |
| U13 | Signing later: **accepted**. |
| U14 | C++ extractor kept as the test oracle: **accepted**; **retired 2026-09-27** (decisions.md): replaced by committed Rust snapshot hashes. |
| — | Everything user-facing is named **ReRAC** (identifier `rerac`; renamed from randcrw on 2026-09-29). |

The original options and recommendations follow for reference.

### 9.1 Original options (recommendation in bold)
1. **U1 Data tiers:** raw archive (Tier 0) + regenerable engine cache (Tier 1) + optional exports (Tier 2).
   **Recommend yes.** Alternatives: raw-only (today), or convert-everything (lossy; §5.1).
2. **U2 Extraction scope:** everything on the disc, including PAL-only movies and scenes (~4.0 GiB), or NTSC-only
   (~2.6 GiB). **Recommend everything by default**, with a "skip PAL-only data" advanced option. Once the ISO is
   deleted, anything skipped is gone for good.
3. **U3 Launcher technology:** **separate egui/eframe app** / Tauri (closest to OpenGOAL, needs Node) / a screen
   inside the Bevy game.
4. **U4 Launcher placement:** **its own cargo workspace `launcher/`**, so it can never affect the Bevy dev build.
   The alternative is a workspace member. Resolver-2 feature unification only applies when built together, but
   `cargo test --workspace` would then build it.
5. **U5 New dependencies:** approve **`eframe`/`egui` and `rfd`** for the launcher only. **Keep `rc-extract` and
   `rc-data` dependency-free** (in-house SHA-1/SHA-256 and PNG writer). Optional: `png`, and `cargo-packager` as a
   tool.
6. **U6 Supported builds:** **NTSC-U v1.00 only.** Other RC1 builds are identified and refused with a message.
   PAL/JP support would need their own address maps; the overlay and ELF VAs differ.
7. **U7 Hash manifest in the repo:** commit a per-lump size + SHA-1 table for the supported build
   (`tools/extract/rc1_ntsc_v100.tsv` or in `build_db`; built as `crates/rc-extract/data/scus_971_99.tsv`). It is verification data only, like the ELF SHA-1 and ISO
   SHA-256 already in decisions.md. **Recommend yes.**
8. **U8 Data locations:** per-OS defaults as in §6.2, user-movable; the dev default stays `<workspace>/extracted`.
   **Recommend yes.**
9. **U9 Remove ISO reading from the engine** entirely (`RC_ISO`/`RC_SOURCE` go away; the disc reader becomes
   extractor-only). **Recommend yes.**
10. **U10 FMV path:** archive PSS raw and demux losslessly now. **Recommend deferring the decoder choice** to the
    FMV port. Options then: a native Rust MPEG-2 decoder (large job; no mature pure-Rust crate), transcoding at
    extraction with a user-installed or bundled ffmpeg (LGPL/GPL, external binary), or pl_mpeg-style MPEG-1
    (quality loss).
11. **U11 Disc image formats:** **`.iso` and `.bin` only in v1.** CHD/CSO later (a `chd` crate dependency).
12. **U12 Update channel:** **manual for now.** A release-feed check later, once the distribution question is
    settled.
13. **U13 Code signing and notarisation:** needs the user's Apple Developer ID. **Defer until packaging.**
14. **U14 C++ `tools/extract`:** **keep it as the dump oracle** until the loaders are all ported (existing
    decision). The Rust extractor becomes the only `unpack`. **Retired 2026-09-27** (decisions.md): the loaders are
    all ported and the golden tests check committed snapshot hashes instead.
