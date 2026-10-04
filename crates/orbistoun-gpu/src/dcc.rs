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
    if levels <= 1 {
        return meta_bytes(width, height, pipe_aligned);
    }
    let block = meta_block(2, pipe_aligned);
    let first_in_tail =
        crate::tiling::SurfaceLayout::Rx64Kb.first_level_in_tail(width, height, levels);
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
    use super::{Dcc, Keys, MetaBlock, classify, meta_block, meta_bytes, meta_start};

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
