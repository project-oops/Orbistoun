//! Detiling a guest's surfaces - turning a tiled render target or texture back into a linear image.
//!
//! Colour targets and textures are tiled: texel `(x, y)` sits at a swizzled offset the hardware
//! computes, not at `(y * width + x) * bpp`. A wrong swizzle passes every test built from linear
//! data and corrupts every tiled surface, so the swizzle is anchored to hardware (D010). One texel
//! was read back from a point draw into a 32-bpp `64KB_R_X` target: texel `(15, 15)` at tiled byte
//! 4348. A display-size readback (1920x1080, every pixel against a linear control) confirms the
//! in-block swizzle with row-major 64 KiB blocks. [`SurfaceLayout::of`] names every mode and
//! element size modelled - `64KB_R_X`, `4KB_D_X`, `64KB_D_X`, `64KB_S`, `4KB_S` and `LINEAR` at
//! the sizes it lists - and the run path detiles through it; any other mode is refused by name.
//! Block-compressed data needs no decode, since Vulkan consumes it natively.

use crate::registers::SwizzleMode;

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

/// A 64-bpp `64KB_R_X` block's extent in texels: 64 KiB of eight-byte texels, 128 across and 64
/// down.
const RX_BPP8_BLOCK_WIDTH: u32 = 128;
const RX_BPP8_BLOCK_HEIGHT: u32 = 64;

/// Where each of the low seven bits of `x`, then six of `y`, lands in a 64-bpp `64KB_R_X` block's
/// byte offset: the offset is the XOR of the entries whose coordinate bit is set.
///
/// Printed by `tools/addrlib/equation 64KB_R_X 8 32` for this console's configuration, which checks
/// row-major blocks under these terms and [`RX_BPP8_Y_ABOVE`] against addrlib for every texel of a
/// 32 x 32-block surface and a 5 x 3-block one.
const RX_BPP8_X_BITS: [usize; 7] = [0x8, 0x20, 0x40, 0x2100, 0x200, 0x800, 0x8400];
/// The same for `y`.
const RX_BPP8_Y_BITS: [usize; 6] = [0x10, 0x80, 0x1000, 0x100, 0x4200, 0x400];
/// What `y` bit 6, the first above the block, adds inside it: the one term above the block at this
/// size, where at one and four bytes a texel there are none.
const RX_BPP8_Y_ABOVE: usize = 0x800;

/// Byte offset of texel `(x, y)` in a 64-bpp `64KB_R_X` surface `width` texels wide: its block,
/// row-major, then its place in the block, before any pipe-bank XOR.
#[must_use]
pub fn tiled_byte_offset_64kb_rx_bpp8_surface(x: u32, y: u32, width: u32) -> usize {
    let blocks_per_row = width.div_ceil(RX_BPP8_BLOCK_WIDTH) as usize;
    let block =
        (y / RX_BPP8_BLOCK_HEIGHT) as usize * blocks_per_row + (x / RX_BPP8_BLOCK_WIDTH) as usize;
    let mut offset = 0;
    for (bit, place) in RX_BPP8_X_BITS.iter().enumerate() {
        if x >> bit & 1 == 1 {
            offset ^= place;
        }
    }
    for (bit, place) in RX_BPP8_Y_BITS.iter().enumerate() {
        if y >> bit & 1 == 1 {
            offset ^= place;
        }
    }
    if y >> RX_BPP8_Y_BITS.len() & 1 == 1 {
        offset ^= RX_BPP8_Y_ABOVE;
    }
    block * BLOCK_BYTES + offset
}

/// Texels per side of a 128-bpp `64KB_R_X` block: 64 KiB of sixteen-byte texels, 64 x 64.
const RX_BPP16_BLOCK_EXTENT: u32 = 64;

/// Where each of the low six bits of `x`, then of `y`, lands in a 128-bpp `64KB_R_X` block's byte
/// offset: the offset is the XOR of the entries whose coordinate bit is set.
///
/// Printed by `tools/addrlib/equation 64KB_R_X 16 32` for this console's configuration, which checks
/// row-major blocks under these terms and the two above the block ([`RX_BPP16_X_ABOVE`],
/// [`RX_BPP16_Y_ABOVE`]) against addrlib for every texel of a 32 x 32-block surface and a 5 x 3-block
/// one.
const RX_BPP16_X_BITS: [usize; 6] = [0x10, 0x40, 0x2000, 0x100, 0x8200, 0x800];
/// The same for `y`.
const RX_BPP16_Y_BITS: [usize; 6] = [0x20, 0x80, 0x1000, 0x4100, 0x200, 0x400];
/// What `x` bit 6, the first above the block, adds inside it.
const RX_BPP16_X_ABOVE: usize = 0x400;
/// What `y` bit 6, the first above the block, adds inside it.
const RX_BPP16_Y_ABOVE: usize = 0x800;

/// Byte offset of texel `(x, y)` in a 128-bpp `64KB_R_X` surface `width` texels wide: its block,
/// row-major, then its place in the block, before any pipe-bank XOR.
#[must_use]
pub fn tiled_byte_offset_64kb_rx_bpp16_surface(x: u32, y: u32, width: u32) -> usize {
    let blocks_per_row = width.div_ceil(RX_BPP16_BLOCK_EXTENT) as usize;
    let block = (y / RX_BPP16_BLOCK_EXTENT) as usize * blocks_per_row
        + (x / RX_BPP16_BLOCK_EXTENT) as usize;
    let mut offset = 0;
    for (bit, place) in RX_BPP16_X_BITS.iter().enumerate() {
        if x >> bit & 1 == 1 {
            offset ^= place;
        }
    }
    for (bit, place) in RX_BPP16_Y_BITS.iter().enumerate() {
        if y >> bit & 1 == 1 {
            offset ^= place;
        }
    }
    if x >> RX_BPP16_X_BITS.len() & 1 == 1 {
        offset ^= RX_BPP16_X_ABOVE;
    }
    if y >> RX_BPP16_Y_BITS.len() & 1 == 1 {
        offset ^= RX_BPP16_Y_ABOVE;
    }
    block * BLOCK_BYTES + offset
}

/// A 16-bpp `64KB_R_X` block's extent in texels: 64 KiB of two-byte texels, 256 across and 128
/// down.
const RX_BPP2_BLOCK_WIDTH: u32 = 256;
const RX_BPP2_BLOCK_HEIGHT: u32 = 128;

/// Where each of the low eight bits of `x`, then seven of `y`, lands in a 16-bpp `64KB_R_X` block's
/// byte offset: the offset is the XOR of the entries whose coordinate bit is set.
///
/// Printed by `tools/addrlib/equation 64KB_R_X 2 32` for this console's configuration, which finds
/// no term above the block and checks row-major blocks under these terms against addrlib for every
/// texel of a 32 x 32-block surface and a 5 x 3-block one.
const RX_BPP2_X_BITS: [usize; 8] = [0x2, 0x4, 0x8, 0x180, 0x200, 0x800, 0x2400, 0x8000];
/// The same for `y`.
const RX_BPP2_Y_BITS: [usize; 7] = [0x10, 0x20, 0x40, 0x100, 0x1200, 0x400, 0x4800];

/// Byte offset of texel `(x, y)` in a 16-bpp `64KB_R_X` surface `width` texels wide: its block,
/// row-major, then its place in the block, before any pipe-bank XOR.
#[must_use]
pub fn tiled_byte_offset_64kb_rx_bpp2_surface(x: u32, y: u32, width: u32) -> usize {
    let blocks_per_row = width.div_ceil(RX_BPP2_BLOCK_WIDTH) as usize;
    let block =
        (y / RX_BPP2_BLOCK_HEIGHT) as usize * blocks_per_row + (x / RX_BPP2_BLOCK_WIDTH) as usize;
    let mut offset = 0;
    for (bit, place) in RX_BPP2_X_BITS.iter().enumerate() {
        if x >> bit & 1 == 1 {
            offset ^= place;
        }
    }
    for (bit, place) in RX_BPP2_Y_BITS.iter().enumerate() {
        if y >> bit & 1 == 1 {
            offset ^= place;
        }
    }
    block * BLOCK_BYTES + offset
}

/// Texels per side of a 128-bpp `64KB_D_X` block: 64 KiB of sixteen-byte texels, 64 x 64.
const DX_BPP16_BLOCK_EXTENT: u32 = 64;

/// Where each of the low six bits of `x`, then of `y`, lands in a 128-bpp `64KB_D_X` block's byte
/// offset: the offset is the XOR of the entries whose coordinate bit is set.
///
/// From addrlib for this console's configuration, as [`RX_BPP1_X_BITS`], read off
/// `Addr2ComputeSurfaceAddrFromCoord` one coordinate bit at a time. Row-major blocks with this
/// equation inside give addrlib's address for all 16,384 texels of a 128 x 128 surface.
const DX_BPP16_X_BITS: [usize; 6] = [0x10, 0x40, 0x200, 0x800, 0x2400, 0x8100];
/// The same for `y`.
const DX_BPP16_Y_BITS: [usize; 6] = [0x20, 0x80, 0x100, 0x400, 0x1800, 0x4200];

/// Byte offset of texel `(x, y)` in a 128-bpp `64KB_D_X` surface `width` texels wide: its block,
/// row-major, then its place in the block, with the surface's pipe-bank XOR on the 256-byte runs
/// (`blkOffset ^ (pipeBankXor << 8)`, `gfx10addrlib.cpp:4786-4849`) as for the other `_X` modes.
#[must_use]
pub fn tiled_byte_offset_64kb_dx_bpp16_xor(x: u32, y: u32, width: u32, pipe_bank_xor: u8) -> usize {
    let blocks_per_row = width.div_ceil(DX_BPP16_BLOCK_EXTENT) as usize;
    let block = (y / DX_BPP16_BLOCK_EXTENT) as usize * blocks_per_row
        + (x / DX_BPP16_BLOCK_EXTENT) as usize;
    let mut offset = 0;
    for (bit, place) in DX_BPP16_X_BITS.iter().enumerate() {
        if x >> bit & 1 == 1 {
            offset ^= place;
        }
    }
    for (bit, place) in DX_BPP16_Y_BITS.iter().enumerate() {
        if y >> bit & 1 == 1 {
            offset ^= place;
        }
    }
    block * BLOCK_BYTES + (offset ^ (usize::from(pipe_bank_xor) << 8))
}

/// Where each of the low six bits of `x`, then of `y`, lands in a 128-bpp `64KB_S` block's byte
/// offset - the standard swizzle, with no pipe XOR: a block-compressed image's elements, sixteen
/// bytes each.
///
/// From addrlib for this console's configuration, read off `Addr2ComputeSurfaceAddrFromCoord` as
/// [`DX_BPP16_X_BITS`] was, and the whole equation - row-major 64 x 64-element blocks across the
/// pitch, the XOR of these inside - for every element of a 256 x 256 surface and of a 1019 x 187
/// one (pitch 1024).
const S_BPP16_X_BITS: [usize; 6] = [0x40, 0x80, 0x200, 0x800, 0x2000, 0x8000];
/// The same for `y`.
const S_BPP16_Y_BITS: [usize; 6] = [0x10, 0x20, 0x100, 0x400, 0x1000, 0x4000];

/// Byte offset of element `(x, y)` in a 128-bpp `64KB_S` surface `width` elements wide: its
/// 64 x 64 block, row-major across the pitch (the width rounded up to whole blocks), then the XOR of
/// its coordinate bits' places.
#[must_use]
pub fn tiled_byte_offset_64kb_s_bpp16(x: u32, y: u32, width: u32) -> usize {
    let blocks_per_row = width.div_ceil(DX_BPP16_BLOCK_EXTENT) as usize;
    let block = (y / DX_BPP16_BLOCK_EXTENT) as usize * blocks_per_row
        + (x / DX_BPP16_BLOCK_EXTENT) as usize;
    let mut offset = 0;
    for (bit, place) in S_BPP16_X_BITS.iter().enumerate() {
        if x >> bit & 1 == 1 {
            offset ^= place;
        }
    }
    for (bit, place) in S_BPP16_Y_BITS.iter().enumerate() {
        if y >> bit & 1 == 1 {
            offset ^= place;
        }
    }
    block * BLOCK_BYTES + offset
}

/// Bytes a whole 128-bpp `64KB_S` surface `width` x `height` elements occupies: every block it
/// touches, whole.
#[must_use]
pub fn surface_bytes_64kb_s_bpp16(width: u32, height: u32) -> usize {
    width.div_ceil(DX_BPP16_BLOCK_EXTENT) as usize
        * height.div_ceil(DX_BPP16_BLOCK_EXTENT) as usize
        * BLOCK_BYTES
}

/// Byte offset of element `(x, y)` in a 128-bpp `4KB_S` surface `width` elements wide: its
/// 16 x 16 block, row-major across the pitch, then the low twelve bits of the `64KB_S` equation,
/// which is all a 4 KiB block holds. Read off `Addr2ComputeSurfaceAddrFromCoord` for this console's
/// configuration (`SW_4KB_S`, 128 bpp, a 512 x 256 surface): every x and y bit below 16, a second
/// block across and down, and (300, 200).
#[must_use]
pub fn tiled_byte_offset_4kb_s_bpp16(x: u32, y: u32, width: u32) -> usize {
    let blocks_per_row = width.div_ceil(S4_BPP16_BLOCK_EXTENT) as usize;
    let block = (y / S4_BPP16_BLOCK_EXTENT) as usize * blocks_per_row
        + (x / S4_BPP16_BLOCK_EXTENT) as usize;
    let mut offset = 0;
    for (bit, place) in S_BPP16_X_BITS.iter().take(4).enumerate() {
        if x >> bit & 1 == 1 {
            offset ^= place;
        }
    }
    for (bit, place) in S_BPP16_Y_BITS.iter().take(4).enumerate() {
        if y >> bit & 1 == 1 {
            offset ^= place;
        }
    }
    block * S4_BLOCK_BYTES + offset
}

/// Byte offset of element `(x, y)` in a 64-bpp `4KB_S` surface `width` elements wide - a `BC1`
/// image's blocks: its 32 x 16-element block, row-major across the pitch, then the element's bits
/// as addrlib's pattern for this mode and size places them (`gfx10SwizzlePattern.h`,
/// `GFX10_SW_4K_S_PATINFO` at 8 bpe: `GFX10_SW_PATTERN_NIBBLE01` row 3 then `NIBBLE2` row 4, `X0 Y0
/// Y1 X1 X2 Y2 X3 Y3 X4` from byte bit 3). The same tables give the 32-bpp form's bits exactly.
#[must_use]
pub fn tiled_byte_offset_4kb_s_bpp8(x: u32, y: u32, width: u32) -> usize {
    const X_BITS: [usize; 5] = [0x8, 0x40, 0x80, 0x200, 0x800];
    const Y_BITS: [usize; 4] = [0x10, 0x20, 0x100, 0x400];
    let blocks_per_row = width.div_ceil(S4_BPP8_BLOCK_WIDTH) as usize;
    let block =
        (y / S4_BPP8_BLOCK_HEIGHT) as usize * blocks_per_row + (x / S4_BPP8_BLOCK_WIDTH) as usize;
    let mut offset = 0;
    for (bit, place) in X_BITS.iter().enumerate() {
        if x >> bit & 1 == 1 {
            offset ^= place;
        }
    }
    for (bit, place) in Y_BITS.iter().enumerate() {
        if y >> bit & 1 == 1 {
            offset ^= place;
        }
    }
    block * S4_BLOCK_BYTES + offset
}

/// Tiles an 8-bpp `4KB_S` surface's linear texels, one byte each, into `tiled`, each through `map`:
/// [`SurfaceLayout::tile_mapped`] for [`SurfaceLayout::S4KbBpp1`].
fn tile_4kb_s_bpp1(
    (linear, tiled): (&[u32], &mut [u32]),
    (width, height): (u32, u32),
    map: impl Fn(u32) -> u32,
) -> Result<(), DetileError> {
    let needed_words = SurfaceLayout::S4KbBpp1.words(width, height);
    let texels = width as usize * height as usize;
    if tiled.len() < needed_words || linear.len() != texels {
        return Err(DetileError::TiledDataTooShort {
            needed_words: needed_words.max(texels),
            got_words: tiled.len().min(linear.len()),
        });
    }
    for (index, &texel) in linear.iter().enumerate() {
        let (x, y) = (index as u32 % width, index as u32 / width);
        let at = tiled_byte_offset_4kb_s_bpp1(x, y, width);
        let shift = (at % 4) * 8;
        let word = &mut tiled[at / 4];
        *word = (*word & !(0xFF << shift)) | ((map(texel) & 0xFF) << shift);
    }
    Ok(())
}

/// Tiles a 16-bpp `64KB_R_X` surface's linear texels, the low half of a word each, into their
/// halves of `tiled`'s little-endian words, each through `map`, leaving the other halves alone:
/// [`SurfaceLayout::tile_mapped`] for [`SurfaceLayout::Rx64KbBpp2`].
fn tile_64kb_rx_bpp2(
    (linear, tiled): (&[u32], &mut [u32]),
    (width, height): (u32, u32),
    map: impl Fn(u32) -> u32,
) -> Result<(), DetileError> {
    let needed_words = SurfaceLayout::Rx64KbBpp2.words(width, height);
    let texels = width as usize * height as usize;
    if tiled.len() < needed_words || linear.len() != texels {
        return Err(DetileError::TiledDataTooShort {
            needed_words: needed_words.max(texels),
            got_words: tiled.len().min(linear.len()),
        });
    }
    for (index, &texel) in linear.iter().enumerate() {
        let (x, y) = (index as u32 % width, index as u32 / width);
        let at = tiled_byte_offset_64kb_rx_bpp2_surface(x, y, width);
        let shift = (at % 4) * 8;
        let word = &mut tiled[at / 4];
        *word = (*word & !(0xFFFF << shift)) | ((map(texel) & 0xFFFF) << shift);
    }
    Ok(())
}

/// Tiles an eight-byte layout's linear elements, two words each, into `tiled`, each word through
/// `map`: [`SurfaceLayout::tile_mapped`] for [`SurfaceLayout::S4KbBpp8`] and
/// [`SurfaceLayout::Rx64KbBpp8`].
fn tile_bpp8(
    layout: SurfaceLayout,
    (linear, tiled): (&[u32], &mut [u32]),
    (width, height): (u32, u32),
    map: impl Fn(u32) -> u32,
) -> Result<(), DetileError> {
    let needed_words = layout.words(width, height);
    let words = width as usize * height as usize * 2;
    if tiled.len() < needed_words || linear.len() != words {
        return Err(DetileError::TiledDataTooShort {
            needed_words: needed_words.max(words),
            got_words: tiled.len().min(linear.len()),
        });
    }
    for (index, element) in linear.chunks_exact(2).enumerate() {
        let (x, y) = (index as u32 % width, index as u32 / width);
        let at = layout.eight_byte_offset(x, y, width) / 4;
        for (slot, &word) in tiled[at..at + 2].iter_mut().zip(element) {
            *slot = map(word);
        }
    }
    Ok(())
}

/// Byte offset of texel `(x, y)` in an 8-bpp `4KB_S` surface `width` texels wide: its 64 x 64
/// block, row-major across the pitch, then the texel's bits as addrlib's pattern places them
/// (`gfx10SwizzlePattern.h`, `GFX10_SW_4K_S_PATINFO` at 1 bpe: `GFX10_SW_PATTERN_NIBBLE01` row 0
/// then `NIBBLE2` row 1, `X0 X1 X2 X3 Y0 Y1 Y2 Y3 Y4 X4 Y5 X5` from byte bit 0).
#[must_use]
pub fn tiled_byte_offset_4kb_s_bpp1(x: u32, y: u32, width: u32) -> usize {
    const X_BITS: [usize; 6] = [0x1, 0x2, 0x4, 0x8, 0x200, 0x800];
    const Y_BITS: [usize; 6] = [0x10, 0x20, 0x40, 0x80, 0x100, 0x400];
    let blocks_per_row = width.div_ceil(S4_BPP1_BLOCK_EXTENT) as usize;
    let block =
        (y / S4_BPP1_BLOCK_EXTENT) as usize * blocks_per_row + (x / S4_BPP1_BLOCK_EXTENT) as usize;
    let mut offset = 0;
    for (bit, place) in X_BITS.iter().enumerate() {
        if x >> bit & 1 == 1 {
            offset ^= place;
        }
    }
    for (bit, place) in Y_BITS.iter().enumerate() {
        if y >> bit & 1 == 1 {
            offset ^= place;
        }
    }
    block * S4_BLOCK_BYTES + offset
}

/// An 8-bpp `4KB_S` block's extent in texels, across and down.
const S4_BPP1_BLOCK_EXTENT: u32 = 64;

/// A 64-bpp `4KB_S` block's extent in elements: five x bits and four y bits of its twelve.
const S4_BPP8_BLOCK_WIDTH: u32 = 32;
const S4_BPP8_BLOCK_HEIGHT: u32 = 16;

/// Byte offset of texel `(x, y)` in a 32-bpp `4KB_S` surface `width` texels wide: its 32 x 32
/// block, row-major across the pitch, then the texel's bits interleaved within it. Read off
/// `Addr2ComputeSurfaceAddrFromCoord` for this console's configuration (`SW_4KB_S`, 32 bpp): every
/// x and y bit below 32, the next block across and down, and (13, 27), (45, 50), (99, 69) in a
/// 100 x 70 surface.
#[must_use]
pub fn tiled_byte_offset_4kb_s_bpp4(x: u32, y: u32, width: u32) -> usize {
    const X_BITS: [usize; 5] = [0x4, 0x8, 0x80, 0x200, 0x800];
    const Y_BITS: [usize; 5] = [0x10, 0x20, 0x40, 0x100, 0x400];
    let blocks_per_row = width.div_ceil(S4_BPP4_BLOCK_EXTENT) as usize;
    let block =
        (y / S4_BPP4_BLOCK_EXTENT) as usize * blocks_per_row + (x / S4_BPP4_BLOCK_EXTENT) as usize;
    let mut offset = 0;
    for (bit, place) in X_BITS.iter().enumerate() {
        if x >> bit & 1 == 1 {
            offset |= place;
        }
    }
    for (bit, place) in Y_BITS.iter().enumerate() {
        if y >> bit & 1 == 1 {
            offset |= place;
        }
    }
    block * S4_BLOCK_BYTES + offset
}

/// Byte offset of texel `(x, y)` in a 32-bpp `64KB_S` surface `width` texels wide: its 128 x 128
/// block, row-major across the pitch, then the texel's bits interleaved within it. addrlib's
/// pattern for `SW_64K_S` at four bytes an element is the same for every pipe and packer count
/// (`gfx10SwizzlePattern.h`, `GFX10_SW_64K_S_RBPLUS_PATINFO` rows `{1, 2, 3, 3, 0}`): nibbles 0-1
/// row 2 put x0, x1, y0, y1, y2, x2 in bits 2-7, nibble 2 row 3 y3, x3, y4, x4 in bits 8-11, and
/// nibble 3 row 3 y5, x5, y6, x6 in bits 12-15 - the `4KB_S` equation's twelve bits, continued.
/// The same rows at sixteen bytes (`{1, 4, 5, 5, 0}`) give [`tiled_byte_offset_64kb_s_bpp16`]'s places.
#[must_use]
pub fn tiled_byte_offset_64kb_s_bpp4(x: u32, y: u32, width: u32) -> usize {
    const X_BITS: [usize; 7] = [0x4, 0x8, 0x80, 0x200, 0x800, 0x2000, 0x8000];
    const Y_BITS: [usize; 7] = [0x10, 0x20, 0x40, 0x100, 0x400, 0x1000, 0x4000];
    let blocks_per_row = width.div_ceil(S64_BPP4_BLOCK_EXTENT) as usize;
    let block = (y / S64_BPP4_BLOCK_EXTENT) as usize * blocks_per_row
        + (x / S64_BPP4_BLOCK_EXTENT) as usize;
    let mut offset = 0;
    for (bit, place) in X_BITS.iter().enumerate() {
        if x >> bit & 1 == 1 {
            offset |= place;
        }
    }
    for (bit, place) in Y_BITS.iter().enumerate() {
        if y >> bit & 1 == 1 {
            offset |= place;
        }
    }
    block * BLOCK_BYTES + offset
}

/// A 32-bpp `64KB_S` block's side in texels: 16384 texels of four bytes.
const S64_BPP4_BLOCK_EXTENT: u32 = 128;

/// A 32-bpp `4KB_S` block's side in texels: 1024 texels of four bytes.
const S4_BPP4_BLOCK_EXTENT: u32 = 32;

/// A 128-bpp `4KB_S` block's side in elements: 256 elements of sixteen bytes.
const S4_BPP16_BLOCK_EXTENT: u32 = 16;
/// A `4KB_S` block's bytes.
const S4_BLOCK_BYTES: usize = 4096;

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

/// What each bit of a slice index adds to a 32-bpp `64KB_R_X` volume's offset inside its block: the
/// offset is the XOR of the entries whose bit is set. Such a volume's blocks are thin, 128 x 128 x
/// 1, its in-block terms for `x` and `y` a 2D surface's, and its slices follow one another a whole
/// slice of blocks apart (D823). Read off addrlib for this console's configuration by
/// `tools/addrlib/equation 64KB_R_X 4 32 3d`, which checks every texel of three volumes - one of
/// them a quarter of a block across - against that rule.
const RX_BPP4_SLICE_BITS: [usize; 5] = [0x800, 0x400, 0x200, 0x100, 0x0];

/// The XOR slice `z` of a 32-bpp `64KB_R_X` volume puts on its in-block byte offsets; `None` past
/// the slices whose terms were read.
#[must_use]
pub fn slice_xor_64kb_rx_bpp4(z: u32) -> Option<usize> {
    if z >> RX_BPP4_SLICE_BITS.len() != 0 {
        return None;
    }
    Some(
        RX_BPP4_SLICE_BITS
            .iter()
            .enumerate()
            .filter(|&(bit, _)| (z >> bit) & 1 == 1)
            .fold(0, |offset, (_, term)| offset ^ term),
    )
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
                        // Past the end only where the words are a surface's first words alone.
                        if let Some(word) =
                            blocks.get_mut((x / extent) * BLOCK_WORDS + (column[x % extent] ^ r))
                        {
                            *word = map(texel);
                        }
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

/// [`tile_surface_64kb_rx_bpp4_mapped`] into only the surface's first `tiled.len()` words: each texel
/// whose word lies past them is not written. What a copy of a surface's first bytes holds of the
/// frame, tiled where the copy lands rather than into a whole surface and cut short.
///
/// # Errors
///
/// [`DetileError::TiledDataTooShort`] when `linear` is not exactly `width * height` texels.
pub fn tile_prefix_64kb_rx_bpp4_mapped(
    linear: &[u32],
    width: u32,
    height: u32,
    pipe_bank_xor: u8,
    tiled: &mut [u32],
    map: impl Fn(u32) -> u32 + Sync,
) -> Result<(), DetileError> {
    let texels = width as usize * height as usize;
    if linear.len() != texels {
        return Err(DetileError::TiledDataTooShort {
            needed_words: texels,
            got_words: linear.len(),
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

/// Texels per side of a 128-bpp `4KB_D_X` block: 4 KiB of sixteen-byte texels, 16 x 16.
const DX4_BPP16_BLOCK_EXTENT: u32 = 16;

/// Where each of the low six bits of `x`, then of `y`, lands in a 128-bpp `4KB_D_X` block's byte
/// offset, the bits above the block folding into the pipe bits as at 32 bpp. Read off
/// `Addr2ComputeSurfaceAddrFromCoord` as [`DX_X_BITS`] was - the same measurement reproduces that
/// table exactly - and the whole equation for every texel of 256 x 256, 96 x 80 and 32 x 32
/// surfaces.
const DX4_BPP16_X_BITS: [usize; 6] = [0x10, 0x40, 0x200, 0x800, 0x400, 0x100];
/// The same for `y`.
const DX4_BPP16_Y_BITS: [usize; 6] = [0x20, 0x80, 0x100, 0x400, 0x800, 0x200];

/// Byte offset of texel `(x, y)` in a 128-bpp `4KB_D_X` surface `width` texels wide: its block,
/// row-major, then the XOR of its coordinate bits' places.
#[must_use]
pub fn tiled_byte_offset_4kb_dx_bpp16_surface(x: u32, y: u32, width: u32) -> usize {
    let blocks_per_row = width.div_ceil(DX4_BPP16_BLOCK_EXTENT) as usize;
    let block = (y / DX4_BPP16_BLOCK_EXTENT) as usize * blocks_per_row
        + (x / DX4_BPP16_BLOCK_EXTENT) as usize;
    let mut offset = 0;
    for (bit, place) in DX4_BPP16_X_BITS.iter().enumerate() {
        if x >> bit & 1 == 1 {
            offset ^= place;
        }
    }
    for (bit, place) in DX4_BPP16_Y_BITS.iter().enumerate() {
        if y >> bit & 1 == 1 {
            offset ^= place;
        }
    }
    block * DX_BLOCK_BYTES + offset
}

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

/// `log2` of a micro block's 256 bytes (`Block256_2d`), the unit a mip tail places its levels in.
const MICRO_BYTES_LOG2: u32 = 8;

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
    /// `64KB_D_X` at sixteen bytes a texel: 64 x 64-texel blocks, row-major, each laid out by
    /// addrlib's 128-bpp equation ([`tiled_byte_offset_64kb_dx_bpp16_xor`]). A texel is four words.
    Dx64KbBpp16,
    /// `4KB_D_X` at sixteen bytes a texel: 16 x 16-texel blocks, by addrlib's 128-bpp equation
    /// ([`tiled_byte_offset_4kb_dx_bpp16_surface`]). A texel is four words.
    Dx4KbBpp16,
    /// `64KB_S` at four bytes a texel: 128 x 128-texel blocks ([`tiled_byte_offset_64kb_s_bpp4`]).
    S64Kb,
    /// `64KB_S` at sixteen bytes an element, a block-compressed image's blocks: 64 x 64-element
    /// blocks ([`tiled_byte_offset_64kb_s_bpp16`]). An element is four words.
    S64KbBpp16,
    /// `4KB_S` at four bytes a texel: 32 x 32-texel blocks ([`tiled_byte_offset_4kb_s_bpp4`]).
    S4Kb,
    /// `4KB_S` at sixteen bytes an element: 16 x 16-element blocks
    /// ([`tiled_byte_offset_4kb_s_bpp16`]). An element is four words.
    S4KbBpp16,
    /// `4KB_S` at eight bytes an element, a `BC1` image's blocks: 32 x 16-element blocks
    /// ([`tiled_byte_offset_4kb_s_bpp8`]). An element is two words.
    S4KbBpp8,
    /// `4KB_S` at one byte a texel: 64 x 64-texel blocks ([`tiled_byte_offset_4kb_s_bpp1`]).
    S4KbBpp1,
    /// `64KB_R_X` at eight bytes a texel: 128 x 64-texel blocks, row-major, each by addrlib's
    /// 64-bpp equation ([`tiled_byte_offset_64kb_rx_bpp8_surface`]). A texel is two words.
    Rx64KbBpp8,
    /// `64KB_R_X` at two bytes a texel: 256 x 128-texel blocks, row-major, each by addrlib's
    /// 16-bpp equation ([`tiled_byte_offset_64kb_rx_bpp2_surface`]). A texel is half a word.
    Rx64KbBpp2,
    /// `64KB_R_X` at sixteen bytes a texel: 64 x 64-texel blocks, row-major, each by addrlib's
    /// 128-bpp equation ([`tiled_byte_offset_64kb_rx_bpp16_surface`]). A texel is four words.
    Rx64KbBpp16,
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
            // Only its sixteen-byte form is modelled, which `at_bytes_per_texel` keeps to.
            SwizzleMode::Tiled64KbDX => Some(Self::Dx64KbBpp16),
            SwizzleMode::Linear => Some(Self::Linear),
            SwizzleMode::Tiled64KbS => Some(Self::S64Kb),
            SwizzleMode::Tiled4KbS => Some(Self::S4Kb),
            SwizzleMode::Other(_) => None,
        }
    }

    /// This 32-bpp layout's counterpart at `bytes_per_texel`, or `None` where that size is not
    /// modelled in it.
    #[must_use]
    pub const fn at_bytes_per_texel(self, bytes_per_texel: u32) -> Option<Self> {
        match (self, bytes_per_texel) {
            (
                Self::Dx64KbBpp16
                | Self::Dx4KbBpp16
                | Self::S64KbBpp16
                | Self::S4KbBpp16
                | Self::Rx64KbBpp16,
                16,
            )
            | (Self::S4KbBpp8 | Self::Rx64KbBpp8, 8)
            | (Self::S4KbBpp1, 1)
            | (Self::Rx64KbBpp2, 2) => Some(self),
            (
                Self::Dx64KbBpp16
                | Self::Dx4KbBpp16
                | Self::S64KbBpp16
                | Self::S4KbBpp16
                | Self::S4KbBpp8
                | Self::Rx64KbBpp8
                | Self::Rx64KbBpp2
                | Self::Rx64KbBpp16
                | Self::S4KbBpp1,
                _,
            ) => None,
            (Self::Dx4Kb, 16) => Some(Self::Dx4KbBpp16),
            (Self::S64Kb, 16) => Some(Self::S64KbBpp16),
            (Self::S4Kb, 16) => Some(Self::S4KbBpp16),
            (Self::S4Kb, 8) => Some(Self::S4KbBpp8),
            (Self::S4Kb, 1) => Some(Self::S4KbBpp1),
            (Self::Rx64Kb, 1) => Some(Self::Rx64KbBpp1),
            (Self::Rx64Kb, 8) => Some(Self::Rx64KbBpp8),
            (Self::Rx64Kb, 2) => Some(Self::Rx64KbBpp2),
            (Self::Rx64Kb, 16) => Some(Self::Rx64KbBpp16),
            (Self::Linear, 1) => Some(Self::LinearBpp1),
            (_, 4) => Some(self),
            _ => None,
        }
    }

    /// `log2` of the bytes one texel of this layout takes.
    #[must_use]
    pub const fn texel_log2(self) -> u32 {
        match self {
            Self::Rx64KbBpp1 | Self::LinearBpp1 | Self::S4KbBpp1 => 0,
            Self::Rx64KbBpp2 => 1,
            Self::Rx64Kb | Self::Dx4Kb | Self::Linear | Self::S4Kb | Self::S64Kb => 2,
            Self::S4KbBpp8 | Self::Rx64KbBpp8 => 3,
            Self::Dx64KbBpp16
            | Self::Dx4KbBpp16
            | Self::S64KbBpp16
            | Self::S4KbBpp16
            | Self::Rx64KbBpp16 => 4,
        }
    }

    /// Byte offset of element `(x, y)` in an eight-byte layout's surface `width` elements wide.
    fn eight_byte_offset(self, x: u32, y: u32, width: u32) -> usize {
        match self {
            Self::Rx64KbBpp8 => tiled_byte_offset_64kb_rx_bpp8_surface(x, y, width),
            // `S4KbBpp8`, the only other layout of eight-byte elements.
            _ => tiled_byte_offset_4kb_s_bpp8(x, y, width),
        }
    }

    /// Byte offset of texel `(x, y)` in a sixteen-byte layout's surface `width` texels wide.
    fn sixteen_byte_offset(self, x: u32, y: u32, width: u32, pipe_bank_xor: u8) -> usize {
        match self {
            Self::Dx4KbBpp16 => tiled_byte_offset_4kb_dx_bpp16_surface(x, y, width),
            Self::S64KbBpp16 => tiled_byte_offset_64kb_s_bpp16(x, y, width),
            Self::S4KbBpp16 => tiled_byte_offset_4kb_s_bpp16(x, y, width),
            Self::Rx64KbBpp16 => tiled_byte_offset_64kb_rx_bpp16_surface(x, y, width),
            _ => tiled_byte_offset_64kb_dx_bpp16_xor(x, y, width, pipe_bank_xor),
        }
    }

    /// Byte offset of texel `(x, y)` in a standard or 4 KiB four-byte layout's surface `width`
    /// texels wide.
    fn four_byte_offset(self, x: u32, y: u32, width: u32) -> usize {
        match self {
            Self::S4Kb => tiled_byte_offset_4kb_s_bpp4(x, y, width),
            Self::S64Kb => tiled_byte_offset_64kb_s_bpp4(x, y, width),
            _ => tiled_byte_offset_4kb_dx_bpp4_surface(x, y, width),
        }
    }

    /// Byte offset of texel `(x, y)` in a `64KB_R_X` surface `width` texels wide of four- or
    /// eight-byte texels, under no pipe-bank XOR: where a DCC compress block's 256 bytes start
    /// (D873). `None` for any other layout.
    #[must_use]
    pub fn compress_block_offset(self, x: u32, y: u32, width: u32) -> Option<usize> {
        match self {
            Self::Rx64Kb => Some(tiled_byte_offset_64kb_rx_bpp4_surface(x, y, width)),
            Self::Rx64KbBpp8 => Some(tiled_byte_offset_64kb_rx_bpp8_surface(x, y, width)),
            _ => None,
        }
    }

    /// Whether this is a linear layout, whose levels each have rows of their own.
    pub(crate) const fn is_linear(self) -> bool {
        matches!(self, Self::Linear | Self::LinearBpp1)
    }

    /// A linear layout's row pitch in texels: 256 bytes' worth, as [`linear_level_of`] aligns it.
    pub(crate) const fn linear_pitch(self, width: u32) -> u32 {
        width.next_multiple_of(256 >> self.texel_log2())
    }

    /// Words a whole surface of this layout occupies.
    #[must_use]
    pub fn words(self, width: u32, height: u32) -> usize {
        match self {
            Self::Rx64Kb => surface_words_64kb_rx_bpp4(width, height),
            Self::Dx4Kb => surface_words_4kb_dx_bpp4(width, height),
            Self::S4Kb => {
                width.div_ceil(S4_BPP4_BLOCK_EXTENT) as usize
                    * height.div_ceil(S4_BPP4_BLOCK_EXTENT) as usize
                    * (S4_BLOCK_BYTES / 4)
            }
            Self::S64Kb => {
                width.div_ceil(S64_BPP4_BLOCK_EXTENT) as usize
                    * height.div_ceil(S64_BPP4_BLOCK_EXTENT) as usize
                    * (BLOCK_BYTES / 4)
            }
            // Every row but the last at its pitch; the last ends at its width.
            Self::Linear => {
                linear_row_pitch(width) as usize * (height.max(1) as usize - 1) + width as usize
            }
            // Whole 64 KiB blocks, so whole words.
            Self::Rx64KbBpp1 => surface_bytes_64kb_rx_bpp1(width, height) / 4,
            Self::S4KbBpp1 => {
                width.div_ceil(S4_BPP1_BLOCK_EXTENT) as usize
                    * height.div_ceil(S4_BPP1_BLOCK_EXTENT) as usize
                    * S4_BLOCK_BYTES
                    / 4
            }
            Self::Rx64KbBpp16 => {
                width.div_ceil(RX_BPP16_BLOCK_EXTENT) as usize
                    * height.div_ceil(RX_BPP16_BLOCK_EXTENT) as usize
                    * BLOCK_BYTES
                    / 4
            }
            Self::Dx64KbBpp16 => {
                width.div_ceil(DX_BPP16_BLOCK_EXTENT) as usize
                    * height.div_ceil(DX_BPP16_BLOCK_EXTENT) as usize
                    * BLOCK_BYTES
                    / 4
            }
            Self::S64KbBpp16 => surface_bytes_64kb_s_bpp16(width, height) / 4,
            Self::S4KbBpp16 => {
                width.div_ceil(S4_BPP16_BLOCK_EXTENT) as usize
                    * height.div_ceil(S4_BPP16_BLOCK_EXTENT) as usize
                    * S4_BLOCK_BYTES
                    / 4
            }
            Self::Rx64KbBpp2 => {
                width.div_ceil(RX_BPP2_BLOCK_WIDTH) as usize
                    * height.div_ceil(RX_BPP2_BLOCK_HEIGHT) as usize
                    * BLOCK_BYTES
                    / 4
            }
            Self::Rx64KbBpp8 => {
                width.div_ceil(RX_BPP8_BLOCK_WIDTH) as usize
                    * height.div_ceil(RX_BPP8_BLOCK_HEIGHT) as usize
                    * BLOCK_BYTES
                    / 4
            }
            Self::S4KbBpp8 => {
                width.div_ceil(S4_BPP8_BLOCK_WIDTH) as usize
                    * height.div_ceil(S4_BPP8_BLOCK_HEIGHT) as usize
                    * S4_BLOCK_BYTES
                    / 4
            }
            Self::Dx4KbBpp16 => {
                width.div_ceil(DX4_BPP16_BLOCK_EXTENT) as usize
                    * height.div_ceil(DX4_BPP16_BLOCK_EXTENT) as usize
                    * DX_BLOCK_BYTES
                    / 4
            }
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
            Self::Rx64Kb
            | Self::Rx64KbBpp1
            | Self::Rx64KbBpp8
            | Self::Rx64KbBpp2
            | Self::Rx64KbBpp16
            | Self::Dx64KbBpp16
            | Self::S64Kb
            | Self::S64KbBpp16 => 16,
            // A linear surface has no blocks; its "block" is a row's 256 bytes, never a tail's.
            Self::Dx4Kb
            | Self::Dx4KbBpp16
            | Self::S4Kb
            | Self::S4KbBpp16
            | Self::S4KbBpp8
            | Self::S4KbBpp1 => 12,
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
        // The micro block's side: its 256 bytes' texels, square - 16 at one byte a texel, 8 at
        // four, 4 at sixteen.
        let micro = 1 << ((MICRO_BYTES_LOG2 - self.texel_log2()) / 2);
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
            Self::Rx64Kb | Self::Rx64KbBpp1 | Self::Dx64KbBpp16 => true,
            // The standard swizzles have no pipe XOR (`ADDR_SW_64KB_S`, `addrtypes.h:236`).
            Self::Dx4Kb
            | Self::Dx4KbBpp16
            | Self::Linear
            | Self::LinearBpp1
            | Self::S64Kb
            | Self::S64KbBpp16
            | Self::S4Kb
            | Self::S4KbBpp16
            | Self::S4KbBpp8
            | Self::Rx64KbBpp8
            | Self::Rx64KbBpp2
            | Self::Rx64KbBpp16
            | Self::S4KbBpp1 => pipe_bank_xor == 0,
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
            Self::Dx4Kb | Self::S4Kb | Self::S64Kb => {
                let mut linear = Vec::with_capacity(width as usize * height as usize);
                for y in 0..height {
                    for x in 0..width {
                        linear.push(map(tiled[self.four_byte_offset(x, y, width) / 4]));
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
            // Each texel is one byte of the little-endian words, its rows at the pitch: each row
            // copied from the bytes as they lie, which the host reads in the same order.
            Self::LinearBpp1 => {
                let pitch = self.linear_pitch(width) as usize;
                let bytes = zerocopy::IntoBytes::as_bytes(tiled);
                let mut linear = Vec::with_capacity(width as usize * height as usize);
                for row in 0..height as usize {
                    let start = row * pitch;
                    linear.extend(
                        bytes[start..start + width as usize]
                            .iter()
                            .map(|&texel| map(u32::from(texel))),
                    );
                }
                linear
            }
            // Each texel is one half of the little-endian words.
            Self::Rx64KbBpp2 => (0..height)
                .flat_map(|y| (0..width).map(move |x| (x, y)))
                .map(|(x, y)| {
                    let at = tiled_byte_offset_64kb_rx_bpp2_surface(x, y, width);
                    map(tiled[at / 4] >> ((at % 4) * 8) & 0xFFFF)
                })
                .collect(),
            // Each texel is one byte of the little-endian words.
            Self::Rx64KbBpp1 => (0..height)
                .flat_map(|y| (0..width).map(move |x| (x, y)))
                .map(|(x, y)| {
                    let at = tiled_byte_offset_64kb_rx_bpp1_xor(x, y, width, pipe_bank_xor);
                    map(tiled[at / 4] >> ((at % 4) * 8) & 0xFF)
                })
                .collect(),
            // Each texel is one byte of the little-endian words.
            Self::S4KbBpp1 => (0..height)
                .flat_map(|y| (0..width).map(move |x| (x, y)))
                .map(|(x, y)| {
                    let at = tiled_byte_offset_4kb_s_bpp1(x, y, width);
                    map(tiled[at / 4] >> ((at % 4) * 8) & 0xFF)
                })
                .collect(),
            // Each element is two words, in order.
            Self::S4KbBpp8 | Self::Rx64KbBpp8 => (0..height)
                .flat_map(|y| (0..width).map(move |x| (x, y)))
                .flat_map(|(x, y)| {
                    let at = self.eight_byte_offset(x, y, width) / 4;
                    [map(tiled[at]), map(tiled[at + 1])]
                })
                .collect(),
            // Each texel is four words, in order.
            Self::Dx64KbBpp16
            | Self::Dx4KbBpp16
            | Self::S64KbBpp16
            | Self::S4KbBpp16
            | Self::Rx64KbBpp16 => (0..height)
                .flat_map(|y| (0..width).map(move |x| (x, y)))
                .flat_map(|(x, y)| {
                    let at = self.sixteen_byte_offset(x, y, width, pipe_bank_xor) / 4;
                    tiled[at..at + 4]
                        .iter()
                        .map(|&word| map(word))
                        .collect::<Vec<_>>()
                })
                .collect(),
        }
    }

    /// [`Self::tile_mapped`] into only the surface's first `tiled.len()` words, for a layout whose
    /// texels can be tiled into a prefix of it: `64KB_R_X`, the layout a GL context draws its colour
    /// target in. `None` for any other, which is tiled into a whole surface instead.
    pub fn tile_prefix_mapped(
        self,
        linear: &[u32],
        (width, height): (u32, u32),
        pipe_bank_xor: u8,
        tiled: &mut [u32],
        map: impl Fn(u32) -> u32 + Sync,
    ) -> Option<Result<(), DetileError>> {
        self.tiles_prefixes().then(|| {
            tile_prefix_64kb_rx_bpp4_mapped(linear, width, height, pipe_bank_xor, tiled, map)
        })
    }

    /// How many of a `width` x `height` surface's first words a tile of a whole image writes every
    /// one of, so that what memory held there need not be laid down first: for `64KB_R_X` the full
    /// block rows of a surface whole blocks wide - a 1920 x 1080 frame's first 1024 rows, nearly
    /// all of it. Zero for anything else, whose padding words may lie anywhere.
    #[must_use]
    pub fn words_tiled_whole(self, width: u32, height: u32) -> usize {
        if self != Self::Rx64Kb || width % SINGLE_BLOCK_EXTENT != 0 {
            return 0;
        }
        let rows = (height / SINGLE_BLOCK_EXTENT) as usize;
        rows * (width / SINGLE_BLOCK_EXTENT) as usize * BLOCK_WORDS
    }

    /// Whether [`Self::tile_prefix_mapped`] tiles into a prefix of this layout's surface.
    #[must_use]
    pub const fn tiles_prefixes(self) -> bool {
        matches!(self, Self::Rx64Kb)
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
            Self::Dx4Kb | Self::S4Kb | Self::S64Kb => {
                let needed_words = self.words(width, height);
                let texels = width as usize * height as usize;
                if tiled.len() < needed_words || linear.len() != texels {
                    return Err(DetileError::TiledDataTooShort {
                        needed_words: needed_words.max(texels),
                        got_words: tiled.len().min(linear.len()),
                    });
                }
                for (y, row) in linear.chunks(width as usize).enumerate() {
                    for (x, &texel) in row.iter().enumerate() {
                        tiled[self.four_byte_offset(x as u32, y as u32, width) / 4] = map(texel);
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
            Self::Rx64KbBpp2 => tile_64kb_rx_bpp2((linear, tiled), (width, height), map),
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
            // Each element's two words, in order.
            Self::S4KbBpp8 | Self::Rx64KbBpp8 => {
                tile_bpp8(self, (linear, tiled), (width, height), map)
            }
            Self::S4KbBpp1 => tile_4kb_s_bpp1((linear, tiled), (width, height), map),
            // Each texel's four words, in order.
            Self::Dx64KbBpp16
            | Self::Dx4KbBpp16
            | Self::S64KbBpp16
            | Self::S4KbBpp16
            | Self::Rx64KbBpp16 => {
                self.tile_bpp16((linear, tiled), (width, height), pipe_bank_xor, map)
            }
        }
    }

    /// Tiles a sixteen-byte layout's linear texels, four words each, into `tiled`, each word through
    /// `map`: [`Self::tile_mapped`] for the sixteen-byte layouts.
    fn tile_bpp16(
        self,
        (linear, tiled): (&[u32], &mut [u32]),
        (width, height): (u32, u32),
        pipe_bank_xor: u8,
        map: impl Fn(u32) -> u32,
    ) -> Result<(), DetileError> {
        let needed_words = self.words(width, height);
        let words = width as usize * height as usize * 4;
        if tiled.len() < needed_words || linear.len() != words {
            return Err(DetileError::TiledDataTooShort {
                needed_words: needed_words.max(words),
                got_words: tiled.len().min(linear.len()),
            });
        }
        for (index, texel) in linear.chunks_exact(4).enumerate() {
            let (x, y) = (index as u32 % width, index as u32 / width);
            let at = self.sixteen_byte_offset(x, y, width, pipe_bank_xor) / 4;
            for (slot, &word) in tiled[at..at + 4].iter_mut().zip(texel) {
                *slot = map(word);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    /// Tiling into a surface's first words writes exactly what tiling into the whole surface puts
    /// there, across a block row's end and partway into a block, and nothing past them.
    #[test]
    fn a_prefix_tiles_as_the_whole_surface_cut_short() {
        let (width, height) = (300, 200);
        let linear: Vec<u32> = (0..width * height).collect();
        let words = surface_words_64kb_rx_bpp4(width, height);
        let mut whole = vec![u32::MAX; words];
        super::tile_surface_64kb_rx_bpp4_mapped(&linear, width, height, 3, &mut whole, |w| w)
            .expect("tiles");
        for prefix in [
            words - 1,
            super::BLOCK_WORDS * 3 + 777,
            (width * height) as usize,
        ] {
            let mut cut = vec![u32::MAX; prefix];
            super::SurfaceLayout::Rx64Kb
                .tile_prefix_mapped(&linear, (width, height), 3, &mut cut, |w| w)
                .expect("a layout that tiles prefixes")
                .expect("tiles");
            assert_eq!(cut, whole[..prefix], "{prefix} words");
        }
        assert!(
            super::SurfaceLayout::S64Kb
                .tile_prefix_mapped(&linear, (width, height), 0, &mut [0; 4], |w| w)
                .is_none()
        );
    }

    /// Every word a whole image's tile is said to write is written, at any pipe-bank XOR, and the
    /// words after them hold padding it leaves: what memory held need not be laid down under them.
    #[test]
    fn the_words_tiled_whole_are_every_one_written() {
        for (width, height, xor) in [(256, 200, 0), (384, 300, 5), (128, 128, 7)] {
            let linear = vec![0u32; (width * height) as usize];
            let mut tiled = vec![u32::MAX; surface_words_64kb_rx_bpp4(width, height)];
            super::tile_surface_64kb_rx_bpp4_mapped(&linear, width, height, xor, &mut tiled, |w| w)
                .expect("tiles");
            let whole = super::SurfaceLayout::Rx64Kb.words_tiled_whole(width, height);
            assert!(whole > 0, "{width} x {height}");
            assert!(
                tiled[..whole].iter().all(|&w| w == 0),
                "{width} x {height}: every word written"
            );
            if height % 128 != 0 {
                assert!(
                    tiled[whole..].contains(&u32::MAX),
                    "{width} x {height}: padding after them"
                );
            }
        }
        assert_eq!(super::SurfaceLayout::Rx64Kb.words_tiled_whole(300, 200), 0);
        assert_eq!(super::SurfaceLayout::S64Kb.words_tiled_whole(256, 256), 0);
    }

    /// A 32-bpp `64KB_R_X` volume's slice XOR is the XOR of the terms addrlib gives for each bit
    /// of the slice index (`tools/addrlib/equation 64KB_R_X 4 32 3d`): slice 0 none, 1 `0x800`, 3
    /// `0xc00`, 31 `0xf00`; past slice 31 nothing was read.
    #[test]
    fn a_volume_slice_xors_its_index_into_the_block() {
        use super::slice_xor_64kb_rx_bpp4;
        assert_eq!(slice_xor_64kb_rx_bpp4(0), Some(0));
        assert_eq!(slice_xor_64kb_rx_bpp4(1), Some(0x800));
        assert_eq!(slice_xor_64kb_rx_bpp4(3), Some(0xc00));
        assert_eq!(slice_xor_64kb_rx_bpp4(16), Some(0));
        assert_eq!(slice_xor_64kb_rx_bpp4(31), Some(0xf00));
        assert_eq!(slice_xor_64kb_rx_bpp4(32), None);
    }

    /// `4KB_S` at one byte a texel places each texel by addrlib's pattern for this mode and size
    /// (`GFX10_SW_4K_S_PATINFO` at 1 bpe: nibbles 01 row 0 and 2 row 1): X0-X3, Y0-Y4, X4, Y5, X5
    /// from byte bit 0, a 64 x 64-texel block.
    #[test]
    fn the_1_byte_4kb_s_offsets_are_addrlibs() {
        use super::{SurfaceLayout, SwizzleMode, tiled_byte_offset_4kb_s_bpp1};
        for (bit, at) in [0x1, 0x2, 0x4, 0x8, 0x200, 0x800].into_iter().enumerate() {
            assert_eq!(
                tiled_byte_offset_4kb_s_bpp1(1 << bit, 0, 128),
                at,
                "x bit {bit}"
            );
        }
        for (bit, at) in [0x10, 0x20, 0x40, 0x80, 0x100, 0x400]
            .into_iter()
            .enumerate()
        {
            assert_eq!(
                tiled_byte_offset_4kb_s_bpp1(0, 1 << bit, 128),
                at,
                "y bit {bit}"
            );
        }
        assert_eq!(
            tiled_byte_offset_4kb_s_bpp1(64, 0, 128),
            0x1000,
            "the next block across"
        );
        assert_eq!(tiled_byte_offset_4kb_s_bpp1(0, 64, 128), 0x2000, "and down");
        let layout = SurfaceLayout::of(SwizzleMode::Tiled4KbS)
            .and_then(|layout| layout.at_bytes_per_texel(1))
            .expect("modelled");
        let tiled: Vec<u32> = (0..layout.words(16, 16) as u32)
            .map(|w| w.wrapping_mul(0x0404_0404).wrapping_add(0x0302_0100))
            .collect();
        let linear = layout.detile_mapped(&tiled, 16, 16, 0, |b| b);
        assert_eq!(linear.len(), 16 * 16, "a texel each");
        assert_eq!(linear[..4], [0, 1, 2, 3], "the first row's bytes, in order");
        assert_eq!(linear[16], 0x10, "row 1 starts at byte 0x10");
    }

    /// `4KB_S` at eight bytes an element places each element by addrlib's pattern for this mode and
    /// size (`gfx10SwizzlePattern.h`, `GFX10_SW_4K_S_PATINFO` 8 bpe: nibbles 01 row 3 and 2 row 4):
    /// X0 Y0 Y1 X1 X2 Y2 X3 Y3 X4 from byte bit 3, a 32 x 16-element block.
    #[test]
    fn the_8_byte_4kb_s_offsets_are_addrlibs() {
        use super::{SurfaceLayout, SwizzleMode, tiled_byte_offset_4kb_s_bpp8};
        assert_eq!(tiled_byte_offset_4kb_s_bpp8(1, 0, 64), 0x8);
        assert_eq!(tiled_byte_offset_4kb_s_bpp8(0, 1, 64), 0x10);
        assert_eq!(tiled_byte_offset_4kb_s_bpp8(0, 2, 64), 0x20);
        assert_eq!(tiled_byte_offset_4kb_s_bpp8(2, 0, 64), 0x40);
        assert_eq!(tiled_byte_offset_4kb_s_bpp8(4, 0, 64), 0x80);
        assert_eq!(tiled_byte_offset_4kb_s_bpp8(0, 4, 64), 0x100);
        assert_eq!(tiled_byte_offset_4kb_s_bpp8(8, 0, 64), 0x200);
        assert_eq!(tiled_byte_offset_4kb_s_bpp8(0, 8, 64), 0x400);
        assert_eq!(tiled_byte_offset_4kb_s_bpp8(16, 0, 64), 0x800);
        assert_eq!(
            tiled_byte_offset_4kb_s_bpp8(31, 15, 64),
            0xff8,
            "the block's last element"
        );
        assert_eq!(
            tiled_byte_offset_4kb_s_bpp8(32, 0, 64),
            0x1000,
            "the next block across"
        );
        assert_eq!(
            tiled_byte_offset_4kb_s_bpp8(0, 16, 64),
            0x2000,
            "and down, two blocks a row"
        );
        let layout = SurfaceLayout::of(SwizzleMode::Tiled4KbS)
            .and_then(|layout| layout.at_bytes_per_texel(8))
            .expect("modelled");
        assert_eq!(layout.words(64, 32), 4 * 4096 / 4);
        let tiled: Vec<u32> = (0..layout.words(64, 32) as u32).collect();
        let linear = layout.detile_mapped(&tiled, 64, 32, 0, |w| w);
        assert_eq!(linear.len(), 64 * 32 * 2, "two words an element");
        assert_eq!(linear[2..4], [2, 3], "element (1, 0) is bytes 8..16");
    }

    /// `4KB_S` at four bytes a texel answers what addrlib answers for this console: a chain's
    /// levels and tail places for 64 x 64 x 7, and texels of a 100 x 70 surface, a detile and tile
    /// round trip among them.
    #[test]
    fn a_32_bpp_4kb_s_surface_is_addressed_as_addrlib_addresses_it() {
        use super::{SurfaceLayout, tiled_byte_offset_4kb_s_bpp4 as at};
        for ((x, y), addr) in [
            ((1, 0), 4),
            ((4, 0), 128),
            ((16, 0), 2048),
            ((0, 1), 16),
            ((0, 4), 64),
            ((0, 16), 1024),
            ((32, 0), 4096),
            ((0, 32), 8192),
        ] {
            assert_eq!(at(x, y, 64), addr, "({x}, {y}) in 64 wide");
        }
        for ((x, y), addr) in [((13, 27), 1972), ((45, 50), 22180), ((99, 69), 45148)] {
            assert_eq!(at(x, y, 100), addr, "({x}, {y}) in 100 wide");
        }
        let layout = SurfaceLayout::of(SwizzleMode::Tiled4KbS)
            .and_then(|layout| layout.at_bytes_per_texel(4))
            .expect("modelled");
        assert_eq!(layout.words(100, 70), 49152 / 4);
        assert_eq!(layout.level_offset(64, 64, 7, 0), Some(8192));
        assert_eq!(layout.level_offset(64, 64, 7, 1), Some(4096));
        let tails: Vec<Option<(u32, u32)>> = (2..7)
            .map(|level| layout.tail_origin(64, 64, 7, level))
            .collect();
        assert_eq!(
            tails,
            [(16, 0), (8, 16), (0, 24), (0, 16), (8, 8)].map(Some)
        );
        let linear: Vec<u32> = (0..100 * 70).collect();
        let mut tiled = vec![0; layout.words(100, 70)];
        layout
            .tile_mapped(&linear, 100, 70, 0, &mut tiled, |w| w)
            .expect("tiles");
        assert_eq!(tiled[45148 / 4], 69 * 100 + 99);
        assert_eq!(layout.detile_mapped(&tiled, 100, 70, 0, |w| w), linear);
    }

    /// `64KB_S` at four bytes a texel is addrlib's pattern for it (`GFX10_SW_64K_S_RBPLUS_PATINFO`,
    /// nibble rows 2, 3, 3): its low twelve bits are the `4KB_S` equation's within a 32 x 32
    /// square, y5, x5, y6, x6 then take bits 12-15, and a 128 x 128 block follows across the pitch
    /// and down - the 444 x 456 `8_8_8_8` image PPSA28061 samples is 4 x 4 blocks. Its sixteen-byte
    /// form stays the block-compressed one.
    #[test]
    fn a_32_bpp_64kb_s_surface_is_addressed_as_addrlib_patterns_it() {
        use super::{
            SurfaceLayout, tiled_byte_offset_4kb_s_bpp4, tiled_byte_offset_64kb_s_bpp4 as at,
        };
        for y in 0..32 {
            for x in 0..32 {
                assert_eq!(
                    at(x, y, 444),
                    tiled_byte_offset_4kb_s_bpp4(x, y, 32),
                    "({x}, {y})"
                );
            }
        }
        for ((x, y), addr) in [
            ((0, 32), 0x1000),
            ((32, 0), 0x2000),
            ((0, 64), 0x4000),
            ((64, 0), 0x8000),
            ((127, 127), 0xFFFC),
            ((128, 0), 0x1_0000),
            ((0, 128), 4 * 0x1_0000),
            ((443, 455), 15 * 0x1_0000 + 0x6A7C),
        ] {
            assert_eq!(at(x, y, 444), addr, "({x}, {y}) in 444 wide");
        }
        let layout = SurfaceLayout::of(SwizzleMode::Tiled64KbS).expect("modelled");
        assert_eq!(layout.at_bytes_per_texel(4), Some(SurfaceLayout::S64Kb));
        assert_eq!(
            layout.at_bytes_per_texel(16),
            Some(SurfaceLayout::S64KbBpp16)
        );
        assert_eq!(layout.words(444, 456), 16 * 0x1_0000 / 4);
        let linear: Vec<u32> = (0..444 * 456).collect();
        let mut tiled = vec![0; layout.words(444, 456)];
        layout
            .tile_mapped(&linear, 444, 456, 0, &mut tiled, |w| w)
            .expect("tiles");
        assert_eq!(tiled[at(443, 455, 444) / 4], 455 * 444 + 443);
        assert_eq!(layout.detile_mapped(&tiled, 444, 456, 0, |w| w), linear);
    }

    /// A 2048 x 1024 `BC3` texture of 12 levels in `4KB_S`, as PPSA02664 samples one: 512 x 256
    /// blocks, placed and addressed as addrlib's `Addr2ComputeSurfaceInfo` and
    /// `Addr2ComputeSurfaceAddrFromCoord` place and address them for this console's configuration.
    #[test]
    fn a_4kb_s_chain_is_placed_as_addrlib_places_it() {
        use super::SurfaceLayout;
        let layout = SurfaceLayout::of(SwizzleMode::Tiled4KbS)
            .and_then(|layout| layout.at_bytes_per_texel(16))
            .expect("modelled");
        let (width, height, levels) = (512, 256, 12);
        assert_eq!(layout.first_level_in_tail(width, height, levels), 6);
        let offsets: Vec<Option<u64>> = (0..6)
            .map(|level| layout.level_offset(width, height, levels, level))
            .collect();
        assert_eq!(
            offsets,
            [704_512, 180_224, 49152, 16384, 8192, 4096].map(Some)
        );
        let tails: Vec<Option<(u32, u32)>> = (6..12)
            .map(|level| layout.tail_origin(width, height, levels, level))
            .collect();
        assert_eq!(
            tails,
            [(8, 0), (4, 8), (0, 12), (0, 8), (4, 4), (4, 0)].map(Some)
        );
        for ((x, y), addr) in [
            ((1, 0), 704_576),
            ((8, 0), 706_560),
            ((0, 8), 705_536),
            ((16, 0), 708_608),
            ((0, 16), 835_584),
            ((5, 9), 706_128),
            ((300, 200), 2_354_688),
        ] {
            assert_eq!(
                704_512 + layout.sixteen_byte_offset(x, y, width, 0),
                addr,
                "level 0 ({x}, {y})"
            );
        }
        // Tail levels, within the tail's block at the surface's start.
        for ((x, y), addr) in [((8, 0), 2048), ((11, 2), 2272), ((0, 8), 1024)] {
            assert_eq!(
                layout.sixteen_byte_offset(x, y, 16, 0),
                addr,
                "tail ({x}, {y})"
            );
        }
    }

    /// `64KB_S` at sixteen bytes an element answers what addrlib answers for this console: the
    /// addresses `Addr2ComputeSurfaceAddrFromCoord` gave for these elements.
    #[test]
    fn the_64kb_s_bpp16_equation_is_addrlibs() {
        use super::tiled_byte_offset_64kb_s_bpp16 as at;
        assert_eq!(at(0, 0, 256), 0);
        assert_eq!(at(129, 3, 256), 131_184);
        assert_eq!(at(255, 255, 256), 1_048_560);
        assert_eq!(at(256, 0, 1019), 262_144);
        assert_eq!(at(300, 77, 1019), 1_347_344);
        assert_eq!(super::surface_bytes_64kb_s_bpp16(1019, 187), 3_145_728);
    }

    use super::{
        DetileError, detile_64kb_rx_bpp4, detile_surface_64kb_rx_bpp4, surface_words_64kb_rx_bpp4,
        tile_surface_64kb_rx_bpp4, tiled_byte_offset_64kb_rx_bpp4 as offset,
        tiled_byte_offset_64kb_rx_bpp4_surface as at,
    };
    use crate::registers::SwizzleMode;

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

    /// The run path's layout places the hardware-measured texel: a 64 x 64 32-bpp `64KB_R_X`
    /// surface detiled through [`super::SurfaceLayout`] puts tiled byte 4348 at texel `(15, 15)`,
    /// and nowhere else.
    #[test]
    fn the_run_path_layout_places_the_measured_pixel() {
        const RED: u32 = 0xff00_00ff;
        let layout = super::SurfaceLayout::of(SwizzleMode::Tiled64KbRX).expect("modelled");
        let mut tiled = vec![0u32; layout.words(64, 64)];
        tiled[4348 / 4] = RED;
        let linear = layout.detile_mapped(&tiled, 64, 64, 0, |texel| texel);
        assert_eq!(linear[15 * 64 + 15], RED, "texel (15,15)");
        assert_eq!(linear.iter().filter(|&&p| p == RED).count(), 1);
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

    /// `64KB_R_X` at sixteen bytes a texel is row-major 64 x 64-texel blocks, each by addrlib's
    /// equation, with `x` bit 6 adding `0x400` and `y` bit 6 `0x800` inside the block
    /// (`tools/addrlib/equation 64KB_R_X 16 32`, checked against addrlib for 4.2 million texels).
    /// A texel is four words.
    #[test]
    fn sixteen_byte_64kb_r_x_is_addrlib_s_equation() {
        use super::{SurfaceLayout, tiled_byte_offset_64kb_rx_bpp16_surface};
        assert_eq!(
            SurfaceLayout::Rx64Kb.at_bytes_per_texel(16),
            Some(SurfaceLayout::Rx64KbBpp16)
        );
        let layout = SurfaceLayout::Rx64KbBpp16;
        assert_eq!(layout.texel_log2(), 4);
        for ((x, y), offset) in [
            ((1, 0), 0x10),
            ((0, 1), 0x20),
            ((4, 0), 0x2000),
            ((64, 0), 0x1_0400),
            ((0, 64), 0x2_0800),
            ((64, 64), 0x3_0c00),
        ] {
            assert_eq!(
                tiled_byte_offset_64kb_rx_bpp16_surface(x, y, 128),
                offset,
                "({x}, {y})"
            );
        }
        let (width, height) = (70, 66);
        let linear: Vec<u32> = (0..width * height * 4).collect();
        let mut tiled = vec![0_u32; layout.words(width, height)];
        layout
            .tile_mapped(&linear, width, height, 0, &mut tiled, |w| w)
            .expect("tiled");
        assert_eq!(
            layout.detile_mapped(&tiled, width, height, 0, |w| w),
            linear
        );
    }

    /// `64KB_R_X` at two bytes a texel is row-major 256 x 128-texel blocks, each by addrlib's
    /// equation, with no term above the block (`tools/addrlib/equation 64KB_R_X 2 32`, checked
    /// against addrlib for 34 million texels). Each texel is the low half of its linear word, and
    /// tiling one leaves the other half of its tiled word alone.
    #[test]
    fn two_byte_64kb_r_x_is_addrlib_s_equation() {
        use super::{SurfaceLayout, tiled_byte_offset_64kb_rx_bpp2_surface};
        assert_eq!(
            SurfaceLayout::Rx64Kb.at_bytes_per_texel(2),
            Some(SurfaceLayout::Rx64KbBpp2)
        );
        let layout = SurfaceLayout::Rx64KbBpp2;
        assert_eq!(layout.texel_log2(), 1);
        for ((x, y), offset) in [
            ((1, 0), 0x2),
            ((8, 0), 0x180),
            ((0, 1), 0x10),
            ((0, 16), 0x1200),
            ((256, 0), 0x1_0000),
            ((0, 128), 0x2_0000),
        ] {
            assert_eq!(
                tiled_byte_offset_64kb_rx_bpp2_surface(x, y, 512),
                offset,
                "({x}, {y})"
            );
        }
        let (width, height) = (300, 150);
        let linear: Vec<u32> = (0..width * height).map(|i| i & 0xFFFF).collect();
        let mut tiled = vec![0xAAAA_AAAA_u32; layout.words(width, height)];
        layout
            .tile_mapped(&linear, width, height, 0, &mut tiled, |w| w)
            .expect("tiled");
        assert_eq!(
            layout.detile_mapped(&tiled, width, height, 0, |w| w),
            linear
        );
        let mut one = vec![0xAAAA_AAAA_u32; layout.words(width, height)];
        let mut single = vec![0_u32; (width * height) as usize];
        single[1] = 0x1234;
        layout
            .tile_mapped(&single, width, height, 0, &mut one, |w| w)
            .expect("tiled");
        assert_eq!(one[0], 0x1234_0000, "texel 1 is the high half of word 0");
    }

    /// `64KB_R_X` at eight bytes a texel is row-major 128 x 64-texel blocks, each by addrlib's
    /// equation, with `y` bit 6 adding `0x800` inside the block (`tools/addrlib/equation 64KB_R_X
    /// 8`, which checks it against addrlib for 8.5 million texels). A 240 x 135 surface, Grand
    /// Theft Auto V's half-resolution target, tiles and detiles back to itself, two words a texel.
    #[test]
    fn eight_byte_64kb_r_x_is_addrlib_s_equation() {
        use super::{SurfaceLayout, tiled_byte_offset_64kb_rx_bpp8_surface};
        let layout = SurfaceLayout::Rx64Kb.at_bytes_per_texel(8);
        assert_eq!(layout, Some(SurfaceLayout::Rx64KbBpp8));
        let layout = SurfaceLayout::Rx64KbBpp8;
        assert_eq!(layout.texel_log2(), 3);
        // From the tool, on a surface two blocks wide.
        for ((x, y), offset) in [
            ((1, 0), 0x8),
            ((0, 1), 0x10),
            ((8, 0), 0x2100),
            ((0, 64), 0x2_0800),
            ((1, 64), 0x2_0808),
            ((7, 64), 0x2_0868),
            ((128, 0), 0x1_0000),
        ] {
            assert_eq!(
                tiled_byte_offset_64kb_rx_bpp8_surface(x, y, 256),
                offset,
                "({x}, {y})"
            );
        }
        let (width, height) = (240, 135);
        let linear: Vec<u32> = (0..width * height * 2).collect();
        let mut tiled = vec![0_u32; layout.words(width, height)];
        layout
            .tile_mapped(&linear, width, height, 0, &mut tiled, |w| w)
            .expect("tiled");
        assert_eq!(
            layout.detile_mapped(&tiled, width, height, 0, |w| w),
            linear
        );
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

    /// A 128-bpp `64KB_D_X` surface lies as addrlib places it for this console: each coordinate
    /// bit's contribution as `Addr2ComputeSurfaceAddrFromCoord` gives it, 64 x 64-texel blocks in
    /// row-major order, and a texel's four words in order - here round-tripped under a pipe-bank
    /// XOR, which moves each texel by it shifted to the 256-byte interleave.
    #[test]
    fn a_sixteen_byte_64kb_d_x_surface_lies_as_addrlib_places_it() {
        use super::{SurfaceLayout, tiled_byte_offset_64kb_dx_bpp16_xor as at};
        assert_eq!(at(1, 0, 128, 0), 0x10);
        assert_eq!(at(0, 1, 128, 0), 0x20);
        assert_eq!(at(16, 0, 128, 0), 0x2400, "a bit that lands in two places");
        assert_eq!(at(0, 32, 128, 0), 0x4200);
        assert_eq!(at(63, 63, 128, 0), 0xf0f0);
        assert_eq!(at(64, 0, 128, 0), 0x1_0000, "the next block across");
        assert_eq!(at(0, 64, 128, 0), 0x2_0000, "the block below");
        assert_eq!(at(1, 0, 128, 3), 0x10 ^ 0x300);
        assert_eq!(
            SurfaceLayout::of(SwizzleMode::Tiled64KbDX)
                .and_then(|layout| layout.at_bytes_per_texel(16)),
            Some(SurfaceLayout::Dx64KbBpp16)
        );
        assert_eq!(
            crate::registers::decode_swizzle_mode(26),
            SwizzleMode::Tiled64KbDX
        );

        let (width, height) = (96, 70);
        let layout = SurfaceLayout::Dx64KbBpp16;
        let linear: Vec<u32> = (0..width * height * 4).collect();
        let mut tiled = vec![0u32; layout.words(width, height)];
        assert_eq!(tiled.len(), 4 * 0x1_0000 / 4, "two blocks across, two down");
        layout
            .tile_mapped(&linear, width, height, 5, &mut tiled, |w| w)
            .expect("the surface is covered");
        let texel = (2 * width + 3) as usize;
        let word = at(3, 2, width, 5) / 4;
        assert_eq!(tiled[word..word + 4], linear[texel * 4..texel * 4 + 4]);
        assert_eq!(
            layout.detile_mapped(&tiled, width, height, 5, |w| w),
            linear
        );
    }

    /// A 128 x 128, ten-level chain of sixteen-byte `64KB_D_X` texels lies as addrlib lays it out
    /// for this console (`ComputeSurfaceInfo`, `ComputeSurfaceAddrFromCoord` per level): level 0
    /// at 128 KiB and level 1 at 64 KiB, and levels 2 on in the tail's block at offset 0, each at
    /// the texel addrlib's level `(0, 0)` lands on.
    #[test]
    fn a_sixteen_byte_64kb_d_x_chain_lies_as_addrlib_places_it() {
        use super::SurfaceLayout;
        let layout = SurfaceLayout::Dx64KbBpp16;
        assert_eq!(layout.first_level_in_tail(128, 128, 10), 2);
        assert_eq!(layout.level_offset(128, 128, 10, 0), Some(131_072));
        assert_eq!(layout.level_offset(128, 128, 10, 1), Some(65_536));
        for (level, origin) in [
            (2, (32, 0)),
            (3, (0, 32)),
            (4, (16, 0)),
            (5, (0, 16)),
            (6, (8, 0)),
            (7, (4, 8)),
        ] {
            assert_eq!(
                layout.tail_origin(128, 128, 10, level),
                Some(origin),
                "level {level}"
            );
        }

        // Level 2, 32 x 32, read and written in the tail's block: its texel (0, 0) is the block's
        // (32, 0), four words each, and the tail's other bytes are left as they were.
        let level =
            crate::registers::chain_level((0, 0), (128, 128), (10, 2), layout).expect("placed");
        assert_eq!((level.base, level.width, level.height), (0, 32, 32));
        let mut tiled = vec![0xffff_ffff_u32; level.words()];
        let linear: Vec<u32> = (0..32 * 32 * 4).collect();
        level
            .tile_mapped(&linear, &mut tiled, |w| w)
            .expect("the level is covered");
        let first = super::tiled_byte_offset_64kb_dx_bpp16_xor(32, 0, 64, 0) / 4;
        assert_eq!(tiled[first..first + 4], [0, 1, 2, 3]);
        assert_eq!(
            tiled[0], 0xffff_ffff,
            "the tail's level at (0, 0) is not this one"
        );
        assert_eq!(level.detile_mapped(&tiled, |w| w), linear);
    }

    /// A 128-bpp `4KB_D_X` surface lies as addrlib places it for this console: its coordinate bits'
    /// places, the bits above the 16 x 16 block folding in, and the tail of a 32 x 32, six-level
    /// chain from level 2 at addrlib's origins.
    #[test]
    fn a_sixteen_byte_4kb_d_x_surface_lies_as_addrlib_places_it() {
        use super::{SurfaceLayout, tiled_byte_offset_4kb_dx_bpp16_surface as at};
        assert_eq!(at(1, 0, 32), 0x10);
        assert_eq!(
            at(16, 0, 32),
            0x1000 ^ 0x400,
            "the next block, a pipe bit folded in"
        );
        assert_eq!(at(0, 16, 32), 0x2000 ^ 0x800);
        assert_eq!(at(32, 0, 64), 0x2000 ^ 0x100);
        let layout = SurfaceLayout::of(SwizzleMode::Tiled4KbDX)
            .and_then(|layout| layout.at_bytes_per_texel(16))
            .expect("modelled");
        assert_eq!(layout, SurfaceLayout::Dx4KbBpp16);
        assert_eq!(layout.first_level_in_tail(32, 32, 6), 2);
        assert_eq!(layout.level_offset(32, 32, 6, 0), Some(8192));
        assert_eq!(layout.level_offset(32, 32, 6, 1), Some(4096));
        for (level, origin) in [(2, (8, 0)), (3, (4, 8)), (4, (0, 12)), (5, (0, 8))] {
            assert_eq!(
                layout.tail_origin(32, 32, 6, level),
                Some(origin),
                "level {level}"
            );
        }
        let (width, height) = (40, 20);
        let linear: Vec<u32> = (0..width * height * 4).collect();
        let mut tiled = vec![0u32; layout.words(width, height)];
        layout
            .tile_mapped(&linear, width, height, 0, &mut tiled, |w| w)
            .expect("covered");
        assert_eq!(
            layout.detile_mapped(&tiled, width, height, 0, |w| w),
            linear
        );
    }
}
