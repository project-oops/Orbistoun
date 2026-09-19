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

/// Version 1.4, which the mesh-shader extension requires.
///
/// Declared only by modules that need it, for the reason the note above gives: from 1.4 an
/// entry point must list **every** global variable in its interface, not only the inputs and
/// outputs, and getting that wrong fails validation with a message about interfaces that says
/// nothing about which variable is missing. A mesh module's globals are exactly its three
/// output arrays, so listing them all is no burden there.
pub const VERSION_1_4: u32 = 0x0001_0400;

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
    /// Imports an extended instruction set (e.g. `GLSL.std.450`), binding it to a result id.
    pub const EXT_INST_IMPORT: u16 = 11;
    /// Invokes an instruction from an imported extended set - the faithful spelling of the
    /// transcendentals and min/max/abs the core set has no opcode for.
    pub const EXT_INST: u16 = 12;
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
    /// A constant aggregate - a vector, or an array of them - from constant parts.
    pub const CONSTANT_COMPOSITE: u16 = 44;
    /// An aggregate built at run time from values, rather than from constants.
    ///
    /// What an export needs: four registers become the `vec4` a colour output takes.
    pub const COMPOSITE_CONSTRUCT: u16 = 80;
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
    /// Declares how many vertices and primitives a mesh workgroup will actually emit.
    ///
    /// Two operands, both ids. Everything a mesh shader writes afterwards is within what it
    /// declared here - which is the instruction a guest's `MSG_GS_ALLOC_REQ` corresponds to.
    pub const SET_MESH_OUTPUTS_EXT: u16 = 5295;
    /// A texture: the element type, its dimensionality, and how it is used.
    pub const TYPE_IMAGE: u16 = 25;
    /// An image paired with the sampler that reads it.
    pub const TYPE_SAMPLED_IMAGE: u16 = 27;
    /// Samples a texture, letting the implementation pick the level of detail from the
    /// derivatives of the coordinate - which only a fragment stage has.
    pub const IMAGE_SAMPLE_IMPLICIT_LOD: u16 = 87;
    /// Samples a texture at a level of detail the instruction names.
    ///
    /// The form a guest's `image_sample_lz` asks for, and the one available in a stage with
    /// no derivatives. Carries a literal operand mask after the coordinate, then the level.
    pub const IMAGE_SAMPLE_EXPLICIT_LOD: u16 = 88;
    /// Reads one texel by its integer coordinate, with no sampler involved.
    ///
    /// What a guest's `image_load` is: an image descriptor, a texel coordinate, and no
    /// filtering, wrapping or level selection to speak of. Carries a literal operand mask like
    /// the explicit sampling form, and Vulkan requires the level be named for a non-multisampled
    /// image - so in practice it always carries one.
    pub const IMAGE_FETCH: u16 = 95;
    /// The image inside a sampled image, so it can be fetched from rather than sampled.
    ///
    /// A fetch takes an image; a descriptor binds the image and its sampler together. This is
    /// the one that gets from the second to the first, and it is why reading a texel needs no
    /// binding of its own.
    pub const IMAGE: u16 = 100;
    /// Writes one texel of a storage image by its integer coordinate.
    ///
    /// What a guest's `image_store` is. No result: an image write is an effect, like a store to
    /// memory, so nothing names its outcome.
    pub const IMAGE_WRITE: u16 = 99;
    /// Builds a vector from components of two others, by index.
    pub const VECTOR_SHUFFLE: u16 = 79;
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
    /// Converts a signed integer to the float of the same value.
    pub const CONVERT_S_TO_F: u16 = 111;
    /// Converts an unsigned integer to the float of the same value.
    pub const CONVERT_U_TO_F: u16 = 112;
    /// Converts an unsigned integer to another width - truncating to a narrower one, which is
    /// how a packed field is narrowed to the sixteen bits a half occupies.
    pub const UCONVERT: u16 = 113;
    /// Converts a float to another width, widening a half to a single-precision float.
    pub const FCONVERT: u16 = 115;
    /// Chooses between two values without branching.
    ///
    /// How a masked write is expressed when the alternative would be a merge block per
    /// lane: keep the old value where the mask says the lane is inactive.
    pub const SELECT: u16 = 169;
    /// Unsigned right shift.
    pub const SHIFT_RIGHT_LOGICAL: u16 = 194;
    /// Signed right shift - the sign bit fills the vacated high bits, which is what makes it
    /// the tool for sign-extending a narrow signed field up to a full word.
    pub const SHIFT_RIGHT_ARITHMETIC: u16 = 195;
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
    /// Written by the shader and read by the next stage, or by the framebuffer.
    ///
    /// Where a vertex shader puts its position and a fragment shader its colour. Like
    /// [`INPUT`], an output variable must appear in the entry point's interface.
    pub const OUTPUT: u32 = 3;
    /// Read-only storage the pipeline binds: images, samplers, and the two together.
    ///
    /// Not a buffer. A sampled image is bound through a descriptor and read with a sampling
    /// instruction rather than loaded, which is why it has a storage class of its own.
    pub const UNIFORM_CONSTANT: u32 = 0;
    /// Module-scope storage private to one invocation.
    ///
    /// Used here for a constant table a shader indexes: a composite constant cannot be
    /// indexed dynamically, but a `Private` variable initialised with one can.
    pub const PRIVATE: u32 = 6;
}

/// The optional operands a sampling instruction may carry, as a mask.
///
/// Measured from compiled output, like everything else here: a GLSL shader calling
/// `textureLod` was compiled and disassembled, and its sampling instruction carries mask `2`
/// followed by the level (worklog 566).
pub mod image_operands {
    /// A level of detail follows the mask.
    pub const LOD: u32 = 2;
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
    /// Which interface slot an input or output occupies.
    ///
    /// A fragment shader's colour output needs one: location zero is colour attachment
    /// zero, which is what a render pass's first attachment is.
    pub const LOCATION: u32 = 30;
    /// No interpolation: every fragment of a primitive reads the provoking vertex's value.
    ///
    /// What the guest's parameter-move instruction asks for. Reading the same attribute
    /// without this decoration interpolates it, which is a different number everywhere except
    /// at one vertex - so the decoration is the translation rather than a hint about it.
    pub const FLAT: u32 = 14;
    /// Marks a storage image the module only ever writes.
    ///
    /// Emitted because the reference compiler emits it for a write-only image, and because it
    /// is true of everything this project stores to: a guest's `image_store` writes, and the
    /// instruction that reads is a fetch through the sampled binding instead.
    pub const NON_READABLE: u32 = 25;
    /// Which descriptor set.
    pub const DESCRIPTOR_SET: u32 = 34;
    /// Byte offset of a structure member.
    pub const OFFSET: u32 = 35;
}

/// Capability values.
pub mod capability {
    /// Shader stages. The baseline for anything graphics or compute.
    pub const SHADER: u32 = 1;
    /// Permits writing to a storage image whose format the module does not declare.
    ///
    /// **The whole reason a store is translatable at all.** A storage image normally names its
    /// format in the module, and a guest's format lives in a descriptor this project does not
    /// decode - so declaring one would be inventing it. With this, the module says `Unknown` and
    /// the format is whatever the pipeline bound, which is a fact rather than a guess (D692).
    ///
    /// The device has to offer `shaderStorageImageWriteWithoutFormat`, and a module declaring a
    /// capability the device was not created with is refused rather than run - which is the
    /// behaviour to want.
    pub const STORAGE_IMAGE_WRITE_WITHOUT_FORMAT: u32 = 56;
    /// Mesh and task stages, from `SPV_EXT_mesh_shader`.
    ///
    /// It implies [`SHADER`], so a mesh module declares this alone - which is what the
    /// reference compiler emits, and what the numbers here were read out of (worklog 557).
    pub const MESH_SHADING_EXT: u32 = 5283;
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
    /// 16-bit floating-point types and arithmetic.
    ///
    /// Needed to widen a packed half to a float through a real 16-bit float type, which is
    /// the driver's own IEEE conversion rather than a hand-rolled one that has to get
    /// subnormals and infinities right without a way to notice when it does not.
    pub const FLOAT16: u32 = 9;
    /// 16-bit integer types, for narrowing a packed field to the width a half occupies before
    /// it is read as one.
    pub const INT16: u32 = 22;
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
    /// The clip-space position a vertex shader writes.
    pub const POSITION: u32 = 0;
    /// A mesh shader's point index array: one vertex index per primitive (a `uint`).
    pub const PRIMITIVE_POINT_INDICES_EXT: u32 = 5294;
    /// A mesh shader's line index array: two vertex indices per primitive (a `uvec2`).
    pub const PRIMITIVE_LINE_INDICES_EXT: u32 = 5295;
    /// A mesh shader's triangle index array: three vertex indices per primitive (a `uvec3`).
    pub const PRIMITIVE_TRIANGLE_INDICES_EXT: u32 = 5296;
    /// Which vertex of the draw this invocation is.
    ///
    /// Signed, in Vulkan's environment - the variable is declared `int`, and declaring it
    /// unsigned produces a module a driver rejects rather than one that misbehaves.
    pub const VERTEX_INDEX: u32 = 42;
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
    /// A vertex shader.
    pub const VERTEX: u32 = 0;
    /// A mesh shader: one workgroup produces a small set of vertices and primitives.
    ///
    /// The stage a guest's NGG primitive shader corresponds to (D688) - it declares how much
    /// it will emit before emitting any of it, which is what the guest's geometry-engine
    /// allocation request does.
    pub const MESH_EXT: u32 = 5365;
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
    /// The most vertices a mesh shader's workgroup will emit. One literal operand.
    pub const OUTPUT_VERTICES: u32 = 26;
    /// The most primitives a mesh shader's workgroup will emit. One literal operand.
    pub const OUTPUT_PRIMITIVES_EXT: u32 = 5270;
    /// A mesh shader's primitives are points. No operand.
    pub const OUTPUT_POINTS: u32 = 19;
    /// A mesh shader's primitives are lines. No operand.
    pub const OUTPUT_LINES_EXT: u32 = 5269;
    /// A mesh shader's primitives are triangles. No operand.
    pub const OUTPUT_TRIANGLES_EXT: u32 = 5298;
}

/// How many ordered slots the header is kept in.
const HEADER_SLOTS: usize = 6;

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
        // Extended-instruction imports follow the extensions and precede the memory model,
        // which is the order the logical layout requires (SPIR-V 2.4).
        op::EXT_INST_IMPORT => 2,
        op::MEMORY_MODEL => 3,
        op::ENTRY_POINT => 4,
        // Execution modes, and anything else that belongs after the entry point.
        _ => 5,
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

    /// Imports an extended instruction set by name (e.g. `"GLSL.std.450"`), returning the id
    /// [`Self::ext_inst`] refers to as its set.
    ///
    /// Placed in the header at the slot the logical layout requires - after the extensions and
    /// before the memory model - so a caller cannot put it in the wrong place. Call it once per
    /// set per module and keep the id; a second import of the same set is a second set as far as
    /// the validator is concerned.
    pub fn ext_inst_import(&mut self, name: &str) -> Id {
        let set = self.id();
        let mut operands = vec![set.0];
        operands.extend(Self::literal_string(name));
        self.header(op::EXT_INST_IMPORT, &operands);
        set
    }

    /// Invokes instruction number `instruction` from the imported `set`, over `operands`,
    /// producing a value of `result_type`. Returns the result id.
    ///
    /// The word order is the format's: result type, result, set, the instruction number as a
    /// literal, then the operand ids (SPIR-V 3.42.1, OpExtInst).
    pub fn ext_inst(&mut self, result_type: Id, set: Id, instruction: u32, operands: &[Id]) -> Id {
        let result = self.id();
        let mut words = vec![result_type.0, result.0, set.0, instruction];
        words.extend(operands.iter().map(|id| id.0));
        self.function(op::EXT_INST, &words);
        result
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

/// Builds a vertex shader that covers the whole framebuffer with one triangle.
///
/// # Why this exists
///
/// The framebuffer oracle needs a draw, and a draw needs somewhere for its fragments to come
/// from. This is the smallest vertex shader that produces them: three vertices at `(-1, -1)`,
/// `(3, -1)` and `(-1, 3)` in clip space - a triangle twice the size of the viewport, so every
/// pixel is inside it and the fragment shader runs for all of them (D550).
///
/// **Hand-written, not translated.** The oracle exists to check the translator, so a shader the
/// translator produced could not check it. This is assembled instruction by instruction through
/// the builder, which emits the words it is given.
///
/// # Why a constant table rather than arithmetic
///
/// The usual trick derives the position from `gl_VertexIndex` with shifts and a multiply-add.
/// That is fewer declarations and more opcodes, and every opcode is somewhere this could be
/// wrong. Three positions in an array indexed by the vertex index is one `OpAccessChain` and no
/// arithmetic at all - and a wrong constant is visible by reading it, where a wrong shift is not.
///
/// A composite constant cannot be indexed dynamically, so the array lives in a `Private`
/// variable initialised with one. That is what `Private` is here for.
#[must_use]
pub fn fullscreen_triangle_vertex_module() -> Vec<u32> {
    let mut b = Builder::new();

    let void = b.id();
    let fn_type = b.id();
    let f32_type = b.id();
    let i32_type = b.id();
    let u32_type = b.id();
    let vec4 = b.id();
    let array = b.id();
    let (minus_one, three, zero, one) = (b.id(), b.id(), b.id(), b.id());
    let (corner, right, up) = (b.id(), b.id(), b.id());
    let length = b.id();
    let table = b.id();
    let private_array = b.id();
    let private_vec4 = b.id();
    let output_vec4 = b.id();
    let input_i32 = b.id();
    let positions = b.id();
    let position = b.id();
    let vertex_index = b.id();
    let main = b.id();
    let entry_block = b.id();
    let index = b.id();
    let slot = b.id();
    let chosen = b.id();

    b.header(op::CAPABILITY, &[capability::SHADER]);
    b.header(op::MEMORY_MODEL, &[addressing::LOGICAL, memory::GLSL450]);

    // Every input and output variable the entry point touches is named in its interface.
    // Leaving one out produces a module a driver rejects rather than one that misbehaves.
    let mut entry = vec![execution::VERTEX, main.0];
    entry.extend(Builder::literal_string("main"));
    entry.extend([position.0, vertex_index.0]);
    b.header(op::ENTRY_POINT, &entry);

    b.annotate(
        op::DECORATE,
        &[position.0, decoration::BUILT_IN, built_in::POSITION],
    );
    b.annotate(
        op::DECORATE,
        &[vertex_index.0, decoration::BUILT_IN, built_in::VERTEX_INDEX],
    );

    b.declare(op::TYPE_VOID, &[void.0]);
    b.declare(op::TYPE_FUNCTION, &[fn_type.0, void.0]);
    b.declare(op::TYPE_FLOAT, &[f32_type.0, 32]);
    // Signedness 1: `gl_VertexIndex` is a signed int in Vulkan's environment.
    b.declare(op::TYPE_INT, &[i32_type.0, 32, 1]);
    b.declare(op::TYPE_INT, &[u32_type.0, 32, 0]);
    b.declare(op::TYPE_VECTOR, &[vec4.0, f32_type.0, 4]);

    // Clip-space coordinates, as bit patterns because that is what the format takes.
    b.declare(
        op::CONSTANT,
        &[f32_type.0, minus_one.0, (-1.0f32).to_bits()],
    );
    b.declare(op::CONSTANT, &[f32_type.0, three.0, 3.0f32.to_bits()]);
    b.declare(op::CONSTANT, &[f32_type.0, zero.0, 0.0f32.to_bits()]);
    b.declare(op::CONSTANT, &[f32_type.0, one.0, 1.0f32.to_bits()]);
    b.declare(op::CONSTANT, &[u32_type.0, length.0, 3]);

    // The three corners. `w` is one and `z` is zero: no perspective, on the near plane.
    b.declare(
        op::CONSTANT_COMPOSITE,
        &[vec4.0, corner.0, minus_one.0, minus_one.0, zero.0, one.0],
    );
    b.declare(
        op::CONSTANT_COMPOSITE,
        &[vec4.0, right.0, three.0, minus_one.0, zero.0, one.0],
    );
    b.declare(
        op::CONSTANT_COMPOSITE,
        &[vec4.0, up.0, minus_one.0, three.0, zero.0, one.0],
    );
    b.declare(op::TYPE_ARRAY, &[array.0, vec4.0, length.0]);
    b.declare(
        op::CONSTANT_COMPOSITE,
        &[array.0, table.0, corner.0, right.0, up.0],
    );

    b.declare(
        op::TYPE_POINTER,
        &[private_array.0, storage::PRIVATE, array.0],
    );
    b.declare(
        op::TYPE_POINTER,
        &[private_vec4.0, storage::PRIVATE, vec4.0],
    );
    b.declare(op::TYPE_POINTER, &[output_vec4.0, storage::OUTPUT, vec4.0]);
    b.declare(op::TYPE_POINTER, &[input_i32.0, storage::INPUT, i32_type.0]);

    // The initialiser is the fourth operand, which is what makes the table indexable.
    b.declare(
        op::VARIABLE,
        &[private_array.0, positions.0, storage::PRIVATE, table.0],
    );
    b.declare(op::VARIABLE, &[output_vec4.0, position.0, storage::OUTPUT]);
    b.declare(op::VARIABLE, &[input_i32.0, vertex_index.0, storage::INPUT]);

    b.function(op::FUNCTION, &[void.0, main.0, 0, fn_type.0]);
    b.function(op::LABEL, &[entry_block.0]);
    b.function(op::LOAD, &[i32_type.0, index.0, vertex_index.0]);
    b.function(
        op::ACCESS_CHAIN,
        &[private_vec4.0, slot.0, positions.0, index.0],
    );
    b.function(op::LOAD, &[vec4.0, chosen.0, slot.0]);
    b.function(op::STORE, &[position.0, chosen.0]);
    b.function(op::RETURN, &[]);
    b.function(op::FUNCTION_END, &[]);

    b.check()
        .expect("this crate built a module with identifiers that do not resolve");
    b.finish()
}

/// Builds a fragment shader that writes one colour to attachment zero.
///
/// # Why a constant and nothing else
///
/// The harness this feeds asks a single question - **does a fragment shader's output reach the
/// attachment** - and anything the shader computed would make a failure ambiguous between the
/// pipeline and the arithmetic. A constant makes the answer binary (D550).
///
/// Hand-written for the same reason as the vertex shader beside it: the oracle cannot be built
/// out of the thing it checks.
///
/// The components are taken as bit patterns rather than floats so the caller's expectation and
/// the shader's constant are written the same way once, and compared against a byte value the
/// caller writes separately.
#[must_use]
pub fn constant_colour_fragment_module(colour: [f32; 4]) -> Vec<u32> {
    let mut b = Builder::new();

    let void = b.id();
    let fn_type = b.id();
    let f32_type = b.id();
    let vec4 = b.id();
    let components = [b.id(), b.id(), b.id(), b.id()];
    let value = b.id();
    let output_vec4 = b.id();
    let output = b.id();
    let main = b.id();
    let entry_block = b.id();

    b.header(op::CAPABILITY, &[capability::SHADER]);
    b.header(op::MEMORY_MODEL, &[addressing::LOGICAL, memory::GLSL450]);

    let mut entry = vec![execution::FRAGMENT, main.0];
    entry.extend(Builder::literal_string("main"));
    entry.push(output.0);
    b.header(op::ENTRY_POINT, &entry);
    // Required of every fragment entry point; Vulkan's framebuffer origin is the top left.
    b.header(op::EXECUTION_MODE, &[main.0, mode::ORIGIN_UPPER_LEFT]);

    // Location zero is colour attachment zero.
    b.annotate(op::DECORATE, &[output.0, decoration::LOCATION, 0]);

    b.declare(op::TYPE_VOID, &[void.0]);
    b.declare(op::TYPE_FUNCTION, &[fn_type.0, void.0]);
    b.declare(op::TYPE_FLOAT, &[f32_type.0, 32]);
    b.declare(op::TYPE_VECTOR, &[vec4.0, f32_type.0, 4]);
    for (id, component) in components.iter().zip(colour) {
        b.declare(op::CONSTANT, &[f32_type.0, id.0, component.to_bits()]);
    }
    let mut composite = vec![vec4.0, value.0];
    composite.extend(components.iter().map(|id| id.0));
    b.declare(op::CONSTANT_COMPOSITE, &composite);
    b.declare(op::TYPE_POINTER, &[output_vec4.0, storage::OUTPUT, vec4.0]);
    b.declare(op::VARIABLE, &[output_vec4.0, output.0, storage::OUTPUT]);

    b.function(op::FUNCTION, &[void.0, main.0, 0, fn_type.0]);
    b.function(op::LABEL, &[entry_block.0]);
    b.function(op::STORE, &[output.0, value.0]);
    b.function(op::RETURN, &[]);
    b.function(op::FUNCTION_END, &[]);

    b.check()
        .expect("this crate built a module with identifiers that do not resolve");
    b.finish()
}

/// Declares the per-corner varying values as one constant array.
///
/// The same shape as the position table beside it - three `vec4` constants gathered into an
/// array - so the vertex shader indexes both the same way. Split out for length.
fn declare_varying_table(
    b: &mut Builder,
    f32_type: Id,
    vec4: Id,
    array: Id,
    table: Id,
    corners: [[f32; 4]; 3],
) {
    let mut vertices = Vec::with_capacity(3);
    for value in corners {
        let components: Vec<u32> = value
            .iter()
            .map(|component| {
                let id = b.id();
                b.declare(op::CONSTANT, &[f32_type.0, id.0, component.to_bits()]);
                id.0
            })
            .collect();
        let composite = b.id();
        let mut words = vec![vec4.0, composite.0];
        words.extend(components);
        b.declare(op::CONSTANT_COMPOSITE, &words);
        vertices.push(composite.0);
    }
    let mut words = vec![array.0, table.0];
    words.extend(vertices);
    b.declare(op::CONSTANT_COMPOSITE, &words);
}

/// Builds a vertex shader that covers the framebuffer and hands each corner a value.
///
/// # Why the oracle needs this
///
/// [`fullscreen_triangle_vertex_module`] emits a position and nothing else, so a fragment shader
/// fed by it has no inputs. Interpolation - which is what `v_interp_p1_f32` and its pair
/// compute, and the last capture-free family the translator refuses - cannot be checked against
/// a pipeline that never interpolates anything (D554).
///
/// This is the same triangle with one addition: a `Location 0` output carrying `corners[i]` for
/// vertex `i`. Everything else is unchanged, deliberately - the positions are the same constant
/// table, so a failure here is about the varying rather than about the geometry.
///
/// Hand-assembled, like everything else in this oracle. The translator is what it exists to
/// check.
// A module is a linear sequence of declarations, and every identifier in it is a local the
// next line needs. Splitting further means helpers taking six or eight ids apiece, which
// moves the length rather than removing it and makes the order harder to read - the one
// property that matters in a builder. The varying and position tables are already out.
#[allow(clippy::too_many_lines)]
#[must_use]
pub fn interpolated_vertex_module(corners: [[f32; 4]; 3]) -> Vec<u32> {
    interpolating_vertex_module(&[corners])
}

/// The same triangle carrying **one varying per location**, in order from zero.
///
/// # Why more than one
///
/// A guest's textured pixel shader reads two: a colour it modulates by and a coordinate it
/// samples at. Fed by a vertex module that writes only the first, the second arrives undefined -
/// so the shader samples one texel everywhere and the frame says nothing about whether the
/// coordinate reached the sample. The console's own shader is exactly that shape (worklog 581).
///
/// Each entry is that varying's value at the three corners, and the location is its index.
/// Everything else is [`interpolated_vertex_module`] unchanged - the same position table, so a
/// failure is about the varyings rather than about the geometry.
// A module is a linear sequence of declarations, and every identifier in it is a local the
// next line needs. Splitting further means helpers taking six or eight ids apiece, which
// moves the length rather than removing it and makes the order harder to read - the one
// property that matters in a builder. The varying and position tables are already out.
#[allow(clippy::too_many_lines)]
#[must_use]
pub fn interpolating_vertex_module(varying_values: &[[[f32; 4]; 3]]) -> Vec<u32> {
    let mut b = Builder::new();

    let void = b.id();
    let fn_type = b.id();
    let f32_type = b.id();
    let i32_type = b.id();
    let u32_type = b.id();
    let vec4 = b.id();
    let array = b.id();
    let (minus_one, three, zero, one) = (b.id(), b.id(), b.id(), b.id());
    let (corner, right, up) = (b.id(), b.id(), b.id());
    let length = b.id();
    let table = b.id();
    let private_array = b.id();
    let private_vec4 = b.id();
    let output_vec4 = b.id();
    let input_i32 = b.id();
    let positions = b.id();
    let position = b.id();
    let vertex_index = b.id();
    // One set of identifiers per varying, in location order.
    let slots: Vec<(Id, Id, Id)> = varying_values
        .iter()
        .map(|_| (b.id(), b.id(), b.id()))
        .collect();
    let main = b.id();
    let entry_block = b.id();
    let index = b.id();
    let slot = b.id();
    let chosen = b.id();

    b.header(op::CAPABILITY, &[capability::SHADER]);
    b.header(op::MEMORY_MODEL, &[addressing::LOGICAL, memory::GLSL450]);

    let mut entry = vec![execution::VERTEX, main.0];
    entry.extend(Builder::literal_string("main"));
    entry.extend([position.0, vertex_index.0]);
    entry.extend(slots.iter().map(|(_, _, out)| out.0));
    b.header(op::ENTRY_POINT, &entry);

    b.annotate(
        op::DECORATE,
        &[position.0, decoration::BUILT_IN, built_in::POSITION],
    );
    b.annotate(
        op::DECORATE,
        &[vertex_index.0, decoration::BUILT_IN, built_in::VERTEX_INDEX],
    );
    for (location, (_, _, out)) in slots.iter().enumerate() {
        let location = u32::try_from(location).unwrap_or(0);
        b.annotate(op::DECORATE, &[out.0, decoration::LOCATION, location]);
    }

    b.declare(op::TYPE_VOID, &[void.0]);
    b.declare(op::TYPE_FUNCTION, &[fn_type.0, void.0]);
    b.declare(op::TYPE_FLOAT, &[f32_type.0, 32]);
    b.declare(op::TYPE_INT, &[i32_type.0, 32, 1]);
    b.declare(op::TYPE_INT, &[u32_type.0, 32, 0]);
    b.declare(op::TYPE_VECTOR, &[vec4.0, f32_type.0, 4]);

    b.declare(
        op::CONSTANT,
        &[f32_type.0, minus_one.0, (-1.0f32).to_bits()],
    );
    b.declare(op::CONSTANT, &[f32_type.0, three.0, 3.0f32.to_bits()]);
    b.declare(op::CONSTANT, &[f32_type.0, zero.0, 0.0f32.to_bits()]);
    b.declare(op::CONSTANT, &[f32_type.0, one.0, 1.0f32.to_bits()]);
    b.declare(op::CONSTANT, &[u32_type.0, length.0, 3]);

    b.declare(
        op::CONSTANT_COMPOSITE,
        &[vec4.0, corner.0, minus_one.0, minus_one.0, zero.0, one.0],
    );
    b.declare(
        op::CONSTANT_COMPOSITE,
        &[vec4.0, right.0, three.0, minus_one.0, zero.0, one.0],
    );
    b.declare(
        op::CONSTANT_COMPOSITE,
        &[vec4.0, up.0, minus_one.0, three.0, zero.0, one.0],
    );
    b.declare(op::TYPE_ARRAY, &[array.0, vec4.0, length.0]);
    b.declare(
        op::CONSTANT_COMPOSITE,
        &[array.0, table.0, corner.0, right.0, up.0],
    );

    for ((_, table, _), corners) in slots.iter().zip(varying_values) {
        declare_varying_table(&mut b, f32_type, vec4, array, *table, *corners);
    }

    b.declare(
        op::TYPE_POINTER,
        &[private_array.0, storage::PRIVATE, array.0],
    );
    b.declare(
        op::TYPE_POINTER,
        &[private_vec4.0, storage::PRIVATE, vec4.0],
    );
    b.declare(op::TYPE_POINTER, &[output_vec4.0, storage::OUTPUT, vec4.0]);
    b.declare(op::TYPE_POINTER, &[input_i32.0, storage::INPUT, i32_type.0]);

    b.declare(
        op::VARIABLE,
        &[private_array.0, positions.0, storage::PRIVATE, table.0],
    );
    for (values, table, out) in &slots {
        b.declare(
            op::VARIABLE,
            &[private_array.0, values.0, storage::PRIVATE, table.0],
        );
        b.declare(op::VARIABLE, &[output_vec4.0, out.0, storage::OUTPUT]);
    }
    b.declare(op::VARIABLE, &[output_vec4.0, position.0, storage::OUTPUT]);
    b.declare(op::VARIABLE, &[input_i32.0, vertex_index.0, storage::INPUT]);

    b.function(op::FUNCTION, &[void.0, main.0, 0, fn_type.0]);
    b.function(op::LABEL, &[entry_block.0]);
    b.function(op::LOAD, &[i32_type.0, index.0, vertex_index.0]);
    b.function(
        op::ACCESS_CHAIN,
        &[private_vec4.0, slot.0, positions.0, index.0],
    );
    b.function(op::LOAD, &[vec4.0, chosen.0, slot.0]);
    b.function(op::STORE, &[position.0, chosen.0]);
    for (values, _, out) in &slots {
        let at = b.id();
        let value = b.id();
        b.function(op::ACCESS_CHAIN, &[private_vec4.0, at.0, values.0, index.0]);
        b.function(op::LOAD, &[vec4.0, value.0, at.0]);
        b.function(op::STORE, &[out.0, value.0]);
    }
    b.function(op::RETURN, &[]);
    b.function(op::FUNCTION_END, &[]);

    b.check()
        .expect("this crate built a module with identifiers that do not resolve");
    b.finish()
}

/// Which level of detail a sampling module asks its texture for.
///
/// Not a detail to default: a guest's `image_sample_lz` names level zero *explicitly*, and a
/// fragment stage is also free to let the implementation choose from the derivatives of the
/// coordinate. The two are different instructions, and an oracle that only had one of them
/// could not be what a translation is measured against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lod {
    /// Chosen by the implementation from the derivatives of the coordinate. Fragment stages
    /// only - nothing else has derivatives.
    Implicit,
    /// Level zero, named by the instruction. What a guest's `_lz` form asks for.
    Zero,
}

/// A fragment shader that samples a bound texture at the interpolated coordinate.
///
/// # Why this exists
///
/// The oracle for the one thing a guest's textured pixel shader needs and this project has
/// never had: an image, a sampler, and an instruction that reads one through the other. A
/// guest's `image_sample_lz` takes its texture from a descriptor held in eight scalar registers
/// and its sampler from four more; what those describe has to become *this* on the host, and
/// building the host half first is the order that worked for the mesh stage (D549, worklog 557).
///
/// # Where the numbers came from
///
/// Measured, like every other encoding here. A GLSL fragment shader that samples a texture was
/// compiled with the SDK's compiler and read back with `spirv-dis`: the image type and its seven
/// operands, the sampled-image type, the storage class a descriptor-bound image lives in, and
/// the sampling instruction are that module's own (worklog 566).
///
/// The sampled image sits at set 0, binding 2, because bindings 0 and 1 are the two storage
/// buffers every translated module declares.
///
/// The level of detail is the implementation's, chosen from the derivatives of the coordinate.
/// That is what a fragment stage can do and a guest's `_lz` form asks for level zero explicitly;
/// which of the two a translation should emit is a question for the translation, not for this.
// A module is a linear sequence of declarations, each needed by the line after it - the same
// judgement every other builder here records.
#[allow(clippy::too_many_lines)]
#[must_use]
pub fn sampling_fragment_module(lod: Lod) -> Vec<u32> {
    let mut b = Builder::new();

    let void = b.id();
    let fn_type = b.id();
    let f32_type = b.id();
    let vec4 = b.id();
    let vec2 = b.id();
    let image = b.id();
    let sampled_image = b.id();
    let sampled_ptr = b.id();
    let texture = b.id();
    let input_vec4 = b.id();
    let output_vec4 = b.id();
    let input = b.id();
    let output = b.id();
    let zero = b.id();
    let main = b.id();
    let entry_block = b.id();

    b.header(op::CAPABILITY, &[capability::SHADER]);
    b.header(op::MEMORY_MODEL, &[addressing::LOGICAL, memory::GLSL450]);
    let mut entry = vec![execution::FRAGMENT, main.0];
    entry.extend(Builder::literal_string("main"));
    entry.extend([input.0, output.0]);
    b.header(op::ENTRY_POINT, &entry);
    b.header(op::EXECUTION_MODE, &[main.0, mode::ORIGIN_UPPER_LEFT]);

    b.annotate(op::DECORATE, &[input.0, decoration::LOCATION, 0]);
    b.annotate(op::DECORATE, &[output.0, decoration::LOCATION, 0]);
    b.annotate(op::DECORATE, &[texture.0, decoration::DESCRIPTOR_SET, 0]);
    b.annotate(
        op::DECORATE,
        &[texture.0, decoration::BINDING, TEXTURE_BINDING],
    );

    b.declare(op::TYPE_VOID, &[void.0]);
    b.declare(op::TYPE_FUNCTION, &[fn_type.0, void.0]);
    b.declare(op::TYPE_FLOAT, &[f32_type.0, 32]);
    b.declare(op::TYPE_VECTOR, &[vec4.0, f32_type.0, 4]);
    b.declare(op::TYPE_VECTOR, &[vec2.0, f32_type.0, 2]);
    // Element type, then: two-dimensional, not a depth texture, not an array, not
    // multi-sampled, used with a sampler, and of no declared format - which is what a
    // descriptor-bound sampled image is.
    b.declare(op::TYPE_IMAGE, &[image.0, f32_type.0, 1, 0, 0, 0, 1, 0]);
    b.declare(op::TYPE_SAMPLED_IMAGE, &[sampled_image.0, image.0]);
    b.declare(
        op::TYPE_POINTER,
        &[sampled_ptr.0, storage::UNIFORM_CONSTANT, sampled_image.0],
    );
    b.declare(
        op::VARIABLE,
        &[sampled_ptr.0, texture.0, storage::UNIFORM_CONSTANT],
    );
    b.declare(op::TYPE_POINTER, &[input_vec4.0, storage::INPUT, vec4.0]);
    b.declare(op::VARIABLE, &[input_vec4.0, input.0, storage::INPUT]);
    b.declare(op::TYPE_POINTER, &[output_vec4.0, storage::OUTPUT, vec4.0]);
    b.declare(op::VARIABLE, &[output_vec4.0, output.0, storage::OUTPUT]);

    // The level an explicit sample asks for. Declared whichever form this module emits: a
    // constant nothing references is dead weight in a module, not an error, and declaring it
    // under a branch would put one in the single place a builder should read straight down.
    b.declare(op::CONSTANT, &[f32_type.0, zero.0, 0.0f32.to_bits()]);

    b.function(op::FUNCTION, &[void.0, main.0, 0, fn_type.0]);
    b.function(op::LABEL, &[entry_block.0]);

    let bound = b.id();
    b.function(op::LOAD, &[sampled_image.0, bound.0, texture.0]);
    let attribute = b.id();
    b.function(op::LOAD, &[vec4.0, attribute.0, input.0]);
    // The first two components of the attribute, which is where a texture coordinate is.
    let coordinate = b.id();
    b.function(
        op::VECTOR_SHUFFLE,
        &[vec2.0, coordinate.0, attribute.0, attribute.0, 0, 1],
    );
    let sampled = b.id();
    match lod {
        Lod::Implicit => b.function(
            op::IMAGE_SAMPLE_IMPLICIT_LOD,
            &[vec4.0, sampled.0, bound.0, coordinate.0],
        ),
        // The mask says a level follows, and the level is the constant zero. Two extra
        // operands, which is the whole difference between the two instructions.
        Lod::Zero => b.function(
            op::IMAGE_SAMPLE_EXPLICIT_LOD,
            &[
                vec4.0,
                sampled.0,
                bound.0,
                coordinate.0,
                image_operands::LOD,
                zero.0,
            ],
        ),
    }
    b.function(op::STORE, &[output.0, sampled.0]);
    b.function(op::RETURN, &[]);
    b.function(op::FUNCTION_END, &[]);

    b.check()
        .expect("this crate built a module with identifiers that do not resolve");
    b.finish()
}

/// Which binding a sampled image is bound at.
///
/// Two, because zero and one are the storage buffers every translated module declares - the
/// observation window and guest memory - and a pipeline binds one set.
pub const TEXTURE_BINDING: u32 = 2;

/// Which binding a storage image is bound at.
///
/// Three, after the sampled image. They are two bindings rather than one because they are two
/// different things: a sampled image is read through a sampler and cannot be written, and a
/// storage image is written and declares no sampler. A guest's `image_store` names an image
/// descriptor exactly as `image_load` does, and only the host cares that the two are separate.
pub const STORAGE_IMAGE_BINDING: u32 = 3;

/// A fragment shader that writes one texel of a storage image, and a colour.
///
/// # Why both
///
/// A fragment shader that only stored would be a draw with nothing to look at, and a frame that
/// came back as the clear would be indistinguishable from a draw that never ran. Writing the
/// colour too means the attachment says "the shader ran" and the image says "and this is what it
/// stored", which is the same pair of observations the guest-memory window gives (worklog 570).
///
/// # Where the numbers came from
///
/// Measured. A GLSL fragment shader calling `imageStore` through a format-less `writeonly
/// image2D` was compiled and disassembled: the write instruction, the capability a format-less
/// storage image requires, the `NonReadable` decoration, and the image type with its `Sampled`
/// operand at **2** rather than 1 - which is the whole difference between a storage image and a
/// sampled one (worklog 575).
///
/// The texel and its coordinate are the caller's, so a test can name what it expects to find
/// where, rather than deriving it from a varying and asserting on arithmetic this module did.
// A module is a linear sequence of declarations, each needed by the line after it - the same
// judgement every other builder here records.
#[allow(clippy::too_many_lines)]
#[must_use]
pub fn storing_fragment_module(at: [u32; 2], texel: [f32; 4]) -> Vec<u32> {
    let mut b = Builder::new();

    let void = b.id();
    let fn_type = b.id();
    let f32_type = b.id();
    let u32_type = b.id();
    let vec4 = b.id();
    let uvec2 = b.id();
    let image = b.id();
    let image_ptr = b.id();
    let target = b.id();
    let output_vec4 = b.id();
    let output = b.id();
    let components: [Id; 4] = [b.id(), b.id(), b.id(), b.id()];
    let colour = b.id();
    let at_x = b.id();
    let at_y = b.id();
    let coordinate = b.id();
    let main = b.id();
    let entry_block = b.id();

    b.header(op::CAPABILITY, &[capability::SHADER]);
    b.header(
        op::CAPABILITY,
        &[capability::STORAGE_IMAGE_WRITE_WITHOUT_FORMAT],
    );
    b.header(op::MEMORY_MODEL, &[addressing::LOGICAL, memory::GLSL450]);
    let mut entry = vec![execution::FRAGMENT, main.0];
    entry.extend(Builder::literal_string("main"));
    entry.push(output.0);
    b.header(op::ENTRY_POINT, &entry);
    b.header(op::EXECUTION_MODE, &[main.0, mode::ORIGIN_UPPER_LEFT]);

    b.annotate(op::DECORATE, &[output.0, decoration::LOCATION, 0]);
    b.annotate(op::DECORATE, &[target.0, decoration::DESCRIPTOR_SET, 0]);
    b.annotate(
        op::DECORATE,
        &[target.0, decoration::BINDING, STORAGE_IMAGE_BINDING],
    );
    b.annotate(op::DECORATE, &[target.0, decoration::NON_READABLE]);

    b.declare(op::TYPE_VOID, &[void.0]);
    b.declare(op::TYPE_FUNCTION, &[fn_type.0, void.0]);
    b.declare(op::TYPE_FLOAT, &[f32_type.0, 32]);
    b.declare(op::TYPE_INT, &[u32_type.0, 32, 0]);
    b.declare(op::TYPE_VECTOR, &[vec4.0, f32_type.0, 4]);
    b.declare(op::TYPE_VECTOR, &[uvec2.0, u32_type.0, 2]);
    // The same seven operands a sampled image takes, with **`Sampled` at 2**: written to
    // through an image instruction rather than read through a sampler. The format stays
    // `Unknown`, which is what the capability above buys.
    b.declare(op::TYPE_IMAGE, &[image.0, f32_type.0, 1, 0, 0, 0, 2, 0]);
    b.declare(
        op::TYPE_POINTER,
        &[image_ptr.0, storage::UNIFORM_CONSTANT, image.0],
    );
    b.declare(
        op::VARIABLE,
        &[image_ptr.0, target.0, storage::UNIFORM_CONSTANT],
    );
    b.declare(op::TYPE_POINTER, &[output_vec4.0, storage::OUTPUT, vec4.0]);
    b.declare(op::VARIABLE, &[output_vec4.0, output.0, storage::OUTPUT]);

    for (id, component) in components.iter().zip(texel) {
        b.declare(op::CONSTANT, &[f32_type.0, id.0, component.to_bits()]);
    }
    let mut composite = vec![vec4.0, colour.0];
    composite.extend(components.iter().map(|id| id.0));
    b.declare(op::CONSTANT_COMPOSITE, &composite);
    b.declare(op::CONSTANT, &[u32_type.0, at_x.0, at[0]]);
    b.declare(op::CONSTANT, &[u32_type.0, at_y.0, at[1]]);
    b.declare(
        op::CONSTANT_COMPOSITE,
        &[uvec2.0, coordinate.0, at_x.0, at_y.0],
    );

    b.function(op::FUNCTION, &[void.0, main.0, 0, fn_type.0]);
    b.function(op::LABEL, &[entry_block.0]);

    let bound = b.id();
    b.function(op::LOAD, &[image.0, bound.0, target.0]);
    b.function(op::IMAGE_WRITE, &[bound.0, coordinate.0, colour.0]);
    b.function(op::STORE, &[output.0, colour.0]);
    b.function(op::RETURN, &[]);
    b.function(op::FUNCTION_END, &[]);

    b.check()
        .expect("this crate built a module with identifiers that do not resolve");
    b.finish()
}

/// Builds a fragment shader that writes its interpolated input straight out.
///
/// The counterpart to [`interpolated_vertex_module`]: a `Location 0` input, read and stored to a
/// `Location 0` output with nothing in between. What lands in the attachment is therefore
/// exactly what the pipeline interpolated, which is the thing being checked.
#[must_use]
pub fn passthrough_fragment_module() -> Vec<u32> {
    let mut b = Builder::new();

    let void = b.id();
    let fn_type = b.id();
    let f32_type = b.id();
    let vec4 = b.id();
    let input_vec4 = b.id();
    let output_vec4 = b.id();
    let input = b.id();
    let output = b.id();
    let main = b.id();
    let entry_block = b.id();
    let value = b.id();

    b.header(op::CAPABILITY, &[capability::SHADER]);
    b.header(op::MEMORY_MODEL, &[addressing::LOGICAL, memory::GLSL450]);

    let mut entry = vec![execution::FRAGMENT, main.0];
    entry.extend(Builder::literal_string("main"));
    entry.extend([input.0, output.0]);
    b.header(op::ENTRY_POINT, &entry);
    b.header(op::EXECUTION_MODE, &[main.0, mode::ORIGIN_UPPER_LEFT]);

    // The input's location must match the vertex shader's output location, which is how a
    // varying is paired across the two stages.
    b.annotate(op::DECORATE, &[input.0, decoration::LOCATION, 0]);
    b.annotate(op::DECORATE, &[output.0, decoration::LOCATION, 0]);

    b.declare(op::TYPE_VOID, &[void.0]);
    b.declare(op::TYPE_FUNCTION, &[fn_type.0, void.0]);
    b.declare(op::TYPE_FLOAT, &[f32_type.0, 32]);
    b.declare(op::TYPE_VECTOR, &[vec4.0, f32_type.0, 4]);
    b.declare(op::TYPE_POINTER, &[input_vec4.0, storage::INPUT, vec4.0]);
    b.declare(op::TYPE_POINTER, &[output_vec4.0, storage::OUTPUT, vec4.0]);
    b.declare(op::VARIABLE, &[input_vec4.0, input.0, storage::INPUT]);
    b.declare(op::VARIABLE, &[output_vec4.0, output.0, storage::OUTPUT]);

    b.function(op::FUNCTION, &[void.0, main.0, 0, fn_type.0]);
    b.function(op::LABEL, &[entry_block.0]);
    b.function(op::LOAD, &[vec4.0, value.0, input.0]);
    b.function(op::STORE, &[output.0, value.0]);
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
            // The eighteen spaces that used to sit before "checked" are what D184 is about:
            // this message was written as a line-continued literal, `cargo fmt` collapsed it,
            // and the source indentation was baked into what a reader sees. It had shipped
            // that way. Repaired here rather than left, because the whole point of converting
            // the rest of the tree is that a message reads as the sentence it was written as.
            Self::UnknownOpcode { opcode } => write!(
                f,
                concat!(
                    "opcode {} has no row in the shape table, so this module cannot be ",
                    "checked - add one rather than trusting the result"
                ),
                opcode
            ),
            Self::Undefined { id, opcode } => write!(
                f,
                concat!(
                    "%{} is used by opcode {} but nothing defines it - a driver ",
                    "handed this module faults rather than diagnosing it"
                ),
                id, opcode
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
    // Imports an extended set: result id first, then a literal name string - no identifiers,
    // so `rest` is None.
    (op::EXT_INST_IMPORT, Some(0), &[], None, RestStride::Every),
    // Result type, result, set, a literal instruction number, then operand identifiers. The
    // number at index 3 is a literal, so `rest` starts at 4 and index 3 is named by neither
    // `fixed` nor `rest` - reading it as an identifier would reject a valid module.
    (op::EXT_INST, Some(1), &[0, 2], Some(4), RestStride::Every),
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
    // Result type, result, then every constituent - each an identifier that must exist.
    (
        op::CONSTANT_COMPOSITE,
        Some(1),
        &[0],
        Some(2),
        RestStride::Every,
    ),
    // Result type, result, then every constituent - the same shape as the constant form,
    // and every constituent is an identifier that must already exist.
    (
        op::COMPOSITE_CONSTRUCT,
        Some(1),
        &[0],
        Some(2),
        RestStride::Every,
    ),
    (op::CONSTANT_NULL, Some(1), &[0], None, RestStride::Every),
    // Operand two is the storage class; an initialiser, if present, follows it.
    (op::VARIABLE, Some(1), &[0], Some(3), RestStride::Every),
    // Operand two is a function control mask, operand three the function type.
    (op::FUNCTION, Some(1), &[0, 3], None, RestStride::Every),
    // Every index after the base is an identifier rather than a literal - the detail
    // that made a two-index access chain look like a one-index one, and cost a driver
    // fault to find.
    (op::ACCESS_CHAIN, Some(1), &[0], Some(2), RestStride::Every),
    (op::LOAD, Some(1), &[0, 2], None, RestStride::Every),
    (op::BITCAST, Some(1), &[0, 2], None, RestStride::Every),
    (
        op::CONVERT_S_TO_F,
        Some(1),
        &[0, 2],
        None,
        RestStride::Every,
    ),
    (
        op::CONVERT_U_TO_F,
        Some(1),
        &[0, 2],
        None,
        RestStride::Every,
    ),
    // Declares a mesh workgroup's output counts: two identifiers, no result. The whole
    // instruction is operands, so `rest` starts at zero.
    (
        op::SET_MESH_OUTPUTS_EXT,
        None,
        &[],
        Some(0),
        RestStride::Every,
    ),
    // A texture type: result id, the element type, then six literals saying what kind of
    // texture it is. Only operand one is an identifier - reading the dimensionality or the
    // format as one would reject a module that is fine.
    (op::TYPE_IMAGE, Some(0), &[1], None, RestStride::Every),
    // An image paired with a sampler: result id and the image type it wraps.
    (
        op::TYPE_SAMPLED_IMAGE,
        Some(0),
        &[1],
        None,
        RestStride::Every,
    ),
    // Result type, result, the sampled image, and the coordinate. A tail of image operands
    // may follow - a literal mask and then identifiers - and this crate emits none, so it is
    // named by neither `fixed` nor `rest`. **Emitting one means extending this row**: the
    // alternative is reading the mask as an identifier and rejecting a valid module.
    (
        op::IMAGE_SAMPLE_IMPLICIT_LOD,
        Some(1),
        &[0, 2, 3],
        None,
        RestStride::Every,
    ),
    // The same, plus a literal operand mask at index four and the identifiers it announces
    // from index five - so the mask is named by neither `fixed` nor `rest`, and the level
    // that follows it is checked.
    (
        op::IMAGE_SAMPLE_EXPLICIT_LOD,
        Some(1),
        &[0, 2, 3],
        Some(5),
        RestStride::Every,
    ),
    // The same shape as the explicit sampling form, and for the same reason: a literal mask at
    // index four, and the level it announces from index five.
    (
        op::IMAGE_FETCH,
        Some(1),
        &[0, 2, 3],
        Some(5),
        RestStride::Every,
    ),
    // Result type, result, and the sampled image the image is taken out of.
    (op::IMAGE, Some(1), &[0, 2], None, RestStride::Every),
    // No result at all - an image write is an effect. The image, the coordinate and the texel
    // are all identifiers, so the whole instruction is `rest` from zero.
    (op::IMAGE_WRITE, None, &[], Some(0), RestStride::Every),
    // Result type, result, the two vectors, then literal component indices - which are
    // positions in a vector, not identifiers.
    (
        op::VECTOR_SHUFFLE,
        Some(1),
        &[0, 2, 3],
        None,
        RestStride::Every,
    ),
    (op::UCONVERT, Some(1), &[0, 2], None, RestStride::Every),
    (op::FCONVERT, Some(1), &[0, 2], None, RestStride::Every),
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
        op::SHIFT_RIGHT_ARITHMETIC,
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

/// A mesh shader that emits one triangle covering the viewport, with a colour per corner.
///
/// # Why this exists
///
/// The oracle for the stage a guest's NGG primitive shader translates to (D688). Everything
/// about that correspondence is written down and nothing had been run: a mesh shader declares
/// how many vertices and primitives its workgroup will emit, writes the primitive's indices,
/// and writes per-vertex outputs - which is `MSG_GS_ALLOC_REQ`, `exp prim` and
/// `exp pos`/`exp param`, in that order. This is the host half of it, hand-assembled, so the
/// translated half has something to be compared against rather than only reasoned about.
///
/// # Where the numbers came from
///
/// Measured, not transcribed. A reference mesh shader was compiled with the SDK's GLSL
/// compiler and read back with `spirv-dis`, and every mesh-specific value here - the
/// capability, the execution model, the three execution modes, the index built-in and the
/// opcode that declares the counts - was taken out of that module's words (worklog 557). The
/// structure follows it too: the per-vertex outputs are an array of a `Block` struct whose
/// first member carries `Position`, because that is what the stage requires and not a choice.
///
/// The geometry is the same triangle every other oracle here draws - one that covers the
/// viewport from three corners - so a frame from this is comparable with a frame from the
/// vertex-shader path pixel for pixel.
// A module is a linear sequence of declarations, each needed by the line after it. The same
// judgement the other builders here record: splitting it moves the length rather than removing
// it and hides the order, which is the one property a builder has to show.
#[allow(clippy::too_many_lines)]
#[must_use]
pub fn triangle_mesh_module(corners: [[f32; 4]; 3]) -> Vec<u32> {
    // 1.4, because `SPV_EXT_mesh_shader` requires it - said by `spirv-val` on the first
    // attempt, which is the kind of thing this project would rather be told than assume.
    let mut b = Builder::new().with_version(VERSION_1_4);

    let void = b.id();
    let fn_type = b.id();
    let f32_type = b.id();
    let u32_type = b.id();
    let vec4 = b.id();
    let uvec3 = b.id();
    let per_vertex = b.id();
    let vertex_array = b.id();
    let vertex_array_ptr = b.id();
    let vertices = b.id();
    let colour_array = b.id();
    let colour_array_ptr = b.id();
    let colours = b.id();
    let index_array = b.id();
    let index_array_ptr = b.id();
    let indices = b.id();
    let output_vec4 = b.id();
    let output_uvec3 = b.id();
    let (zero, one, two, three) = (b.id(), b.id(), b.id(), b.id());
    let triangle = b.id();
    let main = b.id();
    let entry_block = b.id();

    // `MeshShadingEXT` implies `Shader`, so it is declared alone - as the reference does.
    b.header(op::CAPABILITY, &[capability::MESH_SHADING_EXT]);
    let mut extension = Vec::new();
    extension.extend(Builder::literal_string("SPV_EXT_mesh_shader"));
    b.header(op::EXTENSION, &extension);
    b.header(op::MEMORY_MODEL, &[addressing::LOGICAL, memory::GLSL450]);

    let mut entry = vec![execution::MESH_EXT, main.0];
    entry.extend(Builder::literal_string("main"));
    entry.extend([vertices.0, colours.0, indices.0]);
    b.header(op::ENTRY_POINT, &entry);
    b.header(op::EXECUTION_MODE, &[main.0, mode::LOCAL_SIZE, 1, 1, 1]);
    b.header(
        op::EXECUTION_MODE,
        &[main.0, mode::OUTPUT_VERTICES, VERTICES],
    );
    b.header(
        op::EXECUTION_MODE,
        &[main.0, mode::OUTPUT_PRIMITIVES_EXT, 1],
    );
    b.header(op::EXECUTION_MODE, &[main.0, mode::OUTPUT_TRIANGLES_EXT]);

    // The per-vertex outputs are a block, and the position is a member of it. A bare array of
    // `vec4` decorated `Position` is what a vertex shader has and is not what this stage takes.
    b.annotate(op::DECORATE, &[per_vertex.0, decoration::BLOCK]);
    b.annotate(
        op::MEMBER_DECORATE,
        &[per_vertex.0, 0, decoration::BUILT_IN, built_in::POSITION],
    );
    b.annotate(op::DECORATE, &[colours.0, decoration::LOCATION, 0]);
    b.annotate(
        op::DECORATE,
        &[
            indices.0,
            decoration::BUILT_IN,
            built_in::PRIMITIVE_TRIANGLE_INDICES_EXT,
        ],
    );

    b.declare(op::TYPE_VOID, &[void.0]);
    b.declare(op::TYPE_FUNCTION, &[fn_type.0, void.0]);
    b.declare(op::TYPE_FLOAT, &[f32_type.0, 32]);
    b.declare(op::TYPE_INT, &[u32_type.0, 32, 0]);
    b.declare(op::TYPE_VECTOR, &[vec4.0, f32_type.0, 4]);
    b.declare(op::TYPE_VECTOR, &[uvec3.0, u32_type.0, 3]);

    b.declare(op::CONSTANT, &[u32_type.0, zero.0, 0]);
    b.declare(op::CONSTANT, &[u32_type.0, one.0, 1]);
    b.declare(op::CONSTANT, &[u32_type.0, two.0, 2]);
    b.declare(op::CONSTANT, &[u32_type.0, three.0, VERTICES]);

    b.declare(op::TYPE_STRUCT, &[per_vertex.0, vec4.0]);
    b.declare(op::TYPE_ARRAY, &[vertex_array.0, per_vertex.0, three.0]);
    b.declare(
        op::TYPE_POINTER,
        &[vertex_array_ptr.0, storage::OUTPUT, vertex_array.0],
    );
    b.declare(
        op::VARIABLE,
        &[vertex_array_ptr.0, vertices.0, storage::OUTPUT],
    );

    b.declare(op::TYPE_ARRAY, &[colour_array.0, vec4.0, three.0]);
    b.declare(
        op::TYPE_POINTER,
        &[colour_array_ptr.0, storage::OUTPUT, colour_array.0],
    );
    b.declare(
        op::VARIABLE,
        &[colour_array_ptr.0, colours.0, storage::OUTPUT],
    );

    b.declare(op::TYPE_ARRAY, &[index_array.0, uvec3.0, one.0]);
    b.declare(
        op::TYPE_POINTER,
        &[index_array_ptr.0, storage::OUTPUT, index_array.0],
    );
    b.declare(
        op::VARIABLE,
        &[index_array_ptr.0, indices.0, storage::OUTPUT],
    );

    b.declare(op::TYPE_POINTER, &[output_vec4.0, storage::OUTPUT, vec4.0]);
    b.declare(
        op::TYPE_POINTER,
        &[output_uvec3.0, storage::OUTPUT, uvec3.0],
    );
    b.declare(
        op::CONSTANT_COMPOSITE,
        &[uvec3.0, triangle.0, zero.0, one.0, two.0],
    );

    // The corner positions and their colours, as constants, one composite each.
    let positions = [
        [-1.0f32, -1.0, 0.0, 1.0],
        [3.0, -1.0, 0.0, 1.0],
        [-1.0, 3.0, 0.0, 1.0],
    ];
    let mut position_ids = Vec::new();
    let mut colour_ids = Vec::new();
    for (place, colour) in positions.into_iter().zip(corners) {
        position_ids.push(constant_vec4(&mut b, f32_type, vec4, place));
        colour_ids.push(constant_vec4(&mut b, f32_type, vec4, colour));
    }

    b.function(op::FUNCTION, &[void.0, main.0, 0, fn_type.0]);
    b.function(op::LABEL, &[entry_block.0]);

    // **Before anything is written**: three vertices, one primitive. A mesh shader that wrote
    // outputs it had not declared would be writing past what the stage allocated for it.
    b.function(op::SET_MESH_OUTPUTS_EXT, &[three.0, one.0]);

    let slots = [zero, one, two];
    for (slot, (place, colour)) in slots
        .into_iter()
        .zip(position_ids.into_iter().zip(colour_ids))
    {
        let position_ptr = b.id();
        b.function(
            op::ACCESS_CHAIN,
            &[output_vec4.0, position_ptr.0, vertices.0, slot.0, zero.0],
        );
        b.function(op::STORE, &[position_ptr.0, place.0]);

        let colour_ptr = b.id();
        b.function(
            op::ACCESS_CHAIN,
            &[output_vec4.0, colour_ptr.0, colours.0, slot.0],
        );
        b.function(op::STORE, &[colour_ptr.0, colour.0]);
    }

    let index_ptr = b.id();
    b.function(
        op::ACCESS_CHAIN,
        &[output_uvec3.0, index_ptr.0, indices.0, zero.0],
    );
    b.function(op::STORE, &[index_ptr.0, triangle.0]);

    b.function(op::RETURN, &[]);
    b.function(op::FUNCTION_END, &[]);
    b.finish()
}

/// How many vertices the mesh oracle emits. Three, because it draws one triangle.
const VERTICES: u32 = 3;

/// One `vec4` constant, from four floats.
fn constant_vec4(b: &mut Builder, f32_type: Id, vec4: Id, value: [f32; 4]) -> Id {
    let components: Vec<Id> = value
        .into_iter()
        .map(|component| {
            let id = b.id();
            b.declare(op::CONSTANT, &[f32_type.0, id.0, component.to_bits()]);
            id
        })
        .collect();
    let composite = b.id();
    let mut operands = vec![vec4.0, composite.0];
    operands.extend(components.iter().map(|id| id.0));
    b.declare(op::CONSTANT_COMPOSITE, &operands);
    composite
}

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

    /// The index of the first instruction with `opcode`, walking the word stream past the
    /// five-word module header. `None` if it is not emitted.
    fn instruction_index(words: &[u32], opcode: u16) -> Option<usize> {
        let mut i = 5;
        while i < words.len() {
            let count = (words[i] >> 16) as usize;
            if count == 0 {
                break;
            }
            if (words[i] & 0xFFFF) as u16 == opcode {
                return Some(i);
            }
            i += count;
        }
        None
    }

    #[test]
    fn an_extended_set_is_imported_before_the_memory_model_whatever_the_call_order() {
        // OpExtInstImport must precede OpMemoryModel in the logical layout (SPIR-V 2.4). The
        // builder places it by slot, so the import lands ahead of the memory model even though
        // this asks for them the other way round - and spirv-val rejects the reverse with a
        // layout error that names neither instruction.
        let mut b = Builder::new();
        b.header(op::CAPABILITY, &[super::capability::SHADER]);
        b.header(
            op::MEMORY_MODEL,
            &[super::addressing::LOGICAL, super::memory::GLSL450],
        );
        let set = b.ext_inst_import("GLSL.std.450");
        assert_eq!(set.0, 1, "the imported set takes the first identifier");

        let words = b.finish();
        let import_at =
            instruction_index(&words, op::EXT_INST_IMPORT).expect("the import is emitted");
        let model_at =
            instruction_index(&words, op::MEMORY_MODEL).expect("the memory model is emitted");
        assert!(
            import_at < model_at,
            "the extended-set import must precede the memory model"
        );
        b.check().expect("identifiers resolve");
    }

    #[test]
    fn an_ext_inst_carries_result_type_result_set_instruction_and_operands_in_order() {
        // The word order OpExtInst requires: result type, result, set, the instruction number
        // as a literal, then the operand ids. A transposition here validates as a different
        // instruction or a wrong-typed one, so it is pinned rather than trusted.
        let mut b = Builder::new();
        let float = b.id();
        let value = b.id();
        let set = b.ext_inst_import("GLSL.std.450");
        // GLSLstd450 Sqrt is instruction 31.
        let result = b.ext_inst(float, set, 31, &[value]);
        assert!(result.0 > set.0, "the result is a fresh identifier");

        let words = b.finish();
        let at = instruction_index(&words, op::EXT_INST).expect("the ext-inst is emitted");
        assert_eq!(
            &words[at + 1..at + 6],
            &[float.0, result.0, set.0, 31, value.0]
        );
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
