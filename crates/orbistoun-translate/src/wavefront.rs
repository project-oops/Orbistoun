//! The wavefront model: one invocation simulates every lane.
//!
//! # What is different from the lane model
//!
//! There, one SPIR-V invocation *is* one guest lane, the execution mask is implicit,
//! and lanes cannot see each other. Here one invocation simulates all sixty-four:
//! vector registers are arrays indexed by lane, and the mask is an ordinary value the
//! shader can read, write and do arithmetic on - exactly as the guest does.
//!
//! Everything the lane model cannot express follows from that. Cross-lane instructions
//! become array reads. `s_andn2_b64 vcc, exec, s[2:3]` becomes integer arithmetic.
//! Nothing depends on the hardware's subgroup size, because no subgroup is involved.
//!
//! The cost is severe: every vector instruction becomes sixty-four operations in one
//! invocation, so the machine's parallelism is thrown away. That is accepted, because
//! this level exists to be **right**, not quick.
//!
//! # The mask lives where the hardware puts it
//!
//! Not in a variable of its own - in the scalar register file, at the two indices the
//! architecture reserves for it. The guest addresses `exec_lo` and `exec_hi` as
//! ordinary scalar registers and manipulates them as two 32-bit halves, so modelling
//! them as anything else would mean translating those accesses specially. This way
//! they need no translation at all.
//!
//! It also sidesteps needing 64-bit integers, and therefore the capability that would
//! demand, on a value the guest never treats as one anyway.
//!
//! # Predication without branching
//!
//! A masked write could be a conditional block per lane. It is a `select` instead:
//! read the old value, compute the new one, and keep whichever the mask calls for.
//!
//! Same result, no merge blocks, no structured control flow to get right - and it
//! keeps this module free of the one thing that makes SPIR-V generation hard, at a
//! level whose whole purpose is being obviously correct.
//!
//! # The observation layout matches the lane model exactly
//!
//! Same buffer, same slots: vector registers of **lane zero**, then scalar registers.
//! That is what makes the two levels comparable - a differential test can only diff
//! them if they report in the same shape. It is a requirement, not a coincidence.
//!
//! # On the duplication with `predicated`
//!
//! Instruction dispatch is repeated here rather than shared. Factoring a backend seam
//! before two implementations exist is guessing where it goes; with both present the
//! seam is observable. Four instructions is a cheap price for finding out, and the
//! factoring is on the list.

use orbistoun_shader::{Decode, EncodingTable, Instruction, Operand};
use orbistoun_spirv::{Builder, Id, addressing, capability, execution, memory, mode, op};
use std::collections::BTreeMap;

use crate::buffer;
use crate::model::{self, Model};
use crate::predicated::{MEMORY_WORDS, OBSERVED_REGISTERS, OBSERVED_WORDS, REGISTER_COUNT};
use crate::{TranslateError, Width};

/// Lanes in a wavefront.
pub const WAVE: u32 = Width::Wave64.lanes();

/// Scalar register holding the low half of the execution mask.
///
/// The architecture reserves these two indices; they are the same codes
/// `data/operands.toml` names, and using them here is what lets guest code that reads
/// or writes the mask need no special handling.
const EXEC_LO: u32 = 126;
/// Scalar register holding the high half of the execution mask.
const EXEC_HI: u32 = 127;

/// Scalar register holding the low half of the condition mask.
///
/// Where a comparison puts its answer. An ordinary register here, which is the whole
/// argument for this model: the guest treats it as one and so can the translation.
const VCC_LO: u32 = 106;
/// Scalar register holding the high half of the condition mask.
const VCC_HI: u32 = 107;

/// Private storage class.
const PRIVATE: u32 = 6;

/// Workgroup storage class: shared between the invocations of a workgroup.
///
/// One invocation is one wavefront here and a workgroup is one invocation, so this is
/// currently indistinguishable from private storage. It is declared as workgroup anyway
/// because that is what the guest's local data share *is* - shared - and using private
/// storage would be correct today and silently wrong the moment a dispatch has more than
/// one wavefront per group.
const WORKGROUP: u32 = 4;

/// Words of local data share a translated module provides.
///
/// A placeholder, like the guest-memory window: the real size is declared per dispatch by
/// the submitting guest. Sized to hold something useful for tests and small enough not to
/// cost anything.
const LOCAL_WORDS: u32 = 256;

/// The register files, once declared.
#[derive(Debug, Clone, Copy)]
struct Files {
    vectors: Id,
    scalars: Id,
    /// Pointer to one lane of one vector register.
    lane_ptr: Id,
    /// Pointer to one scalar register.
    scalar_ptr: Id,
}

/// Declares both register files.
///
/// The vector file is an array of registers, each an array of lanes. The scalar file is
/// one value per register, because scalar registers are uniform across the whole
/// wavefront rather than per lane - which is the entire distinction between the two
/// halves of this architecture.
///
/// Both are null-initialised: a private variable is otherwise undefined at entry, and a
/// test asserting an untouched register reads zero would be asserting on whatever the
/// driver left behind.
fn declare_files(b: &mut Builder, u32_type: Id, registers: Id, lanes: Id) -> Files {
    let lane_array = b.id();
    let vector_array = b.id();
    let vector_array_ptr = b.id();
    let lane_ptr = b.id();
    let vectors = b.id();
    let vector_zero = b.id();

    b.declare(op::TYPE_ARRAY, &[lane_array.0, u32_type.0, lanes.0]);
    b.declare(op::TYPE_ARRAY, &[vector_array.0, lane_array.0, registers.0]);
    b.declare(
        op::TYPE_POINTER,
        &[vector_array_ptr.0, PRIVATE, vector_array.0],
    );
    b.declare(op::TYPE_POINTER, &[lane_ptr.0, PRIVATE, u32_type.0]);
    b.declare(op::CONSTANT_NULL, &[vector_array.0, vector_zero.0]);
    b.declare(
        op::VARIABLE,
        &[vector_array_ptr.0, vectors.0, PRIVATE, vector_zero.0],
    );

    let scalar_array = b.id();
    let scalar_array_ptr = b.id();
    let scalar_ptr = b.id();
    let scalars = b.id();
    let scalar_zero = b.id();

    b.declare(op::TYPE_ARRAY, &[scalar_array.0, u32_type.0, registers.0]);
    b.declare(
        op::TYPE_POINTER,
        &[scalar_array_ptr.0, PRIVATE, scalar_array.0],
    );
    b.declare(op::TYPE_POINTER, &[scalar_ptr.0, PRIVATE, u32_type.0]);
    b.declare(op::CONSTANT_NULL, &[scalar_array.0, scalar_zero.0]);
    b.declare(
        op::VARIABLE,
        &[scalar_array_ptr.0, scalars.0, PRIVATE, scalar_zero.0],
    );

    Files {
        vectors,
        scalars,
        lane_ptr,
        scalar_ptr,
    }
}

/// The observation buffer, once declared.
/// Builds a wavefront-model module for one decoded shader.
#[derive(Debug)]
pub struct Wavefront<'a> {
    /// How many words of guest memory this module addresses.
    ///
    /// Carried rather than read from a constant so a test can widen the window and reach
    /// an address the default cannot hold - which is otherwise impossible, and is why
    /// every buffer test has to pretend memory starts at zero (D101).
    memory_words: u32,
    builder: Builder,
    encodings: &'a EncodingTable,
    /// How many lanes this shader's wavefront has, and therefore how wide its masks are.
    width: Width,
    constants: BTreeMap<u32, Id>,
    u32_type: Id,
    f32_type: Id,
    bool_type: Id,
    /// Pointer to one lane of one vector register.
    lane_ptr: Id,
    /// Pointer to one scalar register.
    scalar_ptr: Id,
    vectors: Id,
    scalars: Id,
    buffer_element_ptr: Id,
    buffer: Id,
    memory_element_ptr: Id,
    memory: Id,
    /// The local data share: storage the lanes of this wavefront exchange values in.
    local: Id,
    /// Pointer to one word of it.
    local_ptr: Id,
    /// The program counter the dispatch loop switches on.
    program_counter: Id,
    /// The scalar condition code, as a private variable holding 0 or 1.
    condition_code: Id,
    translated: usize,
}

impl<'a> Wavefront<'a> {
    /// Prepares the module and sets every lane active.
    pub fn new(encodings: &'a EncodingTable, width: Width) -> Self {
        let mut b = Builder::new().with_version(orbistoun_spirv::VERSION_1_3);

        let void = b.id();
        let fn_type = b.id();
        let u32_type = b.id();
        let f32_type = b.id();
        let bool_type = b.id();
        let main = b.id();
        let entry_block = b.id();
        let wave_count = b.id();
        let register_count = b.id();
        let observed_count = b.id();
        let memory_count = b.id();
        let counter_ptr = b.id();
        let counter = b.id();
        let counter_zero = b.id();
        let scc = b.id();
        let local_count = b.id();
        let local_array = b.id();
        let local_array_ptr = b.id();
        let local_ptr = b.id();
        let local = b.id();

        b.header(op::CAPABILITY, &[capability::SHADER]);
        b.header(op::MEMORY_MODEL, &[addressing::LOGICAL, memory::GLSL450]);

        let mut entry = vec![execution::GL_COMPUTE, main.0];
        entry.extend(Builder::literal_string("main"));
        b.header(op::ENTRY_POINT, &entry);
        b.header(op::EXECUTION_MODE, &[main.0, mode::LOCAL_SIZE, 1, 1, 1]);

        b.declare(op::TYPE_VOID, &[void.0]);
        b.declare(op::TYPE_FUNCTION, &[fn_type.0, void.0]);
        b.declare(op::TYPE_INT, &[u32_type.0, 32, 0]);
        b.declare(op::TYPE_FLOAT, &[f32_type.0, 32]);
        b.declare(op::TYPE_BOOL, &[bool_type.0]);
        // The program counter. Zero-initialised so the shader starts at its first block
        // rather than at whatever the driver left in the variable.
        b.declare(op::TYPE_POINTER, &[counter_ptr.0, PRIVATE, u32_type.0]);
        b.declare(op::CONSTANT, &[u32_type.0, counter_zero.0, 0]);
        b.declare(
            op::VARIABLE,
            &[counter_ptr.0, counter.0, PRIVATE, counter_zero.0],
        );
        // The scalar condition code shares the counter's pointer type and initialiser:
        // both are a private word starting at zero.
        b.declare(
            op::VARIABLE,
            &[counter_ptr.0, scc.0, PRIVATE, counter_zero.0],
        );

        // The local data share. No initialiser - workgroup storage cannot have one, and
        // the guest's is uninitialised too, so a shader reading a word it never wrote
        // gets undefined contents in both.
        b.declare(op::CONSTANT, &[u32_type.0, local_count.0, LOCAL_WORDS]);
        b.declare(op::TYPE_ARRAY, &[local_array.0, u32_type.0, local_count.0]);
        b.declare(
            op::TYPE_POINTER,
            &[local_array_ptr.0, WORKGROUP, local_array.0],
        );
        b.declare(op::TYPE_POINTER, &[local_ptr.0, WORKGROUP, u32_type.0]);
        b.declare(op::VARIABLE, &[local_array_ptr.0, local.0, WORKGROUP]);

        b.declare(op::CONSTANT, &[u32_type.0, wave_count.0, width.lanes()]);
        b.declare(
            op::CONSTANT,
            &[u32_type.0, register_count.0, REGISTER_COUNT],
        );
        b.declare(
            op::CONSTANT,
            &[u32_type.0, observed_count.0, OBSERVED_WORDS],
        );
        b.declare(op::CONSTANT, &[u32_type.0, memory_count.0, MEMORY_WORDS]);

        let files = declare_files(&mut b, u32_type, register_count, wave_count);
        let observation = buffer::declare(&mut b, u32_type, observed_count, buffer::OBSERVATION);
        // Guest memory at its own binding, so a guest address cannot reach the
        // observation window and rewrite registers a test is about to read.
        let guest_memory = buffer::declare(&mut b, u32_type, memory_count, buffer::GUEST_MEMORY);

        b.function(op::FUNCTION, &[void.0, main.0, 0, fn_type.0]);
        b.function(op::LABEL, &[entry_block.0]);

        let mut this = Self {
            builder: b,
            encodings,
            memory_words: MEMORY_WORDS,
            width,
            constants: BTreeMap::new(),
            u32_type,
            f32_type,
            bool_type,
            lane_ptr: files.lane_ptr,
            scalar_ptr: files.scalar_ptr,
            vectors: files.vectors,
            scalars: files.scalars,
            buffer_element_ptr: observation.element_ptr,
            buffer: observation.buffer,
            memory_element_ptr: guest_memory.element_ptr,
            memory: guest_memory.buffer,
            local,
            local_ptr,
            program_counter: counter,
            condition_code: scc,
            translated: 0,
        };

        // Every lane runs at entry. Left at the null initialiser the mask would be
        // zero, every write would be discarded, and the shader would produce a
        // plausible buffer of zeros while executing nothing at all.
        let all = this.constant(u32::MAX);
        this.store_scalar(EXEC_LO, all);
        this.store_scalar(EXEC_HI, all);
        this
    }

    fn constant(&mut self, value: u32) -> Id {
        if let Some(id) = self.constants.get(&value) {
            return *id;
        }
        let id = self.builder.id();
        self.builder
            .declare(op::CONSTANT, &[self.u32_type.0, id.0, value]);
        self.constants.insert(value, id);
        id
    }

    fn scalar_pointer(&mut self, register: u32) -> Id {
        let index = self.constant(register);
        let pointer = self.builder.id();
        self.builder.function(
            op::ACCESS_CHAIN,
            &[self.scalar_ptr.0, pointer.0, self.scalars.0, index.0],
        );
        pointer
    }

    /// The register pair a named 64-bit lane mask occupies.
    ///
    /// Both masks are ordinary scalar registers in this model, which is why the guest's
    /// own arithmetic on them needs no special handling - `s_and_b64 exec, exec, vcc` is
    /// two register reads, two ands and two register writes.
    fn mask_registers(name: &str) -> Result<(u32, u32), TranslateError> {
        match name {
            model::EXEC_LOW_HALF => Ok((EXEC_LO, EXEC_HI)),
            model::VCC_LOW_HALF => Ok((VCC_LO, VCC_HI)),
            _ => Err(TranslateError::Unsupported {
                offset: 0,
                detail: "that named operand is not a 64-bit lane mask this model knows",
            }),
        }
    }

    fn load_scalar(&mut self, register: u32) -> Id {
        let pointer = self.scalar_pointer(register);
        let loaded = self.builder.id();
        self.builder
            .function(op::LOAD, &[self.u32_type.0, loaded.0, pointer.0]);
        loaded
    }

    fn store_scalar(&mut self, register: u32, value: Id) {
        let pointer = self.scalar_pointer(register);
        self.builder.function(op::STORE, &[pointer.0, value.0]);
    }

    fn lane_pointer(&mut self, register: u32, lane: u32) -> Id {
        let register_index = self.constant(register);
        let lane_index = self.constant(lane);
        let pointer = self.builder.id();
        self.builder.function(
            op::ACCESS_CHAIN,
            &[
                self.lane_ptr.0,
                pointer.0,
                self.vectors.0,
                register_index.0,
                lane_index.0,
            ],
        );
        pointer
    }

    fn load_lane(&mut self, register: u32, lane: u32) -> Id {
        let pointer = self.lane_pointer(register, lane);
        let loaded = self.builder.id();
        self.builder
            .function(op::LOAD, &[self.u32_type.0, loaded.0, pointer.0]);
        loaded
    }

    /// Whether a lane is active, as a boolean.
    ///
    /// The mask is two halves because the guest stores it that way, so which half to
    /// consult is decided here rather than by anything downstream.
    fn lane_active(&mut self, lane: u32) -> Id {
        let (half, bit) = if lane < 32 {
            (EXEC_LO, lane)
        } else {
            (EXEC_HI, lane - 32)
        };
        let mask = self.load_scalar(half);
        let shift = self.constant(bit);
        let one = self.constant(1);
        let zero = self.constant(0);

        let shifted = self.builder.id();
        self.builder.function(
            op::SHIFT_RIGHT_LOGICAL,
            &[self.u32_type.0, shifted.0, mask.0, shift.0],
        );
        let isolated = self.builder.id();
        self.builder.function(
            op::BITWISE_AND,
            &[self.u32_type.0, isolated.0, shifted.0, one.0],
        );
        let active = self.builder.id();
        self.builder.function(
            op::INOT_EQUAL,
            &[self.bool_type.0, active.0, isolated.0, zero.0],
        );
        active
    }

    /// Writes one lane of a vector register, if that lane is active.
    ///
    /// A select rather than a branch: the old value is kept where the mask says the
    /// lane is inactive. No merge blocks, and the same result.
    fn store_lane_masked(&mut self, register: u32, lane: u32, value: Id) {
        let active = self.lane_active(lane);
        let old = self.load_lane(register, lane);
        let chosen = self.builder.id();
        self.builder.function(
            op::SELECT,
            &[self.u32_type.0, chosen.0, active.0, value.0, old.0],
        );
        let pointer = self.lane_pointer(register, lane);
        self.builder.function(op::STORE, &[pointer.0, chosen.0]);
    }

    /// Emits the epilogue and returns the module.
    ///
    /// Lane zero of each vector register, then the scalar registers - the same layout
    /// the lane model reports, which is what lets the two be diffed at all.
    pub fn finish(mut self) -> Result<(Vec<u32>, usize), TranslateError> {
        let member = self.constant(0);
        for register in 0..OBSERVED_REGISTERS {
            let value = self.load_lane(register, 0);
            self.write_observation(member, register, value);
        }
        for register in 0..OBSERVED_REGISTERS {
            let value = self.load_scalar(register);
            self.write_observation(member, OBSERVED_REGISTERS + register, value);
        }
        self.builder.function(op::RETURN, &[]);
        self.builder.function(op::FUNCTION_END, &[]);
        self.builder.check()?;
        Ok((self.builder.finish(), self.translated))
    }

    fn write_observation(&mut self, member: Id, slot: u32, value: Id) {
        let index = self.constant(slot);
        let to = self.builder.id();
        self.builder.function(
            op::ACCESS_CHAIN,
            &[
                self.buffer_element_ptr.0,
                to.0,
                self.buffer.0,
                member.0,
                index.0,
            ],
        );
        self.builder.function(op::STORE, &[to.0, value.0]);
    }
}

impl Model for Wavefront<'_> {
    fn encodings(&self) -> &EncodingTable {
        self.encodings
    }

    /// The whole wavefront: one invocation stands in for every lane.
    fn memory_words(&self) -> u32 {
        self.memory_words
    }

    fn lanes(&self) -> u32 {
        self.width.lanes()
    }

    fn constant(&mut self, value: u32) -> Id {
        Self::constant(self, value)
    }

    fn read_source(
        &mut self,
        instruction: &Instruction,
        operand: &Operand,
        lane: u32,
    ) -> Result<Id, TranslateError> {
        match operand {
            Operand::Integer(value) => {
                // A register holds thirty-two bits and an inline constant may be
                // negative, so the conversion is through `i32` to get two's complement:
                // -1 is 0xFFFF_FFFF, which is what the guest would read back. Going via
                // `u32::try_from` refuses it instead, and that refusal was reachable -
                // `s_mov_b64 s[n:n+1], -1` is an ordinary way to set a mask to all ones.
                let value = i32::try_from(*value).map_err(|_| TranslateError::Unsupported {
                    offset: instruction.offset,
                    detail: "inline constant does not fit in a register",
                })? as u32;
                Ok(Self::constant(self, value))
            }
            // Uniform across the wavefront, so the same value for every lane.
            Operand::Scalar(register) => Ok(self.load_scalar(u32::from(*register))),
            Operand::Vector(register) => Ok(self.load_lane(u32::from(*register), lane)),
            // A lane mask read as an ordinary 32-bit source: its low half.
            //
            // `s_and_b32 exec_lo, exec_lo, sN` is how a 32-lane shader narrows its mask,
            // and the mask appears on both sides of it. Without this the name falls
            // through to the inline-float parse and is refused for not being a float,
            // which is a true statement about the wrong thing.
            Operand::Named(named) if model::lane_mask_name(named).is_some() => {
                let mask = model::lane_mask_name(named).expect("checked immediately above");
                let (low, _) = self.read_lane_mask(mask)?;
                Ok(low)
            }
            // An inline float, named by the operand table. Its *bits* go into the
            // register, because that is what a register holds - storing the operand
            // code, or the value converted, are both plausible and both wrong.
            Operand::Named(name) => {
                let bits = name.parse::<f32>().map(f32::to_bits).map_err(|_| {
                    TranslateError::Unsupported {
                        offset: instruction.offset,
                        detail: "named operand is not an inline float",
                    }
                })?;
                Ok(Self::constant(self, bits))
            }
            // A literal: the thirty-two bits that follow the instruction, used verbatim.
            //
            // Uniform across the wavefront like any constant, and not converted - a
            // literal float and a literal integer are the same word and the instruction
            // decides which it is, exactly as with a register.
            Operand::Literal(value) => Ok(Self::constant(self, *value)),
            _ => Err(TranslateError::Unsupported {
                offset: instruction.offset,
                detail: "source operand kind is not translated yet",
            }),
        }
    }

    fn write_vector_lane(&mut self, register: u32, lane: u32, value: Id) {
        self.store_lane_masked(register, lane, value);
    }

    fn write_scalar(&mut self, register: u32, value: Id) {
        Self::store_scalar(self, register, value);
    }

    fn count(&mut self) {
        self.translated += 1;
    }

    fn builder(&mut self) -> &mut Builder {
        &mut self.builder
    }

    fn u32_type(&self) -> Id {
        self.u32_type
    }

    fn f32_type(&self) -> Id {
        self.f32_type
    }

    /// Writes both halves of a lane mask.
    ///
    /// An ordinary pair of register writes, which is the point of this model: a mask is
    /// a value the guest manipulates arithmetically, so representing it as one means the
    /// guest's own arithmetic translates directly instead of being reconstructed.
    fn write_lane_mask(&mut self, name: &str, low: Id, high: Id) -> Result<(), TranslateError> {
        let (lo, hi) = Self::mask_registers(name)?;
        self.store_scalar(lo, low);
        self.store_scalar(hi, high);
        Ok(())
    }

    fn read_lane_mask(&mut self, name: &str) -> Result<(Id, Id), TranslateError> {
        let (lo, hi) = Self::mask_registers(name)?;
        Ok((self.load_scalar(lo), self.load_scalar(hi)))
    }

    fn bool_type(&mut self) -> Id {
        self.bool_type
    }

    fn program_counter(&mut self) -> Id {
        self.program_counter
    }

    fn condition_code(&mut self) -> Id {
        self.condition_code
    }

    fn instructions(&self) -> usize {
        self.translated
    }

    /// Reads one word of the local data share.
    ///
    /// Unmasked: a read has no effect anything else can observe, so suppressing it for an
    /// inactive lane would cost an instruction and change nothing.
    fn read_local(&mut self, word_index: Id) -> Result<Id, TranslateError> {
        let (pointer_type, array, u32_type) = (self.local_ptr, self.local, self.u32_type);
        let b = &mut self.builder;
        let pointer = b.id();
        // One index, not two: this is a bare array rather than a struct containing one,
        // so it has no member to select first. The storage buffers next door do need
        // both, and confusing the two faulted a driver earlier in this crate's life.
        b.function(
            op::ACCESS_CHAIN,
            &[pointer_type.0, pointer.0, array.0, word_index.0],
        );
        let value = b.id();
        b.function(op::LOAD, &[u32_type.0, value.0, pointer.0]);
        Ok(value)
    }

    /// Writes one word of the local data share, keeping the old value where the lane is
    /// inactive.
    ///
    /// Masked, and for a sharper reason than a register write: another lane of this same
    /// wavefront will read this word, so a suppressed write that lands anyway corrupts a
    /// value a different lane is about to use.
    fn write_local(&mut self, word_index: Id, value: Id, lane: u32) -> Result<(), TranslateError> {
        let active = self.lane_active(lane);
        let (pointer_type, array, u32_type) = (self.local_ptr, self.local, self.u32_type);

        let b = &mut self.builder;
        let pointer = b.id();
        b.function(
            op::ACCESS_CHAIN,
            &[pointer_type.0, pointer.0, array.0, word_index.0],
        );
        let old = b.id();
        b.function(op::LOAD, &[u32_type.0, old.0, pointer.0]);
        let chosen = b.id();
        b.function(
            op::SELECT,
            &[u32_type.0, chosen.0, active.0, value.0, old.0],
        );
        b.function(op::STORE, &[pointer.0, chosen.0]);
        Ok(())
    }

    fn memory_buffer(&self) -> Id {
        self.memory
    }

    fn memory_element_ptr(&self) -> Id {
        self.memory_element_ptr
    }

    fn read_scalar(&mut self, register: u32) -> Id {
        Self::load_scalar(self, register)
    }

    /// Masked, by keeping whatever was already there where the lane is inactive.
    ///
    /// The same select the register writes use, and necessary for the same reason: an
    /// inactive lane's store would otherwise land in memory another lane goes on to
    /// read.
    fn write_memory(&mut self, word_index: Id, value: Id, lane: u32) {
        let active = self.lane_active(lane);
        let (element_ptr, buffer, u32_type) = (self.memory_element_ptr, self.memory, self.u32_type);
        let member = Self::constant(self, 0);

        let pointer = self.builder.id();
        self.builder.function(
            op::ACCESS_CHAIN,
            &[element_ptr.0, pointer.0, buffer.0, member.0, word_index.0],
        );
        let old = self.builder.id();
        self.builder
            .function(op::LOAD, &[u32_type.0, old.0, pointer.0]);
        let chosen = self.builder.id();
        self.builder.function(
            op::SELECT,
            &[u32_type.0, chosen.0, active.0, value.0, old.0],
        );
        self.builder.function(op::STORE, &[pointer.0, chosen.0]);
    }
}

/// Translates a whole decoded shader at wavefront fidelity.
pub fn translate(
    decode: &Decode,
    encodings: &EncodingTable,
    width: Width,
) -> Result<(Vec<u32>, usize), TranslateError> {
    let mut module = Wavefront::new(encodings, width);
    crate::control::emit(&mut module, decode, encodings)?;
    module.finish()
}
