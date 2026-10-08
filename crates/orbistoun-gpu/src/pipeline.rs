//! From a submitted command buffer to shaders ready to run.
//!
//! The guest builds a command buffer, writes shader addresses into registers and submits it; that
//! submission and guest memory are the only input (D112). This walks the packets, finds the shader
//! addresses, fetches each shader from guest memory through [`GuestMemory`], translates it, and
//! emits backend commands. A register holds a GPU address, read as a guest address through
//! [`guest_address_of`]. Shaders are cached by content, since a guest may move, replace or alias
//! one. A [`RegisteredShader`] is believed over an address inferred from register writes where both
//! exist, and the report counts agreement and disagreement. A shader that does not translate is
//! reported with its address and reason, and the command using it is omitted.

use std::collections::BTreeMap;

use orbistoun_shader::{
    Capture, EncodingTable, Operand, OperandTable, ShaderCorpus, ShaderError, decode_program,
};
use orbistoun_translate::wavefront::MeshPrimitive;
use orbistoun_translate::wavefront::Stage;
use orbistoun_translate::wavefront::Window;
use orbistoun_translate::wavefront::{
    Assembly, GeometryInputs, TableBase, TableWord, TextureSource, USER_DATA_STAGE_WORDS, UserData,
    WINDOW_SPACE_SCALE,
};
use orbistoun_translate::{Strategy, Width, translate_with_user_data};

use orbistoun_translate::wavefront::FlatTwins;

use crate::backend::{IndexBuffer, Rect, RenderCommand, ResourceId, ShaderStage, USER_DATA_WORDS};
use crate::packet::{PacketWalk, walk};
use crate::registers::{
    BlendControl, ColourTarget, ColourTargetExtent, ColourTargetFormat, DepthControl, DrawCall,
    DrawKind, ImageDescriptor, PrimitiveTopology, RegisterWrite, StencilControl, SwizzleMode,
    ViewportTransform, Vocabulary, WaveWidths, blend_control_at, colour_swizzle_mode_at,
    colour_target_at, colour_target_bases_in, colour_target_dcc_at, colour_target_extent_at,
    colour_target_format_at, decode_blend_control, decode_image_descriptor, depth_control_at,
    dispatch_calls, primitive_topology_at, register_writes, register_writes_reading,
    shader_candidates, stencil_control_at, viewport_transform_from,
};

/// Which queue a command buffer was submitted to.
///
/// The guest's two queues take different work, so a vertex shader in a compute submission is a
/// decode error rather than an unusual frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Queue {
    /// Drawing.
    Draw,
    /// Compute dispatch.
    Compute,
}

impl Queue {
    /// The shader stages a submission to this queue may bind.
    ///
    /// Used to report a stage that cannot belong rather than to drop it: it means the register
    /// vocabulary misidentified something.
    pub const fn permits(self, stage: ShaderStage) -> bool {
        match self {
            Self::Draw => matches!(stage, ShaderStage::Vertex | ShaderStage::Fragment),
            Self::Compute => matches!(stage, ShaderStage::Compute),
        }
    }
}

/// The host stage a shader named by the register vocabulary is translated for.
///
/// A shader bound to the vertex slot is a primitive shader that emits primitives and their
/// vertices, so it translates to a mesh stage (D688). The input is the typed stage the submission
/// reconciled, not the vocabulary's string.
const fn host_stage(stage: ShaderStage) -> Stage {
    match stage {
        ShaderStage::Fragment => Stage::Fragment,
        ShaderStage::Vertex => Stage::Mesh,
        ShaderStage::Compute => Stage::Compute,
    }
}

/// Distinguishes the same shader translated for different stages, in the cache key.
///
/// An arbitrary constant per stage, stable across runs so reports diff cleanly.
const fn stage_salt(stage: Stage) -> u64 {
    match stage {
        Stage::Compute => 0,
        Stage::Fragment => 0x5352_4746_0000_0001,
        Stage::Mesh => 0x4d45_5348_0000_0001,
    }
}

/// What one draw adds to its primitive shader's translation: the geometry-engine inputs it seeds
/// (D730), whether its position export is in window space (D731), the formats its buffers'
/// descriptors name for a format load (D738), and the coordinates its pixel shader's samples
/// saturate (D743).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
struct ForDraw {
    geometry: Option<GeometryInputs>,
    window_space: bool,
    buffer_formats: Option<orbistoun_translate::wavefront::BufferFormats>,
    flat_twins: Option<FlatTwins>,
    saturated: [[bool; 2]; 2],
}

/// No texture slot's coordinates saturated.
const UNSATURATED: [[bool; 2]; 2] = [[false; 2]; 2];

/// Distinguishes the same primitive shader translated for different draws' geometry (D730) or
/// position space (D731), in the cache key.
fn for_draw_salt(for_draw: ForDraw) -> u64 {
    let geometry = for_draw.geometry.map_or(0, |g| {
        0x4745_4f4d_0000_0000
            ^ (u64::from(g.vertices) << 32)
            ^ (u64::from(g.primitives) << 16)
            ^ u64::from(g.first_vertex)
            ^ match g.indices {
                None => 0,
                Some(width) => 0x4944_5800_0000_0000 ^ u64::from(width.bytes()),
            }
            ^ match g.assembly {
                Assembly::List => 0,
                Assembly::LineList => 0x4c4c_0000_0000_0000,
                Assembly::LineStrip => 0x4c53_0000_0000_0000,
                Assembly::Strip { provoking_last } => {
                    0x5354_0000_0000_0000 ^ u64::from(provoking_last)
                }
            }
    });
    let formats = for_draw.buffer_formats.map_or(0, |formats| {
        let words: Vec<u32> = formats
            .0
            .iter()
            .flat_map(|word| [u32::from(word.is_some()), word.unwrap_or(0)])
            .collect();
        0x4655_4d54_0000_0000 ^ crate::content_hash(&words)
    });
    let twins = for_draw.flat_twins.map_or(0, |twins| {
        let words: Vec<u32> = twins
            .0
            .iter()
            .flatten()
            .flat_map(|(attribute, location)| [u32::from(*attribute), u32::from(*location)])
            .collect();
        0x5457_494e_0000_0000 ^ crate::content_hash(&words)
    });
    let saturated = for_draw
        .saturated
        .as_flattened()
        .iter()
        .enumerate()
        .fold(0, |bits, (at, &on)| bits | u64::from(on) << at);
    let saturated = if saturated == 0 {
        0
    } else {
        0x5341_5455_0000_0000 ^ saturated
    };
    geometry
        ^ formats
        ^ twins
        ^ saturated
        ^ if for_draw.window_space {
            0x5749_4e44_5350_4345
        } else {
            0
        }
}

/// Distinguishes the same shader translated for different mesh primitives, in the cache key.
///
/// A mesh module's output shape is in the module (D688), so a point and a triangle built from the
/// same bytes are different modules.
const fn primitive_salt(primitive: MeshPrimitive) -> u64 {
    match primitive {
        MeshPrimitive::Points => 0x504f_494e_0000_0001,
        MeshPrimitive::Lines => 0x4c49_4e45_0000_0001,
        MeshPrimitive::Triangles => 0,
        MeshPrimitive::Rectangles => 0x5245_4354_0000_0001,
    }
}

/// The mesh primitive a decoded topology asks the translator to assemble, or a refusal naming a
/// topology that has no mesh-primitive shape.
///
/// Point, line and triangle map onto the three the mesh extension emits, and a rectangle list onto
/// triangles over each rectangle's fourth corner. An unknown value is refused by name rather than
/// drawn as a triangle.
fn mesh_primitive_of(topology: PrimitiveTopology) -> Result<MeshPrimitive, PipelineError> {
    match topology {
        PrimitiveTopology::PointList => Ok(MeshPrimitive::Points),
        PrimitiveTopology::LineStrip => Ok(MeshPrimitive::Lines),
        PrimitiveTopology::TriangleStrip => Ok(MeshPrimitive::Triangles),
        PrimitiveTopology::RectangleList => Ok(MeshPrimitive::Rectangles),
        other @ PrimitiveTopology::Other(_) => Err(PipelineError::NoMeshPrimitive(other)),
    }
}

/// The strategy a stage's shader is translated with, at the wave width the stream declared for that
/// stage (D145), and the cache-key salt that width contributes.
///
/// A compute dispatch keeps the pipeline's own width: its width is in the dispatch initiator, which
/// is not decoded.
fn stage_strategy(strategy: Strategy, stage: Stage, widths: WaveWidths) -> (Strategy, u64) {
    let strategy = match (strategy, stage) {
        (Strategy::Predicated { fidelity, .. }, Stage::Mesh | Stage::Fragment) => {
            let w32 = if stage == Stage::Mesh {
                widths.primitive_w32
            } else {
                widths.pixel_w32
            };
            Strategy::Predicated {
                fidelity,
                width: if w32 { Width::Wave32 } else { Width::Wave64 },
            }
        }
        (strategy, _) => strategy,
    };
    let salt = match strategy {
        Strategy::Predicated {
            width: Width::Wave32,
            ..
        } => 0x5733_3200_0000_0001,
        _ => 0,
    };
    (strategy, salt)
}

/// Distinguishes a module that reads its draw's bound buffers from one that reaches guest memory
/// only through the window (D733), in the cache key.
const DRAW_BUFFERS_SALT: u64 = 0x4452_4157_4255_4653;

/// Distinguishes the same shader translated against different memory windows, in the cache key.
///
/// A window's base and length are compiled into the module (the base is subtracted and the length
/// is the mask), so two windows produce two modules from one shader.
const fn window_salt(window: Window) -> u64 {
    // The full address, rotated so the length lands in bits a low-half base does not reach; windows
    // that differ only in their high half are different windows.
    window.address().rotate_left(20) ^ window.words() as u64
}

/// A shader the guest registered by name, rather than one inferred from register writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegisteredShader {
    /// Guest address of the shader's code.
    pub address: u64,
    /// Stage it was registered for.
    pub stage: ShaderStage,
}

/// Reads guest memory.
///
/// One method, so a test can supply a `Vec<u8>` and an emulator the mapped guest pages.
pub trait GuestMemory {
    /// Bytes at a guest virtual address, or `None` if the range is not mapped.
    ///
    /// `None` rather than zeros or a short read, because zeros decode into a plausible instruction
    /// stream.
    fn read(&self, address: u64, length: usize) -> Option<&[u8]>;
}

/// The largest shader this will read out of guest memory.
///
/// A shader carries no length; decoding stops at its terminator, so this bounds the search, not the
/// shader. The read narrows until it succeeds, because reading past a mapping fails.
pub const MAX_SHADER_BYTES: usize = 64 * 1024;

/// Why a shader could not be prepared.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShaderFailure {
    /// Guest address the shader was to be read from.
    pub address: u64,
    /// Stage it would have bound to, as the guest's register named it.
    pub stage: String,
    /// What went wrong, in a form a worklist can rank.
    pub reason: String,
}

/// What a submission contains, as counts first.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SubmissionReport {
    /// Packets the walk recognised.
    pub packets: usize,
    /// Register writes extracted from them.
    pub register_writes: usize,
    /// Draws the stream asked for.
    ///
    /// Auto-indexed draws only. An indexed draw's count comes from state and an index buffer that
    /// are not read, so it is not counted.
    pub draws: usize,
    /// The primitive the draw's geometry produces, from `VGT_GS_OUT_PRIM_TYPE`.
    ///
    /// `None` when the stream set it nowhere. The input-assembly register reads `TRILIST` for both
    /// point and triangle draws, so this is what tells them apart.
    pub primitive_topology: Option<PrimitiveTopology>,
    /// Which stages the stream declared thirty-two lanes wide, from `VGT_SHADER_STAGES_EN` and
    /// `SPI_PS_IN_CONTROL`. The primitive and pixel shaders are translated at these widths.
    pub wave_widths: WaveWidths,
    /// Shader addresses the registers named.
    pub shaders_found: usize,
    /// Of those, how many produced a module.
    pub shaders_translated: usize,
    /// Of those, how many came from the cache rather than being translated again.
    pub cache_hits: usize,
    /// Shaders the guest registered by name.
    pub registered: usize,
    /// Shader addresses the register writes implied.
    pub inferred: usize,
    /// Stages where both routes produced an address and the two matched: evidence for the register
    /// vocabulary.
    pub agreed: usize,
    /// Stages where both produced an address and they differed.
    ///
    /// The registered address is what the guest stated, so a mismatch means the register table
    /// found the wrong bits.
    pub disagreed: Vec<Disagreement>,
    /// Stages named by a route that the queue does not permit, reported rather than filtered.
    pub impossible_stages: Vec<String>,
    /// Every shader that did not, and why.
    pub failures: Vec<ShaderFailure>,
    /// Addresses a register named that guest memory recognised.
    ///
    /// Each resolved address is evidence that the GPU and guest address of a shader coincide there.
    /// Counted apart from the shader outcome, because an address can resolve and its shader still
    /// fail to translate.
    pub addresses_resolved: usize,
    /// Addresses a register named that guest memory did not recognise.
    ///
    /// Either the register decode found the wrong bits or a GPU address is not a guest address.
    pub addresses_unresolved: usize,
    /// Shaders that translated, with something about them worth knowing, such as needing the
    /// slowest fidelity.
    pub warnings: Vec<String>,
    /// Every distinct texture the draws' pixel shaders name, in first-use order.
    ///
    /// Read the way the shader reads it: the fragment stage's user data words 0 and 1 address its
    /// descriptor table, whose first eight words are the image descriptor (the GL context's
    /// textured shader loads them with `s_load_dwordx8 s[4:11], s[0:1], 0x00`).
    pub textures: Vec<ImageDescriptor>,
    /// Draws' sampled texture slots nothing could be bound to - an unreadable descriptor, or one
    /// this does not read - so the backend would sample its placeholder.
    pub unbound_textures: usize,
    /// Draws whose viewport transform the stream turned off (`PA_CL_VTE_CNTL`), so their positions
    /// are not the clip space the backend draws.
    pub unmodelled_viewports: usize,
    /// Draws into a one-byte colour target with no `CB_TARGET_MASK` in the stream, so which of a
    /// four-channel drawer's channels stand for its one is not known.
    pub unmasked_one_byte_draws: usize,
    /// Draws that write a colour channel their pixel shader never exports, which would hold
    /// whatever the shader's output held.
    pub unexported_channel_draws: usize,
    /// Draws running a shader that reads or writes guest memory while no window is mapped, so the
    /// module would read zeros where the guest's data is, and the first such access, by name and
    /// offset.
    pub unwindowed_draws: (usize, Option<String>),
    /// Draws whose shaders read through a buffer the draw could not bind exactly (D733), and why the
    /// first was refused.
    pub unbound_buffers: (usize, Option<&'static str>),
    /// Draws a stage's shader was not prepared for, which would run whatever shader was bound
    /// before them.
    pub unshaded_draws: usize,
}

impl SubmissionReport {
    /// This submission in the shader corpus's progress vocabulary (D129).
    ///
    /// A submission asks the corpus's question of shaders that arrived from a guest, so it reports
    /// in the same form. "Complete" is a shader that translated. Blockers are failure reasons
    /// rather than instruction names, because a submission fails for reasons a corpus never sees,
    /// such as an unresolved address.
    pub fn summary(&self) -> orbistoun_shader::coverage::Summary {
        orbistoun_shader::coverage::Summary {
            complete: self.shaders_translated,
            // Counts shaders a translator ran over, comparable with a corpus run that does the
            // same.
            attempted: true,
            shaders: self.shaders_found,
            translatable: self.shaders_translated,
            instructions: self.shaders_found,
            blockers: self
                .failures
                .iter()
                .map(|failure| failure.reason.clone())
                .collect(),
        }
    }
}

/// A stage where the two routes to a shader address did not agree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Disagreement {
    /// Which stage.
    pub stage: ShaderStage,
    /// What the guest registered.
    pub registered: u64,
    /// What the register writes implied.
    pub inferred: u64,
}

/// A shader address to prepare, and where it came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Candidate {
    address: u64,
    stage: ShaderStage,
}

/// One submission, prepared for a backend.
#[derive(Debug, Clone, Default)]
pub struct Submission {
    /// What the backend should do, in order.
    pub commands: Vec<RenderCommand>,
    /// Modules the commands refer to, for a backend that has not seen them before.
    pub modules: BTreeMap<ResourceId, Vec<u32>>,
    /// Colour render targets the commands select, by the id a [`RenderCommand::SetRenderTargets`]
    /// names, to their dimensions. A target carries no bytes, only the size a backend allocates, so
    /// it rides here rather than in [`Self::modules`] and is made resident the same way (D701).
    pub targets: BTreeMap<ResourceId, ColourTargetExtent>,
    /// Colour target zero's base address and extent, decoded from `CB_COLOR0_BASE` and `ATTRIB2`.
    /// The base locates the attachment in guest memory; the extent mirrors the entry in
    /// [`Self::targets`]. `None` when the stream set neither register.
    pub colour_target: Option<ColourTarget>,
    /// Colour target zero's tiling mode (`CB_COLOR0_ATTRIB3`'s `COLOR_SW_MODE`). A backend detiles
    /// a `Tiled64KbRX` attachment and reads a `Linear` one directly. `None` when the stream set no
    /// mode.
    pub colour_target_tiling: Option<SwizzleMode>,
    /// Colour target zero's element layout (`CB_COLOR0_INFO`), the byte order a frame written back
    /// into it must use. `None` when the stream set none.
    pub colour_target_format: Option<ColourTargetFormat>,
    /// Colour target zero's delta colour compression (`CB_COLOR0_INFO.DCC_ENABLE`), `None` when
    /// the target has none.
    pub colour_target_dcc: Option<crate::registers::ColourTargetDcc>,
    /// How many distinct colour target zero bases the stream wrote: one frame is written back only
    /// when all draws went to one target.
    pub colour_target_bases: usize,
    /// The depth- and stencil-test state a draw runs under (`DB_DEPTH_CONTROL`). `None` when the
    /// stream set none: a draw with no depth control has no depth test.
    pub depth_control: Option<DepthControl>,
    /// The stencil operations a draw applies on each test outcome (`DB_STENCIL_CONTROL`). `None`
    /// when the stream set none.
    pub stencil_control: Option<StencilControl>,
    /// Colour target zero's blend state (`CB_BLEND0_CONTROL`). `None` when the stream set none.
    pub blend_control: Option<BlendControl>,
    /// The guest-memory window this submission's shaders read, as words, read at the pipeline's
    /// window (D703). Frame-level, because every module is compiled against one window. Empty when
    /// the window is not mapped.
    pub guest_memory: Vec<u32>,
    /// The guest address of [`Self::guest_memory`]'s first word, for locating an address the
    /// shaders form inside the window.
    pub guest_memory_base: u64,
    /// What was seen and what failed.
    pub report: SubmissionReport,
}

/// Why a shader could not be prepared, and whether its address was the problem.
///
/// An unresolved address is evidence about the address space; anything after it is evidence about
/// the shader, so the two are kept apart.
#[derive(Debug)]
enum PrepareFailure {
    /// Guest memory has nothing at the address the register named.
    Unresolved(String),
    /// The address resolved and something after it did not.
    Resolved(String),
    /// A primitive shader reads the geometry engine's inputs, and it was prepared without a
    /// draw's geometry to seed them (D730).
    NeedsGeometry(String),
}

impl PrepareFailure {
    /// The reason, for a report.
    fn reason(self) -> String {
        match self {
            Self::Unresolved(reason) | Self::Resolved(reason) | Self::NeedsGeometry(reason) => {
                reason
            }
        }
    }
}

/// Colour target zero's base, tiling, element layout and compression, and how many bases the draws
/// drew into, for a backend to build its pipeline from and a write-back to place its frame by. Each
/// is `None` when the stream set its register nowhere: state is read, never assumed.
fn record_colour_target(submission: &mut Submission, writes: &[RegisterWrite], draws: &[DrawCall]) {
    submission.colour_target = colour_target_at(writes);
    submission.colour_target_tiling = colour_swizzle_mode_at(writes);
    submission.colour_target_format = colour_target_format_at(writes);
    submission.colour_target_dcc = colour_target_dcc_at(writes);
    let draw_offsets: Vec<u32> = draws.iter().map(|draw| draw.packet_offset).collect();
    submission.colour_target_bases = colour_target_bases_in(writes, &draw_offsets);
}

/// The shaders one draw runs, by stage.
type DrawShaders = Vec<(ShaderStage, ResourceId)>;

/// How many draws a stage's shader was not prepared for: a draw on the draw queue missing its
/// geometry or its pixel shader, a stage the guest registered aside, since that one is bound once
/// for the whole stream. Such a draw would run whatever shader the backend bound before it.
fn unshaded_draws(
    per_draw: &[DrawShaders],
    queue: Queue,
    registered: &[RegisteredShader],
) -> usize {
    if queue != Queue::Draw {
        return 0;
    }
    let needed: Vec<ShaderStage> = [ShaderStage::Vertex, ShaderStage::Fragment]
        .into_iter()
        .filter(|stage| !registered.iter().any(|r| r.stage == *stage))
        .collect();
    per_draw
        .iter()
        .filter(|shaders| {
            !needed
                .iter()
                .all(|stage| shaders.iter().any(|(bound, _)| bound == stage))
        })
        .count()
}

/// A candidate that was not prepared: its failure recorded, or a primitive shader that needs a
/// draw's geometry (D730), with the reason it gave.
enum Unprepared {
    Failed,
    NeedsGeometry(String),
}

/// Where each of a primitive shader's buffers' fourth descriptor word comes from, by slot: `None`
/// for a buffer read through a scalar base, which has no format.
type FormatWords = std::sync::Arc<[orbistoun_translate::draw_buffers::BufferSource]>;

/// Where a module prepared for no particular draw is held: [`Pipeline::plain_key`].
type PlainKey = (u32, u64, bool, u32);

/// A submission's primitive shaders prepared per draw geometry (D730): the ones that need it, with
/// the reason they gave; what each geometry made; and the refusals already recorded.
#[derive(Default)]
struct ByGeometry {
    /// Candidates prepared without geometry, by whether their positions are in window space (D731),
    /// and what they became; `None` for a failure.
    plain: BTreeMap<PlainKey, Option<ResourceId>>,
    needs: BTreeMap<(u32, u64), String>,
    prepared: BTreeMap<(u32, u64, ForDraw), Option<ResourceId>>,
    refused: std::collections::BTreeSet<(u64, String)>,
    /// The flat twins the draw being bound gives its attributes (D742).
    flat_twins: Option<FlatTwins>,
    /// The coordinates the draw being bound has its pixel shader's samples saturate (D743).
    saturated: [[bool; 2]; 2],
}

/// `VGT_PRIMITIVE_TYPE` (`gfx103.json`, byte `198920`, uconfig dword `0xC242`): the draw's input
/// primitive, `PRIM_TYPE` in bits 5:0.
const VGT_PRIMITIVE_TYPE: u32 = 0xC242;
/// `VGT_INDEX_TYPE` (`gfx103.json`, uconfig `0x3090C`), which radeonsi writes before an indexed draw
/// (`si_state_draw.cpp:1517`): `INDEX_TYPE` in bits 1:0.
const VGT_INDEX_TYPE: u32 = 0xC243;
/// `GE_MULTI_PRIM_IB_RESET_EN` (uconfig `0x3092C`), where GFX10 radeonsi enables primitive restart
/// (`si_state_draw.cpp:1314`): `RESET_EN` in bit 0.
const GE_MULTI_PRIM_IB_RESET_EN: u32 = 0xC24B;

/// `VGT_INDEX_TYPE` where no submission wrote it: `VGT_INDEX_16` (D754, assumed).
const UNWRITTEN_INDEX_TYPE: u32 = 0;

/// An indexed draw's index width from `VGT_INDEX_TYPE`: `VGT_INDEX_16` (0) and `VGT_INDEX_32` (1),
/// an unwritten register reading as sixteen bits (D754); `VGT_INDEX_8` is `None`.
fn index_width(value: Option<u32>) -> Option<orbistoun_translate::wavefront::IndexWidth> {
    use orbistoun_translate::wavefront::IndexWidth;
    match value.unwrap_or(UNWRITTEN_INDEX_TYPE) & 3 {
        0 => Some(IndexWidth::Bits16),
        1 => Some(IndexWidth::Bits32),
        _ => None,
    }
}

/// `DI_PT_LINELIST` and `DI_PT_LINESTRIP` (enum `VGT_DI_PRIM_TYPE`, `gfx103.json:689-690`): lines
/// of two vertices each, and lines sharing vertices in a strip.
const DI_PT_LINELIST: u32 = 2;
/// See [`DI_PT_LINELIST`].
const DI_PT_LINESTRIP: u32 = 3;
/// `DI_PT_TRISTRIP` (enum `VGT_DI_PRIM_TYPE`, `gfx103.json`): triangles sharing vertices in a strip.
const DI_PT_TRISTRIP: u32 = 6;
/// `PA_SU_SC_MODE_CNTL` (`gfx103.json`, context dword `0x205`) and its `PROVOKING_VTX_LAST`, bit 19.
const PA_SU_SC_MODE_CNTL: u32 = 0xA205;
/// See [`PA_SU_SC_MODE_CNTL`].
const PROVOKING_VTX_LAST: u32 = 1 << 19;
/// `DI_PT_TRILIST` and `DI_PT_RECTLIST` (`gfx103.json:691`, `:704`, enum `VGT_DI_PRIM_TYPE`): the
/// input primitives of three vertices each, the second radeonsi's blits.
const DI_PT_TRILIST: u32 = 4;
const DI_PT_RECTLIST: u32 = 17;
/// `VGT_SHADER_STAGES_EN` (`gfx103.json`, dword `0xA2D5`); its `PRIMGEN_PASSTHRU_EN`, bit 25, hands
/// the primitive shader its primitive already packed in `v0` ([`GeometryInputs::passthrough`]).
const VGT_SHADER_STAGES_EN: u32 = 0xA2D5;
const PRIMGEN_PASSTHRU_EN: u32 = 1 << 25;
/// `GE_INDX_OFFSET` (`gfx103.json`, byte `198952`, uconfig dword `0xC24A`): added to every vertex
/// index, so the vertex ids of a draw that sets it are not its indices.
const GE_INDX_OFFSET: u32 = 0xC24A;

/// The geometry-engine inputs a draw hands its primitive shader whole (D730), or why it cannot: a
/// draw of one instance over a list or strip of triangles or lines, with no index offset. One wave may not hold them; [`chunk_plans`] splits those that it does not.
fn draw_geometry(
    draw: &DrawCall,
    mut latest: impl FnMut(u32) -> Option<u32>,
) -> Result<GeometryInputs, String> {
    let (vertices, indices) = match draw.kind {
        DrawKind::Auto { vertices } => (vertices, None),
        DrawKind::Indexed { indices, .. } => {
            if latest(GE_MULTI_PRIM_IB_RESET_EN).is_some_and(|value| value & 1 != 0) {
                return Err(
                    "an indexed draw with primitive restart's geometry-engine inputs are not seeded"
                        .to_owned(),
                );
            }
            let width = index_width(latest(VGT_INDEX_TYPE)).ok_or(
                "an indexed draw whose index size is not sixteen or thirty-two bits is not seeded",
            )?;
            (indices, Some(width))
        }
    };
    if draw.instances != 1 {
        return Err(
            "a draw of more than one instance's geometry-engine inputs are not seeded".to_owned(),
        );
    }
    let topology = latest(VGT_PRIMITIVE_TYPE).map(|value| value & 0x3F);
    let (assembly, primitives) = match topology {
        Some(DI_PT_TRILIST | DI_PT_RECTLIST) => (Assembly::List, vertices / 3),
        Some(DI_PT_LINELIST) => (Assembly::LineList, vertices / 2),
        Some(DI_PT_LINESTRIP) => (Assembly::LineStrip, vertices.saturating_sub(1)),
        Some(DI_PT_TRISTRIP) => (
            Assembly::Strip {
                provoking_last: latest(PA_SU_SC_MODE_CNTL)
                    .is_some_and(|value| value & PROVOKING_VTX_LAST != 0),
            },
            vertices.saturating_sub(2),
        ),
        _ => {
            return Err(format!(
                "the draw's input primitive {topology:?} is not a list or strip of triangles or lines, whose geometry-engine inputs are the ones seeded"
            ));
        }
    };
    let passthrough =
        latest(VGT_SHADER_STAGES_EN).is_some_and(|value| value & PRIMGEN_PASSTHRU_EN != 0);
    if latest(GE_INDX_OFFSET).is_some_and(|value| value != 0) {
        return Err("a draw with an index offset's vertex ids are not seeded".to_owned());
    }
    Ok(GeometryInputs {
        first_vertex: 0,
        vertices,
        primitives,
        assembly,
        indices,
        passthrough,
    })
}

/// One subgroup's share of a draw too large for one wave (D741): its own geometry, and where its
/// vertex threads find their ids.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ChunkPlan {
    geometry: GeometryInputs,
    index_buffer: IndexBuffer,
}

/// A draw split into subgroups of at most `lanes` vertex and primitive threads, as the geometry
/// engine splits one into primitive-shader subgroups (D741); empty for a draw one subgroup holds
/// whole.
///
/// A list's chunk takes whole triangles; a strip's an even count of them, so each chunk's first
/// triangle has the strip's even winding, and its vertices overlap the next chunk's by two. Each
/// chunk reads its vertex ids as indices: an indexed draw's own from where its first primitive's
/// begin (`index_address`, the draw's), a non-indexed draw's counting up from that position.
fn chunk_plans(whole: GeometryInputs, index_address: Option<u64>, lanes: u32) -> Vec<ChunkPlan> {
    if whole.vertices <= lanes && whole.primitives <= lanes {
        return Vec::new();
    }
    let per_chunk = match whole.assembly {
        Assembly::List => lanes / 3,
        Assembly::Strip { .. } => lanes.saturating_sub(2) & !1,
        Assembly::LineList => lanes / 2,
        Assembly::LineStrip => lanes.saturating_sub(1),
    };
    if per_chunk == 0 {
        return Vec::new();
    }
    let width = whole
        .indices
        .unwrap_or(orbistoun_translate::wavefront::IndexWidth::Bits32);
    (0..whole.primitives)
        .step_by(per_chunk as usize)
        .map(|start| {
            let primitives = per_chunk.min(whole.primitives - start);
            let first_index = match whole.assembly {
                Assembly::List => start * 3,
                Assembly::LineList => start * 2,
                Assembly::Strip { .. } | Assembly::LineStrip => start,
            };
            let index_buffer = match (whole.indices, index_address) {
                (Some(width), Some(address)) => IndexBuffer::Guest {
                    address: address + u64::from(first_index) * u64::from(width.bytes()),
                    bytes: width.bytes(),
                },
                _ => IndexBuffer::Counting {
                    first: whole.first_vertex + first_index,
                },
            };
            ChunkPlan {
                geometry: GeometryInputs {
                    first_vertex: 0,
                    vertices: whole.assembly.vertices_for(primitives),
                    primitives,
                    assembly: whole.assembly,
                    indices: Some(width),
                    passthrough: whole.passthrough,
                },
                index_buffer,
            }
        })
        .collect()
}

/// [`chunk_plans`] for `draw`, whose indices, if it has them, are where it names: a draw one wave
/// does not hold is drawn in chunks that it does (D741).
fn draw_chunks(draw: &DrawCall, whole: GeometryInputs, lanes: u32) -> Vec<ChunkPlan> {
    let address = match draw.kind {
        DrawKind::Indexed { address, .. } => Some(address),
        DrawKind::Auto { .. } => None,
    };
    chunk_plans(whole, address, lanes)
}

/// Each chunk's module beside its plan (D741); empty where the draw is drawn whole.
fn chunks_of(modules: &[ResourceId], plans: &[ChunkPlan]) -> Vec<Chunk> {
    if modules.len() != plans.len() {
        return Vec::new();
    }
    modules
        .iter()
        .zip(plans)
        .map(|(&vertex, plan)| Chunk {
            vertex,
            indices: plan.geometry.vertices,
            index_buffer: plan.index_buffer,
        })
        .collect()
}

/// A chunk of a draw as bound (D741): the primitive shader for its geometry, its index count and
/// where its indices are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Chunk {
    vertex: ResourceId,
    indices: u32,
    index_buffer: IndexBuffer,
}

/// A translated shader, with enough of its bytes to notice if the key stops identifying it.
///
/// The key hashes `decoded.consumed`, the decoder's idea of where the shader ends, so a decoder
/// change can change what a key means. A hit is served without re-reading the bytes; keeping the
/// length and the first and last words makes such a change loud for a few comparisons per bind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Cached {
    resource: ResourceId,
    length: usize,
    first: u32,
    last: u32,
}

impl Cached {
    /// What a shader's bytes should look like for this entry to be served.
    fn of(resource: ResourceId, bytes: &[u8]) -> Self {
        Self {
            resource,
            length: bytes.len(),
            first: Self::word(bytes, 0),
            last: Self::word(bytes, bytes.len().saturating_sub(4)),
        }
    }

    /// Whether these bytes are the ones this entry was built from.
    fn matches(self, bytes: &[u8]) -> bool {
        self == Self::of(self.resource, bytes)
    }

    /// The word at `at`, zero-padded when fewer than four bytes remain.
    ///
    /// Padding rather than returning zero keeps two different short shaders of the same length
    /// distinct; `read_window` narrows rather than refusing, so a short one can reach here.
    fn word(bytes: &[u8], at: usize) -> u32 {
        let mut packed = [0u8; 4];
        let available = bytes.get(at..).unwrap_or_default();
        let take = available.len().min(4);
        packed[..take].copy_from_slice(&available[..take]);
        u32::from_le_bytes(packed)
    }
}

/// Translates submissions, caching shaders between them.
///
/// Long-lived, because a frame's submissions bind the same few shaders repeatedly.
#[derive(Debug)]
pub struct Pipeline {
    encodings: EncodingTable,
    operands: OperandTable,
    vocabulary: Vocabulary,
    strategy: Strategy,
    /// The span of guest memory a translated shader may reach, and where it starts.
    ///
    /// Named by the caller rather than derived from the submission: no measured register vocabulary
    /// says which register carries a buffer address. Every shader memory access is checked against
    /// it, so a misplaced window refuses every access.
    window: Window,
    /// Whether each submission places the window itself, from the constant base its geometry shader
    /// forms (D711). Set by [`Self::placing_window_from_shaders`] for a live guest; off for a
    /// caller that places the window by hand.
    places_window: bool,
    /// Whether translated shaders read their stage's user data at entry. Set by
    /// [`Self::feeding_user_data`] for a backend that supplies it per draw.
    feeds_user_data: bool,
    /// Whether each submission is prepared on the register state the ones before it left (D737).
    /// Set by [`Self::carrying_register_state`] for a live guest.
    carries_state: bool,
    /// That state: the last value each register was written, by the submissions carried so far.
    carried: BTreeMap<u32, u32>,
    /// Each stage's user-data layout for the submission being prepared, vertex then fragment.
    user_data: [UserData; 2],
    /// Shader bytes, by content hash, to the resource holding the translation.
    cache: BTreeMap<u64, Cached>,
    /// The bytes each shader address decoded to, end-of-program included.
    decoded: BTreeMap<u64, Vec<u8>>,
    /// The constant base each vertex program address formed, with the program bytes it was found
    /// in.
    bases: BTreeMap<u64, (Vec<u8>, Option<u64>)>,
    /// Each translated module's texture sources, by its resource.
    texture_sources: BTreeMap<ResourceId, Vec<TextureSource>>,
    /// The translated modules whose shaders read or write guest memory, which reach it only
    /// through the window.
    memory_readers: BTreeMap<ResourceId, String>,
    /// The translated primitive shaders that read their vertex ids from the draw's index buffer
    /// (D740), which a draw binds after their traced buffers.
    index_readers: std::collections::BTreeSet<ResourceId>,
    /// The colour channels each translated pixel shader exports, by its resource.
    colour_channels: BTreeMap<ResourceId, u8>,
    /// Texels read from guest memory, shared across submissions while their bytes are unchanged.
    texels: TexelCache,
    /// Each translated module's draw buffers, in slot order (D733).
    buffer_sources: BTreeMap<ResourceId, Vec<orbistoun_translate::draw_buffers::BufferSource>>,
    /// Bytes read for draw buffers, shared across submissions while unwritten.
    buffers: crate::draw_buffers::BufferCache,
    next_resource: u64,
    /// Translations kept across runs, consulted before translating and added to after (D113).
    store: Option<crate::translations::TranslationStore>,
    /// Fills not yet matched against a depth target, which may clear one a later submission binds.
    depth_fills: crate::depth::PendingFills,
    /// Guest dispatch modules, by the program's bytes, its entry state and its window.
    dispatch_modules: BTreeMap<u64, (Vec<u32>, orbistoun_translate::wavefront::ImageSources)>,
    /// Each primitive shader's traced format words, by address and user-data count, with the hash
    /// of the bytes they were traced from ([`Self::format_words`]).
    format_words: BTreeMap<(u64, u32), (u64, Option<FormatWords>)>,
}

/// A guest compute dispatch made ready to run: its module, the window it was built against, and the
/// push-constant block carrying its user data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedDispatch {
    /// The translated module.
    pub module: Vec<u32>,
    /// The guest memory it reads and writes through its buffers; `None` for a program that
    /// addresses none.
    pub window: Option<Window>,
    /// The images it fetches from and stores to, as linear texels, and where the stored one lies
    /// in guest memory.
    pub images: DispatchImages,
    /// Its user data, from word zero of the block.
    pub push: Vec<u32>,
    /// How many groups run.
    pub groups: [u32; 3],
    /// The guest range each of its own buffers binds, by slot, as `(address, bytes)` (D746).
    pub buffers: Vec<(u64, u64)>,
}

impl Pipeline {
    /// Fills of guest memory that happened outside a stream's own packets - a dispatch's buffer
    /// cleared to one word (D750) - kept for the depth surfaces the next draws bind.
    pub(crate) fn note_fills(&mut self, fills: Vec<crate::cp::Fill>) {
        self.depth_fills.note(fills);
    }

    /// Builds a pipeline over the built-in tables.
    ///
    /// # Errors
    ///
    /// If a built-in table does not load, which is a build fault rather than anything about a
    /// guest.
    pub fn new(strategy: Strategy) -> Result<Self, PipelineError> {
        Ok(Self {
            encodings: EncodingTable::builtin().map_err(|e| PipelineError::Table(e.to_string()))?,
            operands: OperandTable::builtin().map_err(|e| PipelineError::Table(e.to_string()))?,
            vocabulary: Vocabulary::builtin().map_err(|e| PipelineError::Table(e.to_string()))?,
            strategy,
            window: Window::default(),
            places_window: false,
            feeds_user_data: false,
            carries_state: false,
            carried: BTreeMap::new(),
            user_data: [UserData::default(); 2],
            cache: BTreeMap::new(),
            decoded: BTreeMap::new(),
            bases: BTreeMap::new(),
            texture_sources: BTreeMap::new(),
            memory_readers: BTreeMap::new(),
            index_readers: std::collections::BTreeSet::new(),
            colour_channels: BTreeMap::new(),
            texels: TexelCache::new(),
            buffer_sources: BTreeMap::new(),
            buffers: crate::draw_buffers::BufferCache::default(),
            next_resource: 1,
            store: None,
            depth_fills: crate::depth::PendingFills::default(),
            dispatch_modules: BTreeMap::new(),
            format_words: BTreeMap::new(),
        })
    }

    /// Makes a guest compute dispatch ready to run: reads and decodes its program, refuses one that
    /// may read user data the stream never wrote, places its window over the buffers it addresses,
    /// and translates it with its entry state and exact memory.
    ///
    /// # Errors
    ///
    /// Each refusal, by name: an unreadable or undecodable program, unknown user data, a window
    /// that cannot be placed, or a translation refused.
    pub fn prepare_dispatch(
        &mut self,
        state: &crate::dispatch::DispatchState,
        memory: &impl GuestMemory,
    ) -> Result<PreparedDispatch, String> {
        let address = guest_address_of(state.program);
        let window = read_window(memory, address)
            .ok_or_else(|| format!("no mapped memory at the compute program {address:#x}"))?;
        let decoded = decode_program(window, &self.encodings, &self.operands);
        if !decoded.terminated || !decoded.is_trustworthy() {
            return Err(format!(
                "the compute program at {address:#x} did not decode cleanly to its end"
            ));
        }
        let program = &window[..decoded.consumed];
        // Each buffer a load or store reaches through a traced descriptor is bound on its own
        // (D746); the window covers the rest.
        let (served, buffers, formats) = self.dispatch_buffers(state, &decoded, memory)?;
        let placed = crate::dispatch::place_window_except(
            state,
            (&decoded, &self.encodings),
            &served,
            |at, len| usize::try_from(len).is_ok_and(|len| memory.read(at, len).is_some()),
        )
        .map_err(str::to_owned)?;
        if let Some(window) = placed {
            let span = (window.address(), u64::from(window.words()) * 4);
            if buffers.iter().any(|&range| overlaps(range, span)) {
                return Err(concat!(
                    "a buffer the dispatch binds overlaps its window, and two copies of one ",
                    "byte would disagree (D746)"
                )
                .to_owned());
            }
        }
        let window_or_none = placed;
        let placed = placed.unwrap_or_default();
        // The images' numeric types come from the descriptors the translation names, so a module
        // translated for float images whose images are integer ones is translated again for them.
        let mut integer_images = state.inputs.integer_images;
        let (module, images) = loop {
            let (module, sources) = self.dispatch_module(
                (&decoded, program, address),
                state,
                (placed, formats),
                integer_images,
            )?;
            let images = dispatch_images(state, &sources, memory)?;
            let wanted = [
                images
                    .fetched
                    .as_ref()
                    .is_some_and(|t| t.format.is_integer()),
                images
                    .stored
                    .as_ref()
                    .is_some_and(|t| t.format.is_integer()),
            ];
            if wanted == integer_images {
                break (module, images);
            }
            integer_images = wanted;
        };
        Ok(PreparedDispatch {
            module,
            window: window_or_none,
            images,
            // The whole block the module declares; a word the stream never wrote, which the
            // translation refused a program for reading, and the rest of the block are zero.
            push: {
                let mut block: Vec<u32> = state.user_data.iter().map(|w| w.unwrap_or(0)).collect();
                block.resize(
                    orbistoun_translate::wavefront::USER_DATA_BLOCK_WORDS as usize,
                    0,
                );
                block
            },
            groups: state.groups,
            buffers,
        })
    }

    /// The accesses a dispatch's own buffers take, by instruction offset, and the guest range each
    /// binds (D746): traced from the program as a draw's are, stores included, and resolved
    /// against the dispatch's user data. Ranges that overlap are refused. For a program that
    /// converts by its descriptors' formats (D738), each buffer's fourth descriptor word too, as a
    /// draw's are resolved; one that does not resolve stays `None`, and an access through it is
    /// refused.
    fn dispatch_buffers(
        &self,
        state: &crate::dispatch::DispatchState,
        decoded: &orbistoun_shader::Decode,
        memory: &impl GuestMemory,
    ) -> Result<DispatchBuffers, String> {
        let count = u32::try_from(state.user_data.len()).unwrap_or(u32::MAX);
        let traced = orbistoun_translate::wavefront::draw_buffers_for(
            decoded,
            &self.encodings,
            Stage::Compute,
            UserData {
                count,
                draw_buffers: true,
                ..UserData::default()
            },
        )
        .map_err(|e| e.to_string())?;
        let mut words = [0u32; USER_DATA_WORDS];
        for (word, value) in words.iter_mut().zip(&state.user_data) {
            *word = value.unwrap_or(0);
        }
        let mut ranges = Vec::with_capacity(traced.sources.len());
        for source in &traced.sources {
            let range = crate::draw_buffers::resolve_range(source, &words, memory)?;
            if ranges.iter().any(|&held| overlaps(held, range)) {
                return Err(concat!(
                    "two buffers the dispatch binds overlap, and two copies of one byte would ",
                    "disagree (D746)"
                )
                .to_owned());
            }
            ranges.push(range);
        }
        let formats =
            orbistoun_translate::wavefront::reads_buffer_formats(decoded, &self.encodings).then(
                || {
                    let mut formats = orbistoun_translate::wavefront::BufferFormats::default();
                    for (slot, source) in formats.0.iter_mut().zip(&traced.sources) {
                        *slot = crate::draw_buffers::resolve_fourth_word(source, &words, memory);
                    }
                    formats
                },
            );
        Ok((traced.served, ranges, formats))
    }

    /// A dispatch's program translated for its entry state, its window, its buffers' formats and its
    /// images' numeric types, from the cache where it was translated before, with where its images
    /// come from.
    fn dispatch_module(
        &mut self,
        (decoded, program, address): (&orbistoun_shader::Decode, &[u8], u64),
        state: &crate::dispatch::DispatchState,
        (placed, formats): (
            Window,
            Option<orbistoun_translate::wavefront::BufferFormats>,
        ),
        integer_images: [bool; 2],
    ) -> Result<(Vec<u32>, orbistoun_translate::wavefront::ImageSources), String> {
        let count = u32::try_from(state.user_data.len()).unwrap_or(u32::MAX);
        let inputs = orbistoun_translate::wavefront::ComputeInputs {
            integer_images,
            ..state.inputs
        };
        let user_data = UserData {
            first_register: 0,
            count,
            block_offset: 0,
            dx10_clamp: state.dx10_clamp,
            pixel_inputs: None,
            compute: Some(inputs),
            geometry: None,
            window_space: false,
            // Its buffers are bound on their own where traced (D746).
            draw_buffers: true,
            buffer_formats: formats,
            flat_twins: None,
            saturated: UNSATURATED,
        };
        let key = content_hash(program)
            // The formats are compiled in, so a dispatch through other formats is another module.
            ^ formats.map_or(0, |formats| {
                let words = formats.0.map(|word| word.map_or(u64::MAX, u64::from));
                content_hash(zerocopy::IntoBytes::as_bytes(&words[..])).rotate_left(7)
            })
            ^ window_salt(placed)
            ^ (u64::from(count) << 56)
            ^ u64::from(inputs.threads[0]) << 40
            ^ u64::from(inputs.threads[1]) << 32
            ^ u64::from(inputs.threads[2]) << 24
            ^ u64::from(inputs.thread_id_components) << 20
            ^ inputs
                .workgroup_ids
                .iter()
                .enumerate()
                .map(|(i, &on)| u64::from(on) << (16 + i))
                .sum::<u64>()
            ^ match state.dx10_clamp {
                None => 0,
                Some(false) => 1 << 12,
                Some(true) => 2 << 12,
            }
            ^ match state.width {
                Width::Wave32 => 1 << 10,
                Width::Wave64 => 0,
            }
            // A partial group's mask is compiled in, so its shape is part of the module's name.
            ^ inputs.partial.map_or(0, |partial| {
                let shape = [partial.last, partial.threads];
                content_hash(zerocopy::IntoBytes::as_bytes(shape.as_flattened())) | 1
            })
            ^ u64::from(integer_images[0]) << 62
            ^ u64::from(integer_images[1]) << 63;
        Ok(if let Some(cached) = self.dispatch_modules.get(&key) {
            cached.clone()
        } else {
            let strategy = Strategy::Predicated {
                fidelity: orbistoun_translate::Fidelity::Wavefront,
                width: state.width,
            };
            let translated = translate_with_user_data(
                decoded,
                &self.encodings,
                strategy,
                (Stage::Compute, MeshPrimitive::default()),
                placed,
                user_data,
            )
            .map_err(|e| {
                format!("the compute program at {address:#x} could not be translated: {e}")
            })?;
            let sources = (translated.textures.clone(), translated.storage);
            self.dispatch_modules
                .insert(key, (translated.module.clone(), sources.clone()));
            (translated.module, sources)
        })
    }

    /// Serves translations from `store` and keeps new ones in it (D113).
    #[must_use]
    pub fn with_translation_store(mut self, store: crate::translations::TranslationStore) -> Self {
        self.store = Some(store);
        self
    }

    /// The kept translations, when this pipeline keeps any.
    pub fn translation_store_mut(&mut self) -> Option<&mut crate::translations::TranslationStore> {
        self.store.as_mut()
    }

    /// Has every submission place the window at the constant 64-bit base its geometry shader forms,
    /// and leaves the window where it was when the shader forms none (D711).
    ///
    /// For a live guest, whose shaders name their own buffers. A caller that places the window by
    /// hand, such as a capture test that relocates a vertex buffer, leaves this off.
    #[must_use]
    pub const fn placing_window_from_shaders(mut self) -> Self {
        self.places_window = true;
        self
    }

    /// Has every shader read its stage's user data at entry, from the push-constant block the
    /// backend fills per draw from each [`RenderCommand::SetUserData`].
    ///
    /// For a backend that supplies the block. Otherwise modules read no block and are unchanged.
    #[must_use]
    pub const fn feeding_user_data(mut self) -> Self {
        self.feeds_user_data = true;
        self
    }

    /// Prepares each submission on the register state the ones before it left, as the GPU runs
    /// it (D737): state advances by [`Self::carry`], once a submission has been carried out.
    ///
    /// For a live guest, whose submissions are one queue's. Streams prepared by hand stay each
    /// whole.
    #[must_use]
    pub const fn carrying_register_state(mut self) -> Self {
        self.carries_state = true;
        self
    }

    /// The register writes `stream` runs under, and how many are its own: the state earlier
    /// submissions left first, so the stream's own writes win, then the stream's (D737). A stream
    /// that clears state, or a pipeline that does not carry it, has only its own. The tables its
    /// indirect register loads name are read from `tables`.
    pub fn writes_in_force(
        &self,
        walked: &PacketWalk,
        stream: &[u8],
        tables: Option<&dyn GuestMemory>,
    ) -> (Vec<RegisterWrite>, usize) {
        let own = register_writes_reading(walked, stream, &self.vocabulary, tables);
        let count = own.len();
        if !self.carries_state || last_clear(walked).is_some() {
            return (own, count);
        }
        let carried = self
            .carried
            .iter()
            .map(|(&register, &value)| RegisterWrite {
                packet_offset: 0,
                register,
                value,
            });
        (carried.chain(own).collect(), count)
    }

    /// Advances the carried register state past `stream`, once it has been carried out (D737):
    /// every register it wrote now holds its last value there. A stream that clears state
    /// (`CLEAR_STATE`) leaves only what it wrote after its last clear. Nothing, unless
    /// [`Self::carrying_register_state`]. The tables its indirect register loads name are read from
    /// `tables`.
    pub fn carry(&mut self, stream: &[u8], tables: Option<&dyn GuestMemory>) {
        if !self.carries_state {
            return;
        }
        let walked = walk(stream);
        let writes = register_writes_reading(&walked, stream, &self.vocabulary, tables);
        let cleared = last_clear(&walked);
        if cleared.is_some() {
            self.carried.clear();
        }
        for write in writes
            .iter()
            .filter(|write| cleared.is_none_or(|at| write.packet_offset > at))
        {
            self.carried.insert(write.register, write.value);
        }
    }

    /// Places the guest-memory window every shader this pipeline translates is built against.
    ///
    /// The default is at address zero, which refuses every access a real guest shader makes. Clears
    /// the cache: the key carries the window too, but entries for the old window could never be hit
    /// again.
    #[must_use]
    pub fn with_window(mut self, window: Window) -> Self {
        self.window = window;
        self.cache.clear();
        self
    }

    /// Where this pipeline's guest-memory window sits.
    #[must_use]
    pub const fn window(&self) -> Window {
        self.window
    }

    /// How many distinct shaders have been translated and kept.
    pub fn cached_shaders(&self) -> usize {
        self.cache.len()
    }

    /// Submits a command buffer that lives in guest memory.
    ///
    /// [`Pipeline::submit`] takes bytes; a guest's submit call passes an address and a length, and
    /// this is the shape that call site needs. Returns `None` when the command buffer is not
    /// readable, which points at the call's arguments rather than at GPU address translation.
    pub fn submit_at(
        &mut self,
        address: u64,
        length: usize,
        queue: Queue,
        registered: &[RegisteredShader],
        memory: &impl GuestMemory,
    ) -> Option<Submission> {
        let stream = memory.read(address, length)?.to_vec();
        Some(self.submit(&stream, queue, registered, memory))
    }

    /// Prepares one submitted command buffer.
    ///
    /// Never fails on the guest's account: packets nobody understands produce an empty command list
    /// and a report saying so. `registered` is what the guest told the graphics library about; it
    /// is believed where it overlaps the register writes, and the report records disagreements.
    pub fn submit(
        &mut self,
        stream: &[u8],
        queue: Queue,
        registered: &[RegisteredShader],
        memory: &impl GuestMemory,
    ) -> Submission {
        use crate::perf::{Span, span};
        let walked = span(Span::PrepareWalk, || walk(stream));
        let (writes, own_writes) = span(Span::PrepareRegisters, || {
            self.writes_in_force(&walked, stream, Some(memory))
        });
        let inferred = shader_candidates(&writes, &self.vocabulary);

        let mut submission = Submission {
            report: SubmissionReport {
                packets: walked.packets.len(),
                register_writes: own_writes,
                ..SubmissionReport::default()
            },
            ..Submission::default()
        };

        // The colour target first, because a draw reads it. Decoding its size register lets a
        // backend allocate a real attachment. The `targets` map is keyed by extent (D702); the
        // decoded base rides on `colour_target` below. A depth target is sized to it.
        let depth_target = colour_target_extent_at(&writes).and_then(|extent| {
            let target = crate::depth::depth_target_at(&writes)?;
            Some((crate::depth::depth_target_id(&target), target, extent))
        });
        if let Some(extent) = colour_target_extent_at(&writes) {
            let base = colour_target_at(&writes).map(|target| target.base);
            let target = colour_target_id(extent, base);
            submission.targets.insert(target, extent);
            submission.commands.push(RenderCommand::SetRenderTargets {
                colour: vec![target],
                depth: depth_target.map(|(id, _, _)| id),
            });
        }

        // The draws, found once for every pass that walks them.
        // An offset draw's index size is the `VGT_INDEX_TYPE` in force at it, which a stream may
        // load indirectly rather than write in a packet, and sixteen bits where none wrote it
        // (D754).
        let mut sweep = crate::registers::RegisterSweep::new(&writes);
        let draws = crate::registers::draw_calls_reading(&walked, stream, &mut |at| {
            Some(
                sweep
                    .latest(at, VGT_INDEX_TYPE)
                    .unwrap_or(UNWRITTEN_INDEX_TYPE),
            )
        });
        record_colour_target(&mut submission, &writes, &draws);
        submission.depth_control = depth_control_at(&writes);
        submission.stencil_control = stencil_control_at(&writes);
        submission.blend_control = blend_control_at(&writes);
        // The primitive the draw produces, from `VGT_GS_OUT_PRIM_TYPE`; `prepare` makes it the mesh
        // module's output shape.
        submission.report.primitive_topology = primitive_topology_at(&writes);
        // Each stage's wave width: the encodings are identical at either width, so the stream's
        // registers are the only source (D145).
        submission.report.wave_widths = crate::registers::wave_widths_at(&writes);

        // The guest-memory window this frame's shaders read, read once because every module shares
        // it (D703). The default window at address zero is unmapped and reads empty.
        let candidates = Self::reconcile(queue, registered, &inferred, &mut submission.report);
        span(Span::PrepareEnvironment, || {
            self.set_environment(&candidates, &writes, memory);
        });
        submission.guest_memory = span(Span::PrepareWindow, || {
            read_guest_window(memory, self.window)
        });
        submission.guest_memory_base = self.window.address();
        submission.report.shaders_found = candidates.len();

        let (per_draw, chunks) = span(Span::PrepareShaders, || {
            self.bind_shaders(
                &candidates,
                (queue, registered),
                (&draws, &writes),
                memory,
                &mut submission,
            )
        });
        submission.report.unwindowed_draws =
            self.unwindowed_draws(&per_draw, &submission.guest_memory);
        submission.report.unexported_channel_draws =
            self.unexported_channel_draws(&per_draw, &draws, &writes);
        submission.report.unshaded_draws = unshaded_draws(&per_draw, queue, registered);

        // A fill of the depth surface before the draws is its clear; the surface is never read.
        if let Some(clear) =
            self.depth_fills
                .clear_in_stream((&walked, stream), &draws, depth_target.as_ref())
        {
            submission.commands.push(clear);
        }

        // Draws and dispatches after the binds, in the stream's own packet order.
        span(Span::PrepareGeometry, || {
            push_geometry_commands(
                &mut submission.commands,
                (&walked, stream, &draws),
                &writes,
                (&per_draw, &chunks, depth_target.map(|(id, _, _)| id)),
                (
                    &mut submission.report.unmodelled_viewports,
                    &mut submission.report.unmasked_one_byte_draws,
                ),
            );
        });
        span(Span::PrepareTextures, || {
            self.bind_resources(&mut submission, memory);
        });
        submission.report.draws = submission
            .commands
            .iter()
            .filter(|command| {
                matches!(
                    command,
                    RenderCommand::Draw { .. } | RenderCommand::DrawIndexed { .. }
                )
            })
            .count();

        submission
    }
}

/// The offset of a stream's last `CLEAR_STATE`, which resets register state to its defaults, or
/// `None` when it has none.
fn last_clear(walked: &PacketWalk) -> Option<u32> {
    walked
        .packets
        .iter()
        .rev()
        .find(|packet| {
            matches!(packet.kind, crate::packet::PacketKind::Command { opcode }
                if opcode == crate::cp::CLEAR_STATE)
        })
        .map(|packet| packet.offset)
}

/// A dispatch's images: the one it fetches from and the one it stores to, each as linear texels
/// read from guest memory, and where the stored one lies so what the dispatch wrote can be tiled
/// back.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DispatchImages {
    /// The fetched image.
    pub fetched: Option<DispatchTexels>,
    /// The stored image, as it was before the dispatch.
    pub stored: Option<DispatchTexels>,
    /// Where the stored image lies, and the bytes it spanned before the dispatch.
    pub stored_surface: Option<(DispatchSurface, Vec<u8>)>,
    /// The stored image's compression keys, when they were all cleared: it was read as the zeros
    /// they mean, and once every texel is written back they are marked uncompressed - expanded, a
    /// state the hardware's own writes leave.
    pub stored_keys_to_expand: Option<KeysToExpand>,
}

/// A compressed image's cleared keys, and the surface they cover - every level of its chain, since
/// the keys of all of them are cleared together. Expanding them writes the surface as the zeros
/// they mean and the keys as uncompressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeysToExpand {
    /// The keys' first byte and how many.
    pub keys: (u64, usize),
    /// The chain's first byte and how many it spans.
    pub surface: (u64, usize),
}

/// A dispatch image's texel format: the guest's `8_8_8_8_UNORM`, its single-channel `8_UNORM`, or
/// `32_32_32_32_UINT`, as which radeonsi copies a block-compressed image's blocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TexelFormat {
    /// Four eight-bit normalised channels, a texel a word.
    Rgba8,
    /// One eight-bit normalised channel, a texel a byte.
    R8,
    /// Four thirty-two-bit unsigned channels, a texel four words.
    Rgba32Uint,
}

impl TexelFormat {
    /// Bytes one texel takes.
    #[must_use]
    pub const fn bytes(self) -> u32 {
        match self {
            Self::Rgba8 => 4,
            Self::R8 => 1,
            Self::Rgba32Uint => 16,
        }
    }

    /// Words one texel takes among a dispatch image's texels: one for a texel of a word or less.
    #[must_use]
    pub const fn words(self) -> u32 {
        self.bytes().div_ceil(4)
    }

    /// Whether the format's channels are unsigned integers, which a shader declares as such.
    #[must_use]
    pub const fn is_integer(self) -> bool {
        matches!(self, Self::Rgba32Uint)
    }
}

/// Where a dispatch image lies in guest memory: a tiled level, placed as a colour target of that
/// shape would be, or a linear image's rows at their pitch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispatchSurface {
    /// A tiled level, at four bytes a texel or - in the one-byte layout - one.
    Tiled(ColourTarget),
    /// A linear image's level.
    Linear {
        /// Its first byte.
        base: u64,
        /// Width in texels.
        width: u32,
        /// Height in texels.
        height: u32,
        /// Texels from one row's start to the next's.
        pitch: u32,
        /// Bytes one texel takes.
        bytes_per_texel: u32,
    },
}

impl DispatchSurface {
    /// Its first byte.
    #[must_use]
    pub const fn base(&self) -> u64 {
        match self {
            Self::Tiled(target) => target.base,
            Self::Linear { base, .. } => *base,
        }
    }

    /// Its extent in texels.
    #[must_use]
    pub const fn extent(&self) -> (u32, u32) {
        match self {
            Self::Tiled(target) => (target.width, target.height),
            Self::Linear { width, height, .. } => (*width, *height),
        }
    }

    /// Bytes it spans from [`Self::base`]: a tiled level's whole surface; a linear one's rows at
    /// their pitch, the last ending at its width.
    #[must_use]
    pub fn bytes(&self) -> usize {
        match self {
            Self::Tiled(target) => target.words() * 4,
            Self::Linear {
                width,
                height,
                pitch,
                bytes_per_texel,
                ..
            } => {
                (*pitch as usize * (*height as usize - 1) + *width as usize)
                    * *bytes_per_texel as usize
            }
        }
    }

    /// Its texels, row-major, each its bits in the low bytes of a word, from the bytes it spans.
    #[must_use]
    pub fn detile(&self, spanned: &[u8]) -> Vec<u32> {
        match self {
            Self::Tiled(target) => target.detile_mapped(&words_of_bytes(spanned), |w| w),
            Self::Linear {
                width,
                height,
                pitch,
                bytes_per_texel,
                ..
            } => {
                let size = *bytes_per_texel as usize;
                // A texel wider than a word is its words in order.
                let word = size.min(4);
                (0..*height as usize)
                    .flat_map(|row| {
                        let start = row * *pitch as usize * size;
                        spanned[start..start + *width as usize * size]
                            .chunks_exact(word)
                            .map(|texel| {
                                texel
                                    .iter()
                                    .rev()
                                    .fold(0_u32, |word, &byte| word << 8 | u32::from(byte))
                            })
                    })
                    .collect()
            }
        }
    }

    /// Places row-major texels into the bytes it spans, leaving the rest as they were.
    ///
    /// # Errors
    ///
    /// When the texels or the bytes do not match its extent.
    pub fn tile(&self, texels: &[u32], spanned: &mut [u8]) -> Result<(), String> {
        let (width, height) = self.extent();
        let words = match self {
            Self::Linear {
                bytes_per_texel, ..
            } => bytes_per_texel.div_ceil(4) as usize,
            // A sixteen-byte tiled texel is four words, as a linear one is.
            Self::Tiled(target) => (1_usize << target.layout.texel_log2()).div_ceil(4),
        };
        if texels.len() != width as usize * height as usize * words || spanned.len() < self.bytes()
        {
            return Err("a dispatch image's texels do not match its extent".to_owned());
        }
        match self {
            Self::Tiled(target) => {
                let mut words = words_of_bytes(spanned);
                target
                    .tile_mapped(texels, &mut words, |w| w)
                    .map_err(|e| format!("{e:?}"))?;
                for (bytes, word) in spanned.chunks_exact_mut(4).zip(&words) {
                    bytes.copy_from_slice(&word.to_le_bytes());
                }
                Ok(())
            }
            Self::Linear {
                pitch,
                bytes_per_texel,
                ..
            } => {
                let size = *bytes_per_texel as usize;
                let word = size.min(4);
                for (row, texels) in texels.chunks(width as usize * words).enumerate() {
                    let start = row * *pitch as usize * size;
                    for (bytes, texel) in spanned[start..].chunks_exact_mut(word).zip(texels) {
                        bytes.copy_from_slice(&texel.to_le_bytes()[..word]);
                    }
                }
                Ok(())
            }
        }
    }
}

/// Little-endian words from bytes, a trailing partial word dropped.
fn words_of_bytes(bytes: &[u8]) -> Vec<u32> {
    bytes
        .chunks_exact(4)
        .map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]))
        .collect()
}

/// One image a dispatch binds, as linear texels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchTexels {
    /// Row-major texels, each its bits in the low bytes of a word.
    pub texels: Vec<u32>,
    /// What a texel is.
    pub format: TexelFormat,
    /// Width in texels.
    pub width: u32,
    /// Height in texels.
    pub height: u32,
}

/// Reads a dispatch's images from guest memory, by where its module said each descriptor came
/// from: its user data, or a table its user data and constants address.
///
/// # Errors
///
/// Each refusal by name: a second fetched image, a descriptor not readable or not written, an image
/// that is not a single 2D `8_8_8_8_UNORM` level in a modelled layout, or compression whose keys say
/// anything but what a plain read or write keeps exact.
fn dispatch_images(
    state: &crate::dispatch::DispatchState,
    (textures, storage): &orbistoun_translate::wavefront::ImageSources,
    memory: &impl GuestMemory,
) -> Result<DispatchImages, String> {
    if textures.len() > 1 {
        return Err("the program fetches from more than one image, and one is bound".to_owned());
    }
    let mut images = DispatchImages::default();
    if let Some(source) = textures.first() {
        let (surface, descriptor, format) = dispatch_image_surface(state, source, memory)?;
        let (width, height) = surface.extent();
        let texels = match surface_keys(&descriptor, format, memory)? {
            crate::dcc::Keys::Uncompressed => surface.detile(&read_spanned(&surface, memory)?),
            crate::dcc::Keys::Clear0000 => {
                vec![0; width as usize * height as usize * format.words() as usize]
            }
            crate::dcc::Keys::Other => {
                return Err(concat!(
                    "the fetched image's compression keys need per-block addressing, which is ",
                    "not modelled"
                )
                .to_owned());
            }
        };
        images.fetched = Some(DispatchTexels {
            texels,
            format,
            width,
            height,
        });
    }
    if let Some(source) = storage {
        let (surface, descriptor, format) = dispatch_image_surface(state, source, memory)?;
        let spanned = read_spanned(&surface, memory)?;
        let (width, height) = surface.extent();
        let texels = match surface_keys(&descriptor, format, memory)? {
            crate::dcc::Keys::Uncompressed => surface.detile(&spanned),
            crate::dcc::Keys::Clear0000 => {
                let chain = surface_layout(&descriptor, format).and_then(|layout| {
                    usize::try_from(layout.chain_bytes(
                        descriptor.width,
                        descriptor.height,
                        descriptor.levels,
                    ))
                    .ok()
                });
                images.stored_keys_to_expand = descriptor
                    .compression
                    .zip(surface_key_bytes(&descriptor, format))
                    .zip(chain)
                    .map(|((dcc, bytes), chain)| KeysToExpand {
                        keys: (crate::dcc::meta_start(dcc), bytes),
                        surface: (descriptor.base, chain),
                    });
                vec![0; width as usize * height as usize * format.words() as usize]
            }
            crate::dcc::Keys::Other => {
                return Err(concat!(
                    "the stored image's compression keys need per-block addressing, which is not ",
                    "modelled"
                )
                .to_owned());
            }
        };
        images.stored = Some(DispatchTexels {
            texels,
            format,
            width,
            height,
        });
        // Under cleared keys the level's bytes are the zeros they mean, whatever memory holds.
        let spanned = if images.stored_keys_to_expand.is_some() {
            vec![0; spanned.len()]
        } else {
            spanned
        };
        images.stored_surface = Some((surface, spanned));
    }
    Ok(images)
}

/// The image a dispatch source names: its descriptor read from the user data or its table,
/// decoded, and placed.
fn dispatch_image_surface(
    state: &crate::dispatch::DispatchState,
    source: &TextureSource,
    memory: &impl GuestMemory,
) -> Result<(DispatchSurface, ImageDescriptor, TexelFormat), String> {
    use orbistoun_translate::wavefront::TableWord;
    let user = |index: u32| {
        state
            .user_data
            .get(index as usize)
            .copied()
            .flatten()
            .ok_or_else(|| "an image descriptor's user data was never written".to_owned())
    };
    let mut words = [0u32; 8];
    if let Some(first) = source.user_data {
        for (i, word) in (first..).zip(words.iter_mut()) {
            *word = user(i)?;
        }
    } else {
        let half = |word: TableWord| match word {
            TableWord::UserData(index) => user(index),
            TableWord::Constant(value) => Ok(value),
        };
        let table = u64::from(half(source.table.low)?) | u64::from(half(source.table.high)?) << 32;
        let at = table + u64::from(source.table_offset.unwrap_or(0));
        let bytes = memory
            .read(at, 32)
            .ok_or_else(|| format!("an image descriptor at {at:#x} is not readable"))?;
        for (word, chunk) in words.iter_mut().zip(bytes.chunks_exact(4)) {
            *word = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        }
    }
    let descriptor = decode_image_descriptor(words);
    let format = dispatch_format(&descriptor)?;
    if words[3] >> 28 != IMAGE_TYPE_2D {
        return Err(format!(
            "a dispatch image of type {}, not 2D",
            words[3] >> 28
        ));
    }
    if descriptor.last_level != descriptor.base_level {
        return Err("a dispatch image view of more than one level".to_owned());
    }
    if descriptor.tiling == SwizzleMode::Linear {
        let surface = linear_surface(&descriptor, words[4], format.bytes()).ok_or_else(|| {
            format!(
                "a linear dispatch image whose level is not placed here: {}-byte texels, {}x{}, {} level(s) viewed from {}, pitch field {:#x}{}",
                format.bytes(),
                descriptor.width,
                descriptor.height,
                descriptor.levels,
                descriptor.base_level,
                words[4] & 0x3FFF,
                if descriptor.compression.is_some() {
                    ", compressed"
                } else {
                    ""
                }
            )
        })?;
        return Ok((surface, descriptor, format));
    }
    let surface = surface_layout(&descriptor, format)
        .and_then(|layout| {
            crate::registers::chain_level(
                (descriptor.base, descriptor.pipe_bank_xor),
                (descriptor.width, descriptor.height),
                (descriptor.levels, descriptor.base_level),
                layout,
            )
        })
        .filter(|surface| surface.layout.models(surface.pipe_bank_xor))
        .ok_or_else(|| {
            format!(
                "a {:?} dispatch image of {}-byte texels, {}x{}, {} level(s) viewed from {}, whose layout is not modelled",
                descriptor.tiling,
                format.bytes(),
                descriptor.width,
                descriptor.height,
                descriptor.levels,
                descriptor.base_level,
            )
        })?;
    Ok((DispatchSurface::Tiled(surface), descriptor, format))
}

/// The texel format a dispatch image's descriptor names, where it is one bound here.
fn dispatch_format(descriptor: &ImageDescriptor) -> Result<TexelFormat, String> {
    match descriptor.format {
        FORMAT_8_8_8_8_UNORM => Ok(TexelFormat::Rgba8),
        FORMAT_8_UNORM => Ok(TexelFormat::R8),
        // A sixteen-byte texel is placed linearly, or in `64KB_D_X`; its other layouts are not
        // modelled.
        FORMAT_32_32_32_32_UINT
            if matches!(
                descriptor.tiling,
                SwizzleMode::Linear | SwizzleMode::Tiled64KbDX | SwizzleMode::Tiled4KbDX
            ) =>
        {
            Ok(TexelFormat::Rgba32Uint)
        }
        _ => Err({
            format!(
                "a dispatch image of format {} ({:?}, {}x{}, levels {}..={})",
                descriptor.format,
                descriptor.tiling,
                descriptor.width,
                descriptor.height,
                descriptor.base_level,
                descriptor.last_level
            )
        }),
    }
}

/// What an image's compression keys say about all its blocks: uncompressed for an image without
/// compression.
fn surface_keys(
    descriptor: &ImageDescriptor,
    format: TexelFormat,
    memory: &impl GuestMemory,
) -> Result<crate::dcc::Keys, String> {
    let Some(dcc) = descriptor.compression else {
        return Ok(crate::dcc::Keys::Uncompressed);
    };
    if descriptor.tiling != SwizzleMode::Tiled64KbRX {
        return Err("a compressed image in a layout other than 64KB_R_X".to_owned());
    }
    let bytes = surface_key_bytes(descriptor, format)
        .ok_or_else(|| "a compressed one-byte-texel mip chain".to_owned())?;
    let keys = memory
        .read(crate::dcc::meta_start(dcc), bytes)
        .ok_or_else(|| "an image's compression keys are not readable".to_owned())?;
    Ok(crate::dcc::classify(keys))
}

/// How many bytes a compressed image's keys take, or `None` where their layout is not modelled: a
/// chain's keys at one byte a texel. One level's are whole metadata blocks.
fn surface_key_bytes(descriptor: &ImageDescriptor, format: TexelFormat) -> Option<usize> {
    let dcc = descriptor.compression?;
    let bytes = crate::dcc::texel_chain_meta_bytes(
        descriptor.width,
        descriptor.height,
        descriptor.levels,
        surface_layout(descriptor, format)?,
        dcc.pipe_aligned,
    );
    usize::try_from(bytes).ok()
}

/// The layout a tiled image's texels lie by at its format's size, or `None` where it is not
/// modelled.
fn surface_layout(
    descriptor: &ImageDescriptor,
    format: TexelFormat,
) -> Option<crate::tiling::SurfaceLayout> {
    crate::tiling::SurfaceLayout::of(descriptor.tiling)?.at_bytes_per_texel(format.bytes())
}

/// The level a linear image's view reads first, where its chain places it: one level at the pitch
/// its descriptor gives, or a chain's level at the pitch addrlib's layout gives every level. `None`
/// for a compressed linear image, which has no keys to read it through, or a custom-pitch chain.
fn linear_surface(
    descriptor: &ImageDescriptor,
    word4: u32,
    bytes_per_texel: u32,
) -> Option<DispatchSurface> {
    if descriptor.compression.is_some() {
        return None;
    }
    let level = descriptor.base_level;
    let (offset, pitch) = if descriptor.levels <= 1 {
        (
            0,
            crate::registers::linear_pitch(word4, descriptor.width, bytes_per_texel),
        )
    } else if word4.trailing_zeros() >= 14 {
        crate::tiling::linear_level_of(
            descriptor.width,
            descriptor.height,
            (descriptor.levels, level),
            bytes_per_texel,
        )?
    } else {
        return None;
    };
    Some(DispatchSurface::Linear {
        base: descriptor.base + offset,
        width: (descriptor.width >> level).max(1),
        height: (descriptor.height >> level).max(1),
        pitch,
        bytes_per_texel,
    })
}

/// The bytes a dispatch image spans.
fn read_spanned(surface: &DispatchSurface, memory: &impl GuestMemory) -> Result<Vec<u8>, String> {
    memory
        .read(surface.base(), surface.bytes())
        .map(<[u8]>::to_vec)
        .ok_or_else(|| format!("a dispatch image at {:#x} is not readable", surface.base()))
}

/// The outcome of preparing one shader.
enum Prepared {
    /// Translated just now; the backend has not seen it.
    Fresh {
        resource: ResourceId,
        module: Vec<u32>,
        /// Anything the translation wants the caller told.
        warnings: Vec<String>,
    },
    /// Served from the cache; the backend already has it.
    Cached { resource: ResourceId },
}

impl Pipeline {
    /// Prepares the reconciled candidates and binds them, then finds the shader each draw ran: the
    /// program addresses in force at its packet, translated where the first pass did not, so a
    /// stream that swaps a stage's program between draws binds each draw's own. A registered stage
    /// keeps its registration. Returns each draw's shaders, in draw order, and the chunks of each
    /// draw too large for one wave (D741), empty for one drawn whole.
    fn bind_shaders(
        &mut self,
        candidates: &[Candidate],
        (queue, registered): (Queue, &[RegisteredShader]),
        (draws, writes): (&[DrawCall], &[RegisterWrite]),
        memory: &impl GuestMemory,
        submission: &mut Submission,
    ) -> (Vec<DrawShaders>, Vec<Vec<Chunk>>) {
        let mut by_geometry = self.bind_up_front(candidates, memory, submission);
        let registered_stages: Vec<ShaderStage> = candidates
            .iter()
            .filter(|c| {
                registered
                    .iter()
                    .any(|r| r.address == c.address && r.stage == c.stage)
            })
            .map(|c| c.stage)
            .collect();

        let mut per_draw = Vec::new();
        let mut per_draw_chunks = Vec::new();
        // One pass over the stream's writes for all its draws.
        let mut sweep = crate::registers::RegisterSweep::new(writes);
        let shader_registers: Vec<u32> = self.vocabulary.shader_register_ids().collect();
        // A draw whose shader registers hold the previous draw's values runs the same shaders; a GL
        // frame rewrites the same addresses before each of many draws.
        let mut previous_writes: Option<Vec<Option<u32>>> = None;
        let mut previous_shaders = Vec::new();
        let mut previous_chunks: Vec<ResourceId> = Vec::new();
        // A primitive shader's threads: a 64-lane wave, or half of one under wave32.
        let lanes = 64 >> u32::from(submission.report.wave_widths.primitive_w32);
        let mut sampled_through_samplers = self.samples_through_samplers();
        for draw in draws {
            let mut written: Vec<Option<u32>> = shader_registers
                .iter()
                .map(|&register| sweep.latest(draw.packet_offset, register))
                .collect();
            // A primitive shader prepared per geometry runs another module for other geometry, so
            // the geometry is part of what must repeat.
            let geometry =
                draw_geometry(draw, |register| sweep.latest(draw.packet_offset, register));
            if !by_geometry.needs.is_empty()
                && let Ok(geometry) = geometry
            {
                written.extend([
                    Some(geometry.vertices),
                    Some(geometry.primitives),
                    Some(geometry.first_vertex),
                ]);
            }
            // A window-space draw's primitive shader writes its position differently (D731).
            let window_space = crate::registers::position_space(|register| {
                sweep.latest(draw.packet_offset, register)
            }) == crate::registers::PositionSpace::Window;
            written.push(Some(u32::from(window_space)));
            // The vertex stage's user data, which names the buffers whose formats a format load
            // converts by (D738).
            let vertex_words = stage_user_data(&mut sweep, draw.packet_offset, 0);
            if !by_geometry.needs.is_empty() {
                written.extend(vertex_words.iter().map(|&word| Some(word)));
            }
            // The pixel stage's, which name the samplers whose half-border clamps its samples
            // saturate for (D743).
            let fragment_words = self.draw_user_data(
                (&mut sweep, draw.packet_offset),
                sampled_through_samplers,
                &mut written,
            );
            let plans = geometry
                .as_ref()
                .map_or_else(|_| Vec::new(), |&whole| draw_chunks(draw, whole, lanes));
            if previous_writes.as_ref() == Some(&written) {
                per_draw.push(Vec::clone(&previous_shaders));
                // The same modules; an indexed draw's chunks read its own indices.
                per_draw_chunks.push(chunks_of(&previous_chunks, &plans));
                continue;
            }
            let mut chunked = Vec::new();
            let mut shaders = Vec::new();
            let latest =
                sweep.before_among(draw.packet_offset, self.vocabulary.shader_register_ids());
            let draw_candidates: Vec<Candidate> = shader_candidates(&latest, &self.vocabulary)
                .into_iter()
                .filter_map(|inferred| {
                    let stage = stage_of(&inferred.stage)?;
                    (queue.permits(stage) && !registered_stages.contains(&stage)).then_some(
                        Candidate {
                            address: inferred.address,
                            stage,
                        },
                    )
                })
                .collect();
            // A pixel shader first prepared here may be the first to sample through a sampler.
            sampled_through_samplers |= self.inputs_for_draw(
                &draw_candidates,
                (&fragment_words, memory),
                &mut by_geometry,
                submission,
            );
            for candidate in draw_candidates {
                let stage = candidate.stage;
                let resource = self.bind_chunked(
                    candidate,
                    (memory, &geometry, &plans, window_space, &vertex_words),
                    (&mut by_geometry, &mut chunked),
                    submission,
                );
                if let Some(resource) = resource {
                    shaders.push((stage, resource));
                }
            }
            per_draw.push(shaders.clone());
            per_draw_chunks.push(chunks_of(&chunked, &plans));
            previous_writes = Some(written);
            previous_shaders = shaders;
            previous_chunks = chunked;
        }
        (per_draw, per_draw_chunks)
    }

    /// Prepares and binds the reconciled candidates before any draw. Returns every (stage, address)
    /// prepared and what it became, `None` for a failure, so it is tried and reported once however
    /// many draws name it; a primitive shader that needs a draw's geometry is prepared per geometry
    /// instead (D730).
    fn bind_up_front(
        &mut self,
        candidates: &[Candidate],
        memory: &impl GuestMemory,
        submission: &mut Submission,
    ) -> ByGeometry {
        let mut by_geometry = ByGeometry::default();
        for &candidate in candidates {
            let key = (candidate.stage as u32, candidate.address);
            let plain = self.plain_key(candidate.stage, candidate.address, false);
            match self.prepare_candidate(candidate, (memory, ForDraw::default()), submission) {
                Ok(resource) => {
                    by_geometry.plain.insert(plain, Some(resource));
                    submission.commands.push(RenderCommand::BindShader {
                        stage: candidate.stage,
                        shader: resource,
                    });
                }
                Err(Unprepared::Failed) => {
                    by_geometry.plain.insert(plain, None);
                }
                // Bound per draw, with each draw's geometry.
                Err(Unprepared::NeedsGeometry(reason)) => {
                    by_geometry.needs.insert(key, reason);
                }
            }
        }
        by_geometry
    }

    /// The flat twins a draw gives its pixel shader's attributes read both interpolated and flat
    /// (D742): one location each, from the first above every parameter its primitive shader exports
    /// and every attribute its pixel shader reads, so neither stage's own locations move. `None`
    /// where the pixel shader reads no attribute both ways, or either program is not yet decoded.
    fn draw_flat_twins(&self, candidates: &[Candidate]) -> Option<FlatTwins> {
        let program = |stage: ShaderStage| {
            let candidate = candidates.iter().find(|c| c.stage == stage)?;
            let bytes = self.decoded.get(&candidate.address)?;
            Some(decode_program(bytes, &self.encodings, &self.operands))
        };
        let fragment = program(ShaderStage::Fragment)?;
        let mixed = orbistoun_translate::wavefront::mixed_attributes(&fragment, &self.encodings);
        if mixed.is_empty() {
            return None;
        }
        let read = orbistoun_translate::wavefront::read_attributes(&fragment, &self.encodings);
        let exported = program(ShaderStage::Vertex).map_or_else(Vec::new, |vertex| {
            orbistoun_translate::wavefront::exported_parameters(&vertex, &self.encodings)
        });
        let first = read
            .iter()
            .chain(&exported)
            .max()
            .map_or(0, |highest| highest + 1);
        FlatTwins::from_first(&mixed, first)
    }

    /// Sets each stage's user-data count and `DX10_CLAMP` to what the registers say at the draw at
    /// `at`, which the draw's modules are translated for and repeat only with, and answers the pixel
    /// stage's words - also part of what must repeat where `with_words` (D743).
    fn draw_user_data(
        &mut self,
        (sweep, at): (&mut crate::registers::RegisterSweep<'_>, u32),
        with_words: bool,
        written: &mut Vec<Option<u32>>,
    ) -> [u32; USER_DATA_WORDS] {
        if self.feeds_user_data {
            for (held, at_draw) in self
                .user_data
                .iter_mut()
                .zip(user_data_layouts_at(sweep, at))
            {
                held.count = at_draw.count;
                held.dx10_clamp = at_draw.dx10_clamp;
            }
            written.extend(self.user_data.iter().map(|layout| Some(layout.count)));
        }
        let words = stage_user_data(sweep, at, 1);
        if with_words {
            written.extend(words.iter().map(|&word| Some(word)));
        }
        words
    }

    /// The key a module prepared for no particular draw is held under in a submission: its stage,
    /// address and position space, and the user-data count it was translated for, since a stream
    /// can give one program two counts.
    fn plain_key(&self, stage: ShaderStage, address: u64, window_space: bool) -> PlainKey {
        let count = match stage {
            ShaderStage::Vertex => self.user_data[0].count,
            ShaderStage::Fragment => self.user_data[1].count,
            ShaderStage::Compute => 0,
        };
        (stage as u32, address, window_space, count)
    }

    /// Whether any module translated so far samples through a sampler it loaded, so a draw's pixel
    /// user data can change what its pixel shader saturates (D743).
    fn samples_through_samplers(&self) -> bool {
        self.texture_sources
            .values()
            .flatten()
            .any(|source| source.sampler_offset.is_some())
    }

    /// Records in `by_geometry` what a draw gives its pixel shader beside its programs: the flat
    /// twins of its attributes (D742) and the coordinates it saturates (D743). Whether any module
    /// now samples through a sampler.
    fn inputs_for_draw(
        &mut self,
        candidates: &[Candidate],
        (words, memory): (&[u32; USER_DATA_WORDS], &impl GuestMemory),
        by_geometry: &mut ByGeometry,
        submission: &mut Submission,
    ) -> bool {
        by_geometry.flat_twins = self.draw_flat_twins(candidates);
        by_geometry.saturated =
            self.draw_saturated(candidates, (words, memory), by_geometry, submission);
        self.samples_through_samplers()
    }

    /// The coordinates a draw's pixel shader saturates for each texture slot (D743): those the
    /// sampler the draw binds there clamps to half a border, read where the pixel shader prepared
    /// for no particular draw loads it from - prepared here first when no earlier draw did. None
    /// where that shader samples through no sampler.
    fn draw_saturated(
        &mut self,
        candidates: &[Candidate],
        (words, memory): (&[u32; USER_DATA_WORDS], &impl GuestMemory),
        by_geometry: &mut ByGeometry,
        submission: &mut Submission,
    ) -> [[bool; 2]; 2] {
        let mut saturated = UNSATURATED;
        let Some(&fragment) = candidates.iter().find(|c| c.stage == ShaderStage::Fragment) else {
            return saturated;
        };
        let plain = self.plain_key(ShaderStage::Fragment, fragment.address, false);
        if let std::collections::btree_map::Entry::Vacant(entry) = by_geometry.plain.entry(plain) {
            entry.insert(
                self.prepare_candidate(fragment, (memory, ForDraw::default()), submission)
                    .ok(),
            );
        }
        let Some(sources) = by_geometry
            .plain
            .get(&plain)
            .copied()
            .flatten()
            .and_then(|resource| self.texture_sources.get(&resource))
        else {
            return saturated;
        };
        for source in sources {
            let (Some(slot), Some(offset)) = (
                saturated.get_mut(source.slot as usize),
                source.sampler_offset,
            ) else {
                continue;
            };
            let at = table_address(source.table, words) + u64::from(offset);
            if let Some(sampling) = read_sampler(at, memory) {
                *slot = sampling.saturated();
            }
        }
        saturated
    }

    /// [`Self::bind_for_draw`] for a draw that may be split into chunks (D741): a primitive shader
    /// seeded with the geometry runs one module per chunk, each listed in `chunked`, and the draw is
    /// unshaded if any chunk's cannot be prepared; one that is not seeded runs the draw whole.
    fn bind_chunked(
        &mut self,
        candidate: Candidate,
        (memory, geometry, plans, window_space, words): (
            &impl GuestMemory,
            &Result<GeometryInputs, String>,
            &[ChunkPlan],
            bool,
            &[u32; USER_DATA_WORDS],
        ),
        (by_geometry, chunked): (&mut ByGeometry, &mut Vec<ResourceId>),
        submission: &mut Submission,
    ) -> Option<ResourceId> {
        let first = plans
            .first()
            .map_or_else(|| geometry.clone(), |plan| Ok(plan.geometry));
        let resource = self.bind_for_draw(
            candidate,
            (memory, &first, window_space, words),
            by_geometry,
            submission,
        );
        if candidate.stage != ShaderStage::Vertex
            || resource.is_none()
            || plans.is_empty()
            || !by_geometry
                .needs
                .contains_key(&(candidate.stage as u32, candidate.address))
        {
            return resource;
        }
        let modules: Option<Vec<ResourceId>> = plans
            .iter()
            .map(|plan| {
                self.bind_for_draw(
                    candidate,
                    (memory, &Ok(plan.geometry), window_space, words),
                    by_geometry,
                    submission,
                )
            })
            .collect();
        *chunked = modules.unwrap_or_default();
        chunked.first().copied()
    }

    /// Binds what each draw's shaders read beside their user data, for a backend that supplies it:
    /// the textures they sample and the buffers they read through (D733).
    fn bind_resources(&mut self, submission: &mut Submission, memory: &impl GuestMemory) {
        submission.report.textures = texture_census(&submission.commands, memory);
        if !self.feeds_user_data {
            return;
        }
        bind_textures(
            &mut submission.commands,
            (&self.texture_sources, &mut self.texels),
            memory,
            &mut submission.report.unbound_textures,
        );
        crate::draw_buffers::bind_draw_buffers(
            &mut submission.commands,
            (&self.buffer_sources, &self.index_readers, &mut self.buffers),
            memory,
            &mut submission.report.unbound_buffers,
        );
    }

    /// Records how a translated module reaches guest memory: the buffers a draw binds for it
    /// (D733), and whether anything else it reads goes through the window. A pixel shader's
    /// exported colour channels are recorded with them, from the same decode.
    fn note_memory(
        &mut self,
        resource: ResourceId,
        (shader, decoded): (&[u8], Option<&orbistoun_shader::Decode>),
        (stage, user_data): (Stage, UserData),
    ) {
        let again;
        let decoded = if let Some(decoded) = decoded {
            decoded
        } else {
            again = decode_program(shader, &self.encodings, &self.operands);
            &again
        };
        // The translation refused more buffers than a stage binds, so what is kept here is what
        // the module declares.
        let buffers = orbistoun_translate::wavefront::draw_buffers_for(
            decoded,
            &self.encodings,
            stage,
            user_data,
        )
        .unwrap_or_default();
        if let Some(access) = reaches_guest_memory(decoded, &self.encodings, &buffers.served) {
            self.memory_readers.insert(resource, access);
        }
        if stage == Stage::Fragment {
            self.colour_channels.insert(
                resource,
                orbistoun_translate::wavefront::exported_colour_channels(decoded, &self.encodings),
            );
        }
        if !buffers.sources.is_empty() {
            self.buffer_sources.insert(resource, buffers.sources);
        }
    }

    /// How many draws run a module that reaches guest memory while `window` is empty, and the
    /// first one's access. A module reaches guest memory only through the window; with none mapped
    /// it reads zeros, so such a draw is counted and refused rather than drawn from them.
    fn unwindowed_draws(
        &self,
        per_draw: &[DrawShaders],
        window: &[u32],
    ) -> (usize, Option<String>) {
        if !window.is_empty() {
            return (0, None);
        }
        let mut first = None;
        let count = per_draw
            .iter()
            .filter(|shaders| {
                let access = shaders
                    .iter()
                    .find_map(|(_, resource)| self.memory_readers.get(resource));
                if first.is_none() {
                    first = access.cloned();
                }
                access.is_some()
            })
            .count();
        (count, first)
    }

    /// How many draws write a colour channel their pixel shader never exports: the channels
    /// `CB_TARGET_MASK` and the target's format let through ([`crate::registers::colour_write_mask`],
    /// every one where the stream set no mask) beyond those the shader's exports write. Such a
    /// channel would hold whatever the output held, so the draw is counted and refused.
    fn unexported_channel_draws(
        &self,
        per_draw: &[DrawShaders],
        draws: &[DrawCall],
        writes: &[RegisterWrite],
    ) -> usize {
        let mut sweep = crate::registers::RegisterSweep::new(writes);
        draws
            .iter()
            .zip(per_draw)
            .filter(|(draw, shaders)| {
                let Some(exported) = shaders.iter().find_map(|(stage, resource)| {
                    (*stage == ShaderStage::Fragment)
                        .then(|| self.colour_channels.get(resource))
                        .flatten()
                }) else {
                    return false;
                };
                let at = draw.packet_offset;
                let mask = sweep
                    .latest(at, crate::registers::CB_TARGET_MASK)
                    .unwrap_or(0xF);
                let info = sweep
                    .latest(at, crate::registers::CB_COLOR0_INFO)
                    .unwrap_or(0);
                crate::registers::colour_write_mask(mask, info) & !exported != 0
            })
            .count()
    }

    /// The module one draw binds for a candidate: the one prepared without geometry, or - for a
    /// primitive shader that reads the geometry engine's inputs - the one for this draw's geometry
    /// (D730). A primitive shader of a window-space draw is its own module (D731). Each is prepared
    /// once however many draws name it.
    fn bind_for_draw(
        &mut self,
        candidate: Candidate,
        (memory, geometry, window_space, words): (
            &impl GuestMemory,
            &Result<GeometryInputs, String>,
            bool,
            &[u32; USER_DATA_WORDS],
        ),
        by_geometry: &mut ByGeometry,
        submission: &mut Submission,
    ) -> Option<ResourceId> {
        // Only the primitive shader writes a position.
        let window_space = window_space && candidate.stage == ShaderStage::Vertex;
        let key = (candidate.stage as u32, candidate.address);
        // A draw that gives attributes flat twins prepares both stages for them (D742), and one
        // whose samplers clamp to half a border its pixel shader (D743).
        let saturates =
            candidate.stage == ShaderStage::Fragment && by_geometry.saturated != UNSATURATED;
        if !by_geometry.needs.contains_key(&key) && by_geometry.flat_twins.is_none() && !saturates {
            let plain = self.plain_key(candidate.stage, candidate.address, window_space);
            if let Some(known) = by_geometry.plain.get(&plain) {
                return *known;
            }
            let for_draw = ForDraw {
                geometry: None,
                window_space,
                buffer_formats: None,
                flat_twins: None,
                saturated: UNSATURATED,
            };
            match self.prepare_candidate(candidate, (memory, for_draw), submission) {
                Ok(resource) => {
                    by_geometry.plain.insert(plain, Some(resource));
                    return Some(resource);
                }
                Err(Unprepared::Failed) => {
                    by_geometry.plain.insert(plain, None);
                    return None;
                }
                Err(Unprepared::NeedsGeometry(reason)) => {
                    by_geometry.needs.insert(key, reason);
                }
            }
        }
        let needs = by_geometry.needs.get(&key).cloned().unwrap_or_default();
        let buffer_formats = (candidate.stage == ShaderStage::Vertex)
            .then(|| self.draw_buffer_formats(candidate.address, words, memory))
            .flatten();
        self.prepare_with_geometry(
            candidate,
            (memory, Some(geometry.clone()), needs, window_space),
            buffer_formats,
            by_geometry,
            submission,
        )
    }

    /// The fourth word of each buffer descriptor a vertex shader with a format load reads through,
    /// at a draw with these user-data words (D738); `None` for a shader without one, or one whose
    /// buffers are not traced. A slot whose descriptor word cannot be resolved stays `None`, and a
    /// format load through it is then refused.
    fn draw_buffer_formats(
        &mut self,
        address: u64,
        words: &[u32; USER_DATA_WORDS],
        memory: &impl GuestMemory,
    ) -> Option<orbistoun_translate::wavefront::BufferFormats> {
        let traced = self.format_words(address)?;
        let mut formats = orbistoun_translate::wavefront::BufferFormats::default();
        for (slot, source) in formats.0.iter_mut().zip(traced.iter()) {
            *slot = crate::draw_buffers::resolve_fourth_word(source, words, memory);
        }
        Some(formats)
    }

    /// Where each of a primitive shader's buffers' fourth descriptor word comes from, by slot, for
    /// the vertex stage's layout now; `None` for a shader that makes no format load, whose formats
    /// no draw needs. Traced once per program and layout and kept: a draw only resolves the words,
    /// and a frame draws one program thousands of times. A program rewritten in place is traced
    /// again.
    fn format_words(&mut self, address: u64) -> Option<FormatWords> {
        let shader = self.decoded.get(&address)?;
        let key = (address, self.user_data[0].count);
        // The whole program's hash: a rewrite can change one word in the middle.
        let content = content_hash(shader);
        if let Some((held, traced)) = self.format_words.get(&key)
            && *held == content
        {
            return traced.clone();
        }
        let decoded = decode_program(shader, &self.encodings, &self.operands);
        let traced =
            orbistoun_translate::wavefront::reads_buffer_formats(&decoded, &self.encodings)
                .then(|| {
                    let user_data = UserData {
                        draw_buffers: true,
                        ..self.user_data[0]
                    };
                    orbistoun_translate::wavefront::draw_buffers_for(
                        &decoded,
                        &self.encodings,
                        host_stage(ShaderStage::Vertex),
                        user_data,
                    )
                    .ok()
                })
                .flatten()
                .map(|buffers| -> FormatWords { buffers.sources.into() });
        self.format_words.insert(key, (content, traced.clone()));
        traced
    }

    /// Prepares a primitive shader that reads the geometry engine's inputs for one draw's geometry
    /// (D730), once per geometry however many draws share it; a draw whose geometry is not one
    /// that is seeded records why, once.
    fn prepare_with_geometry(
        &mut self,
        candidate: Candidate,
        (memory, geometry, needs, window_space): (
            &impl GuestMemory,
            Option<Result<GeometryInputs, String>>,
            String,
            bool,
        ),
        buffer_formats: Option<orbistoun_translate::wavefront::BufferFormats>,
        by_geometry: &mut ByGeometry,
        submission: &mut Submission,
    ) -> Option<ResourceId> {
        let mut refuse = |reason: String, submission: &mut Submission| {
            if by_geometry
                .refused
                .insert((candidate.address, reason.clone()))
            {
                submission.report.failures.push(ShaderFailure {
                    address: candidate.address,
                    stage: format!("{:?}", candidate.stage).to_lowercase(),
                    reason,
                });
            }
        };
        let flat_twins = by_geometry.flat_twins;
        let (geometry, unseeded) = match geometry {
            // Only the primitive shader is seeded with the geometry.
            Some(_) if candidate.stage != ShaderStage::Vertex => (None, None),
            // The draw's counts travel in its words, not the module (D745): one module serves every
            // draw that assembles and indexes the same way.
            Some(Ok(geometry)) => (
                Some(GeometryInputs {
                    vertices: 0,
                    primitives: 0,
                    ..geometry
                }),
                None,
            ),
            // A shader prepared per draw for its buffers' formats (D738) or its flat twins (D742) is
            // tried without the geometry; one that also reads the geometry is then refused, saying
            // why.
            Some(Err(why)) if buffer_formats.is_some() || flat_twins.is_some() => (None, Some(why)),
            Some(Err(why)) => {
                refuse(
                    format!("{needs}; and this draw's cannot be: {why}"),
                    submission,
                );
                return None;
            }
            None => return None,
        };
        let saturated = if candidate.stage == ShaderStage::Fragment {
            by_geometry.saturated
        } else {
            UNSATURATED
        };
        let for_draw = ForDraw {
            geometry,
            window_space,
            buffer_formats,
            flat_twins,
            saturated,
        };
        let key = (candidate.stage as u32, candidate.address, for_draw);
        if let Some(known) = by_geometry.prepared.get(&key) {
            return *known;
        }
        let made = match self.prepare_candidate(candidate, (memory, for_draw), submission) {
            Ok(resource) => Some(resource),
            Err(Unprepared::NeedsGeometry(reason)) => {
                let why = unseeded.unwrap_or_else(|| "it was not given".to_owned());
                let reason = format!("{reason}; and this draw's geometry cannot be seeded: {why}");
                if by_geometry
                    .refused
                    .insert((candidate.address, reason.clone()))
                {
                    submission.report.failures.push(ShaderFailure {
                        address: candidate.address,
                        stage: format!("{:?}", candidate.stage).to_lowercase(),
                        reason,
                    });
                }
                None
            }
            Err(Unprepared::Failed) => None,
        };
        by_geometry.prepared.insert(key, made);
        made
    }

    /// Prepares one shader candidate and records the outcome in the submission's report: the module
    /// travels with the submission when the backend has not seen it, and a failure is counted and
    /// named. The resource it became, or `None` when it could not be prepared.
    fn prepare_candidate(
        &mut self,
        candidate: Candidate,
        (memory, for_draw): (&impl GuestMemory, ForDraw),
        submission: &mut Submission,
    ) -> Result<ResourceId, Unprepared> {
        match self.prepare(
            candidate.address,
            candidate.stage,
            (
                submission.report.primitive_topology,
                submission.report.wave_widths,
                for_draw,
            ),
            memory,
        ) {
            Ok(prepared) => {
                // The address resolved, whatever happened to the shader after that.
                submission.report.addresses_resolved += 1;
                submission.report.shaders_translated += 1;
                let indexed = for_draw.geometry.is_some_and(|g| g.indices.is_some());
                Ok(match prepared {
                    // Only a module the backend has not seen travels with the submission.
                    Prepared::Fresh {
                        resource,
                        module,
                        warnings,
                    } => {
                        submission.report.warnings.extend(warnings);
                        submission.modules.insert(resource, module);
                        if indexed {
                            self.index_readers.insert(resource);
                        }
                        resource
                    }
                    Prepared::Cached { resource } => {
                        submission.report.cache_hits += 1;
                        if indexed {
                            self.index_readers.insert(resource);
                        }
                        resource
                    }
                })
            }
            // Not a failure yet: it is prepared again with each draw's geometry.
            Err(PrepareFailure::NeedsGeometry(reason)) => Err(Unprepared::NeedsGeometry(reason)),
            Err(failure) => {
                // Counted before the reason is consumed, and apart from the shader outcome: address
                // resolution is evidence about the address space.
                match failure {
                    PrepareFailure::Unresolved(_) => {
                        submission.report.addresses_unresolved += 1;
                    }
                    PrepareFailure::Resolved(_) | PrepareFailure::NeedsGeometry(_) => {
                        submission.report.addresses_resolved += 1;
                    }
                }
                submission.report.failures.push(ShaderFailure {
                    address: candidate.address,
                    stage: format!("{:?}", candidate.stage).to_lowercase(),
                    reason: failure.reason(),
                });
                Err(Unprepared::Failed)
            }
        }
    }

    /// Decides which shader addresses to prepare, from both routes.
    ///
    /// Registration wins where the two overlap, and every overlap is counted. Agreement and
    /// disagreement are the only evidence about the register vocabulary, so the inferred route runs
    /// even when registration has answered.
    fn reconcile(
        queue: Queue,
        registered: &[RegisteredShader],
        inferred: &[crate::registers::ShaderCandidate],
        report: &mut SubmissionReport,
    ) -> Vec<Candidate> {
        report.registered = registered.len();
        report.inferred = inferred.len();

        let mut chosen: BTreeMap<u32, Candidate> = BTreeMap::new();
        let key = |stage: ShaderStage| stage as u32;

        for entry in registered {
            if !queue.permits(entry.stage) {
                report
                    .impossible_stages
                    .push(format!("registered {:?} on {queue:?}", entry.stage));
                continue;
            }
            chosen.insert(
                key(entry.stage),
                Candidate {
                    address: entry.address,
                    stage: entry.stage,
                },
            );
        }

        for candidate in inferred {
            let Some(stage) = stage_of(&candidate.stage) else {
                continue;
            };
            if !queue.permits(stage) {
                report
                    .impossible_stages
                    .push(format!("inferred {stage:?} on {queue:?}"));
                continue;
            }
            match chosen.get(&key(stage)) {
                Some(existing) if existing.address == candidate.address => report.agreed += 1,
                Some(existing) => report.disagreed.push(Disagreement {
                    stage,
                    registered: existing.address,
                    inferred: candidate.address,
                }),
                None => {
                    chosen.insert(
                        key(stage),
                        Candidate {
                            address: candidate.address,
                            stage,
                        },
                    );
                }
            }
        }

        chosen.into_values().collect()
    }

    /// Reads, decodes, translates and caches the shader at a guest address.
    ///
    /// Decode, then hash, then check the cache, then translate. Decoding first keys the cache on
    /// the shader's own bytes rather than the window it was read from, and keeps the expensive
    /// step, translation, behind the cache.
    fn prepare(
        &mut self,
        address: u64,
        stage: ShaderStage,
        (topology, widths, for_draw): (Option<PrimitiveTopology>, WaveWidths, ForDraw),
        memory: &impl GuestMemory,
    ) -> Result<Prepared, PrepareFailure> {
        // A shader already decoded at this address with the same bytes is not decoded again: a
        // decode is a function of the bytes.
        if let Some(known) = self.decoded.get(&address)
            && let Some(bytes) = memory.read(guest_address_of(address), known.len())
            && bytes == known.as_slice()
        {
            return self.prepare_decoded(
                address,
                stage,
                (topology, widths, for_draw),
                (bytes, None),
            );
        }
        // The register named a GPU address; guest memory is indexed by a guest one. See
        // `guest_address_of`.
        let window = read_window(memory, guest_address_of(address)).ok_or_else(|| {
            PrepareFailure::Unresolved(format!(
                concat!(
                    "no mapped memory at {:#x}. That address came from a hardware ",
                    "register and is a GPU virtual address; it is being read as a guest ",
                    "virtual address on the assumption the two coincide, which is not yet ",
                    "confirmed - so suspect that before suspecting the register decode"
                ),
                address
            ))
        })?;

        let decoded = decode_program(window, &self.encodings, &self.operands);
        if !decoded.terminated {
            return Err(PrepareFailure::Resolved(format!(
                concat!(
                    "no end-of-program instruction within {} bytes of {:#x} - the ",
                    "address is wrong, or this shader is larger than the window"
                ),
                window.len(),
                address
            )));
        }
        if !decoded.is_trustworthy() {
            return Err(PrepareFailure::Resolved(format!(
                concat!(
                    "the shader at {:#x} did not decode cleanly ",
                    "(desynchronised={}, overran={})"
                ),
                address, decoded.desynchronised, decoded.overran
            )));
        }

        let shader = &window[..decoded.consumed];
        self.decoded.insert(address, shader.to_vec());
        self.prepare_decoded(
            address,
            stage,
            (topology, widths, for_draw),
            (shader, Some(&decoded)),
        )
    }

    /// Translates and caches a shader whose bytes, end-of-program included, are known, or finds its
    /// translation in the cache.
    ///
    /// `decoded` is the decode of exactly `shader` when the caller has one. Otherwise the decode is
    /// repeated only if a translation is needed; it is a function of the bytes, so it is the same.
    fn prepare_decoded(
        &mut self,
        address: u64,
        stage: ShaderStage,
        (topology, widths, for_draw): (Option<PrimitiveTopology>, WaveWidths, ForDraw),
        (shader, decoded): (&[u8], Option<&orbistoun_shader::Decode>),
    ) -> Result<Prepared, PrepareFailure> {
        let host_stage = host_stage(stage);
        // The primitive the mesh output assembles, from the stream's topology. Only the mesh stage
        // emits primitives; a stream that set no topology draws triangles.
        let primitive = if host_stage == Stage::Mesh {
            match topology {
                Some(t) => {
                    mesh_primitive_of(t).map_err(|e| PrepareFailure::Resolved(e.to_string()))?
                }
                None => MeshPrimitive::default(),
            }
        } else {
            MeshPrimitive::default()
        };
        // Keyed by content, stage and primitive: one instruction stream translated for different
        // stages or output shapes gives different modules.
        let user_data = match stage {
            ShaderStage::Vertex => UserData {
                geometry: for_draw.geometry,
                window_space: for_draw.window_space,
                buffer_formats: for_draw.buffer_formats,
                flat_twins: for_draw.flat_twins,
                ..self.user_data[0]
            },
            ShaderStage::Fragment => UserData {
                flat_twins: for_draw.flat_twins,
                saturated: for_draw.saturated,
                ..self.user_data[1]
            },
            ShaderStage::Compute => UserData::default(),
        };
        // A live draw binds the buffers its stages read through (D733).
        let user_data = UserData {
            draw_buffers: self.feeds_user_data && host_stage != Stage::Compute,
            ..user_data
        };
        let (strategy, width_salt) = stage_strategy(self.strategy, host_stage, widths);
        // The user-data layout and the wave width are in the module too.
        let key = content_hash(shader)
            ^ stage_salt(host_stage)
            ^ primitive_salt(primitive)
            ^ window_salt(self.window)
            ^ width_salt
            ^ (u64::from(user_data.count) << 56 | u64::from(user_data.first_register) << 48)
            ^ crate::pixel_inputs::salt(user_data)
            ^ for_draw_salt(for_draw)
            ^ if user_data.draw_buffers {
                DRAW_BUFFERS_SALT
            } else {
                0
            };
        if let Some(&cached) = self.cache.get(&key) {
            if cached.matches(shader) {
                return Ok(Prepared::Cached {
                    resource: cached.resource,
                });
            }
            // The key matched and the shader did not. Refused rather than re-translated: a hash
            // collision and a key that changed meaning are both faults in this crate.
            return Err(PrepareFailure::Resolved(format!(
                concat!(
                    "the shader at {:#x} hashes to a cached entry it does not match ",
                    "({} bytes against {}) - the cache key no longer identifies a shader"
                ),
                address,
                shader.len(),
                cached.length
            )));
        }

        let context = crate::translations::Context {
            strategy,
            stage: host_stage,
            primitive,
            window: self.window,
            user_data,
        };
        // A translation kept from an earlier run is the module this one would make (D113).
        let kept = if let Some(kept) = self.store.as_ref().and_then(|store| store.get(key, shader))
        {
            kept.clone()
        } else {
            let kept = self.translate_now(address, shader, decoded, context)?;
            if let Some(store) = self.store.as_mut() {
                store.insert(key, kept.clone());
            }
            kept
        };

        let resource = ResourceId(self.next_resource);
        self.next_resource += 1;
        self.cache.insert(key, Cached::of(resource, shader));
        // Where each texture the module samples comes from, for binding them per draw.
        self.texture_sources.insert(resource, kept.textures);
        self.note_memory(resource, (shader, decoded), (host_stage, user_data));
        Ok(Prepared::Fresh {
            resource,
            module: kept.module,
            warnings: kept
                .warnings
                .iter()
                .map(|warning| format!("the shader at {address:#x}: {warning}"))
                .collect(),
        })
    }

    /// Translates a shader whose bytes are known, decoding them again when the caller has no
    /// decode.
    fn translate_now(
        &self,
        address: u64,
        shader: &[u8],
        decoded: Option<&orbistoun_shader::Decode>,
        context: crate::translations::Context,
    ) -> Result<crate::translations::Kept, PrepareFailure> {
        let again;
        let decoded = if let Some(decoded) = decoded {
            decoded
        } else {
            again = decode_program(shader, &self.encodings, &self.operands);
            if !again.terminated || !again.is_trustworthy() {
                return Err(PrepareFailure::Resolved(format!(
                    concat!(
                        "the shader at {:#x} decoded cleanly before and does not now, from the ",
                        "same {} bytes - a decode that is not a function of its bytes"
                    ),
                    address,
                    shader.len()
                )));
            }
            &again
        };
        let translated = translate_with_user_data(
            decoded,
            &self.encodings,
            context.strategy,
            (context.stage, context.primitive),
            context.window,
            context.user_data,
        )
        .map_err(|e| {
            let reason = format!("the shader at {address:#x} could not be translated: {e}");
            if matches!(
                e,
                orbistoun_translate::TranslateError::ReadsGeometryInputs
                    | orbistoun_translate::TranslateError::NeedsBufferFormats
                    | orbistoun_translate::TranslateError::NeedsFlatTwins
            ) {
                PrepareFailure::NeedsGeometry(reason)
            } else {
                PrepareFailure::Resolved(reason)
            }
        })?;
        Ok(crate::translations::Kept {
            bytes: shader.to_vec(),
            context,
            module: translated.module,
            textures: translated.textures,
            warnings: translated
                .warnings
                .iter()
                .map(ToString::to_string)
                .collect(),
        })
    }
}

/// A stage's user-data words as they stand at `at`, the stage by its index in
/// [`USER_DATA_REGISTERS`]; a word the stream never wrote reads zero.
fn stage_user_data(
    sweep: &mut crate::registers::RegisterSweep<'_>,
    at: u32,
    stage: usize,
) -> [u32; USER_DATA_WORDS] {
    let mut words = [0u32; USER_DATA_WORDS];
    let (_, first) = USER_DATA_REGISTERS[stage];
    for (register, word) in (first..).zip(words.iter_mut()) {
        *word = sweep.latest(at, register).unwrap_or(0);
    }
    words
}

/// Colour target zero's output state in force at a draw, as commands where it changed since
/// `sent`: the blend state, when the stream set one, as `SetBlend`, and the channels the draw
/// writes, when it set a target mask, as `SetWriteMask`. A one-byte target with no target mask is
/// counted: a drawer would write every channel of the frame standing in for it.
fn push_colour_output(
    commands: &mut Vec<RenderCommand>,
    mut latest: impl FnMut(u32) -> Option<u32>,
    (blend_sent, sent): (&mut Option<u32>, &mut Option<u8>),
    unmasked_one_byte_draws: &mut usize,
) {
    if let Some(value) = latest(crate::registers::CB_BLEND0_CONTROL)
        && *blend_sent != Some(value)
    {
        commands.push(RenderCommand::SetBlend(decode_blend_control(value)));
        *blend_sent = Some(value);
    }
    let info = latest(crate::registers::CB_COLOR0_INFO);
    match latest(crate::registers::CB_TARGET_MASK) {
        Some(mask) => {
            let channels = crate::registers::colour_write_mask(mask, info.unwrap_or(0));
            if *sent != Some(channels) {
                commands.push(RenderCommand::SetWriteMask(channels));
                *sent = Some(channels);
            }
        }
        None if info.is_some_and(|info| {
            crate::registers::decode_colour_target_format(info).format == crate::registers::COLOR_8
        }) =>
        {
            *unmasked_one_byte_draws += 1;
        }
        None => {}
    }
}

/// Appends a submission's draws and dispatches to its command list, in the stream's order.
///
/// An auto draw becomes a [`RenderCommand::Draw`] and an indexed one a
/// [`RenderCommand::DrawIndexed`]; the index buffer's address is decoded (`DrawKind::Indexed`) but
/// bound separately. Each `DISPATCH_DIRECT` becomes a [`RenderCommand::Dispatch`] carrying only
/// workgroup counts, since its guest memory is a windowed buffer the backend binds itself.
fn push_geometry_commands(
    commands: &mut Vec<RenderCommand>,
    (walked, stream, draws): (&PacketWalk, &[u8], &[DrawCall]),
    writes: &[RegisterWrite],
    (shaders, chunks, depth_target): (&[DrawShaders], &[Vec<Chunk>], Option<ResourceId>),
    (unmodelled_viewports, unmasked_one_byte_draws): (&mut usize, &mut usize),
) {
    let mut sent: [Option<[u32; USER_DATA_WORDS]>; 2] = [None, None];
    let mut blend_sent = None;
    let mut mask_sent = None;
    let mut depth_sent = crate::depth::DrawStateSent::default();
    let mut transform_sent = None;
    let mut scissor_sent = None;
    // What each stage has bound so far: the up-front binds, then each draw's own.
    let mut bound: Vec<(ShaderStage, ResourceId)> = commands
        .iter()
        .filter_map(|command| match command {
            RenderCommand::BindShader { stage, shader } => Some((*stage, *shader)),
            _ => None,
        })
        .collect();
    // Each draw's state from the latest writes before it, found in one pass and read a register at
    // a time.
    let mut sweep = crate::registers::RegisterSweep::new(writes);
    for (index, draw) in draws.iter().enumerate() {
        let at = draw.packet_offset;
        // The shader each stage runs for this draw, bound when it is not the one already bound.
        for &(stage, shader) in shaders.get(index).map_or(&[][..], Vec::as_slice) {
            if !bound.contains(&(stage, shader)) {
                bound.retain(|(s, _)| *s != stage);
                bound.push((stage, shader));
                commands.push(RenderCommand::BindShader { stage, shader });
            }
        }
        // The blend state and write mask in force at this draw.
        push_colour_output(
            commands,
            |register| sweep.latest(at, register),
            (&mut blend_sent, &mut mask_sent),
            unmasked_one_byte_draws,
        );
        // The depth, stencil and cull state in force at this draw, and a clear it asks for.
        depth_sent.push(&mut sweep, at, depth_target, commands);
        // The clip-to-pixel transform in force at this draw, when the stream set one and it
        // changed. A window-space draw's primitive shader wrote positions this fixed transform
        // takes back to pixels (D731); a draw whose positions are in any other space is counted,
        // so it is refused rather than drawn as if they were clip space.
        let transform =
            match crate::registers::position_space(|register| sweep.latest(at, register)) {
                crate::registers::PositionSpace::Clip => {
                    viewport_transform_from(|register| sweep.latest(at, register))
                }
                crate::registers::PositionSpace::Window => Some(ViewportTransform {
                    x_scale: WINDOW_SPACE_SCALE,
                    x_offset: 0.0,
                    y_scale: WINDOW_SPACE_SCALE,
                    y_offset: 0.0,
                    depth: crate::registers::DepthMapping::IDENTITY,
                }),
                crate::registers::PositionSpace::Unmodelled(_) => {
                    *unmodelled_viewports += 1;
                    None
                }
            };
        if let Some(transform) = transform
            && transform_sent != Some(transform)
        {
            commands.push(RenderCommand::SetViewportTransform(transform));
            transform_sent = Some(transform);
        }
        // The scissor in force at this draw, as the rectangle the backend restricts it to, when the
        // stream set one and it changed. A draw before any is unrestricted.
        if let Some(scissor) = crate::registers::scissor_from(|register| sweep.latest(at, register))
        {
            let rect = Rect {
                x: i32::try_from(scissor.x).unwrap_or(0),
                y: i32::try_from(scissor.y).unwrap_or(0),
                width: scissor.width,
                height: scissor.height,
            };
            if scissor_sent != Some(rect) {
                commands.push(RenderCommand::SetViewport(rect));
                scissor_sent = Some(rect);
            }
        }
        // Each stage's user data as it stands at this draw, emitted when it changed. A word the
        // stream never wrote reads zero.
        for (slot, (stage, first)) in USER_DATA_REGISTERS.into_iter().enumerate() {
            let mut words = [0u32; USER_DATA_WORDS];
            for (register, word) in (first..).zip(words.iter_mut()) {
                *word = sweep.latest(at, register).unwrap_or(0);
            }
            if sent[slot] != Some(words) {
                commands.push(RenderCommand::SetUserData { stage, words });
                sent[slot] = Some(words);
            }
        }
        if let Some(chunks) = chunks.get(index).filter(|chunks| !chunks.is_empty()) {
            push_chunks(commands, chunks, draw.instances, &mut bound);
            continue;
        }
        commands.push(draw_command(draw, sweep.latest(at, VGT_INDEX_TYPE)));
    }
    for dispatch in dispatch_calls(walked, stream) {
        commands.push(RenderCommand::Dispatch {
            x: dispatch.groups[0],
            y: dispatch.groups[1],
            z: dispatch.groups[2],
        });
    }
}

/// The command that draws `draw` whole; an indexed one's indices are as wide as its
/// `VGT_INDEX_TYPE` says.
fn draw_command(draw: &DrawCall, index_type: Option<u32>) -> RenderCommand {
    match draw.kind {
        DrawKind::Auto { vertices } => RenderCommand::Draw {
            vertices,
            instances: draw.instances,
            first_vertex: 0,
        },
        DrawKind::Indexed { indices, address } => RenderCommand::DrawIndexed {
            indices,
            instances: draw.instances,
            first_index: 0,
            index_buffer: index_width(index_type).map(|width| IndexBuffer::Guest {
                address,
                bytes: width.bytes(),
            }),
        },
    }
}

/// A chunked draw's chunks (D741), each binding its own primitive shader and drawing its indices.
fn push_chunks(
    commands: &mut Vec<RenderCommand>,
    chunks: &[Chunk],
    instances: u32,
    bound: &mut Vec<(ShaderStage, ResourceId)>,
) {
    for chunk in chunks {
        let shader = (ShaderStage::Vertex, chunk.vertex);
        if !bound.contains(&shader) {
            bound.retain(|(stage, _)| *stage != ShaderStage::Vertex);
            bound.push(shader);
            commands.push(RenderCommand::BindShader {
                stage: shader.0,
                shader: shader.1,
            });
        }
        commands.push(RenderCommand::DrawIndexed {
            indices: chunk.indices,
            instances,
            first_index: 0,
            index_buffer: Some(chunk.index_buffer),
        });
    }
}

/// Each stage's first user-data register, as an absolute register index.
///
/// `SPI_SHADER_USER_DATA_PS_0` is `0x2C0C`: a hardware draw capture shows the pixel shader's
/// descriptor-table pointer there (`data/packets.toml`). `SPI_SHADER_USER_DATA_GS_0` is `0x2C8C`:
/// `gfx103.json` maps it at byte `45616` (`0xB230`), and the open-toolchain GL context writes its
/// per-draw vertex offset there (oops-sdk `gl_draw.c`). The vertex stage reads the `GS` set because
/// the vertex program runs as the primitive-shader geometry stage (D688).
const USER_DATA_REGISTERS: [(ShaderStage, u32); 2] = [
    (ShaderStage::Vertex, 0x2C8C),
    (ShaderStage::Fragment, 0x2C0C),
];

/// The distinct image descriptors the draws' pixel shaders name, in first-use order.
///
/// Each fragment [`RenderCommand::SetUserData`]'s words 0 and 1 are a descriptor table's address;
/// its first eight words are the image descriptor. A table that is not readable names nothing; a
/// draw with no texture passes zeros, and zero is not mapped.
fn texture_census(commands: &[RenderCommand], memory: &impl GuestMemory) -> Vec<ImageDescriptor> {
    let mut textures: Vec<ImageDescriptor> = Vec::new();
    for command in commands {
        let RenderCommand::SetUserData {
            stage: ShaderStage::Fragment,
            words,
        } = command
        else {
            continue;
        };
        let table = u64::from(words[0]) | u64::from(words[1]) << 32;
        let Some(bytes) = memory.read(table, 32) else {
            continue;
        };
        let mut descriptor = [0u32; 8];
        for (word, chunk) in descriptor.iter_mut().zip(bytes.chunks_exact(4)) {
            *word = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        }
        let texture = decode_image_descriptor(descriptor);
        if !textures.contains(&texture) {
            textures.push(texture);
        }
    }
    textures
}

/// The address of a descriptor table as the program formed it: from the stage's user-data `words`
/// or constants, a missing word reading zero.
fn table_address(table: TableBase, words: &[u32]) -> u64 {
    let half = |word: TableWord| match word {
        TableWord::UserData(index) => usize::try_from(index)
            .ok()
            .and_then(|index| words.get(index).copied())
            .unwrap_or(0),
        TableWord::Constant(value) => value,
    };
    u64::from(half(table.low)) | u64::from(half(table.high)) << 32
}

/// The sampler descriptor at `at`, decoded, or `None` when it is not readable or asks for sampling
/// no host sampler reproduces exactly ([`crate::registers::decode_sampler_descriptor`]).
fn read_sampler(at: u64, memory: &impl GuestMemory) -> Option<crate::registers::TextureSampling> {
    let bytes = memory.read(at, 16)?;
    let mut words = [0u32; 4];
    for (word, chunk) in words.iter_mut().zip(bytes.chunks_exact(4)) {
        *word = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
    }
    crate::registers::decode_sampler_descriptor(words).ok()
}

/// The accesses a dispatch's own buffers take, by instruction offset, each buffer's guest range as
/// `(address, bytes)`, by slot (D746), and each buffer's descriptor format word, by slot, for a
/// program converting by it (D738).
type DispatchBuffers = (
    BTreeMap<u32, u32>,
    Vec<(u64, u64)>,
    Option<orbistoun_translate::wavefront::BufferFormats>,
);

/// Whether two `(address, bytes)` ranges share a byte.
fn overlaps((a, a_bytes): (u64, u64), (b, b_bytes): (u64, u64)) -> bool {
    a < b.saturating_add(b_bytes) && b < a.saturating_add(a_bytes)
}

/// `SQ_IMG_RSRC_WORD3.TYPE` for a 2D image: 9 (Mesa `ac_descriptors.c:372`, as the SDK's
/// `gl_pack_descriptors` cites it).
const IMAGE_TYPE_2D: u32 = 9;

/// `GFX10_FORMAT_8_8_8_8_UNORM` (`gfx10-rsrc.json:61`).
const FORMAT_8_8_8_8_UNORM: u32 = 56;
/// `GFX10_FORMAT_8_UNORM` (`gfx10-rsrc.json:6`): one eight-bit normalised channel.
const FORMAT_8_UNORM: u32 = 1;
/// `GFX10_FORMAT_BC3_UNORM` (`gfx10-rsrc.json:127`): four-by-four texels to a sixteen-byte block.
const FORMAT_BC3_UNORM: u32 = 173;
/// `GFX10_FORMAT_32_32_32_32_UINT` (`gfx10-rsrc.json:80`).
const FORMAT_32_32_32_32_UINT: u32 = 75;

/// What each texture a submission binds became, by its descriptor's address, slot, sampler address
/// and the coordinates the module saturates: read once per submission.
type TexturesRead = BTreeMap<(u64, u32, Option<u64>, [bool; 2]), Option<RenderCommand>>;

/// Inserts a [`RenderCommand::BindTexture`] after each fragment `SetUserData` whose descriptor
/// table names a texture read exactly, so the draws that follow sample the guest's own texels.
fn bind_textures(
    commands: &mut Vec<RenderCommand>,
    (sources, texels): (&BTreeMap<ResourceId, Vec<TextureSource>>, &mut TexelCache),
    memory: &impl GuestMemory,
    unbound: &mut usize,
) {
    // Each draw's textures, where its pixel shader says they are: for every slot the bound fragment
    // module samples, the descriptor at that slot's table offset (zero for a module whose texture
    // did not come from the table). Re-emitted before a draw whenever the table or the module
    // changed.
    let mut out = Vec::with_capacity(commands.len());
    let (mut words, mut fragment, mut stale) = (None, None, false);
    let mut read: TexturesRead = BTreeMap::new();
    for command in commands.drain(..) {
        match &command {
            RenderCommand::SetUserData {
                stage: ShaderStage::Fragment,
                words: set,
            } => {
                words = Some(*set);
                stale = true;
            }
            RenderCommand::BindShader {
                stage: ShaderStage::Fragment,
                shader,
            } => {
                fragment = Some(*shader);
                stale = true;
            }
            RenderCommand::Draw { .. } | RenderCommand::DrawIndexed { .. } if stale => {
                stale = false;
                let slots = fragment
                    .and_then(|module| sources.get(&module))
                    .map_or(&[][..], Vec::as_slice);
                // A module this pipeline did not translate gets the one default texture source.
                let default = [TextureSource {
                    slot: 0,
                    table_offset: None,
                    table: TableBase::default(),
                    sampler_offset: None,
                    user_data: None,
                    sampler_user_data: None,
                    saturated: [false; 2],
                }];
                let slots = if fragment.is_some_and(|m| !sources.contains_key(&m)) {
                    &default[..]
                } else {
                    slots
                };
                for source in slots {
                    let user = words.as_ref().map_or(&[][..], |w| &w[..]);
                    // A descriptor still in the user data the stage was seeded with is read from
                    // those words, with a sampler beside it there.
                    if let Some(first) = source.user_data {
                        let bound = user_data_texture(source, first, user, memory, texels);
                        if bound.is_none() {
                            *unbound += 1;
                        }
                        out.extend(bound);
                        continue;
                    }
                    // The table where the program formed its address: user-data words or
                    // constants.
                    let table = table_address(source.table, user);
                    let at = table + u64::from(source.table_offset.unwrap_or(0));
                    // Read once per descriptor per submission: guest memory does not change while a
                    // submission is prepared, and a frame binds the same few textures many times.
                    let sampler_at = source
                        .sampler_offset
                        .map(|offset| table + u64::from(offset));
                    let bound = read
                        .entry((at, source.slot, sampler_at, source.saturated))
                        .or_insert_with(|| {
                            if is_null_descriptor(at, memory) {
                                return Some(null_texture(source.slot));
                            }
                            let mut bound = read_texture(at, source.slot, memory, texels)?;
                            // A sampler the module named is read and honoured; one that cannot be
                            // honoured exactly leaves the texture unbound, so the draw is refused
                            // rather than sampled some other way.
                            if let Some(sampler_at) = sampler_at {
                                let decoded = read_sampler(sampler_at, memory)?;
                                // A half-border clamp is honoured only by a module that saturates
                                // the coordinates it names (D743).
                                if decoded.saturated() != source.saturated {
                                    return None;
                                }
                                if let RenderCommand::BindTexture { sampling, .. } = &mut bound {
                                    *sampling = decoded;
                                }
                            }
                            Some(bound)
                        })
                        .clone();
                    // A texture the module samples and nothing binds draws the backend's
                    // placeholder, which is not the guest's picture: counted, so the draw is
                    // refused rather than drawn wrong.
                    if bound.is_none() {
                        *unbound += 1;
                    }
                    out.extend(bound);
                }
            }
            _ => {}
        }
        out.push(command);
    }
    *commands = out;
}

/// The texture a descriptor in the user data names, from word `first`, sampled as the sampler beside
/// it there says when the module samples through one: the texture a pixel shader that never loads
/// its descriptor reads. `None` where the words are not all there, or where the image or the sampler
/// cannot be read exactly.
fn user_data_texture(
    source: &TextureSource,
    first: u32,
    user: &[u32],
    memory: &impl GuestMemory,
    texels: &mut TexelCache,
) -> Option<RenderCommand> {
    let words_from = |start: u32, count: usize| {
        let start = usize::try_from(start).ok()?;
        user.get(start..start.checked_add(count)?)
    };
    let descriptor: [u32; 8] = words_from(first, 8)?.try_into().ok()?;
    if descriptor.iter().all(|&word| word == 0) {
        return Some(null_texture(source.slot));
    }
    let mut bound = texture_from_words(descriptor, source.slot, memory, texels)?;
    if let Some(sampler) = source.sampler_user_data {
        let words: [u32; 4] = words_from(sampler, 4)?.try_into().ok()?;
        let decoded = crate::registers::decode_sampler_descriptor(words).ok()?;
        // A half-border clamp is honoured only by a module that saturates the coordinates it names
        // (D743).
        if decoded.saturated() != source.saturated {
            return None;
        }
        if let RenderCommand::BindTexture { sampling, .. } = &mut bound {
            *sampling = decoded;
        }
    }
    Some(bound)
}

/// Whether the image descriptor at `at` is all zero: a texture unit nothing bound, as RADV writes a
/// null descriptor (`radv_write_image_descriptor`, `memset(dst, 0, size)`).
fn is_null_descriptor(at: u64, memory: &impl GuestMemory) -> bool {
    memory
        .read(at, 32)
        .is_some_and(|bytes| bytes.iter().all(|&b| b == 0))
}

/// What an all-zero image descriptor samples as: zero in every channel. Its `DST_SEL_X` .. `_W` are
/// all `SQ_SEL_0` ([`destination_selects`]), so a sample returns that constant whatever the texel
/// and whatever its sampler says; one texel of zero carries it.
fn null_texture(slot: u32) -> RenderCommand {
    RenderCommand::BindTexture {
        slot,
        hash: crate::content_hash(&[0]),
        texels: std::sync::Arc::from([0_u32]),
        width: 1,
        height: 1,
        sampling: crate::registers::TextureSampling::default(),
        encoding: crate::backend::TextureEncoding::Rgba8,
    }
}

/// The texture a descriptor table's first eight words name, as a [`RenderCommand::BindTexture`], or
/// `None` for a table that is not readable or a texture that is not 2D, linear, `8_8_8_8_UNORM`.
///
/// The row pitch is the descriptor's, not the width: `ADDR_SW_LINEAR` aligns a row to 256 bytes
/// (Mesa `gfx9addrlib.cpp:5117-5127`), and for a 2D image `SQ_IMG_RSRC_WORD4` carries `pitch - 1`
/// in bits 0-13 when the pitch exceeds the width, zero otherwise (`gfx10-rsrc.json:401-406`). Rows
/// are hashed in guest memory and texels already held in `texels` at the same place and shape are
/// shared rather than copied again.
fn read_texture(
    table: u64,
    slot: u32,
    memory: &impl GuestMemory,
    texels: &mut TexelCache,
) -> Option<RenderCommand> {
    let bytes = memory.read(table, 32)?;
    let mut words = [0u32; 8];
    for (word, chunk) in words.iter_mut().zip(bytes.chunks_exact(4)) {
        *word = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
    }
    texture_from_words(words, slot, memory, texels)
}

/// [`read_texture`] for a descriptor's eight words already in hand.
fn texture_from_words(
    words: [u32; 8],
    slot: u32,
    memory: &impl GuestMemory,
    texels: &mut TexelCache,
) -> Option<RenderCommand> {
    let descriptor = decode_image_descriptor(words);
    if descriptor.format == FORMAT_BC3_UNORM {
        return read_bc3_texture(&descriptor, words[3], slot, memory, texels);
    }
    let (format, selects) = sampled_format(&descriptor, words[3])?;
    match (descriptor.tiling, format) {
        (SwizzleMode::Linear, TexelFormat::Rgba8) => {}
        // The tiled layouts modelled, each sampled as it is rendered (crate::tiling); a one-byte
        // linear image with no pitch of its own lies as its layout's chain places it.
        (SwizzleMode::Tiled64KbRX | SwizzleMode::Tiled4KbDX | SwizzleMode::Linear, _)
            if descriptor.tiling != SwizzleMode::Linear || words[4].trailing_zeros() >= 14 =>
        {
            return read_tiled_texture(&descriptor, format, slot, memory, texels)
                .map(|bound| swizzled_texture(bound, format, selects));
        }
        _ => return None,
    }
    let Some(DispatchSurface::Linear {
        base,
        width,
        height,
        pitch,
        ..
    }) = linear_surface(&descriptor, words[4], 4)
    else {
        return None;
    };
    let descriptor = ImageDescriptor {
        base,
        width,
        height,
        ..descriptor
    };
    let (width, height) = (width as usize, height as usize);
    let row_bytes = pitch as usize * 4;
    let span = row_bytes * (height - 1) + width * 4;
    let key = (descriptor.base, descriptor.width, descriptor.height, pitch);
    // Nothing written since it was read, as far as the host's write tracking reports: the cached
    // texels stand.
    if let Some(cached) = texels.get(&key)
        && cached.since.and_then(|since| {
            orbistoun_mem::watch::written_since(descriptor.base, span as u64, since)
        }) == Some(false)
    {
        return Some(swizzled_texture(
            RenderCommand::BindTexture {
                slot,
                hash: cached.hash,
                texels: cached.texels.clone(),
                width: descriptor.width,
                height: descriptor.height,
                sampling: crate::registers::TextureSampling::default(),
                encoding: crate::backend::TextureEncoding::Rgba8,
            },
            format,
            selects,
        ));
    }
    // Marked before the bytes are hashed, so a write racing the hash is seen next time.
    let since = orbistoun_mem::watch::mark(descriptor.base, span as u64);
    let texels_bytes = memory.read(descriptor.base, span)?;
    let rows = || (0..height).map(|row| &texels_bytes[row * row_bytes..][..width * 4]);
    let mut hasher = crate::ContentHasher::new(width * height);
    for row in rows() {
        hasher.bytes(row);
    }
    let hash = hasher.finish();
    let shared = match texels.get(&key) {
        Some(cached) if cached.hash == hash => cached.texels.clone(),
        _ => {
            let mut gathered = Vec::with_capacity(width * height);
            for row in rows() {
                gathered.extend(
                    row.chunks_exact(4)
                        .map(|t| u32::from_le_bytes([t[0], t[1], t[2], t[3]])),
                );
            }
            gathered.into()
        }
    };
    if texels.len() >= TEXEL_CACHE_ENTRIES && !texels.contains_key(&key) {
        texels.clear();
    }
    texels.insert(
        key,
        CachedTexels {
            since,
            hash,
            texels: std::sync::Arc::clone(&shared),
        },
    );
    Some(swizzled_texture(
        RenderCommand::BindTexture {
            slot,
            hash,
            texels: shared,
            width: descriptor.width,
            height: descriptor.height,
            sampling: crate::registers::TextureSampling::default(),
            encoding: crate::backend::TextureEncoding::Rgba8,
        },
        format,
        selects,
    ))
}

/// A 2D texture's texel format and selects, when both are ones this samples exactly
/// ([`selects_exactly`]): `8_8_8_8_UNORM` or `8_UNORM`.
fn sampled_format(descriptor: &ImageDescriptor, word3: u32) -> Option<(TexelFormat, [u32; 4])> {
    let format = match descriptor.format {
        FORMAT_8_8_8_8_UNORM => TexelFormat::Rgba8,
        FORMAT_8_UNORM => TexelFormat::R8,
        _ => return None,
    };
    let selects = destination_selects(word3);
    (word3 >> 28 == IMAGE_TYPE_2D && selects_exactly(format, selects)).then_some((format, selects))
}

/// `SQ_IMG_RSRC_WORD3`'s `DST_SEL_X` .. `DST_SEL_W` (bits 0-2, 3-5, 6-8, 9-11, `gfx10-rsrc.json`):
/// what each channel a sample returns is, as `SQ_SEL_XYZW01` names it - 0 and 1 the constants, 4
/// to 7 the texel's X to W (`gfx6.json`; radeonsi writes them through `ac_map_swizzle`).
const fn destination_selects(word3: u32) -> [u32; 4] {
    [
        word3 & 7,
        (word3 >> 3) & 7,
        (word3 >> 6) & 7,
        (word3 >> 9) & 7,
    ]
}

/// `SQ_SEL_X`: the texel's first channel.
const SELECT_X: u32 = 4;
/// The selects that return a texel as it is.
const IDENTITY_SELECTS: [u32; 4] = [SELECT_X, SELECT_X + 1, SELECT_X + 2, SELECT_X + 3];

/// Whether every select is one this reads exactly for `format`: a constant, or a channel the format
/// holds. A one-byte texel holds X alone; what a select of its Y, Z or W returns is not taken on
/// trust, and the reserved codes are refused.
fn selects_exactly(format: TexelFormat, selects: [u32; 4]) -> bool {
    selects.iter().all(|&select| match select {
        0 | 1 | SELECT_X => true,
        5..=7 => format == TexelFormat::Rgba8,
        _ => false,
    })
}

/// A bound texture's texels as its selects return them, as linear `Rgba8` words: a one-byte texel
/// is its X and the constants, a four-byte one its channels rearranged. A four-byte texture under
/// the identity selects is returned as it is; any other's hash is moved by its selects, so the
/// same bytes read another way are never taken for each other.
fn swizzled_texture(bound: RenderCommand, format: TexelFormat, selects: [u32; 4]) -> RenderCommand {
    let RenderCommand::BindTexture {
        slot,
        hash,
        texels,
        width,
        height,
        sampling,
        encoding: crate::backend::TextureEncoding::Rgba8,
    } = bound
    else {
        return bound;
    };
    if format == TexelFormat::Rgba8 && selects == IDENTITY_SELECTS {
        return RenderCommand::BindTexture {
            slot,
            hash,
            texels,
            width,
            height,
            sampling,
            encoding: crate::backend::TextureEncoding::Rgba8,
        };
    }
    let pick = |channels: [u8; 4], select: u32| match select {
        1 => 0xFF,
        4..=7 => channels[(select - SELECT_X) as usize],
        _ => 0,
    };
    let swizzled: Vec<u32> = texels
        .iter()
        .map(|&texel| {
            let channels = match format {
                // A draw's texture is never a sixteen-byte one: `sampled_format` reads no such
                // format, so its arm is the four-byte one's.
                TexelFormat::Rgba8 | TexelFormat::Rgba32Uint => texel.to_le_bytes(),
                TexelFormat::R8 => [texel.to_le_bytes()[0], 0, 0, 0xFF],
            };
            u32::from_le_bytes(selects.map(|select| pick(channels, select)))
        })
        .collect();
    let moved = crate::content_hash(&[
        selects[0],
        selects[1],
        selects[2],
        selects[3],
        format.bytes(),
    ]);
    RenderCommand::BindTexture {
        slot,
        hash: hash ^ moved.rotate_left(17),
        texels: swizzled.into(),
        width,
        height,
        sampling,
        encoding: crate::backend::TextureEncoding::Rgba8,
    }
}

/// A texture's texels as last read, with their content hash and the host's mark of when.
#[derive(Debug)]
struct CachedTexels {
    since: Option<u64>,
    hash: u64,
    texels: std::sync::Arc<[u32]>,
}

/// Texels read from guest memory, by where they lie and their shape.
type TexelCache = std::collections::HashMap<(u64, u32, u32, u32), CachedTexels>;

/// A tiled texture's first viewed level, read whole, detiled into row-major texels and bound: a
/// four-byte texel a word, a one-byte texel its byte in the low eight bits.
///
/// The level is where its chain places it, a mip-tail level at its origin in the tail's block,
/// read through the same tiling a colour target of that shape is drawn through. A compressed
/// surface's keys decide what it holds: every key uncompressed, the bytes as they are; every key
/// cleared to `0000`, zero throughout; anything else needs per-block key addressing and binds
/// nothing, so the draw is refused rather than sampled from compressed bytes.
///
/// The cache key's last word is no pitch a linear surface has - a tag per layout and tail origin,
/// and whether the keys cleared it - so the same memory read another way never shares texels.
fn read_tiled_texture(
    descriptor: &ImageDescriptor,
    format: TexelFormat,
    slot: u32,
    memory: &impl GuestMemory,
    texels: &mut TexelCache,
) -> Option<RenderCommand> {
    let layout =
        crate::tiling::SurfaceLayout::of(descriptor.tiling)?.at_bytes_per_texel(format.bytes())?;
    let surface = crate::registers::chain_level(
        (descriptor.base, descriptor.pipe_bank_xor),
        (descriptor.width, descriptor.height),
        (descriptor.levels, descriptor.base_level),
        layout,
    )?;
    if !surface.layout.models(surface.pipe_bank_xor) {
        return None;
    }
    let cleared = texture_keys_cleared(descriptor, surface.layout, memory)?;
    let (width, height) = (surface.width, surface.height);
    let span = surface.words() * 4;
    let layout_tag = match surface.layout {
        crate::tiling::SurfaceLayout::Rx64Kb => 0,
        crate::tiling::SurfaceLayout::Dx4Kb => 1 << 31,
        crate::tiling::SurfaceLayout::Linear => 1 << 27,
        crate::tiling::SurfaceLayout::Rx64KbBpp1 => 1 << 26,
        crate::tiling::SurfaceLayout::LinearBpp1 => 1 << 25,
        crate::tiling::SurfaceLayout::Dx64KbBpp16 => 1 << 24,
        crate::tiling::SurfaceLayout::Dx4KbBpp16 => 1 << 23,
    };
    let tail_tag = match surface.place {
        crate::registers::Place::Whole => 0,
        crate::registers::Place::Within { origin: (x, y), .. } => {
            1 << 30 | (x & 0x3FFF) << 14 | (y & 0x3FFF)
        }
    };
    let tag = layout_tag | tail_tag | u32::from(cleared) << 29 | 1 << 28;
    let key = (surface.base, width, height, tag);
    if let Some(cached) = texels.get(&key)
        && cached
            .since
            .and_then(|since| orbistoun_mem::watch::written_since(surface.base, span as u64, since))
            == Some(false)
    {
        return Some(RenderCommand::BindTexture {
            slot,
            hash: cached.hash,
            texels: cached.texels.clone(),
            width,
            height,
            sampling: crate::registers::TextureSampling::default(),
            encoding: crate::backend::TextureEncoding::Rgba8,
        });
    }
    let since = orbistoun_mem::watch::mark(surface.base, span as u64);
    let (hash, shared): (u64, std::sync::Arc<[u32]>) = if cleared {
        let zeros = vec![0u32; width as usize * height as usize];
        (crate::content_hash(&zeros), zeros.into())
    } else {
        let bytes = memory.read(surface.base, span)?;
        let mut hasher = crate::ContentHasher::new(span / 4);
        hasher.bytes(bytes);
        let hash = hasher.finish();
        let shared = match texels.get(&key) {
            Some(cached) if cached.hash == hash => cached.texels.clone(),
            _ => {
                let words: Vec<u32> = bytes
                    .chunks_exact(4)
                    .map(|t| u32::from_le_bytes([t[0], t[1], t[2], t[3]]))
                    .collect();
                surface.detile_mapped(&words, |w| w).into()
            }
        };
        (hash, shared)
    };
    if texels.len() >= TEXEL_CACHE_ENTRIES && !texels.contains_key(&key) {
        texels.clear();
    }
    texels.insert(
        key,
        CachedTexels {
            since,
            hash,
            texels: std::sync::Arc::clone(&shared),
        },
    );
    Some(RenderCommand::BindTexture {
        slot,
        hash,
        texels: shared,
        width,
        height,
        sampling: crate::registers::TextureSampling::default(),
        encoding: crate::backend::TextureEncoding::Rgba8,
    })
}

/// A `BC3_UNORM` texture's blocks, read whole from its `64KB_S` surface and laid out row-major, a
/// block (four words) for every four-by-four texels, for the host to sample natively. Only a 2D,
/// single-level, uncompressed image returning its channels as they are is read; any other is `None`,
/// and the draw is refused rather than sampled some other way.
fn read_bc3_texture(
    descriptor: &ImageDescriptor,
    word3: u32,
    slot: u32,
    memory: &impl GuestMemory,
    texels: &mut TexelCache,
) -> Option<RenderCommand> {
    const WORDS_PER_BLOCK: usize = 4;
    let single_level = descriptor.levels == 1 && descriptor.base_level == descriptor.last_level;
    if word3 >> 28 != IMAGE_TYPE_2D
        || destination_selects(word3) != IDENTITY_SELECTS
        || descriptor.tiling != SwizzleMode::Tiled64KbS
        || descriptor.compression.is_some()
        || !single_level
    {
        return None;
    }
    let (width, height) = (descriptor.width, descriptor.height);
    let (across, down) = (width.div_ceil(4), height.div_ceil(4));
    let span = crate::tiling::surface_bytes_64kb_s_bpp16(across, down);
    let key = (descriptor.base, width, height, 1 << 22);
    if let Some(cached) = texels.get(&key)
        && cached.since.and_then(|since| {
            orbistoun_mem::watch::written_since(descriptor.base, span as u64, since)
        }) == Some(false)
    {
        return Some(bc3_bind(
            slot,
            cached.hash,
            cached.texels.clone(),
            (width, height),
        ));
    }
    let since = orbistoun_mem::watch::mark(descriptor.base, span as u64);
    let bytes = memory.read(descriptor.base, span)?;
    let mut blocks = Vec::with_capacity(across as usize * down as usize * WORDS_PER_BLOCK);
    for y in 0..down {
        for x in 0..across {
            let at = crate::tiling::tiled_byte_offset_64kb_s_bpp16(x, y, across);
            let block = bytes.get(at..at + WORDS_PER_BLOCK * 4)?;
            blocks.extend(
                block
                    .chunks_exact(4)
                    .map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]])),
            );
        }
    }
    let hash = crate::content_hash(&blocks);
    let shared: std::sync::Arc<[u32]> = blocks.into();
    if texels.len() >= TEXEL_CACHE_ENTRIES && !texels.contains_key(&key) {
        texels.clear();
    }
    texels.insert(
        key,
        CachedTexels {
            since,
            hash,
            texels: std::sync::Arc::clone(&shared),
        },
    );
    Some(bc3_bind(slot, hash, shared, (width, height)))
}

/// A [`RenderCommand::BindTexture`] of `BC3` blocks.
fn bc3_bind(
    slot: u32,
    hash: u64,
    texels: std::sync::Arc<[u32]>,
    (width, height): (u32, u32),
) -> RenderCommand {
    RenderCommand::BindTexture {
        slot,
        hash,
        texels,
        width,
        height,
        sampling: crate::registers::TextureSampling::default(),
        encoding: crate::backend::TextureEncoding::Bc3,
    }
}

/// Whether a texture's compression keys say every block is cleared to zero: `Some(false)` for an
/// uncompressed texture or keys all uncompressed, `None` for keys that need per-block addressing or
/// a compressed layout whose keys are not modelled.
fn texture_keys_cleared(
    descriptor: &ImageDescriptor,
    layout: crate::tiling::SurfaceLayout,
    memory: &impl GuestMemory,
) -> Option<bool> {
    let Some(dcc) = descriptor.compression else {
        return Some(false);
    };
    if !matches!(
        layout,
        crate::tiling::SurfaceLayout::Rx64Kb | crate::tiling::SurfaceLayout::Rx64KbBpp1
    ) {
        return None;
    }
    let bytes = crate::dcc::texel_chain_meta_bytes(
        descriptor.width,
        descriptor.height,
        descriptor.levels,
        layout,
        dcc.pipe_aligned,
    );
    let keys = memory.read(crate::dcc::meta_start(dcc), usize::try_from(bytes).ok()?)?;
    match crate::dcc::classify(keys) {
        crate::dcc::Keys::Uncompressed => Some(false),
        crate::dcc::Keys::Clear0000 => Some(true),
        crate::dcc::Keys::Other => None,
    }
}

/// How many textures the cache holds before it restarts: enough for a frame's set, and a bound for
/// a guest streaming new textures every frame.
const TEXEL_CACHE_ENTRIES: usize = 64;

/// Each stage's `SPI_SHADER_PGM_RSRC2`, vertex (the geometry set) then fragment: `gfx103.json` maps
/// `RSRC2_GS` at byte `45612` (register `0x2C8B`) and `RSRC2_PS` at `45100` (`0x2C0B`), and both
/// carry `USER_SGPR` in bits 1-5.
const RSRC2_REGISTERS: [u32; 2] = [0x2C8B, 0x2C0B];

/// Each stage's `SPI_SHADER_PGM_RSRC1`, in the same order: `gfx103.json` maps `RSRC1_GS` at byte
/// `45608` (`0x2C8A`) and `RSRC1_PS` at `45096` (`0x2C0A`), with `DX10_CLAMP` in bit 21.
const RSRC1_REGISTERS: [u32; 2] = [0x2C8A, 0x2C0A];

/// `SPI_SHADER_PGM_RSRC1.DX10_CLAMP`, bit 21 (`gfx103.json`).
const DX10_CLAMP_BIT: u32 = 1 << 21;

/// Where each stage's user data lands, how much there is, and where it sits in the block: the count
/// from the stage's last `RSRC2` write; the first register `s8` for the geometry program (the eight
/// before it are the wave's own, and a vertex program reads its per-draw word from `s8`) and `s0`
/// for the pixel shader; the vertex stage's words first in the block, the fragment stage's after.
fn user_data_layouts(writes: &[RegisterWrite]) -> [UserData; 2] {
    user_data_layouts_by(|register| {
        writes
            .iter()
            .rev()
            .find(|write| write.register == register)
            .map(|write| write.value)
    })
}

/// [`user_data_layouts`] as the registers stand at the draw at byte `at`: a stream that switches
/// shaders switches their counts, and each draw's module reads the words its own `RSRC2` loads.
fn user_data_layouts_at(sweep: &mut crate::registers::RegisterSweep<'_>, at: u32) -> [UserData; 2] {
    user_data_layouts_by(|register| sweep.latest(at, register))
}

/// The two stages' layouts from `last`, which answers a register's value where it is read.
fn user_data_layouts_by(mut last: impl FnMut(u32) -> Option<u32>) -> [UserData; 2] {
    // `USER_SGPR`, bits 5:1, and its sixth bit `USER_SGPR_MSB`, bit 27 of both stages' `RSRC2`
    // (`gfx103.json`): thirty-two registers set the MSB and leave the low five zero.
    let mut count = |register: u32| {
        last(register).map_or(0, |value| (value >> 1) & 0x1f | (value >> 27 & 1) << 5)
    };
    let counts = [count(RSRC2_REGISTERS[0]), count(RSRC2_REGISTERS[1])];
    // `DX10_CLAMP`, bit 21 of the stage's `RSRC1` (Mesa `S_00B848_DX10_CLAMP`): what an output
    // clamp does with a NaN. `None` when the stream set no `RSRC1`.
    let mut dx10_clamp = |register: u32| last(register).map(|value| value & DX10_CLAMP_BIT != 0);
    let clamps = [
        dx10_clamp(RSRC1_REGISTERS[0]),
        dx10_clamp(RSRC1_REGISTERS[1]),
    ];
    [
        UserData {
            first_register: 8,
            count: counts[0],
            block_offset: 0,
            dx10_clamp: clamps[0],
            pixel_inputs: None,
            compute: None,
            geometry: None,
            window_space: false,
            draw_buffers: false,
            buffer_formats: None,
            flat_twins: None,
            saturated: UNSATURATED,
        },
        UserData {
            first_register: 0,
            count: counts[1],
            block_offset: USER_DATA_STAGE_WORDS,
            dx10_clamp: clamps[1],
            // Set by `set_environment` for every backend.
            pixel_inputs: None,
            compute: None,
            geometry: None,
            window_space: false,
            draw_buffers: false,
            buffer_formats: None,
            flat_twins: None,
            saturated: UNSATURATED,
        },
    ]
}

/// The most words a window placed from a shader's base spans (D711): 256 KiB, four times the largest
/// vertex buffer a fully-owned guest allocates, and a single storage-buffer binding any device takes.
const PLACED_WINDOW_MAX_WORDS: u32 = 1 << 16;

impl Pipeline {
    /// What a submission's shaders are translated against, set before any of them is: the memory
    /// window and each stage's user-data layout.
    fn set_environment(
        &mut self,
        candidates: &[Candidate],
        writes: &[RegisterWrite],
        memory: &impl GuestMemory,
    ) {
        // Where the geometry shader says its memory is (D711): a live guest's vertex buffer is a
        // constant the shader forms, not something a register write carries.
        if self.places_window {
            self.place_window(candidates, memory);
        }
        // How many user-data words each stage's shader takes, from its `RSRC2`: what the hardware
        // loads, and so what the module reads at entry.
        self.user_data = if self.feeds_user_data {
            user_data_layouts(writes)
        } else {
            [UserData::default(); 2]
        };
        // Which vector registers the pixel shader starts with: built-in inputs on the host, so
        // every backend supplies them whether or not it feeds user data.
        self.user_data[1].pixel_inputs = crate::pixel_inputs::decode(writes);
    }

    /// Places the window at the constant base the vertex-stage candidate's shader forms (D711).
    ///
    /// The largest power-of-two span from that base, at most [`PLACED_WINDOW_MAX_WORDS`], that is
    /// wholly readable guest memory and does not cross four gigabytes. No base, or no readable
    /// span, leaves the window as it was.
    fn place_window(&mut self, candidates: &[Candidate], memory: &impl GuestMemory) {
        let Some(base) = candidates
            .iter()
            .filter(|candidate| candidate.stage == ShaderStage::Vertex)
            .find_map(|candidate| {
                let address = guest_address_of(candidate.address);
                // The base is a function of the program's bytes: one already found for these same
                // bytes at this address is that answer.
                if let Some((known, base)) = self.bases.get(&address)
                    && memory.read(address, known.len()) == Some(known.as_slice())
                {
                    return *base;
                }
                let bytes = read_window(memory, address)?;
                let decoded = decode_program(bytes, &self.encodings, &self.operands);
                let base = constant_address_base(&decoded, &self.encodings);
                // Only a program that ended is known by its bytes: one that ran off its window would
                // decode differently were more of it mapped.
                if decoded.terminated {
                    self.bases
                        .insert(address, (bytes[..decoded.consumed].to_vec(), base));
                }
                base
            })
        else {
            return;
        };
        let mut words = PLACED_WINDOW_MAX_WORDS;
        while words >= orbistoun_translate::predicated::MEMORY_WORDS {
            if let Some(window) = Window::spanning_address(base, words)
                && memory.read(base, words as usize * 4).is_some()
            {
                if window != self.window {
                    self.window = window;
                    self.cache.clear();
                }
                return;
            }
            words /= 2;
        }
    }
}

/// Whether a shader's results depend on guest memory it reaches through the window: any flat,
/// global, scratch, buffer or scalar buffer access, and any scalar load whose registers something
/// other than an image instruction reads.
///
/// A scalar load that only fills an image's descriptor registers is the exception: the translated
/// module names its image by the register group, and the texture itself is bound per draw from
/// where that load reads (the table provenance of `TextureSource`), so the value the module loads
/// is never used. Which registers are read is found syntactically over the whole program, each
/// scalar source counted with the register after it, which can only count more reads than there
/// are - so a load is excused only when nothing else could read what it wrote. Its local data share
/// and its images are not guest memory the window holds, and neither are the accesses `served`
/// names, which read the draw's bound buffers (D733).
fn reaches_guest_memory(
    decoded: &orbistoun_shader::Decode,
    encodings: &EncodingTable,
    served: &BTreeMap<u32, u32>,
) -> Option<String> {
    const ACCESSES: [&str; 6] = [
        "s_buffer_load",
        "global_",
        "flat_",
        "scratch_",
        "buffer_",
        "tbuffer_",
    ];
    let named: Vec<(&orbistoun_shader::Instruction, &str)> = decoded
        .instructions
        .iter()
        .filter_map(|instruction| {
            let family = encodings
                .encodings()
                .get(usize::from(instruction.encoding?))?;
            Some((
                instruction,
                encodings.mnemonic_for(&family.name, instruction.opcode)?,
            ))
        })
        .collect();
    // Every scalar register something other than an image instruction reads: its sources, and the
    // first operand of a compare, which writes nothing.
    let reads = |instruction: &orbistoun_shader::Instruction, name: &str| {
        let mut read = Vec::new();
        if name.starts_with("image_") {
            return read;
        }
        let first = usize::from(!is_scalar_compare(name));
        for operand in instruction.operands.iter().skip(first) {
            if let Operand::Scalar(register) = operand {
                read.extend([*register, register.saturating_add(1)]);
            }
        }
        read
    };
    let anywhere: std::collections::BTreeSet<u16> = named
        .iter()
        .flat_map(|&(instruction, name)| reads(instruction, name))
        .collect();
    // In a program with no loop, a load's registers are read only by what follows it until they
    // are written again; in one with a loop, anything may follow, so a read anywhere counts.
    let looped = named.iter().any(|&(instruction, name)| {
        (name == "s_branch" || name.starts_with("s_cbranch_"))
            && orbistoun_translate::blocks::branch_target(instruction)
                .is_ok_and(|target| target <= instruction.offset)
    });
    let read_after = |at: usize, first: u16, span: u16| {
        if looped {
            return (first..first.saturating_add(span)).any(|r| anywhere.contains(&r));
        }
        let mut live: std::collections::BTreeSet<u16> =
            (first..first.saturating_add(span)).collect();
        for &(instruction, name) in &named[at + 1..] {
            if reads(instruction, name).iter().any(|r| live.contains(r)) {
                return true;
            }
            if !is_scalar_compare(name)
                && let Some(Operand::Scalar(written)) = instruction.operands.first()
            {
                for register in *written..written.saturating_add(scalar_destination_span(name)) {
                    live.remove(&register);
                }
            }
            if live.is_empty() {
                return false;
            }
        }
        false
    };
    named
        .iter()
        .enumerate()
        .find(|&(at, &(instruction, name))| {
            if served.contains_key(&instruction.offset) {
                return false;
            }
            if ACCESSES.iter().any(|prefix| name.starts_with(prefix)) {
                return true;
            }
            if !name.starts_with("s_load") {
                return false;
            }
            let Some(Operand::Scalar(first)) = instruction.operands.first() else {
                return true;
            };
            read_after(at, *first, scalar_destination_span(name))
        })
        .map(|(_, (instruction, name))| {
            let second = instruction
                .second_word
                .map_or_else(String::new, |word| format!(" {word:#010x}"));
            format!(
                "{name} at {:#x} ({:#010x}{second}) {}",
                instruction.offset,
                instruction.word,
                instruction
                    .operands
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })
}

/// Whether a scalar instruction is a compare, which writes only the condition code: its first
/// operand is a source.
fn is_scalar_compare(name: &str) -> bool {
    name.starts_with("s_cmp") || name.starts_with("s_bitcmp")
}

/// The 64-bit address a shader forms from two constant scalar registers as the base of its memory
/// accesses, or `None` (D711).
///
/// A scalar register is constant only if the whole program writes it exactly once, with `s_mov_b32`
/// of a literal or inline integer, and no other destination (a scalar load's whole range included)
/// touches it. The base is the pair a `v_add_co_u32` takes as its scalar source for the low half
/// and a `v_add_co_ci_u32` takes, one register up, for the high half: the carry-chained add the
/// open-toolchain GL context's geometry shaders form addresses with (oops-sdk
/// `tools/shader/vs-param3.s`, `vs-param4.s`; `tools/shader-fixtures/primitive.s`). A computed
/// address yields nothing rather than a guess.
fn constant_address_base(
    decoded: &orbistoun_shader::Decode,
    encodings: &EncodingTable,
) -> Option<u64> {
    // Named the way the translator names them (`EncodingTable::mnemonic_for`), which covers the
    // long-form families; the carry-out add is a VOP3.
    let name = |instruction: &orbistoun_shader::Instruction| {
        let family = &encodings
            .encodings()
            .get(usize::from(instruction.encoding?))?
            .name;
        encodings.mnemonic_for(family, instruction.opcode)
    };
    // Every scalar register a destination touches, with how many registers each write spans.
    let mut writes: BTreeMap<u16, u32> = BTreeMap::new();
    let mut literal: BTreeMap<u16, u32> = BTreeMap::new();
    for instruction in &decoded.instructions {
        let Some(Operand::Scalar(first)) = instruction.operands.first() else {
            continue;
        };
        let Some(mnemonic) = name(instruction) else {
            continue;
        };
        // Stores and compares name a scalar first without writing it; only moves, ALU and loads
        // write their first operand.
        if !mnemonic.starts_with("s_") || mnemonic.starts_with("s_cmp") {
            continue;
        }
        let span = scalar_destination_span(mnemonic);
        for register in *first..first.saturating_add(span) {
            *writes.entry(register).or_default() += 1;
        }
        if mnemonic == "s_mov_b32" {
            let value = match instruction.operands.get(1) {
                Some(Operand::Literal(value)) => Some(*value),
                Some(Operand::Integer(value)) => u32::try_from(*value).ok(),
                _ => None,
            };
            if let Some(value) = value {
                literal.insert(*first, value);
            }
        }
    }
    let constant = |register: u16| {
        (writes.get(&register) == Some(&1))
            .then(|| literal.get(&register).copied())
            .flatten()
    };
    let scalar_source = |instruction: &orbistoun_shader::Instruction| {
        instruction
            .operands
            .iter()
            .skip(2)
            .find_map(|operand| match operand {
                Operand::Scalar(register) => Some(*register),
                _ => None,
            })
    };
    let low = decoded.instructions.iter().find_map(|instruction| {
        name(instruction)
            .filter(|mnemonic| mnemonic.starts_with("v_add_co_u32"))
            .and(scalar_source(instruction))
            .filter(|register| constant(*register).is_some())
    })?;
    let carried = decoded.instructions.iter().any(|instruction| {
        name(instruction).is_some_and(|mnemonic| mnemonic.starts_with("v_add_co_ci_u32"))
            && scalar_source(instruction) == Some(low + 1)
    });
    if !carried {
        return None;
    }
    Some((u64::from(constant(low + 1)?) << 32) | u64::from(constant(low)?))
}

/// How many consecutive scalar registers an instruction's destination spans: a 64-bit move or a
/// multi-dword load writes several, and every one of them stops being a constant.
fn scalar_destination_span(mnemonic: &str) -> u16 {
    if let Some(count) = mnemonic
        .rsplit_once("dwordx")
        .and_then(|(_, count)| count.parse::<u16>().ok())
    {
        return count;
    }
    if mnemonic.ends_with("_b64") || mnemonic.ends_with("_u64") || mnemonic.ends_with("_i64") {
        return 2;
    }
    1
}

/// Reads as much as can be read at an address, up to [`MAX_SHADER_BYTES`].
///
/// Narrows rather than demanding the whole window, so a shader near the end of a mapping still
/// reads.
fn read_window(memory: &impl GuestMemory, address: u64) -> Option<&[u8]> {
    let mut length = MAX_SHADER_BYTES;
    while length >= MIN_SHADER_BYTES {
        if let Some(bytes) = memory.read(address, length) {
            return Some(bytes);
        }
        length /= 2;
    }
    None
}

/// Reads the guest-memory window a pipeline is built against, as words.
///
/// Exactly the window's span, because that length is the mask the module applies and a region of
/// another length would fold an index to a different word. Empty when the region is not fully
/// mapped, so an unplaced window at address zero reads nothing.
fn read_guest_window(memory: &impl GuestMemory, window: Window) -> Vec<u32> {
    let byte_len = window.words() as usize * 4;
    memory
        .read(window.address(), byte_len)
        .map(|bytes| {
            bytes
                .chunks_exact(4)
                .map(|word| u32::from_le_bytes([word[0], word[1], word[2], word[3]]))
                .collect()
        })
        .unwrap_or_default()
}

/// Converts a GPU virtual address to the guest virtual address holding the same bytes.
///
/// The identity: the hardware shares one coherent memory pool between both processors, so the two
/// address spaces are expected to coincide. A function rather than nothing, so the assumption has
/// one place to change and a failed shader read names which assumption to suspect.
pub const fn guest_address_of(gpu_address: u64) -> u64 {
    gpu_address
}

/// An id for a colour target of a given extent at a given base address.
///
/// Keyed on the address as well as the extent, as D702 set out for when the backend kept anything
/// per target: it keeps each target's frame on the device until the flip (D714), so a
/// double-buffered title's two same-sized targets sharing an id handed one buffer's frame to the
/// other. A stream that sets no base keeps the extent alone. The top bit keeps target ids disjoint
/// from shader ids, which count up from one.
fn colour_target_id(extent: ColourTargetExtent, base: Option<u64>) -> ResourceId {
    use std::hash::{Hash, Hasher};
    const TARGET_NAMESPACE: u64 = 1 << 63;
    let key = match base {
        // Width and height are at most fourteen bits each, so the extent fits below the top bit.
        None => (u64::from(extent.width) << 16) | u64::from(extent.height),
        Some(base) => {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            (base, extent.width, extent.height).hash(&mut hasher);
            hasher.finish() & !TARGET_NAMESPACE
        }
    };
    ResourceId(TARGET_NAMESPACE | key)
}

/// The smallest window worth trying: one instruction that ends a program.
const MIN_SHADER_BYTES: usize = 4;

/// A cache key over a shader's bytes.
///
/// Keyed on content rather than address: a guest may move a shader, write a different one to
/// the same address, or have two addresses hold the same shader.
fn content_hash(bytes: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hasher);
    hasher.finish()
}

/// One shader recovered from a command stream and offered to the corpus.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedShader {
    /// The content id it was stored under - a truncated hash of its bytes.
    pub id: String,
    /// The pipeline stage the shader address was attributed to. Reported, never dispatched on,
    /// because the register mapping it rests on is a hypothesis.
    pub stage: String,
    /// Whether the corpus had not seen it before.
    pub fresh: bool,
    /// Its length in bytes, up to and including the terminator that ended it.
    pub length: usize,
}

/// A shader address that yielded no shader, and why. Reported because an address the register
/// mapping produced but memory could not honour is evidence about that mapping.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureMiss {
    /// The stage the missed address was attributed to.
    pub stage: String,
    /// The address that produced nothing.
    pub address: u64,
    /// Why nothing came of it.
    pub reason: String,
}

/// What one command stream contributed to the shader corpus.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CaptureReport {
    /// Every shader stored, in the order its address appeared.
    pub captured: Vec<CapturedShader>,
    /// Every address that named no readable, terminated shader.
    pub missed: Vec<CaptureMiss>,
}

/// Recovers every shader a command stream points at and offers it to the corpus.
///
/// Walks the packets, reads the shader-address registers, fetches each shader from guest memory and
/// stores it by content. Unlike [`Pipeline::submit`] it does not require a shader to translate: an
/// untranslatable shader is what the census ranks. It requires a terminator, because without an
/// end-of-program instruction the extent is unknown. The report lists what was stored and each
/// address that named nothing.
pub fn capture_shaders(
    stream: &[u8],
    memory: &impl GuestMemory,
    vocabulary: &Vocabulary,
    encodings: &EncodingTable,
    operands: &OperandTable,
    corpus: &mut ShaderCorpus,
) -> Result<CaptureReport, ShaderError> {
    let walked = walk(stream);
    let writes = register_writes(&walked, stream, vocabulary);
    let candidates = shader_candidates(&writes, vocabulary);

    let mut report = CaptureReport::default();
    for candidate in candidates {
        let Some(window) = read_window(memory, guest_address_of(candidate.address)) else {
            report.missed.push(CaptureMiss {
                stage: candidate.stage,
                address: candidate.address,
                reason: "no mapped memory at the address".to_owned(),
            });
            continue;
        };

        // The extent, not trustworthiness, gates a capture: a desynchronised decode still bounds
        // the shader if it reached the terminator.
        let decoded = decode_program(window, encodings, operands);
        if !decoded.terminated {
            report.missed.push(CaptureMiss {
                stage: candidate.stage,
                address: candidate.address,
                reason: format!(
                    "no end-of-program instruction within {} readable bytes",
                    window.len()
                ),
            });
            continue;
        }

        let (id, capture) = corpus.capture(&window[..decoded.consumed])?;
        report.captured.push(CapturedShader {
            id,
            stage: candidate.stage,
            fresh: matches!(capture, Capture::Added),
            length: decoded.consumed,
        });
    }

    Ok(report)
}

/// Why a pipeline could not be built.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PipelineError {
    /// A built-in table failed to load.
    #[error("a built-in table failed to load: {0}")]
    Table(String),
    /// A draw's primitive topology has no mesh-primitive shape to assemble.
    #[error(
        "the draw's primitive topology is {}, which has no mesh-primitive shape to assemble - a point, line or triangle is emitted, and this is refused rather than drawn as one of them",
        .0.label()
    )]
    NoMeshPrimitive(PrimitiveTopology),
}

/// The stage a register name refers to.
///
/// Unknown names produce `None` rather than a guess.
fn stage_of(name: &str) -> Option<ShaderStage> {
    match name {
        "vertex" => Some(ShaderStage::Vertex),
        "fragment" | "pixel" => Some(ShaderStage::Fragment),
        "compute" => Some(ShaderStage::Compute),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{Cached, ResourceId};

    /// The GL cube's vertex program as the hardware ran it: the first 64 words of oracle record B's
    /// payload (`tests/captures/agc-gl-cube-fw1240-b.payload.hex`).
    fn console_vertex_program() -> Vec<u8> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/captures/agc-gl-cube-fw1240-b.payload.hex");
        let text = std::fs::read_to_string(&path).expect("the committed capture");
        text.lines()
            .flat_map(|line| {
                line.split('#')
                    .next()
                    .unwrap_or_default()
                    .split_whitespace()
            })
            .take(64)
            .map(|word| u32::from_str_radix(word, 16).expect("a hex word"))
            .flat_map(u32::to_le_bytes)
            .collect()
    }

    /// A one-byte `8_UNORM` texture is read through the 8-bpp `64KB_R_X` layout and returned as its
    /// selects make it: radeonsi's `R8_UNORM` view, `(X, 0, 0, 1)`, is red over opaque black, and
    /// SuperTuxKart's glyph swizzle, `(1, 1, 1, X)`, white with the byte as alpha. A select of a
    /// channel one byte does not hold binds nothing.
    #[test]
    fn a_one_byte_texture_is_read_through_its_selects() {
        use super::{RenderCommand, read_texture};
        struct Memory(Vec<u8>);
        impl super::GuestMemory for Memory {
            fn read(&self, address: u64, length: usize) -> Option<&[u8]> {
                let start = usize::try_from(address.checked_sub(0x1000)?).ok()?;
                self.0.get(start..start.checked_add(length)?)
            }
        }
        let (table, texels, width, height) = (0x1000_u64, 0x1_0000_u64, 8u32, 4u32);
        let read = |selects: u32| {
            let mut bytes = vec![0u8; 0xF000 + 0x1_0000];
            let descriptor = [
                (texels >> 8) as u32,
                ((texels >> 40) as u32 & 0xff) | (1 << 20) | (((width - 1) & 3) << 30),
                ((width - 1) >> 2) | ((height - 1) << 14) | (1 << 31),
                (9 << 28) | (27 << 20) | selects,
                0,
                0,
                0,
                0,
            ];
            for (i, word) in descriptor.iter().enumerate() {
                bytes[i * 4..i * 4 + 4].copy_from_slice(&word.to_le_bytes());
            }
            for y in 0..height {
                for x in 0..width {
                    let at = (texels - table) as usize
                        + crate::tiling::tiled_byte_offset_64kb_rx_bpp1_surface(x, y, width);
                    bytes[at] = u8::try_from(y * 10 + x).expect("small");
                }
            }
            read_texture(table, 0, &Memory(bytes), &mut super::TexelCache::new())
        };
        let texels_of = |bound: Option<RenderCommand>| match bound {
            Some(RenderCommand::BindTexture { texels, .. }) => texels.to_vec(),
            other => panic!("a one-byte texture binds: {other:?}"),
        };
        // `SQ_SEL_XYZW01`: 0 and 1 the constants, 4 to 7 X to W; three bits a channel.
        let opaque = texels_of(read(4 | 1 << 9));
        assert_eq!(opaque[0], 0xff00_0000);
        assert_eq!(
            opaque[(3 * width + 5) as usize],
            0xff00_0023,
            "35, the byte at (5, 3)"
        );
        let glyph = texels_of(read(1 | 1 << 3 | 1 << 6 | 4 << 9));
        assert_eq!(
            glyph[(2 * width + 7) as usize],
            0x1bff_ffff,
            "27 as alpha over white"
        );
        assert!(read(5 | 1 << 9).is_none(), "a one-byte texel has no Y");
    }

    /// An all-zero image descriptor - a texture unit nothing bound - binds as a texture that reads
    /// zero in every channel: its `DST_SEL` fields are all `SQ_SEL_0`, so a sample returns the
    /// constant whatever the texel, and its sampler, also zero, decides nothing. RADV writes exactly
    /// this for a null descriptor (`radv_write_image_descriptor`, `memset(dst, 0, size)`). 2 Ship 2
    /// Harkinian's pixel shader samples a second unit the game left unbound, at table + 0x40.
    #[test]
    fn an_all_zero_descriptor_reads_zero() {
        use super::{RenderCommand, ShaderStage, TableBase, TextureSource, bind_textures};
        struct Memory(Vec<u8>);
        impl super::GuestMemory for Memory {
            fn read(&self, address: u64, length: usize) -> Option<&[u8]> {
                let start = usize::try_from(address.checked_sub(0x1000)?).ok()?;
                self.0.get(start..start.checked_add(length)?)
            }
        }
        let memory = Memory(vec![0; 0x100]);
        let shader = ResourceId(7);
        let mut words = [0u32; super::USER_DATA_WORDS];
        words[0] = 0x1000;
        let sources = std::collections::BTreeMap::from([(
            shader,
            vec![TextureSource {
                slot: 1,
                table_offset: Some(0x40),
                table: TableBase::default(),
                sampler_offset: Some(0x60),
                user_data: None,
                sampler_user_data: None,
                saturated: [false; 2],
            }],
        )]);
        let mut commands = vec![
            RenderCommand::SetUserData {
                stage: ShaderStage::Fragment,
                words,
            },
            RenderCommand::BindShader {
                stage: ShaderStage::Fragment,
                shader,
            },
            RenderCommand::Draw {
                vertices: 3,
                instances: 1,
                first_vertex: 0,
            },
        ];
        let mut unbound = 0;
        bind_textures(
            &mut commands,
            (&sources, &mut super::TexelCache::new()),
            &memory,
            &mut unbound,
        );
        assert_eq!(unbound, 0);
        let bound = commands.iter().find_map(|command| match command {
            RenderCommand::BindTexture {
                slot,
                texels,
                width,
                height,
                ..
            } => Some((*slot, texels.to_vec(), *width, *height)),
            _ => None,
        });
        assert_eq!(
            bound,
            Some((1, vec![0], 1, 1)),
            "one texel, every channel zero"
        );
    }

    /// A sampler clamping to half a border binds its texture only for a pixel shader translated to
    /// saturate the coordinates it names (D743); for any other the texture is left unbound, so the
    /// draw is refused rather than sampled past the edge as a plain border clamp would.
    #[test]
    fn a_half_border_sampler_binds_only_for_a_saturating_module() {
        use super::{RenderCommand, ShaderStage, TableBase, TextureSource, bind_textures};
        use crate::registers::TextureWrap;
        struct Memory(Vec<u8>);
        impl super::GuestMemory for Memory {
            fn read(&self, address: u64, length: usize) -> Option<&[u8]> {
                let start = usize::try_from(address.checked_sub(0x1000)?).ok()?;
                self.0.get(start..start.checked_add(length)?)
            }
        }
        let (table, texels, width, height) = (0x1000_u64, 0x1_0000_u64, 8u32, 4u32);
        let mut bytes = vec![0u8; 0xF000 + 0x1_0000];
        let image = [
            (texels >> 8) as u32,
            (1 << 20) | (((width - 1) & 3) << 30),
            ((width - 1) >> 2) | ((height - 1) << 14) | (1 << 31),
            (9 << 28) | (27 << 20) | 4 | 1 << 9,
            0,
            0,
            0,
            0,
        ];
        // `GL_CLAMP` across and down, both filters linear, transparent black.
        let sampler = [4 | 4 << 3, 0, 1 << 20 | 1 << 22, 0];
        for (i, word) in image.iter().chain(&sampler).enumerate() {
            bytes[i * 4..i * 4 + 4].copy_from_slice(&word.to_le_bytes());
        }
        let memory = Memory(bytes);
        let shader = ResourceId(7);
        let mut words = [0u32; super::USER_DATA_WORDS];
        words[0] = table as u32;
        let bind = |saturated: [bool; 2]| {
            let sources = std::collections::BTreeMap::from([(
                shader,
                vec![TextureSource {
                    slot: 0,
                    table_offset: Some(0),
                    table: TableBase::default(),
                    sampler_offset: Some(0x20),
                    user_data: None,
                    sampler_user_data: None,
                    saturated,
                }],
            )]);
            let mut commands = vec![
                RenderCommand::SetUserData {
                    stage: ShaderStage::Fragment,
                    words,
                },
                RenderCommand::BindShader {
                    stage: ShaderStage::Fragment,
                    shader,
                },
                RenderCommand::Draw {
                    vertices: 3,
                    instances: 1,
                    first_vertex: 0,
                },
            ];
            let mut unbound = 0;
            bind_textures(
                &mut commands,
                (&sources, &mut super::TexelCache::new()),
                &memory,
                &mut unbound,
            );
            let sampling = commands.iter().find_map(|command| match command {
                RenderCommand::BindTexture { sampling, .. } => Some(*sampling),
                _ => None,
            });
            (unbound, sampling.map(|s| s.wrap))
        };
        assert_eq!(bind([false; 2]), (1, None), "not saturated: refused");
        assert_eq!(
            bind([true; 2]),
            (0, Some([TextureWrap::ClampHalfBorder; 2])),
            "saturated: bound as the sampler says"
        );
    }

    /// A `BC3_UNORM` texture in `64KB_S` is bound as its blocks, detiled row-major, for the host to
    /// sample natively: an 8 x 8 image is two blocks across and two down, each read from where
    /// addrlib's equation puts it.
    #[test]
    fn a_bc3_texture_binds_its_blocks_in_row_order() {
        use super::{RenderCommand, texture_from_words};
        struct Memory(Vec<u8>);
        impl super::GuestMemory for Memory {
            fn read(&self, address: u64, length: usize) -> Option<&[u8]> {
                let start = usize::try_from(address.checked_sub(0x1_0000)?).ok()?;
                self.0.get(start..start.checked_add(length)?)
            }
        }
        let mut bytes = vec![0u8; crate::tiling::surface_bytes_64kb_s_bpp16(2, 2)];
        for (y, x) in [(0u32, 0u32), (0, 1), (1, 0), (1, 1)] {
            let at = crate::tiling::tiled_byte_offset_64kb_s_bpp16(x, y, 2);
            for word in 0..4u32 {
                let value = (y << 12) | (x << 8) | word;
                bytes[at + 4 * word as usize..][..4].copy_from_slice(&value.to_le_bytes());
            }
        }
        let (width, height) = (8u32, 8u32);
        let words = [
            0x100,
            (173 << 20) | (((width - 1) & 3) << 30),
            ((width - 1) >> 2) | ((height - 1) << 14) | (1 << 31),
            // 2D, ADDR_SW_64KB_S, the channels as they are.
            0x9090_0fac,
            0,
            0,
            0,
            0,
        ];
        let bound = texture_from_words(words, 0, &Memory(bytes), &mut super::TexelCache::new());
        let Some(RenderCommand::BindTexture {
            texels,
            width: 8,
            height: 8,
            encoding: crate::backend::TextureEncoding::Bc3,
            ..
        }) = bound
        else {
            panic!("bound as BC3 blocks: {bound:?}");
        };
        let tags: Vec<u32> = texels.chunks(4).map(|block| block[0]).collect();
        assert_eq!(tags, [0x000, 0x100, 0x1000, 0x1100], "row-major blocks");
        assert_eq!(texels[1], 1, "a block's words in order");
    }

    /// A pixel shader that samples through a descriptor and sampler still in its user data binds the
    /// image those words name, sampled as the sampler words say: no table is read. Traced as a table
    /// at the first two words, the same draw would read the descriptor's own words as an address.
    #[test]
    fn a_descriptor_in_the_user_data_binds_its_image() {
        use super::{RenderCommand, ShaderStage, TableBase, TextureSource, bind_textures};
        use crate::registers::TextureWrap;
        struct Memory(Vec<u8>);
        impl super::GuestMemory for Memory {
            fn read(&self, address: u64, length: usize) -> Option<&[u8]> {
                let start = usize::try_from(address.checked_sub(0x1_0000)?).ok()?;
                self.0.get(start..start.checked_add(length)?)
            }
        }
        let (texels, width, height) = (0x1_0000_u64, 8u32, 4u32);
        let memory = Memory(vec![0x7f; 0x1_0000]);
        let image = [
            (texels >> 8) as u32,
            (1 << 20) | (((width - 1) & 3) << 30),
            ((width - 1) >> 2) | ((height - 1) << 14) | (1 << 31),
            (9 << 28) | (27 << 20) | 4 | 1 << 9,
        ];
        // `GL_CLAMP` across and down, both filters linear.
        let sampler = [4 | 4 << 3, 0, 1 << 20 | 1 << 22];
        let mut words = [0u32; super::USER_DATA_WORDS];
        words[..4].copy_from_slice(&image);
        words[8..11].copy_from_slice(&sampler);
        let shader = ResourceId(7);
        let bind = |user_data: Option<u32>| {
            let sources = std::collections::BTreeMap::from([(
                shader,
                vec![TextureSource {
                    slot: 0,
                    table_offset: None,
                    table: TableBase::default(),
                    sampler_offset: None,
                    user_data,
                    sampler_user_data: user_data.map(|_| 8),
                    saturated: [true; 2],
                }],
            )]);
            let mut commands = vec![
                RenderCommand::SetUserData {
                    stage: ShaderStage::Fragment,
                    words,
                },
                RenderCommand::BindShader {
                    stage: ShaderStage::Fragment,
                    shader,
                },
                RenderCommand::Draw {
                    vertices: 3,
                    instances: 1,
                    first_vertex: 0,
                },
            ];
            let mut unbound = 0;
            bind_textures(
                &mut commands,
                (&sources, &mut super::TexelCache::new()),
                &memory,
                &mut unbound,
            );
            let bound = commands.iter().find_map(|command| match command {
                RenderCommand::BindTexture {
                    width,
                    height,
                    sampling,
                    ..
                } => Some((*width, *height, sampling.wrap)),
                _ => None,
            });
            (unbound, bound)
        };
        assert_eq!(
            bind(Some(0)),
            (0, Some((width, height, [TextureWrap::ClampHalfBorder; 2])))
        );
        assert_eq!(bind(None), (1, None), "the words read as a table address");
    }

    /// A draw whose pixel shader no earlier draw prepared still has its half-border samplers read
    /// (D743): the shader is prepared for no particular draw first, which names where its sampler
    /// comes from, and the draw's sampler then says which coordinates to saturate.
    #[test]
    fn a_pixel_shader_first_met_at_a_draw_saturates_for_its_sampler() {
        use super::{ByGeometry, Candidate, Pipeline, ShaderStage, Submission};
        use orbistoun_shader::EncodingTable;
        use orbistoun_translate::{Fidelity, Strategy, Width};
        const PROGRAM: u64 = 0x1_0000;
        const TABLE: u64 = 0x2_0000;
        struct Memory(Vec<u8>);
        impl super::GuestMemory for Memory {
            fn read(&self, address: u64, length: usize) -> Option<&[u8]> {
                let start = usize::try_from(address.checked_sub(PROGRAM)?).ok()?;
                self.0.get(start..start.checked_add(length)?)
            }
        }
        let encodings = EncodingTable::builtin().expect("encodings");
        let s_load = |name: &str, destination: u32, offset: u32| {
            let (family, opcode) = encodings.find_by_name(name).expect("the target has it");
            let encoding = encodings
                .encodings()
                .iter()
                .find(|e| e.name == family)
                .expect("its family");
            [
                encoding.value | (opcode << encoding.opcode.shift) | (destination << 6),
                offset,
            ]
        };
        // The coordinate interpolated into v0 and v1; the image from the table at s[0:1] into
        // s[4:11] and its sampler from 0x20 into s[12:15]; `image_sample_lz v[4:7], v[0:1]`; end.
        let mut program: Vec<u32> = vec![0xc800_0000, 0xc804_0100];
        program.extend(s_load("s_load_dwordx8", 4, 0));
        program.extend(s_load("s_load_dwordx4", 12, 0x20));
        program.extend([
            0xf000_0000 | 39 << 18 | 0xf << 8 | 1 << 3,
            4 << 8 | 1 << 16 | 3 << 21,
            0xbf81_0000,
        ]);
        let mut bytes = vec![0u8; usize::try_from(TABLE - PROGRAM).expect("small") + 0x40];
        for (i, word) in program.iter().enumerate() {
            bytes[i * 4..i * 4 + 4].copy_from_slice(&word.to_le_bytes());
        }
        // `GL_CLAMP` across only, both filters linear.
        let sampler_at = usize::try_from(TABLE - PROGRAM).expect("small") + 0x20;
        bytes[sampler_at..sampler_at + 4].copy_from_slice(&4u32.to_le_bytes());
        bytes[sampler_at + 8..sampler_at + 12]
            .copy_from_slice(&(1u32 << 20 | 1 << 22).to_le_bytes());
        let memory = Memory(bytes);

        let mut pipeline = Pipeline::new(Strategy::Predicated {
            fidelity: Fidelity::Lane,
            width: Width::default(),
        })
        .expect("a pipeline");
        let mut words = [0u32; super::USER_DATA_WORDS];
        words[0] = TABLE as u32;
        let fragment = Candidate {
            address: PROGRAM,
            stage: ShaderStage::Fragment,
        };
        let mut by_geometry = ByGeometry::default();
        let saturated = pipeline.draw_saturated(
            &[fragment],
            (&words, &memory),
            &mut by_geometry,
            &mut Submission::default(),
        );
        assert_eq!(saturated, [[true, false], [false, false]]);
    }

    /// A dispatch whose buffers no window spans binds each on its own (D746): STKT00001's copy,
    /// from one buffer to another 128 MiB away, prepares with both ranges bound and no window,
    /// and two ranges that share a byte are refused.
    #[test]
    fn a_dispatch_binds_buffers_no_window_spans() {
        use super::Pipeline;
        use crate::dispatch::DispatchState;
        use orbistoun_translate::wavefront::ComputeInputs;
        use orbistoun_translate::{Fidelity, Strategy, Width};
        const PROGRAM: u64 = 0x1_0000;
        const SOURCE: u64 = 0x4_0a60_0000;
        const DESTINATION: u64 = 0x4_1260_0000;
        struct Memory(Vec<(u64, Vec<u8>)>);
        impl super::GuestMemory for Memory {
            fn read(&self, address: u64, length: usize) -> Option<&[u8]> {
                self.0.iter().find_map(|(base, bytes)| {
                    let start = usize::try_from(address.checked_sub(*base)?).ok()?;
                    bytes.get(start..start.checked_add(length)?)
                })
            }
        }
        // v_lshlrev_b32 v0, 2, v0; buffer_load_dword v1, v0, s[8:11], 0 offen; s_waitcnt;
        // buffer_store_dword v1, v0, s[12:15], 0 offen; s_endpgm.
        let program: Vec<u8> = [
            0x3400_0082_u32,
            0xe030_1000,
            0x8002_0100,
            0xbf8c_3f70,
            0xe070_1000,
            0x8003_0100,
            0xbf81_0000,
            0xbf9f_0000,
        ]
        .iter()
        .flat_map(|w| w.to_le_bytes())
        .collect();
        let mut program_bytes = program;
        program_bytes.resize(super::MAX_SHADER_BYTES, 0);
        let state = |destination: u64| {
            let mut user_data = vec![Some(0); 16];
            for (at, base) in [(8, SOURCE), (12, destination)] {
                user_data[at] = Some(base as u32);
                user_data[at + 1] = Some((base >> 32) as u32 & 0xffff);
                user_data[at + 2] = Some(64 * 4);
                user_data[at + 3] = Some(0x3101_6fac);
            }
            DispatchState {
                program: PROGRAM,
                user_data,
                inputs: ComputeInputs {
                    thread_id_components: 1,
                    threads: [64, 1, 1],
                    ..ComputeInputs::default()
                },
                width: Width::Wave64,
                dx10_clamp: None,
                groups: [1, 1, 1],
            }
        };
        let memory = Memory(vec![
            (PROGRAM, program_bytes),
            (SOURCE, vec![0x11; 0x400]),
            (DESTINATION, vec![0x22; 0x400]),
        ]);
        let mut pipeline = Pipeline::new(Strategy::Predicated {
            fidelity: Fidelity::Wavefront,
            width: Width::Wave64,
        })
        .expect("a pipeline");
        let prepared = pipeline
            .prepare_dispatch(&state(DESTINATION), &memory)
            .expect("the copy prepares");
        assert_eq!(prepared.buffers, [(SOURCE, 256), (DESTINATION, 256)]);
        assert!(prepared.window.is_none(), "nothing is left to a window");
        let overlapping = pipeline
            .prepare_dispatch(&state(SOURCE + 16), &memory)
            .expect_err("two copies of one byte");
        assert!(overlapping.contains("overlap"), "{overlapping}");
    }

    /// A primitive shader's format words are traced once and served from then on, and a program
    /// rewritten at the same address is traced again: the format follows the descriptor the new
    /// program reads, not the one the old did.
    #[test]
    fn a_rewritten_program_is_traced_for_its_formats_again() {
        use super::{Pipeline, USER_DATA_WORDS};
        use orbistoun_translate::{Fidelity, Strategy, Width};
        struct Nothing;
        impl super::GuestMemory for Nothing {
            fn read(&self, _address: u64, _length: usize) -> Option<&[u8]> {
                None
            }
        }
        let program = |descriptor: u32| -> Vec<u8> {
            // buffer_load_format_xyzw v0, v5, s[descriptor..+3], 0 idxen; s_endpgm.
            [
                0xe00c_2000_u32,
                0x8000_0005 | (descriptor / 4) << 16,
                0xbf81_0000,
            ]
            .iter()
            .flat_map(|w| w.to_le_bytes())
            .collect()
        };
        let mut pipeline = Pipeline::new(Strategy::Predicated {
            fidelity: Fidelity::Wavefront,
            width: Width::Wave64,
        })
        .expect("a pipeline");
        pipeline.user_data[0].first_register = 8;
        pipeline.user_data[0].count = 20;
        let mut words = [0u32; USER_DATA_WORDS];
        words[15] = 0x1111_1111;
        words[19] = 0x2222_2222;
        let address = 0x40_0000;
        pipeline.decoded.insert(address, program(20));
        let first = pipeline.draw_buffer_formats(address, &words, &Nothing);
        assert_eq!(first.map(|f| f.0[0]), Some(Some(0x1111_1111)));
        assert_eq!(
            pipeline
                .draw_buffer_formats(address, &words, &Nothing)
                .map(|f| f.0[0]),
            Some(Some(0x1111_1111)),
            "served again"
        );
        pipeline.decoded.insert(address, program(24));
        assert_eq!(
            pipeline
                .draw_buffer_formats(address, &words, &Nothing)
                .map(|f| f.0[0]),
            Some(Some(0x2222_2222)),
            "the rewritten program's descriptor"
        );
    }

    /// Each draw's user-data count is the one its `RSRC2` loads at that draw, not the stream's last:
    /// STKT00001 sets a primitive shader taking twenty words, draws, then one taking sixteen, and
    /// the first draw's vertex buffer descriptors in words 16-19 were left untraced (D733).
    #[test]
    fn a_draws_user_data_count_is_the_one_in_force_at_it() {
        use super::{RSRC2_REGISTERS, user_data_layouts_at};
        use crate::registers::{RegisterSweep, RegisterWrite};
        let rsrc2 = |words: u32| words << 1;
        let writes = [
            RegisterWrite {
                packet_offset: 0x10,
                register: RSRC2_REGISTERS[0],
                value: rsrc2(20),
            },
            RegisterWrite {
                packet_offset: 0x100,
                register: RSRC2_REGISTERS[0],
                value: rsrc2(16),
            },
        ];
        let mut sweep = RegisterSweep::new(&writes);
        assert_eq!(
            user_data_layouts_at(&mut sweep, 0x50)[0].count,
            20,
            "the first draw's"
        );
        assert_eq!(
            user_data_layouts_at(&mut sweep, 0x200)[0].count,
            16,
            "the second's"
        );
    }

    /// A one-byte linear texture with no pitch of its own is read at 256 texels a row, as
    /// addrlib's linear layout aligns one; under `(X, 0, 0, 1)` each byte is red.
    #[test]
    fn a_one_byte_linear_texture_is_read_at_its_layouts_pitch() {
        use super::{RenderCommand, read_texture};
        struct Memory(Vec<u8>);
        impl super::GuestMemory for Memory {
            fn read(&self, address: u64, length: usize) -> Option<&[u8]> {
                let start = usize::try_from(address.checked_sub(0x1000)?).ok()?;
                self.0.get(start..start.checked_add(length)?)
            }
        }
        let (table, texels, width, height) = (0x1000_u64, 0x2000_u64, 8u32, 3u32);
        let mut bytes = vec![0u8; 0x1000 + 256 * 3];
        let descriptor = [
            (texels >> 8) as u32,
            ((texels >> 40) as u32 & 0xff) | (1 << 20) | (((width - 1) & 3) << 30),
            ((width - 1) >> 2) | ((height - 1) << 14) | (1 << 31),
            (9 << 28) | 4 | 1 << 9,
            0,
            0,
            0,
            0,
        ];
        for (i, word) in descriptor.iter().enumerate() {
            bytes[i * 4..i * 4 + 4].copy_from_slice(&word.to_le_bytes());
        }
        for y in 0..height as usize {
            for x in 0..width as usize {
                bytes[0x1000 + y * 256 + x] = u8::try_from(y * 10 + x).expect("small");
            }
        }
        let Some(RenderCommand::BindTexture { texels, .. }) =
            read_texture(table, 0, &Memory(bytes), &mut super::TexelCache::new())
        else {
            panic!("a linear one-byte texture binds");
        };
        assert_eq!(
            texels[(2 * width + 3) as usize],
            0xff00_0017,
            "23, at row 2's pitch"
        );
    }

    /// A linear texture is read at its descriptor's pitch, not its width, and a descriptor that is
    /// not 2D linear RGBA8 binds nothing.
    #[test]
    fn a_tiled_texture_is_detiled_into_rows() {
        use super::{RenderCommand, read_texture};
        struct Memory(Vec<u8>);
        impl super::GuestMemory for Memory {
            fn read(&self, address: u64, length: usize) -> Option<&[u8]> {
                let start = usize::try_from(address.checked_sub(0x1000)?).ok()?;
                self.0.get(start..start.checked_add(length)?)
            }
        }
        // One 64 KiB block after the descriptor, on the block alignment the layout has: the
        // descriptor's low base bits are the pipe-bank XOR, not address.
        let (table, texels, width, height) = (0x1000_u64, 0x1_0000_u64, 8u32, 4u32);
        let mut bytes = vec![0u8; 0xF000 + 0x1_0000];
        let descriptor = [
            (texels >> 8) as u32,
            ((texels >> 40) as u32 & 0xff) | (56 << 20) | (((width - 1) & 3) << 30),
            ((width - 1) >> 2) | ((height - 1) << 14) | (1 << 31),
            (9 << 28) | (27 << 20) | 0xfac,
            0,
            0,
            0,
            0,
        ];
        for (i, word) in descriptor.iter().enumerate() {
            bytes[i * 4..i * 4 + 4].copy_from_slice(&word.to_le_bytes());
        }
        // Texel (x, y) holds `y * 100 + x`, placed where the measured 64KB_R_X addressing puts it.
        for y in 0..height {
            for x in 0..width {
                let at = (texels - table) as usize
                    + crate::tiling::tiled_byte_offset_64kb_rx_bpp4_surface(x, y, width);
                bytes[at..at + 4].copy_from_slice(&(y * 100 + x).to_le_bytes());
            }
        }
        let mut cache = super::TexelCache::new();
        let Some(RenderCommand::BindTexture {
            texels,
            width: w,
            height: h,
            ..
        }) = read_texture(table, 0, &Memory(bytes), &mut cache)
        else {
            panic!("a tiled RGBA8 texture binds");
        };
        assert_eq!((w, h), (width, height));
        let expected: Vec<u32> = (0..height)
            .flat_map(|y| (0..width).map(move |x| y * 100 + x))
            .collect();
        assert_eq!(
            texels.as_ref(),
            expected.as_slice(),
            "row-major after detiling"
        );
    }

    #[test]
    fn a_linear_texture_is_read_at_its_descriptors_pitch() {
        use super::{RenderCommand, read_texture};
        struct Memory(Vec<u8>);
        impl super::GuestMemory for Memory {
            fn read(&self, address: u64, length: usize) -> Option<&[u8]> {
                let start = usize::try_from(address.checked_sub(0x1000)?).ok()?;
                self.0.get(start..start.checked_add(length)?)
            }
        }
        let (table, texels, width, height, pitch) = (0x1000_u64, 0x1100_u64, 16u32, 2u32, 64u32);
        let mut bytes = vec![0u8; 0x1000];
        let descriptor = [
            (texels >> 8) as u32,
            ((texels >> 40) as u32 & 0xff) | (56 << 20) | (((width - 1) & 3) << 30),
            ((width - 1) >> 2) | ((height - 1) << 14) | (1 << 31),
            (9 << 28) | 0xfac,
            pitch - 1,
            0,
            0,
            0,
        ];
        for (i, word) in descriptor.iter().enumerate() {
            bytes[i * 4..i * 4 + 4].copy_from_slice(&word.to_le_bytes());
        }
        // Texel (x, y) at the pitch holds `y * 100 + x`; the texel a width stride would reach for
        // row 1 holds a marker instead.
        let at = |index: u64| (texels - table + index * 4) as usize;
        for y in 0..u64::from(height) {
            for x in 0..u64::from(width) {
                let value = (y * 100 + x) as u32;
                bytes[at(y * u64::from(pitch) + x)..at(y * u64::from(pitch) + x) + 4]
                    .copy_from_slice(&value.to_le_bytes());
            }
        }
        bytes[at(16)..at(16) + 4].copy_from_slice(&0xdead_beef_u32.to_le_bytes());
        let memory = Memory(bytes.clone());
        let mut cache = super::TexelCache::new();

        let Some(RenderCommand::BindTexture {
            texels,
            width: w,
            height: h,
            hash,
            ..
        }) = read_texture(table, 0, &memory, &mut cache)
        else {
            panic!("a 2D linear RGBA8 texture binds");
        };
        assert_eq!((w, h), (width, height));
        assert_eq!(texels[0], 0);
        assert_eq!(texels[16], 100, "row 1 starts at the pitch, not the width");
        assert_eq!(texels[31], 115);
        assert_eq!(
            hash,
            crate::content_hash(&texels),
            "hashed in place as gathered"
        );

        // A cached texture is re-read when its bytes change, and padding past the width changes
        // nothing.
        let mut edited = bytes.clone();
        edited[at(u64::from(pitch) + 1)..at(u64::from(pitch) + 1) + 4]
            .copy_from_slice(&7u32.to_le_bytes());
        let Some(RenderCommand::BindTexture { texels, .. }) =
            read_texture(table, 0, &Memory(edited), &mut cache)
        else {
            panic!("still binds");
        };
        assert_eq!(texels[17], 7);
        let mut padded = bytes.clone();
        padded[at(20)..at(20) + 4].copy_from_slice(&9u32.to_le_bytes());
        let Some(RenderCommand::BindTexture { texels: again, .. }) =
            read_texture(table, 0, &Memory(padded), &mut cache)
        else {
            panic!("still binds");
        };
        assert_eq!(again[17], 101, "the original bytes, unaffected by padding");

        // A 3D texture (type 0xa) names a slice count in WORD4, not a pitch: nothing binds.
        let mut three_d = bytes;
        three_d[12..16].copy_from_slice(&((0xa_u32 << 28) | 0xfac).to_le_bytes());
        assert!(read_texture(table, 0, &Memory(three_d), &mut cache).is_none());
    }

    /// A linear dispatch image spans its rows at their pitch, the last ending at its width; its
    /// texels come out row-major, and going back in they change only their own bytes - a word
    /// each at four bytes a texel, a byte each at one.
    #[test]
    fn a_linear_dispatch_image_reads_and_writes_its_rows_at_their_pitch() {
        use super::DispatchSurface;
        let words = DispatchSurface::Linear {
            base: 0x1000,
            width: 3,
            height: 2,
            pitch: 64,
            bytes_per_texel: 4,
        };
        assert_eq!(words.bytes(), (64 + 3) * 4);
        let mut spanned: Vec<u8> = (0..67_u32).flat_map(u32::to_le_bytes).collect();
        assert_eq!(words.detile(&spanned), vec![0, 1, 2, 64, 65, 66]);
        words
            .tile(&[10, 11, 12, 13, 14, 15], &mut spanned)
            .expect("tiles");
        assert_eq!(
            &spanned[..16],
            &[10, 0, 0, 0, 11, 0, 0, 0, 12, 0, 0, 0, 3, 0, 0, 0]
        );
        assert_eq!(&spanned[64 * 4..64 * 4 + 4], &[13, 0, 0, 0]);

        let bytes = DispatchSurface::Linear {
            base: 0x1000,
            width: 3,
            height: 2,
            pitch: 256,
            bytes_per_texel: 1,
        };
        assert_eq!(bytes.bytes(), 256 + 3);
        let mut spanned: Vec<u8> = (0..259_u32).map(|i| i as u8).collect();
        assert_eq!(bytes.detile(&spanned), vec![0, 1, 2, 0, 1, 2]);
        bytes
            .tile(&[7, 8, 9, 10, 11, 12], &mut spanned)
            .expect("tiles");
        assert_eq!(&spanned[..4], &[7, 8, 9, 3]);
        assert_eq!(&spanned[255..], &[255, 10, 11, 12]);
    }

    /// A one-byte compressed chain's level is a dispatch surface in the one-byte layout - level 0 of
    /// SuperTuxKart's ten-level 512 x 512 glyph page past the tail block and level 1 - with addrlib's
    /// 12 KiB of keys for the chain; it detiles to its bytes and tiles back only its own.
    #[test]
    fn a_one_byte_chain_level_is_a_dispatch_surface() {
        use super::{DispatchSurface, TexelFormat, surface_key_bytes, surface_layout};
        use crate::registers::{ImageDescriptor, SwizzleMode, chain_level};
        let descriptor = ImageDescriptor {
            base: 0x10_0000,
            width: 512,
            height: 512,
            format: super::FORMAT_8_UNORM,
            tiling: SwizzleMode::Tiled64KbRX,
            pipe_bank_xor: 0,
            levels: 10,
            base_level: 0,
            last_level: 0,
            compression: Some(crate::dcc::Dcc {
                base: 0x20_0000,
                pipe_aligned: true,
            }),
        };
        assert_eq!(surface_key_bytes(&descriptor, TexelFormat::R8), Some(12288));
        let layout = surface_layout(&descriptor, TexelFormat::R8).expect("modelled");
        assert_eq!(layout.chain_bytes(512, 512, 10), 0x6_0000);
        let level = chain_level((descriptor.base, 0), (512, 512), (10, 0), layout).expect("level");
        assert_eq!(level.base, 0x10_0000 + 0x2_0000);
        let surface = DispatchSurface::Tiled(level);
        assert_eq!(surface.bytes(), 0x4_0000);
        let mut spanned = vec![0xEE; surface.bytes()];
        let texels: Vec<u32> = (0..512 * 512).map(|i| i % 199).collect();
        surface.tile(&texels, &mut spanned).expect("tiles");
        assert_eq!(surface.detile(&spanned), texels);
        assert_eq!(
            spanned[crate::tiling::tiled_byte_offset_64kb_rx_bpp1_surface(300, 7, 512)],
            ((7 * 512 + 300) % 199) as u8
        );
    }

    /// A stage's user-register count reads `USER_SGPR_MSB`, bit 27 of its `RSRC2`, as the count's
    /// sixth bit: thirty-two registers set it with the low five bits clear, and read as zero
    /// without it. Both stages' `RSRC2` carry it there (`gfx103.json`).
    #[test]
    fn a_stage_s_user_register_count_reads_its_sixth_bit() {
        let write = |register, value| crate::registers::RegisterWrite {
            packet_offset: 0,
            register,
            value,
        };
        let layouts = super::user_data_layouts(&[
            write(super::RSRC2_REGISTERS[0], 1 << 27),
            write(super::RSRC2_REGISTERS[1], 1 << 27 | 3 << 1),
        ]);
        assert_eq!(layouts[0].count, 32);
        assert_eq!(layouts[1].count, 35, "the low five bits beside it");
    }

    /// The backend's user-data block and the translator's share one layout: size and each stage's
    /// start are pinned equal across two crates that cannot import each other.
    #[test]
    fn the_backends_user_data_block_is_the_translators() {
        use orbistoun_translate::wavefront::{USER_DATA_BLOCK_WORDS, USER_DATA_STAGE_WORDS};
        assert_eq!(crate::USER_DATA_BLOCK_WORDS, USER_DATA_BLOCK_WORDS as usize);
        assert_eq!(
            crate::USER_DATA_BLOCK_OFFSETS,
            [0, USER_DATA_STAGE_WORDS as usize]
        );
        let layouts = super::user_data_layouts(&[]);
        assert_eq!(
            [layouts[0].block_offset, layouts[1].block_offset].map(|o| o as usize),
            crate::USER_DATA_BLOCK_OFFSETS
        );
    }

    /// Each draw carries its own user data: the captured cube stream writes a different vertex
    /// offset into `SPI_SHADER_USER_DATA_GS_0` before each of its twelve draws, and each must reach
    /// its draw.
    #[test]
    fn each_of_the_cubes_twelve_draws_carries_its_own_vertex_offset() {
        use super::{Pipeline, RenderCommand, ShaderStage};
        use orbistoun_translate::{Fidelity, Strategy, Width};

        struct Nothing;
        impl super::GuestMemory for Nothing {
            fn read(&self, _address: u64, _length: usize) -> Option<&[u8]> {
                None
            }
        }
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/captures/agc-gl-cube-fw1240-b.hex");
        let stream: Vec<u8> = std::fs::read_to_string(&path)
            .expect("the committed capture")
            .lines()
            .flat_map(|line| {
                line.split('#')
                    .next()
                    .unwrap_or_default()
                    .split_whitespace()
            })
            .map(|word| u32::from_str_radix(word, 16).expect("a hex word"))
            .flat_map(u32::to_le_bytes)
            .collect();
        let mut pipeline = Pipeline::new(Strategy::Predicated {
            fidelity: Fidelity::Lane,
            width: Width::default(),
        })
        .expect("a pipeline");
        let submission = pipeline.submit(&stream, super::Queue::Draw, &[], &Nothing);

        let mut offset = None;
        let mut offsets = Vec::new();
        for command in &submission.commands {
            match command {
                RenderCommand::SetUserData {
                    stage: ShaderStage::Vertex,
                    words,
                } => offset = Some(words[0]),
                RenderCommand::Draw { .. } => {
                    offsets.push(offset.expect("user data before the draw"));
                }
                _ => {}
            }
        }
        assert_eq!(offsets.len(), 12, "twelve draws: {offsets:?}");
        // Nothing is mapped, so neither stage's shader was prepared for any draw: each would run
        // whatever shaders were bound before it.
        assert_eq!(submission.report.unshaded_draws, 12);
        let mut distinct = offsets.clone();
        distinct.sort_unstable();
        distinct.dedup();
        assert_eq!(distinct.len(), 12, "each draw its own offset: {offsets:?}");
    }

    /// A live pipeline prepares each submission on the register state the ones before it left
    /// (D737): the colour target one stream sized is the next stream's too, once the first is
    /// carried, and a stream that clears state starts from its clear. A pipeline that does not
    /// carry state prepares each stream on its own.
    #[test]
    fn register_state_carries_to_the_next_submission() {
        use super::Pipeline;
        use crate::packet::build::{command_header, nop, set_context_register};
        use orbistoun_translate::{Fidelity, Strategy, Width};

        struct Nothing;
        impl super::GuestMemory for Nothing {
            fn read(&self, _address: u64, _length: usize) -> Option<&[u8]> {
                None
            }
        }
        let bytes =
            |words: &[u32]| -> Vec<u8> { words.iter().flat_map(|w| w.to_le_bytes()).collect() };
        let pipeline = || {
            Pipeline::new(Strategy::Predicated {
                fidelity: Fidelity::Lane,
                width: Width::default(),
            })
            .expect("a pipeline")
        };
        // `CB_COLOR0_ATTRIB2`: 256 x 128.
        let sized = bytes(&set_context_register(0x3B0, (255 << 14) | 127));
        let later = bytes(&nop());
        let cleared = bytes(&[command_header(crate::cp::CLEAR_STATE, 1), 0]);

        let mut live = pipeline().carrying_register_state();
        live.submit(&sized, super::Queue::Draw, &[], &Nothing);
        assert!(
            live.submit(&later, super::Queue::Draw, &[], &Nothing)
                .targets
                .is_empty(),
            "nothing carried before the first is carried out"
        );
        live.carry(&sized, None);
        let next = live.submit(&later, super::Queue::Draw, &[], &Nothing);
        assert_eq!(
            next.targets
                .values()
                .map(|e| (e.width, e.height))
                .collect::<Vec<_>>(),
            vec![(256, 128)],
            "the next stream draws into the target the first sized"
        );
        assert_eq!(
            next.report.register_writes, 0,
            "counted as the stream's own"
        );
        assert!(
            live.submit(&cleared, super::Queue::Draw, &[], &Nothing)
                .targets
                .is_empty(),
            "a clear starts from itself"
        );
        live.carry(&cleared, None);
        assert!(
            live.submit(&later, super::Queue::Draw, &[], &Nothing)
                .targets
                .is_empty(),
            "and nothing from before it carries past it"
        );

        let mut whole = pipeline();
        whole.carry(&sized, None);
        assert!(
            whole
                .submit(&later, super::Queue::Draw, &[], &Nothing)
                .targets
                .is_empty(),
            "a pipeline that does not carry state prepares each stream on its own"
        );
    }

    /// Each draw is restricted to the scissor in force at it, not the stream's last one. Bugdom
    /// draws its 3D pane under one scissor and its HUD bars above and below under another; one
    /// scissor for the whole submission clipped the bars away, leaving the clear colour.
    #[test]
    fn each_draw_is_restricted_to_the_scissor_in_force_at_it() {
        use super::{Pipeline, Rect, RenderCommand};
        use orbistoun_translate::{Fidelity, Strategy, Width};

        struct Nothing;
        impl super::GuestMemory for Nothing {
            fn read(&self, _address: u64, _length: usize) -> Option<&[u8]> {
                None
            }
        }
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/captures/agc-gl-cube-fw1240-b.hex");
        let words: Vec<u32> = std::fs::read_to_string(&path)
            .expect("the committed capture")
            .lines()
            .flat_map(|line| {
                line.split('#')
                    .next()
                    .unwrap_or_default()
                    .split_whitespace()
            })
            .map(|word| u32::from_str_radix(word, 16).expect("a hex word"))
            .collect();
        let bytes: Vec<u8> = words.iter().copied().flat_map(u32::to_le_bytes).collect();
        let draws = crate::registers::draw_calls(&crate::packet::walk(&bytes), &bytes);
        assert_eq!(draws.len(), 12);
        // SET_CONTEXT_REG of GENERIC_SCISSOR_TL and _BR: (x, y) packed as y << 16 | x.
        let scissor = |(x0, y0): (u32, u32), (x1, y1): (u32, u32)| {
            [0xC002_6900, 0x90, y0 << 16 | x0, y1 << 16 | x1]
        };
        let pane = scissor((0, 140), (1920, 945));
        let whole = scissor((0, 0), (1920, 1080));
        // The pane's scissor before the first draw, the whole target's before the seventh.
        let first = draws[0].packet_offset as usize / 4;
        let seventh = draws[6].packet_offset as usize / 4;
        let mut spliced = words[..first].to_vec();
        spliced.extend(pane);
        spliced.extend(&words[first..seventh]);
        spliced.extend(whole);
        spliced.extend(&words[seventh..]);
        let stream: Vec<u8> = spliced.into_iter().flat_map(u32::to_le_bytes).collect();

        let mut pipeline = Pipeline::new(Strategy::Predicated {
            fidelity: Fidelity::Lane,
            width: Width::default(),
        })
        .expect("a pipeline");
        let submission = pipeline.submit(&stream, super::Queue::Draw, &[], &Nothing);
        let mut in_force = None;
        let mut per_draw = Vec::new();
        for command in &submission.commands {
            match command {
                RenderCommand::SetViewport(rect) => in_force = Some(*rect),
                RenderCommand::Draw { .. } => per_draw.push(in_force),
                _ => {}
            }
        }
        let rect = |x, y, width, height| {
            Some(Rect {
                x,
                y,
                width,
                height,
            })
        };
        assert_eq!(per_draw.len(), 12);
        assert!(
            per_draw[..6].iter().all(|r| *r == rect(0, 140, 1920, 805)),
            "{per_draw:?}"
        );
        assert!(
            per_draw[6..].iter().all(|r| *r == rect(0, 0, 1920, 1080)),
            "{per_draw:?}"
        );
    }

    /// The base a live vertex program forms is read from its own words (D711), and a register
    /// written twice names no base.
    #[test]
    fn the_constant_base_a_vertex_program_forms_is_found_and_a_reassigned_one_is_not() {
        use orbistoun_shader::{EncodingTable, OperandTable, decode_program};
        let (encodings, operands) = (
            EncodingTable::builtin().expect("encodings"),
            OperandTable::builtin().expect("operands"),
        );
        let program = console_vertex_program();
        let decoded = decode_program(&program, &encodings, &operands);
        assert_eq!(
            super::constant_address_base(&decoded, &encodings),
            Some(0x2_0090_0000),
            "the pair its vertex loads add to - not the canary's pair behind it"
        );

        // `s_mov_b32 s2, 0x1234` inserted first (a decode stops at `s_endpgm`): s2 is no longer
        // constant, so the vertex loads' pair names nothing and the scan moves to the canary
        // store's constant `s6:s7`, `0x2_0092_0000`.
        let mut reassigned = 0xbe82_03ffu32.to_le_bytes().to_vec();
        reassigned.extend(0x1234u32.to_le_bytes());
        reassigned.extend(program);
        let decoded = decode_program(&reassigned, &encodings, &operands);
        assert_eq!(
            super::constant_address_base(&decoded, &encodings),
            Some(0x2_0092_0000),
            "a register written twice names no base; the next constant pair does"
        );
    }

    /// A shader is decoded again when the bytes at its address change, and only then.
    #[test]
    fn a_shader_rewritten_in_place_is_decoded_again() {
        use super::{Pipeline, Prepared, ShaderStage, WaveWidths};
        use orbistoun_translate::{Fidelity, Strategy, Width};

        const AT: u64 = 0x1_0000;
        struct At(Vec<u8>);
        impl super::GuestMemory for At {
            fn read(&self, address: u64, length: usize) -> Option<&[u8]> {
                let start = usize::try_from(address.checked_sub(AT)?).ok()?;
                self.0.get(start..start.checked_add(length)?)
            }
        }
        let placed = |program: &[u8]| {
            let mut bytes = program.to_vec();
            bytes.resize(super::MAX_SHADER_BYTES, 0);
            At(bytes)
        };
        let mut pipeline = Pipeline::new(Strategy::Predicated {
            fidelity: Fidelity::Lane,
            width: Width::default(),
        })
        .expect("a pipeline");
        let prepare = |pipeline: &mut Pipeline, memory: &At| match pipeline.prepare(
            AT,
            ShaderStage::Vertex,
            (None, WaveWidths::default(), super::ForDraw::default()),
            memory,
        ) {
            Ok(Prepared::Fresh { resource, .. }) => (resource, true),
            Ok(Prepared::Cached { resource }) => (resource, false),
            Err(e) => panic!("the program prepares: {e:?}"),
        };

        let program = console_vertex_program();
        let original = placed(&program);
        let (first, fresh) = prepare(&mut pipeline, &original);
        assert!(fresh, "the first sight of a shader translates it");
        assert_eq!(prepare(&mut pipeline, &original), (first, false));
        let decoded = pipeline.decoded.get(&AT).map_or(0, Vec::len);

        let mut rewritten = 0xbe82_03ffu32.to_le_bytes().to_vec();
        rewritten.extend(0x1234u32.to_le_bytes());
        rewritten.extend(&program);
        let (second, fresh) = prepare(&mut pipeline, &placed(&rewritten));
        assert!(
            fresh && second != first,
            "a rewritten shader translates again"
        );
        assert_eq!(
            pipeline.decoded.get(&AT).map(Vec::as_slice),
            Some(&rewritten[..decoded + 8]),
            "and its decode, one move longer, replaces the earlier one"
        );
    }

    /// A shader that reads guest memory is known as one by the resource it became, so a draw that
    /// runs it with no window mapped can be refused rather than drawn from zeros; a shader that
    /// reads none is not.
    #[test]
    fn a_shader_that_reads_guest_memory_is_known_by_its_resource() {
        use super::{Pipeline, Prepared, ShaderStage, WaveWidths};
        use orbistoun_translate::{Fidelity, Strategy, Width};

        const AT: u64 = 0x1_0000;
        struct At(Vec<u8>);
        impl super::GuestMemory for At {
            fn read(&self, address: u64, length: usize) -> Option<&[u8]> {
                let start = usize::try_from(address.checked_sub(AT)?).ok()?;
                self.0.get(start..start.checked_add(length)?)
            }
        }
        let mut pipeline = Pipeline::new(Strategy::Predicated {
            fidelity: Fidelity::Lane,
            width: Width::default(),
        })
        .expect("a pipeline");
        let mut prepare = |program: &[u8]| {
            let mut bytes = program.to_vec();
            bytes.resize(super::MAX_SHADER_BYTES, 0);
            match pipeline.prepare(
                AT,
                ShaderStage::Vertex,
                (None, WaveWidths::default(), super::ForDraw::default()),
                &At(bytes),
            ) {
                Ok(Prepared::Fresh { resource, .. }) => resource,
                other => panic!("the program prepares fresh: {:?}", other.err()),
            }
        };
        let reading = prepare(&console_vertex_program());
        let silent = prepare(&0xbf81_0000_u32.to_le_bytes());
        assert!(pipeline.memory_readers.contains_key(&reading));
        assert!(!pipeline.memory_readers.contains_key(&silent));
    }

    /// A scalar load that only fills an image's descriptor registers - radeonsi's blit pixel
    /// shader's `s_load_dwordx8 s[8:15], s[0:1], 0x400` before `image_load_mip` - does not make
    /// the shader depend on the window, since the texture is bound from where it reads; the same
    /// load with one of its registers read by anything else does.
    #[test]
    fn a_load_that_only_names_an_image_does_not_reach_the_window() {
        use orbistoun_shader::{EncodingTable, OperandTable, decode_program};
        let encodings = EncodingTable::builtin().expect("encodings");
        let operands = OperandTable::builtin().expect("operands");
        let words = |program: &[u32]| -> Vec<u8> {
            program.iter().flat_map(|word| word.to_le_bytes()).collect()
        };
        let load = [0xf40c_0200, 0xfa00_0400];
        let image = [0xf004_1f08, 0x0002_0002];
        let read_s9 = 0xbe94_0309;
        let end = 0xbf81_0000;
        let only_image = words(&[load[0], load[1], image[0], image[1], end]);
        let also_read = words(&[load[0], load[1], image[0], image[1], read_s9, end]);
        let reaches = |bytes: &[u8]| {
            super::reaches_guest_memory(
                &decode_program(bytes, &encodings, &operands),
                &encodings,
                &std::collections::BTreeMap::new(),
            )
        };
        assert_eq!(reaches(&only_image), None);
        // Its registers read before the load, or after they are written again, are another
        // value's: ACO reuses a texture's registers for a constant buffer's descriptor.
        // s_mov_b64 s[9:10], 0: a read counts its register and the next, as a pair source might.
        let write_s9 = 0xbe89_0480;
        let read_before = words(&[read_s9, load[0], load[1], image[0], image[1], end]);
        let read_after_rewrite =
            words(&[load[0], load[1], image[0], image[1], write_s9, read_s9, end]);
        assert_eq!(reaches(&read_before), None);
        assert_eq!(reaches(&read_after_rewrite), None);
        // The access is named, so a refused draw says which instruction to bind or trace.
        assert_eq!(
            reaches(&also_read).as_deref(),
            Some("s_load_dwordx8 at 0x0 (0xf40c0200 0xfa000400) s8, s0, 0x400")
        );
    }

    /// The window a vertex program places follows its bytes, not its address.
    #[test]
    fn a_vertex_program_rewritten_in_place_places_its_own_window() {
        use super::{Candidate, Pipeline, ShaderStage};
        use orbistoun_translate::{Fidelity, Strategy, Width};

        const AT: u64 = 0x1_0000;
        struct Placed {
            program: Vec<u8>,
            elsewhere: Vec<u8>,
        }
        impl super::GuestMemory for Placed {
            fn read(&self, address: u64, length: usize) -> Option<&[u8]> {
                match usize::try_from(address.checked_sub(AT)?).ok()? {
                    start if start < self.program.len() => {
                        self.program.get(start..start.checked_add(length)?)
                    }
                    _ => self.elsewhere.get(..length),
                }
            }
        }
        let placed = |program: &[u8]| {
            let mut bytes = program.to_vec();
            bytes.resize(super::MAX_SHADER_BYTES, 0);
            Placed {
                program: bytes,
                elsewhere: vec![0; super::PLACED_WINDOW_MAX_WORDS as usize * 4],
            }
        };
        let mut pipeline = Pipeline::new(Strategy::Predicated {
            fidelity: Fidelity::Lane,
            width: Width::default(),
        })
        .expect("a pipeline")
        .placing_window_from_shaders();
        let vertex = [Candidate {
            address: AT,
            stage: ShaderStage::Vertex,
        }];

        let program = console_vertex_program();
        let original = placed(&program);
        pipeline.place_window(&vertex, &original);
        assert_eq!(pipeline.window().address(), 0x2_0090_0000);
        pipeline.place_window(&vertex, &original);
        assert_eq!(pipeline.window().address(), 0x2_0090_0000, "from the cache");

        let mut rewritten = 0xbe82_03ffu32.to_le_bytes().to_vec();
        rewritten.extend(0x1234u32.to_le_bytes());
        rewritten.extend(&program);
        pipeline.place_window(&vertex, &placed(&rewritten));
        assert_eq!(
            pipeline.window().address(),
            0x2_0092_0000,
            "the rewritten program's own base, not the cached one"
        );
    }

    /// A topology maps to the mesh primitive of its shape (D688), or is refused by name when it has
    /// none, such as an unknown value.
    #[test]
    fn a_topology_maps_to_its_primitive_or_is_refused_by_name() {
        use super::{MeshPrimitive, PrimitiveTopology, mesh_primitive_of};

        assert_eq!(
            mesh_primitive_of(PrimitiveTopology::PointList),
            Ok(MeshPrimitive::Points)
        );
        assert_eq!(
            mesh_primitive_of(PrimitiveTopology::LineStrip),
            Ok(MeshPrimitive::Lines)
        );
        assert_eq!(
            mesh_primitive_of(PrimitiveTopology::TriangleStrip),
            Ok(MeshPrimitive::Triangles)
        );

        assert_eq!(
            mesh_primitive_of(PrimitiveTopology::RectangleList),
            Ok(MeshPrimitive::Rectangles)
        );
        let other = mesh_primitive_of(PrimitiveTopology::Other(9))
            .unwrap_err()
            .to_string();
        assert!(
            other.contains("VGT_GS_OUTPRIM_TYPE 9"),
            "an unmeasured value is refused by its raw field: {other}"
        );
    }

    /// The index count distinguishes the three shapes: one for a point, two for a line, three for a
    /// triangle.
    #[test]
    fn a_primitive_carries_its_index_count_and_a_distinct_cache_salt() {
        use super::{MeshPrimitive, primitive_salt};

        assert_eq!(MeshPrimitive::Points.indices(), 1);
        assert_eq!(MeshPrimitive::Lines.indices(), 2);
        assert_eq!(MeshPrimitive::Triangles.indices(), 3);

        // Distinct salts keep the same bytes at two topologies in separate cache entries.
        let salts = [
            primitive_salt(MeshPrimitive::Points),
            primitive_salt(MeshPrimitive::Lines),
            primitive_salt(MeshPrimitive::Triangles),
            primitive_salt(MeshPrimitive::Rectangles),
        ];
        let mut distinct = salts.to_vec();
        distinct.sort_unstable();
        distinct.dedup();
        assert!(
            distinct.len() == salts.len(),
            "each primitive salts the cache key differently: {salts:?}"
        );
    }

    /// A cache entry matches its own bytes, so a `matches` that always answers false fails here.
    #[test]
    fn a_cache_entry_recognises_the_shader_it_was_built_from() {
        let bytes: Vec<u8> = (0..32u8).collect();
        let entry = Cached::of(ResourceId(1), &bytes);
        assert!(entry.matches(&bytes));
    }

    /// Two different shaders never satisfy one cache entry.
    #[test]
    fn a_cache_entry_rejects_a_shader_that_only_looks_like_it() {
        // Each field alone: comparing only the length accepts an edit in the middle, and comparing
        // only the ends accepts a different-sized shader with the same ends.
        let bytes: Vec<u8> = (0..32u8).collect();
        let entry = Cached::of(ResourceId(1), &bytes);

        let mut shorter = bytes.clone();
        shorter.truncate(28);
        assert!(
            !entry.matches(&shorter),
            "a different length is a different shader"
        );

        let mut first_changed = bytes.clone();
        first_changed[0] ^= 0xFF;
        assert!(!entry.matches(&first_changed), "the first word is compared");

        let mut last_changed = bytes.clone();
        let end = last_changed.len() - 1;
        last_changed[end] ^= 0xFF;
        assert!(!entry.matches(&last_changed), "the last word is compared");
    }

    /// A shader shorter than a word is compared without an out-of-bounds index: `read_window`
    /// narrows, so one near the end of a mapping can be that short.
    #[test]
    fn a_shader_too_short_to_hold_a_word_does_not_panic() {
        let entry = Cached::of(ResourceId(1), &[1, 2]);
        assert!(entry.matches(&[1, 2]));
        assert!(!entry.matches(&[3, 4]));
    }

    /// A draw under `PRIMGEN_PASSTHRU_EN` is seeded for a passthrough primitive shader, its
    /// primitive packed in `v0`; one without it is not.
    #[test]
    fn a_passthrough_draw_is_seeded_as_one() {
        use super::{DI_PT_TRILIST, PRIMGEN_PASSTHRU_EN, VGT_PRIMITIVE_TYPE, VGT_SHADER_STAGES_EN};
        use crate::registers::{DrawCall, DrawKind};
        let draw = DrawCall {
            packet_offset: 0,
            instances: 1,
            kind: DrawKind::Auto { vertices: 3 },
        };
        for (stages, passthrough) in [(PRIMGEN_PASSTHRU_EN, true), (0, false)] {
            let registers = move |register| match register {
                VGT_PRIMITIVE_TYPE => Some(DI_PT_TRILIST),
                VGT_SHADER_STAGES_EN => Some(stages),
                _ => None,
            };
            assert_eq!(
                super::draw_geometry(&draw, registers).map(|g| g.passthrough),
                Ok(passthrough)
            );
        }
    }

    /// A draw's geometry-engine inputs are seeded for a non-indexed, single-instance list or strip
    /// of triangles, without passthrough or an index offset (D730); anything else says why not.
    #[test]
    fn a_draw_s_geometry_is_seeded_only_for_one_instance_of_triangles() {
        use super::{
            DI_PT_RECTLIST, DI_PT_TRILIST, GE_INDX_OFFSET, GeometryInputs, VGT_PRIMITIVE_TYPE,
            VGT_SHADER_STAGES_EN, draw_geometry,
        };
        use crate::registers::{DrawCall, DrawKind};
        let draw = |vertices, instances| DrawCall {
            packet_offset: 0,
            instances,
            kind: DrawKind::Auto { vertices },
        };
        let registers = |topology: u32, stages: u32, offset: u32| {
            move |register| match register {
                VGT_PRIMITIVE_TYPE => Some(topology),
                VGT_SHADER_STAGES_EN => Some(stages),
                GE_INDX_OFFSET => Some(offset),
                _ => None,
            }
        };
        assert_eq!(
            draw_geometry(&draw(3, 1), registers(DI_PT_RECTLIST, 0x0041_2010, 0)),
            Ok(GeometryInputs {
                first_vertex: 0,
                vertices: 3,
                primitives: 1,
                assembly: super::Assembly::List,
                indices: None,
                passthrough: false,
            })
        );
        // A strip of six vertices is four triangles, its provoking convention from
        // `PA_SU_SC_MODE_CNTL`.
        let strip = |mode: u32| {
            move |register| match register {
                VGT_PRIMITIVE_TYPE => Some(super::DI_PT_TRISTRIP),
                super::PA_SU_SC_MODE_CNTL => Some(mode),
                _ => None,
            }
        };
        for (mode, provoking_last) in [(super::PROVOKING_VTX_LAST, true), (0, false)] {
            assert_eq!(
                draw_geometry(&draw(6, 1), strip(mode)),
                Ok(GeometryInputs {
                    first_vertex: 0,
                    vertices: 6,
                    primitives: 4,
                    assembly: super::Assembly::Strip { provoking_last },
                    indices: None,
                    passthrough: false,
                })
            );
        }
        assert_eq!(
            draw_geometry(&draw(30, 1), registers(DI_PT_TRILIST, 0, 0)).map(|g| g.primitives),
            Ok(10)
        );
        for refused in [
            draw_geometry(&draw(3, 2), registers(DI_PT_RECTLIST, 0, 0)),
            draw_geometry(&draw(3, 1), registers(5, 0, 0)),
            draw_geometry(&draw(3, 1), registers(DI_PT_RECTLIST, 0, 4)),
        ] {
            assert!(refused.is_err(), "{refused:?}");
        }
        // Larger than a wave is not refused: it is drawn in chunks (D741).
        assert!(draw_geometry(&draw(96, 1), registers(DI_PT_TRILIST, 0, 0)).is_ok());
        // An indexed draw takes its index size from `VGT_INDEX_TYPE` (D740): sixteen and
        // thirty-two bits are read, an unwritten type as sixteen (D754); eight is not, nor is
        // primitive restart.
        let indexed = DrawCall {
            packet_offset: 0,
            instances: 1,
            kind: DrawKind::Indexed {
                indices: 6,
                address: 0,
            },
        };
        let with_index = |kind: Option<u32>, restart: u32| {
            move |register| match register {
                VGT_PRIMITIVE_TYPE => Some(DI_PT_TRILIST),
                super::VGT_INDEX_TYPE => kind,
                super::GE_MULTI_PRIM_IB_RESET_EN => Some(restart),
                _ => None,
            }
        };
        for (kind, width) in [
            (0, orbistoun_translate::wavefront::IndexWidth::Bits16),
            (1, orbistoun_translate::wavefront::IndexWidth::Bits32),
        ] {
            let geometry = draw_geometry(&indexed, with_index(Some(kind), 0));
            assert_eq!(
                geometry.map(|g| (g.vertices, g.primitives, g.indices)),
                Ok((6, 2, Some(width)))
            );
        }
        assert_eq!(
            draw_geometry(&indexed, with_index(None, 0)).map(|g| g.indices),
            Ok(Some(orbistoun_translate::wavefront::IndexWidth::Bits16)),
            "an unwritten size is sixteen bits"
        );
        for refused in [
            draw_geometry(&indexed, with_index(Some(2), 0)),
            draw_geometry(&indexed, with_index(Some(0), 1)),
        ] {
            assert!(refused.is_err(), "{refused:?}");
        }
    }

    /// Lines are seeded as triangles are, two vertices each (`ac_nir_lower_ngg.c:130`): a line list
    /// of six vertices is three lines, a line strip of four three too.
    #[test]
    fn a_line_list_and_strip_are_seeded() {
        use super::{Assembly, DI_PT_LINELIST, DI_PT_LINESTRIP, VGT_PRIMITIVE_TYPE, draw_geometry};
        use crate::registers::{DrawCall, DrawKind};
        for (topology, vertices, assembly) in [
            (DI_PT_LINELIST, 6, Assembly::LineList),
            (DI_PT_LINESTRIP, 4, Assembly::LineStrip),
        ] {
            let draw = DrawCall {
                packet_offset: 0,
                instances: 1,
                kind: DrawKind::Auto { vertices },
            };
            let latest = |register| (register == VGT_PRIMITIVE_TYPE).then_some(topology);
            assert_eq!(
                draw_geometry(&draw, latest).map(|g| (g.primitives, g.assembly)),
                Ok((3, assembly))
            );
        }
    }

    /// A draw one wave does not hold is split as the geometry engine splits it into subgroups
    /// (D741): a list into whole triangles, a strip into an even count of them overlapping by two
    /// vertices, each chunk reading its vertex ids from where its first primitive's begin.
    #[test]
    fn a_draw_larger_than_a_wave_is_split_into_chunks_it_holds() {
        use super::{Assembly, ChunkPlan, GeometryInputs, IndexBuffer, chunk_plans};
        use orbistoun_translate::wavefront::IndexWidth;
        let whole = |vertices, primitives, assembly, indices| GeometryInputs {
            first_vertex: 0,
            vertices,
            primitives,
            assembly,
            indices,
            passthrough: false,
        };
        assert!(chunk_plans(whole(63, 21, Assembly::List, None), None, 64).is_empty());
        // 96 vertices of a list at 64 lanes: 21 triangles, then 11.
        let list = chunk_plans(whole(96, 32, Assembly::List, None), None, 64);
        assert_eq!(
            list,
            [
                ChunkPlan {
                    geometry: whole(63, 21, Assembly::List, Some(IndexWidth::Bits32)),
                    index_buffer: IndexBuffer::Counting { first: 0 },
                },
                ChunkPlan {
                    geometry: whole(33, 11, Assembly::List, Some(IndexWidth::Bits32)),
                    index_buffer: IndexBuffer::Counting { first: 63 },
                },
            ]
        );
        // An indexed strip of 70 vertices (68 triangles) at 32 lanes: 30, 30, then 8 triangles,
        // each starting 30 indices on.
        let strip = Assembly::Strip {
            provoking_last: true,
        };
        let plans = chunk_plans(
            whole(70, 68, strip, Some(IndexWidth::Bits16)),
            Some(0x1000),
            32,
        );
        let got: Vec<_> = plans
            .iter()
            .map(|plan| {
                (
                    plan.geometry.vertices,
                    plan.geometry.primitives,
                    plan.index_buffer,
                )
            })
            .collect();
        let guest = |address| IndexBuffer::Guest { address, bytes: 2 };
        // 100 vertices of a line list at 64 lanes: 32 lines, then 18, the second from index 64.
        let lines = chunk_plans(whole(100, 50, Assembly::LineList, None), None, 64);
        assert_eq!(
            lines
                .iter()
                .map(|plan| (plan.geometry.vertices, plan.index_buffer))
                .collect::<Vec<_>>(),
            [
                (64, IndexBuffer::Counting { first: 0 }),
                (36, IndexBuffer::Counting { first: 64 }),
            ]
        );
        assert_eq!(
            got,
            [
                (32, 30, guest(0x1000)),
                (32, 30, guest(0x1000 + 60)),
                (10, 8, guest(0x1000 + 120)),
            ]
        );
    }

    /// A linear image of sixteen-byte texels reads as each texel's four words in order, row by row
    /// at its pitch, and writes back to the same bytes, leaving a row's padding alone.
    #[test]
    fn a_linear_image_of_sixteen_byte_texels_reads_and_writes_by_words() {
        let surface = super::DispatchSurface::Linear {
            base: 0,
            width: 2,
            height: 2,
            pitch: 3,
            bytes_per_texel: 16,
        };
        let mut spanned: Vec<u8> = (0..2 * 3 * 16).map(|i| i as u8).collect();
        let texels = surface.detile(&spanned);
        let word = |byte: usize| u32::from_le_bytes(spanned[byte..byte + 4].try_into().unwrap());
        let expected: Vec<u32> = [0, 16, 48, 64]
            .iter()
            .flat_map(|&texel| (0..4).map(move |w| texel + w * 4))
            .map(word)
            .collect();
        assert_eq!(texels, expected);
        let written: Vec<u32> = (0..16).map(|i| 0xa0a0_0000 | i).collect();
        surface
            .tile(&written, &mut spanned)
            .expect("the extent matches");
        assert_eq!(surface.detile(&spanned), written);
        assert_eq!(spanned[32], 32, "a row's padding texel is left alone");
    }
}
