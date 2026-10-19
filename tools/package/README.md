# tools/package: release packaging

Dev only, never ships itself. Workflow: `docs/workflows/release.md`.

* **Purpose:** builds a launcher version folder of the game (launcher contract: `docs/plan/launcher_contract.md`) and
  zips it.
* **Command:** `tools/package/package.sh` (release build, then package); `--no-build` packages the binaries already in
  `target/release`.
* **Inputs:** `target/release/rerac` and `rerac-extract`, `crates/rc-engine/assets/shaders/*.wgsl`, the workspace version
  (`version` under `[workspace.package]` in `Cargo.toml`).
* **Outputs:** `dist/rerac-<version>-<hash>-<os>-<arch>/` and its `.zip` (git-ignored); a release (GitHub Actions,
  or HEAD on a `v*` tag) drops the hash: `dist/rerac-<version>-<os>-<arch>/`. The folder is checked against an
  exact allow-list (the two binaries, `assets/shaders/*.wgsl`, `rerac-manifest.json`, `README.txt`), so nothing
  from `tools/`, `extracted/` or `work/` can be packaged.
