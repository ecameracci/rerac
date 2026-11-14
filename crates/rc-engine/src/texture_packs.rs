//! Port-only: PCSX2 texture packs, loaded as they are (docs/plan/graphics_options.md "Texture packs", the
//! README's player steps). A pack is a folder in `texture_packs/` beside the executable
//! (`RC_TEXTURE_PACKS_DIR` overrides it) holding PCSX2 replacement files anywhere below it (PCSX2's own
//! `SCUS-97199/replacements/…` layout, or the files alone), named by PCSX2's texture and CLUT hash
//! (rc_formats::pcsx2). Packs are listed and toggled with the mods (crate::mods, the main menu's Options → Mods); the
//! first enabled pack in folder-name order that has a name wins, and mods' own files win over packs.
//!
//! Every texture the port uploads is 8-bit indexed (PSMT8, CT32 CLUT), so its PCSX2 name is computed from the stored
//! indices and CLUT: [`key_t8`] (level 0 alone: PCSX2 with hardware mipmapping off, as the community packs were
//! dumped) and [`key_chain`] (the tfrag and billboard mip chains hashed together: mipmapping on). The world, moby,
//! shrub and sky loaders find replacements through crate::mod_textures by their decoded picture, so a level load
//! first [`register_level`]s each texture's keys under that picture's key; the HUD (crate::hud_render) and the
//! particles (crate::particle_render) look theirs up directly. Checked against the community pack "SCUS-97199
//! ratchet1 v2": every kind matches by name (graphics_options.md).
//!
//! Files: DDS (BC1, BC2, BC3, BC7, uncompressed 32-bit) or PNG (8-bit RGBA or RGB), alpha raw as PCSX2 dumps it
//! (0x80 = opaque), at a whole multiple of the original's size.

use rc_formats::pcsx2::{self, Key};
use rc_formats::texture::IndexedImage;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

/// The enabled packs' files by key (first pack wins), and the decoded pictures' keys registered by the loaders.
#[derive(Default)]
struct State {
    dirs: Vec<PathBuf>,
    files: HashMap<Key, PathBuf>,
    by_picture: HashMap<String, Vec<Key>>,
}

static STATE: RwLock<Option<State>> = RwLock::new(None);

fn read() -> std::sync::RwLockReadGuard<'static, Option<State>> { STATE.read().unwrap_or_else(|e| e.into_inner()) }
fn write() -> std::sync::RwLockWriteGuard<'static, Option<State>> { STATE.write().unwrap_or_else(|e| e.into_inner()) }

/// `RC_TEXTURE_PACKS_DIR`, else `texture_packs/` beside the (symlink-resolved) executable.
pub fn packs_dir() -> Option<PathBuf> {
    if let Some(v) = std::env::var_os("RC_TEXTURE_PACKS_DIR").filter(|v| !v.is_empty()) { return Some(PathBuf::from(v)); }
    let exe = std::env::current_exe().and_then(std::fs::canonicalize).ok()?;
    Some(exe.parent()?.join("texture_packs"))
}

/// The pack folders in `dir`, in folder-name order.
pub fn scan(dir: &Path) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut dirs: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    dirs.sort();
    dirs
}

/// Every replacement name below `dir` (PCSX2's `dumps` folders skipped), first found kept.
fn index_into(dir: &Path, files: &mut HashMap<Key, PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = rd.flatten().map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            if !p.file_name().is_some_and(|n| n.eq_ignore_ascii_case("dumps")) { index_into(&p, files); }
        } else if let Some(k) = p.file_name().and_then(|n| n.to_str()).and_then(Key::parse) {
            files.entry(k).or_insert(p);
        }
    }
}

/// Sets the enabled packs (crate::mods): their files indexed, in order. An empty list turns packs off.
pub fn set_packs(dirs: &[PathBuf]) {
    let mut st = write();
    if dirs.is_empty() {
        *st = None;
        return;
    }
    if st.as_ref().is_some_and(|s| s.dirs == dirs) { return; }
    let mut files = HashMap::new();
    for d in dirs { index_into(d, &mut files); }
    println!("texture packs: {} replacements in {} pack(s)", files.len(), dirs.len());
    let by_picture = st.take().map(|s| s.by_picture).unwrap_or_default();
    *st = Some(State { dirs: dirs.to_vec(), files, by_picture });
}

/// True when a pack is on (the loaders' quick check before hashing).
pub fn active() -> bool { read().is_some() }

/// The PCSX2 key of one PSMT8 level with its CLUT (level 0 alone); None for a size PCSX2 cannot name it at.
pub fn key_t8(img: &IndexedImage) -> Option<Key> {
    let bits = pcsx2::tex_bits(pcsx2::PSMT8, img.width, img.height)?;
    let clut = twox_hash::XxHash3_64::oneshot(&pcsx2::clut_bytes(img.clut)?);
    let mut b = Vec::with_capacity(img.indices.len());
    pcsx2::t8_hash_bytes(img.indices, img.width, img.height, &mut b);
    Some(Key { tex: twox_hash::XxHash3_64::oneshot(&b), clut: Some(clut), bits })
}

/// The key of a whole mip chain (level 0 first, one CLUT): PCSX2 with hardware mipmapping on. None for one level.
pub fn key_chain(levels: &[IndexedImage]) -> Option<Key> {
    let base = key_t8(levels.first()?)?;
    if levels.len() < 2 { return None; }
    let l: Vec<(&[u8], u32, u32)> = levels.iter().map(|m| (m.indices, m.width, m.height)).collect();
    Some(Key { tex: twox_hash::XxHash3_64::oneshot(&pcsx2::t8_chain_bytes(&l)), ..base })
}

/// The first enabled pack's file for any of `keys`.
pub fn find(keys: &[Key]) -> Option<PathBuf> {
    let st = read();
    let s = st.as_ref()?;
    keys.iter().find_map(|k| s.files.get(k).cloned())
}

/// The file for the image `img` (with its mip chain, when it has one).
pub fn find_image(img: &IndexedImage, chain: &[IndexedImage]) -> Option<PathBuf> {
    let keys: Vec<Key> = key_t8(img).into_iter().chain(key_chain(chain)).collect();
    find(&keys)
}

/// Registers the keys of the picture crate::mod_textures knows as `picture` (its `key`), when a pack has one.
fn register(picture: String, keys: Vec<Key>) {
    let mut st = write();
    let Some(s) = st.as_mut() else { return };
    if keys.iter().any(|k| s.files.contains_key(k)) { s.by_picture.insert(picture, keys); }
}

/// The file for the decoded picture `picture` ([`register`]ed by the level load).
pub fn find_picture(picture: &str) -> Option<PathBuf> {
    let st = read();
    let s = st.as_ref()?;
    s.by_picture.get(picture)?.iter().find_map(|k| s.files.get(k).cloned())
}

/// Registers every world texture of a level (tfrag, moby, tie, shrub tables, shrub billboards, sky) under its decoded
/// picture's key (crate::mod_textures::key), when a pack is on. Returns how many a pack has.
pub fn register_level(core: &rc_formats::level::LevelCore, core_data: &[u8], gs_ram: &[u8]) -> usize {
    use rc_formats::{sky, texture};
    if !active() { return 0; }
    let t0 = std::time::Instant::now();
    let mut n = 0;
    let mut add = |img: &IndexedImage, chain: &[IndexedImage]| {
        let keys: Vec<Key> = key_t8(img).into_iter().chain(key_chain(chain)).collect();
        if keys.is_empty() || find(&keys).is_none() { return; }
        let Ok(t) = img.decode() else { return };
        register(crate::mod_textures::key(&t), keys);
        n += 1;
    };
    for (table, mips) in [(&core.tfrag_textures, true), (&core.moby_textures, false), (&core.tie_textures, false), (&core.shrub_textures, false)] {
        for e in table.iter().filter(|e| e.width > 0 && e.height > 0 && e.data_offset >= 0 && e.palette >= 0) {
            let Ok(img) = texture::entry_image(core, core_data, gs_ram, e) else { continue };
            let chain = if mips { texture::tfrag_mip_images(core, core_data, gs_ram, e).unwrap_or_default() } else { Vec::new() };
            add(&img, &chain);
        }
    }
    for c in &core.shrub_classes {
        let b = &c.billboard;
        if b.width <= 0 || b.height <= 0 { continue; }
        let (Ok(img), chain) = (texture::billboard_image(gs_ram, b), texture::billboard_mip_images(gs_ram, b).unwrap_or_default()) else { continue };
        add(&img, &chain);
    }
    if let Ok(Some(block)) = sky::sky_block(core, core_data) {
        if let Ok(s) = sky::parse_sky_block(block) {
            for i in 0..s.texture_defs.len() {
                if let Ok(img) = sky::sky_texture_image(block, &s, i) { add(&img, &[]); }
            }
        }
    }
    println!("texture packs: {n} of the level's world textures have a replacement ({} ms)", t0.elapsed().as_millis());
    n
}

/// A replacement decoded to RGBA8 with raw GS alpha (0x80 = opaque), level 0: (width, height, bytes). For the
/// layers the port builds on the CPU (the HUD atlas, the particle layers); BC7 is not decoded here.
pub fn load_rgba(path: &Path) -> Result<(u32, u32, Vec<u8>), String> {
    let b = std::fs::read(path).map_err(|e| e.to_string())?;
    if b.get(0..4) == Some(b"DDS ") { dds_rgba(&b) } else { png_rgba(&b) }
}

fn png_rgba(b: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    use bevy::image::{CompressedImageFormats, Image, ImageSampler, ImageType};
    use bevy::render::render_resource::TextureFormat;
    let img = Image::from_buffer(b, ImageType::Extension("png"), CompressedImageFormats::NONE, false, ImageSampler::Default, bevy::asset::RenderAssetUsages::MAIN_WORLD)
        .map_err(|e| e.to_string())?;
    let img = if img.texture_descriptor.format == TextureFormat::Rgba8Unorm { img } else { img.convert(TextureFormat::Rgba8Unorm).ok_or("not convertible to RGBA8")? };
    let (w, h) = (img.texture_descriptor.size.width, img.texture_descriptor.size.height);
    let mut rgba = img.data.ok_or("no pixels")?;
    rgb_png_alpha(&mut rgba);
    Ok((w, h, rgba))
}

/// A pack PNG's alpha, raw: an RGB file (every alpha 0xff once decoded) is opaque at the GS's 0x80, as PCSX2 loads it.
pub fn rgb_png_alpha(rgba: &mut [u8]) {
    if rgba.iter().skip(3).step_by(4).all(|&a| a == 0xff) {
        for a in rgba.iter_mut().skip(3).step_by(4) { *a = 0x80; }
    }
}

/// Level 0 of a DDS: BC1 / BC2 / BC3 or 32-bit masks (crate::mod_textures::from_dds reads the same headers).
pub fn dds_rgba(b: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    let u = |o: usize| b.get(o..o + 4).map(|s| u32::from_le_bytes(s.try_into().unwrap())).ok_or("truncated header");
    let (h, w) = (u(12)?, u(16)?);
    if w == 0 || h == 0 || w > 16384 || h > 16384 { return Err(format!("size {w}x{h}")); }
    let (four, bits) = (b.get(84..88).ok_or("truncated header")?, u(88)?);
    let (mut at, mut bc) = (128usize, 0u8);
    match four {
        b"DXT1" => bc = 1,
        b"DXT2" | b"DXT3" => bc = 2,
        b"DXT4" | b"DXT5" => bc = 3,
        b"DX10" => {
            at = 148;
            bc = match u(128)? { 71 | 72 => 1, 74 | 75 => 2, 77 | 78 => 3, 28 | 29 => 0, f => return Err(format!("DXGI format {f}")) };
        }
        _ if u(80)? & 0x40 != 0 && bits == 32 => {}
        _ => return Err(format!("pixel format {:?}", String::from_utf8_lossy(four))),
    }
    let data = b.get(at..).ok_or("truncated data")?;
    if bc == 0 {
        let masks = if four == b"DX10" { [0xff, 0xff00, 0xff_0000, 0xff00_0000] } else { [u(92)?, u(96)?, u(100)?, u(104)?] };
        let n = (w * h) as usize;
        let px = data.get(..n * 4).ok_or("truncated data")?;
        let mut rgba = Vec::with_capacity(n * 4);
        for p in px.as_chunks::<4>().0 {
            let v = u32::from_le_bytes(*p);
            for (c, &m) in masks.iter().enumerate() { rgba.push(if m == 0 { if c == 3 { 0x80 } else { 0 } } else { ((v & m) >> m.trailing_zeros()) as u8 }); }
        }
        return Ok((w, h, rgba));
    }
    let size = if bc == 1 { 8 } else { 16 };
    let (bw, bh) = (w.div_ceil(4) as usize, h.div_ceil(4) as usize);
    let blocks = data.get(..bw * bh * size).ok_or("truncated data")?;
    let mut rgba = vec![0u8; (w * h * 4) as usize];
    for (i, blk) in blocks.chunks_exact(size).enumerate() {
        let px = bc_block(blk, bc);
        let (x0, y0) = ((i % bw) * 4, (i / bw) * 4);
        for (j, p) in px.iter().enumerate() {
            let (x, y) = (x0 + j % 4, y0 + j / 4);
            if x < w as usize && y < h as usize { rgba[(y * w as usize + x) * 4..][..4].copy_from_slice(p); }
        }
    }
    Ok((w, h, rgba))
}

/// One 4×4 block of BC1 (`bc` 1), BC2 (2) or BC3 (3), row-major.
fn bc_block(b: &[u8], bc: u8) -> [[u8; 4]; 16] {
    let colour = if bc == 1 { b } else { &b[8..16] };
    let c565 = |v: u16| [((v >> 11) & 31) as u32 * 255 / 31, ((v >> 5) & 63) as u32 * 255 / 63, (v & 31) as u32 * 255 / 31];
    let (v0, v1) = (u16::from_le_bytes([colour[0], colour[1]]), u16::from_le_bytes([colour[2], colour[3]]));
    let (a, c) = (c565(v0), c565(v1));
    let mix = |p: u32, q: u32, d: u32| [(a[0] * p + c[0] * q) / d, (a[1] * p + c[1] * q) / d, (a[2] * p + c[2] * q) / d];
    let four = bc != 1 || v0 > v1;
    let pal: [[u32; 3]; 4] = if four { [a, c, mix(2, 1, 3), mix(1, 2, 3)] } else { [a, c, mix(1, 1, 2), [0, 0, 0]] };
    let idx = u32::from_le_bytes([colour[4], colour[5], colour[6], colour[7]]);
    // BC1 holds no alpha: opaque is the GS's 0x80, its punch-through texels 0.
    let mut alpha = [0x80u8; 16];
    match bc {
        1 => { for (j, al) in alpha.iter_mut().enumerate() { if !four && (idx >> (2 * j)) & 3 == 3 { *al = 0; } } }
        2 => { for (j, al) in alpha.iter_mut().enumerate() { *al = ((b[j / 2] >> (4 * (j % 2))) & 15) * 17; } }
        _ => {
            let (a0, a1) = (b[0] as u32, b[1] as u32);
            let ap: [u32; 8] = if a0 > a1 {
                [a0, a1, (6 * a0 + a1) / 7, (5 * a0 + 2 * a1) / 7, (4 * a0 + 3 * a1) / 7, (3 * a0 + 4 * a1) / 7, (2 * a0 + 5 * a1) / 7, (a0 + 6 * a1) / 7]
            } else {
                [a0, a1, (4 * a0 + a1) / 5, (3 * a0 + 2 * a1) / 5, (2 * a0 + 3 * a1) / 5, (a0 + 4 * a1) / 5, 0, 255]
            };
            let bits = u64::from_le_bytes([b[2], b[3], b[4], b[5], b[6], b[7], 0, 0]);
            for (j, al) in alpha.iter_mut().enumerate() { *al = ap[((bits >> (3 * j)) & 7) as usize] as u8; }
        }
    }
    std::array::from_fn(|j| {
        let p = pal[((idx >> (2 * j)) & 3) as usize];
        [p[0] as u8, p[1] as u8, p[2] as u8, alpha[j]]
    })
}

/// A replacement for a `w × h` original as a raw-alpha RGBA picture at its whole scale (1..=8), or None (logged).
pub fn load_scaled(path: &Path, w: u32, h: u32) -> Option<(u32, Vec<u8>)> {
    let (pw, ph, rgba) = load_rgba(path).map_err(|e| eprintln!("texture packs: {}: {e}", path.display())).ok()?;
    let k = pw / w.max(1);
    if !(1..=8).contains(&k) || pw != w * k || ph != h * k {
        eprintln!("texture packs: {}: {pw}x{ph} is not 1..8 times the game's {w}x{h}; skipped", path.display());
        return None;
    }
    Some((k, rgba))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bc1_and_bc3_blocks() {
        // BC1: colour 0 white, colour 1 black, every texel index 2 (2/3 white), opaque at the GS's 0x80.
        let blk = [0xff, 0xff, 0, 0, 0xaa, 0xaa, 0xaa, 0xaa];
        assert_eq!(bc_block(&blk, 1)[5], [170, 170, 170, 0x80]);
        // BC3: alpha endpoints 0x80 / 0, every alpha index 0 (0x80); colour as above, index 0 (white).
        let mut b3 = [0u8; 16];
        b3[0] = 0x80;
        b3[8..12].copy_from_slice(&[0xff, 0xff, 0, 0]);
        assert_eq!(bc_block(&b3, 3)[15], [255, 255, 255, 0x80]);
    }

    #[test]
    fn index_takes_the_first_pack_and_skips_dumps() {
        let root = std::env::temp_dir().join(format!("rerac-packs-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let name = "abc-def-00005993.dds";
        for (d, f) in [("a/SCUS-97199/replacements/sub", name), ("a/SCUS-97199/dumps", "123-456-00005993.png"), ("b", name), ("b", "notapack.txt")] {
            std::fs::create_dir_all(root.join(d)).unwrap();
            std::fs::write(root.join(d).join(f), b"x").unwrap();
        }
        let mut files = HashMap::new();
        for d in scan(&root) { index_into(&d, &mut files); }
        assert_eq!(files.len(), 1);
        assert!(files.values().next().unwrap().starts_with(root.join("a")));
        let _ = std::fs::remove_dir_all(&root);
    }
}
