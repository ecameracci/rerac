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
- **What Update does (to decide).**
  - Hand off to the launcher, which already picks and downloads the zip for its OS and CPU (`docs/plan/launcher_contract.md`; may need a new message there, outside its verbatim section).
  - Or download `rerac-<version>-<os>-<arch>.zip` in the game and swap the files on restart. Windows can't overwrite a running `rerac.exe`, so this needs a helper or a restart step.
- Release assets and names: `docs/workflows/release.md`.
