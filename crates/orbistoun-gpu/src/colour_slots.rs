//! Which colour buffer each of a pixel shader's colour exports lands in, and each colour buffer's
//! registers as the decoders written for colour buffer zero read them.
//!
//! A pixel shader's `exp mrtK` writes the `K`th colour buffer whose `SPI_SHADER_COL_FORMAT` field
//! is not `SPI_SHADER_ZERO`: the exports are compacted, a buffer with no format taking no export
//! number. ACO assigns them that way (`aco_select_ps_epilog.cpp:88-91`, a `ZERO` slot exports
//! nothing; `:428-447`, each export's target is `MRT` plus the count of exports before it), and
//! the hardware reads them back the same way.
//!
//! Colour buffer `n`'s registers lie on a fixed stride from colour buffer zero's (`gfx103.json`):
//! `CB_COLORn_BASE`..`DCC_BASE` fifteen dwords on from `CB_COLOR0_BASE` (`0xA318`, `0xA327`,
//! ..., `0xA381`), each `*_EXT`, `ATTRIB2` and `ATTRIB3` one dword on from colour buffer zero's,
//! `CB_BLENDn_CONTROL` one on from `CB_BLEND0_CONTROL` (`0xA1E0`), and `CB_TARGET_MASK` and
//! `CB_SHADER_MASK` four bits a buffer.

use std::borrow::Cow;

use crate::registers::RegisterWrite;

/// Colour buffers a draw can bind.
pub const COLOUR_BUFFERS: u32 = 8;

/// `SPI_SHADER_COL_FORMAT` (`gfx103.json`, byte `0x28714`, dword `0xA1C5`): four bits a colour
/// buffer, buffer zero lowest, `SPI_SHADER_ZERO` (0) for one the shader exports nothing to.
pub const SPI_SHADER_COL_FORMAT: u32 = 0xA1C5;

/// `CB_COLOR0_BASE`, the first of colour buffer zero's fifteen-dword block (`gfx103.json`).
const MAIN_BLOCK: u32 = 0xA318;
/// The stride between colour buffers' blocks: `CB_COLOR1_BASE` is `0xA327`.
const MAIN_BLOCK_STRIDE: u32 = 15;
/// Colour buffer zero's one-dword-a-buffer registers: `BASE_EXT`, `CMASK_BASE_EXT`,
/// `FMASK_BASE_EXT`, `DCC_BASE_EXT`, `ATTRIB2` and `ATTRIB3` (`gfx103.json`).
const PER_BUFFER: [u32; 6] = [0xA390, 0xA398, 0xA3A0, 0xA3A8, 0xA3B0, 0xA3B8];
/// `CB_BLEND0_CONTROL`, followed by colour buffers one to seven's.
const BLEND_CONTROL: u32 = 0xA1E0;
/// `CB_TARGET_MASK` and `CB_SHADER_MASK`: four bits a colour buffer.
const NIBBLE_MASKS: [u32; 2] = [0xA08E, 0xA08F];

/// The colour buffers a pixel shader's exports land in, in export order: element `K` is the buffer
/// `exp mrtK` writes. `None` when the stream never set `SPI_SHADER_COL_FORMAT`: which buffer an
/// export reaches is then not known, and is not assumed.
#[must_use]
pub fn exported_colour_buffers(writes: &[RegisterWrite]) -> Option<Vec<u32>> {
    let format = writes
        .iter()
        .rev()
        .find(|write| write.register == SPI_SHADER_COL_FORMAT)?
        .value;
    Some(buffers_of(format))
}

/// The buffers `format`'s non-zero fields name, lowest first.
#[must_use]
pub fn buffers_of(format: u32) -> Vec<u32> {
    (0..COLOUR_BUFFERS)
        .filter(|buffer| (format >> (buffer * 4)) & 0xF != 0)
        .collect()
}

/// Which colour-buffer register `register` is, as colour buffer zero's, and whose: `None` for a
/// register that is no one buffer's.
fn as_buffer_zero(register: u32) -> Option<(u32, u32)> {
    if (MAIN_BLOCK..MAIN_BLOCK + MAIN_BLOCK_STRIDE * COLOUR_BUFFERS).contains(&register) {
        let offset = register - MAIN_BLOCK;
        return Some((
            offset / MAIN_BLOCK_STRIDE,
            MAIN_BLOCK + offset % MAIN_BLOCK_STRIDE,
        ));
    }
    PER_BUFFER
        .into_iter()
        .chain([BLEND_CONTROL])
        .find(|&first| (first..first + COLOUR_BUFFERS).contains(&register))
        .map(|first| (register - first, first))
}

/// Colour buffer `buffer`'s register at the place colour buffer zero's `register` has: the same
/// register again for one that is no one buffer's.
#[must_use]
pub const fn of_buffer(register: u32, buffer: u32) -> u32 {
    if register >= MAIN_BLOCK && register < MAIN_BLOCK + MAIN_BLOCK_STRIDE {
        return register + MAIN_BLOCK_STRIDE * buffer;
    }
    let mut first = 0;
    while first < PER_BUFFER.len() {
        if register == PER_BUFFER[first] {
            return register + buffer;
        }
        first += 1;
    }
    if register == BLEND_CONTROL {
        return register + buffer;
    }
    register
}

/// Colour buffer `buffer`'s value of colour buffer zero's `register`, from `latest`, which reads a
/// register as the stream stands: its own register's, or its four bits of `CB_TARGET_MASK` or
/// `CB_SHADER_MASK` moved lowest. A register that is no one buffer's reads as it is.
pub fn read_as_buffer(
    latest: &mut impl FnMut(u32) -> Option<u32>,
    register: u32,
    buffer: u32,
) -> Option<u32> {
    if NIBBLE_MASKS.contains(&register) {
        return latest(register).map(|value| value >> (buffer * 4));
    }
    latest(of_buffer(register, buffer))
}

/// The writes as the decoders written for colour buffer zero read colour buffer `buffer`'s: its
/// registers moved onto buffer zero's, buffer zero's own dropped, so a register `buffer` never
/// had written reads as unwritten rather than as buffer zero's, and `CB_TARGET_MASK` and
/// `CB_SHADER_MASK` shifted so its four bits are the lowest. Every other write is kept, in order.
#[must_use]
pub fn colour_buffer_writes(writes: &[RegisterWrite], buffer: u32) -> Cow<'_, [RegisterWrite]> {
    if buffer == 0 {
        return Cow::Borrowed(writes);
    }
    Cow::Owned(
        writes
            .iter()
            .filter_map(|write| match as_buffer_zero(write.register) {
                Some((owner, zero)) if owner == buffer => Some(RegisterWrite {
                    register: zero,
                    ..*write
                }),
                Some(_) => None,
                None if NIBBLE_MASKS.contains(&write.register) => Some(RegisterWrite {
                    value: write.value >> (buffer * 4),
                    ..*write
                }),
                None => Some(*write),
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::{
        buffers_of, colour_buffer_writes, exported_colour_buffers, of_buffer, read_as_buffer,
    };
    use crate::registers::RegisterWrite;

    fn write(register: u32, value: u32) -> RegisterWrite {
        RegisterWrite {
            packet_offset: 0,
            register,
            value,
        }
    }

    /// Exports are compacted past a buffer with no format: with buffer zero unformatted, `mrt0`
    /// reaches buffer one.
    #[test]
    fn exports_skip_a_buffer_with_no_format() {
        assert_eq!(buffers_of(0x0000_0044), vec![0, 1]);
        assert_eq!(buffers_of(0x0000_0040), vec![1]);
        assert_eq!(buffers_of(0x4000_0004), vec![0, 7]);
        assert_eq!(buffers_of(0), Vec::<u32>::new());
        assert_eq!(exported_colour_buffers(&[]), None);
        assert_eq!(
            exported_colour_buffers(&[write(0xA1C5, 4), write(0xA1C5, 0x44)]),
            Some(vec![0, 1]),
            "the latest write"
        );
    }

    /// Buffer one's registers read as buffer zero's, its blend and masks included; buffer zero's
    /// own are gone, and other registers pass through.
    #[test]
    fn a_buffer_reads_as_buffer_zero() {
        let writes = [
            write(0xA318, 0x100), // CB_COLOR0_BASE
            write(0xA31B, 0x7),   // CB_COLOR0_VIEW
            write(0xA327, 0x200), // CB_COLOR1_BASE
            write(0xA32B, 0x55),  // CB_COLOR1_INFO
            write(0xA3B1, 0x99),  // CB_COLOR1_ATTRIB2
            write(0xA3B9, 0x77),  // CB_COLOR1_ATTRIB3
            write(0xA391, 0x1),   // CB_COLOR1_BASE_EXT
            write(0xA1E1, 0x33),  // CB_BLEND1_CONTROL
            write(0xA1E0, 0x22),  // CB_BLEND0_CONTROL
            write(0xA336, 0x300), // CB_COLOR2_BASE
            write(0xA08E, 0xF3),  // CB_TARGET_MASK
            write(0xA1C5, 0x44),  // SPI_SHADER_COL_FORMAT
        ];
        let one = colour_buffer_writes(&writes, 1);
        let got: Vec<(u32, u32)> = one.iter().map(|w| (w.register, w.value)).collect();
        assert_eq!(
            got,
            vec![
                (0xA318, 0x200),
                (0xA31C, 0x55),
                (0xA3B0, 0x99),
                (0xA3B8, 0x77),
                (0xA390, 0x1),
                (0xA1E0, 0x33),
                (0xA08E, 0xF),
                (0xA1C5, 0x44),
            ]
        );
        assert!(matches!(
            colour_buffer_writes(&writes, 0),
            std::borrow::Cow::Borrowed(_)
        ));
        let seven: Vec<u32> = colour_buffer_writes(&[write(0xA38E, 1), write(0xA3BF, 2)], 7)
            .iter()
            .map(|w| w.register)
            .collect();
        assert_eq!(
            seven,
            vec![0xA325, 0xA3B8],
            "buffer seven's DCC_BASE and ATTRIB3"
        );
    }

    /// Buffer zero's registers name buffer `n`'s by the same strides, and a mask's four bits move.
    #[test]
    fn a_register_of_buffer_zero_names_another_buffer_s() {
        assert_eq!(of_buffer(0xA31C, 1), 0xA32B, "CB_COLOR1_INFO");
        assert_eq!(of_buffer(0xA318, 7), 0xA381, "CB_COLOR7_BASE");
        assert_eq!(of_buffer(0xA3B0, 2), 0xA3B2, "CB_COLOR2_ATTRIB2");
        assert_eq!(of_buffer(0xA1E0, 3), 0xA1E3, "CB_BLEND3_CONTROL");
        assert_eq!(of_buffer(0xA105, 3), 0xA105, "CB_BLEND_RED is everyone's");
        let mut latest = |register| (register == 0xA08E).then_some(0x0000_00F3);
        assert_eq!(read_as_buffer(&mut latest, 0xA08E, 1), Some(0xF));
        assert_eq!(read_as_buffer(&mut latest, 0xA08E, 0), Some(0xF3));
    }
}
