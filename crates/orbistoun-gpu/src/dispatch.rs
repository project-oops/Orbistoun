//! A compute dispatch, read out of the registers a stream set before it.
//!
//! `DISPATCH_DIRECT` names only the grid and the dispatch initiator; the program, its user data and
//! its thread group are compute shader registers the stream wrote earlier (Mesa
//! `src/amd/registers/gfx10.json`: `COMPUTE_PGM_LO` at byte `0xB830`, `COMPUTE_PGM_RSRC2` at
//! `0xB84C`, `COMPUTE_NUM_THREAD_X` at `0xB81C`, `COMPUTE_USER_DATA_0` at `0xB900`; a register's
//! index here is its byte address over four). A dispatch is carried out only when every one of
//! them it depends on is known and names something modelled exactly; anything else is a refusal
//! naming what is missing.

use orbistoun_shader::{Decode, EncodingTable, Operand};
use orbistoun_translate::Width;
use orbistoun_translate::wavefront::{ComputeInputs, PartialGroups, Window};

use crate::registers::RegisterWrite;

/// `COMPUTE_START_X`; `_Y` and `_Z` follow.
const START_X: u32 = 0xB810 / 4;
/// `COMPUTE_NUM_THREAD_X`; `_Y` and `_Z` follow.
const NUM_THREAD_X: u32 = 0xB81C / 4;
/// `COMPUTE_PGM_LO`: the program address in 256-byte units.
const PGM_LO: u32 = 0xB830 / 4;
/// `COMPUTE_PGM_HI`: bits 39:32 of the program address over 256, in `DATA` bits 7:0.
const PGM_HI: u32 = 0xB834 / 4;
/// `COMPUTE_PGM_RSRC1`.
const PGM_RSRC1: u32 = 0xB848 / 4;
/// `COMPUTE_PGM_RSRC2`.
const PGM_RSRC2: u32 = 0xB84C / 4;
/// `COMPUTE_USER_DATA_0`; sixteen follow.
const USER_DATA_0: u32 = 0xB900 / 4;
/// User-data registers a compute wave can take.
const USER_DATA_WORDS: u32 = 16;

/// `COMPUTE_DISPATCH_INITIATOR` fields (`gfx10.json`).
mod initiator {
    pub(super) const COMPUTE_SHADER_EN: u32 = 1 << 0;
    pub(super) const PARTIAL_TG_EN: u32 = 1 << 1;
    pub(super) const FORCE_START_AT_000: u32 = 1 << 2;
    pub(super) const ORDERED_APPEND_ENBL: u32 = 1 << 3;
    pub(super) const USE_THREAD_DIMENSIONS: u32 = 1 << 5;
    pub(super) const CS_W32_EN: u32 = 1 << 15;
}

/// `COMPUTE_PGM_RSRC2` fields (`gfx10.json`).
mod rsrc2 {
    pub(super) const SCRATCH_EN: u32 = 1 << 0;
    pub(super) const USER_SGPR_SHIFT: u32 = 1;
    pub(super) const USER_SGPR_MASK: u32 = 0x1F;
    pub(super) const TRAP_PRESENT: u32 = 1 << 6;
    pub(super) const TGID_X_EN: u32 = 7;
    pub(super) const TG_SIZE_EN: u32 = 1 << 10;
    pub(super) const TIDIG_COMP_CNT_SHIFT: u32 = 11;
    pub(super) const EXCP_EN_MSB: u32 = 0x3 << 13;
    pub(super) const EXCP_EN: u32 = 0x7F << 24;
}

/// `COMPUTE_PGM_RSRC1.DX10_CLAMP`, bit 21.
const DX10_CLAMP: u32 = 1 << 21;

/// Everything a dispatch runs with, read from the stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchState {
    /// Where the program is.
    pub program: u64,
    /// The user-data words the wave starts with, `None` for one the stream never wrote.
    pub user_data: Vec<Option<u32>>,
    /// The workgroup and thread ids the wave starts with, and its group's shape.
    pub inputs: ComputeInputs,
    /// The wave width, from `CS_W32_EN`.
    pub width: Width,
    /// `DX10_CLAMP`, when `COMPUTE_PGM_RSRC1` was written.
    pub dx10_clamp: Option<bool>,
    /// How many groups run, in each dimension.
    pub groups: [u32; 3],
}

/// Refuses an initiator asking for what is not modelled, and a start that is not zero.
fn check_initiator(
    initiator_word: u32,
    latest: &impl Fn(u32) -> Option<u32>,
) -> Result<(), &'static str> {
    if initiator_word & initiator::COMPUTE_SHADER_EN == 0 {
        return Err("the dispatch initiator does not enable the compute shader");
    }
    for (bit, why) in [
        (
            initiator::USE_THREAD_DIMENSIONS,
            "a grid given in threads (USE_THREAD_DIMENSIONS) is not modelled",
        ),
        (
            initiator::ORDERED_APPEND_ENBL,
            "ordered append (ORDERED_APPEND_ENBL) is not modelled",
        ),
    ] {
        if initiator_word & bit != 0 {
            return Err(why);
        }
    }
    if initiator_word & initiator::FORCE_START_AT_000 == 0 {
        for register in START_X..START_X + 3 {
            match latest(register) {
                Some(0) => {}
                Some(_) => return Err("a dispatch starting at a non-zero group is not modelled"),
                None => {
                    return Err(concat!(
                        "the start is not forced to zero and COMPUTE_START_X/Y/Z were never ",
                        "written, so where the grid starts is unknown"
                    ));
                }
            }
        }
    }
    Ok(())
}

/// Reads the dispatch at `packet_offset` from the writes before it.
///
/// # Errors
///
/// A register the dispatch depends on that the stream never wrote, or a field asking for something
/// not modelled: a partial group, thread dimensions in place of groups, ordered append, scratch
/// memory, a trap handler or exceptions, the group-size register, or a start that is not zero.
pub fn state_at(
    writes: &[RegisterWrite],
    packet_offset: u32,
    groups: [u32; 3],
    initiator_word: u32,
) -> Result<DispatchState, &'static str> {
    let latest = |register: u32| {
        writes
            .iter()
            .rev()
            .find(|w| w.packet_offset < packet_offset && w.register == register)
            .map(|w| w.value)
    };
    check_initiator(initiator_word, &latest)?;

    let (Some(low), Some(high)) = (latest(PGM_LO), latest(PGM_HI)) else {
        return Err("COMPUTE_PGM_LO/HI were never written, so there is no program");
    };
    let program = (u64::from(low) << 8) | (u64::from(high & 0xFF) << 40);
    let Some(rsrc2_word) = latest(PGM_RSRC2) else {
        return Err("COMPUTE_PGM_RSRC2 was never written, so the wave's inputs are unknown");
    };
    for (mask, why) in [
        (
            rsrc2::SCRATCH_EN,
            "scratch memory (SCRATCH_EN) is not modelled",
        ),
        (
            rsrc2::TRAP_PRESENT,
            "a trap handler (TRAP_PRESENT) is not modelled",
        ),
        (
            rsrc2::TG_SIZE_EN,
            "the thread group size register (TG_SIZE_EN) is not modelled",
        ),
        (
            rsrc2::EXCP_EN | rsrc2::EXCP_EN_MSB,
            "shader exceptions (EXCP_EN) are not modelled",
        ),
    ] {
        if rsrc2_word & mask != 0 {
            return Err(why);
        }
    }
    let user_sgprs = (rsrc2_word >> rsrc2::USER_SGPR_SHIFT) & rsrc2::USER_SGPR_MASK;
    if user_sgprs > USER_DATA_WORDS {
        return Err("more user SGPRs than there are compute user-data registers");
    }
    let workgroup_ids = [0, 1, 2].map(|i| rsrc2_word >> (rsrc2::TGID_X_EN + i) & 1 != 0);
    let thread_id_components = ((rsrc2_word >> rsrc2::TIDIG_COMP_CNT_SHIFT) & 0x3) + 1;

    let mut threads = [0_u32; 3];
    let mut partial_threads = [0_u32; 3];
    for (i, (slot, partial)) in threads.iter_mut().zip(&mut partial_threads).enumerate() {
        let register = NUM_THREAD_X + u32::try_from(i).unwrap_or(0);
        let Some(word) = latest(register) else {
            return Err(
                "COMPUTE_NUM_THREAD_X/Y/Z were never written, so the group's shape is unknown",
            );
        };
        // `NUM_THREAD_FULL`, bits 15:0, and `NUM_THREAD_PARTIAL`, bits 31:16, which the last group
        // along the dimension runs under PARTIAL_TG_EN.
        *slot = word & 0xFFFF;
        *partial = word >> 16;
    }
    let partial = if initiator_word & initiator::PARTIAL_TG_EN == 0 {
        None
    } else {
        // radv and radeonsi write a whole group's count where the grid divides evenly, never zero
        // (`radv_cmd_buffer.c:14779-14785`); a group of zero threads, or more than a whole one,
        // is not one a dispatch asks for.
        if partial_threads
            .iter()
            .zip(&threads)
            .any(|(&partial, &full)| partial == 0 || partial > full)
        {
            return Err("a partial thread group of none, or more than a whole group's, threads");
        }
        Some(PartialGroups {
            last: groups.map(|count| count.saturating_sub(1)),
            threads: partial_threads,
        })
    };

    let user_data: Vec<Option<u32>> = (0..user_sgprs).map(|i| latest(USER_DATA_0 + i)).collect();
    // A word never written is passed as zero and the translator refuses a program that reads it.
    let unwritten_user_data = user_data
        .iter()
        .enumerate()
        .filter(|(_, word)| word.is_none())
        .fold(0, |mask, (i, _)| mask | 1 << i);
    Ok(DispatchState {
        program,
        user_data,
        inputs: ComputeInputs {
            workgroup_ids,
            thread_id_components,
            threads,
            unwritten_user_data,
            partial,
        },
        width: if initiator_word & initiator::CS_W32_EN != 0 {
            Width::Wave32
        } else {
            Width::Wave64
        },
        dx10_clamp: latest(PGM_RSRC1).map(|word| word & DX10_CLAMP != 0),
        groups,
    })
}

/// The guest memory a buffer descriptor names, as `(first byte, bytes)`.
fn descriptor_span(words: [u32; 4]) -> (u64, u64) {
    let base = u64::from(words[0]) | (u64::from(words[1] & 0xFFFF) << 32);
    let stride = u64::from((words[1] >> 16) & 0x3FFF);
    let records = u64::from(words[2]);
    // `OOB_SELECT` 3 (raw) counts records in bytes; the structured modes count them in strides
    // (`gfx10-rsrc.json`, `SQ_BUF_RSRC_WORD3`).
    let raw = (words[3] >> 28) & 0x3 == 3;
    let bytes = if stride == 0 || raw {
        records
    } else {
        records * stride
    };
    (base, bytes)
}

/// The largest window a module can address.
const MOST_WINDOW_WORDS: u32 = 1 << 16;
/// The granularity a window base moves by when the natural one is not wholly readable: the
/// guest page.
const PAGE: u64 = 0x4000;

/// Places a dispatch's window over every buffer its program addresses, or answers `None` for a
/// program that addresses none.
///
/// Every untyped buffer access names its descriptor's scalar registers; each must be user data the
/// stream wrote, since a descriptor loaded from memory is not known before the program runs. The
/// window is the smallest power of two words covering them all, placed at the lowest base whose
/// whole span `readable` vouches for and that stays inside one 4 GiB span.
///
/// # Errors
///
/// A buffer access whose descriptor is not in known user data, buffers
/// spanning more than a window holds, or no readable placement.
pub fn place_window(
    state: &DispatchState,
    decode: &Decode,
    encodings: &EncodingTable,
    readable: impl Fn(u64, u64) -> bool,
) -> Result<Option<Window>, &'static str> {
    let families = encodings.encodings();
    let mut span: Option<(u64, u64)> = None;
    for instruction in &decode.instructions {
        let is_buffer = instruction
            .encoding
            .and_then(|e| families.get(usize::from(e)))
            .and_then(|family| encodings.mnemonic_for(&family.name, instruction.opcode))
            .is_some_and(|name| name.starts_with("buffer_"));
        if !is_buffer {
            continue;
        }
        let Some(Operand::Scalar(first)) = instruction.operands.get(2) else {
            return Err("a buffer access whose descriptor is not in scalar registers");
        };
        let first = usize::from(*first);
        let words = state
            .user_data
            .get(first..first + 4)
            .and_then(|words| words.iter().copied().collect::<Option<Vec<u32>>>())
            .ok_or(concat!(
                "a buffer access whose descriptor is not in user data the stream wrote, so ",
                "where its memory is is not known before the program runs"
            ))?;
        let (base, bytes) = descriptor_span([words[0], words[1], words[2], words[3]]);
        let end = base.saturating_add(bytes);
        span = Some(match span {
            None => (base, end),
            Some((low, high)) => (low.min(base), high.max(end)),
        });
    }
    // A program that addresses no buffer - an image copy - has no window to place.
    let Some((low, high)) = span else {
        return Ok(None);
    };
    let needed_words = (high - low).div_ceil(4).max(1);
    if needed_words > u64::from(MOST_WINDOW_WORDS) {
        return Err("the program's buffers span more guest memory than one window addresses");
    }
    let words = u32::try_from(needed_words.next_power_of_two()).unwrap_or(MOST_WINDOW_WORDS);
    let length = u64::from(words) * 4;
    // From the lowest buffer's own base, then down a page at a time while the window still
    // reaches the highest.
    let mut base = low & !3;
    loop {
        if base + length >= high
            && readable(base, length)
            && let Some(window) = Window::spanning_address(base, words)
        {
            return Ok(Some(window));
        }
        if base < PAGE || base - PAGE + length < high {
            return Err(concat!(
                "no placement of the window over the program's buffers is wholly readable ",
                "guest memory"
            ));
        }
        base = (base - PAGE) & !(PAGE - 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(register: u32, value: u32) -> RegisterWrite {
        RegisterWrite {
            packet_offset: 0,
            register,
            value,
        }
    }

    /// The registers radeonsi's compute clear sets before its dispatch, as the port's first
    /// command buffer carries them.
    fn clear_writes() -> Vec<RegisterWrite> {
        let mut writes = vec![
            write(PGM_HI, 0),
            write(PGM_LO, 0x0401_a000),
            write(PGM_RSRC1, 0x002c_0001),
            write(PGM_RSRC2, 0x98),
            write(USER_DATA_0, 0x01c0_0000),
            write(USER_DATA_0 + 2, 0x01c0_0000),
            write(USER_DATA_0 + 3, 0),
            write(NUM_THREAD_X, 0x40),
            write(NUM_THREAD_X + 1, 1),
            write(NUM_THREAD_X + 2, 1),
        ];
        for (i, word) in [
            0xffff_f3ff,
            0xffff_f3ff,
            0xffff_f3ff,
            0xffff_f3ff,
            0x0187_0000,
            4,
            0x0003_0000,
            0x3101_6fac,
        ]
        .into_iter()
        .enumerate()
        {
            writes.push(write(USER_DATA_0 + 4 + i as u32, word));
        }
        writes
    }

    /// The clear's dispatch reads as radeonsi set it up: twelve user SGPRs with word one never
    /// written, the x workgroup id after them, one thread id component, 64 threads, wave64.
    #[test]
    fn the_clear_dispatch_is_read_from_its_registers() {
        let state = state_at(&clear_writes(), 0x100, [0xc0, 1, 1], 0x45).expect("readable");
        assert_eq!(state.program, 0x4_01a0_0000);
        assert_eq!(state.user_data.len(), 12);
        assert_eq!(state.user_data[1], None);
        assert_eq!(state.user_data[8], Some(0x0187_0000));
        assert_eq!(
            state.inputs,
            ComputeInputs {
                workgroup_ids: [true, false, false],
                thread_id_components: 1,
                threads: [64, 1, 1],
                unwritten_user_data: 1 << 1,
                partial: None,
            }
        );
        assert_eq!(state.width, Width::Wave64);
        assert_eq!(state.dx10_clamp, Some(true));
    }

    /// What is not modelled is refused by name; a write after the dispatch is not its state.
    #[test]
    fn unmodelled_fields_and_missing_registers_are_refused() {
        let writes = clear_writes();
        assert!(
            state_at(&writes, 0x100, [1, 1, 1], 0x44).is_err(),
            "no shader"
        );
        assert!(
            state_at(&writes, 0x100, [1, 1, 1], 0x47).is_err(),
            "partial"
        );
        assert!(
            state_at(&writes, 0x100, [1, 1, 1], 0x41).is_err(),
            "start unknown"
        );
        let mut scratch = writes.clone();
        scratch.push(write(PGM_RSRC2, 0x99));
        assert!(
            state_at(&scratch, 0x100, [1, 1, 1], 0x45).is_err(),
            "scratch"
        );
        let mut late = writes;
        for w in &mut late {
            if w.register == PGM_LO {
                w.packet_offset = 0x200;
            }
        }
        assert!(
            state_at(&late, 0x100, [1, 1, 1], 0x45).is_err(),
            "program written after"
        );
    }

    /// The clear's program, as the port's first command buffer carries it.
    fn clear_program() -> (Decode, EncodingTable) {
        let encodings = EncodingTable::builtin().expect("encodings");
        let operands = orbistoun_shader::OperandTable::builtin().expect("operands");
        let words: [u32; 11] = [
            0xd746_0000,
            0x0401_0c0c,
            0xd700_0004,
            0x0000_0880,
            0xd700_0006,
            0x0000_0c80,
            0x3400_0084,
            0xe078_1000,
            0x8002_0400,
            0xbf81_0000,
            0xbf9f_0000,
        ];
        let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        let decode = orbistoun_shader::decode_program(&bytes, &encodings, &operands);
        assert!(decode.terminated && decode.is_trustworthy());
        (decode, encodings)
    }

    /// The window covers the clear's 192 KiB buffer in a 256 KiB span, moved down a page at a time
    /// until it lies wholly inside the readable mapping the buffer ends at.
    #[test]
    fn the_window_covers_the_buffers_inside_readable_memory() {
        let (decode, encodings) = clear_program();
        let state = state_at(&clear_writes(), 0x100, [0xc0, 1, 1], 0x45).expect("readable");
        let (start, end) = (0x4_0100_0000_u64, 0x4_018a_0000_u64);
        let window = place_window(&state, &decode, &encodings, |at, len| {
            at >= start && at + len <= end
        })
        .expect("placed")
        .expect("a window over its buffers");
        assert_eq!((window.address(), window.words()), (0x4_0186_0000, 1 << 16));

        assert!(
            place_window(&state, &decode, &encodings, |_, _| false).is_err(),
            "nowhere readable"
        );
        let mut unknown = state;
        unknown.user_data[8] = None;
        assert!(place_window(&unknown, &decode, &encodings, |_, _| true).is_err());
    }
}
