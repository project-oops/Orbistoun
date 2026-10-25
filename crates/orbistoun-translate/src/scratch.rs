//! A compute wave's private memory, scratch (D954).
//!
//! A program given scratch (`COMPUTE_PGM_RSRC2.SCRATCH_EN`) holds `COMPUTE_TMPRING_SIZE.WAVESIZE`
//! bytes a wave, and finds its wave's offset into the scratch ring in the scalar register after its
//! workgroup ids (Mesa `src/amd/vulkan/radv_shader_args.c:648-663`: the workgroup ids, then the
//! group size, then `scratch_offset`). It reaches that memory two ways:
//!
//! - through a buffer descriptor that adds each lane's id to its index and swizzles (`ADD_TID_ENABLE`
//!   and `SWIZZLE_ENABLE`, an index stride of the wave's width), with the wave's offset as the
//!   access's scalar offset: ACO's MUBUF scratch (`src/amd/compiler/aco_ir.cpp:1945-1958`, the
//!   descriptor; `src/amd/compiler/instruction_selection/aco_select_nir_intrinsics.cpp:3412-3419`,
//!   the access), and LLVM's on this generation;
//! - through the flat encoding's scratch segment, bits 15:14 one (`aco_assembler.cpp:906-907`),
//!   whose address is the lane's own offset into its private space.
//!
//! No other wave and no later dispatch reads a wave's scratch, so where the ring lies in guest memory
//! is never observed. The module holds it as a private array instead, indexed by the access's byte
//! address from the wave's scratch base: the wave's offset register reads zero, as every register a
//! module seeds nothing into does, and a descriptor's base is not read. Where one invocation is the
//! whole wave the array is the wave's scratch as the hardware lays it out; where each invocation is
//! one lane (D760) it holds that lane's own words, and an address in another lane's is an escape.

use std::collections::BTreeSet;

use orbistoun_shader::{Decode, EncodingTable, Instruction, Operand};
use orbistoun_spirv::{Id, op};

use crate::TranslateError;
use crate::model::{Model, SCALAR_REGISTERS};
use crate::wavefront::ComputeInputs;

/// The scratch segment in a flat-encoded access's bits 15:14 (`aco_assembler.cpp:906-907`).
const SCRATCH_SEGMENT: u32 = 1;
/// A flat scratch access's `SADDR` naming neither an address register nor a scalar one: the address
/// is its immediate alone (`aco_assembler.cpp:931-936`).
const SADDR_OFF: u32 = 0x7f;
/// `SADDR` naming `null`: the address is the vector register's.
const SADDR_NULL: u32 = 125;

/// The scalar register a wave's scratch offset arrives in: after `user_sgprs` user-data words and
/// the workgroup ids `inputs` enables. The group-size register, which would come between, is not
/// modelled and refused before (`TG_SIZE_EN`).
#[must_use]
pub fn offset_register(user_sgprs: u32, inputs: &ComputeInputs) -> u32 {
    let ids = inputs
        .workgroup_ids
        .iter()
        .filter(|&&enabled| enabled)
        .count();
    user_sgprs + u32::try_from(ids).unwrap_or(3)
}

/// The family and mnemonic of an instruction, where the tables name it.
fn named<'a>(
    encodings: &'a EncodingTable,
    instruction: &Instruction,
) -> Option<(&'a str, &'a str)> {
    let family = encodings
        .encodings()
        .get(usize::from(instruction.encoding?))?;
    Some((
        family.name.as_str(),
        encodings.mnemonic_for(&family.name, instruction.opcode)?,
    ))
}

/// Whether a flat-encoded access names the scratch segment.
fn scratch_segment(family: &str, instruction: &Instruction) -> bool {
    family == "FLAT" && (instruction.word >> 14) & 3 == SCRATCH_SEGMENT
}

/// Whether a buffer access adds the wave's scratch offset, held in `register`, as its scalar offset:
/// `SOFFSET`, its second word's bits 31:24 (`aco_assembler.cpp:587`).
fn adds_wave_offset(family: &str, name: &str, instruction: &Instruction, register: u32) -> bool {
    family == "MUBUF"
        && (name.starts_with("buffer_load_") || name.starts_with("buffer_store_"))
        && instruction
            .second_word
            .is_some_and(|word| word >> 24 == register)
}

/// The byte offsets of a program's scratch accesses: every flat access to the scratch segment, and,
/// where the wave's scratch offset arrives in `register`, every buffer load or store adding it.
#[must_use]
pub fn accesses(
    decode: &Decode,
    encodings: &EncodingTable,
    register: Option<u32>,
) -> BTreeSet<u32> {
    decode
        .instructions
        .iter()
        .filter(|instruction| {
            named(encodings, instruction).is_some_and(|(family, name)| {
                scratch_segment(family, instruction)
                    || register.is_some_and(|register| {
                        adds_wave_offset(family, name, instruction, register)
                    })
            })
        })
        .map(|instruction| instruction.offset)
        .collect()
}

/// How many scalar registers from its first operand an instruction writes: a scalar load's words,
/// a pair for a 64-bit or lane-mask result, one otherwise.
fn written_words(name: &str) -> u32 {
    if let Some((_, count)) = name.rsplit_once("dwordx") {
        return count.parse().unwrap_or(16);
    }
    let wide = ["_b64", "_u64", "_i64"]
        .iter()
        .any(|kind| name.contains(kind));
    if wide || name.starts_with("v_cmp") {
        2
    } else {
        1
    }
}

/// Refuses a program that writes the register its wave's scratch offset arrives in and has buffer
/// accesses that add it: those would then add something other than the offset the module reads as
/// zero. A program that reaches scratch only through the flat encoding's scratch segment may use
/// the register for anything once it has read it - Grand Theft Auto V's ray traversal passes copy
/// it into `FLAT_SCRATCH` and load a descriptor over it (guest-observed, D964). A register is taken
/// as written wherever an instruction's destination - its first operand, or the carry-out after it
/// - spans it.
///
/// # Errors
///
/// Such a write, at its offset.
pub fn check(
    decode: &Decode,
    encodings: &EncodingTable,
    register: u32,
) -> Result<(), TranslateError> {
    let adds_it = decode.instructions.iter().any(|instruction| {
        named(encodings, instruction)
            .is_some_and(|(family, name)| adds_wave_offset(family, name, instruction, register))
    });
    if !adds_it {
        return Ok(());
    }
    for instruction in &decode.instructions {
        let Some((family, name)) = named(encodings, instruction) else {
            continue;
        };
        // A compare and a program-control instruction write no scalar register; a store's first
        // operand is its data.
        if matches!(family, "SOPC" | "SOPP") || name.starts_with("s_cmpk") || name.contains("store")
        {
            continue;
        }
        let destinations = instruction.operands.iter().take(2).enumerate();
        for (at, operand) in destinations {
            let Operand::Scalar(first) = operand else {
                continue;
            };
            // The second operand is a destination only as a carry-out.
            if at == 1 && !name.contains("_co_") {
                continue;
            }
            let first = u32::from(*first);
            let words = if at == 0 { written_words(name) } else { 2 };
            if (first..first + words).contains(&register) {
                return Err(TranslateError::Unsupported {
                    offset: instruction.offset,
                    detail: "the program writes the register its wave's scratch offset arrives in (D954)",
                });
            }
        }
    }
    Ok(())
}

/// Carries out `instruction` where it is a scratch access, answering whether it was: a flat access
/// to the scratch segment, or a buffer load or store adding the wave's scratch offset.
///
/// # Errors
///
/// A scratch-segment access in a module whose dispatch gives it no scratch, and a scratch access of
/// a form not translated.
pub(crate) fn access<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<bool, TranslateError> {
    let Some((family, _)) = named(model.encodings(), instruction) else {
        return Ok(false);
    };
    let scratch = model.scratch();
    if scratch_segment(family, instruction) {
        if scratch.is_none() {
            return Err(TranslateError::Unsupported {
                offset: instruction.offset,
                detail: "a scratch-segment access in a module whose dispatch gives it no scratch (D878, D954)",
            });
        }
        flat(model, instruction, name)?;
        return Ok(true);
    }
    match scratch {
        Some((_, register)) if adds_wave_offset(family, name, instruction, register) => {
            buffer(model, instruction, name)?;
            Ok(true)
        }
        _ => Ok(false),
    }
}

/// What a scratch access moves: how many dwords, or a sub-dword load's width in bits and whether it
/// sign-extends.
#[derive(Clone, Copy)]
enum Width {
    Dwords(u32),
    Part(u32, bool),
}

fn refuse(instruction: &Instruction, detail: &'static str) -> TranslateError {
    TranslateError::Unsupported {
        offset: instruction.offset,
        detail,
    }
}

/// The lane id an access adds: the invocation's lane where each is one (D760), the module's loop's
/// where one is open over the lanes, else the per-lane loop's.
pub(crate) fn lane_id<M: Model + ?Sized>(model: &mut M, lane: u32) -> Id {
    match model.lane_number(lane).or(model.looped_lane()) {
        Some(lane) => lane,
        None => model.constant(lane),
    }
}

/// A buffer load or store adding the wave's scratch offset (`buffer_load_dword` to `x4`,
/// `buffer_store_dword` to `x4`, `buffer_load_ushort`), its fields from the encoding
/// (`aco_assembler.cpp:561-600`): the immediate in bits 11:0, `offen` 12, `idxen` 13, `lds` 16; the
/// address registers, data register, descriptor's first register over four and scalar offset in the
/// second word's bytes.
///
/// The byte it reaches is AMD's published RDNA 2 instruction set's buffer address less the
/// descriptor's base: the scalar offset, plus, where the descriptor swizzles, with `index` the
/// address register's index plus the lane id and `offset` its offset plus the immediate,
///
/// ```text
/// ((index / IS) * stride + (offset / 4) * 4) * IS + (index % IS) * 4 + offset % 4
/// ```
///
/// for the index stride `IS` (`8 << INDEX_STRIDE`) and the four-byte element GFX9 on fix
/// (`aco_ir.cpp:1953-1954`); or `offset + stride * index` where it does not. A descriptor that does
/// not add the lane id is not scratch's, and its access is an escape.
fn buffer<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    let (loading, width) = buffer_width(instruction, name)?;
    let word = instruction.word;
    let Some(second) = instruction.second_word else {
        return Err(refuse(
            instruction,
            "a buffer access without its second word",
        ));
    };
    if word >> 16 & 1 != 0 || second >> 23 & 1 != 0 {
        return Err(refuse(
            instruction,
            "a scratch buffer access into the local data share or with TFE, which is not translated",
        ));
    }
    let (immediate, offen, idxen) = (word & 0xfff, word >> 12 & 1 != 0, word >> 13 & 1 != 0);
    let (vaddr, data) = (second & 0xff, second >> 8 & 0xff);
    let (resource, soffset) = ((second >> 16 & 0x1f) * 4, second >> 24);
    if resource + 4 > SCALAR_REGISTERS || soffset >= SCALAR_REGISTERS {
        return Err(refuse(
            instruction,
            "a scratch buffer access whose descriptor or offset is not in scalar registers",
        ));
    }
    let dwords = match width {
        Width::Dwords(count) => count,
        Width::Part(..) => 1,
    };
    if data + dwords > 256 || vaddr + u32::from(idxen) + u32::from(offen) > 256 {
        return Err(refuse(
            instruction,
            "a scratch buffer access runs past the end of the vector register file",
        ));
    }
    let (stride, swizzled, stride_shift, adds_lane) = descriptor_fields(model, resource);
    let scalar_offset = model.read_scalar(soffset);
    for lane in crate::model::running_lanes(model) {
        let zero = model.constant(0);
        let mut next = vaddr;
        let mut take = |model: &mut M, on: bool| -> Result<Id, TranslateError> {
            if !on {
                return Ok(zero);
            }
            let register = u16::try_from(next).unwrap_or(u16::MAX);
            next += 1;
            model.read_source(instruction, &Operand::Vector(register), lane)
        };
        let vindex = take(model, idxen)?;
        let voffset = take(model, offen)?;
        let id = lane_id(model, lane);
        let index = model.add(vindex, id);
        let values = if loading {
            Vec::new()
        } else {
            (0..dwords)
                .map(|step| {
                    let register = u16::try_from(data + step).unwrap_or(u16::MAX);
                    model.read_source(instruction, &Operand::Vector(register), lane)
                })
                .collect::<Result<Vec<_>, _>>()?
        };
        for step in 0..dwords {
            let immediate = model.constant(immediate + 4 * step);
            let offset = model.add(voffset, immediate);
            let swizzled_at = swizzled_place(model, (index, offset), (stride, stride_shift));
            let scaled = model.binary(op::IMUL, stride, index);
            let linear_at = model.add(offset, scaled);
            let at = model.select(swizzled, swizzled_at, linear_at);
            let at = model.add(scalar_offset, at);
            let moved = if loading {
                None
            } else {
                Some(values[step as usize])
            };
            if let Some(value) = element(model, (at, adds_lane), (width, moved), lane)? {
                model.write_vector_lane(data + step, lane, value);
            }
        }
    }
    model.count();
    Ok(())
}

/// Whether a scratch buffer access loads, and what it moves.
fn buffer_width(instruction: &Instruction, name: &str) -> Result<(bool, Width), TranslateError> {
    if name == "buffer_load_ushort" {
        return Ok((true, Width::Part(16, false)));
    }
    let dwords = match name.rsplit_once("dword") {
        Some((_, "")) => 1,
        Some((_, "x2")) => 2,
        Some((_, "x3")) => 3,
        Some((_, "x4")) => 4,
        _ => {
            return Err(refuse(
                instruction,
                "a scratch buffer access of a width not translated (D954)",
            ));
        }
    };
    Ok((name.starts_with("buffer_load_"), Width::Dwords(dwords)))
}

/// The scratch descriptor's fields in the four registers from `resource`, the wave's own: its
/// stride (word 1 bits 29:16), whether it swizzles (31), its index stride's power of two (word 3
/// bits 22:21, plus three) and whether it adds the lane id (23) (`gfx10-rsrc.json`). A descriptor
/// the program computes is read so too, as a device-addressed access places it (D967).
pub(crate) fn descriptor_fields<M: Model + ?Sized>(
    model: &mut M,
    resource: u32,
) -> (Id, Id, Id, Id) {
    let second_word = model.read_scalar(resource + 1);
    let fourth_word = model.read_scalar(resource + 3);
    let field = |model: &mut M, value: Id, shift: u32, mask: u32| {
        let shift = model.constant(shift);
        let shifted = model.binary(op::SHIFT_RIGHT_LOGICAL, value, shift);
        let mask = model.constant(mask);
        model.binary(op::BITWISE_AND, shifted, mask)
    };
    let stride = field(model, second_word, 16, 0x3fff);
    let swizzle = field(model, second_word, 31, 1);
    let swizzled = model.is_not_zero(swizzle);
    let code = field(model, fourth_word, 21, 3);
    let three = model.constant(3);
    let stride_shift = model.add(code, three);
    let adds_lane = field(model, fourth_word, 23, 1);
    let adds_lane = model.is_not_zero(adds_lane);
    (stride, swizzled, stride_shift, adds_lane)
}

/// A swizzled buffer address's part past the scalar offset, for `index` and `offset`, with the
/// descriptor's stride and its index stride's power of two.
pub(crate) fn swizzled_place<M: Model + ?Sized>(
    model: &mut M,
    (index, offset): (Id, Id),
    (stride, stride_shift): (Id, Id),
) -> Id {
    let (two, three, one) = (model.constant(2), model.constant(3), model.constant(1));
    let index_high = model.binary(op::SHIFT_RIGHT_LOGICAL, index, stride_shift);
    let records = model.binary(op::IMUL, index_high, stride);
    let elements = model.binary(op::SHIFT_RIGHT_LOGICAL, offset, two);
    let element_bytes = model.binary(op::SHIFT_LEFT_LOGICAL, elements, two);
    let row = model.add(records, element_bytes);
    let rows = model.binary(op::SHIFT_LEFT_LOGICAL, row, stride_shift);
    let index_stride = model.binary(op::SHIFT_LEFT_LOGICAL, one, stride_shift);
    let low_mask = model.binary(op::ISUB, index_stride, one);
    let index_low = model.binary(op::BITWISE_AND, index, low_mask);
    let column = model.binary(op::SHIFT_LEFT_LOGICAL, index_low, two);
    let byte = model.binary(op::BITWISE_AND, offset, three);
    let place = model.add(rows, column);
    model.add(place, byte)
}

/// A flat access to the scratch segment (`global_load_dword` to `x4` and the byte and half loads,
/// `global_store_dword`, `x2` and `x4`, as the decoder names the flat family by opcode, D878), its
/// fields from the encoding (`aco_assembler.cpp:889-938`): the signed twelve-bit immediate in bits
/// 11:0, `lds` 13; the address register, data register, `SADDR` and destination in the second word's
/// bytes. `SADDR` off reads the immediate alone, `null` the address register, a scalar register
/// that register.
///
/// That address is the lane's own offset into its private space, which the hardware interleaves a
/// dword at a time across the wave's lanes; the module lays it out as the buffer form's swizzle with
/// an index stride of the wave's width, so both forms name one byte alike. Only a lane's own words
/// are reachable this way, so the layout is not otherwise observed.
fn flat<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    let (loading, width) = match name {
        "global_load_dword" => (true, Width::Dwords(1)),
        "global_load_dwordx2" => (true, Width::Dwords(2)),
        "global_load_dwordx3" => (true, Width::Dwords(3)),
        "global_load_dwordx4" => (true, Width::Dwords(4)),
        "global_store_dword" => (false, Width::Dwords(1)),
        "global_store_dwordx2" => (false, Width::Dwords(2)),
        "global_store_dwordx4" => (false, Width::Dwords(4)),
        "global_load_ubyte" => (true, Width::Part(8, false)),
        "global_load_sbyte" => (true, Width::Part(8, true)),
        "global_load_ushort" => (true, Width::Part(16, false)),
        "global_load_sshort" => (true, Width::Part(16, true)),
        _ => {
            return Err(refuse(
                instruction,
                "a scratch-segment access of a kind not translated (D954)",
            ));
        }
    };
    let word = instruction.word;
    let Some(second) = instruction.second_word else {
        return Err(refuse(instruction, "a flat access without its second word"));
    };
    if word >> 13 & 1 != 0 {
        return Err(refuse(
            instruction,
            "a scratch-segment access into the local data share, which is not translated",
        ));
    }
    // Sign-extended from bit 11 (`aco_assembler.cpp:903-904`).
    let raised = i32::from_ne_bytes(((word & 0xfff) << 20).to_ne_bytes());
    let immediate = u32::from_ne_bytes((raised >> 20).to_ne_bytes());
    let (vaddr, data, saddr, destination) = (
        second & 0xff,
        second >> 8 & 0xff,
        second >> 16 & 0x7f,
        second >> 24,
    );
    let dwords = match width {
        Width::Dwords(count) => count,
        Width::Part(..) => 1,
    };
    let register = if loading { destination } else { data };
    if register + dwords > 256 {
        return Err(refuse(
            instruction,
            "a scratch-segment access runs past the end of the vector register file",
        ));
    }
    if !matches!(saddr, SADDR_OFF | SADDR_NULL) && saddr >= SCALAR_REGISTERS {
        return Err(refuse(
            instruction,
            "a scratch-segment access whose SADDR names no scalar register",
        ));
    }
    let lanes = model.lanes().max(1);
    let lane_bits = model.constant(lanes.trailing_zeros());
    let yes = model.constant(1);
    let always = model.is_not_zero(yes);
    let zero = model.constant(0);
    for lane in crate::model::running_lanes(model) {
        let base = match saddr {
            SADDR_OFF => zero,
            SADDR_NULL => {
                let register = u16::try_from(vaddr).unwrap_or(u16::MAX);
                model.read_source(instruction, &Operand::Vector(register), lane)?
            }
            register => model.read_scalar(register),
        };
        let values = if loading {
            Vec::new()
        } else {
            (0..dwords)
                .map(|step| {
                    let register = u16::try_from(data + step).unwrap_or(u16::MAX);
                    model.read_source(instruction, &Operand::Vector(register), lane)
                })
                .collect::<Result<Vec<_>, _>>()?
        };
        let id = lane_id(model, lane);
        for step in 0..dwords {
            let immediate = model.constant(immediate.wrapping_add(4 * step));
            let offset = model.add(base, immediate);
            let at = swizzled_place(model, (id, offset), (zero, lane_bits));
            let moved = if loading {
                None
            } else {
                Some(values[step as usize])
            };
            if let Some(value) = element(model, (at, always), (width, moved), lane)? {
                model.write_vector_lane(register + step, lane, value);
            }
        }
    }
    model.count();
    Ok(())
}

/// `HW_REG_FLAT_SCR_LO` and `HW_REG_FLAT_SCR_HI`, the hardware register ids `s_setreg_b32` names
/// in bits 5:0 of its immediate (AMD's published RDNA instruction set, `S_SETREG`): the halves of
/// the flat scratch base.
const FLAT_SCRATCH_REGISTERS: [u32; 2] = [20, 21];

/// `HW_REG_MODE`, the hardware register id of the wave's float mode - rounding and denormals -
/// as `operands-hwreg.out` records it.
const MODE_REGISTER: u32 = 1;

/// `s_setreg_b32` of the flat scratch base, its low or high half: where the wave's scratch lies in
/// guest memory, which the hardware adds to a lane's private address. The module holds scratch as
/// a private array instead, which no address reaches (this module's account), so setting the base
/// changes nothing it reads. Grand Theft Auto V's ray traversal passes set it from their scratch
/// offset at entry (guest-observed, D964). Any other hardware register is refused.
///
/// # Errors
///
/// An `s_setreg_b32` of another register, or in a module whose dispatch gives it no scratch.
pub(crate) fn set_flat_scratch<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
) -> Result<(), TranslateError> {
    let register = instruction.word & 0x3f;
    if register == MODE_REGISTER {
        return Err(refuse(
            instruction,
            "an s_setreg of the mode register, whose float mode a module compiles in from its dispatch's (D965)",
        ));
    }
    if !FLAT_SCRATCH_REGISTERS.contains(&register) {
        return Err(refuse(
            instruction,
            "an s_setreg of a hardware register other than the flat scratch base or the mode",
        ));
    }
    if model.scratch().is_none() {
        return Err(refuse(
            instruction,
            "the flat scratch base set in a module whose dispatch gives it no scratch (D954)",
        ));
    }
    model.count();
    Ok(())
}

/// The word at byte `offset` of `lane`'s own private memory, as a flat access through the private
/// aperture names it (D964): laid out as a scratch-segment access's address is, the low two bits
/// dropped. Where `routed` holds, `stored` - when given - is written there; elsewhere the word is
/// left alone. Answers what the word held, zero where `routed` does not hold or the module's scratch
/// does not hold the word, which is then an escape. A module with no scratch holds no word.
pub(crate) fn private_word<M: Model + ?Sized>(
    model: &mut M,
    (offset, routed): (Id, Id),
    stored: Option<Id>,
    lane: u32,
) -> Id {
    let zero = model.constant(0);
    if model.scratch().is_none() {
        model.note_escape(routed, Some(lane));
        return zero;
    }
    let lanes = model.lanes().max(1);
    let lane_bits = model.constant(lanes.trailing_zeros());
    let id = lane_id(model, lane);
    let aligned = model.constant(!3);
    let offset = model.binary(op::BITWISE_AND, offset, aligned);
    let at = swizzled_place(model, (id, offset), (zero, lane_bits));
    let two = model.constant(2);
    let word = model.binary(op::SHIFT_RIGHT_LOGICAL, at, two);
    let Some((index, held)) = model.scratch_place(word, lane) else {
        model.note_escape(routed, Some(lane));
        return zero;
    };
    let valid = model.both(routed, held);
    let unheld = logical_not(model, held);
    let escaped = model.both(routed, unheld);
    model.note_escape(escaped, Some(lane));
    let index = model.select(valid, index, zero);
    let Some(held_word) = model.read_scratch(index) else {
        return zero;
    };
    if let Some(value) = stored {
        let kept = model.select(valid, value, held_word);
        let _ = model.write_scratch(index, kept, lane);
    }
    model.select(valid, held_word, zero)
}

/// Moves one element of `width` at byte `at` of the wave's scratch for `lane`: stores `moved`, or
/// answers what a load reads. A dword must be aligned and a half lie within its dword. An access the
/// module's scratch does not hold, or that `shaped` - whether the descriptor is scratch's - denies,
/// reads zero and stores nothing, and is an escape (D746).
fn element<M: Model + ?Sized>(
    model: &mut M,
    (at, shaped): (Id, Id),
    (width, moved): (Width, Option<Id>),
    lane: u32,
) -> Result<Option<Id>, TranslateError> {
    let (two, three, zero) = (model.constant(2), model.constant(3), model.constant(0));
    let word = model.binary(op::SHIFT_RIGHT_LOGICAL, at, two);
    let byte = model.binary(op::BITWISE_AND, at, three);
    let fits = match width {
        Width::Dwords(_) => model.compare(op::IEQUAL, byte, zero),
        Width::Part(bits, _) => {
            // The bytes from `byte` on that stay within the dword.
            let last = model.constant(5 - bits / 8);
            model.compare(op::ULESS_THAN, byte, last)
        }
    };
    let Some((index, held)) = model.scratch_place(word, lane) else {
        return Err(TranslateError::Unsupported {
            offset: 0,
            detail: "a scratch access in a model with no scratch (D954)",
        });
    };
    let fits = model.both(fits, shaped);
    let valid = model.both(fits, held);
    let index = model.select(valid, index, zero);
    let escaped = logical_not(model, valid);
    model.note_escape(escaped, Some(lane));
    let held_word = model
        .read_scratch(index)
        .ok_or(TranslateError::Unsupported {
            offset: 0,
            detail: "a scratch access in a model with no scratch (D954)",
        })?;
    if let Some(value) = moved {
        let kept = model.select(valid, value, held_word);
        model
            .write_scratch(index, kept, lane)
            .ok_or(TranslateError::Unsupported {
                offset: 0,
                detail: "a scratch access in a model with no scratch (D954)",
            })?;
        return Ok(None);
    }
    let value = match width {
        Width::Dwords(_) => held_word,
        Width::Part(bits, signed) => {
            let eight = model.constant(8);
            let shift = model.binary(op::IMUL, byte, eight);
            let down = model.binary(op::SHIFT_RIGHT_LOGICAL, held_word, shift);
            let unused = model.constant(32 - bits);
            let top = model.binary(op::SHIFT_LEFT_LOGICAL, down, unused);
            let widen = if signed {
                op::SHIFT_RIGHT_ARITHMETIC
            } else {
                op::SHIFT_RIGHT_LOGICAL
            };
            model.binary(widen, top, unused)
        }
    };
    Ok(Some(model.select(valid, value, zero)))
}

/// The negation of a boolean.
fn logical_not<M: Model + ?Sized>(model: &mut M, value: Id) -> Id {
    let bool_type = model.bool_type();
    let b = model.builder();
    let result = b.id();
    b.function(op::LOGICAL_NOT, &[bool_type.0, result.0, value.0]);
    result
}
