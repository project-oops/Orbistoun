//! The dispatch loop: guest control flow as a switch inside a loop.
//!
//! SPIR-V requires structured control flow; guest code has none and may be irreducible.
//! Every guest block becomes an arm of one `OpSwitch` on a program counter inside one loop.
//! Each arm runs its block, assigns the counter its successor (a conditional branch selects
//! between two values, adding no blocks) and branches to the continue path. The loop exits
//! when the counter reaches the block count, into the merge block where the caller writes
//! its epilogue. The shape is valid however tangled the guest flow (D098). Registers live in
//! memory because a SPIR-V result cannot cross from one arm into another.

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
/// The caller's `finish` then appends its epilogue and return as for a straight-line body.
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
        // Nothing to run. The merge block still exists, because `finish` writes its
        // epilogue into it.
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
    // The merge and continue targets are declared before the branch, which makes this
    // a loop rather than a backward jump SPIR-V rejects.
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
        // The literal is a plain word, not an identifier, so `OpSwitch` has its own stride
        // in the builder's shape table.
        switch.push(u32::try_from(index).unwrap_or(u32::MAX));
        switch.push(arm.0);
    }
    model.builder().function(op::SWITCH, &switch);

    // ---- the arms ---------------------------------------------------------------
    let mut translated = 0usize;
    for (index, block) in blocks.iter().enumerate() {
        model.builder().function(op::LABEL, &[arms[index].0]);
        model.enter_block();
        let before = model.instructions();
        for instruction in &decode.instructions[block.first..block.end] {
            // The terminating branch emits nothing here; its target is the program-counter
            // assignment below.
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
    // Unreachable by construction: the counter holds an arm index or the exit value, which
    // leaves at the header. `OpSwitch` requires a default, and it ends the shader rather
    // than falling into an arm.
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

            // A select rather than a second branch, so the arm has one successor and the
            // switch stays flat.
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
        // The condition code is one bit of state rather than a mask, read directly.
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
    // Only the lanes this model simulates. A one-lane fragment module's mask can carry bits
    // for lanes it does not have (an all-ones entry mask, a whole-quad expansion), so "is any
    // lane live" is asked of its own lanes.
    let lanes = model.lanes();
    let any = if lanes < 32 {
        let existing = model.constant((1 << lanes) - 1);
        model.binary(op::BITWISE_AND, low, existing)
    } else {
        model.binary(op::BITWISE_OR, low, high)
    };
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
