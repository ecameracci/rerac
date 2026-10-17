//! The portable first start: a packaged build started without `--data-dir` keeps its game data in `data/` next to
//! the executable (crate::disc_source). When that folder is missing, incomplete or of another data format, the player
//! picks their disc image (`.iso`) in the system's file dialog (crate::dialog) and the extractor (`rc_extract`,
//! in-process) writes the folder before the window opens. `RC_ISO=<image>` skips the first prompt and the dialog.
//! The progress goes to the session's console and log (crate::standalone::say).

use crate::dialog;
use crate::standalone::say;
use rc_extract::build_db::BUILDS;
use rc_extract::extract::{self, Options};
use rc_extract::{mib, Event, Stage, DEFAULT_THREADS};
use std::path::{Path, PathBuf};
use std::time::Instant;

/// Extracts the player's disc into `out`; `problem` is the folder check's message when a folder was already there.
/// `false`: the player cancelled.
pub fn setup(out: &Path, problem: Option<&str>) -> bool {
    let mut iso = std::env::var_os("RC_ISO").filter(|v| !v.is_empty()).map(PathBuf::from);
    if iso.is_none() {
        let why = problem.map_or(String::new(), |p| format!("{p}\n\n"));
        let text = format!(
            "{why}ReRAC needs the game data from your own Ratchet & Clank disc (NTSC-U, SCUS_971.99) as an .iso disc \
             image.\n\nChoose the .iso next. ReRAC copies what it needs into\n{}\nonce (a few minutes); the .iso is \
             not needed after that.",
            out.display()
        );
        if !dialog::confirm(&text) { return false; }
    }
    loop {
        let Some(path) = iso.take().or_else(dialog::pick_iso) else { return false };
        say(&format!("extracting {} into {} (one time only)", path.display(), out.display()));
        match run(&path, out) {
            Ok(line) => { say(&line); return true; }
            Err(e) => {
                say(&e.to_string());
                if !dialog::retry(&format!("The disc image could not be extracted:\n\n{}\n\nChoose another .iso?", e.message)) {
                    return false;
                }
            }
        }
    }
}

/// `rerac-extract extract` (which ends with `prepare`), reporting each stage's progress in tenths.
fn run(iso: &Path, out: &Path) -> Result<String, rc_extract::Error> {
    let t0 = Instant::now();
    let mut last: Option<(Stage, u64)> = None;
    let mut emit = |e: Event| match e {
        Event::Progress { stage, done, total, .. } => {
            let tenth = (done * 10).checked_div(total).unwrap_or(10);
            if last == Some((stage, tenth)) { return; }
            last = Some((stage, tenth));
            say(&format!("{:<8} {:3}%  {:.0} / {:.0} MiB", stage.name(), tenth * 10, mib(done), mib(total)));
        }
        Event::Info(m) => say(&m),
        Event::Disc(d) => say(&format!("disc: {} ({}, v{})", d.serial, d.region, d.version)),
    };
    let opts = Options { ntsc_only: false, threads: DEFAULT_THREADS, cancel: None };
    let (_, s) = extract::extract(iso, out, BUILDS, &opts, &mut emit)?;
    Ok(format!("extracted {} files, {:.1} MiB in {:.1} s", s.files, mib(s.bytes), t0.elapsed().as_secs_f64()))
}
