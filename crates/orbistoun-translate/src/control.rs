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
    // The waves of a group that meet do so between blocks: one ends at each `s_barrier`.
    let meets = model.group_waves() > 1;
    let blocks = blocks::split_at_barriers(decode, family_of, meets)?;
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

    let segments = if meets {
        segments(decode, &blocks, &family_of)?
    } else {
        std::iter::once(0..blocks.len()).collect()
    };
    let mut translated = 0usize;
    for (n, segment) in segments.into_iter().enumerate() {
        // Between two loops every invocation stands at the top level, so the workgroup's barrier
        // is reached by all of them, once each, as the guest's waves reach theirs.
        if n > 0 {
            workgroup_barrier(model);
        }
        translated += emit_loop(model, decode, (&blocks, segment, n == 0), exit, &family_of)?;
    }
    Ok(translated)
}

/// The blocks cut into runs that each end at an `s_barrier` - the last run at the program's end -
/// refusing a program with a branch from one run into another: a branch over a barrier, or a loop
/// around one, where one wave would meet the others a different number of times (D892).
fn segments(
    decode: &Decode,
    blocks: &[Block],
    family_of: &impl Fn(&Instruction) -> Option<String>,
) -> Result<Vec<std::ops::Range<usize>>, TranslateError> {
    let mut segments = Vec::new();
    let mut start = 0;
    for (index, block) in blocks.iter().enumerate() {
        let last = &decode.instructions[block.end - 1];
        if family_of(last).is_some_and(|family| blocks::is_barrier(last, &family)) {
            segments.push(start..index + 1);
            start = index + 1;
        }
    }
    if start < blocks.len() {
        segments.push(start..blocks.len());
    }
    for segment in &segments {
        for index in segment.clone() {
            let block = &blocks[index];
            let targets = match block.terminator {
                Terminator::End => vec![],
                // The barrier's own block runs on into the next run, past the barrier.
                Terminator::Fallthrough { .. } if index + 1 == segment.end => vec![],
                Terminator::Jump { target } | Terminator::Fallthrough { next: target } => {
                    vec![target]
                }
                Terminator::Branch {
                    target,
                    fallthrough,
                } => vec![target, fallthrough],
            };
            let crosses = targets.into_iter().any(|target| {
                blocks::block_at(blocks, target).is_none_or(|to| !segment.contains(&to))
            });
            if crosses {
                return Err(TranslateError::Unsupported {
                    offset: block.start,
                    detail: concat!(
                        "a thread group of waves branches over an s_barrier or loops around one, ",
                        "so its waves would not all meet there the same number of times"
                    ),
                });
            }
        }
    }
    Ok(segments)
}

/// The workgroup's barrier, ordering the local data share's words across it: what `s_barrier`
/// does for a group's waves.
fn workgroup_barrier<M: Model + ?Sized>(model: &mut M) {
    let scope = model.constant(orbistoun_spirv::scope::WORKGROUP);
    let semantics = model.constant(
        orbistoun_spirv::semantics::ACQUIRE_RELEASE | orbistoun_spirv::semantics::WORKGROUP_MEMORY,
    );
    model
        .builder()
        .function(op::CONTROL_BARRIER, &[scope.0, scope.0, semantics.0]);
}

/// Emits one dispatch loop over the blocks `range` names, running while the program counter is
/// within it, and leaves the builder in its merge block; `first` stores block zero into the counter
/// before the loop.
fn emit_loop<M: Model + ?Sized>(
    model: &mut M,
    decode: &Decode,
    (blocks, range, first): (&[Block], std::ops::Range<usize>, bool),
    exit: u32,
    family_of: &impl Fn(&Instruction) -> Option<String>,
) -> Result<usize, TranslateError> {
    let header = model.builder().id();
    let dispatch = model.builder().id();
    let after = model.builder().id();
    let continue_target = model.builder().id();
    let merge = model.builder().id();
    let default = model.builder().id();
    let arms: Vec<Id> = range.clone().map(|_| model.builder().id()).collect();

    // ---- entry: start at block zero -------------------------------------------
    if first {
        let zero = model.constant(0);
        store_counter(model, zero);
    }
    model.builder().function(op::BRANCH, &[header.0]);

    // ---- header: still running? -----------------------------------------------
    // Within the range: below its end, and - for a later loop - at or past its start, asked as one
    // unsigned comparison of the counter less the start. A wave that ended holds `exit`, past
    // every range, and runs none of the later loops.
    model.builder().function(op::LABEL, &[header.0]);
    let current = load_counter(model);
    let (low, high) = (to_u32(range.start)?, to_u32(range.end)?);
    let (current, limit) = if low == 0 {
        (current, model.constant(high))
    } else {
        let start = model.constant(low);
        (
            model.binary(op::ISUB, current, start),
            model.constant(high - low),
        )
    };
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
    for (index, arm) in range.clone().zip(&arms) {
        // The literal is a plain word, not an identifier, so `OpSwitch` has its own stride
        // in the builder's shape table.
        switch.push(u32::try_from(index).unwrap_or(u32::MAX));
        switch.push(arm.0);
    }
    model.builder().function(op::SWITCH, &switch);

    // ---- the arms ---------------------------------------------------------------
    let mut translated = 0usize;
    for (index, arm) in range.zip(&arms) {
        let block = &blocks[index];
        model.builder().function(op::LABEL, &[arm.0]);
        model.enter_block();
        let before = model.instructions();
        for instruction in &decode.instructions[block.first..block.end] {
            // The terminating branch emits nothing here; its target is the program-counter
            // assignment below.
            if is_terminator(instruction, family_of) {
                continue;
            }
            // An instruction whose lanes each do the same work runs them as one loop, where the
            // model runs the whole wave in one invocation, rather than each emitted in turn.
            let looped = model::lane_looped(model, instruction) && model.open_lane_loop();
            model::instruction(model, instruction)?;
            if looped {
                model.close_lane_loop();
            }
        }
        translated += model.instructions() - before;

        let mut next = successor(model, decode, block, blocks, exit)?;
        if jumps_back(block, blocks, index) {
            let branch = decode.instructions[block.end - 1].offset;
            next = cap_loop(model, (next, to_u32(index)?), exit, branch);
        }
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

/// Backward jumps one invocation takes before the loop it is in is cut and reported (D949).
///
/// Counted across every loop of the program, so it bounds the work of one invocation, which is what
/// the host's watchdog bounds. A loop of three scalar instructions run to the cap takes tens of
/// milliseconds (`a_loop_at_the_cap_runs_and_one_past_it_is_cut` prints it), so a much heavier body
/// is still cut inside the watchdog, and no loop a guest bounds by its data comes near it.
pub const LOOP_CAP: u32 = 1 << 20;

/// Whether the branch ending `block`, the arm at `index`, can jump back: its taken target is that
/// block or one before it.
fn jumps_back(block: &Block, all: &[Block], index: usize) -> bool {
    match block.terminator {
        Terminator::Jump { target } | Terminator::Branch { target, .. } => {
            blocks::block_at(all, target).is_some_and(|to| to <= index)
        }
        Terminator::End | Terminator::Fallthrough { .. } => false,
    }
}

/// Counts a backward jump against the model's loop budget, and sends the invocation to `exit` - the
/// program's end, as `s_endpgm` does - once its jumps pass [`LOOP_CAP`], recording the loop it cut
/// by its backward branch's offset, `branch` (D949). `next` is the block the arm `index` leaves for;
/// it jumps back where it is at or before `index`. A model with no budget runs the loop as the
/// guest wrote it.
fn cap_loop<M: Model + ?Sized>(
    model: &mut M,
    (next, index): (Id, u32),
    exit: u32,
    branch: u32,
) -> Id {
    let Some(budget) = model.loop_budget() else {
        return next;
    };
    let (u32_type, bool_type) = (model.u32_type(), model.bool_type());
    let own = model.constant(index);
    let (zero, one) = (model.constant(0), model.constant(1));
    let (cap, end) = (model.constant(LOOP_CAP), model.constant(exit));
    let b = model.builder();
    let backward = b.id();
    b.function(
        op::ULESS_THAN_EQUAL,
        &[bool_type.0, backward.0, next.0, own.0],
    );
    let step = b.id();
    b.function(op::SELECT, &[u32_type.0, step.0, backward.0, one.0, zero.0]);
    let before = b.id();
    b.function(op::LOAD, &[u32_type.0, before.0, budget.0]);
    let after = b.id();
    b.function(op::IADD, &[u32_type.0, after.0, before.0, step.0]);
    b.function(op::STORE, &[budget.0, after.0]);
    let cut = b.id();
    b.function(op::UGREATER_THAN, &[bool_type.0, cut.0, after.0, cap.0]);
    model.note_loop_cut(cut, branch);
    let b = model.builder();
    let chosen = b.id();
    b.function(op::SELECT, &[u32_type.0, chosen.0, cut.0, end.0, next.0]);
    chosen
}

/// `value` as a program counter value.
fn to_u32(value: usize) -> Result<u32, TranslateError> {
    u32::try_from(value).map_err(|_| TranslateError::Unsupported {
        offset: 0,
        detail: "a shader with more blocks than a program counter can number",
    })
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
    // lane live" is asked of its own lanes; a wave of 32's upper half is a register of its own,
    // which its compares leave as it was.
    let lanes = model.lanes();
    let any = match lanes.cmp(&32) {
        std::cmp::Ordering::Less => {
            let existing = model.constant((1 << lanes) - 1);
            model.binary(op::BITWISE_AND, low, existing)
        }
        std::cmp::Ordering::Equal => low,
        std::cmp::Ordering::Greater => model.binary(op::BITWISE_OR, low, high),
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
pub(crate) fn read_condition_code<M: Model + ?Sized>(model: &mut M, want_set: bool) -> Id {
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
