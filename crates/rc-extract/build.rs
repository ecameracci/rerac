//! The build version `RERAC_BUILD_VERSION`: `<version>-<hash>-<platform>`, the workspace version, the first 8 hex
//! digits of the git commit (`unknown` without git or a repository) and the target as the packages name it
//! (`0.4.0-1a2b3c4d-windows-x86_64`, `-macos-arm64`, `-linux-x86_64`).
//! `tools/package/package.sh` computes the same string and checks the packaged `rerac --version-json` against it.

use std::path::PathBuf;
use std::process::Command;

fn main() {
    let var = |k: &str| std::env::var(k).unwrap_or_default();
    let hash = git(&["rev-parse", "HEAD"]).filter(|h| h.len() >= 8).map_or("unknown".into(), |h| h[..8].to_string());
    let arch = match var("CARGO_CFG_TARGET_ARCH").as_str() { "aarch64" => "arm64".to_string(), arch => arch.to_string() };
    println!("cargo:rustc-env=RERAC_BUILD_VERSION={}-{hash}-{}-{arch}", var("CARGO_PKG_VERSION"), var("CARGO_CFG_TARGET_OS"));
    rerun_on_commit();
}

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    if !out.status.success() { return None; }
    Some(String::from_utf8(out.stdout).ok()?.trim().to_string())
}

/// Reruns this script when HEAD moves: HEAD itself (checkout), its reflog (every commit, reset, pull), the branch
/// it names and packed-refs. Only existing files: Cargo reruns on every build for a missing one.
fn rerun_on_commit() {
    let mut paths = vec!["HEAD".to_string(), "logs/HEAD".to_string(), "packed-refs".to_string()];
    paths.extend(git(&["symbolic-ref", "-q", "HEAD"]));
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default());
    for p in paths {
        let Some(path) = git(&["rev-parse", "--git-path", &p]) else { continue };
        let path = manifest_dir.join(path);
        if path.exists() { println!("cargo:rerun-if-changed={}", path.display()); }
    }
}
