//! A packaged build started on its own, not from a terminal (crate::disc_source's portable data folder; the launcher
//! always passes `--data-dir` and keeps its own logs): the session's stdout and stderr go to
//! `logs/rerac-<unix time>.log` next to the executable (the [`KEPT`] newest are kept), a fatal error or a panic is
//! also shown in a dialog (crate::dialog), and on Windows the console window a double-click opens shows only the
//! first start's progress ([`say`]) and closes when the game window opens ([`hide_console`]). From a terminal
//! nothing changes: the output stays there and errors are its last lines.
//!
//! "Not from a terminal": on Windows the console has no other process attached (a double-click or Explorer made it
//! for this one) and stdout is that console; elsewhere stdout is not a terminal (macOS UNTESTED).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

/// Session logs kept, the new one included.
const KEPT: usize = 10;

struct Session {
    log: PathBuf,
    /// The console's own output handle, saved before stdout moved to the log (Windows; 0 elsewhere).
    console: isize,
}

static SESSION: OnceLock<Session> = OnceLock::new();
static CONSOLE_HIDDEN: AtomicBool = AtomicBool::new(false);
static CRASH_SHOWN: AtomicBool = AtomicBool::new(false);

/// Starts the session log in `<dir>/logs` when the game runs on its own (module docs); otherwise does nothing.
pub fn start(dir: &Path) {
    if !alone() { return; }
    let logs = dir.join("logs");
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let log = logs.join(format!("rerac-{secs}.log"));
    let file = match std::fs::create_dir_all(&logs).and_then(|()| std::fs::File::create(&log)) {
        Ok(f) => f,
        Err(e) => { eprintln!("warning: cannot write the session log {}: {e}", log.display()); return; }
    };
    prune(&logs);
    println!("session log: {}", log.display());
    let console = redirect(file);
    let _ = SESSION.set(Session { log, console });
    println!("ReRAC {} session log", rc_extract::BUILD_VERSION);
}

/// The log's older sessions beyond the [`KEPT`] newest (names sort by time: same digit count until 2286).
fn prune(logs: &Path) {
    let Ok(dir) = std::fs::read_dir(logs) else { return };
    let mut old: Vec<PathBuf> = dir
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("rerac-") && n.ends_with(".log")))
        .collect();
    old.sort();
    let extra = (old.len() + 1).saturating_sub(KEPT);
    for p in &old[..extra] { let _ = std::fs::remove_file(p); }
}

/// One line for the player while the console shows (crate::first_run's progress); always in stdout too.
pub fn say(line: &str) {
    println!("{line}");
    if let Some(s) = SESSION.get().filter(|s| s.console != 0 && !CONSOLE_HIDDEN.load(Ordering::Relaxed)) {
        console_write(s.console, line);
    }
}

/// Closes the session's console window (Windows), right before the game window opens.
pub fn hide_console() {
    if SESSION.get().is_some_and(|s| s.console != 0) && !CONSOLE_HIDDEN.swap(true, Ordering::Relaxed) {
        free_console();
    }
}

/// A fatal error: its `error:` line (the launcher contract's last line), and a dialog when the game runs on its own.
/// The caller exits.
pub fn fatal(message: &str) {
    eprintln!("error: {message}");
    if let Some(s) = SESSION.get() {
        crate::dialog::error(&format!("ReRAC cannot start:\n\n{message}\n\nThe session log is {}", s.log.display()));
    }
}

/// The panic hook's dialog (crate::crash_log), once per process.
pub fn crashed(info: &str, crash_log: Option<&Path>) {
    let Some(s) = SESSION.get() else { return };
    if CRASH_SHOWN.swap(true, Ordering::Relaxed) { return; }
    let details = crash_log.map_or(String::new(), |p| format!("\n\nDetails: {}", p.display()));
    crate::dialog::error(&format!("ReRAC crashed:\n\n{info}{details}\nSession log: {}", s.log.display()));
}

#[cfg(windows)]
mod sys {
    use std::ffi::c_void;
    use std::io::IsTerminal;
    use std::os::windows::io::IntoRawHandle;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetConsoleProcessList(list: *mut u32, count: u32) -> u32;
        fn GetStdHandle(which: u32) -> isize;
        fn SetStdHandle(which: u32, handle: isize) -> i32;
        fn WriteConsoleW(console: isize, text: *const u16, len: u32, written: *mut u32, reserved: *mut c_void) -> i32;
        fn FreeConsole() -> i32;
    }

    const STD_OUTPUT_HANDLE: u32 = -11i32 as u32;
    const STD_ERROR_HANDLE: u32 = -12i32 as u32;

    pub fn alone() -> bool {
        let mut pids = [0u32; 2];
        // SAFETY: the buffer holds the two ids asked for; the result is the number of attached processes.
        let attached = unsafe { GetConsoleProcessList(pids.as_mut_ptr(), pids.len() as u32) };
        attached == 1 && std::io::stdout().is_terminal()
    }

    /// stdout and stderr to `file` (std reads the standard handles on every write); returns the console's handle.
    pub fn redirect(file: std::fs::File) -> isize {
        let handle = file.into_raw_handle() as isize;
        // SAFETY: the process's standard handles; `handle` is a file handle left open for the process's life.
        unsafe {
            let console = GetStdHandle(STD_OUTPUT_HANDLE);
            SetStdHandle(STD_OUTPUT_HANDLE, handle);
            SetStdHandle(STD_ERROR_HANDLE, handle);
            console
        }
    }

    pub fn console_write(console: isize, line: &str) {
        let text: Vec<u16> = line.encode_utf16().chain("\r\n".encode_utf16()).collect();
        let mut written = 0;
        // SAFETY: `text` outlives the call; a stale console handle only makes the call fail.
        unsafe { WriteConsoleW(console, text.as_ptr(), text.len() as u32, &mut written, std::ptr::null_mut()) };
    }

    pub fn free_console() {
        // SAFETY: detaches from the console; stdout and stderr already point at the log.
        unsafe { FreeConsole() };
    }
}

#[cfg(unix)]
mod sys {
    use std::io::IsTerminal;
    use std::os::fd::IntoRawFd;

    pub fn alone() -> bool { !std::io::stdout().is_terminal() }

    pub fn redirect(file: std::fs::File) -> isize {
        let fd = file.into_raw_fd();
        // SAFETY: duplicates an open descriptor over stdout and stderr.
        unsafe {
            libc::dup2(fd, 1);
            libc::dup2(fd, 2);
        }
        0
    }

    pub fn console_write(_console: isize, _line: &str) {}

    pub fn free_console() {}
}

use sys::{alone, console_write, free_console, redirect};
