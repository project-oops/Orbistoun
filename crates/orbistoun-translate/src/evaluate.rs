//! The scalar registers a program prefix leaves, computed on the host for one draw (D753).
//!
//! A descriptor a program computes from words it reads from memory has no static source, but every
//! input is uniform for the draw. The prefix up to the access runs here over the draw's user data and
//! guest memory. Each scalar instruction goes through the translator's own semantics
//! ([`model::instruction`]), driven by a model whose values are numbers. The SPIR-V those semantics
//! emit is read back an instruction at a time and evaluated, so nothing about an instruction is
//! restated here. A value the host cannot know is unknown, and anything computed from it is
//! unknown: a vector result written to a scalar, the execution mask at entry (which differs from
//! wave to wave), and an operation this reader does not evaluate.
//!
//! A primitive shader's global loads run the same way lane by lane (D758): vector instructions too,
//! from the execution mask the module starts with, following each branch whose condition is known,
//! so the addresses each lane forms are what a draw binds.

use orbistoun_shader::{Decode, EncodingTable, Instruction, Operand};
use orbistoun_spirv::{Builder, Id, op};

use crate::TranslateError;
use crate::model::{self, Model};

/// Scalar register codes as an operand field numbers them: `s0`-`s105`, the condition mask at 106,
/// `m0` at 124 and the execution mask at 126.
pub const SCALAR_CODES: usize = 128;
const VCC: usize = 106;
const M0: usize = 124;
const EXEC: usize = 126;
/// The vector registers a primitive shader's module seeds with ids from its draw's geometry before
/// the program runs, in its vertex lanes and zero past them (the wavefront's `seed_geometry`): the
/// primitive's vertex indices and its id in `v0`-`v2`, and the vertex id in `v5`. A lane run does
/// not know them; the module seeds every other register zero.
const SEEDED_IDS: [u32; 4] = [0, 1, 2, 5];
/// The scalar offset field's "no offset" code.
const SOFFSET_NULL: u32 = 0x7d;

/// `OpIAddCarry`, `OpISubBorrow` and `OpUMulExtended` (SPIR-V 3.42.13): two-member results.
const IADD_CARRY: u16 = 149;
const ISUB_BORROW: u16 = 150;
const UMUL_EXTENDED: u16 = 151;
/// `OpBitFieldInsert`, `OpBitFieldSExtract` and `OpBitFieldUExtract` (SPIR-V 3.42.14).
const BIT_FIELD_INSERT: u16 = 201;
const BIT_FIELD_SEXTRACT: u16 = 202;
const BIT_FIELD_UEXTRACT: u16 = 203;
/// `OpULessThanEqual` and `OpLogicalEqual`/`OpLogicalNotEqual` (SPIR-V 3.42.15).
const ULESS_THAN_EQUAL: u16 = 178;
const LOGICAL_EQUAL: u16 = 164;
const LOGICAL_NOT_EQUAL: u16 = 165;

/// What each scalar register code holds after a prefix, where the host can know it.
pub type Scalars = [Option<u32>; SCALAR_CODES];

/// Why a scalar register is unknown at an access, where something made it so.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unknown {
    /// The instruction at this byte left it unknown: a vector result, the execution mask, a read
    /// the host could not make, or an operation the semantics do not run.
    LeftBy(u32),
    /// The paths to the access left it holding different values.
    PathsDisagree,
}

/// Why each scalar register code is unknown at an access ([`Unknown`]); `None` where it is known,
/// or where nothing before the access ever set it.
pub type Unknowns = [Option<Unknown>; SCALAR_CODES];

/// Runs `decode`'s instructions up to byte `until` over a stage whose user-data words land from
/// scalar register `first_register`, with the system registers `system` seeds as the module seeds
/// them, reading guest memory word by word through `read`. A branch whose condition the host
/// knows is followed, as [`lane_addresses`] follows one (D758): `s_branch`, `s_cbranch_scc0` and
/// `scc1` on a known condition code, and `s_cbranch_vccz` and `vccnz` on a known condition mask.
///
/// `None` where a branch the host follows jumps past the access: the wave does not reach it, and
/// what it would read through is never read (D800).
///
/// # Errors
///
/// A reason, where the prefix branches on what the host cannot know, jumps to a computed address,
/// or runs longer than its program allows.
pub fn scalar_prefix(
    decode: &Decode,
    encodings: &EncodingTable,
    (until, program_address): (u32, Option<u64>),
    (first_register, user_data): (u32, &[u32]),
    system: &[(u32, u32)],
    read: &mut dyn FnMut(u64) -> Option<u32>,
) -> Result<Option<Scalars>, &'static str> {
    scalar_prefix_explained(
        decode,
        encodings,
        (until, program_address),
        (first_register, user_data),
        system,
        read,
    )
    .map(|reached| reached.map(|(scalars, _)| scalars))
}

/// [`scalar_prefix`], with why each register unknown at the access is unknown ([`Unknowns`]): what
/// a refusal names when a draw cannot know what it reads through.
///
/// # Errors
///
/// As [`scalar_prefix`].
pub fn scalar_prefix_explained(
    decode: &Decode,
    encodings: &EncodingTable,
    (until, program_address): (u32, Option<u64>),
    (first_register, user_data): (u32, &[u32]),
    system: &[(u32, u32)],
    read: &mut dyn FnMut(u64) -> Option<u32>,
) -> Result<Option<(Scalars, Box<Unknowns>)>, &'static str> {
    // Each path a branch on the execution mask can take to the access, followed in turn: the
    // decisions taken so far, depth first.
    let mut pending: Vec<Vec<bool>> = vec![Vec::new()];
    let mut merged: Option<(Scalars, Box<Unknowns>)> = None;
    let mut runs = 0;
    while let Some(decisions) = pending.pop() {
        runs += 1;
        if runs > MOST_PATHS {
            return Err("the program's paths to the access are too many to follow (D753)");
        }
        let path = prefix_path(
            decode,
            encodings,
            (until, program_address),
            (first_register, user_data),
            system,
            (read, &decisions),
        )?;
        match path {
            Path::Reached(reached) => {
                let (scalars, unknowns) = *reached;
                merged = Some(match merged {
                    None => (scalars, Box::new(unknowns)),
                    // Known only where every path to the access agrees.
                    Some((earlier, mut why)) => {
                        let mut both = earlier;
                        for (register, (kept, other)) in both.iter_mut().zip(scalars).enumerate() {
                            if *kept != other {
                                why[register] = match (*kept, other) {
                                    (Some(_), Some(_)) => Some(Unknown::PathsDisagree),
                                    (None, _) => why[register].or(unknowns[register]),
                                    (Some(_), None) => unknowns[register],
                                };
                                *kept = None;
                            }
                        }
                        (both, why)
                    }
                });
            }
            Path::Skipped => {}
            Path::Undecided => {
                for decision in [true, false] {
                    let mut next = decisions.clone();
                    next.push(decision);
                    pending.push(next);
                }
            }
        }
    }
    Ok(merged)
}

/// How many paths [`scalar_prefix`] follows to one access before it gives up: six branches on the
/// execution mask before it, each both ways, and one more.
const MOST_PATHS: usize = 1 << 7;

/// Where one path through a prefix ended.
enum Path {
    /// At the access, with these scalars, and why those unknown are.
    Reached(Box<(Scalars, Unknowns)>),
    /// Past it, by a branch that jumps over it (D800).
    Skipped,
    /// At a branch on the execution mask whose way the decisions given do not say.
    Undecided,
}

/// One path of [`scalar_prefix`]: each branch on the execution mask that the host cannot know and
/// that lands before the access takes the way `decisions` gives, in order - taken for `true`. A
/// scalar instruction runs whatever the mask holds, so every way is a way the wave may go.
fn prefix_path(
    decode: &Decode,
    encodings: &EncodingTable,
    (until, program_address): (u32, Option<u64>),
    (first_register, user_data): (u32, &[u32]),
    system: &[(u32, u32)],
    (read, decisions): (&mut dyn FnMut(u64) -> Option<u32>, &[bool]),
) -> Result<Path, &'static str> {
    let mut decided = decisions.iter().copied();
    let mut model = Concrete::new(encodings, decode.instructions.len());
    model.program_address = program_address;
    for &(register, word) in system {
        if register < model::SCALAR_REGISTERS {
            model.scalars[register as usize] = Some(word);
        }
    }
    for (register, word) in (first_register as usize..).zip(user_data) {
        if register < model::SCALAR_REGISTERS as usize {
            model.scalars[register] = Some(*word);
        }
    }
    let mut at = 0usize;
    let mut unknowns: Unknowns = [None; SCALAR_CODES];
    // A bound on the steps, since a known branch can go backwards.
    let mut steps = 0usize;
    while let Some(instruction) = decode.instructions.get(at) {
        if instruction.offset == until {
            break;
        }
        if instruction.offset > until {
            return Ok(Path::Skipped);
        }
        steps += 1;
        if steps > 4 * decode.instructions.len() {
            return Err("the program loops before the access (D753)");
        }
        at += 1;
        let Some((family, name)) = named(encodings, instruction) else {
            return Err("an instruction before the access has no name (D753)");
        };
        // A register this step leaves unknown is unknown because of it; one it makes known has no
        // cause any more.
        let before = model.scalars;
        let step = step_prefix(
            &mut model,
            (instruction, family, name),
            (decode, until),
            (&mut decided, read),
        )?;
        for (register, (was, now)) in before.iter().zip(model.scalars).enumerate() {
            match (was, now) {
                (Some(_), None) => unknowns[register] = Some(Unknown::LeftBy(instruction.offset)),
                (_, Some(_)) => unknowns[register] = None,
                (None, None) => {}
            }
        }
        match step {
            Step::Next => {}
            Step::Jump(to) => at = to,
            Step::Ended(path) => return Ok(path),
        }
    }
    Ok(Path::Reached(Box::new((model.scalars, unknowns))))
}

/// Where a step of [`prefix_path`] leaves the walk.
enum Step {
    /// On to the next instruction.
    Next,
    /// On from the instruction at this index.
    Jump(usize),
    /// The path ends here.
    Ended(Path),
}

/// Runs one instruction of a prefix: a branch, a scalar load, scalar arithmetic, or what a vector
/// instruction writes into the scalars.
fn step_prefix(
    model: &mut Concrete<'_>,
    (instruction, family, name): (&Instruction, &str, &str),
    (decode, until): (&Decode, u32),
    (decided, read): (
        &mut impl Iterator<Item = bool>,
        &mut dyn FnMut(u64) -> Option<u32>,
    ),
) -> Result<Step, &'static str> {
    {
        match family {
            "SOPP" if branches(name) => {
                let taken = match name {
                    "s_branch" => Some(true),
                    "s_cbranch_scc0" => model.condition.map(|scc| scc == 0),
                    "s_cbranch_scc1" => model.condition.map(|scc| scc != 0),
                    "s_cbranch_vccz" => vcc_set(&model.scalars).map(|set| !set),
                    "s_cbranch_vccnz" => vcc_set(&model.scalars),
                    // A branch on the execution mask that jumps past the access is never taken
                    // where the access runs, so the access's scalars are those of the path that
                    // falls through - a legacy streamout pass skipping its store for a wave with
                    // no vertices to write (D861).
                    "s_cbranch_execz" | "s_cbranch_execnz" if jumps_past(instruction, until) => {
                        Some(false)
                    }
                    // One landing before the access is followed both ways, a path each (D753).
                    "s_cbranch_execz" | "s_cbranch_execnz" => match decided.next() {
                        Some(taken) => Some(taken),
                        None => return Ok(Step::Ended(Path::Undecided)),
                    },
                    _ => None,
                };
                let Some(taken) = taken else {
                    return Err(
                        "the program branches before the access on what the host cannot know (D753)",
                    );
                };
                if taken {
                    let Some(Operand::Immediate(jump)) = instruction.operands.first() else {
                        return Err("a branch with no target (D753)");
                    };
                    let target = i64::from(instruction.offset) + 4 + 4 * simm16(*jump);
                    // The prefix ends at the access: a jump beyond it skips it (D800).
                    if target > i64::from(until) {
                        return Ok(Step::Ended(Path::Skipped));
                    }
                    let at = decode
                        .instructions
                        .iter()
                        .position(|i| i64::from(i.offset) == target)
                        .unwrap_or(decode.instructions.len());
                    if at == decode.instructions.len() && target != i64::from(until) {
                        return Err("a branch into the middle of an instruction (D753)");
                    }
                    return Ok(Step::Jump(at));
                }
            }
            "SOPP" => {}
            "SMEM" => model.scalar_load(instruction, name, read),
            "SOP1" | "SOP2" | "SOPK" | "SOPC" => {
                if name.starts_with("s_setpc")
                    || name.starts_with("s_swappc")
                    || name.starts_with("s_cbranch")
                {
                    return Err(
                        "the program branches before the access, which is not followed (D753)",
                    );
                }
                if model::instruction(model, instruction).is_err() {
                    model.forget_written(instruction, name);
                }
                model.catch_up();
            }
            _ => model.forget_vector_writes(instruction, name),
        }
    }
    Ok(Step::Next)
}

/// Whether a branch's target lies beyond the access at `until`.
fn jumps_past(instruction: &Instruction, until: u32) -> bool {
    matches!(
        instruction.operands.first(),
        Some(Operand::Immediate(jump)) if i64::from(instruction.offset) + 4 + 4 * simm16(*jump) > i64::from(until)
    )
}

/// Whether the execution mask is non-zero, where the scalar registers say, as [`vcc_set`] says of
/// the condition mask.
fn exec_set(scalars: &Scalars) -> Option<bool> {
    match (scalars[EXEC], scalars[EXEC + 1]) {
        (Some(low), _) if low != 0 => Some(true),
        (_, Some(high)) if high != 0 => Some(true),
        (Some(_), Some(_)) => Some(false),
        _ => None,
    }
}

/// Whether the condition mask is non-zero, where the scalar registers say: a known non-zero low half
/// says so alone, and both halves known say either way.
fn vcc_set(scalars: &Scalars) -> Option<bool> {
    match (scalars[VCC], scalars[VCC + 1]) {
        (Some(low), _) if low != 0 => Some(true),
        (_, Some(high)) if high != 0 => Some(true),
        (Some(_), Some(_)) => Some(false),
        _ => None,
    }
}

/// Why a lane run could not answer: a reason, naming the instruction where one is to blame.
pub type LaneRefusal = std::borrow::Cow<'static, str>;

/// The addresses a primitive shader's global loads read, for one draw: each load's byte offset in
/// the program, and the full 64-bit address every active lane forms, its immediate offset added.
pub type LaneAddresses = Vec<(u32, Vec<u64>)>;

/// Runs `decode` lane by lane over a primitive shader's draw (D758), until the instruction at
/// `until` has been passed, and answers the address each active lane forms at each load in
/// `loads` the run reaches.
///
/// The run starts as the module does, over a wave of `width` lanes: the user-data words from
/// `first_register`, every lane's execution-mask bit set, and every vector register zero but the
/// primitive and vertex ids the module seeds in the draw's vertex lanes, which are unknown here. A
/// lane is active at a load when its mask bit is known set and it is below `lanes`, the draw's
/// vertices. A branch whose condition is known is followed; any other ends the run.
///
/// # Errors
///
/// A reason, where a load's mask or an active lane's address is unknown, or the run cannot follow
/// the program to a load.
pub fn lane_addresses(
    program: &LaneProgram,
    encodings: &EncodingTable,
    (first_register, user_data): (u32, &[u32]),
    (width, lanes): (u32, u32),
    read: &mut dyn FnMut(u64) -> Option<u32>,
) -> Result<LaneAddresses, LaneRefusal> {
    let mut model = Concrete::new(encodings, program.decode.instructions.len());
    for (register, word) in (first_register as usize..).zip(user_data) {
        if register < model::SCALAR_REGISTERS as usize {
            model.scalars[register] = Some(*word);
        }
    }
    // The module's entry: every lane of the wave runs until the program narrows the mask.
    let width = width.clamp(1, 64);
    let mask = u64::MAX >> (64 - width);
    model.scalars[EXEC] = Some(mask as u32);
    model.scalars[EXEC + 1] = Some((mask >> 32) as u32);
    model.width = width;
    model.vertices = lanes;
    model.lanes = true;
    // Only the draw's vertex lanes, where nothing the run does reads one lane from another.
    model.modelled = if program.crosses {
        width
    } else {
        lanes.clamp(1, width)
    };
    run_lanes(&mut model, program, encodings, lanes, read)
}

/// A compute wave's registers at entry beyond its user data, for a lane run (D883): what the
/// hardware loads for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WaveEntry {
    /// The workgroup id components enabled, in the scalar registers after the user data.
    pub group_ids: Vec<u32>,
    /// Each lane's thread id components, lane zero up; `None` for a lane with no thread, which does
    /// not run - past a group's last thread, or outside a partial group (D855).
    pub threads: Vec<Option<[u32; 3]>>,
    /// How many thread id components land in `v0` upward.
    pub components: u32,
}

/// Runs `program` over one compute wave (D883), as [`lane_addresses`] runs a draw's: the user data
/// from `s0`, then `wave`'s group ids; in each lane with a thread its id components from `v0`, and
/// that lane's execution-mask bit; every other vector register zero. The wave is `width` lanes.
/// Each load's addresses are the running lanes', in lane order.
///
/// # Errors
///
/// As [`lane_addresses`].
pub fn wave_lane_addresses(
    program: &LaneProgram,
    encodings: &EncodingTable,
    user_data: &[u32],
    (width, wave): (u32, &WaveEntry),
    read: &mut dyn FnMut(u64) -> Option<u32>,
) -> Result<LaneAddresses, LaneRefusal> {
    let mut model = Concrete::new(encodings, program.decode.instructions.len());
    for (register, word) in user_data.iter().chain(&wave.group_ids).enumerate() {
        if register < model::SCALAR_REGISTERS as usize {
            model.scalars[register] = Some(*word);
        }
    }
    let width = width.clamp(1, 64);
    let lanes = u32::try_from(wave.threads.len())
        .unwrap_or(u32::MAX)
        .min(width);
    let mask = (0..lanes)
        .zip(&wave.threads)
        .filter(|(_, thread)| thread.is_some())
        .fold(0u64, |mask, (lane, _)| mask | 1 << lane);
    model.scalars[EXEC] = Some(mask as u32);
    model.scalars[EXEC + 1] = Some((mask >> 32) as u32);
    model.width = width;
    model.lanes = true;
    model.modelled = if program.crosses {
        width
    } else {
        lanes.clamp(1, width)
    };
    for (lane, ids) in (0..lanes).zip(&wave.threads) {
        let Some(ids) = ids else {
            continue;
        };
        for (register, id) in (0..wave.components.min(3)).zip(ids) {
            model.vectors.insert((register, lane), Some(*id));
        }
    }
    run_lanes(&mut model, program, encodings, lanes, read)
}

/// The run [`lane_addresses`] and [`wave_lane_addresses`] share, from a model at the wave's entry:
/// each load's addresses in the active lanes below `lanes`.
fn run_lanes(
    model: &mut Concrete<'_>,
    program: &LaneProgram,
    encodings: &EncodingTable,
    lanes: u32,
    read: &mut dyn FnMut(u64) -> Option<u32>,
) -> Result<LaneAddresses, LaneRefusal> {
    const UNKNOWN: &str = concat!(
        "a global load whose lanes' addresses, or whose execution mask, the host cannot know for ",
        "the draw or dispatch (D758, D883)"
    );
    let LaneProgram {
        decode,
        loads,
        until,
        kept: needed,
        ..
    } = program;
    let (loads, until) = (loads.as_slice(), *until);
    let mut found = LaneAddresses::new();
    let mut at = 0usize;
    // A bound on the steps, since a known branch can go backwards.
    let mut steps = 0usize;
    // Why each register is unknown, where a step made it so: what a refused branch names.
    let mut causes = Causes::new();
    while let Some(instruction) = decode.instructions.get(at) {
        if instruction.offset > until {
            break;
        }
        if steps > 4 * decode.instructions.len() {
            return Err(LOOPS_PAST_THE_BOUND.into());
        }
        steps += 1;
        let Some((family, name)) = named(encodings, instruction) else {
            return Err("an instruction before a global load has no name (D758)".into());
        };
        let before = model.scalars;
        if loads.contains(&instruction.offset) {
            let addresses = model.load_addresses(instruction, lanes).ok_or(UNKNOWN)?;
            found.push((instruction.offset, addresses));
        }
        at += 1;
        match family {
            "SOPP" if name == "s_endpgm" => break,
            "SOPP" if branches(name) => {
                let taken = match name {
                    "s_branch" => Some(true),
                    "s_cbranch_scc0" => model.condition.map(|scc| scc == 0),
                    "s_cbranch_scc1" => model.condition.map(|scc| scc != 0),
                    // On a mask, where the run knows it: a compute wave's execution mask is
                    // known from its entry (D883).
                    "s_cbranch_execz" => exec_set(&model.scalars).map(|set| !set),
                    "s_cbranch_execnz" => exec_set(&model.scalars),
                    "s_cbranch_vccz" => vcc_set(&model.scalars).map(|set| !set),
                    "s_cbranch_vccnz" => vcc_set(&model.scalars),
                    _ => None,
                };
                let Some(taken) = taken else {
                    return Err(causes.branch((instruction.offset, name)));
                };
                if taken {
                    let Some(Operand::Immediate(jump)) = instruction.operands.first() else {
                        return Err("a branch with no target (D758)".into());
                    };
                    let target = i64::from(instruction.offset) + 4 + 4 * simm16(*jump);
                    // A jump past the run's end leaves every load behind it: the run is done.
                    if target > i64::from(until) {
                        break;
                    }
                    at = decode
                        .instructions
                        .iter()
                        .position(|i| i64::from(i.offset) == target)
                        .ok_or("a branch into the middle of an instruction (D758)")?;
                }
            }
            // A program-control instruction that stays on the line, and an export, write no
            // register.
            "SOPP" | "EXP" => {}
            "SMEM" => model.scalar_load(instruction, name, read),
            "SOP1" | "SOP2" | "SOPK" | "SOPC" => {
                if name.starts_with("s_setpc") || name.starts_with("s_swappc") {
                    return Err("the program jumps to a computed address (D758)".into());
                }
                if model::instruction(model, instruction).is_err() {
                    model.forget_written(instruction, name);
                }
                model.catch_up();
            }
            // A vector instruction nothing at a load's address reads is not run: what it writes
            // stays as it was, which no address depends on.
            "VOP1" | "VOP2" | "VOP3" | "VOP3P" | "VOPC" if !needed[at - 1] => {}
            "VOP1" | "VOP2" | "VOP3" | "VOP3P" | "VOPC" => {
                if model::instruction(model, instruction).is_err() {
                    model.forget_vector(instruction.operands.first(), 4);
                    model.forget_vector_writes(instruction, name);
                }
                model.catch_up();
            }
            // Any other access's destination is what memory held, unknown here.
            _ => model.forget_vector(instruction.operands.first(), written_vectors(name)),
        }
        causes.record(model, (instruction, name), &before, lanes);
    }
    Ok(found)
}

/// The scalar codes a step writes, a mask's or a pair's both halves: its destination where that is
/// a scalar or a mask, and the carry a vector carry operation writes beside its vector result.
fn written_scalars(instruction: &Instruction, name: &str) -> Vec<usize> {
    let carry = name.contains("_co_");
    instruction
        .operands
        .iter()
        .take(if carry { 2 } else { 1 })
        .filter_map(|operand| match operand {
            Operand::Scalar(register) => Some(usize::from(*register)),
            Operand::Named(named) => mask_code(named),
            _ => None,
        })
        .flat_map(|code| [code, code + 1])
        .collect()
}

/// The step that left a register unknown in a lane run: its offset, its name, and what it read
/// unknown, with the causes of those in turn.
type LeftBy<'a> = (u32, &'a str, String);

/// The longest cause a refusal quotes: a long chain is cut, its first steps kept.
const MOST_CAUSE_BYTES: usize = 600;

/// A word as the float nearest it, as SPIR-V's conversion rounds.
fn unsigned_to_float(value: u32) -> f32 {
    // f64 holds every u32 exactly; the narrowing rounds to nearest, even.
    f64::from(value) as f32
}

/// A signed word as the float nearest it.
fn signed_to_float(value: i32) -> f32 {
    f64::from(value) as f32
}

/// A float truncated to an unsigned word, where it is in range; `None` otherwise.
fn float_to_unsigned(value: f32) -> Option<u32> {
    let value = f64::from(value).trunc();
    (0.0..=f64::from(u32::MAX))
        .contains(&value)
        .then_some(value as u32)
}

/// A float truncated to a signed word, where it is in range; `None` otherwise.
fn float_to_signed(value: f32) -> Option<i32> {
    let value = f64::from(value).trunc();
    (f64::from(i32::MIN)..=f64::from(i32::MAX))
        .contains(&value)
        .then_some(value as i32)
}

/// An ordered or unordered float comparison by its SPIR-V opcode: an ordered one is false, an
/// unordered one true, where either side is a NaN.
fn spirv_float_compare(opcode: u16, a: f32, b: f32) -> Option<bool> {
    use std::cmp::Ordering::{Equal, Greater, Less};
    let order = a.partial_cmp(&b);
    let holds = |wanted: &[std::cmp::Ordering]| order.is_some_and(|o| wanted.contains(&o));
    let unordered = order.is_none();
    Some(match opcode {
        op::FORD_EQUAL => holds(&[Equal]),
        op::FORD_NOT_EQUAL => holds(&[Less, Greater]),
        op::FORD_LESS_THAN => holds(&[Less]),
        op::FORD_GREATER_THAN => holds(&[Greater]),
        op::FORD_LESS_THAN_EQUAL => holds(&[Less, Equal]),
        op::FORD_GREATER_THAN_EQUAL => holds(&[Greater, Equal]),
        op::FUNORD_EQUAL => unordered || holds(&[Equal]),
        op::FUNORD_NOT_EQUAL => unordered || holds(&[Less, Greater]),
        op::FUNORD_LESS_THAN => unordered || holds(&[Less]),
        op::FUNORD_GREATER_THAN => unordered || holds(&[Greater]),
        op::FUNORD_LESS_THAN_EQUAL => unordered || holds(&[Less, Equal]),
        op::FUNORD_GREATER_THAN_EQUAL => unordered || holds(&[Greater, Equal]),
        _ => return None,
    })
}

/// How many vector registers from its first operand a memory access writes: none for a store, its
/// width for a global, buffer, flat or scratch load - a byte, half or dword one, the `x2` to `x4`
/// forms two to four - and four for any other, whose width the name does not say.
pub(crate) fn written_vectors(name: &str) -> u32 {
    if name.contains("store") || name.starts_with("ds_write") {
        return 0;
    }
    let loads = [
        "global_load_",
        "buffer_load_",
        "flat_load_",
        "scratch_load_",
    ];
    let Some(kind) = loads.iter().find_map(|load| name.strip_prefix(load)) else {
        return 4;
    };
    match kind.strip_prefix("dwordx") {
        Some(count) => count.parse().unwrap_or(4),
        None if kind.starts_with("dword") || kind.contains("byte") || kind.contains("short") => 1,
        None => 4,
    }
}

/// The code of a scalar source unknown before a step, where it is.
fn unknown_register(code: usize, before: &Scalars) -> Option<usize> {
    before
        .get(code)
        .copied()
        .flatten()
        .is_none()
        .then_some(code)
}

/// The code of a mask's half unknown before a step, either; `None` where both are known.
fn unknown_half(code: usize, before: &Scalars) -> Option<usize> {
    (code..code + 2).find(|&c| before.get(c).copied().flatten().is_none())
}

/// Why each register a lane run holds is unknown, where a step made it so: the scalars by code, and
/// the vector registers unknown in a running lane.
struct Causes<'a> {
    scalars: Vec<Option<LeftBy<'a>>>,
    vectors: Vec<Option<LeftBy<'a>>>,
}

impl<'a> Causes<'a> {
    fn new() -> Self {
        Self {
            scalars: vec![None; SCALAR_CODES],
            vectors: vec![None; 256],
        }
    }

    /// Records what a step left unknown: a scalar it made unknown, or one it wrote and could not
    /// know - a mask a compare wrote from what the run does not know - and a vector destination
    /// unknown in a running lane; and forgets the cause of whatever it made known.
    fn record(
        &mut self,
        model: &Concrete<'_>,
        (instruction, name): (&Instruction, &'a str),
        before: &Scalars,
        lanes: u32,
    ) {
        let written = written_scalars(instruction, name);
        for (code, (was, now)) in before.iter().zip(model.scalars).enumerate() {
            let left = now.is_none() && (was.is_some() || written.contains(&code));
            if now.is_some() {
                self.scalars[code] = None;
            } else if left {
                let why = self.sources(instruction, before);
                self.scalars[code] = Some((instruction.offset, name, why));
            }
        }
        if let Some(Operand::Vector(register)) = instruction.operands.first() {
            let unknown = (0..lanes.min(model.modelled)).any(|lane| {
                model.lane_runs(lane) == Some(true)
                    && model.vector(u32::from(*register), lane).is_none()
            });
            let cause =
                unknown.then(|| (instruction.offset, name, self.sources(instruction, before)));
            if let Some(slot) = self.vectors.get_mut(usize::from(*register)) {
                *slot = cause;
            }
        }
    }

    /// What made a step's result unknown, as far as its sources say: each source unknown before
    /// it, with the step that left it so and that step's own cause, or that nothing had set it.
    /// Empty where every source was known: the operation itself is what the run does not carry
    /// out.
    fn sources(&self, instruction: &Instruction, before: &Scalars) -> String {
        let reasons: Vec<String> = instruction
            .operands
            .iter()
            .skip(1)
            .filter_map(|operand| {
                let (cause, spelled) = match operand {
                    Operand::Scalar(register) => (
                        self.scalars[unknown_register(usize::from(*register), before)?].as_ref(),
                        format!("s{register}"),
                    ),
                    Operand::Named(named) => (
                        self.scalars[unknown_half(mask_code(named)?, before)?].as_ref(),
                        named.clone(),
                    ),
                    Operand::Vector(register) => (
                        Some(self.vectors.get(usize::from(*register))?.as_ref()?),
                        format!("v{register}"),
                    ),
                    _ => return None,
                };
                Some(match cause {
                    Some((at, by, why)) => {
                        format!("{spelled} unknown after `{by}` at {at:#x}{why}")
                    }
                    None => format!("{spelled} unknown before any step set it"),
                })
            })
            .collect();
        if reasons.is_empty() {
            return String::new();
        }
        let why = format!(", which read {}", reasons.join(" and "));
        if why.len() <= MOST_CAUSE_BYTES {
            return why;
        }
        // The first steps and the last, where the chain reaches the step that began it.
        let head = (0..=MOST_CAUSE_BYTES / 3)
            .rev()
            .find(|&at| why.is_char_boundary(at))
            .unwrap_or(0);
        let tail = (why.len() - MOST_CAUSE_BYTES / 2..why.len())
            .find(|&at| why.is_char_boundary(at))
            .unwrap_or(why.len());
        format!("{} ... {}", &why[..head], &why[tail..])
    }

    /// A refusal for a branch at `at` on what the run cannot know, naming the step that left the
    /// execution mask or `vcc` unknown where one did, and why.
    fn branch(&self, (at, name): (u32, &str)) -> LaneRefusal {
        let causes: Vec<String> = [(EXEC, "the execution mask"), (VCC, "vcc")]
            .iter()
            .filter_map(|&(code, mask)| {
                let (step, by, why) = self.scalars[code]
                    .as_ref()
                    .or(self.scalars[code + 1].as_ref())?;
                Some(format!("{mask} is unknown after `{by}` at {step:#x}{why}"))
            })
            .collect();
        format!(
            "the program branches at `{name}` at {at:#x} on what the host cannot know{}{} (D758)",
            if causes.is_empty() { "" } else { ": " },
            causes.join("; ")
        )
        .into()
    }
}

/// A branch's word count, as its sixteen-bit signed `SIMM16` field holds it: the decoder reads an
/// immediate field unsigned, so a branch back reads as a large jump forward until it is
/// sign-extended (AMD's published RDNA instruction set, `S_BRANCH`).
pub(crate) fn simm16(field: i64) -> i64 {
    i64::from(i16::from_le_bytes((field as u16).to_le_bytes()))
}

/// Where a lane run over `loads` may stop: past the last load, and past every branch after it
/// that jumps back to it or before it, found to a fixed point - a loop whose branch back follows
/// its loads runs them again, so the run follows it.
pub(crate) fn run_end(decode: &Decode, encodings: &EncodingTable, loads: &[u32]) -> u32 {
    let mut end = loads.iter().copied().max().unwrap_or(0);
    loop {
        let further = decode
            .instructions
            .iter()
            .filter(|instruction| instruction.offset > end)
            .filter(|instruction| {
                named(encodings, instruction).is_some_and(|(family, name)| {
                    family == "SOPP" && (name == "s_branch" || name.starts_with("s_cbranch"))
                })
            })
            .filter(|instruction| {
                matches!(
                    instruction.operands.first(),
                    Some(Operand::Immediate(jump))
                        if i64::from(instruction.offset) + 4 + 4 * simm16(*jump) <= i64::from(end)
                )
            })
            .map(|instruction| instruction.offset)
            .max();
        match further {
            Some(offset) => end = offset,
            None => return end,
        }
    }
}

/// A program prepared for lane runs (D758): decoded, with which instructions a run needs and
/// whether any of them reads one lane from another. All of it is fixed by the program, so it is found
/// once and serves every draw.
#[derive(Debug, Clone)]
pub struct LaneProgram {
    decode: Decode,
    /// The global loads' byte offsets.
    loads: Vec<u32>,
    /// The last of them: a run ends once past it.
    until: u32,
    /// Which instructions a run runs, by index ([`address_slice`]).
    kept: Vec<bool>,
    /// Whether a kept instruction reads across lanes ([`crosses_lanes`]).
    crosses: bool,
    /// The program compiled for native runs, where every instruction a run reaches is one they
    /// carry out exactly (D850); else the byte of the first that is not.
    quick: Result<crate::quick_lanes::Program, u32>,
}

impl LaneProgram {
    /// `decode` prepared for runs that find the addresses of its global loads at `loads`.
    #[must_use]
    pub fn new(decode: Decode, encodings: &EncodingTable, loads: &[u32]) -> Self {
        let kept = address_slice(&decode, encodings, loads);
        let crosses = crosses_lanes(&decode, encodings, &kept);
        let quick = crate::quick_lanes::compile(&decode, encodings, loads, (&kept, crosses));
        Self {
            until: run_end(&decode, encodings, loads),
            loads: loads.to_vec(),
            decode,
            kept,
            crosses,
            quick,
        }
    }

    /// Whether it runs natively as well as through the model (D850).
    #[must_use]
    pub const fn runs_quickly(&self) -> bool {
        self.quick.is_ok()
    }

    /// The instruction at `at`, named with its byte, as a refusal quotes it; the native run's step
    /// bound where `at` is [`u32::MAX`].
    #[must_use]
    pub fn instruction_named(&self, at: u32) -> String {
        if at == u32::MAX {
            return format!("its step bound, {MOST_NATIVE_STEPS} steps");
        }
        let name = self
            .decode
            .instructions
            .iter()
            .find(|instruction| instruction.offset == at)
            .and_then(|instruction| {
                named(&EncodingTable::builtin().ok()?, instruction).map(|(_, name)| name.to_owned())
            })
            .unwrap_or_else(|| "an instruction with no name".to_owned());
        format!("`{name}` at {at:#x}")
    }

    /// The first instruction the native run does not carry out, named with its byte, where there
    /// is one: what a refusal of a loop only the native run could follow quotes.
    #[must_use]
    pub fn native_declined(&self) -> Option<String> {
        let at = *self.quick.as_ref().err()?;
        Some(self.instruction_named(at))
    }
}

/// [`wave_lane_addresses`] found natively, where the program compiled for that and the run knows
/// every value it needs (D850, D883); `None` otherwise, and the model then decides.
pub fn quick_wave_lane_addresses(
    program: &LaneProgram,
    user_data: &[u32],
    wave: (u32, &WaveEntry),
    read: &mut dyn FnMut(u64) -> Option<u32>,
) -> Option<LaneAddresses> {
    let bound = 4 * program.decode.instructions.len();
    crate::quick_lanes::run_wave(program.quick.as_ref().ok()?, user_data, wave, (bound, read)).ok()
}

/// The most steps the native run follows a wave's loop for, where the model stops at its own bound
/// ([`LOOPS_PAST_THE_BOUND`]): a resource limit, past which the loop is refused.
pub const MOST_NATIVE_STEPS: usize = 1 << 26;

/// [`wave_lane_addresses`] for a loop longer than the model follows: the native run, followed to
/// [`MOST_NATIVE_STEPS`].
///
/// # Errors
///
/// The byte of the instruction the native run declines - the first it does not carry out, or the
/// step that met a value it does not know - or [`u32::MAX`] where it ran out of steps.
pub fn long_wave_lane_addresses(
    program: &LaneProgram,
    user_data: &[u32],
    wave: (u32, &WaveEntry),
    read: &mut dyn FnMut(u64) -> Option<u32>,
) -> Result<LaneAddresses, u32> {
    let compiled = program.quick.as_ref().map_err(|&at| at)?;
    crate::quick_lanes::run_wave(compiled, user_data, wave, (MOST_NATIVE_STEPS, read))
}

/// The refusal a lane run gives a loop longer than it follows: what a caller with a longer run to
/// hand ([`long_wave_lane_addresses`]) tries that on.
pub const LOOPS_PAST_THE_BOUND: &str = concat!(
    "the program loops more steps than a lane run follows before its last global ",
    "load, whose later passes would read past the addresses reached (D758)"
);

/// [`lane_addresses`] found natively, where the program compiled for that and the run knows every
/// value it needs (D850); `None` otherwise, and the model then decides.
pub fn quick_lane_addresses(
    program: &LaneProgram,
    user_data: (u32, &[u32]),
    lanes: (u32, u32),
    read: &mut dyn FnMut(u64) -> Option<u32>,
) -> Option<LaneAddresses> {
    crate::quick_lanes::run(program.quick.as_ref().ok()?, user_data, lanes, read)
}

/// Which instructions a lane run must run for the addresses at `loads`: by index, every one but the
/// vector instructions whose results nothing reaching a load's address reads.
///
/// Found backwards to a fixed point over the whole program, so a writer on either side of a branch,
/// or round a loop, is kept. Conservatively wide: a vector instruction writing a scalar or a lane
/// mask is always kept, a vector destination counts as a register pair, and every vector source of
/// a kept instruction is needed as a pair.
fn address_slice(decode: &Decode, encodings: &EncodingTable, loads: &[u32]) -> Vec<bool> {
    let pair = |register: u16| [u32::from(register), u32::from(register) + 1];
    let mut needed: std::collections::BTreeSet<u32> = decode
        .instructions
        .iter()
        .filter(|i| loads.contains(&i.offset))
        .filter_map(|i| match i.operands.get(1) {
            Some(Operand::Vector(register)) => Some(pair(*register)),
            _ => None,
        })
        .flatten()
        .collect();
    let mut kept = vec![true; decode.instructions.len()];
    loop {
        let before = needed.len();
        for (index, instruction) in decode.instructions.iter().enumerate().rev() {
            let Some((family, name)) = named(encodings, instruction) else {
                continue;
            };
            if !matches!(family, "VOP1" | "VOP2" | "VOP3" | "VOP3P" | "VOPC") {
                continue;
            }
            let writes_mask = family == "VOPC"
                || name.contains("_co_")
                || name.contains("readlane")
                || name.contains("readfirstlane")
                || !matches!(instruction.operands.first(), Some(Operand::Vector(_)));
            let keep = writes_mask
                || matches!(instruction.operands.first(),
                    Some(Operand::Vector(destination))
                        if pair(*destination).iter().any(|r| needed.contains(r)));
            kept[index] = keep;
            if keep {
                for operand in instruction.operands.iter().skip(1) {
                    if let Operand::Vector(source) = operand {
                        needed.extend(pair(*source));
                    }
                }
            }
        }
        if needed.len() == before {
            return kept;
        }
    }
}

/// Whether any instruction a lane run runs (`kept`, by index) reads one lane's value from another:
/// a lane read, write or permute, a data-parallel or local-data-share access, or a read of a lane
/// mask a vector instruction wrote other than a lane's own bit (a select's or a carry's).
/// Conservatively wide: any instruction with no name counts.
fn crosses_lanes(decode: &Decode, encodings: &EncodingTable, kept: &[bool]) -> bool {
    // Scalar register codes a vector instruction has written a mask into.
    let mut masks: std::collections::BTreeSet<u32> = std::collections::BTreeSet::new();
    let code = |operand: &Operand| match operand {
        Operand::Scalar(register) => Some(u32::from(*register)),
        Operand::Named(name) => mask_code(name)
            .or_else(|| {
                model::lane_mask_high_name(name)
                    .and_then(mask_code)
                    .map(|c| c + 1)
            })
            .map(|c| c as u32),
        _ => None,
    };
    for (instruction, &kept) in decode.instructions.iter().zip(kept) {
        if !kept {
            continue;
        }
        let Some((family, name)) = named(encodings, instruction) else {
            return true;
        };
        if family == "DS"
            || [
                "readlane",
                "readfirstlane",
                "writelane",
                "permlane",
                "_dpp",
                "swizzle",
                "mbcnt_hi",
            ]
            .iter()
            .any(|part| name.contains(part))
        {
            return true;
        }
        let vector = matches!(family, "VOP1" | "VOP2" | "VOP3" | "VOP3P" | "VOPC");
        // A read of a written mask: a lane's own bit only through a select or a carry-in.
        let own_bit =
            name.starts_with("v_cndmask") || name.contains("_co_ci_") || name.contains("subb");
        // A carry-out's or a divide-scale's second operand is written, not read.
        let second = vector && (name.contains("_co_") || name.contains("div_scale"));
        let reads: Vec<u32> = instruction
            .operands
            .iter()
            .skip(if second { 2 } else { 1 })
            .filter_map(code)
            .chain(
                (family == "SOPP" && (name.contains("vcc") || name.contains("exec")))
                    .then_some([VCC as u32, EXEC as u32])
                    .into_iter()
                    .flatten(),
            )
            .collect();
        if !(vector && own_bit)
            && reads
                .iter()
                .any(|r| masks.contains(r) || masks.contains(&(r + 1)))
        {
            return true;
        }
        if vector {
            // What it writes as a mask: its destination when that is not a vector register, and a
            // carry-out's or a divide-scale's second operand.
            for operand in instruction.operands.iter().take(if second { 2 } else { 1 }) {
                if let Some(written) = code(operand) {
                    masks.extend([written, written + 1]);
                }
            }
            if name.starts_with("v_cmpx") {
                masks.extend([EXEC as u32, EXEC as u32 + 1]);
            }
            if family == "VOPC" {
                masks.extend([VCC as u32, VCC as u32 + 1]);
            }
        } else if let Some(written) = instruction.operands.first().and_then(code) {
            // A scalar write replaces whatever mask was there.
            masks.remove(&written);
        }
    }
    false
}

/// An instruction's family and mnemonic.
/// The first instruction of `decode` the tables give no name, described for a refusal: its offset,
/// its encoding family and opcode where the family is known, and its words - what a request for
/// the opcode quotes. `None` where every instruction is named.
#[must_use]
pub fn first_unnamed(decode: &Decode, encodings: &EncodingTable) -> Option<String> {
    let instruction = decode
        .instructions
        .iter()
        .find(|instruction| named(encodings, instruction).is_none())?;
    let family = instruction
        .encoding
        .and_then(|i| encodings.encodings().get(usize::from(i)))
        .map_or("no encoding family", |family| family.name.as_str());
    let words: Vec<String> = instruction
        .words
        .iter()
        .map(|word| format!("{word:#010x}"))
        .collect();
    Some(format!(
        "the instruction at {:#x}, {family} opcode {:#x} ({})",
        instruction.offset,
        instruction.opcode,
        words.join(" ")
    ))
}

/// An opcode the tables give no name, as a program holds it: its family (`no encoding family`
/// where none matched), where it first stands, and how many instructions use it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unnamed {
    /// The encoding family.
    pub family: String,
    /// The opcode within it.
    pub opcode: u32,
    /// The byte of its first instruction.
    pub first: u32,
    /// How many instructions use it.
    pub count: usize,
}

impl std::fmt::Display for Unnamed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} opcode {:#x} at {:#x}",
            self.family, self.opcode, self.first
        )?;
        if self.count > 1 {
            write!(f, " ({} times)", self.count)?;
        }
        Ok(())
    }
}

/// Every opcode of `decode` the tables give no name, once each, in the order they first stand:
/// what a request to record them lists, so one round trip records them all.
#[must_use]
pub fn unnamed_opcodes(decode: &Decode, encodings: &EncodingTable) -> Vec<Unnamed> {
    let mut listed: Vec<Unnamed> = Vec::new();
    for instruction in &decode.instructions {
        if named(encodings, instruction).is_some() {
            continue;
        }
        let family = instruction
            .encoding
            .and_then(|i| encodings.encodings().get(usize::from(i)))
            .map_or("no encoding family", |family| family.name.as_str());
        let opcode = instruction.opcode;
        match listed
            .iter_mut()
            .find(|seen| seen.family == family && seen.opcode == opcode)
        {
            Some(seen) => seen.count += 1,
            None => listed.push(Unnamed {
                family: family.to_owned(),
                opcode,
                first: instruction.offset,
                count: 1,
            }),
        }
    }
    listed
}

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

/// Whether a program-control instruction leaves the straight line.
fn branches(name: &str) -> bool {
    name.starts_with("s_branch")
        || name.starts_with("s_cbranch")
        || name.starts_with("s_setpc")
        || name.starts_with("s_endpgm")
}

/// A value the SPIR-V reader holds for an id.
#[derive(Debug, Clone)]
enum Value {
    Word(u32),
    Bool(bool),
    Pair(u32, u32),
}

/// Values by id. The builder numbers ids from one up, so they index a vector.
#[derive(Default)]
struct Values(Vec<Option<Value>>);

impl Values {
    fn get(&self, id: u32) -> Option<&Value> {
        self.0.get(id as usize)?.as_ref()
    }

    fn insert(&mut self, id: u32, value: Value) {
        let at = id as usize;
        if at >= self.0.len() {
            self.0.resize(at + 1, None);
        }
        self.0[at] = Some(value);
    }
}

/// What one lane of one vector register holds in a lane run.
#[derive(Debug, Clone, Copy, Default)]
enum Lane {
    /// Nothing the run did wrote it: what the module starts it at.
    #[default]
    Unwritten,
    /// Written with what the host cannot know.
    Unknown,
    /// Written with this.
    Known(u32),
}

/// Each vector register's value in each lane, indexed by register and lane.
#[derive(Default)]
struct VectorFile(Vec<Lane>);

impl VectorFile {
    fn get(&self, (register, lane): (u32, u32)) -> Lane {
        self.0
            .get((register * 64 + lane) as usize)
            .copied()
            .unwrap_or_default()
    }

    fn insert(&mut self, (register, lane): (u32, u32), value: Option<u32>) {
        let at = (register * 64 + lane) as usize;
        if at >= self.0.len() {
            self.0.resize(at + 1, Lane::Unwritten);
        }
        self.0[at] = value.map_or(Lane::Unknown, Lane::Known);
    }
}

/// The model the prefix runs through: registers that hold numbers, and a builder whose output is
/// read back and evaluated.
struct Concrete<'a> {
    encodings: &'a EncodingTable,
    builder: Builder,
    /// Values by id; an id with none is unknown.
    values: Values,
    /// How far into the builder's function section has been evaluated.
    read: usize,
    /// The operands of the instruction being evaluated.
    operands: Vec<u32>,
    scalars: Scalars,
    /// Each vector register's value in each lane once written, [`None`] where unknown; only a lane
    /// run writes them.
    vectors: VectorFile,
    /// Whether vector writes are kept (a lane run, D758).
    lanes: bool,
    /// How many lanes the wave has.
    width: u32,
    /// How many of them are the draw's vertices, whose seeded ids are unknown.
    vertices: u32,
    /// How many lanes, from lane zero, the run models.
    modelled: u32,
    /// Where the program was loaded, for `s_getpc_b64` (D842).
    program_address: Option<u64>,
    condition: Option<u32>,
    instructions: usize,
    types: [Id; 6],
    pointers: [Id; 4],
}

/// Indices into [`Concrete::types`].
const U32: usize = 0;
const F32: usize = 1;
const BOOL: usize = 2;
const F16: usize = 3;
const U16: usize = 4;
const GLSL: usize = 5;
/// Indices into [`Concrete::pointers`].
const CONDITION: usize = 0;
const COUNTER: usize = 1;
const MEMORY: usize = 2;
const ELEMENT: usize = 3;

impl<'a> Concrete<'a> {
    fn new(encodings: &'a EncodingTable, instructions: usize) -> Self {
        let mut builder = Builder::new();
        let types = std::array::from_fn(|_| builder.id());
        let pointers = std::array::from_fn(|_| builder.id());
        Self {
            encodings,
            builder,
            values: Values::default(),
            read: 0,
            operands: Vec::new(),
            scalars: [None; SCALAR_CODES],
            vectors: VectorFile::default(),
            lanes: false,
            width: 64,
            vertices: 0,
            modelled: 64,
            program_address: None,
            condition: None,
            instructions,
            types,
            pointers,
        }
    }

    /// A fresh id holding `value`, or unknown for `None`.
    fn hold(&mut self, value: Option<u32>) -> Id {
        let id = self.builder.id();
        if let Some(value) = value {
            self.values.insert(id.0, Value::Word(value));
        }
        id
    }

    /// The number an id holds, evaluating what was emitted since the last look.
    fn word(&mut self, id: Id) -> Option<u32> {
        self.catch_up();
        match self.values.get(id.0)? {
            Value::Word(value) => Some(*value),
            Value::Bool(value) => Some(u32::from(*value)),
            Value::Pair(..) => None,
        }
    }

    /// Evaluates every instruction emitted since the last call, in order.
    fn catch_up(&mut self) {
        while self.read < self.builder.function_words().len() {
            let words = self.builder.function_words();
            let head = words[self.read];
            let count = (head >> 16) as usize;
            if count == 0 || self.read + count > words.len() {
                self.read = words.len();
                return;
            }
            // Through a buffer kept between instructions: a lane run reads thousands.
            let mut operands = std::mem::take(&mut self.operands);
            operands.clear();
            operands.extend_from_slice(&words[self.read + 1..self.read + count]);
            self.read += count;
            self.evaluate((head & 0xffff) as u16, &operands);
            self.operands = operands;
        }
    }

    /// Evaluates one emitted instruction. One this does not know leaves its result unknown.
    fn evaluate(&mut self, opcode: u16, operands: &[u32]) {
        if opcode == op::STORE {
            if let [pointer, value] = operands
                && *pointer == self.pointers[CONDITION].0
            {
                self.condition = self.number(*value);
            }
            return;
        }
        let [_, result, arguments @ ..] = operands else {
            return;
        };
        if let Some(value) = self.compute(opcode, arguments) {
            self.values.insert(*result, value);
        }
    }

    fn number(&self, id: u32) -> Option<u32> {
        match self.values.get(id)? {
            Value::Word(value) => Some(*value),
            Value::Bool(value) => Some(u32::from(*value)),
            Value::Pair(..) => None,
        }
    }

    fn truth(&self, id: u32) -> Option<bool> {
        match self.values.get(id)? {
            Value::Bool(value) => Some(*value),
            Value::Word(value) => Some(*value != 0),
            Value::Pair(..) => None,
        }
    }

    fn compute(&self, opcode: u16, arguments: &[u32]) -> Option<Value> {
        let word = |at: usize| arguments.get(at).and_then(|id| self.number(*id));
        let truth = |at: usize| arguments.get(at).and_then(|id| self.truth(*id));
        let signed = |value: u32| i32::from_ne_bytes(value.to_ne_bytes());
        Some(match opcode {
            op::LOAD => {
                if arguments.first() == Some(&self.pointers[CONDITION].0) {
                    Value::Word(self.condition?)
                } else {
                    return None;
                }
            }
            op::BITCAST => self.values.get(*arguments.first()?)?.clone(),
            op::IADD => Value::Word(word(0)?.wrapping_add(word(1)?)),
            op::ISUB => Value::Word(word(0)?.wrapping_sub(word(1)?)),
            op::IMUL => Value::Word(word(0)?.wrapping_mul(word(1)?)),
            op::UDIV => Value::Word(word(0)?.checked_div(word(1)?)?),
            op::SHIFT_LEFT_LOGICAL => Value::Word(word(0)?.checked_shl(word(1)?)?),
            op::SHIFT_RIGHT_LOGICAL => Value::Word(word(0)?.checked_shr(word(1)?)?),
            op::SHIFT_RIGHT_ARITHMETIC => Value::Word(u32::from_ne_bytes(
                signed(word(0)?).checked_shr(word(1)?)?.to_ne_bytes(),
            )),
            op::BITWISE_AND => Value::Word(word(0)? & word(1)?),
            op::BITWISE_OR => Value::Word(word(0)? | word(1)?),
            op::BITWISE_XOR => Value::Word(word(0)? ^ word(1)?),
            op::NOT => Value::Word(!word(0)?),
            op::BIT_COUNT => Value::Word(word(0)?.count_ones()),
            op::BIT_REVERSE => Value::Word(word(0)?.reverse_bits()),
            BIT_FIELD_UEXTRACT | BIT_FIELD_SEXTRACT => {
                let (value, offset, count) = (word(0)?, word(1)?, word(2)?);
                if count == 0 {
                    Value::Word(0)
                } else if offset.checked_add(count)? > 32 {
                    return None;
                } else {
                    let shifted = value >> offset;
                    let field = shifted & (u32::MAX >> (32 - count));
                    let top = 1u32 << (count - 1);
                    if opcode == BIT_FIELD_SEXTRACT && field & top != 0 {
                        Value::Word(field | !(u32::MAX >> (32 - count)))
                    } else {
                        Value::Word(field)
                    }
                }
            }
            BIT_FIELD_INSERT => {
                let (base, insert, offset, count) = (word(0)?, word(1)?, word(2)?, word(3)?);
                if count == 0 {
                    Value::Word(base)
                } else if offset.checked_add(count)? > 32 {
                    return None;
                } else {
                    let mask = (u32::MAX >> (32 - count)) << offset;
                    Value::Word((base & !mask) | ((insert << offset) & mask))
                }
            }
            op::SELECT => {
                let chosen = if truth(0)? { 1 } else { 2 };
                self.values.get(*arguments.get(chosen)?)?.clone()
            }
            op::IEQUAL => Value::Bool(word(0)? == word(1)?),
            op::INOT_EQUAL => Value::Bool(word(0)? != word(1)?),
            op::ULESS_THAN => Value::Bool(word(0)? < word(1)?),
            ULESS_THAN_EQUAL => Value::Bool(word(0)? <= word(1)?),
            op::UGREATER_THAN => Value::Bool(word(0)? > word(1)?),
            op::UGREATER_THAN_EQUAL => Value::Bool(word(0)? >= word(1)?),
            op::SLESS_THAN => Value::Bool(signed(word(0)?) < signed(word(1)?)),
            op::SLESS_THAN_EQUAL => Value::Bool(signed(word(0)?) <= signed(word(1)?)),
            op::SGREATER_THAN => Value::Bool(signed(word(0)?) > signed(word(1)?)),
            op::SGREATER_THAN_EQUAL => Value::Bool(signed(word(0)?) >= signed(word(1)?)),
            op::LOGICAL_OR => Value::Bool(truth(0)? || truth(1)?),
            op::LOGICAL_AND => Value::Bool(truth(0)? && truth(1)?),
            op::LOGICAL_NOT => Value::Bool(!truth(0)?),
            LOGICAL_EQUAL => Value::Bool(truth(0)? == truth(1)?),
            LOGICAL_NOT_EQUAL => Value::Bool(truth(0)? != truth(1)?),
            IADD_CARRY => {
                let (sum, carried) = word(0)?.overflowing_add(word(1)?);
                Value::Pair(sum, u32::from(carried))
            }
            ISUB_BORROW => {
                let (difference, borrowed) = word(0)?.overflowing_sub(word(1)?);
                Value::Pair(difference, u32::from(borrowed))
            }
            UMUL_EXTENDED => {
                let product = u64::from(word(0)?) * u64::from(word(1)?);
                Value::Pair(product as u32, (product >> 32) as u32)
            }
            op::COMPOSITE_EXTRACT => {
                match (self.values.get(*arguments.first()?)?, arguments.get(1)) {
                    (Value::Pair(low, _), Some(0)) => Value::Word(*low),
                    (Value::Pair(_, high), Some(1)) => Value::Word(*high),
                    _ => return None,
                }
            }
            _ => return self.compute_float(opcode, arguments),
        })
    }

    /// The float operations [`Self::compute`] leaves: floats are held as their bits. The host's
    /// float arithmetic is exact where a device's may round a division differently in the last
    /// place; an integer division corrects for that, and an address formed otherwise is checked by
    /// the run it feeds.
    fn compute_float(&self, opcode: u16, arguments: &[u32]) -> Option<Value> {
        let word = |at: usize| arguments.get(at).and_then(|id| self.number(*id));
        let float = |at: usize| word(at).map(f32::from_bits);
        Some(match opcode {
            op::FADD | op::FSUB | op::FMUL | op::FDIV => {
                let (a, b) = (float(0)?, float(1)?);
                Value::Word(
                    match opcode {
                        op::FADD => a + b,
                        op::FSUB => a - b,
                        op::FMUL => a * b,
                        _ => a / b,
                    }
                    .to_bits(),
                )
            }
            op::CONVERT_U_TO_F => Value::Word(unsigned_to_float(word(0)?).to_bits()),
            op::CONVERT_S_TO_F => {
                Value::Word(signed_to_float(i32::from_ne_bytes(word(0)?.to_ne_bytes())).to_bits())
            }
            // Defined only in range (SPIR-V 3.42.11); the translation saturates before it.
            op::CONVERT_F_TO_U => Value::Word(float_to_unsigned(float(0)?)?),
            op::CONVERT_F_TO_S => Value::Word(u32::from_ne_bytes(
                float_to_signed(float(0)?)?.to_ne_bytes(),
            )),
            _ => Value::Bool(spirv_float_compare(opcode, float(0)?, float(1)?)?),
        })
    }

    /// A scalar load from memory: `s_load_dword*` reads the words at the base pair, its immediate
    /// and its scalar offset; any other scalar memory instruction leaves its destination unknown.
    fn scalar_load(
        &mut self,
        instruction: &Instruction,
        name: &str,
        read: &mut dyn FnMut(u64) -> Option<u32>,
    ) {
        let words = match name.rsplit_once("dwordx") {
            Some((_, count)) => count.parse().unwrap_or(16),
            None => 1,
        };
        // Whatever the destination is named - a register or a lane mask's half, as PPSA04263's
        // `s_buffer_load_dword vcc_lo` is - its code is the word's `SDATA`, bits 12:6, and the
        // scalar codes number `vcc` as they number a register.
        let destination = (instruction.word >> 6 & 0x7f) as usize;
        let [_, Operand::Scalar(base), Operand::Immediate(offset), ..] =
            instruction.operands.as_slice()
        else {
            self.forget(destination, 16);
            return;
        };
        let soffset = match instruction.second_word.map(|second| (second >> 25) & 0x7f) {
            Some(SOFFSET_NULL) | None => Some(0),
            Some(code) => self.scalars.get(code as usize).copied().flatten(),
        };
        let base = usize::from(*base);
        let address = name
            .starts_with("s_load_dword")
            .then_some(())
            .and(self.scalars[base].zip(self.scalars.get(base + 1).copied().flatten()))
            .zip(soffset)
            .map(|((low, high), soffset)| {
                let base = (u64::from(high & 0xffff) << 32) | u64::from(low);
                base.wrapping_add_signed(*offset)
                    .wrapping_add(u64::from(soffset))
                    & !3
            });
        if name.starts_with("s_buffer_load_dword") {
            self.buffer_load(destination, (base, *offset, soffset), words, read);
            return;
        }
        for step in 0..words {
            let value = address.and_then(|address| read(address + 4 * step as u64));
            if let Some(slot) = self.scalars.get_mut(destination + step) {
                *slot = value;
            }
        }
    }

    /// `s_buffer_load_dword*` through the buffer descriptor at scalar `base`: the words at its base
    /// address plus the offsets, each read where it lies within the descriptor's record count - in
    /// bytes with no stride, in strides with one, as the translator's scalar buffer load bounds it
    /// (radeonsi `si_state.c:3506-3527`) - and zero past it. Unknown where the descriptor or the
    /// scalar offset is.
    fn buffer_load(
        &mut self,
        destination: usize,
        (base, offset, soffset): (usize, i64, Option<u32>),
        words: usize,
        read: &mut dyn FnMut(u64) -> Option<u32>,
    ) {
        let descriptor = [0, 1, 2].map(|word| self.scalars.get(base + word).copied().flatten());
        let place = match (descriptor, soffset) {
            ([Some(low), Some(high), Some(records)], Some(soffset)) => {
                let address = (u64::from(high & 0xffff) << 32) | u64::from(low);
                let stride = u64::from(high >> 16 & 0x3fff);
                let limit = if stride == 0 {
                    u64::from(records)
                } else {
                    u64::from(records) * stride
                };
                let start = u64::try_from(offset)
                    .ok()
                    .map(|offset| (offset + u64::from(soffset)) & !3);
                start.map(|start| (address, start, limit))
            }
            _ => None,
        };
        for step in 0..words {
            let value = place.and_then(|(address, start, limit)| {
                let at = start + 4 * step as u64;
                if at + 4 <= limit {
                    read(address + at)
                } else {
                    Some(0)
                }
            });
            if let Some(slot) = self.scalars.get_mut(destination + step) {
                *slot = value;
            }
        }
    }

    /// What vector `register` holds in `lane`: what was written, or else what the module starts it
    /// at - zero, but for the ids its geometry seeds in a vertex lane, which the host does not know.
    fn vector(&self, register: u32, lane: u32) -> Option<u32> {
        match self.vectors.get((register, lane)) {
            Lane::Known(value) => Some(value),
            Lane::Unknown => None,
            Lane::Unwritten if SEEDED_IDS.contains(&register) && lane < self.vertices => None,
            Lane::Unwritten => Some(0),
        }
    }

    /// Whether `lane` runs: its execution-mask bit, where known.
    fn lane_runs(&self, lane: u32) -> Option<bool> {
        let half = self.scalars[EXEC + (lane / 32) as usize]?;
        Some(half & (1 << (lane % 32)) != 0)
    }

    /// The full address each active lane below `lanes` forms at a global load with no scalar base:
    /// its vector address pair plus its immediate offset. [`None`] where a mask bit or an active
    /// lane's address is unknown.
    fn load_addresses(&self, instruction: &Instruction, lanes: u32) -> Option<Vec<u64>> {
        let Some(Operand::Vector(pair)) = instruction.operands.get(1) else {
            return None;
        };
        let offset = match instruction.operands.get(3) {
            Some(Operand::Immediate(offset)) => *offset,
            _ => 0,
        };
        let mut addresses = Vec::new();
        for lane in 0..lanes.min(self.modelled) {
            if !self.lane_runs(lane)? {
                continue;
            }
            let low = self.vector(u32::from(*pair), lane)?;
            let high = self.vector(u32::from(*pair) + 1, lane)?;
            let address = (u64::from(high) << 32) | u64::from(low);
            addresses.push(address.wrapping_add_signed(offset));
        }
        Some(addresses)
    }

    /// Forgets `count` vector registers from the one `operand` names, in every lane.
    fn forget_vector(&mut self, operand: Option<&Operand>, count: u32) {
        if let Some(Operand::Vector(first)) = operand {
            let first = u32::from(*first);
            for register in first..first + count {
                for lane in 0..self.width {
                    self.vectors.insert((register, lane), None);
                }
            }
        }
    }

    /// Forgets `count` registers from `first`.
    fn forget(&mut self, first: usize, count: usize) {
        for slot in self.scalars.iter_mut().skip(first).take(count) {
            *slot = None;
        }
    }

    /// Forgets what an instruction the semantics could not run may have written: its destination,
    /// as wide as a pair, and the condition code.
    fn forget_written(&mut self, instruction: &Instruction, name: &str) {
        if name.starts_with("s_movrel") {
            self.scalars = [None; SCALAR_CODES];
        }
        self.forget_operand(instruction.operands.first());
        if model::writes_condition_code(name) || name.starts_with("s_cmp") {
            self.condition = None;
        }
    }

    /// Forgets the scalars a vector instruction writes: a scalar or mask destination in its first
    /// two operands (a compare's mask, a carry-out, a lane read), and the execution mask for a
    /// compare into it.
    /// A float compare whose sources are all uniform - scalar registers and constants - gives
    /// every lane the same answer, so its mask is zero where it is false and the execution mask
    /// where it is true (the lanes that compare): kept where those are known (D753). The compare's
    /// SDWA form names its sources and its destination pair in its modifier word, as ACO encodes
    /// them; its long form names them as operands, with no modifier. `false` where the compare is
    /// not one of these, and nothing is then written.
    fn uniform_compare(&mut self, instruction: &Instruction, name: &str) -> bool {
        let Some(compare) = float_compare(name) else {
            return false;
        };
        let (sources, destination) = if instruction.word & 0x1ff == SDWA_SOURCE {
            let Some(word) = instruction.second_word else {
                return false;
            };
            // Whole registers only, and no clamp - bit 13 of a compare into `vcc`, of the
            // destination otherwise: anything else is not a plain compare.
            if word >> 16 & 0xf != 6
                || word >> 24 & 0xf != 6
                || word >> 15 & 1 == 0 && word >> 13 & 1 != 0
            {
                return false;
            }
            if word >> 20 & 0x3 != 0
                || word >> 28 & 0x3 != 0
                || word >> 23 & 1 == 0
                || word >> 31 == 0
            {
                return false;
            }
            let destination = if word >> 15 & 1 != 0 {
                (word >> 8 & 0x7f) as usize
            } else {
                VCC
            };
            ([word & 0xff, instruction.word >> 9 & 0xff], destination)
        } else {
            return false;
        };
        let [Some(first), Some(second)] = sources.map(|code| self.uniform_source(code)) else {
            self.forget(destination, 2);
            return true;
        };
        let (first, second) = (f32::from_bits(first), f32::from_bits(second));
        let holds = compare(first, second);
        let (low, high) = if holds {
            (self.scalars[EXEC], self.scalars[EXEC + 1])
        } else {
            (Some(0), Some(0))
        };
        if destination + 1 < SCALAR_CODES {
            self.scalars[destination] = low;
            self.scalars[destination + 1] = high;
        }
        true
    }

    /// The value a uniform source code names, where the host knows it: a scalar register, an
    /// inline integer or an inline float.
    fn uniform_source(&self, code: u32) -> Option<u32> {
        match code {
            0..=127 => self.scalars[code as usize],
            128..=192 => Some(code - 128),
            193..=208 => Some((192_i32 - code as i32) as u32),
            240..=248 => {
                let names = [
                    "0.5", "-0.5", "1.0", "-1.0", "2.0", "-2.0", "4.0", "-4.0", "inv_2pi",
                ];
                model::inline_float_bits(names[(code - 240) as usize])
            }
            _ => None,
        }
    }

    fn forget_vector_writes(&mut self, instruction: &Instruction, name: &str) {
        if name.starts_with("v_cmp")
            && !name.starts_with("v_cmpx")
            && self.uniform_compare(instruction, name)
        {
            return;
        }
        // The destination, a scalar where the instruction reads a lane into one; and a second, a
        // carry-out or a pre-scale flag, only where the operand layout says the instruction has
        // one - otherwise the second operand is a source, read and not written.
        let written = if model::has_scalar_destination(self.encodings, instruction) {
            2
        } else {
            1
        };
        for operand in instruction.operands.iter().take(written) {
            self.forget_operand(Some(operand));
        }
        // The short carry forms write their carry into the condition mask, which no operand names.
        if name.contains("_co_") && name.ends_with("_e32") {
            self.forget(VCC, 2);
        }
        // A compare's SDWA form writes the pair its modifier word names, not the condition mask.
        if instruction.word & 0x1ff == SDWA_SOURCE
            && let Some(word) = instruction.second_word
            && word >> 15 & 1 != 0
        {
            self.forget((word >> 8 & 0x7f) as usize, 2);
        }
        if name.starts_with("v_cmpx") {
            self.forget(EXEC, 2);
        }
        if name.starts_with("v_cmp") {
            self.forget(VCC, 2);
        }
    }

    fn forget_operand(&mut self, operand: Option<&Operand>) {
        match operand {
            Some(Operand::Scalar(register)) => self.forget(usize::from(*register), 2),
            Some(Operand::Named(name)) => {
                if let Some(code) = mask_code(name) {
                    self.forget(code, 2);
                } else if name == model::M0 {
                    self.forget(M0, 1);
                } else if let Some(high) = model::lane_mask_high_name(name).and_then(mask_code) {
                    self.forget(high + 1, 1);
                }
            }
            _ => {}
        }
    }
}

/// The first-source code that marks an instruction's SDWA form.
const SDWA_SOURCE: u32 = 0xf9;

/// The comparison a short-form float compare makes, ordered but for `neq`, which a NaN satisfies.
fn float_compare(name: &str) -> Option<fn(f32, f32) -> bool> {
    Some(match name {
        "v_cmp_lt_f32_e32" => |a, b| a < b,
        "v_cmp_eq_f32_e32" => |a, b| a.partial_cmp(&b) == Some(std::cmp::Ordering::Equal),
        "v_cmp_le_f32_e32" => |a, b| a <= b,
        "v_cmp_gt_f32_e32" => |a, b| a > b,
        "v_cmp_ge_f32_e32" => |a, b| a >= b,
        "v_cmp_neq_f32_e32" => |a, b| a.partial_cmp(&b) != Some(std::cmp::Ordering::Equal),
        _ => return None,
    })
}

/// The register code of a lane mask's low half, by any spelling of it.
fn mask_code(name: &str) -> Option<usize> {
    match model::lane_mask_name(name)? {
        model::EXEC_LOW_HALF => Some(EXEC),
        model::VCC_LOW_HALF => Some(VCC),
        _ => None,
    }
}

impl Model for Concrete<'_> {
    fn encodings(&self) -> &EncodingTable {
        self.encodings
    }

    fn lane_known_idle(&self, lane: u32) -> bool {
        self.lanes && self.lane_runs(lane) == Some(false)
    }

    fn program_address(&self) -> Option<u64> {
        self.program_address
    }

    fn lanes(&self) -> u32 {
        self.width
    }

    fn modelled_lanes(&self) -> u32 {
        self.modelled
    }

    fn constant(&mut self, value: u32) -> Id {
        self.hold(Some(value))
    }

    fn read_source(
        &mut self,
        instruction: &Instruction,
        operand: &Operand,
        lane: u32,
    ) -> Result<Id, TranslateError> {
        Ok(match operand {
            Operand::Integer(value) => {
                let value = i32::try_from(*value).map_err(|_| TranslateError::Unsupported {
                    offset: instruction.offset,
                    detail: "inline constant does not fit in a register",
                })?;
                self.hold(Some(u32::from_ne_bytes(value.to_ne_bytes())))
            }
            Operand::Literal(value) => self.hold(Some(*value)),
            Operand::Scalar(register) => self.read_scalar(u32::from(*register)),
            Operand::Vector(register) => {
                let value = self.vector(u32::from(*register), lane);
                self.hold(value)
            }
            Operand::Named(name) => {
                if let Some(code) = mask_code(name) {
                    self.hold(self.scalars[code])
                } else if let Some(code) = model::lane_mask_high_name(name).and_then(mask_code) {
                    self.hold(self.scalars[code + 1])
                } else if name == model::M0 {
                    self.hold(self.scalars[M0])
                } else {
                    self.hold(name.parse::<f32>().ok().map(f32::to_bits))
                }
            }
            _ => self.hold(None),
        })
    }

    fn write_vector_lane(&mut self, register: u32, lane: u32, value: Id) {
        if !self.lanes {
            return;
        }
        // As the module writes a lane: not at all when it is known not to run, and unknown when
        // whether it runs is.
        let value = match self.lane_runs(lane) {
            Some(false) => return,
            Some(true) => self.word(value),
            None => None,
        };
        self.vectors.insert((register, lane), value);
    }

    // A lane read where the lane is known, in a lane run: that lane's value, unknown where it is or
    // where the lane is not modelled. An unknown lane, or a run that keeps no vector registers (a
    // prefix, D753), is refused, and the destination forgotten.
    fn read_vector_at(&mut self, register: u32, lane: Id) -> Option<Id> {
        if !self.lanes {
            return None;
        }
        let lane = self.word(lane)?;
        let value = (lane < self.modelled)
            .then(|| self.vector(register, lane))
            .flatten();
        Some(self.hold(value))
    }

    fn write_scalar(&mut self, register: u32, value: Id) {
        let value = self.word(value);
        if let Some(slot) = self.scalars.get_mut(register as usize) {
            *slot = value;
        }
    }

    fn count(&mut self) {}

    fn builder(&mut self) -> &mut Builder {
        &mut self.builder
    }

    fn u32_type(&self) -> Id {
        self.types[U32]
    }

    fn f32_type(&self) -> Id {
        self.types[F32]
    }

    fn glsl_set(&mut self) -> Id {
        self.types[GLSL]
    }

    fn f16_type(&mut self) -> Id {
        self.types[F16]
    }

    fn u16_type(&mut self) -> Id {
        self.types[U16]
    }

    fn read_local(&mut self, _word_index: Id) -> Result<Id, TranslateError> {
        Ok(self.hold(None))
    }

    fn write_local(
        &mut self,
        _word_index: Id,
        _value: Id,
        _lane: u32,
    ) -> Result<(), TranslateError> {
        Ok(())
    }

    fn memory_buffer(&self) -> Id {
        self.pointers[MEMORY]
    }

    fn memory_element_ptr(&self) -> Id {
        self.pointers[ELEMENT]
    }

    fn read_scalar(&mut self, register: u32) -> Id {
        let value = self.scalars.get(register as usize).copied().flatten();
        self.hold(value)
    }

    fn read_lane_mask(&mut self, name: &str) -> Result<(Id, Id), TranslateError> {
        let code = mask_code(name).unwrap_or(VCC);
        let (low, high) = (self.scalars[code], self.scalars[code + 1]);
        Ok((self.hold(low), self.hold(high)))
    }

    fn write_lane_mask(&mut self, name: &str, low: Id, high: Id) -> Result<(), TranslateError> {
        let code = mask_code(name).unwrap_or(VCC);
        self.scalars[code] = self.word(low);
        self.scalars[code + 1] = self.word(high);
        Ok(())
    }

    fn write_memory(&mut self, _word_index: Id, _value: Id, _lane: u32) {}

    fn memory_words(&self) -> u32 {
        0
    }

    fn bool_type(&mut self) -> Id {
        self.types[BOOL]
    }

    fn condition_code(&mut self) -> Id {
        // A read emits a load of this pointer, evaluated in order with the stores before it.
        self.catch_up();
        self.pointers[CONDITION]
    }

    fn read_m0(&mut self) -> Id {
        self.hold(self.scalars[M0])
    }

    fn write_m0(&mut self, value: Id) {
        self.scalars[M0] = self.word(value);
    }

    fn program_counter(&mut self) -> Id {
        self.pointers[COUNTER]
    }

    fn instructions(&self) -> usize {
        self.instructions
    }
}

#[cfg(test)]
mod tests {
    use super::{Scalars, VCC, lane_addresses, scalar_prefix};
    use orbistoun_shader::Decode;
    use orbistoun_shader::{EncodingTable, OperandTable, decode_program};
    use std::collections::BTreeMap;

    /// A prefix the access is reached at, from no system registers.
    fn reached(
        decode: &Decode,
        encodings: &EncodingTable,
        until: u32,
        user_data: (u32, &[u32]),
        read: &mut dyn FnMut(u64) -> Option<u32>,
    ) -> Result<Scalars, &'static str> {
        scalar_prefix(decode, encodings, (until, None), user_data, &[], read)
            .map(|scalars| scalars.expect("the access is reached"))
    }

    /// A vector instruction forgets only the scalars it writes: `v_mul_f32_e32 v6, s3, v6` reads
    /// `s3` and writes `v6`, so a descriptor in `s[4:7]` after it is still the draw's. Taking every
    /// scalar in its first two operands for a destination forgot `s[3:4]`, and Grand Theft Auto V's
    /// pixel shader lost the descriptor it reads its constants through (the word below is its).
    #[test]
    fn a_vector_source_scalar_is_not_forgotten() {
        let encodings = EncodingTable::builtin().expect("encodings");
        let operands = OperandTable::builtin().expect("operands");
        let bytes: Vec<u8> = [0x100c_0c03_u32, 0xf420_0002, 0xfa00_0000, 0xbf81_0000]
            .iter()
            .flat_map(|w| w.to_le_bytes())
            .collect();
        let decode = decode_program(&bytes, &encodings, &operands);
        let access = decode.instructions[1].offset;
        let scalars = reached(
            &decode,
            &encodings,
            access,
            (3, &[9, 1, 2, 3, 4]),
            &mut |_| None,
        )
        .expect("evaluated");
        assert_eq!(
            [3, 4, 5, 6, 7].map(|register| scalars[register]),
            [9, 1, 2, 3, 4].map(Some)
        );
    }

    /// The prefix says which instruction left a register unknown at the access: a vector result
    /// read into `s4` by `v_readlane_b32 s4, v0, 0` at byte 0 before a load through `s[4:7]` - a
    /// draw buffer then names it rather than listing what might have. A register no step touched
    /// has no cause.
    #[test]
    fn a_register_left_unknown_names_the_instruction() {
        use super::{Unknown, scalar_prefix_explained};
        let encodings = EncodingTable::builtin().expect("encodings");
        let operands = OperandTable::builtin().expect("operands");
        let (family, opcode) = encodings
            .find_by_name("v_readlane_b32")
            .expect("v_readlane_b32");
        let encoding = encodings
            .encodings()
            .iter()
            .find(|encoding| encoding.name == family)
            .expect("its family");
        // v_readlane_b32 s4, v0, 0; s_buffer_load_dword s0, s[4:7], 0x0; s_endpgm.
        let words = [
            encoding.value | (opcode << encoding.opcode.shift) | 4,
            256 | (128 << 9),
            0xf420_0002,
            0xfa00_0000,
            0xbf81_0000,
        ];
        let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        let decode = decode_program(&bytes, &encodings, &operands);
        let access = decode.instructions[1].offset;
        let (scalars, unknowns) = scalar_prefix_explained(
            &decode,
            &encodings,
            (access, None),
            (4, &[1, 2, 3, 4]),
            &[],
            &mut |_| None,
        )
        .expect("evaluated")
        .expect("reached");
        // A destination the semantics do not run is forgotten as wide as a pair.
        assert_eq!((scalars[4], scalars[5], scalars[6]), (None, None, Some(3)));
        assert_eq!(unknowns[4], Some(Unknown::LeftBy(0)));
        assert_eq!(unknowns[5], Some(Unknown::LeftBy(0)));
        assert_eq!(unknowns[6], None);
        assert_eq!(unknowns[9], None, "never set, by nothing");
    }

    /// Every opcode a program holds that the tables give no name, once each with where it first
    /// stands and how often: what one recording round trip needs, where the first alone took one
    /// round trip an opcode. VOP1 opcodes 0xff and 0xfe are assigned to nothing.
    #[test]
    fn every_unnamed_opcode_is_listed_once() {
        use super::{Unnamed, unnamed_opcodes};
        let encodings = EncodingTable::builtin().expect("encodings");
        let operands = OperandTable::builtin().expect("operands");
        // ?? v0, 0 (0xff); ?? v0, 0 (0xfe); v_mov_b32 v0, 0; ?? v0, 0 (0xff); s_endpgm.
        let words = [
            0x7e01_fe80_u32,
            0x7e01_fc80,
            0x7e00_0280,
            0x7e01_fe80,
            0xbf81_0000,
        ];
        let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        let decode = decode_program(&bytes, &encodings, &operands);
        let listed = unnamed_opcodes(&decode, &encodings);
        assert_eq!(
            listed,
            [
                Unnamed {
                    family: "VOP1".to_owned(),
                    opcode: 0xff,
                    first: 0,
                    count: 2,
                },
                Unnamed {
                    family: "VOP1".to_owned(),
                    opcode: 0xfe,
                    first: 4,
                    count: 1,
                },
            ]
        );
        assert_eq!(listed[0].to_string(), "VOP1 opcode 0xff at 0x0 (2 times)");
        assert_eq!(listed[1].to_string(), "VOP1 opcode 0xfe at 0x4");
    }

    /// A wave whose execution mask the module seeds from merged_wave_info skips an access it
    /// branches past while any lane runs (D800): `s_mov_b32 exec_lo, s3; s_mov_b32 exec_hi, 0`,
    /// then the condition mask copied from it and `s_cbranch_vccnz 1` over the access. A draw of
    /// three vertices seeds a non-zero `s3`, and the access is passed; with no seed the mask is
    /// unknown and the branch is refused.
    #[test]
    fn an_access_a_seeded_wave_branches_past_is_not_reached() {
        let encodings = EncodingTable::builtin().expect("encodings");
        let operands = OperandTable::builtin().expect("operands");
        let bytes: Vec<u8> = [
            0xbefe_0303_u32,
            0xbeff_0380,
            0xbeea_037e,
            0xbeeb_037f,
            0xbf87_0001,
            0xbe84_0385,
            0xbe85_0387,
            0xbf81_0000,
        ]
        .iter()
        .flat_map(|w| w.to_le_bytes())
        .collect();
        let decode = decode_program(&bytes, &encodings, &operands);
        let access = decode.instructions[5].offset;
        let seeds = crate::wavefront::geometry_system_scalars(3, 1);
        let passed = scalar_prefix(
            &decode,
            &encodings,
            (access, None),
            (8, &[]),
            &seeds,
            &mut |_| None,
        );
        assert_eq!(
            passed,
            Ok(None),
            "a wave with lanes branches past the access"
        );
        assert!(
            scalar_prefix(
                &decode,
                &encodings,
                (access, None),
                (8, &[]),
                &[],
                &mut |_| None
            )
            .is_err(),
            "an unseeded mask leaves the branch unknown"
        );
    }

    /// A fetch shader's prefix, as PPSA28061's vertex shader forms its vertex descriptor: a word
    /// from the table at `s[10:11]` picks the descriptor's place in the table at `s[8:9]`, and the
    /// descriptor's fourth word is rebuilt from a constant table under a condition.
    const FETCH: [u32; 20] = [
        0xf404_0105,
        0xfa00_0000, // s_load_dwordx2 s[4:5], s[10:11], 0x0
        0xbf8c_c07f, // s_waitcnt lgkmcnt(0)
        0x93ea_ff04,
        0x0002_0005, // s_bfe_u32 vcc_lo, s4, 0x20005
        0x98eb_ff6a,
        0x000c_0000, // s_lshl4_add_u32 vcc_hi, vcc_lo, 0xc0000
        0xf408_0004,
        0xd600_0000, // s_load_dwordx4 s[0:3], s[8:9], vcc_hi
        0xbf8c_c07f, // s_waitcnt lgkmcnt(0)
        0xbe86_03ff,
        0x022c_0204, // s_mov_b32 s6, 0x022c0204
        0xbe87_03ff,
        0x0fac_03ac, // s_mov_b32 s7, 0x0fac03ac
        0x948c_ff06,
        0x0008_0004, // s_bfe_u64 s[12:13], s[6:7], 0x80004
        0xbf06_8004, // s_cmp_eq_u32 s4, 0
        0x8503_0c03, // s_cselect_b32 s3, s3, s12
        0xe00c_2000,
        0x6b00_0000, // buffer_load_format_xyzw v0, v0, s[0:3], vcc_hi idxen
    ];

    /// GCTS00001's legacy streamout vertex shader (radeonsi through ACO, gfx10 wave32): its store
    /// sits behind `s_cbranch_execz` on the lanes past the wave's vertex count, a branch that jumps
    /// past the store, and its descriptor is loaded from the internal table at `{s0, 4}` (D861).
    const STREAMOUT_VS: [u32; 28] = [
        0xbf8f_0003,
        0x0201_00c1,
        0x9389_ff09,
        0x0007_0010,
        0xbe81_037e,
        0xbfa3_fffe,
        0x7d28_0009,
        0xbf88_000c,
        0xbe82_0301,
        0xbe81_0384,
        0xf408_0100,
        0xfa00_0000,
        0xd76d_0000,
        0x0400_140b,
        0x7e02_0288,
        0x3400_0082,
        0xbf8c_c07f,
        0xe070_1000,
        0x8041_0100,
        0xbe81_0302,
        0xbfa3_ffe3,
        0xbefe_0301,
        0x7e00_02f2,
        0x7e02_0280,
        0xf800_18cf,
        0x0001_0101,
        0xbf81_0000,
        0xbf81_0000,
    ];

    /// A branch on the execution mask that lands before the access is followed both ways (D753):
    /// `s_mov_b32 s0, 5; s_cbranch_execz 1; s_mov_b32 s1, 7`, then the access. Both ways leave `s0`
    /// five, so it is known; only the way that falls through sets `s1`, so it is not. PPSA21564's
    /// compute program at `0x50053c700` loads its descriptor so, after a skippable block.
    #[test]
    fn an_execz_branch_before_the_access_is_followed_both_ways() {
        let encodings = EncodingTable::builtin().expect("encodings");
        let operands = OperandTable::builtin().expect("operands");
        let words = [0xbe80_0385_u32, 0xbf88_0001, 0xbe81_0387, 0xbf81_0000];
        let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        let decode = decode_program(&bytes, &encodings, &operands);
        let access = decode.instructions.last().expect("the access").offset;
        let scalars = reached(&decode, &encodings, access, (0, &[]), &mut |_| None)
            .expect("both ways reach the access");
        assert_eq!(scalars[0], Some(5), "s0 is five either way");
        assert_eq!(scalars[1], None, "s1 is seven one way only");
    }

    #[test]
    fn a_streamout_store_past_an_execz_branch_is_evaluated() {
        let encodings = EncodingTable::builtin().expect("encodings");
        let operands = OperandTable::builtin().expect("operands");
        let bytes: Vec<u8> = STREAMOUT_VS.iter().flat_map(|w| w.to_le_bytes()).collect();
        let decode = decode_program(&bytes, &encodings, &operands);
        let store = decode.instructions[14].offset;
        let table = 0x4_0000_1000_u64;
        let memory = BTreeMap::from([
            (table, 0x0300_0000),
            (table + 4, 4),
            (table + 8, 4),
            (table + 12, 0x3102_7fac),
        ]);
        let user_data = [table as u32, 0, 0, 0, 0, 0, 0, 0, 0, 1 << 16, 0, 0];
        let scalars = reached(
            &decode,
            &encodings,
            store,
            (0, &user_data),
            &mut |address| memory.get(&address).copied(),
        )
        .expect("the store is reached");
        assert_eq!(
            scalars[4..8],
            [Some(0x0300_0000), Some(4), Some(4), Some(0x3102_7fac)]
        );
    }

    #[test]
    fn a_computed_descriptor_is_evaluated_from_the_draws_memory() {
        let encodings = EncodingTable::builtin().expect("encodings");
        let operands = OperandTable::builtin().expect("operands");
        let bytes: Vec<u8> = FETCH.iter().flat_map(|w| w.to_le_bytes()).collect();
        let decode = decode_program(&bytes, &encodings, &operands);
        let access = decode.instructions.last().expect("the access").offset;

        let (index_table, descriptors) = (0x1000_0000u64, 0x2000_0000u64);
        // Bits 6:5 of the first word say 2, so the descriptor is at 0xc0000 + 2 * 16.
        let mut memory = BTreeMap::from([(index_table, 0x40), (index_table + 4, 0x7)]);
        for (step, word) in (0..).zip([0x3000_0000, 0x0010_0000, 100, 0x0003_0fac]) {
            memory.insert(descriptors + 0xc_0020 + 4 * step, word);
        }
        let user_data = [
            descriptors as u32,
            (descriptors >> 32) as u32,
            index_table as u32,
            (index_table >> 32) as u32,
        ];
        let scalars = reached(
            &decode,
            &encodings,
            access,
            (8, &user_data),
            &mut |address| memory.get(&address).copied(),
        )
        .expect("a straight-line prefix");
        assert_eq!(scalars[VCC + 1], Some(0xc_0020), "the descriptor's place");
        assert_eq!(
            scalars[0..3],
            [Some(0x3000_0000), Some(0x0010_0000), Some(100)]
        );
        // s4 is non-zero, so the condition is clear and the fourth word is bits 11:4 of the table.
        assert_eq!(scalars[3], Some(0x20));
    }

    /// The open-toolchain GL context's vertex prologue and one four-component attribute fetch
    /// (oops-sdk `glsl_vs.c`, `vs_emit_ngg_preamble` and `vs_load_attributes`): one triangle a wave,
    /// the vertex index `s12` plus the lane, and the attribute read from `base + stride * index`
    /// with the base and stride from the table at `s[10:11]`; three components or four by the
    /// table's fourth word.
    const GL_FETCH: [u32; 30] = [
        0xbfa0_0001, // s_inst_prefetch 0x1
        0xbe8d_037e, // s_mov_b32 s13, exec_lo
        0xbefc_03ff,
        0x0000_1003, // s_mov_b32 m0, 0x1003
        0xbf80_0000, // s_nop 0
        0xbf90_0009, // s_sendmsg sendmsg(MSG_GS_ALLOC_REQ)
        0xbefe_0381, // s_mov_b32 exec_lo, 1
        0x7e02_02ff,
        0x2028_0600, // v_mov_b32 v1, 0x20280600
        0xf800_0941,
        0x0000_0001, // exp prim v1, off, off, off done
        0xbf8c_ff0f, // s_waitcnt expcnt(0)
        0xbefe_0387, // s_mov_b32 exec_lo, 7
        0xd765_000e,
        0x0001_00c1, // v_mbcnt_lo_u32_b32 v14, -1, 0
        0x4a1c_1c0c, // v_add_nc_u32 v14, s12, v14
        0x7e02_0280, // v_mov_b32 v1, 0
        0xf408_0105,
        0xfa00_0000, // s_load_dwordx4 s[4:7], s[10:11], 0x0
        0xbf8c_c07f, // s_waitcnt lgkmcnt(0)
        0xd569_0004,
        0x0002_1c06, // v_mul_lo_u32 v4, s6, v14
        0xd70f_6a02,
        0x0002_0804, // v_add_co_u32 v2, vcc_lo, s4, v4
        0x5006_0205, // v_add_co_ci_u32_e32 v3, vcc_lo, s5, v1, vcc_lo
        0xbf06_8407, // s_cmp_eq_u32 s7, 4
        0xbf85_0004, // s_cbranch_scc1 4
        0x7e2e_02f2, // v_mov_b32 v23, 1.0
        0xdc3c_8000,
        0x147d_0002, // global_load_dwordx3 v[20:22], v[2:3], off
    ];
    /// What follows [`GL_FETCH`]: the branch over the four-component load, the load, and the end.
    const GL_FETCH_TAIL: [u32; 5] = [
        0xbf82_0002, // s_branch 2
        0xdc38_8000,
        0x147d_0002, // global_load_dwordx4 v[20:23], v[2:3], off
        0xbf8c_3f70, // s_waitcnt vmcnt(0)
        0xbf81_0000, // s_endpgm
    ];

    /// A short-form vector instruction by name: `name vD, src0, vS` (VOP2), or `name vD, src0`
    /// (VOP1) with `vsrc1` none.
    fn short_form(encodings: &EncodingTable, name: &str, dst: u32, src0: u32, vsrc1: u32) -> u32 {
        let (family, opcode) = encodings.find_by_name(name).expect(name);
        let encoding = encodings
            .encodings()
            .iter()
            .find(|encoding| encoding.name == family)
            .expect("its family");
        encoding.value | (opcode << encoding.opcode.shift) | (dst << 17) | (vsrc1 << 9) | src0
    }

    /// A compute wave's lanes form their addresses from their thread ids and their group's id, which
    /// the run seeds as the hardware loads them (D883): `v0` the thread id, `s2` the group id after
    /// two words of user data. Lanes past the wave's threads form none.
    #[test]
    fn a_compute_waves_lanes_form_addresses_from_their_ids() {
        use super::{WaveEntry, wave_lane_addresses};
        let encodings = EncodingTable::builtin().expect("encodings");
        let operands = OperandTable::builtin().expect("operands");
        let words = [
            // v_lshlrev_b32 v1, 2, v0; v_add_nc_u32 v1, s0, v1; v_add_nc_u32 v1, s2, v1.
            short_form(&encodings, "v_lshlrev_b32_e32", 1, 130, 0),
            short_form(&encodings, "v_add_nc_u32_e32", 1, 0, 1),
            short_form(&encodings, "v_add_nc_u32_e32", 1, 2, 1),
            // v_mov_b32 v2, s1.
            short_form(&encodings, "v_mov_b32_e32", 2, 1, 0),
            // global_load_dword v3, v[1:2], off.
            0xdc30_8000,
            0x037d_0001,
            0xbf81_0000,
        ];
        let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        let decode = decode_program(&bytes, &encodings, &operands);
        assert!(decode.is_trustworthy(), "the program decodes cleanly");
        let load = decode.instructions[4].offset;
        let program = super::LaneProgram::new(decode, &encodings, &[load]);
        // The second wave of a group of 104 threads: threads 64 to 103, in lanes 0 to 39; lane 2
        // has no thread, as a partial group leaves one.
        let wave = WaveEntry {
            group_ids: vec![5],
            threads: (64..104)
                .map(|thread| (thread != 66).then_some([thread, 0, 0]))
                .collect(),
            components: 1,
        };
        let found =
            wave_lane_addresses(&program, &encodings, &[0x1000, 7], (64, &wave), &mut |_| {
                None
            });
        let expected: Vec<u64> = (64..104)
            .filter(|&thread| thread != 66)
            .map(|thread| 0x7_0000_1000 + 5 + 4 * thread)
            .collect();
        assert_eq!(found, Ok(vec![(load, expected.clone())]));
        // The native run finds what the model finds, from the same entry (D850).
        assert_eq!(
            super::quick_wave_lane_addresses(&program, &[0x1000, 7], (64, &wave), &mut |_| None),
            Some(vec![(load, expected)])
        );
    }

    /// A mask a compare writes from what the run does not know names the compare: `v_cmp_lt_u32
    /// vcc, v5, v0` with `v5` loaded from memory leaves `vcc` unknown, and a branch on it says so.
    #[test]
    fn a_compare_the_run_cannot_know_is_named() {
        use super::{WaveEntry, wave_lane_addresses};
        let encodings = EncodingTable::builtin().expect("encodings");
        let operands = OperandTable::builtin().expect("operands");
        let words = [
            // global_load_dword v5, v[1:2], off.
            0xdc30_8000,
            0x057d_0001,
            short_form(&encodings, "v_cmp_lt_u32_e32", 0, 256 + 5, 0),
            // s_cbranch_vccz past the second load.
            0xbf86_0002,
            0xdc30_8000,
            0x037d_0001,
            0xbf81_0000,
        ];
        let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        let decode = decode_program(&bytes, &encodings, &operands);
        let loads = [decode.instructions[0].offset, decode.instructions[3].offset];
        let program = super::LaneProgram::new(decode, &encodings, &loads);
        let wave = WaveEntry {
            group_ids: Vec::new(),
            threads: vec![Some([0, 0, 0])],
            components: 1,
        };
        let refused = wave_lane_addresses(&program, &encodings, &[], (64, &wave), &mut |_| None)
            .expect_err("vcc is unknown");
        assert!(
            refused.contains(concat!(
                "vcc is unknown after `v_cmp_lt_u32_e32` at 0x8, which read v5 unknown after ",
                "`global_load_dword` at 0x0"
            )),
            "{refused}"
        );
        // A step that read what an earlier step left unknown carries that step's cause too:
        // `v_mov_b32 v6, v5` after the load, compared, names both.
        let words = [
            0xdc30_8000,
            0x057d_0001,
            short_form(&encodings, "v_mov_b32_e32", 6, 256 + 5, 0),
            short_form(&encodings, "v_cmp_lt_u32_e32", 0, 256 + 6, 0),
            0xbf86_0002,
            0xdc30_8000,
            0x037d_0001,
            0xbf81_0000,
        ];
        let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        let decode = decode_program(&bytes, &encodings, &operands);
        let loads = [decode.instructions[0].offset, decode.instructions[4].offset];
        let program = super::LaneProgram::new(decode, &encodings, &loads);
        let refused = wave_lane_addresses(&program, &encodings, &[], (64, &wave), &mut |_| None)
            .expect_err("vcc is unknown");
        assert!(
            refused.contains(concat!(
                "which read v6 unknown after `v_mov_b32_e32` at 0x8, which read v5 unknown after ",
                "`global_load_dword` at 0x0"
            )),
            "{refused}"
        );
    }

    /// The float operations an integer division goes through run on their bits: a conversion, a
    /// reciprocal, products and the conversion back. `s0` is 8: 8.0, its reciprocal 0.125, 8.0
    /// squared 64.0, times the reciprocal 8.0, and back to 8 - the address every lane forms.
    #[test]
    fn the_float_steps_of_a_division_are_run() {
        use super::{WaveEntry, wave_lane_addresses};
        let encodings = EncodingTable::builtin().expect("encodings");
        let operands = OperandTable::builtin().expect("operands");
        let words = [
            short_form(&encodings, "v_cvt_f32_u32_e32", 1, 0, 0),
            short_form(&encodings, "v_rcp_iflag_f32_e32", 3, 256 + 1, 0),
            short_form(&encodings, "v_mul_f32_e32", 1, 256 + 1, 1),
            short_form(&encodings, "v_mul_f32_e32", 1, 256 + 3, 1),
            short_form(&encodings, "v_cvt_u32_f32_e32", 1, 256 + 1, 0),
            // global_load_dword v3, v[1:2], off.
            0xdc30_8000,
            0x037d_0001,
            0xbf81_0000,
        ];
        let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        let decode = decode_program(&bytes, &encodings, &operands);
        let load = decode.instructions[5].offset;
        let program = super::LaneProgram::new(decode, &encodings, &[load]);
        let wave = WaveEntry {
            group_ids: Vec::new(),
            threads: vec![Some([0, 0, 0]), Some([1, 0, 0])],
            components: 1,
        };
        let found = wave_lane_addresses(&program, &encodings, &[8], (64, &wave), &mut |_| None);
        assert_eq!(found, Ok(vec![(load, vec![8, 8])]));
    }

    /// A loop whose branch back comes after its last load is followed through every pass: three
    /// passes form three addresses. One that runs past the step bound is refused by name, not
    /// answered with the addresses it reached, which its later passes read past.
    #[test]
    fn a_loop_past_the_step_bound_is_refused() {
        use super::{WaveEntry, wave_lane_addresses};
        let encodings = EncodingTable::builtin().expect("encodings");
        let operands = OperandTable::builtin().expect("operands");
        let looped = |bytes_read: u32| {
            [
                // s_mov_b32 s0, 0.
                0xbe80_0380,
                // loop: v_mov_b32 v1, s0; global_load_dword v3, v[1:2], off.
                short_form(&encodings, "v_mov_b32_e32", 1, 0, 0),
                0xdc30_8000,
                0x037d_0001,
                // s_add_u32 s0, s0, 4; s_cmp_lt_u32 s0, 400; s_cbranch_scc1 loop.
                0x8000_8400,
                0xbf0a_ff00,
                bytes_read,
                0xbf85_fff9,
                0xbf81_0000,
            ]
        };
        let decode_of = |words: [u32; 9]| {
            let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
            decode_program(&bytes, &encodings, &operands)
        };
        let decode = decode_of(looped(400));
        let names: Vec<&str> = decode
            .instructions
            .iter()
            .filter_map(|i| super::named(&encodings, i).map(|(_, name)| name))
            .collect();
        assert_eq!(
            names,
            [
                "s_mov_b32",
                "v_mov_b32_e32",
                "global_load_dword",
                "s_add_u32",
                "s_cmp_lt_u32",
                "s_cbranch_scc1",
                "s_endpgm"
            ],
            "the fixture"
        );
        let load = decode.instructions[2].offset;
        let program = super::LaneProgram::new(decode, &encodings, &[load]);
        let wave = WaveEntry {
            group_ids: Vec::new(),
            threads: vec![Some([0, 0, 0])],
            components: 1,
        };
        let refused = wave_lane_addresses(&program, &encodings, &[], (64, &wave), &mut |_| None)
            .expect_err("a hundred passes is past the bound");
        assert!(refused.contains("loops"), "{refused}");
        let short = super::LaneProgram::new(decode_of(looped(12)), &encodings, &[load]);
        assert_eq!(
            wave_lane_addresses(&short, &encodings, &[], (64, &wave), &mut |_| None),
            Ok(vec![(load, vec![0]), (load, vec![4]), (load, vec![8])]),
            "every pass"
        );
    }

    /// The native run carries out what the model does, step for step, over every operation an
    /// integer division and a copy loop take: a shift-add, the float reciprocal and its conversions,
    /// a lane read, the scalar products and difference, the high product, compares into `vcc` and,
    /// in their SDWA form, into a pair, both selects, the mask logic, the SDWA carry-in add and a
    /// branch on the execution mask (D850).
    #[test]
    fn the_native_run_carries_out_what_the_model_does() {
        use super::{WaveEntry, quick_wave_lane_addresses, wave_lane_addresses};
        let encodings = EncodingTable::builtin().expect("encodings");
        let operands = OperandTable::builtin().expect("operands");
        let family = |name: &str| {
            let (family, opcode) = encodings.find_by_name(name).expect(name);
            let encoding = encodings
                .encodings()
                .iter()
                .find(|encoding| encoding.name == family)
                .expect("its family");
            encoding.value | (opcode << encoding.opcode.shift)
        };
        let vop3 =
            |name: &str, dst: u32, [a, b, c]: [u32; 3]| [family(name) | dst, a | b << 9 | c << 18];
        let sop2 = |name: &str, dst: u32, a: u32, b: u32| family(name) | dst << 16 | b << 8 | a;
        let short = |name: &str, dst: u32, src0: u32, vsrc1: u32| {
            short_form(&encodings, name, dst, src0, vsrc1)
        };
        let v = |register: u32| 256 + register;
        let mut words = Vec::new();
        words.extend(vop3("v_lshl_add_u32", 1, [v(0), 128 + 2, 0]));
        words.push(short("v_cvt_f32_u32_e32", 2, 1, 0));
        words.push(short("v_rcp_iflag_f32_e32", 2, v(2), 0));
        words.extend([short("v_mul_f32_e32", 2, 255, 2), 0x4f7f_fffe]);
        words.push(short("v_cvt_u32_f32_e32", 2, v(2), 0));
        words.push(short("v_readfirstlane_b32", 2, v(2), 0));
        words.push(sop2("s_mul_hi_u32", 3, 2, 1));
        words.push(sop2("s_mul_i32", 4, 2, 1));
        words.push(sop2("s_sub_i32", 5, 0, 1));
        words.extend(vop3("v_mul_hi_u32", 3, [v(1), 2, 0]));
        words.push(short("v_sub_nc_u32_e32", 4, v(1), 3));
        words.push(short("v_subrev_nc_u32_e32", 5, 1, 4));
        words.push(short("v_cmp_le_u32_e32", 0, 1, 5));
        words.push(short("v_cndmask_b32_e32", 6, v(4), 5));
        // v_cmp_le_u32_sdwa s[8:9], s1, v6.
        words.extend([
            short("v_cmp_le_u32_e32", 0, 0xf9, 6),
            1 | 8 << 8 | 1 << 15 | 6 << 16 | 1 << 23 | 6 << 24,
        ]);
        words.extend(vop3("v_cndmask_b32_e64", 7, [v(6), v(1), 8]));
        // v_add_co_ci_u32_e64 v8, s[16:17], 0, v7, s[8:9], its carry-out in the word's 14:8.
        let [first, second] = vop3("v_add_co_ci_u32_e64", 8, [128, v(7), 8]);
        words.extend([first | 16 << 8, second]);
        words.push(sop2("s_and_b64", 10, 8, 126));
        words.push(sop2("s_andn2_b64", 12, 126, 8));
        words.push(short("v_add_nc_u32_e32", 1, 5, 7));
        // v_add_co_ci_u32_sdwa v2, vcc, 0, s4, vcc.
        words.extend([short("v_add_co_ci_u32_e32", 2, 0xf9, 4), 0x8686_0680]);
        // s_cbranch_execz to the next instruction.
        words.push(0xbf88_0000);
        // global_load_dword v9, v[1:2], off; s_endpgm.
        words.extend([0xdc30_8000, 0x097d_0001, 0xbf81_0000]);
        let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        let decode = decode_program(&bytes, &encodings, &operands);
        assert!(decode.is_trustworthy(), "the fixture decodes cleanly");
        assert!(
            decode
                .instructions
                .iter()
                .all(|instruction| super::named(&encodings, instruction).is_some()),
            "every instruction is named"
        );
        let load = decode.instructions[decode.instructions.len() - 2].offset;
        let program = super::LaneProgram::new(decode, &encodings, &[load]);
        assert!(
            program.runs_quickly(),
            "every step is one the native run carries out"
        );
        let wave = WaveEntry {
            group_ids: Vec::new(),
            threads: (0..4).map(|thread| Some([thread, 0, 0])).collect(),
            components: 1,
        };
        let model =
            wave_lane_addresses(&program, &encodings, &[100, 7], (64, &wave), &mut |_| None);
        let native = quick_wave_lane_addresses(&program, &[100, 7], (64, &wave), &mut |_| None);
        assert!(model.is_ok(), "{model:?}");
        assert_eq!(native, model.ok());
    }

    /// A program the native run cannot carry out names the first instruction it declines, which a
    /// refusal then quotes: here a `v_xor_b32` the address goes through.
    #[test]
    fn the_native_run_names_what_it_declines() {
        let encodings = EncodingTable::builtin().expect("encodings");
        let operands = OperandTable::builtin().expect("operands");
        let words = [
            short_form(&encodings, "v_xor_b32_e32", 1, 128, 0),
            short_form(&encodings, "v_mov_b32_e32", 2, 128, 0),
            0xdc30_8000,
            0x037d_0001,
            0xbf81_0000,
        ];
        let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        let decode = decode_program(&bytes, &encodings, &operands);
        let load = decode.instructions[2].offset;
        let program = super::LaneProgram::new(decode, &encodings, &[load]);
        assert!(!program.runs_quickly());
        assert_eq!(
            program.native_declined().as_deref(),
            Some("`v_xor_b32_e32` at 0x0")
        );
        // One it compiles but meets a value it does not know in names the step: an address from a
        // register a load left unknown.
        let words = [
            0xdc30_8000,
            0x017d_0001,
            short_form(&encodings, "v_mov_b32_e32", 2, 128, 0),
            0xdc30_8000,
            0x037d_0001,
            0xbf81_0000,
        ];
        let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        let decode = decode_program(&bytes, &encodings, &operands);
        let loads = [decode.instructions[0].offset, decode.instructions[2].offset];
        let program = super::LaneProgram::new(decode, &encodings, &loads);
        let wave = super::WaveEntry {
            group_ids: Vec::new(),
            threads: vec![Some([0, 0, 0])],
            components: 1,
        };
        assert_eq!(
            super::long_wave_lane_addresses(&program, &[], (64, &wave), &mut |_| None)
                .map_err(|at| program.instruction_named(at)),
            Err("`global_load_dword` at 0xc".to_owned())
        );
    }

    /// A load leaves unknown the registers it writes and no more: a byte load into `v0` keeps `v3`,
    /// which the next address is formed from - as Grand Theft Auto V's copy loop forms its column.
    /// Both runs agree.
    #[test]
    fn a_load_forgets_only_what_it_writes() {
        use super::{WaveEntry, long_wave_lane_addresses, wave_lane_addresses};
        let encodings = EncodingTable::builtin().expect("encodings");
        let operands = OperandTable::builtin().expect("operands");
        let words = [
            // global_load_ubyte v0, v[1:2], off; v_mov_b32 v1, v3; global_load_dword v9, v[1:2], off.
            0xdc20_0000,
            0x007d_0001,
            short_form(&encodings, "v_mov_b32_e32", 1, 256 + 3, 0),
            0xdc30_8000,
            0x097d_0001,
            0xbf81_0000,
        ];
        let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        let decode = decode_program(&bytes, &encodings, &operands);
        let loads = [decode.instructions[0].offset, decode.instructions[2].offset];
        let program = super::LaneProgram::new(decode, &encodings, &loads);
        let wave = WaveEntry {
            group_ids: Vec::new(),
            threads: vec![Some([0, 0, 0])],
            components: 1,
        };
        let model = wave_lane_addresses(&program, &encodings, &[], (64, &wave), &mut |_| None);
        assert_eq!(model, Ok(vec![(loads[0], vec![0]), (loads[1], vec![0])]));
        assert_eq!(
            long_wave_lane_addresses(&program, &[], (64, &wave), &mut |_| None).ok(),
            model.ok()
        );
    }

    /// A compare writes zero for a lane that does not run, so what that lane holds is not needed
    /// (AMD's published RDNA instruction set, VOPC): an idle lane's unknown register leaves `vcc`
    /// known, and the branch on it is followed. Both runs agree.
    #[test]
    fn a_compare_needs_no_idle_lane() {
        use super::{WaveEntry, long_wave_lane_addresses, wave_lane_addresses};
        let encodings = EncodingTable::builtin().expect("encodings");
        let operands = OperandTable::builtin().expect("operands");
        let words = [
            // global_load_dword v3, v[1:2], off: v3 unknown in every lane.
            0xdc30_8000,
            0x037d_0001,
            // v_mov_b32 v3, 5 in the running lane; v_cmp_le_u32 vcc, s0, v3.
            short_form(&encodings, "v_mov_b32_e32", 3, 128 + 5, 0),
            short_form(&encodings, "v_cmp_le_u32_e32", 0, 0, 3),
            // s_cbranch_vccz past the second load; global_load_dword v9, v[1:2], off.
            0xbf86_0002,
            0xdc30_8000,
            0x097d_0001,
            0xbf81_0000,
        ];
        let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        let decode = decode_program(&bytes, &encodings, &operands);
        let loads = [decode.instructions[0].offset, decode.instructions[4].offset];
        let program = super::LaneProgram::new(decode, &encodings, &loads);
        // Lane 1 has no thread, and does not run.
        let wave = WaveEntry {
            group_ids: Vec::new(),
            threads: vec![Some([0, 0, 0]), None],
            components: 1,
        };
        let model = wave_lane_addresses(&program, &encodings, &[4], (64, &wave), &mut |_| None);
        assert_eq!(model, Ok(vec![(loads[0], vec![0]), (loads[1], vec![0])]));
        assert_eq!(
            long_wave_lane_addresses(&program, &[4], (64, &wave), &mut |_| None).ok(),
            model.ok()
        );
    }

    /// A lane read is run where its lane is known: `v_readfirstlane_b32 s4, v0` takes the first
    /// running lane's thread id, which every lane then forms its address from.
    #[test]
    fn a_lane_read_takes_the_lane_it_names() {
        use super::{WaveEntry, wave_lane_addresses};
        let encodings = EncodingTable::builtin().expect("encodings");
        let operands = OperandTable::builtin().expect("operands");
        let words = [
            short_form(&encodings, "v_readfirstlane_b32", 4, 256, 0),
            short_form(&encodings, "v_mov_b32_e32", 1, 4, 0),
            // global_load_dword v3, v[1:2], off.
            0xdc30_8000,
            0x037d_0001,
            0xbf81_0000,
        ];
        let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        let decode = decode_program(&bytes, &encodings, &operands);
        let load = decode.instructions[2].offset;
        let program = super::LaneProgram::new(decode, &encodings, &[load]);
        // Lane 0 has no thread, so lane 1's id, 65, is the first.
        let wave = WaveEntry {
            group_ids: Vec::new(),
            threads: (64..70)
                .map(|thread| (thread != 64).then_some([thread, 0, 0]))
                .collect(),
            components: 1,
        };
        let found = wave_lane_addresses(&program, &encodings, &[], (64, &wave), &mut |_| None);
        assert_eq!(found, Ok(vec![(load, vec![65; 5])]));
    }

    /// A branch on the execution mask is followed where the mask is known, as it is at a compute
    /// wave's entry (D883): `s_cbranch_execz` passes over the load in a wave with no thread, and
    /// falls through to it in one with threads.
    #[test]
    fn a_branch_on_a_known_mask_is_followed() {
        use super::{WaveEntry, wave_lane_addresses};
        let encodings = EncodingTable::builtin().expect("encodings");
        let operands = OperandTable::builtin().expect("operands");
        let words = [
            // s_cbranch_execz to the s_endpgm, past the load's two words.
            0xbf88_0002_u32,
            // global_load_dword v3, v[1:2], off.
            0xdc30_8000,
            0x037d_0001,
            0xbf81_0000,
        ];
        let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        let decode = decode_program(&bytes, &encodings, &operands);
        let load = decode.instructions[1].offset;
        let program = super::LaneProgram::new(decode, &encodings, &[load]);
        let run = |threads: Vec<Option<[u32; 3]>>| {
            let wave = WaveEntry {
                group_ids: Vec::new(),
                threads,
                components: 1,
            };
            wave_lane_addresses(&program, &encodings, &[], (64, &wave), &mut |_| None)
        };
        assert_eq!(
            run(vec![None; 64]),
            Ok(Vec::new()),
            "no thread: the load is passed over"
        );
        // A branch on the condition mask, which nothing set, is refused naming the branch.
        let words = [0xbf86_0002_u32, 0xdc30_8000, 0x037d_0001, 0xbf81_0000];
        let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        let decode = decode_program(&bytes, &encodings, &operands);
        let vccz = super::LaneProgram::new(decode, &encodings, &[load]);
        let wave = WaveEntry {
            group_ids: Vec::new(),
            threads: vec![Some([0, 0, 0])],
            components: 1,
        };
        let refused = wave_lane_addresses(&vccz, &encodings, &[], (64, &wave), &mut |_| None)
            .expect_err("vcc is unknown");
        assert!(refused.contains("`s_cbranch_vccz` at 0x0"), "{refused}");
        // A mask a step left unknown names that step: `s_and_saveexec_b64 s[0:1], vcc` with `vcc`
        // never set leaves the execution mask unknown, and the branch on it says so.
        let words = [
            0xbe80_246a_u32,
            0xbf88_0002,
            0xdc30_8000,
            0x037d_0001,
            0xbf81_0000,
        ];
        let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        let decode = decode_program(&bytes, &encodings, &operands);
        assert_eq!(
            decode
                .instructions
                .first()
                .and_then(|i| super::named(&encodings, i)),
            Some(("SOP1", "s_and_saveexec_b64")),
            "the fixture's first instruction"
        );
        let past = decode.instructions[2].offset;
        let saveexec = super::LaneProgram::new(decode, &encodings, &[past]);
        let refused = wave_lane_addresses(&saveexec, &encodings, &[], (64, &wave), &mut |_| None)
            .expect_err("the mask is unknown");
        assert!(
            refused.contains(concat!(
                "the execution mask is unknown after `s_and_saveexec_b64` at 0x0, which read vcc_lo ",
                "unknown before any step set it"
            )),
            "{refused}"
        );
        assert_eq!(
            run(vec![Some([0, 0, 0]), Some([1, 0, 0])]),
            Ok(vec![(load, vec![0, 0])]),
            "two threads, whose address registers are zero"
        );
    }

    /// Each lane of a GL attribute fetch forms its vertex's address from the draw's table and base
    /// vertex, through the branch the table's component count takes; the lanes the program's mask
    /// leaves out, and those past the draw's vertices, form none.
    #[test]
    fn a_global_loads_lanes_are_evaluated_from_the_draws_memory() {
        let encodings = EncodingTable::builtin().expect("encodings");
        let operands = OperandTable::builtin().expect("operands");
        let words: Vec<u32> = GL_FETCH.iter().chain(&GL_FETCH_TAIL).copied().collect();
        let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        let decode = decode_program(&bytes, &encodings, &operands);
        assert!(decode.is_trustworthy(), "the fixture decodes cleanly");
        let loads: Vec<u32> = decode
            .instructions
            .iter()
            .filter(|i| i.word >> 26 == 0x37)
            .map(|i| i.offset)
            .collect();
        assert_eq!(loads.len(), 2, "the three- and four-component loads");

        let (table, vertices) = (0x2000_0000u64, 0x74_3000_0000u64);
        let memory = BTreeMap::from([
            (table, vertices as u32),
            (table + 4, (vertices >> 32) as u32),
            (table + 8, 28),
            (table + 12, 4),
        ]);
        // s[8:9] the uniform block, s[10:11] the attribute table, s12 the base vertex.
        let user_data = [0x1000_0000, 0, table as u32, (table >> 32) as u32, 3];
        let program = super::LaneProgram::new(decode, &encodings, &loads);
        let run = |lanes| {
            lane_addresses(
                &program,
                &encodings,
                (8, &user_data),
                (32, lanes),
                &mut |address| memory.get(&address).copied(),
            )
        };
        let expected: Vec<u64> = (3..6).map(|index| vertices + 28 * index).collect();
        // Four components, so the branch skips the three-component load.
        assert_eq!(run(64), Ok(vec![(loads[1], expected.clone())]));
        assert_eq!(run(2), Ok(vec![(loads[1], expected[..2].to_vec())]));

        // The native run finds what the model finds (D850), for each base vertex, lane count and
        // component count, the three-component load's branch included.
        assert!(
            program.runs_quickly(),
            "every instruction it reaches is one it runs"
        );
        for components in [3, 4] {
            let mut memory = memory.clone();
            memory.insert(table + 12, components);
            for base in [0, 3, 0x7fff_fffe] {
                let user_data = [0x1000_0000, 0, table as u32, (table >> 32) as u32, base];
                for lanes in [1, 2, 3, 7, 32] {
                    let read = |address: u64| memory.get(&address).copied();
                    let model = lane_addresses(
                        &program,
                        &encodings,
                        (8, &user_data),
                        (32, lanes),
                        &mut |a| read(a),
                    );
                    let quick = super::quick_lane_addresses(
                        &program,
                        (8, &user_data),
                        (32, lanes),
                        &mut |a| read(a),
                    );
                    assert_eq!(
                        quick.map(Ok),
                        Some(model),
                        "{components} components, base {base:#x}, {lanes} lanes"
                    );
                }
            }
        }
        // A program kept only through its last load, as a draw's prefix is: the branch over the
        // four-component load goes past its words. Taken, the model refuses it and the native run
        // declines; not taken, both find the load.
        let cut: Vec<u32> = GL_FETCH
            .iter()
            .chain(&GL_FETCH_TAIL[..3])
            .copied()
            .collect();
        let cut_bytes: Vec<u8> = cut.iter().flat_map(|w| w.to_le_bytes()).collect();
        let cut_program = super::LaneProgram::new(
            decode_program(&cut_bytes, &encodings, &operands),
            &encodings,
            &loads,
        );
        assert!(cut_program.runs_quickly());
        for components in [3, 4] {
            let mut memory = memory.clone();
            memory.insert(table + 12, components);
            let read = |address: u64| memory.get(&address).copied();
            let model = lane_addresses(
                &cut_program,
                &encodings,
                (8, &user_data),
                (32, 3),
                &mut |a| read(a),
            );
            let quick =
                super::quick_lane_addresses(&cut_program, (8, &user_data), (32, 3), &mut |a| {
                    read(a)
                });
            match model {
                Ok(found) => assert_eq!(quick, Some(found), "{components} components"),
                Err(_) => assert_eq!(quick, None, "{components} components"),
            }
        }
        // A descriptor it cannot read leaves the native run to the model.
        let quick = super::quick_lane_addresses(&program, (8, &user_data), (32, 3), &mut |_| None);
        assert_eq!(quick, None);
    }

    /// A prefix follows a branch whose condition the host knows (D753, as D758 does for lanes):
    /// `s_cmp_eq_u32 s8, 0; s_cbranch_scc1 1` skips `s_mov_b32 s4, 5` when the draw's first
    /// user-data word is zero and runs it when not, and `s_mov_b32 s5, 7` after both. A branch on
    /// what the host cannot know, `s_cbranch_vccz` at entry, is still refused.
    #[test]
    fn a_prefix_follows_a_branch_it_can_decide() {
        let encodings = EncodingTable::builtin().expect("encodings");
        let operands = OperandTable::builtin().expect("operands");
        let program = |branch: u32| -> Vec<u8> {
            [
                0xbf06_8008_u32,
                branch,
                0xbe84_0385,
                0xbe85_0387,
                0xbf81_0000,
            ]
            .iter()
            .flat_map(|w| w.to_le_bytes())
            .collect()
        };
        let decode = decode_program(&program(0xbf85_0001), &encodings, &operands);
        let end = decode.instructions.last().expect("s_endpgm").offset;
        for (first, s4) in [(0, None), (1, Some(5))] {
            let scalars = reached(&decode, &encodings, end, (8, &[first]), &mut |_| None)
                .expect("a branch the host can decide");
            assert_eq!(scalars[4], s4, "first user-data word {first}");
            assert_eq!(scalars[5], Some(7));
        }
        let unknown = decode_program(&program(0xbf86_0001), &encodings, &operands);
        assert!(reached(&unknown, &encodings, end, (8, &[0]), &mut |_| None).is_err());
        // `s_mov_b32 vcc_lo, 1; s_mov_b32 vcc_hi, 0; s_cbranch_vccnz 1` over the same moves: the
        // mask the program set is known, so the branch is taken and s4 never written.
        let bytes: Vec<u8> = [
            0xbeea_0381_u32,
            0xbeeb_0380,
            0xbf87_0001,
            0xbe84_0385,
            0xbe85_0387,
            0xbf81_0000,
        ]
        .iter()
        .flat_map(|w| w.to_le_bytes())
        .collect();
        let known = decode_program(&bytes, &encodings, &operands);
        let end = known.instructions.last().expect("s_endpgm").offset;
        let scalars = reached(&known, &encodings, end, (8, &[]), &mut |_| None)
            .expect("a branch on a known mask");
        assert_eq!((scalars[4], scalars[5]), (None, Some(7)));
    }

    /// A compare of two uniform sources in its SDWA form writes a uniform mask (D753): zero where
    /// it is false, the execution mask where it is true. `s_mov_b32 exec_lo, 3; s_mov_b32 exec_hi,
    /// 0` then `v_cmp_neq_f32_sdwa s[4:5], 0, s8`: the draw's first user-data word 0 leaves s4 zero,
    /// and 1.0 leaves it the mask, 3.
    #[test]
    fn a_uniform_compare_writes_a_uniform_mask() {
        let encodings = EncodingTable::builtin().expect("encodings");
        let operands = OperandTable::builtin().expect("operands");
        // src0 inline zero, scalar; dst s[4:5]; both selects DWORD; src1 scalar s8.
        let sdwa = 128 | 4 << 8 | 1 << 15 | 6 << 16 | 1 << 23 | 6 << 24 | 1 << 31;
        let bytes: Vec<u8> = [0xbefe_0383_u32, 0xbeff_0380, 0x7c1a_10f9, sdwa, 0xbf81_0000]
            .iter()
            .flat_map(|w| w.to_le_bytes())
            .collect();
        let decode = decode_program(&bytes, &encodings, &operands);
        let end = decode.instructions.last().expect("s_endpgm").offset;
        for (first, s4) in [(0_u32, Some(0)), (1.0f32.to_bits(), Some(3))] {
            let scalars = reached(&decode, &encodings, end, (8, &[first]), &mut |_| None)
                .expect("a straight-line prefix");
            assert_eq!(scalars[4], s4, "first user-data word {first:#x}");
        }
    }

    /// A scalar buffer load reads through the descriptor its base names, within the record count:
    /// a 16-byte buffer at 0x1000 gives `s_buffer_load_dword s4, s[8:11], 0x4` its second word, and
    /// `s5` at 0x10, past the records, reads zero.
    #[test]
    fn a_scalar_buffer_load_reads_within_its_records() {
        let encodings = EncodingTable::builtin().expect("encodings");
        let operands = OperandTable::builtin().expect("operands");
        let bytes: Vec<u8> = [
            0xf420_0104_u32,
            0xfa00_0004,
            0xf420_0144,
            0xfa00_0010,
            0xbf8c_c07f,
            0xbf81_0000,
        ]
        .iter()
        .flat_map(|w| w.to_le_bytes())
        .collect();
        let decode = decode_program(&bytes, &encodings, &operands);
        let end = decode.instructions.last().expect("s_endpgm").offset;
        let memory = BTreeMap::from([(0x1004_u64, 0xabcd), (0x1010, 0x1234)]);
        let scalars = reached(
            &decode,
            &encodings,
            end,
            (8, &[0x1000, 0, 16, 0x0002_0fac]),
            &mut |address| memory.get(&address).copied(),
        )
        .expect("a straight-line prefix");
        assert_eq!(scalars[4], Some(0xabcd), "within the records");
        assert_eq!(scalars[5], Some(0), "past them");
    }

    /// A scalar load into the condition mask's halves reads as one into any scalar register:
    /// PPSA04263's fragment programs load `vcc_lo` with `s_buffer_load_dword` and offset a buffer
    /// access by it, and its draw was refused while the load left `vcc` unknown.
    #[test]
    fn a_scalar_load_into_vcc_is_known() {
        let encodings = EncodingTable::builtin().expect("encodings");
        let operands = OperandTable::builtin().expect("operands");
        // s_buffer_load_dword vcc_lo, s[8:11], 0x4; s_buffer_load_dword vcc_hi, s[8:11], 0x10;
        // s_waitcnt; s_endpgm.
        let bytes: Vec<u8> = [
            0xf420_1a84_u32,
            0xfa00_0004,
            0xf420_1ac4,
            0xfa00_0010,
            0xbf8c_c07f,
            0xbf81_0000,
        ]
        .iter()
        .flat_map(|w| w.to_le_bytes())
        .collect();
        let decode = decode_program(&bytes, &encodings, &operands);
        let end = decode.instructions.last().expect("s_endpgm").offset;
        let memory = BTreeMap::from([(0x1004_u64, 0xabcd), (0x1010, 0x1234)]);
        let scalars = reached(
            &decode,
            &encodings,
            end,
            (8, &[0x1000, 0, 16, 0x0002_0fac]),
            &mut |address| memory.get(&address).copied(),
        )
        .expect("a straight-line prefix");
        assert_eq!(scalars[VCC], Some(0xabcd), "vcc_lo, within the records");
        assert_eq!(scalars[VCC + 1], Some(0), "vcc_hi, past them");
    }

    /// A value the host cannot know stays unknown: the execution mask at entry, and so anything
    /// read from memory at an address made from it.
    #[test]
    fn what_depends_on_the_wave_is_unknown() {
        let encodings = EncodingTable::builtin().expect("encodings");
        let operands = OperandTable::builtin().expect("operands");
        // s_mov_b32 s4, exec_lo; s_endpgm
        let bytes: Vec<u8> = [0xbe84_037e_u32, 0xbf81_0000]
            .iter()
            .flat_map(|w| w.to_le_bytes())
            .collect();
        let decode = decode_program(&bytes, &encodings, &operands);
        let scalars = reached(&decode, &encodings, 4, (8, &[]), &mut |_| Some(0))
            .expect("a straight-line prefix");
        assert_eq!(scalars[4], None);
    }
}
