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

mod compute_inputs;
mod pixel_inputs;
pub use compute_inputs::{ComputeInputs, PartialGroups};
pub use pixel_inputs::{PixelInputs, Seeded, SystemValue};

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
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum MeshPrimitive {
    /// One vertex per primitive.
    Points,
    /// Two vertices per primitive.
    Lines,
    /// Three vertices per primitive. The default.
    #[default]
    Triangles,
    /// Three corners per primitive, of a rectangle whose fourth corner is `v1 + v2 - v0`: the
    /// `RECTLIST` radeonsi's blits draw (`si_pipe.h:2110`), whose three vertices are `(x1, y1)`,
    /// `(x1, y2)` and `(x2, y1)` (`si_nir_lower_vs_inputs.c:97-122`). Emitted as two triangles over
    /// a fourth vertex whose position and parameters are carried the same way, so each parameter
    /// stays the one plane the three corners define.
    Rectangles,
}

impl MeshPrimitive {
    /// How many of the packed `exp prim` indices this primitive uses, which is also the width of
    /// its index built-in (a `uint`, `uvec2` or `uvec3`).
    #[must_use]
    pub const fn indices(self) -> u32 {
        match self {
            Self::Points => 1,
            Self::Lines => 2,
            Self::Triangles | Self::Rectangles => 3,
        }
    }

    /// How many vertices and primitives the module declares room for: one of each per lane, and
    /// for a rectangle a fourth vertex and a second triangle per primitive as well.
    const fn slots(self) -> u32 {
        match self {
            Self::Rectangles => 2 * MESH_SLOTS,
            _ => MESH_SLOTS,
        }
    }

    /// The `OutputPoints`/`OutputLinesEXT`/`OutputTrianglesEXT` execution mode declaring the shape.
    const fn output_mode(self) -> u32 {
        match self {
            Self::Points => mode::OUTPUT_POINTS,
            Self::Lines => mode::OUTPUT_LINES_EXT,
            Self::Triangles | Self::Rectangles => mode::OUTPUT_TRIANGLES_EXT,
        }
    }

    /// The `PrimitivePointIndicesEXT`/`Line`/`Triangle` built-in decorating the index array.
    const fn indices_built_in(self) -> u32 {
        match self {
            Self::Points => built_in::PRIMITIVE_POINT_INDICES_EXT,
            Self::Lines => built_in::PRIMITIVE_LINE_INDICES_EXT,
            Self::Triangles | Self::Rectangles => built_in::PRIMITIVE_TRIANGLE_INDICES_EXT,
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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
    /// A pixel shader's input registers, which place its system values after the interpolants.
    /// `None` when the stream set none, and then no vector register is seeded.
    #[serde(default)]
    pub pixel_inputs: Option<PixelInputs>,
    /// A guest compute dispatch's entry state. With it a compute module seeds the workgroup and
    /// thread ids and keeps its memory exact, refusing by flag what it cannot place; without it a
    /// compute module is the register-observing harness it always was.
    #[serde(default)]
    pub compute: Option<ComputeInputs>,
    /// A primitive shader's geometry-engine inputs for the draw it runs (D730). Without them a
    /// mesh module that reads those inputs is refused.
    #[serde(default)]
    pub geometry: Option<GeometryInputs>,
    /// Whether a primitive shader's position export is in window space (D731): `PA_CL_VTE_CNTL` in
    /// the form radeonsi gives a window-space shader, x, y and z already divided and the fourth
    /// component `1/W`, with no viewport scale or offset (`si_state_shaders.cpp:1321-1322`). The
    /// module then writes the clip-space position that, under the viewport [`WINDOW_SPACE_SCALE`]
    /// names, lands on the same pixel.
    #[serde(default)]
    pub window_space: bool,
    /// Whether a draw binds the buffers this stage's shader reads through (D733). With it every
    /// access [`crate::draw_buffers::trace`] traces reads the buffer bound at its slot, and one
    /// that stores through such a buffer is refused; without it every access reaches the window.
    #[serde(default)]
    pub draw_buffers: bool,
    /// The fourth word of each draw buffer's descriptor at the draw the module is translated for
    /// (D738), by slot: the `FORMAT` and selects a format load converts by. `None` when the module
    /// is translated for no particular draw, and then a format load is refused.
    #[serde(default)]
    pub buffer_formats: Option<BufferFormats>,
    /// The second, flat location of each attribute a draw's pixel shader reads both interpolated
    /// and flat (D742): the pixel shader's flat reads take it, and the primitive shader exports the
    /// parameter there too. `None` when the module is translated for no particular draw, and then
    /// a pixel shader that reads an attribute both ways is refused.
    #[serde(default)]
    pub flat_twins: Option<FlatTwins>,
}

/// Each attribute a pixel shader reads both interpolated and flat, with the flat location the draw
/// gives it (D742), as `(attribute, location)`; the unused entries `None`. Bytes, since both are
/// below the thirty-two parameters and their twins.
#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    serde::Serialize,
    serde::Deserialize,
)]
pub struct FlatTwins(pub [Option<(u8, u8)>; FLAT_TWINS]);

/// The most attributes a draw gives a flat twin; more is refused.
pub const FLAT_TWINS: usize = 8;

impl FlatTwins {
    /// The twins for `mixed`, in order, from location `first` up; `None` for more than
    /// [`FLAT_TWINS`], or a location past a byte.
    #[must_use]
    pub fn from_first(mixed: &[u32], first: u32) -> Option<Self> {
        if mixed.len() > FLAT_TWINS {
            return None;
        }
        let mut twins = [None; FLAT_TWINS];
        for ((slot, attribute), location) in twins.iter_mut().zip(mixed).zip(first..) {
            *slot = Some((u8::try_from(*attribute).ok()?, u8::try_from(location).ok()?));
        }
        Some(Self(twins))
    }

    /// The flat location of `attribute`, if it has one.
    #[must_use]
    pub fn location_of(&self, attribute: u32) -> Option<u32> {
        self.0
            .iter()
            .flatten()
            .find(|(seen, _)| u32::from(*seen) == attribute)
            .map(|(_, location)| u32::from(*location))
    }
}

/// Each draw buffer's descriptor's fourth word at one draw, by slot (D738). `None` for a slot whose
/// buffer is not a descriptor the draw resolved.
#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    serde::Serialize,
    serde::Deserialize,
)]
pub struct BufferFormats(pub [Option<u32>; orbistoun_spirv::DRAW_BUFFERS_PER_STAGE as usize]);

/// Whether a program converts a buffer load by its descriptor's format (D738), and so is translated
/// per draw with [`UserData::buffer_formats`].
#[must_use]
pub fn reads_buffer_formats(decode: &Decode, encodings: &EncodingTable) -> bool {
    decode.instructions.iter().any(|instruction| {
        instruction
            .encoding
            .and_then(|i| encodings.encodings().get(usize::from(i)))
            .and_then(|e| encodings.mnemonic_for(&e.name, instruction.opcode))
            .is_some_and(|name| name.starts_with("buffer_load_format_"))
    })
}

/// The viewport scale a window-space draw runs under (D731), in pixels, on both axes, with no
/// offset: a window-space position `(x, y, z, 1/W)` is written `(x W / S, y W / S, z W, W)`, which
/// Vulkan's divide and this viewport take back to `(x, y, z)`. A power of two, so the scaling is
/// exact; and large enough that every position on a target of up to `S` pixels lies inside the
/// clip volume.
pub const WINDOW_SPACE_SCALE: f32 = 8192.0;

/// What the geometry engine hands a primitive shader for a draw one subgroup holds whole (D730):
/// the draw's vertices, one per vertex thread from `first_vertex`, and its primitives of three
/// vertices each, primitive `p` over vertices `3p`, `3p + 1` and `3p + 2` - a triangle list or a
/// rectangle list, not indexed.
///
/// Seeded where GFX10's non-passthrough primitive shader receives them (`si_shader_args.c:304-371`):
/// `s2` gs_tg_info, input vertices at bit 12 and primitives at bit 22
/// (`ac_nir_lower_intrinsics_to_args.c:273-276`); `s3` merged_wave_info, this wave's vertex and
/// primitive threads at bits 0 and 8 and the one wave of the group counted at bit 28
/// (`:36`, `:99`, `:127`, `:418`); `v0` the first two vertex indices, sixteen bits each, and `v1`
/// the third (`ac_nir_lower_ngg.c:130-131`); `v2` the primitive id; and `v5` the vertex id
/// (`si_shader_args.c:95`). The invocation id, the fifth vertex register, the user VGPRs and the
/// instance id read zero: one invocation, no adjacency, one instance.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct GeometryInputs {
    /// The vertex id of the first vertex thread.
    pub first_vertex: u32,
    /// Vertex threads: the draw's vertex count.
    pub vertices: u32,
    /// Primitive threads.
    pub primitives: u32,
    /// How the primitives take their vertices.
    #[serde(default)]
    pub assembly: Assembly,
    /// For an indexed draw, how wide its indices are: each vertex thread's id is then the index at
    /// its position in the draw's index buffer, which the draw binds after the traced buffers
    /// (D740). `None` for a draw whose vertex ids count up from [`Self::first_vertex`].
    #[serde(default)]
    pub indices: Option<IndexWidth>,
}

/// How wide an indexed draw's indices are: `VGT_INDEX_TYPE`'s `VGT_INDEX_16` and `VGT_INDEX_32`
/// (`gfx103.json`).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum IndexWidth {
    /// Sixteen bits, two to a word.
    Bits16,
    /// Thirty-two bits.
    Bits32,
}

impl IndexWidth {
    /// Bytes one index takes.
    #[must_use]
    pub const fn bytes(self) -> u32 {
        match self {
            Self::Bits16 => 2,
            Self::Bits32 => 4,
        }
    }
}

/// The input register a primitive shader's vertex thread finds its vertex id in: `v5`, after the
/// vertex indices, primitive id and invocation id (GFX10's non-passthrough primitive shader).
const VERTEX_ID_REGISTER: u32 = 5;

/// How a draw's primitives take their three vertices each.
#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    serde::Serialize,
    serde::Deserialize,
)]
pub enum Assembly {
    /// A list: primitive `i` is vertices `3i`, `3i + 1`, `3i + 2`.
    #[default]
    List,
    /// A strip: primitive `i` is vertices `i`, `i + 1`, `i + 2`, an odd one reversed to keep the
    /// strip's winding, in the rotation that puts its provoking vertex where the draw's convention
    /// looks: `i + 2` last under the last-vertex convention, `i` first under the first-vertex one
    /// (the GL triangle-strip rules radeonsi's draws keep; `PROVOKING_VTX_LAST`, bit 19 of
    /// `PA_SU_SC_MODE_CNTL`, which radeonsi sets to `!flatshade_first`).
    Strip {
        /// Whether the last vertex is the provoking one.
        provoking_last: bool,
    },
    /// A list of lines: primitive `i` is vertices `2i`, `2i + 1`. A line's two indices are packed
    /// in the first input register as a triangle's first two are; the third is not read
    /// (`ac_nir_lower_ngg.c:130`, two vertices per primitive).
    LineList,
    /// A strip of lines: primitive `i` is vertices `i`, `i + 1`.
    LineStrip,
}

impl Assembly {
    /// The three vertices primitive `primitive` takes, in order.
    #[must_use]
    pub const fn vertices_of(self, primitive: u32) -> [u32; 3] {
        let i = primitive;
        match self {
            Self::List => [3 * i, 3 * i + 1, 3 * i + 2],
            Self::Strip { .. } if i % 2 == 0 => [i, i + 1, i + 2],
            Self::Strip {
                provoking_last: true,
            } => [i + 1, i, i + 2],
            Self::Strip {
                provoking_last: false,
            } => [i, i + 2, i + 1],
            // A line has two vertices; the third index is not read.
            Self::LineList => [2 * i, 2 * i + 1, 0],
            Self::LineStrip => [i, i + 1, 0],
        }
    }

    /// How many vertices `primitives` primitives take.
    #[must_use]
    pub const fn vertices_for(self, primitives: u32) -> u32 {
        match self {
            Self::List => primitives * 3,
            Self::Strip { .. } | Self::LineStrip if primitives == 0 => 0,
            Self::Strip { .. } => primitives + 2,
            Self::LineList => primitives * 2,
            Self::LineStrip => primitives + 1,
        }
    }
}

impl GeometryInputs {
    /// Why a subgroup of `lanes` threads cannot hold these whole, if it cannot.
    #[must_use]
    pub const fn refusal(self, lanes: u32) -> Option<&'static str> {
        if self.vertices > lanes || self.primitives > lanes {
            Some("the draw has more vertices or primitives than one wave's threads")
        } else if self.assembly.vertices_for(self.primitives) > self.vertices {
            Some("the draw's primitives name vertices it does not have")
        } else {
            None
        }
    }
}
/// Words in the push-constant block: thirty-two per stage, two stages - 256 bytes, which a device
/// is refused for not taking (the backend checks `maxPushConstantsSize`).
pub const USER_DATA_BLOCK_WORDS: u32 = 64;

/// The most user-data words one stage may take within the block: the hardware's thirty-two user
/// registers (`USER_SGPR` with its `USER_SGPR_MSB`, `gfx103.json`). radeonsi puts a vertex
/// shader's buffer descriptors among them.
pub const USER_DATA_STAGE_WORDS: u32 = 32;

/// How a fragment input is read.
///
/// The guest decides per instruction: `v_interp_p1_f32` and its pair interpolate, and
/// `v_interp_mov_f32` reads a parameter without interpolating. On the host it is a decoration on
/// the input variable, fixed at declaration, so it is found in the same pass that finds the
/// attributes, and an attribute read both ways takes a second, flat input (D742).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Interpolation {
    /// Interpolated across the primitive, which is the default and needs no decoration.
    Smooth,
    /// The provoking vertex's value, everywhere. `Flat`.
    Flat,
}

/// Declares that the module's 32-bit arithmetic keeps signed zeros, infinities and NaNs, as the
/// guest's does: its IEEE results propagate them (AMD's published RDNA ISA), and without the
/// declaration a driver may fold them away. A device that cannot honour it gets the module with
/// the declaration removed (`orbistoun_spirv::without_inf_nan_preserve`).
pub(crate) fn declare_inf_nan_preserve(b: &mut Builder, main: Id) {
    b.header(op::CAPABILITY, &[capability::SIGNED_ZERO_INF_NAN_PRESERVE]);
    b.header(
        op::EXTENSION,
        &Builder::literal_string(orbistoun_spirv::FLOAT_CONTROLS),
    );
    b.header(
        op::EXECUTION_MODE,
        &[main.0, mode::SIGNED_ZERO_INF_NAN_PRESERVE, 32],
    );
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

    declare_inf_nan_preserve(b, main);
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
                &[main.0, mode::OUTPUT_VERTICES, primitive.slots()],
            );
            b.header(
                op::EXECUTION_MODE,
                &[main.0, mode::OUTPUT_PRIMITIVES_EXT, primitive.slots()],
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
/// [`None`] for a compute module, which still declares the `vec4` an image access's texel needs.
fn declare_colour_output(
    b: &mut Builder,
    (f32_type, vec4): (Id, Id),
    output_ptr: Id,
    (output, stage): (Option<Id>, Stage),
) -> Option<(Id, Id)> {
    let Some(colour) = output else {
        if stage == Stage::Compute {
            b.declare(op::TYPE_VECTOR, &[vec4.0, f32_type.0, 4]);
        }
        return None;
    };
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
    b.declare(op::CONSTANT, &[u32_type.0, slots.0, primitive.slots()]);
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
    /// A rectangle list's copies of what it emits, declared once the mesh outputs are.
    rectangles: Option<RectangleCopies>,
}

impl MeshReserved {
    /// Reserves ids for a mesh module's outputs, or nothing at another stage.
    fn new(b: &mut Builder, stage: Stage, primitive: MeshPrimitive, locations: &[u32]) -> Self {
        if stage != Stage::Mesh {
            return Self {
                vertices: Id(0),
                indices: Id(0),
                parameters: BTreeMap::new(),
                rectangles: None,
            };
        }
        Self {
            vertices: b.id(),
            indices: b.id(),
            parameters: locations
                .iter()
                .map(|location| (*location, b.id()))
                .collect(),
            rectangles: RectangleCopies::reserve(b, stage, primitive, locations),
        }
    }

    /// Every variable the entry point must list.
    fn interface(&self, stage: Stage) -> Vec<u32> {
        if stage != Stage::Mesh {
            return Vec::new();
        }
        let mut ids = vec![self.vertices.0, self.indices.0];
        ids.extend(self.parameters.values().map(|id| id.0));
        ids.extend(self.rectangles.iter().flat_map(RectangleCopies::interface));
        ids
    }

    /// Declares a rectangle list's copies, once the mesh outputs they mirror are declared.
    fn declare_rectangles(
        &self,
        b: &mut Builder,
        mesh: Option<&MeshOutputs>,
        vec4: Id,
        ids: &Reserved,
    ) -> Option<RectangleCopies> {
        let (copies, outputs) = self.rectangles.clone().zip(mesh)?;
        Some(copies.declare(b, (ids.u32_type, vec4, outputs.index_type), ids.counter_ptr))
    }
}

/// The SPIR-V version a module for `stage` declares: 1.4 for a mesh module, which its extension
/// requires, and 1.3 for everything else, where an entry point lists only its inputs and outputs.
const fn module_version(stage: Stage) -> u32 {
    if matches!(stage, Stage::Mesh) {
        orbistoun_spirv::VERSION_1_4
    } else {
        orbistoun_spirv::VERSION_1_3
    }
}

/// Declares the types every module has: `void` and the entry point's function type, the 32-bit
/// integer and float, and the boolean.
fn declare_base_types(b: &mut Builder, ids: &Reserved) {
    b.declare(op::TYPE_VOID, &[ids.void.0]);
    b.declare(op::TYPE_FUNCTION, &[ids.fn_type.0, ids.void.0]);
    b.declare(op::TYPE_INT, &[ids.u32_type.0, 32, 0]);
    b.declare(op::TYPE_FLOAT, &[ids.f32_type.0, 32]);
    b.declare(op::TYPE_BOOL, &[ids.bool_type.0]);
}

/// A rectangle-list mesh module's own copy of what it emitted, which its outputs cannot be read
/// back for: each lane's position, parameters and primitive indices, and the declared counts. The
/// fourth corner of each rectangle is formed from them once the program has run.
#[derive(Debug, Clone)]
struct RectangleCopies {
    /// `vec4` per lane: the positions.
    positions: Id,
    /// `vec4` per lane, per exported parameter location.
    parameters: BTreeMap<u32, Id>,
    /// `uvec3` per lane: the primitive's three corners.
    indices: Id,
    /// The vertex count the allocation request declared.
    vertices: Id,
    /// The primitive count it declared.
    primitives: Id,
    /// Pointer to one `vec4` of a copy.
    vec4_ptr: Id,
    /// Pointer to one `uvec3` of the index copy.
    index_ptr: Id,
    /// Pointer to one private word.
    word_ptr: Id,
}

impl RectangleCopies {
    /// Reserves the copies' variables, or nothing for any other module: the entry point lists them.
    fn reserve(
        b: &mut Builder,
        stage: Stage,
        primitive: MeshPrimitive,
        locations: &[u32],
    ) -> Option<Self> {
        (stage == Stage::Mesh && primitive == MeshPrimitive::Rectangles).then(|| Self {
            positions: b.id(),
            parameters: locations
                .iter()
                .map(|location| (*location, b.id()))
                .collect(),
            indices: b.id(),
            vertices: b.id(),
            primitives: b.id(),
            vec4_ptr: Id(0),
            index_ptr: Id(0),
            word_ptr: Id(0),
        })
    }

    /// Every variable the entry point must list.
    fn interface(&self) -> Vec<u32> {
        let mut ids = vec![
            self.positions.0,
            self.indices.0,
            self.vertices.0,
            self.primitives.0,
        ];
        ids.extend(self.parameters.values().map(|id| id.0));
        ids
    }

    /// Declares the reserved variables as private storage, one element per lane.
    fn declare(
        mut self,
        b: &mut Builder,
        (u32_type, vec4, uvec3): (Id, Id, Id),
        word_ptr: Id,
    ) -> Self {
        let lanes = b.id();
        b.declare(op::CONSTANT, &[u32_type.0, lanes.0, MESH_SLOTS]);
        let vec4_array = b.id();
        let vec4_array_ptr = b.id();
        let index_array = b.id();
        let index_array_ptr = b.id();
        self.vec4_ptr = b.id();
        self.index_ptr = b.id();
        b.declare(op::TYPE_ARRAY, &[vec4_array.0, vec4.0, lanes.0]);
        b.declare(op::TYPE_POINTER, &[vec4_array_ptr.0, PRIVATE, vec4_array.0]);
        b.declare(op::TYPE_ARRAY, &[index_array.0, uvec3.0, lanes.0]);
        b.declare(
            op::TYPE_POINTER,
            &[index_array_ptr.0, PRIVATE, index_array.0],
        );
        b.declare(op::TYPE_POINTER, &[self.vec4_ptr.0, PRIVATE, vec4.0]);
        b.declare(op::TYPE_POINTER, &[self.index_ptr.0, PRIVATE, uvec3.0]);
        b.declare(op::VARIABLE, &[vec4_array_ptr.0, self.positions.0, PRIVATE]);
        for variable in self.parameters.values() {
            b.declare(op::VARIABLE, &[vec4_array_ptr.0, variable.0, PRIVATE]);
        }
        b.declare(op::VARIABLE, &[index_array_ptr.0, self.indices.0, PRIVATE]);
        for word in [self.vertices, self.primitives] {
            b.declare(op::VARIABLE, &[word_ptr.0, word.0, PRIVATE]);
        }
        self.word_ptr = word_ptr;
        self
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
    /// Each attribute's second, flat location (D742): where a pixel shader's flat reads of an
    /// attribute it also interpolates go, and where a primitive shader exports the parameter again.
    flat_twins: Option<FlatTwins>,
    /// What a mesh module writes, or [`None`] at any other stage.
    mesh: Option<MeshOutputs>,
    /// A rectangle-list mesh module's copies of what it emitted, or [`None`] for any other.
    rectangles: Option<RectangleCopies>,
    /// Whether a mesh module's position export is in window space (D731); see
    /// [`UserData::window_space`].
    window_space: bool,
    /// Each draw buffer's descriptor's fourth word at the draw, by slot (D738).
    buffer_formats: Option<BufferFormats>,
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
    /// The 32-bit float constants declared so far, by bit pattern.
    float_constants: BTreeMap<u32, Id>,
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
    /// Each descriptor register group's descriptor-table offsets, from [`descriptor_table_loads`]:
    /// the image descriptors', the sampler descriptors', and the image load reaching each image
    /// instruction.
    descriptor_loads: (DescriptorLoads, DescriptorLoads, ReachingLoads),
    /// Where a compute module's image descriptors come from, and what it did to their registers.
    images: ImageTrace,
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
    /// A guest dispatch's workgroup id and escape flag, or `None` for any other module.
    dispatch: Option<compute_inputs::DispatchState>,
    /// The high thirty-two bits of the window's guest address.
    memory_high: u32,
    /// A guest dispatch's user-data words the stream never wrote, one bit each.
    unwritten_user_data: u32,
    /// Whether the program reads one of them, whose value nothing here knows.
    reads_unwritten: bool,
    /// Scalar and vector registers written so far, in program order: seeded at entry, or by an
    /// instruction before the one now translated.
    written: (u128, [u64; 4]),
    /// Whether a mesh module read a geometry-engine input it was not given.
    reads_geometry_input: bool,
    /// The draw's bound buffers, and the slot each traced access reads, by the access's offset
    /// (D733). `None` for a module that reads through none.
    draw_buffers: Option<(buffer::DrawBufferArray, BTreeMap<u32, u32>)>,
}

/// A primitive shader's system SGPRs, `s0`-`s7`, ahead of its user SGPRs: gs_tg_info in `s2` and
/// merged_wave_info in `s3` among them (`si_shader_args.c:304-322`).
const GEOMETRY_SYSTEM_SGPRS: u32 = 8;
/// Its input VGPRs, `v0`-`v8`: the vertex offsets, primitive and invocation ids, then the vertex
/// shader's vertex id and instance id (`si_shader_args.c:363-371`, `:93-118`).
const GEOMETRY_INPUT_VGPRS: u32 = 9;

impl Wavefront<'_> {
    /// The texture sources this module samples, in slot order.
    #[must_use]
    pub fn image_sources(&self) -> (Vec<TextureSource>, Option<TextureSource>) {
        (self.texture_sources(), self.images.stored_source)
    }

    /// The texture sources this module samples, in slot order.
    pub fn texture_sources(&self) -> Vec<TextureSource> {
        self.textures
            .iter()
            .map(|bound| TextureSource {
                sampler_offset: bound
                    .sampler
                    .and_then(|sampler| self.sampler_offset(sampler, bound.source.table)),
                ..bound.source
            })
            .collect()
    }

    /// Where a sampler descriptor starting at scalar register `sampler` was loaded from in `table`:
    /// one load, from that same table, or nothing.
    fn sampler_offset(&self, sampler: u32, table: TableBase) -> Option<u32> {
        let loads = self.descriptor_loads.1.get(&sampler)?;
        match loads.iter().collect::<Vec<_>>().as_slice() {
            [(Some(from), offset)] if *from == table => Some(*offset),
            _ => None,
        }
    }

    /// The source of a texture first sampled through `descriptor`: the next slot, and the table
    /// offset the descriptor was loaded from.
    ///
    /// Refused where the texture cannot be identified: a third texture (two bindings exist), a
    /// descriptor loaded from more than one offset, or a second texture when either did not come
    /// from the table. A pipeline finds a texture by its offset, so binding offset zero to both
    /// would draw the wrong image.
    fn new_texture_source(
        &self,
        descriptor: u32,
        reaching: Option<(Option<TableBase>, u32)>,
    ) -> Result<TextureSource, &'static str> {
        let slot = u32::try_from(self.textures.len()).unwrap_or(u32::MAX);
        if slot > 1 {
            return Err("this shader reads more than two textures, and two bindings exist (D690)");
        }
        let user_data = self.resident_descriptor(descriptor)?;
        let (table, table_offset) = match self.descriptor_loads.0.get(&descriptor) {
            // The load that reached this sample, where the registers took more than one.
            _ if reaching.is_some_and(|(table, _)| table.is_some()) => {
                let (table, offset) = reaching.unwrap_or_default();
                (table.unwrap_or_default(), Some(offset))
            }
            None => (TableBase::default(), None),
            Some(loads) if loads.len() == 1 => match loads.first().copied() {
                Some((Some(table), offset)) => (table, Some(offset)),
                _ => {
                    return Err(concat!(
                        "this shader loads an image descriptor through an address it formed from ",
                        "something other than its user data and constants, which is not traced"
                    ));
                }
            },
            Some(_) => {
                return Err(concat!(
                    "this shader loads one image descriptor from more than one place in its ",
                    "descriptor table, so which texture a sample reads is not fixed (D690)"
                ));
            }
        };
        let first_resolved = self.textures.first().is_none_or(|first| {
            first.source.table_offset.is_some() || first.source.user_data.is_some()
        });
        if slot == 1 && ((table_offset.is_none() && user_data.is_none()) || !first_resolved) {
            return Err(concat!(
                "this shader reads two textures and at least one descriptor did not come from ",
                "its descriptor table, so which is which cannot be told (D690)"
            ));
        }
        Ok(TextureSource {
            slot,
            table_offset,
            table,
            sampler_offset: None,
            user_data,
        })
    }

    /// For a compute module, the user-data word a descriptor not loaded from a table starts at:
    /// its eight registers must lie in the seeded user data, unwritten by the program. A compute
    /// dispatch has no table at fixed user-data words to fall back on, so anything else is
    /// refused. `None` at any other stage, and for a descriptor a table load put there.
    fn resident_descriptor(&self, descriptor: u32) -> Result<Option<u32>, &'static str> {
        if self.stage != Stage::Compute || self.descriptor_loads.0.contains_key(&descriptor) {
            return Ok(None);
        }
        let (first, count) = self.images.user_data_registers;
        let span = model::IMAGE_DESCRIPTOR_REGISTERS;
        let seeded = descriptor >= first && descriptor + span <= first + count;
        let untouched = (descriptor..descriptor + span).all(|register| {
            register >= u128::BITS || self.images.written_scalars >> register & 1 == 0
        });
        if seeded && untouched {
            Ok(Some(descriptor - first))
        } else {
            Err(concat!(
                "a compute program reads an image descriptor that is neither loaded from a table ",
                "nor still in the user data it was seeded with, so which image it names is not traced"
            ))
        }
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

/// Declares a draw's buffers at the stage's binding, only where a traced access reads one (D733),
/// with the slot each access reads.
fn declare_draw_buffers(
    b: &mut Builder,
    u32_type: Id,
    (stage, indexed): (Stage, bool),
    buffers: &crate::draw_buffers::DrawBuffers,
) -> Option<(buffer::DrawBufferArray, BTreeMap<u32, u32>)> {
    let binding = match stage {
        // An indexed primitive shader reads its index buffer through them (D740).
        _ if buffers.served.is_empty() && !indexed => return None,
        Stage::Mesh => orbistoun_spirv::GEOMETRY_BUFFERS_BINDING,
        Stage::Fragment => orbistoun_spirv::PIXEL_BUFFERS_BINDING,
        Stage::Compute => return None,
    };
    Some((
        buffer::declare_draw_buffers(b, u32_type, binding),
        buffers.served.clone(),
    ))
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
            (
                Window::default(),
                UserData::default(),
                &crate::draw_buffers::DrawBuffers::default(),
            ),
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
        (window, user_data, buffers): (Window, UserData, &crate::draw_buffers::DrawBuffers),
    ) -> Self {
        let mut b = Builder::new().with_version(module_version(stage));

        let ids = Reserved::new(&mut b);
        let (void, fn_type, main) = (ids.void, ids.fn_type, ids.main);
        let (u32_type, f32_type, bool_type) = (ids.u32_type, ids.f32_type, ids.bool_type);

        // The colour output of a fragment module, reserved before the entry point because its
        // interface must name it.
        let output = (stage == Stage::Fragment).then(|| b.id());
        let vec4 = b.id();
        let output_ptr = b.id();
        // Reserved before the header for the same reason: every input the entry point touches must
        // be named in its interface, and a driver does not reliably reject a module that omits one.
        let input_ids = reserve_attribute_inputs(&mut b, stage, attributes);
        let mut system = pixel_inputs::SystemInputs::reserve(&mut b, stage, user_data.pixel_inputs);
        let mesh_reserved = MeshReserved::new(&mut b, stage, primitive, parameters);
        emit_header(&mut b, stage, primitive, main, output);
        declare_base_types(&mut b, &ids);
        let output = declare_colour_output(&mut b, (f32_type, vec4), output_ptr, (output, stage));
        let inputs = declare_attribute_inputs(&mut b, vec4, &input_ids);
        system.declare(&mut b, vec4, bool_type);
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
        let rectangles = mesh_reserved.declare_rectangles(&mut b, mesh.as_ref(), vec4, &ids);
        let user_data_source = declare_user_data_source(&mut b, stage, u32_type, user_data);
        let dispatch = compute_inputs::DispatchState::for_stage(&mut b, &ids, stage, user_data);
        let indexed = user_data.geometry.is_some_and(|g| g.indices.is_some());
        let draw_buffers = declare_draw_buffers(&mut b, u32_type, (stage, indexed), buffers);

        // Every variable this module has. From 1.4 the entry point names all of them; below that
        // only the inputs and outputs.
        let mut interface: Vec<u32> = input_ids.iter().map(|(_, _, id)| id.0).collect();
        interface.extend(system.interface());
        interface.extend(output.map(|(_, colour)| colour.0));
        interface.extend(mesh_reserved.interface(stage));
        interface.extend(dispatch.map(compute_inputs::DispatchState::interface));
        if stage == Stage::Mesh {
            interface.extend(mesh_interface(
                &ids,
                &files,
                (&observation, &guest_memory),
                user_data_source,
            ));
            interface.extend(draw_buffers.as_ref().map(|(array, _)| array.variable.0));
        }
        emit_entry_point(&mut b, stage, main, &interface);

        b.function(op::FUNCTION, &[void.0, main.0, 0, fn_type.0]);
        b.function(op::LABEL, &[ids.entry_block.0]);

        let mut this = Self {
            stage,
            output,
            inputs,
            flat_twins: user_data.flat_twins,
            mesh,
            rectangles,
            window_space: user_data.window_space,
            buffer_formats: user_data.buffer_formats,
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
            float_constants: BTreeMap::new(),
            glsl_set: None,
            u32_type,
            f32_type,
            f16_type: None,
            u16_type: None,
            textures: Vec::new(),
            descriptor_loads: Default::default(),
            images: ImageTrace::seeded(user_data.first_register, user_data.count),
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
            dispatch,
            memory_high: u32::try_from(window.address() >> 32).unwrap_or(u32::MAX),
            unwritten_user_data: user_data.compute.map_or(0, |c| c.unwritten_user_data),
            reads_unwritten: false,
            written: (0, [0; 4]),
            reads_geometry_input: false,
            draw_buffers,
        };

        this.seed_entry(user_data_source, user_data, &system);
        this
    }

    /// Places what the hardware loads before a wave's first instruction.
    fn seed_entry(
        &mut self,
        user_data_source: Option<UserDataSource>,
        user_data: UserData,
        system: &pixel_inputs::SystemInputs,
    ) {
        // Every lane runs at entry; a zero mask would discard every write and produce a buffer of
        // zeros.
        let all = self.constant(u32::MAX);
        self.store_scalar(EXEC_LO, all);
        self.store_scalar(EXEC_HI, all);

        // The user data where the hardware puts it: word `i` of this stage's range in the block
        // into `s[first + i]`, before the first instruction runs.
        if let Some(source) = user_data_source {
            self.load_user_data(source, user_data);
        }
        // The system values where the hardware puts them: after the interpolants, in field order.
        system.seed(self);
        // A dispatch's ids after the user data and in the first vector registers.
        if let (Some(state), Some(inputs)) = (self.dispatch, user_data.compute) {
            state.seed(self, inputs, user_data.count);
        }
        // A primitive shader's geometry-engine inputs (D730).
        if let (Stage::Mesh, Some(geometry)) = (self.stage, user_data.geometry) {
            self.seed_geometry(geometry);
        }
    }

    /// Seeds the geometry-engine inputs [`GeometryInputs`] describes, every input register in
    /// every lane: a lane that is no vertex or primitive thread reads zero.
    fn seed_geometry(&mut self, geometry: GeometryInputs) {
        let GeometryInputs {
            first_vertex,
            vertices,
            primitives,
            assembly,
            indices,
        } = geometry;
        // An indexed draw's index buffer is bound after the buffers the program reads through.
        let index_slot = self.draw_buffers.as_ref().map_or(0, |(_, served)| {
            served.values().max().map_or(0, |&slot| slot + 1)
        });
        let tg_info = self.constant((vertices << 12) | (primitives << 22));
        self.store_scalar(2, tg_info);
        let wave_info = self.constant(vertices | (primitives << 8) | (1 << 28));
        self.store_scalar(3, wave_info);
        for lane in 0..self.lanes {
            let is_primitive = lane < primitives;
            let [first, second, third] = assembly.vertices_of(lane);
            let words = [
                // v0: the first two vertex indices; v1: the third.
                if is_primitive {
                    first | (second << 16)
                } else {
                    0
                },
                if is_primitive { third } else { 0 },
                // v2: the primitive id; v3: the invocation id; v4: the fifth and sixth indices.
                if is_primitive { lane } else { 0 },
                0,
                0,
                // v5: the vertex id, unless an index buffer gives it; v6, v7: user VGPRs; v8: the
                // instance id.
                if lane < vertices && indices.is_none() {
                    first_vertex + lane
                } else {
                    0
                },
                0,
                0,
                0,
            ];
            for (register, word) in (0..).zip(words) {
                let value = self.constant(word);
                self.store_lane_masked(register, lane, value);
            }
            if let (Some(width), true) = (indices, lane < vertices) {
                let id = self.index_at(index_slot, width, lane);
                self.store_lane_masked(VERTEX_ID_REGISTER, lane, id);
            }
        }
    }

    /// Index `position` of the draw's index buffer at `slot`, `width` wide.
    ///
    /// Whole words of the bound buffer, so an index's word is read and its half taken.
    fn index_at(&mut self, slot: u32, width: IndexWidth, position: u32) -> Id {
        let bit = position * width.bytes() * 8;
        let word_index = self.constant(bit / 32);
        // A module declares the draw buffers it reads, and an indexed one always declares them.
        let Ok(word) = self.read_draw_buffer(slot, word_index) else {
            return self.constant(0);
        };
        match width {
            IndexWidth::Bits32 => word,
            IndexWidth::Bits16 => {
                let shift = self.constant(bit % 32);
                let shifted = self.binary(op::SHIFT_RIGHT_LOGICAL, word, shift);
                let low = self.constant(0xFFFF);
                self.binary(op::BITWISE_AND, shifted, low)
            }
        }
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
        if register < 32 && (self.unwritten_user_data >> register) & 1 != 0 {
            self.reads_unwritten = true;
        }
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
        if register < 128 {
            self.written.0 |= 1 << register;
        }
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
        let value = self.composite_vec4(components);
        self.builder.function(op::STORE, &[pointer.0, value.0]);
    }

    /// Four components as one `vec4` value.
    fn composite_vec4(&mut self, components: [Id; 4]) -> Id {
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
        value
    }

    /// Notes a guest-visible read of a register, and, in a mesh module, a geometry-engine input
    /// read before anything wrote it.
    fn note_read(&mut self, scalar: bool, register: u32) {
        if scalar && register < u128::BITS && self.images.opaque_scalars >> register & 1 != 0 {
            self.images.reads_opaque = true;
        }
        if self.stage != Stage::Mesh || self.reads_geometry_input {
            return;
        }
        let (limit, written) = if scalar {
            (
                GEOMETRY_SYSTEM_SGPRS,
                register < 128 && self.written.0 >> register & 1 != 0,
            )
        } else {
            let (word, bit) = ((register / 64) as usize, register % 64);
            (
                GEOMETRY_INPUT_VGPRS,
                self.written.1.get(word).is_some_and(|w| w >> bit & 1 != 0),
            )
        };
        if register < limit && !written {
            self.reads_geometry_input = true;
        }
    }

    fn store_lane_masked(&mut self, register: u32, lane: u32, value: Id) {
        if let Some(word) = self.written.1.get_mut((register / 64) as usize) {
            *word |= 1 << (register % 64);
        }
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
        if self.reads_geometry_input {
            return Err(TranslateError::ReadsGeometryInputs);
        }
        if self.images.reads_opaque {
            return Err(TranslateError::Unsupported {
                offset: 0,
                detail: concat!(
                    "a dispatch reads an image descriptor it loaded from its table as a value, ",
                    "and the load binds the image rather than reading the words"
                ),
            });
        }
        if self.reads_unwritten {
            return Err(TranslateError::Unsupported {
                offset: 0,
                detail: concat!(
                    "the program reads a user-data word the stream never wrote, so what it ",
                    "starts with is unknown"
                ),
            });
        }
        // Only a compute module publishes its registers: the epilogue is the compute harness's
        // oracle, and a graphics module's oracle is its attachment (D553).
        if let Some(state) = self.dispatch {
            // A guest dispatch's observation is whether it stayed exact, not its registers.
            state.publish(&mut self);
        } else if self.stage == Stage::Compute {
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
        self.complete_rectangles();
        self.builder.function(op::RETURN, &[]);
        self.builder.function(op::FUNCTION_END, &[]);
        self.builder.check()?;
        Ok((self.builder.finish(), self.translated))
    }

    /// A rectangle-list module's epilogue: for each declared primitive, the fourth corner as the
    /// vertex after every emitted one - `v1 + v2 - v0` in position and in each parameter - and the
    /// second triangle, `(v2, v1, v3)`, which winds the same way as `(v0, v1, v2)`.
    ///
    /// The corners are read from the module's copies, indexed by the guest's own indices masked to
    /// the copies' length: an index past the vertices a lane holds is the guest's fault, and reading
    /// past a private array would be the host's.
    fn complete_rectangles(&mut self) {
        let (Some(copies), Some(mesh)) = (self.rectangles.clone(), self.mesh.clone()) else {
            return;
        };
        let (u32_type, bool_type) = (self.u32_type, self.bool_type);
        let vertices = self.builder.id();
        self.builder
            .function(op::LOAD, &[u32_type.0, vertices.0, copies.vertices.0]);
        let primitives = self.builder.id();
        self.builder
            .function(op::LOAD, &[u32_type.0, primitives.0, copies.primitives.0]);
        let last = Self::constant(self, MESH_SLOTS - 1);
        let member = Self::constant(self, 0);
        for primitive in 0..self.lanes.min(MESH_SLOTS) {
            let this = Self::constant(self, primitive);
            let declared = self.builder.id();
            self.builder.function(
                op::ULESS_THAN,
                &[bool_type.0, declared.0, this.0, primitives.0],
            );
            let (body, merge) = (self.builder.id(), self.builder.id());
            self.builder.function(op::SELECTION_MERGE, &[merge.0, 0]);
            self.builder
                .function(op::BRANCH_CONDITIONAL, &[declared.0, body.0, merge.0]);
            self.builder.function(op::LABEL, &[body.0]);

            let pointer = self.builder.id();
            self.builder.function(
                op::ACCESS_CHAIN,
                &[copies.index_ptr.0, pointer.0, copies.indices.0, this.0],
            );
            let corners = self.builder.id();
            self.builder
                .function(op::LOAD, &[mesh.index_type.0, corners.0, pointer.0]);
            let mut raw = [Id(0); 3];
            let mut masked = [Id(0); 3];
            for (k, (raw, masked)) in raw.iter_mut().zip(masked.iter_mut()).enumerate() {
                *raw = self.builder.id();
                self.builder.function(
                    op::COMPOSITE_EXTRACT,
                    &[u32_type.0, raw.0, corners.0, k as u32],
                );
                *masked = self.builder.id();
                self.builder
                    .function(op::BITWISE_AND, &[u32_type.0, masked.0, raw.0, last.0]);
            }
            let fourth = self.builder.id();
            self.builder
                .function(op::IADD, &[u32_type.0, fourth.0, vertices.0, this.0]);

            // The position, then each parameter: `v1 + v2 - v0`.
            let mut arrays = vec![(copies.positions, None)];
            arrays.extend(
                copies
                    .parameters
                    .iter()
                    .map(|(location, copy)| (*copy, mesh.parameters.get(location).copied())),
            );
            for (copy, output) in arrays {
                let corner = self.fourth_corner(copies.vec4_ptr, copy, masked);
                let target = self.builder.id();
                match output {
                    None => self.builder.function(
                        op::ACCESS_CHAIN,
                        &[
                            mesh.vec4_ptr.0,
                            target.0,
                            mesh.vertices.0,
                            fourth.0,
                            member.0,
                        ],
                    ),
                    Some(parameter) => self.builder.function(
                        op::ACCESS_CHAIN,
                        &[mesh.vec4_ptr.0, target.0, parameter.0, fourth.0],
                    ),
                }
                self.builder.function(op::STORE, &[target.0, corner.0]);
            }

            let second = self.builder.id();
            self.builder.function(
                op::COMPOSITE_CONSTRUCT,
                &[mesh.index_type.0, second.0, raw[2].0, raw[1].0, fourth.0],
            );
            let slot = Self::constant(self, 2 * primitive + 1);
            let at = self.builder.id();
            self.builder.function(
                op::ACCESS_CHAIN,
                &[mesh.index_ptr.0, at.0, mesh.indices.0, slot.0],
            );
            self.builder.function(op::STORE, &[at.0, second.0]);
            self.builder.function(op::BRANCH, &[merge.0]);
            self.builder.function(op::LABEL, &[merge.0]);
        }
    }

    /// `v1 + v2 - v0` over three elements of a rectangle copy, by index.
    fn fourth_corner(&mut self, pointer: Id, copy: Id, corners: [Id; 3]) -> Id {
        let vec4 = self.vec4;
        let mut values = [Id(0); 3];
        for (value, index) in values.iter_mut().zip(corners) {
            let at = self.builder.id();
            self.builder
                .function(op::ACCESS_CHAIN, &[pointer.0, at.0, copy.0, index.0]);
            *value = self.builder.id();
            self.builder.function(op::LOAD, &[vec4.0, value.0, at.0]);
        }
        let sum = self.builder.id();
        self.builder
            .function(op::FADD, &[vec4.0, sum.0, values[1].0, values[2].0]);
        let corner = self.builder.id();
        self.builder
            .function(op::FSUB, &[vec4.0, corner.0, sum.0, values[0].0]);
        corner
    }

    /// The clip-space position that lands a window-space one on its pixel (D731): `(x, y, z, q)`,
    /// where `q` is `1/W`, becomes `(x W / S, y W / S, z W, W)` for `W = 1 / q` and `S` the
    /// [`WINDOW_SPACE_SCALE`] the draw's viewport scales by. Vulkan divides by `W` and scales by
    /// `S`, giving back `(x, y, z)`; and `W` is what it interpolates perspective by, as the
    /// hardware does with `1/W` from the fourth component.
    fn window_to_clip(&mut self, [x, y, depth, reciprocal]: [Id; 4]) -> [Id; 4] {
        let one = self.float_constant(1.0);
        let inverse_scale = self.float_constant(WINDOW_SPACE_SCALE.recip());
        let clip_w = self.float_binary(op::FDIV, one, reciprocal);
        let per_pixel = self.float_binary(op::FDIV, inverse_scale, reciprocal);
        [
            self.float_binary(op::FMUL, x, per_pixel),
            self.float_binary(op::FMUL, y, per_pixel),
            self.float_binary(op::FMUL, depth, clip_w),
            clip_w,
        ]
    }

    /// One 32-bit float operation on two float values.
    fn float_binary(&mut self, operation: u16, lhs: Id, rhs: Id) -> Id {
        let result = self.builder.id();
        self.builder
            .function(operation, &[self.f32_type.0, result.0, lhs.0, rhs.0]);
        result
    }

    /// A 32-bit float constant, declared once per value.
    fn float_constant(&mut self, value: f32) -> Id {
        if let Some(id) = self.float_constants.get(&value.to_bits()) {
            return *id;
        }
        let id = self.builder.id();
        self.builder
            .declare(op::CONSTANT, &[self.f32_type.0, id.0, value.to_bits()]);
        self.float_constants.insert(value.to_bits(), id);
        id
    }

    /// Stores `value` into element `index` of a rectangle copy, when this module keeps them.
    fn copy_element(&mut self, array: Id, pointer: Id, index: u32, value: Id) {
        let at = Self::constant(self, index);
        let element = self.builder.id();
        self.builder
            .function(op::ACCESS_CHAIN, &[pointer.0, element.0, array.0, at.0]);
        self.builder.function(op::STORE, &[element.0, value.0]);
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
        let (vertices, primitives) = match self.rectangles.clone() {
            None => (vertices, primitives),
            // Each rectangle adds its fourth corner after the emitted vertices, and a second
            // triangle.
            Some(copies) => {
                self.builder
                    .function(op::STORE, &[copies.vertices.0, vertices.0]);
                self.builder
                    .function(op::STORE, &[copies.primitives.0, primitives.0]);
                let u32_type = self.u32_type;
                let total = self.builder.id();
                self.builder
                    .function(op::IADD, &[u32_type.0, total.0, vertices.0, primitives.0]);
                let doubled = self.builder.id();
                self.builder.function(
                    op::IADD,
                    &[u32_type.0, doubled.0, primitives.0, primitives.0],
                );
                (total, doubled)
            }
        };
        self.builder
            .function(op::SET_MESH_OUTPUTS_EXT, &[vertices.0, primitives.0]);
        Some(())
    }

    fn write_mesh_position(&mut self, lane: u32, components: [Id; 4]) -> Option<()> {
        let mesh = self.mesh.clone()?;
        let components = if self.window_space {
            self.window_to_clip(components)
        } else {
            components
        };
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
        if let Some(copies) = self.rectangles.clone() {
            let value = self.composite_vec4(components);
            self.copy_element(copies.positions, copies.vec4_ptr, lane, value);
        }
        Some(())
    }

    fn write_mesh_parameter(
        &mut self,
        location: u32,
        lane: u32,
        components: [Id; 4],
    ) -> Option<()> {
        // A parameter with a flat twin is exported at its twin's location too (D742).
        if let Some(twin) = self
            .flat_twins
            .and_then(|twins| twins.location_of(location))
            && twin != location
        {
            self.write_mesh_parameter(twin, lane, components)?;
        }
        let mesh = self.mesh.clone()?;
        let variable = *mesh.parameters.get(&location)?;
        let slot = Self::constant(self, lane);
        let pointer = self.builder.id();
        self.builder.function(
            op::ACCESS_CHAIN,
            &[mesh.vec4_ptr.0, pointer.0, variable.0, slot.0],
        );
        self.store_vec4(pointer, components);
        if let Some(copies) = self.rectangles.clone()
            && let Some(copy) = copies.parameters.get(&location).copied()
        {
            let value = self.composite_vec4(components);
            self.copy_element(copy, copies.vec4_ptr, lane, value);
        }
        Some(())
    }

    fn write_mesh_indices(&mut self, lane: u32, indices: &[Id]) -> Option<()> {
        let mesh = self.mesh.clone()?;
        // A rectangle's first triangle is its three corners; its second follows it.
        let slot = if self.rectangles.is_some() {
            Self::constant(self, 2 * lane)
        } else {
            Self::constant(self, lane)
        };
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
        if let Some(copies) = self.rectangles.clone() {
            self.copy_element(copies.indices, copies.index_ptr, lane, value);
        }
        Some(())
    }

    fn mesh_primitive(&self) -> MeshPrimitive {
        self.primitive
    }

    fn read_vector_at(&mut self, register: u32, lane: Id) -> Option<Id> {
        let register_index = self.constant(register);
        let pointer = self.builder.id();
        self.builder.function(
            op::ACCESS_CHAIN,
            &[
                self.lane_ptr.0,
                pointer.0,
                self.vectors.0,
                register_index.0,
                lane.0,
            ],
        );
        let loaded = self.builder.id();
        self.builder
            .function(op::LOAD, &[self.u32_type.0, loaded.0, pointer.0]);
        Some(loaded)
    }

    fn attribute_input(&self, attribute: u32, flat: bool) -> Option<(Id, Id)> {
        let (vec4, _) = self.output?;
        let location = flat
            .then(|| {
                self.flat_twins
                    .and_then(|twins| twins.location_of(attribute))
            })
            .flatten()
            .unwrap_or(attribute);
        self.inputs.get(&location).map(|input| (vec4, *input))
    }

    /// The whole wavefront: one invocation stands in for every lane.
    fn memory_words(&self) -> u32 {
        self.memory_words
    }

    fn memory_base(&self) -> u32 {
        self.memory_base
    }

    fn memory_high(&self) -> u32 {
        self.memory_high
    }

    fn exact_memory(&self) -> bool {
        self.dispatch.is_some()
    }

    fn load_descriptor_opaquely(&mut self, first: u32, registers: u32) -> bool {
        // One image descriptor, or two side by side - each half traced as one.
        let traced = registers % model::IMAGE_DESCRIPTOR_REGISTERS == 0
            && (first..first + registers)
                .step_by(model::IMAGE_DESCRIPTOR_REGISTERS as usize)
                .all(|half| self.descriptor_loads.0.contains_key(&half));
        if !self.exact_memory() || !traced || registers > 2 * model::IMAGE_DESCRIPTOR_REGISTERS {
            return false;
        }
        for register in first..first + registers {
            if register < u128::BITS {
                self.images.opaque_scalars |= 1 << register;
            }
        }
        true
    }

    fn note_escape(&mut self, escaped: Id, lane: Option<u32>) {
        if let Some(state) = self.dispatch {
            state.note(self, escaped, lane);
        }
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
            Operand::Scalar(register) => {
                self.note_read(true, u32::from(*register));
                Ok(self.load_scalar(u32::from(*register)))
            }
            Operand::Vector(register) => {
                self.note_read(false, u32::from(*register));
                Ok(self.load_lane(u32::from(*register), lane))
            }
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
        if register < u128::BITS {
            self.images.written_scalars |= 1 << register;
            self.images.opaque_scalars &= !(1 << register);
        }
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
        if self.stage == Stage::Mesh {
            return Err(concat!(
                "a primitive shader stores to an image, and its stage binds none - a storage image ",
                "is bound to a pixel shader or a compute dispatch"
            ));
        }
        if let Some((stored, was)) = self.images.stored {
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

        // A compute module's storage image is the guest's, so where its descriptor came from is what
        // binds it; a fragment module's is a harness image.
        if self.stage == Stage::Compute {
            let mut source = self.new_texture_source(descriptor, None)?;
            source.slot = 0;
            self.images.stored_source = Some(source);
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
        self.images.stored = Some((stored, descriptor));
        Ok(stored)
    }

    fn sampled_image(
        &mut self,
        descriptor: u32,
        sampler: Option<u32>,
        at: u32,
    ) -> Result<model::Texture, &'static str> {
        // The fragment stage only: the harness binds its sampled image with fragment stage flags,
        // and the `vec4` type a sample answers with is declared by the colour output, which only a
        // fragment module has.
        // A compute module fetches: with no derivatives, a sample's level is not defined there.
        if self.stage == Stage::Mesh || (self.stage == Stage::Compute && sampler.is_some()) {
            return Err(concat!(
                "only a pixel shader samples a texture here, and a compute dispatch fetches one - ",
                "a primitive shader's image and a compute sample are not bound"
            ));
        }
        // The load that filled the descriptor registers for this sample, where it is known: a shader
        // that reuses the registers for a second texture names a different one there.
        let reaching = self.descriptor_loads.2.get(&at).copied();
        let loaded_by = |bound: &BoundTexture| {
            reaching.is_some_and(|(table, offset)| {
                table == Some(bound.source.table) && bound.source.table_offset == Some(offset)
            })
        };
        let rebound = reaching.is_some()
            && self
                .textures
                .iter()
                .any(|b| b.descriptor == descriptor && b.disturbed && !loaded_by(b));
        if !rebound
            && let Some(bound) = self.textures.iter_mut().find(|b| {
                b.descriptor == descriptor && (reaching.is_none() || !b.disturbed || loaded_by(b))
            })
        {
            // The same load reached it again: the same texture.
            if bound.disturbed && loaded_by(bound) {
                bound.disturbed = false;
            }
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
        let source = self.new_texture_source(descriptor, reaching)?;
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

    fn draw_buffer_slot(&self, instruction: &Instruction) -> Option<u32> {
        let (_, served) = self.draw_buffers.as_ref()?;
        served.get(&instruction.offset).copied()
    }

    fn buffer_descriptor_format(&self, slot: u32) -> Option<u32> {
        self.buffer_formats?
            .0
            .get(usize::try_from(slot).ok()?)
            .copied()
            .flatten()
    }

    fn read_draw_buffer(&mut self, slot: u32, word_index: Id) -> Result<Id, TranslateError> {
        let Some((array, _)) = self.draw_buffers else {
            return Err(TranslateError::Unsupported {
                offset: 0,
                detail: "a draw buffer read in a module that declares none (D733)",
            });
        };
        let u32_type = self.u32_type;
        let buffer = Self::constant(self, slot);
        let member = Self::constant(self, 0);
        let zero = member;
        let bool_type = self.bool_type;
        let b = &mut self.builder;
        let block = b.id();
        b.function(
            op::ACCESS_CHAIN,
            &[array.block_ptr.0, block.0, array.variable.0, buffer.0],
        );
        let length = b.id();
        b.function(op::ARRAY_LENGTH, &[u32_type.0, length.0, block.0, 0]);
        // Past the bound words the read takes word zero instead: defined, and never the answer,
        // since the host binds every word the descriptor admits.
        let inside = b.id();
        b.function(
            op::ULESS_THAN,
            &[bool_type.0, inside.0, word_index.0, length.0],
        );
        let index = b.id();
        b.function(
            op::SELECT,
            &[u32_type.0, index.0, inside.0, word_index.0, zero.0],
        );
        let pointer = b.id();
        b.function(
            op::ACCESS_CHAIN,
            &[
                array.element_ptr.0,
                pointer.0,
                array.variable.0,
                buffer.0,
                member.0,
                index.0,
            ],
        );
        let value = b.id();
        b.function(op::LOAD, &[u32_type.0, value.0, pointer.0]);
        Ok(value)
    }

    fn memory_buffer(&self) -> Id {
        self.memory
    }

    fn memory_element_ptr(&self) -> Id {
        self.memory_element_ptr
    }

    fn read_scalar(&mut self, register: u32) -> Id {
        self.note_read(true, register);
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

/// What a module tracks to say where a compute dispatch's image descriptors are.
#[derive(Debug, Clone, Copy, Default)]
struct ImageTrace {
    /// The storage image, declared the first time an instruction stores to one.
    ///
    /// Declaring it declares a capability that a device without the matching feature refuses, so a
    /// module that never stores never declares one (D692). The image descriptor register it was
    /// first named with rides along, so stores follow the same register rules as samples.
    stored: Option<(model::Stored, u32)>,
    /// Where the stored image's descriptor came from, for a compute module, whose storage image is
    /// the guest's own rather than a harness image.
    stored_source: Option<TextureSource>,
    /// Scalar registers the program has written, one bit each: a descriptor still in the registers
    /// the user data seeded is read from the user data.
    written_scalars: u128,
    /// The user-data registers this stage was seeded with: the first, and how many.
    user_data_registers: (u32, u32),
    /// Scalar registers holding an image descriptor a dispatch loaded from its table without
    /// reading memory, one bit each: the image is bound from the table itself, so the registers
    /// hold nothing a value read could use.
    opaque_scalars: u128,
    /// Whether anything read one of those registers as a value.
    reads_opaque: bool,
}

impl ImageTrace {
    fn seeded(first: u32, count: u32) -> Self {
        Self {
            user_data_registers: (first, count),
            ..Self::default()
        }
    }
}

/// The textures a module samples, in slot order, and its stored image, where each one's
/// descriptor came from.
pub type ImageSources = (Vec<TextureSource>, Option<TextureSource>);

/// Which texture a translated module samples at which binding, and where its descriptor comes from:
/// the descriptor table the image descriptor was loaded from, and the byte offset in it. `None` when
/// the module did not load it from a table; a pipeline then reads offset zero of the table at the
/// stage's first two user-data words.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TextureSource {
    /// 0 for the first texture the module samples, 1 for a second.
    pub slot: u32,
    /// The descriptor's byte offset in the table, when a load from the table put it there.
    pub table_offset: Option<u32>,
    /// Where the table's address came from.
    #[serde(default)]
    pub table: TableBase,
    /// The sampler descriptor's byte offset in the same table, when the module samples through one
    /// it loaded from there. `None` for a fetch, which names no sampler, and for a sampler that
    /// came from anywhere else: the texture is then read with default sampling.
    #[serde(default)]
    pub sampler_offset: Option<u32>,
    /// The stage's user-data word the descriptor's eight registers start at, when the program
    /// reads it where the user data put it, never having written those registers: a compute
    /// dispatch's descriptors arrive this way. `None` for a descriptor from a table.
    #[serde(default)]
    pub user_data: Option<u32>,
}

/// One half of a descriptor table's 64-bit address, as the program formed it before loading from
/// it: a user-data word of the stage, or a constant.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum TableWord {
    /// The stage's user-data word at this index.
    UserData(u32),
    /// This value.
    Constant(u32),
}

/// A descriptor table's address: its low and high words. The default is the stage's first two
/// user-data words, where the open-toolchain GL context puts its table.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct TableBase {
    /// Address bits 31:0.
    pub low: TableWord,
    /// Address bits 63:32.
    pub high: TableWord,
}

impl Default for TableBase {
    fn default() -> Self {
        Self {
            low: TableWord::UserData(0),
            high: TableWord::UserData(1),
        }
    }
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
    first_register: u32,
) -> (DescriptorLoads, DescriptorLoads, ReachingLoads) {
    let mut loads: DescriptorLoads = BTreeMap::new();
    let mut sampler_loads: DescriptorLoads = BTreeMap::new();
    // The image descriptor load that last filled each register group on every path here, and the
    // one reaching each image instruction.
    let mut live = LiveLoads::new(decode, encodings);
    let mut reaching: ReachingLoads = BTreeMap::new();
    // What each scalar register holds, in program order: a user-data word from where the hardware
    // loads the stage's, or a constant a move put there. Anything else written is untraced.
    let mut held: Vec<Option<TableWord>> = vec![None; model::SCALAR_REGISTERS as usize];
    for (index, word) in
        (0..USER_DATA_STAGE_WORDS).zip(held.iter_mut().skip(first_register as usize))
    {
        *word = Some(TableWord::UserData(index));
    }
    let forget = |held: &mut Vec<Option<TableWord>>, register: u16, count: usize| {
        for word in held.iter_mut().skip(usize::from(register)).take(count) {
            *word = None;
        }
    };
    for instruction in &decode.instructions {
        live.arrive(instruction.offset);
        let Some(family) = instruction
            .encoding
            .and_then(|i| encodings.encodings().get(usize::from(i)))
            .map(|e| e.name.as_str())
        else {
            live.loads.clear();
            continue;
        };
        let name = encodings
            .mnemonic_for(family, instruction.opcode)
            .unwrap_or("");
        live.leave(instruction, name);
        if let Some(load) =
            image_descriptor(instruction, name).and_then(|first| live.loads.get(&first))
        {
            reaching.insert(instruction.offset, *load);
        }
        match (name, instruction.operands.as_slice()) {
            ("s_mov_b32", [Operand::Scalar(destination), source]) => {
                forget_live(&mut live.loads, *destination, 1);
                let value = moved_word(&held, source);
                if let Some(slot) = held.get_mut(usize::from(*destination)) {
                    *slot = value;
                }
            }
            (
                "s_load_dword" | "s_load_dwordx2" | "s_load_dwordx4" | "s_load_dwordx8"
                | "s_load_dwordx16",
                [
                    Operand::Scalar(destination),
                    Operand::Scalar(base),
                    Operand::Immediate(offset),
                ],
            ) => {
                // Eight words is an image descriptor, four a sampler descriptor, kept apart so a
                // sampler's load is never taken for an image's. Sixteen are two image descriptors
                // side by side, as a compiler merges two adjacent eight-word loads.
                let (into, descriptors) = match name {
                    "s_load_dwordx8" => (Some(&mut loads), 1),
                    "s_load_dwordx16" => (Some(&mut loads), 2),
                    "s_load_dwordx4" => (Some(&mut sampler_loads), 1),
                    _ => (None, 0),
                };
                let width = scalar_load_width(name);
                forget_live(&mut live.loads, *destination, width);
                let image = matches!(name, "s_load_dwordx8" | "s_load_dwordx16");
                if let Some(into) = into
                    && let Ok(offset) = u32::try_from(*offset)
                {
                    let at = |register: u16| held.get(usize::from(register)).copied().flatten();
                    let table = at(*base)
                        .zip(at(base + 1))
                        .map(|(low, high)| TableBase { low, high });
                    for descriptor in 0..descriptors {
                        let load = (table, offset + descriptor * 32);
                        let group = u32::from(*destination) + descriptor * 8;
                        into.entry(group).or_default().insert(load);
                        if image {
                            live.loads.insert(group, load);
                        }
                    }
                }
                forget(&mut held, *destination, width as usize);
            }
            // A scalar compare writes only the condition code: its first operand is a source.
            (_, _) if name.starts_with("s_cmp") || name.starts_with("s_bitcmp") => {}
            // Any other write: the destination first, and a carry-out's second, a pair wide at
            // most - untraced from here on.
            (_, [Operand::Scalar(destination), rest @ ..]) => {
                forget(&mut held, *destination, 2);
                forget_live(&mut live.loads, *destination, 2);
                if name.contains("_co_")
                    && let Some(Operand::Scalar(carry)) = rest.first()
                {
                    forget(&mut held, *carry, 2);
                    forget_live(&mut live.loads, *carry, 2);
                }
            }
            (_, [_, Operand::Scalar(carry), ..]) if name.contains("_co_") => {
                forget(&mut held, *carry, 2);
                forget_live(&mut live.loads, *carry, 2);
            }
            _ => {}
        }
    }
    (loads, sampler_loads, reaching)
}

/// The first register of the image descriptor an image instruction names: its first scalar
/// operand. `None` for any other instruction.
fn image_descriptor(instruction: &Instruction, name: &str) -> Option<u32> {
    if !name.starts_with("image_") {
        return None;
    }
    instruction
        .operands
        .iter()
        .find_map(|operand| match operand {
            Operand::Scalar(first) => Some(u32::from(*first)),
            _ => None,
        })
}

/// How many registers a scalar load names in its mnemonic fills.
fn scalar_load_width(name: &str) -> u32 {
    match name {
        "s_load_dword" => 1,
        "s_load_dwordx2" => 2,
        "s_load_dwordx4" => 4,
        "s_load_dwordx16" => 16,
        _ => 8,
    }
}

/// What an `s_mov_b32` from `source` puts in its destination, where it is traced.
fn moved_word(held: &[Option<TableWord>], source: &Operand) -> Option<TableWord> {
    match source {
        Operand::Scalar(from) => held.get(usize::from(*from)).copied().flatten(),
        Operand::Integer(value) => i32::try_from(*value)
            .ok()
            .map(|v| TableWord::Constant(u32::from_ne_bytes(v.to_ne_bytes()))),
        Operand::Literal(value) => Some(TableWord::Constant(*value)),
        _ => None,
    }
}

/// The image descriptor loads that fill each register group on every path to the instruction being
/// traced, carried in program order: a branch target keeps what every path arriving there agrees
/// on - the forward branches to it and the fall-through into it - and a loop's head, which a later
/// branch reaches with what is not yet known, keeps nothing.
struct LiveLoads {
    loads: Loads,
    /// What each forward branch carried to its target.
    pending: BTreeMap<u32, Vec<Loads>>,
    targets: std::collections::BTreeSet<u32>,
    /// Targets a branch after them goes to.
    backward: std::collections::BTreeSet<u32>,
    /// Whether the previous instruction lets control fall into this one.
    falls_through: bool,
}

impl LiveLoads {
    fn new(decode: &Decode, encodings: &EncodingTable) -> Self {
        let mut targets = std::collections::BTreeSet::new();
        let mut backward = std::collections::BTreeSet::new();
        for instruction in &decode.instructions {
            if is_branch(instruction, encodings)
                && let Ok(target) = crate::blocks::branch_target(instruction)
            {
                targets.insert(target);
                if target <= instruction.offset {
                    backward.insert(target);
                }
            }
        }
        Self {
            loads: BTreeMap::new(),
            pending: BTreeMap::new(),
            targets,
            backward,
            falls_through: true,
        }
    }

    /// Control reaches the instruction at `offset`.
    fn arrive(&mut self, offset: u32) {
        if !self.targets.contains(&offset) {
            return;
        }
        if self.backward.contains(&offset) {
            self.loads.clear();
            return;
        }
        let mut arriving = self.pending.remove(&offset).unwrap_or_default();
        if self.falls_through {
            arriving.push(std::mem::take(&mut self.loads));
        }
        let mut paths = arriving.into_iter();
        let mut agreed = paths.next().unwrap_or_default();
        for path in paths {
            agreed.retain(|group, load| path.get(group) == Some(load));
        }
        self.loads = agreed;
    }

    /// The instruction named `name` runs: a branch carries what holds to its target, and an
    /// unconditional one or the end lets nothing fall through.
    fn leave(&mut self, instruction: &Instruction, name: &str) {
        if (name == "s_branch" || name.starts_with("s_cbranch_"))
            && let Ok(target) = crate::blocks::branch_target(instruction)
            && target > instruction.offset
        {
            self.pending
                .entry(target)
                .or_default()
                .push(self.loads.clone());
        }
        self.falls_through = !matches!(name, "s_branch" | "s_endpgm");
    }
}

/// The image descriptor load filling each register group, by its first register.
type Loads = BTreeMap<u32, (Option<TableBase>, u32)>;

/// Whether `instruction` is a branch.
fn is_branch(instruction: &Instruction, encodings: &EncodingTable) -> bool {
    instruction
        .encoding
        .and_then(|i| encodings.encodings().get(usize::from(i)))
        .and_then(|e| encodings.mnemonic_for(&e.name, instruction.opcode))
        .is_some_and(|name| name == "s_branch" || name.starts_with("s_cbranch_"))
}

/// Forgets the image descriptor loads whose register groups overlap `count` registers from
/// `register`.
fn forget_live(live: &mut BTreeMap<u32, (Option<TableBase>, u32)>, register: u16, count: u32) {
    let (first, end) = (u32::from(register), u32::from(register) + count);
    live.retain(|group, _| group + model::IMAGE_DESCRIPTOR_REGISTERS <= first || *group >= end);
}

/// The image descriptor load reaching each image instruction, by the instruction's offset: the one
/// that last filled its descriptor registers in program order, with no branch target between.
type ReachingLoads = BTreeMap<u32, (Option<TableBase>, u32)>;

/// Descriptor loads by the first register they fill: the table each was loaded from (when the
/// program formed its address from user data or constants) and the byte offset.
type DescriptorLoads = BTreeMap<u32, std::collections::BTreeSet<(Option<TableBase>, u32)>>;

/// Every attribute the shader interpolates, in order and without repeats, with how it is read.
///
/// A pass of its own because fragment inputs are global variables declared in the header, before
/// any instruction is translated, and the `Flat` decoration belongs to the variable. An attribute
/// read both ways cannot carry one decoration, and the caller refuses it rather than picking.
fn interpolated_attributes(
    decode: &Decode,
    encodings: &EncodingTable,
    twins: Option<FlatTwins>,
) -> Result<Vec<(u32, Interpolation)>, TranslateError> {
    let read = attribute_reads(decode, encodings);
    let mut inputs = Vec::new();
    for (attribute, smooth, flat) in read {
        match (smooth, flat) {
            (true, true) => {
                // Both: the attribute's own location interpolates, and its flat reads take the
                // twin the draw gives it (D742).
                let twin = twins
                    .and_then(|twins| twins.location_of(attribute))
                    .ok_or(TranslateError::NeedsFlatTwins)?;
                inputs.push((attribute, Interpolation::Smooth));
                inputs.push((twin, Interpolation::Flat));
            }
            (_, true) => inputs.push((attribute, Interpolation::Flat)),
            _ => inputs.push((attribute, Interpolation::Smooth)),
        }
    }
    Ok(inputs)
}

/// The attributes a pixel shader reads both interpolated and flat, which a draw gives a flat twin
/// (D742), in the order they are first read.
#[must_use]
pub fn mixed_attributes(decode: &Decode, encodings: &EncodingTable) -> Vec<u32> {
    attribute_reads(decode, encodings)
        .into_iter()
        .filter(|(_, smooth, flat)| *smooth && *flat)
        .map(|(attribute, _, _)| attribute)
        .collect()
}

/// Every attribute a pixel shader reads, in the order first read.
#[must_use]
pub fn read_attributes(decode: &Decode, encodings: &EncodingTable) -> Vec<u32> {
    attribute_reads(decode, encodings)
        .into_iter()
        .map(|(attribute, _, _)| attribute)
        .collect()
}

/// Every attribute a pixel shader reads, in the order first read, with whether it is interpolated
/// and whether it is read flat: the hardware chooses per instruction, not per attribute.
fn attribute_reads(decode: &Decode, encodings: &EncodingTable) -> Vec<(u32, bool, bool)> {
    let mut attributes: Vec<(u32, bool, bool)> = Vec::new();
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
        let flat = how == Interpolation::Flat;
        match attributes
            .iter_mut()
            .find(|(seen, _, _)| *seen == attribute)
        {
            Some((_, smooth_seen, flat_seen)) => {
                *smooth_seen |= !flat;
                *flat_seen |= flat;
            }
            None => attributes.push((attribute, !flat, flat)),
        }
    }
    attributes
}

/// Which parameter locations a shader exports, in the order a mesh module declares them.
///
/// The mesh counterpart of `interpolated_attributes`: output variables are declared in the header.
/// A guest's `exp param3` becomes location 3. Position, primitive indices and colour attachments
/// have their own variables and are skipped.
#[must_use]
pub fn exported_parameters(decode: &Decode, encodings: &EncodingTable) -> Vec<u32> {
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

/// The channels of colour attachment zero a pixel shader's exports write, red in bit 0 to alpha in
/// bit 3: the union of every `exp mrt0`'s `EN` field (`aco_assembler.cpp:1005`). A channel outside
/// it holds whatever the output held, so a draw that writes it to its target is not the guest's.
#[must_use]
pub fn exported_colour_channels(decode: &Decode, encodings: &EncodingTable) -> u8 {
    let mut channels = 0;
    for instruction in &decode.instructions {
        let named = instruction
            .encoding
            .and_then(|i| encodings.encodings().get(usize::from(i)))
            .and_then(|e| encodings.mnemonic_for(&e.name, instruction.opcode));
        if named == Some("exp")
            && let Some(Operand::Immediate(0)) = instruction.operands.first()
        {
            channels |= (instruction.word & 0xF) as u8;
        }
    }
    channels
}

/// The buffers a draw binds for this module (D733): traced for a draw's stage when the caller binds
/// them, and none otherwise.
///
/// # Errors
///
/// More buffers than a stage's binding holds, refused rather than left to the window.
pub fn draw_buffers_for(
    decode: &Decode,
    encodings: &EncodingTable,
    stage: Stage,
    user_data: UserData,
) -> Result<crate::draw_buffers::DrawBuffers, TranslateError> {
    if !user_data.draw_buffers || stage == Stage::Compute {
        return Ok(crate::draw_buffers::DrawBuffers::default());
    }
    let buffers =
        crate::draw_buffers::trace(decode, encodings, user_data.first_register, user_data.count);
    if buffers.sources.len() > orbistoun_spirv::DRAW_BUFFERS_PER_STAGE as usize {
        return Err(TranslateError::Unsupported {
            offset: 0,
            detail: "the shader reads through more buffers than a stage of a draw binds (D733)",
        });
    }
    Ok(buffers)
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
) -> Result<(Vec<u32>, usize, ImageSources), TranslateError> {
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
    if let (Stage::Fragment, Some(inputs)) = (stage, user_data.pixel_inputs) {
        inputs.seeded()?;
    }
    if let (Stage::Compute, Some(inputs)) = (stage, user_data.compute) {
        inputs.check(width.lanes())?;
    }
    if let (Stage::Mesh, Some(geometry)) = (stage, user_data.geometry)
        && let Some(detail) = geometry.refusal(width.lanes())
    {
        return Err(TranslateError::Unsupported { offset: 0, detail });
    }
    let attributes = interpolated_attributes(decode, encodings, user_data.flat_twins)?;
    let mut parameters = exported_parameters(decode, encodings);
    // A primitive shader exports each parameter with a flat twin at the twin's location too
    // (D742).
    if let (Stage::Mesh, Some(twins)) = (stage, user_data.flat_twins) {
        let twinned: Vec<u32> = parameters
            .iter()
            .filter_map(|location| twins.location_of(*location))
            .collect();
        for location in twinned {
            if !parameters.contains(&location) {
                parameters.push(location);
            }
        }
    }
    let buffers = draw_buffers_for(decode, encodings, stage, user_data)?;
    let mut module = Wavefront::for_stage(
        encodings,
        width,
        stage,
        primitive,
        &attributes,
        &parameters,
        (window, user_data, &buffers),
    );
    module.descriptor_loads = descriptor_table_loads(decode, encodings, user_data.first_register);
    crate::control::emit(&mut module, decode, encodings)?;
    let sources = module.image_sources();
    let (words, translated) = module.finish()?;
    Ok((words, translated, sources))
}

#[cfg(test)]
mod assembly_tests {
    /// A strip's triangles take overlapping vertices, each odd one reversed in the rotation that
    /// keeps the provoking vertex where the convention looks: `i + 2` last, or `i` first.
    #[test]
    fn a_strip_s_triangles_keep_winding_and_their_provoking_vertex() {
        use super::Assembly;
        let last = Assembly::Strip {
            provoking_last: true,
        };
        let first = Assembly::Strip {
            provoking_last: false,
        };
        assert_eq!(Assembly::List.vertices_of(2), [6, 7, 8]);
        // A line takes two vertices; the third index is not read.
        assert_eq!(Assembly::LineList.vertices_of(2), [4, 5, 0]);
        assert_eq!(Assembly::LineStrip.vertices_of(2), [2, 3, 0]);
        assert_eq!(Assembly::LineList.vertices_for(3), 6);
        assert_eq!(Assembly::LineStrip.vertices_for(3), 4);
        assert_eq!(last.vertices_of(0), [0, 1, 2]);
        assert_eq!(last.vertices_of(1), [2, 1, 3], "reversed, 3 last");
        assert_eq!(first.vertices_of(1), [1, 3, 2], "reversed, 1 first");
        assert_eq!(last.vertices_of(2), [2, 3, 4]);
        // The two odd orderings are rotations of each other: the same winding.
        let [a, b, c] = last.vertices_of(5);
        assert_eq!(first.vertices_of(5), [b, c, a]);
        assert_eq!(last.vertices_for(4), 6);
        assert_eq!(Assembly::List.vertices_for(4), 12);
    }
}
