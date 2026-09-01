//! Building SPIR-V modules.
//!
//! # This crate knows nothing about the guest
//!
//! It emits SPIR-V. It has never heard of wavefronts, execution masks or vector
//! registers, and it must stay that way - the same boundary `orbistoun-gpu` holds
//! against Vulkan, for the same reason. Translation lives above this and maps guest
//! semantics onto what is here.
//!
//! # Words, not text
//!
//! SPIR-V is a binary format of 32-bit words: a five-word header, then instructions,
//! each beginning with a word packing its length and opcode. Nothing here goes via an
//! assembler, because a translator that emits text and shells out to `spirv-as` cannot
//! run where it is needed.
//!
//! # Identifiers are handed out, never chosen
//!
//! Every result in a module is a number, and the header declares a bound that must
//! exceed all of them. [`Builder`] allocates them, so a mismatch between the bound and
//! the identifiers in use cannot happen - which is a whole class of module that
//! validates as malformed for a reason nobody can see by reading it.
//!
//! # Verification
//!
//! Structural properties are checked by unit tests here. Whether the output is *valid
//! SPIR-V* is answered by `spirv-val`, run over emitted modules by
//! `tools/validate-spirv.sh` - a real validator rather than this crate's opinion of
//! itself, which is the same argument the shader decoder's differential test makes.

use core::fmt;

/// First word of every module.
pub const MAGIC: u32 = 0x0723_0203;

/// Version 1.0, as the format packs it: minor in the second byte.
///
/// The floor. Every consumer accepts it, so anything that can be said in 1.0 is said
/// in 1.0.
pub const VERSION_1_0: u32 = 0x0001_0000;

/// Version 1.3.
///
/// Needed only for the `StorageBuffer` storage class. Version 1.0 can describe the same
/// thing using `Uniform` plus a `BufferBlock` decoration, but that spelling is
/// deprecated and drivers treat it as legacy - so a module that needs a storage buffer
/// declares 1.3 and one that does not stays at 1.0.
///
/// 1.4 is deliberately not used: from there on, an entry point must list *every* global
/// variable in its interface, and getting that wrong is a validation failure with a
/// confusing message.
pub const VERSION_1_3: u32 = 0x0001_0300;

/// Generator identifier. Zero means unregistered, which is honest: the registry exists
/// for tool vendors and this is not one of them.
pub const GENERATOR: u32 = 0;

/// An identifier for a result within a module.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Id(pub u32);

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "%{}", self.0)
    }
}

/// The opcodes this crate emits.
///
/// A subset, and named rather than numbered at the call site. Every value is checked
/// by `spirv-val` the first time a module using it is emitted - a wrong number
/// produces an instruction the validator rejects by name, which is about as loud as a
/// mistake can be.
pub mod op {
    /// Declares a capability the module needs.
    pub const CAPABILITY: u16 = 17;
    /// Declares a SPIR-V extension the module relies on, by name.
    ///
    /// Needed because this project emits SPIR-V 1.3, and the float controls that let a
    /// module say "do not flush subnormals to zero" were an extension until 1.4. Naming
    /// the extension is cheaper than raising the version, which would raise the Vulkan
    /// version the host must support along with it.
    pub const EXTENSION: u16 = 10;
    /// Declares the addressing and memory model.
    pub const MEMORY_MODEL: u16 = 14;
    /// Names a function as an entry point.
    pub const ENTRY_POINT: u16 = 15;
    /// Declares how an entry point executes.
    pub const EXECUTION_MODE: u16 = 16;
    /// The void type.
    pub const TYPE_VOID: u16 = 19;
    /// The boolean type.
    pub const TYPE_BOOL: u16 = 20;
    /// An integer type.
    pub const TYPE_INT: u16 = 21;
    /// A vector of a component type.
    ///
    /// Needed for exactly one thing so far: a subgroup ballot answers with four words,
    /// because the largest subgroup SPIR-V admits is 128 lanes wide.
    pub const TYPE_VECTOR: u16 = 23;
    /// Pulls one component out of a composite by a literal index.
    pub const COMPOSITE_EXTRACT: u16 = 81;
    /// The set of invocations in a subgroup for which a condition holds, as a bit mask.
    ///
    /// This is what makes a per-invocation `bool` and a guest lane mask the same thing:
    /// each invocation says whether *it* is active, and the ballot turns that into the
    /// mask word the guest's scalar instructions expect to read.
    pub const GROUP_NON_UNIFORM_BALLOT: u16 = 339;
    /// A floating-point type.
    pub const TYPE_FLOAT: u16 = 22;
    /// A function type.
    pub const TYPE_FUNCTION: u16 = 33;
    /// Begins a function.
    pub const FUNCTION: u16 = 54;
    /// Ends a function.
    pub const FUNCTION_END: u16 = 56;
    /// Begins a block.
    pub const LABEL: u16 = 248;
    /// Returns from a function with no value.
    pub const RETURN: u16 = 253;
    /// Unconditional branch.
    pub const BRANCH: u16 = 249;
    /// Branch on a boolean.
    pub const BRANCH_CONDITIONAL: u16 = 250;
    /// Multi-way branch on an integer.
    pub const SWITCH: u16 = 251;
    /// Declares the merge and continue targets of a loop.
    ///
    /// Must be the second-to-last instruction in its block, immediately before the
    /// branch. That ordering is what makes a backward branch a *loop* rather than
    /// something the validator rejects.
    pub const LOOP_MERGE: u16 = 246;
    /// Declares where a selection converges.
    pub const SELECTION_MERGE: u16 = 247;
    /// Integer equality, producing a boolean.
    pub const IEQUAL: u16 = 170;
    /// A constant.
    pub const CONSTANT: u16 = 43;
    /// A fixed-length array type.
    pub const TYPE_ARRAY: u16 = 28;
    /// A structure type.
    pub const TYPE_STRUCT: u16 = 30;
    /// A pointer type.
    pub const TYPE_POINTER: u16 = 32;
    /// A variable.
    pub const VARIABLE: u16 = 59;
    /// Computes a pointer into a composite.
    pub const ACCESS_CHAIN: u16 = 65;
    /// Writes through a pointer.
    pub const STORE: u16 = 62;
    /// Annotates a result.
    pub const DECORATE: u16 = 71;
    /// Annotates a structure member.
    pub const MEMBER_DECORATE: u16 = 72;
    /// Reads through a pointer.
    pub const LOAD: u16 = 61;
    /// Reinterprets a value's bits as another type of the same width.
    ///
    /// **Not a conversion.** A register holds thirty-two bits and the instruction
    /// decides how to read them, so translating float arithmetic means bitcasting -
    /// converting would take the bit pattern of 1.0 and produce the float 1065353216.0.
    pub const BITCAST: u16 = 124;
    /// Integer addition, for address arithmetic.
    pub const IADD: u16 = 128;
    /// Integer subtraction.
    pub const ISUB: u16 = 130;
    /// Integer multiplication.
    pub const IMUL: u16 = 132;
    /// Floating-point addition.
    pub const FADD: u16 = 129;
    /// Floating-point subtraction.
    pub const FSUB: u16 = 131;
    /// Floating-point multiplication.
    pub const FMUL: u16 = 133;
    /// Floating-point division.
    pub const FDIV: u16 = 136;
    /// Chooses between two values without branching.
    ///
    /// How a masked write is expressed when the alternative would be a merge block per
    /// lane: keep the old value where the mask says the lane is inactive.
    pub const SELECT: u16 = 169;
    /// Unsigned right shift.
    pub const SHIFT_RIGHT_LOGICAL: u16 = 194;
    /// Left shift.
    pub const SHIFT_LEFT_LOGICAL: u16 = 196;
    /// Unsigned integer less-than.
    pub const ULESS_THAN: u16 = 176;
    /// Unsigned integer greater-than.
    pub const UGREATER_THAN: u16 = 172;
    /// Unsigned integer greater-than-or-equal.
    ///
    /// Unsigned throughout the buffer bounds checks: a record count and a byte offset are
    /// both magnitudes, and a signed comparison would call an offset above two billion
    /// negative and therefore in range.
    pub const UGREATER_THAN_EQUAL: u16 = 174;
    /// Signed integer less-than.
    ///
    /// Distinct from the unsigned form for a reason that is invisible in most tests: the
    /// two agree on every pair of non-negative values and disagree on every pair where
    /// one is negative.
    pub const SLESS_THAN: u16 = 177;
    /// Signed integer greater-than.
    pub const SGREATER_THAN: u16 = 173;
    /// Signed integer less-than-or-equal.
    pub const SLESS_THAN_EQUAL: u16 = 179;
    /// Signed integer greater-than-or-equal.
    pub const SGREATER_THAN_EQUAL: u16 = 175;
    /// Bitwise and.
    pub const BITWISE_AND: u16 = 199;
    /// Bitwise or.
    pub const BITWISE_OR: u16 = 197;
    /// Bitwise exclusive or.
    pub const BITWISE_XOR: u16 = 198;
    /// Logical or, on booleans rather than on bits.
    pub const LOGICAL_OR: u16 = 166;
    /// Bitwise complement.
    pub const NOT: u16 = 200;
    /// Counts the set bits of an integer.
    pub const BIT_COUNT: u16 = 205;
    /// Integer inequality, producing a boolean.
    pub const INOT_EQUAL: u16 = 171;
    /// Ordered float equality.
    ///
    /// *Ordered* means the result is false when either operand is a NaN, which is what
    /// the guest's comparison does. The unordered forms answer true instead, and the
    /// difference is invisible until a shader produces a NaN - at which point every
    /// branch taken on it inverts.
    pub const FORD_EQUAL: u16 = 180;
    /// Ordered float less-than.
    pub const FORD_LESS_THAN: u16 = 184;
    /// Ordered float greater-than.
    pub const FORD_GREATER_THAN: u16 = 186;
    /// Whether a float is a NaN.
    ///
    /// Preferred over comparing a value with itself. The self-comparison trick is exact
    /// but relies on the comparison *not* being folded away, and a compiler entitled to
    /// assume no NaNs is entitled to fold it. Asking the question directly cannot be
    /// optimised into the wrong answer.
    pub const IS_NAN: u16 = 156;
    /// Whether a float is an infinity, of either sign.
    pub const IS_INF: u16 = 157;
    /// Logical and, on booleans rather than on bits.
    pub const LOGICAL_AND: u16 = 167;
    /// Logical negation of a boolean.
    pub const LOGICAL_NOT: u16 = 168;
    /// Ordered float less-than-or-equal.
    pub const FORD_LESS_THAN_EQUAL: u16 = 188;
    /// Ordered float greater-than-or-equal.
    pub const FORD_GREATER_THAN_EQUAL: u16 = 190;
    /// A composite whose every element is zero.
    ///
    /// Needed for a private variable's initialiser: without one its contents are
    /// undefined at entry, and a test asserting an untouched register reads zero would
    /// be asserting on whatever the driver happened to leave there.
    pub const CONSTANT_NULL: u16 = 46;
    /// The boolean `true`.
    pub const CONSTANT_TRUE: u16 = 41;
}

/// The `SPV_KHR_float_controls` extension, by name.
///
/// Requesting it is how a module states that its arithmetic depends on subnormals being
/// preserved rather than flushed to zero. Without it an implementation may flush, and a
/// shader whose correctness depends on the difference has no way to say so.
pub const FLOAT_CONTROLS: &str = "SPV_KHR_float_controls";

/// Storage classes.
pub mod storage {
    /// A buffer the shader may read and write. Needs version 1.3 or later.
    pub const STORAGE_BUFFER: u32 = 12;
    /// Read-only, supplied by the pipeline rather than by the shader.
    ///
    /// Built-in variables live here. At this version an input variable must also be
    /// listed in the entry point's interface, which is easy to forget and produces a
    /// module that is rejected rather than one that misbehaves.
    pub const INPUT: u32 = 1;
}

/// Decorations.
pub mod decoration {
    /// Marks a structure as a shader interface block.
    pub const BLOCK: u32 = 2;
    /// Bytes between consecutive array elements.
    pub const ARRAY_STRIDE: u32 = 6;
    /// Which binding within a descriptor set.
    pub const BINDING: u32 = 33;
    /// Marks a variable as one the implementation fills in.
    pub const BUILT_IN: u32 = 11;
    /// Which descriptor set.
    pub const DESCRIPTOR_SET: u32 = 34;
    /// Byte offset of a structure member.
    pub const OFFSET: u32 = 35;
}

/// Capability values.
pub mod capability {
    /// Shader stages. The baseline for anything graphics or compute.
    pub const SHADER: u32 = 1;
    /// Permits a module to require that subnormal results are kept, not flushed.
    ///
    /// From `SPV_KHR_float_controls`. A device that does not offer it cannot run a
    /// module that declares it, which is the point: refusing to load beats loading and
    /// silently computing zero where the guest expected a subnormal.
    pub const DENORM_PRESERVE: u32 = 4464;
    /// Invocations may ask about their subgroup at all.
    pub const GROUP_NON_UNIFORM: u32 = 61;
    /// Invocations may take a ballot across their subgroup.
    pub const GROUP_NON_UNIFORM_BALLOT: u32 = 64;
}

/// Execution scopes, for the group operations.
pub mod scope {
    /// The subgroup: the invocations the hardware runs in lockstep.
    ///
    /// A literal *identifier* in the encoding rather than a literal number, so it has to
    /// be declared as a constant like any other value - which is easy to get wrong,
    /// because it reads like a flag.
    pub const SUBGROUP: u32 = 3;
}

/// Built-in variables, by their decoration value.
pub mod built_in {
    /// This invocation's index within its subgroup.
    ///
    /// The guest's lane number, when one invocation is one lane.
    pub const SUBGROUP_LOCAL_INVOCATION_ID: u32 = 41;
}

/// Addressing models.
pub mod addressing {
    /// No physical addressing. What a shader uses.
    pub const LOGICAL: u32 = 0;
}

/// Memory models.
pub mod memory {
    /// The model shaders are written against.
    pub const GLSL450: u32 = 1;
}

/// Execution models.
pub mod execution {
    /// A compute shader.
    pub const GL_COMPUTE: u32 = 5;
    /// A fragment shader.
    pub const FRAGMENT: u32 = 4;
}

/// Execution modes.
pub mod mode {
    /// Fragment shaders declare their origin convention.
    pub const ORIGIN_UPPER_LEFT: u32 = 7;
    /// Compute shaders declare their workgroup size.
    pub const LOCAL_SIZE: u32 = 17;
    /// Subnormal results of the given width are preserved rather than flushed.
    ///
    /// Takes the bit width as its one literal operand, so a module can ask for it at
    /// 32 bits without committing to 16 or 64.
    pub const DENORM_PRESERVE: u32 = 4459;
}

/// How many ordered slots the header is kept in.
const HEADER_SLOTS: usize = 5;

/// Which slot a header instruction belongs in.
///
/// # Why the header is ordered by opcode and not by call order
///
/// D102 gave the builder sections so that correctness stopped being a property of the
/// order calls happen to be written in. It solved that *between* sections and left it
/// inside them, and the header is the section where that matters: the format requires
/// every capability before the memory model, which must precede the entry point, which
/// must precede the execution modes.
///
/// It went wrong exactly as the original fault did. A module that needed two extra
/// capabilities declared them from the code that needed them, which runs after the entry
/// point is written - so they were emitted after it. The driver accepted the module, which
/// is worse than rejecting it: the layout was wrong and nothing said so.
///
/// So the opcode decides the slot, and a capability declared last is still emitted first.
/// The same principle D102 states, applied one level down.
const fn header_slot(opcode: u16) -> usize {
    match opcode {
        op::CAPABILITY => 0,
        op::EXTENSION => 1,
        op::MEMORY_MODEL => 2,
        op::ENTRY_POINT => 3,
        // Execution modes, and anything else that belongs after the entry point.
        _ => 4,
    }
}

/// Assembles a module word by word.
#[derive(Debug, Clone)]
pub struct Builder {
    /// Capabilities, the memory model, entry points and execution modes.
    header: [Vec<u32>; HEADER_SLOTS],
    /// Decorations.
    annotations: Vec<u32>,
    /// Types, constants and global variables, in dependency order.
    declarations: Vec<u32>,
    /// Function bodies.
    functions: Vec<u32>,
    next_id: u32,
    version: u32,
}

impl Default for Builder {
    fn default() -> Self {
        Self::new()
    }
}

impl Builder {
    /// Creates an empty builder.
    pub const fn new() -> Self {
        Self {
            // Each slot empty; `header` places an instruction by its opcode.
            header: [const { Vec::new() }; HEADER_SLOTS],
            annotations: Vec::new(),
            declarations: Vec::new(),
            functions: Vec::new(),
            // Identifier zero is reserved by the format, so allocation starts at one.
            next_id: 1,
            version: VERSION_1_0,
        }
    }

    /// Declares a later version.
    ///
    /// Raised only when something in the module needs it. A version higher than the
    /// module requires narrows what will accept it and buys nothing.
    #[must_use]
    pub const fn with_version(mut self, version: u32) -> Self {
        self.version = version;
        self
    }

    /// Reserves a fresh identifier.
    pub const fn id(&mut self) -> Id {
        let id = Id(self.next_id);
        self.next_id += 1;
        id
    }

    /// Appends a capability, memory model, entry point or execution mode.
    pub fn header(&mut self, opcode: u16, operands: &[u32]) {
        encode(&mut self.header[header_slot(opcode)], opcode, operands);
    }

    /// Appends a decoration.
    ///
    /// Its own section because the format requires **every** decoration to precede
    /// **every** type, and a builder with one undifferentiated preamble makes that a
    /// property of the order calls happen to be written in rather than a property of
    /// the builder. It was got wrong the first time a second buffer was declared, and
    /// the validator's answer - "Decorate is in an invalid layout section" - names the
    /// symptom rather than the cause.
    pub fn annotate(&mut self, opcode: u16, operands: &[u32]) {
        encode(&mut self.annotations, opcode, operands);
    }

    /// Appends a type, constant or global variable.
    ///
    /// Order within this section is preserved, because a type may name one declared
    /// before it.
    pub fn declare(&mut self, opcode: u16, operands: &[u32]) {
        encode(&mut self.declarations, opcode, operands);
    }

    /// Appends an instruction to the function section.
    pub fn function(&mut self, opcode: u16, operands: &[u32]) {
        encode(&mut self.functions, opcode, operands);
    }

    /// Encodes a string as the format does: NUL-terminated, packed four bytes to a
    /// word, little-endian, and always with at least one terminating zero.
    ///
    /// The padding rule is the part that is easy to get wrong: a string whose length is
    /// an exact multiple of four still needs a whole extra word of zeros, because
    /// without it there is no terminator.
    pub fn literal_string(text: &str) -> Vec<u32> {
        let mut bytes = text.as_bytes().to_vec();
        bytes.push(0);
        while bytes.len() % 4 != 0 {
            bytes.push(0);
        }
        bytes
            .chunks_exact(4)
            .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect()
    }

    /// The finished module.
    pub fn finish(&self) -> Vec<u32> {
        let mut words = Vec::with_capacity(
            5 + self.header.len()
                + self.annotations.len()
                + self.declarations.len()
                + self.functions.len(),
        );
        words.push(MAGIC);
        words.push(self.version);
        words.push(GENERATOR);
        // The bound must exceed every identifier in use. Allocating them here is what
        // makes that true by construction rather than by arithmetic somebody has to
        // keep right.
        words.push(self.next_id);
        words.push(0); // schema, reserved
        // The order the format requires. Enforced here rather than by callers, so that
        // declaring something new cannot put a decoration in the wrong place - and, within
        // the header, so that declaring a capability late cannot put it after the entry
        // point.
        for slot in &self.header {
            words.extend_from_slice(slot);
        }
        words.extend_from_slice(&self.annotations);
        words.extend_from_slice(&self.declarations);
        words.extend_from_slice(&self.functions);
        words
    }

    /// Checks the identifiers in the module refer to something.
    ///
    /// # Why this exists
    ///
    /// An identifier used but never defined is not a malformed *instruction* - every
    /// word is well-formed, the length is right, the opcode is real. It is a module
    /// that reads perfectly and means nothing, and a driver handed one does not
    /// diagnose it. It faults. Twice now that fault has presented as
    /// `STATUS_ACCESS_VIOLATION` inside the graphics driver with no indication of
    /// which identifier or which instruction was at fault, and both times the answer
    /// came from `spirv-val` in a virtual machine rather than from anything here.
    ///
    /// The builder hands out every identifier, so it is the one place that can say
    /// which were never given a meaning. Doing it here turns a driver crash into a
    /// named error, which is the same trade [`finish`](Self::finish) already makes for
    /// the identifier bound.
    ///
    /// # What it does not do
    ///
    /// It is not a validator and must not grow into one - `spirv-val` exists, is
    /// authoritative, and disagreeing with it would be worse than silence. This checks
    /// three properties a builder is uniquely placed to check, and nothing else.
    pub fn check(&self) -> Result<(), ModuleError> {
        let header: Vec<u32> = self.header.concat();
        let sections = [
            (header.as_slice(), true),
            (self.annotations.as_slice(), true),
            (self.declarations.as_slice(), false),
            // Function bodies forward-reference by necessity: a branch names a label
            // that appears later, and a loop header names its own merge block before
            // either exists. Only the "defined somewhere" half of the check applies
            // here - which is the half that caught the real bug, an identifier reserved
            // and never given a meaning at all.
            (self.functions.as_slice(), true),
        ];

        // Every opcode first. An unknown one makes the rest of this check produce
        // confident nonsense, so it is reported rather than skipped.
        for (words, _) in sections {
            for (opcode, _) in Instructions::new(words) {
                if Shape::of(opcode).is_none() {
                    return Err(ModuleError::UnknownOpcode { opcode });
                }
            }
        }

        let mut defined: Vec<bool> = vec![false; self.next_id as usize];
        for (words, _) in sections {
            for (opcode, operands) in Instructions::new(words) {
                let Some(shape) = Shape::of(opcode) else {
                    continue;
                };
                if let Some(index) = shape.result
                    && let Some(&id) = operands.get(index)
                {
                    let slot = defined
                        .get_mut(id as usize)
                        .ok_or(ModuleError::IdAboveBound { id, opcode })?;
                    if *slot {
                        return Err(ModuleError::DefinedTwice { id, opcode });
                    }
                    *slot = true;
                }
            }
        }

        // Forward references are legal in the header and the annotations - an entry
        // point names a function declared later, and a decoration names the variable it
        // decorates. In the declarations and the function bodies they are not, so those
        // two sections are checked in order as well as for existence.
        let mut seen: Vec<bool> = vec![false; self.next_id as usize];
        for (words, may_forward_reference) in sections {
            for (opcode, operands) in Instructions::new(words) {
                let Some(shape) = Shape::of(opcode) else {
                    continue;
                };
                for id in shape.uses(operands) {
                    if !defined.get(id as usize).copied().unwrap_or(false) {
                        return Err(ModuleError::Undefined { id, opcode });
                    }
                    if !may_forward_reference && !seen[id as usize] {
                        return Err(ModuleError::UsedBeforeDefined { id, opcode });
                    }
                }
                if let Some(index) = shape.result
                    && let Some(&id) = operands.get(index)
                {
                    seen[id as usize] = true;
                }
            }
        }

        Ok(())
    }

    /// The finished module as bytes, for writing to a file.
    pub fn finish_bytes(&self) -> Vec<u8> {
        self.finish().iter().flat_map(|w| w.to_le_bytes()).collect()
    }

    /// How many identifiers have been handed out.
    pub const fn id_count(&self) -> u32 {
        self.next_id - 1
    }
}

/// Packs an instruction: one word of length and opcode, then operands.
fn encode(into: &mut Vec<u32>, opcode: u16, operands: &[u32]) {
    let length = u32::try_from(operands.len() + 1).unwrap_or(u32::MAX);
    into.push((length << 16) | u32::from(opcode));
    into.extend_from_slice(operands);
}

/// Builds the smallest module that validates: a compute entry point that returns.
///
/// Exists so the emitter can be exercised end to end before anything is translated. If
/// this does not validate, nothing built on top of it will, and the fault is here
/// rather than in whatever was being translated - which is a distinction worth being
/// able to make cheaply.
pub fn minimal_compute_module(workgroup: [u32; 3]) -> Vec<u32> {
    let mut b = Builder::new();

    let void = b.id();
    let fn_type = b.id();
    let main = b.id();
    let entry_block = b.id();

    b.header(op::CAPABILITY, &[capability::SHADER]);
    b.header(op::MEMORY_MODEL, &[addressing::LOGICAL, memory::GLSL450]);

    let mut entry = vec![execution::GL_COMPUTE, main.0];
    entry.extend(Builder::literal_string("main"));
    b.header(op::ENTRY_POINT, &entry);

    b.header(
        op::EXECUTION_MODE,
        &[
            main.0,
            mode::LOCAL_SIZE,
            workgroup[0],
            workgroup[1],
            workgroup[2],
        ],
    );

    b.declare(op::TYPE_VOID, &[void.0]);
    b.declare(op::TYPE_FUNCTION, &[fn_type.0, void.0]);

    // Function control `None` is zero.
    b.function(op::FUNCTION, &[void.0, main.0, 0, fn_type.0]);
    b.function(op::LABEL, &[entry_block.0]);
    b.function(op::RETURN, &[]);
    b.function(op::FUNCTION_END, &[]);

    b.check()
        .expect("this crate built a module with identifiers that do not resolve");
    b.finish()
}

/// Builds a compute shader that writes one known value into a storage buffer.
///
/// The point of it is to prove a *runner*, not to be useful. A dispatch harness that
/// has never executed a shader whose answer is known cannot be trusted with one whose
/// answer is not - so this is the first thing it runs, and if the value does not come
/// back the fault is in the harness rather than in anything translated.
///
/// One invocation, one write. Indexing by invocation identifier would need builtin
/// inputs and prove nothing extra about the plumbing.
pub fn storage_buffer_write_module(value: u32, elements: u32) -> Vec<u32> {
    let mut b = Builder::new().with_version(VERSION_1_3);

    let void = b.id();
    let fn_type = b.id();
    let u32_type = b.id();
    let array = b.id();
    let block = b.id();
    let block_ptr = b.id();
    let element_ptr = b.id();
    let buffer = b.id();
    let count = b.id();
    let index = b.id();
    let written = b.id();
    let main = b.id();
    let entry_block = b.id();
    let chain = b.id();

    b.header(op::CAPABILITY, &[capability::SHADER]);
    b.header(op::MEMORY_MODEL, &[addressing::LOGICAL, memory::GLSL450]);

    let mut entry = vec![execution::GL_COMPUTE, main.0];
    entry.extend(Builder::literal_string("main"));
    b.header(op::ENTRY_POINT, &entry);
    b.header(op::EXECUTION_MODE, &[main.0, mode::LOCAL_SIZE, 1, 1, 1]);

    // Decorations describe the memory layout a host must match. An array stride of
    // four and a member offset of zero say the buffer is tightly packed from its
    // start, which is what the runner allocates.
    b.annotate(op::DECORATE, &[array.0, decoration::ARRAY_STRIDE, 4]);
    b.annotate(op::DECORATE, &[block.0, decoration::BLOCK]);
    b.annotate(op::MEMBER_DECORATE, &[block.0, 0, decoration::OFFSET, 0]);
    b.annotate(op::DECORATE, &[buffer.0, decoration::DESCRIPTOR_SET, 0]);
    b.annotate(op::DECORATE, &[buffer.0, decoration::BINDING, 0]);

    b.declare(op::TYPE_VOID, &[void.0]);
    b.declare(op::TYPE_FUNCTION, &[fn_type.0, void.0]);
    // Width 32, signedness 0: unsigned.
    b.declare(op::TYPE_INT, &[u32_type.0, 32, 0]);
    b.declare(op::CONSTANT, &[u32_type.0, count.0, elements]);
    b.declare(op::CONSTANT, &[u32_type.0, index.0, 0]);
    b.declare(op::CONSTANT, &[u32_type.0, written.0, value]);
    b.declare(op::TYPE_ARRAY, &[array.0, u32_type.0, count.0]);
    b.declare(op::TYPE_STRUCT, &[block.0, array.0]);
    b.declare(
        op::TYPE_POINTER,
        &[block_ptr.0, storage::STORAGE_BUFFER, block.0],
    );
    b.declare(
        op::TYPE_POINTER,
        &[element_ptr.0, storage::STORAGE_BUFFER, u32_type.0],
    );
    b.declare(
        op::VARIABLE,
        &[block_ptr.0, buffer.0, storage::STORAGE_BUFFER],
    );

    b.function(op::FUNCTION, &[void.0, main.0, 0, fn_type.0]);
    b.function(op::LABEL, &[entry_block.0]);
    // Two indices: member zero of the block, then element zero of the array.
    b.function(
        op::ACCESS_CHAIN,
        &[element_ptr.0, chain.0, buffer.0, index.0, index.0],
    );
    b.function(op::STORE, &[chain.0, written.0]);
    b.function(op::RETURN, &[]);
    b.function(op::FUNCTION_END, &[]);

    b.check()
        .expect("this crate built a module with identifiers that do not resolve");
    b.finish()
}

/// Why a module's identifiers do not hang together.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModuleError {
    /// An opcode was emitted that the shape table does not describe.
    ///
    /// Checked before anything else, because it makes every other answer unreliable: an
    /// instruction whose shape is unknown is skipped, so the identifier it defines is
    /// never recorded, and the next instruction to use that identifier is reported as
    /// referring to nothing. That is a **false** failure naming the wrong instruction,
    /// and it cost a confusing detour the first time a new opcode was added without its
    /// row.
    UnknownOpcode {
        /// The opcode.
        opcode: u16,
    },
    /// An identifier is used but nothing defines it.
    Undefined {
        /// The identifier.
        id: u32,
        /// The opcode that used it.
        opcode: u16,
    },
    /// An identifier is used before the instruction that defines it.
    UsedBeforeDefined {
        /// The identifier.
        id: u32,
        /// The opcode that used it.
        opcode: u16,
    },
    /// Two instructions claim the same result identifier.
    DefinedTwice {
        /// The identifier.
        id: u32,
        /// The opcode that redefined it.
        opcode: u16,
    },
    /// A result identifier is at or above the declared bound.
    IdAboveBound {
        /// The identifier.
        id: u32,
        /// The opcode that produced it.
        opcode: u16,
    },
}

impl fmt::Display for ModuleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::UnknownOpcode { opcode } => write!(
                f,
                "opcode {opcode} has no row in the shape table, so this module cannot be                  checked - add one rather than trusting the result"
            ),
            Self::Undefined { id, opcode } => write!(
                f,
                "%{id} is used by opcode {opcode} but nothing defines it - a driver \
                 handed this module faults rather than diagnosing it"
            ),
            Self::UsedBeforeDefined { id, opcode } => write!(
                f,
                "%{id} is used by opcode {opcode} before the instruction defining it"
            ),
            Self::DefinedTwice { id, opcode } => {
                write!(
                    f,
                    "%{id} is defined twice, the second time by opcode {opcode}"
                )
            }
            Self::IdAboveBound { id, opcode } => write!(
                f,
                "%{id}, from opcode {opcode}, is at or above the declared bound"
            ),
        }
    }
}

impl core::error::Error for ModuleError {}

/// Walks a section, yielding each instruction's opcode and operand words.
struct Instructions<'a> {
    words: &'a [u32],
    at: usize,
}

impl<'a> Instructions<'a> {
    const fn new(words: &'a [u32]) -> Self {
        Self { words, at: 0 }
    }
}

impl<'a> Iterator for Instructions<'a> {
    type Item = (u16, &'a [u32]);

    fn next(&mut self) -> Option<Self::Item> {
        let head = *self.words.get(self.at)?;
        let length = (head >> 16) as usize;
        // A zero length would not advance, so the walk would never end. It cannot
        // happen - `encode` always writes at least the head word - but a loop that
        // hangs on malformed input is a worse failure than one that stops.
        let length = length.max(1);
        let end = (self.at + length).min(self.words.len());
        let operands = &self.words[self.at + 1..end];
        self.at = end;
        Some(((head & 0xFFFF) as u16, operands))
    }
}

/// Where an instruction keeps its result identifier and which operands name others.
///
/// Data rather than a match arm per opcode, and deliberately minimal: only the opcodes
/// this crate emits appear, and an opcode absent from the table is skipped rather than
/// guessed at. Guessing would mean treating a literal as an identifier, which produces
/// a confident complaint about a module that is fine - worse than not checking.
struct Shape {
    /// Operand index holding the result identifier.
    result: Option<usize>,
    /// Operand indices naming identifiers that must already exist.
    fixed: &'static [usize],
    /// Operand index from which the remaining words are identifiers.
    rest: Option<usize>,
    /// Whether every remaining word is an identifier, or every other one.
    stride: RestStride,
}

impl Shape {
    /// The identifiers an instruction uses, given its operands.
    fn uses(&self, operands: &[u32]) -> Vec<u32> {
        let mut ids: Vec<u32> = self
            .fixed
            .iter()
            .filter_map(|&i| operands.get(i).copied())
            .collect();
        if let Some(from) = self.rest
            && from < operands.len()
        {
            let step = match self.stride {
                RestStride::Every => 1,
                RestStride::Alternating => 2,
            };
            ids.extend(operands[from..].iter().step_by(step).copied());
        }
        ids
    }

    /// The shape of an opcode, if this crate knows it.
    fn of(opcode: u16) -> Option<Self> {
        SHAPES
            .iter()
            .find(|entry| entry.0 == opcode)
            .map(|&(_, result, fixed, rest, stride)| Self {
                result,
                fixed,
                rest,
                stride,
            })
    }
}

/// One row of [`SHAPES`].
type ShapeEntry = (
    u16,
    Option<usize>,
    &'static [usize],
    Option<usize>,
    RestStride,
);

/// How to walk the operands from a shape's `rest` index.
///
/// `Every` is the usual case. `Alternating` exists for `OpSwitch`, whose tail is
/// literal-then-label repeated - reading it as all identifiers would report the case
/// values as undefined identifiers and reject a module that is fine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RestStride {
    Every,
    Alternating,
}

/// Where each opcode keeps its identifiers.
///
/// `(opcode, result operand index, operand indices naming existing identifiers, index
/// from which every remaining operand is an identifier)`.
///
/// A table rather than a match, because that is what it is: the same four facts about
/// each opcode, with nothing computed. An opcode absent from it is skipped rather than
/// guessed at - guessing would mean reading a literal as an identifier and complaining
/// confidently about a module that is fine, which is worse than not checking.
static SHAPES: &[ShapeEntry] = &[
    // Types declare their identifier first. The trailing words of an integer or float
    // type are widths, so `rest` is not simply always set.
    (op::TYPE_VOID, Some(0), &[], None, RestStride::Every),
    (op::TYPE_BOOL, Some(0), &[], None, RestStride::Every),
    (op::TYPE_INT, Some(0), &[], None, RestStride::Every),
    // Result id, then the component type and a literal count.
    // The component type is an identifier; the count is a literal. The result is
    // named by `Some(0)` and must not also be listed as a use of itself.
    (op::TYPE_VECTOR, Some(0), &[1], None, RestStride::Every),
    (op::CONSTANT_TRUE, Some(1), &[0], None, RestStride::Every),
    // Result type, result, composite, then literal indices.
    (
        op::COMPOSITE_EXTRACT,
        Some(1),
        &[0, 2],
        None,
        RestStride::Every,
    ),
    // Result type, result, an execution-scope identifier, and the predicate.
    (
        op::GROUP_NON_UNIFORM_BALLOT,
        Some(1),
        &[0, 2, 3],
        None,
        RestStride::Every,
    ),
    (op::TYPE_FLOAT, Some(0), &[], None, RestStride::Every),
    // Return type then parameter types, all identifiers.
    (op::TYPE_FUNCTION, Some(0), &[], Some(1), RestStride::Every),
    // Element type and a constant holding the length.
    (op::TYPE_ARRAY, Some(0), &[1, 2], None, RestStride::Every),
    (op::TYPE_STRUCT, Some(0), &[], Some(1), RestStride::Every),
    // Operand one is a storage class literal, so only operand two is named.
    (op::TYPE_POINTER, Some(0), &[2], None, RestStride::Every),
    (op::LABEL, Some(0), &[], None, RestStride::Every),
    // Typed results: operand zero is the type, operand one the identifier.
    // Operand two of a constant is its literal value.
    (op::CONSTANT, Some(1), &[0], None, RestStride::Every),
    (op::CONSTANT_NULL, Some(1), &[0], None, RestStride::Every),
    // Operand two is the storage class; an initialiser, if present, follows it.
    (op::VARIABLE, Some(1), &[0], None, RestStride::Every),
    // Operand two is a function control mask, operand three the function type.
    (op::FUNCTION, Some(1), &[0, 3], None, RestStride::Every),
    // Every index after the base is an identifier rather than a literal - the detail
    // that made a two-index access chain look like a one-index one, and cost a driver
    // fault to find.
    (op::ACCESS_CHAIN, Some(1), &[0], Some(2), RestStride::Every),
    (op::LOAD, Some(1), &[0, 2], None, RestStride::Every),
    (op::BITCAST, Some(1), &[0, 2], None, RestStride::Every),
    (op::IADD, Some(1), &[0, 2, 3], None, RestStride::Every),
    (op::ISUB, Some(1), &[0, 2, 3], None, RestStride::Every),
    (op::IMUL, Some(1), &[0, 2, 3], None, RestStride::Every),
    (op::FADD, Some(1), &[0, 2, 3], None, RestStride::Every),
    (op::FSUB, Some(1), &[0, 2, 3], None, RestStride::Every),
    (op::FMUL, Some(1), &[0, 2, 3], None, RestStride::Every),
    (op::FDIV, Some(1), &[0, 2, 3], None, RestStride::Every),
    (
        op::SHIFT_RIGHT_LOGICAL,
        Some(1),
        &[0, 2, 3],
        None,
        RestStride::Every,
    ),
    (
        op::SHIFT_LEFT_LOGICAL,
        Some(1),
        &[0, 2, 3],
        None,
        RestStride::Every,
    ),
    (op::ULESS_THAN, Some(1), &[0, 2, 3], None, RestStride::Every),
    (op::SLESS_THAN, Some(1), &[0, 2, 3], None, RestStride::Every),
    (
        op::SGREATER_THAN,
        Some(1),
        &[0, 2, 3],
        None,
        RestStride::Every,
    ),
    (
        op::SLESS_THAN_EQUAL,
        Some(1),
        &[0, 2, 3],
        None,
        RestStride::Every,
    ),
    (
        op::SGREATER_THAN_EQUAL,
        Some(1),
        &[0, 2, 3],
        None,
        RestStride::Every,
    ),
    (
        op::BITWISE_AND,
        Some(1),
        &[0, 2, 3],
        None,
        RestStride::Every,
    ),
    (op::BITWISE_OR, Some(1), &[0, 2, 3], None, RestStride::Every),
    (
        op::BITWISE_XOR,
        Some(1),
        &[0, 2, 3],
        None,
        RestStride::Every,
    ),
    (op::LOGICAL_OR, Some(1), &[0, 2, 3], None, RestStride::Every),
    (op::NOT, Some(1), &[0, 2], None, RestStride::Every),
    (op::BIT_COUNT, Some(1), &[0, 2], None, RestStride::Every),
    (op::INOT_EQUAL, Some(1), &[0, 2, 3], None, RestStride::Every),
    (op::FORD_EQUAL, Some(1), &[0, 2, 3], None, RestStride::Every),
    (
        op::FORD_LESS_THAN,
        Some(1),
        &[0, 2, 3],
        None,
        RestStride::Every,
    ),
    (
        op::FORD_GREATER_THAN,
        Some(1),
        &[0, 2, 3],
        None,
        RestStride::Every,
    ),
    (op::SELECT, Some(1), &[0, 2, 3, 4], None, RestStride::Every),
    // Effects and annotations, which produce nothing.
    (op::STORE, None, &[0, 1], None, RestStride::Every),
    (op::DECORATE, None, &[0], None, RestStride::Every),
    (op::MEMBER_DECORATE, None, &[0], None, RestStride::Every),
    // Operand zero is the execution model; what follows the function is a literal
    // string, so nothing beyond operand one is read.
    (op::ENTRY_POINT, None, &[1], None, RestStride::Every),
    (op::EXECUTION_MODE, None, &[0], None, RestStride::Every),
    (op::CAPABILITY, None, &[], None, RestStride::Every),
    // A literal string, so no identifiers at all.
    (op::EXTENSION, None, &[], None, RestStride::Every),
    (op::MEMORY_MODEL, None, &[], None, RestStride::Every),
    (op::FUNCTION_END, None, &[], None, RestStride::Every),
    (op::RETURN, None, &[], None, RestStride::Every),
    // Control flow. Every operand of these is a label except the control masks and the
    // switch's case values.
    (op::BRANCH, None, &[0], None, RestStride::Every),
    (
        op::BRANCH_CONDITIONAL,
        None,
        &[0, 1, 2],
        None,
        RestStride::Every,
    ),
    // Operand two is a loop-control literal.
    (op::LOOP_MERGE, None, &[0, 1], None, RestStride::Every),
    // Operand one is a selection-control literal.
    (op::SELECTION_MERGE, None, &[0], None, RestStride::Every),
    // Selector, default label, then (literal, label) repeated - so the tail starts at
    // operand three and takes every *other* word.
    (op::SWITCH, None, &[0, 1], Some(3), RestStride::Alternating),
    (op::IEQUAL, Some(1), &[0, 2, 3], None, RestStride::Every),
    (
        op::UGREATER_THAN,
        Some(1),
        &[0, 2, 3],
        None,
        RestStride::Every,
    ),
    (
        op::UGREATER_THAN_EQUAL,
        Some(1),
        &[0, 2, 3],
        None,
        RestStride::Every,
    ),
    // Unary predicates: result type, result, one operand.
    (op::IS_NAN, Some(1), &[0, 2], None, RestStride::Every),
    (op::IS_INF, Some(1), &[0, 2], None, RestStride::Every),
    (op::LOGICAL_NOT, Some(1), &[0, 2], None, RestStride::Every),
    (
        op::LOGICAL_AND,
        Some(1),
        &[0, 2, 3],
        None,
        RestStride::Every,
    ),
    (
        op::FORD_LESS_THAN_EQUAL,
        Some(1),
        &[0, 2, 3],
        None,
        RestStride::Every,
    ),
    (
        op::FORD_GREATER_THAN_EQUAL,
        Some(1),
        &[0, 2, 3],
        None,
        RestStride::Every,
    ),
];

#[cfg(test)]
mod tests {
    use super::{
        Builder, Id, MAGIC, ModuleError, VERSION_1_0, decoration, minimal_compute_module, op,
        storage,
    };

    #[test]
    fn a_module_begins_with_the_magic_word_and_a_bound() {
        let words = minimal_compute_module([64, 1, 1]);
        assert_eq!(words[0], MAGIC);
        assert_eq!(words[1], VERSION_1_0);
        assert!(words.len() > 5, "a header alone is not a module");
    }

    #[test]
    fn the_bound_exceeds_every_identifier_handed_out() {
        // A bound at or below an identifier in use makes the module malformed for a
        // reason invisible to anyone reading it. Allocating identifiers through the
        // builder is what makes this hold by construction.
        let mut b = Builder::new();
        let first = b.id();
        let last = b.id();
        let words = b.finish();
        assert!(words[3] > last.0);
        assert_eq!(first.0, 1, "identifier zero is reserved by the format");
    }

    #[test]
    fn an_instruction_word_packs_its_length_and_opcode() {
        // Getting this backwards produces a module the validator rejects with an
        // opcode nobody recognises, which reads as a wrong opcode rather than a wrong
        // header.
        let mut b = Builder::new();
        b.header(op::CAPABILITY, &[1]);
        let words = b.finish();
        let instruction = words[5];
        assert_eq!(instruction >> 16, 2, "one word of header plus one operand");
        assert_eq!(instruction & 0xFFFF, u32::from(op::CAPABILITY));
    }

    #[test]
    fn a_string_is_terminated_even_when_its_length_is_a_multiple_of_four() {
        // The rule that is easy to miss: four characters do not fit in one word,
        // because the terminator still needs somewhere to go.
        assert_eq!(Builder::literal_string("main").len(), 2);
        assert_eq!(Builder::literal_string("abc").len(), 1);
        let words = Builder::literal_string("main");
        assert_eq!(words[1], 0, "the whole trailing word is the terminator");
    }

    #[test]
    fn a_string_packs_four_bytes_to_a_word_little_endian() {
        let words = Builder::literal_string("abc");
        assert_eq!(words[0], u32::from_le_bytes([b'a', b'b', b'c', 0]));
    }

    #[test]
    fn bytes_and_words_describe_the_same_module() {
        let mut b = Builder::new();
        b.header(op::CAPABILITY, &[1]);
        let words = b.finish();
        let bytes = b.finish_bytes();
        assert_eq!(bytes.len(), words.len() * 4);
        assert_eq!(&bytes[..4], &MAGIC.to_le_bytes());
    }

    #[test]
    fn an_array_whose_length_was_never_declared_is_named() {
        // The bug this check exists for, reduced. An identifier was reserved for the
        // array's length and the `OpConstant` defining it was never emitted, so the
        // array type referred to nothing. Every instruction is well-formed; the module
        // is meaningless. The driver did not say so - it faulted, twice, and the
        // diagnosis came from `spirv-val` in a virtual machine both times.
        let mut b = Builder::new();
        let u32_type = b.id();
        let length = b.id();
        let array = b.id();
        b.declare(op::TYPE_INT, &[u32_type.0, 32, 0]);
        // No `OpConstant` for `length`.
        b.declare(op::TYPE_ARRAY, &[array.0, u32_type.0, length.0]);

        assert_eq!(
            b.check(),
            Err(ModuleError::Undefined {
                id: length.0,
                opcode: op::TYPE_ARRAY
            })
        );
    }

    #[test]
    fn a_capability_declared_late_is_still_emitted_first() {
        // The format wants every capability before the memory model, which precedes the
        // entry point. D102 made *sections* independent of call order and left the inside
        // of a section dependent on it - and a module that declared two extra capabilities
        // from the code needing them emitted them after its entry point. The driver
        // accepted it, which is worse than a rejection: the layout was wrong and nothing
        // said so.
        let mut b = Builder::new();
        b.header(op::CAPABILITY, &[1]);
        b.header(op::MEMORY_MODEL, &[0, 1]);
        let main = b.id();
        let mut entry = vec![5, main.0];
        entry.extend(Builder::literal_string("main"));
        b.header(op::ENTRY_POINT, &entry);
        // Declared last, the way a feature discovered mid-translation would be.
        b.header(op::CAPABILITY, &[61]);

        let words = b.clone().finish();
        let mut opcodes = Vec::new();
        let mut at = 5;
        while at < words.len() && opcodes.len() < 4 {
            opcodes.push((words[at] & 0xFFFF) as u16);
            at += ((words[at] >> 16) as usize).max(1);
        }
        assert_eq!(
            opcodes,
            vec![
                op::CAPABILITY,
                op::CAPABILITY,
                op::MEMORY_MODEL,
                op::ENTRY_POINT
            ],
            "a capability declared after the entry point still belongs before it"
        );
    }

    #[test]
    fn a_function_body_may_name_a_label_that_appears_later() {
        // The exemption D111 records, and the reason it had to exist. A forward branch
        // names a block that does not exist yet, and a loop header names its own merge
        // before either block is written. Both are legal and unavoidable, so the ordering
        // half of the check cannot apply inside a function.
        let mut b = Builder::new();
        let void = b.id();
        let fn_type = b.id();
        let main = b.id();
        let entry = b.id();
        let later = b.id();
        b.declare(op::TYPE_VOID, &[void.0]);
        b.declare(op::TYPE_FUNCTION, &[fn_type.0, void.0]);

        b.function(op::FUNCTION, &[void.0, main.0, 0, fn_type.0]);
        b.function(op::LABEL, &[entry.0]);
        // Branch to a block written afterwards.
        b.function(op::BRANCH, &[later.0]);
        b.function(op::LABEL, &[later.0]);
        b.function(op::RETURN, &[]);
        b.function(op::FUNCTION_END, &[]);

        assert_eq!(
            b.check(),
            Ok(()),
            "a forward branch is ordinary control flow"
        );
    }

    #[test]
    fn a_function_body_may_not_name_something_defined_nowhere() {
        // The half D111 *kept*, in the section where it was narrowed. Relaxing the
        // ordering rule inside a function is forced; relaxing it into "anything goes"
        // would give up the thing that caught the fault this check was written for - an
        // identifier reserved and never given a meaning, which crashed a driver.
        //
        // That fault lived in the declarations section and there are tests for it there.
        // There were none here, so what the narrowing left behind was never checked.
        let mut b = Builder::new();
        let void = b.id();
        let fn_type = b.id();
        let main = b.id();
        let entry = b.id();
        let nowhere = b.id();
        b.declare(op::TYPE_VOID, &[void.0]);
        b.declare(op::TYPE_FUNCTION, &[fn_type.0, void.0]);

        b.function(op::FUNCTION, &[void.0, main.0, 0, fn_type.0]);
        b.function(op::LABEL, &[entry.0]);
        b.function(op::BRANCH, &[nowhere.0]);
        b.function(op::FUNCTION_END, &[]);

        assert_eq!(
            b.check(),
            Err(ModuleError::Undefined {
                id: nowhere.0,
                opcode: op::BRANCH
            }),
            "a branch to a block that is never written is the fault this check exists for"
        );
    }

    #[test]
    fn a_type_used_before_it_is_declared_is_named() {
        // Legal to write, illegal to run: the declarations section is ordered, so a
        // type may only name one already declared.
        let mut b = Builder::new();
        let u32_type = b.id();
        let length = b.id();
        let array = b.id();
        b.declare(op::TYPE_INT, &[u32_type.0, 32, 0]);
        b.declare(op::TYPE_ARRAY, &[array.0, u32_type.0, length.0]);
        b.declare(op::CONSTANT, &[u32_type.0, length.0, 16]);

        assert_eq!(
            b.check(),
            Err(ModuleError::UsedBeforeDefined {
                id: length.0,
                opcode: op::TYPE_ARRAY
            })
        );
    }

    #[test]
    fn a_decoration_may_name_a_variable_declared_later() {
        // The other side of the ordering rule, and the reason it is not applied to
        // every section: a decoration necessarily precedes what it decorates, and an
        // entry point names a function declared after it. Checking those in order
        // would reject every well-formed module this crate emits.
        let mut b = Builder::new();
        let u32_type = b.id();
        let pointer = b.id();
        let variable = b.id();
        b.annotate(op::DECORATE, &[variable.0, decoration::BINDING, 0]);
        b.declare(op::TYPE_INT, &[u32_type.0, 32, 0]);
        b.declare(
            op::TYPE_POINTER,
            &[pointer.0, storage::STORAGE_BUFFER, u32_type.0],
        );
        b.declare(
            op::VARIABLE,
            &[pointer.0, variable.0, storage::STORAGE_BUFFER],
        );

        assert_eq!(b.check(), Ok(()));
    }

    #[test]
    fn two_instructions_cannot_claim_the_same_result() {
        let mut b = Builder::new();
        let id = b.id();
        b.declare(op::TYPE_INT, &[id.0, 32, 0]);
        b.declare(op::TYPE_FLOAT, &[id.0, 32]);

        assert_eq!(
            b.check(),
            Err(ModuleError::DefinedTwice {
                id: id.0,
                opcode: op::TYPE_FLOAT
            })
        );
    }

    #[test]
    fn an_access_chain_index_counts_as_an_identifier() {
        // The indices of an access chain are identifiers, not literals. Reading them
        // as literals is what made a two-index chain look like a one-index one, which
        // was the *previous* driver fault. A check that skipped them would have missed
        // it, so it is worth a test of its own.
        let mut b = Builder::new();
        let u32_type = b.id();
        let missing = Id(b.id_count() + 40);
        b.declare(op::TYPE_INT, &[u32_type.0, 32, 0]);
        let result = b.id();
        b.function(
            op::ACCESS_CHAIN,
            &[u32_type.0, result.0, u32_type.0, missing.0],
        );

        assert!(matches!(
            b.check(),
            Err(ModuleError::Undefined { .. } | ModuleError::IdAboveBound { .. })
        ));
    }

    #[test]
    fn the_minimal_module_declares_exactly_one_function() {
        let words = minimal_compute_module([64, 1, 1]);
        let ends = words
            .iter()
            .filter(|w| (*w & 0xFFFF) == u32::from(op::FUNCTION_END) && (*w >> 16) == 1)
            .count();
        assert_eq!(ends, 1);
    }
}
