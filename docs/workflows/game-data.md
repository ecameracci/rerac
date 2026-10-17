# Workflow: game data (extract, verify, prepare, export)

Tool: `rerac-extract` (product crate `crates/rc-extract`; the launcher runs the same binary). Layout of a data
folder: `docs/plan/launcher_extractor.md` §4.1. All commands run from the repo root with
`export PATH="/opt/homebrew/opt/rustup/bin:$PATH"`; `RC_ISO` can stand in for `--iso`.

## The commands

| Task | Command |
|---|---|
| Identify a disc image | `cargo run --release -p rc-extract -- identify --iso "<disc>.iso"` |
| Extract (Tier 0 archive, then the engine cache) | `cargo run --release -p rc-extract -- extract --iso "<disc>.iso" --out <data folder>` |
| Verify a data folder against the built-in size/SHA-1 table | `cargo run --release -p rc-extract -- verify --out <data folder>` |
| Rebuild the engine cache (`<data folder>/cache/v1`) | `cargo run --release -p rc-extract -- prepare --out <data folder>` |
| Export usable formats (PNG, WAV, glTF, JSON) | `cargo run --release -p rc-extract -- export --out <data folder> --to work/exports` |

`export` defaults to `<data folder>/exports/`; for dev work send it to `work/exports/` (git-ignored) so `extracted/`
stays game data only.

## The development data folder: `extracted/`

Tests, `rc-trace` and `cargo dev` read `<repo>/extracted` (`RC_EXTRACTED` overrides it). **The one command to
(re)build it:**

```
cargo xtask regen-data
```

It needs the disc image: set `RC_ISO` once (fish: `set -Ux RC_ISO "<disc>.iso"`; zsh / bash: an
`export RC_ISO="<disc>.iso"` line in `~/.zshrc` / `~/.bashrc`; without it the command prints these lines with your
disc's path), or pass `--iso <disc>.iso`. It lists what `extracted/` holds, builds the dev `rerac-extract`, extracts
into `work/data-staging`, verifies it, and only then swaps it in and deletes the old folder, so a failure never
leaves you without data. About 8 s; the new data is about 4.4 GiB (3,072 files: 2,937 Tier 0 files,
`extract-info.json`, a 438 MiB engine cache). `cargo xtask regen-data --check` only lists what the folder holds.
Details and exit codes: `tools/xtask/README.md`.

The retired C++ extractor's leftovers (`.dec` copies, `*_dump.bin`, `core/` / `gameplay/` / `textures/` splits,
PNG / OBJ previews, `overlay.elf` / `overlay.txt`, `vu/`) are read by nothing; the overlay ELFs and VU listings were
copied to `work/ghidra-import/` and `work/vu/` on 2026-09-27. `regen-data` recognises them, lists their counts and
deletes them with the old folder. Any other file stops it (move it to `work/` or `~/PS2/ratchet1/`, or pass
`--force` to delete it).

**What may live in `extracted/`:** only what `rerac-extract` writes: the Tier 0 archive (`toc.bin`, `boot/`,
`global/`, `levels/NN/*.bin` and their `bindata/ music/ speech/ scene/` folders, `extract-info.json`) and the engine
cache `cache/`. **What may not:** trace dumps, savestates, reports, captures, exports, Ghidra material, anything a dev
tool writes. Those go to `work/` (generated) or `~/PS2/ratchet1/` (personal).

## How the engine finds its data

The engine never reads the disc image; it reads only a data folder that `rerac-extract` wrote (layout:
`docs/plan/launcher_extractor.md` §4.1; contract: `docs/plan/launcher_contract.md`).

```
cargo dev -- --data-dir <data folder>     # or RC_DATA_DIR=<data folder> cargo dev
cargo dev -- --version-json               # {"name":"rerac","version":"0.2.0","game":"rac1","data_format":1}
```

The data folder is chosen in this order:

1. `--data-dir <folder>` (also `--data-dir=<folder>`), how the launcher starts the game;
2. `RC_DATA_DIR`;
3. a packaged build (`tools/package`: `assets/shaders` next to the executable): `data/` next to the executable, the
   portable data folder;
4. the development default: `RC_EXTRACTED`, else `<repo>/extracted` (a `rerac-extract` data folder, same layout).

The folder must exist and hold `toc.bin`, and for 1 to 3 also the extractor's `extract-info.json` with a matching
`data_format` (the development tree may lack it: one warning). When the portable folder fails that check (the first
start, an interrupted extraction, another data format), the engine asks for the disc image in the system's file
dialog (`RC_ISO=<image>` skips the prompt and the dialog) and extracts it there in-process before opening the window
(`crates/rc-engine/src/first_run.rs`); cancelling exits with code 3. Such a build also keeps its saves in `memcard/`
next to the executable (unless `RC_SAVE_DIR` or the `memcard_dir` setting says otherwise); on its first start that
folder is a copy of the default card folder (`<config base>/rerac/memcard`), which stays as it was
(`crates/rc-engine/src/saves.rs`). Started that way but not from a terminal
(a double-click), the session's output goes to `logs/rerac-<unix time>.log` next to the executable (the 10 newest
are kept), fatal errors and panics are also shown in a dialog, and on Windows the console window shows only the first
start's progress and closes when the game window opens (`crates/rc-engine/src/standalone.rs`). Otherwise the engine prints one `error:` line saying
what to do (re-extract via the launcher) and exits before opening a window: code 3 (folder missing, not a data folder,
or extraction incomplete), code 4 (data format mismatch or unreadable `extract-info.json`), code 2 (`--data-dir`
without a value). `--version-json` prints the contract line and exits without touching any data.

## The engine cache (Tier 1)

`extract` ends by building `<data folder>/cache/v1/` (stage `prepare`, about 0.2 s, 440 MiB): every level's core
data, gameplay file and HUD banks, WAD-decompressed once, plus `stamp.toml`. `rerac-extract prepare --out <data
folder>` rebuilds or refreshes it from the archive alone (no disc). The engine reads lumps through `rc-data` (once per
process each); when the cache is missing, stale (stamp: cache version, converter versions, `extract-info.json` hash)
or a lump is corrupt (XXH64 trailer), it rebuilds what it needs on the fly with one log line per lump, and falls back
to in-memory decompression when the folder cannot be written. The development `extracted/` tree therefore gets
`extracted/cache/v1/` on the first `cargo dev`. The folder can be deleted at any time. `RC_CACHE=0` skips it;
`RC_PERF_LOG=1` prints where each lump came from.

## Exports (Tier 2, optional)

`rerac-extract export --out <data folder> [--to <dir>] [--what textures,audio,models,levels,collision,text|all]
[--level NN]` writes usable formats from the archive (no disc): indexed PNG textures with JSON sidecars (original CLUT
and GS format), WAV audio with loop points, glTF 2.0 levels, collision and skinned, animated moby models, and JSON
tables and text. Default folder `<data folder>/exports/`; the full disc is about 60,000 files and 3.5 GiB in about
10 s. The game never reads them; the launcher's "Export assets…" runs the same command. Layout and what is lossy:
`docs/plan/launcher_extractor.md` §5.5; how mods will use them: `docs/plan/mods.md` §2.1a.

## What tests read

Tests and `rc-trace` read the development `extracted/` tree (`RC_EXTRACTED` overrides it;
`rc_formats::test_data::root()`); they read only Tier 0 files, so any `rerac-extract` data folder works, and they
skip when it is absent. Decompressed lumps, core blocks (`moby_class/NNNN`, `ratchet_seq/NNN`, …) and gameplay
sections (`level_settings`, …) come from the Rust loaders (`rc_formats::test_data`). An `extracted/` tree written by
the retired C++ extractor still works; its extra files are no longer read, and `cargo xtask regen-data` regenerates a
clean tree (above).

## Players

Players never run these by hand: the launcher (docs/workflows/launcher.md) runs `extract` into its own data folder
and starts the game with `--data-dir`.
