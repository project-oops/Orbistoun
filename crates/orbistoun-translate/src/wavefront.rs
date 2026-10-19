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
use crate::lane_exact::LaneMasks;
use crate::model::{self, Model};
use crate::predicated::{MEMORY_WORDS, OBSERVED_REGISTERS, OBSERVED_WORDS, REGISTER_COUNT};
use crate::{TranslateError, Width};

mod compute_inputs;
mod input_controls;
mod pixel_inputs;
pub use compute_inputs::{ComputeInputs, GLOBAL_BASE_PUSH_WORD, PARTIAL_PUSH_WORD, PartialGroups};
pub use input_controls::{INPUT_CONTROLS, InputControls};
pub use orbistoun_spirv::MAX_TEXTURES;
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
    /// Set when a scalar write lands inside the image group.
    ///
    /// Register numbers alone would let a shader load a second descriptor into the same registers
    /// and sample two textures while naming one; a write into the group makes the next sample
    /// refuse.
    disturbed: bool,
    /// Set when a scalar write lands inside the sampler group: the next sample refuses unless the
    /// same table load reached its sampler as reached the first one's, [`Self::sampler_load`].
    sampler_disturbed: bool,
    /// The table load that filled the sampler registers for the first sample, where it is known.
    sampler_load: Option<(Option<TableBase>, u32)>,
}

/// Words of local data share a translated module provides when its stage declares none
/// ([`UserData::local_words`]).
///
/// A placeholder, large enough for tests and cheap. A radeonsi culling primitive shader keeps 28
/// bytes a vertex, 1792 for a wave of 64, so a program's own declaration is what sizes a draw's.
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
    /// Pointer to one `float` of a vertex's clip distances, member one of its block.
    float_ptr: Id,
    /// The user clip planes, four `float`s each, loaded at entry from the push-constant block
    /// [`Self::plane_block`] names (D857): a draw's `PA_CL_UCP_*`, zero clipping nothing.
    planes: [[Id; 4]; CLIP_PLANES],
    /// The push-constant block the planes are read from, word `4 * plane + axis` from offset zero,
    /// and the pointer to one of its `float`s - which a mesh module may use, since its user data
    /// arrives in the draw-data buffer, not the block (D718).
    plane_block: (Id, Id),
    /// A layered module's layers (D859): `None` for one that exports none.
    layers: Option<MeshLayers>,
    /// A module that exports a point size (D867): a workgroup word per vertex slot holding the
    /// size's bits, one until the vertex exports its own. `None` for one that exports none.
    point_sizes: Option<Id>,
    /// A module whose draw clips by exported distances (D867): workgroup words holding each
    /// vertex's, [`CLIP_DISTANCES`] a vertex slot, zero until it exports them; `None` for one that
    /// clips only by the user planes.
    distances: Option<Id>,
    /// Which exported distances clip, one bit each (`PA_CL_VS_OUT_CNTL.CLIP_DIST_ENA_*`).
    clip_mask: u8,
}

/// What a module that exports a layer declares (D859): each primitive's cull flag, and a workgroup
/// word per vertex slot holding the layer that vertex exported, zero until it does.
#[derive(Debug, Clone, Copy)]
struct MeshLayers {
    /// The `CullPrimitiveEXT` array, one `bool` per primitive slot.
    cull: Id,
    /// Pointer to one `bool` of it.
    cull_ptr: Id,
    /// The layers, one word per vertex slot.
    words: Id,
}

/// Declares a layered module's cull flags and layer words (D859), where [`MeshReserved`] reserved
/// them.
fn declare_mesh_layers(
    b: &mut Builder,
    (u32_type, bool_type): (Id, Id),
    primitive: MeshPrimitive,
    reserved: &MeshReserved,
) -> Option<MeshLayers> {
    let (cull, words) = reserved.layers?;
    b.annotate(
        op::DECORATE,
        &[cull.0, decoration::BUILT_IN, built_in::CULL_PRIMITIVE_EXT],
    );
    b.annotate(op::DECORATE, &[cull.0, decoration::PER_PRIMITIVE_EXT]);
    let slots = b.id();
    b.declare(op::CONSTANT, &[u32_type.0, slots.0, primitive.slots()]);
    let (flags, flags_ptr, cull_ptr) = (b.id(), b.id(), b.id());
    b.declare(op::TYPE_ARRAY, &[flags.0, bool_type.0, slots.0]);
    b.declare(op::TYPE_POINTER, &[flags_ptr.0, OUTPUT, flags.0]);
    b.declare(op::TYPE_POINTER, &[cull_ptr.0, OUTPUT, bool_type.0]);
    b.declare(op::VARIABLE, &[flags_ptr.0, cull.0, OUTPUT]);
    let vertex_slots = b.id();
    b.declare(op::CONSTANT, &[u32_type.0, vertex_slots.0, MESH_SLOTS]);
    let (array, array_ptr) = (b.id(), b.id());
    b.declare(op::TYPE_ARRAY, &[array.0, u32_type.0, vertex_slots.0]);
    b.declare(op::TYPE_POINTER, &[array_ptr.0, WORKGROUP, array.0]);
    b.declare(op::VARIABLE, &[array_ptr.0, words.0, WORKGROUP]);
    Some(MeshLayers {
        cull,
        cull_ptr,
        words,
    })
}

/// The per-vertex block member a module that exports a point size writes it to (D867), after the
/// position and the clip distances.
const POINT_SIZE_MEMBER: u32 = 2;

/// The clip and cull distances a vertex can export: four in each of `pos2` and `pos3`'s vectors.
pub const CLIP_DISTANCES: u32 = 8;

/// The bits of `1.0`, the size a vertex that exports none is drawn at.
const ONE_BITS: u32 = 0x3f80_0000;

/// The user clip planes a module carries, `PA_CL_UCP_0` to `_5`.
pub const CLIP_PLANES: usize = 6;

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
    /// Where each attribute a pixel shader interpolates comes from (`SPI_PS_INPUT_CNTL_*`): the
    /// vertex output it reads, a constant, and whether it is flat. `None` when the stream wrote
    /// none, and then attribute `n` reads output `n`, interpolated as its instruction says.
    #[serde(default)]
    pub input_controls: Option<InputControls>,
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
    pub window_space: WindowSpace,
    /// Whether a primitive shader runs one invocation per lane (D760), and the host's subgroup width
    /// it runs on: a workgroup of the wave's width, each invocation one lane, its masks built and
    /// its lanes read by subgroup ballot and shuffle where the subgroup holds the whole wave, and
    /// through workgroup memory between barriers where it does not. `None` simulates the whole wave
    /// in one invocation.
    ///
    /// A guest dispatch's is the same: run one invocation per lane (D880) on a subgroup this wide,
    /// zero where the host's subgroup is not known. Its program's needs decide how its masks are
    /// held ([`crate::lane_exact::lane_masks`], D884).
    #[serde(default)]
    pub per_invocation: Option<u32>,
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
    /// Whether a vertex program's position and parameter exports go nowhere: a legacy vertex
    /// shader's streamout pass, run as a compute dispatch with the rasterizer killed
    /// (`DX_RASTERIZATION_KILL`), which keeps only what it stores to the streamout buffers (D861).
    #[serde(default)]
    pub discard_vertex_exports: bool,
    /// Whether a geometry stage also writes the clip-space position of its first
    /// [`CAPTURED_VERTICES`] vertices into the observation buffer, four words each - where a draw's
    /// vertices land, read back by a run that asks (`ORBISTOUN_CAPTURE_POSITIONS`). It only adds
    /// stores; what the draw rasterises is unchanged.
    #[serde(default)]
    pub capture_positions: bool,
    /// The draw's `PA_CL_VS_OUT_CNTL`, which says what a primitive shader's `pos1` to `pos3`
    /// exports are (D867): the misc vector where `VS_OUT_MISC_VEC_ENA` is set, then clip and cull
    /// distances 0-3 and 4-7 where `VS_OUT_CCDIST0/1_VEC_ENA` are, in that order, and which
    /// distances clip or cull. `None` where it is not known, which takes `pos1` for the misc
    /// vector and refuses `pos2` and `pos3`.
    #[serde(default)]
    pub position_vectors: Option<u32>,
    /// The draw's `PA_SU_POINT_SIZE`: the size, in eighths of a pixel, a point is drawn at whose
    /// vertex exports none (D867). `None` where it is not known, which draws such a point at one.
    #[serde(default)]
    pub point_size: Option<u32>,
    /// For each texture slot a pixel shader samples, which coordinates, across then down, it
    /// saturates before sampling: those the draw's sampler clamps to half a border (D743).
    #[serde(default)]
    pub saturated: [[bool; 2]; MAX_TEXTURES],
    /// The local data share the stage's program declares, in words: its `RSRC2`'s `LDS_SIZE`, in
    /// the 512-byte units radeonsi encodes it in (`ac_shader_encode_lds_size`). `None` when the
    /// stream declared none, and then the module provides a fixed 1 KiB.
    #[serde(default)]
    pub local_words: Option<u32>,
    /// The guest address the program was loaded at: what `s_getpc_b64` reads (D842). `None` when
    /// the module is translated for no particular address, and then that instruction is refused.
    #[serde(default)]
    pub program_address: Option<u64>,
    /// The colour targets a draw's pixel shader writes that hold integers, bit `K` for `mrtK`
    /// (D851): their outputs are `uvec4`s that take the exported bits as they are, which an
    /// attachment of an integer format requires. Zero for every float or normalised target.
    #[serde(default)]
    pub integer_targets: u8,
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

/// The slots [`BufferFormats`] keeps a format for: the first thirty-two of a stage's
/// [`orbistoun_spirv::DRAW_BUFFERS_PER_STAGE`], a bit each in its mask. A typed access through a later
/// slot finds no format, as for a slot the draw did not resolve, and is refused.
const FORMATTED_SLOTS: usize = 32;

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
pub struct BufferFormats {
    /// Each slot's word, zero where none was resolved.
    words: [u32; FORMATTED_SLOTS],
    /// Which slots were resolved, a bit each: the words packed, so the draw state a module is
    /// translated for stays small enough to pass by value.
    resolved: u32,
}

impl BufferFormats {
    /// Records `slot`'s word, or that the draw resolved none; a slot past a stage's buffers is
    /// none of a draw's.
    pub fn set(&mut self, slot: usize, word: Option<u32>) {
        let Some(at) = self.words.get_mut(slot) else {
            return;
        };
        *at = word.unwrap_or(0);
        self.resolved = self.resolved & !(1 << slot) | u32::from(word.is_some()) << slot;
    }

    /// `slot`'s word, `None` where the draw resolved none.
    #[must_use]
    pub fn get(&self, slot: usize) -> Option<u32> {
        let word = *self.words.get(slot)?;
        (self.resolved >> slot & 1 == 1).then_some(word)
    }

    /// Every slot's word in order, `None` where the draw resolved none.
    pub fn slots(&self) -> impl Iterator<Item = Option<u32>> + '_ {
        (0..self.words.len()).map(|slot| self.get(slot))
    }
}

/// Whether a program converts a buffer load or store by its descriptor's format (D738), and so is translated
/// per draw with [`UserData::buffer_formats`].
#[must_use]
pub fn reads_buffer_formats(decode: &Decode, encodings: &EncodingTable) -> bool {
    decode.instructions.iter().any(|instruction| {
        instruction
            .encoding
            .and_then(|i| encodings.encodings().get(usize::from(i)))
            .and_then(|e| encodings.mnemonic_for(&e.name, instruction.opcode))
            .is_some_and(|name| {
                name.starts_with("buffer_load_format_") || name.starts_with("buffer_store_format_")
            })
    })
}

/// The viewport scale a window-space draw runs under (D731), in pixels, on both axes, with no
/// offset: a window-space position `(x, y, z, 1/W)` is written `(x W / S, y W / S, z W, W)`, which
/// Vulkan's divide and this viewport take back to `(x, y, z)`. A power of two, so the scaling is
/// exact; and large enough that every position on a target of up to `S` pixels lies inside the
/// clip volume.
pub const WINDOW_SPACE_SCALE: f32 = 8192.0;

/// The space a primitive shader's position export is in, as the draw's `PA_CL_VTE_CNTL` says.
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
pub enum WindowSpace {
    /// Clip space, mapped through the viewport transform.
    #[default]
    Clip,
    /// Window space in radeonsi's form (D731): x, y and z already divided, the fourth component
    /// `1/W`.
    Divided,
    /// Window space before the divide (D780): `PA_CL_VTE_CNTL` zero - no scale or offset, and the
    /// hardware divides x, y and z by the fourth component, `W`, to reach the pixel and its depth.
    Undivided,
}

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
/// (`si_shader_args.c:95`), and `v8` the instance, the workgroup's `y` (D863). The invocation id,
/// the fifth vertex register and the user VGPRs read zero: one invocation, no adjacency.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct GeometryInputs {
    /// The vertex id of the first vertex thread; for an indexed draw, what is added to each index
    /// it fetches (`GE_INDX_OFFSET`, D843).
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
    /// A passthrough primitive shader's (`VGT_SHADER_STAGES_EN.PRIMGEN_PASSTHRU_EN`): `v0` holds
    /// the primitive already packed as `exp prim` takes it, three nine-bit vertex indices at bits
    /// 0, 10 and 20 (Mesa `ac_nir_lower_intrinsics_to_args.c:278`, `ac_nir_lower_ngg.c:127`),
    /// in place of the first two indices sixteen bits each.
    #[serde(default)]
    pub passthrough: bool,
    /// A legacy vertex shader's draw (D881): no primitive shader runs (`VGT_SHADER_STAGES_EN` sets no
    /// `PRIMGEN_EN`), so the program neither declares its counts nor exports its primitives. The
    /// module does both from the draw's own, a lane a vertex, and seeds the vertex shader's inputs:
    /// the vertex id in `v0` and the instance id in `v3` (radeonsi `declare_vs_input_vgprs`, GFX10).
    #[serde(default)]
    pub legacy: bool,
    /// A legacy vertex shader that takes streamout words (`SPI_SHADER_PGM_RSRC2_VS.SO_EN`): its
    /// buffer stores are its streamout, which the streamout pass carries out exactly (D861) and the
    /// draw drops (D881) - with streamout off the hardware hands it no vertices to store.
    #[serde(default)]
    pub streamout: bool,
}

/// What a module does of a legacy vertex shader's streamout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StreamoutRole {
    /// Nothing: no streamout, or a program that is no legacy vertex shader.
    None,
    /// The streamout pass (D861): the stores are carried out and the position and parameter
    /// exports dropped, the rasterizer being killed.
    Pass,
    /// The draw (D881): the exports drawn and the stores dropped, the streamout pass's to make.
    Draw,
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

/// The input register a primitive shader's vertex thread finds its instance id in: `v8`, after the
/// vertex id and the two user VGPRs (`si_shader_args.c:363-371`, D863).
const INSTANCE_ID_REGISTER: u32 = 8;

/// The input register a primitive shader's vertex thread finds its vertex id in: `v5`, after the
/// vertex indices, primitive id and invocation id (GFX10's non-passthrough primitive shader).
const VERTEX_ID_REGISTER: u32 = 5;

/// Where a legacy vertex shader's vertex id arrives on GFX10: `v0` (radeonsi
/// `declare_vs_input_vgprs`), D881.
const LEGACY_VERTEX_ID_REGISTER: u32 = 0;

/// Where its instance id arrives: `v3`, after a user VGPR and the primitive id.
const LEGACY_INSTANCE_ID_REGISTER: u32 = 3;

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
    /// A list of quads, two triangles each (D798): quad `q` is vertices `4q` to `4q + 3`, split as
    /// Mesa splits one for its provoking convention (`u_indices_gen.py:129-136`) - `(0, 1, 3)` and
    /// `(1, 2, 3)` under the last-vertex convention, `(0, 1, 2)` and `(0, 2, 3)` under the first.
    Quads {
        /// Whether the last vertex is the provoking one.
        provoking_last: bool,
    },
    /// A list of points: primitive `i` is vertex `i`, its one index packed in the first input
    /// register as a triangle's first is; the others are not read (`ac_nir_lower_ngg.c:130`, one
    /// vertex per primitive).
    Points,
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
            Self::Points => [i, 0, 0],
            Self::Quads { provoking_last } => {
                let base = i / 2 * 4;
                match (i % 2 == 0, provoking_last) {
                    (true, true) => [base, base + 1, base + 3],
                    (false, true) => [base + 1, base + 2, base + 3],
                    (true, false) => [base, base + 1, base + 2],
                    (false, false) => [base, base + 2, base + 3],
                }
            }
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
            Self::Quads { .. } => primitives.div_ceil(2) * 4,
            Self::Points => primitives,
        }
    }

    /// How many primitives a draw of `vertices` vertices makes: whole triangles of a list, whole
    /// lines, and whole quads two triangles each. The host's count of what the module counts at
    /// run time from the draw's vertex count (D745).
    #[must_use]
    pub const fn primitives_from(self, vertices: u32) -> u32 {
        match self {
            Self::List => vertices / 3,
            Self::LineList => vertices / 2,
            Self::Strip { .. } => vertices.saturating_sub(2),
            Self::LineStrip => vertices.saturating_sub(1),
            Self::Quads { .. } => vertices / 4 * 2,
            Self::Points => vertices,
        }
    }
}

/// The system scalar registers a primitive shader's module seeds for a draw of `vertices` vertices
/// and `primitives` primitives, by register: gs_tg_info in `s2` and merged_wave_info in `s3`, laid
/// out as [`GeometryInputs`] says. A host replay of the program's prefix starts from them as the
/// module does (D800).
#[must_use]
pub const fn geometry_system_scalars(vertices: u32, primitives: u32) -> [(u32, u32); 2] {
    [
        (2, vertices << 12 | primitives << 22),
        (3, vertices | primitives << 8 | 1 << 28),
    ]
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
    /// Interpolated linearly in screen space, from the `LINEAR_*` barycentrics. `NoPerspective`.
    NoPerspective,
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
/// and a fragment output's location. A compute module's workgroup is `waves` invocations, one per
/// wave of the guest's thread group (D858).
fn emit_header(
    b: &mut Builder,
    (stage, primitive, per_lane, waves): (Stage, MeshPrimitive, Option<(u32, bool)>, u32),
    main: Id,
    (output, depth): (Option<Id>, Option<Id>),
) {
    if stage == Stage::Mesh {
        // `MeshShadingEXT` implies `Shader`, and the extension must be declared with it.
        b.header(op::CAPABILITY, &[capability::MESH_SHADING_EXT]);
        // Every vertex carries its distances inside the user clip planes (D830).
        b.header(op::CAPABILITY, &[capability::CLIP_DISTANCE]);
        // One invocation per lane builds masks by ballot and reads lanes by shuffle where its
        // subgroup holds the wave (D760).
        if let Some((_, false)) = per_lane {
            for needed in [
                capability::GROUP_NON_UNIFORM,
                capability::GROUP_NON_UNIFORM_BALLOT,
                capability::GROUP_NON_UNIFORM_SHUFFLE,
            ] {
                b.header(op::CAPABILITY, &[needed]);
            }
        }
        let mut extension = Vec::new();
        extension.extend(Builder::literal_string("SPV_EXT_mesh_shader"));
        b.header(op::EXTENSION, &extension);
    } else {
        b.header(op::CAPABILITY, &[capability::SHADER]);
    }

    declare_inf_nan_preserve(b, main);
    b.header(op::MEMORY_MODEL, &[addressing::LOGICAL, memory::GLSL450]);

    match stage {
        // An invocation a wave (D858), or a lane of each wave where it runs one per lane (D880).
        Stage::Compute => {
            let size = waves * per_lane.map_or(1, |(lanes, _)| lanes);
            b.header(op::EXECUTION_MODE, &[main.0, mode::LOCAL_SIZE, size, 1, 1]);
        }
        // Required of every fragment entry point: Vulkan's framebuffer origin is the top left.
        Stage::Fragment => {
            b.header(op::EXECUTION_MODE, &[main.0, mode::ORIGIN_UPPER_LEFT]);
            // A module that exports depth replaces the rasterised depth with it.
            if depth.is_some() {
                b.header(op::EXECUTION_MODE, &[main.0, mode::DEPTH_REPLACING]);
            }
        }
        // One invocation stands in for the guest's whole wave; the maximum output is the lane
        // count, since the guest arranges one vertex per lane. The actual count is the runtime
        // value declared with `MSG_GS_ALLOC_REQ`.
        Stage::Mesh => {
            // A workgroup of the wave's width when each invocation is one lane (D760).
            let size = per_lane.map_or(1, |(lanes, _)| lanes);
            b.header(op::EXECUTION_MODE, &[main.0, mode::LOCAL_SIZE, size, 1, 1]);
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
    controls: Option<InputControls>,
) -> Vec<ReservedAttribute> {
    if stage != Stage::Fragment {
        return Vec::new();
    }
    attributes
        .iter()
        .map(|&(attribute, how)| {
            // Checked before the module was begun (`check_input_controls`), so a refusal here is
            // not reached; the attribute's own output stands in.
            let source = controls.map_or(input_controls::Source::Output(attribute), |controls| {
                controls
                    .source(attribute)
                    .unwrap_or(input_controls::Source::Output(attribute))
            });
            let flat = controls.is_some_and(|controls| controls.flat(attribute));
            ReservedAttribute {
                attribute,
                how: if flat { Interpolation::Flat } else { how },
                variable: b.id(),
                source,
            }
        })
        .collect()
}

/// One attribute a fragment module reads, reserved before the header: where it reads from, and
/// how it is interpolated.
struct ReservedAttribute {
    attribute: u32,
    how: Interpolation,
    variable: Id,
    source: input_controls::Source,
}

impl ReservedAttribute {
    /// Whether the entry point's interface names it: an input does, a constant is a module
    /// variable and below SPIR-V 1.4 must not be listed.
    fn is_input(&self) -> bool {
        matches!(self.source, input_controls::Source::Output(_))
    }
}

/// Declares one `vec4` per attribute the shader interpolates: a fragment input at the location of
/// the vertex output it reads, or a constant where its control names one.
///
/// Attribute `n` reads output `n` unless the stream's `SPI_PS_INPUT_CNTL_n` names another
/// ([`InputControls`]). Empty for a compute module.
fn declare_attribute_inputs(
    b: &mut Builder,
    (f32_type, vec4): (Id, Id),
    reserved: &[ReservedAttribute],
) -> BTreeMap<u32, Id> {
    let mut inputs = BTreeMap::new();
    if reserved.is_empty() {
        return inputs;
    }
    let input_ptr = b.id();
    b.declare(op::TYPE_POINTER, &[input_ptr.0, INPUT, vec4.0]);
    let mut constant_ptr = None;
    for input in reserved {
        let variable = input.variable;
        match input.source {
            input_controls::Source::Output(location) => {
                b.annotate(op::DECORATE, &[variable.0, decoration::LOCATION, location]);
                // Smooth is what an undecorated input does, so it takes no decoration.
                match input.how {
                    Interpolation::Flat => {
                        b.annotate(op::DECORATE, &[variable.0, decoration::FLAT]);
                    }
                    Interpolation::NoPerspective => {
                        b.annotate(op::DECORATE, &[variable.0, decoration::NO_PERSPECTIVE]);
                    }
                    Interpolation::Smooth => {}
                }
                b.declare(op::VARIABLE, &[input_ptr.0, variable.0, INPUT]);
            }
            input_controls::Source::Constant(value) => {
                let pointer = *constant_ptr.get_or_insert_with(|| {
                    let pointer = b.id();
                    b.declare(op::TYPE_POINTER, &[pointer.0, PRIVATE, vec4.0]);
                    pointer
                });
                let components: Vec<u32> = value
                    .iter()
                    .map(|component| {
                        let id = b.id();
                        b.declare(op::CONSTANT, &[f32_type.0, id.0, component.to_bits()]);
                        id.0
                    })
                    .collect();
                let whole = b.id();
                let mut composite = vec![vec4.0, whole.0];
                composite.extend(components);
                b.declare(op::CONSTANT_COMPOSITE, &composite);
                b.declare(op::VARIABLE, &[pointer.0, variable.0, PRIVATE, whole.0]);
            }
        }
        inputs.insert(input.attribute, variable);
    }
    inputs
}

/// Refuses, before a module is begun, an attribute whose control names what is not translated
/// ([`InputControls`]), and a remap alongside flat twins (D742), whose locations are the
/// attributes' own.
fn check_input_controls(
    attributes: &[(u32, Interpolation)],
    user_data: &UserData,
) -> Result<(), TranslateError> {
    let Some(controls) = user_data.input_controls else {
        return Ok(());
    };
    if user_data.flat_twins.is_some() && !controls.is_identity() {
        return Err(TranslateError::Unsupported {
            offset: 0,
            detail: concat!(
                "a pixel shader that reads an attribute both flat and interpolated, with inputs ",
                "the stream routes to other outputs, which is not translated"
            ),
        });
    }
    for (attribute, _) in attributes {
        controls.source(*attribute)?;
    }
    Ok(())
}

/// Declares the `vec4` colour output a fragment module exports to, and answers it with its type.
///
/// [`None`] for a compute module, which still declares the `vec4` an image access's texel needs.
fn declare_colour_output(
    b: &mut Builder,
    (f32_type, vec4): (Id, Id),
    output_ptr: Id,
    (output, stage, unsigned_ptr): (Option<Id>, Stage, Option<Id>),
) -> Option<(Id, Id)> {
    let Some(colour) = output else {
        if stage == Stage::Compute {
            b.declare(op::TYPE_VECTOR, &[vec4.0, f32_type.0, 4]);
        }
        return None;
    };
    b.declare(op::TYPE_VECTOR, &[vec4.0, f32_type.0, 4]);
    b.declare(op::TYPE_POINTER, &[output_ptr.0, OUTPUT, vec4.0]);
    // An integer target's output is a `uvec4` (D851); the `vec4` stays the module's float vector.
    b.declare(
        op::VARIABLE,
        &[unsigned_ptr.unwrap_or(output_ptr).0, colour.0, OUTPUT],
    );
    Some((vec4, colour))
}

/// Reserves a fragment module's colour output, and its depth output when it exports depth
/// (`exp mrtz`); neither for any other stage.
fn reserve_fragment_outputs(
    b: &mut Builder,
    stage: Stage,
    writes_depth: bool,
) -> (Option<Id>, Option<Id>) {
    let fragment = stage == Stage::Fragment;
    let output = fragment.then(|| b.id());
    (output, (fragment && writes_depth).then(|| b.id()))
}

/// Reserves a fragment module's colour outputs past `mrt0`'s: one for each target from `mrt1` to
/// `mrt7` that `targets` (bit `K` for `mrtK`, [`exported_colour_targets`]) has an export to. None
/// for any other stage.
fn reserve_more_colour_outputs(b: &mut Builder, stage: Stage, targets: u8) -> [Option<Id>; 8] {
    let mut more = [None; 8];
    if stage == Stage::Fragment {
        for (target, slot) in more.iter_mut().enumerate().skip(1) {
            if targets & (1 << target) != 0 {
                *slot = Some(b.id());
            }
        }
    }
    more
}

/// What a fragment module writes, each variable with its type.
#[derive(Debug, Clone, Copy)]
struct FragmentOutputs {
    /// The `vec4` colour output `mrt0` writes, declared for every fragment module.
    colour: Option<(Id, Id)>,
    /// The `vec4` colour outputs `mrt1` to `mrt7` write, by target, declared for each the module
    /// exports to; element zero is always empty.
    more_colour: [Option<Id>; 8],
    /// The `float` `FragDepth` output, declared for one that exports depth.
    depth: Option<(Id, Id)>,
    /// The `SampleMask` output, declared for one that exports a sample mask: the `Output` pointer
    /// type to one `uint` and the variable.
    sample_mask: Option<(Id, Id)>,
    /// The colour targets that hold integers, bit `K` for `mrtK` (D851): their outputs are
    /// `uvec4`s rather than `vec4`s.
    integer: u8,
    /// The `uvec4` type and the `Output` pointer to it the integer targets' outputs are declared
    /// with, when there are any.
    unsigned: Option<(Id, Id)>,
}

impl FragmentOutputs {
    /// Declares the outputs [`reserve_fragment_outputs`] reserved: the colour output
    /// ([`declare_colour_output`]) and, when reserved, the depth one ([`declare_depth_output`]).
    fn declare(
        b: &mut Builder,
        (f32_type, u32_type, vec4): (Id, Id, Id),
        output_ptr: Id,
        (colour, depth, stage, integer): (Option<Id>, Option<Id>, Stage, u8),
    ) -> Self {
        let integer = if stage == Stage::Fragment { integer } else { 0 };
        let unsigned = (integer != 0).then(|| {
            let vector = b.declare_type(op::TYPE_VECTOR, &[u32_type.0, 4]);
            let pointer = b.id();
            b.declare(op::TYPE_POINTER, &[pointer.0, OUTPUT, vector.0]);
            (vector, pointer)
        });
        let zero = unsigned
            .filter(|_| integer & 1 != 0)
            .map(|(_, pointer)| pointer);
        Self {
            colour: declare_colour_output(b, (f32_type, vec4), output_ptr, (colour, stage, zero)),
            more_colour: [None; 8],
            depth: depth.map(|variable| declare_depth_output(b, f32_type, variable)),
            sample_mask: None,
            integer,
            unsigned,
        }
    }

    /// The type of `mrtK`'s output: the `uvec4` for a target that holds integers, else `vec4`.
    fn colour_type(&self, target: u32, vec4: Id) -> Id {
        match self.unsigned {
            Some((unsigned, _)) if target < 8 && self.integer >> target & 1 == 1 => unsigned,
            _ => vec4,
        }
    }

    /// Declares the colour outputs [`reserve_more_colour_outputs`] reserved, each a `vec4` at the
    /// location of its target's number: location `K` is the colour attachment a render pass lists
    /// `K`th, the one the pipeline binds `mrtK`'s colour buffer to.
    fn with_more_colour(mut self, b: &mut Builder, output_ptr: Id, more: [Option<Id>; 8]) -> Self {
        if self.colour.is_none() {
            return self;
        }
        for (location, variable) in (0u32..).zip(more) {
            if let Some(variable) = variable {
                b.annotate(op::DECORATE, &[variable.0, decoration::LOCATION, location]);
                let pointer = match self.unsigned {
                    Some((_, unsigned)) if self.integer >> location & 1 == 1 => unsigned,
                    _ => output_ptr,
                };
                b.declare(op::VARIABLE, &[pointer.0, variable.0, OUTPUT]);
            }
        }
        self.more_colour = more;
        self
    }

    /// Declares the `SampleMask` output reserved as `variable`, an array of one `uint`.
    fn with_sample_mask(mut self, b: &mut Builder, u32_type: Id, variable: Option<Id>) -> Self {
        self.sample_mask = variable.map(|variable| {
            let length = b.id();
            b.declare(op::CONSTANT, &[u32_type.0, length.0, 1]);
            let array = b.id();
            b.declare(op::TYPE_ARRAY, &[array.0, u32_type.0, length.0]);
            let pointer = b.id();
            b.declare(op::TYPE_POINTER, &[pointer.0, OUTPUT, array.0]);
            let element = b.id();
            b.declare(op::TYPE_POINTER, &[element.0, OUTPUT, u32_type.0]);
            b.annotate(
                op::DECORATE,
                &[variable.0, decoration::BUILT_IN, built_in::SAMPLE_MASK],
            );
            b.declare(op::VARIABLE, &[pointer.0, variable.0, OUTPUT]);
            (element, variable)
        });
        self
    }

    /// The variables the entry point's interface names.
    fn interface(self) -> impl Iterator<Item = u32> {
        self.colour
            .into_iter()
            .chain(self.depth)
            .chain(self.sample_mask)
            .map(|(_, variable)| variable.0)
            .chain(
                self.more_colour
                    .into_iter()
                    .flatten()
                    .map(|variable| variable.0),
            )
    }
}

/// Declares the `FragDepth` output a fragment module exports depth to, and answers it with its
/// type.
fn declare_depth_output(b: &mut Builder, f32_type: Id, variable: Id) -> (Id, Id) {
    let pointer = b.id();
    b.annotate(
        op::DECORATE,
        &[variable.0, decoration::BUILT_IN, built_in::FRAG_DEPTH],
    );
    b.declare(op::TYPE_POINTER, &[pointer.0, OUTPUT, f32_type.0]);
    b.declare(op::VARIABLE, &[pointer.0, variable.0, OUTPUT]);
    (f32_type, variable)
}

/// Declares the user clip planes a mesh module reads (D830, D857): the `float[6]` type a vertex's
/// clip distances are, and the push-constant block of six planes of four `float`s the draw's planes
/// arrive in, `4 * plane + axis` from offset zero. Answers the type, the block's variable and the
/// pointer to one of its `float`s; the planes are loaded at entry.
fn declare_clip_planes(
    b: &mut Builder,
    (f32_type, u32_type): (Id, Id),
    length: u32,
) -> (Id, (Id, Id)) {
    let plane_count = b.id();
    b.declare(op::CONSTANT, &[u32_type.0, plane_count.0, length]);
    let distances = b.id();
    b.declare(op::TYPE_ARRAY, &[distances.0, f32_type.0, plane_count.0]);
    let words = b.id();
    b.declare(
        op::CONSTANT,
        &[u32_type.0, words.0, (CLIP_PLANES * 4) as u32],
    );
    let (array, block, block_ptr, element_ptr, variable) = (b.id(), b.id(), b.id(), b.id(), b.id());
    b.annotate(op::DECORATE, &[array.0, decoration::ARRAY_STRIDE, 4]);
    b.annotate(op::DECORATE, &[block.0, decoration::BLOCK]);
    b.annotate(op::MEMBER_DECORATE, &[block.0, 0, decoration::OFFSET, 0]);
    b.declare(op::TYPE_ARRAY, &[array.0, f32_type.0, words.0]);
    b.declare(op::TYPE_STRUCT, &[block.0, array.0]);
    let push = orbistoun_spirv::storage::PUSH_CONSTANT;
    b.declare(op::TYPE_POINTER, &[block_ptr.0, push, block.0]);
    b.declare(op::TYPE_POINTER, &[element_ptr.0, push, f32_type.0]);
    b.declare(op::VARIABLE, &[block_ptr.0, variable.0, push]);
    (distances, (variable, element_ptr))
}

/// Declares `words`, a workgroup array of `count` words.
fn declare_workgroup_words(b: &mut Builder, u32_type: Id, words: Id, count: u32) {
    let length = b.id();
    b.declare(op::CONSTANT, &[u32_type.0, length.0, count]);
    let (array, array_ptr) = (b.id(), b.id());
    b.declare(op::TYPE_ARRAY, &[array.0, u32_type.0, length.0]);
    b.declare(op::TYPE_POINTER, &[array_ptr.0, WORKGROUP, array.0]);
    b.declare(op::VARIABLE, &[array_ptr.0, words.0, WORKGROUP]);
}

/// Declares the per-vertex block's type: the position, the clip distances - each user plane's
/// (D830), and every exported one's where the draw clips by any - and, member two where the module
/// exports one, the point size (D867), with the workgroup words exported sizes and distances are
/// kept in until the vertices are written. Answers the user planes' push-constant block.
fn declare_vertex_block(
    b: &mut Builder,
    per_vertex: Id,
    (f32_type, u32_type, vec4): (Id, Id, Id),
    reserved: &MeshReserved,
) -> (Id, Id) {
    // Member one, every vertex's distance inside each user clip plane (D830).
    b.annotate(
        op::MEMBER_DECORATE,
        &[
            per_vertex.0,
            1,
            decoration::BUILT_IN,
            built_in::CLIP_DISTANCE,
        ],
    );
    // Every exported distance's place, where the draw clips by any (D867); the user planes' alone
    // otherwise.
    let length = if reserved.distances.is_some() {
        CLIP_DISTANCES
    } else {
        CLIP_PLANES as u32
    };
    let (distances, plane_block) = declare_clip_planes(b, (f32_type, u32_type), length);
    if let Some((_, words)) = reserved.distances {
        declare_workgroup_words(b, u32_type, words, MESH_SLOTS * CLIP_DISTANCES);
    }
    if let Some(words) = reserved.point_sizes {
        b.annotate(
            op::MEMBER_DECORATE,
            &[
                per_vertex.0,
                POINT_SIZE_MEMBER,
                decoration::BUILT_IN,
                built_in::POINT_SIZE,
            ],
        );
        b.declare(
            op::TYPE_STRUCT,
            &[per_vertex.0, vec4.0, distances.0, f32_type.0],
        );
        declare_workgroup_words(b, u32_type, words, MESH_SLOTS);
    } else {
        b.declare(op::TYPE_STRUCT, &[per_vertex.0, vec4.0, distances.0]);
    }
    plane_block
}

/// Declares everything a mesh module writes: the vertices, the indices, and one array per exported
/// parameter. [`None`] for any other stage.
///
/// The per-vertex outputs are an array of a `Block` struct whose member zero carries `Position`, as
/// the stage requires; a bare `vec4` array decorated `Position` is a vertex-shader shape and is
/// refused.
fn declare_mesh_outputs(
    b: &mut Builder,
    (f32_type, u32_type, vec4): (Id, Id, Id),
    (stage, primitive): (Stage, MeshPrimitive),
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
    // The triangle's `uvec3` is the `WorkgroupId` input's type too.
    let index_type = if index_width == 1 {
        u32_type
    } else {
        b.declare_type(op::TYPE_VECTOR, &[u32_type.0, index_width])
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

    let plane_block = declare_vertex_block(b, per_vertex, (f32_type, u32_type, vec4), reserved);
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

    let float_ptr = b.id();
    b.declare(op::TYPE_POINTER, &[float_ptr.0, OUTPUT, f32_type.0]);

    Some(MeshOutputs {
        vertices: reserved.vertices,
        indices: reserved.indices,
        parameters,
        vec4_ptr,
        index_ptr,
        index_type,
        index_width,
        float_ptr,
        planes: [[Id(0); 4]; CLIP_PLANES],
        plane_block,
        layers: None,
        point_sizes: reserved.point_sizes,
        distances: reserved.distances.map(|(_, words)| words),
        clip_mask: reserved.distances.map_or(0, |(mask, _)| mask),
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
    /// Where the program's exports are staged until it has run, declared once the mesh outputs are.
    staged: Option<StagedOutputs>,
    /// For a module that exports a layer (D859): the per-primitive cull flags, and the workgroup
    /// words each vertex's layer is kept in.
    layers: Option<(Id, Id)>,
    /// For a module that exports a point size (D867): the workgroup words each vertex's size is
    /// kept in.
    point_sizes: Option<Id>,
    /// For a module whose draw clips by exported distances (D867): which, one bit each, and the
    /// workgroup words each vertex's distances are kept in, [`CLIP_DISTANCES`] a vertex.
    distances: Option<(u8, Id)>,
}

impl MeshReserved {
    /// Reserves ids for a mesh module's outputs, or nothing at another stage; `layered` for one
    /// that exports a layer, `sized` for one that exports a point size, and `clip_mask` the
    /// exported distances its draw clips by.
    fn new(
        b: &mut Builder,
        stage: Stage,
        locations: &[u32],
        ((layered, sized), clip_mask): ((bool, bool), u8),
    ) -> Self {
        if stage != Stage::Mesh {
            return Self {
                vertices: Id(0),
                indices: Id(0),
                parameters: BTreeMap::new(),
                staged: None,
                layers: None,
                point_sizes: None,
                distances: None,
            };
        }
        Self {
            vertices: b.id(),
            indices: b.id(),
            parameters: locations
                .iter()
                .map(|location| (*location, b.id()))
                .collect(),
            staged: Some(StagedOutputs::reserve(b, locations)),
            layers: layered.then(|| (b.id(), b.id())),
            point_sizes: sized.then(|| b.id()),
            distances: (clip_mask != 0).then(|| (clip_mask, b.id())),
        }
    }

    /// Every variable the entry point must list.
    fn interface(&self, stage: Stage) -> Vec<u32> {
        if stage != Stage::Mesh {
            return Vec::new();
        }
        let mut ids = vec![self.vertices.0, self.indices.0];
        ids.extend(self.parameters.values().map(|id| id.0));
        ids.extend(self.staged.iter().flat_map(StagedOutputs::interface));
        ids.extend(
            self.layers
                .iter()
                .flat_map(|(cull, words)| [cull.0, words.0]),
        );
        ids.extend(self.point_sizes.iter().map(|words| words.0));
        ids.extend(self.distances.iter().map(|(_, words)| words.0));
        ids
    }

    /// Declares the staging, once the mesh outputs it mirrors are declared.
    fn declare_staged(
        &self,
        b: &mut Builder,
        mesh: Option<&MeshOutputs>,
        vec4: Id,
        ids: &Reserved,
    ) -> Option<StagedOutputs> {
        let (staged, outputs) = self.staged.clone().zip(mesh)?;
        Some(staged.declare(
            b,
            (ids.u32_type, vec4, outputs.index_type),
            (ids.counter_ptr, ids.counter_zero),
        ))
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

/// A mesh module's private copy of what the program exports - each lane's position, parameters and
/// primitive indices, and the counts its allocation request declared - written to the outputs only
/// once the program has run.
///
/// `OpSetMeshOutputsEXT` must come before every output write on every path to it
/// (`SPV_EXT_mesh_shader`). The program runs as a loop over its blocks, with the allocation request
/// in one arm and the exports in others, so no point inside the loop precedes them all. The
/// epilogue after the loop does: it declares the counts, then writes what was staged
/// ([`Wavefront::emit_mesh_outputs`]). Writing outputs before the declaration is a module AMD's
/// driver faulted compiling (a null read in `amdvlk64.dll`) and NVIDIA's ran. A rectangle's fourth
/// corner is formed from the same copies.
#[derive(Debug, Clone)]
struct StagedOutputs {
    /// `vec4` per lane: the positions.
    positions: Id,
    /// `vec4` per lane, per exported parameter location.
    parameters: BTreeMap<u32, Id>,
    /// One index element per lane: the primitive's corners.
    indices: Id,
    /// The vertex count the allocation request declared; zero until it does.
    vertices: Id,
    /// The primitive count it declared; zero until it does.
    primitives: Id,
    /// Pointer to one `vec4` of a copy.
    vec4_ptr: Id,
    /// Pointer to one element of the index copy.
    index_ptr: Id,
    /// Pointer to one private word.
    word_ptr: Id,
}

impl StagedOutputs {
    /// Reserves the staging's variables: the entry point lists them.
    fn reserve(b: &mut Builder, locations: &[u32]) -> Self {
        Self {
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
        }
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

    /// Declares the reserved variables as private storage, one element per lane, and the counts as
    /// words starting at zero: a program that never requests an allocation emits nothing.
    fn declare(
        mut self,
        b: &mut Builder,
        (u32_type, vec4, index_type): (Id, Id, Id),
        (word_ptr, zero): (Id, Id),
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
        b.declare(op::TYPE_ARRAY, &[index_array.0, index_type.0, lanes.0]);
        b.declare(
            op::TYPE_POINTER,
            &[index_array_ptr.0, PRIVATE, index_array.0],
        );
        b.declare(op::TYPE_POINTER, &[self.vec4_ptr.0, PRIVATE, vec4.0]);
        b.declare(op::TYPE_POINTER, &[self.index_ptr.0, PRIVATE, index_type.0]);
        b.declare(op::VARIABLE, &[vec4_array_ptr.0, self.positions.0, PRIVATE]);
        for variable in self.parameters.values() {
            b.declare(op::VARIABLE, &[vec4_array_ptr.0, variable.0, PRIVATE]);
        }
        b.declare(op::VARIABLE, &[index_array_ptr.0, self.indices.0, PRIVATE]);
        for word in [self.vertices, self.primitives] {
            b.declare(op::VARIABLE, &[word_ptr.0, word.0, PRIVATE, zero.0]);
        }
        self.word_ptr = word_ptr;
        self
    }
}

/// The words of local data share a module declares for a stage that declares `declared`: that
/// many, or [`LOCAL_WORDS`] where it declares none, rounded up to a power of two so an index is
/// masked into range ([`Model::local_word_index`]).
fn local_share_words(declared: Option<u32>) -> u32 {
    declared.unwrap_or(LOCAL_WORDS).max(1).next_power_of_two()
}

/// Declares the local data share: workgroup storage the lanes of a wavefront exchange values
/// through.
fn declare_local_share(
    b: &mut Builder,
    (u32_type, words): (Id, u32),
    local_count: Id,
    local_array: Id,
    local_array_ptr: Id,
    local_ptr: Id,
    local: Id,
) {
    // No initialiser: workgroup storage cannot have one, and the guest's is uninitialised too.
    b.declare(op::CONSTANT, &[u32_type.0, local_count.0, words]);
    b.declare(op::TYPE_ARRAY, &[local_array.0, u32_type.0, local_count.0]);
    b.declare(
        op::TYPE_POINTER,
        &[local_array_ptr.0, WORKGROUP, local_array.0],
    );
    b.declare(op::TYPE_POINTER, &[local_ptr.0, WORKGROUP, u32_type.0]);
    b.declare(op::VARIABLE, &[local_array_ptr.0, local.0, WORKGROUP]);
}

/// How many vertices a geometry stage writes the positions of when asked
/// ([`UserData::capture_positions`]), four words each: as many as the observation buffer holds
/// beside [`CAPTURED_COUNTS`].
pub const CAPTURED_VERTICES: u32 = (OBSERVED_WORDS - 4) / 4;

/// Where in the observation buffer a capturing geometry stage writes the vertices and primitives
/// it declared with `MSG_GS_ALLOC_REQ`, a word each: a draw whose vertices ran and that declared no
/// primitive rasterises nothing.
pub const CAPTURED_COUNTS: u32 = CAPTURED_VERTICES * 4;

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
///
/// The type is the module's one `uvec3`: a triangle mesh module has already declared it for its
/// indices, and a second declaration is invalid SPIR-V that AMD's driver faulted compiling.
fn declare_workgroup_id(b: &mut Builder, u32_type: Id) -> (Id, Id) {
    let pointer = b.id();
    let variable = b.id();
    b.annotate(
        op::DECORATE,
        &[variable.0, decoration::BUILT_IN, built_in::WORKGROUP_ID],
    );
    let uvec3 = b.declare_type(op::TYPE_VECTOR, &[u32_type.0, 3]);
    b.declare(op::TYPE_POINTER, &[pointer.0, INPUT, uvec3.0]);
    b.declare(op::VARIABLE, &[pointer.0, variable.0, INPUT]);
    (uvec3, variable)
}

/// Whether a module for `stage` runs one invocation per lane (D760) - the wave's width, and whether
/// its lanes exchange through workgroup memory because the host's subgroup is narrower - and how
/// many lanes the per-lane loops run: one then, else the lanes one invocation simulates. A guest
/// dispatch run per lane exchanges through workgroup memory only where its `masks` say (D880,
/// D884).
fn lanes_per_invocation(
    stage: Stage,
    width: Width,
    user_data: &UserData,
    masks: Option<LaneMasks>,
) -> (Option<(u32, bool)>, u32) {
    let dispatch = stage == Stage::Compute && user_data.compute.is_some();
    let per_lane = user_data
        .per_invocation
        .filter(|_| stage == Stage::Mesh || dispatch)
        .map(|subgroup| {
            let exchanges = if dispatch {
                masks == Some(LaneMasks::Exchange)
            } else {
                subgroup < width.lanes()
            };
            (width.lanes(), exchanges)
        });
    let modelled = if per_lane.is_some() {
        1
    } else {
        simulated_lanes(stage, width)
    };
    (per_lane, modelled)
}

/// A module's draw-buffer array and the slot each traced access reads (D733).
type DrawBuffersDeclared = (buffer::DrawBufferArray, BTreeMap<u32, u32>);

/// What a module run one invocation per lane declares (D760): the input that is its lane, and the
/// workgroup memory its lanes exchange through when the wave is wider than the host's subgroup.
#[derive(Debug, Clone, Copy)]
struct PerLane {
    input: Id,
    exchange: Option<(Id, Id)>,
    /// The `SubgroupId` input, where a guest dispatch's group of several waves makes its masks by
    /// ballot: each wave is a subgroup (D884).
    subgroup: Option<Id>,
}

impl PerLane {
    /// Declares them for a wave of `lanes`, exchanging where `exchanges`; `element` is the workgroup
    /// word pointer type the local data share declared. `by_index` takes the lane from the
    /// invocation's place in its workgroup though nothing is exchanged: a guest dispatch's, whose
    /// workgroup may be several waves (D880) - unless `by_subgroup`, where its waves are subgroups
    /// whose ballots are its masks (D884).
    fn declare(
        b: &mut Builder,
        (u32_type, element): (Id, Id),
        (lanes, exchanges): (u32, bool),
        (by_index, by_subgroup): (bool, bool),
    ) -> Self {
        Self {
            input: declare_lane_input(b, u32_type, exchanges || by_index),
            exchange: exchanges.then(|| declare_exchange(b, (u32_type, element), lanes)),
            subgroup: by_subgroup.then(|| declare_subgroup_id(b, u32_type)),
        }
    }

    /// The variables the entry point names.
    fn interface(&self) -> Vec<u32> {
        let mut variables = self.inputs();
        variables.extend(
            self.exchange
                .iter()
                .flat_map(|(mask, lanes)| [mask.0, lanes.0]),
        );
        variables
    }

    /// The inputs alone, which a module below SPIR-V 1.4 names and no other variable.
    fn inputs(&self) -> Vec<u32> {
        let mut variables = vec![self.input.0];
        variables.extend(self.subgroup.map(|input| input.0));
        variables
    }

    /// Loads the lane at entry.
    fn load(self, b: &mut Builder, u32_type: Id) -> Id {
        let lane = b.id();
        b.function(op::LOAD, &[u32_type.0, lane.0, self.input.0]);
        lane
    }
}

/// Declares the built-in input that is this invocation's lane when each invocation is one lane
/// (D760): its place in its subgroup where the subgroup holds the wave, and its place in the
/// workgroup where the lanes `exchange` through workgroup memory.
fn declare_lane_input(b: &mut Builder, u32_type: Id, exchange: bool) -> Id {
    let pointer = b.id();
    let variable = b.id();
    let which = if exchange {
        built_in::LOCAL_INVOCATION_INDEX
    } else {
        built_in::SUBGROUP_LOCAL_INVOCATION_ID
    };
    b.annotate(op::DECORATE, &[variable.0, decoration::BUILT_IN, which]);
    b.declare(op::TYPE_POINTER, &[pointer.0, INPUT, u32_type.0]);
    b.declare(op::VARIABLE, &[pointer.0, variable.0, INPUT]);
    variable
}

/// Declares the `SubgroupId` built-in input: which subgroup of its workgroup an invocation is in.
fn declare_subgroup_id(b: &mut Builder, u32_type: Id) -> Id {
    let pointer = b.id();
    let variable = b.id();
    b.annotate(
        op::DECORATE,
        &[variable.0, decoration::BUILT_IN, built_in::SUBGROUP_ID],
    );
    b.declare(op::TYPE_POINTER, &[pointer.0, INPUT, u32_type.0]);
    b.declare(op::VARIABLE, &[pointer.0, variable.0, INPUT]);
    variable
}

/// This invocation's lane when each invocation is one lane, and - for a guest dispatch whose group
/// is several waves of `width` run one invocation per lane (D880) - the first lane of its wave as a
/// place in its group: the invocation's place is the input, its lane that place's low bits, and its
/// wave's start the place less the lane. Where its waves are subgroups (D884), its lane is its place
/// in its subgroup and its wave's start its subgroup's number times the width.
fn lane_and_wave(
    b: &mut Builder,
    u32_type: Id,
    lane: Option<PerLane>,
    width: Option<u32>,
) -> (Option<Id>, Option<Id>) {
    let Some(index) = lane.map(|lane| lane.load(b, u32_type)) else {
        return (None, None);
    };
    let Some(width) = width else {
        return (Some(index), None);
    };
    if let Some(subgroup) = lane.and_then(|lane| lane.subgroup) {
        let (number, wide) = (b.id(), b.id());
        b.function(op::LOAD, &[u32_type.0, number.0, subgroup.0]);
        b.declare(op::CONSTANT, &[u32_type.0, wide.0, width]);
        let base = b.id();
        b.function(op::IMUL, &[u32_type.0, base.0, number.0, wide.0]);
        return (Some(index), Some(base));
    }
    let low_bits = b.id();
    b.declare(op::CONSTANT, &[u32_type.0, low_bits.0, width - 1]);
    let lane = b.id();
    b.function(op::BITWISE_AND, &[u32_type.0, lane.0, index.0, low_bits.0]);
    let base = b.id();
    b.function(op::ISUB, &[u32_type.0, base.0, index.0, lane.0]);
    (Some(lane), Some(base))
}

/// Declares the workgroup memory a wave's lanes exchange through when it is wider than the host's
/// subgroup (D760): a two-word lane mask, and one word per lane. `element` is the workgroup word
/// pointer type the local data share declared.
fn declare_exchange(b: &mut Builder, (u32_type, element): (Id, Id), lanes: u32) -> (Id, Id) {
    let mut array = |length: u32| {
        let (count, array, pointer, variable) = (b.id(), b.id(), b.id(), b.id());
        b.declare(op::CONSTANT, &[u32_type.0, count.0, length]);
        b.declare(op::TYPE_ARRAY, &[array.0, u32_type.0, count.0]);
        b.declare(op::TYPE_POINTER, &[pointer.0, WORKGROUP, array.0]);
        b.declare(op::VARIABLE, &[pointer.0, variable.0, WORKGROUP]);
        variable
    };
    let mask = array(2);
    let words = array(lanes);
    let _ = element;
    (mask, words)
}

/// Builds a wavefront-model module for one decoded shader.
#[derive(Debug)]
pub struct Wavefront<'a> {
    /// Which stage this module is for, and - when it is a fragment - where a colour goes.
    stage: Stage,
    /// The outputs a fragment module writes; none for any other stage.
    output: FragmentOutputs,
    /// The fragment input for each attribute the shader interpolates, by attribute index.
    ///
    /// Declared in the header, so the attributes are found by a pass over the decode before any
    /// instruction is translated.
    inputs: BTreeMap<u32, Id>,
    /// Each attribute's second, flat location (D742): where a pixel shader's flat reads of an
    /// attribute it also interpolates go, and where a primitive shader exports the parameter again.
    flat_twins: Option<FlatTwins>,
    /// What the module does of a legacy vertex shader's streamout (D861, D881).
    streamout: StreamoutRole,
    /// Whether the epilogue writes its first vertices' positions to the observation buffer
    /// ([`UserData::capture_positions`]).
    capture_positions: bool,
    /// The draw's position-vector routing ([`UserData::position_vectors`]).
    position_vectors: Option<u32>,
    /// The draw's point size register ([`UserData::point_size`]).
    point_size: Option<u32>,
    /// Whether a compute dispatch's fetched and stored images are unsigned integer ones.
    integer_images: [bool; 2],
    /// The coordinates each texture slot's samples saturate (D743).
    saturated: [[bool; 2]; MAX_TEXTURES],
    /// The four-component unsigned vector an integer image's texel is, declared on first use.
    uvec4: Option<Id>,
    /// What a mesh module writes, or [`None`] at any other stage.
    mesh: Option<MeshOutputs>,
    /// A mesh module's staged exports, or [`None`] at any other stage.
    staged: Option<StagedOutputs>,
    /// Whether a mesh module's position export is in window space (D731); see
    /// [`UserData::window_space`].
    window_space: WindowSpace,
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
    /// How many words of local data share the module declares ([`local_share_words`]).
    local_words: u32,
    builder: Builder,
    encodings: &'a EncodingTable,
    /// How many lanes the wave has, as its masks hold them: the lanes one invocation simulates
    /// ([`simulated_lanes`]), or the wave's width when each invocation is one lane.
    lanes: u32,
    /// How many lanes the per-lane loops run: [`Self::lanes`], or one when each invocation is one
    /// lane (D760).
    modelled: u32,
    /// This invocation's lane when each invocation is one lane (D760); `None` where one invocation
    /// simulates the wave.
    invocation: Option<Id>,
    /// The first lane of this invocation's wave, as a place in its group, where a guest dispatch's
    /// group of several waves runs one invocation per lane (D880).
    wave_base: Option<Id>,
    /// How a guest dispatch run one invocation per lane holds its masks (D880, D884); `None` for
    /// every other module.
    lane_masks: Option<LaneMasks>,
    /// A fragment module that reads its quad's other lanes: this invocation's index in its
    /// subgroup, whose low two bits are its place in its quad. `None` everywhere else.
    quad_invocation: Option<Id>,
    /// Where the lanes exchange masks and values when the wave is wider than the host's subgroup
    /// (D760): a two-word mask and a word per lane, in workgroup memory. `None` where a subgroup
    /// ballot and shuffle reach every lane, or one invocation is the whole wave.
    exchange: Option<(Id, Id)>,
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
    /// the image descriptors', the sampler descriptors', and the image load and the sampler load
    /// reaching each image instruction.
    descriptor_loads: (
        DescriptorLoads,
        DescriptorLoads,
        ReachingLoads,
        ReachingLoads,
    ),
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
    /// How many waves of its thread group a compute module's workgroup runs (D858).
    group_waves: u32,
    /// The scalar condition code, as a private variable holding 0 or 1.
    condition_code: Id,
    /// The `m0` register, as a private word starting at zero.
    m0: Id,
    translated: usize,
    /// The stage's `DX10_CLAMP` mode, from its `RSRC1`; `None` when unknown.
    dx10_clamp: Option<bool>,
    /// Where the program was loaded, for `s_getpc_b64` (D842); `None` when unknown.
    program_address: Option<u64>,
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
    /// A mesh module's instance: its workgroup's `y`, a workgroup per instance (D863).
    instance_id: Option<Id>,
    /// A mesh module's draw-data buffer and the first word of this workgroup's stride in it (D718),
    /// where its draw's vertex count is read from (D745).
    draw_data: Option<(buffer::StorageBuffer, Id)>,
    /// The push-constant block user data arrives in, where the module reads one: a dispatch's
    /// partial-group shape follows its user data there (D855).
    push_block: Option<buffer::StorageBuffer>,
    /// The draw's bound buffers, and the slot each traced access reads, by the access's offset
    /// (D733). `None` for a module that reads through none.
    draw_buffers: Option<(buffer::DrawBufferArray, BTreeMap<u32, u32>)>,
    /// A dispatch's write mask beside its buffers (D809): a word for each word of a buffer the
    /// program only stores to, set where a store lands, so the host writes back what the program
    /// stored and never reads such a buffer.
    write_mask: Option<buffer::DrawBufferArray>,
    /// The slots of the buffers the program only stores to, which [`Self::write_mask`] marks.
    unread_slots: std::collections::BTreeSet<u32>,
    /// The store that kept the program's buffers from being traced, by offset and name.
    untraced_by: Option<(u32, String)>,
    /// The global data share, declared on the first access to it (D789).
    global_share: Option<buffer::StorageBuffer>,
    /// Scalar registers the program filled with a plain copy of one still holding a user-data word,
    /// and that word: a descriptor moved out of its user data is still read from there.
    scalar_copies: BTreeMap<u32, u32>,
    /// Vector registers holding, in every lane, a user-data word a plain copy from a scalar put
    /// there, by the word: a dispatch's level, moved before the store that names it.
    vector_copies: BTreeMap<u32, u32>,
}

/// A primitive shader's system SGPRs, `s0`-`s7`, ahead of its user SGPRs: gs_tg_info in `s2` and
/// merged_wave_info in `s3` among them (`si_shader_args.c:304-322`).
const GEOMETRY_SYSTEM_SGPRS: u32 = 8;
/// Its input VGPRs, `v0`-`v8`: the vertex offsets, primitive and invocation ids, then the vertex
/// shader's vertex id and instance id (`si_shader_args.c:363-371`, `:93-118`).
const GEOMETRY_INPUT_VGPRS: u32 = 9;

impl Wavefront<'_> {
    /// Marks word `index` of buffer `buffer` in the write mask (D809), where the store landed:
    /// inside the buffer, and in a running lane. The mask has a word for each of the buffer's and
    /// one more past them; a store that did not land marks that last word instead, so a plain
    /// store of one does it - no atomic, no read - and never clears or sets another word's mark.
    fn mark_written(
        &mut self,
        mask: buffer::DrawBufferArray,
        (buffer, index): (Id, Id),
        (inside, active): (Id, Option<Id>),
    ) {
        let u32_type = self.u32_type;
        let bool_type = self.bool_type;
        let (one, zero) = (Self::constant(self, 1), Self::constant(self, 0));
        let b = &mut self.builder;
        let landed = match active {
            None => inside,
            Some(active) => {
                let both = b.id();
                b.function(op::LOGICAL_AND, &[bool_type.0, both.0, inside.0, active.0]);
                both
            }
        };
        let block = b.id();
        b.function(
            op::ACCESS_CHAIN,
            &[mask.block_ptr.0, block.0, mask.variable.0, buffer.0],
        );
        let length = b.id();
        b.function(op::ARRAY_LENGTH, &[u32_type.0, length.0, block.0, 0]);
        let spare = b.id();
        b.function(op::ISUB, &[u32_type.0, spare.0, length.0, one.0]);
        let target = b.id();
        b.function(
            op::SELECT,
            &[u32_type.0, target.0, landed.0, index.0, spare.0],
        );
        let pointer = b.id();
        b.function(
            op::ACCESS_CHAIN,
            &[
                mask.element_ptr.0,
                pointer.0,
                mask.variable.0,
                buffer.0,
                zero.0,
                target.0,
            ],
        );
        b.function(op::STORE, &[pointer.0, one.0]);
    }

    /// The texture sources this module samples, in slot order.
    #[must_use]
    pub fn image_sources(&self) -> (Vec<TextureSource>, Option<TextureSource>) {
        (self.texture_sources(), self.images.stored_source)
    }

    /// The texture sources this module samples, in slot order.
    pub fn texture_sources(&self) -> Vec<TextureSource> {
        self.textures
            .iter()
            .map(|bound| {
                // An image held in user data has no table of its own: its source names its
                // sampler's, where the sampler was loaded from one.
                let table = bound
                    .source
                    .user_data
                    .and(bound.sampler)
                    .and_then(|sampler| self.sampler_table(sampler))
                    .unwrap_or(bound.source.table);
                (bound, table)
            })
            .map(|(bound, table)| TextureSource {
                table,
                sampler_offset: bound
                    .sampler
                    .and_then(|sampler| self.sampler_offset(sampler, table)),
                sampler_user_data: bound
                    .sampler
                    .and_then(|sampler| self.resident_sampler(sampler)),
                saturated: bound.texture.saturate,
                ..bound.source
            })
            .collect()
    }

    /// Settles each texture's sampler before an access whose sampler registers `reaching` filled:
    /// a sampler reloaded from where the first access's came from is the same sampler, and any
    /// other write into its registers makes the texture another one (D690).
    fn settle_samplers(&mut self, reaching: Option<(Option<TableBase>, u32)>) {
        for bound in &mut self.textures {
            if bound.sampler_disturbed
                && bound.sampler_load.is_some()
                && bound.sampler_load == reaching
            {
                bound.sampler_disturbed = false;
            }
            if bound.sampler_disturbed {
                bound.disturbed = true;
            }
        }
    }

    /// The one table a sampler descriptor starting at scalar register `sampler` was loaded from.
    fn sampler_table(&self, sampler: u32) -> Option<TableBase> {
        let loads = self.descriptor_loads.1.get(&sampler)?;
        match loads.iter().collect::<Vec<_>>().as_slice() {
            [(Some(from), _)] => Some(*from),
            _ => None,
        }
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
    /// Refused where the texture cannot be identified: a fifth texture (four bindings exist), a
    /// descriptor loaded from more than one offset, or a texture after the first when it or an
    /// earlier one did not come from the table. A pipeline finds a texture by its offset, so binding offset zero to both
    /// would draw the wrong image.
    /// Places `variable` at `binding` of descriptor set zero.
    fn decorate_binding(&mut self, variable: Id, binding: u32) {
        self.builder
            .annotate(op::DECORATE, &[variable.0, decoration::DESCRIPTOR_SET, 0]);
        self.builder
            .annotate(op::DECORATE, &[variable.0, decoration::BINDING, binding]);
    }

    fn new_texture_source(
        &self,
        descriptor: u32,
        (reaching, shape): (Reaching, model::ImageShape),
    ) -> Result<TextureSource, &'static str> {
        let slot = u32::try_from(self.textures.len()).unwrap_or(u32::MAX);
        if slot as usize >= MAX_TEXTURES {
            return Err(
                "this shader reads more than sixteen textures, and sixteen bindings exist (D690)",
            );
        }
        // Registers no load reached that still hold the user data they were seeded with read the
        // user data's texture, whatever a later load puts there: GTA V's intro video shader samples
        // its luma plane from `s[0:7]` and then loads a volume's descriptor into them.
        let seeded = (reaching.is_none() && matches!(self.stage, Stage::Compute | Stage::Fragment))
            .then(|| self.still_seeded(descriptor, model::IMAGE_DESCRIPTOR_REGISTERS))
            .flatten();
        let user_data = match seeded {
            Some(word) => Some(word),
            None => self.resident_descriptor(descriptor)?,
        };
        let (table, table_offset) = match self.descriptor_loads.0.get(&descriptor) {
            _ if seeded.is_some() => (TableBase::default(), None),
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
        let earlier_resolved = self.textures.iter().all(|earlier| {
            earlier.source.table_offset.is_some() || earlier.source.user_data.is_some()
        });
        if slot >= 1 && ((table_offset.is_none() && user_data.is_none()) || !earlier_resolved) {
            return Err(concat!(
                "this shader reads more than one texture and at least one descriptor did not come from ",
                "its descriptor table, so which is which cannot be told (D690)"
            ));
        }
        Ok(TextureSource {
            slot,
            table_offset,
            table,
            sampler_offset: None,
            saturated: [false; 2],
            user_data,
            sampler_user_data: None,
            cube: shape.cube,
            volume: shape.volume,
            compare: shape.compare,
            level_user_data: None,
        })
    }

    /// For a compute module, the user-data word a descriptor not loaded from a table starts at:
    /// its eight registers must lie in the seeded user data, unwritten by the program. A compute
    /// dispatch has no table at fixed user-data words to fall back on, so anything else is
    /// refused. `None` at any other stage, and for a descriptor a table load put there.
    fn resident_descriptor(&self, descriptor: u32) -> Result<Option<u32>, &'static str> {
        let resident_stage = matches!(self.stage, Stage::Compute | Stage::Fragment);
        if !resident_stage || self.descriptor_loads.0.contains_key(&descriptor) {
            return Ok(None);
        }
        let seeded = self.still_seeded(descriptor, model::IMAGE_DESCRIPTOR_REGISTERS);
        if let Some(word) = seeded {
            Ok(Some(word))
        } else if self.stage == Stage::Fragment {
            // A pixel shader's descriptor from neither is read where a pipeline finds an untraced
            // one: offset zero of the table its first two user-data words name.
            Ok(None)
        } else {
            Err(concat!(
                "a compute program reads an image descriptor that is neither loaded from a table ",
                "nor still in the user data it was seeded with, so which image it names is not traced"
            ))
        }
    }
}

impl Wavefront<'_> {
    /// The user-data word `span` registers from `first` start at, where the user data seeded them
    /// and the program has written none of them.
    fn still_seeded(&self, first: u32, span: u32) -> Option<u32> {
        let (seeded_from, count) = self.images.user_data_registers;
        let seeded = first >= seeded_from && first + span <= seeded_from + count;
        let untouched = (first..first + span).all(|register| {
            register >= u128::BITS || self.images.written_scalars >> register & 1 == 0
        });
        if seeded && untouched {
            return Some(first - seeded_from);
        }
        // Or copied there, in order, from consecutive user-data words: `s_mov_b64 s[20:21], s[6:7]`
        // and its siblings move a pixel shader's image descriptor before it samples.
        let word = |register: u32| {
            let seeded_here = register >= seeded_from
                && register < seeded_from + count
                && (register >= u128::BITS || self.images.written_scalars >> register & 1 == 0);
            if seeded_here {
                Some(register - seeded_from)
            } else {
                self.scalar_copies.get(&register).copied()
            }
        };
        let start = word(first)?;
        (1..span)
            .all(|step| word(first + step) == Some(start + step))
            .then_some(start)
    }

    /// Where a sampler descriptor starting at scalar register `sampler` lies in the user data, for
    /// a stage that reads descriptors there, as [`Self::resident_descriptor`] finds an image's:
    /// never loaded, never written, within the words the user data seeded.
    fn resident_sampler(&self, sampler: u32) -> Option<u32> {
        if !matches!(self.stage, Stage::Compute | Stage::Fragment)
            || self.descriptor_loads.1.contains_key(&sampler)
        {
            return None;
        }
        self.still_seeded(sampler, 4)
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
    (modelled, local_words): (u32, Option<u32>),
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
        (u32_type, local_share_words(local_words)),
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
        modelled,
        window.words(),
    );

    let files = declare_files(b, u32_type, ids.register_count, ids.wave_count);
    let (observation, guest_memory) =
        declare_buffers(b, u32_type, ids.observed_count, ids.memory_count);
    (files, observation, guest_memory)
}

/// The variables a 1.4 mesh module's entry point names beyond its inputs and outputs.
fn mesh_interface(
    (ids, files): (&Reserved, &Files),
    (observation, guest_memory): (&buffer::StorageBuffer, &buffer::StorageBuffer),
    (user_data_source, draw_buffers, lane): (
        Option<UserDataSource>,
        Option<&DrawBuffersDeclared>,
        Option<PerLane>,
    ),
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
    interface.extend(draw_buffers.map(|(array, _)| array.variable.0));
    interface.extend(lane.iter().flat_map(PerLane::interface));
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
        Stage::Mesh => (orbistoun_spirv::GEOMETRY_BUFFERS_BINDING, false),
        Stage::Fragment => (orbistoun_spirv::PIXEL_BUFFERS_BINDING, false),
        // A dispatch stores through its buffers, which are written back (D746).
        Stage::Compute => (orbistoun_spirv::COMPUTE_BUFFERS_BINDING, true),
    };
    Some((
        buffer::declare_draw_buffers(b, u32_type, binding),
        buffers.served.clone(),
    ))
}

/// Declares the block a draw's user data arrives in, when the stage reads any.
fn declare_user_data_source(
    b: &mut Builder,
    (stage, reads_buffers): (Stage, bool),
    u32_type: Id,
    user_data: &UserData,
) -> Option<UserDataSource> {
    // Declared only when this stage reads user data, so a module that reads none is unchanged.
    //
    // A mesh module reads its words per draw (D718): one host dispatch carries a run of guest
    // draws, one workgroup each, so the words come from the draw-data buffer at the workgroup's
    // stride. One seeded with its geometry reads its draw's vertex count there too (D745), user
    // data or none.
    // One that reads buffers finds them there too (D747).
    let seeded = stage == Stage::Mesh && (user_data.geometry.is_some() || reads_buffers);
    // A dispatch with partial groups reads their shape from the block, user data or none (D855).
    let partial = stage == Stage::Compute
        && user_data
            .compute
            .is_some_and(|compute| compute.partial.is_some());
    (user_data.count > 0 || seeded || partial).then(|| {
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

/// What a module is prepared against beside its stage: the guest-memory window, the user data,
/// the draw buffers it reads, whether it exports depth ([`exports_depth`]), whether it reads
/// its quad's other lanes ([`reads_quad`]), the colour targets it exports to, whether it
/// exports a layer ([`exports_layer`]), and how a guest dispatch run per lane holds its masks
/// ([`crate::lane_exact::lane_masks`]).
type StageContext<'b> = (
    Window,
    UserData,
    &'b crate::draw_buffers::DrawBuffers,
    ((bool, bool), bool, u8, (bool, bool)),
    Option<LaneMasks>,
);

impl Wavefront<'_> {
    /// A pointer to the global data share's word at `byte_address`, declaring the share at
    /// [`orbistoun_spirv::GDS_BINDING`] on first use (D789). A compute module only: the other
    /// stages' entry points would have to name it, and none reaches it yet.
    fn global_share_word(
        &mut self,
        instruction: &Instruction,
        byte_address: Id,
    ) -> Result<Id, TranslateError> {
        if self.stage != Stage::Compute {
            return Err(TranslateError::Unsupported {
                offset: instruction.offset,
                detail: "a global data share (GDS) access outside a compute program",
            });
        }
        let u32_type = self.u32_type;
        let share = if let Some(share) = self.global_share {
            share
        } else {
            let words = self.builder.id();
            self.builder.declare(
                op::CONSTANT,
                &[u32_type.0, words.0, orbistoun_spirv::GDS_WORDS],
            );
            let share = buffer::declare(
                &mut self.builder,
                u32_type,
                words,
                orbistoun_spirv::GDS_BINDING,
            );
            self.global_share = Some(share);
            share
        };
        let two = Self::constant(self, 2);
        let mask = Self::constant(self, orbistoun_spirv::GDS_WORDS - 1);
        let zero = Self::constant(self, 0);
        let b = &mut self.builder;
        let shifted = b.id();
        b.function(
            op::SHIFT_RIGHT_LOGICAL,
            &[u32_type.0, shifted.0, byte_address.0, two.0],
        );
        let index = b.id();
        b.function(op::BITWISE_AND, &[u32_type.0, index.0, shifted.0, mask.0]);
        let pointer = b.id();
        b.function(
            op::ACCESS_CHAIN,
            &[
                share.element_ptr.0,
                pointer.0,
                share.buffer.0,
                zero.0,
                index.0,
            ],
        );
        Ok(pointer)
    }
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
                ((false, false), false, 0, (false, false)),
                None,
            ),
        )
    }

    /// Prepares the module for a named stage.
    #[allow(
        clippy::too_many_lines,
        reason = "one module's declarations in the order SPIR-V requires them"
    )]
    pub fn for_stage(
        encodings: &'a EncodingTable,
        width: Width,
        stage: Stage,
        primitive: MeshPrimitive,
        attributes: &[(u32, Interpolation)],
        parameters: &[u32],
        (
            window,
            user_data,
            buffers,
            ((writes_depth, sample_mask_exported), reads_quad, colour_targets, layered),
            masks,
        ): StageContext<'_>,
    ) -> Self {
        let mut b = Builder::new().with_version(module_version(stage));

        let ids = Reserved::new(&mut b);
        let (void, fn_type, main) = (ids.void, ids.fn_type, ids.main);
        let (u32_type, f32_type, bool_type) = (ids.u32_type, ids.f32_type, ids.bool_type);

        // The colour and depth outputs of a fragment module, reserved before the entry point
        // because its interface must name them.
        let (output, depth) = reserve_fragment_outputs(&mut b, stage, writes_depth);
        let sample_mask = (stage == Stage::Fragment && sample_mask_exported).then(|| b.id());
        let more_colour = reserve_more_colour_outputs(&mut b, stage, colour_targets);
        let vec4 = b.id();
        let output_ptr = b.id();
        // Reserved before the header for the same reason: every input the entry point touches must
        // be named in its interface, and a driver does not reliably reject a module that omits one.
        let input_ids =
            reserve_attribute_inputs(&mut b, stage, attributes, user_data.input_controls);
        let mut system = pixel_inputs::SystemInputs::reserve(&mut b, stage, user_data.pixel_inputs);
        let clip_mask = model::exported_clip_distances(user_data.position_vectors);
        let mesh_reserved = MeshReserved::new(&mut b, stage, parameters, (layered, clip_mask));
        let layered = layered.0;
        let (per_lane, modelled) = lanes_per_invocation(stage, width, &user_data, masks);
        let lane_masks = per_lane.and(masks).filter(|_| stage == Stage::Compute);
        let ballots = lane_masks == Some(LaneMasks::Ballot);
        // A guest dispatch's group runs as one invocation per wave (D858).
        let waves = user_data
            .compute
            .map_or(1, |inputs| inputs.waves(width.lanes()));
        emit_header(
            &mut b,
            (stage, primitive, per_lane, waves),
            main,
            (output, depth),
        );
        declare_base_types(&mut b, &ids);
        let output = FragmentOutputs::declare(
            &mut b,
            (f32_type, u32_type, vec4),
            output_ptr,
            (output, depth, stage, user_data.integer_targets),
        )
        .with_more_colour(&mut b, output_ptr, more_colour)
        .with_sample_mask(&mut b, u32_type, sample_mask);
        let inputs = declare_attribute_inputs(&mut b, (f32_type, vec4), &input_ids);
        system.declare(&mut b, vec4, bool_type);
        let mut mesh = declare_mesh_outputs(
            &mut b,
            (f32_type, u32_type, vec4),
            (stage, primitive),
            &mesh_reserved,
        );
        if let Some(mesh) = mesh.as_mut() {
            mesh.layers =
                declare_mesh_layers(&mut b, (u32_type, bool_type), primitive, &mesh_reserved);
        }
        let (files, observation, guest_memory) =
            declare_state(&mut b, &ids, (modelled, user_data.local_words), window);
        let lane_ids = per_lane.map(|shape| {
            PerLane::declare(
                &mut b,
                (u32_type, ids.local_ptr),
                shape,
                (stage == Stage::Compute && !ballots, ballots && waves > 1),
            )
        });
        // A guest dispatch whose waves are subgroups makes its masks by their ballots (D884).
        if ballots {
            for needed in [
                capability::GROUP_NON_UNIFORM,
                capability::GROUP_NON_UNIFORM_BALLOT,
            ] {
                b.header(op::CAPABILITY, &[needed]);
            }
        }
        // A fragment module that reads its quad's other lanes reads them as the host's quad
        // neighbours, and finds its own place in the quad from its subgroup index.
        let quad_input = (stage == Stage::Fragment && reads_quad).then(|| {
            for needed in [
                capability::GROUP_NON_UNIFORM,
                capability::GROUP_NON_UNIFORM_QUAD,
            ] {
                b.header(op::CAPABILITY, &[needed]);
            }
            declare_lane_input(&mut b, u32_type, false)
        });
        let staged = mesh_reserved.declare_staged(&mut b, mesh.as_ref(), vec4, &ids);
        let indexed = user_data.geometry.is_some_and(|g| g.indices.is_some());
        let reads_buffers = !buffers.served.is_empty() || indexed;
        let user_data_source =
            // A layered module reads the layer its draw's pass renders from the draw's words (D859).
            declare_user_data_source(&mut b, (stage, reads_buffers || layered), u32_type, &user_data);
        let dispatch = compute_inputs::DispatchState::for_stage(
            &mut b,
            (&ids, width.lanes()),
            stage,
            &user_data,
        );
        let draw_buffers = declare_draw_buffers(&mut b, u32_type, (stage, indexed), buffers);
        let write_mask = (stage == Stage::Compute && draw_buffers.is_some()).then(|| {
            buffer::declare_draw_buffers(
                &mut b,
                u32_type,
                (orbistoun_spirv::COMPUTE_WRITTEN_BINDING, true),
            )
        });

        // Every variable this module has. From 1.4 the entry point names all of them; below that
        // only the inputs and outputs.
        let mut interface: Vec<u32> = input_ids
            .iter()
            .filter(|input| input.is_input())
            .map(|input| input.variable.0)
            .collect();
        interface.extend(system.interface());
        interface.extend(output.interface());
        interface.extend(mesh_reserved.interface(stage));
        interface.extend(
            dispatch
                .into_iter()
                .flat_map(compute_inputs::DispatchState::interface),
        );
        interface.extend(quad_input.map(|input| input.0));
        // A guest dispatch run one invocation per lane reads its lane (D880), and its subgroup
        // where its waves are subgroups (D884). Its exchange is workgroup memory, which a module
        // below SPIR-V 1.4 does not name.
        if stage == Stage::Compute {
            interface.extend(lane_ids.iter().flat_map(PerLane::inputs));
        }
        // The block the user clip planes arrive in (D857).
        interface.extend(mesh.as_ref().map(|mesh| mesh.plane_block.0.0));
        if stage == Stage::Mesh {
            interface.extend(mesh_interface(
                (&ids, &files),
                (&observation, &guest_memory),
                (user_data_source, draw_buffers.as_ref(), lane_ids),
            ));
        }
        emit_entry_point(&mut b, stage, main, &interface);

        b.function(op::FUNCTION, &[void.0, main.0, 0, fn_type.0]);
        b.function(op::LABEL, &[ids.entry_block.0]);
        let (invocation, wave_base) = lane_and_wave(
            &mut b,
            u32_type,
            lane_ids,
            (stage == Stage::Compute && waves > 1).then(|| width.lanes()),
        );
        let mut this = Self {
            invocation,
            wave_base,
            lane_masks,
            quad_invocation: quad_input.map(|input| {
                let loaded = b.id();
                b.function(op::LOAD, &[u32_type.0, loaded.0, input.0]);
                loaded
            }),
            stage,
            output,
            inputs,
            flat_twins: user_data.flat_twins,
            streamout: if user_data.discard_vertex_exports {
                StreamoutRole::Pass
            } else if user_data
                .geometry
                .is_some_and(|geometry| geometry.legacy && geometry.streamout)
            {
                StreamoutRole::Draw
            } else {
                StreamoutRole::None
            },
            capture_positions: user_data.capture_positions,
            position_vectors: user_data.position_vectors,
            point_size: user_data.point_size,
            integer_images: user_data.compute.unwrap_or_default().integer_images,
            saturated: user_data.saturated,
            uvec4: None,
            mesh,
            staged,
            window_space: user_data.window_space,
            buffer_formats: user_data.buffer_formats,
            primitive,
            vec4,
            memory_base: window.base,
            dx10_clamp: user_data.dx10_clamp,
            program_address: user_data.program_address,
            builder: b,
            encodings,
            memory_words: window.words(),
            local_words: local_share_words(user_data.local_words),
            lanes: per_lane.map_or_else(|| simulated_lanes(stage, width), |(lanes, _)| lanes),
            modelled,
            exchange: lane_ids.and_then(|lane| lane.exchange),
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
            group_waves: if stage == Stage::Compute { waves } else { 1 },
            condition_code: ids.scc,
            m0: ids.m0,
            translated: 0,
            dispatch,
            memory_high: u32::try_from(window.address() >> 32).unwrap_or(u32::MAX),
            unwritten_user_data: user_data.compute.unwrap_or_default().unwritten_user_data,
            reads_unwritten: false,
            written: (0, [0; 4]),
            reads_geometry_input: false,
            instance_id: None,
            draw_data: None,
            push_block: None,
            draw_buffers,
            write_mask,
            unread_slots: buffers.stored_only.clone(),
            untraced_by: buffers.untraced_by.clone(),
            global_share: None,
            scalar_copies: BTreeMap::new(),
            vector_copies: BTreeMap::new(),
        };

        this.seed_entry(user_data_source, &user_data, &system);
        this
    }

    /// Places what the hardware loads before a wave's first instruction.
    fn seed_entry(
        &mut self,
        user_data_source: Option<UserDataSource>,
        user_data: &UserData,
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
        // The draw's user clip planes, from the block a mesh module reads them in (D857).
        if let Some(block) = self.mesh.as_ref().map(|mesh| mesh.plane_block) {
            let planes = self.load_clip_planes(block);
            if let Some(mesh) = self.mesh.as_mut() {
                mesh.planes = planes;
            }
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
        // A vertex that exports no layer is in layer zero (D859).
        if let Some(layers) = self.mesh.as_ref().and_then(|mesh| mesh.layers) {
            let zero = self.constant(0);
            for lane in 0..self.modelled.min(MESH_SLOTS) {
                let slot = self.lane_slot(lane);
                let pointer = self.workgroup_word(layers.words, slot);
                self.builder.function(op::STORE, &[pointer.0, zero.0]);
            }
        }
        // A distance a vertex does not export clips nothing (D867).
        if let Some(words) = self.mesh.as_ref().and_then(|mesh| mesh.distances) {
            let zero = self.constant(0);
            for lane in 0..self.modelled.min(MESH_SLOTS) {
                let slot = self.lane_slot(lane);
                for index in 0..CLIP_DISTANCES {
                    let at = self.distance_word(slot, index);
                    let pointer = self.workgroup_word(words, at);
                    self.builder.function(op::STORE, &[pointer.0, zero.0]);
                }
            }
        }
        // A vertex that exports no point size is drawn at the draw's, `PA_SU_POINT_SIZE`'s height
        // in eighths of a pixel as radeonsi writes it (`si_state.c` 945), or at one (D867).
        if let Some(words) = self.mesh.as_ref().and_then(|mesh| mesh.point_sizes) {
            let bits = self.point_size.map_or(ONE_BITS, |register| {
                let height = u16::try_from(register & 0xFFFF).unwrap_or(0);
                (f32::from(height) / 8.0).to_bits()
            });
            let one = self.constant(bits);
            for lane in 0..self.modelled.min(MESH_SLOTS) {
                let slot = self.lane_slot(lane);
                let pointer = self.workgroup_word(words, slot);
                self.builder.function(op::STORE, &[pointer.0, one.0]);
            }
        }
    }

    /// Seeds the geometry-engine inputs [`GeometryInputs`] describes, every input register in
    /// every lane: a lane that is no vertex or primitive thread reads zero. The counts are the
    /// draw's, read from its words at run time (D745): its vertex count, and the primitive count
    /// the assembly makes of it.
    fn seed_geometry(&mut self, geometry: GeometryInputs) {
        if geometry.legacy {
            self.seed_legacy_vertices(geometry);
            return;
        }
        let GeometryInputs {
            first_vertex,
            vertices: whole,
            assembly,
            indices,
            passthrough,
            ..
        } = geometry;
        // An indexed draw's index buffer is bound after the buffers the program reads through.
        let index_slot = self.draw_buffers.as_ref().map_or(0, |(_, served)| {
            served.values().max().map_or(0, |&slot| slot + 1)
        });
        let vertices = self
            .draw_word(orbistoun_spirv::DRAW_DATA_VERTICES_WORD)
            .unwrap_or_else(|| self.constant(whole));
        let primitives = self.primitives_of(assembly, vertices);
        // Where a draw's vertex ids count up from, from its words where it has them (D761).
        let first_id = self
            .draw_word(orbistoun_spirv::DRAW_DATA_FIRST_VERTEX_WORD)
            .map(|word| {
                let base = self.constant(first_vertex);
                self.binary(op::IADD, word, base)
            });
        let (twelve, twenty_two, eight) = (self.constant(12), self.constant(22), self.constant(8));
        let shifted_vertices = self.binary(op::SHIFT_LEFT_LOGICAL, vertices, twelve);
        let shifted_primitives = self.binary(op::SHIFT_LEFT_LOGICAL, primitives, twenty_two);
        let tg_info = self.binary(op::BITWISE_OR, shifted_vertices, shifted_primitives);
        self.store_scalar(2, tg_info);
        let primitives_at_eight = self.binary(op::SHIFT_LEFT_LOGICAL, primitives, eight);
        let one_wave = self.constant(1 << 28);
        let counts = self.binary(op::BITWISE_OR, vertices, primitives_at_eight);
        let wave_info = self.binary(op::BITWISE_OR, counts, one_wave);
        self.store_scalar(3, wave_info);
        let zero = self.constant(0);
        // One invocation per lane (D760): the same inputs, for the lane this invocation is.
        if let Some(at) = self.invocation {
            self.seed_invocation_geometry(
                at,
                geometry,
                (vertices, primitives, first_id),
                index_slot,
            );
            return;
        }
        for lane in 0..self.lanes {
            let at = self.constant(lane);
            let is_primitive = self.compare(op::ULESS_THAN, at, primitives);
            let is_vertex = self.compare(op::ULESS_THAN, at, vertices);
            let [first, second, third] = assembly.vertices_of(lane);
            // v0: the first two vertex indices; v1: the third; v2: the primitive id; v3: the
            // invocation id; v4: the fifth and sixth indices; v5: the vertex id, unless an index
            // buffer gives it; v6, v7: user VGPRs; v8: the instance id.
            let packed = if passthrough {
                first | (second << 10) | (third << 20)
            } else {
                first | (second << 16)
            };
            let words: [(Option<Id>, u32); 9] = [
                (Some(is_primitive), packed),
                (Some(is_primitive), third),
                (Some(is_primitive), lane),
                (None, 0),
                (None, 0),
                (indices.is_none().then_some(is_vertex), first_vertex + lane),
                (None, 0),
                (None, 0),
                (None, 0),
            ];
            for (register, (when, word)) in (0..).zip(words) {
                let value = match when {
                    Some(condition) => {
                        let word = self.constant(word);
                        self.select(condition, word, zero)
                    }
                    None => zero,
                };
                self.store_lane_masked(register, lane, value);
            }
            if let (None, Some(first)) = (indices, first_id) {
                let at = self.constant(lane);
                let id = self.binary(op::IADD, first, at);
                let id = self.select(is_vertex, id, zero);
                self.store_lane_masked(VERTEX_ID_REGISTER, lane, id);
            }
            if let Some(instance) = self.instance_id {
                let id = self.select(is_vertex, instance, zero);
                self.store_lane_masked(INSTANCE_ID_REGISTER, lane, id);
            }
            if let Some(width) = indices {
                // A lane past the draw's vertices reads the first index and keeps zero.
                let position = self.select(is_vertex, at, zero);
                let id = self.index_at(index_slot, width, position);
                let id = self.offset_index(id, first_vertex);
                let id = self.select(is_vertex, id, zero);
                self.store_lane_masked(VERTEX_ID_REGISTER, lane, id);
            }
        }
    }

    /// A legacy vertex shader's draw (D881): the counts declared from the draw - its vertices, and
    /// the primitives its assembly makes of them - each primitive's vertices written as an
    /// `exp prim` would, and each vertex lane's id in `v0` (its index, for an indexed draw) and
    /// instance in `v3`. Lanes past the draw's vertices read zero.
    fn seed_legacy_vertices(&mut self, geometry: GeometryInputs) {
        let GeometryInputs {
            first_vertex,
            vertices: whole,
            assembly,
            indices,
            ..
        } = geometry;
        let index_slot = self.draw_buffers.as_ref().map_or(0, |(_, served)| {
            served.values().max().map_or(0, |&slot| slot + 1)
        });
        let vertices = self
            .draw_word(orbistoun_spirv::DRAW_DATA_VERTICES_WORD)
            .unwrap_or_else(|| self.constant(whole));
        let primitives = self.primitives_of(assembly, vertices);
        let _ = self.set_mesh_outputs(vertices, primitives);
        let first = self.constant(first_vertex);
        let base = match self.draw_word(orbistoun_spirv::DRAW_DATA_FIRST_VERTEX_WORD) {
            Some(word) => self.binary(op::IADD, word, first),
            None => first,
        };
        let zero = self.constant(0);
        let width = self.primitive.indices() as usize;
        let lanes: Vec<(u32, Id)> = match self.invocation {
            // One invocation per lane (D760): this invocation's own lane, in its slot zero.
            Some(at) => vec![(0, at)],
            None => (0..self.lanes)
                .map(|lane| (lane, self.constant(lane)))
                .collect(),
        };
        for (lane, at) in lanes {
            let corners = self.vertices_of_lane(assembly, at);
            let _ = self.write_mesh_indices(lane, &corners[..width]);
            let is_vertex = self.compare(op::ULESS_THAN, at, vertices);
            let id = match indices {
                Some(width) => {
                    let position = self.select(is_vertex, at, zero);
                    let id = self.index_at(index_slot, width, position);
                    self.offset_index(id, first_vertex)
                }
                None => self.binary(op::IADD, base, at),
            };
            let id = self.select(is_vertex, id, zero);
            self.store_lane_masked(LEGACY_VERTEX_ID_REGISTER, lane, id);
            let instance = self.instance_id.unwrap_or(zero);
            let instance = self.select(is_vertex, instance, zero);
            self.store_lane_masked(LEGACY_INSTANCE_ID_REGISTER, lane, instance);
        }
    }

    /// [`Self::seed_geometry`]'s per-lane inputs for the one lane this invocation is (D760), its
    /// index `at` a run-time value: the same registers, words and conditions.
    fn seed_invocation_geometry(
        &mut self,
        at: Id,
        geometry: GeometryInputs,
        (vertices, primitives, first_id): (Id, Id, Option<Id>),
        index_slot: u32,
    ) {
        let GeometryInputs {
            first_vertex,
            assembly,
            indices,
            passthrough,
            ..
        } = geometry;
        let zero = self.constant(0);
        let is_primitive = self.compare(op::ULESS_THAN, at, primitives);
        let is_vertex = self.compare(op::ULESS_THAN, at, vertices);
        let [first, second, third] = self.vertices_of_lane(assembly, at);
        let (ten, sixteen, twenty) = (self.constant(10), self.constant(16), self.constant(20));
        let packed = if passthrough {
            let second = self.binary(op::SHIFT_LEFT_LOGICAL, second, ten);
            let third = self.binary(op::SHIFT_LEFT_LOGICAL, third, twenty);
            let both = self.binary(op::BITWISE_OR, first, second);
            self.binary(op::BITWISE_OR, both, third)
        } else {
            let second = self.binary(op::SHIFT_LEFT_LOGICAL, second, sixteen);
            self.binary(op::BITWISE_OR, first, second)
        };
        let base = first_id.unwrap_or_else(|| self.constant(first_vertex));
        let vertex_id = self.binary(op::IADD, base, at);
        let words: [(Option<Id>, Id); 9] = [
            (Some(is_primitive), packed),
            (Some(is_primitive), third),
            (Some(is_primitive), at),
            (None, zero),
            (None, zero),
            (indices.is_none().then_some(is_vertex), vertex_id),
            (None, zero),
            (None, zero),
            (None, zero),
        ];
        for (register, (when, word)) in (0..).zip(words) {
            let value = match when {
                Some(condition) => self.select(condition, word, zero),
                None => zero,
            };
            self.store_lane_masked(register, 0, value);
        }
        // The instance, a workgroup per instance (D863).
        if let Some(instance) = self.instance_id {
            let id = self.select(is_vertex, instance, zero);
            self.store_lane_masked(INSTANCE_ID_REGISTER, 0, id);
        }
        if let Some(width) = indices {
            let position = self.select(is_vertex, at, zero);
            let id = self.index_at(index_slot, width, position);
            let id = self.offset_index(id, first_vertex);
            let id = self.select(is_vertex, id, zero);
            self.store_lane_masked(VERTEX_ID_REGISTER, 0, id);
        }
    }

    /// A fetched index with the draw's index offset added (D843): `GE_INDX_OFFSET` moves each
    /// index an indexed draw fetches.
    fn offset_index(&mut self, index: Id, offset: u32) -> Id {
        if offset == 0 {
            return index;
        }
        let offset = self.constant(offset);
        self.binary(op::IADD, index, offset)
    }

    /// [`Assembly::vertices_of`] for a primitive `at` known only at run time (D760).
    fn vertices_of_lane(&mut self, assembly: Assembly, at: Id) -> [Id; 3] {
        let (zero, one, two, three) = (
            self.constant(0),
            self.constant(1),
            self.constant(2),
            self.constant(3),
        );
        let next = self.binary(op::IADD, at, one);
        let after = self.binary(op::IADD, at, two);
        match assembly {
            Assembly::List => {
                let start = self.binary(op::IMUL, at, three);
                let second = self.binary(op::IADD, start, one);
                let third = self.binary(op::IADD, start, two);
                [start, second, third]
            }
            Assembly::LineList => {
                let start = self.binary(op::IMUL, at, two);
                let second = self.binary(op::IADD, start, one);
                [start, second, zero]
            }
            Assembly::LineStrip => [at, next, zero],
            Assembly::Points => [at, zero, zero],
            // A quad's two triangles, as `vertices_of` splits them (D798).
            Assembly::Quads { provoking_last } => {
                let quad = self.binary(op::SHIFT_RIGHT_LOGICAL, at, one);
                let four = self.constant(4);
                let base = self.binary(op::IMUL, quad, four);
                let parity = self.binary(op::BITWISE_AND, at, one);
                let odd = self.compare(op::INOT_EQUAL, parity, zero);
                let plus = |model: &mut Self, by: Id| model.binary(op::IADD, base, by);
                let (b1, b2, b3) = (plus(self, one), plus(self, two), plus(self, three));
                if provoking_last {
                    let first = self.select(odd, b1, base);
                    let second = self.select(odd, b2, b1);
                    [first, second, b3]
                } else {
                    let second = self.select(odd, b2, b1);
                    let third = self.select(odd, b3, b2);
                    [base, second, third]
                }
            }
            // An odd triangle of a strip swaps two of its vertices, as `vertices_of` does.
            Assembly::Strip { provoking_last } => {
                let parity = self.binary(op::BITWISE_AND, at, one);
                let odd = self.compare(op::INOT_EQUAL, parity, zero);
                if provoking_last {
                    let first = self.select(odd, next, at);
                    let second = self.select(odd, at, next);
                    [first, second, after]
                } else {
                    let second = self.select(odd, after, next);
                    let third = self.select(odd, next, after);
                    [at, second, third]
                }
            }
        }
    }

    /// Where a primitive shader's buffer at `slot` lies, from its draw's words (D747): the arena, its
    /// first word there, and how many words it holds. `None` without draw data.
    fn geometry_buffer_place(&mut self, slot: u32) -> Option<[Id; 3]> {
        let first = orbistoun_spirv::DRAW_DATA_BUFFERS_WORD + slot * 3;
        Some([
            self.draw_word(first)?,
            self.draw_word(first + 1)?,
            self.draw_word(first + 2)?,
        ])
    }

    /// Word `index` of the range `[arena, first, words]` of the geometry binding's arenas: past its
    /// words the range's first instead, defined and never the answer, as a bound buffer reads.
    fn read_arena(
        &mut self,
        array: buffer::DrawBufferArray,
        [arena, first, words]: [Id; 3],
        index: Id,
    ) -> Id {
        let u32_type = self.u32_type;
        let bool_type = self.bool_type;
        let zero = Self::constant(self, 0);
        let b = &mut self.builder;
        let inside = b.id();
        b.function(op::ULESS_THAN, &[bool_type.0, inside.0, index.0, words.0]);
        let within = b.id();
        b.function(
            op::SELECT,
            &[u32_type.0, within.0, inside.0, index.0, zero.0],
        );
        let at = b.id();
        b.function(op::IADD, &[u32_type.0, at.0, first.0, within.0]);
        let pointer = b.id();
        b.function(
            op::ACCESS_CHAIN,
            &[
                array.element_ptr.0,
                pointer.0,
                array.variable.0,
                arena.0,
                zero.0,
                at.0,
            ],
        );
        let value = b.id();
        b.function(op::LOAD, &[u32_type.0, value.0, pointer.0]);
        value
    }

    /// Word `word` of this workgroup's stride of the draw-data buffer (D718), or `None` for a module
    /// that reads none.
    fn draw_word(&mut self, word: u32) -> Option<Id> {
        let (block, first) = self.draw_data?;
        let u32_type = self.u32_type;
        let within = self.constant(word);
        let index = self.builder.id();
        self.builder
            .function(op::IADD, &[u32_type.0, index.0, first.0, within.0]);
        let member = self.constant(0);
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
        Some(value)
    }

    /// The primitive count `assembly` makes of `vertices`, at run time: a list's whole triangles
    /// or lines, a strip's vertices past its first two or one.
    fn primitives_of(&mut self, assembly: Assembly, vertices: Id) -> Id {
        let saturating_less = |model: &mut Self, by: u32| {
            let by = model.constant(by);
            let short = model.compare(op::ULESS_THAN, vertices, by);
            let less = model.binary(op::ISUB, vertices, by);
            let zero = model.constant(0);
            model.select(short, zero, less)
        };
        match assembly {
            Assembly::List => {
                let three = self.constant(3);
                self.binary(op::UDIV, vertices, three)
            }
            Assembly::LineList => {
                let two = self.constant(2);
                self.binary(op::UDIV, vertices, two)
            }
            Assembly::Strip { .. } => saturating_less(self, 2),
            Assembly::LineStrip => saturating_less(self, 1),
            Assembly::Points => vertices,
            // Whole quads, two triangles each.
            Assembly::Quads { .. } => {
                let two = self.constant(2);
                let quads = self.binary(op::SHIFT_RIGHT_LOGICAL, vertices, two);
                let one = self.constant(1);
                self.binary(op::SHIFT_LEFT_LOGICAL, quads, one)
            }
        }
    }

    /// The index at `position` of the draw's index buffer at `slot`, `width` wide.
    ///
    /// Whole words of the bound buffer, so an index's word is read and its half taken.
    fn index_at(&mut self, slot: u32, width: IndexWidth, position: Id) -> Id {
        let (word_index, shift) = match width {
            IndexWidth::Bits32 => (position, None),
            IndexWidth::Bits16 => {
                let one = self.constant(1);
                let word = self.binary(op::SHIFT_RIGHT_LOGICAL, position, one);
                let odd = self.binary(op::BITWISE_AND, position, one);
                let four = self.constant(4);
                (word, Some(self.binary(op::SHIFT_LEFT_LOGICAL, odd, four)))
            }
        };
        // A module declares the draw buffers it reads, and an indexed one always declares them.
        let Ok(word) = self.read_draw_buffer(slot, word_index) else {
            return self.constant(0);
        };
        match shift {
            None => word,
            Some(shift) => {
                let shifted = self.binary(op::SHIFT_RIGHT_LOGICAL, word, shift);
                let low = self.constant(0xFFFF);
                self.binary(op::BITWISE_AND, shifted, low)
            }
        }
    }

    /// The user clip planes, loaded from the push-constant block `(variable, element pointer)`:
    /// word `4 * plane + axis` (D857).
    fn load_clip_planes(&mut self, (block, element_ptr): (Id, Id)) -> [[Id; 4]; CLIP_PLANES] {
        let member = self.constant(0);
        let mut planes = [[Id(0); 4]; CLIP_PLANES];
        for (index, value) in (0u32..).zip(planes.iter_mut().flatten()) {
            let at = self.constant(index);
            let pointer = self.builder.id();
            self.builder.function(
                op::ACCESS_CHAIN,
                &[element_ptr.0, pointer.0, block.0, member.0, at.0],
            );
            *value = self.builder.id();
            self.builder
                .function(op::LOAD, &[self.f32_type.0, value.0, pointer.0]);
        }
        planes
    }

    /// Loads each user-data word from `source` into its scalar register, at entry.
    fn load_user_data(&mut self, source: UserDataSource, user_data: &UserData) {
        let u32_type = self.u32_type;
        let member = self.constant(0);
        // Where this stage's words begin: its share of the push-constant block, or this workgroup's
        // stride of the draw-data buffer (D718).
        let (block, first_word) = match source {
            UserDataSource::Push(block) => {
                self.push_block = Some(block);
                (block, None)
            }
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
                self.draw_data = Some((buffer, first));
                // A workgroup per instance (D863): its `y` is the instance.
                let y = self.builder.id();
                self.builder
                    .function(op::COMPOSITE_EXTRACT, &[u32_type.0, y.0, id.0, 1]);
                self.instance_id = Some(y);
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

    /// Word `index` of the push-constant block, loaded where it is read; `None` for a module that
    /// declares no block.
    fn push_word(&mut self, index: u32) -> Option<Id> {
        let block = self.push_block?;
        let (u32_type, member, at) = (self.u32_type, self.constant(0), self.constant(index));
        let pointer = self.builder.id();
        self.builder.function(
            op::ACCESS_CHAIN,
            &[
                block.element_ptr.0,
                pointer.0,
                block.buffer.0,
                member.0,
                at.0,
            ],
        );
        let value = self.builder.id();
        self.builder
            .function(op::LOAD, &[u32_type.0, value.0, pointer.0]);
        Some(value)
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
        // One invocation per lane (D760): which lane this is is a run-time value, so a lane is
        // known only where the wave's whole mask is.
        if self.invocation.is_some() {
            let full = u64::MAX >> (64 - self.lanes);
            let mask = u64::from(self.known_exec[0]?)
                | (if self.lanes > 32 {
                    u64::from(self.known_exec[1]?)
                } else {
                    0
                } << 32);
            return match mask & full {
                0 => Some(false),
                bits if bits == full => Some(true),
                _ => None,
            };
        }
        let (half, bit) = if lane < 32 { (0, lane) } else { (1, lane - 32) };
        self.known_exec[half].map(|mask| mask >> bit & 1 != 0)
    }

    /// Where `lane`'s outputs and mask bit go: the lane itself, or this invocation's lane when
    /// each invocation is one lane (D760).
    fn lane_slot(&mut self, lane: u32) -> Id {
        self.invocation
            .unwrap_or_else(|| Self::constant(self, lane))
    }

    /// Waits for every lane of the workgroup, making each one's workgroup-memory writes visible to
    /// the others (D760).
    fn workgroup_barrier(&mut self) {
        let scope = Self::constant(self, orbistoun_spirv::scope::WORKGROUP);
        let semantics = Self::constant(
            self,
            orbistoun_spirv::semantics::ACQUIRE_RELEASE
                | orbistoun_spirv::semantics::WORKGROUP_MEMORY,
        );
        self.builder
            .function(op::CONTROL_BARRIER, &[scope.0, scope.0, semantics.0]);
    }

    /// The index of vertex `slot`'s distance `index` among the exported distances' words (D867).
    fn distance_word(&mut self, slot: Id, index: u32) -> Id {
        let (stride, offset) = (
            Self::constant(self, CLIP_DISTANCES),
            Self::constant(self, index),
        );
        let scaled = self.builder.id();
        self.builder
            .function(op::IMUL, &[self.u32_type.0, scaled.0, slot.0, stride.0]);
        let at = self.builder.id();
        self.builder
            .function(op::IADD, &[self.u32_type.0, at.0, scaled.0, offset.0]);
        at
    }

    /// A pointer to word `index` of a workgroup array.
    fn workgroup_word(&mut self, array: Id, index: Id) -> Id {
        let pointer = self.builder.id();
        self.builder.function(
            op::ACCESS_CHAIN,
            &[self.local_ptr.0, pointer.0, array.0, index.0],
        );
        pointer
    }

    /// Every lane's `condition` as a mask's two halves, through workgroup memory, for a wave wider
    /// than the host's subgroup (D760): cleared by lane zero, each lane's bit ored in, then read.
    fn exchange_mask(&mut self, mask: Id, invocation: Id, condition: Id) -> (Id, Id) {
        let (u32_type, bool_type) = (self.u32_type, self.bool_type);
        let (zero, one, five, thirty_one) = (
            Self::constant(self, 0),
            Self::constant(self, 1),
            Self::constant(self, 5),
            Self::constant(self, 31),
        );
        let scope = Self::constant(self, orbistoun_spirv::scope::WORKGROUP);
        let semantics = Self::constant(
            self,
            orbistoun_spirv::semantics::ACQUIRE_RELEASE
                | orbistoun_spirv::semantics::WORKGROUP_MEMORY,
        );
        let halves = [
            self.workgroup_word(mask, zero),
            self.workgroup_word(mask, one),
        ];
        // No lane still reads the last mask when lane zero clears it.
        self.workgroup_barrier();
        let first = self.builder.id();
        self.builder
            .function(op::IEQUAL, &[bool_type.0, first.0, invocation.0, zero.0]);
        let (clear, cleared) = (self.builder.id(), self.builder.id());
        self.builder.function(op::SELECTION_MERGE, &[cleared.0, 0]);
        self.builder
            .function(op::BRANCH_CONDITIONAL, &[first.0, clear.0, cleared.0]);
        self.builder.function(op::LABEL, &[clear.0]);
        for half in halves {
            self.builder.function(op::STORE, &[half.0, zero.0]);
        }
        self.builder.function(op::BRANCH, &[cleared.0]);
        self.builder.function(op::LABEL, &[cleared.0]);
        self.workgroup_barrier();
        // This lane's bit, in the half its lane falls in.
        let b = &mut self.builder;
        let place = b.id();
        b.function(
            op::BITWISE_AND,
            &[u32_type.0, place.0, invocation.0, thirty_one.0],
        );
        let bit = b.id();
        b.function(op::SHIFT_LEFT_LOGICAL, &[u32_type.0, bit.0, one.0, place.0]);
        let contribution = b.id();
        b.function(
            op::SELECT,
            &[u32_type.0, contribution.0, condition.0, bit.0, zero.0],
        );
        let which = b.id();
        b.function(
            op::SHIFT_RIGHT_LOGICAL,
            &[u32_type.0, which.0, invocation.0, five.0],
        );
        let pointer = self.workgroup_word(mask, which);
        let ored = self.builder.id();
        self.builder.function(
            op::ATOMIC_OR,
            &[
                u32_type.0,
                ored.0,
                pointer.0,
                scope.0,
                semantics.0,
                contribution.0,
            ],
        );
        self.workgroup_barrier();
        let (low, high) = (self.builder.id(), self.builder.id());
        self.builder
            .function(op::LOAD, &[u32_type.0, low.0, halves[0].0]);
        self.builder
            .function(op::LOAD, &[u32_type.0, high.0, halves[1].0]);
        (low, high)
    }

    /// Lane `from`'s `value`, through workgroup memory, for a wave wider than the host's subgroup
    /// (D760): every lane writes its own word, then each reads the one it asks for.
    fn exchange_value(&mut self, words: Id, invocation: Id, value: Id, from: Id) -> Id {
        // No lane still reads the last exchange when this one is written.
        self.workgroup_barrier();
        let own = self.workgroup_word(words, invocation);
        self.builder.function(op::STORE, &[own.0, value.0]);
        self.workgroup_barrier();
        let read = self.workgroup_word(words, from);
        let value = self.builder.id();
        self.builder
            .function(op::LOAD, &[self.u32_type.0, value.0, read.0]);
        value
    }

    /// `halves` with this invocation's bit set where `condition` holds, for a guest dispatch run one
    /// invocation per lane (D880): its own bit and no other lane's, since its program reads a mask
    /// only a lane's own bit at a time.
    fn own_lane_bit(&mut self, (low, high): (Id, Id), invocation: Id, condition: Id) -> (Id, Id) {
        let (u32_type, bool_type) = (self.u32_type, self.bool_type);
        let (zero, one, thirty_one, thirty_two) = (
            Self::constant(self, 0),
            Self::constant(self, 1),
            Self::constant(self, 31),
            Self::constant(self, 32),
        );
        let within = self.binary(op::BITWISE_AND, invocation, thirty_one);
        let bit = self.binary(op::SHIFT_LEFT_LOGICAL, one, within);
        let b = &mut self.builder;
        let set = b.id();
        b.function(op::SELECT, &[u32_type.0, set.0, condition.0, bit.0, zero.0]);
        let in_high = b.id();
        b.function(
            op::UGREATER_THAN_EQUAL,
            &[bool_type.0, in_high.0, invocation.0, thirty_two.0],
        );
        let (to_low, to_high) = (b.id(), b.id());
        b.function(
            op::SELECT,
            &[u32_type.0, to_low.0, in_high.0, zero.0, set.0],
        );
        b.function(
            op::SELECT,
            &[u32_type.0, to_high.0, in_high.0, set.0, zero.0],
        );
        let low = self.binary(op::BITWISE_OR, low, to_low);
        let high = self.binary(op::BITWISE_OR, high, to_high);
        (low, high)
    }

    /// Whether other invocations may store guest memory while this one does: each invocation one
    /// lane (D760, D880), or a guest dispatch, whose waves (D858) and groups run as invocations
    /// side by side. A lane that does not run then must not write back a word it read: a thread
    /// past a group's end has ids that wrap onto another's, and its old value would race that
    /// thread's store.
    const fn stores_beside_others(&self) -> bool {
        self.invocation.is_some() || self.dispatch.is_some()
    }

    /// Stores `value` through `pointer` where `condition` holds, and nothing otherwise.
    fn store_if(&mut self, condition: Id, pointer: Id, value: Id) {
        let b = &mut self.builder;
        let (store, merge) = (b.id(), b.id());
        b.function(op::SELECTION_MERGE, &[merge.0, 0]);
        b.function(op::BRANCH_CONDITIONAL, &[condition.0, store.0, merge.0]);
        b.function(op::LABEL, &[store.0]);
        b.function(op::STORE, &[pointer.0, value.0]);
        b.function(op::BRANCH, &[merge.0]);
        b.function(op::LABEL, &[merge.0]);
    }

    /// Whether this invocation's bit of a mask held as two halves is set, when each invocation is
    /// one lane (D760).
    fn invocation_bit(&mut self, invocation: Id, (low, high): (Id, Id)) -> Id {
        let (u32_type, bool_type) = (self.u32_type, self.bool_type);
        let (one, zero, thirty_one, thirty_two) = (
            Self::constant(self, 1),
            Self::constant(self, 0),
            Self::constant(self, 31),
            Self::constant(self, 32),
        );
        let b = &mut self.builder;
        let in_high = b.id();
        b.function(
            op::UGREATER_THAN_EQUAL,
            &[bool_type.0, in_high.0, invocation.0, thirty_two.0],
        );
        let half = b.id();
        b.function(op::SELECT, &[u32_type.0, half.0, in_high.0, high.0, low.0]);
        let within = b.id();
        b.function(
            op::BITWISE_AND,
            &[u32_type.0, within.0, invocation.0, thirty_one.0],
        );
        let shifted = b.id();
        b.function(
            op::SHIFT_RIGHT_LOGICAL,
            &[u32_type.0, shifted.0, half.0, within.0],
        );
        let bit = b.id();
        b.function(op::BITWISE_AND, &[u32_type.0, bit.0, shifted.0, one.0]);
        let set = b.id();
        b.function(op::INOT_EQUAL, &[bool_type.0, set.0, bit.0, zero.0]);
        set
    }

    /// The four-component unsigned vector type, declared once on first use.
    fn unsigned_vec4(&mut self) -> Id {
        if let Some(id) = self.uvec4 {
            return id;
        }
        let id = self
            .builder
            .declare_type(op::TYPE_VECTOR, &[self.u32_type.0, 4]);
        self.uvec4 = Some(id);
        id
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
        if let Some(invocation) = self.invocation {
            let low = self.load_scalar(EXEC_LO);
            let high = self.load_scalar(EXEC_HI);
            return self.invocation_bit(invocation, (low, high));
        }
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

    /// Writes vertex `slot`'s distance inside each user clip plane: `dot(plane, position)` over
    /// the clip-space position it was given, which the host clips where it is negative, as the
    /// guest's clipper does against `PA_CL_UCP_*` (D830).
    fn write_clip_distances(&mut self, mesh: &MeshOutputs, slot: Id, position: [Id; 4]) {
        let f32_type = self.f32_type;
        let member = Self::constant(self, 1);
        // A distance the vertex exported, where its draw clips by it, stands in its plane's place
        // (D867); one past the planes that it does not export clips nothing.
        if let Some(words) = mesh.distances {
            for index in 0..CLIP_DISTANCES {
                let exported = mesh.clip_mask >> index & 1 != 0;
                if !exported && index < CLIP_PLANES as u32 {
                    continue;
                }
                let at = self.distance_word(slot, index);
                let pointer = self.workgroup_word(words, at);
                let (bits, distance) = (self.builder.id(), self.builder.id());
                self.builder
                    .function(op::LOAD, &[self.u32_type.0, bits.0, pointer.0]);
                self.builder
                    .function(op::BITCAST, &[f32_type.0, distance.0, bits.0]);
                let element = Self::constant(self, index);
                let target = self.builder.id();
                self.builder.function(
                    op::ACCESS_CHAIN,
                    &[
                        mesh.float_ptr.0,
                        target.0,
                        mesh.vertices.0,
                        slot.0,
                        member.0,
                        element.0,
                    ],
                );
                self.builder.function(op::STORE, &[target.0, distance.0]);
            }
        }
        for (index, plane) in (0u32..).zip(mesh.planes) {
            if mesh.distances.is_some() && mesh.clip_mask >> index & 1 != 0 {
                continue;
            }
            let mut sum: Option<Id> = None;
            for (component, coefficient) in position.into_iter().zip(plane) {
                let product = self.builder.id();
                self.builder.function(
                    op::FMUL,
                    &[f32_type.0, product.0, component.0, coefficient.0],
                );
                sum = Some(match sum {
                    None => product,
                    Some(before) => {
                        let added = self.builder.id();
                        self.builder
                            .function(op::FADD, &[f32_type.0, added.0, before.0, product.0]);
                        added
                    }
                });
            }
            let Some(distance) = sum else {
                continue;
            };
            let element = Self::constant(self, index);
            let pointer = self.builder.id();
            self.builder.function(
                op::ACCESS_CHAIN,
                &[
                    mesh.float_ptr.0,
                    pointer.0,
                    mesh.vertices.0,
                    slot.0,
                    member.0,
                    element.0,
                ],
            );
            self.builder.function(op::STORE, &[pointer.0, distance.0]);
        }
    }

    /// Writes vertex `slot`'s point size, from the word it exported to (D867).
    fn copy_point_size(&mut self, mesh: &MeshOutputs, words: Id, slot: Id) {
        let (u32_type, f32_type) = (self.u32_type, self.f32_type);
        let pointer = self.workgroup_word(words, slot);
        let (bits, size) = (self.builder.id(), self.builder.id());
        self.builder
            .function(op::LOAD, &[u32_type.0, bits.0, pointer.0]);
        self.builder
            .function(op::BITCAST, &[f32_type.0, size.0, bits.0]);
        let member = Self::constant(self, POINT_SIZE_MEMBER);
        let target = self.builder.id();
        self.builder.function(
            op::ACCESS_CHAIN,
            &[
                mesh.float_ptr.0,
                target.0,
                mesh.vertices.0,
                slot.0,
                member.0,
            ],
        );
        self.builder.function(op::STORE, &[target.0, size.0]);
    }

    /// A `vec4` value's four components.
    fn extract_vec4(&mut self, value: Id) -> [Id; 4] {
        let f32_type = self.f32_type;
        std::array::from_fn(|component| {
            let id = self.builder.id();
            self.builder.function(
                op::COMPOSITE_EXTRACT,
                &[f32_type.0, id.0, value.0, component as u32],
            );
            id
        })
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
        self.emit_mesh_outputs();
        self.builder.function(op::RETURN, &[]);
        self.builder.function(op::FUNCTION_END, &[]);
        self.builder.check()?;
        Ok((self.builder.finish(), self.translated))
    }

    /// A mesh module's epilogue: declares the counts the program's allocation request set, writes
    /// each lane's staged exports to the slots within them, then completes a rectangle list.
    ///
    /// Every invocation reaches it once the program has run, so the declaration precedes every
    /// output write on every path, as the format requires ([`StagedOutputs`]).
    ///
    /// Exports are staged unmasked, and a lane's slot is written when it is within the counts.
    /// That is exact when the emitted vertices are the low lanes, as a primitive shader arranges
    /// them: it narrows the mask to `(1 << n) - 1`, so an inactive lane's slot is past the count. A
    /// sparse vertex mask would emit a vertex it did not mean to; the mask is a runtime value, so
    /// this is an assumption rather than a check (D688).
    fn emit_mesh_outputs(&mut self) {
        let (Some(staged), Some(mesh)) = (self.staged.clone(), self.mesh.clone()) else {
            return;
        };
        let u32_type = self.u32_type;
        let vertices = self.builder.id();
        self.builder
            .function(op::LOAD, &[u32_type.0, vertices.0, staged.vertices.0]);
        let primitives = self.builder.id();
        self.builder
            .function(op::LOAD, &[u32_type.0, primitives.0, staged.primitives.0]);
        let declared = if self.primitive == MeshPrimitive::Rectangles {
            // Each rectangle adds its fourth corner after the emitted vertices, and a second
            // triangle.
            let total = self.builder.id();
            self.builder
                .function(op::IADD, &[u32_type.0, total.0, vertices.0, primitives.0]);
            let doubled = self.builder.id();
            self.builder.function(
                op::IADD,
                &[u32_type.0, doubled.0, primitives.0, primitives.0],
            );
            [total, doubled]
        } else {
            [vertices, primitives]
        };
        self.builder
            .function(op::SET_MESH_OUTPUTS_EXT, &[declared[0].0, declared[1].0]);
        if self.capture_positions {
            let member = Self::constant(self, 0);
            self.write_observation(member, CAPTURED_COUNTS, vertices);
            self.write_observation(member, CAPTURED_COUNTS + 1, primitives);
        }
        // A layered module keeps only the primitives in its pass's layer (D859): every lane's layer
        // written, and the layer this pass renders.
        let culling = mesh.layers.map(|layers| {
            self.workgroup_barrier();
            let pass = self
                .draw_word(orbistoun_spirv::DRAW_DATA_LAYER_WORD)
                .unwrap_or_else(|| self.constant(orbistoun_spirv::DRAW_LAYER_UNUSED));
            (layers, pass)
        });

        let member = Self::constant(self, 0);
        // The lanes this invocation staged: the whole wave, or its own one lane (D760).
        for lane in 0..self.modelled.min(MESH_SLOTS) {
            let slot = self.lane_slot(lane);
            let element = Self::constant(self, lane);

            // The lane's vertex, when the request counted it.
            let merge = self.open_below(slot, vertices);
            let position = self.load_staged(staged.vec4_ptr, staged.positions, element, self.vec4);
            let target = self.builder.id();
            self.builder.function(
                op::ACCESS_CHAIN,
                &[mesh.vec4_ptr.0, target.0, mesh.vertices.0, slot.0, member.0],
            );
            self.builder.function(op::STORE, &[target.0, position.0]);
            let components = self.extract_vec4(position);
            if self.capture_positions {
                self.capture_position(member, slot, components);
            }
            self.write_clip_distances(&mesh, slot, components);
            if let Some(words) = mesh.point_sizes {
                self.copy_point_size(&mesh, words, slot);
            }
            for (location, copy) in &staged.parameters {
                let Some(output) = mesh.parameters.get(location).copied() else {
                    continue;
                };
                let value = self.load_staged(staged.vec4_ptr, *copy, element, self.vec4);
                let target = self.builder.id();
                self.builder.function(
                    op::ACCESS_CHAIN,
                    &[mesh.vec4_ptr.0, target.0, output.0, slot.0],
                );
                self.builder.function(op::STORE, &[target.0, value.0]);
            }
            self.close_below(merge);

            // Its primitive, likewise. A rectangle's first triangle is its three corners; its second
            // follows it.
            let merge = self.open_below(slot, primitives);
            let at = if self.primitive == MeshPrimitive::Rectangles {
                Self::constant(self, 2 * lane)
            } else {
                slot
            };
            let corners =
                self.load_staged(staged.index_ptr, staged.indices, element, mesh.index_type);
            let target = self.builder.id();
            self.builder.function(
                op::ACCESS_CHAIN,
                &[mesh.index_ptr.0, target.0, mesh.indices.0, at.0],
            );
            self.builder.function(op::STORE, &[target.0, corners.0]);
            if let Some((layers, pass)) = culling {
                self.cull_outside_pass((layers, pass), (&mesh, corners), at);
            }
            self.close_below(merge);
        }
        self.complete_rectangles();
    }

    /// Culls the primitive at `at` - and a rectangle's second triangle after it - unless it is in
    /// the layer `pass` renders (D859): its layer is its first vertex's, and a pass of
    /// [`orbistoun_spirv::DRAW_LAYER_UNUSED`] keeps every primitive.
    fn cull_outside_pass(
        &mut self,
        (layers, pass): (MeshLayers, Id),
        (mesh, corners): (&MeshOutputs, Id),
        at: Id,
    ) {
        let u32_type = self.u32_type;
        let first = if mesh.index_width == 1 {
            corners
        } else {
            let first = self.builder.id();
            self.builder
                .function(op::COMPOSITE_EXTRACT, &[u32_type.0, first.0, corners.0, 0]);
            first
        };
        // An index past the vertex slots is the guest's fault; reading past the words would be ours.
        let last = Self::constant(self, MESH_SLOTS - 1);
        let slot = self.binary(op::BITWISE_AND, first, last);
        let pointer = self.workgroup_word(layers.words, slot);
        let layer = self.builder.id();
        self.builder
            .function(op::LOAD, &[u32_type.0, layer.0, pointer.0]);
        let unused = Self::constant(self, orbistoun_spirv::DRAW_LAYER_UNUSED);
        let used = self.compare(op::INOT_EQUAL, pass, unused);
        let elsewhere = self.compare(op::INOT_EQUAL, layer, pass);
        let cull = self.builder.id();
        self.builder.function(
            op::LOGICAL_AND,
            &[self.bool_type.0, cull.0, used.0, elsewhere.0],
        );
        let mut slots = vec![at];
        if self.primitive == MeshPrimitive::Rectangles {
            let one = Self::constant(self, 1);
            slots.push(self.binary(op::IADD, at, one));
        }
        for slot in slots {
            let target = self.builder.id();
            self.builder.function(
                op::ACCESS_CHAIN,
                &[layers.cull_ptr.0, target.0, layers.cull.0, slot.0],
            );
            self.builder.function(op::STORE, &[target.0, cull.0]);
        }
    }

    /// Opens a block run only when `value < bound`, returning the merge [`Self::close_below`]
    /// closes it at.
    fn open_below(&mut self, value: Id, bound: Id) -> Id {
        let below = self.builder.id();
        self.builder.function(
            op::ULESS_THAN,
            &[self.bool_type.0, below.0, value.0, bound.0],
        );
        let (body, merge) = (self.builder.id(), self.builder.id());
        self.builder.function(op::SELECTION_MERGE, &[merge.0, 0]);
        self.builder
            .function(op::BRANCH_CONDITIONAL, &[below.0, body.0, merge.0]);
        self.builder.function(op::LABEL, &[body.0]);
        merge
    }

    /// Closes a block [`Self::open_below`] opened.
    fn close_below(&mut self, merge: Id) {
        self.builder.function(op::BRANCH, &[merge.0]);
        self.builder.function(op::LABEL, &[merge.0]);
    }

    /// Element `element` of a staged copy, of type `element_type`.
    fn load_staged(&mut self, pointer: Id, copy: Id, element: Id, element_type: Id) -> Id {
        let at = self.builder.id();
        self.builder
            .function(op::ACCESS_CHAIN, &[pointer.0, at.0, copy.0, element.0]);
        let value = self.builder.id();
        self.builder
            .function(op::LOAD, &[element_type.0, value.0, at.0]);
        value
    }

    /// A rectangle-list module's epilogue: for each declared primitive, the fourth corner as the
    /// vertex after every emitted one - `v1 + v2 - v0` in position and in each parameter - and the
    /// second triangle, `(v2, v1, v3)`, which winds the same way as `(v0, v1, v2)`.
    ///
    /// The corners are read from the module's copies, indexed by the guest's own indices masked to
    /// the copies' length: an index past the vertices a lane holds is the guest's fault, and reading
    /// past a private array would be the host's.
    fn complete_rectangles(&mut self) {
        let (Some(copies), Some(mesh)) = (self.staged.clone(), self.mesh.clone()) else {
            return;
        };
        if self.primitive != MeshPrimitive::Rectangles {
            return;
        }
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
                if output.is_none() {
                    let vec4_components = self.extract_vec4(corner);
                    self.write_clip_distances(&mesh, fourth, vec4_components);
                }
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

    /// The clip-space position that lands an undivided window-space one on its pixel (D780):
    /// `(X, Y, Z, W)`, whose pixel the hardware takes as `(X / W, Y / W)` and depth as `Z / W`,
    /// becomes `(X / S, Y / S, Z, W)` for `S` the [`WINDOW_SPACE_SCALE`] the draw's viewport scales
    /// by. Vulkan divides by `W` and scales by `S`, giving back the same pixel and depth.
    fn undivided_window_to_clip(&mut self, [x, y, depth, w]: [Id; 4]) -> [Id; 4] {
        let inverse_scale = self.float_constant(WINDOW_SPACE_SCALE.recip());
        [
            self.float_binary(op::FMUL, x, inverse_scale),
            self.float_binary(op::FMUL, y, inverse_scale),
            depth,
            w,
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

    /// Writes vertex `slot`'s clip-space position into the observation buffer at words
    /// `slot * 4 ..`, its components' bits as they are, where `slot` is one of the first
    /// [`CAPTURED_VERTICES`] ([`UserData::capture_positions`]).
    fn capture_position(&mut self, member: Id, slot: Id, components: [Id; 4]) {
        let captured = Self::constant(self, CAPTURED_VERTICES);
        let merge = self.open_below(slot, captured);
        let four = Self::constant(self, 4);
        let base = self.binary(op::IMUL, slot, four);
        for (offset, component) in (0..).zip(components) {
            let offset = Self::constant(self, offset);
            let at = self.binary(op::IADD, base, offset);
            let bits = self.builder.id();
            self.builder
                .function(op::BITCAST, &[self.u32_type.0, bits.0, component.0]);
            let to = self.builder.id();
            self.builder.function(
                op::ACCESS_CHAIN,
                &[
                    self.buffer_element_ptr.0,
                    to.0,
                    self.buffer.0,
                    member.0,
                    at.0,
                ],
            );
            self.builder.function(op::STORE, &[to.0, bits.0]);
        }
        self.close_below(merge);
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

    fn program_address(&self) -> Option<u64> {
        self.program_address
    }

    fn colour_output(&self, target: u32) -> Option<(Id, Id)> {
        let (vec4, colour) = self.output.colour?;
        let kind = self.output.colour_type(target, vec4);
        match target {
            0 => Some((kind, colour)),
            _ => self
                .output
                .more_colour
                .get(usize::try_from(target).ok()?)
                .copied()
                .flatten()
                .map(|variable| (kind, variable)),
        }
    }

    fn colour_is_integer(&self, target: u32) -> bool {
        self.output.colour.is_some() && target < 8 && self.output.integer >> target & 1 == 1
    }

    fn depth_output(&self) -> Option<(Id, Id)> {
        self.output.depth
    }

    fn sample_mask_output(&self) -> Option<(Id, Id)> {
        self.output.sample_mask
    }

    // The four hooks below stage what the program exports; the outputs are written once it has
    // run, after the counts are declared (`emit_mesh_outputs`).

    fn set_mesh_outputs(&mut self, vertices: Id, primitives: Id) -> Option<()> {
        let staged = self.staged.clone()?;
        self.builder
            .function(op::STORE, &[staged.vertices.0, vertices.0]);
        self.builder
            .function(op::STORE, &[staged.primitives.0, primitives.0]);
        Some(())
    }

    fn write_mesh_position(&mut self, lane: u32, components: [Id; 4]) -> Option<()> {
        let staged = self.staged.clone()?;
        let components = match self.window_space {
            WindowSpace::Clip => components,
            WindowSpace::Divided => self.window_to_clip(components),
            WindowSpace::Undivided => self.undivided_window_to_clip(components),
        };
        let value = self.composite_vec4(components);
        self.copy_element(staged.positions, staged.vec4_ptr, lane, value);
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
        let staged = self.staged.clone()?;
        // A location the module declares no output for is refused, staged or not.
        mesh.parameters.get(&location)?;
        let copy = *staged.parameters.get(&location)?;
        let value = self.composite_vec4(components);
        self.copy_element(copy, staged.vec4_ptr, lane, value);
        Some(())
    }

    fn write_mesh_point_size(&mut self, lane: u32, bits: Id) -> Option<()> {
        let words = self.mesh.as_ref()?.point_sizes?;
        let slot = self.lane_slot(lane);
        let pointer = self.workgroup_word(words, slot);
        self.builder.function(op::STORE, &[pointer.0, bits.0]);
        Some(())
    }

    fn write_mesh_layer(&mut self, lane: u32, layer: Id) -> Option<()> {
        let words = self.mesh.as_ref()?.layers?.words;
        let slot = self.lane_slot(lane);
        let pointer = self.workgroup_word(words, slot);
        self.builder.function(op::STORE, &[pointer.0, layer.0]);
        Some(())
    }

    fn write_mesh_indices(&mut self, lane: u32, indices: &[Id]) -> Option<()> {
        let mesh = self.mesh.clone()?;
        let staged = self.staged.clone()?;

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
        self.copy_element(staged.indices, staged.index_ptr, lane, value);
        Some(())
    }

    fn mesh_primitive(&self) -> MeshPrimitive {
        self.primitive
    }

    fn read_vector_at(&mut self, register: u32, lane: Id) -> Option<Id> {
        // One invocation per lane (D760): the other lane's register is that invocation's own.
        if let (Some(invocation), Some((_, words))) = (self.invocation, self.exchange) {
            let own = self.load_lane(register, 0);
            return Some(self.exchange_value(words, invocation, own, lane));
        }
        if self.invocation.is_some() {
            let own = self.load_lane(register, 0);
            let scope = Self::constant(self, orbistoun_spirv::scope::SUBGROUP);
            let read = self.builder.id();
            self.builder.function(
                op::GROUP_NON_UNIFORM_SHUFFLE,
                &[self.u32_type.0, read.0, scope.0, own.0, lane.0],
            );
            return Some(read);
        }
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
        let (vec4, _) = self.output.colour?;
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

    fn local_words(&self) -> u32 {
        self.local_words
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

    fn modelled_lanes(&self) -> u32 {
        self.modelled
    }

    fn lane_index(&mut self) -> Option<Id> {
        self.invocation
    }

    fn quad_position(&mut self) -> Option<Id> {
        let invocation = self.quad_invocation?;
        let three = self.constant(3);
        Some(self.binary(op::BITWISE_AND, invocation, three))
    }

    fn quad_broadcast(&mut self, value: Id, from: u32) -> Option<Id> {
        self.quad_invocation?;
        let scope = Self::constant(self, orbistoun_spirv::scope::SUBGROUP);
        let index = self.constant(from);
        let read = self.builder.id();
        self.builder.function(
            op::GROUP_NON_UNIFORM_QUAD_BROADCAST,
            &[self.u32_type.0, read.0, scope.0, value.0, index.0],
        );
        Some(read)
    }

    fn merge_local(&mut self, word_index: Id, (keep, set): (Id, Id), lane: u32) -> bool {
        if self.invocation.is_none() || self.lane_known(lane) == Some(false) {
            return self.invocation.is_some();
        }
        let active = self.lane_active(lane);
        let (pointer_type, array, u32_type) = (self.local_ptr, self.local, self.u32_type);
        let scope = Self::constant(self, orbistoun_spirv::scope::WORKGROUP);
        let semantics = Self::constant(
            self,
            orbistoun_spirv::semantics::ACQUIRE_RELEASE
                | orbistoun_spirv::semantics::WORKGROUP_MEMORY,
        );
        let b = &mut self.builder;
        let pointer = b.id();
        b.function(
            op::ACCESS_CHAIN,
            &[pointer_type.0, pointer.0, array.0, word_index.0],
        );
        let (merge, store) = (b.id(), b.id());
        b.function(op::SELECTION_MERGE, &[merge.0, 0]);
        b.function(op::BRANCH_CONDITIONAL, &[active.0, store.0, merge.0]);
        b.function(op::LABEL, &[store.0]);
        let (kept, placed) = (b.id(), b.id());
        b.function(
            op::ATOMIC_AND,
            &[u32_type.0, kept.0, pointer.0, scope.0, semantics.0, keep.0],
        );
        b.function(
            op::ATOMIC_OR,
            &[u32_type.0, placed.0, pointer.0, scope.0, semantics.0, set.0],
        );
        b.function(op::BRANCH, &[merge.0]);
        b.function(op::LABEL, &[merge.0]);
        true
    }

    fn after_local_access(&mut self) {
        if self.invocation.is_none() {
            return;
        }
        let scope = Self::constant(self, orbistoun_spirv::scope::WORKGROUP);
        let semantics = Self::constant(
            self,
            orbistoun_spirv::semantics::ACQUIRE_RELEASE
                | orbistoun_spirv::semantics::WORKGROUP_MEMORY,
        );
        self.builder
            .function(op::CONTROL_BARRIER, &[scope.0, scope.0, semantics.0]);
    }

    fn set_lane_bit(&mut self, halves: (Id, Id), lane: u32, condition: Id) -> (Id, Id) {
        let Some(invocation) = self.invocation else {
            return model::constant_lane_bit(self, halves, lane, condition);
        };
        // A guest dispatch run per lane holds only its own lane's bits where its program reads no
        // mask whole, only each lane its own bit (D880); one that reads a mask whole makes every
        // lane's, by ballot or exchange as a primitive shader does (D884).
        if self.lane_masks == Some(LaneMasks::Own) {
            return self.own_lane_bit(halves, invocation, condition);
        }
        if let Some((mask, _)) = self.exchange {
            let (low, high) = self.exchange_mask(mask, invocation, condition);
            let low = self.binary(op::BITWISE_OR, halves.0, low);
            let high = self.binary(op::BITWISE_OR, halves.1, high);
            return (low, high);
        }
        // One invocation per lane (D760): the subgroup's ballot is the mask, every lane's bit.
        let uvec4 = self.unsigned_vec4();
        let scope = Self::constant(self, orbistoun_spirv::scope::SUBGROUP);
        let u32_type = self.u32_type;
        let b = &mut self.builder;
        let ballot = b.id();
        b.function(
            op::GROUP_NON_UNIFORM_BALLOT,
            &[uvec4.0, ballot.0, scope.0, condition.0],
        );
        let (low, high) = (b.id(), b.id());
        b.function(op::COMPOSITE_EXTRACT, &[u32_type.0, low.0, ballot.0, 0]);
        b.function(op::COMPOSITE_EXTRACT, &[u32_type.0, high.0, ballot.0, 1]);
        let low = self.binary(op::BITWISE_OR, halves.0, low);
        let high = self.binary(op::BITWISE_OR, halves.1, high);
        (low, high)
    }

    fn lane_bit(&mut self, low: Id, high: Id, lane: u32) -> Id {
        match self.invocation {
            Some(invocation) => self.invocation_bit(invocation, (low, high)),
            None => model::constant_lane_test(self, (low, high), lane),
        }
    }

    fn position_vectors(&self) -> Option<u32> {
        self.position_vectors
    }

    fn write_mesh_clip_distance(&mut self, lane: u32, index: u32, bits: Id) -> Option<()> {
        let words = self.mesh.as_ref()?.distances?;
        let slot = self.lane_slot(lane);
        let at = self.distance_word(slot, index);
        let pointer = self.workgroup_word(words, at);
        self.builder.function(op::STORE, &[pointer.0, bits.0]);
        Some(())
    }

    fn drops_streamout_stores(&self) -> bool {
        self.streamout == StreamoutRole::Draw
    }

    fn discards_vertex_exports(&self) -> bool {
        self.streamout == StreamoutRole::Pass
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
            Operand::Named(named) if model::lane_mask_high_name(named).is_some() => {
                let mask = model::lane_mask_high_name(named).expect("checked immediately above");
                let (_, high) = self.read_lane_mask(mask)?;
                Ok(high)
            }
            Operand::Named(named) if model::lane_mask_name(named).is_some() => {
                let mask = model::lane_mask_name(named).expect("checked immediately above");
                let (low, _) = self.read_lane_mask(mask)?;
                Ok(low)
            }
            // The m0 register read back as a source: whatever the shader last wrote.
            // Uniform across the wavefront, like every scalar.
            Operand::Named(name) if name == model::M0 => Ok(self.read_m0()),
            // The condition code read as a word, as `s_mul_i32 d, 1.0, src_scc` turns it into a float.
            Operand::Named(name) if name == model::SRC_SCC => Ok(self.read_condition_word()),
            // An inline float, named by the operand table. Its bits go into the register, as the
            // hardware holds them.
            Operand::Named(name) => {
                let bits = model::inline_float_bits(name).ok_or(TranslateError::Unsupported {
                    offset: instruction.offset,
                    detail: "named operand is not an inline float",
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
        // Whatever it held is gone; a copy says so again once the write is done.
        self.vector_copies.remove(&register);
        self.store_lane_masked(register, lane, value);
    }

    fn write_scalar(&mut self, register: u32, value: Id) {
        // A user-data word the stream never wrote is unknown only until the program writes its
        // register: a read after that reads the program's own value. In program order, as the
        // translation emits it.
        if register < u32::BITS {
            self.unwritten_user_data &= !(1 << register);
        }
        if register < u128::BITS {
            self.images.written_scalars |= 1 << register;
            self.images.opaque_scalars &= !(1 << register);
        }
        // Whatever it held is gone; a copy says so again once the write is done.
        self.scalar_copies.remove(&register);
        // A write into either descriptor group means the next sample does not read the texture the
        // last one did. Recorded here because this is the only place that sees writes (D690).
        for bound in &mut self.textures {
            let within = |first: u32, count: u32| register >= first && register < first + count;
            // The sampler's range exists only once something named a sampler.
            let sampler_disturbed = bound
                .sampler
                .is_some_and(|first| within(first, model::SAMPLER_DESCRIPTOR_REGISTERS));
            if within(bound.descriptor, model::IMAGE_DESCRIPTOR_REGISTERS) {
                bound.disturbed = true;
            }
            if sampler_disturbed {
                bound.sampler_disturbed = true;
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

    fn storage_image(
        &mut self,
        descriptor: u32,
        (volume, level): (bool, Option<u32>),
    ) -> Result<model::Stored, &'static str> {
        // The fragment stage only: the harness binds its storage image with fragment stage flags,
        // and the `vec4` texel type comes from the colour output, which only a fragment module has.
        if self.stage == Stage::Mesh {
            return Err(concat!(
                "a primitive shader stores to an image, and its stage binds none - a storage image ",
                "is bound to a pixel shader or a compute dispatch"
            ));
        }
        if volume && self.stage != Stage::Compute {
            return Err(concat!(
                "a pixel shader stores to a three-dimensional image, and its harness binds a ",
                "two-dimensional one"
            ));
        }
        if let Some((stored, was)) = self.images.stored {
            let same_level = self
                .images
                .stored_source
                .is_none_or(|source| source.level_user_data == level);
            if was != descriptor || stored.volume != volume || !same_level {
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
            let shape = model::ImageShape {
                volume,
                ..model::ImageShape::default()
            };
            let mut source = self.new_texture_source(descriptor, (None, shape))?;
            source.slot = 0;
            source.level_user_data = level;
            self.images.stored_source = Some(source);
        }

        let pointer = self.builder.id();
        let variable = self.builder.id();

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

        // The seven operands a sampled image takes: `Dim` 2D, or 3D for a volume (D823), `Sampled`
        // at 2 (written through an image instruction, not read through a sampler) and the format
        // `Unknown`, which the capability above permits.
        let integer = self.stage == Stage::Compute && self.integer_images[1];
        let (element, value) = if integer {
            (self.u32_type, self.unsigned_vec4())
        } else {
            (self.f32_type, self.vec4)
        };
        let image = self.builder.declare_type(
            op::TYPE_IMAGE,
            &[element.0, 1 + u32::from(volume), 0, 0, 0, 2, 0],
        );
        self.builder
            .declare(op::TYPE_POINTER, &[pointer.0, UNIFORM_CONSTANT, image.0]);
        self.builder
            .declare(op::VARIABLE, &[pointer.0, variable.0, UNIFORM_CONSTANT]);
        // A sampled texture's fetch declares the same `uvec2`, and the module's one `uvec3` serves a
        // volume.
        let texel = self
            .builder
            .declare_type(op::TYPE_VECTOR, &[self.u32_type.0, 2 + u32::from(volume)]);

        let stored = model::Stored {
            variable,
            image,
            texel,
            value,
            integer,
            volume,
        };
        self.images.stored = Some((stored, descriptor));
        Ok(stored)
    }

    fn sampled_image(
        &mut self,
        descriptor: u32,
        sampler: Option<u32>,
        (at, shape): (u32, model::ImageShape),
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
        // A sampler reloaded from where the first sample's came from is the same sampler: only a
        // load from elsewhere, or one not traced, makes it another.
        let sampler_reaching = self.descriptor_loads.3.get(&at).copied();
        self.settle_samplers(sampler_reaching);
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
        // One image read through another sampler is another texture, its own slot (D690): a
        // sampler only conflicts with a sampler, and a fetch names none.
        let other_sampler =
            |b: &BoundTexture| matches!((b.sampler, sampler), (Some(was), Some(now)) if was != now);
        if !rebound
            && let Some(bound) = self.textures.iter_mut().find(|b| {
                b.descriptor == descriptor
                    && (reaching.is_none() || !b.disturbed || loaded_by(b))
                    && !other_sampler(b)
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
            if bound.texture.shape != shape {
                return Err(CUBE_AND_FLAT);
            }
            // The first instruction to name a sampler records it, which may come after the first to
            // name the image.
            bound.sampler = bound.sampler.or(sampler);
            return Ok(bound.texture);
        }
        let source = self.new_texture_source(descriptor, (reaching, shape))?;
        let binding = orbistoun_spirv::TEXTURE_BINDINGS[source.slot as usize];

        let pointer = self.builder.id();
        let variable = self.builder.id();
        self.decorate_binding(variable, binding);

        // Element type, then: two-dimensional, not depth, not arrayed, single-sampled, used with a
        // sampler, and no declared format - the same seven values the hand-written oracle declares.
        // A compute dispatch's fetched image may be an unsigned integer one, declared as such.
        let integer = self.stage == Stage::Compute && self.integer_images[0];
        let (element, result) = if integer {
            (self.u32_type, self.unsigned_vec4())
        } else {
            (self.f32_type, self.vec4)
        };
        let ((dim, components), depth) = (image_dim(shape), u32::from(shape.compare));
        // A second texture of the same shape shares these types: each is declared once per module,
        // and only the pointer and variable are the texture's own.
        let image = self
            .builder
            .declare_type(op::TYPE_IMAGE, &[element.0, dim, depth, 0, 0, 1, 0]);
        let combined = self
            .builder
            .declare_type(op::TYPE_SAMPLED_IMAGE, &[image.0]);
        self.builder
            .declare(op::TYPE_POINTER, &[pointer.0, UNIFORM_CONSTANT, combined.0]);
        self.builder
            .declare(op::VARIABLE, &[pointer.0, variable.0, UNIFORM_CONSTANT]);
        let coordinate = self
            .builder
            .declare_type(op::TYPE_VECTOR, &[self.f32_type.0, components]);
        // The integer coordinate a fetch takes: a texel index rather than a position across the
        // image. Declared alongside the sample's type because the unused one costs one declaration.
        let texel = self
            .builder
            .declare_type(op::TYPE_VECTOR, &[self.u32_type.0, 2]);

        let texture = model::Texture {
            variable,
            sampled: combined,
            image,
            coordinate,
            texel,
            result,
            integer,
            saturate: self
                .saturated
                .get(source.slot as usize)
                .copied()
                .unwrap_or_default(),
            shape,
        };
        self.textures.push(BoundTexture {
            texture,
            descriptor,
            sampler,
            source,
            disturbed: false,
            sampler_disturbed: false,
            sampler_load: sampler.and(sampler_reaching),
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

    fn group_waves(&self) -> u32 {
        self.group_waves
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

    fn note_vector_copy(&mut self, destination: u32, source: u32) {
        let word = self
            .still_seeded(source, 1)
            .or_else(|| self.scalar_copies.get(&source).copied());
        if let Some(word) = word {
            self.vector_copies.insert(destination, word);
        }
    }

    fn vector_user_data(&self, register: u32) -> Option<u32> {
        self.vector_copies.get(&register).copied()
    }

    fn note_scalar_copy(&mut self, destination: u32, source: u32) {
        let word = self
            .still_seeded(source, 1)
            .or_else(|| self.scalar_copies.get(&source).copied());
        if let Some(word) = word {
            self.scalar_copies.insert(destination, word);
        }
    }

    fn write_m0(&mut self, value: Id) {
        let pointer = self.m0;
        self.builder.function(op::STORE, &[pointer.0, value.0]);
    }

    fn instructions(&self) -> usize {
        self.translated
    }

    fn read_global_share(
        &mut self,
        instruction: &Instruction,
        (byte_address, in_range): (Id, Id),
    ) -> Result<Id, TranslateError> {
        let pointer = self.global_share_word(instruction, byte_address)?;
        let u32_type = self.u32_type;
        let zero = Self::constant(self, 0);
        let b = &mut self.builder;
        let value = b.id();
        b.function(op::LOAD, &[u32_type.0, value.0, pointer.0]);
        let chosen = b.id();
        b.function(
            op::SELECT,
            &[u32_type.0, chosen.0, in_range.0, value.0, zero.0],
        );
        Ok(chosen)
    }

    fn write_global_share(
        &mut self,
        instruction: &Instruction,
        (byte_address, in_range): (Id, Id),
        value: Id,
        lane: u32,
    ) -> Result<(), TranslateError> {
        if self.lane_known(lane) == Some(false) {
            return Ok(());
        }
        let pointer = self.global_share_word(instruction, byte_address)?;
        let active = self.lane_active(lane);
        let store = self.both(active, in_range);
        // A branch rather than a select of the old word: other workgroups write the share at the
        // same time, so a lane that does not store writes nothing back.
        let b = &mut self.builder;
        let (write, merge) = (b.id(), b.id());
        b.function(op::SELECTION_MERGE, &[merge.0, 0]);
        b.function(op::BRANCH_CONDITIONAL, &[store.0, write.0, merge.0]);
        b.function(op::LABEL, &[write.0]);
        b.function(op::STORE, &[pointer.0, value.0]);
        b.function(op::BRANCH, &[merge.0]);
        b.function(op::LABEL, &[merge.0]);
        Ok(())
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
        // One invocation per lane (D760): another lane may write this word at the same time, so an
        // inactive lane writes nothing rather than its old value back.
        if self.invocation.is_some() {
            let b = &mut self.builder;
            let (store, merge) = (b.id(), b.id());
            b.function(op::SELECTION_MERGE, &[merge.0, 0]);
            b.function(op::BRANCH_CONDITIONAL, &[active.0, store.0, merge.0]);
            b.function(op::LABEL, &[store.0]);
            b.function(op::STORE, &[pointer.0, value.0]);
            b.function(op::BRANCH, &[merge.0]);
            b.function(op::LABEL, &[merge.0]);
            return Ok(());
        }

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

    fn untraced_by(&self) -> Option<&(u32, String)> {
        self.untraced_by.as_ref()
    }

    fn draw_buffer_base(&mut self, slot: u32) -> Option<Id> {
        // A primitive shader's draw data carries each slot's base beside its place (D758); a
        // dispatch's push block carries its global loads' buffer's, the one such slot (D883).
        match self.stage {
            Stage::Mesh => self.draw_word(orbistoun_spirv::DRAW_DATA_BASES_WORD + slot),
            Stage::Compute => self.push_word(GLOBAL_BASE_PUSH_WORD),
            Stage::Fragment => None,
        }
    }

    fn buffer_descriptor_format(&self, slot: u32) -> Option<u32> {
        self.buffer_formats?.get(usize::try_from(slot).ok()?)
    }

    fn buffer_formats_given(&self) -> bool {
        self.buffer_formats.is_some()
    }

    fn write_draw_buffer(
        &mut self,
        slot: u32,
        (word_index, value): (Id, Id),
        outside: Id,
        lane: u32,
    ) -> Result<(), TranslateError> {
        let known = self.lane_known(lane);
        if known == Some(false) {
            return Ok(());
        }
        let Some((array, _)) = self.draw_buffers else {
            return Err(TranslateError::Unsupported {
                offset: 0,
                detail: "a draw buffer store in a module that declares none (D746)",
            });
        };
        let u32_type = self.u32_type;
        let buffer = Self::constant(self, slot);
        let member = Self::constant(self, 0);
        let zero = member;
        let bool_type = self.bool_type;
        let active = match known {
            Some(true) => None,
            _ => Some(self.lane_active(lane)),
        };
        let beside_others = self.stores_beside_others();
        let b = &mut self.builder;
        let block = b.id();
        b.function(
            op::ACCESS_CHAIN,
            &[array.block_ptr.0, block.0, array.variable.0, buffer.0],
        );
        let length = b.id();
        b.function(op::ARRAY_LENGTH, &[u32_type.0, length.0, block.0, 0]);
        // Past the bound words the store lands on word zero with word zero's own value: defined,
        // and no change, since the host binds every word the descriptor admits.
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
        if beside_others {
            // Another invocation may store this word at the same time, so a store that does not
            // land writes nothing rather than what it read (D858, D880).
            let landed = match active {
                None => inside,
                Some(active) => {
                    let both = b.id();
                    b.function(op::LOGICAL_AND, &[bool_type.0, both.0, inside.0, active.0]);
                    both
                }
            };
            self.store_if(landed, pointer, value);
        } else {
            let old = b.id();
            b.function(op::LOAD, &[u32_type.0, old.0, pointer.0]);
            let kept = b.id();
            b.function(op::SELECT, &[u32_type.0, kept.0, inside.0, value.0, old.0]);
            let chosen = match active {
                None => kept,
                Some(active) => {
                    let chosen = b.id();
                    b.function(op::SELECT, &[u32_type.0, chosen.0, active.0, kept.0, old.0]);
                    chosen
                }
            };
            b.function(op::STORE, &[pointer.0, chosen.0]);
        }
        if let Some(mask) = self.write_mask
            && self.unread_slots.contains(&slot)
        {
            let bool_type = self.bool_type;
            let stored = self.builder.id();
            self.builder
                .function(op::LOGICAL_NOT, &[bool_type.0, stored.0, outside.0]);
            let landed = self.builder.id();
            self.builder.function(
                op::LOGICAL_AND,
                &[bool_type.0, landed.0, inside.0, stored.0],
            );
            self.mark_written(mask, (buffer, index), (landed, active));
        }
        Ok(())
    }

    fn read_draw_buffer(&mut self, slot: u32, word_index: Id) -> Result<Id, TranslateError> {
        let Some((array, _)) = self.draw_buffers else {
            return Err(TranslateError::Unsupported {
                offset: 0,
                detail: "a draw buffer read in a module that declares none (D733)",
            });
        };
        // A primitive shader's buffer is a range of an arena its draw data names (D747).
        if self.stage == Stage::Mesh
            && let Some(place) = self.geometry_buffer_place(slot)
        {
            return Ok(self.read_arena(array, place, word_index));
        }
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
        // Another invocation may store this word at the same time - another lane run as its own
        // (D880), or another wave or group of a guest dispatch (D858) - so a lane that does not run
        // stores nothing rather than writing back what it read.
        if self.stores_beside_others() {
            self.store_if(active, pointer, value);
            return;
        }
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
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TextureSource {
    /// 0 for the first texture the module samples, 1 for a second, up to
    /// [`orbistoun_spirv::MAX_TEXTURES`].
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
    /// The stage's user-data word the sampler descriptor's four registers start at, when the
    /// program samples through them where the user data put them, never having loaded or written
    /// them. `None` for a sampler from a table, from anywhere else, or for a fetch.
    #[serde(default)]
    pub sampler_user_data: Option<u32>,
    /// The coordinates, across then down, the module's samples of this texture saturate (D743): a
    /// sampler clamping to half a border is honoured only by a module translated to.
    #[serde(default)]
    pub saturated: [bool; 2],
    /// Whether the module samples this texture as a cube, which the host binds as six faces seen
    /// through a cube view (D811).
    #[serde(default)]
    pub cube: bool,
    /// Whether the module samples this texture as a volume, which the host binds as a
    /// three-dimensional image of its slices (D823).
    #[serde(default)]
    pub volume: bool,
    /// Whether the module samples this texture as depth compared against a reference (D816).
    #[serde(default)]
    pub compare: bool,
    /// The stage's user-data word holding the level a store writes, counted from the descriptor's
    /// base level: an `image_store_mip` whose level the program copied from its user data. The
    /// host binds that level's own surface. `None` for a store to the level the view names.
    #[serde(default)]
    pub level_user_data: Option<u32>,
}

/// The table load that reached a descriptor's registers, where it is known: the table's base and
/// the byte offset in it.
type Reaching = Option<(Option<TableBase>, u32)>;

/// Why a shader reading one image as a cube and as a flat image is refused (D811).
const CUBE_AND_FLAT: &str = concat!(
    "this shader reads one image in two shapes - a cube and a flat image, or compared and not - ",
    "and a binding is one (D811, D816)"
);

/// SPIR-V's `Dim2D`, the dimensionality a flat texture binding is declared with.
const DIM_2D: u32 = 1;

/// SPIR-V's `Dim3D`: a volume binding, sampled at three coordinates (D823).
const DIM_3D: u32 = 2;

/// The SPIR-V dimensionality a sampled binding of `shape` is declared with, and how many float
/// components its sampling coordinate has.
const fn image_dim(shape: model::ImageShape) -> (u32, u32) {
    if shape.cube {
        (DIM_CUBE, 3)
    } else if shape.volume {
        (DIM_3D, 3)
    } else {
        (DIM_2D, 2)
    }
}

/// SPIR-V's `DimCube`: a cube binding, sampled at a direction of three floats (D811).
const DIM_CUBE: u32 = 3;

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
) -> (
    DescriptorLoads,
    DescriptorLoads,
    ReachingLoads,
    ReachingLoads,
) {
    let mut loads: DescriptorLoads = BTreeMap::new();
    let mut sampler_loads: DescriptorLoads = BTreeMap::new();
    // The descriptor load that last filled each register group on every path here, and the image
    // and sampler loads reaching each image instruction.
    let mut live = LiveLoads::new(decode, encodings);
    let mut reaching: ReachingLoads = BTreeMap::new();
    let mut sampler_reaching: ReachingLoads = BTreeMap::new();
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
        live.reach(instruction, name, (&mut reaching, &mut sampler_reaching));
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
                    offset @ (Operand::Immediate(_) | Operand::Named(_)),
                ],
            ) if offset_or_null(offset).is_some() => {
                let offset = offset_or_null(offset).unwrap_or_default();
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
                let width_of_one = match name {
                    "s_load_dwordx4" => model::SAMPLER_DESCRIPTOR_REGISTERS,
                    _ => model::IMAGE_DESCRIPTOR_REGISTERS,
                };
                if let Some(into) = into
                    && let Ok(offset) = u32::try_from(offset)
                {
                    let at = |register: u16| held.get(usize::from(register)).copied().flatten();
                    let table = at(*base)
                        .zip(at(base + 1))
                        .map(|(low, high)| TableBase { low, high });
                    for descriptor in 0..descriptors {
                        let load = (table, offset + descriptor * 32);
                        let group = u32::from(*destination) + descriptor * 8;
                        into.entry(group).or_default().insert(load);
                        live.loads.insert(group, (load, width_of_one));
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
    (loads, sampler_loads, reaching, sampler_reaching)
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

/// The first register of the sampler descriptor an image instruction names: its second scalar
/// operand. `None` for a fetch, which names none, and for any other instruction.
fn image_sampler(instruction: &Instruction, name: &str) -> Option<u32> {
    if !name.starts_with("image_") {
        return None;
    }
    instruction
        .operands
        .iter()
        .filter_map(|operand| match operand {
            Operand::Scalar(first) => Some(u32::from(*first)),
            _ => None,
        })
        .nth(1)
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

/// The descriptor loads that fill each register group on every path to the instruction being
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

    /// Records the image and sampler loads reaching `instruction`, where it is an image
    /// instruction and a load of the matching width last filled the group it names.
    fn reach(
        &self,
        instruction: &Instruction,
        name: &str,
        (images, samplers): (&mut ReachingLoads, &mut ReachingLoads),
    ) {
        let filled = |first: u32, width: u32| {
            self.loads
                .get(&first)
                .filter(|(_, filled)| *filled == width)
                .map(|(load, _)| *load)
        };
        if let Some(load) = image_descriptor(instruction, name)
            .and_then(|first| filled(first, model::IMAGE_DESCRIPTOR_REGISTERS))
        {
            images.insert(instruction.offset, load);
        }
        if let Some(load) = image_sampler(instruction, name)
            .and_then(|first| filled(first, model::SAMPLER_DESCRIPTOR_REGISTERS))
        {
            samplers.insert(instruction.offset, load);
        }
    }
}

/// The descriptor load filling each register group, by its first register, with how many registers
/// it filled: eight for an image descriptor, four for a sampler's.
type Loads = BTreeMap<u32, ((Option<TableBase>, u32), u32)>;

/// Whether `instruction` is a branch.
fn is_branch(instruction: &Instruction, encodings: &EncodingTable) -> bool {
    instruction
        .encoding
        .and_then(|i| encodings.encodings().get(usize::from(i)))
        .and_then(|e| encodings.mnemonic_for(&e.name, instruction.opcode))
        .is_some_and(|name| name == "s_branch" || name.starts_with("s_cbranch_"))
}

/// Forgets the descriptor loads whose register groups overlap `count` registers from `register`.
fn forget_live(live: &mut Loads, register: u16, count: u32) {
    let (first, end) = (u32::from(register), u32::from(register) + count);
    live.retain(|group, (_, width)| group + *width <= first || *group >= end);
}

/// The image descriptor load reaching each image instruction, by the instruction's offset: the one
/// that last filled its descriptor registers in program order, with no branch target between.
type ReachingLoads = BTreeMap<u32, (Option<TableBase>, u32)>;

/// Descriptor loads by the first register they fill: the table each was loaded from (when the
/// program formed its address from user data or constants) and the byte offset.
type DescriptorLoads = BTreeMap<u32, std::collections::BTreeSet<(Option<TableBase>, u32)>>;

/// A scalar load's offset operand: its immediate, or zero where it names only the null scalar
/// offset, as a load with neither an immediate nor a register offset is written
/// (`s_load_dwordx4 s[64:67], s[28:29], null`).
fn offset_or_null(operand: &Operand) -> Option<i64> {
    match operand {
        Operand::Immediate(offset) => Some(*offset),
        Operand::Named(name) if name == "null" => Some(0),
        _ => None,
    }
}

/// Every attribute the shader interpolates, in order and without repeats, with how it is read.
///
/// A pass of its own because fragment inputs are global variables declared in the header, before
/// any instruction is translated, and the `Flat` decoration belongs to the variable. An attribute
/// read both ways cannot carry one decoration, and the caller refuses it rather than picking.
fn interpolated_attributes(
    decode: &Decode,
    encodings: &EncodingTable,
    (twins, pixel_inputs): (Option<FlatTwins>, Option<PixelInputs>),
) -> Result<Vec<(u32, Interpolation)>, TranslateError> {
    let read = attribute_reads(decode, encodings);
    // Which interpolated attributes read the linear barycentrics, where the shader loads any.
    let linear = match pixel_inputs.filter(|inputs| inputs.reads_linear()) {
        Some(inputs) => linear_attributes(decode, encodings, inputs)?,
        None => Vec::new(),
    };
    let mut inputs = Vec::new();
    for (attribute, smooth, flat) in read {
        if smooth && !flat && linear.contains(&attribute) {
            inputs.push((attribute, Interpolation::NoPerspective));
            continue;
        }
        if smooth && flat && linear.contains(&attribute) {
            return Err(TranslateError::Unsupported {
                offset: 0,
                detail: "an attribute read both flat and without perspective is not translated",
            });
        }
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

/// The attributes a pixel shader interpolates with the linear barycentrics (`noperspective`): each
/// `v_interp_p1_f32` whose I register holds a `LINEAR_*` field at entry, as `inputs` lays them out.
///
/// # Errors
///
/// An interpolation whose I register an earlier instruction may have written, so which barycentrics
/// it holds is not known, and an attribute interpolated both ways.
fn linear_attributes(
    decode: &Decode,
    encodings: &EncodingTable,
    inputs: PixelInputs,
) -> Result<Vec<u32>, TranslateError> {
    let mut written = std::collections::BTreeSet::new();
    let mut kinds: BTreeMap<u32, pixel_inputs::Barycentric> = BTreeMap::new();
    for instruction in &decode.instructions {
        let Some((family, name)) = instruction
            .encoding
            .and_then(|i| encodings.encodings().get(usize::from(i)))
            .and_then(|e| {
                Some((
                    e.name.as_str(),
                    encodings.mnemonic_for(&e.name, instruction.opcode)?,
                ))
            })
        else {
            continue;
        };
        if name == "v_interp_p1_f32_e32" {
            let (Some(Operand::Vector(i)), Some(Operand::Immediate(attribute))) =
                (instruction.operands.get(1), instruction.operands.get(2))
            else {
                continue;
            };
            let i = u32::from(*i);
            let kind = (!written.contains(&i))
                .then(|| inputs.barycentric_of(i))
                .flatten()
                .ok_or(TranslateError::Unsupported {
                    offset: instruction.offset,
                    detail: concat!(
                        "an interpolation whose barycentric register is not one the hardware ",
                        "loads untouched, while the shader loads linear barycentrics, so whether ",
                        "it interpolates with perspective is not known"
                    ),
                })?;
            let attribute = u32::try_from(*attribute).unwrap_or(u32::MAX);
            if kinds
                .insert(attribute, kind)
                .is_some_and(|before| before != kind)
            {
                return Err(TranslateError::Unsupported {
                    offset: instruction.offset,
                    detail: "an attribute interpolated both with and without perspective",
                });
            }
        }
        // What a vector instruction writes, as far as its first operand tells: a store's first
        // operand is what it stores, and an export's is its target.
        let stores = name.contains("store") || name.contains("write") || family == "EXP";
        if !stores && let Some(Operand::Vector(register)) = instruction.operands.first() {
            written.insert(u32::from(*register));
        }
    }
    Ok(kinds
        .into_iter()
        .filter(|(_, kind)| *kind == pixel_inputs::Barycentric::Linear)
        .map(|(attribute, _)| attribute)
        .collect())
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

/// Whether a primitive shader exports a layer: any `exp pos1` enabling its third channel, where the
/// misc vector carries the render-target layer (D859). Such a module culls each primitive whose
/// layer is not the one its draw's pass renders.
#[must_use]
pub fn exports_layer(decode: &Decode, encodings: &EncodingTable) -> bool {
    decode.instructions.iter().any(|instruction| {
        let named = instruction
            .encoding
            .and_then(|i| encodings.encodings().get(usize::from(i)))
            .and_then(|e| encodings.mnemonic_for(&e.name, instruction.opcode));
        named == Some("exp")
            && instruction.operands.first() == Some(&Operand::Immediate(model::EXPORT_MISC))
            && instruction.word & model::EXPORT_LAYER_CHANNEL != 0
    })
}

/// Whether a primitive shader exports a point size: any `exp pos1` enabling its first channel,
/// where the misc vector carries it (D867).
#[must_use]
pub fn exports_point_size(decode: &Decode, encodings: &EncodingTable) -> bool {
    decode.instructions.iter().any(|instruction| {
        let named = instruction
            .encoding
            .and_then(|i| encodings.encodings().get(usize::from(i)))
            .and_then(|e| encodings.mnemonic_for(&e.name, instruction.opcode));
        named == Some("exp")
            && instruction.operands.first() == Some(&Operand::Immediate(model::EXPORT_MISC))
            && instruction.word & model::EXPORT_POINT_SIZE_CHANNEL != 0
    })
}

/// The local-data-share instructions a primitive shader run one invocation per lane may use (D760):
/// each followed by a workgroup barrier that keeps the wave's program order, its byte writes and
/// ors merged by atomics so lanes sharing a word keep each other's bits.
const PER_LANE_LOCAL: [&str; 7] = [
    "ds_read_b32",
    "ds_write_b32",
    "ds_read2_b32",
    "ds_write2_b32",
    "ds_read_u8",
    "ds_write_b8",
    "ds_or_b32",
];

/// Whether a primitive shader can run one invocation per lane (D760): it exchanges nothing between
/// lanes but through lane masks, lane reads and the local data share, which a subgroup ballot, a
/// shuffle and workgroup memory with barriers and atomics stand in for. A lane permute reads lanes
/// in patterns nothing here models, and keeps the whole wave in one invocation.
#[must_use]
pub fn runs_per_lane(decode: &Decode, encodings: &EncodingTable) -> bool {
    decode.instructions.iter().all(|instruction| {
        let Some(family) = instruction
            .encoding
            .and_then(|i| encodings.encodings().get(usize::from(i)))
        else {
            return false;
        };
        let name = encodings
            .mnemonic_for(&family.name, instruction.opcode)
            .unwrap_or("");
        // Local-data-share accesses barriered and merged as the per-lane model does them.
        let local = family.name == "DS";
        (!local || PER_LANE_LOCAL.contains(&name))
            && !["permlane64", "permlanex16", "swizzle", "bpermute", "_dpp"]
                .iter()
                .any(|part| name.contains(part))
    })
}

/// Whether a primitive shader exports the `v0` it was entered with as its primitive (D759): an
/// `exp prim` whose first source is `v0`, with nothing before it in the program writing `v0`.
///
/// Such a shader was compiled for passthrough. Mesa's NGG lowering exports the packed primitive it
/// was handed unchanged only in passthrough mode, and otherwise packs the export from the vertex
/// indices, which entry `v0` then holds sixteen bits each (`ac_nir_lower_ngg.c:127-131`, `:139-151`). Conservative: any instruction
/// naming `v0` as its first operand before the export counts as a write.
#[must_use]
pub fn exports_entry_primitive(decode: &Decode, encodings: &EncodingTable) -> bool {
    for instruction in &decode.instructions {
        let named = instruction
            .encoding
            .and_then(|i| encodings.encodings().get(usize::from(i)))
            .and_then(|e| encodings.mnemonic_for(&e.name, instruction.opcode));
        if named == Some("exp") {
            if instruction.operands.first() == Some(&Operand::Immediate(model::EXPORT_PRIMITIVE)) {
                return instruction.operands.get(1) == Some(&Operand::Vector(0));
            }
            continue;
        }
        if instruction.operands.first() == Some(&Operand::Vector(0)) {
            return false;
        }
    }
    false
}

/// Whether a pixel shader exports depth and a sample mask: an `exp mrtz` enabling Z, which needs a
/// `FragDepth` output declared before the entry point is, and one enabling the sample mask, which
/// needs a `SampleMask` one.
#[must_use]
pub fn exports_depth(decode: &Decode, encodings: &EncodingTable) -> (bool, bool) {
    let enabled = decode
        .instructions
        .iter()
        .filter(|instruction| {
            let named = instruction
                .encoding
                .and_then(|i| encodings.encodings().get(usize::from(i)))
                .and_then(|e| encodings.mnemonic_for(&e.name, instruction.opcode));
            named == Some("exp")
                && instruction.operands.first() == Some(&Operand::Immediate(model::EXPORT_MRTZ))
        })
        .fold(0, |enabled, instruction| enabled | instruction.word);
    (enabled & 1 != 0, enabled & 4 != 0)
}

/// What a compute program shares between the waves of its group: whether any instruction is
/// `s_barrier`, and whether any is a local data share access (`ds_*`, the GDS forms aside).
#[must_use]
pub fn group_program(decode: &Decode, encodings: &EncodingTable) -> compute_inputs::GroupProgram {
    let mut program = compute_inputs::GroupProgram::default();
    for instruction in &decode.instructions {
        let named = instruction
            .encoding
            .and_then(|i| encodings.encodings().get(usize::from(i)))
            .and_then(|e| encodings.mnemonic_for(&e.name, instruction.opcode));
        match named {
            Some("s_barrier") => program.barrier = true,
            Some(name) if name.starts_with("ds_") && instruction.word & model::DS_GDS == 0 => {
                program.local_share = true;
            }
            _ => {}
        }
    }
    program
}

/// Whether a shader reads its quad's other lanes: any DPP16 form, which needs the quad
/// capability and the subgroup index declared before the entry point is.
#[must_use]
pub fn reads_quad(decode: &Decode, encodings: &EncodingTable) -> bool {
    decode
        .instructions
        .iter()
        .any(|instruction| model::dpp_word(encodings, instruction).is_some())
}

/// The channels of colour attachment zero a pixel shader's exports write, red in bit 0 to alpha in
/// bit 3: the union of every `exp mrt0`'s `EN` field (`aco_assembler.cpp:1005`). A channel outside
/// it holds whatever the output held, so a draw that writes it to its target is not the guest's.
#[must_use]
pub fn exported_colour_channels(decode: &Decode, encodings: &EncodingTable) -> u8 {
    exported_target_channels(decode, encodings)[0]
}

/// [`exported_colour_channels`] for each colour target, `mrt0` to `mrt7` by number.
#[must_use]
pub fn exported_target_channels(decode: &Decode, encodings: &EncodingTable) -> [u8; 8] {
    let mut channels = [0; 8];
    for instruction in &decode.instructions {
        let named = instruction
            .encoding
            .and_then(|i| encodings.encodings().get(usize::from(i)))
            .and_then(|e| encodings.mnemonic_for(&e.name, instruction.opcode));
        if named == Some("exp")
            && let Some(Operand::Immediate(target)) = instruction.operands.first()
            && let Some(slot) = usize::try_from(*target)
                .ok()
                .and_then(|target| channels.get_mut(target))
        {
            *slot |= (instruction.word & 0xF) as u8;
        }
    }
    channels
}

/// The colour targets a pixel shader exports to, bit `K` for `mrtK`: every `exp` that names one,
/// whatever channels it enables.
#[must_use]
pub fn exported_colour_targets(decode: &Decode, encodings: &EncodingTable) -> u8 {
    let mut targets = 0;
    for instruction in &decode.instructions {
        let named = instruction
            .encoding
            .and_then(|i| encodings.encodings().get(usize::from(i)))
            .and_then(|e| encodings.mnemonic_for(&e.name, instruction.opcode));
        if named == Some("exp")
            && let Some(Operand::Immediate(target @ 0..=7)) = instruction.operands.first()
        {
            targets |= 1 << target;
        }
    }
    targets
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
    if !user_data.draw_buffers {
        return Ok(crate::draw_buffers::DrawBuffers::default());
    }
    // A dispatch's buffers are bound writable and written back, so its stores are traced too
    // (D746); a draw's are snapshots.
    let buffers = crate::draw_buffers::trace(
        decode,
        encodings,
        (user_data.first_register, user_data.count),
        (stage == Stage::Compute, stage == Stage::Mesh),
    );
    if buffers.sources.len() > orbistoun_spirv::DRAW_BUFFERS_PER_STAGE as usize {
        return Err(TranslateError::TooManyBuffers {
            traced: buffers.sources.len(),
            most: orbistoun_spirv::DRAW_BUFFERS_PER_STAGE,
        });
    }
    let mut buffers = buffers;
    // A computed access's prefix may read the program's own address (D842).
    for source in &mut buffers.sources {
        if let crate::draw_buffers::BufferSource::Computed(computed) = source {
            computed.program_address = user_data.program_address;
        }
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
    let masks = dispatch_masks(decode, encodings, (stage, width), &user_data)?;
    // A primitive shader that exports its entry `v0` was compiled for passthrough, whatever the
    // stream wrote (D759).
    let mut user_data = user_data;
    if stage == Stage::Mesh
        && let Some(geometry) = user_data.geometry.as_mut()
        && !geometry.passthrough
        && exports_entry_primitive(decode, encodings)
    {
        geometry.passthrough = true;
    }
    if let (Stage::Mesh, Some(geometry)) = (stage, user_data.geometry)
        && let Some(detail) = geometry.refusal(width.lanes())
    {
        return Err(TranslateError::Unsupported { offset: 0, detail });
    }
    let attributes = interpolated_attributes(
        decode,
        encodings,
        (user_data.flat_twins, user_data.pixel_inputs),
    )?;
    if stage == Stage::Fragment {
        check_input_controls(&attributes, &user_data)?;
    }
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
        (
            window,
            user_data,
            &buffers,
            (
                exports_depth(decode, encodings),
                reads_quad(decode, encodings),
                exported_colour_targets(decode, encodings),
                (
                    stage == Stage::Mesh
                        && model::misc_vector_present(user_data.position_vectors)
                        && exports_layer(decode, encodings),
                    // Every point has a size, its vertex's or the draw's (D867).
                    stage == Stage::Mesh
                        && (primitive == MeshPrimitive::Points
                            || model::point_size_read(user_data.position_vectors)
                                && exports_point_size(decode, encodings)),
                ),
            ),
            masks,
        ),
    );
    module.descriptor_loads = descriptor_table_loads(decode, encodings, user_data.first_register);
    crate::control::emit(&mut module, decode, encodings)?;
    let sources = module.image_sources();
    let (words, translated) = module.finish()?;
    Ok((words, translated, sources))
}

/// A guest dispatch's entry state checked against its program, and - run one invocation per lane on
/// the subgroup its `per_invocation` gives - how it holds its masks (D880, D884). `None` for any
/// other module, and for a dispatch that keeps the whole wave in one invocation.
fn dispatch_masks(
    decode: &Decode,
    encodings: &EncodingTable,
    (stage, width): (Stage, Width),
    user_data: &UserData,
) -> Result<Option<LaneMasks>, TranslateError> {
    let (Stage::Compute, Some(inputs)) = (stage, user_data.compute) else {
        return Ok(None);
    };
    inputs.check()?;
    let Some(subgroup) = user_data.per_invocation else {
        return Ok(None);
    };
    inputs.refuse_meeting_waves(width.lanes(), group_program(decode, encodings))?;
    // One invocation per lane is exact only for a program that never meets its wave (D880), or
    // meets it only through masks its lanes can exchange (D884).
    let needs = crate::lane_exact::lane_needs(decode, encodings, width.lanes());
    let held = crate::lane_exact::lane_masks(
        &needs,
        width.lanes(),
        Some(subgroup).filter(|&lanes| lanes != 0),
        inputs.waves(width.lanes()),
    );
    held.map(Some)
        .map_err(|detail| TranslateError::Unsupported {
            // The need that refused it: any but a whole mask before one.
            offset: needs
                .needs
                .iter()
                .find(|(need, _, _)| *need != crate::lane_exact::LaneNeed::WholeMask)
                .or_else(|| needs.needs.first())
                .map_or(0, |&(_, offset, _)| offset),
            detail,
        })
}

#[cfg(test)]
mod per_lane_tests {
    use orbistoun_shader::{EncodingTable, OperandTable, decode_program};

    fn per_lane(words: &[u32]) -> bool {
        let encodings = EncodingTable::builtin().expect("encodings");
        let operands = OperandTable::builtin().expect("operands");
        let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        let decode = decode_program(&bytes, &encodings, &operands);
        assert!(decode.is_trustworthy(), "the fixture decodes cleanly");
        super::runs_per_lane(&decode, &encodings)
    }

    /// A fetch shader's buffers are each traced to a slot of their own, and nineteen fit one
    /// stage: PPSA03416's vertex shader, past its title screen, forms seventeen descriptors by
    /// computation beside two it reads from user data (D733). Seventeen loads, each with a scalar
    /// offset and so each found per draw, are seventeen slots and bind.
    #[test]
    fn a_fetch_shader_s_computed_buffers_fit_a_stage() {
        const END: u32 = 0xbf81_0000;
        let encodings = EncodingTable::builtin().expect("encodings");
        let operands = OperandTable::builtin().expect("operands");
        let mut words = Vec::new();
        for _ in 0..17 {
            // buffer_load_format_x v4, v4, s[12:15], s11 idxen
            words.extend([0xe000_2000_u32, 0x0b03_0404]);
        }
        words.push(END);
        let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        let decode = decode_program(&bytes, &encodings, &operands);
        assert!(decode.is_trustworthy(), "the fixture decodes cleanly");
        let buffers = super::draw_buffers_for(
            &decode,
            &encodings,
            super::Stage::Mesh,
            super::UserData {
                count: 16,
                draw_buffers: true,
                ..super::UserData::default()
            },
        )
        .expect("seventeen buffers bind");
        assert_eq!(buffers.sources.len(), 17);
    }

    /// A program whose lanes exchange only through masks, lane reads and the local data share
    /// runs one invocation per lane (D760); a byte write to the share does too, merged by atomics.
    #[test]
    fn local_share_writes_run_per_lane() {
        const END: u32 = 0xbf81_0000;
        // v_mov_b32 v1, 0
        assert!(per_lane(&[0x7e02_0280, END]));
        // ds_write_b32 v0, v1; ds_write_b8 v0, v1
        assert!(per_lane(&[0xd834_0000, 0x0000_0100, END]));
        assert!(per_lane(&[0xd878_0000, 0x0000_0100, END]));
    }
}

#[cfg(test)]
mod local_share_tests {
    use super::{MeshPrimitive, Stage, UserData, Window};
    use crate::{Fidelity, Strategy, Width};
    use orbistoun_shader::{EncodingTable, OperandTable, decode_program};

    /// The words of the workgroup array a module declares for its local data share: the length of
    /// the array its one `Workgroup` variable points at.
    fn declared_words(local_words: Option<u32>) -> u32 {
        const END: u32 = 0xbf81_0000;
        let encodings = EncodingTable::builtin().expect("encodings");
        let operands = OperandTable::builtin().expect("operands");
        let decode = decode_program(&END.to_le_bytes(), &encodings, &operands);
        let module = crate::translate_with_user_data(
            &decode,
            &encodings,
            Strategy::Predicated {
                fidelity: Fidelity::Wavefront,
                width: Width::default(),
            },
            (Stage::Compute, MeshPrimitive::default()),
            Window::default(),
            UserData {
                local_words,
                ..UserData::default()
            },
        )
        .expect("translates")
        .module;
        // Instructions after the five-word header: the opcode in the low half of each first word,
        // the word count in the high half.
        let mut instructions = Vec::new();
        let mut at = 5;
        while at < module.len() {
            let count = (module[at] >> 16) as usize;
            instructions.push((module[at] & 0xFFFF, &module[at + 1..at + count]));
            at += count;
        }
        let find = |opcode: u32, result: Option<u32>, at: usize| {
            instructions
                .iter()
                .find(|(op, operands)| {
                    *op == opcode && result.is_none_or(|id| operands.get(at) == Some(&id))
                })
                .map(|(_, operands)| *operands)
                .expect("declared")
        };
        let (variable, pointer, array, constant) = (
            u32::from(orbistoun_spirv::op::VARIABLE),
            u32::from(orbistoun_spirv::op::TYPE_POINTER),
            u32::from(orbistoun_spirv::op::TYPE_ARRAY),
            u32::from(orbistoun_spirv::op::CONSTANT),
        );
        let shared = instructions
            .iter()
            .find(|(op, operands)| *op == variable && operands.get(2) == Some(&super::WORKGROUP))
            .map(|(_, operands)| operands[0])
            .expect("a workgroup variable");
        let pointed_at = find(pointer, Some(shared), 0)[2];
        let length = find(array, Some(pointed_at), 0)[2];
        find(constant, Some(length), 1)[2]
    }

    /// The local share is as large as the stage declares: radeonsi's culling primitive shader keeps
    /// 28 bytes a vertex, 1792 for a wave of 64, past the 1 KiB a module provides when nothing
    /// declares a size, and a write past an array's end is undefined.
    #[test]
    fn the_local_share_is_as_large_as_the_stage_declares() {
        assert_eq!(declared_words(Some(512)), 512);
        assert_eq!(declared_words(None), super::LOCAL_WORDS);
    }
}

#[cfg(test)]
mod passthrough_tests {
    use orbistoun_shader::{EncodingTable, OperandTable, decode_program};

    fn exports(words: &[u32]) -> bool {
        let encodings = EncodingTable::builtin().expect("encodings");
        let operands = OperandTable::builtin().expect("operands");
        let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        let decode = decode_program(&bytes, &encodings, &operands);
        assert!(decode.is_trustworthy(), "the fixture decodes cleanly");
        super::exports_entry_primitive(&decode, &encodings)
    }

    /// `exp prim v0, off, off, off done` straight from entry is a passthrough shader's export; one
    /// after `v0` is rewritten, or of another register, is not (D759).
    #[test]
    fn only_an_export_of_entry_v0_is_passthrough() {
        const EXP_PRIM_V0: [u32; 2] = [0xf800_0941, 0x0000_0000];
        const EXP_PRIM_V1: [u32; 2] = [0xf800_0941, 0x0000_0001];
        const MOV_V0_ZERO: u32 = 0x7e00_0280; // v_mov_b32 v0, 0
        const MOV_V1_ZERO: u32 = 0x7e02_0280; // v_mov_b32 v1, 0
        const END: u32 = 0xbf81_0000;
        assert!(exports(&[MOV_V1_ZERO, EXP_PRIM_V0[0], EXP_PRIM_V0[1], END]));
        assert!(!exports(&[
            MOV_V0_ZERO,
            EXP_PRIM_V0[0],
            EXP_PRIM_V0[1],
            END
        ]));
        assert!(!exports(&[EXP_PRIM_V1[0], EXP_PRIM_V1[1], END]));
        assert!(!exports(&[END]));
    }
}

#[cfg(test)]
mod unwritten_user_data_tests {
    /// An unwritten user-data word is refused only when the program reads it before writing its
    /// register: radeonsi's buffer copy saves the execution mask into `s[0:1]` and reads it back,
    /// and STKT00001's stream never wrote word one.
    #[test]
    fn a_word_the_program_writes_first_is_not_unwritten() {
        use super::{
            ComputeInputs, MeshPrimitive, Stage, UserData, Window, translate_with_user_data,
        };
        use crate::Width;
        use orbistoun_shader::{EncodingTable, OperandTable, decode};
        let encodings = EncodingTable::builtin().expect("encodings");
        let operands = OperandTable::builtin().expect("operands");
        let translate = |words: &[u32]| {
            let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
            let decoded = decode(&bytes, &encodings, &operands);
            translate_with_user_data(
                &decoded,
                &encodings,
                Width::Wave64,
                (Stage::Compute, MeshPrimitive::default()),
                Window::default(),
                UserData {
                    first_register: 0,
                    count: 4,
                    compute: Some(ComputeInputs {
                        threads: [64, 1, 1],
                        thread_id_components: 1,
                        unwritten_user_data: 1 << 1,
                        ..ComputeInputs::default()
                    }),
                    ..UserData::default()
                },
            )
        };
        // s_mov_b32 s1, 0; v_mov_b32 v0, s1; s_endpgm.
        assert!(translate(&[0xbe81_0380, 0x7e00_0201, 0xbf81_0000]).is_ok());
        // v_mov_b32 v0, s1; s_endpgm: the word as the stream left it.
        assert!(translate(&[0x7e00_0201, 0xbf81_0000]).is_err());
    }
}

#[cfg(test)]
mod exact_memory_tests {
    /// A guest dispatch's access to guest memory that is not yet checked against its window is
    /// refused naming the instruction, so a report says which access a program needs: Grand Theft
    /// Auto V's first sixteen-wave dispatch was refused at its offset alone.
    #[test]
    fn an_unchecked_access_is_refused_by_name() {
        use super::{
            ComputeInputs, MeshPrimitive, Stage, UserData, Window, translate_with_user_data,
        };
        use crate::Width;
        use orbistoun_shader::{EncodingTable, OperandTable, decode};
        let encodings = EncodingTable::builtin().expect("encodings");
        let operands = OperandTable::builtin().expect("operands");
        let (family, opcode) = encodings
            .find_by_name("global_load_dword")
            .expect("global_load_dword");
        let encoding = encodings
            .encodings()
            .iter()
            .find(|encoding| encoding.name == family)
            .expect("its family");
        // global_load_dword v1, v[0:1], off; s_endpgm.
        let words = [
            encoding.value | (opcode << encoding.opcode.shift),
            (0x7d << 16) | (1 << 24),
            0xbf81_0000,
        ];
        let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        let decoded = decode(&bytes, &encodings, &operands);
        let refused = translate_with_user_data(
            &decoded,
            &encodings,
            Width::Wave64,
            (Stage::Compute, MeshPrimitive::default()),
            Window::spanning_address(0x4_0000_0000, 1024).expect("a window"),
            UserData {
                compute: Some(ComputeInputs {
                    threads: [64, 1, 1],
                    thread_id_components: 1,
                    ..ComputeInputs::default()
                }),
                ..UserData::default()
            },
        )
        .expect_err("refused")
        .to_string();
        assert!(
            refused.contains("(global_load_dword)") && refused.contains("checked against"),
            "{refused}"
        );
    }

    /// A dispatch's global load with no scalar base reads the buffer its lanes' addresses span
    /// (D883), so it is placed exactly and translates: Grand Theft Auto V's compute program's
    /// `global_load_ubyte v0, v[1:2], off` (0xdc200000 0x007d0001).
    #[test]
    fn a_traced_global_load_in_a_dispatch_translates() {
        use super::{
            ComputeInputs, MeshPrimitive, Stage, UserData, Window, translate_with_user_data,
        };
        use crate::Width;
        use orbistoun_shader::{EncodingTable, OperandTable, decode};
        let encodings = EncodingTable::builtin().expect("encodings");
        let operands = OperandTable::builtin().expect("operands");
        let words = [0xdc20_0000_u32, 0x007d_0001, 0xbf81_0000];
        let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        let decoded = decode(&bytes, &encodings, &operands);
        let translated = translate_with_user_data(
            &decoded,
            &encodings,
            Width::Wave64,
            (Stage::Compute, MeshPrimitive::default()),
            Window::spanning_address(0x4_0000_0000, 1024).expect("a window"),
            UserData {
                compute: Some(ComputeInputs {
                    threads: [64, 1, 1],
                    thread_id_components: 1,
                    ..ComputeInputs::default()
                }),
                // Two words of user data, which a dispatch's push block carries.
                count: 2,
                draw_buffers: true,
                ..UserData::default()
            },
        );
        assert!(translated.is_ok(), "{:?}", translated.err());
    }

    /// A scalar buffer load that reads through no traced buffer, because the program also stores
    /// some other way, is refused naming that store: the tracer traces nothing then, and the load
    /// alone does not say why.
    #[test]
    fn an_untraced_load_names_the_store_that_stopped_the_trace() {
        use super::{
            ComputeInputs, MeshPrimitive, Stage, UserData, Window, translate_with_user_data,
        };
        use crate::Width;
        use orbistoun_shader::{EncodingTable, OperandTable, decode};
        let encodings = EncodingTable::builtin().expect("encodings");
        let operands = OperandTable::builtin().expect("operands");
        let (family, opcode) = encodings
            .find_by_name("s_buffer_load_dword")
            .expect("s_buffer_load_dword");
        let encoding = encodings
            .encodings()
            .iter()
            .find(|encoding| encoding.name == family)
            .expect("its family");
        // s_buffer_load_dword s4, s[0:3], 0 (no offset register); global_store_dword v[0:1], v2,
        // off; s_endpgm.
        let words = [
            encoding.value | (opcode << encoding.opcode.shift) | (4 << 6),
            125 << 25,
            0xdc70_8000,
            0x007d_0200,
            0xbf81_0000,
        ];
        let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        let decoded = decode(&bytes, &encodings, &operands);
        let refused = translate_with_user_data(
            &decoded,
            &encodings,
            Width::Wave64,
            (Stage::Compute, MeshPrimitive::default()),
            Window::spanning_address(0x4_0000_0000, 1024).expect("a window"),
            UserData {
                count: 4,
                draw_buffers: true,
                compute: Some(ComputeInputs {
                    threads: [64, 1, 1],
                    thread_id_components: 1,
                    ..ComputeInputs::default()
                }),
                ..UserData::default()
            },
        )
        .expect_err("refused")
        .to_string();
        assert!(
            refused.contains("(s_buffer_load_dword)")
                && refused.contains("global_store_dword at 0x8"),
            "{refused}"
        );
    }

    /// `image_store v[2:5], v[0:2], s[0:7] dmask:0xf` of the dimensionality whose code is
    /// `dimension`, translated as a compute dispatch whose user data holds the descriptor.
    fn store_of_dimension(
        dimension: u32,
    ) -> Result<(Vec<u32>, usize, super::ImageSources), crate::TranslateError> {
        use super::{
            ComputeInputs, MeshPrimitive, Stage, UserData, Window, translate_with_user_data,
        };
        use crate::Width;
        use orbistoun_shader::{EncodingTable, OperandTable, decode};
        let encodings = EncodingTable::builtin().expect("encodings");
        let operands = OperandTable::builtin().expect("operands");
        let (family, opcode) = encodings.find_by_name("image_store").expect("image_store");
        let encoding = encodings
            .encodings()
            .iter()
            .find(|encoding| encoding.name == family)
            .expect("its family");
        // The store; s_endpgm.
        let words = [
            encoding.value | (opcode << encoding.opcode.shift) | (0xf << 8) | (dimension << 3),
            2 << 8,
            0xbf81_0000,
        ];
        let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        let decoded = decode(&bytes, &encodings, &operands);
        translate_with_user_data(
            &decoded,
            &encodings,
            Width::Wave64,
            (Stage::Compute, MeshPrimitive::default()),
            Window::spanning_address(0x4_0000_0000, 1024).expect("a window"),
            UserData {
                count: 8,
                compute: Some(ComputeInputs {
                    threads: [64, 1, 1],
                    thread_id_components: 1,
                    ..ComputeInputs::default()
                }),
                ..UserData::default()
            },
        )
    }

    /// `image_store_mip v[0:3], v[4:6], s[0:7] dmask:0xf dim:2D` after `v_mov_b32 v6, <level>`,
    /// translated as a compute dispatch whose user data holds the descriptor and, at word 8, a
    /// level: the shape of PPSA04263's copy into a level, `v_mov_b32 v6, s6` before its store.
    fn store_to_a_level(
        level: u32,
    ) -> Result<(Vec<u32>, usize, super::ImageSources), crate::TranslateError> {
        use super::{
            ComputeInputs, MeshPrimitive, Stage, UserData, Window, translate_with_user_data,
        };
        use crate::Width;
        use orbistoun_shader::{EncodingTable, OperandTable, decode};
        let encodings = EncodingTable::builtin().expect("encodings");
        let operands = OperandTable::builtin().expect("operands");
        let (family, opcode) = encodings
            .find_by_name("image_store_mip")
            .expect("image_store_mip");
        let encoding = encodings
            .encodings()
            .iter()
            .find(|encoding| encoding.name == family)
            .expect("its family");
        // v_mov_b32 v6, <level>; the store to the 2D image (dimension code 1); s_endpgm.
        let words = [
            0x7e0c_0200 | level,
            encoding.value | (opcode << encoding.opcode.shift) | (0xf << 8) | (1 << 3),
            4,
            0xbf81_0000,
        ];
        let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        let decoded = decode(&bytes, &encodings, &operands);
        translate_with_user_data(
            &decoded,
            &encodings,
            Width::Wave64,
            (Stage::Compute, MeshPrimitive::default()),
            Window::spanning_address(0x4_0000_0000, 1024).expect("a window"),
            UserData {
                count: 9,
                compute: Some(ComputeInputs {
                    threads: [64, 1, 1],
                    thread_id_components: 1,
                    ..ComputeInputs::default()
                }),
                ..UserData::default()
            },
        )
    }

    /// A store to a level its user data names binds that level: the stored image's source says
    /// which word holds it, so the host binds the level's own surface. A level the program
    /// computes is refused by name rather than written to the wrong level.
    #[test]
    fn a_store_to_a_level_names_the_word_holding_it() {
        let (_, _, (_, stored)) = store_to_a_level(8).expect("a store to a level from s8");
        assert_eq!(stored.and_then(|source| source.level_user_data), Some(8));
        // v_mov_b32 v6, 1 (inline integer code 129): a level no user data holds.
        let refused = store_to_a_level(129).expect_err("refused").to_string();
        assert!(refused.contains("level"), "{refused}");
    }

    /// An image access of a dimensionality not translated is refused naming it, so the report
    /// says which image a program needs: a store to a 1D array, code 4.
    #[test]
    fn an_image_of_another_dimension_is_refused_by_its_name() {
        let refused = store_of_dimension(4).expect_err("refused").to_string();
        assert!(
            refused.contains("(image_store) reaches a 1D array image"),
            "{refused}"
        );
    }

    /// A dispatch's store to a three-dimensional image writes through three coordinates to a
    /// three-dimensional storage image, whose source says it is a volume so the host binds one:
    /// Grand Theft Auto V's first sixteen-wave dispatch stores so.
    #[test]
    fn a_dispatch_stores_to_a_volume() {
        let (module, _, (_, stored)) = store_of_dimension(2).expect("a store to a volume");
        assert!(stored.is_some_and(|source| source.volume), "{stored:?}");
        // `OpTypeImage` (opcode 0x19, nine words): result, sampled type, then `Dim`, 2 for 3D.
        let dims: Vec<u32> = module
            .windows(4)
            .filter(|w| w[0] == (9 << 16) | 0x19)
            .map(|w| w[3])
            .collect();
        assert_eq!(dims, [2], "one storage image, three-dimensional");
    }
}

#[cfg(test)]
mod wave32_mask_tests {
    /// In a thirty-two-lane wave a lane mask is one scalar register: `v_cndmask_b32_e64`'s mask
    /// `s[6:7]` reads `s6` alone, as the compare that wrote it wrote `s6` alone. Reading `s7` too
    /// took it for one of the geometry engine's system registers read before it was written, and
    /// refused CRFT00001's primitive shader (its instruction at 392, as ACO compiled it).
    #[test]
    fn a_wave32_lane_mask_is_one_register() {
        use super::{
            Assembly, GeometryInputs, MeshPrimitive, Stage, UserData, Window,
            translate_with_user_data,
        };
        use crate::Width;
        use orbistoun_shader::{EncodingTable, OperandTable, decode};
        let encodings = EncodingTable::builtin().expect("encodings");
        let operands = OperandTable::builtin().expect("operands");
        // v_cmp_ne_i32_e64 s6, v0, v1; v_cndmask_b32_e64 v11, v11, 1.0, s[6:7]; s_endpgm.
        let words: [u32; 5] = [
            0xd485_0006,
            0x0002_0300,
            0xd501_000b,
            0x0019_e50b,
            0xbf81_0000,
        ];
        let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        let decoded = decode(&bytes, &encodings, &operands);
        let names: Vec<String> = decoded
            .instructions
            .iter()
            .map(|i| crate::instruction_name(i, &encodings))
            .collect();
        assert_eq!(
            names,
            ["v_cmp_ne_i32_e64", "v_cndmask_b32_e64", "s_endpgm"],
            "the hand-assembled words"
        );
        let user_data = UserData {
            first_register: 8,
            geometry: Some(GeometryInputs {
                first_vertex: 0,
                vertices: 3,
                primitives: 1,
                assembly: Assembly::List,
                indices: None,
                passthrough: false,
                legacy: false,
                streamout: false,
            }),
            ..UserData::default()
        };
        let translated = translate_with_user_data(
            &decoded,
            &encodings,
            Width::Wave32,
            (Stage::Mesh, MeshPrimitive::default()),
            Window::default(),
            user_data,
        );
        assert!(translated.is_ok(), "{:?}", translated.err());
    }
}

#[cfg(test)]
mod assembly_tests {
    /// A quad list's quad is two triangles, split as Mesa splits one for each provoking convention
    /// (D798): `(0, 1, 3)`, `(1, 2, 3)` with the last vertex provoking, `(0, 1, 2)`, `(0, 2, 3)`
    /// with the first; the next quad starts four vertices on.
    #[test]
    fn a_quad_is_two_triangles_split_for_its_provoking_vertex() {
        use super::Assembly;
        let last = Assembly::Quads {
            provoking_last: true,
        };
        let first = Assembly::Quads {
            provoking_last: false,
        };
        assert_eq!(last.vertices_of(0), [0, 1, 3]);
        assert_eq!(last.vertices_of(1), [1, 2, 3]);
        assert_eq!(first.vertices_of(0), [0, 1, 2]);
        assert_eq!(first.vertices_of(1), [0, 2, 3]);
        assert_eq!(last.vertices_of(2), [4, 5, 7], "the second quad");
        assert_eq!(first.vertices_for(6), 12, "three quads");
    }

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
        assert_eq!(Assembly::Points.vertices_of(5), [5, 0, 0]);
        assert_eq!(Assembly::Points.vertices_for(4), 4);
        assert_eq!(Assembly::Points.primitives_from(4), 4);
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

#[cfg(test)]
mod capture_tests {
    /// A mesh module translated to capture its positions stores into the observation buffer, a store
    /// a component for each lane it models, where one translated without does not: the store is the capture, and without it a draw's positions
    /// read back as zeros (GTA V's title, every draw).
    #[test]
    fn capturing_positions_adds_the_observation_stores() {
        use super::{
            Assembly, GeometryInputs, MeshPrimitive, Stage, UserData, Window,
            translate_with_user_data,
        };
        use crate::Width;
        use orbistoun_shader::{EncodingTable, OperandTable, decode};
        let encodings = EncodingTable::builtin().expect("encodings");
        let operands = OperandTable::builtin().expect("operands");
        // exp pos0 v0, v1, v2, v3 done; s_endpgm.
        let words: [u32; 3] = [0xf800_08cf, 0x0302_0100, 0xbf81_0000];
        let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        let decoded = decode(&bytes, &encodings, &operands);
        let translate = |capture_positions| {
            let user_data = UserData {
                first_register: 8,
                geometry: Some(GeometryInputs {
                    first_vertex: 0,
                    vertices: 3,
                    primitives: 1,
                    assembly: Assembly::List,
                    indices: None,
                    passthrough: false,
                    legacy: false,
                    streamout: false,
                }),
                capture_positions,
                ..UserData::default()
            };
            translate_with_user_data(
                &decoded,
                &encodings,
                Width::Wave32,
                (Stage::Mesh, MeshPrimitive::default()),
                Window::default(),
                user_data,
            )
            .expect("translates")
            .0
        };
        // `OpStore`, opcode 62 (0x3e), three words.
        let stores = |module: &[u32]| module.iter().filter(|&&w| w == (3 << 16) | 0x3e).count();
        let (without, with) = (translate(false), translate(true));
        assert!(stores(&with) > stores(&without), "the capture's stores");
    }
}
