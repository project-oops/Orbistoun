//! A lane run's addresses, found natively (D850).
//!
//! [`crate::evaluate::lane_addresses`] runs a program through the translator's model: every
//! instruction is emitted as SPIR-V and the emission evaluated, which keeps one source of truth for
//! what each instruction means and costs some twenty microseconds a draw. A title drawing a
//! triangle at a time with its first vertex in user data runs it every draw, since no kept answer
//! ever matches (Ship of Harkinian: a quarter of its frame). The programs those draws run are a
//! handful of integer instructions - a lane id, a descriptor load, a multiply and a 64-bit add -
//! so this runs that handful directly, over plain words, and declines anything else.
//!
//! A program is compiled once. Compiling declines a program holding any instruction it does not
//! run exactly, and a run declines wherever a value it needs is unknown; the caller then runs the
//! model, which says why. Whoever calls this checks its answers against the model's for each
//! program's first runs (D850).

use orbistoun_shader::{Decode, EncodingTable, Instruction, Operand};

/// The scalar register codes a run keeps: as the model's.
const SCALAR_CODES: usize = 128;
/// The condition mask's low half's code.
const VCC: usize = 106;
/// `m0`'s code.
const M0: usize = 124;
/// The execution mask's low half's code.
const EXEC: usize = 126;
/// The vector registers a primitive shader's module seeds with the draw's geometry ids in its
/// vertex lanes, unknown to a run: as the model's.
const SEEDED_IDS: [u32; 4] = [0, 1, 2, 5];
/// A branch target outside the program's words.
const OUTSIDE: usize = usize::MAX;
/// The scalar offset field's "no offset" code.
const SOFFSET_NULL: u32 = 0x7d;
/// The vector registers a run keeps.
const VECTOR_REGISTERS: usize = 256;
/// The scalar registers user data can land in: as the model's.
const SCALAR_REGISTERS: usize = crate::model::SCALAR_REGISTERS as usize;

/// Where an operand's value comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Source {
    /// A scalar register, by code.
    Scalar(usize),
    /// A vector register, per lane.
    Vector(u32),
    /// A constant.
    Constant(u32),
}

/// One instruction, as a run carries it out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    /// Writes nothing a run reads: a program-control no-op, an export, or a vector instruction no
    /// address depends on.
    Nothing,
    /// The end of the program.
    End,
    /// `s_mov_b32`.
    ScalarMove { destination: usize, source: Source },
    /// `s_cmp_eq_u32` (`equal`) or `s_cmp_lg_u32`.
    Compare { a: Source, b: Source, equal: bool },
    /// `s_load_dword*`: `words` words from the base pair plus the offset.
    ScalarLoad {
        destination: usize,
        base: usize,
        offset: i64,
        words: usize,
    },
    /// `s_branch`, or `s_cbranch_scc0`/`scc1` (`on` the condition it jumps on).
    Branch { target: usize, on: Option<bool> },
    /// `v_mov_b32`.
    Move { destination: u32, source: Source },
    /// `v_lshlrev_b32`: `value` shifted left by `amount`'s low five bits.
    ShiftLeft {
        destination: u32,
        amount: Source,
        value: Source,
    },
    /// `v_add_nc_u32`.
    Add {
        destination: u32,
        a: Source,
        b: Source,
    },
    /// `v_mul_lo_u32`.
    MultiplyLow {
        destination: u32,
        a: Source,
        b: Source,
    },
    /// `v_add_co_u32`, its carry into the mask at `carry`.
    AddCarry {
        destination: u32,
        carry: usize,
        a: Source,
        b: Source,
    },
    /// `v_add_co_ci_u32`, its carry in from the mask at `carry_in` and out to `carry`.
    AddWithCarry {
        destination: u32,
        carry: usize,
        a: Source,
        b: Source,
        carry_in: usize,
    },
    /// `v_mbcnt_lo_u32_b32`: the bits of `mask` below the lane, plus `add`.
    CountBelow {
        destination: u32,
        mask: Source,
        add: Source,
    },
    /// A global load whose addresses are found: the vector pair and the immediate offset. Its
    /// destination, `forget` registers from it, is unknown afterwards.
    Load {
        at: u32,
        pair: u32,
        offset: i64,
        destination: Option<u32>,
    },
}

/// A program compiled for runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Program {
    steps: Vec<Step>,
    /// Each step's byte offset, for the run's bound.
    offsets: Vec<u32>,
    /// The last load: a run ends past it.
    until: u32,
}

/// The scalar code a named register stands for, as a source or destination.
fn named_code(name: &str) -> Option<usize> {
    Some(match name {
        "vcc_lo" | "vcc" => VCC,
        "vcc_hi" => VCC + 1,
        "exec_lo" | "exec" => EXEC,
        "exec_hi" => EXEC + 1,
        "m0" => M0,
        _ => return None,
    })
}

/// An operand as a source; `None` for one a run does not read exactly.
fn source(operand: &Operand) -> Option<Source> {
    Some(match operand {
        Operand::Scalar(code) => Source::Scalar(usize::from(*code)),
        Operand::Vector(register) => Source::Vector(u32::from(*register)),
        Operand::Integer(value) => Source::Constant(*value as u32),
        Operand::Literal(value) => Source::Constant(*value),
        Operand::Named(name) => Source::Scalar(named_code(name)?),
        Operand::Immediate(_) | Operand::Unrecognised(_) => return None,
    })
}

/// A scalar destination's code.
fn scalar_destination(operand: &Operand) -> Option<usize> {
    match operand {
        Operand::Scalar(code) => Some(usize::from(*code)),
        Operand::Named(name) => named_code(name),
        _ => None,
    }
}

/// A vector destination's register.
fn vector_destination(operand: Option<&Operand>) -> Option<u32> {
    match operand {
        Some(Operand::Vector(register)) => Some(u32::from(*register)),
        _ => None,
    }
}

/// Whether a long-form vector instruction carries a modifier a run does not apply: absolute value,
/// operand select, clamp, output modifier or negation. A carry form (VOP3B) holds its carry-out
/// register where the others hold absolute value and operand select, so only its clamp counts there.
fn modified(instruction: &Instruction, carry: bool) -> bool {
    let first = instruction.word;
    let second = instruction.second_word.unwrap_or(0);
    let fields = if carry {
        first >> 15 & 1
    } else {
        first >> 8 & 0xff
    };
    fields != 0 || second >> 27 != 0
}

/// Compiles `decode` for runs that find the addresses of its global loads at `loads`, running only
/// the instructions `kept` keeps of the vector ones. `None` where any instruction a run reaches is
/// one it does not carry out exactly.
#[must_use]
pub fn compile(
    decode: &Decode,
    encodings: &EncodingTable,
    loads: &[u32],
    kept: &[bool],
) -> Option<Program> {
    loads.iter().copied().max()?;
    let until = crate::evaluate::run_end(decode, encodings, loads);
    let mut steps = Vec::new();
    let mut offsets = Vec::new();
    for (index, instruction) in decode.instructions.iter().enumerate() {
        if instruction.offset > until {
            break;
        }
        let family = encodings
            .encodings()
            .get(usize::from(instruction.encoding?))?;
        let name = encodings.mnemonic_for(&family.name, instruction.opcode)?;
        let family = family.name.as_str();
        let vector = matches!(family, "VOP1" | "VOP2" | "VOP3" | "VOP3P" | "VOPC");
        let step = if loads.contains(&instruction.offset) {
            load_step(instruction)
        } else if vector && !kept.get(index).copied().unwrap_or(true) {
            Some(Step::Nothing)
        } else if vector {
            vector_step(family, name, instruction)
        } else {
            scalar_step(family, name, instruction, decode)
        };
        let step = step?;
        steps.push(step);
        offsets.push(instruction.offset);
    }
    Some(Program {
        steps,
        offsets,
        until,
    })
}

/// A global load whose addresses are found: one without a scalar base, which would add to every
/// lane's address.
fn load_step(instruction: &Instruction) -> Option<Step> {
    let operands = instruction.operands.as_slice();
    let Some(Operand::Vector(pair)) = operands.get(1) else {
        return None;
    };
    if !matches!(operands.get(2), Some(Operand::Named(null)) if null == "null") {
        return None;
    }
    let offset = match operands.get(3) {
        Some(Operand::Immediate(offset)) => *offset,
        _ => 0,
    };
    Some(Step::Load {
        at: instruction.offset,
        pair: u32::from(*pair),
        offset,
        destination: vector_destination(operands.first()),
    })
}

/// A scalar, program-control, memory or export instruction as a step; `None` for one a run does
/// not carry out exactly.
fn scalar_step(
    family: &str,
    name: &str,
    instruction: &Instruction,
    decode: &Decode,
) -> Option<Step> {
    let operands = instruction.operands.as_slice();
    Some(match (family, name) {
        ("SOPP", "s_endpgm") => Step::End,
        ("SOPP", "s_branch" | "s_cbranch_scc0" | "s_cbranch_scc1") => {
            let Some(Operand::Immediate(jump)) = operands.first() else {
                return None;
            };
            // A branch past the words kept - over the last load - has no step to go to; a run
            // taking it declines, where the model refuses it.
            let to = i64::from(instruction.offset) + 4 + 4 * crate::evaluate::simm16(*jump);
            let target = decode
                .instructions
                .iter()
                .position(|i| i64::from(i.offset) == to)
                .unwrap_or(OUTSIDE);
            let on = match name {
                "s_cbranch_scc0" => Some(false),
                "s_cbranch_scc1" => Some(true),
                _ => None,
            };
            Step::Branch { target, on }
        }
        ("SOPP", "s_inst_prefetch" | "s_nop" | "s_sendmsg" | "s_waitcnt") | ("EXP", _) => {
            Step::Nothing
        }
        ("SOP1", "s_mov_b32") => Step::ScalarMove {
            destination: scalar_destination(operands.first()?)?,
            source: source(operands.get(1)?)?,
        },
        ("SOPC", "s_cmp_eq_u32" | "s_cmp_lg_u32") => Step::Compare {
            a: source(operands.first()?)?,
            b: source(operands.get(1)?)?,
            equal: name == "s_cmp_eq_u32",
        },
        ("SMEM", _) if name.starts_with("s_load_dword") => {
            let words = match name.rsplit_once("dwordx") {
                Some((_, count)) => count.parse().ok()?,
                None => 1,
            };
            let [
                Operand::Scalar(destination),
                Operand::Scalar(base),
                Operand::Immediate(offset),
                ..,
            ] = operands
            else {
                return None;
            };
            let soffset = instruction.second_word.map(|second| (second >> 25) & 0x7f);
            if !matches!(soffset, Some(SOFFSET_NULL) | None) {
                return None;
            }
            Step::ScalarLoad {
                destination: usize::from(*destination),
                base: usize::from(*base),
                offset: *offset,
                words,
            }
        }
        _ => return None,
    })
}

/// A vector instruction as a step; `None` for one a run does not carry out exactly, a long form
/// carrying a modifier among them.
fn vector_step(family: &str, name: &str, instruction: &Instruction) -> Option<Step> {
    let operands = instruction.operands.as_slice();
    if family == "VOP3" && modified(instruction, name.contains("_co_")) {
        return None;
    }
    let destination = vector_destination(operands.first())?;
    let at = |index: usize| operands.get(index).and_then(source);
    let mask = |index: usize| operands.get(index).and_then(scalar_destination);
    Some(match (family, name) {
        ("VOP1", "v_mov_b32_e32") => Step::Move {
            destination,
            source: at(1)?,
        },
        ("VOP2", "v_lshlrev_b32_e32") | ("VOP3", "v_lshlrev_b32_e64") => Step::ShiftLeft {
            destination,
            amount: at(1)?,
            value: at(2)?,
        },
        ("VOP2", "v_add_nc_u32_e32") | ("VOP3", "v_add_nc_u32") => Step::Add {
            destination,
            a: at(1)?,
            b: at(2)?,
        },
        ("VOP3", "v_mul_lo_u32") => Step::MultiplyLow {
            destination,
            a: at(1)?,
            b: at(2)?,
        },
        ("VOP3", "v_mbcnt_lo_u32_b32") => Step::CountBelow {
            destination,
            mask: at(1)?,
            add: at(2)?,
        },
        ("VOP3", "v_add_co_u32") => Step::AddCarry {
            destination,
            carry: mask(1)?,
            a: at(2)?,
            b: at(3)?,
        },
        ("VOP2", "v_add_co_ci_u32_e32") | ("VOP3", "v_add_co_ci_u32") => Step::AddWithCarry {
            destination,
            carry: mask(1)?,
            a: at(2)?,
            b: at(3)?,
            carry_in: mask(4)?,
        },
        _ => return None,
    })
}

/// A run's registers.
struct Lanes {
    scalars: [Option<u32>; SCALAR_CODES],
    /// By register, then lane, for the modelled lanes.
    vectors: Vec<Option<u32>>,
    modelled: u32,
    condition: Option<bool>,
}

impl Lanes {
    /// A run's registers at entry: user data from `first_register`, every lane of a wave `width`
    /// wide running, every vector register zero but the seeded ids in the draw's `lanes` vertex
    /// lanes, which are unknown.
    fn start((first_register, user_data): (u32, &[u32]), (width, lanes): (u32, u32)) -> Self {
        let width = width.clamp(1, 64);
        let modelled = lanes.clamp(1, width);
        let mut state = Self {
            scalars: [None; SCALAR_CODES],
            vectors: vec![Some(0); VECTOR_REGISTERS * modelled as usize],
            modelled,
            condition: None,
        };
        for register in SEEDED_IDS {
            for lane in 0..lanes.min(modelled) {
                state.set_vector(register, lane, None);
            }
        }
        for (register, word) in (first_register as usize..).zip(user_data) {
            if register < SCALAR_REGISTERS {
                state.scalars[register] = Some(*word);
            }
        }
        let mask = u64::MAX >> (64 - width);
        state.scalars[EXEC] = Some(mask as u32);
        state.scalars[EXEC + 1] = Some((mask >> 32) as u32);
        state
    }

    /// The address each running lane below `lanes` forms from the vector pair at `pair` and
    /// `offset`; `None` where a mask bit or an address is unknown.
    fn addresses(&self, pair: u32, offset: i64, lanes: u32) -> Option<Vec<u64>> {
        let mut addresses = Vec::new();
        for lane in 0..lanes {
            if !self.mask_bit(EXEC, lane)? {
                continue;
            }
            let low = self.vector(pair, lane)?;
            let high = self.vector(pair + 1, lane)?;
            let address = (u64::from(high) << 32) | u64::from(low);
            addresses.push(address.wrapping_add_signed(offset));
        }
        Some(addresses)
    }

    /// Makes `count` vector registers from `first` unknown in every lane.
    fn forget(&mut self, first: u32, count: u32) {
        for register in first..first + count {
            for lane in 0..self.modelled {
                self.set_vector(register, lane, None);
            }
        }
    }

    fn scalar(&self, code: usize) -> Option<u32> {
        self.scalars.get(code).copied().flatten()
    }

    fn vector(&self, register: u32, lane: u32) -> Option<u32> {
        self.vectors
            .get((register * self.modelled + lane) as usize)
            .copied()
            .flatten()
    }

    fn set_vector(&mut self, register: u32, lane: u32, value: Option<u32>) {
        if let Some(slot) = self
            .vectors
            .get_mut((register * self.modelled + lane) as usize)
        {
            *slot = value;
        }
    }

    fn read(&self, source: Source, lane: u32) -> Option<u32> {
        match source {
            Source::Scalar(code) => self.scalar(code),
            Source::Vector(register) => self.vector(register, lane),
            Source::Constant(value) => Some(value),
        }
    }

    /// Bit `lane` of the mask whose low half is at `code`.
    fn mask_bit(&self, code: usize, lane: u32) -> Option<bool> {
        let half = self.scalar(code + (lane / 32) as usize)?;
        Some(half & (1 << (lane % 32)) != 0)
    }

    /// Sets bit `lane` of the mask whose low half is at `code`, which must be known.
    fn set_mask_bit(&mut self, code: usize, lane: u32, bit: bool) -> Option<()> {
        let slot = self.scalars.get_mut(code + (lane / 32) as usize)?;
        let half = (*slot)?;
        let flag = 1 << (lane % 32);
        *slot = Some(if bit { half | flag } else { half & !flag });
        Some(())
    }
}

/// The addresses each global load forms in each active lane below `lanes`, for a wave `width`
/// lanes wide whose user data lands from `first_register`, reading memory through `read`: what
/// [`crate::evaluate::lane_addresses`] answers, a load a branch passes over left out. `None` where
/// a value a step needs is unknown, or a branch's condition is; the model then decides.
pub fn run(
    program: &Program,
    (first_register, user_data): (u32, &[u32]),
    (width, lanes): (u32, u32),
    read: &mut dyn FnMut(u64) -> Option<u32>,
) -> Option<crate::evaluate::LaneAddresses> {
    let state = Lanes::start((first_register, user_data), (width, lanes));
    run_from(state, program, lanes, read)
}

/// The addresses each global load forms over one compute wave (D883), as
/// [`crate::evaluate::wave_lane_addresses`] answers: the user data from `s0`, then the group ids;
/// each lane with a thread its id components from `v0` and its execution-mask bit.
pub fn run_wave(
    program: &Program,
    user_data: &[u32],
    (width, wave): (u32, &crate::evaluate::WaveEntry),
    read: &mut dyn FnMut(u64) -> Option<u32>,
) -> Option<crate::evaluate::LaneAddresses> {
    let width = width.clamp(1, 64);
    let lanes = u32::try_from(wave.threads.len()).ok()?.min(width);
    let modelled = lanes.clamp(1, width);
    let mut state = Lanes {
        scalars: [None; SCALAR_CODES],
        vectors: vec![Some(0); VECTOR_REGISTERS * modelled as usize],
        modelled,
        condition: None,
    };
    for (register, word) in user_data.iter().chain(&wave.group_ids).enumerate() {
        if register < SCALAR_REGISTERS {
            state.scalars[register] = Some(*word);
        }
    }
    let mut mask = 0u64;
    for (lane, ids) in (0..lanes).zip(&wave.threads) {
        let Some(ids) = ids else {
            continue;
        };
        mask |= 1 << lane;
        for (register, id) in (0..wave.components.min(3)).zip(ids) {
            state.set_vector(register, lane, Some(*id));
        }
    }
    state.scalars[EXEC] = Some(mask as u32);
    state.scalars[EXEC + 1] = Some((mask >> 32) as u32);
    run_from(state, program, lanes, read)
}

/// The run [`run`] and [`run_wave`] share, from the wave's entry.
fn run_from(
    mut state: Lanes,
    program: &Program,
    lanes: u32,
    read: &mut dyn FnMut(u64) -> Option<u32>,
) -> Option<crate::evaluate::LaneAddresses> {
    let modelled = state.modelled;
    let mut found = Vec::new();
    let mut at = 0usize;
    let mut steps = 0usize;
    while let Some(&step) = program.steps.get(at) {
        if program.offsets[at] > program.until {
            break;
        }
        // A run its bound cuts short is declined: the model refuses it by name.
        if steps > 4 * program.steps.len() {
            return None;
        }
        steps += 1;
        at += 1;
        match step {
            Step::Nothing => {}
            Step::End => break,
            Step::ScalarMove {
                destination,
                source,
            } => {
                let value = state.read(source, 0);
                *state.scalars.get_mut(destination)? = value;
            }
            Step::Compare { a, b, equal } => {
                let (a, b) = (state.read(a, 0)?, state.read(b, 0)?);
                state.condition = Some((a == b) == equal);
            }
            Step::ScalarLoad {
                destination,
                base,
                offset,
                words,
            } => {
                let low = state.scalar(base)?;
                let high = state.scalar(base + 1)?;
                let address = ((u64::from(high & 0xffff) << 32) | u64::from(low))
                    .wrapping_add_signed(offset)
                    & !3;
                for step in 0..words {
                    let value = read(address + 4 * step as u64);
                    *state.scalars.get_mut(destination + step)? = value;
                }
            }
            Step::Branch { target, on } => {
                let taken = match on {
                    None => true,
                    Some(on) => state.condition? == on,
                };
                if taken {
                    if target == OUTSIDE {
                        return None;
                    }
                    at = target;
                }
            }
            Step::Load {
                at: load,
                pair,
                offset,
                destination,
            } => {
                let addresses = state.addresses(pair, offset, lanes.min(modelled))?;
                found.push((load, addresses));
                // What it loads is memory's, unknown here.
                if let Some(first) = destination {
                    state.forget(first, 4);
                }
            }
            vector => run_vector(&mut state, vector)?,
        }
    }
    // The loads the run reached, as the model answers: a branch past one leaves it out.
    Some(found)
}

/// One vector step in every active modelled lane; `None` where a value it needs is unknown.
fn run_vector(state: &mut Lanes, step: Step) -> Option<()> {
    for lane in 0..state.modelled {
        if !state.mask_bit(EXEC, lane)? {
            continue;
        }
        match step {
            Step::Move {
                destination,
                source,
            } => {
                let value = state.read(source, lane);
                state.set_vector(destination, lane, value);
            }
            Step::Add { destination, a, b } => {
                let value = state.read(a, lane)?.wrapping_add(state.read(b, lane)?);
                state.set_vector(destination, lane, Some(value));
            }
            Step::ShiftLeft {
                destination,
                amount,
                value,
            } => {
                let value = state.read(value, lane)? << (state.read(amount, lane)? & 31);
                state.set_vector(destination, lane, Some(value));
            }
            Step::MultiplyLow { destination, a, b } => {
                let value = state.read(a, lane)?.wrapping_mul(state.read(b, lane)?);
                state.set_vector(destination, lane, Some(value));
            }
            Step::CountBelow {
                destination,
                mask,
                add,
            } => {
                // The low half counts the mask's bits below lanes 0-31; lanes past 31 see all 32.
                let below = if lane >= 32 {
                    u32::MAX
                } else {
                    (1u32 << lane) - 1
                };
                let value = (state.read(mask, lane)? & below)
                    .count_ones()
                    .wrapping_add(state.read(add, lane)?);
                state.set_vector(destination, lane, Some(value));
            }
            Step::AddCarry {
                destination,
                carry,
                a,
                b,
            } => {
                let sum = u64::from(state.read(a, lane)?) + u64::from(state.read(b, lane)?);
                state.set_vector(destination, lane, Some(sum as u32));
                set_carry(state, carry, lane, sum >> 32 != 0)?;
            }
            Step::AddWithCarry {
                destination,
                carry,
                a,
                b,
                carry_in,
            } => {
                let sum = u64::from(state.read(a, lane)?)
                    + u64::from(state.read(b, lane)?)
                    + u64::from(state.mask_bit(carry_in, lane)?);
                state.set_vector(destination, lane, Some(sum as u32));
                set_carry(state, carry, lane, sum >> 32 != 0)?;
            }
            _ => return None,
        }
    }
    Some(())
}

/// Writes a lane's carry into the mask at `carry`, starting it from zero if it was unknown: a
/// carry-out writes every active lane's bit, and only a lane's own bit is read back (a program
/// reading another lane's is refused before a run, [`crate::evaluate`]'s `crosses_lanes`).
fn set_carry(state: &mut Lanes, carry: usize, lane: u32, bit: bool) -> Option<()> {
    let slot = state.scalars.get_mut(carry + (lane / 32) as usize)?;
    if slot.is_none() {
        *slot = Some(0);
    }
    state.set_mask_bit(carry, lane, bit)
}
