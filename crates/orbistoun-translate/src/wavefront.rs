//! The wavefront model: one invocation simulates every lane.
//!
//! Unlike the lane model, vector registers are arrays indexed by lane and the execution mask is an
//! ordinary value, so cross-lane instructions become array reads and mask arithmetic becomes
//! integer arithmetic, independent of the host subgroup size. Every vector instruction costs one
//! operation per lane; this level exists to be right, not fast. The mask lives in the scalar file
//! at `exec_lo` and `exec_hi`, as two 32-bit halves, so guest accesses to it need no special
//! translation. A masked write is a `select` against the old value rather than a branch, so no
//! structured control flow is needed. The observation layout matches the lane model's (lane zero's
//! vector registers, then scalar registers), so the two levels can be diffed.

use std::collections::BTreeMap;

use orbistoun_shader::{Decode, EncodingTable, Instruction, Operand};
use orbistoun_spirv::{
    Builder, Id, addressing, built_in, capability, decoration, execution, memory, mode, op,
};

use crate::buffer;
use crate::model::{self, Model};
use crate::predicated::{MEMORY_WORDS, OBSERVED_REGISTERS, OBSERVED_WORDS, REGISTER_COUNT};
use crate::{TranslateError, Width};

/// Lanes in a wavefront.
pub const WAVE: u32 = Width::Wave64.lanes();

/// Scalar register holding the low half of the execution mask.
///
/// The architecture reserves these two indices, the same codes `data/operands.toml` names, so guest
/// code that reads or writes the mask needs no special handling.
const EXEC_LO: u32 = 126;
/// Scalar register holding the high half of the execution mask.
const EXEC_HI: u32 = 127;

/// Which half of the execution mask a scalar register is, if it is one: 0 low, 1 high.
const fn exec_half(register: u32) -> Option<usize> {
    match register {
        EXEC_LO => Some(0),
        EXEC_HI => Some(1),
        _ => None,
    }
}

/// Scalar register holding the low half of the condition mask.
///
/// Where a comparison puts its answer; an ordinary register, as the guest treats it.
const VCC_LO: u32 = 106;
/// Scalar register holding the high half of the condition mask.
const VCC_HI: u32 = 107;

/// Private storage class.
const PRIVATE: u32 = 6;

/// Workgroup storage class: shared between the invocations of a workgroup.
///
/// A workgroup is one invocation here, so this behaves like private storage; it is declared as
/// workgroup because the guest's local data share is shared, which matters once a group holds more
/// than one wavefront.
const WORKGROUP: u32 = 4;

/// The `Output` storage class, where a fragment shader's colour lives.
const OUTPUT: u32 = 3;

/// The `Input` storage class, where an interpolated fragment attribute arrives.
const INPUT: u32 = 1;

/// The `UniformConstant` storage class, where a descriptor-bound sampled image lives.
///
/// An image is read through a sampling instruction rather than loaded, so it has its own storage
/// class.
const UNIFORM_CONSTANT: u32 = 0;

/// A texture a module samples, declared on first use because most modules never sample.
///
/// The register numbers do not locate the texture; they let a sample that names a different
/// descriptor pair, or follows a write into the recorded pair, be refused (D690).
#[derive(Debug, Clone, Copy)]
struct BoundTexture {
    /// What a translated sample needs, handed back whole.
    texture: model::Texture,
    /// First scalar register of the image descriptor the guest named.
    descriptor: u32,
    /// First scalar register of the sampler descriptor, when one was named.
    ///
    /// [`None`] until something samples. A fetch names no sampler, so a module that only fetches
    /// never records one and is never compared on it.
    sampler: Option<u32>,
    /// Which of the module's textures this is, and where its descriptor came from.
    source: TextureSource,
    /// Set when a scalar write lands inside either group.
    ///
    /// Register numbers alone would let a shader load a second descriptor into the same registers
    /// and sample two textures while naming one; a write into the group makes the next sample
    /// refuse.
    disturbed: bool,
}

/// Words of local data share a translated module provides.
///
/// A placeholder: the real size is declared per dispatch by the guest. Large enough for tests and
/// cheap.
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
/// The vector file is an array of registers, each an array of lanes; the scalar file is one value
/// per register, uniform across the wavefront. Both are null-initialised, because a private
/// variable is otherwise undefined at entry.
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

/// Which pipeline stage a translated module is built for.
///
/// A compute module's epilogue copies registers into a storage buffer for a test to read. A shader
/// that exports colour needs a graphics pipeline, so it is a fragment module with an output
/// variable (D553). The stages differ in execution model, execution mode, and whether the epilogue
/// writes the observation window; a fragment module's oracle is its attachment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// A compute dispatch whose epilogue publishes the registers. The default.
    Compute,
    /// A fragment shader with a colour output at location zero.
    Fragment,
    /// A mesh shader: one workgroup declares how many vertices and primitives it will emit, writes
    /// the primitive's indices, and writes the vertices' positions and parameters.
    ///
    /// What a guest's primitive shader is (D688): `MSG_GS_ALLOC_REQ` is the declaration, `exp prim`
    /// the indices, and `exp pos`/`exp param` the per-vertex outputs, one lane per vertex.
    Mesh,
}

/// The primitive a mesh module assembles its vertices into.
///
/// The stream sets it in `VGT_GS_OUT_PRIM_TYPE`, `orbistoun-gpu` decodes it, and the caller maps it
/// here; only [`Stage::Mesh`] reads it. Vulkan ignores a mesh pipeline's input-assembly topology,
/// so the module states the shape in three places that must agree: the output execution mode, the
/// per-primitive index built-in, and how many packed `exp prim` indices are read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MeshPrimitive {
    /// One vertex per primitive.
    Points,
    /// Two vertices per primitive.
    Lines,
    /// Three vertices per primitive. The default.
    #[default]
    Triangles,
}

impl MeshPrimitive {
    /// How many of the packed `exp prim` indices this primitive uses, which is also the width of
    /// its index built-in (a `uint`, `uvec2` or `uvec3`).
    #[must_use]
    pub const fn indices(self) -> u32 {
        match self {
            Self::Points => 1,
            Self::Lines => 2,
            Self::Triangles => 3,
        }
    }

    /// The `OutputPoints`/`OutputLinesEXT`/`OutputTrianglesEXT` execution mode declaring the shape.
    const fn output_mode(self) -> u32 {
        match self {
            Self::Points => mode::OUTPUT_POINTS,
            Self::Lines => mode::OUTPUT_LINES_EXT,
            Self::Triangles => mode::OUTPUT_TRIANGLES_EXT,
        }
    }

    /// The `PrimitivePointIndicesEXT`/`Line`/`Triangle` built-in decorating the index array.
    const fn indices_built_in(self) -> u32 {
        match self {
            Self::Points => built_in::PRIMITIVE_POINT_INDICES_EXT,
            Self::Lines => built_in::PRIMITIVE_LINE_INDICES_EXT,
            Self::Triangles => built_in::PRIMITIVE_TRIANGLE_INDICES_EXT,
        }
    }
}

/// How many vertices and primitives a mesh module declares room for.
///
/// One per lane, as the guest arranges a primitive shader's wave. A maximum: the shader emits the
/// runtime counts it declares with `MSG_GS_ALLOC_REQ`.
const MESH_SLOTS: u32 = 64;

/// The outputs a mesh module writes, declared together because they are meaningless apart.
#[derive(Debug, Clone)]
struct MeshOutputs {
    /// The per-vertex block array; member zero of each element carries `Position`.
    vertices: Id,
    /// The primitive index array: one index element per primitive, of [`Self::index_type`].
    indices: Id,
    /// One output array per parameter location the shader exports, by location.
    parameters: BTreeMap<u32, Id>,
    /// Pointer to one `vec4` in an output array.
    vec4_ptr: Id,
    /// Pointer to one index element in the index array.
    index_ptr: Id,
    /// The index element type: a scalar `uint` for a point, a `uvec2`/`uvec3` for a line/triangle.
    index_type: Id,
    /// How many vertex indices one primitive carries: 1, 2 or 3.
    index_width: u32,
}

/// Where the guest-memory window sits in the guest's address space.
///
/// A translated shader reaches guest memory through one storage buffer that is a window: a fixed
/// number of words at a base, with every access checked against it. [`Window::at`] takes the
/// default length and [`Window::spanning`] a chosen one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window {
    /// The low thirty-two bits of the guest address of the window's first word: the half a
    /// translated shader compares against, because its memory accesses read the low half of their
    /// address register pair (see `Model::memory_base`).
    pub base: u32,
    /// How many words it spans. Private so the power-of-two rule in [`Window::spanning`] holds.
    words: u32,
    /// The high thirty-two bits of the window's guest address. A shader never sees it; it is where
    /// the window's words are read from. Zero below four gigabytes (D711).
    high: u32,
}

impl Window {
    /// A window of the default length, anchored at `base`.
    #[must_use]
    pub const fn at(base: u32) -> Self {
        Self {
            base,
            words: MEMORY_WORDS,
            high: 0,
        }
    }

    /// A window of `words` words at `base`, or `None` when `words` is not a power of two.
    ///
    /// [`crate::model::Model::word_index`] masks an index with `words - 1`, which is the bound only
    /// for a power of two, while `address_within_window` compares against the true count. With any
    /// other length the check admits an address the mask folds to a different word, so the length
    /// is refused rather than rounded, and the field is private.
    #[must_use]
    pub const fn spanning(base: u32, words: u32) -> Option<Self> {
        if words == 0 || !words.is_power_of_two() {
            return None;
        }
        Some(Self {
            base,
            words,
            high: 0,
        })
    }

    /// A window of `words` words at a full 64-bit guest address, or `None` when `words` is not a
    /// power of two or the window would cross a four-gigabyte boundary.
    ///
    /// A translated shader compares only the low half of an address, so words past a boundary would
    /// compare below the base and be refused (D711).
    #[must_use]
    pub const fn spanning_address(address: u64, words: u32) -> Option<Self> {
        let Some(window) = Self::spanning(address as u32, words) else {
            return None;
        };
        let end = (address as u32 as u64) + (words as u64) * 4;
        if end > 1 << 32 {
            return None;
        }
        Some(Self {
            high: (address >> 32) as u32,
            ..window
        })
    }

    /// How many words this window spans.
    #[must_use]
    pub const fn words(self) -> u32 {
        self.words
    }

    /// The window's full guest address - where its words are read from.
    #[must_use]
    pub const fn address(self) -> u64 {
        ((self.high as u64) << 32) | self.base as u64
    }
}

impl Default for Window {
    /// The default length at address zero.
    fn default() -> Self {
        Self::at(0)
    }
}

/// Where a stage's user data lands in its scalar registers, and where it sits in the push-constant
/// block a draw supplies it through.
///
/// The hardware loads user data into a stage's first scalar registers before its first instruction;
/// a translated module reads it from the push-constant block at entry. `count` zero reads nothing
/// and declares no block.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UserData {
    /// The first scalar register the words land in: `s0` for a pixel shader, `s8` for the geometry
    /// program a vertex stage runs as.
    pub first_register: u32,
    /// How many words: the stage's `USER_SGPR` count.
    pub count: u32,
    /// Where this stage's words start in the block, in words.
    pub block_offset: u32,
    /// The stage's `DX10_CLAMP` mode bit (`SPI_SHADER_PGM_RSRC1` bit 21, Mesa
    /// `S_00B848_DX10_CLAMP`): whether an output clamp turns a NaN into zero (set) or passes it
    /// through (clear). `None` when no `RSRC1` was seen, and then a clamped instruction is refused.
    pub dx10_clamp: Option<bool>,
}

/// Words in the push-constant block: sixteen per stage, two stages. 128 bytes is the smallest
/// `maxPushConstantsSize` a Vulkan device may report, so every device takes it.
pub const USER_DATA_BLOCK_WORDS: u32 = 32;

/// The most user-data words one stage may take within the block.
pub const USER_DATA_STAGE_WORDS: u32 = 16;

/// How a fragment input is read.
///
/// The guest decides per attribute: `v_interp_p1_f32` and its pair interpolate, and
/// `v_interp_mov_f32` reads a parameter without interpolating. On the host it is a decoration on
/// the input variable, fixed at declaration, so it is found in the same pass that finds the
/// attributes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Interpolation {
    /// Interpolated across the primitive, which is the default and needs no decoration.
    Smooth,
    /// The provoking vertex's value, everywhere. `Flat`.
    Flat,
}

/// Emits the module header: capabilities, the memory model, the entry point and its execution mode,
/// and a fragment output's location.
fn emit_header(
    b: &mut Builder,
    stage: Stage,
    primitive: MeshPrimitive,
    main: Id,
    output: Option<Id>,
) {
    if stage == Stage::Mesh {
        // `MeshShadingEXT` implies `Shader`, and the extension must be declared with it.
        b.header(op::CAPABILITY, &[capability::MESH_SHADING_EXT]);
        let mut extension = Vec::new();
        extension.extend(Builder::literal_string("SPV_EXT_mesh_shader"));
        b.header(op::EXTENSION, &extension);
    } else {
        b.header(op::CAPABILITY, &[capability::SHADER]);
    }

    b.header(op::MEMORY_MODEL, &[addressing::LOGICAL, memory::GLSL450]);

    match stage {
        Stage::Compute => {
            b.header(op::EXECUTION_MODE, &[main.0, mode::LOCAL_SIZE, 1, 1, 1]);
        }
        // Required of every fragment entry point: Vulkan's framebuffer origin is the top left.
        Stage::Fragment => {
            b.header(op::EXECUTION_MODE, &[main.0, mode::ORIGIN_UPPER_LEFT]);
        }
        // One invocation stands in for the guest's whole wave; the maximum output is the lane
        // count, since the guest arranges one vertex per lane. The actual count is the runtime
        // value declared with `MSG_GS_ALLOC_REQ`.
        Stage::Mesh => {
            b.header(op::EXECUTION_MODE, &[main.0, mode::LOCAL_SIZE, 1, 1, 1]);
            b.header(
                op::EXECUTION_MODE,
                &[main.0, mode::OUTPUT_VERTICES, MESH_SLOTS],
            );
            b.header(
                op::EXECUTION_MODE,
                &[main.0, mode::OUTPUT_PRIMITIVES_EXT, MESH_SLOTS],
            );
            // The shape the stream asked for: Vulkan reads the mesh output topology from here,
            // never from the pipeline's input assembly (D688).
            b.header(op::EXECUTION_MODE, &[main.0, primitive.output_mode()]);
        }
    }
    if let Some(colour) = output {
        // Location zero is colour attachment zero, the attachment a render pass lists first.
        b.annotate(op::DECORATE, &[colour.0, decoration::LOCATION, 0]);
    }
}

/// Writes the entry point, which names every variable the stage requires it to.
///
/// Written after the declarations: the builder orders header instructions by opcode, so it still
/// lands ahead of the execution modes. From SPIR-V 1.4 the interface lists every global variable,
/// which do not exist until declared; below 1.4 it lists only inputs and outputs, and listing more
/// is invalid.
fn emit_entry_point(b: &mut Builder, stage: Stage, main: Id, interface: &[u32]) {
    let model = match stage {
        Stage::Compute => execution::GL_COMPUTE,
        Stage::Fragment => execution::FRAGMENT,
        Stage::Mesh => execution::MESH_EXT,
    };
    let mut entry = vec![model, main.0];
    entry.extend(Builder::literal_string("main"));
    entry.extend_from_slice(interface);
    b.header(op::ENTRY_POINT, &entry);
}

/// How many guest lanes one invocation of a module for `stage` simulates.
///
/// One at the fragment stage, where one invocation is one pixel: the host rasteriser decides
/// coverage, the interpolated inputs are this pixel's, and the export reads lane zero. No
/// translated instruction reads another lane's registers, so lane zero's result is unchanged.
/// Branch tests on whether any lane survives then ask it of this pixel alone (`control` clamps them
/// to [`Model::lanes`]). Every other stage keeps the whole wavefront: a compute dispatch's lanes
/// are its threads, and a mesh module's are its vertices (D688).
fn simulated_lanes(stage: Stage, width: Width) -> u32 {
    match stage {
        Stage::Fragment => 1,
        Stage::Compute | Stage::Mesh => width.lanes(),
    }
}

/// Declares the four size constants the module's arrays and buffers are built from.
fn declare_counts(b: &mut Builder, u32_type: Id, counts: [Id; 4], lanes: u32, memory_words: u32) {
    let [wave, registers, observed, memory] = counts;
    b.declare(op::CONSTANT, &[u32_type.0, wave.0, lanes]);
    b.declare(op::CONSTANT, &[u32_type.0, registers.0, REGISTER_COUNT]);
    b.declare(op::CONSTANT, &[u32_type.0, observed.0, OBSERVED_WORDS]);
    // The declared array and the bounds check use the same number: `Model::word_index` masks with
    // `memory_words - 1` and `address_within_window` compares against `memory_words`, so a
    // different array length would let an index past the buffer through.
    b.declare(op::CONSTANT, &[u32_type.0, memory.0, memory_words]);
}

/// Declares the two storage buffers every module binds.
///
/// Guest memory has its own binding so a guest address cannot reach the observation window (D101).
fn declare_buffers(
    b: &mut Builder,
    u32_type: Id,
    observed_count: Id,
    memory_count: Id,
) -> (buffer::StorageBuffer, buffer::StorageBuffer) {
    (
        buffer::declare(b, u32_type, observed_count, buffer::OBSERVATION),
        buffer::declare(b, u32_type, memory_count, buffer::GUEST_MEMORY),
    )
}

/// Reserves one identifier per interpolated attribute, before the header is written.
///
/// Separate from declaring them because the entry point's interface names the identifiers before
/// the variables are declared.
fn reserve_attribute_inputs(
    b: &mut Builder,
    stage: Stage,
    attributes: &[(u32, Interpolation)],
) -> Vec<(u32, Interpolation, Id)> {
    if stage != Stage::Fragment {
        return Vec::new();
    }
    attributes
        .iter()
        .map(|(attr, how)| (*attr, *how, b.id()))
        .collect()
}

/// Declares one `vec4` fragment input per attribute the shader interpolates.
///
/// Location `n` is attribute `n`: the attribute index is an operand of the instruction, and a
/// fragment input's location names the same slot. Empty for a compute module.
fn declare_attribute_inputs(
    b: &mut Builder,
    vec4: Id,
    reserved: &[(u32, Interpolation, Id)],
) -> BTreeMap<u32, Id> {
    let mut inputs = BTreeMap::new();
    if reserved.is_empty() {
        return inputs;
    }
    let input_ptr = b.id();
    b.declare(op::TYPE_POINTER, &[input_ptr.0, INPUT, vec4.0]);
    for (attribute, how, variable) in reserved {
        b.annotate(
            op::DECORATE,
            &[variable.0, decoration::LOCATION, *attribute],
        );
        // Smooth is what an undecorated input does, so it takes no decoration.
        if *how == Interpolation::Flat {
            b.annotate(op::DECORATE, &[variable.0, decoration::FLAT]);
        }
        b.declare(op::VARIABLE, &[input_ptr.0, variable.0, INPUT]);
        inputs.insert(*attribute, *variable);
    }
    inputs
}

/// Declares the `vec4` colour output a fragment module exports to, and answers it with its type.
///
/// [`None`] for a compute module.
fn declare_colour_output(
    b: &mut Builder,
    f32_type: Id,
    vec4: Id,
    output_ptr: Id,
    output: Option<Id>,
) -> Option<(Id, Id)> {
    let colour = output?;
    b.declare(op::TYPE_VECTOR, &[vec4.0, f32_type.0, 4]);
    b.declare(op::TYPE_POINTER, &[output_ptr.0, OUTPUT, vec4.0]);
    b.declare(op::VARIABLE, &[output_ptr.0, colour.0, OUTPUT]);
    Some((vec4, colour))
}

/// Declares everything a mesh module writes: the vertices, the indices, and one array per exported
/// parameter. [`None`] for any other stage.
///
/// The per-vertex outputs are an array of a `Block` struct whose member zero carries `Position`, as
/// the stage requires; a bare `vec4` array decorated `Position` is a vertex-shader shape and is
/// refused.
fn declare_mesh_outputs(
    b: &mut Builder,
    f32_type: Id,
    u32_type: Id,
    vec4: Id,
    stage: Stage,
    primitive: MeshPrimitive,
    reserved: &MeshReserved,
) -> Option<MeshOutputs> {
    if stage != Stage::Mesh {
        return None;
    }
    let slots = b.id();
    b.declare(op::CONSTANT, &[u32_type.0, slots.0, MESH_SLOTS]);
    b.declare(op::TYPE_VECTOR, &[vec4.0, f32_type.0, 4]);
    // The index element carries one vertex index per component: `uint` for a point, `uvec2` for a
    // line, `uvec3` for a triangle, matching the `PrimitivePointIndices`/`Line`/`Triangle`
    // built-in.
    let index_width = primitive.indices();
    let index_type = if index_width == 1 {
        u32_type
    } else {
        let v = b.id();
        b.declare(op::TYPE_VECTOR, &[v.0, u32_type.0, index_width]);
        v
    };

    let per_vertex = b.id();
    b.annotate(op::DECORATE, &[per_vertex.0, decoration::BLOCK]);
    b.annotate(
        op::MEMBER_DECORATE,
        &[per_vertex.0, 0, decoration::BUILT_IN, built_in::POSITION],
    );
    b.annotate(
        op::DECORATE,
        &[
            reserved.indices.0,
            decoration::BUILT_IN,
            primitive.indices_built_in(),
        ],
    );
    for (location, id) in &reserved.parameters {
        b.annotate(op::DECORATE, &[id.0, decoration::LOCATION, *location]);
    }

    b.declare(op::TYPE_STRUCT, &[per_vertex.0, vec4.0]);
    let vertex_array = b.id();
    let vertex_array_ptr = b.id();
    b.declare(op::TYPE_ARRAY, &[vertex_array.0, per_vertex.0, slots.0]);
    b.declare(
        op::TYPE_POINTER,
        &[vertex_array_ptr.0, OUTPUT, vertex_array.0],
    );
    b.declare(
        op::VARIABLE,
        &[vertex_array_ptr.0, reserved.vertices.0, OUTPUT],
    );

    let index_array = b.id();
    let index_array_ptr = b.id();
    b.declare(op::TYPE_ARRAY, &[index_array.0, index_type.0, slots.0]);
    b.declare(
        op::TYPE_POINTER,
        &[index_array_ptr.0, OUTPUT, index_array.0],
    );
    b.declare(
        op::VARIABLE,
        &[index_array_ptr.0, reserved.indices.0, OUTPUT],
    );

    let parameter_array = b.id();
    let parameter_array_ptr = b.id();
    b.declare(op::TYPE_ARRAY, &[parameter_array.0, vec4.0, slots.0]);
    b.declare(
        op::TYPE_POINTER,
        &[parameter_array_ptr.0, OUTPUT, parameter_array.0],
    );
    let mut parameters = BTreeMap::new();
    for (location, id) in &reserved.parameters {
        b.declare(op::VARIABLE, &[parameter_array_ptr.0, id.0, OUTPUT]);
        parameters.insert(*location, *id);
    }

    let vec4_ptr = b.id();
    let index_ptr = b.id();
    b.declare(op::TYPE_POINTER, &[vec4_ptr.0, OUTPUT, vec4.0]);
    b.declare(op::TYPE_POINTER, &[index_ptr.0, OUTPUT, index_type.0]);

    Some(MeshOutputs {
        vertices: reserved.vertices,
        indices: reserved.indices,
        parameters,
        vec4_ptr,
        index_ptr,
        index_type,
        index_width,
    })
}

/// The identifiers a mesh module's outputs need before the entry point is written.
///
/// From SPIR-V 1.4, which the mesh extension requires, an entry point must list every global
/// variable.
#[derive(Debug, Clone)]
struct MeshReserved {
    vertices: Id,
    indices: Id,
    parameters: BTreeMap<u32, Id>,
}

impl MeshReserved {
    /// Reserves ids for a mesh module's outputs, or nothing at another stage.
    fn new(b: &mut Builder, stage: Stage, locations: &[u32]) -> Self {
        if stage != Stage::Mesh {
            return Self {
                vertices: Id(0),
                indices: Id(0),
                parameters: BTreeMap::new(),
            };
        }
        Self {
            vertices: b.id(),
            indices: b.id(),
            parameters: locations
                .iter()
                .map(|location| (*location, b.id()))
                .collect(),
        }
    }

    /// Every variable the entry point must list.
    fn interface(&self, stage: Stage) -> Vec<u32> {
        if stage != Stage::Mesh {
            return Vec::new();
        }
        let mut ids = vec![self.vertices.0, self.indices.0];
        ids.extend(self.parameters.values().map(|id| id.0));
        ids
    }
}

/// Declares the local data share: workgroup storage the lanes of a wavefront exchange values
/// through.
fn declare_local_share(
    b: &mut Builder,
    u32_type: Id,
    local_count: Id,
    local_array: Id,
    local_array_ptr: Id,
    local_ptr: Id,
    local: Id,
) {
    // No initialiser: workgroup storage cannot have one, and the guest's is uninitialised too.
    b.declare(op::CONSTANT, &[u32_type.0, local_count.0, LOCAL_WORDS]);
    b.declare(op::TYPE_ARRAY, &[local_array.0, u32_type.0, local_count.0]);
    b.declare(
        op::TYPE_POINTER,
        &[local_array_ptr.0, WORKGROUP, local_array.0],
    );
    b.declare(op::TYPE_POINTER, &[local_ptr.0, WORKGROUP, u32_type.0]);
    b.declare(op::VARIABLE, &[local_array_ptr.0, local.0, WORKGROUP]);
}

/// Where a stage's user data is read from at entry.
#[derive(Debug, Clone, Copy)]
enum UserDataSource {
    /// The push-constant block, at the stage's share of it.
    Push(buffer::StorageBuffer),
    /// The draw-data buffer, at this workgroup's stride (D718): a mesh module, one workgroup per
    /// guest draw of a batch. `workgroup` is the `uvec3` type and the `WorkgroupId` input.
    Draws {
        buffer: buffer::StorageBuffer,
        workgroup: (Id, Id),
    },
}

impl UserDataSource {
    /// The variables it adds to the entry point's interface.
    fn interface(self) -> Vec<u32> {
        match self {
            Self::Push(block) => vec![block.buffer.0],
            Self::Draws { buffer, workgroup } => vec![buffer.buffer.0, workgroup.1.0],
        }
    }
}

/// Declares the `WorkgroupId` built-in input: the `uvec3` type, and the variable.
fn declare_workgroup_id(b: &mut Builder, u32_type: Id) -> (Id, Id) {
    let uvec3 = b.id();
    let pointer = b.id();
    let variable = b.id();
    b.annotate(
        op::DECORATE,
        &[variable.0, decoration::BUILT_IN, built_in::WORKGROUP_ID],
    );
    b.declare(op::TYPE_VECTOR, &[uvec3.0, u32_type.0, 3]);
    b.declare(op::TYPE_POINTER, &[pointer.0, INPUT, uvec3.0]);
    b.declare(op::VARIABLE, &[pointer.0, variable.0, INPUT]);
    (uvec3, variable)
}

/// Builds a wavefront-model module for one decoded shader.
#[derive(Debug)]
pub struct Wavefront<'a> {
    /// Which stage this module is for, and - when it is a fragment - where a colour goes.
    stage: Stage,
    /// The `vec4` output variable and its type, declared only for [`Stage::Fragment`].
    output: Option<(Id, Id)>,
    /// The fragment input for each attribute the shader interpolates, by attribute index.
    ///
    /// Declared in the header, so the attributes are found by a pass over the decode before any
    /// instruction is translated.
    inputs: BTreeMap<u32, Id>,
    /// What a mesh module writes, or [`None`] at any other stage.
    mesh: Option<MeshOutputs>,
    /// The primitive a mesh module assembles. Read only at [`Stage::Mesh`].
    primitive: MeshPrimitive,
    /// The four-component float vector, which the stages that have one share.
    vec4: Id,
    /// The guest address the memory window starts at. See [`Model::memory_base`].
    memory_base: u32,
    /// How many words of guest memory this module addresses.
    ///
    /// Carried rather than a constant so a test can widen the window and reach addresses the
    /// default cannot hold.
    memory_words: u32,
    builder: Builder,
    encodings: &'a EncodingTable,
    /// How many lanes one invocation simulates; see [`simulated_lanes`].
    lanes: u32,
    /// Each half of the execution mask, where the instructions since the start of this block wrote
    /// it a constant, as a primitive shader does to pick its threads (`s_mov_b32 exec_lo, 1`, then
    /// `7`). A lane known inactive emits nothing, and one known active writes without a select.
    known_exec: [Option<u32>; 2],
    constants: BTreeMap<u32, Id>,
    /// The imported `GLSL.std.450` set id, cached after the first extended instruction imports it.
    glsl_set: Option<Id>,
    u32_type: Id,
    f32_type: Id,
    /// The sixteen-bit types, declared on first use along with their capabilities.
    ///
    /// [`None`] until a typed buffer load of a half-format channel needs them, so other modules ask
    /// the device for neither feature.
    f16_type: Option<Id>,
    u16_type: Option<Id>,
    /// The sampled images, each declared the first time an instruction samples it, at most two, in
    /// first-sample order.
    ///
    /// Empty for a module that does not sample, so its pipeline layout carries no image binding.
    textures: Vec<BoundTexture>,
    /// Each descriptor register group's descriptor-table offsets, from [`descriptor_table_loads`].
    descriptor_loads: BTreeMap<u32, std::collections::BTreeSet<u32>>,
    /// The storage image, declared the first time an instruction stores to one.
    ///
    /// Declaring it declares a capability that a device without the matching feature refuses, so a
    /// module that never stores never declares one (D692). The image descriptor register it was
    /// first named with rides along, so stores follow the same register rules as samples.
    stored: Option<(model::Stored, u32)>,
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
    /// The `m0` register, as a private word starting at zero.
    m0: Id,
    translated: usize,
    /// The stage's `DX10_CLAMP` mode, from its `RSRC1`; `None` when unknown.
    dx10_clamp: Option<bool>,
}

impl Wavefront<'_> {
    /// The texture sources this module samples, in slot order.
    #[must_use]
    pub fn texture_sources(&self) -> Vec<TextureSource> {
        self.textures.iter().map(|bound| bound.source).collect()
    }

    /// The source of a texture first sampled through `descriptor`: the next slot, and the table
    /// offset the descriptor was loaded from.
    ///
    /// Refused where the texture cannot be identified: a third texture (two bindings exist), a
    /// descriptor loaded from more than one offset, or a second texture when either did not come
    /// from the table. A pipeline finds a texture by its offset, so binding offset zero to both
    /// would draw the wrong image.
    fn new_texture_source(&self, descriptor: u32) -> Result<TextureSource, &'static str> {
        let slot = u32::try_from(self.textures.len()).unwrap_or(u32::MAX);
        if slot > 1 {
            return Err("this shader reads more than two textures, and two bindings exist (D690)");
        }
        let table_offset = match self.descriptor_loads.get(&descriptor) {
            None => None,
            Some(offsets) if offsets.len() == 1 => offsets.first().copied(),
            Some(_) => {
                return Err(concat!(
                    "this shader loads one image descriptor from more than one place in its ",
                    "descriptor table, so which texture a sample reads is not fixed (D690)"
                ));
            }
        };
        let first_resolved = self
            .textures
            .first()
            .is_none_or(|first| first.source.table_offset.is_some());
        if slot == 1 && (table_offset.is_none() || !first_resolved) {
            return Err(concat!(
                "this shader reads two textures and at least one descriptor did not come from ",
                "its descriptor table, so which is which cannot be told (D690)"
            ));
        }
        Ok(TextureSource { slot, table_offset })
    }
}

/// The identifiers a module reserves first, in the order it reserves them.
#[derive(Debug, Clone, Copy)]
struct Reserved {
    void: Id,
    fn_type: Id,
    u32_type: Id,
    f32_type: Id,
    bool_type: Id,
    main: Id,
    entry_block: Id,
    wave_count: Id,
    register_count: Id,
    observed_count: Id,
    memory_count: Id,
    counter_ptr: Id,
    counter: Id,
    counter_zero: Id,
    scc: Id,
    m0: Id,
    local_count: Id,
    local_array: Id,
    local_array_ptr: Id,
    local_ptr: Id,
    local: Id,
}

impl Reserved {
    /// Reserves every identifier, in field order.
    fn new(b: &mut Builder) -> Self {
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
        let m0 = b.id();
        let local_count = b.id();
        let local_array = b.id();
        let local_array_ptr = b.id();
        let local_ptr = b.id();
        let local = b.id();
        Self {
            void,
            fn_type,
            u32_type,
            f32_type,
            bool_type,
            main,
            entry_block,
            wave_count,
            register_count,
            observed_count,
            memory_count,
            counter_ptr,
            counter,
            counter_zero,
            scc,
            m0,
            local_count,
            local_array,
            local_array_ptr,
            local_ptr,
            local,
        }
    }
}

/// Declares the private words, the local share, the counts, the register files and the two
/// storage buffers.
fn declare_state(
    b: &mut Builder,
    ids: &Reserved,
    stage: Stage,
    width: Width,
    window: Window,
) -> (Files, buffer::StorageBuffer, buffer::StorageBuffer) {
    let u32_type = ids.u32_type;
    let (counter_ptr, counter_zero) = (ids.counter_ptr, ids.counter_zero);
    // The program counter, the scalar condition code and m0: one private word each, sharing a
    // pointer type and a zero initialiser, so the shader starts at its first block.
    b.declare(op::TYPE_POINTER, &[counter_ptr.0, PRIVATE, u32_type.0]);
    b.declare(op::CONSTANT, &[u32_type.0, counter_zero.0, 0]);
    for word in [ids.counter, ids.scc, ids.m0] {
        b.declare(
            op::VARIABLE,
            &[counter_ptr.0, word.0, PRIVATE, counter_zero.0],
        );
    }

    declare_local_share(
        b,
        u32_type,
        ids.local_count,
        ids.local_array,
        ids.local_array_ptr,
        ids.local_ptr,
        ids.local,
    );
    declare_counts(
        b,
        u32_type,
        [
            ids.wave_count,
            ids.register_count,
            ids.observed_count,
            ids.memory_count,
        ],
        simulated_lanes(stage, width),
        window.words(),
    );

    let files = declare_files(b, u32_type, ids.register_count, ids.wave_count);
    let (observation, guest_memory) =
        declare_buffers(b, u32_type, ids.observed_count, ids.memory_count);
    (files, observation, guest_memory)
}

/// The variables a 1.4 mesh module's entry point names beyond its inputs and outputs.
fn mesh_interface(
    ids: &Reserved,
    files: &Files,
    (observation, guest_memory): (&buffer::StorageBuffer, &buffer::StorageBuffer),
    user_data_source: Option<UserDataSource>,
) -> Vec<u32> {
    let mut interface = vec![
        ids.counter.0,
        ids.scc.0,
        ids.m0.0,
        ids.local.0,
        files.vectors.0,
        files.scalars.0,
        observation.buffer.0,
        guest_memory.buffer.0,
    ];
    if let Some(source) = user_data_source {
        interface.extend(source.interface());
    }
    interface
}

/// Declares the block a draw's user data arrives in, when the stage reads any.
fn declare_user_data_source(
    b: &mut Builder,
    stage: Stage,
    u32_type: Id,
    user_data: UserData,
) -> Option<UserDataSource> {
    // Declared only when this stage reads user data, so a module that reads none is unchanged.
    //
    // A mesh module reads its words per draw (D718): one host dispatch carries a run of guest
    // draws, one workgroup each, so the words come from the draw-data buffer at the workgroup's
    // stride.
    (user_data.count > 0).then(|| {
        if stage == Stage::Mesh {
            let words = b.id();
            b.declare(
                op::CONSTANT,
                &[
                    u32_type.0,
                    words.0,
                    orbistoun_spirv::DRAW_DATA_MOST_DRAWS * orbistoun_spirv::DRAW_DATA_STRIDE_WORDS,
                ],
            );
            let buffer = buffer::declare(b, u32_type, words, orbistoun_spirv::DRAW_DATA_BINDING);
            UserDataSource::Draws {
                buffer,
                workgroup: declare_workgroup_id(b, u32_type),
            }
        } else {
            let words = b.id();
            b.declare(op::CONSTANT, &[u32_type.0, words.0, USER_DATA_BLOCK_WORDS]);
            UserDataSource::Push(buffer::declare_push_constants(b, u32_type, words))
        }
    })
}

impl<'a> Wavefront<'a> {
    /// Prepares the module and sets every lane active.
    pub fn new(encodings: &'a EncodingTable, width: Width) -> Self {
        Self::for_stage(
            encodings,
            width,
            Stage::Compute,
            MeshPrimitive::default(),
            &[],
            &[],
            (Window::default(), UserData::default()),
        )
    }

    /// Prepares the module for a named stage.
    pub fn for_stage(
        encodings: &'a EncodingTable,
        width: Width,
        stage: Stage,
        primitive: MeshPrimitive,
        attributes: &[(u32, Interpolation)],
        parameters: &[u32],
        (window, user_data): (Window, UserData),
    ) -> Self {
        // A mesh module declares 1.4, which its extension requires; everything else is 1.3, where
        // an entry point lists only its inputs and outputs.
        let version = if stage == Stage::Mesh {
            orbistoun_spirv::VERSION_1_4
        } else {
            orbistoun_spirv::VERSION_1_3
        };
        let mut b = Builder::new().with_version(version);

        let ids = Reserved::new(&mut b);
        let (void, fn_type, main) = (ids.void, ids.fn_type, ids.main);
        let (u32_type, f32_type, bool_type) = (ids.u32_type, ids.f32_type, ids.bool_type);

        // The colour output of a fragment module, reserved before the entry point because its
        // interface must name it.
        let output = match stage {
            Stage::Compute | Stage::Mesh => None,
            Stage::Fragment => Some(b.id()),
        };
        let vec4 = b.id();
        let output_ptr = b.id();
        // Reserved before the header for the same reason: every input the entry point touches must
        // be named in its interface, and a driver does not reliably reject a module that omits one.
        let input_ids = reserve_attribute_inputs(&mut b, stage, attributes);
        let mesh_reserved = MeshReserved::new(&mut b, stage, parameters);
        emit_header(&mut b, stage, primitive, main, output);

        b.declare(op::TYPE_VOID, &[void.0]);
        b.declare(op::TYPE_FUNCTION, &[fn_type.0, void.0]);
        b.declare(op::TYPE_INT, &[u32_type.0, 32, 0]);
        b.declare(op::TYPE_FLOAT, &[f32_type.0, 32]);

        b.declare(op::TYPE_BOOL, &[bool_type.0]);
        let output = declare_colour_output(&mut b, f32_type, vec4, output_ptr, output);
        let inputs = declare_attribute_inputs(&mut b, vec4, &input_ids);
        let mesh = declare_mesh_outputs(
            &mut b,
            f32_type,
            u32_type,
            vec4,
            stage,
            primitive,
            &mesh_reserved,
        );
        let (files, observation, guest_memory) = declare_state(&mut b, &ids, stage, width, window);
        let user_data_source = declare_user_data_source(&mut b, stage, u32_type, user_data);

        // Every variable this module has. From 1.4 the entry point names all of them; below that
        // only the inputs and outputs.
        let mut interface: Vec<u32> = input_ids.iter().map(|(_, _, id)| id.0).collect();
        interface.extend(output.map(|(_, colour)| colour.0));
        interface.extend(mesh_reserved.interface(stage));
        if stage == Stage::Mesh {
            interface.extend(mesh_interface(
                &ids,
                &files,
                (&observation, &guest_memory),
                user_data_source,
            ));
        }
        emit_entry_point(&mut b, stage, main, &interface);

        b.function(op::FUNCTION, &[void.0, main.0, 0, fn_type.0]);
        b.function(op::LABEL, &[ids.entry_block.0]);

        let mut this = Self {
            stage,
            output,
            inputs,
            mesh,
            primitive,
            vec4,
            memory_base: window.base,
            dx10_clamp: user_data.dx10_clamp,
            builder: b,
            encodings,
            memory_words: window.words(),
            lanes: simulated_lanes(stage, width),
            known_exec: [None; 2],
            constants: BTreeMap::new(),
            glsl_set: None,
            u32_type,
            f32_type,
            f16_type: None,
            u16_type: None,
            textures: Vec::new(),
            descriptor_loads: BTreeMap::new(),
            stored: None,
            bool_type,
            lane_ptr: files.lane_ptr,
            scalar_ptr: files.scalar_ptr,
            vectors: files.vectors,
            scalars: files.scalars,
            buffer_element_ptr: observation.element_ptr,
            buffer: observation.buffer,
            memory_element_ptr: guest_memory.element_ptr,
            memory: guest_memory.buffer,
            local: ids.local,
            local_ptr: ids.local_ptr,
            program_counter: ids.counter,
            condition_code: ids.scc,
            m0: ids.m0,
            translated: 0,
        };

        // Every lane runs at entry; a zero mask would discard every write and produce a buffer of
        // zeros.
        let all = this.constant(u32::MAX);
        this.store_scalar(EXEC_LO, all);
        this.store_scalar(EXEC_HI, all);

        // The user data where the hardware puts it: word `i` of this stage's range in the block
        // into `s[first + i]`, before the first instruction runs.
        if let Some(source) = user_data_source {
            this.load_user_data(source, user_data);
        }
        this
    }

    /// Loads each user-data word from `source` into its scalar register, at entry.
    fn load_user_data(&mut self, source: UserDataSource, user_data: UserData) {
        let u32_type = self.u32_type;
        let member = self.constant(0);
        // Where this stage's words begin: its share of the push-constant block, or this workgroup's
        // stride of the draw-data buffer (D718).
        let (block, first_word) = match source {
            UserDataSource::Push(block) => (block, None),
            UserDataSource::Draws { buffer, workgroup } => {
                let (uvec3, input) = workgroup;
                let id = self.builder.id();
                self.builder.function(op::LOAD, &[uvec3.0, id.0, input.0]);
                let x = self.builder.id();
                self.builder
                    .function(op::COMPOSITE_EXTRACT, &[u32_type.0, x.0, id.0, 0]);
                let stride = self.constant(orbistoun_spirv::DRAW_DATA_STRIDE_WORDS);
                let first = self.builder.id();
                self.builder
                    .function(op::IMUL, &[u32_type.0, first.0, x.0, stride.0]);
                (buffer, Some(first))
            }
        };
        for i in 0..user_data.count {
            let index = match first_word {
                None => self.constant(user_data.block_offset + i),
                Some(first) => {
                    let within = self.constant(i);
                    let index = self.builder.id();
                    self.builder
                        .function(op::IADD, &[u32_type.0, index.0, first.0, within.0]);
                    index
                }
            };
            let pointer = self.builder.id();
            self.builder.function(
                op::ACCESS_CHAIN,
                &[
                    block.element_ptr.0,
                    pointer.0,
                    block.buffer.0,
                    member.0,
                    index.0,
                ],
            );
            let value = self.builder.id();
            self.builder
                .function(op::LOAD, &[u32_type.0, value.0, pointer.0]);
            self.store_scalar(user_data.first_register + i, value);
        }
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
    /// Both masks are ordinary scalar registers here, so `s_and_b64 exec, exec, vcc` is two
    /// register reads, two ands and two register writes.
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
        if let Some(known) = self.known_exec_half(register) {
            return self.constant(known);
        }
        let pointer = self.scalar_pointer(register);
        let loaded = self.builder.id();
        self.builder
            .function(op::LOAD, &[self.u32_type.0, loaded.0, pointer.0]);
        loaded
    }

    fn store_scalar(&mut self, register: u32, value: Id) {
        if let Some(half) = exec_half(register) {
            self.known_exec[half] = self.constant_value(value);
        }
        let pointer = self.scalar_pointer(register);
        self.builder.function(op::STORE, &[pointer.0, value.0]);
    }

    /// The value a half of the execution mask is known to hold here, if it is known.
    fn known_exec_half(&self, register: u32) -> Option<u32> {
        exec_half(register).and_then(|half| self.known_exec[half])
    }

    /// The value `id` was declared as, when it is one of this module's constants.
    fn constant_value(&self, id: Id) -> Option<u32> {
        self.constants
            .iter()
            .find_map(|(value, constant)| (*constant == id).then_some(*value))
    }

    /// Whether `lane`'s bit of the execution mask is known, and if so whether it is set.
    fn lane_known(&self, lane: u32) -> Option<bool> {
        let (half, bit) = if lane < 32 { (0, lane) } else { (1, lane - 32) };
        self.known_exec[half].map(|mask| mask >> bit & 1 != 0)
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

    /// Whether a lane is active, as a boolean, read from the mask half that holds its bit.
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

    /// Stores four components as a `vec4` through a pointer.
    ///
    /// Unmasked, because a mesh module's output storage must not be read and a select needs the old
    /// value. This is safe when the emitted vertices are the low lanes, as a primitive shader
    /// arranges them: it narrows the mask to `(1 << n) - 1`, so an inactive lane's slot is past the
    /// declared count and never read. A sparse vertex mask would write a vertex it did not mean to
    /// emit; the mask is a runtime value, so this is an assumption rather than a check (D688).
    fn store_vec4(&mut self, pointer: Id, components: [Id; 4]) {
        let vec4 = self.vec4;
        let value = self.builder.id();
        self.builder.function(
            op::COMPOSITE_CONSTRUCT,
            &[
                vec4.0,
                value.0,
                components[0].0,
                components[1].0,
                components[2].0,
                components[3].0,
            ],
        );
        self.builder.function(op::STORE, &[pointer.0, value.0]);
    }

    fn store_lane_masked(&mut self, register: u32, lane: u32, value: Id) {
        match self.lane_known(lane) {
            // Known inactive: the write does not happen.
            Some(false) => return,
            // Known active: nothing to keep, so no select and no load of the old value.
            Some(true) => {
                let pointer = self.lane_pointer(register, lane);
                self.builder.function(op::STORE, &[pointer.0, value.0]);
                return;
            }
            None => {}
        }
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
    /// Lane zero of each vector register, then the scalar registers: the lane model's layout, so
    /// the two can be diffed.
    pub fn finish(mut self) -> Result<(Vec<u32>, usize), TranslateError> {
        // Only a compute module publishes its registers: the epilogue is the compute harness's
        // oracle, and a graphics module's oracle is its attachment (D553).
        if self.stage == Stage::Compute {
            let member = self.constant(0);
            for register in 0..OBSERVED_REGISTERS {
                let value = self.load_lane(register, 0);
                self.write_observation(member, register, value);
            }
            for register in 0..OBSERVED_REGISTERS {
                let value = self.load_scalar(register);
                self.write_observation(member, OBSERVED_REGISTERS + register, value);
            }
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

    fn dx10_clamp(&self) -> Option<bool> {
        self.dx10_clamp
    }

    fn colour_output(&self) -> Option<(Id, Id)> {
        self.output
    }

    fn set_mesh_outputs(&mut self, vertices: Id, primitives: Id) -> Option<()> {
        self.mesh.as_ref()?;
        self.builder
            .function(op::SET_MESH_OUTPUTS_EXT, &[vertices.0, primitives.0]);
        Some(())
    }

    fn write_mesh_position(&mut self, lane: u32, components: [Id; 4]) -> Option<()> {
        let mesh = self.mesh.clone()?;
        let slot = Self::constant(self, lane);
        let member = Self::constant(self, 0);
        let pointer = self.builder.id();
        self.builder.function(
            op::ACCESS_CHAIN,
            &[
                mesh.vec4_ptr.0,
                pointer.0,
                mesh.vertices.0,
                slot.0,
                member.0,
            ],
        );
        self.store_vec4(pointer, components);
        Some(())
    }

    fn write_mesh_parameter(
        &mut self,
        location: u32,
        lane: u32,
        components: [Id; 4],
    ) -> Option<()> {
        let mesh = self.mesh.clone()?;
        let variable = *mesh.parameters.get(&location)?;
        let slot = Self::constant(self, lane);
        let pointer = self.builder.id();
        self.builder.function(
            op::ACCESS_CHAIN,
            &[mesh.vec4_ptr.0, pointer.0, variable.0, slot.0],
        );
        self.store_vec4(pointer, components);
        Some(())
    }

    fn write_mesh_indices(&mut self, lane: u32, indices: &[Id]) -> Option<()> {
        let mesh = self.mesh.clone()?;
        let slot = Self::constant(self, lane);
        let pointer = self.builder.id();
        self.builder.function(
            op::ACCESS_CHAIN,
            &[mesh.index_ptr.0, pointer.0, mesh.indices.0, slot.0],
        );

        // A point's index element is the scalar index; a line or triangle composes a vector of two
        // or three. The caller supplies exactly `index_width` of them.
        let value = if mesh.index_width == 1 {
            indices[0]
        } else {
            let composite = self.builder.id();
            let mut operands = vec![mesh.index_type.0, composite.0];
            operands.extend(indices.iter().map(|id| id.0));
            self.builder.function(op::COMPOSITE_CONSTRUCT, &operands);
            composite
        };
        // Unmasked, as `store_vec4` explains: a mesh module's outputs cannot be read.
        self.builder.function(op::STORE, &[pointer.0, value.0]);
        Some(())
    }

    fn mesh_primitive(&self) -> MeshPrimitive {
        self.primitive
    }

    fn attribute_input(&self, attribute: u32) -> Option<(Id, Id)> {
        let (vec4, _) = self.output?;
        self.inputs.get(&attribute).map(|input| (vec4, *input))
    }

    /// The whole wavefront: one invocation stands in for every lane.
    fn memory_words(&self) -> u32 {
        self.memory_words
    }

    fn memory_base(&self) -> u32 {
        self.memory_base
    }

    fn lanes(&self) -> u32 {
        self.lanes
    }

    fn lane_may_run(&self, lane: u32) -> bool {
        self.lane_known(lane) != Some(false)
    }

    fn enter_block(&mut self) {
        // A block can be reached from more than one place, each with its own mask.
        self.known_exec = [None; 2];
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
                // A register holds thirty-two bits and an inline constant may be negative, so the
                // conversion goes through `i32` to get two's complement: -1 is 0xFFFF_FFFF, as in
                // `s_mov_b64 s[n:n+1], -1`.
                let value = i32::try_from(*value).map_err(|_| TranslateError::Unsupported {
                    offset: instruction.offset,
                    detail: "inline constant does not fit in a register",
                })? as u32;
                Ok(Self::constant(self, value))
            }
            // Uniform across the wavefront, so the same value for every lane.
            Operand::Scalar(register) => Ok(self.load_scalar(u32::from(*register))),
            Operand::Vector(register) => Ok(self.load_lane(u32::from(*register), lane)),
            // A lane mask read as an ordinary 32-bit source: its low half, as in `s_and_b32
            // exec_lo, exec_lo, sN` narrowing a 32-lane shader's mask.
            Operand::Named(named) if model::lane_mask_name(named).is_some() => {
                let mask = model::lane_mask_name(named).expect("checked immediately above");
                let (low, _) = self.read_lane_mask(mask)?;
                Ok(low)
            }
            // The m0 register read back as a source: whatever the shader last wrote.
            // Uniform across the wavefront, like every scalar.
            Operand::Named(name) if name == model::M0 => Ok(self.read_m0()),
            // An inline float, named by the operand table. Its bits go into the register, as the
            // hardware holds them.
            Operand::Named(name) => {
                let bits = name.parse::<f32>().map(f32::to_bits).map_err(|_| {
                    TranslateError::Unsupported {
                        offset: instruction.offset,
                        detail: "named operand is not an inline float",
                    }
                })?;
                Ok(Self::constant(self, bits))
            }
            // A literal: the thirty-two bits that follow the instruction, used verbatim. The
            // instruction decides whether it is a float or an integer.
            Operand::Literal(value) => Ok(Self::constant(self, *value)),
            _ => Err(TranslateError::Unsupported {
                offset: instruction.offset,
                detail: "source operand kind is not translated yet",
            }),
        }
    }

    /// Writes one lane of a vector register, if that lane is active.
    ///
    /// A select rather than a branch: the old value is kept where the mask says the lane is
    /// inactive.
    fn write_vector_lane(&mut self, register: u32, lane: u32, value: Id) {
        self.store_lane_masked(register, lane, value);
    }

    fn write_scalar(&mut self, register: u32, value: Id) {
        // A write into either descriptor group means the next sample does not read the texture the
        // last one did. Recorded here because this is the only place that sees writes (D690).
        for bound in &mut self.textures {
            let within = |first: u32, count: u32| register >= first && register < first + count;
            // The sampler's range exists only once something named a sampler.
            let sampler_disturbed = bound
                .sampler
                .is_some_and(|first| within(first, model::SAMPLER_DESCRIPTOR_REGISTERS));
            if within(bound.descriptor, model::IMAGE_DESCRIPTOR_REGISTERS) || sampler_disturbed {
                bound.disturbed = true;
            }
        }
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

    fn storage_image(&mut self, descriptor: u32) -> Result<model::Stored, &'static str> {
        // The fragment stage only: the harness binds its storage image with fragment stage flags,
        // and the `vec4` texel type comes from the colour output, which only a fragment module has.
        if self.stage != Stage::Fragment {
            return Err(concat!(
                "only a fragment module stores to an image here - a storage image is bound with ",
                "fragment stage flags, and a guest's pixel shader is what asked for one"
            ));
        }
        if let Some((stored, was)) = self.stored {
            if was != descriptor {
                return Err(concat!(
                    "this shader stores to more than one image and a pipeline binds one - ",
                    "resolving which is which needs the descriptors decoded, and writing ",
                    "whichever happened to be bound would corrupt a texture nobody asked to ",
                    "write (D690)"
                ));
            }
            return Ok(stored);
        }

        let image = self.builder.id();
        let pointer = self.builder.id();
        let variable = self.builder.id();
        let texel = self.builder.id();

        // The capability the format-less declaration needs. A device without the matching feature
        // refuses a module carrying it, so it is declared lazily (D692).
        self.builder.header(
            op::CAPABILITY,
            &[capability::STORAGE_IMAGE_WRITE_WITHOUT_FORMAT],
        );
        self.builder
            .annotate(op::DECORATE, &[variable.0, decoration::DESCRIPTOR_SET, 0]);
        self.builder.annotate(
            op::DECORATE,
            &[
                variable.0,
                decoration::BINDING,
                orbistoun_spirv::STORAGE_IMAGE_BINDING,
            ],
        );
        // Written and never read, as a guest's store is; the reference compiler marks a write-only
        // image the same way.
        self.builder
            .annotate(op::DECORATE, &[variable.0, decoration::NON_READABLE]);

        // The seven operands a sampled image takes, with `Sampled` at 2 (written through an image
        // instruction, not read through a sampler) and the format `Unknown`, which the capability
        // above permits.
        self.builder.declare(
            op::TYPE_IMAGE,
            &[image.0, self.f32_type.0, 1, 0, 0, 0, 2, 0],
        );
        self.builder
            .declare(op::TYPE_POINTER, &[pointer.0, UNIFORM_CONSTANT, image.0]);
        self.builder
            .declare(op::VARIABLE, &[pointer.0, variable.0, UNIFORM_CONSTANT]);
        self.builder
            .declare(op::TYPE_VECTOR, &[texel.0, self.u32_type.0, 2]);

        let stored = model::Stored {
            variable,
            image,
            texel,
            value: self.vec4,
        };
        self.stored = Some((stored, descriptor));
        Ok(stored)
    }

    fn sampled_image(
        &mut self,
        descriptor: u32,
        sampler: Option<u32>,
    ) -> Result<model::Texture, &'static str> {
        // The fragment stage only: the harness binds its sampled image with fragment stage flags,
        // and the `vec4` type a sample answers with is declared by the colour output, which only a
        // fragment module has.
        if self.stage != Stage::Fragment {
            return Err(concat!(
                "only a fragment module samples a texture here - a sampled image is bound with ",
                "fragment stage flags, and a guest's textured shading is what asked for one"
            ));
        }
        if let Some(bound) = self
            .textures
            .iter_mut()
            .find(|b| b.descriptor == descriptor)
        {
            if bound.disturbed {
                return Err(concat!(
                    "the registers holding this shader's image descriptor were written ",
                    "between one access and the next, so the two read different textures and ",
                    "only one is bound for them (D690)"
                ));
            }
            // A sampler only conflicts with a sampler; a fetch names none.
            if matches!((bound.sampler, sampler), (Some(was), Some(now)) if was != now) {
                return Err(concat!(
                    "this shader reads one image through two samplers, and a binding carries ",
                    "one (D690)"
                ));
            }
            // The first instruction to name a sampler records it, which may come after the first to
            // name the image.
            bound.sampler = bound.sampler.or(sampler);
            return Ok(bound.texture);
        }
        let source = self.new_texture_source(descriptor)?;
        let binding = if source.slot == 0 {
            orbistoun_spirv::TEXTURE_BINDING
        } else {
            orbistoun_spirv::SECOND_TEXTURE_BINDING
        };

        let image = self.builder.id();
        let combined = self.builder.id();
        let pointer = self.builder.id();
        let variable = self.builder.id();
        let coordinate = self.builder.id();

        self.builder
            .annotate(op::DECORATE, &[variable.0, decoration::DESCRIPTOR_SET, 0]);
        self.builder
            .annotate(op::DECORATE, &[variable.0, decoration::BINDING, binding]);

        // Element type, then: two-dimensional, not depth, not arrayed, single-sampled, used with a
        // sampler, and no declared format - the same seven values the hand-written oracle declares.
        self.builder.declare(
            op::TYPE_IMAGE,
            &[image.0, self.f32_type.0, 1, 0, 0, 0, 1, 0],
        );
        self.builder
            .declare(op::TYPE_SAMPLED_IMAGE, &[combined.0, image.0]);
        self.builder
            .declare(op::TYPE_POINTER, &[pointer.0, UNIFORM_CONSTANT, combined.0]);
        self.builder
            .declare(op::VARIABLE, &[pointer.0, variable.0, UNIFORM_CONSTANT]);
        self.builder
            .declare(op::TYPE_VECTOR, &[coordinate.0, self.f32_type.0, 2]);
        // The integer coordinate a fetch takes: a texel index rather than a position across the
        // image. Declared alongside the sample's type because the unused one costs one declaration.
        let texel = self.builder.id();
        self.builder
            .declare(op::TYPE_VECTOR, &[texel.0, self.u32_type.0, 2]);

        let texture = model::Texture {
            variable,
            sampled: combined,
            image,
            coordinate,
            texel,
            result: self.vec4,
        };
        self.textures.push(BoundTexture {
            texture,
            descriptor,
            sampler,
            source,
            disturbed: false,
        });
        Ok(texture)
    }

    /// Writes both halves of a lane mask, as an ordinary pair of register writes.
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

    /// Reads one word of the local data share.
    ///
    /// Unmasked: a read has no observable effect.
    fn read_local(&mut self, word_index: Id) -> Result<Id, TranslateError> {
        let (pointer_type, array, u32_type) = (self.local_ptr, self.local, self.u32_type);
        let b = &mut self.builder;
        let pointer = b.id();
        // One index: this is a bare array rather than a struct containing one, unlike the storage
        // buffers.
        b.function(
            op::ACCESS_CHAIN,
            &[pointer_type.0, pointer.0, array.0, word_index.0],
        );
        let value = b.id();
        b.function(op::LOAD, &[u32_type.0, value.0, pointer.0]);
        Ok(value)
    }

    /// Writes one word of the local data share, keeping the old value where the lane is inactive.
    ///
    /// Masked because another lane of the same wavefront may read this word.
    fn write_local(&mut self, word_index: Id, value: Id, lane: u32) -> Result<(), TranslateError> {
        let known = self.lane_known(lane);
        if known == Some(false) {
            return Ok(());
        }
        let (pointer_type, array, u32_type) = (self.local_ptr, self.local, self.u32_type);
        let pointer = self.builder.id();
        self.builder.function(
            op::ACCESS_CHAIN,
            &[pointer_type.0, pointer.0, array.0, word_index.0],
        );
        if known == Some(true) {
            self.builder.function(op::STORE, &[pointer.0, value.0]);
            return Ok(());
        }
        let active = self.lane_active(lane);

        let b = &mut self.builder;
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

    /// Masked by keeping the old value where the lane is inactive, because another lane may read
    /// that memory.
    fn write_memory(&mut self, word_index: Id, value: Id, lane: u32) {
        let known = self.lane_known(lane);
        if known == Some(false) {
            return;
        }
        let (element_ptr, buffer, u32_type) = (self.memory_element_ptr, self.memory, self.u32_type);
        let member = Self::constant(self, 0);

        let pointer = self.builder.id();
        self.builder.function(
            op::ACCESS_CHAIN,
            &[element_ptr.0, pointer.0, buffer.0, member.0, word_index.0],
        );
        if known == Some(true) {
            self.builder.function(op::STORE, &[pointer.0, value.0]);
            return;
        }
        let active = self.lane_active(lane);
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

/// Which texture a translated module samples at which binding, and where its descriptor comes from:
/// the byte offset in the pixel shader's descriptor table (addressed by its first two user-data
/// registers) that the image descriptor was loaded from. `None` when the module did not load it
/// from there; a pipeline then reads offset zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextureSource {
    /// 0 for the first texture the module samples, 1 for a second.
    pub slot: u32,
    /// The descriptor's byte offset in the table, when a load from the table put it there.
    pub table_offset: Option<u32>,
}

/// The descriptor-table offsets each eight-register group is loaded from: every `s_load_dwordx8
/// s[d:d+7], s[0:1], offset` in the program, as `d` to its offsets.
///
/// `s[0:1]` is where a pixel shader's first two user-data words land, and the open-toolchain GL
/// context puts its descriptor table's address there; its second texture unit loads the image
/// descriptor from `+0x40` and the first from `+0x00` (oops-sdk `tools/shader/tex-prolog2.s`). A
/// group loaded from more than one offset has no single source and is refused where it is sampled.
fn descriptor_table_loads(
    decode: &Decode,
    encodings: &EncodingTable,
) -> BTreeMap<u32, std::collections::BTreeSet<u32>> {
    let mut loads: BTreeMap<u32, std::collections::BTreeSet<u32>> = BTreeMap::new();
    for instruction in &decode.instructions {
        let Some(family) = instruction
            .encoding
            .and_then(|i| encodings.encodings().get(usize::from(i)))
            .map(|e| e.name.as_str())
        else {
            continue;
        };
        if encodings.mnemonic_for(family, instruction.opcode) != Some("s_load_dwordx8") {
            continue;
        }
        if let [
            Operand::Scalar(destination),
            Operand::Scalar(0),
            Operand::Immediate(offset),
        ] = instruction.operands.as_slice()
            && let Ok(offset) = u32::try_from(*offset)
        {
            loads
                .entry(u32::from(*destination))
                .or_default()
                .insert(offset);
        }
    }
    loads
}

/// Every attribute the shader interpolates, in order and without repeats, with how it is read.
///
/// A pass of its own because fragment inputs are global variables declared in the header, before
/// any instruction is translated, and the `Flat` decoration belongs to the variable. An attribute
/// read both ways cannot carry one decoration, and the caller refuses it rather than picking.
fn interpolated_attributes(
    decode: &Decode,
    encodings: &EncodingTable,
) -> Result<Vec<(u32, Interpolation)>, TranslateError> {
    let mut attributes: Vec<(u32, Interpolation)> = Vec::new();
    for instruction in &decode.instructions {
        let Some(family) = instruction
            .encoding
            .and_then(|i| encodings.encodings().get(usize::from(i)))
            .map(|e| e.name.as_str())
        else {
            continue;
        };
        let Some(name) = encodings.mnemonic_for(family, instruction.opcode) else {
            continue;
        };
        let how = match name {
            "v_interp_p1_f32_e32" | "v_interp_p2_f32_e32" => Interpolation::Smooth,
            "v_interp_mov_f32_e32" => Interpolation::Flat,
            _ => continue,
        };
        // The attribute is the third operand. A decode with fewer is refused by the translation, so
        // it contributes no input here.
        let Some(Operand::Immediate(attribute)) = instruction.operands.get(2) else {
            continue;
        };
        let Ok(attribute) = u32::try_from(*attribute) else {
            continue;
        };
        match attributes.iter().find(|(seen, _)| *seen == attribute) {
            Some((_, seen_as)) if *seen_as != how => {
                return Err(TranslateError::Unsupported {
                    offset: instruction.offset,
                    detail: concat!(
                        "this shader reads one attribute both interpolated and flat, and a host ",
                        "input is one or the other; refused rather than picking one"
                    ),
                });
            }
            Some(_) => {}
            None => attributes.push((attribute, how)),
        }
    }
    Ok(attributes)
}

/// Which parameter locations a shader exports, in the order a mesh module declares them.
///
/// The mesh counterpart of `interpolated_attributes`: output variables are declared in the header.
/// A guest's `exp param3` becomes location 3. Position, primitive indices and colour attachments
/// have their own variables and are skipped.
fn exported_parameters(decode: &Decode, encodings: &EncodingTable) -> Vec<u32> {
    let mut locations: Vec<u32> = Vec::new();
    for instruction in &decode.instructions {
        let named = instruction
            .encoding
            .and_then(|i| encodings.encodings().get(usize::from(i)))
            .and_then(|e| encodings.mnemonic_for(&e.name, instruction.opcode));
        if named != Some("exp") {
            continue;
        }
        let Some(Operand::Immediate(target)) = instruction.operands.first() else {
            continue;
        };
        if let Some(location) = model::export_parameter_location(*target)
            && !locations.contains(&location)
        {
            locations.push(location);
        }
    }
    locations
}

/// Translates a whole decoded shader at wavefront fidelity, for a named stage.
///
/// # Errors
///
/// Whatever the translation refuses: an unsupported instruction or an untrustworthy decode.
pub fn translate_for(
    decode: &Decode,
    encodings: &EncodingTable,
    width: Width,
    stage: Stage,
    window: Window,
) -> Result<(Vec<u32>, usize), TranslateError> {
    // A triangle. A caller that decoded the stream's topology uses [`translate_for_primitive`].
    translate_for_primitive(
        decode,
        encodings,
        width,
        stage,
        MeshPrimitive::default(),
        window,
    )
}

/// As [`translate_for`], for a caller that knows the mesh primitive the stream set.
pub fn translate_for_primitive(
    decode: &Decode,
    encodings: &EncodingTable,
    width: Width,
    stage: Stage,
    primitive: MeshPrimitive,
    window: Window,
) -> Result<(Vec<u32>, usize), TranslateError> {
    translate_with_user_data(
        decode,
        encodings,
        width,
        (stage, primitive),
        window,
        UserData::default(),
    )
    .map(|(words, translated, _)| (words, translated))
}

/// As [`translate_for_primitive`], for a module that reads its stage's user data at entry.
///
/// # Errors
///
/// As the translation refuses anything, and when the stage takes more user-data words than its
/// share of the push-constant block holds ([`USER_DATA_STAGE_WORDS`]): refused rather than
/// truncated, because a missing word would read zero and look plausible.
pub fn translate_with_user_data(
    decode: &Decode,
    encodings: &EncodingTable,
    width: Width,
    (stage, primitive): (Stage, MeshPrimitive),
    window: Window,
    user_data: UserData,
) -> Result<(Vec<u32>, usize, Vec<TextureSource>), TranslateError> {
    if user_data.count > USER_DATA_STAGE_WORDS
        || user_data.block_offset + user_data.count > USER_DATA_BLOCK_WORDS
    {
        return Err(TranslateError::Unsupported {
            offset: 0,
            detail: "the stage takes more user-data words than the push-constant block holds for it",
        });
    }
    // A mesh module's words come from the draw-data buffer, which holds the geometry stage's share
    // of the block, the first (D718).
    if stage == Stage::Mesh && user_data.count > 0 && user_data.block_offset != 0 {
        return Err(TranslateError::Unsupported {
            offset: 0,
            detail: "a mesh module's user data is the geometry stage's share of the block, at offset zero",
        });
    }
    let attributes = interpolated_attributes(decode, encodings)?;
    let parameters = exported_parameters(decode, encodings);
    let mut module = Wavefront::for_stage(
        encodings,
        width,
        stage,
        primitive,
        &attributes,
        &parameters,
        (window, user_data),
    );
    module.descriptor_loads = descriptor_table_loads(decode, encodings);
    crate::control::emit(&mut module, decode, encodings)?;
    let sources = module.texture_sources();
    let (words, translated) = module.finish()?;
    Ok((words, translated, sources))
}
