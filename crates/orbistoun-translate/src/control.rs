//! The dispatch loop: guest control flow as a switch inside a loop.
//!
//! # The shape
//!
//! ```text
//! entry:      pc = 0; branch header
//! header:     loop-merge(merge, continue); branch-conditional (pc < blocks) dispatch merge
//! dispatch:   selection-merge(after); switch pc -> arm0 | arm1 | … | default
//! arm0:       <block 0's instructions>; pc = <successor>; branch after
//! …
//! default:    pc = exit; branch after
//! after:      branch continue
//! continue:   branch header
//! merge:      <epilogue>; return
//! ```
//!
//! # Why this rather than reconstructing structure
//!
//! SPIR-V requires structured control flow. The guest has none: a flat stream, signed
//! offsets, and no obligation to be reducible. Recovering structure from that is a real
//! compiler problem, and for irreducible flow there is no structure to recover - only a
//! transformation that invents one.
//!
//! This shape needs no analysis at all. Every guest block becomes an arm, every branch
//! becomes an assignment to the program counter, and the result is valid however tangled
//! the guest's flow is - including backwards, into the middle of anything, and from
//! several places at once (D110).
//!
//! It was the plan from the beginning, which is why the register file lives in memory:
//! a SPIR-V result belongs to the block that produced it and cannot cross into another
//! arm, so registers had to outlive a block. That decision was made for this.
//!
//! # A conditional branch adds no blocks
//!
//! The arm computes the condition and *selects* between two program counters. No merge
//! block, no nesting, no second arm - the branch has already been turned into data by
//! the time control leaves the switch. Every arm has exactly one predecessor and one
//! successor, which is what makes the shape uniformly valid.
//!
//! # The cost
//!
//! Every guest block is a switch dispatch, and every register access is a load and a
//! store. That is slow, and it is the same trade [`crate::Fidelity::Wavefront`] already
//! makes: be correct first, and let the differential oracle check the fast paths later.
//! A shader with one block still pays for the loop; collapsing that case is available
//! whenever it is worth measuring, and is deliberately not done yet, because the
//! single-block path is the one every existing test exercises and a second code path
//! for it would be the under-tested one.

use orbistoun_shader::{Decode, EncodingTable, Instruction};
use orbistoun_spirv::{Id, op};

use crate::TranslateError;
use crate::blocks::{self, Block, Condition, Terminator};
use crate::model::{self, Model};

/// Loop control: no hint.
const NO_LOOP_CONTROL: u32 = 0;
/// Selection control: no hint.
const NO_SELECTION_CONTROL: u32 = 0;

/// Emits the whole function body, and leaves the builder inside the merge block.
///
/// The caller's `finish` then appends its epilogue and return exactly as it did when the
/// body was a straight line - which is why adding control flow did not change what a
/// model reports.
pub fn emit<M: Model + ?Sized>(
    model: &mut M,
    decode: &Decode,
    encodings: &EncodingTable,
) -> Result<usize, TranslateError> {
    let names: Vec<&str> = encodings
        .encodings()
        .iter()
        .map(|e| e.name.as_str())
        .collect();
    let family_of = |instruction: &Instruction| {
        instruction
            .encoding
            .and_then(|e| names.get(usize::from(e)).map(|n| (*n).to_owned()))
    };
    let blocks = blocks::split(decode, family_of)?;
    if blocks.is_empty() {
        // Nothing to run. The merge block still has to exist, because `finish` writes
        // its epilogue into it and a function with no blocks is not a function.
        let merge = model.builder().id();
        model.builder().function(op::LABEL, &[merge.0]);
        return Ok(0);
    }

    let exit = u32::try_from(blocks.len()).map_err(|_| TranslateError::Unsupported {
        offset: 0,
        detail: "a shader with more blocks than a program counter can number",
    })?;

    let header = model.builder().id();
    let dispatch = model.builder().id();
    let after = model.builder().id();
    let continue_target = model.builder().id();
    let merge = model.builder().id();
    let default = model.builder().id();
    let arms: Vec<Id> = (0..blocks.len()).map(|_| model.builder().id()).collect();

    // ---- entry: start at block zero -------------------------------------------
    let zero = model.constant(0);
    store_counter(model, zero);
    model.builder().function(op::BRANCH, &[header.0]);

    // ---- header: still running? -----------------------------------------------
    model.builder().function(op::LABEL, &[header.0]);
    let current = load_counter(model);
    let limit = model.constant(exit);
    let bool_type = model.bool_type();
    let running = model.builder().id();
    model.builder().function(
        op::ULESS_THAN,
        &[bool_type.0, running.0, current.0, limit.0],
    );
    // The merge and continue targets are declared before the branch that needs them,
    // which is what makes this a loop rather than a backward jump SPIR-V will reject.
    model.builder().function(
        op::LOOP_MERGE,
        &[merge.0, continue_target.0, NO_LOOP_CONTROL],
    );
    model
        .builder()
        .function(op::BRANCH_CONDITIONAL, &[running.0, dispatch.0, merge.0]);

    // ---- dispatch: pick the arm ------------------------------------------------
    model.builder().function(op::LABEL, &[dispatch.0]);
    let selector = load_counter(model);
    model
        .builder()
        .function(op::SELECTION_MERGE, &[after.0, NO_SELECTION_CONTROL]);
    let mut switch = vec![selector.0, default.0];
    for (index, arm) in arms.iter().enumerate() {
        // The literal is a plain word, not an identifier - which is why `OpSwitch` needs
        // its own stride in the builder's shape table.
        switch.push(u32::try_from(index).unwrap_or(u32::MAX));
        switch.push(arm.0);
    }
    model.builder().function(op::SWITCH, &switch);

    // ---- the arms ---------------------------------------------------------------
    let mut translated = 0usize;
    for (index, block) in blocks.iter().enumerate() {
        model.builder().function(op::LABEL, &[arms[index].0]);
        let before = model.instructions();
        for instruction in &decode.instructions[block.first..block.end] {
            // The terminating branch itself emits nothing here - where it goes is the
            // program-counter assignment below, and translating it twice would be a
            // second, contradictory answer.
            if is_terminator(instruction, &family_of) {
                continue;
            }
            model::instruction(model, instruction)?;
        }
        translated += model.instructions() - before;

        let next = successor(model, decode, block, &blocks, exit)?;
        store_counter(model, next);
        model.builder().function(op::BRANCH, &[after.0]);
    }

    // ---- default: a program counter no arm claims -------------------------------
    // Unreachable by construction - the counter only ever holds an arm index or the
    // exit value, and the exit value leaves at the header. It exists because `OpSwitch`
    // requires a default, and it stops rather than falling into an arm, so a counter
    // that somehow went wrong ends the shader instead of running an arbitrary block.
    model.builder().function(op::LABEL, &[default.0]);
    let limit = model.constant(exit);
    store_counter(model, limit);
    model.builder().function(op::BRANCH, &[after.0]);

    // ---- the plumbing back to the header ----------------------------------------
    model.builder().function(op::LABEL, &[after.0]);
    model.builder().function(op::BRANCH, &[continue_target.0]);
    model.builder().function(op::LABEL, &[continue_target.0]);
    model.builder().function(op::BRANCH, &[header.0]);

    // The epilogue goes here, written by the caller.
    model.builder().function(op::LABEL, &[merge.0]);
    Ok(translated)
}

/// Whether this instruction is the one that ends its block.
fn is_terminator(
    instruction: &Instruction,
    family_of: &impl Fn(&Instruction) -> Option<String>,
) -> bool {
    family_of(instruction).is_some_and(|family| {
        blocks::branch_condition(instruction, &family).is_some()
            || (family == "SOPP" && instruction.opcode == ENDPGM)
    })
}

/// `s_endpgm`.
const ENDPGM: u32 = 1;

/// The program counter value a block leaves behind.
fn successor<M: Model + ?Sized>(
    model: &mut M,
    decode: &Decode,
    block: &Block,
    all: &[Block],
    exit: u32,
) -> Result<Id, TranslateError> {
    let index_of = |offset: u32| -> Result<u32, TranslateError> {
        blocks::block_at(all, offset)
            .and_then(|i| u32::try_from(i).ok())
            .ok_or(TranslateError::Unsupported {
                offset: block.start,
                detail: "a successor is not the start of any block",
            })
    };

    match block.terminator {
        Terminator::End => Ok(model.constant(exit)),
        Terminator::Jump { target } | Terminator::Fallthrough { next: target } => {
            let index = index_of(target)?;
            Ok(model.constant(index))
        }
        Terminator::Branch {
            target,
            fallthrough,
        } => {
            let last = &decode.instructions[block.end - 1];
            let taken = model.constant(index_of(target)?);
            let not_taken = model.constant(index_of(fallthrough)?);
            let condition = branch_condition_value(model, last)?;

            // A select rather than a second branch. The guest's conditional has become
            // data by the time control leaves this arm, so the arm has one successor and
            // the switch stays flat.
            let u32_type = model.u32_type();
            let b = model.builder();
            let chosen = b.id();
            b.function(
                op::SELECT,
                &[u32_type.0, chosen.0, condition.0, taken.0, not_taken.0],
            );
            Ok(chosen)
        }
    }
}

/// Evaluates what a conditional branch tests.
fn branch_condition_value<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
) -> Result<Id, TranslateError> {
    let condition = blocks::BRANCHES
        .iter()
        .find(|(opcode, _)| *opcode == instruction.opcode)
        .map(|(_, condition)| *condition)
        .ok_or(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "this block ends in something that is not a branch",
        })?;

    let (name, want_zero) = match condition {
        Condition::ExecutionMaskZero => (model::EXEC_LOW_HALF, true),
        Condition::ExecutionMaskNonZero => (model::EXEC_LOW_HALF, false),
        Condition::ConditionMaskZero => (model::VCC_LOW_HALF, true),
        Condition::ConditionMaskNonZero => (model::VCC_LOW_HALF, false),
        // The condition code is one bit of state rather than a mask, so it is read
        // directly rather than through the mask path below.
        Condition::ScalarConditionClear | Condition::ScalarConditionSet => {
            let want_set = condition == Condition::ScalarConditionSet;
            return Ok(read_condition_code(model, want_set));
        }
        Condition::Always => {
            return Err(TranslateError::Unsupported {
                offset: instruction.offset,
                detail: "an unconditional branch has no condition to evaluate",
            });
        }
    };

    let (low, high) = model.read_lane_mask(name)?;
    let any = model.binary(op::BITWISE_OR, low, high);
    let zero = model.constant(0);
    let bool_type = model.bool_type();
    let u32_type = model.u32_type();
    let _ = u32_type;

    let b = model.builder();
    let result = b.id();
    let opcode = if want_zero {
        op::IEQUAL
    } else {
        op::INOT_EQUAL
    };
    b.function(opcode, &[bool_type.0, result.0, any.0, zero.0]);
    Ok(result)
}

/// Whether the scalar condition code is set, or clear.
fn read_condition_code<M: Model + ?Sized>(model: &mut M, want_set: bool) -> Id {
    let (pointer, u32_type, bool_type) =
        (model.condition_code(), model.u32_type(), model.bool_type());
    let zero = model.constant(0);
    let b = model.builder();
    let value = b.id();
    b.function(op::LOAD, &[u32_type.0, value.0, pointer.0]);
    let result = b.id();
    let opcode = if want_set { op::INOT_EQUAL } else { op::IEQUAL };
    b.function(opcode, &[bool_type.0, result.0, value.0, zero.0]);
    result
}

/// Reads the program counter.
fn load_counter<M: Model + ?Sized>(model: &mut M) -> Id {
    let (pointer, u32_type) = (model.program_counter(), model.u32_type());
    let b = model.builder();
    let value = b.id();
    b.function(op::LOAD, &[u32_type.0, value.0, pointer.0]);
    value
}

/// Writes the program counter.
fn store_counter<M: Model + ?Sized>(model: &mut M, value: Id) {
    let pointer = model.program_counter();
    model.builder().function(op::STORE, &[pointer.0, value.0]);
}
