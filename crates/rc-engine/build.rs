//! Windows (MSVC): links `icon/rerac.ico` (the launcher's app icon, re-rac/rerac-launcher `src-tauri/icons/icon.ico`)
//! into `rerac.exe` as icon group 1, so Explorer and the taskbar show it; crate::window_icon puts the same resource
//! on the window. The `.res` file is written here (std only): `link.exe` takes it as an input as it is.

use std::path::PathBuf;

const ICON: &str = "icon/rerac.ico";
/// The group's resource id, read back by crate::window_icon.
const GROUP_ID: u16 = 1;
const RT_ICON: u16 = 3;
const RT_GROUP_ICON: u16 = 14;

fn main() {
    println!("cargo:rerun-if-changed={ICON}");
    let var = |k: &str| std::env::var(k).unwrap_or_default();
    if var("CARGO_CFG_TARGET_OS") != "windows" || var("CARGO_CFG_TARGET_ENV") != "msvc" { return; }
    let ico = std::fs::read(ICON).expect("reading the icon");
    let res = PathBuf::from(var("OUT_DIR")).join("rerac.res");
    std::fs::write(&res, icon_res(&ico)).expect("writing the icon resource");
    println!("cargo:rustc-link-arg-bin=rerac={}", res.display());
}

fn u16_at(b: &[u8], o: usize) -> u16 { u16::from_le_bytes([b[o], b[o + 1]]) }
fn u32_at(b: &[u8], o: usize) -> u32 { u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]) }

/// One `.res` entry: RESOURCEHEADER with ordinal type and name, then the data padded to 4 bytes.
fn entry(out: &mut Vec<u8>, kind: u16, id: u16, flags: u16, data: &[u8]) {
    out.extend((data.len() as u32).to_le_bytes());
    out.extend(32u32.to_le_bytes());
    for v in [0xffff, kind, 0xffff, id] { out.extend(u16::to_le_bytes(v)); }
    out.extend(0u32.to_le_bytes()); // DataVersion
    out.extend(flags.to_le_bytes());
    out.extend(0x0409u16.to_le_bytes()); // LanguageId: en-US
    out.extend([0u8; 8]); // Version, Characteristics
    out.extend(data);
    out.resize(out.len().next_multiple_of(4), 0);
}

/// The ICO's images as RT_ICON 1..=n and their directory as RT_GROUP_ICON [`GROUP_ID`] (each ICONDIRENTRY's file
/// offset becomes the image's resource id).
fn icon_res(ico: &[u8]) -> Vec<u8> {
    assert!(ico.len() >= 6 && u16_at(ico, 0) == 0 && u16_at(ico, 2) == 1, "{ICON} is not an .ico file");
    let n = u16_at(ico, 4);
    let mut out = Vec::new();
    entry(&mut out, 0, 0, 0, &[]);
    let mut group = [0u16, 1, n].iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<u8>>();
    for i in 0..n {
        let d = 6 + 16 * i as usize;
        let (size, offset) = (u32_at(ico, d + 8) as usize, u32_at(ico, d + 12) as usize);
        entry(&mut out, RT_ICON, i + 1, 0x1010, &ico[offset..offset + size]);
        group.extend(&ico[d..d + 12]);
        group.extend((i + 1).to_le_bytes());
    }
    entry(&mut out, RT_GROUP_ICON, GROUP_ID, 0x1030, &group);
    out
}
