# Check for Update (planned, beta)

Status: not started (2026-11-02). Planned for the beta build, after the first playthrough pass. Fork-only: never upstreamed.

## What

A **Check for Update** item in the main menu that opens a page showing:

- the current version (the build version, `<version>-<hash>-<platform>`, `crates/rc-extract/build.rs`);
- the newest version, if there is one;
- **Update** (only when there is an update) and **Back**.

Updates come from this fork's releases (`ecameracci/rerac`).

## Notes

- **No HTTP client in the engine today.** A check needs HTTPS (e.g. `ureq` with TLS): a new dependency, so it needs a reason and a note per the repo rules, and it costs build time. Keep it out of `crates/rc-extract` (dependency-free).
- **Prereleases.** GitHub's `releases/latest` skips prereleases, and every release so far is one (`-alpha`). Either list releases and take prereleases, or make the beta releases non-prerelease.
- **Comparison.** Compare the tag's `X.Y.Z[-pre]` with the build's version; offline or an error shows "Couldn't check", never an error state.
- **Network only on request.** Check when the page opens, never in the background.
- **Update installs it itself, without the launcher.** The game downloads the release's
  `rerac-<version>-<os>-<arch>.zip` for its OS and CPU and replaces the packaged files (the allow-list in
  `docs/workflows/release.md`: `rerac`, `rerac-extract`, `assets/shaders/*.wgsl`, `rerac-manifest.json`, `README.txt`),
  then restarts. Open points:
  - Windows can't overwrite a running `rerac.exe`: rename it aside (allowed while running) and move the new one in,
    or hand off to a small helper that waits for the game to exit.
  - Unpack to a temporary folder next to the game and check it (e.g. `rerac --version-json`) before swapping, so a
    failed download leaves the old build working.
  - macOS: a downloaded zip is quarantined by Gatekeeper (no signing yet); a `.app` bundle swaps as a whole.
  - Extracted game data (`extracted/`) and the settings are left alone; a new extractor version may need a re-extract.
- Release assets and names: `docs/workflows/release.md`.
