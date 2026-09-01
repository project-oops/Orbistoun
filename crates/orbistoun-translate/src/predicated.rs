//! The predicated strategy: one invocation per lane, registers in memory.
//!
//! # The register file is memory, not values
//!
//! A SPIR-V result is bound to the block that produced it, and the dispatch loop this
//! strategy is heading towards puts every guest block behind a different arm of a
//! switch. Values cannot cross those arms, so the guest's registers have to live
//! somewhere that outlives a block - a private array, indexed by register number.
//!
//! That is also why this is the slow strategy: every guest instruction becomes a load,
//! an operation and a store, where a structured translation would keep the value in a
//! register the driver can see. Correctness first (D098).
//!
//! # Registers are copied out so they can be asserted on
//!
//! Before returning, the low registers are written into the storage buffer. Nothing
//! about the guest asks for that - it exists so a test can say "after this instruction,
//! v3 holds 9" and have a real device settle it. Without it the register file is
//! invisible and translation is unverifiable until something renders, which is months
//! away.
//!
//! # Refusing is the default
//!
//! An instruction this does not know how to translate is an error, never a no-op. A
//! shader missing one instruction computes the wrong thing while looking like it
//! worked, and that is far harder to find than a translator that stops and names what
//! it hit.

use orbistoun_shader::{Decode, EncodingTable, Instruction, Operand};
use orbistoun_spirv::{
    Builder, Id, addressing, built_in, capability, decoration, execution, memory, mode, op, scope,
    storage,
};
use std::collections::BTreeMap;

use crate::TranslateError;
use crate::Width;
use crate::buffer;
use crate::model::{self, Model};

/// Vector registers the register file holds.
///
/// The architecture's maximum. Sized for the worst case rather than for what a
/// particular shader uses, because a shader that indexes past the end would otherwise
/// corrupt whatever followed.
pub const REGISTER_COUNT: u32 = 256;

/// How many registers of each file are copied into the storage buffer for inspection.
///
/// Small on purpose. This is a window for tests, not a memory dump, and every word
/// costs a store in every translated shader.
pub const OBSERVED_REGISTERS: u32 = 8;

/// Words the observation buffer needs: the vector file, then the scalar file.
pub const OBSERVED_WORDS: u32 = OBSERVED_REGISTERS * 2;

/// Words of guest memory a translated module can reach.
///
/// Small, and a placeholder for a real mapping. A guest address is an arbitrary
/// sixty-four-bit value; here it indexes a buffer directly, which works because the
/// tests choose the addresses. When real submissions arrive this becomes a base and a
/// length, and an address outside them has to be refused rather than wrapped - a store
/// that silently lands somewhere else is the worst failure this layer can produce.
pub const MEMORY_WORDS: u32 = 64;

/// Private storage class: per-invocation, outlives a block.
const PRIVATE: u32 = 6;

/// The machinery that turns a per-invocation `bool` into a guest lane mask.
///
/// # Why one invocation per lane can have a mask at all
///
/// The per-lane model refuses masks because its single invocation is not lane zero, it
/// is an unspecified lane, and a sixty-four-bit mask is not a thing one lane holds.
///
/// A **subgroup** changes that. The invocations of a subgroup run together and can be
/// asked, all at once, which of them satisfy a condition - and the answer comes back as
/// exactly the bit mask the guest's scalar instructions expect. So each invocation keeps
/// one boolean saying whether *it* is active, and a ballot materialises the mask
/// whenever the shader wants to read one as a value.
///
/// That only works while one invocation is one lane, which means the host's subgroup must
/// be as wide as the guest's wavefront. That is a fact about the device rather than about
/// the shader, so it is not checked here - it is *declared* by the translated module and
/// checked where the device is known.
#[derive(Debug, Clone, Copy)]
struct Mask {
    /// Whether this invocation's lane is active. A `bool` in `Private` storage.
    active: Id,
    /// The condition mask, per invocation, the same way.
    condition: Id,
    /// This invocation's index within its subgroup: the guest's lane number.
    lane: Id,
    /// The subgroup execution scope, as the constant the group operations take.
    scope: Id,
    /// The type a ballot answers with: four words.
    ballot_type: Id,
}

/// Builds a module for one decoded shader.
#[derive(Debug)]
pub struct Predicated<'a> {
    /// How many words of guest memory this module addresses.
    ///
    /// Carried rather than read from a constant so a test can widen the window and reach
    /// an address the default cannot hold - which is otherwise impossible, and is why
    /// every buffer test has to pretend memory starts at zero (D101).
    memory_words: u32,
    builder: Builder,
    encodings: &'a EncodingTable,
    /// Present when lanes can be masked - see [`Mask`]. `None` is the per-lane model,
    /// which refuses every mask it is asked about.
    mask: Option<Mask>,
    /// Deduplicated unsigned constants. SPIR-V wants one declaration per value, and a
    /// second identical constant is at best noise in the module.
    constants: BTreeMap<u32, Id>,
    u32_type: Id,
    f32_type: Id,
    bool_type: Id,
    register_ptr: Id,
    registers: Id,
    scalars: Id,
    buffer_element_ptr: Id,
    buffer: Id,
    memory_element_ptr: Id,
    memory: Id,
    /// The program counter the dispatch loop switches on.
    program_counter: Id,
    /// The scalar condition code, as a private variable holding 0 or 1.
    condition_code: Id,
    /// Instructions emitted into the function body so far.
    translated: usize,
    /// The subgroup width this module needs, when it needs one.
    required_subgroup: Option<u32>,
}

/// Declares the module's capabilities, entry point and workgroup size.
///
/// Split out because it is self-contained and because the constructor around it is long
/// enough that the parts common to both models were hard to pick out of it.
///
/// `lane_input` is the built-in this module reads its lane number from, when it has one.
/// At this SPIR-V version an input variable must be named in the entry point's interface
/// as well as declared, and leaving it out produces a module that is rejected rather than
/// one that misbehaves.
fn declare_entry_point(builder: &mut Builder, main: Id, lane_input: Option<Id>, group: u32) {
    builder.header(op::CAPABILITY, &[capability::SHADER]);
    builder.header(op::MEMORY_MODEL, &[addressing::LOGICAL, memory::GLSL450]);

    let mut entry = vec![execution::GL_COMPUTE, main.0];
    entry.extend(Builder::literal_string("main"));
    entry.extend(lane_input.map(|id| id.0));
    builder.header(op::ENTRY_POINT, &entry);
    // One invocation per lane, so a workgroup is a wavefront.
    builder.header(op::EXECUTION_MODE, &[main.0, mode::LOCAL_SIZE, group, 1, 1]);
}

/// Declares everything a subgroup mask needs, and returns the handles.
///
/// Split out of the constructor because it is a self-contained piece - two capabilities,
/// a built-in input, a vector type and two flags - and inlining it made the constructor
/// long enough that the parts which are common to both models were hard to pick out.
fn declare_mask(builder: &mut Builder, lane: Id, u32_type: Id, bool_type: Id) -> Mask {
    let (ballot_type, lane_ptr, bool_ptr) = (builder.id(), builder.id(), builder.id());
    let (active, condition, truth, scope) =
        (builder.id(), builder.id(), builder.id(), builder.id());

    builder.header(op::CAPABILITY, &[capability::GROUP_NON_UNIFORM]);
    builder.header(op::CAPABILITY, &[capability::GROUP_NON_UNIFORM_BALLOT]);

    // The lane index is an input the implementation fills in, so it is decorated
    // as a built-in and - at this version - has to appear in the entry point's
    // interface as well. Leaving it out produces a module that is rejected.
    builder.annotate(
        op::DECORATE,
        &[
            lane.0,
            decoration::BUILT_IN,
            built_in::SUBGROUP_LOCAL_INVOCATION_ID,
        ],
    );

    // Four words, because that is what a ballot answers with whatever the
    // subgroup's actual width.
    builder.declare(op::TYPE_VECTOR, &[ballot_type.0, u32_type.0, 4]);
    builder.declare(op::TYPE_POINTER, &[lane_ptr.0, storage::INPUT, u32_type.0]);
    builder.declare(op::VARIABLE, &[lane_ptr.0, lane.0, storage::INPUT]);
    builder.declare(op::TYPE_POINTER, &[bool_ptr.0, PRIVATE, bool_type.0]);
    builder.declare(op::CONSTANT_TRUE, &[bool_type.0, truth.0]);
    // Every lane starts active, the same way the wavefront model starts with
    // every bit of its mask set.
    builder.declare(op::VARIABLE, &[bool_ptr.0, active.0, PRIVATE, truth.0]);
    builder.declare(op::VARIABLE, &[bool_ptr.0, condition.0, PRIVATE, truth.0]);
    builder.declare(op::CONSTANT, &[u32_type.0, scope.0, scope::SUBGROUP]);

    Mask {
        active,
        condition,
        lane,
        scope,
        ballot_type,
    }
}

impl<'a> Predicated<'a> {
    /// Prepares a module with no lane mask: the per-lane model.
    pub fn new(encodings: &'a EncodingTable) -> Self {
        Self::build(encodings, None)
    }

    /// Prepares a module whose lanes are the invocations of a subgroup.
    ///
    /// The same model with a mask bolted on, because that is the only difference: one
    /// invocation is one lane either way, and what a subgroup adds is the ability to ask
    /// all of them at once and get a mask word back.
    ///
    /// The module it produces is only correct where the host's subgroup is as wide as the
    /// guest's wavefront, which is a property of the device. [`Predicated::finish`]
    /// reports the width it needs so that can be checked where it is known.
    pub fn subgroup(encodings: &'a EncodingTable, width: Width) -> Self {
        let mut this = Self::build(encodings, Some(width.lanes()));
        this.required_subgroup = Some(width.lanes());
        this
    }

    /// Prepares a module: types, the register file, and the observation buffer.
    fn build(encodings: &'a EncodingTable, lanes: Option<u32>) -> Self {
        let with_mask = lanes.is_some();
        let mut builder = Builder::new().with_version(orbistoun_spirv::VERSION_1_3);

        let void = builder.id();
        let fn_type = builder.id();
        let u32_type = builder.id();
        let f32_type = builder.id();
        let bool_type = builder.id();
        let register_array = builder.id();
        let register_array_ptr = builder.id();
        let register_ptr = builder.id();
        let registers = builder.id();
        let scalars = builder.id();
        let register_zero = builder.id();
        let main = builder.id();
        let entry_block = builder.id();
        let register_count = builder.id();
        let observed_count = builder.id();
        let memory_count = builder.id();
        let counter_ptr = builder.id();
        let counter = builder.id();
        let counter_zero = builder.id();
        let scc = builder.id();

        // Reserved before the entry point is written, because at this version an input
        // variable has to be named in the entry point's interface and the entry point is
        // emitted before the variable is declared.
        let lane_input = with_mask.then(|| builder.id());
        declare_entry_point(&mut builder, main, lane_input, lanes.unwrap_or(1));

        builder.declare(op::TYPE_VOID, &[void.0]);
        builder.declare(op::TYPE_FUNCTION, &[fn_type.0, void.0]);
        builder.declare(op::TYPE_INT, &[u32_type.0, 32, 0]);
        builder.declare(op::TYPE_FLOAT, &[f32_type.0, 32]);
        builder.declare(op::TYPE_BOOL, &[bool_type.0]);
        builder.declare(
            op::CONSTANT,
            &[u32_type.0, register_count.0, REGISTER_COUNT],
        );
        builder.declare(
            op::CONSTANT,
            &[u32_type.0, observed_count.0, OBSERVED_WORDS],
        );

        // The register file. Given a null initialiser because a private variable is
        // otherwise undefined at entry, and a test asserting that an untouched register
        // reads zero would then be asserting on whatever the driver left there.
        builder.declare(
            op::TYPE_ARRAY,
            &[register_array.0, u32_type.0, register_count.0],
        );
        builder.declare(
            op::TYPE_POINTER,
            &[register_array_ptr.0, PRIVATE, register_array.0],
        );
        builder.declare(op::TYPE_POINTER, &[register_ptr.0, PRIVATE, u32_type.0]);
        // The program counter. Zero-initialised so the shader starts at its first
        // block rather than at whatever the driver left in the variable.
        builder.declare(op::TYPE_POINTER, &[counter_ptr.0, PRIVATE, u32_type.0]);
        builder.declare(op::CONSTANT, &[u32_type.0, counter_zero.0, 0]);
        builder.declare(
            op::VARIABLE,
            &[counter_ptr.0, counter.0, PRIVATE, counter_zero.0],
        );
        // The scalar condition code shares the counter's pointer type and initialiser:
        // both are a private word starting at zero.
        builder.declare(
            op::VARIABLE,
            &[counter_ptr.0, scc.0, PRIVATE, counter_zero.0],
        );
        builder.declare(op::CONSTANT_NULL, &[register_array.0, register_zero.0]);
        builder.declare(
            op::VARIABLE,
            &[register_array_ptr.0, registers.0, PRIVATE, register_zero.0],
        );
        // A second file, identical in shape. The guest addresses scalar and vector
        // registers separately, so one array shared between them would put s2 where v2
        // lives and corrupt whichever was written second.
        builder.declare(
            op::VARIABLE,
            &[register_array_ptr.0, scalars.0, PRIVATE, register_zero.0],
        );

        builder.declare(op::CONSTANT, &[u32_type.0, memory_count.0, MEMORY_WORDS]);

        let observation =
            buffer::declare(&mut builder, u32_type, observed_count, buffer::OBSERVATION);
        let guest_memory =
            buffer::declare(&mut builder, u32_type, memory_count, buffer::GUEST_MEMORY);

        let mask = lane_input.map(|lane| declare_mask(&mut builder, lane, u32_type, bool_type));

        builder.function(op::FUNCTION, &[void.0, main.0, 0, fn_type.0]);
        builder.function(op::LABEL, &[entry_block.0]);

        Self {
            builder,
            encodings,
            memory_words: MEMORY_WORDS,
            mask,
            constants: BTreeMap::new(),
            u32_type,
            f32_type,
            bool_type,
            register_ptr,
            registers,
            scalars,
            buffer_element_ptr: observation.element_ptr,
            buffer: observation.buffer,
            memory_element_ptr: guest_memory.element_ptr,
            memory: guest_memory.buffer,
            program_counter: counter,
            condition_code: scc,
            translated: 0,
            required_subgroup: None,
        }
    }

    /// The variable holding this invocation's bit of a named mask.
    const fn mask_variable(mask: Mask, name: &str) -> Id {
        if matches!(name.as_bytes(), b"vcc_lo") {
            mask.condition
        } else {
            mask.active
        }
    }

    /// This invocation's lane number, from the built-in.
    fn lane_index(&mut self, mask: Mask) -> Id {
        let u32_type = self.u32_type;
        let b = &mut self.builder;
        let value = b.id();
        b.function(op::LOAD, &[u32_type.0, value.0, mask.lane.0]);
        value
    }

    /// Whether this invocation's lane is active.
    fn load_flag(&mut self, variable: Id) -> Id {
        let bool_type = self.bool_type;
        let b = &mut self.builder;
        let value = b.id();
        b.function(op::LOAD, &[bool_type.0, value.0, variable.0]);
        value
    }

    /// An unsigned constant, declared once however often it is used.
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

    /// A pointer to one register of one file.
    fn register_pointer(&mut self, file: Id, register: u32) -> Id {
        let index = self.constant(register);
        let pointer = self.builder.id();
        self.builder.function(
            op::ACCESS_CHAIN,
            &[self.register_ptr.0, pointer.0, file.0, index.0],
        );
        pointer
    }

    /// Writes a value into a register.
    fn store_register(&mut self, file: Id, register: u32, value: Id) {
        let pointer = self.register_pointer(file, register);
        self.builder.function(op::STORE, &[pointer.0, value.0]);
    }

    /// Reads a register.
    fn load_register(&mut self, file: Id, register: u32) -> Id {
        let pointer = self.register_pointer(file, register);
        let loaded = self.builder.id();
        self.builder
            .function(op::LOAD, &[self.u32_type.0, loaded.0, pointer.0]);
        loaded
    }

    /// Emits the epilogue and returns the module.
    ///
    /// The epilogue copies the low registers into the storage buffer. Unrolled rather
    /// than looped: a loop needs a structured merge block, and this is a fixed handful
    /// of stores.
    pub fn finish(mut self) -> Result<(Vec<u32>, usize), TranslateError> {
        // The buffer is a struct holding one array, so an access chain into it takes
        // *two* indices: the member, which is always zero, and then the element. Using
        // the register number for both put element N at member N - which is in bounds
        // only for register zero, and which the driver answered with an access
        // violation rather than a diagnostic.
        let member = self.constant(0);
        // Vector file first, then scalar. A reader needs to know which half is which, so
        // the layout is stated here and mirrored by the accessors in the tests.
        for (base, file) in [(0, self.registers), (OBSERVED_REGISTERS, self.scalars)] {
            for register in 0..OBSERVED_REGISTERS {
                let value = self.load_register(file, register);
                let slot = self.constant(base + register);
                let to = self.builder.id();
                self.builder.function(
                    op::ACCESS_CHAIN,
                    &[
                        self.buffer_element_ptr.0,
                        to.0,
                        self.buffer.0,
                        member.0,
                        slot.0,
                    ],
                );
                self.builder.function(op::STORE, &[to.0, value.0]);
            }
        }
        self.builder.function(op::RETURN, &[]);
        self.builder.function(op::FUNCTION_END, &[]);
        self.builder.check()?;
        Ok((self.builder.finish(), self.translated))
    }
}

impl Predicated<'_> {
    /// The refusal both mask methods return.
    ///
    /// One message rather than two, because the reason is one reason and two copies
    /// drift. The offset is zero because this is a property of the model rather than of
    /// any particular instruction - the caller knows which instruction it was asking
    /// about.
    /// The refusal both local-share methods return.
    fn no_local_share() -> TranslateError {
        TranslateError::Unsupported {
            offset: 0,
            detail: "the lane model has no local data share. Lanes are separate \
                     invocations here, so storage they share cannot be represented - \
                     each would get its own and read back only what it wrote itself. \
                     Translate at wavefront fidelity instead",
        }
    }

    fn no_lane_masks() -> TranslateError {
        TranslateError::Unsupported {
            offset: 0,
            detail: "the lane model has no execution mask and no condition mask. Lanes \
                     are separate invocations here, so neither an inactive lane nor a \
                     per-lane comparison result can be represented - translate at \
                     wavefront fidelity instead",
        }
    }
}

impl Model for Predicated<'_> {
    fn encodings(&self) -> &EncodingTable {
        self.encodings
    }

    /// One. An invocation *is* a lane in this model, so a per-lane loop runs once and
    /// the lane index is always zero.
    fn memory_words(&self) -> u32 {
        self.memory_words
    }

    fn lanes(&self) -> u32 {
        1
    }

    fn constant(&mut self, value: u32) -> Id {
        Self::constant(self, value)
    }

    fn read_source(
        &mut self,
        instruction: &Instruction,
        operand: &Operand,
        _lane: u32,
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
            Operand::Vector(register) => {
                let file = self.registers;
                Ok(self.load_register(file, u32::from(*register)))
            }
            Operand::Scalar(register) => {
                let file = self.scalars;
                Ok(self.load_register(file, u32::from(*register)))
            }
            // A lane mask, in a model that has none. Refused by name rather than left
            // to fail as "not an inline float", which is true and unhelpful.
            Operand::Named(named) if model::lane_mask_name(named).is_some() => {
                Err(TranslateError::Unsupported {
                    offset: instruction.offset,
                    detail: "this model has no lane mask, so a mask cannot be read as a                              source - translate at wavefront fidelity instead",
                })
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

    /// Unmasked, and that is a real gap rather than an oversight.
    ///
    /// This model has no execution mask: with one invocation per lane, divergence is
    /// the hardware's business. That holds while every instruction is unconditional and
    /// stops holding the moment control flow arrives, which is why the mask and control
    /// flow want building together (D098).
    fn write_vector_lane(&mut self, register: u32, _lane: u32, value: Id) {
        let file = self.registers;

        // An inactive lane must not write. Expressed as a select against what the
        // register already held rather than as a branch, because a branch here would be
        // divergent control flow around a store and the whole point of this model is
        // that the invocations stay together.
        let value = match self.mask {
            Some(mask) => {
                let previous = self.load_register(file, register);
                let flag = self.load_flag(mask.active);
                let u32_type = self.u32_type;
                let b = &mut self.builder;
                let chosen = b.id();
                b.function(
                    op::SELECT,
                    &[u32_type.0, chosen.0, flag.0, value.0, previous.0],
                );
                chosen
            }
            None => value,
        };
        self.store_register(file, register, value);
    }

    /// One bit of a mask, for *this* invocation's lane.
    ///
    /// Overridden because the lane index the caller passes is the index of a loop this
    /// model does not run - it has one lane, and which lane that is is a runtime fact
    /// only the built-in knows. Taking the caller's zero would make every invocation
    /// read lane zero's bit, which is a shader that runs and is wrong in a way no
    /// single-lane test can see.
    fn lane_bit(&mut self, low: Id, high: Id, _lane: u32) -> Id {
        let Some(mask) = self.mask else {
            // Unreachable through the dispatch - anything reading a mask is refused
            // before it gets here - and answering "inactive" would be a quiet lie if it
            // ever were reached.
            let zero = Self::constant(self, 0);
            return self.is_not_zero(zero);
        };
        let lane = self.lane_index(mask);
        let thirty_two = Self::constant(self, 32);
        let one = Self::constant(self, 1);
        let zero = Self::constant(self, 0);

        let u32_type = self.u32_type;
        let bool_type = self.bool_type;
        let b = &mut self.builder;
        let upper = b.id();
        b.function(
            op::ULESS_THAN,
            &[bool_type.0, upper.0, thirty_two.0, lane.0],
        );
        let wrapped = b.id();
        b.function(op::ISUB, &[u32_type.0, wrapped.0, lane.0, thirty_two.0]);
        let index = b.id();
        b.function(
            op::SELECT,
            &[u32_type.0, index.0, upper.0, wrapped.0, lane.0],
        );
        let word = b.id();
        b.function(op::SELECT, &[u32_type.0, word.0, upper.0, high.0, low.0]);
        let shifted = b.id();
        b.function(
            op::SHIFT_RIGHT_LOGICAL,
            &[u32_type.0, shifted.0, word.0, index.0],
        );
        let bit = b.id();
        b.function(op::BITWISE_AND, &[u32_type.0, bit.0, shifted.0, one.0]);
        let flag = b.id();
        b.function(op::INOT_EQUAL, &[bool_type.0, flag.0, bit.0, zero.0]);
        flag
    }

    fn write_scalar(&mut self, register: u32, value: Id) {
        let file = self.scalars;
        self.store_register(file, register, value);
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

    /// Refused. This model has no lane masks.
    ///
    /// One invocation per lane and no way for a lane to be inactive, so a sixty-four-bit
    /// mask has nowhere to land. Ignoring the write would produce a shader where every
    /// lane runs regardless of what the guest disabled - plausible output, wrong answer,
    /// nothing to point at. The whole reason D098 keeps three levels is that they differ
    /// in correctness, and this is the difference.
    fn write_lane_mask(&mut self, name: &str, low: Id, high: Id) -> Result<(), TranslateError> {
        let Some(mask) = self.mask else {
            return Err(Self::no_lane_masks());
        };

        // The reverse of the ballot: a mask arrives as a word, and this invocation keeps
        // only the bit that is its own. Which half that bit is in depends on the lane,
        // which is a runtime value here - so both halves are shifted and the right one
        // selected, rather than branched on.
        let lane = self.lane_index(mask);
        let thirty_two = Self::constant(self, 32);
        let one = Self::constant(self, 1);
        let zero = Self::constant(self, 0);

        let u32_type = self.u32_type;
        let bool_type = self.bool_type;
        let b = &mut self.builder;

        let upper = b.id();
        b.function(
            op::ULESS_THAN,
            &[bool_type.0, upper.0, thirty_two.0, lane.0],
        );
        let wrapped = b.id();
        b.function(op::ISUB, &[u32_type.0, wrapped.0, lane.0, thirty_two.0]);
        let index = b.id();
        b.function(
            op::SELECT,
            &[u32_type.0, index.0, upper.0, wrapped.0, lane.0],
        );
        let word = b.id();
        b.function(op::SELECT, &[u32_type.0, word.0, upper.0, high.0, low.0]);
        let shifted = b.id();
        b.function(
            op::SHIFT_RIGHT_LOGICAL,
            &[u32_type.0, shifted.0, word.0, index.0],
        );
        let bit = b.id();
        b.function(op::BITWISE_AND, &[u32_type.0, bit.0, shifted.0, one.0]);
        let flag = b.id();
        b.function(op::INOT_EQUAL, &[bool_type.0, flag.0, bit.0, zero.0]);

        let variable = Self::mask_variable(mask, name);
        self.builder.function(op::STORE, &[variable.0, flag.0]);
        Ok(())
    }

    /// Refused, for the same reason.
    ///
    /// Answering "every lane is active" would be defensible for `exec` and is nonsense
    /// for `vcc`, which holds whatever the last comparison produced. One method covers
    /// both masks, so it refuses both rather than being right about one of them.
    fn read_lane_mask(&mut self, name: &str) -> Result<(Id, Id), TranslateError> {
        let Some(mask) = self.mask else {
            return Err(Self::no_lane_masks());
        };

        // The whole trick, in three instructions: every invocation says whether its lane
        // is active, the subgroup is polled, and the answer *is* the mask word the
        // guest's scalar instructions expect to read.
        let variable = Self::mask_variable(mask, name);
        let flag = self.load_flag(variable);

        let ballot_type = mask.ballot_type;
        let scope = mask.scope;
        let u32_type = self.u32_type;
        let b = &mut self.builder;
        let ballot = b.id();
        b.function(
            op::GROUP_NON_UNIFORM_BALLOT,
            &[ballot_type.0, ballot.0, scope.0, flag.0],
        );
        // A ballot answers with four words whatever the subgroup's width; the guest's
        // mask is the first two of them.
        let low = b.id();
        b.function(op::COMPOSITE_EXTRACT, &[u32_type.0, low.0, ballot.0, 0]);
        let high = b.id();
        b.function(op::COMPOSITE_EXTRACT, &[u32_type.0, high.0, ballot.0, 1]);
        Ok((low, high))
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

    /// Refused. One lane per invocation means no lanes to share between.
    ///
    /// Each invocation would get its own copy, so a shader using this to exchange values
    /// between lanes would read back only what it wrote itself. That runs, and is wrong.
    fn read_local(&mut self, _word_index: Id) -> Result<Id, TranslateError> {
        Err(Self::no_local_share())
    }

    fn write_local(&mut self, _i: Id, _v: Id, _lane: u32) -> Result<(), TranslateError> {
        Err(Self::no_local_share())
    }

    fn memory_buffer(&self) -> Id {
        self.memory
    }

    fn memory_element_ptr(&self) -> Id {
        self.memory_element_ptr
    }

    fn read_scalar(&mut self, register: u32) -> Id {
        let file = self.scalars;
        self.load_register(file, register)
    }

    /// Unmasked, like every write in this model - there is no execution mask here.
    fn write_memory(&mut self, word_index: Id, value: Id, _lane: u32) {
        let (element_ptr, buffer) = (self.memory_element_ptr, self.memory);
        let member = Self::constant(self, 0);
        let pointer = self.builder.id();
        self.builder.function(
            op::ACCESS_CHAIN,
            &[element_ptr.0, pointer.0, buffer.0, member.0, word_index.0],
        );
        self.builder.function(op::STORE, &[pointer.0, value.0]);
    }
}

/// Translates a whole decoded shader.
/// Translates a whole decoded shader with the invocations of a subgroup as its lanes.
///
/// Returns the module, the instruction count, and **the subgroup width the module needs.**
/// One invocation is one lane here, so the host's subgroup has to be exactly as wide as
/// the guest's wavefront. That is a property of the device rather than of the shader, so
/// it is reported for checking where the device is known rather than assumed here.
pub fn translate_subgroup(
    decode: &Decode,
    encodings: &EncodingTable,
    width: Width,
) -> Result<(Vec<u32>, usize, u32), TranslateError> {
    let mut module = Predicated::subgroup(encodings, width);
    crate::control::emit(&mut module, decode, encodings)?;
    let required = module.required_subgroup.expect("set by `subgroup`");
    let (words, count) = module.finish()?;
    Ok((words, count, required))
}

/// Translates a whole decoded shader with one invocation per lane and **no mask.**
///
/// The per-lane model: fast, simple, and refuses anything that needs to know which lane
/// it is. Its sibling above is the same machine with a subgroup mask.
pub fn translate(
    decode: &Decode,
    encodings: &EncodingTable,
) -> Result<(Vec<u32>, usize), TranslateError> {
    let mut module = Predicated::new(encodings);
    crate::control::emit(&mut module, decode, encodings)?;
    module.finish()
}
