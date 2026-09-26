//! The predicated strategy: one invocation per lane, registers in memory.
//!
//! A SPIR-V result belongs to the block that produced it, and the dispatch loop puts each
//! guest block in a different switch arm, so the guest's registers live in a private
//! array indexed by register number. Every guest instruction becomes a load, an operation
//! and a store: correctness first (D098). Before returning, the low registers are copied
//! into a storage buffer so a test can assert on them on a real device. An instruction this
//! does not translate is an error, never a no-op.

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
use crate::wavefront::Window;

/// Vector registers the register file holds.
///
/// The architecture's maximum, so a shader indexing high registers cannot run off the end.
pub const REGISTER_COUNT: u32 = 256;

/// How many registers of each file are copied into the storage buffer for inspection.
///
/// A window for tests, not a memory dump; every word costs a store in every translated
/// shader.
pub const OBSERVED_REGISTERS: u32 = 8;

/// Words the observation buffer needs: the vector file, then the scalar file.
pub const OBSERVED_WORDS: u32 = OBSERVED_REGISTERS * 2;

/// Words of guest memory a translated module can reach.
///
/// The default window length. The caller supplies the window's base and length
/// ([`Window`]); an access outside it reads zero and drops writes (D101).
pub const MEMORY_WORDS: u32 = 64;

/// Private storage class: per-invocation, outlives a block.
const PRIVATE: u32 = 6;

/// The machinery that turns a per-invocation `bool` into a guest lane mask.
///
/// The per-lane model's invocation is an unspecified lane, which cannot hold a
/// sixty-four-bit mask. In a subgroup, each invocation keeps one boolean saying whether it
/// is active, and a ballot materialises the mask word the guest's scalar instructions
/// expect. That needs the host subgroup to be as wide as the guest wavefront, a device
/// property the module declares and the caller checks.
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
    /// Carried rather than read from a constant so a test can widen the window and reach an
    /// address the default cannot hold (D101).
    memory_words: u32,
    /// The guest address the memory window starts at. See [`Model::memory_base`].
    ///
    /// Zero unless a caller says otherwise; a real guest's buffers are never at zero.
    memory_base: u32,
    builder: Builder,
    encodings: &'a EncodingTable,
    /// Present when lanes can be masked - see [`Mask`]. `None` is the per-lane model,
    /// which refuses every mask it is asked about.
    mask: Option<Mask>,
    /// Deduplicated unsigned constants: one declaration per value.
    constants: BTreeMap<u32, Id>,
    /// The imported `GLSL.std.450` set id, cached after the first extended instruction imports it.
    glsl_set: Option<Id>,
    u32_type: Id,
    f32_type: Id,
    /// The sixteen-bit types, declared on first use along with their capabilities.
    ///
    /// [`None`] until something needs them, which for nearly every module is never: only a
    /// typed buffer load of a half-format channel reaches them. Declaring them eagerly would
    /// require two device features every module does not use.
    f16_type: Option<Id>,
    u16_type: Option<Id>,
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
    /// The `m0` register, as a private word starting at zero.
    m0: Id,
    /// Instructions emitted into the function body so far.
    translated: usize,
    /// The subgroup width this module needs, when it needs one.
    required_subgroup: Option<u32>,
}

/// Declares the module's capabilities, entry point and workgroup size.
///
/// `lane_input` is the built-in this module reads its lane number from, when it has one. At
/// this SPIR-V version an input variable must be named in the entry point's interface as
/// well as declared, or the module is rejected.
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
/// Two capabilities, a built-in input, a vector type and two flags.
fn declare_mask(builder: &mut Builder, lane: Id, u32_type: Id, bool_type: Id) -> Mask {
    let (ballot_type, lane_ptr, bool_ptr) = (builder.id(), builder.id(), builder.id());
    let (active, condition, truth, scope) =
        (builder.id(), builder.id(), builder.id(), builder.id());

    builder.header(op::CAPABILITY, &[capability::GROUP_NON_UNIFORM]);
    builder.header(op::CAPABILITY, &[capability::GROUP_NON_UNIFORM_BALLOT]);

    // The lane index is an input the implementation fills in, so it is decorated as a
    // built-in and, at this version, listed in the entry point's interface.
    builder.annotate(
        op::DECORATE,
        &[
            lane.0,
            decoration::BUILT_IN,
            built_in::SUBGROUP_LOCAL_INVOCATION_ID,
        ],
    );

    // Four words: what a ballot answers with whatever the subgroup's width.
    builder.declare(op::TYPE_VECTOR, &[ballot_type.0, u32_type.0, 4]);
    builder.declare(op::TYPE_POINTER, &[lane_ptr.0, storage::INPUT, u32_type.0]);
    builder.declare(op::VARIABLE, &[lane_ptr.0, lane.0, storage::INPUT]);
    builder.declare(op::TYPE_POINTER, &[bool_ptr.0, PRIVATE, bool_type.0]);
    builder.declare(op::CONSTANT_TRUE, &[bool_type.0, truth.0]);
    // Every lane starts active, as the wavefront model starts with every mask bit set.
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
    pub fn new(encodings: &'a EncodingTable, window: Window) -> Self {
        Self::build(encodings, None, window)
    }

    /// Prepares a module whose lanes are the invocations of a subgroup.
    ///
    /// The same model with a mask added: one invocation is one lane either way, and a
    /// subgroup can be polled for a mask word. Correct only where the host subgroup is as
    /// wide as the guest wavefront; [`Predicated::finish`] reports the width it needs.
    pub fn subgroup(encodings: &'a EncodingTable, width: Width, window: Window) -> Self {
        let mut this = Self::build(encodings, Some(width.lanes()), window);
        this.required_subgroup = Some(width.lanes());
        this
    }

    /// Prepares a module: types, the register file, and the observation buffer.
    fn build(encodings: &'a EncodingTable, lanes: Option<u32>, window: Window) -> Self {
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
        let m0 = builder.id();

        // Reserved before the entry point is written: the interface names the input
        // variable, and the entry point is emitted before the variable is declared.
        let lane_input = lanes.is_some().then(|| builder.id());
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

        // The register file, with a null initialiser: a private variable is otherwise
        // undefined at entry, and an untouched register must read zero.
        builder.declare(
            op::TYPE_ARRAY,
            &[register_array.0, u32_type.0, register_count.0],
        );
        builder.declare(
            op::TYPE_POINTER,
            &[register_array_ptr.0, PRIVATE, register_array.0],
        );
        builder.declare(op::TYPE_POINTER, &[register_ptr.0, PRIVATE, u32_type.0]);
        // The program counter, the scalar condition code and m0: one private word each,
        // sharing a pointer type and a zero initialiser, so the shader starts at its first
        // block.
        builder.declare(op::TYPE_POINTER, &[counter_ptr.0, PRIVATE, u32_type.0]);
        builder.declare(op::CONSTANT, &[u32_type.0, counter_zero.0, 0]);
        for word in [counter, scc, m0] {
            builder.declare(
                op::VARIABLE,
                &[counter_ptr.0, word.0, PRIVATE, counter_zero.0],
            );
        }
        builder.declare(op::CONSTANT_NULL, &[register_array.0, register_zero.0]);
        builder.declare(
            op::VARIABLE,
            &[register_array_ptr.0, registers.0, PRIVATE, register_zero.0],
        );
        // A second file, identical in shape: the guest addresses scalar and vector
        // registers separately.
        builder.declare(
            op::VARIABLE,
            &[register_array_ptr.0, scalars.0, PRIVATE, register_zero.0],
        );

        builder.declare(op::CONSTANT, &[u32_type.0, memory_count.0, window.words()]);

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
            memory_base: window.base,
            memory_words: window.words(),
            mask,
            constants: BTreeMap::new(),
            glsl_set: None,
            u32_type,
            f32_type,
            f16_type: None,
            u16_type: None,
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
            m0,
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
    /// The epilogue copies the low registers into the storage buffer, unrolled because a
    /// loop needs a structured merge block and this is a fixed handful of stores.
    pub fn finish(mut self) -> Result<(Vec<u32>, usize), TranslateError> {
        // The buffer is a struct holding one array, so an access chain takes two indices:
        // the member (always zero), then the element.
        let member = self.constant(0);
        // Vector file first, then scalar, mirrored by the accessors in the tests.
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
    /// The refusal both local-share methods return.
    fn no_local_share() -> TranslateError {
        TranslateError::Unsupported {
            offset: 0,
            detail: concat!(
                "the lane model has no local data share. Lanes are separate ",
                "invocations here, so storage they share cannot be represented - ",
                "each would get its own and read back only what it wrote itself. ",
                "Translate at wavefront fidelity instead"
            ),
        }
    }

    /// The refusal both mask methods return. The offset is zero because this is a property
    /// of the model; the caller knows which instruction asked.
    fn no_lane_masks() -> TranslateError {
        TranslateError::Unsupported {
            offset: 0,
            detail: concat!(
                "the lane model has no execution mask and no condition mask. Lanes ",
                "are separate invocations here, so neither an inactive lane nor a ",
                "per-lane comparison result can be represented - translate at ",
                "wavefront fidelity instead"
            ),
        }
    }
}

impl Model for Predicated<'_> {
    fn encodings(&self) -> &EncodingTable {
        self.encodings
    }

    fn memory_words(&self) -> u32 {
        self.memory_words
    }

    fn memory_base(&self) -> u32 {
        self.memory_base
    }

    /// One: an invocation is a lane in this model, so a per-lane loop runs once.
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
                // A register holds thirty-two bits and an inline constant may be negative,
                // so the conversion goes through `i32` for two's complement: -1 is
                // 0xFFFF_FFFF, as in `s_mov_b64 s[n:n+1], -1` setting a mask to all ones.
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
            // A lane mask, in a model that has none, refused by name.
            Operand::Named(named) if model::lane_mask_name(named).is_some() => {
                Err(TranslateError::Unsupported {
                    offset: instruction.offset,
                    detail: concat!(
                        "this model has no lane mask, so a mask cannot be read as a source - ",
                        "translate at wavefront fidelity instead"
                    ),
                })
            }
            // The m0 register read back as a source: whatever the shader last wrote.
            Operand::Named(name) if name == model::M0 => Ok(self.read_m0()),
            // An inline float, named by the operand table. Its bits go into the register,
            // as a register holds bits.
            Operand::Named(name) => {
                let bits = name.parse::<f32>().map(f32::to_bits).map_err(|_| {
                    TranslateError::Unsupported {
                        offset: instruction.offset,
                        detail: "named operand is not an inline float",
                    }
                })?;
                Ok(Self::constant(self, bits))
            }
            // A literal: the thirty-two bits following the instruction, used verbatim. The
            // instruction decides whether they are a float or an integer.
            Operand::Literal(value) => Ok(Self::constant(self, *value)),
            _ => Err(TranslateError::Unsupported {
                offset: instruction.offset,
                detail: "source operand kind is not translated yet",
            }),
        }
    }

    /// Writes under this invocation's active flag when the model has a subgroup mask;
    /// the per-lane model has no execution mask and writes unconditionally (D098).
    fn write_vector_lane(&mut self, register: u32, _lane: u32, value: Id) {
        let file = self.registers;

        // An inactive lane must not write. A select against the register's previous value
        // rather than a branch, so the invocations do not diverge around a store.
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
    /// Overridden because the caller's lane index belongs to a loop this model does not run:
    /// which lane an invocation is comes from the built-in at run time. Taking the caller's
    /// zero would make every invocation read lane zero's bit.
    fn lane_bit(&mut self, low: Id, high: Id, _lane: u32) -> Id {
        let Some(mask) = self.mask else {
            // Unreachable: anything reading a mask is refused before dispatch reaches
            // here.
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

    fn glsl_set(&mut self) -> Id {
        if let Some(set) = self.glsl_set {
            return set;
        }
        let set = self.builder.ext_inst_import("GLSL.std.450");
        self.glsl_set = Some(set);
        set
    }

    fn f16_type(&mut self) -> Id {
        if let Some(id) = self.f16_type {
            return id;
        }
        let id = self.builder.id();
        self.builder.header(op::CAPABILITY, &[capability::FLOAT16]);
        self.builder.declare(op::TYPE_FLOAT, &[id.0, 16]);
        self.f16_type = Some(id);
        id
    }

    fn u16_type(&mut self) -> Id {
        if let Some(id) = self.u16_type {
            return id;
        }
        let id = self.builder.id();
        self.builder.header(op::CAPABILITY, &[capability::INT16]);
        self.builder.declare(op::TYPE_INT, &[id.0, 16, 0]);
        self.u16_type = Some(id);
        id
    }

    /// Refused without a subgroup mask: one invocation per lane with no way to be inactive,
    /// so ignoring the write would run every lane the guest disabled. With a mask, this
    /// invocation keeps its own bit.
    fn write_lane_mask(&mut self, name: &str, low: Id, high: Id) -> Result<(), TranslateError> {
        let Some(mask) = self.mask else {
            return Err(Self::no_lane_masks());
        };

        // The reverse of the ballot: this invocation keeps only its own bit of the mask.
        // Which half holds it depends on the run-time lane, so both halves are shifted and
        // the right one selected.
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

    /// Refused without a subgroup mask. Answering "every lane is active" would be wrong for
    /// `vcc`, which holds the last comparison's result.
    fn read_lane_mask(&mut self, name: &str) -> Result<(Id, Id), TranslateError> {
        let Some(mask) = self.mask else {
            return Err(Self::no_lane_masks());
        };

        // Every invocation reports whether its lane is active; the ballot's answer is the
        // mask word the guest's scalar instructions read.
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
        // A ballot answers with four words whatever the subgroup's width; the guest's mask
        // is the first two.
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

    fn read_m0(&mut self) -> Id {
        let (u32_type, pointer) = (self.u32_type, self.m0);
        let value = self.builder.id();
        self.builder
            .function(op::LOAD, &[u32_type.0, value.0, pointer.0]);
        value
    }

    fn write_m0(&mut self, value: Id) {
        let pointer = self.m0;
        self.builder.function(op::STORE, &[pointer.0, value.0]);
    }

    fn instructions(&self) -> usize {
        self.translated
    }

    /// Refused: each invocation would get its own copy, so lanes exchanging values would
    /// read back only what each wrote.
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

    /// Unmasked, like every write in this model.
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

/// Translates a whole decoded shader with the invocations of a subgroup as its lanes.
///
/// Returns the module, the instruction count, and the subgroup width the module needs: one
/// invocation is one lane, so the host subgroup must be exactly as wide as the guest
/// wavefront, which the caller checks against the device.
pub fn translate_subgroup(
    decode: &Decode,
    encodings: &EncodingTable,
    width: Width,
    window: Window,
) -> Result<(Vec<u32>, usize, u32), TranslateError> {
    let mut module = Predicated::subgroup(encodings, width, window);
    crate::control::emit(&mut module, decode, encodings)?;
    let required = module.required_subgroup.expect("set by `subgroup`");
    let (words, count) = module.finish()?;
    Ok((words, count, required))
}

/// Translates a whole decoded shader with one invocation per lane and no mask.
///
/// Refuses anything that needs to know which lane it is.
pub fn translate(
    decode: &Decode,
    encodings: &EncodingTable,
    window: Window,
) -> Result<(Vec<u32>, usize), TranslateError> {
    let mut module = Predicated::new(encodings, window);
    crate::control::emit(&mut module, decode, encodings)?;
    module.finish()
}
