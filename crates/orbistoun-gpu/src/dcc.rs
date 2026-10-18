//! A colour target's delta colour compression (DCC) metadata, as far as writing a frame back
//! uncompressed needs it.
//!
//! A DCC target keeps one key byte per 256-byte block of the surface, saying how that block is
//! stored. Nothing here compresses: a frame is written back as uncompressed blocks with every key
//! marking its block uncompressed, which is a state the hardware itself produces. What the keys
//! held before is decoded only where it is exact without per-block addressing - every key
//! uncompressed, or every key cleared to all-zero bits - and anything else is refused by name.
//!
//! The metadata's size follows addrlib's GFX10 rules (`gfx10addrlib.cpp`, `Gfx10Lib`) for the
//! configuration the collection's radeonsi runs under: 16 pipes, a 256-byte pipe interleave, one
//! compressed fragment and no RB+. The pipe count and interleave are the `GB_ADDR_CONFIG` oops-mesa's
//! winsys answers (`src/winsys/drm_device.c`, `NUM_PIPES = 4`), the value whose `64KB_R_X` equation
//! is the layout [`crate::tiling`] measured on hardware; the part is identified as GFX1013 at
//! external revision `0x82` (`src/winsys/device_info.c`), outside every revision range addrlib gives
//! RB+ (`amdgpu_asic_addr.h:91-98`, `gfx10addrlib.cpp:1005-1046`).

use crate::registers::SwizzleMode;

/// `DCC_UNCOMPRESSED` (`ac_descriptors.h:30`): the block is stored uncompressed.
pub const KEY_UNCOMPRESSED: u8 = 0xFF;
/// `DCC_CLEAR_0000` (`ac_descriptors.h:29`): every bit of the block is zero, whatever memory holds.
pub const KEY_CLEAR_0000: u8 = 0x00;
/// `GFX8_DCC_CLEAR_REG` (`ac_descriptors.h:37`), the form GFX10 keeps: every texel of the block is
/// the colour `CB_COLORn_CLEAR_WORD0/1` hold, whatever memory holds.
pub const KEY_CLEAR_REGISTER: u8 = 0x20;
/// `GFX9_DCC_CLEAR_SINGLE` (`ac_descriptors.h:38`): every texel of the block is the value its first
/// texel holds, the clear stored "at the beginning of every 256B block" (comp-to-single,
/// `radv_meta_nir.c:834`).
pub const KEY_CLEAR_SINGLE: u8 = 0x10;

/// GFX10's DCC key equation for a single-sample `64KB_R_X` surface (D873): which key byte describes
/// the compress block at a texel. Read off addrlib under the console's configuration by
/// `tools/addrlib/dcc` - `Addr2ComputeDccInfo`'s `equation.gfx10_bits` from element four, as Mesa
/// keeps it (`ac_copy_dcc_equation`, `ac_surface.c` 2189-2198) - which also checks the address
/// [`KeyEquation::key_offset`] computes against `Addr2ComputeDccAddrFromCoord` for every compress
/// block of three surfaces under three pipe XORs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyEquation {
    /// A compress block's texels across and down: one key's.
    pub compress: (u32, u32),
    /// `log2` of a meta block's texels across and down.
    meta_log2: (u32, u32),
    /// `log2` of a texel's bytes.
    texel_log2: u32,
    /// Element `4i + c`: which bits of coordinate `c` (x, y, z, sample) XOR into address bit `i + 1`.
    bits: [u16; 64],
}

/// The equations [`key_equation`] answers: four-byte texels, then eight, each pipe aligned then not,
/// as `tools/addrlib/dcc` printed them.
const KEY_EQUATIONS: [(u32, bool, [u16; 48]); 4] = [
    (
        4,
        true,
        [
            0x8, 0, 0, 0, 0, 0x10, 0, 0, 0x40, 0, 0, 0, 0, 0x40, 0, 0, 0x80, 0, 0, 0, 0, 0x80, 0,
            0, 0x100, 0, 0, 0, 0, 0x100, 0, 0, 0x8, 0x8, 0x8, 0, 0x10, 0x10, 0x4, 0, 0x40, 0x20,
            0x2, 0, 0x20, 0x40, 0x1, 0,
        ],
    ),
    (
        4,
        false,
        [
            0x8, 0, 0, 0, 0, 0x8, 0, 0, 0x10, 0, 0, 0, 0, 0x10, 0, 0, 0x20, 0, 0, 0, 0, 0x20, 0, 0,
            0x40, 0, 0, 0, 0, 0x40, 0, 0, 0x80, 0, 0, 0, 0, 0x80, 0, 0, 0x100, 0, 0, 0, 0, 0x100,
            0, 0,
        ],
    ),
    (
        8,
        true,
        [
            0, 0x4, 0, 0, 0x8, 0, 0, 0, 0, 0x10, 0, 0, 0x40, 0, 0, 0, 0, 0x40, 0, 0, 0x80, 0, 0, 0,
            0, 0x80, 0, 0, 0x100, 0, 0, 0, 0x8, 0x8, 0x8, 0, 0x10, 0x10, 0x4, 0, 0x40, 0x20, 0x2,
            0, 0x20, 0x40, 0x1, 0,
        ],
    ),
    (
        8,
        false,
        [
            0, 0x4, 0, 0, 0x8, 0, 0, 0, 0, 0x8, 0, 0, 0x10, 0, 0, 0, 0, 0x10, 0, 0, 0x20, 0, 0, 0,
            0, 0x20, 0, 0, 0x40, 0, 0, 0, 0, 0x40, 0, 0, 0x80, 0, 0, 0, 0, 0x80, 0, 0, 0x100, 0, 0,
            0,
        ],
    ),
];

/// The key equation for a `64KB_R_X` surface of `texel_bytes`-byte texels, or `None` for a texel
/// size not read off addrlib. Its compress block is a 256-byte micro block, and its meta block
/// 4 KiB: 8 x 8 texels and 512 x 512 at four bytes, 8 x 4 and 512 x 256 at eight.
#[must_use]
pub fn key_equation(texel_bytes: u32, pipe_aligned: bool) -> Option<KeyEquation> {
    let (_, _, terms) = KEY_EQUATIONS
        .iter()
        .find(|(bytes, aligned, _)| *bytes == texel_bytes && *aligned == pipe_aligned)?;
    let mut bits = [0_u16; 64];
    bits[..terms.len()].copy_from_slice(terms);
    let texel_log2 = texel_bytes.trailing_zeros();
    // A compress block is 256 bytes, eight texels across; a meta block 4 KiB of keys, 512 across.
    let compress = (8, 256 >> texel_log2 >> 3);
    Some(KeyEquation {
        compress,
        meta_log2: (9, 12 + 8 - texel_log2 - 9),
        texel_log2,
        bits,
    })
}

impl KeyEquation {
    /// A surface `width` texels across, as its keys are pitched: whole meta blocks.
    #[must_use]
    pub const fn pitch(&self, width: u32) -> u32 {
        width.next_multiple_of(1 << self.meta_log2.0)
    }

    /// The byte of the keys, from their first, that describes the compress block holding texel
    /// `(x, y)` of a surface whose keys are `pitch` texels across, under `pipe_xor` - Mesa's
    /// `gfx10_nir_meta_addr_from_coord` (`ac_nir_surface.c` 11-57) as `ac_nir_dcc_addr_from_coord`
    /// calls it, for one slice and sample.
    #[must_use]
    pub fn key_offset(&self, (x, y): (u32, u32), pitch: u32, pipe_xor: u32) -> u64 {
        let (width_log2, height_log2) = self.meta_log2;
        // The meta block's key bytes, `log2`, as a nibble address: the bias is `log2(bytes) - 8`.
        let size_log2 = width_log2 + height_log2 + self.texel_log2 - 8;
        let coords = [x, y, 0, 0];
        let mut address = 0_u64;
        for i in 1..=size_log2 {
            let mut v = 0_u32;
            for (c, coord) in coords.iter().enumerate() {
                let mut mask = self
                    .bits
                    .get((i as usize - 1) * 4 + c)
                    .copied()
                    .unwrap_or(0);
                while mask != 0 {
                    v ^= (coord >> mask.trailing_zeros()) & 1;
                    mask &= mask - 1;
                }
            }
            address |= u64::from(v) << i;
        }
        let block_mask = (1_u64 << size_log2) - 1;
        let pipe =
            (u64::from(pipe_xor) & ((1 << PIPES_LOG2) - 1)) << PIPE_INTERLEAVE_LOG2 & block_mask;
        let index = u64::from(y >> height_log2) * u64::from(pitch >> width_log2)
            + u64::from(x >> width_log2);
        (index << size_log2) + ((address >> 1) ^ pipe)
    }
}

/// The word every word of a fast-cleared surface holds, where its keys say so exactly without
/// knowing which key is which block's: every key a clear to `0000` or to the register with the
/// register colour zero, or every key a clear to the register with a four-byte texel, which the
/// colour's low word is. `None` for anything else, or a register colour the stream never wrote.
#[must_use]
pub fn cleared_fill(keys: &[u8], clear: Option<[u32; 2]>, texel_bytes: u32) -> Option<u32> {
    let clear = clear?;
    let only = |codes: &[u8]| keys.iter().all(|key| codes.contains(key));
    if clear == [0, 0] && only(&[KEY_CLEAR_0000, KEY_CLEAR_REGISTER]) {
        return Some(0);
    }
    (texel_bytes == 4 && only(&[KEY_CLEAR_REGISTER])).then_some(clear[0])
}

/// `log2` of the pipe count addrlib is configured with (`GB_ADDR_CONFIG.NUM_PIPES = 4`).
const PIPES_LOG2: u32 = 4;
/// `log2` of the pipe interleave (`GB_ADDR_CONFIG.PIPE_INTERLEAVE_SIZE = 0`, 256 bytes).
const PIPE_INTERLEAVE_LOG2: u32 = 8;
/// `GetMetaCacheSizeLog2` for colour data (`gfx10addrlib.cpp:5033-5041`).
const META_CACHE_SIZE_LOG2: u32 = 6;
/// A colour compress block is 256 bytes (`GetMetaBlkSize`, `compBlkSizeLog2`,
/// `gfx10addrlib.cpp:1308`), and one key byte describes it (`GetMetaElementSizeLog2` is 0 for
/// colour, `gfx10addrlib.cpp:5006-5008`).
const COMPRESS_BLOCK_LOG2: u32 = 8;
/// A `64KB` swizzle mode's block (`GetBlockSizeLog2`).
const DATA_BLOCK_LOG2: u32 = 16;

/// Where a colour target's DCC keys are and how they are laid out, from `CB_COLOR0_DCC_BASE` and
/// `CB_COLOR0_ATTRIB3.DCC_PIPE_ALIGNED`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dcc {
    /// `CB_COLOR0_DCC_BASE` (and `_EXT`) as a byte address, the metadata pipe XOR still in its low
    /// bits (`ac_descriptors.c:1504-1506`).
    pub base: u64,
    /// `DCC_PIPE_ALIGNED`: the keys are spread across the pipes with the surface.
    pub pipe_aligned: bool,
}

/// One metadata block: the bytes it holds and the pixels it covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MetaBlock {
    /// `log2` of its size in bytes.
    pub bytes_log2: u32,
    /// Pixels it covers across.
    pub width: u32,
    /// Pixels it covers down.
    pub height: u32,
}

/// `Gfx10Lib::GetMetaBlkSize` for colour data on a thin, single-sample `64KB_R_X` surface of
/// `2^element_log2`-byte elements (`gfx10addrlib.cpp:1294-1391`). `64KB_R_X` is neither a standard
/// nor a display swizzle, so a pipe-aligned surface takes the overlap branch: with no RB+, the
/// overlap is the pipe count over the 256-byte micro block (`GetMetaOverlapLog2`,
/// `gfx10addrlib.cpp:1187-1218`), and the block is at least a pipe interleave per pipe. An
/// unaligned one is capped at 4 KiB.
#[must_use]
pub const fn meta_block(element_log2: u32, pipe_aligned: bool) -> MetaBlock {
    let bytes_log2 = if pipe_aligned {
        // The 256-byte micro block's own size, `8 - elemLog2` texel bits (`GetBlk256SizeLog2`).
        let micro_log2 = 8 - element_log2;
        let overlap = PIPES_LOG2.saturating_sub(micro_log2);
        let sized = META_CACHE_SIZE_LOG2 + overlap + PIPES_LOG2;
        let floor = PIPE_INTERLEAVE_LOG2 + PIPES_LOG2;
        if sized > floor { sized } else { floor }
    } else if DATA_BLOCK_LOG2 < 12 {
        DATA_BLOCK_LOG2
    } else {
        12
    };
    // `metablkBitsLog2`: the pixels the block's keys cover, split with the odd bit across.
    let bits = bytes_log2 + COMPRESS_BLOCK_LOG2 - element_log2;
    MetaBlock {
        bytes_log2,
        width: 1 << (bits / 2 + bits % 2),
        height: 1 << (bits / 2),
    }
}

/// The bytes of DCC metadata a single-level, single-slice surface of `width` by `height` 32-bit
/// texels has (`dccRamSize`, `gfx10addrlib.cpp:432-491`): the surface padded to whole metadata
/// blocks, a block's bytes each.
#[must_use]
pub const fn meta_bytes(width: u32, height: u32, pipe_aligned: bool) -> u64 {
    texel_meta_bytes(width, height, 2, pipe_aligned)
}

/// [`meta_bytes`] for a single level of `2^element_log2`-byte texels: at one byte a texel a 4 KiB
/// metadata block covers 1024 x 1024 texels, so a 512 x 512 level has one (addrlib's `dccRamSize`
/// and `metaBlkWidth` under this configuration).
#[must_use]
pub const fn texel_meta_bytes(
    width: u32,
    height: u32,
    element_log2: u32,
    pipe_aligned: bool,
) -> u64 {
    let block = meta_block(element_log2, pipe_aligned);
    let across = width.div_ceil(block.width) as u64;
    let down = height.div_ceil(block.height) as u64;
    (across * down) << block.bytes_log2
}

/// The bytes of DCC metadata a `levels`-level 2D chain of a `64KB_R_X` surface has, level 0
/// `width` by `height` 32-bit texels (`dccRamSize`, `gfx10addrlib.cpp:436-468`): one metadata block
/// for the mip tail when there is one, then each level above it padded to whole blocks. A single
/// level is [`meta_bytes`].
#[must_use]
pub const fn chain_meta_bytes(width: u32, height: u32, levels: u32, pipe_aligned: bool) -> u64 {
    texel_chain_meta_bytes(
        width,
        height,
        levels,
        crate::tiling::SurfaceLayout::Rx64Kb,
        pipe_aligned,
    )
}

/// [`chain_meta_bytes`] for a chain laid out as `layout` lays one out, at its texels' size: at one
/// byte a texel, SuperTuxKart's ten-level 512 x 512 glyph page has a tail block and one block for
/// each of its two levels above, 12 KiB (addrlib's `dccRamSize`).
#[must_use]
pub const fn texel_chain_meta_bytes(
    width: u32,
    height: u32,
    levels: u32,
    layout: crate::tiling::SurfaceLayout,
    pipe_aligned: bool,
) -> u64 {
    let element_log2 = layout.texel_log2();
    if levels <= 1 {
        return texel_meta_bytes(width, height, element_log2, pipe_aligned);
    }
    let block = meta_block(element_log2, pipe_aligned);
    let first_in_tail = layout.first_level_in_tail(width, height, levels);
    let mut bytes = if first_in_tail < levels {
        1 << block.bytes_log2
    } else {
        0
    };
    let mut level = 0;
    while level < first_in_tail {
        let (mip_width, mip_height) = crate::tiling::mip_layout_extent(width, height, level);
        let across = mip_width.div_ceil(block.width) as u64;
        let down = mip_height.div_ceil(block.height) as u64;
        bytes += (across * down) << block.bytes_log2;
        level += 1;
    }
    bytes
}

/// The metadata's first byte: the base aligned down to a metadata block, which is the alignment
/// addrlib gives it (`dccRamBaseAlign`, `gfx10addrlib.cpp:426`), dropping the pipe XOR radeonsi
/// ORs into the low bits.
#[must_use]
pub const fn meta_start(dcc: Dcc) -> u64 {
    let block = meta_block(2, dcc.pipe_aligned);
    dcc.base & !((1 << block.bytes_log2) - 1)
}

/// What a surface's keys say about all of its blocks at once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Keys {
    /// Every block is stored uncompressed: memory holds the surface as it is.
    Uncompressed,
    /// Every block is cleared to all-zero bits, whatever memory holds.
    Clear0000,
    /// The keys differ, or say something else; decoding them needs per-block key addressing.
    Other,
}

/// Classifies a surface's whole key range.
#[must_use]
pub fn classify(keys: &[u8]) -> Keys {
    match keys.first() {
        Some(&first) if keys.iter().all(|&key| key == first) => match first {
            KEY_UNCOMPRESSED => Keys::Uncompressed,
            KEY_CLEAR_0000 => Keys::Clear0000,
            _ => Keys::Other,
        },
        _ => Keys::Other,
    }
}

/// How many of `keys` mark their block uncompressed, how many cleared to all-zero bits, how many
/// cleared to the register's colour, and how many compressed, with the most common compressed key:
/// what a refusal of mixed keys says, since the first three are exact with per-block addressing and
/// the last needs the block's encoding.
#[must_use]
pub fn census(keys: &[u8]) -> String {
    let mut counts = [0_usize; 256];
    for &key in keys {
        counts[usize::from(key)] += 1;
    }
    let (uncompressed, cleared, register) = (
        counts[usize::from(KEY_UNCOMPRESSED)],
        counts[usize::from(KEY_CLEAR_0000)],
        counts[usize::from(KEY_CLEAR_REGISTER)],
    );
    let compressed = keys.len() - uncompressed - cleared - register;
    let common = (0..=255_u8)
        .filter(|&key| ![KEY_UNCOMPRESSED, KEY_CLEAR_0000, KEY_CLEAR_REGISTER].contains(&key))
        .max_by_key(|&key| counts[usize::from(key)])
        .filter(|_| compressed > 0)
        .map_or_else(String::new, |key| format!(", most often {key:#04x}"));
    format!(
        "{} key(s): {uncompressed} uncompressed, {cleared} cleared to 0000, {register} cleared to the register, {compressed} compressed{common}",
        keys.len()
    )
}

/// Why a DCC target's metadata is not one this writes back to, or `None` when it is: a
/// single-sample, single-slice 2D `64KB_R_X` surface of any number of levels - the case whose
/// metadata is one run of whole blocks ([`chain_meta_bytes`]).
#[must_use]
pub const fn unsupported(
    tiling: Option<SwizzleMode>,
    single_slice: bool,
    single_sample: bool,
) -> Option<&'static str> {
    if !matches!(tiling, Some(SwizzleMode::Tiled64KbRX)) {
        Some("DCC on a swizzle mode other than 64KB_R_X")
    } else if !single_slice {
        Some("DCC on a target with more than one slice, or a view of one")
    } else if !single_sample {
        Some("DCC on a multisampled target")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{Dcc, Keys, MetaBlock, census, classify, meta_block, meta_bytes, meta_start};

    /// A fast clear fills its surface exactly where its keys need no per-block addressing: zeros
    /// for clears to `0000` and to a zero register colour together, the colour for clears to the
    /// register alone over four-byte texels, and nothing where the colour is unknown, the texel
    /// wider, or a block is compressed.
    #[test]
    fn a_fast_clear_fills_its_surface_where_its_keys_say_so() {
        use super::cleared_fill;
        assert_eq!(cleared_fill(&[0x00, 0x20, 0x20], Some([0, 0]), 4), Some(0));
        assert_eq!(
            cleared_fill(&[0x20, 0x20], Some([0x1122_3344, 0]), 4),
            Some(0x1122_3344)
        );
        assert_eq!(cleared_fill(&[0x00, 0x20], Some([0x1122_3344, 0]), 4), None);
        assert_eq!(cleared_fill(&[0x20, 0x20], None, 4), None);
        assert_eq!(cleared_fill(&[0x20, 0x20], Some([1, 0]), 8), None);
        assert_eq!(cleared_fill(&[0x20, 0x44], Some([0, 0]), 4), None);
    }

    /// The key equation answers addrlib's key byte for a compress block: the samples
    /// `tools/addrlib/dcc 8 1` and `dcc 4 1` print from `Addr2ComputeDccAddrFromCoord`, for a
    /// 1920 x 1080 pipe-aligned surface.
    #[test]
    fn a_key_offset_is_addrlib_s() {
        use super::key_equation;
        let eight = key_equation(8, true).expect("eight-byte texels");
        let four = key_equation(4, true).expect("four-byte texels");
        assert_eq!((eight.compress, four.compress), ((8, 4), (8, 8)));
        assert_eq!(eight.pitch(1920), 2048);
        for (equation, expected) in [
            (eight, [0x102, 0x204, 0x93b, 0x93be, 0x1348f, 0x5103]),
            (four, [0x101, 0x202, 0x91d, 0x535f, 0xb447, 0x1181]),
        ] {
            let samples = [
                (8, 0, 0),
                (0, 16, 0),
                (200, 100, 0),
                (1000, 600, 5),
                (1912, 1076, 13),
                (520, 260, 0),
            ];
            for ((x, y, pipe_xor), want) in samples.into_iter().zip(expected) {
                let got = equation.key_offset((x, y), equation.pitch(1920), pipe_xor);
                assert_eq!(got, want, "({x}, {y}) under pipe xor {pipe_xor}");
            }
        }
        assert!(key_equation(2, true).is_none());
    }

    /// A census counts each kind of key and names the commonest compressed one.
    #[test]
    fn a_census_counts_each_kind_of_key() {
        assert_eq!(
            census(&[0xFF, 0x00, 0x00, 0x20, 0x44, 0x44, 0x40]),
            "7 key(s): 1 uncompressed, 2 cleared to 0000, 1 cleared to the register, 3 compressed, most often 0x44"
        );
        assert_eq!(
            census(&[0xFF, 0x00]),
            "2 key(s): 1 uncompressed, 1 cleared to 0000, 0 cleared to the register, 0 compressed"
        );
    }

    /// A 32-bit pipe-aligned surface's metadata block is 4 KiB covering 512x512 texels, and an
    /// unaligned one is too: `GetMetaBlkSize` with 16 pipes and no RB+.
    #[test]
    fn a_32_bit_meta_block_is_4k_over_512_by_512() {
        let expected = MetaBlock {
            bytes_log2: 12,
            width: 512,
            height: 512,
        };
        assert_eq!(meta_block(2, true), expected);
        assert_eq!(meta_block(2, false), expected);
    }

    /// The sizes Craft's radeonsi cleared when it made its textures (guest-observed: the
    /// `NUM_RECORDS` of the buffer each metadata-initialising dispatch wrote) - 4 KiB for 256x256
    /// and 512x512, 16 KiB for 1024x1024.
    #[test]
    fn meta_bytes_match_what_radeonsi_initialised() {
        assert_eq!(meta_bytes(256, 256, true), 0x1000);
        assert_eq!(meta_bytes(512, 512, true), 0x1000);
        assert_eq!(meta_bytes(1024, 1024, true), 0x4000);
        assert_eq!(meta_bytes(1920, 1080, true), 4 * 3 * 0x1000);
    }

    /// A ten-level 512 x 512 chain at one byte a texel: addrlib's `dccRamSize` 12288.
    #[test]
    fn a_one_byte_chain_takes_addrlibs_key_bytes() {
        use super::texel_chain_meta_bytes;
        use crate::tiling::SurfaceLayout::Rx64KbBpp1;
        assert_eq!(
            texel_chain_meta_bytes(512, 512, 10, Rx64KbBpp1, true),
            12288
        );
        assert_eq!(texel_chain_meta_bytes(512, 512, 1, Rx64KbBpp1, true), 4096);
    }

    /// At one byte a texel a metadata block covers 1024 x 1024 texels: addrlib's `dccRamSize` for
    /// 512 x 512, 300 x 200, 1024 x 1024 and 2048 x 512 single-level 8-bpp `64KB_R_X` surfaces.
    #[test]
    fn one_byte_texels_take_addrlibs_key_bytes() {
        use super::texel_meta_bytes;
        for (width, height, bytes) in [
            (512, 512, 4096),
            (300, 200, 4096),
            (1024, 1024, 4096),
            (2048, 512, 8192),
        ] {
            assert_eq!(
                texel_meta_bytes(width, height, 0, true),
                bytes,
                "{width}x{height}"
            );
        }
    }

    /// A mip chain's keys are addrlib's `dccRamSize` for this configuration (pipe-aligned), and
    /// one level's are [`meta_bytes`].
    #[test]
    fn a_mip_chains_keys_are_addrlibs_size() {
        use super::chain_meta_bytes;
        assert_eq!(chain_meta_bytes(2048, 1024, 12, true), 57344);
        assert_eq!(chain_meta_bytes(1920, 1080, 11, true), 81920);
        assert_eq!(chain_meta_bytes(300, 200, 9, true), 16384);
        assert_eq!(chain_meta_bytes(100, 37, 3, true), 8192);
        assert_eq!(chain_meta_bytes(64, 64, 7, true), 4096);
        assert_eq!(
            chain_meta_bytes(1920, 1080, 1, true),
            meta_bytes(1920, 1080, true)
        );
    }

    /// The metadata starts at its block, whatever XOR sits in the base's low bits.
    #[test]
    fn meta_start_drops_the_pipe_xor() {
        let dcc = Dcc {
            base: 0x4_0284_0000 | 0x300,
            pipe_aligned: true,
        };
        assert_eq!(meta_start(dcc), 0x4_0284_0000);
    }

    /// Uniform keys decode; anything else is left to per-block addressing.
    #[test]
    fn only_uniform_uncompressed_or_cleared_keys_decode() {
        assert_eq!(classify(&[0xFF; 64]), Keys::Uncompressed);
        assert_eq!(classify(&[0x00; 64]), Keys::Clear0000);
        assert_eq!(
            classify(&[0x20; 64]),
            Keys::Other,
            "a clear to the register"
        );
        let mut mixed = [0xFF; 64];
        mixed[3] = 0;
        assert_eq!(classify(&mixed), Keys::Other);
        assert_eq!(classify(&[]), Keys::Other);
    }
}
