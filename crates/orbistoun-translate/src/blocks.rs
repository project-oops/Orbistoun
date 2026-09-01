//! Splitting a decoded shader into basic blocks.
//!
//! # Why this is needed at all
//!
//! SPIR-V demands structured control flow: merge blocks, a reducible graph, no jumping
//! into the middle of a construct. The guest has none of that. It has a flat instruction
//! stream and a signed offset, and it will happily branch backwards into the middle of
//! anything.
//!
//! Rather than reconstruct structure that may not exist, every guest block becomes an
//! arm of one switch inside one loop, selected by a program counter (D110). That shape
//! is always valid however tangled the guest's flow is - but it needs the blocks, which
//! is what this module finds.
//!
//! # What starts a block
//!
//! Three things: the entry point, any instruction a branch targets, and the instruction
//! after a branch. The last is what makes a conditional's not-taken path a block of its
//! own.
//!
//! # A branch target that is not an instruction boundary
//!
//! An offset can land inside an instruction - from a miscomputed target, a decode that
//! went wrong earlier, or data being executed. It is reported rather than rounded to a
//! nearby boundary: a shader whose branch lands mid-instruction is not a shader this
//! translator understands, and quietly moving the target would produce a plausible
//! program that is not the one the guest wrote.

use orbistoun_shader::{Decode, Instruction, Operand};

use crate::TranslateError;

/// How a block ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Terminator {
    /// Execution stops.
    End,
    /// Always to one place.
    Jump {
        /// Byte offset of the target.
        target: u32,
    },
    /// To one of two places, on a condition the terminating instruction names.
    Branch {
        /// Taken.
        target: u32,
        /// Not taken: the instruction after the branch.
        fallthrough: u32,
    },
    /// Runs into the next block, because the guest's next instruction is a branch target.
    Fallthrough {
        /// Byte offset of the next instruction.
        next: u32,
    },
}

/// One basic block.
#[derive(Debug, Clone)]
pub struct Block {
    /// Byte offset of the first instruction.
    pub start: u32,
    /// Index of the first instruction, into the decode's list.
    pub first: usize,
    /// One past the last instruction.
    pub end: usize,
    /// How it ends.
    pub terminator: Terminator,
}

/// The branch opcodes, and what makes each take.
///
/// SOPP, all of them, all four bytes, all carrying a signed word offset in the low half.
/// Listed rather than matched on a range because the family also holds `s_endpgm`,
/// `s_waitcnt` and a dozen instructions that are not branches at all.
pub const BRANCHES: &[(u32, Condition)] = &[
    (2, Condition::Always),
    (4, Condition::ScalarConditionClear),
    (5, Condition::ScalarConditionSet),
    (6, Condition::ConditionMaskZero),
    (7, Condition::ConditionMaskNonZero),
    (8, Condition::ExecutionMaskZero),
    (9, Condition::ExecutionMaskNonZero),
];

/// What a branch tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Condition {
    /// Unconditional.
    Always,
    /// The scalar condition code is clear.
    ScalarConditionClear,
    /// The scalar condition code is set.
    ScalarConditionSet,
    /// No lane's condition bit is set.
    ConditionMaskZero,
    /// Some lane's condition bit is set.
    ConditionMaskNonZero,
    /// No lane is active.
    ExecutionMaskZero,
    /// Some lane is active.
    ExecutionMaskNonZero,
}

/// `s_endpgm`.
const ENDPGM: u32 = 1;

/// The condition a branch instruction tests, if it is one.
pub fn branch_condition(instruction: &Instruction, family: &str) -> Option<Condition> {
    if family != "SOPP" {
        return None;
    }
    BRANCHES
        .iter()
        .find(|(opcode, _)| *opcode == instruction.opcode)
        .map(|(_, condition)| *condition)
}

/// Where a branch goes.
///
/// The offset is a **signed** count of dwords from the instruction *after* the branch.
/// The decoder reports the field as encoded, which keeps it agreeing with the reference
/// operand for operand - a disassembler prints `-6` as `65530` - so the sign extension
/// happens here, where the instruction's meaning is known.
///
/// Sixteen bits, from the instruction's definition rather than from the operand layout.
/// The layout records the field it observed; how to read it is not something the bits
/// can say.
pub fn branch_target(instruction: &Instruction) -> Result<u32, TranslateError> {
    let Some(Operand::Immediate(raw)) = instruction.operands.first() else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a branch carries no target - the operand layout for this opcode is \
                     missing, so where it goes is not known",
        });
    };
    let offset = i64::from(i32::from(
        i16::try_from(*raw & 0xFFFF).unwrap_or(*raw as i16),
    ));
    let after = i64::from(instruction.offset) + i64::from(instruction.length);
    let target = after + offset * 4;

    u32::try_from(target).map_err(|_| TranslateError::Unsupported {
        offset: instruction.offset,
        detail: "a branch target falls outside the shader",
    })
}

/// Splits a decoded shader into basic blocks.
///
/// # Errors
///
/// A branch whose target is not the start of an instruction, or falls outside the
/// shader. Both mean the stream is not what it appears to be, and rounding to a nearby
/// boundary would produce a program that runs and is not the guest's.
pub fn split(
    decode: &Decode,
    family_of: impl Fn(&Instruction) -> Option<String>,
) -> Result<Vec<Block>, TranslateError> {
    let instructions = &decode.instructions;
    if instructions.is_empty() {
        return Ok(Vec::new());
    }

    // Byte offset to instruction index, so a branch target can be checked against real
    // boundaries rather than assumed to land on one.
    let index_of: std::collections::BTreeMap<u32, usize> = instructions
        .iter()
        .enumerate()
        .map(|(i, instruction)| (instruction.offset, i))
        .collect();

    let mut starts: std::collections::BTreeSet<usize> = std::collections::BTreeSet::new();
    starts.insert(0);

    for (i, instruction) in instructions.iter().enumerate() {
        let Some(family) = family_of(instruction) else {
            continue;
        };
        if branch_condition(instruction, &family).is_none() {
            continue;
        }
        let target = branch_target(instruction)?;
        let at = *index_of.get(&target).ok_or(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a branch target is not the start of any instruction - the \
                         stream is not what it appears to be, and moving the target to \
                         a nearby boundary would run a program the guest did not write",
        })?;
        starts.insert(at);
        // The instruction after a branch begins a block too: it is the not-taken path,
        // and for an unconditional branch it is unreachable-but-present, which the guest
        // is entitled to do.
        if i + 1 < instructions.len() {
            starts.insert(i + 1);
        }
    }

    let boundaries: Vec<usize> = starts.into_iter().collect();
    let mut blocks = Vec::with_capacity(boundaries.len());
    for (n, &first) in boundaries.iter().enumerate() {
        let end = boundaries.get(n + 1).copied().unwrap_or(instructions.len());
        let last = &instructions[end - 1];
        let family = family_of(last);
        let terminator = match family.as_deref().and_then(|f| branch_condition(last, f)) {
            Some(Condition::Always) => Terminator::Jump {
                target: branch_target(last)?,
            },
            Some(_) => Terminator::Branch {
                target: branch_target(last)?,
                fallthrough: last.offset + last.length,
            },
            None if family.as_deref() == Some("SOPP") && last.opcode == ENDPGM => Terminator::End,
            // Runs off the end of the shader with no terminator. Treated as ending
            // rather than as an error: a decode that stops early is already reported by
            // `is_trustworthy`, and this is not the place to report it twice.
            None if end == instructions.len() => Terminator::End,
            None => Terminator::Fallthrough {
                next: last.offset + last.length,
            },
        };
        blocks.push(Block {
            start: instructions[first].offset,
            first,
            end,
            terminator,
        });
    }
    Ok(blocks)
}

/// The index of the block starting at a byte offset.
pub fn block_at(blocks: &[Block], offset: u32) -> Option<usize> {
    blocks.iter().position(|block| block.start == offset)
}

#[cfg(test)]
mod tests {
    use super::*;
    use orbistoun_shader::{EncodingTable, OperandTable, decode};

    /// Decodes a word stream and splits it, resolving families through the table.
    fn split_words(words: &[u32]) -> Result<Vec<Block>, TranslateError> {
        let table = EncodingTable::builtin().expect("encodings");
        let operands = OperandTable::builtin().expect("operands");
        let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        let decoded = decode(&bytes, &table, &operands);
        assert!(decoded.is_trustworthy(), "the fixture must decode cleanly");
        let names: Vec<String> = table.encodings().iter().map(|e| e.name.clone()).collect();
        split(&decoded, |i| {
            i.encoding.and_then(|e| names.get(usize::from(e)).cloned())
        })
    }

    /// `s_branch <offset>` and friends: SOPP, opcode at bit 16, offset in the low half.
    const fn branch(opcode: u32, offset: i16) -> u32 {
        0xBF80_0000 | (opcode << 16) | (offset as u16 as u32)
    }

    const ENDPGM_WORD: u32 = 0xBF81_0000;
    /// `v_mov_b32_e32 v0, 0`, as filler with no control-flow meaning.
    const NOP: u32 = 0x7E00_0280;

    #[test]
    fn a_shader_with_no_branches_is_one_block() {
        // The case every existing test exercises, and the one the dispatch loop must
        // not make worse: one arm, entered once, left once.
        let blocks = split_words(&[NOP, NOP, ENDPGM_WORD]).expect("split");
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].terminator, Terminator::End);
        assert_eq!(blocks[0].first, 0);
        assert_eq!(blocks[0].end, 3);
    }

    #[test]
    fn a_forward_branch_splits_into_three() {
        // The branch ends a block; its target starts one; the instruction after it
        // starts one too, because that is the not-taken path.
        //
        //   0x0 nop
        //   0x4 s_cbranch_execz +1   -> 0xc
        //   0x8 nop                   (not taken)
        //   0xc s_endpgm              (taken)
        let blocks = split_words(&[NOP, branch(8, 1), NOP, ENDPGM_WORD]).expect("split");
        assert_eq!(blocks.len(), 3, "blocks were {blocks:?}");
        assert_eq!(
            blocks[0].terminator,
            Terminator::Branch {
                target: 0xc,
                fallthrough: 0x8
            }
        );
        assert_eq!(blocks[1].start, 0x8);
        assert_eq!(blocks[2].start, 0xc);
        assert_eq!(blocks[2].terminator, Terminator::End);
    }

    #[test]
    fn a_block_running_into_a_branch_target_falls_through() {
        // Block 1 has no terminating branch - it simply reaches an instruction that
        // something else jumps to. Without an explicit fallthrough the dispatch loop
        // would leave the program counter unchanged and spin forever on that arm.
        let blocks = split_words(&[NOP, branch(8, 1), NOP, ENDPGM_WORD]).expect("split");
        assert_eq!(
            blocks[1].terminator,
            Terminator::Fallthrough { next: 0xc },
            "blocks were {blocks:?}"
        );
    }

    #[test]
    fn a_backward_branch_makes_a_loop() {
        // The case predication cannot express at all, and the reason this module exists.
        //
        //   0x0 nop            <- target
        //   0x4 nop
        //   0x8 s_cbranch_execnz -3  -> 0x0
        //   0xc s_endpgm
        let blocks = split_words(&[NOP, NOP, branch(9, -3), ENDPGM_WORD]).expect("split");
        assert_eq!(
            blocks[0].terminator,
            Terminator::Branch {
                target: 0x0,
                fallthrough: 0xc
            },
            "blocks were {blocks:?}"
        );
        assert_eq!(
            block_at(&blocks, 0x0),
            Some(0),
            "the branch target must be a block start"
        );
    }

    #[test]
    fn an_unconditional_branch_is_a_jump_with_no_fallthrough() {
        // The offset counts dwords from the instruction *after* the branch, so +1 from
        // a branch at 0x0 is 0x4 + 4 = 0x8 - not 0xc. Getting that base wrong is a
        // one-instruction error in every branch in every shader, and it lands on a real
        // instruction boundary most of the time, so nothing downstream would complain.
        let blocks = split_words(&[branch(2, 1), NOP, ENDPGM_WORD]).expect("split");
        assert_eq!(blocks[0].terminator, Terminator::Jump { target: 0x8 });
        assert_eq!(blocks.last().expect("a block").start, 0x8);
    }

    #[test]
    fn a_target_that_is_not_an_instruction_boundary_is_refused() {
        // An offset can land inside an instruction. Rounding to a nearby boundary would
        // produce a program that runs and is not the one the guest wrote, which is the
        // worst outcome available - so it is an error.
        //
        // Any eight-byte instruction will do, so the table is asked for one rather than
        // a word being written down here - a written-down encoding belongs to one
        // architecture generation, and on the next it matches no family, decodes as four
        // unrecognised bytes, and this test fails claiming the fixture is malformed.
        //
        //   0x0 s_cbranch_execz +1  -> 0x8, which is inside the instruction below
        //   0x4 an eight-byte instruction, so it spans 0x4..0xc
        //   0xc s_endpgm
        let table = EncodingTable::builtin().expect("encodings");
        let wide = table
            .encodings()
            .iter()
            .find(|encoding| encoding.width_bytes == 8)
            .expect("some family is eight bytes wide");
        let words = [branch(8, 1), wide.value, 0, ENDPGM_WORD];
        let error = split_words(&words).expect_err("a mid-instruction target must be refused");
        assert!(
            error
                .to_string()
                .contains("not the start of any instruction"),
            "the error should say what is wrong, got: {error}"
        );
    }
}
