//! Port-only: the mods' world texture overrides (crate::mods, docs/plan/mods.md). A mod replaces a texture the engine
//! decodes (tfrag, tie, shrub and billboard, moby, sky) with `assets/textures/hash/<key>.dds` or `.png`, where `key`
//! is [`key`] of the decoded image: the XXH3-64 of its RGBA8 bytes as `texture::decode_indexed8` returns them (alpha
//! 0x80 → 0xff) in 16 hex digits, then `_<width>x<height>`. The same key holds wherever the texture appears (every
//! level, the title world). The override is the same picture at a whole power-of-two scale (1..=16):
//! * DDS: DXT5 (BC3) or uncompressed 32-bit RGBA (DX10 headers too), with or without mip levels; alpha raw as the GS
//!   reads it (0x80 = opaque), as PCSX2 texture packs store it;
//! * PNG: RGBA, alpha as `rerac-extract export` writes it (255 = opaque); the mip levels are generated.
//!
//! The renderers keep the original's texel alpha range (crate::gs_state's passes) and move the GS mip level so the
//! replacement gives one texel per pixel of the frame ([`Hd::lod_scale`]); mobys and the sky sample with the
//! hardware's LOD over the override's levels. Overrides apply at the next level load (crate::mods' toggles do not
//! reload the world).

use bevy::asset::RenderAssetUsages;
use bevy::image::{CompressedImageFormats, ImageSampler, ImageSamplerDescriptor, ImageType};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use rc_formats::texture::Texture;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

/// The override file name stem of a decoded texture.
pub fn key(t: &Texture) -> String { format!("{:016x}_{}x{}", twox_hash::XxHash3_64::oneshot(&t.rgba), t.width, t.height) }

/// A world renderer's image per (texture, sampler): the image, its texel alpha range, the highest mip level to sample
/// and the LOD's distance divisor ([`Hd::into_parts`]; 1 for the game's own texture).
pub type WorldImage = (Handle<Image>, crate::gs_state::AlphaRange, u32, f32);

/// A loaded override.
pub struct Hd {
    pub image: Image,
    /// Override size / original size (a power of two).
    pub scale: u32,
    /// Mip levels in [`Self::image`].
    pub levels: u32,
}

impl Hd {
    /// The highest mip level to sample for an original whose highest is `mxl`: the original's lowest detail, no less.
    pub fn mxl(&self, mxl: u32) -> u32 { (mxl + self.lod_scale().log2().ceil() as u32).min(self.levels - 1) }

    /// The factor the GS LOD's distance constant is divided by (LOD = log2(z / n) + K, + log2 of this): the GS LOD
    /// picks one texel per pixel of the PS2's 416 lines, so at a frame of h lines the override's level is
    /// log2(scale · 416 / h) above it (never below the original's density, never past full detail).
    pub fn lod_scale(&self) -> f32 { lod_scale(self.scale, FRAME_HEIGHT.load(Ordering::Relaxed)) }

    /// (image, [`Self::mxl`] of `mxl`, [`Self::lod_scale`]).
    pub fn into_parts(self, mxl: u32) -> (Image, u32, f32) {
        let (m, s) = (self.mxl(mxl), self.lod_scale());
        (self.image, m, s)
    }
}

/// [`Hd::lod_scale`] for a frame `height` lines tall (0: not known yet, taken as 1080).
fn lod_scale(scale: u32, height: u32) -> f32 {
    let r = if height == 0 { 1080.0 } else { height as f32 } / 416.0;
    (scale as f32 / r).clamp(1.0, scale as f32)
}

/// The game frame's height when the world loads (crate::display::GameFrame; [`track_frame`]).
static FRAME_HEIGHT: AtomicU32 = AtomicU32::new(0);

/// Keeps [`FRAME_HEIGHT`] current for the loaders (a resolution change applies from the next level load).
pub fn track_frame(frame: Option<Res<crate::display::GameFrame>>) {
    if let Some(f) = frame { FRAME_HEIGHT.store(f.size.y, Ordering::Relaxed); }
}

static FOUND: AtomicUsize = AtomicUsize::new(0);
static LOOKED: AtomicUsize = AtomicUsize::new(0);

/// The enabled mods' override of `t` with `sampler`, if one has it and it loads.
pub fn find(t: &Texture, sampler: ImageSamplerDescriptor) -> Option<Hd> {
    if !crate::mods::active() { return None; }
    LOOKED.fetch_add(1, Ordering::Relaxed);
    let k = key(t);
    let (path, dds) = match crate::mods::find_active(&format!("textures/hash/{k}.dds")) {
        Some(p) => (p, true),
        None => (crate::mods::find_active(&format!("textures/hash/{k}.png"))?, false),
    };
    let bytes = std::fs::read(&path).map_err(|e| eprintln!("mods: {}: {e}", path.display())).ok()?;
    let res = if dds { from_dds(&bytes) } else { from_png(&bytes) };
    let (mut image, w, levels) = res.map_err(|e| eprintln!("mods: {}: {e}", path.display())).ok()?;
    let h = image.texture_descriptor.size.height;
    let scale = w / t.width.max(1);
    if !scale.is_power_of_two() || scale > 16 || w != t.width * scale || h != t.height * scale {
        eprintln!("mods: {}: {w}x{h} is not {}x{} at a power-of-two scale", path.display(), t.width, t.height);
        return None;
    }
    image.sampler = ImageSampler::Descriptor(sampler);
    FOUND.fetch_add(1, Ordering::Relaxed);
    Some(Hd { image, scale, levels })
}

/// The lookups since the last call, then reset: `mods: <what>: n of m textures replaced` when any was.
pub fn report(what: &str) {
    let (n, m) = (FOUND.swap(0, Ordering::Relaxed), LOOKED.swap(0, Ordering::Relaxed));
    if n > 0 { println!("mods: {what}: {n} of {m} textures replaced"); }
}

fn image(w: u32, h: u32, levels: u32, format: TextureFormat, data: Vec<u8>) -> Image {
    let mut img = Image::new_uninit(Extent3d { width: w, height: h, depth_or_array_layers: 1 }, TextureDimension::D2, format, RenderAssetUsages::RENDER_WORLD);
    img.texture_descriptor.mip_level_count = levels;
    img.data = Some(data);
    img
}

/// (image, width, levels) of a DDS file.
fn from_dds(b: &[u8]) -> Result<(Image, u32, u32), String> {
    let u = |o: usize| b.get(o..o + 4).map(|s| u32::from_le_bytes(s.try_into().unwrap())).ok_or("truncated header");
    if b.get(0..4) != Some(b"DDS ") { return Err("not a DDS file".into()); }
    let (h, w) = (u(12)?, u(16)?);
    let levels = u(28)?.max(1).min(32 - w.max(h).leading_zeros());
    if w == 0 || h == 0 || w > 16384 || h > 16384 { return Err(format!("size {w}x{h}")); }
    let (four, bits) = (b.get(84..88).ok_or("truncated header")?, u(88)?);
    let (mut data_at, mut kind) = (128usize, None);
    if four == b"DXT5" {
        kind = Some(true);
    } else if four == b"DX10" {
        data_at = 148;
        kind = match u(128)? { 77 => Some(true), 28 => Some(false), f => return Err(format!("DXGI format {f} (BC3 or RGBA8 only)")) };
    } else if u(80)? & 0x40 != 0 && bits == 32 {
        kind = Some(false);
    }
    let Some(bc3) = kind else { return Err(format!("pixel format {:?} (DXT5 or 32-bit RGBA only)", String::from_utf8_lossy(four))) };
    let level_size = |l: u32| -> usize {
        let (lw, lh) = ((w >> l).max(1) as usize, (h >> l).max(1) as usize);
        if bc3 { lw.div_ceil(4) * lh.div_ceil(4) * 16 } else { lw * lh * 4 }
    };
    let size: usize = (0..levels).map(level_size).sum();
    let data = b.get(data_at..data_at + size).ok_or("truncated data")?;
    if bc3 {
        if w % 4 != 0 || h % 4 != 0 { return Err(format!("BC3 size {w}x{h} is not a multiple of 4")); }
        return Ok((image(w, h, levels, TextureFormat::Bc3RgbaUnorm, data.to_vec()), w, levels));
    }
    let masks = if four == b"DX10" { [0xff, 0xff00, 0xff_0000, 0xff00_0000] } else { [u(92)?, u(96)?, u(100)?, u(104)?] };
    let mut rgba = Vec::with_capacity(size);
    for px in data.as_chunks::<4>().0 {
        let v = u32::from_le_bytes(*px);
        for (c, &m) in masks.iter().enumerate() {
            rgba.push(if m == 0 { if c == 3 { 0x80 } else { 0 } } else { ((v & m) >> m.trailing_zeros()) as u8 });
        }
    }
    Ok((image(w, h, levels, TextureFormat::Rgba8Unorm, rgba), w, levels))
}

/// (image, width, levels) of a PNG: alpha to the GS range, every mip level by 2×2 box filtering.
fn from_png(b: &[u8]) -> Result<(Image, u32, u32), String> {
    let img = Image::from_buffer(b, ImageType::Extension("png"), CompressedImageFormats::NONE, false, ImageSampler::Default, RenderAssetUsages::MAIN_WORLD)
        .map_err(|e| e.to_string())?;
    let img = if img.texture_descriptor.format == TextureFormat::Rgba8Unorm { img } else { img.convert(TextureFormat::Rgba8Unorm).ok_or("not convertible to RGBA8")? };
    let (w, h) = (img.texture_descriptor.size.width, img.texture_descriptor.size.height);
    let mut level = img.data.ok_or("no pixels")?;
    for a in level.iter_mut().skip(3).step_by(4) { *a = ((*a as u16 + 1) >> 1) as u8; }
    let mut data = level.clone();
    let (mut lw, mut lh, mut levels) = (w, h, 1);
    while lw > 1 || lh > 1 {
        let (nw, nh) = ((lw / 2).max(1), (lh / 2).max(1));
        level = half(&level, lw, lh, nw, nh);
        data.extend_from_slice(&level);
        (lw, lh, levels) = (nw, nh, levels + 1);
    }
    Ok((image(w, h, levels, TextureFormat::Rgba8Unorm, data), w, levels))
}

/// `rgba` (w × h) box-filtered to nw × nh (each w / 2, h / 2 or 1).
fn half(rgba: &[u8], w: u32, h: u32, nw: u32, nh: u32) -> Vec<u8> {
    let (kx, ky) = (w / nw, h / nh);
    let mut out = Vec::with_capacity((nw * nh * 4) as usize);
    for y in 0..nh {
        for x in 0..nw {
            for c in 0..4 {
                let mut s = 0u32;
                for dy in 0..ky {
                    for dx in 0..kx { s += rgba[(((y * ky + dy) * w + x * kx + dx) * 4 + c) as usize] as u32; }
                }
                out.push(((s + kx * ky / 2) / (kx * ky)) as u8);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_is_xxh3_of_the_decoded_pixels() {
        // XXH3-64 (seed 0) of no bytes is 2d06800538d394c2 (the reference test vector).
        assert_eq!(key(&Texture { width: 0, height: 0, rgba: Vec::new() }), "2d06800538d394c2_0x0");
        let t = Texture { width: 1, height: 1, rgba: vec![1, 2, 3, 0xff] };
        assert_eq!(key(&t), format!("{:016x}_1x1", twox_hash::XxHash3_64::oneshot(&[1, 2, 3, 0xff])));
    }

    fn dds(w: u32, h: u32, levels: u32, four: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut b = vec![0u8; 128];
        b[0..4].copy_from_slice(b"DDS ");
        b[12..16].copy_from_slice(&h.to_le_bytes());
        b[16..20].copy_from_slice(&w.to_le_bytes());
        b[28..32].copy_from_slice(&levels.to_le_bytes());
        if four == b"RGBA" {
            b[80..84].copy_from_slice(&0x41u32.to_le_bytes());
            b[88..92].copy_from_slice(&32u32.to_le_bytes());
            for (i, m) in [0xff_0000u32, 0xff00, 0xff, 0xff00_0000].iter().enumerate() { b[92 + 4 * i..96 + 4 * i].copy_from_slice(&m.to_le_bytes()); }
        } else {
            b[80..84].copy_from_slice(&4u32.to_le_bytes());
            b[84..88].copy_from_slice(four);
        }
        b.extend_from_slice(data);
        b
    }

    #[test]
    fn lod_follows_the_frame_height() {
        // 416 lines: the original's density at 4x is two levels up; 1664 lines: full detail; never below 1 or above 4.
        assert_eq!((lod_scale(4, 416), lod_scale(4, 1664), lod_scale(4, 4000), lod_scale(4, 200)), (4.0, 1.0, 1.0, 4.0));
        assert_eq!(lod_scale(1, 1080), 1.0);
        let hd = Hd { image: Image::default(), scale: 4, levels: 10 };
        FRAME_HEIGHT.store(416, Ordering::Relaxed);
        assert_eq!(hd.mxl(3), 5);
        FRAME_HEIGHT.store(0, Ordering::Relaxed);
    }

    #[test]
    fn dds_bc3_mips_and_bgra() {
        // 8×8 BC3, 4 levels: 4 + 1 + 1 + 1 blocks.
        let (img, w, levels) = from_dds(&dds(8, 8, 4, b"DXT5", &[0u8; 7 * 16])).unwrap();
        assert_eq!((w, levels, img.texture_descriptor.format, img.data.as_ref().unwrap().len()), (8, 4, TextureFormat::Bc3RgbaUnorm, 7 * 16));
        assert!(from_dds(&dds(8, 8, 4, b"DXT5", &[0u8; 6 * 16])).is_err(), "truncated");
        // BGRA masks → RGBA, alpha kept raw.
        let (img, _, levels) = from_dds(&dds(1, 1, 1, b"RGBA", &[0x30, 0x20, 0x10, 0x80])).unwrap();
        assert_eq!((levels, img.data.unwrap()), (1, vec![0x10, 0x20, 0x30, 0x80]));
        assert!(from_dds(&dds(1, 1, 1, b"DXT1", &[0u8; 8])).is_err());
    }

    #[test]
    fn png_alpha_to_the_gs_range_with_mips() {
        let mut png = image_png(2, 2, &[[10, 20, 30, 255], [10, 20, 30, 255], [50, 60, 70, 0], [50, 60, 70, 128]]);
        let (img, w, levels) = from_png(&std::mem::take(&mut png)).unwrap();
        let d = img.data.unwrap();
        assert_eq!((w, levels, d.len()), (2, 2, 2 * 2 * 4 + 4));
        assert_eq!((d[3], d[11], d[15]), (0x80, 0, 0x40));
        assert_eq!(&d[16..20], &[30, 40, 50, ((0x80 + 0x80 + 0x40 + 2) / 4) as u8]);
    }

    fn image_png(w: u32, h: u32, px: &[[u8; 4]]) -> Vec<u8> {
        let img = Image::new(Extent3d { width: w, height: h, depth_or_array_layers: 1 }, TextureDimension::D2, px.concat(), TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::MAIN_WORLD);
        let mut out = std::io::Cursor::new(Vec::new());
        img.try_into_dynamic().unwrap().write_to(&mut out, bevy::image::ImageFormat::Png.as_image_crate_format().unwrap()).unwrap();
        out.into_inner()
    }
}
