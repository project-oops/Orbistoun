//! Splitting a decoded shader into basic blocks.
//!
//! Guest code is a flat stream with signed branch offsets and no structure; every guest
//! block becomes an arm of one switch inside one loop (D098), and this module finds the
//! blocks. A block starts at the entry point, at any branch target, and after a branch (the
//! not-taken path). A branch target inside an instruction is refused rather than rounded to
//! a nearby boundary, which would run a program the guest did not write.

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
/// All SOPP, four bytes, with a signed word offset in the low half. Listed rather than a
/// range because the family also holds `s_endpgm`, `s_waitcnt` and other non-branches.
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
/// The offset is a signed count of dwords from the instruction after the branch. The
/// decoder reports the field as encoded, agreeing with the reference (a disassembler
/// prints `-6` as `65530`), so the sixteen-bit sign extension happens here, from the
/// instruction's definition.
pub fn branch_target(instruction: &Instruction) -> Result<u32, TranslateError> {
    let Some(Operand::Immediate(raw)) = instruction.operands.first() else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: concat!(
                "a branch carries no target - the operand layout for this opcode is ",
                "missing, so where it goes is not known"
            ),
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
/// A branch whose target is not the start of an instruction, or falls outside the shader:
/// the stream is not what it appears to be.
pub fn split(
    decode: &Decode,
    family_of: impl Fn(&Instruction) -> Option<String>,
) -> Result<Vec<Block>, TranslateError> {
    let instructions = &decode.instructions;
    if instructions.is_empty() {
        return Ok(Vec::new());
    }

    // Byte offset to instruction index, so a branch target is checked against real
    // boundaries.
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
            detail: concat!(
                "a branch target is not the start of any instruction - the ",
                "stream is not what it appears to be, and moving the target to ",
                "a nearby boundary would run a program the guest did not write"
            ),
        })?;
        starts.insert(at);
        // The instruction after a branch begins a block: the not-taken path, or
        // unreachable-but-present code after an unconditional branch.
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
            // Runs off the end with no terminator: treated as ending, since an early stop
            // is already reported by `is_trustworthy`.
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

    /// A shader with no branches is one block ending in `End`.
    #[test]
    fn a_shader_with_no_branches_is_one_block() {
        let blocks = split_words(&[NOP, NOP, ENDPGM_WORD]).expect("split");
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].terminator, Terminator::End);
        assert_eq!(blocks[0].first, 0);
        assert_eq!(blocks[0].end, 3);
    }

    /// A conditional branch splits the stream into three blocks.
    #[test]
    fn a_forward_branch_splits_into_three() {
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

    /// A block running into a branch target falls through explicitly, so the program
    /// counter advances.
    #[test]
    fn a_block_running_into_a_branch_target_falls_through() {
        let blocks = split_words(&[NOP, branch(8, 1), NOP, ENDPGM_WORD]).expect("split");
        assert_eq!(
            blocks[1].terminator,
            Terminator::Fallthrough { next: 0xc },
            "blocks were {blocks:?}"
        );
    }

    /// A backward branch targets a block start, forming a loop.
    #[test]
    fn a_backward_branch_makes_a_loop() {
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

    /// An unconditional branch counts from the instruction after it, with no fallthrough.
    #[test]
    fn an_unconditional_branch_is_a_jump_with_no_fallthrough() {
        // +1 from a branch at 0x0 is 0x4 + 4 = 0x8, not 0xc.
        let blocks = split_words(&[branch(2, 1), NOP, ENDPGM_WORD]).expect("split");
        assert_eq!(blocks[0].terminator, Terminator::Jump { target: 0x8 });
        assert_eq!(blocks.last().expect("a block").start, 0x8);
    }

    /// A branch target inside an instruction is refused.
    #[test]
    fn a_target_that_is_not_an_instruction_boundary_is_refused() {
        // The eight-byte instruction comes from the table so the test survives a retarget.
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
