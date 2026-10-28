# gilrs-core 0.6.8, patched

Third-party code (Apache-2.0 / MIT, https://gitlab.com/gilrs-project/gilrs), copied from the crates.io release and
used through `[patch.crates-io]` in the workspace `Cargo.toml`. Excluded from the workspace: it is not one of ours.

**The change** (`src/platform/windows_wgi/gamepad.rs`, the polling thread): every Windows.Gaming.Input reading is
diffed against the previous one. Upstream skipped a reading whose timestamp matched the previous reading's, yet still
made it the next comparison baseline, so a press or release it carried was never sent. With Steam Input's virtual pad
(the Steam Controller under Steam) that showed as presses needing a second try and buttons staying held. The timestamp
fields only that check used are removed.

**The second change** (`src/platform/windows_wgi/gamepad.rs`, `Gilrs::handle_event`): only a Connected event adds a
gamepad. The polling thread can read a controller that just appeared before `RawGameControllerAdded` fires; upstream
then added the gamepad on that button or axis event, gilrs received input for an id it had no data for yet, and
`Gilrs::gamepad` panicked (index out of bounds). Steam re-creates its virtual pads when a controller it wraps is
unplugged, which made this reachable by unplugging an Xbox pad with Steam Input on. Such early events are dropped.

**Removing it:** once a gilrs-core release diffs every reading (or keeps the baseline on a skip), delete this folder,
the `[patch.crates-io]` entry and the `exclude` line, then `cargo update -p gilrs-core`.
