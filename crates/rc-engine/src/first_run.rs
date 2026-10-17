//! The portable first start: a packaged build started without `--data-dir` keeps its game data in `data/` next to
//! the executable (crate::disc_source). When that folder is missing, incomplete or of another data format, the player
//! picks their disc image (`.iso`) in the system's file dialog and the extractor (`rc_extract`, in-process) writes the
//! folder before the window opens. `RC_ISO=<image>` skips the first prompt and the dialog.
//!
//! Dialogs: Windows `MessageBoxW` / `GetOpenFileNameW`; macOS `osascript` (UNTESTED); other Unix `zenity`, else
//! `kdialog`, else the terminal (UNTESTED).

use rc_extract::build_db::BUILDS;
use rc_extract::extract::{self, Options};
use rc_extract::{mib, Event, Stage, DEFAULT_THREADS};
use std::io::Write;
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
        println!("extracting {} into {} (one time only)", path.display(), out.display());
        match run(&path, out) {
            Ok(line) => { println!("{line}"); return true; }
            Err(e) => {
                eprintln!("{e}");
                if !dialog::retry(&format!("The disc image could not be extracted:\n\n{}\n\nChoose another .iso?", e.message)) {
                    return false;
                }
            }
        }
    }
}

/// `rerac-extract extract` (which ends with `prepare`), printing each stage's progress in tenths.
fn run(iso: &Path, out: &Path) -> Result<String, rc_extract::Error> {
    let t0 = Instant::now();
    let mut last: Option<(Stage, u64)> = None;
    let mut emit = |e: Event| {
        let mut o = std::io::stdout().lock();
        let _ = match e {
            Event::Progress { stage, done, total, .. } => {
                let tenth = (done * 10).checked_div(total).unwrap_or(10);
                if last == Some((stage, tenth)) { return; }
                last = Some((stage, tenth));
                writeln!(o, "{:<8} {:3}%  {:.0} / {:.0} MiB", stage.name(), tenth * 10, mib(done), mib(total))
            }
            Event::Info(m) => writeln!(o, "{m}"),
            Event::Disc(d) => writeln!(o, "disc: {} ({}, v{})", d.serial, d.region, d.version),
        };
        let _ = o.flush();
    };
    let opts = Options { ntsc_only: false, threads: DEFAULT_THREADS, cancel: None };
    let (_, s) = extract::extract(iso, out, BUILDS, &opts, &mut emit)?;
    Ok(format!("extracted {} files, {:.1} MiB in {:.1} s", s.files, mib(s.bytes), t0.elapsed().as_secs_f64()))
}

#[cfg(windows)]
mod dialog {
    use std::ffi::{c_void, OsString};
    use std::os::windows::ffi::OsStringExt;
    use std::path::PathBuf;
    use std::ptr::{null, null_mut};

    /// OPENFILENAMEW (commdlg.h).
    #[repr(C)]
    struct OpenFileName {
        struct_size: u32,
        owner: *mut c_void,
        instance: *mut c_void,
        filter: *const u16,
        custom_filter: *mut u16,
        max_cust_filter: u32,
        filter_index: u32,
        file: *mut u16,
        max_file: u32,
        file_title: *mut u16,
        max_file_title: u32,
        initial_dir: *const u16,
        title: *const u16,
        flags: u32,
        file_offset: u16,
        file_extension: u16,
        def_ext: *const u16,
        cust_data: isize,
        hook: *mut c_void,
        template_name: *const u16,
        reserved_ptr: *mut c_void,
        reserved: u32,
        flags_ex: u32,
    }

    #[link(name = "comdlg32")]
    unsafe extern "system" { fn GetOpenFileNameW(ofn: *mut OpenFileName) -> i32; }
    #[link(name = "user32")]
    unsafe extern "system" { fn MessageBoxW(owner: *mut c_void, text: *const u16, caption: *const u16, kind: u32) -> i32; }

    const MB_OKCANCEL: u32 = 0x1;
    const MB_RETRYCANCEL: u32 = 0x5;
    const MB_ICONWARNING: u32 = 0x30;
    const MB_ICONINFORMATION: u32 = 0x40;
    const MB_SETFOREGROUND: u32 = 0x1_0000;
    const IDOK: i32 = 1;
    const IDRETRY: i32 = 4;
    const OFN_NOCHANGEDIR: u32 = 0x8;
    const OFN_PATHMUSTEXIST: u32 = 0x800;
    const OFN_FILEMUSTEXIST: u32 = 0x1000;
    const OFN_EXPLORER: u32 = 0x8_0000;

    fn wide(s: &str) -> Vec<u16> { s.encode_utf16().chain([0]).collect() }

    fn message(text: &str, kind: u32) -> i32 {
        let (text, caption) = (wide(text), wide("ReRAC"));
        // SAFETY: both strings are NUL-terminated and outlive the call; no owner window.
        unsafe { MessageBoxW(null_mut(), text.as_ptr(), caption.as_ptr(), kind | MB_SETFOREGROUND) }
    }

    pub fn confirm(text: &str) -> bool { message(text, MB_OKCANCEL | MB_ICONINFORMATION) == IDOK }

    pub fn retry(text: &str) -> bool { message(text, MB_RETRYCANCEL | MB_ICONWARNING) == IDRETRY }

    pub fn pick_iso() -> Option<PathBuf> {
        // Pairs of NUL-terminated strings, ended by an empty one.
        let filter: Vec<u16> = "Disc images (*.iso)\0*.iso\0All files (*.*)\0*.*\0\0".encode_utf16().collect();
        let title = wide("Choose your Ratchet & Clank disc image (.iso)");
        let mut file = vec![0u16; 32 * 1024];
        let mut ofn = OpenFileName {
            struct_size: size_of::<OpenFileName>() as u32,
            owner: null_mut(),
            instance: null_mut(),
            filter: filter.as_ptr(),
            custom_filter: null_mut(),
            max_cust_filter: 0,
            filter_index: 1,
            file: file.as_mut_ptr(),
            max_file: file.len() as u32,
            file_title: null_mut(),
            max_file_title: 0,
            initial_dir: null(),
            title: title.as_ptr(),
            flags: OFN_EXPLORER | OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR,
            file_offset: 0,
            file_extension: 0,
            def_ext: null(),
            cust_data: 0,
            hook: null_mut(),
            template_name: null(),
            reserved_ptr: null_mut(),
            reserved: 0,
            flags_ex: 0,
        };
        // SAFETY: every pointer in `ofn` points into a buffer that outlives the call; `file` holds `max_file` u16s.
        if unsafe { GetOpenFileNameW(&mut ofn) } == 0 { return None; }
        let n = file.iter().position(|&c| c == 0).unwrap_or(file.len());
        Some(OsString::from_wide(&file[..n]).into())
    }
}

#[cfg(target_os = "macos")]
mod dialog {
    use std::path::PathBuf;
    use std::process::Command;

    /// The script's stdout, `None` when it failed (a Cancel button is an error to AppleScript).
    fn osascript(script: &str) -> Option<String> {
        let o = Command::new("osascript").arg("-e").arg(script).output().ok()?;
        o.status.success().then(|| String::from_utf8_lossy(&o.stdout).trim_end().to_string())
    }

    fn quote(s: &str) -> String { format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"")) }

    fn ask(text: &str, ok: &str) -> bool {
        osascript(&format!(
            "display dialog {} with title \"ReRAC\" buttons {{\"Quit\", {}}} default button 2 cancel button 1",
            quote(text), quote(ok)
        ))
        .is_some()
    }

    pub fn confirm(text: &str) -> bool { ask(text, "Choose .iso") }

    pub fn retry(text: &str) -> bool { ask(text, "Choose again") }

    pub fn pick_iso() -> Option<PathBuf> {
        osascript("POSIX path of (choose file with prompt \"Choose your Ratchet & Clank disc image (.iso)\")")
            .filter(|p| !p.is_empty())
            .map(PathBuf::from)
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
mod dialog {
    use std::io::{BufRead, IsTerminal, Write};
    use std::path::PathBuf;
    use std::process::Command;

    /// Exit status and stdout of `cmd`; `None` when it is not installed.
    fn tool(cmd: &str, args: &[&str]) -> Option<(bool, String)> {
        let o = Command::new(cmd).args(args).output().ok()?;
        Some((o.status.success(), String::from_utf8_lossy(&o.stdout).trim_end().to_string()))
    }

    fn ask(text: &str, ok: &str) -> bool {
        let ok_label = format!("--ok-label={ok}");
        let text_arg = format!("--text={text}");
        if let Some((yes, _)) = tool("zenity", &["--question", "--no-markup", "--title=ReRAC", &text_arg, &ok_label, "--cancel-label=Quit"]) {
            return yes;
        }
        if let Some((yes, _)) = tool("kdialog", &["--title", "ReRAC", "--yesno", text]) { return yes; }
        eprintln!("{text}");
        std::io::stdin().is_terminal()
    }

    pub fn confirm(text: &str) -> bool { ask(text, "Choose .iso") }

    pub fn retry(text: &str) -> bool { ask(text, "Choose again") }

    pub fn pick_iso() -> Option<PathBuf> {
        let title = "Choose your Ratchet & Clank disc image (.iso)";
        let picked = tool("zenity", &["--file-selection", &format!("--title={title}"), "--file-filter=Disc images | *.iso *.ISO"])
            .or_else(|| tool("kdialog", &["--title", title, "--getopenfilename", ".", "*.iso *.ISO|Disc images"]));
        let path = match picked {
            Some((ok, path)) => ok.then_some(path),
            None if std::io::stdin().is_terminal() => {
                print!("Path to your disc image (.iso): ");
                let _ = std::io::stdout().flush();
                let mut line = String::new();
                std::io::stdin().lock().read_line(&mut line).ok().map(|_| line.trim().to_string())
            }
            None => None,
        };
        path.filter(|p| !p.is_empty()).map(PathBuf::from)
    }
}
