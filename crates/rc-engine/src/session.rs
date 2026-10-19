//! **Port-only: play sessions** (a development aid). Every interactive run keeps a folder with what it takes to
//! reproduce it, and a frame-exact replay of that folder runs the same game again (docs/workflows/play-sessions.md).
//!
//! * **Recording**: every run that is not frame-exact (`RC_RECORD=0`: none) writes
//!   `<settings folder>/sessions/<local date and time>/` (crate::render_settings: `%APPDATA%\rerac` on Windows):
//!   * `session.txt`: the version, the command line, the boot level and the `RC_*` environment;
//!   * `log.txt`: stdout and stderr, still shown where they went before (the terminal, crate::standalone's log);
//!   * `input.tsv`: per update (Bevy frame, 1-based): the 60 Hz steps it ran (crate::frame_pace), the gameplay tick
//!     budget, the 18 pad bytes (crate::input_map::PadFrame) and the keyboard keys held; a line whenever one of them
//!     changes and at least every [`HEARTBEAT`] updates, so a crash loses at most that many;
//!   * `start/settings.toml`, `start/memcard/`: the settings (the file beside the executable merged over the port
//!     settings file, `render_settings::effective_text`) and the memory card as the run found them;
//!   * `markers.tsv`, `marker_NN.png`: **F8** bug markers (update, tick, level, Ratchet's place and yaw as an
//!     `RC_HERO_AT` value; the F9 moby dump goes to the log), each with a picture of the game frame;
//!   * `crash.txt`: a panic's message and backtrace (crate::crash_log).
//!
//!   The [`KEPT`] newest sessions without markers are kept; a marked session stays until it is deleted.
//! * **Replay** (`RC_REPLAY=<session folder>`): the session's start (its `RC_*` variables unless the replay sets them,
//!   copies of its settings file and card), frame-exact, every update with the recorded steps, tick budget, pad and
//!   keys (F11 left out). The markers fire again at their updates, into `RC_REPLAY_OUT` (default `<session>/replay`)
//!   with the replay's own log; it exits after the last recorded update (`RC_REPLAY_UNTIL=<update>`: after that one).
//!   `RC_SCREENSHOT_FRAME` / `RC_DUMP_FRAMES` capture as in any frame-exact run (their frame = the update number).
//!
//! Not reproduced: the window's shape when the aspect is `fill` (the view widens with it), the mods (frame-exact runs
//! load none unless `RC_MODS_DIR` is set), the mouse (the fly camera's look; the right stick from the mouse is in the
//! pad bytes), the save dates the card gets, and a run of another build or other game data.

use crate::determinism::FrameNumber;
use crate::input_map::PadFrame;
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};
use bevy::time::TimeUpdateStrategy;
use rc_game::pad::PadInput;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Sessions without markers kept, the new one included.
const KEPT: usize = 10;
/// Updates between two `input.tsv` lines at most.
const HEARTBEAT: u64 = 60;
/// Variables a replay never takes from the session: where the data, the settings, the card and the outputs are, and
/// the recording itself.
const NOT_REPLAYED: [&str; 14] = [
    "RC_RECORD", "RC_REPLAY", "RC_REPLAY_OUT", "RC_REPLAY_UNTIL", "RC_DATA_DIR", "RC_EXTRACTED", "RC_ISO", "RC_SETTINGS_FILE",
    "RC_SAVE_DIR", "RC_MODS_DIR", "RC_SCREENSHOT", "RC_SCREENSHOT_DELAY", "RC_PERSONAL", "RC_WORK",
];
/// The keyboard keys recorded (the pad keys of crate::input_map, the engine and fly-camera keys, and the rest of the
/// letters, digits and function keys).
const KEYS: &[KeyCode] = {
    use KeyCode::*;
    &[
        KeyA, KeyB, KeyC, KeyD, KeyE, KeyF, KeyG, KeyH, KeyI, KeyJ, KeyK, KeyL, KeyM, KeyN, KeyO, KeyP, KeyQ, KeyR, KeyS,
        KeyT, KeyU, KeyV, KeyW, KeyX, KeyY, KeyZ, Digit0, Digit1, Digit2, Digit3, Digit4, Digit5, Digit6, Digit7, Digit8,
        Digit9, F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12, ArrowUp, ArrowDown, ArrowLeft, ArrowRight, Space, Enter,
        Backspace, Tab, Escape, ShiftLeft, ShiftRight, ControlLeft, ControlRight, AltLeft, AltRight, Comma, Period,
    ]
};

/// The output folder: the session's, or the replay's (`RC_REPLAY_OUT`).
static DIR: OnceLock<PathBuf> = OnceLock::new();
static REPLAY: OnceLock<Replay> = OnceLock::new();

/// One `input.tsv` line: the state from update `frame` on.
#[derive(Clone, Debug, PartialEq)]
struct Entry {
    frame: u64,
    steps: u32,
    budget: u32,
    pad: PadInput,
    keys: Vec<KeyCode>,
}

struct Replay {
    entries: Vec<Entry>,
    /// The last update replayed.
    until: u64,
}

impl Replay {
    /// The state of update `frame` (1-based).
    fn at(&self, frame: u64) -> Option<&Entry> {
        let i = self.entries.partition_point(|e| e.frame <= frame);
        i.checked_sub(1).map(|i| &self.entries[i])
    }
}

/// The session's (or the replay's) folder, once [`start`] made it.
pub fn dir() -> Option<&'static Path> { DIR.get().map(PathBuf::as_path) }

/// A replay's 60 Hz steps of update `frame` (crate::determinism counts the game ticks with them).
pub fn replay_steps(frame: u64) -> Option<u32> { REPLAY.get().map(|r| r.at(frame).map_or(1, |e| e.steps)) }

fn env(name: &str) -> Option<String> { std::env::var(name).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty()) }

/// `RC_REPLAY`: the session's start, before anything reads the environment (the first call in `main`).
pub fn prepare() {
    let Some(session) = env("RC_REPLAY").map(PathBuf::from) else { return };
    let fail = |m: String| -> ! {
        eprintln!("error: RC_REPLAY: {m}");
        std::process::exit(1)
    };
    let read = |name: &str| std::fs::read_to_string(session.join(name)).unwrap_or_else(|e| fail(format!("{}: {e}", session.join(name).display())));
    let (text, input) = (read("session.txt"), read("input.tsv"));
    let (entries, end) = parse_input(&input).unwrap_or_else(|e| fail(format!("input.tsv: {e}")));
    let out = env("RC_REPLAY_OUT").map(PathBuf::from).unwrap_or_else(|| session.join("replay"));
    let start = out.join("start");
    let _ = std::fs::remove_dir_all(&start);
    if let Err(e) = std::fs::create_dir_all(&start) { fail(format!("{}: {e}", start.display())); }
    for (k, v) in text.lines().filter_map(|l| l.strip_prefix("env ")?.split_once('=')) {
        if !NOT_REPLAYED.contains(&k) && std::env::var_os(k).is_none() { std::env::set_var(k, v); }
    }
    let settings = session.join("start").join("settings.toml");
    let settings = if settings.is_file() {
        let to = start.join("settings.toml");
        std::fs::copy(&settings, &to).unwrap_or_else(|e| fail(format!("{}: {e}", settings.display())));
        to.into_os_string()
    } else {
        "0".into()
    };
    std::env::set_var("RC_SETTINGS_FILE", settings);
    let card = session.join("start").join("memcard");
    let card = if card.is_dir() {
        let to = start.join("memcard");
        crate::saves::copy_dir(&card, &to).unwrap_or_else(|e| fail(format!("{}: {e}", card.display())));
        to.into_os_string()
    } else {
        "0".into()
    };
    std::env::set_var("RC_SAVE_DIR", card);
    if !crate::determinism::deterministic() { std::env::set_var("RC_DETERMINISTIC", "1"); }
    let last = entries.last().map_or(0, |e| e.frame).max(end.unwrap_or(0));
    let until = env("RC_REPLAY_UNTIL").and_then(|v| v.parse().ok()).unwrap_or(last).min(last);
    println!("replay: {} ({} input lines, updates 1..={until}) into {}", session.display(), entries.len(), out.display());
    let _ = DIR.set(out);
    let _ = REPLAY.set(Replay { entries, until });
}

/// The session folder and its log (after crate::standalone's redirection, which the log then also feeds); a replay's
/// log goes into its output folder.
pub fn start() {
    if let Some(out) = DIR.get() {
        if let Err(e) = std::fs::create_dir_all(out).and_then(|()| std::fs::File::create(out.join("log.txt"))).and_then(sys::tee) {
            eprintln!("replay: no log in {}: {e}", out.display());
        }
        return;
    }
    if crate::determinism::deterministic() || env("RC_RECORD").as_deref() == Some("0") { return; }
    let Some(base) = crate::render_settings::settings_path().and_then(|p| p.parent().map(Path::to_path_buf)) else { return };
    let sessions = base.join("sessions");
    let stamp = crate::saves::local_time().map_or_else(
        || std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()).to_string(),
        |[y, mo, d, h, mi, s]| format!("{y:04}-{mo:02}-{d:02}_{h:02}-{mi:02}-{s:02}"),
    );
    let dir = (1..).map(|n| sessions.join(if n == 1 { stamp.clone() } else { format!("{stamp}-{n}") })).find(|d| !d.exists()).unwrap_or_default();
    prune(&sessions);
    if let Err(e) = std::fs::create_dir_all(dir.join("start")) {
        eprintln!("session: cannot create {}: {e}; not recording", dir.display());
        return;
    }
    if let Some(text) = crate::render_settings::effective_text() {
        let to = dir.join("start").join("settings.toml");
        if let Err(e) = std::fs::write(&to, text) { eprintln!("session: cannot write {}: {e}", to.display()); }
    }
    // No card stays no card in the replay; a card folder not made yet is an empty card.
    if let Some(card) = crate::saves::card_root() {
        let to = dir.join("start").join("memcard");
        let res = if card.is_dir() { crate::saves::copy_dir(&card, &to) } else { std::fs::create_dir_all(&to) };
        if let Err(e) = res { eprintln!("session: cannot copy the memory card {}: {e}", card.display()); }
    }
    let mut text = format!("rerac session 1\nversion {}\nargs {}\nlevel {}\n", rc_extract::BUILD_VERSION, std::env::args().skip(1).collect::<Vec<_>>().join(" "), crate::level_load::boot_level_index());
    let mut vars: Vec<(String, String)> = std::env::vars().filter(|(k, _)| k.starts_with("RC_")).collect();
    vars.sort();
    for (k, v) in vars { text += &format!("env {k}={v}\n"); }
    if let Err(e) = std::fs::write(dir.join("session.txt"), text) {
        eprintln!("session: cannot write {}: {e}; not recording", dir.display());
        return;
    }
    if let Err(e) = std::fs::File::create(dir.join("log.txt")).and_then(sys::tee) { eprintln!("session: no log: {e}"); }
    println!("session: recording into {} (F8: a bug marker; RC_RECORD=0: no recording)", dir.display());
    let _ = DIR.set(dir);
}

/// The sessions without markers beyond the [`KEPT`] newest (names sort by time).
fn prune(sessions: &Path) {
    let Ok(rd) = std::fs::read_dir(sessions) else { return };
    let mut old: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.is_dir() && !p.join("markers.tsv").exists()).collect();
    old.sort();
    let extra = (old.len() + 1).saturating_sub(KEPT);
    for p in &old[..extra] { let _ = std::fs::remove_dir_all(p); }
}

fn pad_hex(p: &PadInput) -> String { p.bytes().iter().map(|b| format!("{b:02x}")).collect() }

/// The 18 libpad2 bytes back to a read ([`PadInput::bytes`]' inverse).
fn pad_from(b: &[u8; 18]) -> PadInput {
    let mut p = PadInput::neutral();
    p.buttons = !(((b[0] as u16) << 8) | b[1] as u16);
    (p.rx, p.ry, p.lx, p.ly) = (b[2], b[3], b[4], b[5]);
    p.pressure.copy_from_slice(&b[6..]);
    p
}

fn key_named(s: &str) -> Option<KeyCode> { KEYS.iter().copied().find(|k| format!("{k:?}") == s) }

fn line(e: &Entry) -> String {
    let keys = if e.keys.is_empty() { "-".to_string() } else { e.keys.iter().map(|k| format!("{k:?}")).collect::<Vec<_>>().join(",") };
    format!("{}\t{}\t{}\t{}\t{keys}\n", e.frame, e.steps, e.budget, pad_hex(&e.pad))
}

/// `input.tsv`: the lines and the `# end <update>` trailer of a run that exited normally.
fn parse_input(text: &str) -> Result<(Vec<Entry>, Option<u64>), String> {
    let mut entries: Vec<Entry> = Vec::new();
    let mut end = None;
    for (n, l) in text.lines().enumerate() {
        if let Some(v) = l.strip_prefix("# end ") { end = v.trim().parse().ok(); }
        if l.starts_with('#') || l.trim().is_empty() { continue; }
        let bad = |what: &str| format!("line {}: bad {what}: {l:?}", n + 1);
        let f: Vec<&str> = l.split('\t').collect();
        if f.len() != 5 { return Err(bad("field count")); }
        let hex = f[3].as_bytes();
        if hex.len() != 36 { return Err(bad("pad")); }
        let mut b = [0u8; 18];
        for (i, v) in b.iter_mut().enumerate() {
            *v = std::str::from_utf8(&hex[2 * i..2 * i + 2]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()).ok_or_else(|| bad("pad"))?;
        }
        let keys = if f[4] == "-" { Vec::new() } else { f[4].split(',').map(|k| key_named(k).ok_or_else(|| bad("key"))).collect::<Result<_, _>>()? };
        let e = Entry { frame: f[0].parse().map_err(|_| bad("update"))?, steps: f[1].parse().map_err(|_| bad("steps"))?, budget: f[2].parse().map_err(|_| bad("budget"))?, pad: pad_from(&b), keys };
        if entries.last().is_some_and(|p| p.frame >= e.frame) { return Err(bad("update order")); }
        entries.push(e);
    }
    Ok((entries, end))
}

/// The open `input.tsv` of a recording.
#[derive(Resource)]
pub(crate) struct Recorder {
    file: std::fs::File,
    last: Option<Entry>,
}

impl Recorder {
    fn write(&mut self, e: Entry) {
        let same = self.last.as_ref().is_some_and(|l| (l.steps, l.budget, l.pad, &l.keys) == (e.steps, e.budget, e.pad, &e.keys));
        if same && self.last.as_ref().is_some_and(|l| e.frame < l.frame + HEARTBEAT) { return; }
        let _ = self.file.write_all(line(&e).as_bytes());
        self.last = Some(e);
    }
}

/// The pad and the tick budget of this update (in crate::gameplay's input chain, after its budget): recorded, or in a
/// replay the recorded ones.
pub(crate) fn pad_frame(
    frame: Res<FrameNumber>,
    strategy: Res<TimeUpdateStrategy>,
    keys: Res<ButtonInput<KeyCode>>,
    mut pad: ResMut<PadFrame>,
    mut budget: ResMut<crate::gameplay::TickBudget>,
    rec: Option<ResMut<Recorder>>,
) {
    if let Some(r) = REPLAY.get() {
        if let Some(e) = r.at(frame.0) { (pad.0, budget.0) = (e.pad, e.budget); }
        return;
    }
    let Some(mut rec) = rec else { return };
    let steps = match *strategy {
        TimeUpdateStrategy::FixedTimesteps(n) => n,
        _ => 1,
    };
    // A key pressed and released between two updates counts as held for this one.
    let held = KEYS.iter().copied().filter(|&k| keys.pressed(k) || keys.just_pressed(k)).collect();
    rec.write(Entry { frame: frame.0, steps, budget: budget.0, pad: pad.0, keys: held });
}

/// A replay's steps for the coming update (before the time update), and its end: past the last update the game holds
/// (no steps) and the app exits, unless a frame capture is set, which then ends the run itself.
fn steps(frame: Res<FrameNumber>, mut strategy: ResMut<TimeUpdateStrategy>, mut exit: MessageWriter<AppExit>, mut done: Local<bool>) {
    let Some(r) = REPLAY.get() else { return };
    let next = frame.0 + 1;
    if next > r.until {
        if !std::mem::replace(&mut *done, true) {
            let capture = crate::determinism::screenshot_frame().is_some() || crate::determinism::dump_frames().is_some();
            println!("replay: done after update {}{}", frame.0, if capture { "; holding for the capture" } else { "" });
            if !capture { exit.write(AppExit::Success); }
        }
        *strategy = TimeUpdateStrategy::FixedTimesteps(0);
        return;
    }
    *strategy = TimeUpdateStrategy::FixedTimesteps(r.at(next).map_or(1, |e| e.steps));
}

/// A replay's keys (after the frame-exact mode cleared the live ones): held from the update they were first recorded
/// in, so `just_pressed` / `just_released` fire on the same updates as in the run.
fn keys(frame: Res<FrameNumber>, mut keys: ResMut<ButtonInput<KeyCode>>, mut prev: Local<Vec<KeyCode>>) {
    let Some(r) = REPLAY.get() else { return };
    let now: Vec<KeyCode> = r.at(frame.0).map(|e| e.keys.iter().copied().filter(|&k| k != KeyCode::F11).collect()).unwrap_or_default();
    keys.reset_all();
    for &k in prev.iter() { keys.press(k); }
    keys.clear();
    for &k in prev.iter().filter(|k| !now.contains(k)) { keys.release(k); }
    for &k in &now { keys.press(k); }
    *prev = now;
}

/// F8: a bug marker (module docs).
fn marker(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    frame: Res<FrameNumber>,
    play: Option<Res<crate::gameplay::Play>>,
    game_frame: Res<crate::display::GameFrame>,
    mut n: Local<u32>,
) {
    if !keys.just_pressed(KeyCode::F8) { return; }
    let Some(dir) = dir() else { return };
    *n += 1;
    let level = crate::level_load::level_index();
    let tick = play.as_ref().map_or(0, |p| p.game.counter);
    let place = play.as_ref().map_or_else(
        || "-".to_string(),
        |p| {
            let (at, yaw) = (p.game.hero.position(), rc_game::hero::physics::to_f32x3(p.game.hero.rot)[2]);
            format!("{:.2},{:.2},{:.2},{yaw:.3}", at[0], at[1], at[2])
        },
    );
    let list = dir.join("markers.tsv");
    let header = if list.exists() { "" } else { "# marker\tupdate\ttick\tlevel\tratchet x,y,z,yaw\n" };
    let res = std::fs::OpenOptions::new().create(true).append(true).open(&list).and_then(|mut f| writeln!(f, "{header}{}\t{}\t{tick}\t{level}\t{place}", *n, frame.0));
    if let Err(e) = res { eprintln!("marker: cannot write {}: {e}", list.display()); }
    let png = dir.join(format!("marker_{:02}.png", *n));
    println!("marker {}: update {} tick {tick} level {level} Ratchet {place} (RC_LEVEL={level} RC_HERO_AT={place}); picture {}", *n, frame.0, png.display());
    commands.spawn(Screenshot::image(game_frame.image.clone())).observe(move |shot: On<ScreenshotCaptured>| {
        match shot.image.clone().try_into_dynamic() {
            Ok(img) => {
                if let Err(e) = img.to_rgb8().save(&png) { eprintln!("marker: cannot save {}: {e}", png.display()); }
            }
            Err(e) => eprintln!("marker: cannot convert the picture: {e:?}"),
        }
    });
}

/// The recording's `# end <update>` when the app exits.
fn end(mut exits: MessageReader<AppExit>, frame: Res<FrameNumber>, rec: Option<ResMut<Recorder>>) {
    if exits.read().next().is_none() { return; }
    if let Some(mut rec) = rec {
        let _ = writeln!(rec.file, "# end {}", frame.0);
    }
}

pub struct SessionPlugin;

impl Plugin for SessionPlugin {
    fn build(&self, app: &mut App) {
        if REPLAY.get().is_some() {
            app.add_systems(First, steps.before(bevy::time::TimeSystems)).add_systems(
                PreUpdate,
                keys.after(bevy::input::InputSystems).after(crate::determinism::no_live_input).before(crate::input_map::sample_system),
            );
        } else if let Some(d) = dir() {
            let path = d.join("input.tsv");
            match std::fs::File::create(&path).and_then(|mut f| f.write_all(b"# rerac input 1: update\tsteps\tbudget\tpad\tkeys\n").map(|()| f)) {
                Ok(file) => {
                    app.insert_resource(Recorder { file, last: None }).add_systems(Last, end);
                }
                Err(e) => eprintln!("session: cannot write {}: {e}; no input recording", path.display()),
            }
        }
        if dir().is_some() { app.add_systems(Update, marker); }
    }
}

#[cfg(windows)]
mod sys {
    use std::io::{Read, Write};
    use std::os::windows::io::{FromRawHandle, IntoRawHandle};

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetStdHandle(which: u32) -> isize;
        fn SetStdHandle(which: u32, handle: isize) -> i32;
        fn GetConsoleMode(console: isize, mode: *mut u32) -> i32;
        fn WriteConsoleW(console: isize, text: *const u16, len: u32, written: *mut u32, reserved: *mut std::ffi::c_void) -> i32;
    }

    const STD_OUTPUT_HANDLE: u32 = -11i32 as u32;
    const STD_ERROR_HANDLE: u32 = -12i32 as u32;

    /// stdout and stderr into a pipe (std reads the standard handles on every write) whose thread copies them to `log`
    /// and to the old stdout: a console as UTF-16, anything else as bytes.
    pub fn tee(mut log: std::fs::File) -> std::io::Result<()> {
        let (mut reader, writer) = std::io::pipe()?;
        // SAFETY: the process's standard handles; the pipe's write handle stays open for the process's life.
        let prev = unsafe { GetStdHandle(STD_OUTPUT_HANDLE) };
        let valid = prev != 0 && prev != -1;
        let mut mode = 0;
        // SAFETY: only reads the console mode of a handle (fails on a non-console).
        let console = valid && unsafe { GetConsoleMode(prev, &mut mode) } != 0;
        let handle = writer.into_raw_handle() as isize;
        std::thread::Builder::new().name("session log".into()).spawn(move || {
            // SAFETY: the old stdout handle stays open (never closed here: ManuallyDrop).
            let mut out = (valid && !console).then(|| std::mem::ManuallyDrop::new(unsafe { std::fs::File::from_raw_handle(prev as _) }));
            let (mut buf, mut pending) = ([0u8; 8192], Vec::new());
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 { break; }
                let _ = log.write_all(&buf[..n]);
                if let Some(o) = out.as_mut() { let _ = o.write_all(&buf[..n]); }
                if !console { continue; }
                pending.extend_from_slice(&buf[..n]);
                let take = match std::str::from_utf8(&pending) {
                    Ok(s) => s.len(),
                    Err(e) if e.error_len().is_some() => pending.len(),
                    Err(e) => e.valid_up_to(),
                };
                let text: Vec<u16> = String::from_utf8_lossy(&pending[..take]).encode_utf16().collect();
                pending.drain(..take);
                let mut written = 0;
                // SAFETY: `text` outlives the call.
                unsafe { WriteConsoleW(prev, text.as_ptr(), text.len() as u32, &mut written, std::ptr::null_mut()) };
            }
        })?;
        // SAFETY: as above.
        unsafe {
            SetStdHandle(STD_OUTPUT_HANDLE, handle);
            SetStdHandle(STD_ERROR_HANDLE, handle);
        }
        Ok(())
    }
}

#[cfg(unix)]
mod sys {
    use std::io::{Read, Write};
    use std::os::fd::{FromRawFd, IntoRawFd};

    /// stdout and stderr into a pipe whose thread copies them to `log` and to the old stdout.
    pub fn tee(mut log: std::fs::File) -> std::io::Result<()> {
        let (mut reader, writer) = std::io::pipe()?;
        // SAFETY: duplicates the open stdout, then the pipe over stdout and stderr.
        let prev = unsafe { libc::dup(1) };
        let fd = writer.into_raw_fd();
        std::thread::Builder::new().name("session log".into()).spawn(move || {
            // SAFETY: `prev` is this thread's own duplicate.
            let mut out = (prev >= 0).then(|| unsafe { std::fs::File::from_raw_fd(prev) });
            let mut buf = [0u8; 8192];
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 { break; }
                let _ = log.write_all(&buf[..n]);
                if let Some(o) = out.as_mut() { let _ = o.write_all(&buf[..n]); }
            }
        })?;
        // SAFETY: as above.
        unsafe {
            libc::dup2(fd, 1);
            libc::dup2(fd, 2);
            libc::close(fd);
        }
        Ok(())
    }
}

#[cfg(not(any(windows, unix)))]
mod sys {
    pub fn tee(_log: std::fs::File) -> std::io::Result<()> { Ok(()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rc_game::pad::button;

    #[test]
    fn input_lines_round_trip() {
        let a = Entry { frame: 1, steps: 1, budget: 1, pad: PadInput::neutral(), keys: vec![] };
        let b = Entry { frame: 7, steps: 2, budget: 2, pad: PadInput::neutral().stick(0.0, -1.0).press(button::CROSS | button::R1), keys: vec![KeyCode::KeyW, KeyCode::F8] };
        let text = format!("# rerac input 1\n{}{}# end 90\n", line(&a), line(&b));
        let (entries, end) = parse_input(&text).unwrap();
        assert_eq!(entries, vec![a.clone(), b.clone()]);
        assert_eq!(end, Some(90));
        let r = Replay { entries, until: 90 };
        assert_eq!(r.at(6).map(|e| e.frame), Some(1));
        assert_eq!(r.at(7).map(|e| e.steps), Some(2));
        assert_eq!(r.at(90).map(|e| e.keys.len()), Some(2));
        assert!(parse_input("3\t1\t1\tzz\t-\n").is_err());
        assert!(parse_input(&format!("{}{}", line(&b), line(&a))).is_err());
    }

    #[test]
    fn pad_bytes_round_trip() {
        let p = PadInput::neutral().rstick(0.5, 1.0).press(button::L2 | button::TRIANGLE | button::START);
        assert_eq!(pad_from(&p.bytes()), p);
        assert_eq!(pad_from(&PadInput::neutral().bytes()), PadInput::neutral());
    }

    #[test]
    fn every_key_name_parses() {
        for &k in KEYS { assert_eq!(key_named(&format!("{k:?}")), Some(k)); }
    }
}
