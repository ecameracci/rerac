# Workflow: play sessions (bug reports with a replay)

Every interactive run records a session folder (`crates/rc-engine/src/session.rs`): the log, the pad input, the
settings and the memory card it started from. A frame-exact replay of the folder runs the same game again, tick for
tick, so a bug seen in play can be reproduced, traced and captured without playing again.

## For the player

1. Play as usual (`cargo dev`, a built `rerac`, or the launcher). The game prints
   `session: recording into <folder>` at the start.
2. When something is wrong, press **F8** at that moment (once per bug). Each press saves `marker_NN.png` (the game
   frame), one line in `markers.tsv` (the update, the tick, the level, Ratchet's place) and the F9 dump in the log.
3. Quit the game, then send the session folder (zip it) with one line per marker: what is wrong.

The folders are under `sessions/` next to the settings file (`%APPDATA%\rerac\sessions\` on Windows,
`~/Library/Application Support/rerac/sessions/` on macOS, `~/.config/rerac/sessions/` on Linux), named by the local
start time. The 10 newest sessions without markers are kept; sessions with markers stay until deleted. `RC_RECORD=0`
turns the recording off.

| File | Content |
| --- | --- |
| `session.txt` | version, command line, boot level, every `RC_*` variable |
| `log.txt` | everything the run printed (stdout and stderr); `crash.txt` holds a panic's backtrace |
| `input.tsv` | per update: 60 Hz steps, tick budget, the 18 pad bytes, the keys held (a line per change, at least every 60 updates) |
| `start/` | the settings file and the memory card as the run found them |
| `markers.tsv`, `marker_NN.png` | the F8 markers |

## For the agent

```
RC_REPLAY=<session folder> cargo dev                                   # replay to the end, markers again
RC_REPLAY=<folder> RC_PLAY_TRACE=1 cargo dev                           # plus one trace line per tick
RC_REPLAY=<folder> RC_SCREENSHOT_FRAME=<update> RC_SCREENSHOT=a.png cargo dev
RC_REPLAY=<folder> RC_DUMP_FRAMES=<a>..<b> RC_DUMP_DIR=work/captures/x cargo dev
RC_REPLAY=<folder> RC_REPLAY_UNTIL=<update> RC_MOBY_DUMP_AT=<tick> cargo dev
```

The replay is frame-exact (`RC_DETERMINISTIC=1`): each update runs the recorded steps and tick budget with the
recorded pad and keys; the live devices are ignored. The session's `RC_*` variables apply unless the replay sets
them (so a trace switch can be added). Settings and card are copied into `RC_REPLAY_OUT` (default
`<session>/replay/`), where the replay's own `log.txt` and the markers' pictures go. Frame numbers (`RC_SCREENSHOT_FRAME`,
`RC_DUMP_FRAMES`, the markers' `update`) are updates; ticks (`RC_MOBY_DUMP_AT`, `RC_RESPAWN_PROBE_AT`, the trace) are
the game counter.

Use a session as a reproduction, not as a test input: like savestates, sessions are personal files (docs/test-requests/README.md).

**Verified 2026-10-17** (Windows, Novalis, a 452-update real-time run with a scripted pad that ran 1 to 4 steps per
update): all 827 `RC_PLAY_TRACE` lines of the replay equal the run's (state, position, animation, camera, `rand`,
mobys, particles, sounds, HP), and an F8 added to `input.tsv` fired its marker at its update in the replay.

**Not reproduced** (G-TOOL-023): the window's shape when the aspect is `fill` (the view widens with it), the mods
(frame-exact runs load none unless `RC_MODS_DIR`), the mouse (the fly camera's look; the right stick from the mouse
is in the pad bytes), the save dates the card gets, a key tapped and released between two updates (held for one
update instead), and a run of another build or other game data. A session recorded before a code change can stop
matching after it: replay with the build the player used.
