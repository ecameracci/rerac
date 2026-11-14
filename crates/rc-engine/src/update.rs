//! **Fork-only**: Check for Update (docs/plan/check_for_update.md), the engine side of the main menu's page
//! (`rc_game::menus::pause::update`). The network is touched only when the page opens ([`check`]) and on Update
//! ([`install`]), each on its own thread; the menu reads [`view`] every front-end tick.
//!
//! * **Check**: GitHub's release list of [`REPO`] (`releases/latest` skips prereleases, and every release so far is
//!   one); the newest release with a zip for this build's platform ([`choose`]). Newer means a higher `X.Y.Z` than the
//!   build's ([`is_newer`]): the build version (`<version>-<hash>-<platform>`, `crates/rc-extract/build.rs`) carries
//!   no `-pre`, so a release of the same `X.Y.Z` is never offered. Offline or any error: "Couldn't check".
//!   `RC_UPDATE_AS=<X.Y.Z>` compares as that version instead (a test switch: offers the newest release to any build).
//! * **Install**: only from a packaged folder (`rerac-manifest.json` and `assets/shaders/` beside the executable).
//!   Downloads `rerac-<X.Y.Z>-<os>-<arch>.zip` (docs/workflows/release.md) into [`STAGING`] beside the game, unpacks
//!   its packaged files (the release allow-list, [`packaged`]) to `new/`, runs the new `rerac --version-json` and
//!   checks it names the release's version and this platform, then swaps the files in: each old file is renamed into
//!   `old/` first (Windows allows renaming a running `rerac.exe`, not overwriting it) and every move is undone on a
//!   failure. Settings, `data/` and `mods/` are not in the allow-list, so they stay. Then the new executable starts
//!   with this process's arguments and this one exits ([`take_restart`]); its start removes [`STAGING`] ([`cleanup`]).
//!
//! HTTPS is `ureq` with rustls (ring) and the webpki roots: the smallest pure-Rust client (no OpenSSL / system TLS,
//! the same on all three platforms); the JSON (`serde_json`) and the zip's deflate (`flate2`) were already in the
//! build through Bevy.

use anyhow::{anyhow, bail, ensure, Context, Result};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// The fork whose releases are offered.
pub const REPO: &str = "ecameracci/rerac";
/// The work folder beside the executable (download, `new/`, `old/`).
pub const STAGING: &str = ".rerac-update";
const USER_AGENT: &str = "rerac-update";
/// `std::env::consts::EXE_SUFFIX` on the executables' names.
const EXE: &str = std::env::consts::EXE_SUFFIX;

/// A release version `X.Y.Z[-pre]`; prereleases order alpha < beta < rc < none (`rc2` after `rc1`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    pub core: (u32, u32, u32),
    /// (rank, number): alpha 0, beta 1, rc 2, a release 3; unknown tags rank as alpha.
    pub pre: (u8, u32),
}

impl Version {
    /// A tag (`v0.6.0-beta`) or plain version (`0.6.0`).
    pub fn parse(s: &str) -> Option<Version> {
        let s = s.strip_prefix('v').unwrap_or(s);
        let (core, pre) = s.split_once('-').map_or((s, None), |(c, p)| (c, Some(p)));
        let mut it = core.split('.').map(|n| n.parse::<u32>().ok());
        let core = (it.next()??, it.next()??, it.next()??);
        if it.next().is_some() { return None; }
        let pre = match pre {
            None => (3, 0),
            Some(p) => {
                let p = p.to_ascii_lowercase();
                let name = p.trim_end_matches(|c: char| c.is_ascii_digit() || c == '.');
                let n = p[name.len()..].trim_start_matches('.').parse().unwrap_or(0);
                (match name { "beta" => 1, "rc" => 2, _ => 0 }, n)
            }
        };
        Some(Version { core, pre })
    }

    /// `X.Y.Z`.
    pub fn core_str(&self) -> String { format!("{}.{}.{}", self.core.0, self.core.1, self.core.2) }
}

/// The build version's `X.Y.Z` and platform (`0.5.3-9eb6b3f8-windows-x86_64` → `0.5.3`, `windows-x86_64`).
pub fn build_parts(build: &str) -> Option<(Version, &str)> {
    let mut it = build.splitn(3, '-');
    let core = Version::parse(it.next()?)?;
    let _hash = it.next()?;
    Some((core, it.next().filter(|p| !p.is_empty())?))
}

/// A release is offered when its `X.Y.Z` is above the build's (the build has no `-pre`).
pub fn is_newer(release: &Version, build: &Version) -> bool { release.core > build.core }

/// A release asset.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Asset {
    pub name: String,
    pub url: String,
    pub size: u64,
}

/// A published release (drafts left out).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    pub tag: String,
    pub version: Version,
    pub assets: Vec<Asset>,
}

/// The zip of `version` for `platform` (`rerac-0.5.3-windows-x86_64.zip`; docs/workflows/release.md).
pub fn asset_name(version: &Version, platform: &str) -> String { format!("rerac-{}-{platform}.zip", version.core_str()) }

/// GitHub's `GET /repos/{repo}/releases` body: the releases with a version tag, drafts left out.
pub fn parse_releases(json: &str) -> Result<Vec<Release>> {
    let v: serde_json::Value = serde_json::from_str(json)?;
    let list = v.as_array().ok_or_else(|| anyhow!("not a release list"))?;
    let s = |r: &serde_json::Value, k: &str| r.get(k).and_then(|x| x.as_str()).unwrap_or_default().to_string();
    Ok(list
        .iter()
        .filter(|r| !r.get("draft").and_then(|d| d.as_bool()).unwrap_or(false))
        .filter_map(|r| {
            let tag = s(r, "tag_name");
            let version = Version::parse(&tag)?;
            let assets = r.get("assets").and_then(|a| a.as_array()).map_or(Vec::new(), |a| {
                a.iter()
                    .map(|x| Asset { name: s(x, "name"), url: s(x, "browser_download_url"), size: x.get("size").and_then(|n| n.as_u64()).unwrap_or(0) })
                    .collect()
            });
            Some(Release { tag, version, assets })
        })
        .collect())
}

/// The newest release and its zip for `platform`: the newest that has one, else the newest (without a zip).
pub fn choose<'a>(releases: &'a [Release], platform: &str) -> Option<(&'a Release, Option<&'a Asset>)> {
    let zip = |r: &'a Release| r.assets.iter().find(|a| a.name == asset_name(&r.version, platform));
    releases
        .iter()
        .filter_map(|r| zip(r).map(|a| (r, Some(a))))
        .max_by_key(|(r, _)| r.version)
        .or_else(|| releases.iter().max_by_key(|r| r.version).map(|r| (r, None)))
}

/// Where the check and the install stand.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
enum Status {
    #[default]
    Idle,
    Checking,
    Failed,
    Checked { release: Release, asset: Option<Asset>, newer: bool },
    Downloading { release: Release, got: u64, total: u64 },
    Installing { release: Release },
    InstallFailed { release: Release, asset: Asset, why: String },
    Restart { exe: PathBuf },
}

static STATUS: Mutex<Status> = Mutex::new(Status::Idle);

fn status() -> MutexGuard<'static, Status> { STATUS.lock().unwrap_or_else(|e| e.into_inner()) }

/// What the page shows: (this version, newest, status line, Update offered, busy).
pub fn view() -> (String, String, String, bool, bool) {
    let current = rc_extract::BUILD_VERSION.to_string();
    let shown = |r: &Release| r.tag.trim_start_matches('v').to_string();
    match &*status() {
        Status::Idle | Status::Checking => (current, "Checking...".into(), String::new(), false, false),
        Status::Failed => (current, "Couldn't check".into(), String::new(), false, false),
        Status::Checked { release, asset, newer } => {
            let line = match (newer, asset) {
                (false, _) => "Up to date",
                (true, None) => "No download for this system yet",
                (true, Some(_)) => "An update is available",
            };
            (current, shown(release), line.into(), *newer && asset.is_some(), false)
        }
        Status::Downloading { release, got, total } => {
            let mb = |b: u64| (b + (1 << 19)) >> 20;
            (current, shown(release), format!("Downloading... {} / {} MB", mb(*got), mb(*total)), false, true)
        }
        Status::Installing { release } => (current, shown(release), "Installing...".into(), false, true),
        Status::InstallFailed { release, why, .. } => (current, shown(release), format!("Update failed: {why}"), true, false),
        Status::Restart { .. } => (current, String::new(), "Restarting...".into(), false, true),
    }
}

/// The page opened: list the releases (a thread), unless a download or install is running.
pub fn check() {
    {
        let mut s = status();
        if matches!(*s, Status::Downloading { .. } | Status::Installing { .. } | Status::Restart { .. }) { return; }
        *s = Status::Checking;
    }
    std::thread::spawn(|| {
        let next = match fetch_releases() {
            Ok(list) => {
                let as_ = std::env::var("RC_UPDATE_AS").ok().and_then(|v| Version::parse(v.trim()));
                let build = build_parts(rc_extract::BUILD_VERSION).map(|(b, p)| (as_.unwrap_or(b), p));
                match (build, build.and_then(|(_, p)| choose(&list, p))) {
                    (Some((b, _)), Some((r, a))) => {
                        println!("update: newest {} ({}), this build {}", r.tag, a.map_or("no zip for this platform", |a| a.name.as_str()), rc_extract::BUILD_VERSION);
                        Status::Checked { release: r.clone(), asset: a.cloned(), newer: is_newer(&r.version, &b) }
                    }
                    _ => {
                        eprintln!("update: no release found (build {})", rc_extract::BUILD_VERSION);
                        Status::Failed
                    }
                }
            }
            Err(e) => {
                eprintln!("update: couldn't check: {e:#}");
                Status::Failed
            }
        };
        let mut s = status();
        if *s == Status::Checking { *s = next; }
    });
}

/// ✕ on Update: download and install the offered release (a thread).
pub fn install() {
    let (release, asset) = {
        let mut s = status();
        let (release, asset) = match &*s {
            Status::Checked { release, asset: Some(a), newer: true } => (release.clone(), a.clone()),
            Status::InstallFailed { release, asset, .. } => (release.clone(), asset.clone()),
            _ => return,
        };
        *s = Status::Downloading { release: release.clone(), got: 0, total: asset.size };
        (release, asset)
    };
    std::thread::spawn(move || {
        let next = match install_release(&release, &asset) {
            Ok(exe) => {
                println!("update: {} installed, restarting {}", release.tag, exe.display());
                Status::Restart { exe }
            }
            Err(e) => {
                eprintln!("update: {} failed: {e:#}", release.tag);
                let why = format!("{e}");
                Status::InstallFailed { release, asset, why }
            }
        };
        *status() = next;
    });
}

/// The installed executable to start, once (the caller exits the app after starting it).
pub fn take_restart() -> Option<PathBuf> {
    let mut s = status();
    let Status::Restart { exe } = &*s else { return None };
    let exe = exe.clone();
    *s = Status::Idle;
    Some(exe)
}

/// Starts the installed executable with this process's arguments.
pub fn restart(exe: &Path) -> Result<()> {
    std::process::Command::new(exe).args(std::env::args_os().skip(1)).spawn().with_context(|| format!("start {}", exe.display()))?;
    Ok(())
}

/// At start: removes a finished update's [`STAGING`] beside the executable (a thread: on Windows the previous
/// process may still hold its renamed `rerac.exe` for a moment).
pub fn cleanup() {
    let Some(dir) = game_dir() else { return };
    let stage = dir.join(STAGING);
    if !stage.exists() { return; }
    std::thread::spawn(move || {
        for _ in 0..20 {
            if std::fs::remove_dir_all(&stage).is_ok() || !stage.exists() { return; }
            std::thread::sleep(Duration::from_millis(500));
        }
    });
}

fn agent(body_timeout: Option<Duration>) -> ureq::Agent {
    ureq::Agent::config_builder()
        .user_agent(USER_AGENT)
        .timeout_connect(Some(Duration::from_secs(15)))
        .timeout_recv_response(Some(Duration::from_secs(30)))
        .timeout_recv_body(body_timeout)
        .build()
        .into()
}

fn fetch_releases() -> Result<Vec<Release>> {
    let url = format!("https://api.github.com/repos/{REPO}/releases?per_page=30");
    let mut resp = agent(Some(Duration::from_secs(30))).get(&url).header("Accept", "application/vnd.github+json").call()?;
    let body = resp.body_mut().with_config().limit(8 << 20).read_to_string()?;
    parse_releases(&body)
}

/// The folder of the (symlink-resolved) executable.
fn game_dir() -> Option<PathBuf> { std::env::current_exe().and_then(std::fs::canonicalize).ok()?.parent().map(Path::to_path_buf) }

fn install_release(release: &Release, asset: &Asset) -> Result<PathBuf> {
    let dir = game_dir().ok_or_else(|| anyhow!("no game folder"))?;
    ensure!(dir.join("rerac-manifest.json").is_file() && dir.join("assets/shaders").is_dir(), "not a packaged build");
    let platform = build_parts(rc_extract::BUILD_VERSION).map(|(_, p)| p).ok_or_else(|| anyhow!("unknown platform"))?;
    let stage = dir.join(STAGING);
    if stage.exists() { std::fs::remove_dir_all(&stage).with_context(|| format!("remove {}", stage.display()))?; }
    std::fs::create_dir_all(&stage)?;
    let zip = stage.join("download.zip");
    download(&asset.url, &zip, |got, total| {
        if let Status::Downloading { got: g, total: t, .. } = &mut *status() { (*g, *t) = (got, total); }
    })
    .context("download")?;
    *status() = Status::Installing { release: release.clone() };
    let new = stage.join("new");
    let files = unpack(&std::fs::read(&zip)?, &new).context("unpack")?;
    verify(&new.join(format!("rerac{EXE}")), &release.version, platform).context("check")?;
    swap(&dir, &new, &stage.join("old"), &files).context("swap")?;
    Ok(dir.join(format!("rerac{EXE}")))
}

fn download(url: &str, to: &Path, mut progress: impl FnMut(u64, u64)) -> Result<()> {
    let mut resp = agent(None).get(url).call()?;
    let total = resp.body().content_length().unwrap_or(0);
    let mut r = resp.body_mut().with_config().limit(u64::MAX).reader();
    let mut f = std::fs::File::create(to)?;
    let (mut got, mut buf) = (0u64, vec![0u8; 1 << 16]);
    loop {
        let n = r.read(&mut buf)?;
        if n == 0 { break; }
        f.write_all(&buf[..n])?;
        got += n as u64;
        progress(got, total.max(got));
    }
    ensure!(total == 0 || got == total, "short download ({got} of {total} bytes)");
    f.sync_all()?;
    Ok(())
}

/// A packaged file's path inside the version folder (docs/workflows/release.md's allow-list), `/`-separated.
pub fn packaged(rel: &str) -> bool {
    let exe = |n: &str| rel == n || rel == format!("{n}.exe");
    if exe("rerac") || exe("rerac-extract") || rel == "rerac-manifest.json" || rel == "README.txt" { return true; }
    rel.strip_prefix("assets/shaders/").is_some_and(|n| n.ends_with(".wgsl") && !n.contains('/') && !n.contains('\\') && !n.starts_with('.'))
}

fn u16_at(b: &[u8], o: usize) -> Result<usize> { Ok(u16::from_le_bytes(b.get(o..o + 2).ok_or_else(|| anyhow!("truncated zip"))?.try_into()?) as usize) }
fn u32_at(b: &[u8], o: usize) -> Result<u32> { Ok(u32::from_le_bytes(b.get(o..o + 4).ok_or_else(|| anyhow!("truncated zip"))?.try_into()?)) }

/// Unpacks the packaged files of a release zip (one top folder, stored or deflated entries) into `to`, the top
/// folder dropped; everything else is skipped. Returns their paths (`/`-separated). Each entry's CRC-32 is checked.
pub fn unpack(zip: &[u8], to: &Path) -> Result<Vec<String>> {
    // End of central directory: the last "PK\5\6" within the 64 KiB comment window.
    let from = zip.len().saturating_sub(22 + 0xffff);
    let eocd = (from..zip.len().saturating_sub(21)).rev().find(|&i| zip[i..i + 4] == *b"PK\x05\x06").ok_or_else(|| anyhow!("not a zip"))?;
    let (count, mut at) = (u16_at(zip, eocd + 10)?, u32_at(zip, eocd + 16)? as usize);
    let mut files = Vec::new();
    for _ in 0..count {
        ensure!(u32_at(zip, at)? == 0x0201_4b50, "bad central directory");
        let (method, crc, csize, usize_) = (u16_at(zip, at + 10)?, u32_at(zip, at + 16)?, u32_at(zip, at + 20)? as usize, u32_at(zip, at + 24)? as usize);
        let (nlen, elen, clen) = (u16_at(zip, at + 28)?, u16_at(zip, at + 30)?, u16_at(zip, at + 32)?);
        let local = u32_at(zip, at + 42)? as usize;
        let name = String::from_utf8_lossy(zip.get(at + 46..at + 46 + nlen).ok_or_else(|| anyhow!("truncated zip"))?).replace('\\', "/");
        at += 46 + nlen + elen + clen;
        let Some((_, rel)) = name.split_once('/') else { continue };
        if rel.ends_with('/') || !packaged(rel) { continue; }
        ensure!(u32_at(zip, local)? == 0x0403_4b50, "bad local header for {name}");
        let start = local + 30 + u16_at(zip, local + 26)? + u16_at(zip, local + 28)?;
        let data = zip.get(start..start + csize).ok_or_else(|| anyhow!("truncated entry {name}"))?;
        let path = to.join(rel);
        if let Some(p) = path.parent() { std::fs::create_dir_all(p)?; }
        let mut out = flate2::CrcWriter::new(std::fs::File::create(&path)?);
        let n = match method {
            0 => { out.write_all(data)?; data.len() as u64 }
            8 => std::io::copy(&mut flate2::read::DeflateDecoder::new(data), &mut out)?,
            m => bail!("{name}: compression method {m}"),
        };
        ensure!(n == usize_ as u64 && out.crc().sum() == crc, "{name}: size or CRC mismatch");
        out.flush()?;
        #[cfg(unix)]
        if !rel.contains('/') && !rel.contains('.') {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))?;
        }
        files.push(rel.to_string());
    }
    for need in [format!("rerac{EXE}"), "rerac-manifest.json".to_string()] {
        ensure!(files.contains(&need), "the zip has no {need}");
    }
    Ok(files)
}

/// Runs `exe --version-json` (≤ 30 s) and checks it names `rerac`, the release's `X.Y.Z` and `platform`.
fn verify(exe: &Path, version: &Version, platform: &str) -> Result<()> {
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("--version-json").stdin(std::process::Stdio::null()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut child = cmd.spawn().with_context(|| format!("run {}", exe.display()))?;
    let t0 = Instant::now();
    let st = loop {
        if let Some(st) = child.try_wait()? { break st; }
        if t0.elapsed() > Duration::from_secs(30) {
            let _ = child.kill();
            bail!("--version-json timed out");
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let mut out = String::new();
    if let Some(mut o) = child.stdout.take() { o.read_to_string(&mut out)?; }
    ensure!(st.success(), "--version-json exited with {st}");
    check_version_json(&out, version, platform)
}

/// The `--version-json` line of the new build names `rerac`, `X.Y.Z` and `platform`.
pub fn check_version_json(out: &str, version: &Version, platform: &str) -> Result<()> {
    let line = out.lines().find(|l| l.trim_start().starts_with('{')).ok_or_else(|| anyhow!("no --version-json line"))?;
    let v: serde_json::Value = serde_json::from_str(line)?;
    let (name, ver) = (v["name"].as_str().unwrap_or_default(), v["version"].as_str().unwrap_or_default());
    let ok = name == "rerac" && ver.starts_with(&format!("{}-", version.core_str())) && ver.ends_with(&format!("-{platform}"));
    ensure!(ok, "the new build says {name} {ver}");
    Ok(())
}

/// Moves `files` from `new` into `dir`, each old one into `old` first; stale shaders go to `old` too. Undoes every
/// move on a failure.
fn swap(dir: &Path, new: &Path, old: &Path, files: &[String]) -> Result<()> {
    let mut done: Vec<(PathBuf, Option<PathBuf>)> = Vec::new();
    let mut step = |src: Option<&Path>, target: &Path, keep: &Path| -> std::io::Result<()> {
        let had = target.exists();
        if had {
            if let Some(p) = keep.parent() { std::fs::create_dir_all(p)?; }
            std::fs::rename(target, keep)?;
        }
        if let Some(src) = src {
            if let Some(p) = target.parent() { std::fs::create_dir_all(p)?; }
            if let Err(e) = std::fs::rename(src, target) {
                if had { let _ = std::fs::rename(keep, target); }
                return Err(e);
            }
        }
        done.push((target.to_path_buf(), had.then(|| keep.to_path_buf())));
        Ok(())
    };
    let mut result = files.iter().try_for_each(|rel| step(Some(&new.join(rel)), &dir.join(rel), &old.join(rel)));
    if result.is_ok() {
        let shaders = dir.join("assets/shaders");
        let stale: Vec<String> = std::fs::read_dir(&shaders)
            .map(|rd| rd.flatten().filter_map(|e| e.file_name().to_str().map(|n| format!("assets/shaders/{n}"))).collect())
            .unwrap_or_default();
        result = stale.iter().filter(|rel| packaged(rel) && !files.contains(rel)).try_for_each(|rel| step(None, &dir.join(rel), &old.join(rel)));
    }
    if let Err(e) = result {
        for (target, keep) in done.into_iter().rev() {
            if let Some(keep) = keep {
                let _ = std::fs::remove_file(&target);
                let _ = std::fs::rename(&keep, &target);
            } else {
                let _ = std::fs::remove_file(&target);
            }
        }
        return Err(e.into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version { Version::parse(s).unwrap() }

    #[test]
    fn versions_order_and_compare_by_core() {
        assert!(v("v0.6.0-alpha") < v("v0.6.0-beta") && v("0.6.0-beta") < v("0.6.0-rc1") && v("0.6.0-rc1") < v("0.6.0-rc2"));
        assert!(v("0.6.0-rc2") < v("0.6.0") && v("0.5.10") > v("0.5.9-beta"));
        assert_eq!(v("1.2.3-Beta.4").pre, (1, 4));
        for bad in ["", "v1.2", "1.2.3.4", "1.x.3", "release"] { assert_eq!(Version::parse(bad), None, "{bad}"); }
        let build = v("0.5.3");
        assert!(is_newer(&v("v0.6.0-beta"), &build) && is_newer(&v("v0.5.4-alpha"), &build));
        assert!(!is_newer(&v("v0.5.3-alpha"), &build) && !is_newer(&v("v0.5.3"), &build) && !is_newer(&v("v0.5.2"), &build));
    }

    #[test]
    fn build_version_parts() {
        let (b, p) = build_parts("0.5.3-9eb6b3f8-windows-x86_64").unwrap();
        assert_eq!((b.core, p), ((0, 5, 3), "windows-x86_64"));
        assert_eq!(build_parts("0.5.3-unknown-macos-arm64").map(|(_, p)| p), Some("macos-arm64"));
        assert_eq!(build_parts("0.5.3"), None);
        assert!(build_parts(rc_extract::BUILD_VERSION).is_some());
    }

    fn release(tag: &str, platforms: &[&str]) -> Release {
        let version = v(tag);
        let mut assets: Vec<Asset> = platforms.iter().map(|p| Asset { name: asset_name(&version, p), url: format!("https://x/{tag}/{p}"), size: 1 }).collect();
        assets.push(Asset { name: "rerac-manifest.json".into(), url: String::new(), size: 1 });
        Release { tag: tag.into(), version, assets }
    }

    #[test]
    fn asset_choice() {
        assert_eq!(asset_name(&v("v0.6.0-beta"), "windows-x86_64"), "rerac-0.6.0-windows-x86_64.zip");
        let all = ["windows-x86_64", "macos-arm64", "linux-x86_64"];
        let list = vec![release("v0.5.3-alpha", &all), release("v0.6.0-beta", &["macos-arm64"]), release("v0.5.2-alpha", &all)];
        // The newest with this platform's zip; the macOS zip is not taken for Windows.
        let (r, a) = choose(&list, "windows-x86_64").unwrap();
        assert_eq!((r.tag.as_str(), a.map(|a| a.name.as_str())), ("v0.5.3-alpha", Some("rerac-0.5.3-windows-x86_64.zip")));
        assert_eq!(choose(&list, "macos-arm64").unwrap().0.tag, "v0.6.0-beta");
        // No zip for the platform anywhere: the newest, without a zip.
        let (r, a) = choose(&list, "linux-arm64").unwrap();
        assert_eq!((r.tag.as_str(), a), ("v0.6.0-beta", None));
        assert!(choose(&[], "macos-arm64").is_none());
    }

    #[test]
    fn release_list_skips_drafts_and_odd_tags() {
        let json = r#"[
            {"tag_name":"v0.6.0-beta","draft":true,"prerelease":true,"assets":[]},
            {"tag_name":"nightly","draft":false,"assets":[]},
            {"tag_name":"v0.5.3-alpha","draft":false,"prerelease":true,"assets":[
                {"name":"rerac-0.5.3-windows-x86_64.zip","size":36112155,"browser_download_url":"https://github.com/ecameracci/rerac/releases/download/v0.5.3-alpha/rerac-0.5.3-windows-x86_64.zip"}]}
        ]"#;
        let list = parse_releases(json).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!((list[0].tag.as_str(), list[0].assets[0].size), ("v0.5.3-alpha", 36112155));
        assert!(parse_releases("{\"message\":\"rate limited\"}").is_err());
    }

    #[test]
    fn only_packaged_files_unpack() {
        for ok in ["rerac", "rerac.exe", "rerac-extract.exe", "rerac-manifest.json", "README.txt", "assets/shaders/moby.wgsl"] { assert!(packaged(ok), "{ok}"); }
        for no in ["data/x", "settings.toml", "assets/shaders/../../x.wgsl", "assets/shaders/a/b.wgsl", "../rerac", "assets/shaders/x.txt", "mods/m/mod.toml"] {
            assert!(!packaged(no), "{no}");
        }
    }

    #[test]
    fn version_json_is_checked() {
        let r = v("v0.6.0-beta");
        assert!(check_version_json(r#"{"name":"rerac","version":"0.6.0-1a2b3c4d-windows-x86_64","game":"rac1","data_format":1}"#, &r, "windows-x86_64").is_ok());
        assert!(check_version_json(r#"{"name":"rerac","version":"0.5.3-1a2b3c4d-windows-x86_64"}"#, &r, "windows-x86_64").is_err());
        assert!(check_version_json(r#"{"name":"rerac","version":"0.6.0-1a2b3c4d-macos-arm64"}"#, &r, "windows-x86_64").is_err());
        assert!(check_version_json("", &r, "windows-x86_64").is_err());
    }

    /// A stored zip of `entries` under one top folder (what `unpack` reads).
    fn zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let (mut out, mut cd) = (Vec::new(), Vec::new());
        for (name, data) in entries {
            let name = format!("top/{name}");
            let mut crc = flate2::Crc::new();
            crc.update(data);
            let off = out.len() as u32;
            out.extend_from_slice(b"PK\x03\x04");
            out.extend_from_slice(&[20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
            for x in [crc.sum(), data.len() as u32, data.len() as u32] { out.extend_from_slice(&x.to_le_bytes()); }
            out.extend_from_slice(&(name.len() as u16).to_le_bytes());
            out.extend_from_slice(&[0, 0]);
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(data);
            cd.extend_from_slice(b"PK\x01\x02");
            cd.extend_from_slice(&[20, 0, 20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
            for x in [crc.sum(), data.len() as u32, data.len() as u32] { cd.extend_from_slice(&x.to_le_bytes()); }
            cd.extend_from_slice(&(name.len() as u16).to_le_bytes());
            cd.extend_from_slice(&[0; 12]);
            cd.extend_from_slice(&off.to_le_bytes());
            cd.extend_from_slice(name.as_bytes());
        }
        let at = out.len() as u32;
        out.extend_from_slice(&cd);
        out.extend_from_slice(b"PK\x05\x06\0\0\0\0");
        let n = (entries.len() as u16).to_le_bytes();
        out.extend_from_slice(&[n[0], n[1], n[0], n[1]]);
        out.extend_from_slice(&(cd.len() as u32).to_le_bytes());
        out.extend_from_slice(&at.to_le_bytes());
        out.extend_from_slice(&[0, 0]);
        out
    }

    #[test]
    fn unpack_and_swap() {
        let root = std::env::temp_dir().join(format!("rerac-update-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (game, new, old) = (root.join("game"), root.join("new"), root.join("old"));
        let exe = format!("rerac{EXE}");
        let z = zip(&[(&exe, b"new exe"), ("rerac-manifest.json", b"{}"), ("assets/shaders/a.wgsl", b"a2"), ("data/x", b"no"), ("README.txt", b"r")]);
        let files = unpack(&z, &new).unwrap();
        assert_eq!(files, [exe.clone(), "rerac-manifest.json".into(), "assets/shaders/a.wgsl".into(), "README.txt".into()]);
        assert!(!new.join("data").exists());
        // The old game: its exe, a shader the new build dropped, its settings and data (left alone).
        for (p, d) in [(exe.as_str(), "old exe"), ("assets/shaders/a.wgsl", "a1"), ("assets/shaders/gone.wgsl", "g"), ("settings.toml", "s"), ("data/x", "d")] {
            std::fs::create_dir_all(game.join(p).parent().unwrap()).unwrap();
            std::fs::write(game.join(p), d).unwrap();
        }
        swap(&game, &new, &old, &files).unwrap();
        let read = |p: &Path| std::fs::read_to_string(p).unwrap();
        assert_eq!((read(&game.join(&exe)), read(&old.join(&exe))), ("new exe".into(), "old exe".into()));
        assert_eq!(read(&game.join("assets/shaders/a.wgsl")), "a2");
        assert!(!game.join("assets/shaders/gone.wgsl").exists() && old.join("assets/shaders/gone.wgsl").exists());
        assert_eq!((read(&game.join("settings.toml")), read(&game.join("data/x"))), ("s".into(), "d".into()));
        // A zip without the executable is refused.
        assert!(unpack(&zip(&[("README.txt", b"r")]), &root.join("bad")).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }
}
