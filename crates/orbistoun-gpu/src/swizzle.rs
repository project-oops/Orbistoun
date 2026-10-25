//! Every tiled swizzle mode at every element size a sampled format takes, as the exclusive-or
//! equations addrlib addresses them by (D923).
//!
//! On this generation a tiled surface is blocks of 256 bytes, 4 KiB or 64 KiB, row-major across the
//! surface's pitch. Within a block each bit of an element's `x` and `y` flips a fixed set of bits
//! of its byte offset, so a block is laid out by one term per coordinate bit, and in a few modes the
//! low bits of the block's own column and row flip more ("above the block").
//!
//! [`EQUATIONS`] holds every such basis for `64KB_R_X`, `64KB_D_X`, `4KB_D_X`, `64KB_S`, `4KB_S`,
//! `64KB_Z_X` and `256B_S` at one, two, four, eight and sixteen bytes an element, as
//! `tools/addrlib/equation` reads them off Mesa's addrlib under the console's configuration (family
//! NV, revision 0x82, `GB_ADDR_CONFIG` 0x4) and checks them against addrlib's own address for every
//! texel of two surfaces. addrlib refuses `64KB_Z_X` at sixteen bytes, so it is absent. An element
//! is a texel, or a block-compressed format's block.
//!
//! The `_X` modes then exclusive-or the surface's pipe-bank XOR into each block's offsets, shifted
//! by the 256-byte pipe interleave and kept within the block (`gfx10addrlib.cpp:4785-4787,4816`,
//! `ComputeSurfaceAddrFromCoordMacroTiled`); the others take none.

use crate::registers::SwizzleMode;

/// One swizzle mode's equation at one element size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Equation {
    /// The mode.
    pub mode: SwizzleMode,
    /// Bytes an element.
    pub bytes: u32,
    /// A block's extent in elements, across and down.
    pub block: (u32, u32),
    /// A block's bytes.
    pub block_bytes: u32,
    /// The offset bits each bit of `x` within a block flips, the lowest first.
    pub x: &'static [u32],
    /// The offset bits each bit of `y` within a block flips.
    pub y: &'static [u32],
    /// The offset bits the lowest three bits of the block's column flip.
    pub x_above: [u32; 3],
    /// The offset bits the lowest three bits of the block's row flip.
    pub y_above: [u32; 3],
}

const NONE_ABOVE: [u32; 3] = [0; 3];

/// Every equation `tools/addrlib/equation` reads, by mode then element size.
pub const EQUATIONS: [Equation; 34] = [
    Equation {
        mode: SwizzleMode::Tiled64KbRX,
        bytes: 1,
        block: (256, 256),
        block_bytes: 65536,
        x: &[0x1, 0x2, 0x4, 0x140, 0x200, 0x800, 0x2400, 0x8000],
        y: &[0x10, 0x8, 0x20, 0x100, 0x280, 0x400, 0x1800, 0x4000],
        x_above: NONE_ABOVE,
        y_above: NONE_ABOVE,
    },
    Equation {
        mode: SwizzleMode::Tiled64KbRX,
        bytes: 2,
        block: (256, 128),
        block_bytes: 65536,
        x: &[0x2, 0x4, 0x8, 0x180, 0x200, 0x800, 0x2400, 0x8000],
        y: &[0x10, 0x20, 0x40, 0x100, 0x1200, 0x400, 0x4800],
        x_above: NONE_ABOVE,
        y_above: NONE_ABOVE,
    },
    Equation {
        mode: SwizzleMode::Tiled64KbRX,
        bytes: 4,
        block: (128, 128),
        block_bytes: 65536,
        x: &[0x4, 0x8, 0x80, 0x100, 0x2200, 0x800, 0x8400],
        y: &[0x10, 0x20, 0x40, 0x1100, 0x200, 0x400, 0x4800],
        x_above: NONE_ABOVE,
        y_above: NONE_ABOVE,
    },
    Equation {
        mode: SwizzleMode::Tiled64KbRX,
        bytes: 8,
        block: (128, 64),
        block_bytes: 65536,
        x: &[0x8, 0x20, 0x40, 0x2100, 0x200, 0x800, 0x8400],
        y: &[0x10, 0x80, 0x1000, 0x100, 0x4200, 0x400],
        x_above: NONE_ABOVE,
        y_above: [0x800, 0, 0],
    },
    Equation {
        mode: SwizzleMode::Tiled64KbRX,
        bytes: 16,
        block: (64, 64),
        block_bytes: 65536,
        x: &[0x10, 0x40, 0x2000, 0x100, 0x8200, 0x800],
        y: &[0x20, 0x80, 0x1000, 0x4100, 0x200, 0x400],
        x_above: [0x400, 0, 0],
        y_above: [0x800, 0, 0],
    },
    Equation {
        mode: SwizzleMode::Tiled64KbDX,
        bytes: 1,
        block: (256, 256),
        block_bytes: 65536,
        x: &[0x1, 0x2, 0x4, 0x40, 0x200, 0x800, 0x2400, 0x8100],
        y: &[0x10, 0x8, 0x20, 0x80, 0x100, 0x400, 0x1800, 0x4200],
        x_above: NONE_ABOVE,
        y_above: NONE_ABOVE,
    },
    Equation {
        mode: SwizzleMode::Tiled64KbDX,
        bytes: 2,
        block: (256, 128),
        block_bytes: 65536,
        x: &[0x2, 0x4, 0x8, 0x80, 0x200, 0x800, 0x2400, 0x8100],
        y: &[0x10, 0x20, 0x40, 0x100, 0x400, 0x1800, 0x4200],
        x_above: NONE_ABOVE,
        y_above: NONE_ABOVE,
    },
    Equation {
        mode: SwizzleMode::Tiled64KbDX,
        bytes: 4,
        block: (128, 128),
        block_bytes: 65536,
        x: &[0x4, 0x8, 0x80, 0x200, 0x800, 0x2400, 0x8100],
        y: &[0x10, 0x20, 0x40, 0x100, 0x400, 0x1800, 0x4200],
        x_above: NONE_ABOVE,
        y_above: NONE_ABOVE,
    },
    Equation {
        mode: SwizzleMode::Tiled64KbDX,
        bytes: 8,
        block: (128, 64),
        block_bytes: 65536,
        x: &[0x8, 0x20, 0x40, 0x200, 0x800, 0x2400, 0x8100],
        y: &[0x10, 0x80, 0x100, 0x400, 0x1800, 0x4200],
        x_above: NONE_ABOVE,
        y_above: NONE_ABOVE,
    },
    Equation {
        mode: SwizzleMode::Tiled64KbDX,
        bytes: 16,
        block: (64, 64),
        block_bytes: 65536,
        x: &[0x10, 0x40, 0x200, 0x800, 0x2400, 0x8100],
        y: &[0x20, 0x80, 0x100, 0x400, 0x1800, 0x4200],
        x_above: NONE_ABOVE,
        y_above: NONE_ABOVE,
    },
    Equation {
        mode: SwizzleMode::Tiled4KbDX,
        bytes: 1,
        block: (64, 64),
        block_bytes: 4096,
        x: &[0x1, 0x2, 0x4, 0x40, 0x200, 0x800],
        y: &[0x10, 0x8, 0x20, 0x80, 0x100, 0x400],
        x_above: [0x400, 0x100, 0],
        y_above: [0x800, 0x200, 0],
    },
    Equation {
        mode: SwizzleMode::Tiled4KbDX,
        bytes: 2,
        block: (64, 32),
        block_bytes: 4096,
        x: &[0x2, 0x4, 0x8, 0x80, 0x200, 0x800],
        y: &[0x10, 0x20, 0x40, 0x100, 0x400],
        x_above: [0x400, 0x100, 0],
        y_above: [0x800, 0x200, 0],
    },
    Equation {
        mode: SwizzleMode::Tiled4KbDX,
        bytes: 4,
        block: (32, 32),
        block_bytes: 4096,
        x: &[0x4, 0x8, 0x80, 0x200, 0x800],
        y: &[0x10, 0x20, 0x40, 0x100, 0x400],
        x_above: [0x400, 0x100, 0],
        y_above: [0x800, 0x200, 0],
    },
    Equation {
        mode: SwizzleMode::Tiled4KbDX,
        bytes: 8,
        block: (32, 16),
        block_bytes: 4096,
        x: &[0x8, 0x20, 0x40, 0x200, 0x800],
        y: &[0x10, 0x80, 0x100, 0x400],
        x_above: [0x400, 0x100, 0],
        y_above: [0x800, 0x200, 0],
    },
    Equation {
        mode: SwizzleMode::Tiled4KbDX,
        bytes: 16,
        block: (16, 16),
        block_bytes: 4096,
        x: &[0x10, 0x40, 0x200, 0x800],
        y: &[0x20, 0x80, 0x100, 0x400],
        x_above: [0x400, 0x100, 0],
        y_above: [0x800, 0x200, 0],
    },
    Equation {
        mode: SwizzleMode::Tiled64KbS,
        bytes: 1,
        block: (256, 256),
        block_bytes: 65536,
        x: &[0x1, 0x2, 0x4, 0x8, 0x200, 0x800, 0x2000, 0x8000],
        y: &[0x10, 0x20, 0x40, 0x80, 0x100, 0x400, 0x1000, 0x4000],
        x_above: NONE_ABOVE,
        y_above: NONE_ABOVE,
    },
    Equation {
        mode: SwizzleMode::Tiled64KbS,
        bytes: 2,
        block: (256, 128),
        block_bytes: 65536,
        x: &[0x2, 0x4, 0x8, 0x80, 0x200, 0x800, 0x2000, 0x8000],
        y: &[0x10, 0x20, 0x40, 0x100, 0x400, 0x1000, 0x4000],
        x_above: NONE_ABOVE,
        y_above: NONE_ABOVE,
    },
    Equation {
        mode: SwizzleMode::Tiled64KbS,
        bytes: 4,
        block: (128, 128),
        block_bytes: 65536,
        x: &[0x4, 0x8, 0x80, 0x200, 0x800, 0x2000, 0x8000],
        y: &[0x10, 0x20, 0x40, 0x100, 0x400, 0x1000, 0x4000],
        x_above: NONE_ABOVE,
        y_above: NONE_ABOVE,
    },
    Equation {
        mode: SwizzleMode::Tiled64KbS,
        bytes: 8,
        block: (128, 64),
        block_bytes: 65536,
        x: &[0x8, 0x40, 0x80, 0x200, 0x800, 0x2000, 0x8000],
        y: &[0x10, 0x20, 0x100, 0x400, 0x1000, 0x4000],
        x_above: NONE_ABOVE,
        y_above: NONE_ABOVE,
    },
    Equation {
        mode: SwizzleMode::Tiled64KbS,
        bytes: 16,
        block: (64, 64),
        block_bytes: 65536,
        x: &[0x40, 0x80, 0x200, 0x800, 0x2000, 0x8000],
        y: &[0x10, 0x20, 0x100, 0x400, 0x1000, 0x4000],
        x_above: NONE_ABOVE,
        y_above: NONE_ABOVE,
    },
    Equation {
        mode: SwizzleMode::Tiled4KbS,
        bytes: 1,
        block: (64, 64),
        block_bytes: 4096,
        x: &[0x1, 0x2, 0x4, 0x8, 0x200, 0x800],
        y: &[0x10, 0x20, 0x40, 0x80, 0x100, 0x400],
        x_above: NONE_ABOVE,
        y_above: NONE_ABOVE,
    },
    Equation {
        mode: SwizzleMode::Tiled4KbS,
        bytes: 2,
        block: (64, 32),
        block_bytes: 4096,
        x: &[0x2, 0x4, 0x8, 0x80, 0x200, 0x800],
        y: &[0x10, 0x20, 0x40, 0x100, 0x400],
        x_above: NONE_ABOVE,
        y_above: NONE_ABOVE,
    },
    Equation {
        mode: SwizzleMode::Tiled4KbS,
        bytes: 4,
        block: (32, 32),
        block_bytes: 4096,
        x: &[0x4, 0x8, 0x80, 0x200, 0x800],
        y: &[0x10, 0x20, 0x40, 0x100, 0x400],
        x_above: NONE_ABOVE,
        y_above: NONE_ABOVE,
    },
    Equation {
        mode: SwizzleMode::Tiled4KbS,
        bytes: 8,
        block: (32, 16),
        block_bytes: 4096,
        x: &[0x8, 0x40, 0x80, 0x200, 0x800],
        y: &[0x10, 0x20, 0x100, 0x400],
        x_above: NONE_ABOVE,
        y_above: NONE_ABOVE,
    },
    Equation {
        mode: SwizzleMode::Tiled4KbS,
        bytes: 16,
        block: (16, 16),
        block_bytes: 4096,
        x: &[0x40, 0x80, 0x200, 0x800],
        y: &[0x10, 0x20, 0x100, 0x400],
        x_above: NONE_ABOVE,
        y_above: NONE_ABOVE,
    },
    Equation {
        mode: SwizzleMode::Tiled64KbZX,
        bytes: 1,
        block: (256, 256),
        block_bytes: 65536,
        x: &[0x1, 0x4, 0x10, 0x140, 0x200, 0x800, 0x2400, 0x8000],
        y: &[0x2, 0x8, 0x20, 0x100, 0x280, 0x400, 0x1800, 0x4000],
        x_above: NONE_ABOVE,
        y_above: NONE_ABOVE,
    },
    Equation {
        mode: SwizzleMode::Tiled64KbZX,
        bytes: 2,
        block: (256, 128),
        block_bytes: 65536,
        x: &[0x2, 0x8, 0x20, 0x180, 0x200, 0x800, 0x2400, 0x8000],
        y: &[0x4, 0x10, 0x40, 0x100, 0x1200, 0x400, 0x4800],
        x_above: NONE_ABOVE,
        y_above: NONE_ABOVE,
    },
    Equation {
        mode: SwizzleMode::Tiled64KbZX,
        bytes: 4,
        block: (128, 128),
        block_bytes: 65536,
        x: &[0x4, 0x10, 0x40, 0x100, 0x2200, 0x800, 0x8400],
        y: &[0x8, 0x20, 0x80, 0x1100, 0x200, 0x400, 0x4800],
        x_above: NONE_ABOVE,
        y_above: NONE_ABOVE,
    },
    Equation {
        mode: SwizzleMode::Tiled64KbZX,
        bytes: 8,
        block: (128, 64),
        block_bytes: 65536,
        x: &[0x8, 0x20, 0x80, 0x2100, 0x200, 0x800, 0x8400],
        y: &[0x10, 0x40, 0x1000, 0x100, 0x4200, 0x400],
        x_above: NONE_ABOVE,
        y_above: [0x800, 0, 0],
    },
    Equation {
        mode: SwizzleMode::Tiled256BS,
        bytes: 1,
        block: (16, 16),
        block_bytes: 256,
        x: &[0x1, 0x2, 0x4, 0x8],
        y: &[0x10, 0x20, 0x40, 0x80],
        x_above: NONE_ABOVE,
        y_above: NONE_ABOVE,
    },
    Equation {
        mode: SwizzleMode::Tiled256BS,
        bytes: 2,
        block: (16, 8),
        block_bytes: 256,
        x: &[0x2, 0x4, 0x8, 0x80],
        y: &[0x10, 0x20, 0x40],
        x_above: NONE_ABOVE,
        y_above: NONE_ABOVE,
    },
    Equation {
        mode: SwizzleMode::Tiled256BS,
        bytes: 4,
        block: (8, 8),
        block_bytes: 256,
        x: &[0x4, 0x8, 0x80],
        y: &[0x10, 0x20, 0x40],
        x_above: NONE_ABOVE,
        y_above: NONE_ABOVE,
    },
    Equation {
        mode: SwizzleMode::Tiled256BS,
        bytes: 8,
        block: (8, 4),
        block_bytes: 256,
        x: &[0x8, 0x40, 0x80],
        y: &[0x10, 0x20],
        x_above: NONE_ABOVE,
        y_above: NONE_ABOVE,
    },
    Equation {
        mode: SwizzleMode::Tiled256BS,
        bytes: 16,
        block: (4, 4),
        block_bytes: 256,
        x: &[0x40, 0x80],
        y: &[0x10, 0x20],
        x_above: NONE_ABOVE,
        y_above: NONE_ABOVE,
    },
];

/// `mode`'s equation at `bytes` an element, or `None` where addrlib gives none here.
#[must_use]
pub fn equation(mode: SwizzleMode, bytes: u32) -> Option<&'static Equation> {
    EQUATIONS
        .iter()
        .find(|equation| equation.mode == mode && equation.bytes == bytes)
}

/// Whether a mode exclusive-ors the pipe-bank XOR into its offsets: the `_X` modes.
const fn takes_xor(mode: SwizzleMode) -> bool {
    matches!(
        mode,
        SwizzleMode::Tiled64KbRX
            | SwizzleMode::Tiled64KbDX
            | SwizzleMode::Tiled4KbDX
            | SwizzleMode::Tiled64KbZX
    )
}

/// The exclusive-or of the terms whose bits of `value` are set.
fn flipped(terms: &[u32], value: u32) -> u32 {
    terms
        .iter()
        .enumerate()
        .filter(|(bit, _)| value >> bit & 1 == 1)
        .fold(0, |offset, (_, term)| offset ^ term)
}

impl Equation {
    /// Blocks across a surface of `pitch` elements.
    #[must_use]
    pub const fn blocks_across(&self, pitch: u32) -> u32 {
        pitch.div_ceil(self.block.0)
    }

    /// Bytes a `pitch` x `height`-element surface spans: every block it touches, whole.
    #[must_use]
    pub const fn surface_bytes(&self, pitch: u32, height: u32) -> usize {
        self.blocks_across(pitch) as usize
            * height.div_ceil(self.block.1) as usize
            * self.block_bytes as usize
    }

    /// The byte offset of element (`x`, `y`) in a surface of `pitch` elements under pipe-bank XOR
    /// `pipe_bank_xor`; `None` where the mode takes no XOR and one is given.
    #[must_use]
    pub fn offset(&self, (x, y): (u32, u32), pitch: u32, pipe_bank_xor: u32) -> Option<usize> {
        if pipe_bank_xor != 0 && !takes_xor(self.mode) {
            return None;
        }
        let (width, height) = self.block;
        let (column, row) = (x / width, y / height);
        let within = flipped(self.x, x % width)
            ^ flipped(self.y, y % height)
            ^ flipped(&self.x_above, column)
            ^ flipped(&self.y_above, row)
            ^ ((pipe_bank_xor << 8) & (self.block_bytes - 1));
        let block = row as usize * self.blocks_across(pitch) as usize + column as usize;
        Some(block * self.block_bytes as usize + within as usize)
    }

    /// A `width` x `height`-element surface of `pitch` elements read out of `tiled` into row-major
    /// elements; `None` when `tiled` is shorter than the surface or the XOR is refused.
    #[must_use]
    pub fn detile(
        &self,
        tiled: &[u8],
        (width, height): (u32, u32),
        pitch: u32,
        pipe_bank_xor: u32,
    ) -> Option<Vec<u8>> {
        let size = self.bytes as usize;
        let mut out = vec![0; width as usize * height as usize * size];
        let table = self.offset_table((width, height), pitch, pipe_bank_xor)?;
        for y in 0..height as usize {
            for x in 0..width as usize {
                let from = table.at(x, y);
                let to = (y * width as usize + x) * size;
                out[to..to + size].copy_from_slice(tiled.get(from..from + size)?);
            }
        }
        Some(out)
    }

    /// Its offsets over a `width` x `height`-element surface of `pitch` elements as an
    /// [`crate::tiling::OffsetTable`]: [`Self::offset`] is a block's start plus an XOR of terms of
    /// `x`, of `y`, and of the XOR. `None` where the XOR is refused.
    fn offset_table(
        &self,
        (width, height): (u32, u32),
        pitch: u32,
        pipe_bank_xor: u32,
    ) -> Option<crate::tiling::OffsetTable> {
        self.offset((0, 0), pitch, pipe_bank_xor)?;
        Some(crate::tiling::OffsetTable::of(
            (width, height),
            self.block_bytes as usize,
            |x, y| {
                self.offset((x, y), pitch, pipe_bank_xor)
                    .unwrap_or_default()
            },
        ))
    }

    /// Row-major `linear` elements written into `tiled` at their places: [`Self::detile`]
    /// reversed. `None` when either is too short or the XOR is refused.
    pub fn tile(
        &self,
        linear: &[u8],
        tiled: &mut [u8],
        (width, height): (u32, u32),
        (pitch, pipe_bank_xor): (u32, u32),
    ) -> Option<()> {
        let size = self.bytes as usize;
        let table = self.offset_table((width, height), pitch, pipe_bank_xor)?;
        for y in 0..height as usize {
            for x in 0..width as usize {
                let to = table.at(x, y);
                let from = (y * width as usize + x) * size;
                tiled
                    .get_mut(to..to + size)?
                    .copy_from_slice(linear.get(from..from + size)?);
            }
        }
        Some(())
    }
}

#[cfg(test)]
mod tests {
    use super::{EQUATIONS, equation};
    use crate::registers::SwizzleMode;
    use crate::tiling;

    /// Each equation is a basis: every element of a block lands on its own offset, inside the
    /// block, at the element's alignment.
    #[test]
    fn every_equation_places_each_element_once() {
        for found in &EQUATIONS {
            let (width, height) = found.block;
            let mut seen = vec![false; found.block_bytes as usize];
            for y in 0..height {
                for x in 0..width {
                    let at = found.offset((x, y), width, 0).expect("no XOR");
                    assert_eq!(at % found.bytes as usize, 0, "{found:?}");
                    assert!(
                        !seen[at],
                        "{:?} at {} bytes: ({x}, {y})",
                        found.mode, found.bytes
                    );
                    seen[at] = true;
                }
            }
            assert_eq!(width * height * found.bytes, found.block_bytes);
        }
    }

    /// The equations agree with the layouts `tiling.rs` modelled by hand, each of which was checked
    /// against addrlib or hardware when it was written.
    #[test]
    fn the_equations_agree_with_the_hand_modelled_layouts() {
        type Hand = fn(u32, u32, u32) -> usize;
        let cases: [(SwizzleMode, u32, Hand); 16] = [
            (
                SwizzleMode::Tiled64KbRX,
                1,
                tiling::tiled_byte_offset_64kb_rx_bpp1_surface,
            ),
            (
                SwizzleMode::Tiled64KbRX,
                2,
                tiling::tiled_byte_offset_64kb_rx_bpp2_surface,
            ),
            (
                SwizzleMode::Tiled64KbRX,
                4,
                tiling::tiled_byte_offset_64kb_rx_bpp4_surface,
            ),
            (
                SwizzleMode::Tiled64KbRX,
                8,
                tiling::tiled_byte_offset_64kb_rx_bpp8_surface,
            ),
            (
                SwizzleMode::Tiled64KbRX,
                16,
                tiling::tiled_byte_offset_64kb_rx_bpp16_surface,
            ),
            (
                SwizzleMode::Tiled64KbZX,
                2,
                tiling::tiled_byte_offset_64kb_zx_bpp2,
            ),
            (
                SwizzleMode::Tiled64KbZX,
                4,
                tiling::tiled_byte_offset_64kb_zx_bpp4,
            ),
            (
                SwizzleMode::Tiled64KbS,
                4,
                tiling::tiled_byte_offset_64kb_s_bpp4,
            ),
            (
                SwizzleMode::Tiled64KbS,
                8,
                tiling::tiled_byte_offset_64kb_s_bpp8,
            ),
            (
                SwizzleMode::Tiled64KbS,
                16,
                tiling::tiled_byte_offset_64kb_s_bpp16,
            ),
            (
                SwizzleMode::Tiled4KbS,
                1,
                tiling::tiled_byte_offset_4kb_s_bpp1,
            ),
            (
                SwizzleMode::Tiled4KbS,
                2,
                tiling::tiled_byte_offset_4kb_s_bpp2,
            ),
            (
                SwizzleMode::Tiled4KbS,
                16,
                tiling::tiled_byte_offset_4kb_s_bpp16,
            ),
            (
                SwizzleMode::Tiled256BS,
                4,
                tiling::tiled_byte_offset_256b_s_bpp4,
            ),
            (
                SwizzleMode::Tiled256BS,
                8,
                tiling::tiled_byte_offset_256b_s_bpp8,
            ),
            (
                SwizzleMode::Tiled256BS,
                16,
                tiling::tiled_byte_offset_256b_s_bpp16,
            ),
        ];
        for (mode, bytes, hand) in cases {
            let found = equation(mode, bytes).expect("an equation");
            // Three blocks across and two down.
            let pitch = found.block.0 * 3;
            for y in (0..found.block.1 * 2).step_by(3) {
                for x in (0..pitch).step_by(5) {
                    assert_eq!(
                        found.offset((x, y), pitch, 0),
                        Some(hand(x, y, pitch)),
                        "{mode:?} at {bytes} bytes, ({x}, {y})"
                    );
                }
            }
        }
    }

    /// A surface tiled then detiled is the elements it began as, under a pipe-bank XOR; a mode
    /// without one refuses one.
    /// Every equation's offset table places each element where its offset does, over surfaces of
    /// part blocks and several block rows and columns, under a pipe-bank XOR where the mode takes
    /// one.
    #[test]
    fn every_equation_s_table_places_elements_as_its_offsets() {
        for found in &EQUATIONS {
            let (block_width, block_height) = found.block;
            let (width, height) = (2 * block_width + 3, 3 * block_height + 1);
            for xor in [0, 0x1d] {
                let Some(table) = found.offset_table((width, height), width + 5, xor) else {
                    assert!(found.offset((0, 0), width + 5, xor).is_none());
                    continue;
                };
                for y in 0..height {
                    for x in 0..width {
                        assert_eq!(
                            Some(table.at(x as usize, y as usize)),
                            found.offset((x, y), width + 5, xor),
                            "{:?} {} bytes, ({x}, {y}) xor {xor:#x}",
                            found.mode,
                            found.bytes
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn detiling_undoes_tiling() {
        let found = equation(SwizzleMode::Tiled64KbRX, 8).expect("64KB_R_X at 8 bytes");
        let (width, height) = (200, 70);
        let linear: Vec<u8> = (0..width * height * 8)
            .map(|i| (i * 7 + i / 251) as u8)
            .collect();
        let mut tiled = vec![0; found.surface_bytes(width, height)];
        found
            .tile(&linear, &mut tiled, (width, height), (width, 5))
            .expect("tiled");
        let back = found
            .detile(&tiled, (width, height), width, 5)
            .expect("detiled");
        assert_eq!(back, linear);
        let standard = equation(SwizzleMode::Tiled64KbS, 4).expect("64KB_S");
        assert_eq!(standard.offset((0, 0), 128, 1), None, "64KB_S takes no XOR");
    }
}
