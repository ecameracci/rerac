//! Port-only: PCSX2's texture replacement names, so a pack made for PCSX2 (`<serial>/replacements/*.dds|png`) loads
//! unchanged (crate users: rc-engine's texture packs; docs/plan/graphics_options.md "Texture packs").
//!
//! Learned from PCSX2's source (GPL-3.0; read only for the scheme, nothing copied): `GSTextureReplacements.cpp`
//! (`TEXTURE_FILENAME_*`, `ParseReplacementName`), `GSTextureCache.cpp` (`HashCacheKey::Create`, `HashTexture`,
//! `HashTextureLevel`, `PaletteKeyHash`), `GSClut.cpp` (`Read32`). A name is `<tex>-<clut>-<bits>.<dds|png>`:
//! * `tex`: XXH3-64 (seed 0) of the texture as it lies in GS memory. A level at least one block in each direction
//!   (PSMT8: 16×16) is hashed block by block, left to right then down, each block's 256 bytes as the GS stores them
//!   ([`t8_block_offset`]); a smaller one row by row, one index byte per pixel ([`t8_hash_bytes`]). With PCSX2's
//!   hardware mipmapping on, a mipmapped draw also hashes its lower levels into the same state ([`t8_chain_bytes`]).
//! * `clut`: XXH3-64 of the 256-entry CLUT expanded to 32-bit RGBA in index order (CSM1 undone, raw alpha 0..0x80):
//!   [`clut_bytes`].
//! * `bits`: `PSM | TW << 6 | TH << 10 | TA0 << 15 | AEM << 23 | TA1 << 24` in `%08x` ([`tex_bits`]; TEXA is 0 for a
//!   palette format; bit 14, once TCC, is set in older packs and ignored).
//!
//! Both hashes are written `%llx` (lower case, no leading zeros); the parse takes any case and padding.

/// GS PSM of an 8-bit indexed texture.
pub const PSMT8: u32 = 0x13;

/// Byte offset of pixel (`x`, `y`) inside a PSMT8 block (16×16 pixels, 256 bytes; GS User's Manual, the PSMT8 page
/// and column layout): four 16×4 columns of 64 bytes; in a column the pixel's 32-bit word group is `x mod 8 / 2`,
/// moved by two on the column's lower or upper half-rows by the column's parity, its byte lane `y mod 4 / 2` and
/// the halves of the row interleaved by 2 bytes.
pub fn t8_block_offset(x: u32, y: u32) -> usize {
    let (c, r) = (y >> 2, y & 3);
    let swap = (r >> 1) ^ (c & 1);
    let group = ((x & 7) / 2 + 2 * swap) & 3;
    (c * 64 + group * 16 + (x & 1) * 4 + ((x >> 3) & 1) * 2 + (r & 1) * 8 + (r >> 1)) as usize
}

/// Appends the bytes PCSX2 hashes for one PSMT8 level of `w × h` indices (row-major, as the disc stores them).
pub fn t8_hash_bytes(indices: &[u8], w: u32, h: u32, out: &mut Vec<u8>) {
    let (wu, hu) = (w as usize, h as usize);
    if indices.len() < wu * hu { return; }
    if w < 16 || h < 16 {
        out.extend_from_slice(&indices[..wu * hu]);
        return;
    }
    let mut block = [0u8; 256];
    for by in (0..h).step_by(16) {
        for bx in (0..w).step_by(16) {
            for y in 0..16 {
                for x in 0..16 { block[t8_block_offset(x, y)] = indices[(by + y) as usize * wu + (bx + x) as usize]; }
            }
            out.extend_from_slice(&block);
        }
    }
}

/// [`t8_hash_bytes`] of each level in turn (level 0 first), one hash state over them all: PCSX2 with hardware
/// mipmapping on, a draw over levels 0..=MXL (at most 6).
pub fn t8_chain_bytes(levels: &[(&[u8], u32, u32)]) -> Vec<u8> {
    let mut out = Vec::new();
    for &(ix, w, h) in levels.iter().take(7) { t8_hash_bytes(ix, w, h, &mut out); }
    out
}

/// The 1024 bytes PCSX2 hashes for a 256-entry RGBA32 CLUT stored in CSM1 order (`clut`, raw alpha).
pub fn clut_bytes(clut: &[u8]) -> Option<[u8; 1024]> {
    if clut.len() < 1024 { return None; }
    let mut out = [0u8; 1024];
    for (i, e) in out.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let o = crate::texture::clut_index(i as u32) as usize * 4;
        e.copy_from_slice(&clut[o..o + 4]);
    }
    Some(out)
}

/// The name's `bits` for a palette texture of `w × h`: None unless both are powers of two (TEX0's TW / TH, at most
/// 1024: PCSX2 hashes the whole `1 << TW` by `1 << TH` area, which another size does not fill).
pub fn tex_bits(psm: u32, w: u32, h: u32) -> Option<u32> {
    let ok = |v: u32| v.is_power_of_two() && v <= 1024;
    (ok(w) && ok(h)).then(|| psm | w.trailing_zeros() << 6 | h.trailing_zeros() << 10)
}

/// A replacement's key: what a lookup matches (bit 14 dropped).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Key {
    pub tex: u64,
    /// None for a texture without a palette.
    pub clut: Option<u64>,
    pub bits: u32,
}

impl Key {
    /// The key of a file name (`<tex>[-<clut>]-<bits>.<ext>`, any case); None for another shape, a region name
    /// (`-r<w>x<h>-`) and a mip dump (`-mipN`), which PCSX2 does not load either.
    pub fn parse(file_name: &str) -> Option<Key> {
        let (stem, ext) = file_name.rsplit_once('.')?;
        if !ext.eq_ignore_ascii_case("dds") && !ext.eq_ignore_ascii_case("png") { return None; }
        let parts: Vec<&str> = stem.split('-').collect();
        let hex64 = |s: &str| (!s.is_empty() && s.len() <= 16).then(|| u64::from_str_radix(s, 16).ok()).flatten();
        let bits = |s: &str| (!s.is_empty() && s.len() <= 8).then(|| u32::from_str_radix(s, 16).ok()).flatten().map(|b| b & !(1 << 14));
        match parts.as_slice() {
            [t, b] => Some(Key { tex: hex64(t)?, clut: None, bits: bits(b)? }),
            [t, c, b] => Some(Key { tex: hex64(t)?, clut: Some(hex64(c)?), bits: bits(b)? }),
            _ => None,
        }
    }

    /// The file name stem PCSX2 dumps this key under (bit 14 clear).
    pub fn stem(&self) -> String {
        match self.clut {
            Some(c) => format!("{:x}-{:x}-{:08x}", self.tex, c, self.bits),
            None => format!("{:x}-{:08x}", self.tex, self.bits),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_layout_is_a_permutation_with_the_manuals_first_rows() {
        let mut seen = [false; 256];
        for y in 0..16 { for x in 0..16 { seen[t8_block_offset(x, y)] = true; } }
        assert!(seen.iter().all(|&s| s));
        let row = |y: u32| (0..16).map(|x| t8_block_offset(x, y)).collect::<Vec<_>>();
        assert_eq!(row(0), [0, 4, 16, 20, 32, 36, 48, 52, 2, 6, 18, 22, 34, 38, 50, 54]);
        assert_eq!(row(2), [33, 37, 49, 53, 1, 5, 17, 21, 35, 39, 51, 55, 3, 7, 19, 23]);
        assert_eq!(row(4), [96, 100, 112, 116, 64, 68, 80, 84, 98, 102, 114, 118, 66, 70, 82, 86]);
        assert_eq!(row(15), [201, 205, 217, 221, 233, 237, 249, 253, 203, 207, 219, 223, 235, 239, 251, 255]);
    }

    #[test]
    fn small_levels_hash_row_linear_and_blocks_go_across_then_down() {
        let ix: Vec<u8> = (0..64).collect();
        let mut out = Vec::new();
        t8_hash_bytes(&ix, 8, 8, &mut out);
        assert_eq!(out, ix);
        let ix: Vec<u8> = (0..32 * 16).map(|i| (i % 32 / 16) as u8).collect();
        let mut out = Vec::new();
        t8_hash_bytes(&ix, 32, 16, &mut out);
        assert_eq!((out.len(), out[0], out[255], out[256]), (512, 0, 0, 1));
    }

    #[test]
    fn names_round_trip() {
        let k = Key::parse("11F9dd5d8c63dcea-08dd80b369783795-00005993.DDS").unwrap();
        assert_eq!(k, Key { tex: 0x11f9dd5d8c63dcea, clut: Some(0x8dd80b369783795), bits: 0x1993 });
        assert_eq!(k.stem(), "11f9dd5d8c63dcea-8dd80b369783795-00001993");
        assert_eq!(tex_bits(PSMT8, 64, 64), Some(0x1993));
        assert_eq!(tex_bits(PSMT8, 48, 64), None);
        assert!(Key::parse("1-r16x0-00001993.dds").is_none() && Key::parse("1-2-00001993-mip1.png").is_none());
        assert_eq!(Key::parse("abc-00004cd3.png").map(|k| k.clut), Some(None));
        assert_eq!(clut_bytes(&[0u8; 1023]), None);
    }
}
