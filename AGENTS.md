# AGENTS.md

Entry point for coding agents in this repository. It points at the docs that hold the rules; read them, don't
re-derive them.

## Fork only: never upstream

This file belongs to the fork `ecameracci/rerac`, not to upstream `re-rac/rerac`.

* Never include `AGENTS.md` in a commit, branch or pull request meant for upstream. Upstream changes start from a
  branch off upstream `main` and take the wanted commits without this file (cherry-pick, or drop it in the branch).
* Keep changes to `AGENTS.md` in their own commits, never mixed with code, so they are easy to leave out.

## Branches and pull requests

* `main` mirrors upstream's `main` (`.github/workflows/mirror-upstream-main.yml`, force-pushed hourly): never commit
  to it and never target it.
* `dev` is the fork's default branch and is protected: no direct pushes, no rewriting. Work on a branch off `dev` and
  open the pull request against the fork's `dev` (`gh pr create --repo ecameracci/rerac --base dev`).
* Commit only when asked, and only the files of the task: the user often has unrelated edits in the tree.

## What to read first

| Topic | Doc |
|---|---|
| Standing rules, repository map, working method, briefs | `docs/plan/orchestration.md` |
| Decisions (Rust/Bevy, native-first fidelity, dependency-free extractor) | `docs/plan/decisions.md` |
| Tests: tiers, areas, which command to run | `docs/workflows/testing.md` |
| Known gaps (add yours, strike the ones you close) | `docs/plan/gaps.md` |
| Packaging, releases, the build version | `docs/workflows/release.md` |
| Launcher contract (its "verbatim" section is never edited) | `docs/plan/launcher_contract.md` |
| Env switches, game data, play sessions | `docs/workflows/dev-switches.md`, `game-data.md`, `play-sessions.md` |

## Hard rules (details in the docs above)

* Nothing disc-derived is ever committed or packaged. `extracted/`, `work/`, `target/` and `dist/` are git-ignored
  output; tests read `extracted/` and skip when it is absent, and never read personal files (`tools/repo-checks`).
* Build times are sacred: never change the Bevy version, `[profile.*]`, `.cargo/config.toml`, dependency versions or
  Bevy features. Run the engine with `cargo dev`; one cargo or engine process at a time.
* Tests run only through `cargo xtask test-quick | test-job | test-full`, never `cargo test` or `cargo nextest` by
  hand. Checks: `cargo check-all`, `cargo clippy-all`.
* `crates/` ships and never depends on `tools/`. No new external dependency without a reason and a note; the
  extractor (`crates/rc-extract`) stays dependency-free.
* Match the code's style: terse doc comments that cite the files, functions and addresses they rely on.

## Platform notes

* macOS (main platform): `export PATH="/opt/homebrew/opt/rustup/bin:$PATH"`.
* Windows (PowerShell): cargo is often not on `PATH`: `$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"`. A bare
  `bash` can be WSL's; run shell scripts with Git for Windows' bash (`cargo xtask package` finds it itself).
