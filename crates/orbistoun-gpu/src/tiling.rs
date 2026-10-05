//! Detiling a guest's surfaces - turning a tiled render target or texture back into a linear image.
//!
//! Colour targets and textures are tiled: texel `(x, y)` sits at a swizzled offset the hardware
//! computes, not at `(y * width + x) * bpp`. A wrong swizzle passes every test built from linear
//! data and corrupts every tiled surface, so the swizzle is anchored to hardware (D010). One texel
//! was read back from a point draw into a 32-bpp `64KB_R_X` target: texel `(15, 15)` at tiled byte
//! 4348. A display-size readback (1920x1080, every pixel against a linear control) confirms the
//! in-block swizzle with row-major 64 KiB blocks. Only `64KB_R_X` at 32 bpp is modelled: it is the
//! colour-target mode, and texture tilings such as `4KB_S` have no measurement, so they are refused
//! by name. Block-compressed data needs no decode, since Vulkan consumes it natively.

use crate::registers::{
    ImageDescriptor, RegisterWrite, SwizzleMode, colour_swizzle_mode_at, colour_target_at,
};

/// The byte offset of texel `(x, y)` within a 64 KiB `R_X` tile, for a four-byte (`R8G8B8A8`-class)
/// element - the `64KB_R_X` swizzle for a 32-bpp surface.
///
/// The offset within one 64 KiB block, which is the whole address for a surface up to 128x128
/// texels. The pattern is a set of `XOR`ed bit selections, the shape of a hardware address
/// equation. It fits the hardware readback `(15, 15)` -> byte 4348 and matches obSCEne's tiler at
/// every texel of the 128x128 block, as a bijection.
#[must_use]
pub fn tiled_byte_offset_64kb_rx_bpp4(x: u32, y: u32) -> u32 {
    let mut offset = 0;
    // The row's contribution: y's low bits fan out across the block.
    offset ^= (y << 4) & 0x0070;
    offset ^= (y << 5) & 0x0f00;
    offset ^= (y << 9) & 0x1000;
    offset ^= (y << 8) & 0x4000;
    // The column's contribution.
    offset ^= (x << 2) & 0x000c;
    offset ^= (x << 5) & 0x0380;
    offset ^= (x << 4) & 0x0400;
    offset ^= (x << 6) & 0x0800;
    offset ^= (x << 9) & 0xa000;
    offset
}

/// Bytes in one `64KB_R_X` block.
const BLOCK_BYTES: usize = 64 * 1024;

/// The byte offset of texel `(x, y)` in a whole 32-bpp `64KB_R_X` surface `width` texels wide.
///
/// Blocks run row-major: the surface is `ceil(width / 128)` blocks wide, texel `(x, y)` is in block
/// `(y / 128) * blocks_per_row + x / 128`, and inside a block the swizzle is
/// [`tiled_byte_offset_64kb_rx_bpp4`]'s, with no per-block rotation. A hardware readback of a
/// 1920x1080 `64KB_R_X` draw, detiled with this rule, matches a linear control at every pixel.
/// The in-block basis equals the open-toolchain SDK's tiler (`src/agc/agc_tiler.c` in oops-sdk),
/// which a test below checks.
#[must_use]
pub fn tiled_byte_offset_64kb_rx_bpp4_surface(x: u32, y: u32, width: u32) -> usize {
    let blocks_per_row = width.div_ceil(SINGLE_BLOCK_EXTENT) as usize;
    let block =
        (y / SINGLE_BLOCK_EXTENT) as usize * blocks_per_row + (x / SINGLE_BLOCK_EXTENT) as usize;
    block * BLOCK_BYTES
        + tiled_byte_offset_64kb_rx_bpp4(x % SINGLE_BLOCK_EXTENT, y % SINGLE_BLOCK_EXTENT) as usize
}

/// Texels per side of an 8-bpp `64KB_R_X` block: 64 KiB of one-byte texels, 256 x 256.
const RX_BPP1_BLOCK_EXTENT: u32 = 256;

/// Where each of the low eight bits of `x`, then of `y`, lands in an 8-bpp `64KB_R_X` block's byte
/// offset: the offset is the XOR of the entries whose coordinate bit is set.
///
/// From addrlib for this console's configuration - family NV, revision 0x82, `GB_ADDR_CONFIG` 0x4,
/// the one whose 32-bpp `64KB_R_X` equation is the layout measured on hardware - read off
/// `Addr2ComputeSurfaceAddrFromCoord` one coordinate bit at a time. The same rule, row-major blocks
/// with this equation inside each and no rotation between them, gives addrlib's address for all
/// 262,144 texels of a 512 x 512 surface.
const RX_BPP1_X_BITS: [usize; 8] = [0x1, 0x2, 0x4, 0x140, 0x200, 0x800, 0x2400, 0x8000];
/// The same for `y`.
const RX_BPP1_Y_BITS: [usize; 8] = [0x10, 0x8, 0x20, 0x100, 0x280, 0x400, 0x1800, 0x4000];

/// Byte offset of texel `(x, y)` in an 8-bpp `64KB_R_X` surface `width` texels wide: its block,
/// row-major, then its place in the block, before any pipe-bank XOR.
#[must_use]
pub fn tiled_byte_offset_64kb_rx_bpp1_surface(x: u32, y: u32, width: u32) -> usize {
    let blocks_per_row = width.div_ceil(RX_BPP1_BLOCK_EXTENT) as usize;
    let block =
        (y / RX_BPP1_BLOCK_EXTENT) as usize * blocks_per_row + (x / RX_BPP1_BLOCK_EXTENT) as usize;
    let mut offset = 0;
    for (bit, place) in RX_BPP1_X_BITS.iter().enumerate() {
        if x >> bit & 1 == 1 {
            offset ^= place;
        }
    }
    for (bit, place) in RX_BPP1_Y_BITS.iter().enumerate() {
        if y >> bit & 1 == 1 {
            offset ^= place;
        }
    }
    block * BLOCK_BYTES + offset
}

/// [`tiled_byte_offset_64kb_rx_bpp1_surface`] with the surface's pipe-bank XOR applied: the
/// 256-byte runs within each block move by it, `blkOffset ^ (pipeBankXor << 8)`
/// (`gfx10addrlib.cpp:4786-4849`), as at four bytes a texel.
#[must_use]
pub fn tiled_byte_offset_64kb_rx_bpp1_xor(x: u32, y: u32, width: u32, pipe_bank_xor: u8) -> usize {
    tiled_byte_offset_64kb_rx_bpp1_surface(x, y, width) ^ (usize::from(pipe_bank_xor) << 8)
}

/// Bytes a whole 8-bpp `64KB_R_X` surface occupies: every block it touches, whole.
#[must_use]
pub fn surface_bytes_64kb_rx_bpp1(width: u32, height: u32) -> usize {
    width.div_ceil(RX_BPP1_BLOCK_EXTENT) as usize
        * height.div_ceil(RX_BPP1_BLOCK_EXTENT) as usize
        * BLOCK_BYTES
}

/// Words a whole 32-bpp `64KB_R_X` surface occupies: every block it touches, whole.
#[must_use]
pub fn surface_words_64kb_rx_bpp4(width: u32, height: u32) -> usize {
    width.div_ceil(SINGLE_BLOCK_EXTENT) as usize
        * height.div_ceil(SINGLE_BLOCK_EXTENT) as usize
        * (BLOCK_BYTES / 4)
}

/// Detiles a whole 32-bpp `64KB_R_X` surface of any size into a linear, row-major image, through
/// [`tiled_byte_offset_64kb_rx_bpp4_surface`].
///
/// # Errors
///
/// [`DetileError::TiledDataTooShort`] when `tiled` does not cover every block the surface touches.
pub fn detile_surface_64kb_rx_bpp4(
    tiled: &[u32],
    width: u32,
    height: u32,
) -> Result<Vec<u32>, DetileError> {
    let needed_words = surface_words_64kb_rx_bpp4(width, height);
    if tiled.len() < needed_words {
        return Err(DetileError::TiledDataTooShort {
            needed_words,
            got_words: tiled.len(),
        });
    }
    Ok(detile_surface_64kb_rx_bpp4_mapped(
        tiled,
        width,
        height,
        0,
        |w| w,
    ))
}

/// The in-block swizzle as two tables of word offsets, one per column and one per row.
///
/// [`tiled_byte_offset_64kb_rx_bpp4`] XORs a term that depends only on `x` with one that depends
/// only on `y`, so a texel's word within its block is `column[x] ^ row[y]`.
fn swizzle_tables() -> &'static ([usize; 128], [usize; 128]) {
    static TABLES: std::sync::OnceLock<([usize; 128], [usize; 128])> = std::sync::OnceLock::new();
    TABLES.get_or_init(|| {
        let column =
            std::array::from_fn(|x| tiled_byte_offset_64kb_rx_bpp4(x as u32, 0) as usize / 4);
        let row = std::array::from_fn(|y| tiled_byte_offset_64kb_rx_bpp4(0, y as u32) as usize / 4);
        (column, row)
    })
}

/// Words in one 64 KiB block.
const BLOCK_WORDS: usize = BLOCK_BYTES / 4;

/// A surface's pipe-bank XOR as a word offset within its block. The XOR applies at the 256-byte
/// pipe interleave, `blkOffset ^ (pipeBankXor << m_pipeInterleaveLog2)` in addrlib
/// (`gfx10addrlib.cpp:4786-4849`), so it permutes 256-byte runs within a block and never moves a
/// texel out of one. radeonsi carries it in the low byte of the surface's 256-byte base
/// (`ac_descriptors.c:1477-1481`, `cb_color_base |= tile_swizzle`).
const fn xor_words(pipe_bank_xor: u8) -> usize {
    (pipe_bank_xor as usize) << 6
}

/// [`detile_surface_64kb_rx_bpp4`] for a surface with pipe-bank XOR `pipe_bank_xor` (see
/// `xor_words`), passing each texel through `map` (a component swap) in the same pass, and
/// assuming `tiled` covers the surface.
///
/// One thread per row of blocks: each row is a contiguous run of `tiled` and of the linear image,
/// so the rows convert independently.
///
/// # Panics
///
/// When `tiled` is shorter than [`surface_words_64kb_rx_bpp4`]; the checked form reports that as
/// an error instead.
#[must_use]
pub fn detile_surface_64kb_rx_bpp4_mapped(
    tiled: &[u32],
    width: u32,
    height: u32,
    pipe_bank_xor: u8,
    map: impl Fn(u32) -> u32 + Sync,
) -> Vec<u32> {
    let (column, row) = swizzle_tables();
    let xor = xor_words(pipe_bank_xor);
    let (w, extent) = (width as usize, SINGLE_BLOCK_EXTENT as usize);
    let block_row_words = w.div_ceil(extent) * BLOCK_WORDS;
    let mut linear = vec![0u32; w * height as usize];
    std::thread::scope(|scope| {
        for (by, rows) in linear.chunks_mut(w * extent).enumerate() {
            let blocks = &tiled[by * block_row_words..(by + 1) * block_row_words];
            let map = &map;
            scope.spawn(move || {
                for (dy, out) in rows.chunks_mut(w).enumerate() {
                    let r = row[dy] ^ xor;
                    for (x, texel) in out.iter_mut().enumerate() {
                        *texel = map(blocks[(x / extent) * BLOCK_WORDS + (column[x % extent] ^ r)]);
                    }
                }
            });
        }
    });
    linear
}

/// [`tile_surface_64kb_rx_bpp4`], passing each texel through `map`, one thread per row of blocks
/// as in [`detile_surface_64kb_rx_bpp4_mapped`]. Assumes the sizes the checked form checks.
fn tile_mapped(
    linear: &[u32],
    width: u32,
    pipe_bank_xor: u8,
    tiled: &mut [u32],
    map: &(impl Fn(u32) -> u32 + Sync),
) {
    let (column, row) = swizzle_tables();
    let xor = xor_words(pipe_bank_xor);
    let (w, extent) = (width as usize, SINGLE_BLOCK_EXTENT as usize);
    let block_row_words = w.div_ceil(extent) * BLOCK_WORDS;
    std::thread::scope(|scope| {
        for (rows, blocks) in linear
            .chunks(w * extent)
            .zip(tiled.chunks_mut(block_row_words))
        {
            scope.spawn(move || {
                for (dy, line) in rows.chunks(w).enumerate() {
                    let r = row[dy] ^ xor;
                    for (x, &texel) in line.iter().enumerate() {
                        blocks[(x / extent) * BLOCK_WORDS + (column[x % extent] ^ r)] = map(texel);
                    }
                }
            });
        }
    });
}

/// [`tile_surface_64kb_rx_bpp4`] for a surface with pipe-bank XOR `pipe_bank_xor` (see
/// `xor_words`), passing each texel through `map` (a component swap) in the same pass.
///
/// # Errors
///
/// As [`tile_surface_64kb_rx_bpp4`].
pub fn tile_surface_64kb_rx_bpp4_mapped(
    linear: &[u32],
    width: u32,
    height: u32,
    pipe_bank_xor: u8,
    tiled: &mut [u32],
    map: impl Fn(u32) -> u32 + Sync,
) -> Result<(), DetileError> {
    let needed_words = surface_words_64kb_rx_bpp4(width, height);
    let texels = width as usize * height as usize;
    if tiled.len() < needed_words || linear.len() != texels {
        return Err(DetileError::TiledDataTooShort {
            needed_words: needed_words.max(texels),
            got_words: tiled.len().min(linear.len()),
        });
    }
    tile_mapped(linear, width, pipe_bank_xor, tiled, &map);
    Ok(())
}

/// Tiles a linear, row-major 32-bpp image into a whole `64KB_R_X` surface's words - the inverse of
/// [`detile_surface_64kb_rx_bpp4`], for writing a rendered frame back where the guest reads it.
/// Words the image does not cover (a partial edge block's padding) are left as they were.
///
/// # Errors
///
/// [`DetileError::TiledDataTooShort`] when `tiled` does not cover every block the surface touches,
/// and the same when `linear` is not exactly `width * height` texels.
pub fn tile_surface_64kb_rx_bpp4(
    linear: &[u32],
    width: u32,
    height: u32,
    tiled: &mut [u32],
) -> Result<(), DetileError> {
    tile_surface_64kb_rx_bpp4_mapped(linear, width, height, 0, tiled, |w| w)
}

/// The widest surface the single-block swizzle covers, per dimension, at 32 bpp.
///
/// A 64 KiB block holds 128x128 four-byte texels; a larger surface spans multiple blocks.
const SINGLE_BLOCK_EXTENT: u32 = 128;

/// Why a surface could not be detiled, stated rather than papered over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetileError {
    /// The surface is larger than one 64 KiB block in some dimension, which the single-block detile
    /// does not model.
    SurfaceExceedsBlock {
        /// Width in texels, as asked.
        width: u32,
        /// Height in texels, as asked.
        height: u32,
    },
    /// The tiled words given do not reach the highest offset the swizzle addresses for this
    /// surface.
    TiledDataTooShort {
        /// Words the swizzle needs to reach every texel.
        needed_words: usize,
        /// Words actually supplied.
        got_words: usize,
    },
    /// The surface's format is not one the 32-bpp swizzle applies to - the element is a different
    /// size, or a size this does not model - so detiling it would read across or within elements.
    UnsupportedFormat {
        /// The GFX10 `IMG_FORMAT` code the descriptor carried.
        format: u32,
    },
}

impl std::fmt::Display for DetileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SurfaceExceedsBlock { width, height } => write!(
                f,
                "surface {width}x{height} exceeds one 64KB block ({SINGLE_BLOCK_EXTENT} per side); block-level tiling is not measured"
            ),
            Self::TiledDataTooShort {
                needed_words,
                got_words,
            } => write!(
                f,
                "tiled data is {got_words} words, swizzle needs {needed_words} to reach every texel"
            ),
            Self::UnsupportedFormat { format } => write!(
                f,
                "image format {format} is not a 32-bpp format the measured swizzle applies to"
            ),
        }
    }
}

impl std::error::Error for DetileError {}

/// Detile a 32-bpp `64KB_R_X` surface into a linear, row-major image of `width * height` texels.
///
/// `tiled` is the surface's own words in guest memory (the caller slices at the descriptor's base).
/// The result at index `y * width + x` is the texel the swizzle placed at
/// [`tiled_byte_offset_64kb_rx_bpp4`]`(x, y)`.
///
/// # Errors
///
/// [`DetileError::SurfaceExceedsBlock`] if either dimension is over 128;
/// [`DetileError::TiledDataTooShort`] if `tiled` does not reach the highest swizzled word this
/// surface addresses.
pub fn detile_64kb_rx_bpp4(
    tiled: &[u32],
    width: u32,
    height: u32,
) -> Result<Vec<u32>, DetileError> {
    if width > SINGLE_BLOCK_EXTENT || height > SINGLE_BLOCK_EXTENT {
        return Err(DetileError::SurfaceExceedsBlock { width, height });
    }

    // The highest word the swizzle reaches decides whether the data is long enough, checked once so
    // the per-texel reads below are all in bounds.
    let mut needed_words = 0usize;
    for y in 0..height {
        for x in 0..width {
            let word = tiled_byte_offset_64kb_rx_bpp4(x, y) as usize / 4;
            needed_words = needed_words.max(word + 1);
        }
    }
    if tiled.len() < needed_words {
        return Err(DetileError::TiledDataTooShort {
            needed_words,
            got_words: tiled.len(),
        });
    }

    let mut linear = vec![0u32; (width * height) as usize];
    for y in 0..height {
        for x in 0..width {
            let word = tiled_byte_offset_64kb_rx_bpp4(x, y) as usize / 4;
            linear[(y * width + x) as usize] = tiled[word];
        }
    }
    Ok(linear)
}

/// Bytes per texel for a GFX10 `IMG_FORMAT` code, or `None` for one whose element size this does
/// not model.
///
/// The codes are `GFX10_FORMAT_*` (`src/amd/registers/gfx10-rsrc.json` in oops-mesa), laid out in
/// ascending element size: `1..=6` one byte, `7..=19` two, `20..=61` four, `62..=71` eight,
/// `72..=74` twelve, `75..=77` sixteen; `128`/`129`/`130` are the `8`/`8_8`/`8_8_8_8` sRGB
/// variants. Every other code (packed 16-bit, depth, subsampled, FMASK, block-compressed) is
/// refused rather than sized from its name, since a wrong size mis-reads every texel.
fn element_bytes(format: u32) -> Option<u32> {
    match format {
        1..=6 | 128 => Some(1),   // single 8-bit channel, and 8_SRGB
        7..=19 | 129 => Some(2),  // 16-bit or 8_8, and 8_8_SRGB
        20..=61 | 130 => Some(4), // the 32-bpp block, and 8_8_8_8_SRGB
        62..=71 => Some(8),
        72..=74 => Some(12),
        75..=77 => Some(16),
        _ => None,
    }
}

/// The element size the measured `64KB_R_X` swizzle is defined for: four bytes, 32 bpp.
const MEASURED_ELEMENT_BYTES: u32 = 4;

/// A thin wrapper over [`detile_64kb_rx_bpp4`] that takes the extent from the descriptor. It
/// refuses a format that is not 32 bpp (`element_bytes`); confirming the tiling mode is
/// `64KB_R_X` is the caller's job.
///
/// # Errors
///
/// [`DetileError::UnsupportedFormat`] for a non-32-bpp format, else [`detile_64kb_rx_bpp4`]'s
/// refusals.
pub fn detile_image(descriptor: &ImageDescriptor, tiled: &[u32]) -> Result<Vec<u32>, DetileError> {
    if element_bytes(descriptor.format) != Some(MEASURED_ELEMENT_BYTES) {
        return Err(DetileError::UnsupportedFormat {
            format: descriptor.format,
        });
    }
    detile_64kb_rx_bpp4(tiled, descriptor.width, descriptor.height)
}

/// A detiled surface - a colour target or a texture - its pixels laid out linearly, row-major.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Surface {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// `width * height` pixels, row-major.
    pub pixels: Vec<u32>,
}

/// Why a surface could not be detiled out of a guest-memory window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SurfaceError {
    /// The base or extent was not set - a colour target whose stream wrote neither.
    Incomplete,
    /// The tiling mode is one orbistoun does not detile - linear, or a mode it does not model.
    UnsupportedTiling(SwizzleMode),
    /// The target's base is not inside the guest-memory window given.
    OutsideWindow {
        /// The surface's base address.
        base: u64,
        /// The address the guest slice's first word sits at.
        window_base: u64,
        /// How many words the window holds.
        window_words: usize,
    },
    /// The detile refused: the surface is larger than one block, or the window is too short for it.
    Detile(DetileError),
}

impl std::fmt::Display for SurfaceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Incomplete => write!(f, "the surface's base or extent was not set"),
            Self::UnsupportedTiling(mode) => {
                write!(f, "tiling mode {mode:?} is not one orbistoun detiles")
            }
            Self::OutsideWindow {
                base,
                window_base,
                window_words,
            } => write!(
                f,
                "surface base {base:#x} is outside the guest window at {window_base:#x} ({window_words} words)"
            ),
            Self::Detile(inner) => write!(f, "detile refused: {inner}"),
        }
    }
}

impl std::error::Error for SurfaceError {}

/// Detiles a surface out of a guest-memory window, given its base, extent and tiling mode.
///
/// The shared core of [`detile_colour_target`] and [`detile_texture`]: the base is mapped into
/// `guest`, whose first word sits at `window_base`, and [`detile_64kb_rx_bpp4`] unswizzles it. Any
/// mode but `64KB_R_X`, including linear, is refused (D010). The caller ensures a 32-bpp format.
fn detile_surface(
    base: u64,
    width: u32,
    height: u32,
    tiling: SwizzleMode,
    guest: &[u32],
    window_base: u64,
) -> Result<Surface, SurfaceError> {
    if tiling != SwizzleMode::Tiled64KbRX {
        return Err(SurfaceError::UnsupportedTiling(tiling));
    }

    let outside = SurfaceError::OutsideWindow {
        base,
        window_base,
        window_words: guest.len(),
    };
    let offset_bytes = base
        .checked_sub(window_base)
        .ok_or_else(|| outside.clone())?;
    let offset_words = usize::try_from(offset_bytes / 4).map_err(|_| outside.clone())?;
    let words = guest.get(offset_words..).ok_or(outside)?;

    let pixels = detile_64kb_rx_bpp4(words, width, height).map_err(SurfaceError::Detile)?;
    Ok(Surface {
        width,
        height,
        pixels,
    })
}

/// Detiles colour buffer zero out of a guest-memory window, reading its base, extent and tiling
/// mode from the register writes.
///
/// [`colour_target_at`] gives the base and size and [`colour_swizzle_mode_at`] the tiling; then
/// `detile_surface` maps the base into `guest` (first word at `window_base`) and unswizzles it.
///
/// # Errors
///
/// [`SurfaceError::Incomplete`] when the stream set no base or extent, else `detile_surface`'s
/// refusals (a tiling not modelled, a base outside the window, or a detile refusal).
pub fn detile_colour_target(
    writes: &[RegisterWrite],
    guest: &[u32],
    window_base: u64,
) -> Result<Surface, SurfaceError> {
    let target = colour_target_at(writes).ok_or(SurfaceError::Incomplete)?;
    let mode = colour_swizzle_mode_at(writes).ok_or(SurfaceError::Incomplete)?;
    detile_surface(
        target.base,
        target.width,
        target.height,
        mode,
        guest,
        window_base,
    )
}

/// Detiles a texture out of a guest-memory window, from its decoded descriptor.
///
/// The texture analog of [`detile_colour_target`]: an [`ImageDescriptor`] carries the base, extent,
/// format and tiling (`crate::registers::decode_image_descriptor`). A non-32-bpp format is refused
/// before the window is touched, since a different element size is a different swizzle.
///
/// # Errors
///
/// [`SurfaceError::Detile`]`(`[`DetileError::UnsupportedFormat`]`)` for a non-32-bpp format;
/// otherwise `UnsupportedTiling` unless the descriptor's tiling is `64KB_R_X`, else the window and
/// detile refusals. It never returns `Incomplete`: a descriptor always carries base and extent.
pub fn detile_texture(
    descriptor: &ImageDescriptor,
    guest: &[u32],
    window_base: u64,
) -> Result<Surface, SurfaceError> {
    if element_bytes(descriptor.format) != Some(MEASURED_ELEMENT_BYTES) {
        return Err(SurfaceError::Detile(DetileError::UnsupportedFormat {
            format: descriptor.format,
        }));
    }
    detile_surface(
        descriptor.base,
        descriptor.width,
        descriptor.height,
        descriptor.tiling,
        guest,
        window_base,
    )
}

/// Texels per side of a 32-bpp `4KB_D_X` block: 4 KiB of four-byte texels, 32 x 32.
const DX_BLOCK_EXTENT: u32 = 32;
/// Bytes one `4KB_D_X` block holds.
const DX_BLOCK_BYTES: usize = 4096;

/// Where each of the low seven bits of `x`, then of `y`, lands in a 32-bpp `4KB_D_X` block's
/// byte offset: the offset is the XOR of the entries whose coordinate bit is set. Bits 5 and 6
/// lie above the 32-texel block and fold into the pipe bits, which is what the `_X` names.
///
/// From addrlib (`gfx10addrlib.cpp`, `gfx10SwizzlePattern.h`) for this console's configuration -
/// family NV, revision 0x82 (GFX1013), `GB_ADDR_CONFIG` 0x4: 16 pipes, a 256-byte interleave, no
/// RB+ - read off `Addr2ComputeSurfaceAddrFromCoord` one coordinate bit at a time, and confirmed
/// to be the whole equation over 200,000 texels of a 1024 x 1024 surface. The same library and
/// configuration give the `64KB_R_X` layout above for all 2,073,600 texels of a display surface,
/// the layout measured on hardware; radeonsi in the guest lays its surfaces out with them.
const DX_X_BITS: [usize; 7] = [0x4, 0x8, 0x80, 0x200, 0x800, 0x400, 0x100];
/// The same for `y`.
const DX_Y_BITS: [usize; 7] = [0x10, 0x20, 0x40, 0x100, 0x400, 0x800, 0x200];

/// Byte offset of texel `(x, y)` in a 32-bpp `4KB_D_X` surface `width` texels wide: its block,
/// row-major with the pitch rounded up to whole blocks, then its place in the block.
#[must_use]
pub fn tiled_byte_offset_4kb_dx_bpp4_surface(x: u32, y: u32, width: u32) -> usize {
    let blocks_per_row = width.div_ceil(DX_BLOCK_EXTENT) as usize;
    let block = (y / DX_BLOCK_EXTENT) as usize * blocks_per_row + (x / DX_BLOCK_EXTENT) as usize;
    let mut offset = 0;
    for (bit, place) in DX_X_BITS.iter().enumerate() {
        if x >> bit & 1 == 1 {
            offset ^= place;
        }
    }
    for (bit, place) in DX_Y_BITS.iter().enumerate() {
        if y >> bit & 1 == 1 {
            offset ^= place;
        }
    }
    block * DX_BLOCK_BYTES + offset
}

/// Words a whole 32-bpp `4KB_D_X` surface occupies: every block it touches, whole.
#[must_use]
pub fn surface_words_4kb_dx_bpp4(width: u32, height: u32) -> usize {
    width.div_ceil(DX_BLOCK_EXTENT) as usize
        * height.div_ceil(DX_BLOCK_EXTENT) as usize
        * (DX_BLOCK_BYTES / 4)
}

/// Texels per side of a 32-bpp 256-byte micro block (`Block256_2d`), the unit a mip tail places
/// its levels in.
const MICRO_EXTENT: u32 = 8;

/// Level `level`'s extent as a chain's layout counts it, level 0 `width` x `height`: each side
/// halved per level, rounding up, never below one (`Gfx10Lib::GetMipSize`, `ShiftCeil`,
/// `gfx10addrlib.h:367-383`). A level's texels are its sides halved rounding down; the layout
/// rounds up, so an odd side's level is padded rather than cut.
pub const fn mip_layout_extent(width: u32, height: u32, level: u32) -> (u32, u32) {
    (shift_ceil(width, level), shift_ceil(height, level))
}

/// `side` halved `level` times, rounding up, never below one.
const fn shift_ceil(side: u32, level: u32) -> u32 {
    let side = if side == 0 { 1 } else { side };
    if level >= 32 {
        return 1;
    }
    let shifted = (side + (1 << level) - 1) >> level;
    if shifted == 0 { 1 } else { shifted }
}

/// Where level `level` of a `levels`-level linear 2D chain of 32-bit texels, level 0 `width` x
/// `height`, starts in bytes from the surface's base, and its row pitch in texels; `None` past the
/// chain. [`linear_level_of`] at four bytes a texel.
#[must_use]
pub const fn linear_level(width: u32, height: u32, levels: u32, level: u32) -> Option<(u64, u32)> {
    linear_level_of(width, height, (levels, level), 4)
}

/// [`linear_level`] at `bytes_per_texel`, one, two or four.
///
/// `Gfx10Lib::HwlComputeSurfaceInfoLinear` (`gfx10addrlib.cpp:5084-5105`): the levels are stored
/// last first, each its rows at a 256-byte-aligned pitch - 256 / `bytes_per_texel` texels - its
/// own height apart. Checked against addrlib at one byte a texel: a ten-level 512 x 512 chain puts
/// level 0 at 130816 and level 9 first, and 300 x 200's level 4 takes thirteen rows.
#[must_use]
pub const fn linear_level_of(
    width: u32,
    height: u32,
    (levels, level): (u32, u32),
    bytes_per_texel: u32,
) -> Option<(u64, u32)> {
    if level >= levels || !matches!(bytes_per_texel, 1 | 2 | 4) {
        return None;
    }
    let align = 256 / bytes_per_texel;
    let mut offset = 0;
    let mut below = levels;
    while below > level + 1 {
        below -= 1;
        let (w, h) = mip_layout_extent(width, height, below);
        offset += w.next_multiple_of(align) as u64 * h as u64 * bytes_per_texel as u64;
    }
    let (w, _) = mip_layout_extent(width, height, level);
    Some((offset, w.next_multiple_of(align)))
}

/// How a colour target's texels lie in memory, for the swizzle modes modelled: at 32 bpp, and
/// `64KB_R_X` at 8 bpp.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SurfaceLayout {
    /// `64KB_R_X`, the render-target mode measured on hardware.
    #[default]
    Rx64Kb,
    /// `4KB_D_X`, from addrlib under the configuration that reproduces the measured one.
    Dx4Kb,
    /// `LINEAR`: rows at a 256-byte-aligned pitch, 64 texels, which the hardware derives from the
    /// width (`ADDR_SW_LINEAR`, as [`crate::registers::linear_pitch`] gives it for an image with no
    /// custom pitch; a colour target has no pitch register to give another).
    Linear,
    /// `64KB_R_X` at one byte a texel: 256 x 256-texel blocks, row-major, each laid out by
    /// addrlib's 8-bpp equation ([`tiled_byte_offset_64kb_rx_bpp1_surface`]).
    Rx64KbBpp1,
    /// `LINEAR` at one byte a texel: rows at a 256-byte-aligned pitch, 256 texels
    /// ([`linear_level_of`]).
    LinearBpp1,
}

/// A linear surface's row pitch in texels: its width rounded up to 64, 256 bytes.
const fn linear_row_pitch(width: u32) -> u32 {
    width.next_multiple_of(64)
}

impl SurfaceLayout {
    /// The layout a swizzle mode names, or `None` for one not modelled.
    #[must_use]
    pub const fn of(mode: SwizzleMode) -> Option<Self> {
        match mode {
            SwizzleMode::Tiled64KbRX => Some(Self::Rx64Kb),
            SwizzleMode::Tiled4KbDX => Some(Self::Dx4Kb),
            SwizzleMode::Linear => Some(Self::Linear),
            SwizzleMode::Other(_) => None,
        }
    }

    /// This 32-bpp layout's counterpart at `bytes_per_texel`, or `None` where that size is not
    /// modelled in it.
    #[must_use]
    pub const fn at_bytes_per_texel(self, bytes_per_texel: u32) -> Option<Self> {
        match (self, bytes_per_texel) {
            (Self::Rx64Kb, 1) => Some(Self::Rx64KbBpp1),
            (Self::Linear, 1) => Some(Self::LinearBpp1),
            (_, 4) => Some(self),
            _ => None,
        }
    }

    /// `log2` of the bytes one texel of this layout takes.
    #[must_use]
    pub const fn texel_log2(self) -> u32 {
        match self {
            Self::Rx64KbBpp1 | Self::LinearBpp1 => 0,
            Self::Rx64Kb | Self::Dx4Kb | Self::Linear => 2,
        }
    }

    /// Whether this is a linear layout, whose levels each have rows of their own.
    const fn is_linear(self) -> bool {
        matches!(self, Self::Linear | Self::LinearBpp1)
    }

    /// A linear layout's row pitch in texels: 256 bytes' worth, as [`linear_level_of`] aligns it.
    const fn linear_pitch(self, width: u32) -> u32 {
        width.next_multiple_of(256 >> self.texel_log2())
    }

    /// Words a whole surface of this layout occupies.
    #[must_use]
    pub fn words(self, width: u32, height: u32) -> usize {
        match self {
            Self::Rx64Kb => surface_words_64kb_rx_bpp4(width, height),
            Self::Dx4Kb => surface_words_4kb_dx_bpp4(width, height),
            // Every row but the last at its pitch; the last ends at its width.
            Self::Linear => {
                linear_row_pitch(width) as usize * (height.max(1) as usize - 1) + width as usize
            }
            // Whole 64 KiB blocks, so whole words.
            Self::Rx64KbBpp1 => surface_bytes_64kb_rx_bpp1(width, height) / 4,
            // Every row's bytes but the last at its pitch; the last ends at its width, and the
            // words cover the last byte.
            Self::LinearBpp1 => (self.linear_pitch(width) as usize * (height.max(1) as usize - 1)
                + width as usize)
                .div_ceil(4),
        }
    }

    /// `log2` of the bytes one block of this layout holds (`GetBlockSizeLog2`).
    const fn block_log2(self) -> u32 {
        match self {
            Self::Rx64Kb | Self::Rx64KbBpp1 => 16,
            // A linear surface has no blocks; its "block" is a row's 256 bytes, never a tail's.
            Self::Dx4Kb => 12,
            Self::Linear | Self::LinearBpp1 => 8,
        }
    }

    /// A block's extent in texels, across and down: its texel bits split evenly, as
    /// `ComputeBlockDimensionForSurf` gives a thin 2D surface.
    const fn block_extent(self) -> (u32, u32) {
        let texels_log2 = self.block_log2() - self.texel_log2();
        (1 << (texels_log2 - texels_log2 / 2), 1 << (texels_log2 / 2))
    }

    /// The first level of a `levels`-level 2D chain, level 0 `width` x `height`, that lies in the
    /// mip tail - the one block every level from it down shares - or `levels` when none does.
    ///
    /// `Gfx10Lib::ComputeSurfaceInfoMacroTiled` (`gfx10addrlib.cpp:3904-3940`): a level is in the
    /// tail when it fits half a block across and a whole block down (`GetMipTailDim`) and no more
    /// levels follow than a tail holds (`GetMaxNumMipsInTail`). A single level has no tail.
    #[must_use]
    pub const fn first_level_in_tail(self, width: u32, height: u32, levels: u32) -> u32 {
        // A linear chain has no tail: every level has rows of its own.
        if levels <= 1 || self.is_linear() {
            return levels;
        }
        let (block_width, block_height) = self.block_extent();
        let log2 = self.block_log2();
        let most_in_tail = if log2 <= 11 {
            1 + (1 << (log2 - 9))
        } else {
            log2 - 4
        };
        let mut level = 0;
        while level < levels {
            let (w, h) = mip_layout_extent(width, height, level);
            if w <= block_width / 2 && h <= block_height && levels - level <= most_in_tail {
                return level;
            }
            level += 1;
        }
        levels
    }

    /// Bytes level `level` of a chain occupies outside the tail: its extent padded to whole blocks.
    const fn level_bytes(self, width: u32, height: u32, level: u32) -> u64 {
        if self.is_linear() {
            let (w, h) = mip_layout_extent(width, height, level);
            return (self.linear_pitch(w) as u64 * h as u64) << self.texel_log2();
        }
        let (block_width, block_height) = self.block_extent();
        let (w, h) = mip_layout_extent(width, height, level);
        (w.next_multiple_of(block_width) as u64 * h.next_multiple_of(block_height) as u64)
            << self.texel_log2()
    }

    /// Where level `level` of a `levels`-level 2D chain, level 0 `width` x `height`, starts,
    /// in bytes from the surface's base; `None` for a level in the mip tail or past the chain.
    ///
    /// The chain is stored smallest first (`gfx10addrlib.cpp:3995-4010`): the tail's block, when
    /// there is one, then each level above it upwards, so level 0 comes last. Within its place a
    /// level is laid out as a single-level surface of its own extent would be.
    #[must_use]
    pub const fn level_offset(
        self,
        width: u32,
        height: u32,
        levels: u32,
        level: u32,
    ) -> Option<u64> {
        if self.is_linear() {
            return match linear_level_of(width, height, (levels, level), 1 << self.texel_log2()) {
                Some((offset, _)) => Some(offset),
                None => None,
            };
        }
        let first_in_tail = self.first_level_in_tail(width, height, levels);
        if level >= first_in_tail {
            return None;
        }
        let mut offset = if first_in_tail < levels {
            1 << self.block_log2()
        } else {
            0
        };
        let mut below = level + 1;
        while below < first_in_tail {
            offset += self.level_bytes(width, height, below);
            below += 1;
        }
        Some(offset)
    }

    /// Where level `level` of a `levels`-level 2D chain, level 0 `width` x `height`, lies in the
    /// mip tail's block, as the texel of that block its own `(0, 0)` is; `None` for a level outside
    /// the tail or past the chain. The tail's block is the chain's first (`macroBlockOffset` 0).
    ///
    /// `gfx10addrlib.cpp:4012-4063`: the level's place in the tail is an offset, `16 << m` or
    /// `m << 8` for `m` counting down from the tail's capacity, whose bits interleave into x and y
    /// in units of the 256-byte micro block - 8 x 8 texels at 32 bpp, 16 x 16 at 8. Every layout's
    /// blocks are an even power of two, so x and y are not exchanged.
    #[must_use]
    pub const fn tail_origin(
        self,
        width: u32,
        height: u32,
        levels: u32,
        level: u32,
    ) -> Option<(u32, u32)> {
        let first_in_tail = self.first_level_in_tail(width, height, levels);
        if level < first_in_tail || level >= levels {
            return None;
        }
        let log2 = self.block_log2();
        let most_in_tail = if log2 <= 11 {
            1 + (1 << (log2 - 9))
        } else {
            log2 - 4
        };
        let m = most_in_tail - 1 - (level - first_in_tail);
        let offset = if m > 6 { 16 << m } else { m << 8 };
        let mut x = 0;
        let mut y = 0;
        let mut bit = 0;
        while bit < 6 {
            x |= (offset >> (9 + bit)) & (1 << bit);
            y |= (offset >> (8 + bit)) & (1 << bit);
            bit += 1;
        }
        // The micro block's side: its 256 bytes' texels, square.
        let micro = MICRO_EXTENT << ((2 - self.texel_log2()) / 2);
        Some((x * micro, y * micro))
    }

    /// A block's extent in texels, across and down.
    #[must_use]
    pub const fn block_texels(self) -> (u32, u32) {
        self.block_extent()
    }

    /// Bytes a whole `levels`-level 2D chain, level 0 `width` x `height`, occupies
    /// (`surfSize`).
    #[must_use]
    pub const fn chain_bytes(self, width: u32, height: u32, levels: u32) -> u64 {
        let first_in_tail = self.first_level_in_tail(width, height, levels);
        let mut bytes = if first_in_tail < levels {
            1 << self.block_log2()
        } else {
            0
        };
        let mut level = 0;
        while level < first_in_tail {
            bytes += self.level_bytes(width, height, level);
            level += 1;
        }
        bytes
    }

    /// Whether a surface of this layout with pipe-bank XOR `pipe_bank_xor` is modelled: every
    /// `64KB_R_X` one, and a `4KB_D_X` one without an XOR.
    #[must_use]
    pub const fn models(self, pipe_bank_xor: u8) -> bool {
        match self {
            Self::Rx64Kb | Self::Rx64KbBpp1 => true,
            Self::Dx4Kb | Self::Linear | Self::LinearBpp1 => pipe_bank_xor == 0,
        }
    }

    /// Detiles a whole surface into a linear, row-major image, each texel through `map`.
    ///
    /// # Panics
    ///
    /// When `tiled` is shorter than [`Self::words`].
    #[must_use]
    pub fn detile_mapped(
        self,
        tiled: &[u32],
        width: u32,
        height: u32,
        pipe_bank_xor: u8,
        map: impl Fn(u32) -> u32 + Sync,
    ) -> Vec<u32> {
        match self {
            Self::Rx64Kb => {
                detile_surface_64kb_rx_bpp4_mapped(tiled, width, height, pipe_bank_xor, map)
            }
            Self::Dx4Kb => {
                let mut linear = Vec::with_capacity(width as usize * height as usize);
                for y in 0..height {
                    for x in 0..width {
                        linear.push(map(
                            tiled[tiled_byte_offset_4kb_dx_bpp4_surface(x, y, width) / 4]
                        ));
                    }
                }
                linear
            }
            Self::Linear => {
                let pitch = linear_row_pitch(width) as usize;
                (0..height as usize)
                    .flat_map(|row| tiled[row * pitch..][..width as usize].iter().copied())
                    .map(map)
                    .collect()
            }
            // Each texel is one byte of the little-endian words, its rows at the pitch.
            Self::LinearBpp1 => {
                let pitch = self.linear_pitch(width) as usize;
                (0..height as usize)
                    .flat_map(|row| (0..width as usize).map(move |x| row * pitch + x))
                    .map(|at| map(tiled[at / 4] >> ((at % 4) * 8) & 0xFF))
                    .collect()
            }
            // Each texel is one byte of the little-endian words.
            Self::Rx64KbBpp1 => (0..height)
                .flat_map(|y| (0..width).map(move |x| (x, y)))
                .map(|(x, y)| {
                    let at = tiled_byte_offset_64kb_rx_bpp1_xor(x, y, width, pipe_bank_xor);
                    map(tiled[at / 4] >> ((at % 4) * 8) & 0xFF)
                })
                .collect(),
        }
    }

    /// Tiles a linear, row-major image into a whole surface, each texel through `map`, leaving
    /// the words the image does not cover as they were.
    ///
    /// # Errors
    ///
    /// [`DetileError::TiledDataTooShort`] when `tiled` does not cover the surface or `linear` is
    /// not `width * height` texels.
    pub fn tile_mapped(
        self,
        linear: &[u32],
        width: u32,
        height: u32,
        pipe_bank_xor: u8,
        tiled: &mut [u32],
        map: impl Fn(u32) -> u32 + Sync,
    ) -> Result<(), DetileError> {
        match self {
            Self::Rx64Kb => {
                tile_surface_64kb_rx_bpp4_mapped(linear, width, height, pipe_bank_xor, tiled, map)
            }
            Self::Dx4Kb => {
                let needed_words = surface_words_4kb_dx_bpp4(width, height);
                let texels = width as usize * height as usize;
                if tiled.len() < needed_words || linear.len() != texels {
                    return Err(DetileError::TiledDataTooShort {
                        needed_words: needed_words.max(texels),
                        got_words: tiled.len().min(linear.len()),
                    });
                }
                for (y, row) in linear.chunks(width as usize).enumerate() {
                    for (x, &texel) in row.iter().enumerate() {
                        tiled[tiled_byte_offset_4kb_dx_bpp4_surface(x as u32, y as u32, width)
                            / 4] = map(texel);
                    }
                }
                Ok(())
            }
            Self::Linear => {
                let needed_words = self.words(width, height);
                let texels = width as usize * height as usize;
                if tiled.len() < needed_words || linear.len() != texels {
                    return Err(DetileError::TiledDataTooShort {
                        needed_words: needed_words.max(texels),
                        got_words: tiled.len().min(linear.len()),
                    });
                }
                let pitch = linear_row_pitch(width) as usize;
                for (y, row) in linear.chunks(width as usize).enumerate() {
                    for (slot, &texel) in tiled[y * pitch..][..row.len()].iter_mut().zip(row) {
                        *slot = map(texel);
                    }
                }
                Ok(())
            }
            // Each texel's low byte, into its byte of the little-endian words.
            Self::LinearBpp1 => {
                let needed_words = self.words(width, height);
                let texels = width as usize * height as usize;
                if tiled.len() < needed_words || linear.len() != texels {
                    return Err(DetileError::TiledDataTooShort {
                        needed_words: needed_words.max(texels),
                        got_words: tiled.len().min(linear.len()),
                    });
                }
                let pitch = self.linear_pitch(width) as usize;
                for (index, &texel) in linear.iter().enumerate() {
                    let at = index / width as usize * pitch + index % width as usize;
                    let shift = (at % 4) * 8;
                    let word = &mut tiled[at / 4];
                    *word = (*word & !(0xFF << shift)) | ((map(texel) & 0xFF) << shift);
                }
                Ok(())
            }
            // Each texel's low byte, into its byte of the little-endian words.
            Self::Rx64KbBpp1 => {
                let needed_words = self.words(width, height);
                let texels = width as usize * height as usize;
                if tiled.len() < needed_words || linear.len() != texels {
                    return Err(DetileError::TiledDataTooShort {
                        needed_words: needed_words.max(texels),
                        got_words: tiled.len().min(linear.len()),
                    });
                }
                for (index, &texel) in linear.iter().enumerate() {
                    let (x, y) = (index as u32 % width, index as u32 / width);
                    let at = tiled_byte_offset_64kb_rx_bpp1_xor(x, y, width, pipe_bank_xor);
                    let shift = (at % 4) * 8;
                    let word = &mut tiled[at / 4];
                    *word = (*word & !(0xFF << shift)) | ((map(texel) & 0xFF) << shift);
                }
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DetileError, SurfaceError, detile_64kb_rx_bpp4, detile_colour_target,
        detile_surface_64kb_rx_bpp4, detile_texture, surface_words_64kb_rx_bpp4,
        tile_surface_64kb_rx_bpp4, tiled_byte_offset_64kb_rx_bpp4 as offset,
        tiled_byte_offset_64kb_rx_bpp4_surface as at,
    };
    use crate::registers::{ImageDescriptor, RegisterWrite, SwizzleMode};

    /// A pipe-bank XOR moves every texel within its block by the XOR shifted to the 256-byte pipe
    /// interleave (`gfx10addrlib.cpp:4786-4849`: `blkOffset ^ (pipeBankXor << 8)`), and the two
    /// directions stay each other's inverse under it.
    #[test]
    fn a_pipe_bank_xor_moves_each_texel_within_its_block() {
        use super::{detile_surface_64kb_rx_bpp4_mapped, tile_surface_64kb_rx_bpp4_mapped};
        let (width, height) = (256, 130);
        let linear: Vec<u32> = (0..width * height).collect();
        let mut tiled = vec![0u32; surface_words_64kb_rx_bpp4(width, height)];
        tile_surface_64kb_rx_bpp4_mapped(&linear, width, height, 0xc0, &mut tiled, |w| w)
            .expect("tiles");
        for (x, y) in [(0, 0), (15, 15), (200, 3), (255, 129), (130, 128)] {
            let word = (at(x, y, width) ^ (0xc0 << 8)) / 4;
            assert_eq!(tiled[word], y * width + x, "({x},{y})");
        }
        assert_eq!(
            detile_surface_64kb_rx_bpp4_mapped(&tiled, width, height, 0xc0, |w| w),
            linear
        );
    }

    /// Inside the first block, the whole-surface address is the one-block swizzle, for any width.
    #[test]
    fn surface_address_inside_block_zero_is_the_block_swizzle() {
        for (x, y) in [(0, 0), (15, 15), (127, 0), (0, 127), (127, 127), (64, 33)] {
            for width in [128, 129, 1920] {
                assert_eq!(
                    at(x, y, width),
                    offset(x, y) as usize,
                    "({x},{y}) width {width}"
                );
            }
        }
    }

    /// Blocks run row-major, 64 KiB each, with no per-block rotation: a 1920-wide surface is 15
    /// blocks wide, so (128,0) is block 1 and (0,128) is block 15, each at in-block (0,0).
    #[test]
    fn surface_blocks_are_row_major_and_unrotated() {
        assert_eq!(at(128, 0, 1920), 65536);
        assert_eq!(at(0, 128, 1920), 15 * 65536);
        assert_eq!(
            at(1919, 1079, 1920),
            (8 * 15 + 14) * 65536 + offset(127, 55) as usize
        );
        // A width that is not a whole number of blocks still rounds up: 129 wide is 2 blocks.
        assert_eq!(at(0, 128, 129), 2 * 65536);
        assert_eq!(surface_words_64kb_rx_bpp4(1920, 1080), 135 * 16384);
    }

    /// The in-block swizzle equals the open-toolchain SDK's basis at every texel
    /// (`src/agc/agc_tiler.c` in oops-sdk).
    #[test]
    fn block_swizzle_equals_the_measured_basis() {
        const X_BASIS: [u32; 7] = [0x4, 0x8, 0x80, 0x100, 0x2200, 0x800, 0x8400];
        const Y_BASIS: [u32; 7] = [0x10, 0x20, 0x40, 0x1100, 0x200, 0x400, 0x4800];
        let fold = |v: u32, basis: &[u32; 7]| {
            (0..7)
                .filter(|b| v >> b & 1 == 1)
                .fold(0, |acc, b| acc ^ basis[b])
        };
        for y in 0..128 {
            for x in 0..128 {
                assert_eq!(
                    offset(x, y),
                    fold(x, &X_BASIS) ^ fold(y, &Y_BASIS),
                    "({x},{y})"
                );
            }
        }
    }

    /// Tiling then detiling returns the image, at a size spanning partial edge blocks - the
    /// property the write-back of a rendered frame depends on.
    #[test]
    fn tile_then_detile_round_trips_a_partial_block_surface() {
        let (width, height) = (300, 150);
        let linear: Vec<u32> = (0..width * height)
            .map(|i: u32| i.wrapping_mul(2_654_435_761))
            .collect();
        let mut tiled = vec![0u32; surface_words_64kb_rx_bpp4(width, height)];
        tile_surface_64kb_rx_bpp4(&linear, width, height, &mut tiled).expect("tiles");
        assert_eq!(
            detile_surface_64kb_rx_bpp4(&tiled, width, height).expect("detiles"),
            linear
        );
    }

    /// A surface whose blocks the data does not cover is refused, in both directions.
    #[test]
    fn surface_shorter_than_its_blocks_is_refused() {
        let short = vec![0u32; surface_words_64kb_rx_bpp4(300, 150) - 1];
        assert!(matches!(
            detile_surface_64kb_rx_bpp4(&short, 300, 150),
            Err(DetileError::TiledDataTooShort { .. })
        ));
        let mut short = short;
        assert!(tile_surface_64kb_rx_bpp4(&vec![0; 300 * 150], 300, 150, &mut short).is_err());
        let mut whole = vec![0u32; surface_words_64kb_rx_bpp4(300, 150)];
        assert!(tile_surface_64kb_rx_bpp4(&[0; 10], 300, 150, &mut whole).is_err());
    }

    /// Register writes setting colour buffer zero's base (256-byte units), extent and tiling.
    fn target_writes(base_256b: u32, attrib2: u32, attrib3: u32) -> Vec<RegisterWrite> {
        let write = |register, value| RegisterWrite {
            packet_offset: 0,
            register,
            value,
        };
        // CB_COLOR0_BASE 0xA318, CB_COLOR0_ATTRIB2 0xA3B0, CB_COLOR0_ATTRIB3 0xA3B8.
        vec![
            write(0xA318, base_256b),
            write(0xA3B0, attrib2),
            write(0xA3B8, attrib3),
        ]
    }

    /// The bridge reads base, extent and tiling from the stream and detiles the surface, with the
    /// base-to-window offset applied: the measured pixel at tiled byte 4348 comes back at linear
    /// index `15*64+15`.
    #[test]
    fn detile_colour_target_reads_the_stream_and_places_the_measured_pixel() {
        const RED: u32 = 0xff00_00ff;
        // Base 0x2000e0000; window starts 0x400 bytes (256 words) earlier, so the surface is at
        // word offset 256 in the guest slice.
        let base = 0x2_000e_0000u64;
        let window_base = base - 0x400;
        let writes = target_writes(0x0200_0e00, 0x000f_c03f, 0x08c6_c000);
        let mut guest = vec![0u32; 256 + 4096];
        guest[256 + 4348 / 4] = RED; // the measured tiled byte, in the surface at offset 256

        let surface = detile_colour_target(&writes, &guest, window_base).expect("detiles");
        assert_eq!((surface.width, surface.height), (64, 64));
        assert_eq!(
            surface.pixels[(15 * 64 + 15) as usize],
            RED,
            "texel (15,15)"
        );
        assert_eq!(surface.pixels.iter().filter(|&&p| p == RED).count(), 1);
    }

    /// A stream missing the base or the extent is refused as incomplete.
    #[test]
    fn detile_colour_target_refuses_incomplete_state() {
        let write = |register, value| RegisterWrite {
            packet_offset: 0,
            register,
            value,
        };
        // Extent and tiling set, but no base.
        let writes = vec![write(0xA3B0, 0x000f_c03f), write(0xA3B8, 0x08c6_c000)];
        assert_eq!(
            detile_colour_target(&writes, &[0u32; 4096], 0),
            Err(SurfaceError::Incomplete)
        );
    }

    /// A tiling mode orbistoun does not detile is refused by name, not read with the wrong swizzle.
    #[test]
    fn detile_colour_target_refuses_a_mode_it_does_not_model() {
        // ATTRIB3 = 0 -> COLOR_SW_MODE 0 -> linear, which the detile does not handle.
        let writes = target_writes(0x0200_0e00, 0x000f_c03f, 0x0000_0000);
        assert_eq!(
            detile_colour_target(&writes, &[0u32; 4096], 0x2_000e_0000),
            Err(SurfaceError::UnsupportedTiling(SwizzleMode::Linear))
        );
    }

    /// A base below the window is refused, not read from a negative offset.
    #[test]
    fn detile_colour_target_refuses_a_base_outside_the_window() {
        let writes = target_writes(0x0200_0e00, 0x000f_c03f, 0x08c6_c000);
        // Window starts *after* the base, so the base is not in it.
        let window_base = 0x2_000e_0000u64 + 0x1000;
        match detile_colour_target(&writes, &[0u32; 4096], window_base) {
            Err(SurfaceError::OutsideWindow { base, .. }) => assert_eq!(base, 0x2_000e_0000),
            other => panic!("expected an outside-window refusal, got {other:?}"),
        }
    }

    /// A tiled texture detiles from its descriptor: the measured pixel at byte 4348 comes back at
    /// linear index `15*64+15`.
    #[test]
    fn detile_texture_reads_the_descriptor_and_places_the_measured_pixel() {
        const RED: u32 = 0xff00_00ff;
        let base = 0x2_000e_0000u64;
        let window_base = base - 0x400;
        let descriptor = ImageDescriptor {
            base,
            width: 64,
            height: 64,
            format: 56, // GFX10_FORMAT_8_8_8_8_UNORM, a 32-bpp format
            tiling: SwizzleMode::Tiled64KbRX,
            pipe_bank_xor: 0,
            levels: 1,
            base_level: 0,
            last_level: 0,
            compression: None,
        };
        let mut guest = vec![0u32; 256 + 4096];
        guest[256 + 4348 / 4] = RED;

        let surface = detile_texture(&descriptor, &guest, window_base).expect("detiles");
        assert_eq!((surface.width, surface.height), (64, 64));
        assert_eq!(
            surface.pixels[(15 * 64 + 15) as usize],
            RED,
            "texel (15,15)"
        );
        assert_eq!(surface.pixels.iter().filter(|&&p| p == RED).count(), 1);
    }

    /// A linear texture is refused by its mode, not detiled with the tiled swizzle.
    #[test]
    fn detile_texture_refuses_a_linear_descriptor() {
        let descriptor = ImageDescriptor {
            base: 0x2_000e_0000,
            width: 64,
            height: 64,
            format: 56, // GFX10_FORMAT_8_8_8_8_UNORM, a 32-bpp format
            tiling: SwizzleMode::Linear,
            pipe_bank_xor: 0,
            levels: 1,
            base_level: 0,
            last_level: 0,
            compression: None,
        };
        assert_eq!(
            detile_texture(&descriptor, &[0u32; 4096], 0x2_000e_0000),
            Err(SurfaceError::UnsupportedTiling(SwizzleMode::Linear))
        );
    }

    /// A non-32-bpp texture is refused before the window is touched, not detiled with the 32-bpp
    /// swizzle: a `16_16_16_16` surface would have every texel read across elements.
    #[test]
    fn detile_texture_refuses_a_non_32_bpp_format() {
        let descriptor = ImageDescriptor {
            base: 0x2_000e_0000,
            width: 64,
            height: 64,
            format: 71, // GFX10_FORMAT_16_16_16_16_FLOAT, eight bytes per texel
            tiling: SwizzleMode::Tiled64KbRX,
            pipe_bank_xor: 0,
            levels: 1,
            base_level: 0,
            last_level: 0,
            compression: None,
        };
        assert_eq!(
            detile_texture(&descriptor, &[0u32; 4096], 0x2_000e_0000),
            Err(SurfaceError::Detile(DetileError::UnsupportedFormat {
                format: 71
            })),
        );
    }

    /// The element-size decode agrees with the cited `GFX10_FORMAT` table at each size boundary,
    /// and a block-compressed code is refused rather than sized.
    #[test]
    fn the_format_element_size_matches_the_cited_table() {
        use super::element_bytes;
        assert_eq!(element_bytes(56), Some(4), "8_8_8_8_UNORM");
        assert_eq!(element_bytes(50), Some(4), "2_10_10_10_UNORM");
        assert_eq!(element_bytes(36), Some(4), "10_11_11_FLOAT");
        assert_eq!(element_bytes(19), Some(2), "8_8_SINT, just below the block");
        assert_eq!(element_bytes(62), Some(8), "32_32_UINT, just above it");
        assert_eq!(element_bytes(71), Some(8), "16_16_16_16_FLOAT");
        assert_eq!(element_bytes(130), Some(4), "8_8_8_8_SRGB");
        assert_eq!(element_bytes(169), None, "BC1_UNORM is refused, not sized");
        assert_eq!(element_bytes(0), None, "INVALID is refused");
    }

    /// The hardware-measured byte and a second point obSCEne's tiler computes.
    ///
    /// `(15,15)` -> byte 4348 is the texel read back on hardware; `(32,21)` -> byte 2640 is the
    /// software model's value, which this closed form reproduces. A linear layout fails the test
    /// (byte 4348 sits at `(63,16)` there).
    #[test]
    fn the_measured_byte_and_the_model_agreed_second_point() {
        assert_eq!(offset(15, 15), 4348, "the one measured byte");
        assert_eq!(offset(15, 15), 0x10fc);
        assert_eq!(
            offset(32, 21),
            2640,
            "the point obSCEne's tiler computes, reproduced"
        );
        assert_eq!(offset(32, 21), 0xa50);
        assert_ne!(offset(32, 21), 4348, "and not 4348");
    }

    /// The origin is byte 0 and the element's own two low bits are never swizzled, so no texel is
    /// read across its four-byte element.
    #[test]
    fn the_origin_is_zero_and_offsets_are_element_aligned() {
        assert_eq!(offset(0, 0), 0);
        for x in 0..16 {
            for y in 0..16 {
                assert_eq!(offset(x, y) & 0x3, 0, "texel ({x},{y}) is element-aligned");
            }
        }
    }

    /// The swizzle is a bijection over a 128x128 block: no two texels share a byte.
    #[test]
    fn the_swizzle_is_a_bijection_over_the_block() {
        let mut seen = std::collections::HashSet::new();
        for y in 0..128 {
            for x in 0..128 {
                assert!(seen.insert(offset(x, y)), "texel ({x},{y}) collides");
            }
        }
        assert_eq!(seen.len(), 128 * 128);
    }

    /// Detiling inverts the swizzle: a surface whose every texel carries its own `(x,y)` comes back
    /// row-major.
    #[test]
    fn detile_returns_each_texel_to_its_linear_place() {
        let (width, height) = (16u32, 16u32);
        // Lay a tiled buffer out as the hardware would: each texel's encoded (x,y) at its swizzled
        // offset. `+1` because the highest offset is inclusive.
        let mut needed = 0usize;
        for y in 0..height {
            for x in 0..width {
                needed = needed.max(offset(x, y) as usize / 4 + 1);
            }
        }
        let mut tiled = vec![0u32; needed];
        for y in 0..height {
            for x in 0..width {
                tiled[offset(x, y) as usize / 4] = (y << 16) | x;
            }
        }

        let linear = detile_64kb_rx_bpp4(&tiled, width, height).expect("fits in one block");
        for y in 0..height {
            for x in 0..width {
                assert_eq!(
                    linear[(y * width + x) as usize],
                    (y << 16) | x,
                    "texel ({x},{y}) landed wrong"
                );
            }
        }
    }

    /// The measured pixel detiles to its linear place and nothing else moves: a 64x64 surface with
    /// one marker at word 1087 comes back with it at linear index `15*64+15` and every other pixel
    /// clear.
    #[test]
    fn the_measured_pixel_detiles_to_its_linear_index() {
        const RED: u32 = 0xff00_00ff;
        let (width, height) = (64u32, 64u32);
        let mut tiled = vec![0u32; 4096];
        tiled[4348 / 4] = RED; // the measured tiled byte, as a word index

        let linear = detile_64kb_rx_bpp4(&tiled, width, height).expect("64x64 fits in one block");
        assert_eq!(linear[(15 * width + 15) as usize], RED, "texel (15,15)");
        assert_eq!(
            linear.iter().filter(|&&p| p == RED).count(),
            1,
            "only the measured texel is red"
        );
    }

    /// A surface past one block is refused, not silently mis-tiled.
    #[test]
    fn a_surface_beyond_one_block_is_refused() {
        let tiled = vec![0u32; 16384];
        assert_eq!(
            detile_64kb_rx_bpp4(&tiled, 129, 64),
            Err(DetileError::SurfaceExceedsBlock {
                width: 129,
                height: 64
            })
        );
    }

    /// Tiled data too short to cover the swizzle is refused rather than read out of bounds: a 64x64
    /// surface reaches word 1087, and one word shorter must refuse.
    #[test]
    fn tiled_data_too_short_is_refused() {
        let tiled = vec![0u32; 8];
        match detile_64kb_rx_bpp4(&tiled, 64, 64) {
            Err(DetileError::TiledDataTooShort { got_words, .. }) => assert_eq!(got_words, 8),
            other => panic!("expected a too-short refusal, got {other:?}"),
        }
    }

    /// `4KB_D_X` offsets are addrlib's for this console's configuration, at three sizes whose
    /// pitch and height are not whole blocks (the probe's corners and eight texels each), and
    /// each size's extent is addrlib's surface size.
    #[test]
    fn dx_4kb_offsets_are_addrlibs() {
        use super::{surface_words_4kb_dx_bpp4, tiled_byte_offset_4kb_dx_bpp4_surface as dx};
        /// Width, height, addrlib's surface bytes, and texels with addrlib's offsets.
        type Case = (u32, u32, usize, &'static [(u32, u32, usize)]);
        let cases: [Case; 3] = [
            (
                165,
                165,
                147_456,
                &[
                    (0, 0, 0),
                    (164, 0, 21632),
                    (0, 164, 124_992),
                    (164, 164, 146_624),
                    (34, 54, 30824),
                    (8, 51, 28208),
                    (111, 11, 14012),
                    (5, 71, 49908),
                    (39, 35, 31932),
                    (60, 35, 30384),
                    (124, 132, 114_624),
                    (150, 133, 116_952),
                ],
            ),
            (
                100,
                37,
                32768,
                &[
                    (0, 0, 0),
                    (99, 0, 13580),
                    (0, 36, 18496),
                    (99, 36, 32076),
                    (98, 1, 13592),
                    (41, 30, 4964),
                    (24, 25, 3856),
                    (48, 23, 6256),
                    (4, 33, 18576),
                    (12, 12, 960),
                    (80, 22, 11616),
                    (13, 1, 660),
                ],
            ),
            (
                1000,
                70,
                393_216,
                &[
                    (0, 0, 0),
                    (999, 0, 128_396),
                    (0, 69, 262_736),
                    (999, 69, 391_132),
                    (536, 45, 197_456),
                    (28, 53, 132_816),
                    (719, 53, 225_244),
                    (626, 12, 80968),
                    (420, 27, 53680),
                    (447, 10, 57260),
                    (893, 64, 376_196),
                    (648, 27, 83760),
                ],
            ),
        ];
        for (width, height, bytes, texels) in cases {
            assert_eq!(
                surface_words_4kb_dx_bpp4(width, height) * 4,
                bytes,
                "{width}x{height}"
            );
            for &(x, y, want) in texels {
                assert_eq!(dx(x, y, width), want, "({x},{y}) in {width}x{height}");
            }
        }
    }

    /// A `4KB_D_X` surface tiles and detiles back to the image it was given.
    #[test]
    fn dx_4kb_tiles_and_detiles_round_trip() {
        use super::SurfaceLayout;
        let (width, height) = (165, 165);
        let linear: Vec<u32> = (0..width * height)
            .map(|i: u32| i.wrapping_mul(2_654_435_761))
            .collect();
        let mut tiled = vec![0u32; SurfaceLayout::Dx4Kb.words(width, height)];
        SurfaceLayout::Dx4Kb
            .tile_mapped(&linear, width, height, 0, &mut tiled, |w| w)
            .expect("tiles");
        assert_eq!(
            SurfaceLayout::Dx4Kb.detile_mapped(&tiled, width, height, 0, |w| w),
            linear
        );
    }

    /// A mip chain's levels lie where addrlib puts them for this console's configuration
    /// (`Addr2ComputeSurfaceInfo` with `pMipInfo`), levels in the tail have no place of their own,
    /// and the chain's size is addrlib's `surfSize`.
    #[test]
    fn mip_levels_lie_where_addrlib_puts_them() {
        use super::SurfaceLayout::{self, Dx4Kb, Rx64Kb};
        /// Layout, level 0's extent, levels, the chain's bytes, each level's offset out of the tail.
        type Chain = (SurfaceLayout, u32, u32, u32, u64, &'static [u64]);
        let cases: [Chain; 8] = [
            (
                Rx64Kb,
                2048,
                1024,
                12,
                11_272_192,
                &[2_883_584, 786_432, 262_144, 131_072, 65536],
            ),
            (
                Rx64Kb,
                1920,
                1080,
                11,
                12_648_448,
                &[3_801_088, 1_179_648, 393_216, 131_072, 65536],
            ),
            (Rx64Kb, 300, 200, 9, 655_360, &[262_144, 131_072, 65536]),
            (Rx64Kb, 100, 37, 3, 131_072, &[65536]),
            (Rx64Kb, 64, 64, 7, 65536, &[]),
            (Dx4Kb, 165, 165, 8, 208_896, &[61440, 24576, 8192, 4096]),
            (
                Dx4Kb,
                1000,
                70,
                10,
                589_824,
                &[196_608, 65536, 32768, 16384, 8192, 4096],
            ),
            (Dx4Kb, 16, 16, 5, 4096, &[]),
        ];
        for (layout, width, height, levels, bytes, offsets) in cases {
            let shape = format!("{layout:?} {width}x{height} x{levels}");
            assert_eq!(layout.chain_bytes(width, height, levels), bytes, "{shape}");
            let in_tail = u32::try_from(offsets.len()).expect("few levels");
            assert_eq!(
                layout.first_level_in_tail(width, height, levels),
                in_tail,
                "{shape}"
            );
            for level in 0..levels {
                let want = offsets.get(level as usize).copied();
                assert_eq!(
                    layout.level_offset(width, height, levels, level),
                    want,
                    "{shape} {level}"
                );
            }
        }
        // One level has no tail, whatever its size.
        assert_eq!(Rx64Kb.level_offset(64, 64, 1, 0), Some(0));
        assert_eq!(Rx64Kb.chain_bytes(64, 64, 1), 65536);
    }

    /// Level 0 of a chain is laid out as a single-level surface of its extent, from where the
    /// chain places it: addrlib's texel addresses (`Addr2ComputeSurfaceAddrFromCoord`, mip 0 of
    /// the chain) less the level's offset are the single-level ones.
    #[test]
    fn a_levels_texels_are_a_single_level_surfaces_from_its_offset() {
        use super::SurfaceLayout::{Dx4Kb, Rx64Kb};
        use super::{
            tiled_byte_offset_4kb_dx_bpp4_surface as dx,
            tiled_byte_offset_64kb_rx_bpp4_surface as rx,
        };
        /// Level 0's extent, levels, and texels with addrlib's address for each.
        type Texels = (u32, u32, u32, &'static [(u32, u32, u64)]);
        let rx_cases: [Texels; 2] = [
            (
                2048,
                1024,
                12,
                &[
                    (0, 0, 2_883_584),
                    (1, 0, 2_883_588),
                    (2047, 1023, 11_268_348),
                    (100, 200, 3_986_816),
                    (1500, 3, 3_647_408),
                ],
            ),
            (
                1920,
                1080,
                11,
                &[
                    (0, 0, 3_801_088),
                    (1919, 1079, 12_626_428),
                    (777, 555, 8_131_636),
                    (33, 1000, 10_704_132),
                ],
            ),
        ];
        for (width, height, levels, texels) in rx_cases {
            let base = Rx64Kb
                .level_offset(width, height, levels, 0)
                .expect("level 0");
            for &(x, y, want) in texels {
                assert_eq!(
                    base + rx(x, y, width) as u64,
                    want,
                    "({x},{y}) in {width}x{height}"
                );
            }
        }
        let base = Dx4Kb.level_offset(165, 165, 8, 0).expect("level 0");
        for (x, y, want) in [
            (0, 0, 61440),
            (164, 164, 208_064),
            (34, 54, 92264),
            (111, 11, 75452),
        ] {
            assert_eq!(base + dx(x, y, 165) as u64, want, "({x},{y})");
        }
    }

    /// A tail level's origin in the tail's block is addrlib's `mipTailCoordX`/`Y`, for each level
    /// in the tail, and a level outside it has none.
    #[test]
    fn tail_levels_lie_where_addrlib_puts_them() {
        use super::SurfaceLayout::{Dx4Kb, Rx64Kb};
        let rx = [
            (64, 0),
            (0, 64),
            (32, 0),
            (0, 32),
            (16, 0),
            (8, 16),
            (0, 24),
        ];
        for (level, origin) in (4..11).zip(rx) {
            assert_eq!(
                Rx64Kb.tail_origin(1024, 128, 11, level),
                Some(origin),
                "{level}"
            );
        }
        assert_eq!(Rx64Kb.tail_origin(1024, 128, 11, 3), None);
        let dx = [(16, 0), (8, 16), (0, 24), (0, 16), (8, 8)];
        for (level, origin) in (2..7).zip(dx) {
            assert_eq!(Dx4Kb.tail_origin(64, 32, 7, level), Some(origin), "{level}");
        }
        assert_eq!(Dx4Kb.tail_origin(64, 32, 7, 7), None, "past the chain");
    }

    /// A tail level's texels are the block's at its origin: addrlib's addresses for level 6 of a
    /// 1024x128 `64KB_R_X` chain and level 2 of a 64x32 `4KB_D_X` one, through the target's own
    /// tiling, which leaves the rest of the block - the other levels - as it was.
    #[test]
    fn a_tail_level_is_tiled_at_its_origin_in_the_block() {
        use super::SurfaceLayout::{self, Dx4Kb, Rx64Kb};
        use crate::registers::ColourTarget;
        /// Layout, the level's extent, its origin, and texels with addrlib's byte for each.
        type Level = (SurfaceLayout, u32, u32, (u32, u32), [(u32, u32, usize); 2]);
        let cases: [Level; 2] = [
            (Rx64Kb, 16, 32, (32, 0), [(0, 0, 2048), (15, 31, 6908)]),
            (Dx4Kb, 16, 8, (16, 0), [(0, 0, 2048), (15, 7, 2812)]),
        ];
        for (layout, width, height, origin, texels) in cases {
            let target = ColourTarget {
                base: 0,
                width,
                height,
                pipe_bank_xor: 0,
                layout,
                place: crate::registers::Place::Within {
                    span: layout.block_texels(),
                    origin,
                },
            };
            let linear: Vec<u32> = (1..=width * height).collect();
            let mut tiled = vec![0xEEEE_EEEE; target.words()];
            target
                .tile_mapped(&linear, &mut tiled, |w| w)
                .expect("tiles");
            for (x, y, byte) in texels {
                assert_eq!(tiled[byte / 4], y * width + x + 1, "{layout:?} ({x},{y})");
            }
            let written = tiled.iter().filter(|&&w| w != 0xEEEE_EEEE).count();
            assert_eq!(written, linear.len(), "only the level's texels");
            assert_eq!(target.detile_mapped(&tiled, |w| w), linear);
        }
    }

    /// At odd sizes a chain's levels are laid out with their sides rounded up, as addrlib's
    /// `GetMipSize` does: a 129-wide level 1 is 65 wide, too wide for the tail, so the tail starts
    /// at level 2 - rounding down would have put it at level 1 and every level above it in the
    /// wrong place. Offsets, tails and sizes are addrlib's for each.
    #[test]
    fn odd_sized_chains_lie_where_addrlib_puts_them() {
        use super::SurfaceLayout::{self, Dx4Kb, Rx64Kb};
        /// Layout, level 0's extent, levels, the chain's bytes, and each level's offset out of
        /// the tail.
        type Chain = (SurfaceLayout, u32, u32, u32, u64, &'static [u64]);
        let cases: [Chain; 5] = [
            (Rx64Kb, 129, 129, 8, 393_216, &[131_072, 65536]),
            (Rx64Kb, 257, 3, 9, 458_752, &[262_144, 131_072, 65536]),
            (Rx64Kb, 65, 513, 10, 720_896, &[393_216, 196_608, 65536]),
            (Dx4Kb, 33, 33, 6, 24576, &[8192, 4096]),
            (Dx4Kb, 129, 17, 8, 49152, &[28672, 16384, 8192, 4096]),
        ];
        for (layout, width, height, levels, bytes, offsets) in cases {
            let shape = format!("{layout:?} {width}x{height} x{levels}");
            assert_eq!(layout.chain_bytes(width, height, levels), bytes, "{shape}");
            for level in 0..levels {
                let want = offsets.get(level as usize).copied();
                assert_eq!(
                    layout.level_offset(width, height, levels, level),
                    want,
                    "{shape} {level}"
                );
            }
        }
        assert_eq!(crate::dcc::chain_meta_bytes(129, 129, 8, true), 12288);
        assert_eq!(crate::dcc::chain_meta_bytes(65, 513, 10, true), 20480);
    }

    /// A linear chain stores its last level first, each at its own 64-texel pitch and rounded-up
    /// height: addrlib's offsets for 100 x 30 and 65 x 7.
    #[test]
    fn a_linear_chain_lies_where_addrlib_puts_it() {
        use super::linear_level;
        assert_eq!(linear_level(100, 30, 3, 0), Some((5888, 128)));
        assert_eq!(linear_level(100, 30, 3, 1), Some((2048, 64)));
        assert_eq!(linear_level(100, 30, 3, 2), Some((0, 64)));
        assert_eq!(linear_level(65, 7, 4, 0), Some((1792, 128)));
        assert_eq!(linear_level(65, 7, 4, 2), Some((256, 64)));
        assert_eq!(linear_level(65, 7, 4, 4), None);
        assert_eq!(linear_level(100, 30, 1, 0), Some((0, 128)));
        // One byte a texel: addrlib's offsets and pitches for a ten-level 512 x 512 chain and a
        // five-level 300 x 200 one.
        assert_eq!(
            super::linear_level_of(512, 512, (10, 0), 1),
            Some((130_816, 512))
        );
        assert_eq!(
            super::linear_level_of(512, 512, (10, 2), 1),
            Some((32512, 256))
        );
        assert_eq!(
            super::linear_level_of(512, 512, (10, 8), 1),
            Some((256, 256))
        );
        assert_eq!(super::linear_level_of(512, 512, (10, 9), 1), Some((0, 256)));
        assert_eq!(
            super::linear_level_of(300, 200, (5, 0), 1),
            Some((48128, 512))
        );
        assert_eq!(
            super::linear_level_of(300, 200, (5, 3), 1),
            Some((3328, 256))
        );
    }

    /// A level stored padded reads at its chain's stride: level 1 of a 129 x 17 `4KB_D_X` chain is
    /// 64 texels wide but laid out 65 wide, so 96 to a row, and level 1 of a 100 x 30 linear chain
    /// sits at 2048 with a 64-texel pitch. addrlib's addresses for each, through the target the
    /// chain gives the level.
    #[test]
    fn a_padded_level_reads_at_its_chains_stride() {
        use super::SurfaceLayout::{self, Dx4Kb, Linear};
        use crate::registers::{Place, chain_level};
        /// Layout, level 0's extent, levels, and texels of level 1 with addrlib's byte for each.
        type Case = (SurfaceLayout, (u32, u32), u32, &'static [(u32, u32, u64)]);
        let cases: [Case; 2] = [
            (
                Dx4Kb,
                (129, 17),
                8,
                &[
                    (0, 0, 16384),
                    (63, 7, 24316),
                    (33, 1, 21524),
                    (40, 5, 22096),
                ],
            ),
            (
                Linear,
                (100, 30),
                3,
                &[(0, 0, 2048), (49, 14, 5828), (7, 3, 2844)],
            ),
        ];
        for (layout, extent, levels, texels) in cases {
            let target = chain_level((0, 0), extent, (levels, 1), layout).expect("placed");
            assert_eq!(
                (target.width, target.height),
                (extent.0 >> 1, extent.1 >> 1),
                "{layout:?}"
            );
            let (width, height) = (target.width, target.height);
            let linear: Vec<u32> = (1..=width * height).collect();
            let mut words = vec![0u32; target.words()];
            target
                .tile_mapped(&linear, &mut words, |w| w)
                .expect("tiles");
            for &(x, y, byte) in texels {
                let at = usize::try_from(byte - target.base).expect("in range") / 4;
                assert_eq!(words[at], y * width + x + 1, "{layout:?} ({x},{y})");
            }
            assert_eq!(target.detile_mapped(&words, |w| w), linear, "{layout:?}");
        }
        let dx = chain_level((0, 0), (129, 17), (8, 1), Dx4Kb).expect("placed");
        assert_eq!(
            dx.place,
            Place::Within {
                span: (65, 9),
                origin: (0, 0)
            }
        );
    }

    /// The 8-bpp `64KB_R_X` layout gives addrlib's address for each texel sampled, block corners
    /// and texels across blocks of a 512 x 512 surface among them.
    #[test]
    fn an_8_bpp_rx_surface_lies_where_addrlib_puts_it() {
        use super::{surface_bytes_64kb_rx_bpp1, tiled_byte_offset_64kb_rx_bpp1_surface as at};
        for (x, y, want) in [
            (0, 0, 0),
            (255, 255, 61695),
            (256, 0, 65536),
            (0, 256, 131_072),
            (511, 511, 258_303),
            (37, 412, 150_437),
            (300, 77, 69748),
            (129, 3, 32793),
        ] {
            assert_eq!(at(x, y, 512), want, "({x},{y})");
        }
        assert_eq!(surface_bytes_64kb_rx_bpp1(512, 512), 262_144);
        assert_eq!(surface_bytes_64kb_rx_bpp1(300, 200), 131_072);
    }

    /// An 8-bpp `64KB_R_X` colour target lies byte for byte where the 8-bpp equation puts it,
    /// pipe-bank XOR applied, and tiling it back leaves every other byte of its words alone.
    #[test]
    fn an_8_bpp_rx_layout_tiles_each_texel_into_its_byte() {
        use super::{SurfaceLayout, tiled_byte_offset_64kb_rx_bpp1_xor as at};
        let layout = SurfaceLayout::Rx64KbBpp1;
        let (width, height, xor) = (300, 200, 0x3);
        assert_eq!(layout.words(width, height), 2 * 65536 / 4);
        assert_eq!(layout.chain_bytes(width, height, 1), 2 * 65536);
        assert_eq!(layout.block_texels(), (256, 256));
        let mut tiled = vec![0xA5A5_A5A5u32; layout.words(width, height)];
        let linear: Vec<u32> = (0..width * height).map(|i| i % 251).collect();
        layout
            .tile_mapped(&linear, width, height, xor, &mut tiled, |w| w)
            .expect("tiles");
        let bytes: Vec<u8> = tiled.iter().flat_map(|w| w.to_le_bytes()).collect();
        for (x, y) in [(0, 0), (255, 199), (256, 0), (299, 199), (37, 150)] {
            assert_eq!(
                u32::from(bytes[at(x, y, width, xor)]),
                (y * width + x) % 251,
                "({x},{y})"
            );
        }
        let untouched: usize = bytes.iter().map(|&b| usize::from(b == 0xA5)).sum();
        assert!(untouched >= 2 * 65536 - (width * height) as usize);
        assert_eq!(
            layout.detile_mapped(&tiled, width, height, xor, |w| w),
            linear,
            "detiling is tiling's inverse"
        );
        assert_eq!(SurfaceLayout::Rx64Kb.at_bytes_per_texel(1), Some(layout));
        assert_eq!(SurfaceLayout::Dx4Kb.at_bytes_per_texel(1), None);
    }

    /// An 8-bpp `64KB_R_X` chain lies where addrlib puts it - SuperTuxKart's 512 x 512 glyph page
    /// and its nine levels below: two levels of whole blocks above a tail from level 2, each tail
    /// level at its origin in 16 x 16-texel micro blocks.
    #[test]
    fn an_8_bpp_rx_chain_lies_where_addrlib_puts_it() {
        use super::SurfaceLayout;
        let layout = SurfaceLayout::Rx64KbBpp1;
        let (width, height, levels) = (512, 512, 10);
        assert_eq!(layout.first_level_in_tail(width, height, levels), 2);
        assert_eq!(layout.chain_bytes(width, height, levels), 393_216);
        assert_eq!(layout.level_offset(width, height, levels, 0), Some(131_072));
        assert_eq!(layout.level_offset(width, height, levels, 1), Some(65536));
        assert_eq!(layout.level_offset(width, height, levels, 2), None);
        for (level, origin) in [
            (2, (128, 0)),
            (3, (0, 128)),
            (4, (64, 0)),
            (5, (0, 64)),
            (6, (32, 0)),
            (7, (16, 32)),
            (8, (0, 48)),
            (9, (0, 32)),
        ] {
            assert_eq!(
                layout.tail_origin(width, height, levels, level),
                Some(origin),
                "level {level}"
            );
        }
    }

    /// A one-byte linear chain's level lies where addrlib's linear layout puts it, its rows at a
    /// 256-texel pitch, and tiling it back changes only its own bytes.
    #[test]
    fn a_one_byte_linear_level_lies_at_its_pitch() {
        use super::SurfaceLayout;
        let layout = SurfaceLayout::LinearBpp1;
        assert_eq!(SurfaceLayout::Linear.at_bytes_per_texel(1), Some(layout));
        assert_eq!(layout.level_offset(512, 512, 10, 1), Some(65280));
        assert_eq!(layout.chain_bytes(512, 512, 10), 392_960);
        let (width, height) = (300, 3);
        assert_eq!(layout.words(width, height), (512 * 2 + 300) / 4);
        let mut tiled = vec![0xA5A5_A5A5u32; layout.words(width, height)];
        let linear: Vec<u32> = (0..width * height).map(|i| i % 241).collect();
        layout
            .tile_mapped(&linear, width, height, 0, &mut tiled, |w| w)
            .expect("tiles");
        let bytes: Vec<u8> = tiled.iter().flat_map(|w| w.to_le_bytes()).collect();
        assert_eq!(
            u32::from(bytes[512 + 7]),
            (300 + 7) % 241,
            "row 1 at the pitch"
        );
        assert_eq!(bytes[300], 0xA5, "the pitch's padding is kept");
        assert_eq!(
            layout.detile_mapped(&tiled, width, height, 0, |w| w),
            linear
        );
    }
}
