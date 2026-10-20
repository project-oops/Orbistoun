//! Building SPIR-V modules.
//!
//! This crate emits SPIR-V and knows nothing about the guest: translation above it maps guest
//! semantics onto what is here. Modules are built as 32-bit words directly, with no assembler, so
//! the translator runs wherever it is needed. [`Builder`] allocates every identifier, so the
//! header's bound always exceeds them. Structural properties are unit-tested here; validity is
//! decided by `spirv-val`, run over emitted modules by `tools/validate-spirv.sh`.

use core::fmt;
use std::collections::BTreeMap;

/// First word of every module.
pub const MAGIC: u32 = 0x0723_0203;

/// Version 1.0, as the format packs it: minor in the second byte. Accepted by every consumer, so
/// anything expressible in 1.0 is declared as 1.0.
pub const VERSION_1_0: u32 = 0x0001_0000;

/// Version 1.3.
///
/// Needed for the `StorageBuffer` storage class; the 1.0 spelling (`Uniform` plus `BufferBlock`) is
/// deprecated. A module without a storage buffer stays at 1.0.
pub const VERSION_1_3: u32 = 0x0001_0300;

/// Version 1.4, which the mesh-shader extension requires.
///
/// Declared only where needed: from 1.4 an entry point lists every global variable in its
/// interface, not only inputs and outputs, and an omission fails validation without naming the
/// variable.
pub const VERSION_1_4: u32 = 0x0001_0400;

/// Generator identifier. Zero means unregistered; the registry is for tool vendors.
pub const GENERATOR: u32 = 0;

/// An identifier for a result within a module.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Id(pub u32);

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "%{}", self.0)
    }
}

/// The opcodes this crate emits, named rather than numbered at the call site. A wrong value is
/// rejected by name by `spirv-val`.
pub mod op {
    /// Declares a capability the module needs.
    pub const CAPABILITY: u16 = 17;
    /// Declares a SPIR-V extension the module relies on, by name.
    ///
    /// The float controls that keep subnormals were an extension before 1.4, and naming the
    /// extension avoids raising the SPIR-V and Vulkan versions.
    pub const EXTENSION: u16 = 10;
    /// Imports an extended instruction set (e.g. `GLSL.std.450`), binding it to a result id.
    pub const EXT_INST_IMPORT: u16 = 11;
    /// Invokes an instruction from an imported extended set: the transcendentals and min/max/abs
    /// that have no core opcode.
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
    pub const TYPE_VECTOR: u16 = 23;
    /// Pulls one component out of a composite by a literal index.
    pub const COMPOSITE_EXTRACT: u16 = 81;
    /// The set of invocations in a subgroup for which a condition holds, as a bit mask.
    ///
    /// Turns each invocation's per-lane `bool` into the mask word the guest's scalar instructions
    /// read.
    pub const GROUP_NON_UNIFORM_BALLOT: u16 = 339;
    /// Another invocation's value of an operand, by that invocation's index in the subgroup.
    ///
    /// Reads one lane's register from another when one invocation is one lane.
    pub const GROUP_NON_UNIFORM_SHUFFLE: u16 = 345;
    /// Another invocation's value of an operand, by its index within this invocation's quad: in a
    /// fragment shader, the pixels of a 2x2 block, top-left 0, top-right 1, bottom-left 2,
    /// bottom-right 3.
    ///
    /// Reads a pixel's neighbour where a guest instruction permutes a quad's lanes.
    pub const GROUP_NON_UNIFORM_QUAD_BROADCAST: u16 = 365;
    /// Waits until every invocation of the scope reaches it, making the memory the semantics name
    /// visible across them.
    pub const CONTROL_BARRIER: u16 = 224;
    /// Ands a value into the word a pointer names, as one indivisible step.
    pub const ATOMIC_AND: u16 = 240;
    /// Ors a value into the word a pointer names, as one indivisible step.
    pub const ATOMIC_OR: u16 = 241;
    /// Adds a value to the word a pointer names, as one indivisible step, answering what it held.
    pub const ATOMIC_I_ADD: u16 = 234;
    /// Keeps the unsigned minimum of the word a pointer names and a value, as one indivisible step,
    /// answering what it held.
    pub const ATOMIC_U_MIN: u16 = 237;
    /// Keeps the unsigned maximum of the word a pointer names and a value, as one indivisible step,
    /// answering what it held.
    pub const ATOMIC_U_MAX: u16 = 239;
    /// Keeps the signed minimum of the word a pointer names and a value, as one indivisible step,
    /// answering what it held.
    pub const ATOMIC_S_MIN: u16 = 236;
    /// Keeps the signed maximum of the word a pointer names and a value, as one indivisible step,
    /// answering what it held.
    pub const ATOMIC_S_MAX: u16 = 238;
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
    /// Ends a fragment invocation without writing its outputs: the pixel is discarded.
    pub const KILL: u16 = 252;
    /// Unconditional branch.
    pub const BRANCH: u16 = 249;
    /// Branch on a boolean.
    pub const BRANCH_CONDITIONAL: u16 = 250;
    /// Multi-way branch on an integer.
    pub const SWITCH: u16 = 251;
    /// Declares the merge and continue targets of a loop.
    ///
    /// Must immediately precede the block's branch; that is what makes a backward branch a loop.
    pub const LOOP_MERGE: u16 = 246;
    /// Declares where a selection converges.
    pub const SELECTION_MERGE: u16 = 247;
    /// Integer equality, producing a boolean.
    pub const IEQUAL: u16 = 170;
    /// A constant.
    pub const CONSTANT: u16 = 43;
    /// A scalar constant a pipeline may replace when it is created, by the `SpecId` it is
    /// decorated with; the literal is its value otherwise.
    pub const SPEC_CONSTANT: u16 = 50;
    /// A constant aggregate (a vector, or an array of them) from constant parts.
    pub const CONSTANT_COMPOSITE: u16 = 44;
    /// An aggregate built at run time from values; an export builds its `vec4` colour this way.
    pub const COMPOSITE_CONSTRUCT: u16 = 80;
    /// A fixed-length array type.
    pub const TYPE_ARRAY: u16 = 28;
    /// An array whose length is the bound buffer's, the last member of a storage block.
    pub const TYPE_RUNTIME_ARRAY: u16 = 29;
    /// How many elements a storage block's runtime array holds, as the bound buffer has them.
    pub const ARRAY_LENGTH: u16 = 68;
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
    /// Declares how many vertices and primitives a mesh workgroup will emit.
    ///
    /// Two id operands. Everything the shader writes afterwards is within this; it corresponds to a
    /// guest's `MSG_GS_ALLOC_REQ`.
    pub const SET_MESH_OUTPUTS_EXT: u16 = 5295;
    /// A texture: the element type, its dimensionality, and how it is used.
    pub const TYPE_IMAGE: u16 = 25;
    /// An image paired with the sampler that reads it.
    pub const TYPE_SAMPLED_IMAGE: u16 = 27;
    /// Samples a texture, with the implementation choosing the level of detail from the
    /// coordinate's derivatives, which only a fragment stage has.
    pub const IMAGE_SAMPLE_IMPLICIT_LOD: u16 = 87;
    /// Samples a texture at a level of detail the instruction names.
    ///
    /// The form a guest's `image_sample_lz` asks for, and the one available without derivatives.
    /// Carries a literal operand mask after the coordinate, then the level.
    pub const IMAGE_SAMPLE_EXPLICIT_LOD: u16 = 88;
    /// Samples a depth texture at a level the instruction names, compared against a reference: the
    /// filtered result of the comparison, one float. A guest's `image_sample_c_lz` (D816).
    ///
    /// 90: the Vulkan SDK's `spirv-as` assembles `OpImageSampleDrefExplicitLod` as `0x0008005a`.
    pub const IMAGE_SAMPLE_DREF_EXPLICIT_LOD: u16 = 90;
    /// Samples a depth texture at the level its derivatives choose, compared against a reference.
    /// A guest's `image_sample_c` in a pixel shader. 89, `0x00060059` for the plain form.
    pub const IMAGE_SAMPLE_DREF_IMPLICIT_LOD: u16 = 89;
    /// Reads one texel by its integer coordinate, with no sampler involved.
    ///
    /// What a guest's `image_load` is. Carries a literal operand mask like the explicit sampling
    /// form; Vulkan requires the level for a non-multisampled image, so it always carries one.
    pub const IMAGE_FETCH: u16 = 95;
    /// One component of the four texels a sample at a position would filter (D913): a guest's
    /// `image_gather4`. Result type, result, sampled image, coordinate, component.
    pub const IMAGE_GATHER: u16 = 96;
    /// The image inside a sampled image, so it can be fetched from rather than sampled.
    ///
    /// A descriptor binds image and sampler together and a fetch takes the image alone, so reading
    /// a texel needs no binding of its own.
    pub const IMAGE: u16 = 100;
    /// `OpImageQuerySizeLod`: an image's extent at a level, which needs the `ImageQuery` capability.
    pub const IMAGE_QUERY_SIZE_LOD: u16 = 103;
    /// `OpImageQuerySize`: a storage image's extent, an array's layers last, which needs the
    /// `ImageQuery` capability.
    pub const IMAGE_QUERY_SIZE: u16 = 104;
    /// Writes one texel of a storage image by its integer coordinate.
    ///
    /// What a guest's `image_store` is. It has no result: an image write is an effect.
    pub const IMAGE_WRITE: u16 = 99;
    /// Builds a vector from components of two others, by index.
    pub const VECTOR_SHUFFLE: u16 = 79;
    /// Reads through a pointer.
    pub const LOAD: u16 = 61;
    /// Reinterprets a value's bits as another type of the same width.
    ///
    /// Not a conversion: a register holds thirty-two bits and the instruction decides how to read
    /// them, so float arithmetic on registers bitcasts.
    pub const BITCAST: u16 = 124;
    /// Integer addition, for address arithmetic.
    pub const IADD: u16 = 128;
    /// Integer subtraction.
    pub const ISUB: u16 = 130;
    /// `OpUDiv`.
    pub const UDIV: u16 = 134;
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
    /// Converts a float to an unsigned integer, rounding toward zero; undefined out of range.
    pub const CONVERT_F_TO_U: u16 = 109;
    /// Converts a float to a signed integer, rounding toward zero; undefined out of range.
    pub const CONVERT_F_TO_S: u16 = 110;
    /// Converts a signed integer to the float of the same value.
    pub const CONVERT_S_TO_F: u16 = 111;
    /// Converts an unsigned integer to the float of the same value.
    pub const CONVERT_U_TO_F: u16 = 112;
    /// Converts an unsigned integer to another width; truncating narrows a packed field to the
    /// sixteen bits a half occupies.
    pub const UCONVERT: u16 = 113;
    /// Converts a float to another width, widening a half to a single-precision float.
    pub const FCONVERT: u16 = 115;
    /// Chooses between two values without branching.
    ///
    /// A masked write keeps the old value where the mask says the lane is inactive.
    pub const SELECT: u16 = 169;
    /// Unsigned right shift.
    pub const SHIFT_RIGHT_LOGICAL: u16 = 194;
    /// Signed right shift: the sign bit fills the vacated high bits, which sign-extends a narrow
    /// signed field to a full word.
    pub const SHIFT_RIGHT_ARITHMETIC: u16 = 195;
    /// Left shift.
    pub const SHIFT_LEFT_LOGICAL: u16 = 196;
    /// Unsigned integer less-than.
    pub const ULESS_THAN: u16 = 176;
    /// Unsigned integer less-than-or-equal.
    pub const ULESS_THAN_EQUAL: u16 = 178;
    /// Unsigned integer greater-than.
    pub const UGREATER_THAN: u16 = 172;
    /// Unsigned integer greater-than-or-equal.
    ///
    /// The buffer bounds checks are unsigned: a record count and a byte offset are magnitudes, and
    /// a signed comparison would treat an offset above two billion as negative and in range.
    pub const UGREATER_THAN_EQUAL: u16 = 174;
    /// Signed integer less-than. Agrees with the unsigned form except where an operand is negative.
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
    /// Reverses the bits of an integer.
    pub const BIT_REVERSE: u16 = 204;
    /// Integer inequality, producing a boolean.
    pub const INOT_EQUAL: u16 = 171;
    /// Ordered float equality.
    ///
    /// Ordered means false when either operand is a NaN, as the guest's comparison is. The
    /// unordered forms answer true.
    pub const FORD_EQUAL: u16 = 180;
    /// Unordered float equality: true when the operands are equal or either is a NaN.
    pub const FUNORD_EQUAL: u16 = 181;
    /// Ordered float inequality: false when either operand is a NaN.
    pub const FORD_NOT_EQUAL: u16 = 182;
    /// Unordered float inequality: true when the operands differ or either is a NaN.
    pub const FUNORD_NOT_EQUAL: u16 = 183;
    /// Unordered float less-than: true when either operand is a NaN.
    pub const FUNORD_LESS_THAN: u16 = 185;
    /// Unordered float greater-than: true when either operand is a NaN.
    pub const FUNORD_GREATER_THAN: u16 = 187;
    /// Unordered float less-than-or-equal: true when either operand is a NaN.
    pub const FUNORD_LESS_THAN_EQUAL: u16 = 189;
    /// Unordered float greater-than-or-equal: true when either operand is a NaN.
    pub const FUNORD_GREATER_THAN_EQUAL: u16 = 191;
    /// Ordered float less-than.
    pub const FORD_LESS_THAN: u16 = 184;
    /// Ordered float greater-than.
    pub const FORD_GREATER_THAN: u16 = 186;
    /// Whether a float is a NaN.
    ///
    /// Preferred over comparing a value with itself, which a compiler assuming no NaNs may fold
    /// away.
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
    /// A private variable's initialiser: without one its contents are undefined at entry.
    pub const CONSTANT_NULL: u16 = 46;
    /// The boolean `true`.
    pub const CONSTANT_TRUE: u16 = 41;
}

/// The `SPV_KHR_float_controls` extension, by name.
///
/// How a module states that its arithmetic depends on subnormals being preserved rather than
/// flushed to zero.
pub const FLOAT_CONTROLS: &str = "SPV_KHR_float_controls";

/// Storage classes.
pub mod storage {
    /// A buffer the shader may read and write. Needs version 1.3 or later.
    pub const STORAGE_BUFFER: u32 = 12;
    /// Read-only, supplied by the pipeline rather than by the shader.
    ///
    /// Built-in variables live here. An input variable must be listed in the entry point's
    /// interface, or the module is rejected.
    pub const INPUT: u32 = 1;
    /// Written by the shader and read by the next stage, or by the framebuffer.
    ///
    /// Where a vertex shader puts its position and a fragment shader its colour. Like [`INPUT`], an
    /// output variable must appear in the entry point's interface.
    pub const OUTPUT: u32 = 3;
    /// Read-only storage the pipeline binds: images, samplers, and the two together.
    ///
    /// A sampled image is bound through a descriptor and read by a sampling instruction rather than
    /// loaded, so it has its own storage class.
    pub const UNIFORM_CONSTANT: u32 = 0;
    /// A small block the host writes per draw with a command rather than a buffer (SPIR-V
    /// `StorageClass` 9). How the user data a guest hands each draw reaches its shaders.
    pub const PUSH_CONSTANT: u32 = 9;
    /// Module-scope storage private to one invocation.
    ///
    /// Used for a constant table a shader indexes: a composite constant cannot be indexed
    /// dynamically, but a `Private` variable initialised with one can.
    pub const PRIVATE: u32 = 6;
}

/// The optional operands a sampling instruction may carry, as a mask.
///
/// Taken from compiled output: a GLSL `textureLod` call disassembles to mask `2` followed by the
/// level.
pub mod image_operands {
    /// A level of detail follows the mask.
    pub const LOD: u32 = 2;
    /// The coordinate's derivatives across and down a pixel follow the mask, choosing the level
    /// (SPIR-V `Grad`, image operand `0x4`).
    pub const GRAD: u32 = 4;
}

/// Decorations.
pub mod decoration {
    /// The number a pipeline names a specialization constant by when it replaces it.
    pub const SPEC_ID: u32 = 1;
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
    /// Location zero of a fragment shader's colour output is colour attachment zero, a render
    /// pass's first attachment.
    pub const LOCATION: u32 = 30;
    /// No interpolation: every fragment of a primitive reads the provoking vertex's value.
    ///
    /// What the guest's parameter-move instruction asks for; without it the attribute is
    /// interpolated, which differs everywhere except at one vertex.
    pub const FLAT: u32 = 14;
    /// Interpolation linear in screen space rather than perspective-correct: what the guest's
    /// `LINEAR_*` barycentrics give a `noperspective` attribute.
    pub const NO_PERSPECTIVE: u32 = 13;
    /// Marks a storage image the module only ever writes.
    ///
    /// The reference compiler emits it for a write-only image, and it holds for everything stored
    /// to here: a guest's `image_store` writes, and reads go through the sampled binding.
    pub const NON_READABLE: u32 = 25;
    /// Marks memory the module only ever reads: a draw's bound buffers.
    pub const NON_WRITABLE: u32 = 24;
    /// Which descriptor set.
    pub const DESCRIPTOR_SET: u32 = 34;
    /// Byte offset of a structure member.
    pub const OFFSET: u32 = 35;
    /// The result is computed by its own operation alone: a driver may not contract it with
    /// another into one rounding, such as a multiply and an add into a fused multiply-add.
    pub const NO_CONTRACTION: u32 = 42;
    /// Marks a mesh shader output as one value per primitive rather than per vertex.
    pub const PER_PRIMITIVE_EXT: u32 = 5271;
}

/// Capability values.
pub mod capability {
    /// Shader stages. The baseline for anything graphics or compute.
    pub const SHADER: u32 = 1;
    /// Writing clip distances. A device without the `shaderClipDistance` feature refuses a module
    /// declaring this.
    pub const CLIP_DISTANCE: u32 = 32;
    /// The geometry stage's built-ins, which include a fragment shader reading its `Layer`.
    ///
    /// A device without the `geometryShader` feature refuses a module declaring this.
    pub const GEOMETRY: u32 = 2;
    /// Permits writing to a storage image whose format the module does not declare.
    ///
    /// A guest's format lives in a descriptor that is not decoded, so the module says `Unknown` and
    /// the format is whatever the pipeline bound (D692). A device without
    /// `shaderStorageImageWriteWithoutFormat` refuses a module declaring this.
    pub const STORAGE_IMAGE_WRITE_WITHOUT_FORMAT: u32 = 56;
    /// Mesh and task stages, from `SPV_EXT_mesh_shader`.
    ///
    /// It implies [`SHADER`], so a mesh module declares this alone, as the reference compiler does.
    pub const MESH_SHADING_EXT: u32 = 5283;
    /// Permits a module to require that subnormal results are kept, not flushed.
    ///
    /// From `SPV_KHR_float_controls`. A device that does not offer it refuses the module rather
    /// than computing zero where the guest expected a subnormal.
    pub const DENORM_PRESERVE: u32 = 4464;
    /// Permits a module to require that signed zeros, infinities and NaNs survive its arithmetic.
    ///
    /// From `SPV_KHR_float_controls`. Without it a driver may assume no infinity or NaN occurs and
    /// fold them away; the guest's hardware keeps them.
    pub const SIGNED_ZERO_INF_NAN_PRESERVE: u32 = 4466;
    /// Querying an image's size, for a translated sample whose texel offsets are scaled by it.
    pub const IMAGE_QUERY: u32 = 50;
    /// Invocations may ask about their subgroup at all.
    pub const GROUP_NON_UNIFORM: u32 = 61;
    /// Invocations may take a ballot across their subgroup.
    pub const GROUP_NON_UNIFORM_BALLOT: u32 = 64;
    /// Invocations may read each other's values by index.
    pub const GROUP_NON_UNIFORM_SHUFFLE: u32 = 65;
    /// Invocations may read each other's values within their quad.
    pub const GROUP_NON_UNIFORM_QUAD: u32 = 68;
    /// 16-bit floating-point types and arithmetic.
    ///
    /// Widens a packed half through a real 16-bit float type, using the driver's IEEE conversion
    /// for subnormals and infinities.
    pub const FLOAT16: u32 = 9;
    /// 16-bit integer types, for narrowing a packed field to the width of a half before it is read
    /// as one.
    pub const INT16: u32 = 22;
}

/// Execution scopes, for the group operations.
pub mod scope {
    /// The subgroup: the invocations the hardware runs in lockstep.
    ///
    /// A scope is an identifier operand in the encoding, so it is declared as a constant like any
    /// other value.
    pub const SUBGROUP: u32 = 3;
    /// The workgroup: every invocation of one dispatch group.
    pub const WORKGROUP: u32 = 2;
    /// The device: every invocation of the dispatch, across all its groups.
    pub const DEVICE: u32 = 1;
}

/// Memory semantics, for the barriers.
pub mod semantics {
    /// Earlier writes become visible to later reads, both ways across the barrier.
    pub const ACQUIRE_RELEASE: u32 = 0x8;
    /// The ordering applies to workgroup memory.
    pub const WORKGROUP_MEMORY: u32 = 0x100;
    /// The ordering applies to storage buffers.
    pub const UNIFORM_MEMORY: u32 = 0x40;
}

/// Built-in variables, by their decoration value.
pub mod built_in {
    /// This invocation's index within its subgroup: the guest's lane number when one invocation is
    /// one lane.
    pub const SUBGROUP_LOCAL_INVOCATION_ID: u32 = 41;
    /// Which subgroup of its workgroup this invocation is in: the guest's wave, where a dispatch's
    /// waves are subgroups (D884).
    pub const SUBGROUP_ID: u32 = 40;
    /// This invocation's index within its workgroup, flattened: the guest's lane number when one
    /// invocation is one lane and the wave is wider than a subgroup.
    pub const LOCAL_INVOCATION_INDEX: u32 = 29;
    /// This workgroup's index within its dispatch (a `uvec3`): which draw of a batch a mesh
    /// workgroup is (D718).
    pub const WORKGROUP_ID: u32 = 26;
    /// This invocation's index within the whole dispatch (a `uvec3`).
    pub const GLOBAL_INVOCATION_ID: u32 = 28;
    /// The clip-space position a vertex shader writes.
    pub const POSITION: u32 = 0;
    /// A point's size in pixels, which a stage before rasterisation writes per vertex.
    pub const POINT_SIZE: u32 = 1;
    /// The distances a vertex lies inside each clip plane (a `float` array): the primitive is
    /// clipped where one is negative.
    pub const CLIP_DISTANCE: u32 = 3;
    /// The depth a fragment shader writes in place of the rasterised one (a `float`).
    pub const FRAG_DEPTH: u32 = 22;
    /// The samples a fragment shader keeps of those it covers (an array of one `uint` up to 32
    /// samples): a sample whose bit is clear is not written.
    pub const SAMPLE_MASK: u32 = 20;
    /// A mesh shader's point index array: one vertex index per primitive (a `uint`).
    pub const PRIMITIVE_POINT_INDICES_EXT: u32 = 5294;
    /// A mesh shader's line index array: two vertex indices per primitive (a `uvec2`).
    pub const PRIMITIVE_LINE_INDICES_EXT: u32 = 5295;
    /// A mesh shader's triangle index array: three vertex indices per primitive (a `uvec3`).
    pub const PRIMITIVE_TRIANGLE_INDICES_EXT: u32 = 5296;
    /// A mesh shader's per-primitive flag that discards the primitive (a `bool`).
    pub const CULL_PRIMITIVE_EXT: u32 = 5299;
    /// Which vertex of the draw this invocation is.
    ///
    /// Signed in Vulkan's environment: the variable is declared `int`, and an unsigned declaration
    /// is rejected.
    pub const VERTEX_INDEX: u32 = 42;
    /// A fragment's window position (a `vec4`): pixel centre in `x` and `y`, depth in `z`, and the
    /// reciprocal of clip-space `w` in `w`.
    pub const FRAG_COORD: u32 = 15;
    /// Whether the fragment's primitive faces the viewer (a `bool`).
    pub const FRONT_FACING: u32 = 17;
    /// The framebuffer layer a fragment is rendered into (a 32-bit integer).
    pub const LAYER: u32 = 9;
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
    /// The stage a guest's primitive shader corresponds to (D688): it declares how much it will
    /// emit before emitting any of it, as the guest's allocation request does.
    pub const MESH_EXT: u32 = 5365;
}

/// Execution modes.
pub mod mode {
    /// Fragment shaders declare their origin convention.
    pub const ORIGIN_UPPER_LEFT: u32 = 7;
    /// A fragment shader that writes `FragDepth` declares it replaces the rasterised depth.
    pub const DEPTH_REPLACING: u32 = 12;
    /// Compute shaders declare their workgroup size.
    pub const LOCAL_SIZE: u32 = 17;
    /// Subnormal results of the given width are preserved rather than flushed.
    ///
    /// Takes the bit width as its literal operand, so a module can ask for 32 bits alone.
    pub const DENORM_PRESERVE: u32 = 4459;
    /// Signed zeros, infinities and NaNs of the given width are preserved. Takes the bit width.
    pub const SIGNED_ZERO_INF_NAN_PRESERVE: u32 = 4461;
    /// The most vertices a mesh shader's workgroup will emit. One literal operand.
    pub const OUTPUT_VERTICES: u32 = 26;
    /// The most primitives a mesh shader's workgroup will emit. One literal operand.
    pub const OUTPUT_PRIMITIVES_EXT: u32 = 5270;
    /// A mesh shader's primitives are points. No operand.
    pub const OUTPUT_POINTS: u32 = 27;
    /// A mesh shader's primitives are lines. No operand.
    pub const OUTPUT_LINES_EXT: u32 = 5269;
    /// A mesh shader's primitives are triangles. No operand.
    pub const OUTPUT_TRIANGLES_EXT: u32 = 5298;
}

/// A module without its `SignedZeroInfNanPreserve` declarations: the execution modes, the
/// capability and the float-controls extension, for a device that cannot honour them.
///
/// Every other instruction is kept word for word. A malformed stream is returned unchanged, so a
/// module this cannot read is refused by the device rather than altered here.
#[must_use]
pub fn without_inf_nan_preserve(words: &[u32]) -> Vec<u32> {
    let extension = Builder::literal_string(FLOAT_CONTROLS);
    let mut out = Vec::with_capacity(words.len());
    let Some(header) = words.get(..5) else {
        return words.to_vec();
    };
    out.extend_from_slice(header);
    let mut at = 5;
    while at < words.len() {
        let count = (words[at] >> 16) as usize;
        let opcode = (words[at] & 0xffff) as u16;
        let Some(instruction) = words.get(at..at + count).filter(|_| count > 0) else {
            return words.to_vec();
        };
        let drop = match opcode {
            op::CAPABILITY => instruction.get(1) == Some(&capability::SIGNED_ZERO_INF_NAN_PRESERVE),
            op::EXECUTION_MODE => instruction.get(2) == Some(&mode::SIGNED_ZERO_INF_NAN_PRESERVE),
            op::EXTENSION => instruction[1..] == extension[..],
            _ => false,
        };
        if !drop {
            out.extend_from_slice(instruction);
        }
        at += count;
    }
    out
}

/// How many ordered slots the header is kept in.
const HEADER_SLOTS: usize = 6;

/// Which slot a header instruction belongs in.
///
/// The format requires every capability before the memory model, which precedes the entry point,
/// which precedes the execution modes. The opcode decides the slot, so a capability declared after
/// the entry point is still emitted first.
const fn header_slot(opcode: u16) -> usize {
    match opcode {
        op::CAPABILITY => 0,
        op::EXTENSION => 1,
        // Extended-instruction imports follow the extensions and precede the memory model, as the
        // logical layout requires (SPIR-V 2.4).
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
    /// Every type the format allows once per module, by opcode and operands after the result, so
    /// [`Self::declare_type`] answers a second request with the first declaration.
    types: BTreeMap<(u16, Vec<u32>), Id>,
    next_id: u32,
    version: u32,
}

/// Whether the format allows one declaration of a type per module: every non-aggregate type.
///
/// Two `uvec3` declarations are invalid SPIR-V (`spirv-val`: "Duplicate non-aggregate type
/// declarations are not allowed"). One driver accepts the module and another faults compiling it,
/// so the builder holds the rule rather than every caller. Structs, arrays and pointers may repeat.
const fn is_unique_type(opcode: u16) -> bool {
    matches!(
        opcode,
        op::TYPE_VOID
            | op::TYPE_BOOL
            | op::TYPE_INT
            | op::TYPE_FLOAT
            | op::TYPE_VECTOR
            | op::TYPE_IMAGE
            | op::TYPE_SAMPLED_IMAGE
            | op::TYPE_FUNCTION
    )
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
            types: BTreeMap::new(),
            // Identifier zero is reserved by the format, so allocation starts at one.
            next_id: 1,
            version: VERSION_1_0,
        }
    }

    /// Declares a later version.
    ///
    /// Raised only when something in the module needs it; a higher version narrows what accepts the
    /// module.
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
    /// Its own section because the format requires every decoration to precede every type, which
    /// the builder guarantees regardless of call order.
    pub fn annotate(&mut self, opcode: u16, operands: &[u32]) {
        encode(&mut self.annotations, opcode, operands);
    }

    /// Appends a type, constant or global variable.
    ///
    /// Order within this section is preserved, because a type may name one declared before it.
    pub fn declare(&mut self, opcode: u16, operands: &[u32]) {
        if is_unique_type(opcode)
            && let Some((&result, rest)) = operands.split_first()
        {
            self.types
                .entry((opcode, rest.to_vec()))
                .or_insert(Id(result));
        }
        encode(&mut self.declarations, opcode, operands);
    }

    /// Declares a type the format allows once per module, or answers the one already declared.
    ///
    /// `operands` are the type's words after its result: `[component, count]` for a vector. A type
    /// declared twice is invalid SPIR-V that [`Self::check`] refuses, so any type more than one
    /// part of a module may want - a texture's coordinate, the `WorkgroupId` input's `uvec3` - is
    /// declared through here.
    pub fn declare_type(&mut self, opcode: u16, operands: &[u32]) -> Id {
        debug_assert!(is_unique_type(opcode), "opcode {opcode} may repeat");
        if let Some(&id) = self.types.get(&(opcode, operands.to_vec())) {
            return id;
        }
        let id = self.id();
        let mut words = Vec::with_capacity(operands.len() + 1);
        words.push(id.0);
        words.extend_from_slice(operands);
        self.declare(opcode, &words);
        id
    }

    /// Appends an instruction to the function section.
    pub fn function(&mut self, opcode: u16, operands: &[u32]) {
        encode(&mut self.functions, opcode, operands);
    }

    /// The function section so far, as encoded: what a reader of the instructions emitted since
    /// some point reads from.
    #[must_use]
    pub fn function_words(&self) -> &[u32] {
        &self.functions
    }

    /// Imports an extended instruction set by name (e.g. `"GLSL.std.450"`), returning the id
    /// [`Self::ext_inst`] refers to as its set.
    ///
    /// Placed in the header slot the logical layout requires, after the extensions and before the
    /// memory model. Call it once per set per module and keep the id; a second import is a second
    /// set.
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

    /// Encodes a string as the format does: NUL-terminated, packed four bytes to a word,
    /// little-endian.
    ///
    /// A string whose length is a multiple of four still gets a whole extra word of zeros as its
    /// terminator.
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
        // The bound must exceed every identifier in use, which allocation through the builder
        // guarantees.
        words.push(self.next_id);
        words.push(0); // schema, reserved
        // The order the format requires, enforced here rather than by callers.
        for slot in &self.header {
            words.extend_from_slice(slot);
        }
        words.extend_from_slice(&self.annotations);
        words.extend_from_slice(&self.declarations);
        words.extend_from_slice(&self.functions);
        words
    }

    /// Checks the identifiers in the module refer to something (D103).
    ///
    /// An identifier used but never defined leaves every instruction well-formed, and a driver
    /// handed such a module faults rather than diagnosing it. The builder hands out every
    /// identifier, so it can name the undefined ones. This checks three properties a builder is
    /// placed to check and nothing else; `spirv-val` is the validator.
    pub fn check(&self) -> Result<(), ModuleError> {
        let header: Vec<u32> = self.header.concat();
        let sections = [
            (header.as_slice(), true),
            (self.annotations.as_slice(), true),
            (self.declarations.as_slice(), false),
            // Function bodies forward-reference by necessity (a branch names a later label, a loop
            // header its merge block), so only the "defined somewhere" half applies.
            (self.functions.as_slice(), true),
        ];

        // Every opcode first: an unknown one makes the rest of the check unreliable, so it is
        // reported.
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

        // Forward references are legal in the header and the annotations: an entry point names a
        // function declared later, and a decoration names the variable it decorates. The
        // declarations and function bodies are also checked for order.
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

        // A non-aggregate type declared twice: valid word by word, refused by the format, and
        // fatal to a driver that indexes its types by shape.
        let mut types: BTreeMap<(u16, &[u32]), u32> = BTreeMap::new();
        for (opcode, operands) in Instructions::new(&self.declarations) {
            if !is_unique_type(opcode) {
                continue;
            }
            let Some((&second, rest)) = operands.split_first() else {
                continue;
            };
            if let Some(&first) = types.get(&(opcode, rest)) {
                return Err(ModuleError::DeclaredTwice {
                    first,
                    second,
                    opcode,
                });
            }
            types.insert((opcode, rest), second);
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
/// Exercises the emitter end to end without translation; if this fails to validate, the fault is in
/// the emitter.
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
/// Proves a dispatch runner: if the value does not come back, the fault is in the harness rather
/// than in anything translated. One invocation, one write.
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

    // An array stride of four and a member offset of zero declare the buffer tightly packed from
    // its start, as the runner allocates it.
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

/// The invocations in one group of [`stored_word_compaction_module`].
pub const COMPACTION_GROUP: u32 = 64;

/// Builds the compute shader that gathers a dispatch's stored words (D809) into a short list, so
/// that what crosses back to the host is the words stored rather than the whole buffer and its
/// write mask.
///
/// Set 0 binds the write mask at binding 0, the buffer at binding 1 and the list at binding 2. The
/// push constants are the buffer's word count, the list's capacity in pairs, and the words one row
/// of groups covers: a buffer needing more groups than one dimension may hold is dispatched as rows
/// of them. The list is a count, a flag, the buffer's first word, then the pairs. Each invocation
/// takes one word; a word its mask marks claims the next pair with an atomic add on the count, and
/// fills it with its index and its value when the pair is within the capacity, and a word stored
/// with a value other than the first word's sets the flag. A count past the capacity says the list
/// overflowed; a count of every word with the flag clear says the buffer was stored whole with one
/// value - a clear - whatever the capacity. The pairs are in no order.
/// Hand-written, as the oracles are (D549): it runs on the host's own results, not the guest's.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn stored_word_compaction_module() -> Vec<u32> {
    let mut b = Builder::new().with_version(VERSION_1_3);

    let void = b.id();
    let fn_type = b.id();
    let uint = b.id();
    let uvec3 = b.id();
    let boolean = b.id();
    let words = b.id();
    let block = b.id();
    let block_ptr = b.id();
    let list = b.id();
    let list_ptr = b.id();
    let word_ptr = b.id();
    let input_ptr = b.id();
    let invocation = b.id();
    let push = b.id();
    let push_ptr = b.id();
    let push_word_ptr = b.id();
    let constants = b.id();
    let mask = b.id();
    let buffer = b.id();
    let out = b.id();
    let [zero, one, two, three] = [b.id(), b.id(), b.id(), b.id()];
    let main = b.id();
    let [entry, within, marked, fits, end_fits, end_marked, end] =
        [b.id(), b.id(), b.id(), b.id(), b.id(), b.id(), b.id()];
    let [at_first, past_first, differs, past_differs] = [b.id(), b.id(), b.id(), b.id()];

    b.header(op::CAPABILITY, &[capability::SHADER]);
    b.header(op::MEMORY_MODEL, &[addressing::LOGICAL, memory::GLSL450]);
    let mut entry_point = vec![execution::GL_COMPUTE, main.0];
    entry_point.extend(Builder::literal_string("main"));
    entry_point.push(invocation.0);
    b.header(op::ENTRY_POINT, &entry_point);
    b.header(
        op::EXECUTION_MODE,
        &[main.0, mode::LOCAL_SIZE, COMPACTION_GROUP, 1, 1],
    );

    b.annotate(op::DECORATE, &[words.0, decoration::ARRAY_STRIDE, 4]);
    b.annotate(op::DECORATE, &[block.0, decoration::BLOCK]);
    b.annotate(op::MEMBER_DECORATE, &[block.0, 0, decoration::OFFSET, 0]);
    // The list: its count, the flag, the first word, then the pairs.
    b.annotate(op::DECORATE, &[list.0, decoration::BLOCK]);
    for (member, offset) in [(0, 0), (1, 4), (2, 8), (3, 12)] {
        b.annotate(
            op::MEMBER_DECORATE,
            &[list.0, member, decoration::OFFSET, offset],
        );
    }
    b.annotate(op::DECORATE, &[push.0, decoration::BLOCK]);
    b.annotate(op::MEMBER_DECORATE, &[push.0, 0, decoration::OFFSET, 0]);
    b.annotate(op::MEMBER_DECORATE, &[push.0, 1, decoration::OFFSET, 4]);
    b.annotate(op::MEMBER_DECORATE, &[push.0, 2, decoration::OFFSET, 8]);
    for (variable, binding) in [(mask, 0), (buffer, 1), (out, 2)] {
        b.annotate(op::DECORATE, &[variable.0, decoration::DESCRIPTOR_SET, 0]);
        b.annotate(op::DECORATE, &[variable.0, decoration::BINDING, binding]);
    }
    b.annotate(
        op::DECORATE,
        &[
            invocation.0,
            decoration::BUILT_IN,
            built_in::GLOBAL_INVOCATION_ID,
        ],
    );

    b.declare(op::TYPE_VOID, &[void.0]);
    b.declare(op::TYPE_FUNCTION, &[fn_type.0, void.0]);
    b.declare(op::TYPE_INT, &[uint.0, 32, 0]);
    b.declare(op::TYPE_VECTOR, &[uvec3.0, uint.0, 3]);
    b.declare(op::TYPE_BOOL, &[boolean.0]);
    b.declare(op::CONSTANT, &[uint.0, zero.0, 0]);
    b.declare(op::CONSTANT, &[uint.0, one.0, 1]);
    b.declare(op::CONSTANT, &[uint.0, two.0, 2]);
    b.declare(op::CONSTANT, &[uint.0, three.0, 3]);
    b.declare(op::TYPE_RUNTIME_ARRAY, &[words.0, uint.0]);
    b.declare(op::TYPE_STRUCT, &[block.0, words.0]);
    b.declare(op::TYPE_STRUCT, &[list.0, uint.0, uint.0, uint.0, words.0]);
    b.declare(op::TYPE_STRUCT, &[push.0, uint.0, uint.0, uint.0]);
    b.declare(
        op::TYPE_POINTER,
        &[block_ptr.0, storage::STORAGE_BUFFER, block.0],
    );
    b.declare(
        op::TYPE_POINTER,
        &[list_ptr.0, storage::STORAGE_BUFFER, list.0],
    );
    b.declare(
        op::TYPE_POINTER,
        &[word_ptr.0, storage::STORAGE_BUFFER, uint.0],
    );
    b.declare(op::TYPE_POINTER, &[input_ptr.0, storage::INPUT, uvec3.0]);
    b.declare(
        op::TYPE_POINTER,
        &[push_ptr.0, storage::PUSH_CONSTANT, push.0],
    );
    b.declare(
        op::TYPE_POINTER,
        &[push_word_ptr.0, storage::PUSH_CONSTANT, uint.0],
    );
    b.declare(
        op::VARIABLE,
        &[block_ptr.0, mask.0, storage::STORAGE_BUFFER],
    );
    b.declare(
        op::VARIABLE,
        &[block_ptr.0, buffer.0, storage::STORAGE_BUFFER],
    );
    b.declare(op::VARIABLE, &[list_ptr.0, out.0, storage::STORAGE_BUFFER]);
    b.declare(op::VARIABLE, &[input_ptr.0, invocation.0, storage::INPUT]);
    b.declare(
        op::VARIABLE,
        &[push_ptr.0, constants.0, storage::PUSH_CONSTANT],
    );

    let [id3, id, count_at, count, inside] = [b.id(), b.id(), b.id(), b.id(), b.id()];
    let [column, row, row_at, row_words, row_start] = [b.id(), b.id(), b.id(), b.id(), b.id()];
    let [flag_at, flag, is_marked] = [b.id(), b.id(), b.id()];
    let [counter, slot, capacity_at, capacity, room] = [b.id(), b.id(), b.id(), b.id(), b.id()];
    let [value_at, value, first, first_at, second, second_at] =
        [b.id(), b.id(), b.id(), b.id(), b.id(), b.id()];
    let [
        is_first,
        base_word_at,
        base_word,
        kept_at,
        other,
        flag_out,
        flagged,
    ] = [b.id(), b.id(), b.id(), b.id(), b.id(), b.id(), b.id()];

    b.function(op::FUNCTION, &[void.0, main.0, 0, fn_type.0]);
    b.function(op::LABEL, &[entry.0]);
    b.function(op::LOAD, &[uvec3.0, id3.0, invocation.0]);
    // The word: across the row of groups, then down the rows, as many words to a row as the third
    // push constant says - more groups than one dimension may hold are dispatched as rows.
    b.function(op::COMPOSITE_EXTRACT, &[uint.0, column.0, id3.0, 0]);
    b.function(op::COMPOSITE_EXTRACT, &[uint.0, row.0, id3.0, 1]);
    b.function(
        op::ACCESS_CHAIN,
        &[push_word_ptr.0, row_at.0, constants.0, two.0],
    );
    b.function(op::LOAD, &[uint.0, row_words.0, row_at.0]);
    b.function(op::IMUL, &[uint.0, row_start.0, row.0, row_words.0]);
    b.function(op::IADD, &[uint.0, id.0, row_start.0, column.0]);
    b.function(
        op::ACCESS_CHAIN,
        &[push_word_ptr.0, count_at.0, constants.0, zero.0],
    );
    b.function(op::LOAD, &[uint.0, count.0, count_at.0]);
    b.function(op::ULESS_THAN, &[boolean.0, inside.0, id.0, count.0]);
    b.function(op::SELECTION_MERGE, &[end.0, 0]);
    b.function(op::BRANCH_CONDITIONAL, &[inside.0, within.0, end.0]);

    // A word inside the buffer. The first word's invocation keeps its value in the list.
    b.function(op::LABEL, &[within.0]);
    b.function(
        op::ACCESS_CHAIN,
        &[word_ptr.0, base_word_at.0, buffer.0, zero.0, zero.0],
    );
    b.function(op::LOAD, &[uint.0, base_word.0, base_word_at.0]);
    b.function(op::IEQUAL, &[boolean.0, is_first.0, id.0, zero.0]);
    b.function(op::SELECTION_MERGE, &[past_first.0, 0]);
    b.function(
        op::BRANCH_CONDITIONAL,
        &[is_first.0, at_first.0, past_first.0],
    );
    b.function(op::LABEL, &[at_first.0]);
    b.function(op::ACCESS_CHAIN, &[word_ptr.0, kept_at.0, out.0, two.0]);
    b.function(op::STORE, &[kept_at.0, base_word.0]);
    b.function(op::BRANCH, &[past_first.0]);

    // Is it marked stored?
    b.function(op::LABEL, &[past_first.0]);
    b.function(
        op::ACCESS_CHAIN,
        &[word_ptr.0, flag_at.0, mask.0, zero.0, id.0],
    );
    b.function(op::LOAD, &[uint.0, flag.0, flag_at.0]);
    b.function(op::INOT_EQUAL, &[boolean.0, is_marked.0, flag.0, zero.0]);
    b.function(op::SELECTION_MERGE, &[end_marked.0, 0]);
    b.function(
        op::BRANCH_CONDITIONAL,
        &[is_marked.0, marked.0, end_marked.0],
    );

    // Marked. A value other than the first word's sets the flag - an atomic only where one differs,
    // so a clear costs none.
    b.function(op::LABEL, &[marked.0]);
    b.function(
        op::ACCESS_CHAIN,
        &[word_ptr.0, value_at.0, buffer.0, zero.0, id.0],
    );
    b.function(op::LOAD, &[uint.0, value.0, value_at.0]);
    b.function(op::INOT_EQUAL, &[boolean.0, other.0, value.0, base_word.0]);
    b.function(op::SELECTION_MERGE, &[past_differs.0, 0]);
    b.function(
        op::BRANCH_CONDITIONAL,
        &[other.0, differs.0, past_differs.0],
    );
    b.function(op::LABEL, &[differs.0]);
    b.function(op::ACCESS_CHAIN, &[word_ptr.0, flag_out.0, out.0, one.0]);
    b.function(
        op::ATOMIC_OR,
        &[uint.0, flagged.0, flag_out.0, one.0, zero.0, one.0],
    );
    b.function(op::BRANCH, &[past_differs.0]);

    // Claim a pair. Device scope, no memory ordering: only the count and the flag are shared, and
    // the list is read after the dispatch has finished.
    b.function(op::LABEL, &[past_differs.0]);
    b.function(op::ACCESS_CHAIN, &[word_ptr.0, counter.0, out.0, zero.0]);
    b.function(
        op::ATOMIC_I_ADD,
        &[uint.0, slot.0, counter.0, one.0, zero.0, one.0],
    );
    b.function(
        op::ACCESS_CHAIN,
        &[push_word_ptr.0, capacity_at.0, constants.0, one.0],
    );
    b.function(op::LOAD, &[uint.0, capacity.0, capacity_at.0]);
    b.function(op::ULESS_THAN, &[boolean.0, room.0, slot.0, capacity.0]);
    b.function(op::SELECTION_MERGE, &[end_fits.0, 0]);
    b.function(op::BRANCH_CONDITIONAL, &[room.0, fits.0, end_fits.0]);

    // Within the capacity: the pair is the word's index and its value.
    b.function(op::LABEL, &[fits.0]);
    b.function(op::IMUL, &[uint.0, first.0, slot.0, two.0]);
    b.function(
        op::ACCESS_CHAIN,
        &[word_ptr.0, first_at.0, out.0, three.0, first.0],
    );
    b.function(op::STORE, &[first_at.0, id.0]);
    b.function(op::IADD, &[uint.0, second.0, first.0, one.0]);
    b.function(
        op::ACCESS_CHAIN,
        &[word_ptr.0, second_at.0, out.0, three.0, second.0],
    );
    b.function(op::STORE, &[second_at.0, value.0]);
    b.function(op::BRANCH, &[end_fits.0]);

    b.function(op::LABEL, &[end_fits.0]);
    b.function(op::BRANCH, &[end_marked.0]);
    b.function(op::LABEL, &[end_marked.0]);
    b.function(op::BRANCH, &[end.0]);
    b.function(op::LABEL, &[end.0]);
    b.function(op::RETURN, &[]);
    b.function(op::FUNCTION_END, &[]);

    b.check()
        .expect("this crate built a module with identifiers that do not resolve");
    b.finish()
}

/// Builds a vertex shader that covers the whole framebuffer with one triangle.
///
/// Three vertices at `(-1, -1)`, `(3, -1)` and `(-1, 3)` in clip space cover every pixel, so the
/// fragment shader runs for all of them. Hand-written, because the oracle checks the translator
/// (D549). The positions are a constant table indexed by the vertex index, one `OpAccessChain` and
/// no arithmetic; a composite constant cannot be indexed dynamically, so the table is a `Private`
/// variable initialised with one.
#[must_use]
pub fn fullscreen_triangle_vertex_module() -> Vec<u32> {
    fullscreen_triangle_vertex_module_at_depth(0.0)
}

/// [`fullscreen_triangle_vertex_module`]'s triangle at clip-space depth `z`, so two of them at
/// different depths tell a depth test from draw order.
// A module is a linear sequence of declarations, each a local the next line needs.
#[allow(clippy::too_many_lines)]
#[must_use]
pub fn fullscreen_triangle_vertex_module_at_depth(z: f32) -> Vec<u32> {
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
    // Depth zero is the `zero` constant itself, so that module is unchanged by the parameter.
    let depth = if z.to_bits() == 0 { zero } else { b.id() };

    b.header(op::CAPABILITY, &[capability::SHADER]);
    b.header(op::MEMORY_MODEL, &[addressing::LOGICAL, memory::GLSL450]);

    // Every input and output variable the entry point touches is named in its interface, or a
    // driver rejects the module.
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

    if z.to_bits() != 0 {
        b.declare(op::CONSTANT, &[f32_type.0, depth.0, z.to_bits()]);
    }

    // The three corners. `w` is one and `z` the depth asked for: no perspective.
    b.declare(
        op::CONSTANT_COMPOSITE,
        &[vec4.0, corner.0, minus_one.0, minus_one.0, depth.0, one.0],
    );
    b.declare(
        op::CONSTANT_COMPOSITE,
        &[vec4.0, right.0, three.0, minus_one.0, depth.0, one.0],
    );
    b.declare(
        op::CONSTANT_COMPOSITE,
        &[vec4.0, up.0, minus_one.0, three.0, depth.0, one.0],
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
/// It asks only whether a fragment shader's output reaches the attachment, so the colour is a
/// constant and a failure cannot be the arithmetic's (D549). The components are bit patterns so the
/// caller's expectation and the shader's constant are written the same way.
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

/// Declares the per-corner varying values as one constant array, the same shape as the position
/// table, so the vertex shader indexes both the same way.
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
/// The [`fullscreen_triangle_vertex_module`] triangle plus a `Location 0` output carrying
/// `corners[i]` for vertex `i`, so interpolation can be checked against a pipeline. The positions
/// are the same table, so a failure is about the varying rather than the geometry.
// A module is a linear sequence of declarations, each a local the next line needs; splitting it
// further only moves the length into helpers taking many ids.
#[allow(clippy::too_many_lines)]
#[must_use]
pub fn interpolated_vertex_module(corners: [[f32; 4]; 3]) -> Vec<u32> {
    interpolating_vertex_module(&[corners])
}

/// The same triangle carrying one varying per location, in order from zero.
///
/// A guest's textured pixel shader reads two varyings, a colour and a coordinate; with only the
/// first written, the second arrives undefined. Each entry is that varying's value at the three
/// corners. Otherwise [`interpolated_vertex_module`] unchanged.
// A module is a linear sequence of declarations, each a local the next line needs; splitting it
// further only moves the length into helpers taking many ids.
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
/// A guest's `image_sample_lz` names level zero explicitly, while a fragment stage may let the
/// implementation choose from derivatives. They are different instructions, and the oracle offers
/// both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lod {
    /// Chosen by the implementation from the derivatives of the coordinate. Fragment stages only.
    Implicit,
    /// Level zero, named by the instruction. What a guest's `_lz` form asks for.
    Zero,
}

/// A fragment shader that samples a bound texture at the interpolated coordinate.
///
/// The host half of a guest's textured pixel shader, built before the translation targets it
/// (D549). The image type and its seven operands, the sampled-image type, the storage class and the
/// sampling instruction are taken from a GLSL sampling shader compiled and disassembled with
/// `spirv-dis`. The sampled image is at set 0, binding 2, after the two storage buffers every
/// translated module declares.
// A module is a linear sequence of declarations, each needed by the line after it.
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
    // Element type, then: two-dimensional, not depth, not arrayed, single-sampled, used with a
    // sampler, and no declared format, which is a descriptor-bound sampled image.
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

    // The level an explicit sample asks for, declared whichever form this module emits: an
    // unreferenced constant is harmless and keeps the builder branch-free.
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
        // The mask says a level follows, and the level is the constant zero.
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
/// Two, because zero and one are the observation window and guest memory, which every translated
/// module declares, and a pipeline binds one set.
pub const TEXTURE_BINDING: u32 = 2;

/// Which binding a storage image is bound at.
///
/// Three, after the sampled image. Separate bindings because a sampled image is read through a
/// sampler and cannot be written, and a storage image is written and has no sampler.
pub const STORAGE_IMAGE_BINDING: u32 = 3;

/// Which binding each of a compute dispatch's storage images is bound at, in the order its program
/// first stores to them: [`STORAGE_IMAGE_BINDING`], then 21 onwards, after the last of
/// [`TEXTURE_BINDINGS`].
pub const STORAGE_IMAGE_BINDINGS: [u32; MAX_STORED_IMAGES] = [STORAGE_IMAGE_BINDING, 21, 22, 23];

/// How many images a compute dispatch may store to, one binding each: four, the storage images one
/// stage may reach on every Vulkan device (the required minimum of
/// `maxPerStageDescriptorStorageImages`). A pixel shader stores to one.
pub const MAX_STORED_IMAGES: usize = 4;

/// Which binding a pixel shader's second sampled image is bound at: four, after the storage image.
/// The open-toolchain GL context's second texture unit samples one.
pub const SECOND_TEXTURE_BINDING: u32 = 4;

/// Which binding each of a pixel shader's sampled images is bound at, by slot: [`TEXTURE_BINDING`],
/// [`SECOND_TEXTURE_BINDING`], then seven onwards, after the global data share's binding (D690).
pub const TEXTURE_BINDINGS: [u32; MAX_TEXTURES] = {
    let mut bindings = [0; MAX_TEXTURES];
    bindings[0] = TEXTURE_BINDING;
    bindings[1] = SECOND_TEXTURE_BINDING;
    let mut slot = 2;
    while slot < MAX_TEXTURES {
        bindings[slot] = 5 + slot as u32;
        slot += 1;
    }
    bindings
};

/// How many sampled images a pixel shader may read, one binding each (D690): sixteen, the sampled
/// images and the samplers one stage may reach on every Vulkan device (the required minimum of
/// `maxPerStageDescriptorSampledImages` and `maxPerStageDescriptorSamplers`).
pub const MAX_TEXTURES: usize = 16;

/// Which binding a compute module reaches the global data share at: six, after the draw data
/// (D789). Declared only by a module that accesses it.
pub const GDS_BINDING: u32 = 6;

/// The global data share's words: 64 KiB, a modelled capacity (D789, assumed) above the 48 KiB
/// PPSA25872's program declares in `m0`. An access past it is dropped, as one past `m0` is.
pub const GDS_WORDS: u32 = 0x4000;

/// Whether `module` declares a variable at `binding` of descriptor set zero: an `OpDecorate ...
/// Binding binding` beside an `OpDecorate ... DescriptorSet 0` on the same target.
#[must_use]
pub fn declares_binding(module: &[u32], binding: u32) -> bool {
    let mut at = 5;
    let (mut bound, mut in_set_zero) = (Vec::new(), Vec::new());
    while at < module.len() {
        let count = (module[at] >> 16) as usize;
        if count == 0 {
            break;
        }
        if module[at] & 0xffff == u32::from(op::DECORATE)
            && let Some(&[target, kind, value]) = module.get(at + 1..at + 4)
        {
            if kind == decoration::BINDING && value == binding {
                bound.push(target);
            }
            if kind == decoration::DESCRIPTOR_SET && value == 0 {
                in_set_zero.push(target);
            }
        }
        at += count;
    }
    bound.iter().any(|target| in_set_zero.contains(target))
}

/// Whether `module` is laid out as SPIR-V: the magic number, a five-word header with a non-zero id
/// bound and a zero schema, then instructions each at least one word long that end exactly at its
/// end. A module that is not is refused before a device sees it: a driver may report it, but a
/// layer checking the module walks it first, and malformed words can take that walk past the end.
#[must_use]
pub fn well_formed(module: &[u32]) -> bool {
    if module.len() < 5 || module[0] != MAGIC || module[3] == 0 || module[4] != 0 {
        return false;
    }
    let mut at = 5;
    while at < module.len() {
        let count = (module[at] >> 16) as usize;
        if count == 0 {
            return false;
        }
        at += count;
    }
    at == module.len()
}

/// The bytes `module`'s push-constant block declares: its largest `PushConstant` variable, made of
/// 32-bit scalars, arrays of them and structures of those, laid end to end as every block here is.
/// A pipeline's push-constant range must cover it, whatever a dispatch pushes into it. `None` where
/// it declares none, or one of another shape.
#[must_use]
pub fn push_constant_bytes(module: &[u32]) -> Option<u32> {
    use std::collections::HashMap;
    let (mut constants, mut sizes, mut pointees) = (HashMap::new(), HashMap::new(), HashMap::new());
    let mut largest = None;
    let mut at = 5;
    while at < module.len() {
        let count = (module[at] >> 16) as usize;
        let operands = module.get(at + 1..at + count.max(1))?;
        if count == 0 {
            return None;
        }
        let opcode = module[at] & 0xffff;
        match (u16::try_from(opcode).ok(), operands) {
            (Some(op::TYPE_INT | op::TYPE_FLOAT), &[id, 32, ..]) => {
                sizes.insert(id, 4);
            }
            (Some(op::CONSTANT), &[_, id, value]) => {
                constants.insert(id, value);
            }
            (Some(op::TYPE_ARRAY), &[id, element, length]) => {
                if let (Some(&size), Some(&length)) = (sizes.get(&element), constants.get(&length))
                {
                    sizes.insert(id, size * length);
                }
            }
            (Some(op::TYPE_STRUCT), &[id, ref members @ ..]) => {
                if let Some(size) = members
                    .iter()
                    .map(|member| sizes.get(member))
                    .sum::<Option<u32>>()
                {
                    sizes.insert(id, size);
                }
            }
            (Some(op::TYPE_POINTER), &[id, storage::PUSH_CONSTANT, pointee]) => {
                pointees.insert(id, pointee);
            }
            (Some(op::VARIABLE), &[pointer, _, storage::PUSH_CONSTANT]) => {
                let size = pointees
                    .get(&pointer)
                    .and_then(|pointee| sizes.get(pointee))?;
                largest = largest.max(Some(*size));
            }
            _ => {}
        }
        at += count;
    }
    largest
}

/// Which binding a mesh module reads its per-draw user data from: five (D718). One host dispatch
/// carries a run of guest draws, one workgroup each, and each workgroup reads
/// [`DRAW_DATA_STRIDE_WORDS`] words at `workgroup * stride`.
pub const DRAW_DATA_BINDING: u32 = 5;

/// Words each draw has in the draw-data buffer: one stage's share of the user-data block, the
/// draw's vertex count at [`DRAW_DATA_VERTICES_WORD`] (D745), where each of its geometry buffers
/// lies at [`DRAW_DATA_BUFFERS_WORD`] (D747) and each one's guest base at
/// [`DRAW_DATA_BASES_WORD`] (D758), padded to a multiple of four.
pub const DRAW_DATA_STRIDE_WORDS: u32 =
    (DRAW_DATA_BASES_WORD + DRAW_BUFFERS_PER_STAGE).next_multiple_of(4);

/// Where in a draw's stride of the draw-data buffer its vertex count is: after the thirty-two
/// user-data words (D745).
pub const DRAW_DATA_VERTICES_WORD: u32 = 32;

/// Where in a draw's stride its first vertex id is (D761): the vertex a chunk of a draw too large
/// for one wave starts at, which its vertex threads count up from in place of an index buffer.
pub const DRAW_DATA_FIRST_VERTEX_WORD: u32 = 33;

/// Where in a draw's stride the layer it draws is (D859): the slice of its view this pass of the
/// draw renders, whose primitives alone it keeps, or `u32::MAX` for a draw that does not use the
/// layer and keeps every primitive.
pub const DRAW_DATA_LAYER_WORD: u32 = 34;

/// [`DRAW_DATA_LAYER_WORD`]'s value for a draw that keeps every primitive whatever its layer.
pub const DRAW_LAYER_UNUSED: u32 = u32::MAX;

/// Where in a draw's stride its instance count is (D863): the draw runs a workgroup per instance,
/// the instance its `WorkgroupId.y`, and only draws of one count share a dispatch.
pub const DRAW_DATA_INSTANCES_WORD: u32 = 35;

/// Where in a draw's stride its geometry buffers' places begin (D747): for each slot, three words -
/// the arena at the geometry binding it is a range of, its first word there, and how many words it
/// holds.
pub const DRAW_DATA_BUFFERS_WORD: u32 = 36;

/// Where in a draw's stride its geometry buffers' guest bases begin (D758): for each slot, the low
/// half of the guest address its range starts at, which a global load's address is read against.
pub const DRAW_DATA_BASES_WORD: u32 = DRAW_DATA_BUFFERS_WORD + DRAW_BUFFERS_PER_STAGE * 3;

/// The most draws one dispatch carries, and so how many strides the draw-data binding spans.
pub const DRAW_DATA_MOST_DRAWS: u32 = 4096;

/// The descriptor set a draw's buffers are bound in (D733): its own, beside set zero, which a
/// pipeline owns, so a pipeline serves every draw whatever buffers it binds.
pub const DRAW_BUFFERS_SET: u32 = 1;

/// Which binding of [`DRAW_BUFFERS_SET`] a primitive shader's buffers are at.
pub const GEOMETRY_BUFFERS_BINDING: u32 = 0;

/// Which binding of [`DRAW_BUFFERS_SET`] a pixel shader's buffers are at.
pub const PIXEL_BUFFERS_BINDING: u32 = 1;

/// Where a compute dispatch's traced buffers are bound in the draw-buffer set: two, after the two
/// draw stages', and writable, since a dispatch's stores through them are written back (D746).
pub const COMPUTE_BUFFERS_BINDING: u32 = 2;

/// Where a compute dispatch's write masks are bound in the draw-buffer set (D809): a word for each
/// word of a buffer at [`COMPUTE_BUFFERS_BINDING`] the program only stores to, set where the
/// dispatch stored, slot for slot.
pub const COMPUTE_WRITTEN_BINDING: u32 = 3;

/// How many buffers one stage of a draw or a dispatch binds: the length of each binding's array.
/// Sixty-four: a retail vertex shader traces eleven, its descriptors, a table and the streams its
/// fetch forms, PPSA03416's nineteen, seventeen of them descriptors its fetch computes (D733), and
/// PPSA04263's scene compute program fifty. Desktop devices bind far more storage buffers per stage;
/// the backend checks before it relies on it.
pub const DRAW_BUFFERS_PER_STAGE: u32 = 64;

/// A fragment shader that writes one texel of a storage image, and a colour.
///
/// The colour shows the shader ran; the image shows what it stored. The write instruction, the
/// capability a format-less storage image needs, the `NonReadable` decoration and the image type
/// with `Sampled` at 2 are taken from a compiled GLSL `imageStore` through a format-less `writeonly
/// image2D`. The texel and coordinate are the caller's, so a test names what it expects where.
// A module is a linear sequence of declarations, each needed by the line after it.
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
    // The seven operands a sampled image takes, with `Sampled` at 2 (written through an image
    // instruction, not read through a sampler) and the format `Unknown`, which the capability above
    // permits.
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
/// The counterpart to [`interpolated_vertex_module`]: a `Location 0` input stored to a `Location 0`
/// output, so the attachment holds exactly what the pipeline interpolated.
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

    // The input's location matches the vertex shader's output location, which pairs a varying
    // across the two stages.
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
    /// Checked first: an instruction of unknown shape is skipped, so its result is never recorded
    /// and a later use would be reported against the wrong instruction.
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
    /// A type the format allows once per module is declared a second time.
    DeclaredTwice {
        /// The first declaration's identifier.
        first: u32,
        /// The second's.
        second: u32,
        /// The type's opcode.
        opcode: u16,
    },
}

impl fmt::Display for ModuleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            // One literal, so the message carries no source indentation.
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
            Self::DeclaredTwice {
                first,
                second,
                opcode,
            } => write!(
                f,
                concat!(
                    "%{} declares the same type as %{} (opcode {}) - invalid SPIR-V, which ",
                    "one driver accepts and another faults compiling"
                ),
                second, first, opcode
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
        // A zero length would never advance the walk; `encode` always writes the head word, but a
        // loop that hangs on malformed input is worse than one that stops.
        let length = length.max(1);
        let end = (self.at + length).min(self.words.len());
        let operands = &self.words[self.at + 1..end];
        self.at = end;
        Some(((head & 0xFFFF) as u16, operands))
    }
}

/// Where an instruction keeps its result identifier and which operands name others.
///
/// Only the opcodes this crate emits appear; an absent opcode is skipped rather than guessed at,
/// because reading a literal as an identifier would reject a valid module.
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
/// `Every` is the usual case. `Alternating` is for `OpSwitch`, whose tail is literal-then-label
/// repeated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RestStride {
    Every,
    Alternating,
}

/// Where each opcode keeps its identifiers.
///
/// `(opcode, result operand index, operand indices naming existing identifiers, index from which
/// every remaining operand is an identifier)`. An opcode absent from the table is skipped rather
/// than guessed at.
static SHAPES: &[ShapeEntry] = &[
    // Types declare their identifier first. The trailing words of an integer or float
    // type are widths, so `rest` is not simply always set.
    (op::TYPE_VOID, Some(0), &[], None, RestStride::Every),
    (op::TYPE_BOOL, Some(0), &[], None, RestStride::Every),
    (op::TYPE_INT, Some(0), &[], None, RestStride::Every),
    // Result id, then the component type (an identifier) and a literal count. The result is named
    // by `Some(0)` and is not also a use of itself.
    (op::TYPE_VECTOR, Some(0), &[1], None, RestStride::Every),
    // Imports an extended set: result id, then a literal name string, so no identifiers.
    (op::EXT_INST_IMPORT, Some(0), &[], None, RestStride::Every),
    // Result type, result, set, a literal instruction number, then operand identifiers. Index 3 is
    // the literal, named by neither `fixed` nor `rest`.
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
    // Execution scope, memory scope and semantics, all identifiers.
    (
        op::CONTROL_BARRIER,
        None,
        &[0, 1, 2],
        None,
        RestStride::Every,
    ),
    // Result type, result, pointer, scope, semantics and value.
    (
        op::ATOMIC_AND,
        Some(1),
        &[0, 2, 3, 4, 5],
        None,
        RestStride::Every,
    ),
    (
        op::ATOMIC_OR,
        Some(1),
        &[0, 2, 3, 4, 5],
        None,
        RestStride::Every,
    ),
    (
        op::ATOMIC_U_MIN,
        Some(1),
        &[0, 2, 3, 4, 5],
        None,
        RestStride::Every,
    ),
    (
        op::ATOMIC_U_MAX,
        Some(1),
        &[0, 2, 3, 4, 5],
        None,
        RestStride::Every,
    ),
    (
        op::ATOMIC_I_ADD,
        Some(1),
        &[0, 2, 3, 4, 5],
        None,
        RestStride::Every,
    ),
    (
        op::ATOMIC_S_MIN,
        Some(1),
        &[0, 2, 3, 4, 5],
        None,
        RestStride::Every,
    ),
    (
        op::ATOMIC_S_MAX,
        Some(1),
        &[0, 2, 3, 4, 5],
        None,
        RestStride::Every,
    ),
    // Result type, result, an execution-scope identifier, the value and the invocation index.
    (
        op::GROUP_NON_UNIFORM_SHUFFLE,
        Some(1),
        &[0, 2, 3, 4],
        None,
        RestStride::Every,
    ),
    // Result type, result, an execution-scope identifier, the value and the index in the quad.
    (
        op::GROUP_NON_UNIFORM_QUAD_BROADCAST,
        Some(1),
        &[0, 2, 3, 4],
        None,
        RestStride::Every,
    ),
    (op::TYPE_FLOAT, Some(0), &[], None, RestStride::Every),
    // Return type then parameter types, all identifiers.
    (op::TYPE_FUNCTION, Some(0), &[], Some(1), RestStride::Every),
    // Element type and a constant holding the length.
    (op::TYPE_ARRAY, Some(0), &[1, 2], None, RestStride::Every),
    // Result id and the element type.
    (
        op::TYPE_RUNTIME_ARRAY,
        Some(0),
        &[1],
        None,
        RestStride::Every,
    ),
    // Result type, result, the block pointer, then the member index as a literal.
    (op::ARRAY_LENGTH, Some(1), &[0, 2], None, RestStride::Every),
    (op::TYPE_STRUCT, Some(0), &[], Some(1), RestStride::Every),
    // Operand one is a storage class literal, so only operand two is named.
    (op::TYPE_POINTER, Some(0), &[2], None, RestStride::Every),
    (op::LABEL, Some(0), &[], None, RestStride::Every),
    // Typed results: operand zero is the type, operand one the identifier.
    // Operand two of a constant is its literal value.
    (op::CONSTANT, Some(1), &[0], None, RestStride::Every),
    (op::SPEC_CONSTANT, Some(1), &[0], None, RestStride::Every),
    // Result type, result, then every constituent - each an identifier that must exist.
    (
        op::CONSTANT_COMPOSITE,
        Some(1),
        &[0],
        Some(2),
        RestStride::Every,
    ),
    // Result type, result, then every constituent, each an identifier that must exist.
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
    // Every index after the base is an identifier, not a literal.
    (op::ACCESS_CHAIN, Some(1), &[0], Some(2), RestStride::Every),
    (op::LOAD, Some(1), &[0, 2], None, RestStride::Every),
    (op::BITCAST, Some(1), &[0, 2], None, RestStride::Every),
    (
        op::CONVERT_F_TO_U,
        Some(1),
        &[0, 2],
        None,
        RestStride::Every,
    ),
    (
        op::CONVERT_F_TO_S,
        Some(1),
        &[0, 2],
        None,
        RestStride::Every,
    ),
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
    // A texture type: result id, the element type, then six literals. Only operand one is an
    // identifier.
    (op::TYPE_IMAGE, Some(0), &[1], None, RestStride::Every),
    // An image paired with a sampler: result id and the image type it wraps.
    (
        op::TYPE_SAMPLED_IMAGE,
        Some(0),
        &[1],
        None,
        RestStride::Every,
    ),
    // Result type, result, the sampled image, and the coordinate. This crate emits no trailing
    // image operands for this opcode; emitting one needs this row extended, or its mask would be
    // read as an identifier.
    (
        op::IMAGE_SAMPLE_IMPLICIT_LOD,
        Some(1),
        &[0, 2, 3],
        None,
        RestStride::Every,
    ),
    // The same, plus a literal operand mask at index four and the identifiers it announces from
    // index five, so the level is checked and the mask is not.
    (
        op::IMAGE_SAMPLE_EXPLICIT_LOD,
        Some(1),
        &[0, 2, 3],
        Some(5),
        RestStride::Every,
    ),
    // The same with the reference at index four, so the mask is at five and the level from six.
    (
        op::IMAGE_SAMPLE_DREF_EXPLICIT_LOD,
        Some(1),
        &[0, 2, 3, 4],
        Some(6),
        RestStride::Every,
    ),
    // Result type, result, the sampled image, the coordinate and the reference; no trailing image
    // operands are emitted for it.
    (
        op::IMAGE_SAMPLE_DREF_IMPLICIT_LOD,
        Some(1),
        &[0, 2, 3, 4],
        None,
        RestStride::Every,
    ),
    // The same shape as the explicit sampling form: a literal mask at index four, and the level
    // from index five.
    (
        op::IMAGE_FETCH,
        Some(1),
        &[0, 2, 3],
        Some(5),
        RestStride::Every,
    ),
    // Result type, result, the sampled image, the coordinate and the component; no trailing image
    // operands are emitted for it.
    (
        op::IMAGE_GATHER,
        Some(1),
        &[0, 2, 3, 4],
        None,
        RestStride::Every,
    ),
    // Result type, result, and the sampled image the image is taken out of.
    (op::IMAGE, Some(1), &[0, 2], None, RestStride::Every),
    // Result type, result, the image and the level whose size it asks.
    (
        op::IMAGE_QUERY_SIZE_LOD,
        Some(1),
        &[0, 2, 3],
        None,
        RestStride::Every,
    ),
    // Result type, result, and the image whose size it asks.
    (
        op::IMAGE_QUERY_SIZE,
        Some(1),
        &[0, 2],
        None,
        RestStride::Every,
    ),
    // No result: an image write is an effect. The image, coordinate and texel are all identifiers,
    // so the whole instruction is `rest` from zero.
    (op::IMAGE_WRITE, None, &[], Some(0), RestStride::Every),
    // Result type, result, the two vectors, then literal component indices.
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
    (op::UDIV, Some(1), &[0, 2, 3], None, RestStride::Every),
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
    (
        op::ULESS_THAN_EQUAL,
        Some(1),
        &[0, 2, 3],
        None,
        RestStride::Every,
    ),
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
    (op::BIT_REVERSE, Some(1), &[0, 2], None, RestStride::Every),
    (op::INOT_EQUAL, Some(1), &[0, 2, 3], None, RestStride::Every),
    (op::FORD_EQUAL, Some(1), &[0, 2, 3], None, RestStride::Every),
    (
        op::FUNORD_EQUAL,
        Some(1),
        &[0, 2, 3],
        None,
        RestStride::Every,
    ),
    (
        op::FORD_NOT_EQUAL,
        Some(1),
        &[0, 2, 3],
        None,
        RestStride::Every,
    ),
    (
        op::FUNORD_LESS_THAN,
        Some(1),
        &[0, 2, 3],
        None,
        RestStride::Every,
    ),
    (
        op::FUNORD_GREATER_THAN,
        Some(1),
        &[0, 2, 3],
        None,
        RestStride::Every,
    ),
    (
        op::FUNORD_LESS_THAN_EQUAL,
        Some(1),
        &[0, 2, 3],
        None,
        RestStride::Every,
    ),
    (
        op::FUNORD_GREATER_THAN_EQUAL,
        Some(1),
        &[0, 2, 3],
        None,
        RestStride::Every,
    ),
    (
        op::FUNORD_NOT_EQUAL,
        Some(1),
        &[0, 2, 3],
        None,
        RestStride::Every,
    ),
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
    (op::KILL, None, &[], None, RestStride::Every),
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
/// The host oracle for the stage a guest's primitive shader translates to (D688): it declares its
/// counts, writes the primitive's indices, then writes per-vertex outputs, matching
/// `MSG_GS_ALLOC_REQ`, `exp prim` and `exp pos`/`exp param`. Every mesh-specific value (capability,
/// execution model, execution modes, index built-in, count opcode) is taken from a reference mesh
/// shader compiled with the SDK's GLSL compiler and read with `spirv-dis`. The per-vertex outputs
/// are an array of a `Block` struct whose first member carries `Position`, as the stage requires.
/// The triangle is the one every other oracle here draws, so frames compare pixel for pixel.
#[must_use]
pub fn triangle_mesh_module(corners: [[f32; 4]; 3]) -> Vec<u32> {
    // 1.4, because `SPV_EXT_mesh_shader` requires it.
    let mut b = Builder::new().with_version(VERSION_1_4);

    let ids = MeshIds::new(&mut b);
    emit_mesh_header(&mut b, &ids, MeshShape::Triangle);
    declare_mesh_types(&mut b, &ids, MeshShape::Triangle);
    let MeshIds {
        void,
        fn_type,
        f32_type,
        vec4,
        vertices,
        colours,
        indices,
        output_vec4,
        output_uvec3,
        zero,
        one,
        two,
        three,
        triangle,
        main,
        entry_block,
        ..
    } = ids;

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

    // Declared before anything is written: three vertices, one primitive.
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

/// A mesh shader that emits one line of one colour between two clip-space points.
///
/// [`triangle_mesh_module`]'s module with `OutputLinesEXT` and `PrimitiveLineIndicesEXT` over a
/// two-index primitive in place of the triangle's (`SPV_EXT_mesh_shader`): the oracle for a line a
/// guest's primitive shader emits, which rasterizes at the pipeline's line width.
#[must_use]
pub fn line_mesh_module(colour: [f32; 4], ends: [[f32; 4]; 2]) -> Vec<u32> {
    let mut b = Builder::new().with_version(VERSION_1_4);

    let ids = MeshIds::new(&mut b);
    emit_mesh_header(&mut b, &ids, MeshShape::Line);
    declare_mesh_types(&mut b, &ids, MeshShape::Line);
    let MeshIds {
        void,
        fn_type,
        f32_type,
        vec4,
        vertices,
        colours,
        indices,
        output_vec4,
        output_uvec3,
        zero,
        one,
        two,
        triangle,
        main,
        entry_block,
        ..
    } = ids;

    let position_ids: Vec<Id> = ends
        .into_iter()
        .map(|place| constant_vec4(&mut b, f32_type, vec4, place))
        .collect();
    let colour_id = constant_vec4(&mut b, f32_type, vec4, colour);

    b.function(op::FUNCTION, &[void.0, main.0, 0, fn_type.0]);
    b.function(op::LABEL, &[entry_block.0]);
    // Two vertices, one primitive.
    b.function(op::SET_MESH_OUTPUTS_EXT, &[two.0, one.0]);
    for (slot, place) in [zero, one].into_iter().zip(position_ids) {
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
        b.function(op::STORE, &[colour_ptr.0, colour_id.0]);
    }
    // `output_uvec3` and `triangle` are the two-component index pointer and the line `(0, 1)`
    // here ([`declare_mesh_types`]).
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

/// The primitive a mesh oracle emits.
#[derive(Clone, Copy, PartialEq, Eq)]
enum MeshShape {
    /// One triangle: `OutputTrianglesEXT`, three indices.
    Triangle,
    /// One line: `OutputLinesEXT`, two indices.
    Line,
}

/// The identifiers the mesh oracle reserves before it writes anything, in reservation order.
#[derive(Clone, Copy)]
struct MeshIds {
    void: Id,
    fn_type: Id,
    f32_type: Id,
    u32_type: Id,
    vec4: Id,
    uvec3: Id,
    per_vertex: Id,
    vertex_array: Id,
    vertex_array_ptr: Id,
    vertices: Id,
    colour_array: Id,
    colour_array_ptr: Id,
    colours: Id,
    index_array: Id,
    index_array_ptr: Id,
    indices: Id,
    output_vec4: Id,
    output_uvec3: Id,
    zero: Id,
    one: Id,
    two: Id,
    three: Id,
    triangle: Id,
    main: Id,
    entry_block: Id,
}

impl MeshIds {
    /// Reserves every identifier, in field order.
    fn new(b: &mut Builder) -> Self {
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
        Self {
            void,
            fn_type,
            f32_type,
            u32_type,
            vec4,
            uvec3,
            per_vertex,
            vertex_array,
            vertex_array_ptr,
            vertices,
            colour_array,
            colour_array_ptr,
            colours,
            index_array,
            index_array_ptr,
            indices,
            output_vec4,
            output_uvec3,
            zero,
            one,
            two,
            three,
            triangle,
            main,
            entry_block,
        }
    }
}

/// Writes the mesh oracle's capability, extension, entry point, execution modes and decorations.
fn emit_mesh_header(b: &mut Builder, ids: &MeshIds, shape: MeshShape) {
    let MeshIds {
        per_vertex,
        vertices,
        colours,
        indices,
        main,
        ..
    } = *ids;
    // `MeshShadingEXT` implies `Shader`, so it is declared alone, as the reference does.
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
    let (output, indices_built_in) = match shape {
        MeshShape::Triangle => (
            mode::OUTPUT_TRIANGLES_EXT,
            built_in::PRIMITIVE_TRIANGLE_INDICES_EXT,
        ),
        MeshShape::Line => (mode::OUTPUT_LINES_EXT, built_in::PRIMITIVE_LINE_INDICES_EXT),
    };
    b.header(op::EXECUTION_MODE, &[main.0, output]);

    // The per-vertex outputs are a block with the position as a member; a bare `vec4` array
    // decorated `Position` is the vertex-stage shape.
    b.annotate(op::DECORATE, &[per_vertex.0, decoration::BLOCK]);
    b.annotate(
        op::MEMBER_DECORATE,
        &[per_vertex.0, 0, decoration::BUILT_IN, built_in::POSITION],
    );
    b.annotate(op::DECORATE, &[colours.0, decoration::LOCATION, 0]);
    b.annotate(
        op::DECORATE,
        &[indices.0, decoration::BUILT_IN, indices_built_in],
    );
}

/// Declares the mesh oracle's types, constants and output variables. For a line, `uvec3` is a
/// two-component vector and `triangle` the line's indices `(0, 1)`.
fn declare_mesh_types(b: &mut Builder, ids: &MeshIds, shape: MeshShape) {
    let MeshIds {
        void,
        fn_type,
        f32_type,
        u32_type,
        vec4,
        uvec3,
        per_vertex,
        vertex_array,
        vertex_array_ptr,
        vertices,
        colour_array,
        colour_array_ptr,
        colours,
        index_array,
        index_array_ptr,
        indices,
        output_vec4,
        output_uvec3,
        zero,
        one,
        two,
        three,
        triangle,
        ..
    } = *ids;
    b.declare(op::TYPE_VOID, &[void.0]);
    b.declare(op::TYPE_FUNCTION, &[fn_type.0, void.0]);
    b.declare(op::TYPE_FLOAT, &[f32_type.0, 32]);
    b.declare(op::TYPE_INT, &[u32_type.0, 32, 0]);
    b.declare(op::TYPE_VECTOR, &[vec4.0, f32_type.0, 4]);
    let index_width = if shape == MeshShape::Line { 2 } else { 3 };
    b.declare(op::TYPE_VECTOR, &[uvec3.0, u32_type.0, index_width]);

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
    if shape == MeshShape::Line {
        b.declare(
            op::CONSTANT_COMPOSITE,
            &[uvec3.0, triangle.0, zero.0, one.0],
        );
    } else {
        b.declare(
            op::CONSTANT_COMPOSITE,
            &[uvec3.0, triangle.0, zero.0, one.0, two.0],
        );
    }
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

    /// A module's push-constant block is sized as declared: the compaction module's three words, and
    /// none for a module that declares no block.
    #[test]
    fn a_push_constant_block_is_sized_as_declared() {
        assert_eq!(
            super::push_constant_bytes(&super::stored_word_compaction_module()),
            Some(12)
        );
        assert_eq!(
            super::push_constant_bytes(&minimal_compute_module([1, 1, 1])),
            None
        );
    }

    /// A module the builder makes is laid out as SPIR-V; the right magic word followed by words
    /// that are no instructions is not, nor is a module cut short inside an instruction.
    #[test]
    fn only_a_module_laid_out_as_spir_v_is_well_formed() {
        let made = minimal_compute_module([1, 1, 1]);
        assert!(super::well_formed(&made));
        let rubbish = [MAGIC, 0x0001_0300, 0, 1, 0, 0xDEAD_BEEF, 0xDEAD_BEEF];
        assert!(!super::well_formed(&rubbish));
        // Its first instruction, `OpCapability`, is two words: cut after the first of them.
        assert_eq!(made[5] >> 16, 2);
        assert!(!super::well_formed(&made[..6]));
        assert!(!super::well_formed(&made[..4]));
    }

    /// A stage binds the eleven buffers PPSA02664's vertex shaders trace (four descriptors in user
    /// data, a table, and six vertex streams its fetch forms) and the fifty Grand Theft Auto V's scene
    /// compute program reads through, and a draw's stride holds every slot's place and base after
    /// its user data.
    #[test]
    fn a_draw_stage_binds_what_a_retail_vertex_shader_reads() {
        use super::{
            DRAW_BUFFERS_PER_STAGE, DRAW_DATA_BASES_WORD, DRAW_DATA_BUFFERS_WORD,
            DRAW_DATA_STRIDE_WORDS,
        };
        const {
            assert!(DRAW_BUFFERS_PER_STAGE >= 50);
            assert!(DRAW_DATA_BASES_WORD == DRAW_DATA_BUFFERS_WORD + 3 * DRAW_BUFFERS_PER_STAGE);
            assert!(DRAW_DATA_STRIDE_WORDS >= DRAW_DATA_BASES_WORD + DRAW_BUFFERS_PER_STAGE);
            assert!(DRAW_DATA_STRIDE_WORDS % 4 == 0);
        }
    }

    /// Stripping removes exactly the float-controls declaration: the capability, the extension and
    /// the execution mode go, and every other word stays.
    #[test]
    fn stripping_removes_only_the_inf_nan_declaration() {
        let plain = minimal_compute_module([1, 1, 1]);
        let mut words = plain.clone();
        let main = 1_u32;
        let name = Builder::literal_string(super::FLOAT_CONTROLS);
        let mut extension = vec![(((name.len() + 1) as u32) << 16) | u32::from(op::EXTENSION)];
        extension.extend(name);
        let capability = [
            (2 << 16) | u32::from(op::CAPABILITY),
            super::capability::SIGNED_ZERO_INF_NAN_PRESERVE,
        ];
        let mode = [
            (4 << 16) | u32::from(op::EXECUTION_MODE),
            main,
            super::mode::SIGNED_ZERO_INF_NAN_PRESERVE,
            32,
        ];
        words.extend_from_slice(&capability);
        words.extend_from_slice(&extension);
        words.extend_from_slice(&mode);
        assert_eq!(super::without_inf_nan_preserve(&words), plain);
        assert_eq!(super::without_inf_nan_preserve(&plain), plain);
    }

    /// A stream that cannot be walked is handed back unchanged, for the device to refuse.
    #[test]
    fn a_malformed_stream_is_not_altered() {
        let mut words = minimal_compute_module([1, 1, 1]);
        words.push(0x00ff_0011);
        assert_eq!(super::without_inf_nan_preserve(&words), words);
    }

    /// A module starts with the magic word, and its header carries a bound.
    #[test]
    fn a_module_begins_with_the_magic_word_and_a_bound() {
        let words = minimal_compute_module([64, 1, 1]);
        assert_eq!(words[0], MAGIC);
        assert_eq!(words[1], VERSION_1_0);
        assert!(words.len() > 5, "a header alone is not a module");
    }

    /// The header's bound exceeds every identifier the builder handed out.
    #[test]
    fn the_bound_exceeds_every_identifier_handed_out() {
        // A bound at or below an identifier in use makes the module malformed.
        let mut b = Builder::new();
        let first = b.id();
        let last = b.id();
        let words = b.finish();
        assert!(words[3] > last.0);
        assert_eq!(first.0, 1, "identifier zero is reserved by the format");
    }

    /// An instruction's first word packs its length above its opcode.
    #[test]
    fn an_instruction_word_packs_its_length_and_opcode() {
        // Getting this backwards makes the validator report an unknown opcode rather than a bad
        // header.
        let mut b = Builder::new();
        b.header(op::CAPABILITY, &[1]);
        let words = b.finish();
        let instruction = words[5];
        assert_eq!(instruction >> 16, 2, "one word of header plus one operand");
        assert_eq!(instruction & 0xFFFF, u32::from(op::CAPABILITY));
    }

    /// A mesh shader's output shapes are the SPIR-V specification's numbers (section 3.6).
    #[test]
    fn mesh_output_modes_are_the_specification_numbers() {
        // 19 is `InputPoints`, a geometry-shader mode: NVIDIA rasterised a mesh point declared with
        // it, the AMD driver drew nothing.
        assert_eq!(super::mode::OUTPUT_POINTS, 27, "OutputPoints");
        assert_eq!(super::mode::OUTPUT_LINES_EXT, 5269, "OutputLinesEXT");
        assert_eq!(
            super::mode::OUTPUT_TRIANGLES_EXT,
            5298,
            "OutputTrianglesEXT"
        );
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

    /// An extended-set import precedes the memory model whatever the call order (SPIR-V 2.4).
    #[test]
    fn an_extended_set_is_imported_before_the_memory_model_whatever_the_call_order() {
        // The builder places it by slot, so the import lands ahead of the memory model although
        // this asks for them the other way round.
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

    /// An `OpExtInst` carries result type, result, set, instruction number and operands in that
    /// order.
    #[test]
    fn an_ext_inst_carries_result_type_result_set_instruction_and_operands_in_order() {
        // A transposition validates as a different or wrong-typed instruction, so the order is
        // pinned.
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

    /// A string whose length is a multiple of four still gets a terminating word.
    #[test]
    fn a_string_is_terminated_even_when_its_length_is_a_multiple_of_four() {
        // Four characters fill a word, so the terminator needs a second.
        assert_eq!(Builder::literal_string("main").len(), 2);
        assert_eq!(Builder::literal_string("abc").len(), 1);
        let words = Builder::literal_string("main");
        assert_eq!(words[1], 0, "the whole trailing word is the terminator");
    }

    /// A string packs four bytes to a word, little-endian.
    #[test]
    fn a_string_packs_four_bytes_to_a_word_little_endian() {
        let words = Builder::literal_string("abc");
        assert_eq!(words[0], u32::from_le_bytes([b'a', b'b', b'c', 0]));
    }

    /// The byte form of a module is its word form, little-endian.
    #[test]
    fn bytes_and_words_describe_the_same_module() {
        let mut b = Builder::new();
        b.header(op::CAPABILITY, &[1]);
        let words = b.finish();
        let bytes = b.finish_bytes();
        assert_eq!(bytes.len(), words.len() * 4);
        assert_eq!(&bytes[..4], &MAGIC.to_le_bytes());
    }

    /// `check` names an identifier that is used but never defined.
    #[test]
    fn an_array_whose_length_was_never_declared_is_named() {
        // An identifier reserved for the array's length whose `OpConstant` is never emitted: every
        // instruction is well-formed and the module is meaningless, and a driver faults on it.
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

    /// A capability declared after the entry point is still emitted before it.
    #[test]
    fn a_capability_declared_late_is_still_emitted_first() {
        // The format wants every capability before the memory model, which precedes the entry
        // point.
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

    /// A function body may branch to a label defined later.
    #[test]
    fn a_function_body_may_name_a_label_that_appears_later() {
        // A forward branch names a block not yet written, and a loop header names its own merge
        // first, so the ordering half of the check does not apply inside a function (D103).
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

    /// A function body may not name an identifier defined nowhere.
    #[test]
    fn a_function_body_may_not_name_something_defined_nowhere() {
        // Relaxing ordering inside a function does not relax existence: an identifier reserved and
        // never given a meaning is still reported there.
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

    /// A type used before it is declared is named.
    #[test]
    fn a_type_used_before_it_is_declared_is_named() {
        // The declarations section is ordered, so a type may only name one already declared.
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

    /// A decoration may name a variable declared after it.
    #[test]
    fn a_decoration_may_name_a_variable_declared_later() {
        // A decoration necessarily precedes what it decorates, and an entry point names a function
        // declared after it, so those sections are not checked for order.
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

    /// Two instructions cannot claim the same result identifier.
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

    /// Two `uvec3` declarations - a triangle's indices and the `WorkgroupId` input each declaring
    /// its own - are refused: AMD's driver faulted compiling such a mesh module.
    #[test]
    fn a_vector_type_declared_twice_is_refused() {
        let mut b = Builder::new();
        let uint = b.id();
        let (first, second) = (b.id(), b.id());
        b.declare(op::TYPE_INT, &[uint.0, 32, 0]);
        b.declare(op::TYPE_VECTOR, &[first.0, uint.0, 3]);
        b.declare(op::TYPE_VECTOR, &[second.0, uint.0, 3]);

        assert_eq!(
            b.check(),
            Err(ModuleError::DeclaredTwice {
                first: first.0,
                second: second.0,
                opcode: op::TYPE_VECTOR
            })
        );
    }

    /// Asking for a type already declared, by either route, answers the declaration made.
    #[test]
    fn declare_type_answers_the_type_already_declared() {
        let mut b = Builder::new();
        let uint = b.id();
        let uvec3 = b.id();
        b.declare(op::TYPE_INT, &[uint.0, 32, 0]);
        b.declare(op::TYPE_VECTOR, &[uvec3.0, uint.0, 3]);

        assert_eq!(b.declare_type(op::TYPE_VECTOR, &[uint.0, 3]), uvec3);
        let uvec2 = b.declare_type(op::TYPE_VECTOR, &[uint.0, 2]);
        assert_ne!(uvec2, uvec3);
        assert_eq!(b.declare_type(op::TYPE_VECTOR, &[uint.0, 2]), uvec2);
        assert_eq!(b.check(), Ok(()));
    }

    /// Structs and pointers may repeat: two identical blocks are two distinct types.
    #[test]
    fn an_aggregate_or_pointer_may_be_declared_twice() {
        let mut b = Builder::new();
        let uint = b.id();
        let (one, two, p1, p2) = (b.id(), b.id(), b.id(), b.id());
        b.declare(op::TYPE_INT, &[uint.0, 32, 0]);
        b.declare(op::TYPE_STRUCT, &[one.0, uint.0]);
        b.declare(op::TYPE_STRUCT, &[two.0, uint.0]);
        b.declare(op::TYPE_POINTER, &[p1.0, 7, uint.0]);
        b.declare(op::TYPE_POINTER, &[p2.0, 7, uint.0]);

        assert_eq!(b.check(), Ok(()));
    }

    /// An access chain's indices count as identifier uses.
    #[test]
    fn an_access_chain_index_counts_as_an_identifier() {
        // The indices of an access chain are identifiers, not literals; reading them as literals
        // makes a two-index chain look like a one-index one.
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

    /// The minimal module declares exactly one function.
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
