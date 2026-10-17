//! Port-only: the mods folder (docs/plan/mods.md, Stage 3a: asset overrides).
//!
//! A mod is a folder in `mods/` beside the executable (`RC_MODS_DIR` overrides it), holding a `mod.toml` (top-level
//! `id`, `name`, `version`, `description` strings; the folder name when `id` is missing) and an `assets/` tree. A mod
//! overrides a game asset by shipping a file at the path `rerac-extract export` writes for it
//! (`assets/textures/levels/NN/hud/frame_NNNN_WxH.png`, …). The engine reads, today:
//! * the HUD frames and FX textures (crate::hud_render): a PNG at `textures/levels/NN/hud/frame_NNNN_WxH.png` or
//!   `textures/levels/NN/fx/NNN_WxH.png`, the original size times a whole factor (1..=8), RGBA or any PNG the
//!   decoder reads; alpha as the export writes it (255 = the PS2's opaque 0x80).
//! * the world, moby and sky textures (crate::mod_textures): `textures/hash/<key>.dds` or `.png`, keyed by the decoded
//!   picture's hash, applied at the next level load.
//!
//! Which mods are on is the port settings file's `mods = <id>, <id>` (crate::render_settings); a mod found for the
//! first time starts off. The main menu's Options → Mods page toggles them ([`Mods::set_enabled`]); the HUD
//! rebuilds its atlas when [`Mods::generation`] changes. Mods are searched in folder-name order and the first that
//! has a file wins. Deterministic runs (`RC_DETERMINISTIC`, the golden tests) see no mods unless `RC_MODS_DIR` is set
//! (the enabled set then comes from an explicit `RC_SETTINGS_FILE`).

use bevy::prelude::*;
use std::path::{Path, PathBuf};

/// One mod folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub dir: PathBuf,
    pub enabled: bool,
}

/// The mods found at start and which are on.
#[derive(Resource, Clone, Debug, Default)]
pub struct Mods {
    /// The folder searched (None: mods are off in this run).
    pub dir: Option<PathBuf>,
    pub list: Vec<ModInfo>,
    /// Bumped by every change of the enabled set.
    pub generation: u64,
}

/// The settings file key of the enabled ids.
const KEY: &str = "mods";

impl Mods {
    /// Scans the mods folder (creating it when missing, so players find where mods go) and reads the enabled set.
    pub fn startup() -> Mods {
        let explicit = std::env::var_os("RC_MODS_DIR").is_some_and(|v| !v.is_empty());
        if crate::determinism::deterministic() && !explicit { return Mods::default(); }
        let Some(dir) = mods_dir() else { return Mods::default() };
        if !dir.exists() {
            match std::fs::create_dir_all(&dir) {
                Ok(()) => println!("mods: created {}", dir.display()),
                Err(e) => eprintln!("mods: cannot create {}: {e}", dir.display()),
            }
        }
        let on = crate::render_settings::load_key(KEY).unwrap_or_default();
        let mut m = Mods { list: scan(&dir), dir: Some(dir), generation: 0 };
        let on: Vec<&str> = on.split(',').map(str::trim).filter(|s| !s.is_empty()).collect();
        for info in &mut m.list { info.enabled = on.contains(&info.id.as_str()); }
        m.publish();
        let d = m.dir.as_deref().map_or(String::new(), |d| d.display().to_string());
        match m.list.len() {
            0 => println!("mods: none in {d}"),
            _ => println!("mods: {} in {d}: {}", m.list.len(), m.list.iter().map(|i| format!("{} ({})", i.id, if i.enabled { "on" } else { "off" })).collect::<Vec<_>>().join(", ")),
        }
        m
    }

    /// Turns mod `i` on or off and saves the enabled set.
    pub fn set_enabled(&mut self, i: usize, on: bool) {
        let Some(info) = self.list.get_mut(i) else { return };
        if info.enabled == on { return; }
        info.enabled = on;
        println!("mods: {} {}", info.id, if on { "on" } else { "off" });
        self.generation += 1;
        self.publish();
        let ids: Vec<&str> = self.list.iter().filter(|i| i.enabled).map(|i| i.id.as_str()).collect();
        crate::render_settings::save_key(KEY, &ids.join(", "));
    }

    /// True when any mod is on.
    pub fn any_enabled(&self) -> bool { self.list.iter().any(|i| i.enabled) }

    /// The file of the first enabled mod that has `assets/<rel>` (`rel` with `/` separators).
    pub fn find(&self, rel: &str) -> Option<PathBuf> {
        self.list.iter().filter(|i| i.enabled).map(|i| i.dir.join("assets").join(rel)).find(|p| p.is_file())
    }

    /// The enabled mods' folders for the loaders that run without the resource ([`find_active`]).
    fn publish(&self) {
        let dirs = self.list.iter().filter(|i| i.enabled).map(|i| i.dir.clone()).collect();
        *ACTIVE.write().unwrap_or_else(|e| e.into_inner()) = dirs;
    }
}

/// The enabled mods' folders, in search order ([`Mods::publish`]).
static ACTIVE: std::sync::RwLock<Vec<PathBuf>> = std::sync::RwLock::new(Vec::new());

/// True when any mod is on (the loaders' quick check before hashing a texture).
pub fn active() -> bool { !ACTIVE.read().unwrap_or_else(|e| e.into_inner()).is_empty() }

/// [`Mods::find`] for loaders without the resource (level loads, crate::mod_textures).
pub fn find_active(rel: &str) -> Option<PathBuf> {
    ACTIVE.read().unwrap_or_else(|e| e.into_inner()).iter().map(|d| d.join("assets").join(rel)).find(|p| p.is_file())
}

/// `RC_MODS_DIR`, else `mods/` beside the (symlink-resolved) executable.
fn mods_dir() -> Option<PathBuf> {
    if let Some(v) = std::env::var_os("RC_MODS_DIR").filter(|v| !v.is_empty()) { return Some(PathBuf::from(v)); }
    let exe = std::env::current_exe().and_then(std::fs::canonicalize).ok()?;
    Some(exe.parent()?.join("mods"))
}

/// The mod folders in `dir` (each with a `mod.toml`), in folder-name order.
fn scan(dir: &Path) -> Vec<ModInfo> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut dirs: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.join("mod.toml").is_file()).collect();
    dirs.sort();
    dirs.into_iter()
        .filter_map(|d| {
            let text = std::fs::read_to_string(d.join("mod.toml")).map_err(|e| eprintln!("mods: {}: {e}", d.display())).ok()?;
            let folder = d.file_name()?.to_string_lossy().into_owned();
            Some(parse_info(&text, &folder, d))
        })
        .collect()
}

/// The top-level `key = "value"` strings of a `mod.toml` (before its first `[table]`).
fn parse_info(text: &str, folder: &str, dir: PathBuf) -> ModInfo {
    let get = |key: &str| -> Option<String> {
        text.lines()
            .map(str::trim)
            .take_while(|l| !l.starts_with('['))
            .filter_map(|l| l.split_once('='))
            .find(|(k, _)| k.trim() == key)
            .map(|(_, v)| v.trim().trim_matches('"').to_string())
            .filter(|v| !v.is_empty())
    };
    let id = get("id").unwrap_or_else(|| folder.to_string());
    let name = get("name").unwrap_or_else(|| id.clone());
    ModInfo { id, name, version: get("version").unwrap_or_default(), description: get("description").unwrap_or_default(), dir, enabled: false }
}

pub struct ModsPlugin;

impl Plugin for ModsPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Mods::startup())
            .add_systems(First, crate::mod_textures::track_frame)
            .add_systems(Last, || crate::mod_textures::report("world textures"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mod_toml_top_level_strings() {
        let t = "id = \"hd-ui\"\nname = \"HD UI\"\nversion = \"1.0.0\"\n\n[assets]\nname = \"not this\"\n";
        let m = parse_info(t, "folder", PathBuf::from("x"));
        assert_eq!((m.id.as_str(), m.name.as_str(), m.version.as_str(), m.enabled), ("hd-ui", "HD UI", "1.0.0", false));
        let m = parse_info("[assets]\nid = \"no\"\n", "folder", PathBuf::from("x"));
        assert_eq!((m.id.as_str(), m.name.as_str()), ("folder", "folder"));
    }

    #[test]
    fn first_enabled_mod_with_the_file_wins() {
        let root = std::env::temp_dir().join(format!("rerac-mods-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for (m, has) in [("a", false), ("b", true), ("c", true)] {
            let d = root.join(m);
            std::fs::create_dir_all(d.join("assets/textures")).unwrap();
            std::fs::write(d.join("mod.toml"), format!("id = \"{m}\"\n")).unwrap();
            if has { std::fs::write(d.join("assets/textures/x.png"), m).unwrap(); }
        }
        let mut mods = Mods { list: scan(&root), dir: Some(root.clone()), generation: 0 };
        assert_eq!(mods.list.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(), ["a", "b", "c"]);
        assert_eq!(mods.find("textures/x.png"), None, "all off");
        for i in 0..3 { mods.list[i].enabled = true; }
        assert_eq!(mods.find("textures/x.png"), Some(root.join("b/assets/textures/x.png")));
        mods.list[1].enabled = false;
        assert_eq!(mods.find("textures/x.png"), Some(root.join("c/assets/textures/x.png")));
        let _ = std::fs::remove_dir_all(&root);
    }
}
