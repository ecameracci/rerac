# Check for Update (beta)

Status: built (2026-11-07) for 0.6.0-beta. Fork-only: never upstreamed.

## What

Main menu → Options → **Update** (named "Check for Update" until 2026-11-14, cut off by the list panel) (after Mods, before Quit Game) opens a page showing:

- **This version**: the build version (`<version>-<hash>-<platform>`, `crates/rc-extract/build.rs`);
- **Newest**: the newest release of `ecameracci/rerac` ("Checking..." while the request runs, "Couldn't check"
  offline or on any error);
- a status line: "Up to date", "An update is available", "No download for this system yet", "Downloading... a / b MB",
  "Installing...", "Update failed: …", "Restarting...";
- the rows **Update** (only when a newer release has a zip for this platform) and **Back**.

## Code

- `crates/rc-game/src/menus/pause/update.rs`: the page, built like the Mods page (the Port Options' main-menu page
  frames, list panel stretched, title label copy, port-only records 0x7f00_0060 / 0x7f00_0120 / 0x7f00_0220 and
  callbacks 0x7f00_1018 / 0x7f00_101c, strings −0x400..−0x404). `PageMenu::set_update_view` before each front-end
  tick, `PageMenu::take_update_pick` after it. ✕ on Back is △; while busy only △ / the close keys work.
- `crates/rc-engine/src/update.rs`: the check, the install, the restart. `menu_render::front_end_frame` calls
  `update::check` when `MenuOut::entered` is the page (the only network contact besides Update), `update::install`
  on ✕ Update, and starts the new build and exits on `update::take_restart`. `main` calls `update::cleanup`.

## How it works

- **Check**: `GET https://api.github.com/repos/ecameracci/rerac/releases?per_page=30` (`releases/latest` skips
  prereleases, and every release is one). Drafts and tags that are not `vX.Y.Z[-pre]` are skipped. The newest
  release that has `rerac-<X.Y.Z>-<os>-<arch>.zip` for this build's platform (the platform is the build version's
  tail) is shown, else the newest (no Update). Prereleases order alpha < beta < rc < release.
- **Newer** means a higher `X.Y.Z` than the build's. The build version carries no `-pre`, so a release of the same
  `X.Y.Z` (0.6.0 after 0.6.0-beta) is never offered: bump the patch for a release that must reach beta players.
  `RC_UPDATE_AS=<X.Y.Z>` compares as that version (test switch, `docs/workflows/dev-switches.md`).
- **Install** (a thread; the page shows the progress):
  1. Only from a packaged folder (`rerac-manifest.json` and `assets/shaders/` beside the executable), else "Update
     failed: not a packaged build" (`cargo dev`, `target/*`).
  2. Download the zip into `.rerac-update/download.zip` beside the game (redirects followed, the length checked).
  3. Unpack only the release allow-list (`rerac`, `rerac-extract`, `assets/shaders/*.wgsl`, `rerac-manifest.json`,
     `README.txt`; `docs/workflows/release.md`) into `.rerac-update/new/`, dropping the zip's top folder; stored or
     deflated entries, each CRC-32 checked; on Unix the executables get mode 755.
  4. Run the new `rerac --version-json` (≤ 30 s, no window) and check it says `rerac`, the release's `X.Y.Z` and this
     platform. Any failure so far leaves the old build untouched.
  5. Swap: each old file is renamed into `.rerac-update/old/` (Windows allows renaming the running `rerac.exe`, not
     overwriting it), then the new one moved in; shaders the new build dropped go to `old/` too. Any failure undoes
     every move.
  6. Start the new `rerac` with this process's arguments and exit. The next start removes `.rerac-update` (retrying
     for 10 s while the old process lets go of its renamed executable).
- **Left alone**: the settings (`%APPDATA%` / `~/.config` / `~/Library/Application Support`, or a `settings.toml`
  beside the game), `data/` (extracted game data), `mods/`, `memcard/`: none is in the allow-list. A build with a new
  `data_format` asks for a re-extract on its first start as usual (`crate::disc_source`).
- **macOS**: a zip the game downloads itself is not quarantined (only quarantine-aware apps like browsers set the
  flag). A `.app` bundle layout is not handled (releases ship a bare folder).

## Dependency (`crates/rc-engine/Cargo.toml`)

`ureq` 3 with `default-features = false, features = ["rustls"]`: rustls on ring with the webpki roots. The smallest
pure-Rust HTTPS client: no OpenSSL or system TLS, the same on the three platforms, and 15 small crates added to the
lock (ureq, ureq-proto, rustls, rustls-webpki, rustls-pki-types, ring, webpki-roots, http, httparse, base64 0.23,
untrusted, subtle, zeroize, utf8-zero, getrandom 0.2). `serde_json` (the release list) and `flate2` (the zip's
deflate) were already built for Bevy (bevy_gltf, png), so they add no build time. `crates/rc-extract` stays
dependency-free.

## Tested

- Unit tests (`update.rs`, `pause/update.rs`): version parsing / order / "newer", the build version's parts, the
  asset choice, the release list JSON, the allow-list, the `--version-json` check, unpack + swap on a temp folder
  (settings and `data/` stay, stale shaders move out).
- Windows, for real (2026-11-07), a `cargo xtask package` of this branch (`0.5.3-d29e9bd2-windows-x86_64`) unzipped
  to a scratch folder, run with `--data-dir`, `RC_SKIP_LOGOS=1` and
  `RC_PLAY_SCRIPT="300:press START,340:press DOWN,360:press DOWN,380:press X,420:press DOWN,435:press DOWN,450:press DOWN,465:press DOWN,480:press X"`
  (main menu → Options → Update):
  - without `RC_UPDATE_AS`: Newest 0.5.3-alpha, "Up to date", only Back;
  - with `RC_UPDATE_AS=0.5.2` and `700:press X` (Update): the real `rerac-0.5.3-windows-x86_64.zip` of v0.5.3-alpha
    downloaded, checked, swapped in while `rerac.exe` ran, and the game restarted into it (`--version-json`:
    `0.5.3-9eb6b3f8-windows-x86_64`); the old files sat in `.rerac-update/old/`.
  - this branch's build started with a leftover `.rerac-update/` beside it removed the folder.
- Not run: macOS and Linux (the code paths are the same but for the 755 mode).
