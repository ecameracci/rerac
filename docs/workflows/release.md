# Workflow: release packaging

Tool: `tools/package/package.sh` (README there), run through `cargo xtask package` (`tools/xtask`). Launcher
contract: `docs/plan/launcher_contract.md`.

| Task | Command |
|---|---|
| Build and package a release | `cargo xtask package` |
| Package the binaries already in `target/release` | `cargo xtask package --no-build` |

`cargo xtask package [args]` runs `tools/package/package.sh [args]` from the repo root; calling the script directly
does the same.

Output: `dist/rerac-<version>-<hash>-<os>-<arch>/` and its `.zip` (git-ignored); a release package (GitHub Actions,
or HEAD on a `v*` tag) is `dist/rerac-<version>-<os>-<arch>/`, the names the release assets and the launcher's zip
choice use. The version is the one workspace version,
`version` under `[workspace.package]` in `Cargo.toml` (every crate has `version.workspace = true`). The build is a
plain `--release` build (never `--features dev`).

The build version is `<version>-<hash>-<platform>`: that version, the first 8 hex digits of the git commit (`unknown`
without git) and the package's `<os>-<arch>`, e.g. `0.4.0-1a2b3c4d-windows-x86_64` (`crates/rc-extract/build.rs`). `rerac-manifest.json`,
`rerac --version-json`, `rerac-extract --version`, `extract-info.json`'s `extractor_version`, `README.txt`, the session
logs, `session.txt` and the crash log carry it, so every build names its commit. A local package's folder and zip
carry it too; a release package's keep the plain version.

## The allow-list

The script packages exactly these and fails if anything else ends up in the folder:

* `rerac` (crate `rc-engine`) and `rerac-extract` (crate `rc-extract`),
* `assets/shaders/*.wgsl`,
* `rerac-manifest.json` (the launcher manifest),
* `README.txt`.

So nothing from `tools/`, `extracted/`, `work/` or `~/PS2/ratchet1/` can ship, and no disc data can be packaged. The
script also runs the packaged `rerac --version-json` to check it.

macOS, Windows (Git Bash) and Linux (the release workflow's Ubuntu job; its package has been run) are tested. The
portable first start and standalone sessions (`docs/workflows/game-data.md`) are tested on Windows and Linux, not yet
on macOS. On Windows, `cargo xtask package`
runs Git for Windows' bash (found through `git --exec-path`), not a bare `bash`, which can be WSL's. No signing yet: a
downloaded zip is quarantined by macOS Gatekeeper.

## The icon

`crates/rc-engine/icon/rerac.ico` is the launcher's app icon (re-rac/rerac-launcher `src-tauri/icons/icon.ico`).
On Windows (MSVC) `crates/rc-engine/build.rs` links it into `rerac.exe` (Explorer, taskbar) and
`crates/rc-engine/src/window_icon.rs` puts it on the window's title bar. macOS and Linux get no icon yet: a bare
binary has none (a macOS `.app` would carry the launcher's `icon.icns`).

## Where the runtime finds its shaders

The runtime loads its shaders from `assets/` next to its (symlink-resolved) executable, or from `../Resources/assets`
in a macOS `.app`; when neither exists (`cargo dev`, `target/*/rerac`) it uses the repo's `crates/rc-engine/assets`
(`main.rs` `asset_dir`).

## Cutting a release

1. Set the version in `Cargo.toml` (`[workspace.package]`, `version = "X.Y.Z"`) and add a `## vX.Y.Z[-pre]` section
   to `CHANGELOG.md`. It is player-facing: plain and short, no build internals. Commit on `main`.
2. Tag that commit with an annotated tag and push the tag:

   ```sh
   git tag -a vX.Y.Z[-pre] -m "ReRAC vX.Y.Z[-pre]"
   git push origin main vX.Y.Z[-pre]
   ```

   The tag's `X.Y.Z` must equal the workspace version (the release workflow checks); the `-pre` part (`-alpha`,
   `-beta`, `-rc…`) exists only in the tag.

## GitHub Actions (`.github/workflows/`)

Only releases run on GitHub (no CI on pushes: checks and tests run locally, `docs/workflows/testing.md`). The release
workflow builds on `macos-15` (Apple Silicon, the main tested platform; `macos-14` until its retirement on 2026-11-02,
whose brownouts cancelled v0.5.1-alpha's first run), `windows-latest` and `ubuntu-22.04` (glibc 2.35; the job installs
ALSA, udev, Wayland and xkbcommon headers and `zip`), installs stable Rust with rustup and caches with
`Swatinem/rust-cache`.

* **`release.yml`** (push of a `v*` tag; or by hand with `workflow_dispatch`, input `tag`: an existing tag to release
  again, or empty to only build the packages as workflow artifacts): one build job per platform runs
  `tools/package/package.sh` (directly: on Windows a bare `bash` from `cargo xtask package` can be WSL's) and checks
  that the tag matches the workspace version; then one release job publishes `rerac-<version>-macos-arm64.zip`,
  `rerac-<version>-windows-x86_64.zip` and `rerac-<version>-linux-x86_64.zip` (each left out with a warning if its
  job failed) and the
  macOS `rerac-manifest.json`. The launcher picks the zip for its OS and CPU. A missing macOS package stops the
  release. It is marked a prerelease when the tag contains `-alpha`,
  `-beta` or `-rc`. The notes are the tag's `CHANGELOG.md` section, else the tag's annotation. Permissions:
  `contents: write`.

Secrets:

* `GITHUB_TOKEN` (automatic) creates the release.
* `SITE_DISPATCH_TOKEN` (optional repository secret): a token that may create `repository_dispatch` events on
  `re-rac/re-rac.github.io` (fine-grained: that repository, Contents read and write). When it exists, the release
  workflow sends the site a `release-published` event (payload: `tag`, `repository`) so the site can rebuild; without
  it the step is skipped.
