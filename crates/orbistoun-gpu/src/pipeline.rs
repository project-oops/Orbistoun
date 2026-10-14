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
use orbistoun_translate::wavefront::MAX_TEXTURES;
use orbistoun_translate::wavefront::MeshPrimitive;
use orbistoun_translate::wavefront::Stage;
use orbistoun_translate::wavefront::Window;
use orbistoun_translate::wavefront::{
    Assembly, GeometryInputs, TableBase, TableWord, TextureSource, USER_DATA_STAGE_WORDS, UserData,
    WINDOW_SPACE_SCALE, WindowSpace,
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
    colour_target_format_at, colour_target_runs, decode_image_descriptor, depth_control_at,
    dispatch_calls, register_writes, register_writes_reading, shader_candidates,
    stencil_control_at, viewport_transform_from,
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
    window_space: WindowSpace,
    buffer_formats: Option<orbistoun_translate::wavefront::BufferFormats>,
    flat_twins: Option<FlatTwins>,
    saturated: [[bool; 2]; MAX_TEXTURES],
    /// The colour targets that hold integers, bit `K` for `mrtK` (D851).
    integer_targets: u8,
}

/// No texture slot's coordinates saturated.
const UNSATURATED: [[bool; 2]; MAX_TEXTURES] = [[false; 2]; MAX_TEXTURES];

/// The shaders a run could not translate, by address: what it writes beside its report, so each
/// refusal can be read against the code. At most [`REFUSED_SHADERS_KEPT`].
fn refused_shaders() -> &'static std::sync::Mutex<BTreeMap<u64, Vec<u8>>> {
    static SHADERS: std::sync::Mutex<BTreeMap<u64, Vec<u8>>> =
        std::sync::Mutex::new(BTreeMap::new());
    &SHADERS
}

/// How many refused shaders a run keeps.
const REFUSED_SHADERS_KEPT: usize = 16;

/// Keeps a shader that could not be translated, while there is room.
fn keep_refused_shader(address: u64, shader: &[u8]) {
    let mut kept = refused_shaders()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if kept.len() < REFUSED_SHADERS_KEPT || kept.contains_key(&address) {
        kept.insert(address, shader.to_vec());
    }
}

/// The shaders a run could not translate, each with its address.
#[must_use]
pub fn refused_shader_programs() -> Vec<(u64, Vec<u8>)> {
    refused_shaders()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .iter()
        .map(|(address, bytes)| (*address, bytes.clone()))
        .collect()
}

/// The program of the most recent dispatch refused while its buffers were traced, and where it
/// lies: what a run writes beside its report, so the refusal can be read against the code.
fn refused_program() -> &'static std::sync::Mutex<Option<(u64, Vec<u8>)>> {
    static PROGRAM: std::sync::Mutex<Option<(u64, Vec<u8>)>> = std::sync::Mutex::new(None);
    &PROGRAM
}

/// The program of the most recent dispatch refused while its buffers were traced, and where it
/// lies.
#[must_use]
pub fn last_refused_program() -> Option<(u64, Vec<u8>)> {
    refused_program()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

/// A dispatch refused while its buffers were traced: its program kept ([`last_refused_program`]),
/// and a refusal for an instruction with no name naming the program at `address`, the instruction,
/// and every opcode in the program the tables lack, so one request records them all.
fn dispatch_refusal(
    why: String,
    (decoded, encodings): (&orbistoun_shader::Decode, &EncodingTable),
    (address, program): (u64, &[u8]),
) -> String {
    *refused_program()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some((address, program.to_vec()));
    if !why.contains("has no name") {
        return why;
    }
    let Some(which) = orbistoun_translate::evaluate::first_unnamed(decoded, encodings) else {
        return why;
    };
    with_unnamed_census(
        format!("{why}: the program at {address:#x}, {which}"),
        (decoded, encodings),
    )
}

/// A compute program the translator refused: kept beside the report as a refused shader is, and
/// the refusal naming the instruction it refused and every opcode the program holds unnamed.
fn compute_refusal(
    error: &orbistoun_translate::TranslateError,
    (decoded, encodings): (&orbistoun_shader::Decode, &EncodingTable),
    (address, program): (u64, &[u8]),
) -> String {
    keep_refused_shader(address, program);
    let named = refused_instruction(error)
        .and_then(|offset| instruction_text(decoded, encodings, offset))
        .map(|text| format!(" ({text})"))
        .unwrap_or_default();
    with_unnamed_census(
        format!("the compute program at {address:#x} could not be translated: {error}{named}"),
        (decoded, encodings),
    )
}

/// `why`, followed by every opcode of the program the tables give no name, where there is one:
/// what one request to record them lists.
fn with_unnamed_census(
    why: String,
    (decoded, encodings): (&orbistoun_shader::Decode, &EncodingTable),
) -> String {
    let every: Vec<String> = orbistoun_translate::evaluate::unnamed_opcodes(decoded, encodings)
        .iter()
        .map(ToString::to_string)
        .collect();
    if every.is_empty() {
        return why;
    }
    format!(
        "{why}; every opcode the program holds that no table names: {}",
        every.join(", ")
    )
}

#[cfg(test)]
mod refusal_tests {
    use orbistoun_shader::{EncodingTable, OperandTable, decode_program};

    /// A dispatch refused for an instruction with no name lists every opcode of its program the
    /// tables lack, not the first alone: VOP1 opcodes 0xff and 0xfe are assigned to nothing.
    #[test]
    fn a_refusal_for_no_name_lists_every_unnamed_opcode() {
        let encodings = EncodingTable::builtin().expect("encodings");
        let operands = OperandTable::builtin().expect("operands");
        let words = [0x7e01_fe80_u32, 0x7e01_fc80, 0x7e01_fe80, 0xbf81_0000];
        let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        let decoded = decode_program(&bytes, &encodings, &operands);
        let why = super::dispatch_refusal(
            "an instruction has no name".to_owned(),
            (&decoded, &encodings),
            (0x7400_0000, &bytes),
        );
        assert!(why.contains("the program at 0x74000000"), "{why}");
        assert!(
            why.ends_with("VOP1 opcode 0xff at 0x0 (2 times), VOP1 opcode 0xfe at 0x4"),
            "{why}"
        );
    }

    /// A refusal is followed by the program's unnamed opcodes where it has any, and left as it is
    /// where every instruction is named.
    #[test]
    fn a_refusal_carries_the_census_only_where_there_is_one() {
        let encodings = EncodingTable::builtin().expect("encodings");
        let operands = OperandTable::builtin().expect("operands");
        let program =
            |words: &[u32]| -> Vec<u8> { words.iter().flat_map(|w| w.to_le_bytes()).collect() };
        let unnamed = program(&[0x7e01_fc80, 0xbf81_0000]);
        let decoded = decode_program(&unnamed, &encodings, &operands);
        assert_eq!(
            super::with_unnamed_census("refused".to_owned(), (&decoded, &encodings)),
            "refused; every opcode the program holds that no table names: VOP1 opcode 0xfe at 0x0"
        );
        let named = program(&[0x7e00_0280, 0xbf81_0000]);
        let decoded = decode_program(&named, &encodings, &operands);
        assert_eq!(
            super::with_unnamed_census("refused".to_owned(), (&decoded, &encodings)),
            "refused"
        );
    }

    /// An access refused as not translated is quoted with its operands too: what it reads through
    /// is what decides how it can be made exact.
    #[test]
    fn a_refusal_not_translated_quotes_its_operands() {
        let encodings = EncodingTable::builtin().expect("encodings");
        let operands = OperandTable::builtin().expect("operands");
        let words = [0x7e00_0280_u32, 0xbf81_0000];
        let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        let decoded = decode_program(&bytes, &encodings, &operands);
        let error = orbistoun_translate::TranslateError::NotTranslated {
            offset: 0,
            mnemonic: "v_mov_b32_e32".to_owned(),
            detail: "refused",
        };
        let why = super::compute_refusal(&error, (&decoded, &encodings), (0x7400_4560, &bytes));
        assert!(
            why.ends_with("(`v_mov_b32_e32 v0, 0`, word 0x7e000280)"),
            "{why}"
        );
    }

    /// A compute program the translator refuses is named by the instruction it refused, words and
    /// all, and kept beside the report, as a refused shader is.
    #[test]
    fn a_refused_compute_program_names_its_instruction_and_is_kept() {
        let encodings = EncodingTable::builtin().expect("encodings");
        let operands = OperandTable::builtin().expect("operands");
        // v_mov_b32 v0, 0; s_endpgm.
        let words = [0x7e00_0280_u32, 0xbf81_0000];
        let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        let decoded = decode_program(&bytes, &encodings, &operands);
        let error = orbistoun_translate::TranslateError::Unsupported {
            offset: 0,
            detail: "refused",
        };
        let why = super::compute_refusal(&error, (&decoded, &encodings), (0x7400_1230, &bytes));
        assert!(
            why.starts_with("the compute program at 0x74001230 could not be translated: "),
            "{why}"
        );
        assert!(
            why.ends_with("(`v_mov_b32_e32 v0, 0`, word 0x7e000280)"),
            "{why}"
        );
        assert!(
            super::refused_shader_programs()
                .iter()
                .any(|(address, kept)| *address == 0x7400_1230 && *kept == bytes),
            "the program is kept"
        );
    }
}

/// Distinguishes the same primitive shader translated for different draws' geometry (D730) or
/// position space (D731), in the cache key.
fn for_draw_salt(for_draw: ForDraw) -> u64 {
    let geometry = for_draw.geometry.map_or(0, |g| {
        0x4745_4f4d_0000_0000
            ^ (u64::from(g.vertices) << 32)
            ^ (u64::from(g.primitives) << 16)
            ^ u64::from(g.first_vertex)
            ^ u64::from(g.legacy) << 63
            ^ u64::from(g.streamout) << 62
            ^ match g.indices {
                None => 0,
                Some(width) => 0x4944_5800_0000_0000 ^ u64::from(width.bytes()),
            }
            ^ match g.assembly {
                Assembly::List => 0,
                Assembly::LineList => 0x4c4c_0000_0000_0000,
                Assembly::LineStrip => 0x4c53_0000_0000_0000,
                Assembly::Points => 0x5054_0000_0000_0000,
                Assembly::Strip { provoking_last } => {
                    0x5354_0000_0000_0000 ^ u64::from(provoking_last)
                }
                Assembly::Quads { provoking_last } => {
                    0x5155_0000_0000_0000 ^ u64::from(provoking_last)
                }
            }
    });
    let formats = for_draw.buffer_formats.map_or(0, |formats| {
        let words: Vec<u32> = formats
            .slots()
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
    let integer = if for_draw.integer_targets == 0 {
        0
    } else {
        0x494e_5400_0000_0000 ^ u64::from(for_draw.integer_targets)
    };
    geometry
        ^ formats
        ^ twins
        ^ saturated
        ^ integer
        ^ match for_draw.window_space {
            WindowSpace::Clip => 0,
            WindowSpace::Divided => 0x5749_4e44_5350_4345,
            WindowSpace::Undivided => 0x5749_4e44_554e_4456,
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

/// Distinguishes a primitive shader run one invocation per lane (D760), and the subgroup it runs on,
/// in the cache key; for a dispatch, the subgroup its masks may be made by (D884).
const fn per_lane_salt(subgroup: Option<u32>) -> u64 {
    match subgroup {
        Some(width) => 0x5045_524c_414e_4500 ^ width as u64,
        None => 0,
    }
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
    /// Each texture the draws bind, by its descriptor, once: where it lies in guest memory, its
    /// format and layout - what a texture that reads as one value is traced to.
    pub bound_textures: Vec<ImageDescriptor>,
    /// Draws' sampled texture slots nothing could be bound to - an unreadable descriptor, or one
    /// this does not read - so the backend would sample its placeholder.
    pub unbound_textures: usize,
    /// The first of [`Self::unbound_textures`], described: its slot, where its descriptor was read,
    /// and the words there.
    pub first_unbound_texture: Option<String>,
    /// Draws whose viewport transform the stream turned off (`PA_CL_VTE_CNTL`), so their positions
    /// are not the clip space the backend draws.
    pub unmodelled_viewports: usize,
    /// Draws into a one-byte colour target with no `CB_TARGET_MASK` in the stream, so which of a
    /// four-channel drawer's channels stand for its one is not known.
    pub unmasked_one_byte_draws: usize,
    /// Draws that write a colour channel their pixel shader never exports, which would hold
    /// whatever the shader's output held.
    pub unexported_channel_draws: usize,
    /// Draws whose pixel shader exports to a colour target (`mrtK`) the submission binds no colour
    /// buffer for, so what it exports there would be drawn nowhere.
    pub unbound_export_draws: usize,
    /// Draws running a shader that reads or writes guest memory while no window is mapped, so the
    /// module would read zeros where the guest's data is, and the first such access, by name and
    /// offset.
    pub unwindowed_draws: (usize, Option<String>),
    /// Draws whose shaders read through a buffer the draw could not bind exactly (D733), and why the
    /// first was refused.
    pub unbound_buffers: (usize, Option<std::borrow::Cow<'static, str>>),
    /// Draws a stage's shader was not prepared for, which would run whatever shader was bound
    /// before them.
    pub unshaded_draws: usize,
    /// Draws whose primitives' layers or viewports are not drawn as the hardware draws them
    /// (D859), and why the first was refused.
    pub unmodelled_layers: (usize, Option<String>),
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
    /// The tables the stream's indirect register loads read, as they stood when it was submitted:
    /// what its draw segments and the registers it carries forward read them from.
    pub tables: crate::registers::TableSnapshot,
    /// Where the border colour table is, as `TA_BC_BASE_ADDR` last said: what a sampler's table
    /// border colour is read from (D837). `None` where the stream set it nowhere.
    pub border_table: Option<u64>,
    /// The register state earlier submissions left, as it stood when this one was prepared: what
    /// its draw segments and dispatches run under when it is carried out (D737), however far the
    /// carried state has moved on by then (D821).
    pub carried_in: BTreeMap<u32, u32>,
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
    /// Every colour target zero the draws draw into, one per run of draws into a base: what
    /// carrying the submission out writes, which a later submission's preparation reads only once
    /// it has been carried out (D874). Empty when nothing is drawn.
    pub drawn_targets: Vec<ColourTarget>,
    /// Each compute dispatch's state, in stream order, `None` for one the registers do not give:
    /// what carrying the submission out writes through them is worked out from these as it is
    /// queued (D876).
    pub dispatch_states: Vec<Option<crate::dispatch::DispatchState>>,
    /// Whether any of its draws streams out (D861), writing buffers through a compute wave.
    pub streams_out: bool,
    /// Whether its preparation read memory a submission queued ahead of it draws into, or the frame
    /// pending while one was queued, before that submission had drawn it (D882): carried out, its
    /// draws are prepared again from the stream, after the ones ahead of it.
    pub read_ahead_of_draws: bool,
    /// The depth- and stencil-test state a draw runs under (`DB_DEPTH_CONTROL`). `None` when the
    /// stream set none: a draw with no depth control has no depth test.
    pub depth_control: Option<DepthControl>,
    /// The stencil operations a draw applies on each test outcome (`DB_STENCIL_CONTROL`). `None`
    /// when the stream set none.
    pub stencil_control: Option<StencilControl>,
    /// Colour target zero's blend state (`CB_BLEND0_CONTROL`). `None` when the stream set none.
    pub blend_control: Option<BlendControl>,
    /// The colour attachments after the first, in the order [`RenderCommand::SetRenderTargets`]
    /// lists them (D825): each a further colour buffer the pixel shader's exports reach. Empty
    /// when the draws export to the first alone.
    pub more_colour_targets: Vec<MoreColourTarget>,
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
    submission.drawn_targets = drawn_targets(submission, writes, &draw_offsets);
}

/// The colour target zero each run of draws draws into ([`colour_target_runs`]), from the writes
/// before the run's first draw. One base needs no sweep: it is [`Submission::colour_target`].
fn drawn_targets(
    submission: &Submission,
    writes: &[RegisterWrite],
    draw_offsets: &[u32],
) -> Vec<ColourTarget> {
    if draw_offsets.is_empty() {
        return Vec::new();
    }
    if submission.colour_target_bases <= 1 {
        return submission.colour_target.into_iter().collect();
    }
    let spans: Vec<(u32, u32)> = draw_offsets.iter().map(|&at| (at, at + 1)).collect();
    let mut targets: Vec<ColourTarget> = colour_target_runs(writes, &spans)
        .into_iter()
        .filter_map(|(first, _)| {
            // Carried state rides at offset zero, ahead of the stream's own writes, in order.
            let before = writes.partition_point(|write| write.packet_offset <= first);
            colour_target_at(&writes[..before])
        })
        .collect();
    targets.dedup();
    targets
}

/// Each dispatch's state from the writes before it, and whether any draw streams out
/// ([`Submission::dispatch_states`], [`Submission::streams_out`]).
fn record_compute_work(
    submission: &mut Submission,
    writes: &[RegisterWrite],
    (walked, stream): (&PacketWalk, &[u8]),
    draws: &[DrawCall],
) {
    submission.dispatch_states = walked
        .packets
        .iter()
        .filter(|packet| {
            packet.kind
                == (crate::packet::PacketKind::Command {
                    opcode: crate::packet::build::DISPATCH_DIRECT,
                })
        })
        .map(|packet| {
            let start = packet.body_offset() as usize;
            let body = stream.get(start..start + 16)?;
            let word = |at: usize| {
                u32::from_le_bytes([body[at], body[at + 1], body[at + 2], body[at + 3]])
            };
            crate::dispatch::state_at(writes, packet.offset, [word(0), word(4), word(8)], word(12))
                .ok()
        })
        .collect();
    let mut sweep = crate::registers::RegisterSweep::new(writes);
    submission.streams_out = draws.iter().any(|draw| {
        sweep
            .latest(draw.packet_offset, crate::streamout::VGT_STRMOUT_CONFIG)
            .is_some_and(|config| config & 0xF != 0)
    });
}

/// The shaders one draw runs, by stage.
type DrawShaders = Vec<(ShaderStage, ResourceId)>;

/// A colour attachment after the first, as a submission set it up (D825): what
/// [`Submission::colour_target`] and its neighbours are for the first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MoreColourTarget {
    /// The id [`RenderCommand::SetRenderTargets`] names it by.
    pub id: ResourceId,
    /// Which colour buffer it is: `CB_COLORn`'s `n`.
    pub buffer: u32,
    /// Its extent.
    pub extent: ColourTargetExtent,
    /// Its base and layout, `None` where the stream did not set them.
    pub target: Option<ColourTarget>,
    /// Its tiling mode.
    pub tiling: Option<SwizzleMode>,
    /// Its element layout.
    pub format: Option<ColourTargetFormat>,
    /// Its delta colour compression.
    pub dcc: Option<crate::registers::ColourTargetDcc>,
}

/// The colour buffers a submission's attachments are, in attachment order: those
/// `SPI_SHADER_COL_FORMAT` formats, as the pixel shader's exports are numbered (D825), or colour
/// buffer zero alone where it formats none or the stream never set it.
fn attachment_buffers(writes: &[RegisterWrite]) -> Vec<u32> {
    match crate::colour_slots::exported_colour_buffers(writes) {
        Some(buffers) if !buffers.is_empty() => buffers,
        _ => vec![0],
    }
}

/// The colour targets a segment's pixel shader writes that hold integers, bit `K` for `mrtK`
/// (D851): those whose `CB_COLORn_INFO` names `NUMBER_UINT` or `NUMBER_SINT`, `mrtK` being the
/// `K`th colour buffer `SPI_SHADER_COL_FORMAT` formats.
fn integer_targets_of(writes: &[RegisterWrite]) -> u8 {
    attachment_buffers(writes)
        .iter()
        .take(8)
        .enumerate()
        .fold(0, |bits, (target, &buffer)| {
            let view = crate::colour_slots::colour_buffer_writes(writes, buffer);
            let integer = colour_target_format_at(&view).is_some_and(|f| f.is_integer());
            bits | u8::from(integer) << target
        })
}

/// The colour attachments `buffers` are, each read through the decoders colour buffer zero's are,
/// up to the first the stream gave no extent: attachment `K` is always `mrtK`'s buffer, so an
/// export to that one or a later one is refused as unbound rather than drawn into the next.
fn more_colour_targets(writes: &[RegisterWrite], buffers: &[u32]) -> Vec<MoreColourTarget> {
    buffers
        .iter()
        .map_while(|&buffer| {
            let view = crate::colour_slots::colour_buffer_writes(writes, buffer);
            let extent = colour_target_extent_at(&view)?;
            let target = colour_target_at(&view);
            Some(MoreColourTarget {
                id: colour_target_id(extent, target.map(|target| target.base)),
                buffer,
                extent,
                target,
                tiling: colour_swizzle_mode_at(&view),
                format: colour_target_format_at(&view),
                dcc: colour_target_dcc_at(&view),
            })
        })
        .collect()
}

/// A depth target with the id it is bound by and the colour extent it is sized to.
type SizedDepthTarget = (ResourceId, crate::depth::DepthTarget, ColourTargetExtent);

/// Selects the submission's render targets: the colour attachments its pixel shaders export to
/// (D825), the first sized and recorded as `targets` keeps it (D702) and the rest as
/// [`Submission::more_colour_targets`], and a depth target sized to the first. Answers the depth
/// target and the colour buffers in attachment order.
fn set_render_targets(
    submission: &mut Submission,
    writes: &[RegisterWrite],
) -> (Option<SizedDepthTarget>, Vec<u32>) {
    let buffers = attachment_buffers(writes);
    let first = crate::colour_slots::colour_buffer_writes(writes, buffers[0]);
    let depth_target = colour_target_extent_at(&first).and_then(|extent| {
        let target = crate::depth::depth_target_at(writes)?;
        Some((crate::depth::depth_target_id(&target), target, extent))
    });
    if let Some(extent) = colour_target_extent_at(&first) {
        let base = colour_target_at(&first).map(|target| target.base);
        let target = colour_target_id(extent, base);
        submission.targets.insert(target, extent);
        submission.more_colour_targets = more_colour_targets(writes, &buffers[1..]);
        let mut colour = vec![target];
        colour.extend(submission.more_colour_targets.iter().map(|more| more.id));
        submission.commands.push(RenderCommand::SetRenderTargets {
            colour,
            depth: depth_target.map(|(id, _, _)| id),
        });
    }
    (depth_target, buffers)
}

/// Unbinds the colour attachments after the first that no draw's pixel shader exports to: a
/// format the guest left set for a buffer its shaders never write binds nothing to draw (D825).
fn keep_exported_attachments(submission: &mut Submission, exported: usize) {
    let keep = exported.max(1);
    submission.more_colour_targets.truncate(keep - 1);
    if let Some(RenderCommand::SetRenderTargets { colour, .. }) = submission
        .commands
        .iter_mut()
        .find(|command| matches!(command, RenderCommand::SetRenderTargets { .. }))
    {
        colour.truncate(keep);
    }
}

#[cfg(test)]
mod attachment_tests {
    use super::{
        Submission, attachment_buffers, keep_exported_attachments, more_colour_targets,
        set_render_targets,
    };
    use crate::{RegisterWrite, RenderCommand};

    fn write(register: u32, value: u32) -> RegisterWrite {
        RegisterWrite {
            packet_offset: 0,
            register,
            value,
        }
    }

    /// A 64x32 linear `8_8_8_8` surface at `base` for colour buffer `n`: its base, `INFO`,
    /// `ATTRIB2` and `ATTRIB3`.
    fn surface(n: u32, base: u32) -> [RegisterWrite; 4] {
        [
            write(0xA318 + 15 * n, base),
            write(0xA31C + 15 * n, 0xA << 2),
            write(0xA3B0 + n, (63 << 14) | 31),
            write(0xA3B8 + n, 0),
        ]
    }

    /// The attachments are the buffers `SPI_SHADER_COL_FORMAT` formats, in order; buffer zero
    /// alone where it formats none or was never set.
    #[test]
    fn attachments_follow_the_export_formats() {
        assert_eq!(attachment_buffers(&[]), vec![0]);
        assert_eq!(attachment_buffers(&[write(0xA1C5, 0)]), vec![0]);
        assert_eq!(attachment_buffers(&[write(0xA1C5, 0x44)]), vec![0, 1]);
        assert_eq!(attachment_buffers(&[write(0xA1C5, 0x40)]), vec![1]);
    }

    /// Attachments stop at the first buffer with no extent, so attachment `K` stays `mrtK`'s.
    #[test]
    fn attachments_stop_at_a_buffer_with_no_extent() {
        let mut writes = surface(1, 0x200).to_vec();
        writes.extend(surface(3, 0x400));
        let more = more_colour_targets(&writes, &[1, 2, 3]);
        assert_eq!(more.iter().map(|m| m.buffer).collect::<Vec<_>>(), vec![1]);
        assert_eq!(more[0].target.map(|t| t.base), Some(0x200 << 8));
    }

    /// Two formatted buffers bind two attachments, the first in `targets`; an attachment no
    /// draw exports to is unbound again.
    #[test]
    fn a_second_attachment_is_bound_while_exported() {
        let mut writes = surface(0, 0x100).to_vec();
        writes.extend(surface(1, 0x200));
        writes.push(write(0xA1C5, 0x44));
        let mut submission = Submission::default();
        let (_, buffers) = set_render_targets(&mut submission, &writes);
        assert_eq!(buffers, vec![0, 1]);
        assert_eq!(submission.targets.len(), 1);
        assert_eq!(submission.more_colour_targets.len(), 1);
        let colour = |submission: &Submission| match &submission.commands[..] {
            [RenderCommand::SetRenderTargets { colour, .. }] => colour.len(),
            _ => 0,
        };
        assert_eq!(colour(&submission), 2);
        keep_exported_attachments(&mut submission, 2);
        assert_eq!(colour(&submission), 2);
        keep_exported_attachments(&mut submission, 1);
        assert_eq!(colour(&submission), 1);
        assert!(submission.more_colour_targets.is_empty());
    }
}

/// How many colour buffers the commands bind: the latest [`RenderCommand::SetRenderTargets`]'s.
fn bound_colour_targets(commands: &[RenderCommand]) -> usize {
    commands
        .iter()
        .rev()
        .find_map(|command| match command {
            RenderCommand::SetRenderTargets { colour, .. } => Some(colour.len()),
            _ => None,
        })
        .unwrap_or(0)
}

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
type PlainKey = (
    u32,
    u64,
    WindowSpace,
    u32,
    (Option<u32>, Option<u32>),
    Option<u32>,
);

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
    saturated: [[bool; 2]; MAX_TEXTURES],
    /// The colour targets of the segment being bound that hold integers (D851).
    integer_targets: u8,
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

/// `DI_PT_POINTLIST` (enum `VGT_DI_PRIM_TYPE`, `gfx103.json`): a point a vertex.
const DI_PT_POINTLIST: u32 = 1;
/// `DI_PT_LINELIST` and `DI_PT_LINESTRIP` (enum `VGT_DI_PRIM_TYPE`, `gfx103.json:689-690`): lines
/// of two vertices each, and lines sharing vertices in a strip.
const DI_PT_LINELIST: u32 = 2;
/// See [`DI_PT_LINELIST`].
const DI_PT_LINESTRIP: u32 = 3;
/// `DI_PT_TRISTRIP` (enum `VGT_DI_PRIM_TYPE`, `gfx103.json`): triangles sharing vertices in a strip.
const DI_PT_TRISTRIP: u32 = 6;
/// `DI_PT_QUADSTRIP` (enum `VGT_DI_PRIM_TYPE`, `gfx10.json:811`): quads sharing an edge in a strip,
/// seeded as the triangle strip over the same vertices (D797).
const DI_PT_QUADSTRIP: u32 = 20;
/// `DI_PT_QUADLIST` (enum `VGT_DI_PRIM_TYPE`, `gfx10.json:810`): quads of four vertices each, seeded
/// two triangles a quad (D798).
const DI_PT_QUADLIST: u32 = 19;
/// `PA_SU_SC_MODE_CNTL` (`gfx103.json`, context dword `0x205`) and its `PROVOKING_VTX_LAST`, bit 19.
const PA_SU_SC_MODE_CNTL: u32 = 0xA205;
/// See [`PA_SU_SC_MODE_CNTL`].
const PROVOKING_VTX_LAST: u32 = 1 << 19;
/// The shaders a draw runs, from the candidates its registers name: those its queue permits and
/// no registration already gave, the vertex program the primitive shader's or - where `legacy` -
/// the legacy vertex shader's (D881).
fn draw_candidates(
    inferred: Vec<crate::registers::ShaderCandidate>,
    legacy: bool,
    (queue, registered_stages): (Queue, &[ShaderStage]),
) -> Vec<Candidate> {
    inferred
        .into_iter()
        .filter(|inferred| runs_vertex_candidate(&inferred.stage, legacy))
        .filter_map(|inferred| {
            let stage = stage_of(&inferred.stage)?;
            (queue.permits(stage) && !registered_stages.contains(&stage)).then_some(Candidate {
                address: inferred.address,
                stage,
            })
        })
        .collect()
}

/// Whether a candidate named `name` is the vertex program a draw runs: the primitive shader's, or -
/// where `legacy`, a draw with none - the legacy vertex shader's (D881). Every other stage is.
fn runs_vertex_candidate(name: &str, legacy: bool) -> bool {
    match name {
        "vertex" => !legacy,
        "legacy-vertex" => legacy,
        _ => true,
    }
}

/// The output primitive a legacy vertex shader's draw assembles (D881), as `VGT_GS_OUT_PRIM_TYPE`
/// codes, from its input topology: radeonsi writes that register only for a primitive or geometry
/// shader (`si_state_draw.cpp` 1083), so a legacy draw's is its input's.
fn legacy_output_primitive(primitive_type: Option<u32>) -> Option<u32> {
    match primitive_type? & 0x3F {
        DI_PT_POINTLIST => Some(0),
        DI_PT_LINELIST | DI_PT_LINESTRIP => Some(1),
        DI_PT_TRILIST | DI_PT_TRISTRIP | DI_PT_QUADLIST | DI_PT_QUADSTRIP => Some(2),
        DI_PT_RECTLIST | DI_PT_2D_RECTANGLE => Some(3),
        _ => None,
    }
}

/// `DI_PT_TRILIST` and `DI_PT_RECTLIST` (`gfx103.json:691`, `:704`, enum `VGT_DI_PRIM_TYPE`): the
/// input primitives of three vertices each, the second radeonsi's blits.
const DI_PT_TRILIST: u32 = 4;
const DI_PT_RECTLIST: u32 = 17;
/// `DI_PT_2D_RECTANGLE` (`gfx103.json`, enum `VGT_DI_PRIM_TYPE` 7): the topology AGC's prim state
/// draws through output primitive 3, a rectangle (D755), three corners a rectangle as a
/// rectangle list's - PPSA21564's three-vertex full-screen pass is one.
const DI_PT_2D_RECTANGLE: u32 = 7;
/// `VGT_SHADER_STAGES_EN` (`gfx103.json`, dword `0xA2D5`); its `PRIMGEN_PASSTHRU_EN`, bit 25, hands
/// the primitive shader its primitive already packed in `v0` ([`GeometryInputs::passthrough`]).
const VGT_SHADER_STAGES_EN: u32 = 0xA2D5;
const PRIMGEN_PASSTHRU_EN: u32 = 1 << 25;
/// `GE_INDX_OFFSET` (`gfx103.json`, byte `198952`, uconfig dword `0xC24A`): added to every index an
/// indexed draw fetches, and to nothing an auto draw generates - measured (D843, obSCEne ix01).
const GE_INDX_OFFSET: u32 = 0xC24A;

/// The geometry-engine inputs a draw hands its primitive shader whole (D730), or why it cannot: a
/// draw over a list or strip of triangles or lines, each of its instances a workgroup that finds
/// its instance in `v8` (D863). One wave may not hold them; [`chunk_plans`] splits those that it does not.
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
    let topology = latest(VGT_PRIMITIVE_TYPE).map(|value| value & 0x3F);
    let (assembly, primitives) = match topology {
        Some(DI_PT_TRILIST | DI_PT_RECTLIST | DI_PT_2D_RECTANGLE) => (Assembly::List, vertices / 3),
        Some(DI_PT_LINELIST) => (Assembly::LineList, vertices / 2),
        Some(DI_PT_LINESTRIP) => (Assembly::LineStrip, vertices.saturating_sub(1)),
        Some(DI_PT_POINTLIST) => (Assembly::Points, vertices),
        Some(DI_PT_TRISTRIP) => (
            Assembly::Strip {
                provoking_last: latest(PA_SU_SC_MODE_CNTL)
                    .is_some_and(|value| value & PROVOKING_VTX_LAST != 0),
            },
            vertices.saturating_sub(2),
        ),
        // Its whole quads, two triangles each, as Mesa splits them (D798).
        Some(DI_PT_QUADLIST) => (
            Assembly::Quads {
                provoking_last: latest(PA_SU_SC_MODE_CNTL)
                    .is_some_and(|value| value & PROVOKING_VTX_LAST != 0),
            },
            vertices / 4 * 2,
        ),
        // Its whole quads, two triangles each: an odd last vertex closes none (D797).
        Some(DI_PT_QUADSTRIP) => (
            Assembly::Strip {
                provoking_last: latest(PA_SU_SC_MODE_CNTL)
                    .is_some_and(|value| value & PROVOKING_VTX_LAST != 0),
            },
            (vertices & !1).saturating_sub(2),
        ),
        _ => {
            return Err(format!(
                "the draw's input primitive {topology:?} is not a list or strip of triangles or lines, whose geometry-engine inputs are the ones seeded"
            ));
        }
    };
    let stages = latest(VGT_SHADER_STAGES_EN);
    let passthrough = stages.is_some_and(|value| value & PRIMGEN_PASSTHRU_EN != 0);
    let legacy = crate::registers::legacy_vertex(stages);
    // `SPI_SHADER_PGM_RSRC2_VS.SO_EN`, bit 12: its stores are its streamout (D881).
    let streamout = legacy && latest(0x2C4B).is_some_and(|value| value & 1 << 12 != 0);
    // The offset is added to each index an indexed draw fetches; an auto draw's ids count up from
    // zero whatever it holds (D843, obSCEne ix01).
    let offset = latest(GE_INDX_OFFSET).unwrap_or(0);
    Ok(GeometryInputs {
        first_vertex: if indices.is_some() { offset } else { 0 },
        vertices,
        primitives,
        assembly,
        indices,
        passthrough,
        legacy,
        streamout,
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
        Assembly::Points => lanes,
        // Whole quads: four vertices, two triangles each.
        Assembly::Quads { .. } => lanes / 4 * 2,
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
                // A line's two vertices, and a quad's four for its two triangles.
                Assembly::LineList | Assembly::Quads { .. } => start * 2,
                Assembly::Strip { .. } | Assembly::LineStrip | Assembly::Points => start,
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
                    // An indexed chunk's fetched indices take the draw's index offset (D843); an
                    // auto chunk's ids are its words' (D761).
                    first_vertex: if whole.indices.is_some() {
                        whole.first_vertex
                    } else {
                        0
                    },
                    vertices: whole.assembly.vertices_for(primitives),
                    primitives,
                    assembly: whole.assembly,
                    // A chunk of a draw with no indices counts its ids up from its first vertex,
                    // which its draw's words carry, so every chunk runs one module (D761).
                    indices: whole.indices.map(|_| width),
                    passthrough: whole.passthrough,
                    legacy: whole.legacy,
                    streamout: whole.streamout,
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

/// How many pairs of programs [`Pipeline::draw_flat_twins`] keeps its answer for before it starts
/// again: a title's every pair, and a bound for one streaming new programs every frame.
const FLAT_TWINS_KEPT: usize = 4096;

/// What a draw's shaders are chosen by besides its registers: whether a primitive shader is
/// prepared per geometry, and the geometry where one is, and whether the pixel stage's words count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DrawTerms {
    per_geometry: bool,
    with_words: bool,
    geometry: Option<(u32, u32, u32)>,
}

/// What a draw's shaders are chosen by in its registers, and what preparing them reads
/// ([`Pipeline::draw_key`]).
struct DrawKey {
    /// The register values the shaders are chosen by, in a fixed order.
    written: Vec<Option<u32>>,
    window_space: WindowSpace,
    vertex_words: [u32; USER_DATA_WORDS],
    fragment_words: [u32; USER_DATA_WORDS],
}

/// How many programs [`ProgramVersions`] keeps for one address.
const VERSIONS_KEPT: usize = 8;

/// The last few programs seen at each shader address, end-of-program included, the one most
/// recently found there first, each with what was worked out from its bytes. A GL layer that writes
/// each draw's program into one slot puts a different program at the same address from draw to
/// draw, and what is worked out from a program is a function of its bytes, so a program seen there
/// before is not worked out again.
#[derive(Debug)]
struct ProgramVersions<T>(BTreeMap<u64, Vec<(Vec<u8>, T)>>);

impl<T> Default for ProgramVersions<T> {
    fn default() -> Self {
        Self(BTreeMap::new())
    }
}

impl<T> ProgramVersions<T> {
    /// The program most recently found at `address`, and what was worked out from it.
    fn current(&self, address: u64) -> Option<&(Vec<u8>, T)> {
        self.0.get(&address)?.first()
    }

    /// Records `bytes`, and `value` worked out from them, as the program now at `address`.
    fn insert_with(&mut self, address: u64, bytes: Vec<u8>, value: T) {
        let versions = self.0.entry(address).or_default();
        versions.retain(|(known, _)| *known != bytes);
        versions.insert(0, (bytes, value));
        versions.truncate(VERSIONS_KEPT);
    }

    /// The bytes of a program kept for `address` that memory holds there now, and what was worked
    /// out from them, made the one [`Self::current`] answers; `None` where memory holds none of
    /// them.
    fn held_with<'m>(
        &mut self,
        address: u64,
        memory: &'m impl GuestMemory,
    ) -> Option<(&'m [u8], &T)> {
        let versions = self.0.get_mut(&address)?;
        let (at, bytes) = versions.iter().enumerate().find_map(|(at, (known, _))| {
            memory
                .read(guest_address_of(address), known.len())
                .filter(|bytes| *bytes == known.as_slice())
                .map(|bytes| (at, bytes))
        })?;
        versions[..=at].rotate_right(1);
        Some((bytes, &versions[0].1))
    }
}

/// The bytes each shader address decoded to, with their content hash.
type DecodedPrograms = ProgramVersions<u64>;

impl ProgramVersions<u64> {
    /// The program most recently found at `address`.
    fn get(&self, address: u64) -> Option<&Vec<u8>> {
        self.current(address).map(|(bytes, _)| bytes)
    }

    /// The content hash of the program most recently found at `address`.
    fn hash(&self, address: u64) -> Option<u64> {
        self.current(address).map(|&(_, hash)| hash)
    }

    /// Records `bytes` as the program now at `address`.
    fn insert(&mut self, address: u64, bytes: Vec<u8>) {
        let hash = content_hash(&bytes);
        self.insert_with(address, bytes, hash);
    }

    /// The bytes of a program kept for `address` that memory holds there now, made the one
    /// [`Self::get`] answers; `None` where memory holds none of them.
    fn held<'m>(&mut self, address: u64, memory: &'m impl GuestMemory) -> Option<&'m [u8]> {
        self.held_with(address, memory).map(|(bytes, _)| bytes)
    }
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
    /// The stream the last preparation read - by where it lies and its length - with its own
    /// register writes and its last `CLEAR_STATE`: what [`Self::carry`] of that same stream takes
    /// rather than walking and reading it again. Taken once.
    last_read: Option<LastRead>,
    /// The wave widths in force at the draw whose shaders are being bound, while one is: a
    /// submission may change them between draws.
    draw_widths: Option<WaveWidths>,
    /// The `VGT_GS_OUT_PRIM_TYPE` in force at the draw whose shaders are being bound, where the
    /// stream or the state it carries holds one: a submission may draw points, then triangles.
    draw_primitive: Option<u32>,
    /// The `VGT_GS_OUT_PRIM_TYPE` a draw holds where nothing wrote it: its reset value, zero
    /// (`POINTLIST`), in a submission that clears state (`CLEAR_STATE`); unknown in one that does
    /// not.
    unwritten_primitive: Option<u32>,
    /// The `VGT_SHADER_STAGES_EN` in force at the draw whose shaders are being bound, which says
    /// whether it runs a legacy vertex shader (D881).
    draw_stages: Option<u32>,
    /// The one the submission being prepared leaves in force, for a module prepared for no
    /// particular draw.
    submission_stages: Option<u32>,
    /// The `VGT_GS_OUT_PRIM_TYPE` the submission being prepared leaves in force - its last write, or
    /// what holds where it writes none - which a module prepared for no particular draw emits.
    submission_primitive: Option<u32>,
    /// Each stage's user-data layout for the submission being prepared, vertex then fragment.
    user_data: [UserData; 2],
    /// Shader bytes, by content hash, to the resource holding the translation.
    cache: BTreeMap<u64, Cached>,
    /// The bytes each shader address decoded to, end-of-program included.
    decoded: DecodedPrograms,
    /// Whether a primitive shader runs one invocation per lane (D760), by its bytes' content hash:
    /// the answer reads the whole decoded program, and a shader already translated is bound again
    /// at every submission without its decode.
    per_lane: BTreeMap<u64, bool>,
    /// The texels a dispatch image last detiled to, with the surface and bytes they came from.
    detiled: DetiledImages,
    /// Shaders whose translation, keyed as [`Self::cache`] is, found it needs a draw's geometry
    /// (D730), with their bytes and the reason: tried before every draw is bound, at every
    /// submission, and a whole translation each time to reach the same answer.
    needs_geometry: BTreeMap<u64, (Vec<u8>, String)>,
    /// The constant base each vertex program formed, by the address it was found at.
    bases: ProgramVersions<Option<u64>>,
    /// Each translated module's texture sources, by its resource.
    texture_sources: BTreeMap<ResourceId, Vec<TextureSource>>,
    /// The translated modules whose shaders read or write guest memory, which reach it only
    /// through the window.
    memory_readers: BTreeMap<ResourceId, String>,
    /// The translated primitive shaders that read their vertex ids from the draw's index buffer
    /// (D740), which a draw binds after their traced buffers.
    index_readers: std::collections::BTreeSet<ResourceId>,
    /// The assembly each primitive shader translated for a draw's geometry takes its primitives
    /// by, from which a draw's system registers are seeded on the host (D800).
    assemblies: BTreeMap<ResourceId, Assembly>,
    /// The colour channels each translated pixel shader exports, by its resource.
    colour_channels: BTreeMap<ResourceId, [u8; 8]>,
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
    dispatch_modules: BTreeMap<u64, (DispatchModule, orbistoun_translate::wavefront::ImageSources)>,
    /// Each primitive shader's traced format words, by address and user-data count, with the hash
    /// of the bytes they were traced from ([`Self::format_words`]).
    format_words: BTreeMap<(u64, u32), (u64, Option<FormatWords>)>,
    /// The formats [`Self::draw_buffer_formats`] last resolved and what it resolved them for, kept
    /// while one submission's draws are bound ([`Self::bind_shaders`]). A draw split
    /// into chunks asks again for every chunk with the same program, words and counts, and the
    /// submission's memory and programs do not change while it is prepared.
    formats_memo: FormatsMemo,
    /// [`Self::draw_flat_twins`]'s answer for each pair of programs - pixel, then primitive - by
    /// their content hashes: working it out decodes both, it is a function of their bytes, and a
    /// frame draws the same pair thousands of times across its submissions.
    flat_twins_kept: BTreeMap<(Option<u64>, Option<u64>), Option<FlatTwins>>,
}

/// Whether [`Pipeline::draw_buffer_formats`] keeps what it resolves, and what it kept.
#[derive(Debug)]
enum FormatsMemo {
    /// Outside a submission's draws: every call resolves.
    Off,
    /// While a submission's draws are bound: the last resolution, if any.
    Kept(Option<Box<FormatsResolved>>),
}

/// What [`Pipeline::draw_buffer_formats`] was asked - the program, the user-data words and the
/// system scalars - and what it answered.
type FormatsResolved = (
    (u64, [u32; USER_DATA_WORDS], Vec<(u32, u32)>),
    Option<orbistoun_translate::wavefront::BufferFormats>,
);

/// A translated compute module, shared rather than copied for each dispatch - a guest's run to half
/// a megabyte - with its [`crate::content_hash`].
type DispatchModule = (std::sync::Arc<[u32]>, u64);

/// A guest compute dispatch made ready to run: its module, the window it was built against, and the
/// push-constant block carrying its user data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedDispatch {
    /// The translated module, shared with the translation kept for it.
    pub module: std::sync::Arc<[u32]>,
    /// Its [`crate::content_hash`], which the device names its pipeline by.
    pub module_hash: u64,
    /// Where the guest program it was translated from lies.
    pub program: u64,
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
    /// Whether each of them, by slot, is one the program only stores to (D809): its words before
    /// the run are not read, and what the run stored is found from its write mask.
    pub unread: Vec<bool>,
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
            last_read: None,
            draw_widths: None,
            draw_primitive: None,
            unwritten_primitive: None,
            draw_stages: None,
            submission_stages: None,
            submission_primitive: None,
            user_data: [UserData::default(); 2],
            cache: BTreeMap::new(),
            decoded: DecodedPrograms::default(),
            per_lane: BTreeMap::new(),
            detiled: DetiledImages::default(),
            needs_geometry: BTreeMap::new(),
            bases: ProgramVersions::default(),
            texture_sources: BTreeMap::new(),
            memory_readers: BTreeMap::new(),
            index_readers: std::collections::BTreeSet::new(),
            assemblies: BTreeMap::new(),
            colour_channels: BTreeMap::new(),
            texels: TexelCache::new(),
            buffer_sources: BTreeMap::new(),
            buffers: crate::draw_buffers::BufferCache::default(),
            next_resource: 1,
            store: None,
            depth_fills: crate::depth::PendingFills::default(),
            dispatch_modules: BTreeMap::new(),
            format_words: BTreeMap::new(),
            formats_memo: FormatsMemo::Off,
            flat_twins_kept: BTreeMap::new(),
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
        let (served, buffers, formats, unread, global_base) = self
            .dispatch_buffers(state, &decoded, memory)
            .map_err(|why| {
                dispatch_refusal(why, (&decoded, &self.encodings), (address, program))
            })?;
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
        let ((module, module_hash), images) = loop {
            let (module, sources) = self.dispatch_module(
                (&decoded, program, address),
                state,
                (placed, formats),
                integer_images,
            )?;
            let images = dispatch_images(state, &sources, memory, &mut self.detiled)?;
            let wanted = [
                fetched_integer(&images)?,
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
            module_hash,
            program: address,
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
                // A partial group's shape, where the module reads it (D855).
                if let Some(partial) = state.inputs.partial {
                    let at = orbistoun_translate::wavefront::PARTIAL_PUSH_WORD as usize;
                    block[at..at + 6].copy_from_slice(&partial.push_words());
                }
                // The global loads' buffer's guest base, its low half, as the module reads it
                // (D883).
                if let Some(base) = global_base {
                    block[orbistoun_translate::wavefront::GLOBAL_BASE_PUSH_WORD as usize] =
                        base as u32;
                }
                block
            },
            groups: state.groups,
            buffers,
            unread,
        })
    }

    /// The guest memory a compute dispatch can write, as (first byte, bytes), worked out from its
    /// bindings without running or translating it (D876): each buffer it binds on its own (D746)
    /// that one of its buffer stores or atomics goes through, and the window over its other buffer
    /// accesses where one of those stores through it - a buffer or window it only reads, it does
    /// not write (D882). `None` where that does not bound it: a program that does not decode or
    /// trace, or one that stores other than through a buffer - an image, a global or flat address.
    #[must_use]
    pub fn dispatch_write_spans(
        &self,
        state: &crate::dispatch::DispatchState,
        memory: &impl GuestMemory,
    ) -> Option<Vec<(u64, u64)>> {
        let window = read_window(memory, guest_address_of(state.program))?;
        let decoded = decode_program(window, &self.encodings, &self.operands);
        if !decoded.terminated || !decoded.is_trustworthy() {
            return None;
        }
        let families = self.encodings.encodings();
        // Where each buffer store or atomic is, by its offset in the program.
        let mut stores = Vec::new();
        for instruction in &decoded.instructions {
            let Some(name) = instruction
                .encoding
                .and_then(|e| families.get(usize::from(e)))
                .and_then(|family| {
                    self.encodings
                        .mnemonic_for(&family.name, instruction.opcode)
                })
            else {
                continue;
            };
            if !(name.contains("store") || name.contains("atomic")) || name.starts_with("ds_") {
                continue;
            }
            if !name.starts_with("buffer_") {
                return None;
            }
            stores.push(instruction.offset);
        }
        if stores.is_empty() {
            return Some(Vec::new());
        }
        let (served, buffers, _, _, _) = self.dispatch_buffers(state, &decoded, memory).ok()?;
        let mut spans: Vec<(u64, u64)> = Vec::new();
        let mut through_window = false;
        for at in &stores {
            match served.get(at) {
                Some(&slot) => spans.extend(buffers.get(slot as usize).copied()),
                None => through_window = true,
            }
        }
        spans.sort_unstable();
        spans.dedup();
        if through_window {
            let placed = crate::dispatch::place_window_except(
                state,
                (&decoded, &self.encodings),
                &served,
                |at, len| usize::try_from(len).is_ok_and(|len| memory.read(at, len).is_some()),
            )
            .ok()?;
            spans.extend(placed.map(|window| (window.address(), u64::from(window.words()) * 4)));
        }
        Some(spans)
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
        let mut global_base = None;
        for source in &traced.sources {
            // The global loads' buffer is what every wave of the dispatch reads (D883).
            let range =
                if let orbistoun_translate::draw_buffers::BufferSource::Global(global) = source {
                    let count = state.user_data.len().min(USER_DATA_WORDS);
                    let range = crate::draw_buffers::dispatch_global_range(
                        global,
                        (
                            &words[..count],
                            state.inputs,
                            state.groups,
                            state.width.lanes(),
                        ),
                        memory,
                    )?;
                    global_base = Some(range.0);
                    range
                } else {
                    crate::draw_buffers::resolve_range(source, (&words, 0, &[]), memory)?
                };
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
                    for (slot, source) in traced.sources.iter().enumerate() {
                        formats.set(
                            slot,
                            crate::draw_buffers::resolve_fourth_word(source, (&words, &[]), memory),
                        );
                    }
                    formats
                },
            );
        // A buffer the program only stores to is not read before it runs (D809).
        let unread = (0..ranges.len())
            .map(|slot| u32::try_from(slot).is_ok_and(|slot| traced.stored_only.contains(&slot)))
            .collect();
        Ok((traced.served, ranges, formats, unread, global_base))
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
    ) -> Result<(DispatchModule, orbistoun_translate::wavefront::ImageSources), String> {
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
            input_controls: None,
            compute: Some(inputs),
            geometry: None,
            window_space: WindowSpace::Clip,
            // The host's subgroup, which a program reading a mask whole makes its masks by where
            // it holds the wave (D884); the translation decides whether it runs per lane at all.
            per_invocation: crate::host_subgroup(),
            // Its buffers are bound on their own where traced (D746).
            draw_buffers: true,
            buffer_formats: formats,
            flat_twins: None,
            discard_vertex_exports: state.vertex_streamout,
            position_vectors: None,
            point_size: None,
            saturated: UNSATURATED,
            local_words: state.local_words,
            program_address: None,
            integer_targets: 0,
        };
        let key = content_hash(program)
            // The formats are compiled in, so a dispatch through other formats is another module.
            ^ formats.map_or(0, |formats| {
                let words: Vec<u64> = formats
                    .slots()
                    .map(|word| word.map_or(u64::MAX, u64::from))
                    .collect();
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
            // A partial group's shape is read from the push block, so only whether it has one is
            // part of the module's name: every grid a program runs over shares one module (D855).
            ^ u64::from(inputs.partial.is_some()) << 9
            ^ u64::from(integer_images[0]) << 62
            ^ u64::from(integer_images[1]) << 63
            // A streamout pass drops the exports a dispatch of the same program would refuse.
            ^ u64::from(state.vertex_streamout) << 61
            // A streamout wave's first vertex is compiled in (D861).
            ^ u64::from(inputs.first_thread).rotate_left(36)
            ^ local_words_salt(state.local_words)
            ^ per_lane_salt(user_data.per_invocation);
        Ok(if let Some(cached) = self.dispatch_modules.get(&key) {
            cached.clone()
        } else {
            // The cheapest model the program runs exactly in: one invocation per lane where it
            // never meets its wave (D880) or meets it only through masks its lanes can exchange
            // (D884), else the whole wave in one.
            let strategy = Strategy::Predicated {
                fidelity: orbistoun_translate::Fidelity::Auto,
                width: state.width,
            };
            let context = crate::translations::Context {
                strategy,
                stage: Stage::Compute,
                primitive: MeshPrimitive::default(),
                window: placed,
                user_data,
            };
            let kept = self.kept_dispatch(key, (decoded, program, address), &context)?;
            let sources = (kept.textures, kept.storage);
            let hash = crate::content_hash(&kept.module);
            let module = (std::sync::Arc::from(kept.module), hash);
            self.dispatch_modules
                .insert(key, (module.clone(), sources.clone()));
            (module, sources)
        })
    }

    /// A dispatch's program translated against `context`, from the store where an earlier run kept
    /// the module this one would make (D113): one made against the same context, since one asked
    /// for another fidelity is another module (D880). A new translation is kept.
    fn kept_dispatch(
        &mut self,
        key: u64,
        (decoded, program, address): (&orbistoun_shader::Decode, &[u8], u64),
        context: &crate::translations::Context,
    ) -> Result<crate::translations::Kept, String> {
        if let Some(kept) = self
            .store
            .as_ref()
            .and_then(|store| store.get(key, program))
            .filter(|kept| kept.context == *context)
        {
            return Ok(kept.clone());
        }
        let translated = translate_with_user_data(
            decoded,
            &self.encodings,
            context.strategy,
            (context.stage, context.primitive),
            context.window,
            context.user_data,
        )
        .map_err(|e| compute_refusal(&e, (decoded, &self.encodings), (address, program)))?;
        let kept = crate::translations::Kept {
            bytes: program.to_vec(),
            context: *context,
            module: translated.module,
            textures: translated.textures,
            storage: translated.storage,
            warnings: translated
                .warnings
                .iter()
                .map(ToString::to_string)
                .collect(),
        };
        if let Some(store) = self.store.as_mut() {
            store.insert(key, kept.clone());
        }
        Ok(kept)
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
        self.writes_over(&self.carried, walked, stream, tables)
    }

    /// [`Self::writes_in_force`] as `submission` was prepared: over the state earlier submissions
    /// had left then ([`Submission::carried_in`]), not what has been carried since.
    pub fn writes_as_submitted(
        &self,
        submission: &Submission,
        walked: &PacketWalk,
        stream: &[u8],
        tables: Option<&dyn GuestMemory>,
    ) -> (Vec<RegisterWrite>, usize) {
        self.writes_over(&submission.carried_in, walked, stream, tables)
    }

    /// The register writes `stream` runs under over `carried`, and how many are its own.
    fn writes_over(
        &self,
        carried: &BTreeMap<u32, u32>,
        walked: &PacketWalk,
        stream: &[u8],
        tables: Option<&dyn GuestMemory>,
    ) -> (Vec<RegisterWrite>, usize) {
        let own = register_writes_reading(walked, stream, &self.vocabulary, tables);
        let count = own.len();
        if !self.carries_state || last_clear(walked).is_some() {
            return (own, count);
        }
        let carried = carried.iter().map(|(&register, &value)| RegisterWrite {
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
        // The writes the preparation of this very stream read, under the same tables, when it was
        // the last one prepared; otherwise read again.
        let (writes, cleared) = match self.last_read.take() {
            Some(read) if read.stream == (stream.as_ptr() as usize, stream.len()) => {
                (read.writes, read.cleared)
            }
            _ => {
                let walked = walk(stream);
                (
                    register_writes_reading(&walked, stream, &self.vocabulary, tables),
                    last_clear(&walked),
                )
            }
        };
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
        let carried = self.carried.clone();
        self.submit_over(carried, stream, queue, registered, memory)
    }

    /// [`Self::submit`] over the register state `carried_in` rather than what has been carried: a
    /// part of a submission prepared as it is carried out runs under the state the whole was
    /// prepared over ([`Submission::carried_in`]).
    pub fn submit_over(
        &mut self,
        carried_in: BTreeMap<u32, u32>,
        stream: &[u8],
        queue: Queue,
        registered: &[RegisteredShader],
        memory: &impl GuestMemory,
    ) -> Submission {
        use crate::perf::{Span, span};
        let walked = span(Span::PrepareWalk, || walk(stream));
        // The tables its indirect loads name, read once, now: what it carries forward and its
        // draw segments read them from, whatever the guest writes there after.
        let tables = crate::registers::TableSnapshot::of(&walked, stream, memory);
        let (writes, own_writes) = span(Span::PrepareRegisters, || {
            self.writes_over(&carried_in, &walked, stream, Some(&tables.over(memory)))
        });
        if self.carries_state {
            self.last_read = Some(LastRead {
                stream: (stream.as_ptr() as usize, stream.len()),
                writes: writes[writes.len() - own_writes..].to_vec(),
                cleared: last_clear(&walked),
            });
        }
        let inferred = shader_candidates(&writes, &self.vocabulary);

        let mut submission = Submission {
            report: SubmissionReport {
                packets: walked.packets.len(),
                register_writes: own_writes,
                ..SubmissionReport::default()
            },
            tables,
            carried_in,
            ..Submission::default()
        };

        // The colour targets first, because a draw reads them. Decoding their size registers lets
        // a backend allocate real attachments. The `targets` map is keyed by extent (D702); the
        // first's decoded base rides on `colour_target` below. A depth target is sized to it.
        let (depth_target, mut buffers) = set_render_targets(&mut submission, &writes);
        let first = crate::colour_slots::colour_buffer_writes(&writes, buffers[0]);

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
        record_colour_target(&mut submission, &first, &draws);
        record_compute_work(&mut submission, &writes, (&walked, stream), &draws);
        submission.border_table = crate::registers::border_table_at(&writes);
        submission.depth_control = depth_control_at(&writes);
        submission.stencil_control = stencil_control_at(&writes);
        submission.blend_control = blend_control_at(&first);
        submission.report.primitive_topology = self.note_primitives(&writes, &walked);
        // Each stage's wave width: the encodings are identical at either width, so the stream's
        // registers are the only source (D145).
        submission.report.wave_widths = crate::registers::wave_widths_at(&writes);

        // The guest-memory window this frame's shaders read, read once because every module shares
        // it (D703). The default window at address zero is unmapped and reads empty.
        // The vertex program a draw runs: the primitive shader's, or a legacy vertex shader's
        // (D881).
        let legacy = crate::registers::legacy_vertex(self.submission_stages);
        let inferred: Vec<_> = inferred
            .into_iter()
            .filter(|candidate| runs_vertex_candidate(&candidate.stage, legacy))
            .collect();
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
        self.count_export_refusals(&mut submission, &per_draw, (&draws, &writes, &mut buffers));
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
                (&writes, &buffers),
                (&per_draw, &chunks, depth_target.map(|(id, _, _)| id)),
                &mut submission.report,
            );
        });
        span(Span::PrepareTextures, || {
            self.bind_resources(&mut submission, memory);
        });
        // Every chunk a draw is carried out as, as D741 counted them (D787).
        submission.report.draws = submission
            .commands
            .iter()
            .map(|command| match command {
                RenderCommand::Draw { .. } | RenderCommand::DrawIndexed { .. } => 1,
                RenderCommand::DrawChunks { chunks, .. } => {
                    usize::try_from(*chunks).unwrap_or(usize::MAX)
                }
                _ => 0,
            })
            .sum();

        submission
    }
}

/// A preparation's own reading of its stream, kept for [`Pipeline::carry`] of the same stream.
#[derive(Debug, Clone)]
struct LastRead {
    /// Where the stream lay and how long it was: the same slice, handed on from the submit.
    stream: (usize, usize),
    writes: Vec<RegisterWrite>,
    cleared: Option<u32>,
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
    /// The fetched image: the program's first texture slot.
    pub fetched: Option<DispatchTexels>,
    /// Where the fetched image was read: its first byte and the bytes it spans.
    pub fetched_span: Option<(u64, u64)>,
    /// The images its further texture slots fetch from, in slot order, each with where it was
    /// read.
    pub more_fetched: Vec<(DispatchTexels, (u64, u64))>,
    /// The stored image, as it was before the dispatch.
    pub stored: Option<DispatchTexels>,
    /// Where each channel of the stored image lies in memory, where its descriptor reorders them:
    /// its texels, before and after, are in the shader's order ([`to_memory_channels`]).
    pub stored_order: Option<[u8; 4]>,
    /// Where the stored image lies, and the bytes it spanned before the dispatch.
    pub stored_surface: Option<(DispatchSurface, Vec<u8>)>,
    /// The stored image's compression keys, when they were all cleared: it was read as the zeros
    /// they mean, and once every texel is written back they are marked uncompressed - expanded, a
    /// state the hardware's own writes leave.
    pub stored_keys_to_expand: Option<KeysToExpand>,
}

impl DispatchImages {
    /// The guest memory its images were read from, as (first byte, bytes): the fetched image's and
    /// the stored one's.
    pub fn read_spans(&self) -> impl Iterator<Item = (u64, u64)> + '_ {
        self.fetched_span
            .into_iter()
            .chain(self.more_fetched.iter().map(|(_, span)| *span))
            .chain(
                self.stored_surface
                    .as_ref()
                    .map(|(surface, _)| (surface.base(), surface.bytes() as u64)),
            )
    }
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
    /// One eight-bit unsigned integer channel, a texel a byte: the guest's `8_UINT`.
    R8Uint,
    /// Four thirty-two-bit unsigned channels, a texel four words.
    Rgba32Uint,
    /// One thirty-two-bit unsigned channel, a texel a word: the guest's `32_UINT`.
    R32Uint,
    /// One thirty-two-bit float channel, a texel a word: the guest's `32_FLOAT`.
    R32Float,
    /// Three packed unsigned floats, eleven, eleven and ten bits, a texel a word (D774).
    Float11_11_10,
    /// Four eight-bit channels, the colour ones sRGB-encoded, a texel a word.
    Rgba8Srgb,
    /// Red, green and blue in ten normalised bits each from the low end, alpha in the top two, a
    /// texel a word: the guest's `2_10_10_10_UNORM`.
    Rgb10A2,
    /// Four sixteen-bit floats, a texel two words: the guest's `16_16_16_16_FLOAT`.
    Rgba16Float,
}

impl TexelFormat {
    /// Bytes one texel takes.
    #[must_use]
    pub const fn bytes(self) -> u32 {
        match self {
            Self::Rgba8
            | Self::Float11_11_10
            | Self::Rgba8Srgb
            | Self::Rgb10A2
            | Self::R32Uint
            | Self::R32Float => 4,
            Self::R8 | Self::R8Uint => 1,
            Self::Rgba16Float => 8,
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
        matches!(self, Self::Rgba32Uint | Self::R32Uint | Self::R8Uint)
    }
}

/// Where a dispatch image lies in guest memory: a tiled level, placed as a colour target of that
/// shape would be, a tiled volume's slices, or a linear image's rows at their pitch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispatchSurface {
    /// A tiled level, at four bytes a texel or - in the one-byte layout - one.
    Tiled(ColourTarget),
    /// A tiled volume (D823): `depth` slices, each laid out as `slice` is and a slice's surface
    /// after the one before, with the slice's XOR on its in-block offsets
    /// ([`crate::tiling::slice_xor_64kb_rx_bpp4`]). Only a 32-bpp `64KB_R_X` volume's slice terms
    /// are read.
    Volume {
        /// The first slice, placed as a 2D level of the volume's width and height.
        slice: ColourTarget,
        /// Slices.
        depth: u32,
    },
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
            Self::Tiled(target) | Self::Volume { slice: target, .. } => target.base,
            Self::Linear { base, .. } => *base,
        }
    }

    /// Its extent in texels.
    #[must_use]
    pub const fn extent(&self) -> (u32, u32) {
        match self {
            Self::Tiled(target) | Self::Volume { slice: target, .. } => {
                (target.width, target.height)
            }
            Self::Linear { width, height, .. } => (*width, *height),
        }
    }

    /// Its slices: a volume's, and one for a level.
    #[must_use]
    pub const fn depth(&self) -> u32 {
        match self {
            Self::Volume { depth, .. } => *depth,
            Self::Tiled(_) | Self::Linear { .. } => 1,
        }
    }

    /// Texels it holds, every slice's.
    #[must_use]
    pub const fn texels(&self) -> usize {
        let (width, height) = self.extent();
        width as usize * height as usize * self.depth() as usize
    }

    /// Bytes it spans from [`Self::base`]: a tiled level's whole surface; a linear one's rows at
    /// their pitch, the last ending at its width.
    #[must_use]
    pub fn bytes(&self) -> usize {
        match self {
            Self::Tiled(target) => target.words() * 4,
            Self::Volume { slice, depth } => slice.words() * 4 * *depth as usize,
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
            Self::Volume { slice, .. } => spanned
                .chunks_exact(slice.words() * 4)
                .zip(0_u32..)
                .flat_map(|(bytes, z)| {
                    let words = slice_words(&words_of_bytes(bytes), z);
                    slice.detile_mapped(&words, |w| w)
                })
                .collect(),
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
            Self::Tiled(target) | Self::Volume { slice: target, .. } => {
                (1_usize << target.layout.texel_log2()).div_ceil(4)
            }
        };
        if texels.len() != self.texels() * words || spanned.len() < self.bytes() {
            return Err("a dispatch image's texels do not match its extent".to_owned());
        }
        match self {
            Self::Volume { slice, .. } => {
                let per_slice = width as usize * height as usize * words;
                for ((bytes, texels), z) in spanned[..self.bytes()]
                    .chunks_exact_mut(slice.words() * 4)
                    .zip(texels.chunks(per_slice))
                    .zip(0_u32..)
                {
                    let mut words = slice_words(&words_of_bytes(bytes), z);
                    slice
                        .tile_mapped(texels, &mut words, |w| w)
                        .map_err(|e| format!("{e:?}"))?;
                    // The slice's XOR undoes itself.
                    for (bytes, word) in bytes.chunks_exact_mut(4).zip(slice_words(&words, z)) {
                        bytes.copy_from_slice(&word.to_le_bytes());
                    }
                }
                Ok(())
            }
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

    /// For each texel, row-major, the word of the bytes it spans that holds it - where every texel
    /// is four bytes, a word of its own. Worked out once a surface, by tiling each texel's own
    /// index, and kept: a dispatch that changes a few texels of a stored image writes those back
    /// through it rather than tiling every texel again to find them.
    #[must_use]
    pub fn texel_slots(&self) -> Option<std::sync::Arc<[u32]>> {
        use std::sync::{Arc, Mutex, PoisonError};
        static KEPT: Mutex<Vec<(DispatchSurface, Arc<[u32]>)>> = Mutex::new(Vec::new());
        let four_bytes = match self {
            Self::Tiled(target) | Self::Volume { slice: target, .. } => {
                target.layout.texel_log2() == 2
            }
            Self::Linear {
                bytes_per_texel, ..
            } => *bytes_per_texel == 4,
        };
        if !four_bytes {
            return None;
        }
        let mut kept = KEPT.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some((_, slots)) = kept.iter().find(|(surface, _)| surface == self) {
            return Some(Arc::clone(slots));
        }
        let texels = u32::try_from(self.texels()).ok()?;
        // No texel's index is all ones, so a word left so holds none.
        let indices: Vec<u32> = (0..texels).collect();
        let mut spanned = vec![0xFF; self.bytes()];
        self.tile(&indices, &mut spanned).ok()?;
        let mut slots = vec![u32::MAX; texels as usize];
        for (word, index) in words_of_bytes(&spanned).into_iter().enumerate() {
            if index != u32::MAX {
                *slots.get_mut(index as usize)? = u32::try_from(word).ok()?;
            }
        }
        // Every texel placed, each in a word of its own.
        if slots.contains(&u32::MAX) {
            return None;
        }
        let slots: Arc<[u32]> = slots.into();
        if kept.len() >= TEXEL_SLOTS_KEPT {
            kept.remove(0);
        }
        kept.push((*self, Arc::clone(&slots)));
        Some(slots)
    }
}

/// How many surfaces' texel slots are kept.
const TEXEL_SLOTS_KEPT: usize = 4;

/// Little-endian words from bytes, a trailing partial word dropped.
fn words_of_bytes(bytes: &[u8]) -> Vec<u32> {
    bytes
        .chunks_exact(4)
        .map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]))
        .collect()
}

/// Slice `z` of a 32-bpp `64KB_R_X` volume with its slice XOR undone, or put back: word `i` of the
/// one is word `i ^ (xor / 4)` of the other. The XOR is under a block, so no word leaves its block.
fn slice_words(words: &[u32], z: u32) -> Vec<u32> {
    let xor = crate::tiling::slice_xor_64kb_rx_bpp4(z).unwrap_or(0) / 4;
    (0..words.len()).map(|i| words[i ^ xor]).collect()
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
    /// A volume's slices, its texels slice after slice (D823); `None` for a 2D image.
    pub depth: Option<u32>,
}

/// A volume's slices, for its texels; `None` for a 2D surface.
const fn volume_depth(surface: &DispatchSurface) -> Option<u32> {
    match surface {
        DispatchSurface::Volume { depth, .. } => Some(*depth),
        DispatchSurface::Tiled(_) | DispatchSurface::Linear { .. } => None,
    }
}

/// Reads a dispatch's images from guest memory, by where its module said each descriptor came
/// from: its user data, or a table its user data and constants address.
///
/// # Errors
///
/// Each refusal by name: more fetched images than a stage binds, a descriptor not readable or not
/// written, an image that is not a single 2D `8_8_8_8_UNORM` level in a modelled layout, or
/// compression whose keys say anything but what a plain read or write keeps exact.
fn dispatch_images(
    state: &crate::dispatch::DispatchState,
    (textures, storage): &orbistoun_translate::wavefront::ImageSources,
    memory: &impl GuestMemory,
    detiled: &mut DetiledImages,
) -> Result<DispatchImages, String> {
    if textures.len() > MAX_TEXTURES {
        return Err(format!(
            "the program fetches from {} images, more than the {MAX_TEXTURES} a stage binds",
            textures.len()
        ));
    }
    let mut images = DispatchImages::default();
    // Each texture slot's image, bound at its slot's binding as a draw's are (D690).
    for (slot, source) in textures.iter().enumerate() {
        let (texels, span) = fetched_image(state, source, memory, detiled)?;
        if slot == 0 {
            images.fetched = Some(texels);
            images.fetched_span = Some(span);
        } else {
            images.more_fetched.push((texels, span));
        }
    }
    if let Some(source) = storage {
        let (surface, descriptor, format, order) = dispatch_image_surface(state, source, memory)?;
        let spanned = read_spanned(&surface, memory)?;
        let (width, height) = surface.extent();
        images.stored_order = order;
        let texels = match surface_keys(&descriptor, format, memory)? {
            crate::dcc::Keys::Uncompressed => {
                let texels = detiled.detile(&surface, &spanned);
                match order {
                    Some(order) => texels
                        .into_iter()
                        .map(|texel| to_shader_channels(texel, order))
                        .collect(),
                    None => texels,
                }
            }
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
                vec![0; surface.texels() * format.words() as usize]
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
            depth: volume_depth(&surface),
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

/// One fetched image's texels, read through `source`'s descriptor, and where they were read.
fn fetched_image(
    state: &crate::dispatch::DispatchState,
    source: &TextureSource,
    memory: &impl GuestMemory,
    detiled: &mut DetiledImages,
) -> Result<(DispatchTexels, (u64, u64)), String> {
    let (surface, descriptor, format, order) = dispatch_image_surface(state, source, memory)?;
    let (width, height) = surface.extent();
    let texels = match surface_keys(&descriptor, format, memory)? {
        crate::dcc::Keys::Uncompressed => {
            let texels = detiled.detile(&surface, &read_spanned(&surface, memory)?);
            match order {
                Some(order) => texels
                    .into_iter()
                    .map(|texel| to_shader_channels(texel, order))
                    .collect(),
                None => texels,
            }
        }
        crate::dcc::Keys::Clear0000 => vec![0; surface.texels() * format.words() as usize],
        crate::dcc::Keys::Other => {
            return Err(concat!(
                "the fetched image's compression keys need per-block addressing, which is ",
                "not modelled"
            )
            .to_owned());
        }
    };
    Ok((
        DispatchTexels {
            texels,
            format,
            width,
            height,
            depth: volume_depth(&surface),
        },
        (surface.base(), surface.bytes() as u64),
    ))
}

/// Whether a dispatch's fetched images hold integers, which its module declares for all of them at
/// once: refused where some do and some do not.
fn fetched_integer(images: &DispatchImages) -> Result<bool, String> {
    let mut each = images
        .fetched
        .iter()
        .chain(images.more_fetched.iter().map(|(texels, _)| texels))
        .map(|texels| texels.format.is_integer());
    let first = each.next().unwrap_or(false);
    if each.any(|integer| integer != first) {
        return Err(
            "the program fetches from integer and float images, which one module declares alike"
                .to_owned(),
        );
    }
    Ok(first)
}

/// The texels dispatch images last detiled to, each with the surface and the bytes it came from: a
/// detile is a function of the two, and MDEM00001's dispatches read the same images every frame -
/// comparing their bytes costs a fraction of swizzling every texel again.
#[derive(Debug, Default)]
struct DetiledImages(Vec<(DispatchSurface, Vec<u8>, Vec<u32>)>);

/// How many detiled images are kept: a dispatch binds two, and a frame runs a few.
const DETILED_KEPT: usize = 8;

impl DetiledImages {
    /// `spanned`, `surface`'s bytes, detiled: the texels kept for them where the surface and the
    /// bytes are the ones kept, else detiled now and kept.
    fn detile(&mut self, surface: &DispatchSurface, spanned: &[u8]) -> Vec<u32> {
        if let Some(at) = self
            .0
            .iter()
            .position(|(kept, bytes, _)| kept == surface && bytes.as_slice() == spanned)
        {
            // The most recent last, so the oldest goes first.
            let entry = self.0.remove(at);
            let texels = entry.2.clone();
            self.0.push(entry);
            return texels;
        }
        let texels = surface.detile(spanned);
        if self.0.len() >= DETILED_KEPT {
            self.0.remove(0);
        }
        self.0.push((*surface, spanned.to_vec(), texels.clone()));
        texels
    }
}

/// A dispatch image as its descriptor places it: where it lies, the descriptor, its texel format,
/// and its channels' order where the descriptor reorders them ([`channel_order`]).
type DispatchImageSurface = (
    DispatchSurface,
    ImageDescriptor,
    TexelFormat,
    Option<[u8; 4]>,
);

/// The image a dispatch source names: its descriptor read from the user data or its table,
/// decoded, and placed.
fn dispatch_image_surface(
    state: &crate::dispatch::DispatchState,
    source: &TextureSource,
    memory: &impl GuestMemory,
) -> Result<DispatchImageSurface, String> {
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
    let format = dispatch_format(&descriptor, words)?;
    // A volume where the program addresses one, three coordinates to its texel (D823); a 2D image
    // otherwise.
    let volume = words[3] >> 28 == IMAGE_TYPE_3D;
    if (words[3] >> 28 != IMAGE_TYPE_2D && !volume) || volume != source.volume {
        return Err(format!(
            "a dispatch image that is not the shape its program addresses ({}): format {}",
            if source.volume { "3D" } else { "2D" },
            describe_dispatch_image(&descriptor, words)
        ));
    }
    // A ten-bit texel's channels are read in their own order: selects that move them are refused
    // rather than read as they are.
    if matches!(format, TexelFormat::Rgb10A2 | TexelFormat::Rgba16Float)
        && destination_selects(words[3]) != IDENTITY_SELECTS
    {
        return Err(format!(
            "a dispatch image whose selects reorder its channels: format {}",
            describe_dispatch_image(&descriptor, words)
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
        return Ok((surface, descriptor, format, channel_order(format, words[3])));
    }
    let surface = tiled_surface(&descriptor, words, format, volume).ok_or_else(|| {
        format!(
            "a dispatch image whose layout is not modelled: format {}",
            describe_dispatch_image(&descriptor, words)
        )
    })?;
    Ok((surface, descriptor, format, channel_order(format, words[3])))
}

/// Where a tiled image's viewed level lies, as its chain places it in its layout: a 2D level, or
/// a `volume`'s slices, each such a level of its width and height (D823). `None` where the layout
/// is not modelled - a volume's slices only where their terms were read: one level of 32-bpp
/// `64KB_R_X`, no deeper than its slice terms go.
fn tiled_surface(
    descriptor: &ImageDescriptor,
    words: [u32; 8],
    format: TexelFormat,
    volume: bool,
) -> Option<DispatchSurface> {
    let surface = surface_layout(descriptor, format)
        .and_then(|layout| {
            crate::registers::chain_level(
                (descriptor.base, descriptor.pipe_bank_xor),
                (descriptor.width, descriptor.height),
                (descriptor.levels, descriptor.base_level),
                layout,
            )
        })
        .filter(|surface| surface.layout.models(surface.pipe_bank_xor))?;
    if !volume {
        return Some(DispatchSurface::Tiled(surface));
    }
    let last_slice = words[4] & DEPTH_FIELD;
    (descriptor.tiling == SwizzleMode::Tiled64KbRX
        && format.bytes() == 4
        && descriptor.levels == 1
        && crate::tiling::slice_xor_64kb_rx_bpp4(last_slice).is_some()
        && surface.pipe_bank_xor == 0)
        .then_some(DispatchSurface::Volume {
            slice: surface,
            depth: last_slice + 1,
        })
}

/// Where each channel a dispatch reads or writes lies in an `8_8_8_8_UNORM` texel in memory: byte
/// `order[i]` holds the shader's channel `i`. Taken from the descriptor's destination selects
/// (`SQ_IMG_RSRC_WORD3`), which radeonsi fills from the view format's own swizzle - a `B8G8R8A8`
/// view of a colour target selects `Z, Y, X, W` (`si_make_texture_descriptor`) - and which a store
/// honours as a load does, or radeonsi's copy from a BGRA target into an RGBA readback image would
/// swap red and blue on hardware. [`None`] for the selects as they are, and for anything other than
/// a reordering of the four channels, which is read as before.
fn channel_order(format: TexelFormat, word3: u32) -> Option<[u8; 4]> {
    let selects = destination_selects(word3);
    if format != TexelFormat::Rgba8 || selects == IDENTITY_SELECTS {
        return None;
    }
    let mut order = [0_u8; 4];
    let mut seen = 0_u32;
    for (slot, select) in order.iter_mut().zip(selects) {
        let byte = select.checked_sub(SELECT_X).filter(|byte| *byte < 4)?;
        seen |= 1 << byte;
        *slot = u8::try_from(byte).ok()?;
    }
    (seen == 0xF).then_some(order)
}

/// Whether a one-channel image's selects return what the host's one-channel view does: X, then
/// zero, zero and one - spelt as the constants, or as the channels the image does not have, which
/// read as those constants.
fn one_channel_selects(word3: u32) -> bool {
    let [x, y, z, w] = destination_selects(word3);
    x == SELECT_X && matches!(y, 0 | 5) && matches!(z, 0 | 6) && matches!(w, 1 | 7)
}

/// `texel` as a shader sees it through `order`: channel `i` from byte `order[i]`.
fn to_shader_channels(texel: u32, order: [u8; 4]) -> u32 {
    let bytes = texel.to_le_bytes();
    u32::from_le_bytes(order.map(|byte| bytes[usize::from(byte)]))
}

/// A texel a shader wrote through `order` as memory holds it: channel `i` into byte `order[i]`.
#[must_use]
pub fn to_memory_channels(texel: u32, order: [u8; 4]) -> u32 {
    let channels = texel.to_le_bytes();
    let mut bytes = [0_u8; 4];
    for (channel, byte) in channels.into_iter().zip(order) {
        bytes[usize::from(byte)] = channel;
    }
    u32::from_le_bytes(bytes)
}

/// A dispatch image's descriptor as a refusal quotes it: its format, then its layout, extent and
/// depth, levels and type, so the image a dispatch needs is read rather than guessed - `9
/// (Tiled64KbRX, 64x64x32, levels 0..=0, type 10)`.
fn describe_dispatch_image(descriptor: &ImageDescriptor, words: [u32; 8]) -> String {
    format!(
        "{} ({:?}, {}x{}x{}, levels {}..={}, type {})",
        descriptor.format,
        descriptor.tiling,
        descriptor.width,
        descriptor.height,
        (words[4] & DEPTH_FIELD) + 1,
        descriptor.base_level,
        descriptor.last_level,
        words[3] >> 28
    )
}

/// The texel format a dispatch image's descriptor, read from `words`, names, where it is one bound
/// here.
fn dispatch_format(descriptor: &ImageDescriptor, words: [u32; 8]) -> Result<TexelFormat, String> {
    match descriptor.format {
        FORMAT_8_8_8_8_UNORM => Ok(TexelFormat::Rgba8),
        FORMAT_8_UNORM => Ok(TexelFormat::R8),
        FORMAT_2_10_10_10_UNORM => Ok(TexelFormat::Rgb10A2),
        FORMAT_16_16_16_16_FLOAT => Ok(TexelFormat::Rgba16Float),
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
        // One channel, read with the constants a one-channel image answers for the others - which
        // the host's one-channel view answers too - or written, where the selects say nothing.
        FORMAT_32_UINT if one_channel_selects(words[3]) => Ok(TexelFormat::R32Uint),
        FORMAT_8_UINT if one_channel_selects(words[3]) => Ok(TexelFormat::R8Uint),
        FORMAT_32_FLOAT if one_channel_selects(words[3]) => Ok(TexelFormat::R32Float),
        _ => Err(format!(
            "a dispatch image of format {}",
            describe_dispatch_image(descriptor, words)
        )),
    }
}

#[cfg(test)]
mod dispatch_format_tests {
    /// A linear `8_UINT` image read through one channel is a one-byte unsigned integer image: what
    /// Grand Theft Auto V's compute program reads, 2048 by 4096.
    #[test]
    fn an_eight_bit_unsigned_image_is_one_byte_of_integer() {
        // Format 5 in dword 1 bits 28:20, width and height less one, `X001` selects, a 2D image.
        let words = [
            0,
            5 << 20 | (2047 & 3) << 30,
            2047 >> 2 | 4095 << 14,
            4 | 5 << 3 | 6 << 6 | 7 << 9 | 9 << 28,
            0,
            0,
            0,
            0,
        ];
        let descriptor = crate::registers::decode_image_descriptor(words);
        let format = super::dispatch_format(&descriptor, words).expect("bound");
        assert_eq!(format, super::TexelFormat::R8Uint);
        assert_eq!((format.bytes(), format.is_integer()), (1, true));
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
        by_geometry.integer_targets = integer_targets_of(writes);
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
        // frame rewrites the same addresses before each of many draws. The registers are read
        // again only once the sweep reports one of those read for the last key given another
        // value: nearly every draw of such a frame repeats the last.
        let mut previous_writes: Option<Vec<Option<u32>>> = None;
        let mut previous_terms: Option<DrawTerms> = None;
        let mut previous_shaders = Vec::new();
        let mut previous_chunks: Vec<ResourceId> = Vec::new();
        let mut sampled_through_samplers = self.samples_through_samplers();
        for draw in draws {
            let geometry =
                draw_geometry(draw, |register| sweep.latest(draw.packet_offset, register));
            // The widths in force at this draw, which its shaders were compiled for: a submission
            // may change them between draws.
            let widths = crate::registers::wave_widths_from(|register| {
                sweep.latest(draw.packet_offset, register)
            });
            self.draw_widths = Some(widths);
            // A primitive shader's threads: a 64-lane wave, or half of one under wave32.
            let lanes = 64 >> u32::from(widths.primitive_w32);
            let plans = geometry
                .as_ref()
                .map_or_else(|_| Vec::new(), |&whole| draw_chunks(draw, whole, lanes));
            let per_geometry = !by_geometry.needs.is_empty();
            let terms = DrawTerms {
                per_geometry,
                with_words: sampled_through_samplers,
                // A primitive shader prepared per geometry runs another module for other geometry,
                // so the geometry is part of what must repeat.
                geometry: geometry
                    .as_ref()
                    .ok()
                    .filter(|_| per_geometry)
                    .map(|g| (g.vertices, g.primitives, g.first_vertex)),
            };
            let same_terms = previous_terms == Some(terms);
            let key = (!same_terms || sweep.noted_changed(draw.packet_offset))
                .then(|| self.draw_key((&mut sweep, draw.packet_offset), &shader_registers, terms))
                .filter(|key| !same_terms || previous_writes.as_ref() != Some(&key.written));
            let Some(DrawKey {
                written,
                window_space,
                vertex_words,
                fragment_words,
            }) = key
            else {
                per_draw.push(Vec::clone(&previous_shaders));
                // The same modules; an indexed draw's chunks read its own indices.
                per_draw_chunks.push(chunks_of(&previous_chunks, &plans));
                continue;
            };
            let mut chunked = Vec::new();
            let mut shaders = Vec::new();
            let latest =
                sweep.before_among(draw.packet_offset, self.vocabulary.shader_register_ids());
            let draw_candidates = draw_candidates(
                shader_candidates(&latest, &self.vocabulary),
                crate::registers::legacy_vertex(self.draw_stages),
                (queue, &registered_stages),
            );
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
            previous_terms = Some(terms);
            previous_shaders = shaders;
            previous_chunks = chunked;
        }
        self.end_draws();
        (per_draw, per_draw_chunks)
    }

    /// The primitive a submission's modules emit, from `VGT_GS_OUT_PRIM_TYPE`, which `prepare`
    /// makes the mesh module's output shape - each draw's own where it is bound. A stream that
    /// clears state and never writes it leaves it at its reset value, zero - `POINTLIST` - which
    /// radeonsi relies on: it tracks the register as zero after a clear and writes it only when that
    /// changes (`ac_cmdbuf.c` 968).
    fn note_primitives(
        &mut self,
        writes: &[RegisterWrite],
        walked: &PacketWalk,
    ) -> Option<PrimitiveTopology> {
        self.unwritten_primitive = last_clear(walked).map(|_| 0);
        let last = |register: u32| {
            writes
                .iter()
                .rev()
                .find(|write| write.register == register)
                .map(|write| write.value)
        };
        self.submission_stages = last(VGT_SHADER_STAGES_EN);
        self.submission_primitive = if crate::registers::legacy_vertex(self.submission_stages) {
            legacy_output_primitive(last(VGT_PRIMITIVE_TYPE))
        } else {
            last(crate::registers::VGT_GS_OUT_PRIM_TYPE).or(self.unwritten_primitive)
        };
        self.submission_primitive
            .map(crate::registers::decode_primitive_topology)
    }

    /// Forgets what held only while one submission's draws were bound: their wave widths and the
    /// formats kept for their chunks.
    fn end_draws(&mut self) {
        self.draw_widths = None;
        self.draw_primitive = None;
        self.draw_stages = None;
        self.formats_memo = FormatsMemo::Off;
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
        let _binding = crate::perf::SpanGuard::start(crate::perf::Span::ShadersUpFront);
        // Formats resolved for one draw serve its other chunks, until this submission's draws end.
        self.formats_memo = FormatsMemo::Kept(None);
        let mut by_geometry = ByGeometry::default();
        for &candidate in candidates {
            let key = (candidate.stage as u32, candidate.address);
            let plain = self.plain_key(candidate.stage, candidate.address, WindowSpace::Clip);
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
    fn draw_flat_twins(&mut self, candidates: &[Candidate]) -> Option<FlatTwins> {
        let hash = |stage: ShaderStage| {
            candidates
                .iter()
                .find(|c| c.stage == stage)
                .and_then(|c| self.decoded.hash(c.address))
        };
        let pair = (hash(ShaderStage::Fragment), hash(ShaderStage::Vertex));
        if let Some(&twins) = self.flat_twins_kept.get(&pair) {
            return twins;
        }
        let twins = self.work_out_flat_twins(candidates);
        if self.flat_twins_kept.len() >= FLAT_TWINS_KEPT {
            self.flat_twins_kept.clear();
        }
        self.flat_twins_kept.insert(pair, twins);
        twins
    }

    /// [`Self::draw_flat_twins`] worked out from the programs.
    fn work_out_flat_twins(&self, candidates: &[Candidate]) -> Option<FlatTwins> {
        let program = |stage: ShaderStage| {
            let candidate = candidates.iter().find(|c| c.stage == stage)?;
            let bytes = self.decoded.get(candidate.address)?;
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

    /// What the shaders of the draw at `at` are chosen by in its registers - the shader addresses,
    /// the position space, the vertex stage's words where a primitive shader is prepared per
    /// geometry, the user-data counts and, where the pixel stage samples through samplers, its
    /// words - with the space and both stages' words, which preparing them reads. The registers
    /// read for the key are noted by `sweep`, and only those: the words left out of it are read
    /// unnoted, so a change to them is not a change of shaders.
    fn draw_key(
        &mut self,
        (sweep, at): (&mut crate::registers::RegisterSweep<'_>, u32),
        shader_registers: &[u32],
        terms: DrawTerms,
    ) -> DrawKey {
        sweep.record_reads();
        let mut written: Vec<Option<u32>> = shader_registers
            .iter()
            .map(|&register| sweep.latest(at, register))
            .collect();
        // A window-space draw's primitive shader writes its position differently (D731, D780).
        let window_space = window_space_at(sweep, at);
        written.push(Some(window_space as u32));
        // The primitive the draw's modules emit: one submission may draw points and triangles. In a
        // cleared stream a draw before the register's first write holds its reset value, zero.
        self.draw_primitive = sweep
            .latest(at, crate::registers::VGT_GS_OUT_PRIM_TYPE)
            .or(self.unwritten_primitive);
        // Whether the draw runs a legacy vertex shader, whose output is its input topology's
        // (D881).
        self.draw_stages = sweep.latest(at, VGT_SHADER_STAGES_EN);
        written.push(self.draw_stages);
        if crate::registers::legacy_vertex(self.draw_stages) {
            self.draw_primitive = legacy_output_primitive(sweep.latest(at, VGT_PRIMITIVE_TYPE));
        }
        written.push(self.draw_primitive);
        // The vertex stage's user data, which names the buffers whose formats a format load
        // converts by (D738).
        let vertex_words = if terms.per_geometry {
            let words = stage_user_data(sweep, at, 0);
            written.extend(words.iter().map(|&word| Some(word)));
            words
        } else {
            sweep.unnoted(|sweep| stage_user_data(sweep, at, 0))
        };
        // The pixel stage's, which name the samplers whose half-border clamps its samples
        // saturate for (D743).
        let fragment_words = self.draw_user_data((sweep, at), terms.with_words, &mut written);
        sweep.stop_recording();
        DrawKey {
            written,
            window_space,
            vertex_words,
            fragment_words,
        }
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
                held.local_words = at_draw.local_words;
                held.position_vectors = at_draw.position_vectors;
                held.point_size = at_draw.point_size;
            }
            written.extend(self.user_data.iter().map(|layout| Some(layout.count)));
            written.push(self.user_data[0].position_vectors);
            written.push(self.user_data[0].point_size);
        }
        if with_words {
            let words = stage_user_data(sweep, at, 1);
            written.extend(words.iter().map(|&word| Some(word)));
            words
        } else {
            sweep.unnoted(|sweep| stage_user_data(sweep, at, 1))
        }
    }

    /// The key a module prepared for no particular draw is held under in a submission: its stage,
    /// address and position space, the user-data count it was translated for, since a stream
    /// can give one program two counts, its further position exports' routing and default point
    /// size (D867), and the primitive its draw emits.
    fn plain_key(&self, stage: ShaderStage, address: u64, window_space: WindowSpace) -> PlainKey {
        let (count, routing) = match stage {
            ShaderStage::Vertex => (
                self.user_data[0].count,
                (
                    self.user_data[0].position_vectors,
                    self.user_data[0].point_size,
                ),
            ),
            ShaderStage::Fragment => (self.user_data[1].count, (None, None)),
            ShaderStage::Compute => (0, (None, None)),
        };
        (
            stage as u32,
            address,
            window_space,
            count,
            routing,
            self.draw_primitive.or(self.submission_primitive),
        )
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
    ) -> [[bool; 2]; MAX_TEXTURES] {
        let mut saturated = UNSATURATED;
        let Some(&fragment) = candidates.iter().find(|c| c.stage == ShaderStage::Fragment) else {
            return saturated;
        };
        let plain = self.plain_key(ShaderStage::Fragment, fragment.address, WindowSpace::Clip);
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
            WindowSpace,
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
        // Every chunk but the last has the same geometry - where each starts is its index buffer's -
        // and so the same module, which is worked out once: a world drawn in wave-sized chunks
        // looked each one up again, through its buffers' formats in guest memory (CRFT00001, 25 us
        // a draw).
        let mut last: Option<(GeometryInputs, Option<ResourceId>)> = None;
        let modules: Option<Vec<ResourceId>> = plans
            .iter()
            .map(|plan| {
                if let Some((geometry, module)) = last
                    && geometry == plan.geometry
                {
                    return module;
                }
                let module = self.bind_for_draw(
                    candidate,
                    (memory, &Ok(plan.geometry), window_space, words),
                    by_geometry,
                    submission,
                );
                last = Some((plan.geometry, module));
                module
            })
            .collect();
        *chunked = modules.unwrap_or_default();
        chunked.first().copied()
    }

    /// Binds what each draw's shaders read beside their user data, for a backend that supplies it:
    /// the textures they sample and the buffers they read through (D733).
    fn bind_resources(&mut self, submission: &mut Submission, memory: &impl GuestMemory) {
        use crate::perf::{Span, span};
        submission.report.textures = span(Span::TextureCensus, || {
            texture_census(&submission.commands, memory)
        });
        if !self.feeds_user_data {
            return;
        }
        span(Span::TextureBind, || {
            submission.report.bound_textures = bind_textures(
                &mut submission.commands,
                (&self.texture_sources, &mut self.texels),
                memory,
                (
                    &mut submission.report.unbound_textures,
                    &mut submission.report.first_unbound_texture,
                ),
            );
            span(Span::TextureBorders, || {
                read_table_borders(&mut submission.commands, submission.border_table, memory);
            });
        });
        span(Span::BufferBind, || {
            crate::draw_buffers::bind_draw_buffers(
                &mut submission.commands,
                (
                    &self.buffer_sources,
                    (&self.index_readers, &self.assemblies),
                    &mut self.buffers,
                ),
                memory,
                &mut submission.report.unbound_buffers,
            );
        });
    }

    /// The host subgroup a primitive shader runs one invocation per lane on (D760): wherever the
    /// host recorded one, for any shader but a rectangle list's and one whose lanes exchange in a
    /// way a ballot, shuffle or workgroup memory cannot stand in for. `None` for every other module.
    /// `address`, for a program that reads its own (`s_getpc_b64`): it is translated for the place
    /// it was loaded, and is another module at another (D842). `None` for any other program.
    fn own_address(
        &self,
        address: u64,
        (shader, decoded): (&[u8], Option<&orbistoun_shader::Decode>),
    ) -> Option<u64> {
        reads_own_address(shader, decoded, (&self.encodings, &self.operands)).then_some(address)
    }

    fn per_lane_subgroup(
        &mut self,
        (stage, primitive): (Stage, MeshPrimitive),
        (shader, decoded): (&[u8], Option<&orbistoun_shader::Decode>),
    ) -> Option<u32> {
        if stage != Stage::Mesh || primitive == MeshPrimitive::Rectangles {
            return None;
        }
        let subgroup = crate::host_subgroup()?;
        let hash = content_hash(shader);
        // A function of the bytes, so kept by their content hash, as the translation cache is.
        let fits = if let Some(&fits) = self.per_lane.get(&hash) {
            fits
        } else {
            let fits = decoded.map_or_else(
                || {
                    let again = decode_program(shader, &self.encodings, &self.operands);
                    orbistoun_translate::wavefront::runs_per_lane(&again, &self.encodings)
                },
                |decoded| orbistoun_translate::wavefront::runs_per_lane(decoded, &self.encodings),
            );
            self.per_lane.insert(hash, fits);
            fits
        };
        fits.then_some(subgroup)
    }

    /// Records how a translated module reaches guest memory: the buffers a draw binds for it
    /// (D733), and whether anything else it reads goes through the window. A pixel shader's
    /// exported colour channels are recorded with them, from the same decode.
    fn note_memory(
        &mut self,
        resource: ResourceId,
        (shader, decoded): (&[u8], Option<&orbistoun_shader::Decode>),
        (stage, user_data, strategy): (Stage, UserData, Strategy),
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
        let mut buffers = buffers;
        // Its global loads run lane by lane over the wave the stream's registers say (D758).
        if let Strategy::Predicated { width, .. } = strategy {
            for source in &mut buffers.sources {
                if let orbistoun_translate::draw_buffers::BufferSource::Global(global) = source {
                    global.width = width.lanes();
                }
            }
        }
        // A legacy vertex shader's draw drops its streamout stores (D881).
        let drops_stores = user_data
            .geometry
            .is_some_and(|geometry| geometry.legacy && geometry.streamout);
        if let Some(access) =
            reaches_guest_memory(decoded, &self.encodings, (&buffers.served, drops_stores))
        {
            self.memory_readers.insert(resource, access);
        }
        if stage == Stage::Fragment {
            self.colour_channels.insert(
                resource,
                orbistoun_translate::wavefront::exported_target_channels(decoded, &self.encodings),
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
        (writes, buffers): (&[RegisterWrite], &[u32]),
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
                let mut latest = |register| sweep.latest(at, register);
                // Each attachment's channels against what its target's exports write.
                buffers.iter().zip(exported).any(|(&buffer, &exported)| {
                    let mut read = |register| {
                        crate::colour_slots::read_as_buffer(&mut latest, register, buffer)
                    };
                    // Written where both the target mask and the shader mask enable a channel.
                    let mask = read(crate::registers::CB_TARGET_MASK).unwrap_or(0xF)
                        & read(crate::registers::CB_SHADER_MASK).unwrap_or(0xF);
                    let info = read(crate::registers::CB_COLOR0_INFO).unwrap_or(0);
                    crate::registers::colour_write_mask(mask, info) & !exported != 0
                })
            })
            .count()
    }

    /// Counts the draws whose pixel shader's exports do not match the colour buffers they write:
    /// [`SubmissionReport::unexported_channel_draws`] and
    /// [`SubmissionReport::unbound_export_draws`].
    fn count_export_refusals(
        &self,
        submission: &mut Submission,
        per_draw: &[DrawShaders],
        (draws, writes, buffers): (&[DrawCall], &[RegisterWrite], &mut Vec<u32>),
    ) {
        keep_exported_attachments(submission, self.exported_attachments(per_draw));
        // The buffers bound from here on: attachment `K` is the `K`th.
        let bound = bound_colour_targets(&submission.commands);
        buffers.truncate(bound.max(1));
        submission.report.unexported_channel_draws =
            self.unexported_channel_draws(per_draw, draws, (writes, buffers));
        submission.report.unbound_export_draws = self.unbound_export_draws(per_draw, bound);
    }

    /// How many colour attachments the draws' pixel shaders export to: one past the highest target
    /// any writes a channel to.
    fn exported_attachments(&self, per_draw: &[DrawShaders]) -> usize {
        per_draw
            .iter()
            .flatten()
            .filter(|(stage, _)| *stage == ShaderStage::Fragment)
            .filter_map(|(_, resource)| self.colour_channels.get(resource))
            .filter_map(|channels| channels.iter().rposition(|&c| c != 0))
            .map(|highest| highest + 1)
            .max()
            .unwrap_or(1)
    }

    /// How many draws run a pixel shader that writes channels to a colour target past the `bound`
    /// the submission binds colour buffers for: `mrtK` is the `K`th bound one.
    fn unbound_export_draws(&self, per_draw: &[DrawShaders], bound: usize) -> usize {
        per_draw
            .iter()
            .filter(|shaders| {
                shaders.iter().any(|(stage, resource)| {
                    *stage == ShaderStage::Fragment
                        && self
                            .colour_channels
                            .get(resource)
                            .is_some_and(|channels| channels.iter().skip(bound).any(|&c| c != 0))
                })
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
            WindowSpace,
            &[u32; USER_DATA_WORDS],
        ),
        by_geometry: &mut ByGeometry,
        submission: &mut Submission,
    ) -> Option<ResourceId> {
        // Only the primitive shader writes a position.
        let window_space = if candidate.stage == ShaderStage::Vertex {
            window_space
        } else {
            WindowSpace::Clip
        };
        let key = (candidate.stage as u32, candidate.address);
        // A draw that gives attributes flat twins prepares both stages for them (D742), and one
        // whose samplers clamp to half a border its pixel shader (D743).
        let saturates = candidate.stage == ShaderStage::Fragment
            && (by_geometry.saturated != UNSATURATED || by_geometry.integer_targets != 0);
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
                integer_targets: 0,
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
            .then(|| {
                let system = geometry.as_ref().map_or(Vec::new(), |inputs| {
                    orbistoun_translate::wavefront::geometry_system_scalars(
                        inputs.vertices,
                        inputs.primitives,
                    )
                    .to_vec()
                });
                self.draw_buffer_formats(candidate.address, (words, &system), memory)
            })
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
        (words, system): (&[u32; USER_DATA_WORDS], &[(u32, u32)]),
        memory: &impl GuestMemory,
    ) -> Option<orbistoun_translate::wavefront::BufferFormats> {
        if let FormatsMemo::Kept(Some(kept)) = &self.formats_memo
            && let ((at, held_words, held_system), formats) = &**kept
            && *at == address
            && held_words == words
            && held_system.as_slice() == system
        {
            return *formats;
        }
        let formats = self.format_words(address).map(|traced| {
            let mut formats = orbistoun_translate::wavefront::BufferFormats::default();
            for (slot, source) in traced.iter().enumerate() {
                formats.set(
                    slot,
                    crate::draw_buffers::resolve_fourth_word(source, (words, system), memory),
                );
            }
            formats
        });
        if let FormatsMemo::Kept(kept) = &mut self.formats_memo {
            *kept = Some(Box::new(((address, *words, system.to_vec()), formats)));
        }
        formats
    }

    /// Where each of a primitive shader's buffers' fourth descriptor word comes from, by slot, for
    /// the vertex stage's layout now; `None` for a shader that makes no format load, whose formats
    /// no draw needs. Traced once per program and layout and kept: a draw only resolves the words,
    /// and a frame draws one program thousands of times. A program rewritten in place is traced
    /// again.
    fn format_words(&mut self, address: u64) -> Option<FormatWords> {
        let shader = self.decoded.get(address)?;
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
            WindowSpace,
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
        let (saturated, integer_targets) = if candidate.stage == ShaderStage::Fragment {
            (by_geometry.saturated, by_geometry.integer_targets)
        } else {
            (UNSATURATED, 0)
        };
        let for_draw = ForDraw {
            geometry,
            window_space,
            buffer_formats,
            flat_twins,
            saturated,
            integer_targets,
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
                // The draw's own primitive where a draw is being bound and the registers hold one,
                // else the submission's - which is also what a cleared stream leaves unwritten.
                self.draw_primitive
                    .or(self.submission_primitive)
                    .map(crate::registers::decode_primitive_topology),
                // The draw's own widths where a draw is being bound, else the submission's.
                self.draw_widths.unwrap_or(submission.report.wave_widths),
                for_draw,
            ),
            memory,
        ) {
            Ok(prepared) => {
                // The address resolved, whatever happened to the shader after that.
                submission.report.addresses_resolved += 1;
                submission.report.shaders_translated += 1;
                let indexed = for_draw.geometry.is_some_and(|g| g.indices.is_some());
                let assembly = for_draw.geometry.map(|g| g.assembly);
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
                        if let Some(assembly) = assembly {
                            self.assemblies.insert(resource, assembly);
                        }
                        resource
                    }
                    Prepared::Cached { resource } => {
                        submission.report.cache_hits += 1;
                        if indexed {
                            self.index_readers.insert(resource);
                        }
                        if let Some(assembly) = assembly {
                            self.assemblies.insert(resource, assembly);
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
        if let Some(bytes) = self.decoded.held(address, memory) {
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
                integer_targets: for_draw.integer_targets,
                ..self.user_data[1]
            },
            ShaderStage::Compute => UserData::default(),
        };
        let (strategy, width_salt) = stage_strategy(self.strategy, host_stage, widths);
        let user_data = UserData {
            // A live draw binds the buffers its stages read through (D733).
            draw_buffers: self.feeds_user_data && host_stage != Stage::Compute,
            per_invocation: self.per_lane_subgroup((host_stage, primitive), (shader, decoded)),
            program_address: self.own_address(address, (shader, decoded)),
            ..user_data
        };
        // The user-data layout and the wave width are in the module too.
        let key = content_hash(shader)
            ^ stage_salt(host_stage)
            ^ primitive_salt(primitive)
            ^ window_salt(self.window)
            ^ width_salt
            ^ layout_salt(&user_data)
            ^ crate::pixel_inputs::salt(&user_data)
            ^ for_draw_salt(for_draw)
            ^ if user_data.draw_buffers {
                DRAW_BUFFERS_SALT
            } else {
                0
            }
            ^ per_lane_salt(user_data.per_invocation);
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
            let kept = self.translate_remembering(key, (address, shader, decoded), &context)?;
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
        self.note_memory(
            resource,
            (shader, decoded),
            (host_stage, user_data, strategy),
        );
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

    /// [`Self::translate_now`], answering at once for a shader already found under `key` to need a
    /// draw's geometry (D730), and remembering one found to now.
    fn translate_remembering(
        &mut self,
        key: u64,
        (address, shader, decoded): (u64, &[u8], Option<&orbistoun_shader::Decode>),
        context: &crate::translations::Context,
    ) -> Result<crate::translations::Kept, PrepareFailure> {
        if let Some((bytes, reason)) = self.needs_geometry.get(&key)
            && bytes.as_slice() == shader
        {
            return Err(PrepareFailure::NeedsGeometry(reason.clone()));
        }
        let translated = self.translate_now(address, shader, decoded, context);
        if let Err(PrepareFailure::NeedsGeometry(reason)) = &translated {
            self.needs_geometry
                .insert(key, (shader.to_vec(), reason.clone()));
        }
        translated
    }

    /// Translates a shader whose bytes are known, decoding them again when the caller has no
    /// decode.
    fn translate_now(
        &self,
        address: u64,
        shader: &[u8],
        decoded: Option<&orbistoun_shader::Decode>,
        context: &crate::translations::Context,
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
            let named = refused_instruction(&e)
                .and_then(|offset| instruction_text(decoded, &self.encodings, offset))
                .map(|text| format!(" ({text})"))
                .unwrap_or_default();
            let reason = format!("the shader at {address:#x} could not be translated: {e}{named}");
            let retried = matches!(
                e,
                orbistoun_translate::TranslateError::ReadsGeometryInputs
                    | orbistoun_translate::TranslateError::NeedsBufferFormats
                    | orbistoun_translate::TranslateError::NeedsFlatTwins
            );
            if !retried {
                keep_refused_shader(address, shader);
            }
            if retried {
                PrepareFailure::NeedsGeometry(reason)
            } else {
                PrepareFailure::Resolved(reason)
            }
        })?;
        Ok(crate::translations::Kept {
            bytes: shader.to_vec(),
            context: *context,
            module: translated.module,
            textures: translated.textures,
            storage: translated.storage,
            warnings: translated
                .warnings
                .iter()
                .map(ToString::to_string)
                .collect(),
        })
    }
}

/// The offset of the instruction a translation refused, when the refusal names one.
const fn refused_instruction(error: &orbistoun_translate::TranslateError) -> Option<u32> {
    match error {
        orbistoun_translate::TranslateError::Unsupported { offset, .. }
        | orbistoun_translate::TranslateError::NotTranslated { offset, .. }
        | orbistoun_translate::TranslateError::Untraced { offset, .. }
        | orbistoun_translate::TranslateError::Unrecognised { offset } => Some(*offset),
        _ => None,
    }
}

/// The instruction at `offset` as a report quotes it: its name, operands and first word, so a
/// refusal says what it refused without the shader's bytes in hand.
pub(crate) fn instruction_text(
    decoded: &orbistoun_shader::decode::Decode,
    encodings: &EncodingTable,
    offset: u32,
) -> Option<String> {
    let instruction = decoded.instructions.iter().find(|i| i.offset == offset)?;
    let family = encodings
        .encodings()
        .get(usize::from(instruction.encoding?))?
        .name
        .as_str();
    let name = encodings
        .mnemonic_for(family, instruction.opcode)
        .map_or_else(
            || format!("{family} opcode {}", instruction.opcode),
            str::to_owned,
        );
    let operands: Vec<String> = instruction
        .operands
        .iter()
        .map(ToString::to_string)
        .collect();
    // A two-word instruction's second word too: its modifiers or wider operands.
    let second = instruction
        .second_word
        .map(|word| format!(" {word:#010x}"))
        .unwrap_or_default();
    Some(format!(
        "`{name} {}`, word {:#010x}{second}",
        operands.join(", "),
        instruction.word
    ))
}

/// The space a draw's primitive shader writes its position in, from `PA_CL_VTE_CNTL` at the draw
/// at `at` (D731, D780).
fn window_space_at(sweep: &mut crate::registers::RegisterSweep<'_>, at: u32) -> WindowSpace {
    match crate::registers::position_space(|register| sweep.latest(at, register)) {
        crate::registers::PositionSpace::Window => WindowSpace::Divided,
        crate::registers::PositionSpace::WindowUndivided => WindowSpace::Undivided,
        _ => WindowSpace::Clip,
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
    let (named, first) = USER_DATA_REGISTERS[stage];
    let first = vertex_user_data_base(named, first, sweep.latest(at, VGT_SHADER_STAGES_EN));
    for (register, word) in (first..).zip(words.iter_mut()) {
        *word = sweep.latest(at, register).unwrap_or(0);
    }
    words
}

/// `CB_BLEND_RED`, `CB_BLEND_GREEN`, `CB_BLEND_BLUE` and `CB_BLEND_ALPHA`, the constants a
/// constant-colour blend factor reads (`gfx103.json`: bytes `0x28414`-`0x28420`, dwords
/// `0xA105`-`0xA108`, on the same scheme as `CB_BLEND0_CONTROL`'s `0xA1E0`).
const CB_BLEND_CONSTANTS: [u32; 4] = [0xA105, 0xA106, 0xA107, 0xA108];

/// The colour output state last emitted as commands, so a draw emits only what changed.
#[derive(Default)]
struct ColourOutputSent {
    /// The blend register sent, with its target's `CB_COLORn_INFO`: a target that holds integers
    /// bypasses blending (D852).
    blend: Option<(u32, u32)>,
    constants: Option<[u32; 4]>,
    rop: Option<u8>,
    mask: Option<u8>,
    more: Option<Vec<crate::ColourOutput>>,
}

/// Colour target zero's output state in force at a draw, as commands where it changed since
/// `sent`: the blend state, when the stream set one, as `SetBlend`, its raster operation, when the
/// stream set `CB_COLOR_CONTROL`, as `SetRasterOp`, and the channels the draw
/// writes, when it set a target mask, as `SetWriteMask`. A one-byte target with no target mask is
/// counted: a drawer would write every channel of the frame standing in for it. The first colour
/// attachment is `buffers`' first colour buffer; each after it is set by
/// [`push_more_colour_outputs`].
fn push_colour_output(
    commands: &mut Vec<RenderCommand>,
    mut latest: impl FnMut(u32) -> Option<u32>,
    sent: &mut ColourOutputSent,
    (unmasked_one_byte_draws, buffers): (&mut usize, &[u32]),
) {
    push_more_colour_outputs(
        commands,
        &mut latest,
        sent,
        buffers.get(1..).unwrap_or_default(),
    );
    let first = buffers.first().copied().unwrap_or(0);
    let mut latest = |register| crate::colour_slots::read_as_buffer(&mut latest, register, first);
    let info = latest(crate::registers::CB_COLOR0_INFO).unwrap_or(0);
    if let Some(value) = latest(crate::registers::CB_BLEND0_CONTROL)
        && sent.blend != Some((value, info))
    {
        commands.push(RenderCommand::SetBlend(
            crate::registers::target_blend_control(value, info),
        ));
        sent.blend = Some((value, info));
    }
    // The blend constants, once the stream has written any of them; one it has not is zero, the
    // register's value after `CLEAR_STATE`.
    let constants = CB_BLEND_CONSTANTS.map(&mut latest);
    if constants.iter().any(Option::is_some) {
        let constants = constants.map(|value| value.unwrap_or(0));
        if sent.constants != Some(constants) {
            commands.push(RenderCommand::SetBlendConstants(constants));
            sent.constants = Some(constants);
        }
    }
    if let Some(rop) = latest(crate::registers::CB_COLOR_CONTROL).map(crate::registers::raster_op)
        && sent.rop != Some(rop)
    {
        commands.push(RenderCommand::SetRasterOp(rop));
        sent.rop = Some(rop);
    }
    let info = latest(crate::registers::CB_COLOR0_INFO);
    match latest(crate::registers::CB_TARGET_MASK) {
        Some(mask) => {
            let shader = latest(crate::registers::CB_SHADER_MASK).unwrap_or(u32::MAX);
            let channels = crate::registers::colour_write_mask(mask & shader, info.unwrap_or(0));
            if sent.mask != Some(channels) {
                commands.push(RenderCommand::SetWriteMask(channels));
                sent.mask = Some(channels);
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

/// The output state of the colour attachments after the first, `buffers` in order, as one
/// `SetMoreColourOutputs` where it changed since `sent`: each buffer's blend state and the channels
/// it writes, every one its format holds where the stream set no target mask.
fn push_more_colour_outputs(
    commands: &mut Vec<RenderCommand>,
    latest: &mut impl FnMut(u32) -> Option<u32>,
    sent: &mut ColourOutputSent,
    buffers: &[u32],
) {
    if buffers.is_empty() {
        return;
    }
    let more: Vec<crate::ColourOutput> = buffers
        .iter()
        .map(|&buffer| {
            let mut read = |register| crate::colour_slots::read_as_buffer(latest, register, buffer);
            let info = read(crate::registers::CB_COLOR0_INFO).unwrap_or(0);
            crate::ColourOutput {
                blend: read(crate::registers::CB_BLEND0_CONTROL)
                    .map(|value| crate::registers::target_blend_control(value, info)),
                write_mask: crate::registers::colour_write_mask(
                    read(crate::registers::CB_TARGET_MASK).unwrap_or(0xF)
                        & read(crate::registers::CB_SHADER_MASK).unwrap_or(0xF),
                    info,
                ),
            }
        })
        .collect();
    if sent.more.as_ref() != Some(&more) {
        commands.push(RenderCommand::SetMoreColourOutputs(more.clone()));
        sent.more = Some(more);
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
    (writes, buffers): (&[RegisterWrite], &[u32]),
    (shaders, chunks, depth_target): (&[DrawShaders], &[Vec<Chunk>], Option<ResourceId>),
    report: &mut SubmissionReport,
) {
    let mut sent: [Option<[u32; USER_DATA_WORDS]>; 2] = [None, None];
    let mut layer_sent = None;
    let mut colour_sent = ColourOutputSent::default();
    let mut line_sent = None;
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
            &mut colour_sent,
            (&mut report.unmasked_one_byte_draws, buffers),
        );
        // The layer this draw renders, which its primitive shader keeps the primitives of (D859).
        push_draw_layer(commands, &mut layer_sent, |r| sweep.latest(at, r), report);
        // The line width in force at this draw, when the stream set one and it changed.
        if let Some(width) = sweep
            .latest(at, crate::registers::PA_SU_LINE_CNTL)
            .map(|value| (value & 0xFFFF) as u16)
            && line_sent != Some(width)
        {
            commands.push(RenderCommand::SetLineWidth(width));
            line_sent = Some(width);
        }
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
                crate::registers::PositionSpace::Window
                | crate::registers::PositionSpace::WindowUndivided => Some(ViewportTransform {
                    x_scale: WINDOW_SPACE_SCALE,
                    x_offset: 0.0,
                    y_scale: WINDOW_SPACE_SCALE,
                    y_offset: 0.0,
                    depth: crate::registers::DepthMapping::IDENTITY,
                }),
                crate::registers::PositionSpace::Unmodelled(_) => {
                    report.unmodelled_viewports += 1;
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
        for (slot, (stage, _)) in USER_DATA_REGISTERS.into_iter().enumerate() {
            let words = stage_user_data(&mut sweep, at, slot);
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

/// The layer a draw renders, as a `SetDrawLayer` where it changed since `sent` - the draw-data
/// word last sent, `None` before any (D859). A draw whose layers are not drawn as the hardware
/// draws them is counted in [`SubmissionReport::unmodelled_layers`], with the first reason. Every
/// draw keeps every primitive until one reads the layer.
fn push_draw_layer(
    commands: &mut Vec<RenderCommand>,
    sent: &mut Option<u32>,
    mut latest: impl FnMut(u32) -> Option<u32>,
    report: &mut SubmissionReport,
) {
    match crate::registers::draw_layer(
        latest(crate::registers::PA_CL_VS_OUT_CNTL),
        latest(crate::registers::CB_COLOR0_VIEW),
    ) {
        Ok(layer) => {
            // The word the draw data carries for no layer, which no layer is.
            let word = layer.unwrap_or(u32::MAX);
            if *sent != Some(word) {
                commands.push(RenderCommand::SetDrawLayer(layer));
                *sent = Some(word);
            }
        }
        Err(why) => {
            report.unmodelled_layers.0 += 1;
            report.unmodelled_layers.1.get_or_insert(why);
        }
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

/// A non-indexed draw's chunks as one [`RenderCommand::DrawChunks`] (D787): every chunk counts
/// from its first vertex through one module, the starts evenly spaced and every chunk but the last
/// the same size. `None` where they are not, and the chunks travel separately.
fn as_draw_chunks(chunks: &[Chunk], instances: u32) -> Option<RenderCommand> {
    let firsts: Vec<u32> = chunks
        .iter()
        .map(|chunk| match chunk.index_buffer {
            IndexBuffer::Counting { first } => Some(first),
            IndexBuffer::Guest { .. } => None,
        })
        .collect::<Option<_>>()?;
    let (first, second, last) = (chunks.first()?, chunks.get(1)?, chunks.last()?);
    let stride = firsts[1].checked_sub(firsts[0])?;
    let even = firsts.iter().enumerate().all(|(i, &at)| {
        u32::try_from(i).ok().and_then(|i| i.checked_mul(stride)) == at.checked_sub(firsts[0])
    });
    let same = chunks.iter().all(|chunk| chunk.vertex == first.vertex)
        && chunks[..chunks.len() - 1]
            .iter()
            .all(|chunk| chunk.indices == second.indices);
    (even && same).then(|| RenderCommand::DrawChunks {
        chunks: u32::try_from(chunks.len()).unwrap_or(u32::MAX),
        vertices: first.indices,
        last: last.indices,
        instances,
        first_vertex: firsts[0],
        stride,
    })
}

/// A chunked draw's chunks (D741): one [`RenderCommand::DrawChunks`] where they count from their
/// first vertices (D787), else each binding its own primitive shader and drawing its indices.
fn push_chunks(
    commands: &mut Vec<RenderCommand>,
    chunks: &[Chunk],
    instances: u32,
    bound: &mut Vec<(ShaderStage, ResourceId)>,
) {
    if let Some(draw) = as_draw_chunks(chunks, instances) {
        let shader = (ShaderStage::Vertex, chunks[0].vertex);
        if !bound.contains(&shader) {
            bound.retain(|(stage, _)| *stage != ShaderStage::Vertex);
            bound.push(shader);
            commands.push(RenderCommand::BindShader {
                stage: shader.0,
                shader: shader.1,
            });
        }
        commands.push(draw);
        return;
    }
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
        commands.push(match chunk.index_buffer {
            // Counted from its first vertex, which the draw's words carry (D761).
            IndexBuffer::Counting { first } => RenderCommand::Draw {
                vertices: chunk.indices,
                instances,
                first_vertex: first,
            },
            IndexBuffer::Guest { .. } => RenderCommand::DrawIndexed {
                indices: chunk.indices,
                instances,
                first_index: 0,
                index_buffer: Some(chunk.index_buffer),
            },
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
/// `SPI_SHADER_USER_DATA_VS_0` (`gfx103.json`, `0x2C4C`): where a legacy vertex shader's user data
/// lies (D881), from `s0`.
const USER_DATA_VS_0: u32 = 0x2C4C;

/// Where `stage`'s user data lies: `first`, or for the vertex stage of a legacy vertex draw - by
/// its `VGT_SHADER_STAGES_EN`, `stages` - [`USER_DATA_VS_0`] (D881).
fn vertex_user_data_base(stage: ShaderStage, first: u32, stages: Option<u32>) -> u32 {
    if stage == ShaderStage::Vertex && crate::registers::legacy_vertex(stages) {
        USER_DATA_VS_0
    } else {
        first
    }
}

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
    Vec<bool>,
    // Where the global loads' buffer starts, where the program has global loads (D883).
    Option<u64>,
);

/// Whether two `(address, bytes)` ranges share a byte.
fn overlaps((a, a_bytes): (u64, u64), (b, b_bytes): (u64, u64)) -> bool {
    a < b.saturating_add(b_bytes) && b < a.saturating_add(a_bytes)
}

/// `SQ_IMG_RSRC_WORD3.TYPE` for a 2D image: 9 (Mesa `ac_descriptors.c:372`, as the SDK's
/// `gl_pack_descriptors` cites it).
const IMAGE_TYPE_2D: u32 = 9;

/// `SQ_IMG_RSRC_WORD3.TYPE` for a three-dimensional image: 10 (`SQ_RSRC_IMG_3D`), as measured
/// (`desc-word-3 0xa0000fac`, obSCEne `REQ-20260920T1050Z-5d7c`).
const IMAGE_TYPE_3D: u32 = 10;

/// `SQ_IMG_RSRC_WORD3.TYPE` for a cube: 11 (`SQ_RSRC_IMG_CUBE`, `gfx6.json:588`).
const IMAGE_TYPE_CUBE: u32 = 11;

/// The faces of a cube, read one after another into one [`RenderCommand::BindTexture`].
const CUBE_FACES: u32 = 6;

/// `SQ_IMG_RSRC_WORD4.DEPTH` (bits 12:0) for one cube: its last layer, the sixth face. On this
/// generation the field is "the last accessible layer" (`ac_descriptors.c:533-537`), so a value
/// past it names an array of cubes, which is not read.
const ONE_CUBE_LAST_LAYER: u32 = CUBE_FACES - 1;

/// `GFX10_FORMAT_8_8_8_8_UNORM` (`gfx10-rsrc.json:61`).
const FORMAT_8_8_8_8_UNORM: u32 = 56;
/// `GFX10_FORMAT_8_UNORM` (`gfx10-rsrc.json:6`): one eight-bit normalised channel.
const FORMAT_8_UNORM: u32 = 1;
/// `GFX10_FORMAT_8_UINT` (`gfx10-rsrc.json`, value 5): one eight-bit unsigned integer channel.
const FORMAT_8_UINT: u32 = 5;
/// `GFX10_FORMAT_BC3_UNORM` (`gfx10-rsrc.json:127`): four-by-four texels to a sixteen-byte block.
const FORMAT_BC3_UNORM: u32 = 173;
/// `GFX10_FORMAT_BC1_SRGB` (`gfx10-rsrc.json:124`): four-by-four texels to an eight-byte block,
/// its colours sRGB-encoded.
const FORMAT_BC1_SRGB: u32 = 170;
/// `GFX10_FORMAT_BC3_SRGB` (`gfx10-rsrc.json:128`): `BC3_UNORM`'s blocks, its colours
/// sRGB-encoded.
const FORMAT_BC3_SRGB: u32 = 174;
/// `GFX10_FORMAT_BC6_UFLOAT` (`gfx10-rsrc.json:133`): four-by-four unsigned half-float texels to a
/// sixteen-byte block.
const FORMAT_BC6_UFLOAT: u32 = 179;

/// A block-compressed format read as its blocks: the bytes a block holds, the encoding the host
/// samples it in, a tag that keeps its cached blocks apart from another format's, and whether it
/// holds no alpha, which the host then samples as one.
#[derive(Clone, Copy)]
struct BlockFormat {
    bytes: u32,
    encoding: crate::backend::TextureEncoding,
    tag: u32,
    opaque: bool,
}

impl BlockFormat {
    /// Whether `selects` return what the host samples this format as: its channels as they are,
    /// and for a format with no alpha, W as the one the host answers or the constant one, as
    /// [`selects_exactly`] takes `10_11_11_FLOAT`'s.
    fn read_exactly(self, selects: [u32; 4]) -> bool {
        if self.opaque {
            matches!(selects, [4, 5, 6, 7 | 1])
        } else {
            selects == IDENTITY_SELECTS
        }
    }
}

/// The block-compressed formats read as their blocks, by `IMG_FORMAT` code.
fn block_format(format: u32) -> Option<BlockFormat> {
    match format {
        FORMAT_BC3_UNORM => Some(BlockFormat {
            bytes: 16,
            encoding: crate::backend::TextureEncoding::Bc3,
            tag: 1 << 22,
            opaque: false,
        }),
        FORMAT_BC1_SRGB => Some(BlockFormat {
            bytes: 8,
            encoding: crate::backend::TextureEncoding::Bc1Srgb,
            tag: 1 << 22 | 1 << 21,
            opaque: false,
        }),
        FORMAT_BC3_SRGB => Some(BlockFormat {
            bytes: 16,
            encoding: crate::backend::TextureEncoding::Bc3Srgb,
            tag: 1 << 22 | 1 << 23,
            opaque: false,
        }),
        FORMAT_BC6_UFLOAT => Some(BlockFormat {
            bytes: 16,
            encoding: crate::backend::TextureEncoding::Bc6hUfloat,
            tag: 1 << 22 | 1 << 20,
            opaque: true,
        }),
        _ => None,
    }
}
/// `GFX10_FORMAT_10_11_11_FLOAT` (`gfx10-rsrc.json`, value 36): three packed unsigned floats.
const FORMAT_10_11_11_FLOAT: u32 = 36;
/// `GFX10_FORMAT_8_8_8_8_SRGB` (`gfx10-rsrc.json`, value 130): `8_8_8_8_UNORM` with the colour
/// channels sRGB-encoded.
const FORMAT_8_8_8_8_SRGB: u32 = 130;
/// `GFX10_FORMAT_32_32_32_32_UINT` (`gfx10-rsrc.json:80`).
const FORMAT_32_32_32_32_UINT: u32 = 75;
/// `GFX10_FORMAT_32_UINT` (`gfx10-rsrc.json`, value 20): one thirty-two-bit unsigned channel.
const FORMAT_32_UINT: u32 = 20;
/// `GFX10_FORMAT_2_10_10_10_UNORM` (`gfx10-rsrc.json`): red, green and blue ten bits each from the
/// low end, alpha two.
const FORMAT_2_10_10_10_UNORM: u32 = 50;

/// What each texture a submission binds became, by its descriptor's address, slot, sampler address
/// and the coordinates the module saturates: read once per submission.
type TexturesRead = BTreeMap<(u64, u32, Option<u64>, [bool; 2]), Option<RenderCommand>>;

/// Inserts a [`RenderCommand::BindTexture`] after each fragment `SetUserData` whose descriptor
/// table names a texture read exactly, so the draws that follow sample the guest's own texels; and
/// answers each texture bound, by its descriptor, once.
fn bind_textures(
    commands: &mut Vec<RenderCommand>,
    (sources, texels): (&BTreeMap<ResourceId, Vec<TextureSource>>, &mut TexelCache),
    memory: &impl GuestMemory,
    (unbound, first_unbound): (&mut usize, &mut Option<String>),
) -> Vec<ImageDescriptor> {
    let mut named = NamedTextures::default();
    // Each draw's textures, where its pixel shader says they are: for every slot the bound fragment
    // module samples, the descriptor at that slot's table offset (zero for a module whose texture
    // did not come from the table). Re-emitted before a draw whenever the table or the module
    // changed.
    let mut out = Vec::with_capacity(commands.len());
    let (mut words, mut fragment, mut stale) = (None, None, false);
    let mut read: TexturesRead = BTreeMap::new();
    // And the user data's, the same way: a stream sets the same words before each of many draws,
    // and asking the host whether a large texture was written since is not free.
    let mut read_from_user: BTreeMap<UserTextureKey, Option<RenderCommand>> = BTreeMap::new();
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
            RenderCommand::Draw { .. }
            | RenderCommand::DrawIndexed { .. }
            | RenderCommand::DrawChunks { .. }
                if stale =>
            {
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
                    cube: false,
                    volume: false,
                    compare: false,
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
                        let bound = read_from_user
                            .entry(user_texture_key(source, first, user))
                            .or_insert_with(|| {
                                user_data_texture(source, first, user, memory, texels)
                            })
                            .clone()
                            .filter(|bound| shaped_for(bound, source));
                        named.note(bound.as_ref(), || user_words(user, first));
                        if bound.is_none() {
                            *unbound += 1;
                            first_unbound.get_or_insert_with(|| {
                                let why = unbound_reason(user_words(user, first), memory, texels);
                                format!(
                                    "{} - {why}",
                                    unbound_in_user_data(source.slot, first, user)
                                )
                            });
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
                    // Named once a submission, as it is first read: not again for each draw.
                    let fresh =
                        !read.contains_key(&(at, source.slot, sampler_at, source.saturated));
                    let bound = read
                        .entry((at, source.slot, sampler_at, source.saturated))
                        .or_insert_with(|| table_texture(source, (at, sampler_at), memory, texels))
                        .clone();
                    let bound = bound.filter(|bound| shaped_for(bound, source));
                    named.note_at(bound.as_ref().filter(|_| fresh), at, memory);
                    // A texture the module samples and nothing binds draws the backend's
                    // placeholder, which is not the guest's picture: counted, so the draw is
                    // refused rather than drawn wrong.
                    if bound.is_none() {
                        *unbound += 1;
                        first_unbound.get_or_insert_with(|| {
                            let why = unbound_reason(read_descriptor(at, memory), memory, texels);
                            format!("{} - {why}", unbound_in_table(source.slot, at, memory))
                        });
                    }
                    out.extend(bound);
                }
            }
            _ => {}
        }
        out.push(command);
    }
    *commands = out;
    named.0
}

/// The textures [`bind_textures`] bound, each by its descriptor, once.
#[derive(Default)]
struct NamedTextures(Vec<ImageDescriptor>);

impl NamedTextures {
    /// Names the texture the descriptor `words` reads describes, where it was bound; `words` is
    /// read only then.
    fn note(&mut self, bound: Option<&RenderCommand>, words: impl FnOnce() -> Option<[u32; 8]>) {
        if let Some(words) = bound.and_then(|_| words()) {
            let descriptor = decode_image_descriptor(words);
            if !self.0.contains(&descriptor) {
                self.0.push(descriptor);
            }
        }
    }

    /// Names the texture whose descriptor is at `at`, where it was bound.
    fn note_at(&mut self, bound: Option<&RenderCommand>, at: u64, memory: &impl GuestMemory) {
        self.note(bound, || read_descriptor(at, memory));
    }
}

/// Everything [`user_data_texture`] reads for a source: the source's slot, shape and saturation, the
/// descriptor's words, the sampler's words where the user data holds them, and the address of the
/// sampler where the table does. Equal keys bind the same texture in one submission.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct UserTextureKey {
    slot: u32,
    cube: bool,
    compare: bool,
    saturated: [bool; 2],
    descriptor: Option<[u32; 8]>,
    /// Whether the sampler is in the user data, and its words there where they all are.
    sampler_in_user: bool,
    sampler_words: Option<[u32; 4]>,
    sampler_at: Option<u64>,
}

/// The [`UserTextureKey`] of `source`, whose descriptor starts at user-data word `first`.
fn user_texture_key(source: &TextureSource, first: u32, user: &[u32]) -> UserTextureKey {
    UserTextureKey {
        slot: source.slot,
        cube: source.cube,
        compare: source.compare,
        saturated: source.saturated,
        descriptor: user_words(user, first),
        sampler_in_user: source.sampler_user_data.is_some(),
        sampler_words: source
            .sampler_user_data
            .and_then(|sampler| user_words(user, sampler)),
        sampler_at: source
            .sampler_offset
            .filter(|_| source.sampler_user_data.is_none())
            .map(|offset| table_address(source.table, user) + u64::from(offset)),
    }
}

/// `N` user-data words from word `start`, where the user data holds them all.
fn user_words<const N: usize>(user: &[u32], start: u32) -> Option<[u32; N]> {
    let start = usize::try_from(start).ok()?;
    user.get(start..start.checked_add(N)?)?.try_into().ok()
}

/// What a source whose descriptor is in a table binds: the null texture for a null descriptor, the
/// depth target a compare names (D817), or the texture read at `at` under the sampler at
/// `sampler_at`. A sampler the module named is read and honoured; one that cannot be honoured
/// exactly leaves the texture unbound, so the draw is refused rather than sampled some other way.
fn table_texture(
    source: &TextureSource,
    (at, sampler_at): (u64, Option<u64>),
    memory: &impl GuestMemory,
    texels: &mut TexelCache,
) -> Option<RenderCommand> {
    if is_null_descriptor(at, memory) {
        return Some(null_texture(source.slot, source.cube));
    }
    // A depth surface is a depth target, which is not in guest memory to read (D817).
    if let Some(depth) = depth_texture(at, source, sampler_at, memory) {
        return Some(depth);
    }
    // A compare of anything else: a linear depth texture the guest uploaded, or nothing.
    if source.compare {
        let words = read_descriptor(at, memory)?;
        let sampling = match sampler_at {
            Some(at) => Some(read_sampler(at, memory)?),
            None => None,
        };
        return linear_depth_compare(words, source, sampling, memory);
    }
    let mut bound = read_texture(at, source.slot, memory, texels)?;
    if let Some(sampler_at) = sampler_at {
        let decoded = read_sampler(sampler_at, memory)?;
        // A half-border clamp is honoured only by a module that saturates the coordinates it
        // names (D743).
        if decoded.saturated() != source.saturated {
            return None;
        }
        if let RenderCommand::BindTexture { sampling, .. }
        | RenderCommand::BindVolumeTexture { sampling, .. } = &mut bound
        {
            *sampling = decoded;
        }
    }
    Some(bound)
}

/// Whether a texture read for `source` is the shape its module samples: a cube's six faces for a
/// module that samples a cube, one image otherwise (D811). Bound the other way, the host's view
/// would not be the image its module declares.
fn shaped_for(bound: &RenderCommand, source: &TextureSource) -> bool {
    match bound {
        // A depth target is sampled flat, compared or not (D816, D817).
        RenderCommand::BindDepthTexture { .. } => !source.cube && !source.volume,
        // A flat image's or a cube's texels, or a linear depth texture's depths compared.
        RenderCommand::BindTexture {
            faces, encoding, ..
        } => {
            let compared = *encoding == crate::backend::TextureEncoding::DepthCompare;
            (*faces == CUBE_FACES) == source.cube && !source.volume && compared == source.compare
        }
        // A volume's slices only under a module that samples three coordinates (D823).
        RenderCommand::BindVolumeTexture { .. } => source.volume && !source.compare,
        _ => false,
    }
}

/// Why the image descriptor `words` binds no texture, as its reader says. One it reads binds as
/// another shape than the module samples, or with a sampler that could not be read.
fn unbound_reason(
    words: Option<[u32; 8]>,
    memory: &impl GuestMemory,
    texels: &mut TexelCache,
) -> String {
    let Some(words) = words else {
        return "its descriptor is not readable".to_owned();
    };
    match texture_from_words(words, 0, memory, texels) {
        Err(why) => why.to_owned(),
        Ok(_) => concat!(
            "it reads as another shape than the module samples, or with a sampler that ",
            "could not be read"
        )
        .to_owned(),
    }
}

/// An unbound slot whose descriptor is in the user data from word `first`, described.
fn unbound_in_user_data(slot: u32, first: u32, user: &[u32]) -> String {
    let at = usize::try_from(first).unwrap_or(usize::MAX);
    let words = user.get(at..).map_or(&[][..], |w| &w[..w.len().min(8)]);
    format!("slot {slot}, from user data {first}: {}", hex_words(words))
}

/// An unbound slot whose descriptor is at `at` in guest memory, described.
fn unbound_in_table(slot: u32, at: u64, memory: &impl GuestMemory) -> String {
    let words: Vec<u32> = memory.read(at, 32).map_or_else(Vec::new, |bytes| {
        bytes
            .chunks_exact(4)
            .map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]))
            .collect()
    });
    format!("slot {slot}, from {at:#x}: {}", hex_words(&words))
}

/// Words as a report quotes them: `[0x1, 0x2]`.
fn hex_words(words: &[u32]) -> String {
    let quoted: Vec<String> = words.iter().map(|word| format!("{word:#x}")).collect();
    format!("[{}]", quoted.join(", "))
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
    let descriptor: [u32; 8] = user_words(user, first)?;
    if is_null_image(&descriptor) {
        return Some(null_texture(source.slot, source.cube));
    }
    // The sampler the module named, read: beside the image in the user data, or loaded from the
    // table the source names. [`None`] where it named none, and `Some(None)` where it cannot be read.
    let named = if let Some(sampler) = source.sampler_user_data {
        Some(
            user_words(user, sampler)
                .and_then(|words| crate::registers::decode_sampler_descriptor(words).ok()),
        )
    } else {
        source.sampler_offset.map(|offset| {
            read_sampler(
                table_address(source.table, user) + u64::from(offset),
                memory,
            )
        })
    };
    // A depth surface is a depth target, which is not in guest memory to read (D817): a sampler
    // named and unreadable binds nothing.
    if !matches!(named, Some(None))
        && let Some(depth) = depth_texture_from(descriptor, source, named.flatten())
    {
        return Some(depth);
    }
    if source.compare {
        return linear_depth_compare(descriptor, source, named.flatten(), memory);
    }
    let mut bound = texture_from_words(descriptor, source.slot, memory, texels).ok()?;
    if let Some(decoded) = named {
        let decoded = decoded?;
        // A half-border clamp is honoured only by a module that saturates the coordinates it names
        // (D743).
        if decoded.saturated() != source.saturated {
            return None;
        }
        if let RenderCommand::BindTexture { sampling, .. }
        | RenderCommand::BindVolumeTexture { sampling, .. } = &mut bound
        {
            *sampling = decoded;
        }
    }
    Some(bound)
}

/// Gives each texture whose sampler takes its border colour from the table that entry's colour, as
/// the table at `table` holds it now (D837). An entry with no table to read, or one not in guest
/// memory, is left a table entry, which no backend samples.
fn read_table_borders(
    commands: &mut [RenderCommand],
    table: Option<u64>,
    memory: &impl GuestMemory,
) {
    use crate::registers::{BORDER_COLOUR_ENTRY_BYTES, BorderColour};
    for command in commands {
        let (RenderCommand::BindTexture { sampling, .. }
        | RenderCommand::BindVolumeTexture { sampling, .. }) = command
        else {
            continue;
        };
        let BorderColour::Table(entry) = sampling.border else {
            continue;
        };
        let at = table.map(|base| base + u64::from(entry) * BORDER_COLOUR_ENTRY_BYTES);
        let Some(bytes) = at.and_then(|at| memory.read(at, BORDER_COLOUR_ENTRY_BYTES as usize))
        else {
            continue;
        };
        let mut colour = [0_u32; 4];
        for (channel, chunk) in colour.iter_mut().zip(bytes.chunks_exact(4)) {
            *channel = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        }
        sampling.border = BorderColour::Custom(colour);
    }
}

/// Whether the image descriptor at `at` is null, as [`is_null_image`] reads one.
fn is_null_descriptor(at: u64, memory: &impl GuestMemory) -> bool {
    memory.read(at, 32).is_some_and(|bytes| {
        let mut words = [0_u32; 8];
        for (word, chunk) in words.iter_mut().zip(bytes.chunks_exact(4)) {
            *word = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        }
        is_null_image(&words)
    })
}

/// Whether an image descriptor is null and samples as zero (D794): its second word zero, as Mesa
/// tests for one (`ac_nir_lower_resinfo.c:20-24`), and every destination select `SQ_SEL_0`, so a
/// sample returns zero whatever its other words hold. An all-zero descriptor, as RADV writes for a
/// texture unit nothing bound (`radv_write_image_descriptor`, `memset(dst, 0, size)`), is one.
fn is_null_image(words: &[u32; 8]) -> bool {
    words[1] == 0 && destination_selects(words[3]) == [0; 4]
}

/// What an all-zero image descriptor samples as: zero in every channel. Its `DST_SEL_X` .. `_W` are
/// all `SQ_SEL_0` ([`destination_selects`]), so a sample returns that constant whatever the texel
/// and whatever its sampler says; one texel of zero carries it, on each of a cube's faces where the
/// module samples a cube.
fn null_texture(slot: u32, cube: bool) -> RenderCommand {
    let faces = if cube { CUBE_FACES } else { 1 };
    let texels: std::sync::Arc<[u32]> = vec![0_u32; faces as usize].into();
    RenderCommand::BindTexture {
        slot,
        hash: crate::content_hash(&texels),
        texels,
        width: 1,
        height: 1,
        sampling: crate::registers::TextureSampling::default(),
        encoding: crate::backend::TextureEncoding::Rgba8,
        faces,
        levels: 1,
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
    texture_from_words(words, slot, memory, texels).ok()
}

/// The swizzle modes a texture is read in through [`read_tiled_texture`], as their layouts place a
/// chain ([`crate::tiling::SurfaceLayout`]).
const SAMPLED_AS_RENDERED: [SwizzleMode; 5] = [
    SwizzleMode::Tiled64KbRX,
    SwizzleMode::Tiled4KbDX,
    SwizzleMode::Tiled4KbS,
    SwizzleMode::Tiled64KbS,
    SwizzleMode::Linear,
];

/// The swizzle modes a depth surface is stored under: addrlib's `ADDR_SW_4KB_Z` (4),
/// `ADDR_SW_64KB_Z` (8), `ADDR_SW_64KB_Z_T` (16), `ADDR_SW_4KB_Z_X` (20) and `ADDR_SW_64KB_Z_X`
/// (24), `addrtypes.h`.
const DEPTH_SWIZZLES: [u32; 5] = [4, 8, 16, 20, 24];

/// The image formats a depth target is sampled as: `16_UNORM` (7), `32_FLOAT` (22) and
/// `X24_8_32_FLOAT` (146), `GFX10_FORMAT` in `gfx10-rsrc.json`.
const DEPTH_IMAGE_FORMATS: [u32; 3] = [7, 22, 146];

/// What a source whose descriptor is at `at` samples, when that is a depth target:
/// [`depth_texture_from`] its words and the sampler at `sampler_at`, which must be readable.
fn depth_texture(
    at: u64,
    source: &TextureSource,
    sampler_at: Option<u64>,
    memory: &impl GuestMemory,
) -> Option<RenderCommand> {
    let words = read_descriptor(at, memory)?;
    let sampling = match sampler_at {
        Some(at) => Some(read_sampler(at, memory)?),
        None => None,
    };
    depth_texture_from(words, source, sampling)
}

/// The eight words of the image descriptor at `at`.
fn read_descriptor(at: u64, memory: &impl GuestMemory) -> Option<[u32; 8]> {
    let bytes = memory.read(at, 32)?;
    let mut words = [0_u32; 8];
    for (word, chunk) in words.iter_mut().zip(bytes.chunks_exact(4)) {
        *word = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
    }
    Some(words)
}

/// `32_FLOAT`, the image format of a depth texture a GL guest uploads as texels (`gfx10-rsrc.json`).
const FORMAT_32_FLOAT: u32 = 22;
/// `16_16_16_16_FLOAT`, four sixteen-bit floats (`gfx10-rsrc.json`, enum `GFX10_FORMAT` 71).
const FORMAT_16_16_16_16_FLOAT: u32 = 71;

/// What a compare source samples when its descriptor is a linear `32_FLOAT` image of one level in
/// guest memory rather than a depth target: its depths, read as they lie, compared through the
/// sampler the module names - a GL `shadow2D` against a depth texture the guest uploaded. The
/// sampler is required, and must be one the module's coordinates were saturated for (D743).
/// [`None`] for any other descriptor or sampler.
fn linear_depth_compare(
    words: [u32; 8],
    source: &TextureSource,
    sampling: Option<crate::registers::TextureSampling>,
    memory: &impl GuestMemory,
) -> Option<RenderCommand> {
    let descriptor = decode_image_descriptor(words);
    let sampling = sampling?;
    if !source.compare
        || source.cube
        || source.volume
        || descriptor.format != FORMAT_32_FLOAT
        || descriptor.tiling != SwizzleMode::Linear
        || words[3] >> 28 != IMAGE_TYPE_2D
        || descriptor.levels > 1
        || sampling.saturated() != source.saturated
    {
        return None;
    }
    let DispatchSurface::Linear {
        base,
        width,
        height,
        pitch,
        ..
    } = linear_surface(&descriptor, words[4], 4)?
    else {
        return None;
    };
    let (w, h, row_bytes) = (width as usize, height as usize, pitch as usize * 4);
    let bytes = memory.read(base, row_bytes * (h - 1) + w * 4)?;
    let texels = row_words(face_rows(bytes, (w, h, 1), (row_bytes, 0)), w * h);
    Some(RenderCommand::BindTexture {
        slot: source.slot,
        hash: crate::content_hash(&texels),
        texels,
        width,
        height,
        sampling,
        encoding: crate::backend::TextureEncoding::DepthCompare,
        faces: 1,
        levels: 1,
    })
}

/// What a source samples when its descriptor is a flat depth surface - a depth swizzle and a depth
/// format (D817): the depth target at the descriptor's address, compared where the module compares
/// and its depth read uncompared otherwise. A compare needs its sampler; an uncompared read with
/// none named is point-sampled at the edge. The sampler must be one the module's coordinates were
/// saturated for (D743). [`None`] for any other descriptor, or a sampler that cannot be honoured.
fn depth_texture_from(
    words: [u32; 8],
    source: &TextureSource,
    sampling: Option<crate::registers::TextureSampling>,
) -> Option<RenderCommand> {
    let (slot, saturated, compare) = (source.slot, source.saturated, source.compare);
    let descriptor = decode_image_descriptor(words);
    let depth_tiled =
        matches!(descriptor.tiling, SwizzleMode::Other(mode) if DEPTH_SWIZZLES.contains(&mode));
    if !depth_tiled
        || !DEPTH_IMAGE_FORMATS.contains(&descriptor.format)
        || words[3] >> 28 != IMAGE_TYPE_2D
    {
        return None;
    }
    let sampling = match sampling {
        Some(sampling) => sampling,
        None if !compare => crate::registers::TextureSampling::CLAMPED_POINT,
        None => return None,
    };
    if sampling.saturated() != saturated || source.cube {
        return None;
    }
    Some(RenderCommand::BindDepthTexture {
        slot,
        target: crate::depth::depth_target_id_at(descriptor.base),
        sampling,
        compare,
    })
}

/// [`read_texture`] for a descriptor's eight words already in hand, or why it is not bound: a
/// block-compressed image by the check its blocks failed, any other by the one reason.
fn texture_from_words(
    words: [u32; 8],
    slot: u32,
    memory: &impl GuestMemory,
    texels: &mut TexelCache,
) -> Result<RenderCommand, &'static str> {
    let descriptor = decode_image_descriptor(words);
    if let Some(block) = block_format(descriptor.format) {
        return crate::perf::span(crate::perf::Span::TextureBlocks, || {
            read_block_texture(&descriptor, &words, block, slot, memory, texels)
        });
    }
    texel_texture(&descriptor, words, slot, memory, texels)
        .ok_or("its format, selects, tiling or shape is not one sampled exactly")
}

/// [`texture_from_words`] for an image read as texels rather than blocks.
fn texel_texture(
    descriptor: &ImageDescriptor,
    words: [u32; 8],
    slot: u32,
    memory: &impl GuestMemory,
    texels: &mut TexelCache,
) -> Option<RenderCommand> {
    let descriptor = *descriptor;
    // A tiled volume's slices are read as a dispatch reads them (D823).
    if words[3] >> 28 == IMAGE_TYPE_3D && descriptor.tiling != SwizzleMode::Linear {
        return tiled_volume_texture(&descriptor, words, slot, memory, texels);
    }
    let (format, selects) = sampled_format(&descriptor, words[3])?;
    let shape = (
        words[3] >> 28 == IMAGE_TYPE_CUBE,
        words[3] >> 28 == IMAGE_TYPE_3D,
    );
    match (descriptor.tiling, format) {
        (SwizzleMode::Linear, TexelFormat::Rgba8 | TexelFormat::Rgba8Srgb) => {}
        // The tiled layouts modelled, each sampled as it is rendered (crate::tiling); a one-byte
        // linear image with no pitch of its own lies as its layout's chain places it. A cube's
        // faces or a volume's slices in them are not modelled.
        (tiling, _)
            if shape == (false, false)
                && SAMPLED_AS_RENDERED.contains(&tiling)
                && (tiling != SwizzleMode::Linear || words[4].trailing_zeros() >= 14) =>
        {
            return read_tiled_texture(&descriptor, format, slot, memory, texels)
                .map(|bound| swizzled_texture(bound, format, selects));
        }
        _ => return None,
    }
    let (base, (width, height), pitch, faces) = linear_texture_surface(&descriptor, words, shape)?;
    // The whole chain, for the levels after the base a flat image carries (D833).
    let chain = descriptor;
    let descriptor = ImageDescriptor {
        base,
        width,
        height,
        ..descriptor
    };
    let (width, height) = (width as usize, height as usize);
    let row_bytes = pitch as usize * 4;
    let slice_bytes = row_bytes * height;
    let span = slice_bytes * (faces as usize - 1) + row_bytes * (height - 1) + width * 4;
    // A cube's six faces, or a volume's slices, are another image than the 2D one at the same
    // address.
    let key = (
        descriptor.base,
        descriptor.width,
        descriptor.height,
        pitch | u32::from(shape.0) << 24 | u32::from(shape.1) << 25,
    );
    if let Some(cached) = kept_texels(texels, key, span as u64) {
        return linear_bind(
            block_bind(
                slot,
                cached.hash,
                cached.texels.clone(),
                (descriptor.width, descriptor.height, faces),
                crate::backend::TextureEncoding::Rgba8,
            ),
            (&chain, memory),
            shape.1,
            (format, selects),
        );
    }
    let _reading = crate::perf::SpanGuard::start(crate::perf::Span::TextureLinear);
    // Marked before the bytes are hashed, so a write racing the hash is seen next time.
    let since = texture_mark(descriptor.base, span as u64);
    let texels_bytes = memory.read(descriptor.base, span)?;
    let count = width * height * faces as usize;
    let rows = face_rows(
        texels_bytes,
        (width, height, faces as usize),
        (row_bytes, slice_bytes),
    );
    let mut hasher = crate::ContentHasher::new(count);
    for row in rows.clone() {
        hasher.bytes(row);
    }
    let hash = hasher.finish();
    let shared = match texels.get(&key) {
        Some(cached) if cached.hash == hash => cached.texels.clone(),
        _ => row_words(rows, count),
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
    linear_bind(
        block_bind(
            slot,
            hash ^ u64::from(encoding_tag(texture_encoding(format))),
            shared,
            (descriptor.width, descriptor.height, faces),
            texture_encoding(format),
        ),
        (&chain, memory),
        shape.1,
        (format, selects),
    )
}

/// A linear texture's bind: a flat image's - with the levels after its base in `chain` (D833) - or
/// a cube's as [`swizzled_texture`] leaves it, and a volume's the same texels - its `faces` its slices - as a [`RenderCommand::BindVolumeTexture`]
/// (D823), read only as they are: a volume whose selects move a channel, or that is not
/// `8_8_8_8_UNORM`, is `None`.
fn linear_bind(
    bound: RenderCommand,
    (chain, memory): (&ImageDescriptor, &impl GuestMemory),
    volume: bool,
    (format, selects): (TexelFormat, [u32; 4]),
) -> Option<RenderCommand> {
    if !volume {
        let bound = with_mip_levels(bound, chain, format, memory);
        return Some(swizzled_texture(bound, format, selects));
    }
    if format != TexelFormat::Rgba8 || selects != IDENTITY_SELECTS {
        return None;
    }
    let RenderCommand::BindTexture {
        slot,
        hash,
        texels,
        width,
        height,
        sampling,
        faces,
        ..
    } = bound
    else {
        return None;
    };
    Some(RenderCommand::BindVolumeTexture {
        slot,
        texels,
        hash,
        width,
        height,
        depth: faces,
        sampling,
        encoding: crate::backend::TextureEncoding::Rgba8,
    })
}

/// A tiled volume's bind (D823): its slices read as a dispatch reads them ([`tiled_surface`]),
/// `8_8_8_8_UNORM` or `2_10_10_10_UNORM` with its selects as they are, and uncompressed. `None` for
/// any other, which is not sampled exactly. Kept, as a linear texture's texels are, until its bytes
/// are written.
fn tiled_volume_texture(
    descriptor: &ImageDescriptor,
    words: [u32; 8],
    slot: u32,
    memory: &impl GuestMemory,
    texels: &mut TexelCache,
) -> Option<RenderCommand> {
    let format = match descriptor.format {
        FORMAT_8_8_8_8_UNORM => TexelFormat::Rgba8,
        FORMAT_2_10_10_10_UNORM => TexelFormat::Rgb10A2,
        _ => return None,
    };
    if destination_selects(words[3]) != IDENTITY_SELECTS || descriptor.compression.is_some() {
        return None;
    }
    let surface = tiled_surface(descriptor, words, format, true)?;
    let (width, height) = surface.extent();
    let depth = surface.depth();
    let encoding = texture_encoding(format);
    let span = surface.bytes();
    // A volume is another image than a 2D one at the same address and extent.
    let key = (surface.base(), width, height, 1 << 26 | depth);
    let (hash, shared) = if let Some(cached) = kept_texels(texels, key, span as u64) {
        (cached.hash, cached.texels.clone())
    } else {
        // Marked before the bytes are read, so a write racing the read is seen next time.
        let since = texture_mark(surface.base(), span as u64);
        let read = surface.detile(memory.read(surface.base(), span)?);
        let hash = crate::content_hash(&read) ^ u64::from(encoding_tag(encoding));
        let shared: std::sync::Arc<[u32]> = read.into();
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
        (hash, shared)
    };
    Some(RenderCommand::BindVolumeTexture {
        slot,
        texels: shared,
        hash,
        width,
        height,
        depth,
        sampling: crate::registers::TextureSampling::default(),
        encoding,
    })
}

/// The texels cached under `key` - its first element their address - where nothing was written to
/// the `span` bytes there since they were read, as far as the host's write tracking reports.
fn kept_texels(texels: &TexelCache, key: (u64, u32, u32, u32), span: u64) -> Option<&CachedTexels> {
    let cached = texels.get(&key)?;
    let kept = cached.since.and_then(|since| {
        crate::perf::span(crate::perf::Span::TextureWatch, || {
            orbistoun_mem::watch::written_since(key.0, span, since)
        })
    }) == Some(false);
    kept.then(|| crate::perf::span(crate::perf::Span::TextureKept, || cached))
}

/// The rows of a linear image's faces, `width` four-byte texels each, `row_bytes` apart within a
/// face and the faces `slice_bytes` apart, face after face.
/// `rows` of four-byte texels as words, `count` of them, each row copied whole rather than a texel
/// at a time: a guest's linear textures are read again whenever they are written, and converting
/// them a word at a time was a tenth of a title's preparation.
fn row_words<'a>(rows: impl Iterator<Item = &'a [u8]>, count: usize) -> std::sync::Arc<[u32]> {
    let mut words = vec![0_u32; count];
    let bytes = zerocopy::IntoBytes::as_mut_bytes(words.as_mut_slice());
    let mut at = 0;
    for row in rows {
        let Some(into) = bytes.get_mut(at..at + row.len()) else {
            break;
        };
        into.copy_from_slice(row);
        at += row.len();
    }
    // The words are the bytes read little-endian, as the guest stored them.
    if cfg!(target_endian = "big") {
        for word in &mut words {
            *word = u32::from_le(*word);
        }
    }
    words.into()
}

fn face_rows(
    bytes: &[u8],
    (width, height, faces): (usize, usize, usize),
    (row_bytes, slice_bytes): (usize, usize),
) -> impl Iterator<Item = &[u8]> + Clone {
    (0..faces * height).map(move |row| {
        let (face, row) = (row / height, row % height);
        &bytes[face * slice_bytes + row * row_bytes..][..width * 4]
    })
}

/// Where a linear four-byte texture's texels lie - its base, extent and row pitch in texels - and
/// how many faces it has: a 2D image's view as [`linear_surface`] places it, or a single cube's six
/// slices, face `f` at `f` slices in, each at the 256-byte pitch the hardware derives from the
/// width, a slice its rows with no padding: measured, six faces sampled at `f * 0x4000` of a 64 x
/// 64 cube (obSCEne `REQ-20260920T1050Z-5d7c`, `20260921-final-eboot.obs.log` 16832-16935), and
/// as oops-sdk's GL lays one out (`gl_tex_cube_upload`). A cube's WORD4 `DEPTH` is its last face,
/// not a pitch. A cube of more than one level, or an array of cubes, is `None`.
///
/// A volume (`shape.1`) lies the same way, its slices for faces, `DEPTH` plus one of them: four
/// slices of a 64 x 64 volume measured at `f * 0x4000` (the same request, lines 16743-16831)
/// (D823). One level only.
fn linear_texture_surface(
    descriptor: &ImageDescriptor,
    words: [u32; 8],
    (cube, volume): (bool, bool),
) -> Option<(u64, (u32, u32), u32, u32)> {
    if volume {
        if descriptor.levels > 1 {
            return None;
        }
        let pitch = crate::registers::linear_pitch(0, descriptor.width, 4);
        let slices = (words[4] & DEPTH_FIELD) + 1;
        let extent = (descriptor.width, descriptor.height);
        return Some((descriptor.base, extent, pitch, slices));
    }
    if !cube {
        let DispatchSurface::Linear {
            base,
            width,
            height,
            pitch,
            ..
        } = linear_surface(descriptor, words[4], 4)?
        else {
            return None;
        };
        return Some((base, (width, height), pitch, 1));
    }
    if words[4] & DEPTH_FIELD != ONE_CUBE_LAST_LAYER || descriptor.levels > 1 {
        return None;
    }
    let pitch = crate::registers::linear_pitch(0, descriptor.width, 4);
    Some((
        descriptor.base,
        (descriptor.width, descriptor.height),
        pitch,
        CUBE_FACES,
    ))
}

/// What sets a texture read in an encoding other than `Rgba8` apart, in its cache entry and the
/// content hash a backend keys its image by: the same bytes are a different image to sample.
const fn encoding_tag(encoding: crate::backend::TextureEncoding) -> u32 {
    match encoding {
        crate::backend::TextureEncoding::Float11_11_10 => 1 << 22,
        crate::backend::TextureEncoding::Rgba8Srgb => 1 << 17,
        crate::backend::TextureEncoding::Rgb10A2 => 1 << 21,
        // Block-compressed blocks carry their format's tag in their own cache keys.
        crate::backend::TextureEncoding::Rgba8
        | crate::backend::TextureEncoding::Bc3
        | crate::backend::TextureEncoding::Bc1Srgb
        | crate::backend::TextureEncoding::Bc3Srgb
        | crate::backend::TextureEncoding::Bc6hUfloat => 0,
        // Never read from guest memory, so never in the cache (D817).
        crate::backend::TextureEncoding::DepthFloat
        | crate::backend::TextureEncoding::DepthCompare => 1 << 23,
    }
}

/// A 2D or cube texture's texel format and selects, when both are ones this samples exactly
/// ([`selects_exactly`]): `8_8_8_8_UNORM` or `8_UNORM`. Which layouts a cube is read in is the
/// caller's to decide.
fn sampled_format(descriptor: &ImageDescriptor, word3: u32) -> Option<(TexelFormat, [u32; 4])> {
    let format = match descriptor.format {
        FORMAT_8_8_8_8_UNORM => TexelFormat::Rgba8,
        FORMAT_8_UNORM => TexelFormat::R8,
        FORMAT_10_11_11_FLOAT => TexelFormat::Float11_11_10,
        FORMAT_8_8_8_8_SRGB => TexelFormat::Rgba8Srgb,
        _ => return None,
    };
    let selects = destination_selects(word3);
    (matches!(word3 >> 28, IMAGE_TYPE_2D | IMAGE_TYPE_3D | IMAGE_TYPE_CUBE)
        && selects_exactly(format, selects))
    .then_some((format, selects))
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
///
/// A packed float texel's channels cannot be moved within the word the host samples, so it is read
/// only as it is, with its missing alpha as the constant one the host also answers (D774).
fn selects_exactly(format: TexelFormat, selects: [u32; 4]) -> bool {
    if format == TexelFormat::Float11_11_10 {
        return matches!(selects, [4, 5, 6, 7 | 1]);
    }
    // An sRGB texel's colour channels are decoded where they lie, so they are read only as they
    // are: a channel moved into another's place would be decoded, or not, as the wrong one.
    if format == TexelFormat::Rgba8Srgb {
        return selects == IDENTITY_SELECTS;
    }
    selects.iter().all(|&select| match select {
        0 | 1 | SELECT_X => true,
        5..=7 => format == TexelFormat::Rgba8,
        _ => false,
    })
}

/// How a bound texture of `format` reaches the host: packed floats as they are, anything else as
/// `Rgba8` words.
const fn texture_encoding(format: TexelFormat) -> crate::backend::TextureEncoding {
    match format {
        TexelFormat::Float11_11_10 => crate::backend::TextureEncoding::Float11_11_10,
        TexelFormat::Rgba8Srgb => crate::backend::TextureEncoding::Rgba8Srgb,
        TexelFormat::Rgb10A2 => crate::backend::TextureEncoding::Rgb10A2,
        // A draw's texture is never a dispatch's eight-byte float one: `sampled_format` reads no
        // such format.
        TexelFormat::Rgba8
        | TexelFormat::R8
        | TexelFormat::R8Uint
        | TexelFormat::Rgba32Uint
        | TexelFormat::R32Uint
        | TexelFormat::R32Float
        | TexelFormat::Rgba16Float => crate::backend::TextureEncoding::Rgba8,
    }
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
        faces: 1,
        levels,
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
            faces: 1,
            levels,
        };
    }
    let moved = crate::content_hash(&[
        selects[0],
        selects[1],
        selects[2],
        selects[3],
        format.bytes(),
    ]);
    let hash = hash ^ moved.rotate_left(17);
    // By extent as well: the levels of one chain are bound under one hash.
    let key = (hash, width, height);
    let kept = swizzled_kept()
        .lock()
        .ok()
        .and_then(|kept| kept.get(&key).cloned());
    let swizzled = kept.unwrap_or_else(|| {
        crate::perf::span(crate::perf::Span::TextureSwizzle, || {
            let swizzled = swizzle_texels(&texels, format, selects);
            if let Ok(mut kept) = swizzled_kept().lock() {
                if kept.len() >= TEXEL_CACHE_ENTRIES {
                    kept.clear();
                }
                kept.insert(key, std::sync::Arc::clone(&swizzled));
            }
            swizzled
        })
    });
    RenderCommand::BindTexture {
        slot,
        hash,
        texels: swizzled,
        width,
        height,
        sampling,
        encoding: crate::backend::TextureEncoding::Rgba8,
        faces: 1,
        levels,
    }
}

/// Swizzled texels by the hash they are bound under and their extent. The hash is the source
/// texels' combined with the format and selects, but the levels of one chain share it, so the
/// extent is part of the key; a frame binds the same few textures many times, and a large one took
/// tens of milliseconds to swizzle at every binding.
fn swizzled_kept() -> &'static std::sync::Mutex<SwizzledKept> {
    static KEPT: std::sync::OnceLock<std::sync::Mutex<SwizzledKept>> = std::sync::OnceLock::new();
    KEPT.get_or_init(Default::default)
}

/// Swizzled texels by (hash, width, height).
type SwizzledKept = std::collections::HashMap<(u64, u32, u32), std::sync::Arc<[u32]>>;

/// Each texel's channels as `selects` pick them from `format`'s.
fn swizzle_texels(texels: &[u32], format: TexelFormat, selects: [u32; 4]) -> std::sync::Arc<[u32]> {
    let pick = |channels: [u8; 4], select: u32| match select {
        1 => 0xFF,
        4..=7 => channels[(select - SELECT_X) as usize],
        _ => 0,
    };
    texels
        .iter()
        .map(|&texel| {
            let channels = match format {
                // A draw's texture is never a sixteen- or eight-byte one: `sampled_format` reads no
                // such format, so its arm is the four-byte one's.
                TexelFormat::Rgba8
                | TexelFormat::Rgba32Uint
                | TexelFormat::Rgba16Float
                | TexelFormat::R32Uint
                | TexelFormat::R32Float
                | TexelFormat::Float11_11_10
                | TexelFormat::Rgba8Srgb
                | TexelFormat::Rgb10A2 => texel.to_le_bytes(),
                TexelFormat::R8 | TexelFormat::R8Uint => [texel.to_le_bytes()[0], 0, 0, 0xFF],
            };
            u32::from_le_bytes(selects.map(|select| pick(channels, select)))
        })
        .collect()
}

/// The host's mark of when a texture's texels were read, counted when there is none: the next
/// binding then reads them again.
fn texture_mark(base: u64, len: u64) -> Option<u64> {
    let since = orbistoun_mem::watch::mark(base, len);
    if since.is_none() {
        crate::perf::span(crate::perf::Span::TextureUnwatched, || ());
    }
    since
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
    use crate::perf::{Span, span};
    let base = span(Span::TextureBase, || {
        read_tiled_base(descriptor, format, slot, memory, texels)
    })?;
    Some(with_mip_levels(base, descriptor, format, memory))
}

/// `base` - a texture's base level, bound - with the levels after it to the descriptor's
/// `LAST_LEVEL` (D833): each where the chain's layout places it, detiled, its texels after the
/// level before's. A four-byte, uncompressed, flat image only; the base alone wherever a level
/// cannot be placed or read, as it was bound before chains were.
fn with_mip_levels(
    base: RenderCommand,
    descriptor: &ImageDescriptor,
    format: TexelFormat,
    memory: &impl GuestMemory,
) -> RenderCommand {
    crate::perf::span(crate::perf::Span::TextureLevels, || {
        with_mip_levels_now(base, descriptor, format, memory)
    })
}

/// [`with_mip_levels`], timed there.
fn with_mip_levels_now(
    base: RenderCommand,
    descriptor: &ImageDescriptor,
    format: TexelFormat,
    memory: &impl GuestMemory,
) -> RenderCommand {
    let last = descriptor
        .last_level
        .min(descriptor.levels.saturating_sub(1));
    if last <= descriptor.base_level
        || descriptor.compression.is_some()
        || format.bytes() != 4
        || !matches!(
            &base,
            RenderCommand::BindTexture {
                faces: 1,
                levels: 1,
                ..
            }
        )
    {
        return base;
    }
    let RenderCommand::BindTexture {
        slot,
        hash,
        texels,
        width,
        height,
        sampling,
        encoding,
        faces,
        ..
    } = base
    else {
        return base;
    };
    let Some((hash, texels)) = kept_chain(descriptor, last, (hash, &texels), memory) else {
        return RenderCommand::BindTexture {
            slot,
            hash,
            texels,
            width,
            height,
            sampling,
            encoding,
            faces,
            levels: 1,
        };
    };
    RenderCommand::BindTexture {
        slot,
        hash,
        texels,
        width,
        height,
        sampling,
        encoding,
        faces,
        levels: last - descriptor.base_level + 1,
    }
}

/// A chain's levels after its base, read and detiled, kept while nothing writes their memory, with
/// the last chain they were joined into.
struct KeptLevels {
    /// The bytes the levels lie in, first and length, and the host's mark of when they were read.
    span: (u64, u64),
    since: Option<u64>,
    texels: Vec<u32>,
    hash: u64,
    /// The base's hash and the chain - the base's texels, then these - last bound with it.
    joined: Option<(u64, std::sync::Arc<[u32]>)>,
}

/// How many chains' levels are kept before all are let go.
const LEVELS_KEPT: usize = 1024;

/// `base`'s chain through `last`: its hash - the base's joined with the levels' - and its texels,
/// the base's then each level's after it. The levels are read and detiled once and kept while the
/// host reports nothing written to them, and the chain is kept with them for the same base: a guest
/// binds one chain draw after draw, and reading and detiling it each time was a quarter of a
/// title's run (SuperTuxKart, 72,000 binds in 45 s). `None` when a level cannot be read.
fn kept_chain(
    descriptor: &ImageDescriptor,
    last: u32,
    (base_hash, base): (u64, &std::sync::Arc<[u32]>),
    memory: &impl GuestMemory,
) -> Option<(u64, std::sync::Arc<[u32]>)> {
    use std::hash::{Hash, Hasher};
    static KEPT: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<u64, KeptLevels>>> =
        std::sync::OnceLock::new();
    let layout = crate::tiling::SurfaceLayout::of(descriptor.tiling)?.at_bytes_per_texel(4)?;
    let mut surfaces = Vec::new();
    for level in descriptor.base_level + 1..=last {
        let surface = crate::registers::chain_level(
            (descriptor.base, descriptor.pipe_bank_xor),
            (descriptor.width, descriptor.height),
            (descriptor.levels, level),
            layout,
        )?;
        if !surface.layout.models(surface.pipe_bank_xor) {
            return None;
        }
        surfaces.push(surface);
    }
    let first = surfaces.iter().map(|surface| surface.base).min()?;
    let end = surfaces
        .iter()
        .map(|surface| surface.base + surface.words() as u64 * 4)
        .max()?;
    let span = (first, end - first);
    let key = {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        (
            descriptor.base,
            (descriptor.width, descriptor.height, descriptor.format),
            (descriptor.pipe_bank_xor, descriptor.levels),
            (descriptor.base_level, last),
            layout_tag(layout),
        )
            .hash(&mut hasher);
        hasher.finish()
    };
    let mut kept = KEPT
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let unchanged = kept.get(&key).is_some_and(|levels| {
        levels.span == span
            && levels.since.and_then(|since| {
                crate::perf::span(crate::perf::Span::TextureWatch, || {
                    orbistoun_mem::watch::written_since(span.0, span.1, since)
                })
            }) == Some(false)
    });
    if unchanged {
        crate::perf::span(crate::perf::Span::TextureKept, || ());
    } else {
        // Marked before the bytes are read, so a write racing the read is seen next time.
        let since = texture_mark(span.0, span.1);
        let mut texels = Vec::new();
        for surface in &surfaces {
            let bytes = memory.read(surface.base, surface.words() * 4)?;
            let words: Vec<u32> = bytes
                .chunks_exact(4)
                .map(|t| u32::from_le_bytes([t[0], t[1], t[2], t[3]]))
                .collect();
            texels.extend(surface.detile_mapped(&words, |w| w));
        }
        if kept.len() >= LEVELS_KEPT && !kept.contains_key(&key) {
            kept.clear();
        }
        let hash = crate::content_hash(&texels);
        kept.insert(
            key,
            KeptLevels {
                span,
                since,
                texels,
                hash,
                joined: None,
            },
        );
    }
    let levels = kept.get_mut(&key)?;
    // The base's hash, joined with the levels after it.
    let hash = base_hash.rotate_left(1) ^ levels.hash;
    if let Some((joined_base, chain)) = &levels.joined
        && *joined_base == base_hash
        && chain.len() == base.len() + levels.texels.len()
    {
        return Some((hash, std::sync::Arc::clone(chain)));
    }
    let mut chain = base.to_vec();
    chain.extend_from_slice(&levels.texels);
    let chain: std::sync::Arc<[u32]> = chain.into();
    levels.joined = Some((base_hash, std::sync::Arc::clone(&chain)));
    Some((hash, chain))
}

/// The bit a texel cache key carries for the layout its texels were detiled from, so the same
/// bytes read under two layouts are two entries.
const fn layout_tag(layout: crate::tiling::SurfaceLayout) -> u32 {
    match layout {
        crate::tiling::SurfaceLayout::Rx64Kb => 0,
        crate::tiling::SurfaceLayout::Dx4Kb => 1 << 31,
        crate::tiling::SurfaceLayout::Linear => 1 << 27,
        crate::tiling::SurfaceLayout::Rx64KbBpp1 => 1 << 26,
        crate::tiling::SurfaceLayout::LinearBpp1 => 1 << 25,
        crate::tiling::SurfaceLayout::Dx64KbBpp16 => 1 << 24,
        crate::tiling::SurfaceLayout::Dx4KbBpp16 => 1 << 23,
        crate::tiling::SurfaceLayout::S64KbBpp16 => 1 << 21,
        crate::tiling::SurfaceLayout::S4KbBpp16 => 1 << 20,
        crate::tiling::SurfaceLayout::S4Kb => 1 << 19,
        crate::tiling::SurfaceLayout::S4KbBpp8 => 1 << 18,
        crate::tiling::SurfaceLayout::S4KbBpp1 => 1 << 16,
        crate::tiling::SurfaceLayout::S64Kb => 1 << 15,
        crate::tiling::SurfaceLayout::Rx64KbBpp8 => 1 << 28,
        crate::tiling::SurfaceLayout::Rx64KbBpp2 => 1 << 29,
        crate::tiling::SurfaceLayout::Rx64KbBpp16 => 1 << 30,
    }
}

/// [`read_tiled_texture`]'s base level, read and kept as every texture's was before chains (D833).
fn read_tiled_base(
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
    let layout_tag = layout_tag(surface.layout);
    let tail_tag = match surface.place {
        crate::registers::Place::Whole => 0,
        crate::registers::Place::Within { origin: (x, y), .. } => {
            1 << 30 | (x & 0x3FFF) << 14 | (y & 0x3FFF)
        }
    };
    let encoding = texture_encoding(format);
    // A packed-float or sRGB read is its own entry, so the same bytes bound as `Rgba8` are never
    // reused for it.
    let float_tag = encoding_tag(encoding);
    let tag = layout_tag | tail_tag | float_tag | u32::from(cleared) << 29 | 1 << 28;
    let key = (surface.base, width, height, tag);
    if let Some(cached) = texels.get(&key)
        && cached
            .since
            .and_then(|since| orbistoun_mem::watch::written_since(surface.base, span as u64, since))
            == Some(false)
    {
        crate::perf::span(crate::perf::Span::TextureKept, || ());
        return Some(RenderCommand::BindTexture {
            slot,
            hash: cached.hash,
            texels: cached.texels.clone(),
            width,
            height,
            sampling: crate::registers::TextureSampling::default(),
            encoding,
            faces: 1,
            levels: 1,
        });
    }
    let since = texture_mark(surface.base, span as u64);
    let (hash, shared): (u64, std::sync::Arc<[u32]>) = if cleared {
        let zeros = vec![0u32; width as usize * height as usize];
        (crate::content_hash(&zeros), zeros.into())
    } else {
        let bytes = memory.read(surface.base, span)?;
        let mut hasher = crate::ContentHasher::new(span / 4);
        hasher.bytes(bytes);
        let hash = hasher.finish() ^ u64::from(float_tag);
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
        encoding,
        faces: 1,
        levels: 1,
    })
}

/// A block-compressed texture's blocks (`BC3_UNORM`'s sixteen bytes, `BC1_SRGB`'s eight) at the
/// level its view reads first, read from its standard-swizzle chain (`64KB_S` or `4KB_S`, the level
/// placed as addrlib places it) and laid out row-major, a block for every four-by-four texels, for
/// the host to sample natively - the one level every texture is bound with. Only a 2D, uncompressed image returning its channels as they are is
/// read, at a level whose blocks the chain's element extent gives; any other is `None`, and the draw
/// is refused rather than sampled some other way.
///
/// A single cube is read as its six faces, one after another: each is a slice of the surface, and
/// addrlib places a slice's whole mip chain before the next (`gfx10addrlib.cpp`,
/// `ComputeSurfaceInfoMacroTiled`: `sliceSize` is the chain's, `surfSize` that times the slices), so
/// face `f`'s chain starts `f` chains in (D811).
fn read_block_texture(
    descriptor: &ImageDescriptor,
    &[_, _, _, word3, word4, ..]: &[u32; 8],
    block: BlockFormat,
    slot: u32,
    memory: &impl GuestMemory,
    texels: &mut TexelCache,
) -> Result<RenderCommand, &'static str> {
    let faces = match word3 >> 28 {
        IMAGE_TYPE_2D => 1,
        IMAGE_TYPE_CUBE if word4 & DEPTH_FIELD == ONE_CUBE_LAST_LAYER => CUBE_FACES,
        _ => return Err("it is a block-compressed image that is neither 2D nor one cube"),
    };
    if !block.read_exactly(destination_selects(word3)) {
        return Err("its selects move channels its blocks are not read with");
    }
    if !matches!(
        descriptor.tiling,
        SwizzleMode::Tiled64KbS | SwizzleMode::Tiled4KbS
    ) {
        return Err("its blocks are in a tiling block-compressed textures are not read in");
    }
    if descriptor.compression.is_some() {
        return Err("its blocks are compressed further (DCC), which is not read");
    }
    let layout = crate::tiling::SurfaceLayout::of(descriptor.tiling)
        .and_then(|layout| layout.at_bytes_per_texel(block.bytes))
        .ok_or("its blocks' size has no layout in its tiling")?;
    // The chain is laid out in blocks, four texels a side (addrlib counts a compressed surface's
    // elements, `pIn->width / 4`); the level's texels are level 0's halved.
    let level = if descriptor.levels <= 1 {
        0
    } else {
        descriptor.base_level
    };
    let (width, height) = (
        (descriptor.width >> level).max(1),
        (descriptor.height >> level).max(1),
    );
    let surface = crate::registers::chain_level(
        (descriptor.base, 0),
        (descriptor.width.div_ceil(4), descriptor.height.div_ceil(4)),
        (descriptor.levels, level),
        layout,
    )
    .ok_or("the level it samples is not placed in its mip chain")?;
    // A level whose blocks are not the chain's halved elements (an odd extent's rounding) is not
    // read from a place it does not have.
    if (surface.width, surface.height) != (width.div_ceil(4), height.div_ceil(4)) {
        return Err("the level it samples is not its chain's halved blocks (an odd extent)");
    }
    let span = surface.words() * 4;
    // Each face's chain, one after another; the last face ends its level's span past its start.
    let stride = layout.chain_bytes(
        descriptor.width.div_ceil(4),
        descriptor.height.div_ceil(4),
        descriptor.levels.max(1),
    );
    let watched = stride * u64::from(faces - 1) + span as u64;
    let tail_tag = match surface.place {
        crate::registers::Place::Whole => 0,
        crate::registers::Place::Within { origin: (x, y), .. } => {
            1 << 30 | (x & 0x3FF) << 10 | (y & 0x3FF)
        }
    };
    let cube_tag = if faces > 1 { 1 << 24 } else { 0 };
    let key = (surface.base, width, height, block.tag | tail_tag | cube_tag);
    if let Some(cached) = texels.get(&key)
        && cached
            .since
            .and_then(|since| orbistoun_mem::watch::written_since(surface.base, watched, since))
            == Some(false)
    {
        crate::perf::span(crate::perf::Span::TextureKept, || ());
        return Ok(block_bind(
            slot,
            cached.hash,
            cached.texels.clone(),
            (width, height, faces),
            block.encoding,
        ));
    }
    let since = texture_mark(surface.base, watched);
    let mut blocks = Vec::new();
    for face in 0..u64::from(faces) {
        let tiled: Vec<u32> = memory
            .read(surface.base + face * stride, span)
            .ok_or("its blocks are not in guest memory it can read")?
            .chunks_exact(4)
            .map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]))
            .collect();
        blocks.extend(surface.detile_mapped(&tiled, |w| w));
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
    Ok(block_bind(
        slot,
        hash,
        shared,
        (width, height, faces),
        block.encoding,
    ))
}

/// `SQ_IMG_RSRC_WORD4.DEPTH`, bits 12:0 (`gfx10-rsrc.json`, `S_00A010_DEPTH_GFX10`).
const DEPTH_FIELD: u32 = 0x1FFF;

/// A [`RenderCommand::BindTexture`] of words in `encoding` - texels, or block-compressed blocks -
/// for `faces` images one after another, sampled as no sampler has said yet.
fn block_bind(
    slot: u32,
    hash: u64,
    texels: std::sync::Arc<[u32]>,
    (width, height, faces): (u32, u32, u32),
    encoding: crate::backend::TextureEncoding,
) -> RenderCommand {
    RenderCommand::BindTexture {
        slot,
        hash,
        texels,
        width,
        height,
        sampling: crate::registers::TextureSampling::default(),
        encoding,
        faces,
        levels: 1,
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

/// `SPI_SHADER_PGM_RSRC1_VS` and `_RSRC2_VS` (`gfx103.json`, `0x2C4A`, `0x2C4B`): a legacy vertex
/// shader's (D881).
const RSRC1_VS: u32 = 0x2C4A;
const RSRC2_VS: u32 = 0x2C4B;

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
    // A legacy vertex shader's resource words are the VS pair, and its user data starts at `s0`
    // (D881).
    let legacy = crate::registers::legacy_vertex(last(VGT_SHADER_STAGES_EN));
    let (rsrc1_vertex, rsrc2_vertex) = if legacy {
        (RSRC1_VS, RSRC2_VS)
    } else {
        (RSRC1_REGISTERS[0], RSRC2_REGISTERS[0])
    };
    // `USER_SGPR`, bits 5:1, and its sixth bit `USER_SGPR_MSB`, bit 27 of both stages' `RSRC2`
    // (`gfx103.json`): thirty-two registers set the MSB and leave the low five zero.
    let mut count = |register: u32| {
        last(register).map_or(0, |value| (value >> 1) & 0x1f | (value >> 27 & 1) << 5)
    };
    let counts = [count(rsrc2_vertex), count(RSRC2_REGISTERS[1])];
    // The geometry program's local data share, from its `RSRC2`'s `LDS_SIZE`. The pixel stage's
    // `RSRC2` holds `EXTRA_LDS_SIZE` there instead, which no translated pixel shader reads.
    let local_words = if legacy {
        None
    } else {
        local_words_of(last(RSRC2_REGISTERS[0]), GS_LDS_SIZE)
    };
    // `DX10_CLAMP`, bit 21 of the stage's `RSRC1` (Mesa `S_00B848_DX10_CLAMP`): what an output
    // clamp does with a NaN. `None` when the stream set no `RSRC1`.
    let mut dx10_clamp = |register: u32| last(register).map(|value| value & DX10_CLAMP_BIT != 0);
    let clamps = [dx10_clamp(rsrc1_vertex), dx10_clamp(RSRC1_REGISTERS[1])];
    [
        UserData {
            first_register: if legacy { 0 } else { 8 },
            count: counts[0],
            block_offset: 0,
            dx10_clamp: clamps[0],
            // What the primitive shader's further position exports are (D867).
            position_vectors: last(crate::registers::PA_CL_VS_OUT_CNTL),
            point_size: last(PA_SU_POINT_SIZE),
            pixel_inputs: None,
            input_controls: None,
            compute: None,
            geometry: None,
            window_space: WindowSpace::Clip,
            per_invocation: None,
            draw_buffers: false,
            buffer_formats: None,
            flat_twins: None,
            discard_vertex_exports: false,
            saturated: UNSATURATED,
            local_words,
            program_address: None,
            integer_targets: 0,
        },
        UserData {
            first_register: 0,
            count: counts[1],
            block_offset: USER_DATA_STAGE_WORDS,
            dx10_clamp: clamps[1],
            position_vectors: None,
            point_size: None,
            // Set by `set_environment` for every backend, with the input controls.
            pixel_inputs: None,
            input_controls: None,
            compute: None,
            geometry: None,
            window_space: WindowSpace::Clip,
            per_invocation: None,
            draw_buffers: false,
            buffer_formats: None,
            flat_twins: None,
            discard_vertex_exports: false,
            saturated: UNSATURATED,
            local_words: None,
            program_address: None,
            integer_targets: 0,
        },
    ]
}

/// The local share's size is declared in the module, so a program declaring another size is
/// another module; none declared salts nothing, which keeps the keys of modules made before.
fn local_words_salt(words: Option<u32>) -> u64 {
    words.map_or(0, |words| {
        content_hash(&words.to_le_bytes()).rotate_left(23) | 1
    })
}

/// What a draw stage's module key takes from its user-data layout: where its words land, how many,
/// and the local share it declares.
/// What sets a module translated for its program's address apart (D842): nothing for one translated
/// for no address.
fn program_address_salt(address: Option<u64>) -> u64 {
    address.map_or(0, |at| at.rotate_left(17) ^ 0x6f2c_91d3_57a0_e48b)
}

/// Whether a program reads its own address (`s_getpc_b64`), and so is a different module at each
/// place it is loaded (D842). The program is decoded here when the caller has no decode.
fn reads_own_address(
    shader: &[u8],
    decoded: Option<&orbistoun_shader::Decode>,
    (encodings, operands): (&EncodingTable, &OperandTable),
) -> bool {
    /// Answers kept by the program's content hash: a function of its bytes, asked with every
    /// submission's every shader, and decoding the program again for it each time was most of
    /// what preparing a cached shader cost.
    static KNOWN: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<u64, bool>>> =
        std::sync::OnceLock::new();
    /// A bound on the answers kept, against a guest generating programs as it goes.
    const KEPT: usize = 4096;
    let known = KNOWN.get_or_init(Default::default);
    let hash = content_hash(shader);
    if let Some(&reads) = known
        .lock()
        .ok()
        .and_then(|known| known.get(&hash).copied())
        .as_ref()
    {
        return reads;
    }
    let again;
    let decoded = if let Some(decoded) = decoded {
        decoded
    } else {
        again = decode_program(shader, encodings, operands);
        &again
    };
    let reads = decoded.instructions.iter().any(|instruction| {
        instruction
            .encoding
            .and_then(|encoding| encodings.encodings().get(usize::from(encoding)))
            .and_then(|family| encodings.mnemonic_for(&family.name, instruction.opcode))
            == Some("s_getpc_b64")
    });
    if let Ok(mut known) = known.lock() {
        if known.len() >= KEPT {
            known.clear();
        }
        known.insert(hash, reads);
    }
    reads
}

fn layout_salt(user_data: &UserData) -> u64 {
    (u64::from(user_data.count) << 56 | u64::from(user_data.first_register) << 48)
        ^ local_words_salt(user_data.local_words)
        ^ program_address_salt(user_data.program_address)
        // The further position exports' routing (D867).
        ^ user_data
            .position_vectors
            .map_or(0, |cntl| 0x504f_5356_0000_0000 ^ u64::from(cntl))
        // The size a point that exports none is drawn at (D867).
        ^ user_data
            .point_size
            .map_or(0, |size| 0x5053_5a00_0000_0000 ^ u64::from(size).rotate_left(24))
}

/// `PA_SU_POINT_SIZE` (`gfx103.json`, byte `0x28A00`, dword `0xA280`): a point's height and width
/// in eighths of a pixel, where its vertex exports no size (D867).
const PA_SU_POINT_SIZE: u32 = 0xA280;

/// `LDS_SIZE` of `SPI_SHADER_PGM_RSRC2_GS`, as `(first bit, bits)`: bits 19-26 (`gfx103.json`).
const GS_LDS_SIZE: (u32, u32) = (19, 8);

/// The words of local data share a stage's `RSRC2` declares: its `LDS_SIZE` field, `(first bit,
/// bits)`, counts 512-byte blocks from GFX7 (`ac_shader_encode_lds_size`). `None` where the stream
/// wrote no `RSRC2` or declared none.
pub(crate) fn local_words_of(rsrc2: Option<u32>, (shift, bits): (u32, u32)) -> Option<u32> {
    /// One 512-byte block, in words.
    const BLOCK_WORDS: u32 = 512 / 4;
    rsrc2
        .map(|value| (value >> shift) & ((1 << bits) - 1))
        .filter(|&blocks| blocks != 0)
        .map(|blocks| blocks * BLOCK_WORDS)
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
        // Where each attribute the pixel shader interpolates comes from: the stream's link of the
        // two stages, which a module translated without it would take as each attribute's own.
        self.user_data[1].input_controls = crate::pixel_inputs::decode_controls(writes);
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
                // The base is a function of the program's bytes: one already found for these same
                // bytes at this address is that answer.
                if let Some((_, &base)) = self.bases.held_with(candidate.address, memory) {
                    return base;
                }
                let bytes = read_window(memory, guest_address_of(candidate.address))?;
                let decoded = decode_program(bytes, &self.encodings, &self.operands);
                let base = constant_address_base(&decoded, &self.encodings);
                // Only a program that ended is known by its bytes: one that ran off its window would
                // decode differently were more of it mapped.
                if decoded.terminated {
                    self.bases.insert_with(
                        candidate.address,
                        bytes[..decoded.consumed].to_vec(),
                        base,
                    );
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

/// A program's instructions with their names, less a legacy vertex shader's streamout stores where
/// the module drops them (`drops_stores`, D881): those neither reach memory nor read what a load
/// gave them.
fn named_instructions<'a>(
    decoded: &'a orbistoun_shader::Decode,
    encodings: &'a EncodingTable,
    drops_stores: bool,
) -> Vec<(&'a orbistoun_shader::Instruction, &'a str)> {
    decoded
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
        .filter(|&(_, name)| {
            !(drops_stores
                && (name.starts_with("buffer_store") || name.starts_with("tbuffer_store")))
        })
        .collect()
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
    (served, drops_stores): (&BTreeMap<u32, u32>, bool),
) -> Option<String> {
    const ACCESSES: [&str; 6] = [
        "s_buffer_load",
        "global_",
        "flat_",
        "scratch_",
        "buffer_",
        "tbuffer_",
    ];
    let named = named_instructions(decoded, encodings, drops_stores);
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
        "vertex" | "legacy-vertex" => Some(ShaderStage::Vertex),
        "fragment" | "pixel" => Some(ShaderStage::Fragment),
        "compute" => Some(ShaderStage::Compute),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{Cached, ResourceId};

    /// What a submission draws into is every target a run of its draws draws into, each from the
    /// base in force at the run (D874): a later submission's preparation waits on all of them.
    #[test]
    fn a_submission_draws_into_the_target_of_each_run() {
        use super::{RegisterWrite, Submission, colour_target_at, drawn_targets};
        let write = |packet_offset, register, value| RegisterWrite {
            packet_offset,
            register,
            value,
        };
        // CB_COLOR0_BASE and CB_COLOR0_ATTRIB2 (16 x 8): carried in, then the stream's own.
        let (base, attrib2) = (0xA318, 0xA3B0);
        let writes = [
            write(0, attrib2, (15 << 14) | 7),
            write(0, base, 0x100),
            write(24, base, 0x200),
        ];
        let first = colour_target_at(&writes[..2]).expect("the first target");
        let second = colour_target_at(&writes).expect("the second target");
        assert_ne!(first.base, second.base);
        let one = Submission {
            colour_target: Some(second),
            colour_target_bases: 1,
            ..Submission::default()
        };
        assert_eq!(drawn_targets(&one, &writes, &[32, 40]), [second]);
        assert!(
            drawn_targets(&one, &writes, &[]).is_empty(),
            "nothing drawn"
        );
        let two = Submission {
            colour_target_bases: 2,
            ..one
        };
        assert_eq!(drawn_targets(&two, &writes, &[16, 32, 40]), [first, second]);
    }

    /// A `32_UINT` image is bound one-channel only where its selects read X then zero, zero and one,
    /// as constants or as the channels it lacks; any other order is a view the host does not give.
    #[test]
    fn a_one_channel_image_takes_only_its_natural_selects() {
        let word3 = |[x, y, z, w]: [u32; 4]| x | y << 3 | z << 6 | w << 9;
        assert!(super::one_channel_selects(word3([4, 0, 0, 1])));
        assert!(super::one_channel_selects(word3([4, 5, 6, 7])));
        assert!(!super::one_channel_selects(word3([4, 4, 4, 4])));
        assert!(!super::one_channel_selects(word3([5, 0, 0, 1])));
        assert!(!super::one_channel_selects(word3([4, 0, 0, 0])));
    }

    /// A pitched texture's rows become its texels row by row, each the bytes read little-endian,
    /// the pitch's padding left out - as converting them a texel at a time gives.
    #[test]
    fn rows_become_words_as_texel_by_texel_would() {
        let (width, height, pitch) = (3_usize, 4_usize, 5_usize);
        let bytes: Vec<u8> = (0..pitch * 4 * height)
            .map(|i| (i * 37 % 251) as u8)
            .collect();
        let rows = || super::face_rows(&bytes, (width, height, 1), (pitch * 4, 0));
        let expected: Vec<u32> = rows()
            .flat_map(|row| row.chunks_exact(4))
            .map(|t| u32::from_le_bytes([t[0], t[1], t[2], t[3]]))
            .collect();
        assert_eq!(
            &*super::row_words(rows(), width * height),
            expected.as_slice()
        );
    }

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
                cube: false,
                volume: false,
                compare: false,
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
        let named = bind_textures(
            &mut commands,
            (&sources, &mut super::TexelCache::new()),
            &memory,
            (&mut unbound, &mut None),
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
        // The texture bound is named by its descriptor, where it lies included.
        assert_eq!(
            named
                .iter()
                .map(|t| (t.width, t.height))
                .collect::<Vec<_>>(),
            bound.iter().map(|b| (b.2, b.3)).collect::<Vec<_>>()
        );
        assert_eq!(
            bound,
            Some((1, vec![0], 1, 1)),
            "one texel, every channel zero"
        );
    }

    /// A descriptor Mesa calls null - its second word zero - with every select `SQ_SEL_0` binds the
    /// null texture whatever its other words hold (D794); with a select that reads the texel it
    /// stays unbound. The words are PPSA28061's: user data 9 to 16 of a pixel shader that blends a
    /// second texture by a factor of zero.
    #[test]
    fn a_null_descriptor_with_zero_selects_reads_zero() {
        use super::{RenderCommand, ShaderStage, TableBase, TextureSource, bind_textures};
        struct Memory;
        impl super::GuestMemory for Memory {
            fn read(&self, _address: u64, _length: usize) -> Option<&[u8]> {
                None
            }
        }
        let bind = |word3: u32| {
            let shader = ResourceId(1);
            let mut words = [0u32; super::USER_DATA_WORDS];
            words[9..17]
                .copy_from_slice(&[0, 0, 0x80_0eb0, word3, 0x1_2255, 0x4000, 0x26e0, 0x7400]);
            let sources = std::collections::BTreeMap::from([(
                shader,
                vec![TextureSource {
                    slot: 0,
                    table_offset: None,
                    table: TableBase::default(),
                    sampler_offset: None,
                    user_data: Some(9),
                    sampler_user_data: Some(17),
                    cube: false,
                    volume: false,
                    compare: false,
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
                &Memory,
                (&mut unbound, &mut None),
            );
            let texels = commands.iter().find_map(|command| match command {
                RenderCommand::BindTexture { texels, .. } => Some(texels.to_vec()),
                _ => None,
            });
            (unbound, texels)
        };
        assert_eq!(bind(0x6000), (0, Some(vec![0])), "every select SQ_SEL_0");
        assert_eq!(bind(0x6000 | 4), (1, None), "X reads the texel");
    }

    /// A slot nothing binds is counted and the first is described - the slot, where its
    /// descriptor was read and the words there - so the refusal says what could not be read. The
    /// words are PPSA02664's: a buffer descriptor where its pixel shader samples from user data.
    #[test]
    fn the_first_unbound_slot_is_described() {
        use super::{RenderCommand, ShaderStage, TableBase, TextureSource, bind_textures};
        struct Memory;
        impl super::GuestMemory for Memory {
            fn read(&self, _address: u64, _length: usize) -> Option<&[u8]> {
                None
            }
        }
        let shader = ResourceId(1);
        let mut words = [0u32; super::USER_DATA_WORDS];
        words[..8].copy_from_slice(&[0x11e9_df14, 0x10_7400, 1, 0x4_dfac, 0, 0x70_00b0, 0, 0]);
        let sources = std::collections::BTreeMap::from([(
            shader,
            vec![TextureSource {
                slot: 0,
                table_offset: None,
                table: TableBase::default(),
                sampler_offset: None,
                user_data: Some(0),
                sampler_user_data: Some(8),
                cube: false,
                volume: false,
                compare: false,
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
        let (mut unbound, mut first) = (0, None);
        bind_textures(
            &mut commands,
            (&sources, &mut super::TexelCache::new()),
            &Memory,
            (&mut unbound, &mut first),
        );
        assert_eq!(unbound, 1);
        assert_eq!(
            first.as_deref(),
            Some(concat!(
                "slot 0, from user data 0: [0x11e9df14, 0x107400, 0x1, 0x4dfac, 0x0, 0x7000b0, 0x0, ",
                "0x0] - its format, selects, tiling or shape is not one sampled exactly"
            ))
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
                    cube: false,
                    volume: false,
                    compare: false,
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
                (&mut unbound, &mut None),
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

    /// A depth compare's texture is the depth target at its descriptor's address, named rather than
    /// read (D817): PPSA03416's shadow map, `16_UNORM` in `64KB_Z_X` at 0x7400d6e50000, sampled with
    /// a sampler comparing less-than. A sample that does not compare reads the same target, its
    /// depth, uncompared: PPSA03416's pixel shaders read their depth surface so.
    #[test]
    fn a_depth_compare_samples_the_depth_target_at_its_address() {
        use super::{RenderCommand, ShaderStage, TableBase, TextureSource, bind_textures};
        struct Memory(Vec<u8>);
        impl super::GuestMemory for Memory {
            fn read(&self, address: u64, length: usize) -> Option<&[u8]> {
                let start = usize::try_from(address.checked_sub(0x1000)?).ok()?;
                self.0.get(start..start.checked_add(length)?)
            }
        }
        let image: [u32; 8] = [
            0x00d6_e500,
            0xc070_0074,
            0x03ff_c3ff,
            0x9180_0924,
            0,
            0x0070_0000,
            0,
            0,
        ];
        // Clamped to the edge, both filters linear, `DEPTH_COMPARE_FUNC` less (1).
        let sampler = [2 | 2 << 3 | 1 << 12, 0, 1 << 20 | 1 << 22, 0];
        let mut bytes = vec![0u8; 0x40];
        for (i, word) in image.iter().chain(&sampler).enumerate() {
            bytes[i * 4..i * 4 + 4].copy_from_slice(&word.to_le_bytes());
        }
        let memory = Memory(bytes);
        let shader = ResourceId(7);
        let mut words = [0u32; super::USER_DATA_WORDS];
        words[0] = 0x1000;
        let bind = |compare: bool| {
            let sources = std::collections::BTreeMap::from([(
                shader,
                vec![TextureSource {
                    slot: 2,
                    table_offset: Some(0),
                    table: TableBase::default(),
                    sampler_offset: Some(0x20),
                    compare,
                    ..TextureSource::default()
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
                (&mut unbound, &mut None),
            );
            let bound = commands.iter().find_map(|command| match command {
                RenderCommand::BindDepthTexture {
                    slot,
                    target,
                    sampling,
                    compare,
                } => Some((*slot, *target, sampling.depth_compare, *compare)),
                _ => None,
            });
            (unbound, bound)
        };
        assert_eq!(
            bind(true),
            (
                0,
                Some((
                    2,
                    crate::depth::depth_target_id_at(0x7400_d6e5_0000),
                    1,
                    true
                ))
            )
        );
        assert_eq!(
            bind(false),
            (
                0,
                Some((
                    2,
                    crate::depth::depth_target_id_at(0x7400_d6e5_0000),
                    1,
                    false
                ))
            ),
            "not a compare: the depth, uncompared"
        );
    }

    /// A depth compare whose descriptor and sampler are in the user data names the depth target too
    /// (D817): PPSA03416's shadow map in user data 8, its sampler in 16.
    #[test]
    fn a_depth_compare_from_user_data_samples_the_depth_target() {
        use super::{RenderCommand, TableBase, TextureSource, user_data_texture};
        struct Memory;
        impl super::GuestMemory for Memory {
            fn read(&self, _: u64, _: usize) -> Option<&[u8]> {
                None
            }
        }
        let mut user = [0_u32; 20];
        user[8..16].copy_from_slice(&[
            0x00d8_ed00,
            0xc070_0074,
            0x03ff_c3ff,
            0x9180_0924,
            0,
            0x0070_0000,
            0,
            0,
        ]);
        user[16..20].copy_from_slice(&[2 | 2 << 3 | 3 << 12, 0, 1 << 20 | 1 << 22, 0]);
        let source = TextureSource {
            slot: 2,
            table_offset: None,
            table: TableBase::default(),
            sampler_offset: None,
            user_data: Some(8),
            sampler_user_data: Some(16),
            cube: false,
            volume: false,
            compare: true,
            saturated: [false; 2],
        };
        let bound = user_data_texture(&source, 8, &user, &Memory, &mut super::TexelCache::new());
        let Some(RenderCommand::BindDepthTexture {
            slot,
            target,
            sampling,
            ..
        }) = bound
        else {
            panic!("{bound:?}");
        };
        assert_eq!(
            (slot, target, sampling.depth_compare),
            (2, crate::depth::depth_target_id_at(0x7400_d8ed_0000), 3)
        );
    }

    /// A program written over another at one address, and the first written back, is found without
    /// decoding either again, and the one found is the one the address answers: 2S2H's GL layer
    /// puts each draw's program in the same slot, and 25500 of 40000 preparations decoded again
    /// when only the last program there was kept.
    #[test]
    fn programs_taking_turns_at_one_address_are_each_found() {
        use super::DecodedPrograms;
        struct Memory(Vec<u8>);
        impl super::GuestMemory for Memory {
            fn read(&self, address: u64, length: usize) -> Option<&[u8]> {
                let start = usize::try_from(address.checked_sub(0x1000)?).ok()?;
                self.0.get(start..start.checked_add(length)?)
            }
        }
        let (first, second) = (vec![1_u8; 12], vec![2_u8; 8]);
        let mut decoded = DecodedPrograms::default();
        decoded.insert(0x1000, first.clone());
        decoded.insert(0x1000, second.clone());
        assert_eq!(decoded.get(0x1000), Some(&second), "the last one written");
        let memory = Memory(first.clone());
        assert_eq!(decoded.held(0x1000, &memory), Some(&first[..]));
        assert_eq!(decoded.get(0x1000), Some(&first), "the one found there now");
        let memory = Memory(second.clone());
        assert_eq!(decoded.held(0x1000, &memory), Some(&second[..]));
        assert_eq!(decoded.held(0x1000, &Memory(vec![3; 12])), None);
    }

    /// A texture in the user data is read once a submission however many draws bind it, as one in a
    /// table is: PPSA25872 sets the same words before each of its draws, and each read asked the
    /// host whether its 64 MiB atlas had been written since - half a millisecond a draw. The words
    /// are PPSA03416's shadow map, whose sampler is read from the table, so each read is seen.
    #[test]
    fn a_user_data_texture_is_read_once_a_submission() {
        use super::{
            RenderCommand, ShaderStage, TableBase, TableWord, TextureSource, bind_textures,
        };
        struct Memory(Vec<u8>, std::cell::Cell<usize>);
        impl super::GuestMemory for Memory {
            fn read(&self, address: u64, length: usize) -> Option<&[u8]> {
                self.1.set(self.1.get() + 1);
                let start = usize::try_from(address.checked_sub(0x1000)?).ok()?;
                self.0.get(start..start.checked_add(length)?)
            }
        }
        let mut bytes = vec![0_u8; 0x20];
        for (i, word) in [2_u32 | 2 << 3 | 6 << 12, 0, 1 << 20 | 1 << 22, 0]
            .iter()
            .enumerate()
        {
            bytes[0x10 + i * 4..0x14 + i * 4].copy_from_slice(&word.to_le_bytes());
        }
        let memory = Memory(bytes, std::cell::Cell::new(0));
        let mut words = [0_u32; super::USER_DATA_WORDS];
        words[28] = 0x1000;
        words[8..16].copy_from_slice(&[
            0x00d6_f900,
            0xc070_0074,
            0x03ff_c3ff,
            0x9180_0924,
            0,
            0x0070_0000,
            0,
            0,
        ]);
        let shader = ResourceId(1);
        let sources = std::collections::BTreeMap::from([(
            shader,
            vec![TextureSource {
                slot: 2,
                table_offset: None,
                table: TableBase {
                    low: TableWord::UserData(28),
                    high: TableWord::UserData(29),
                },
                sampler_offset: Some(0x10),
                user_data: Some(8),
                sampler_user_data: None,
                cube: false,
                volume: false,
                compare: true,
                saturated: [false; 2],
            }],
        )]);
        let draw = RenderCommand::Draw {
            vertices: 3,
            instances: 1,
            first_vertex: 0,
        };
        let set = RenderCommand::SetUserData {
            stage: ShaderStage::Fragment,
            words,
        };
        let mut commands = vec![
            set.clone(),
            RenderCommand::BindShader {
                stage: ShaderStage::Fragment,
                shader,
            },
            draw.clone(),
            set,
            draw,
        ];
        let mut unbound = 0;
        bind_textures(
            &mut commands,
            (&sources, &mut super::TexelCache::new()),
            &memory,
            (&mut unbound, &mut None),
        );
        let bound = commands
            .iter()
            .filter(|command| matches!(command, RenderCommand::BindDepthTexture { .. }))
            .count();
        assert_eq!((unbound, bound), (0, 2), "bound before each draw");
        assert_eq!(memory.1.get(), 1, "the sampler read once");
    }

    /// A depth compare whose descriptor is in the user data and whose sampler was loaded from the
    /// table names the depth target, compared as the table's sampler says: PPSA03416's shadow-map
    /// shaders load the sampler from `s[28:29]` + 0x10.
    #[test]
    fn a_user_data_depth_compare_reads_its_sampler_from_the_table() {
        use super::{RenderCommand, TableBase, TableWord, TextureSource, user_data_texture};
        struct Memory(Vec<u8>);
        impl super::GuestMemory for Memory {
            fn read(&self, address: u64, length: usize) -> Option<&[u8]> {
                let start = usize::try_from(address.checked_sub(0x1000)?).ok()?;
                self.0.get(start..start.checked_add(length)?)
            }
        }
        let mut bytes = vec![0_u8; 0x20];
        for (i, word) in [2_u32 | 2 << 3 | 6 << 12, 0, 1 << 20 | 1 << 22, 0]
            .iter()
            .enumerate()
        {
            bytes[0x10 + i * 4..0x14 + i * 4].copy_from_slice(&word.to_le_bytes());
        }
        let mut user = [0_u32; 32];
        user[28] = 0x1000;
        user[8..16].copy_from_slice(&[
            0x00d6_f900,
            0xc070_0074,
            0x03ff_c3ff,
            0x9180_0924,
            0,
            0x0070_0000,
            0,
            0,
        ]);
        let source = TextureSource {
            slot: 2,
            table_offset: None,
            table: TableBase {
                low: TableWord::UserData(28),
                high: TableWord::UserData(29),
            },
            sampler_offset: Some(0x10),
            user_data: Some(8),
            sampler_user_data: None,
            cube: false,
            volume: false,
            compare: true,
            saturated: [false; 2],
        };
        let bound = user_data_texture(
            &source,
            8,
            &user,
            &Memory(bytes),
            &mut super::TexelCache::new(),
        );
        let Some(RenderCommand::BindDepthTexture { sampling, .. }) = bound else {
            panic!("{bound:?}");
        };
        assert_eq!(
            sampling.depth_compare, 6,
            "greater-or-equal, from the table"
        );
    }

    /// Swizzled texels are kept per size as well as hash: two bindings under one hash - two levels
    /// of one chain - each get texels of their own extent. STKT00001 bound a 128 x 128 level with a
    /// 256 x 256 one's texels, which the device refused.
    #[test]
    fn swizzled_texels_are_kept_per_extent() {
        use super::{RenderCommand, TexelFormat, swizzled_texture};
        let bind = |side: u32| RenderCommand::BindTexture {
            slot: 0,
            hash: 0x5eed_0000_0000_0001,
            texels: vec![0x0403_0201_u32; (side * side) as usize].into(),
            width: side,
            height: side,
            sampling: crate::registers::TextureSampling::CLAMPED_POINT,
            encoding: crate::backend::TextureEncoding::Rgba8,
            faces: 1,
            levels: 1,
        };
        // Blue, green, red, alpha: not the identity, so the texels are swizzled.
        let selects = [6, 5, 4, 7];
        for side in [2_u32, 1] {
            let RenderCommand::BindTexture { texels, .. } =
                swizzled_texture(bind(side), TexelFormat::Rgba8, selects)
            else {
                panic!("a texture binding");
            };
            assert_eq!(texels.len(), (side * side) as usize, "{side} x {side}");
        }
    }

    /// A texture that is not bound says why, so the refusal names the check that failed rather than
    /// leaving it to be worked out from the descriptor's words: a block-compressed image in a tiling
    /// its blocks are not read in, and one of a type that is neither 2D nor a cube.
    #[test]
    fn an_unbound_texture_says_why() {
        use super::texture_from_words;
        struct Nothing;
        impl super::GuestMemory for Nothing {
            fn read(&self, _address: u64, _length: usize) -> Option<&[u8]> {
                None
            }
        }
        let bc3 = |word3: u32| [0x100, 173 << 20 | 3 << 30, 1 | 7 << 14, word3, 0, 0, 0, 0];
        // 2D, `ADDR_SW_64KB_R_X` (27): rendered, not a standard swizzle.
        let refused =
            texture_from_words(bc3(0x91B0_0FAC), 0, &Nothing, &mut super::TexelCache::new());
        assert_eq!(
            refused.err(),
            Some("its blocks are in a tiling block-compressed textures are not read in")
        );
        // A 3D image (type 10) in `64KB_S`.
        let refused =
            texture_from_words(bc3(0xA090_0FAC), 0, &Nothing, &mut super::TexelCache::new());
        assert_eq!(
            refused.err(),
            Some("it is a block-compressed image that is neither 2D nor one cube")
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
        let memory = Memory(bytes);
        let bound = texture_from_words(words, 0, &memory, &mut super::TexelCache::new()).ok();
        let Some(RenderCommand::BindTexture {
            texels,
            width: 8,
            height: 8,
            encoding: crate::backend::TextureEncoding::Bc3,
            faces: 1,
            ..
        }) = bound
        else {
            panic!("bound as BC3 blocks: {bound:?}");
        };
        let tags: Vec<u32> = texels.chunks(4).map(|block| block[0]).collect();
        assert_eq!(tags, [0x000, 0x100, 0x1000, 0x1100], "row-major blocks");
        assert_eq!(texels[1], 1, "a block's words in order");
        // `BC3_SRGB` (174): the same blocks, sampled with their colours decoded from sRGB.
        let mut srgb = words;
        srgb[1] = (174 << 20) | (((width - 1) & 3) << 30);
        let bound = texture_from_words(srgb, 0, &memory, &mut super::TexelCache::new()).ok();
        assert!(
            matches!(
                bound,
                Some(RenderCommand::BindTexture {
                    encoding: crate::backend::TextureEncoding::Bc3Srgb,
                    ..
                })
            ),
            "bound as BC3_SRGB blocks: {bound:?}"
        );
    }

    /// A `BC1_SRGB` texture in `4KB_S` is bound as its eight-byte blocks, detiled row-major, for the
    /// host to sample natively with sRGB decode: an 8 x 8 image is two blocks across and two down,
    /// each read from where addrlib's 8-byte `4KB_S` pattern puts it. PPSA03416's descriptor
    /// (`0xcaa00074`, format 170, `0x90580fac`) is such a texture.
    #[test]
    fn a_bc1_srgb_texture_binds_its_blocks_in_row_order() {
        use super::{RenderCommand, texture_from_words};
        struct Memory(Vec<u8>);
        impl super::GuestMemory for Memory {
            fn read(&self, address: u64, length: usize) -> Option<&[u8]> {
                let start = usize::try_from(address.checked_sub(0x1_0000)?).ok()?;
                self.0.get(start..start.checked_add(length)?)
            }
        }
        let mut bytes = vec![0u8; 4096];
        for (y, x) in [(0u32, 0u32), (0, 1), (1, 0), (1, 1)] {
            let at = crate::tiling::tiled_byte_offset_4kb_s_bpp8(x, y, 2);
            for word in 0..2u32 {
                let value = (y << 12) | (x << 8) | word;
                bytes[at + 4 * word as usize..][..4].copy_from_slice(&value.to_le_bytes());
            }
        }
        let (width, height) = (8u32, 8u32);
        let words = [
            0x100,
            (170 << 20) | (((width - 1) & 3) << 30),
            ((width - 1) >> 2) | ((height - 1) << 14),
            // 2D, ADDR_SW_4KB_S, the channels as they are.
            0x9050_0fac,
            0,
            0x70_0000,
            0,
            0,
        ];
        let bound =
            texture_from_words(words, 0, &Memory(bytes), &mut super::TexelCache::new()).ok();
        let Some(RenderCommand::BindTexture {
            texels,
            width: 8,
            height: 8,
            encoding: crate::backend::TextureEncoding::Bc1Srgb,
            faces: 1,
            ..
        }) = bound
        else {
            panic!("bound as BC1 sRGB blocks: {bound:?}");
        };
        let tags: Vec<u32> = texels.chunks(2).map(|block| block[0]).collect();
        assert_eq!(tags, [0x000, 0x100, 0x1000, 0x1100], "row-major blocks");
        assert_eq!(texels[1], 1, "a block's words in order");
    }

    /// A sampler whose border is a table entry takes that entry's four floats from the table
    /// `TA_BC_BASE_ADDR` names (D837): gl1-probe's `border-and-mirror` blue at entry 0. With no
    /// table, or one not in memory, the entry is left unread.
    #[test]
    fn a_table_border_is_read_from_the_table() {
        use super::RenderCommand;
        use crate::registers::{BorderColour, TextureSampling};
        struct Memory(Vec<u8>);
        impl super::GuestMemory for Memory {
            fn read(&self, address: u64, length: usize) -> Option<&[u8]> {
                let start = usize::try_from(address.checked_sub(0x1_0000)?).ok()?;
                self.0.get(start..start.checked_add(length)?)
            }
        }
        let mut bytes = vec![0u8; 0x200];
        for (i, value) in [0.0_f32, 0.0, 1.0, 1.0].iter().enumerate() {
            bytes[0x100 + 16 + i * 4..][..4].copy_from_slice(&value.to_bits().to_le_bytes());
        }
        let bind = |entry| RenderCommand::BindTexture {
            slot: 0,
            hash: 0,
            texels: vec![0; 1].into(),
            width: 1,
            height: 1,
            sampling: TextureSampling {
                border: BorderColour::Table(entry),
                ..TextureSampling::default()
            },
            encoding: crate::backend::TextureEncoding::Rgba8,
            faces: 1,
            levels: 1,
        };
        let border = |commands: &[RenderCommand]| match &commands[0] {
            RenderCommand::BindTexture { sampling, .. } => sampling.border,
            _ => unreachable!(),
        };
        let memory = Memory(bytes);
        let mut commands = vec![bind(1)];
        super::read_table_borders(&mut commands, Some(0x1_0100), &memory);
        assert_eq!(
            border(&commands),
            BorderColour::Custom([0, 0, 1.0_f32.to_bits(), 1.0_f32.to_bits()])
        );
        let mut unread = vec![bind(1)];
        super::read_table_borders(&mut unread, None, &memory);
        assert_eq!(border(&unread), BorderColour::Table(1), "no table");
        super::read_table_borders(&mut unread, Some(0x9_0000), &memory);
        assert_eq!(
            border(&unread),
            BorderColour::Table(1),
            "a table not in memory"
        );
    }

    /// A linear `8_8_8_8_UNORM` chain binds every level from its base to its last, each read where
    /// the linear chain places it (D833): gl1-probe's `mipmap-levels` texture, an 8 x 8 image with
    /// four levels and no pitch of its own (its descriptor as oops-gl wrote it, moved to 0x10000).
    #[test]
    fn a_linear_mip_chain_binds_every_level() {
        use super::{RenderCommand, texture_from_words};
        struct Memory(Vec<u8>);
        impl super::GuestMemory for Memory {
            fn read(&self, address: u64, length: usize) -> Option<&[u8]> {
                let start = usize::try_from(address.checked_sub(0x1_0000)?).ok()?;
                self.0.get(start..start.checked_add(length)?)
            }
        }
        let mut bytes = vec![0u8; 0x4000];
        for level in 0..4u32 {
            let (offset, pitch) =
                crate::tiling::linear_level_of(8, 8, (4, level), 4).expect("a level of the chain");
            let side = 8 >> level;
            for y in 0..side {
                for x in 0..side {
                    let at = offset as usize + (y * pitch + x) as usize * 4;
                    bytes[at..at + 4].copy_from_slice(&(0x1111_1111 * (level + 1)).to_le_bytes());
                }
            }
        }
        let words = [
            0x100,
            // oops-gl's word, its base's high byte (0x74) cleared.
            0xc380_0000,
            0x8001_c001,
            0x9003_0fac,
            0,
            0x0040_0030,
            0,
            0,
        ];
        let bound =
            texture_from_words(words, 0, &Memory(bytes), &mut super::TexelCache::new()).ok();
        let Some(RenderCommand::BindTexture {
            texels,
            width: 8,
            height: 8,
            levels: 4,
            ..
        }) = bound
        else {
            panic!("bound with its four levels: {bound:?}");
        };
        let mut expected = Vec::new();
        for level in 0..4u32 {
            let side = (8 >> level) as usize;
            expected.extend(std::iter::repeat_n(0x1111_1111 * (level + 1), side * side));
        }
        assert_eq!(texels[..], expected[..], "each level after the one before");
    }

    /// A 32-bpp texture in `4KB_S` binds, each texel read from where that swizzle puts it: the
    /// 4 x 4 `8_8_8_8_UNORM` image PPSA02664 samples beside its `BC3` one.
    #[test]
    fn a_32_bpp_4kb_s_texture_binds_its_texels() {
        use super::{RenderCommand, texture_from_words};
        struct Memory(Vec<u8>);
        impl super::GuestMemory for Memory {
            fn read(&self, address: u64, length: usize) -> Option<&[u8]> {
                let start = usize::try_from(address.checked_sub(0x1_0000)?).ok()?;
                self.0.get(start..start.checked_add(length)?)
            }
        }
        let mut bytes = vec![0u8; 4096];
        for y in 0..4u32 {
            for x in 0..4u32 {
                let at = crate::tiling::tiled_byte_offset_4kb_s_bpp4(x, y, 4);
                bytes[at..at + 4].copy_from_slice(&(y << 8 | x).to_le_bytes());
            }
        }
        let (width, height) = (4u32, 4u32);
        let words = [
            0x100,
            (56 << 20) | (((width - 1) & 3) << 30),
            ((width - 1) >> 2) | ((height - 1) << 14),
            // 2D, ADDR_SW_4KB_S, the channels as they are.
            0x9050_0fac,
            0,
            0x70_0000,
            0,
            0,
        ];
        let bound =
            texture_from_words(words, 0, &Memory(bytes), &mut super::TexelCache::new()).ok();
        let Some(RenderCommand::BindTexture {
            texels,
            width: 4,
            height: 4,
            ..
        }) = bound
        else {
            panic!("bound: {bound:?}");
        };
        let expected: Vec<u32> = (0..4u32)
            .flat_map(|y| (0..4u32).map(move |x| y << 8 | x))
            .collect();
        assert_eq!(texels.to_vec(), expected);
    }

    /// A linear `8_8_8_8_UNORM` cube binds its six faces, face `f` at `f` slices of the 256-byte
    /// pitch in: GLTP00001's 2 x 2 cube, its descriptor as the SDK writes it (`DEPTH` 5 the last
    /// face, not a pitch). A cube in a tiled layout is not read.
    #[test]
    fn a_linear_cube_binds_its_six_faces() {
        use super::{RenderCommand, texture_from_words};
        struct Memory(Vec<u8>);
        impl super::GuestMemory for Memory {
            fn read(&self, address: u64, length: usize) -> Option<&[u8]> {
                let start = usize::try_from(address.checked_sub(0x1_0000)?).ok()?;
                self.0.get(start..start.checked_add(length)?)
            }
        }
        // Rows of 64 texels (256 bytes); a face is two rows.
        let (pitch, side) = (64usize, 2u32);
        let mut bytes = vec![0u8; pitch * 4 * side as usize * 6];
        for face in 0..6u32 {
            for y in 0..side {
                for x in 0..side {
                    let at = ((face * side + y) as usize * pitch + x as usize) * 4;
                    bytes[at..at + 4].copy_from_slice(&(face << 16 | y << 8 | x).to_le_bytes());
                }
            }
        }
        let words = |word3: u32| {
            [
                0x100,
                (56 << 20) | (((side - 1) & 3) << 30),
                ((side - 1) >> 2) | ((side - 1) << 14) | (1 << 31),
                word3,
                5,
                0x40_0000,
                0,
                0,
            ]
        };
        let memory = Memory(bytes);
        let bound = texture_from_words(
            words(0xb000_0fac),
            0,
            &memory,
            &mut super::TexelCache::new(),
        )
        .ok();
        let Some(RenderCommand::BindTexture {
            texels,
            width: 2,
            height: 2,
            faces: 6,
            ..
        }) = bound
        else {
            panic!("bound: {bound:?}");
        };
        let expected: Vec<u32> = (0..6u32)
            .flat_map(|face| {
                (0..side).flat_map(move |y| (0..side).map(move |x| face << 16 | y << 8 | x))
            })
            .collect();
        assert_eq!(texels.to_vec(), expected);
        // The same cube in `4KB_S`, whose faces are not modelled.
        assert!(
            texture_from_words(
                words(0xb050_0fac),
                0,
                &memory,
                &mut super::TexelCache::new()
            )
            .is_err()
        );
    }

    /// A compare of a linear `32_FLOAT` image - a depth texture a GL guest uploaded - binds its
    /// depths as they lie, compared through the module's sampler, and only for a compare; without a
    /// sampler it binds nothing.
    #[test]
    fn a_linear_float_depth_texture_is_compared_through_its_sampler() {
        use super::{RenderCommand, TextureSource, linear_depth_compare, shaped_for};
        struct Memory(Vec<u8>);
        impl super::GuestMemory for Memory {
            fn read(&self, address: u64, length: usize) -> Option<&[u8]> {
                let start = usize::try_from(address.checked_sub(0x1_0000)?).ok()?;
                self.0.get(start..start.checked_add(length)?)
            }
        }
        let mut bytes = vec![0u8; 256];
        bytes[..4].copy_from_slice(&0.25f32.to_bits().to_le_bytes());
        bytes[4..8].copy_from_slice(&0.75f32.to_bits().to_le_bytes());
        // 2 x 1, 32_FLOAT, linear 2D, custom pitch 64 (DEPTH 0x3f), one level: GLTP00001's shape.
        let words = [
            0x100,
            (22 << 20) | (1 << 30),
            1 << 31,
            0x9000_0fac,
            0x3f,
            0x40_0000,
            0,
            0,
        ];
        let compare = TextureSource {
            compare: true,
            ..TextureSource::default()
        };
        let sampling = crate::registers::TextureSampling {
            depth_compare: 1,
            ..crate::registers::TextureSampling::default()
        };
        let memory = Memory(bytes);
        let bound = linear_depth_compare(words, &compare, Some(sampling), &memory);
        let Some(
            ref command @ RenderCommand::BindTexture {
                ref texels,
                width: 2,
                height: 1,
                encoding: crate::backend::TextureEncoding::DepthCompare,
                sampling: bound_sampling,
                ..
            },
        ) = bound
        else {
            panic!("bound: {bound:?}");
        };
        assert_eq!(
            texels.to_vec(),
            [0.25f32.to_bits(), 0.75f32.to_bits()],
            "the depths as they lie"
        );
        assert_eq!(bound_sampling, sampling, "through the module's sampler");
        assert!(shaped_for(command, &compare));
        assert!(
            !shaped_for(command, &TextureSource::default()),
            "not handed to a sample that does not compare"
        );
        assert!(
            linear_depth_compare(words, &compare, None, &memory).is_none(),
            "a compare needs its sampler"
        );
    }

    /// A linear `8_8_8_8_UNORM` volume binds its slices as one three-dimensional texture, slice
    /// `s` at `s` slices of the 256-byte pitch in, `DEPTH` plus one of them (D823), and only under
    /// a module that samples a volume.
    #[test]
    fn a_linear_volume_binds_its_slices() {
        use super::{RenderCommand, TextureSource, shaped_for, texture_from_words};
        struct Memory(Vec<u8>);
        impl super::GuestMemory for Memory {
            fn read(&self, address: u64, length: usize) -> Option<&[u8]> {
                let start = usize::try_from(address.checked_sub(0x1_0000)?).ok()?;
                self.0.get(start..start.checked_add(length)?)
            }
        }
        let (pitch, side, slices) = (64usize, 2u32, 3u32);
        let mut bytes = vec![0u8; pitch * 4 * side as usize * slices as usize];
        for slice in 0..slices {
            for y in 0..side {
                for x in 0..side {
                    let at = ((slice * side + y) as usize * pitch + x as usize) * 4;
                    bytes[at..at + 4].copy_from_slice(&(slice << 16 | y << 8 | x).to_le_bytes());
                }
            }
        }
        let words = [
            0x100,
            (56 << 20) | (((side - 1) & 3) << 30),
            ((side - 1) >> 2) | ((side - 1) << 14) | (1 << 31),
            // 3D, linear, the channels as they are.
            0xa000_0fac,
            slices - 1,
            0x40_0000,
            0,
            0,
        ];
        let bound =
            texture_from_words(words, 0, &Memory(bytes), &mut super::TexelCache::new()).ok();
        let Some(
            ref command @ RenderCommand::BindVolumeTexture {
                ref texels,
                width: 2,
                height: 2,
                depth: 3,
                ..
            },
        ) = bound
        else {
            panic!("bound: {bound:?}");
        };
        let expected: Vec<u32> = (0..slices)
            .flat_map(|slice| {
                (0..side).flat_map(move |y| (0..side).map(move |x| slice << 16 | y << 8 | x))
            })
            .collect();
        assert_eq!(texels.to_vec(), expected);
        let volume = TextureSource {
            volume: true,
            ..TextureSource::default()
        };
        assert!(shaped_for(command, &volume));
        assert!(
            !shaped_for(command, &TextureSource::default()),
            "not handed to a flat sample"
        );
    }

    /// A mipmapped `BC3` texture in `4KB_S` binds the level its view reads first, from where the
    /// chain places it: a 64 x 64 chain of seven levels has level 0 after the tail's block and
    /// level 1 at (8, 0) in that block, eight blocks a side.
    #[test]
    fn a_mipmapped_4kb_s_bc3_texture_binds_its_base_level() {
        use super::{RenderCommand, texture_from_words};
        struct Memory(Vec<u8>);
        impl super::GuestMemory for Memory {
            fn read(&self, address: u64, length: usize) -> Option<&[u8]> {
                let start = usize::try_from(address.checked_sub(0x1_0000)?).ok()?;
                self.0.get(start..start.checked_add(length)?)
            }
        }
        let tag = |level: u32, x: u32, y: u32, word: u32| level << 16 | y << 12 | x << 8 | word;
        let mut bytes = vec![0u8; 8192];
        for y in 0..16 {
            for x in 0..16 {
                let at = 4096 + crate::tiling::tiled_byte_offset_4kb_s_bpp16(x, y, 16);
                for word in 0..4 {
                    bytes[at + 4 * word as usize..][..4]
                        .copy_from_slice(&tag(0, x, y, word).to_le_bytes());
                }
            }
        }
        for y in 0..8 {
            for x in 0..8 {
                let at = crate::tiling::tiled_byte_offset_4kb_s_bpp16(8 + x, y, 16);
                for word in 0..4 {
                    bytes[at + 4 * word as usize..][..4]
                        .copy_from_slice(&tag(1, x, y, word).to_le_bytes());
                }
            }
        }
        let memory = Memory(bytes);
        let (width, height) = (64u32, 64u32);
        let words = |base_level: u32| {
            [
                0x100,
                (173 << 20) | (((width - 1) & 3) << 30),
                ((width - 1) >> 2) | ((height - 1) << 14) | (1 << 31),
                // 2D, ADDR_SW_4KB_S, levels base..=6, the channels as they are.
                0x9056_0fac | base_level << 12,
                0,
                6 << 4,
                0,
                0,
            ]
        };
        for (base_level, side) in [(1, 32), (0, 64)] {
            let bound =
                texture_from_words(words(base_level), 0, &memory, &mut super::TexelCache::new())
                    .ok();
            let Some(RenderCommand::BindTexture {
                texels,
                width: bound_width,
                height: bound_height,
                encoding: crate::backend::TextureEncoding::Bc3,
                faces: 1,
                ..
            }) = bound
            else {
                panic!("level {base_level} bound as BC3 blocks: {bound:?}");
            };
            assert_eq!((bound_width, bound_height), (side, side));
            let across = side / 4;
            assert_eq!(texels.len(), (across * across * 4) as usize);
            for (x, y) in [(0, 0), (1, 0), (0, 1), (across - 1, across - 1)] {
                let at = ((y * across + x) * 4) as usize;
                assert_eq!(
                    texels[at..at + 4],
                    [0, 1, 2, 3].map(|word| tag(base_level, x, y, word)),
                    "level {base_level} block ({x}, {y})"
                );
            }
        }
    }

    /// A `BC6_UFLOAT` cube binds its six faces one after another, each read a whole chain past the
    /// last: 64 x 64 texels in `4KB_S` is sixteen blocks a side, one 4 KiB swizzle block a face, so
    /// face `f` starts `4096 * f` bytes in (D811). A cube whose last layer is past the sixth face is
    /// an array of cubes and is not read, and a module sampling flat is not handed the cube.
    #[test]
    fn a_bc6_cube_binds_its_six_faces_a_chain_apart() {
        use super::{RenderCommand, texture_from_words};
        struct Memory(Vec<u8>);
        impl super::GuestMemory for Memory {
            fn read(&self, address: u64, length: usize) -> Option<&[u8]> {
                let start = usize::try_from(address.checked_sub(0x1_0000)?).ok()?;
                self.0.get(start..start.checked_add(length)?)
            }
        }
        let tag = |face: u32, x: u32, y: u32, word: u32| face << 16 | y << 12 | x << 8 | word;
        let mut bytes = vec![0u8; 6 * 4096];
        for face in 0..6 {
            for y in 0..16 {
                for x in 0..16 {
                    let at = face as usize * 4096
                        + crate::tiling::tiled_byte_offset_4kb_s_bpp16(x, y, 16);
                    for word in 0..4 {
                        bytes[at + 4 * word as usize..][..4]
                            .copy_from_slice(&tag(face, x, y, word).to_le_bytes());
                    }
                }
            }
        }
        let memory = Memory(bytes);
        let (width, height) = (64u32, 64u32);
        let words = |last_layer: u32| {
            [
                0x100,
                (179 << 20) | (((width - 1) & 3) << 30),
                ((width - 1) >> 2) | ((height - 1) << 14) | (1 << 31),
                // A cube, ADDR_SW_4KB_S, one level, X, Y and Z as they are and W one: PPSA03416's
                // selects for its cube, which has no alpha.
                0xb050_03ac,
                last_layer,
                0,
                0,
                0,
            ]
        };
        let bound = texture_from_words(words(5), 0, &memory, &mut super::TexelCache::new()).ok();
        let Some(
            ref command @ RenderCommand::BindTexture {
                ref texels,
                width: 64,
                height: 64,
                encoding: crate::backend::TextureEncoding::Bc6hUfloat,
                faces: 6,
                ..
            },
        ) = bound
        else {
            panic!("bound as six faces of BC6 blocks: {bound:?}");
        };
        assert_eq!(texels.len(), 6 * 16 * 16 * 4);
        for face in 0..6 {
            for (x, y) in [(0, 0), (1, 0), (0, 1), (15, 15)] {
                let at = (face * 16 * 16 * 4 + (y * 16 + x) * 4) as usize;
                assert_eq!(
                    texels[at..at + 4],
                    [0, 1, 2, 3].map(|word| tag(face, x, y, word)),
                    "face {face} block ({x}, {y})"
                );
            }
        }
        let source = |cube: bool| super::TextureSource {
            slot: 0,
            table_offset: None,
            table: super::TableBase::default(),
            sampler_offset: None,
            user_data: None,
            sampler_user_data: None,
            saturated: [false; 2],
            cube,
            volume: false,
            compare: false,
        };
        assert!(super::shaped_for(command, &source(true)));
        assert!(
            !super::shaped_for(command, &source(false)),
            "not handed to a flat sample"
        );
        let volume = super::TextureSource {
            cube: false,
            volume: true,
            ..source(false)
        };
        assert!(
            !super::shaped_for(command, &volume),
            "nor to a volume, which binds its slices (D823)"
        );
        assert!(
            texture_from_words(words(11), 0, &memory, &mut super::TexelCache::new()).is_err(),
            "two cubes are an array, not read"
        );
    }

    /// A dispatch image's selects reorder an `8_8_8_8_UNORM` texel's channels: radeonsi's BGRA view
    /// of a colour target selects `Z, Y, X, W` (`0x99b0_0f2e`, DRIP00001's readback copy), so a
    /// shader reading channel 0 gets memory's byte 2, and writing it back puts it there again.
    /// Selects as they are, constants, repeats and other formats are left alone.
    #[test]
    fn a_dispatch_image_reads_its_channels_in_the_order_its_selects_name() {
        use super::{TexelFormat, channel_order, to_memory_channels, to_shader_channels};
        let bgra = 0x99b0_0f2e;
        let order = channel_order(TexelFormat::Rgba8, bgra).expect("a reordering");
        assert_eq!(order, [2, 1, 0, 3]);
        // Memory holds B, G, R, A in bytes 0-3: 0x40 blue, 0x80 green, 0xff red.
        let memory = 0xffff_8040_u32;
        let shader = to_shader_channels(memory, order);
        assert_eq!(shader.to_le_bytes(), [0xff, 0x80, 0x40, 0xff], "R, G, B, A");
        assert_eq!(to_memory_channels(shader, order), memory);
        // `X, Y, Z, W`, an alpha of one, a repeat and a one-byte format.
        for (format, word3) in [
            (TexelFormat::Rgba8, 0x9050_0fac),
            (TexelFormat::Rgba8, 0x9050_03ac),
            (TexelFormat::Rgba8, 0x9050_0924),
            (TexelFormat::R8, bgra),
        ] {
            assert_eq!(channel_order(format, word3), None, "{word3:#x}");
        }
    }

    /// A 32-bpp `64KB_R_X` volume's texel `(x, y, z)` lies a slice of blocks per slice in, at its
    /// 2D place in the block XORed with the slice's terms (`tools/addrlib/equation 64KB_R_X 4 32
    /// 3d`): a 32 x 32 x 3 volume is three 64 KiB blocks, slice 1's texels moved by `0x800` and
    /// slice 2's by `0x400`. Its texels detile in order and tile back to the same bytes. Grand Theft
    /// Auto V's first sixteen-wave dispatch stores to a 32 x 32 x 32 volume so.
    #[test]
    fn a_volume_lies_slice_after_slice_with_each_slice_s_xor() {
        use super::{DispatchSurface, SwizzleMode};
        use crate::tiling::{SurfaceLayout, tiled_byte_offset_64kb_rx_bpp4};
        let layout = SurfaceLayout::of(SwizzleMode::Tiled64KbRX)
            .and_then(|layout| layout.at_bytes_per_texel(4))
            .expect("a 32-bpp 64KB_R_X layout");
        let slice = crate::registers::chain_level((0x10_0000, 0), (32, 32), (1, 0), layout)
            .expect("one level");
        let volume = DispatchSurface::Volume { slice, depth: 3 };
        assert_eq!(volume.bytes(), 3 * 0x1_0000, "a block a slice");
        assert_eq!(volume.texels(), 32 * 32 * 3);
        let mut spanned = vec![0_u8; volume.bytes()];
        for z in 0..3_u32 {
            let xor = [0, 0x800, 0x400][z as usize];
            for y in 0..32_u32 {
                for x in 0..32_u32 {
                    let at = z as usize * 0x1_0000
                        + (tiled_byte_offset_64kb_rx_bpp4(x, y) as usize ^ xor);
                    let index = (z * 32 + y) * 32 + x;
                    spanned[at..at + 4].copy_from_slice(&index.to_le_bytes());
                }
            }
        }
        let texels = volume.detile(&spanned);
        assert!(
            texels
                .iter()
                .zip(0_u32..)
                .all(|(&texel, index)| texel == index),
            "every texel in order"
        );
        let mut tiled = vec![0_u8; volume.bytes()];
        volume.tile(&texels, &mut tiled).expect("tiled back");
        assert!(tiled == spanned, "tiled back to the same bytes");
    }

    /// A draw samples a tiled ten-bit volume as a dispatch reads it (D823): a `2_10_10_10_UNORM`
    /// 32 x 32 x 2 volume in 32-bpp `64KB_R_X`, its selects as they are - Grand Theft Auto V's
    /// `0xa1b00fac` - binds as a volume of its texels in order, slice after slice, sampled as ten-bit
    /// words.
    #[test]
    fn a_tiled_ten_bit_volume_binds_as_a_volume() {
        use super::{RenderCommand, texture_from_words};
        use crate::tiling::tiled_byte_offset_64kb_rx_bpp4;
        struct Memory(Vec<u8>);
        impl super::GuestMemory for Memory {
            fn read(&self, address: u64, length: usize) -> Option<&[u8]> {
                let start = usize::try_from(address.checked_sub(0x10_0000)?).ok()?;
                self.0.get(start..start.checked_add(length)?)
            }
        }
        let (width, height, depth) = (32_u32, 32_u32, 2_u32);
        let mut bytes = vec![0_u8; 2 * 0x1_0000];
        for z in 0..depth {
            let xor = [0, 0x800][z as usize];
            for y in 0..height {
                for x in 0..width {
                    let at = z as usize * 0x1_0000
                        + (tiled_byte_offset_64kb_rx_bpp4(x, y) as usize ^ xor);
                    let index = (z * height + y) * width + x;
                    bytes[at..at + 4].copy_from_slice(&index.to_le_bytes());
                }
            }
        }
        let words = [
            0x10_0000 >> 8,
            (50 << 20) | (((width - 1) & 3) << 30),
            ((width - 1) >> 2) | ((height - 1) << 14),
            0xa1b0_0fac,
            depth - 1,
            0,
            0,
            0,
        ];
        let bound = texture_from_words(words, 0, &Memory(bytes), &mut super::TexelCache::new())
            .expect("a volume");
        let RenderCommand::BindVolumeTexture {
            texels,
            width: 32,
            height: 32,
            depth: 2,
            encoding: crate::backend::TextureEncoding::Rgb10A2,
            ..
        } = bound
        else {
            panic!("{bound:?}");
        };
        assert!(
            texels
                .iter()
                .zip(0_u32..)
                .all(|(&texel, index)| texel == index),
            "every texel in order"
        );
    }

    /// A dispatch image a refusal quotes is described with its depth and its type, as well as its
    /// format, layout, extent and levels: Grand Theft Auto V's first sixteen-wave dispatch stores
    /// to a volume, which a width and height alone do not say.
    #[test]
    fn a_dispatch_image_is_described_with_its_depth_and_type() {
        let (width, height, depth) = (64u32, 32u32, 16u32);
        let words = [
            0x100,
            (1 << 20) | (((width - 1) & 3) << 30),
            ((width - 1) >> 2) | ((height - 1) << 14),
            10 << 28,
            depth - 1,
            0,
            0,
            0,
        ];
        let descriptor = super::decode_image_descriptor(words);
        let text = super::describe_dispatch_image(&descriptor, words);
        assert!(
            text.contains("64x32x16") && text.ends_with("type 10)"),
            "{text}"
        );
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
                    cube: false,
                    volume: false,
                    compare: false,
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
                (&mut unbound, &mut None),
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
        assert_eq!(saturated[0], [true, false]);
        assert!(saturated[1..].iter().all(|slot| *slot == [false; 2]));
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
                local_words: None,
                groups: [1, 1, 1],
                vertex_streamout: false,
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
        // What carrying it out writes is the buffer it stores to, not the one it only reads (D882).
        assert_eq!(
            pipeline.dispatch_write_spans(&state(DESTINATION), &memory),
            Some(vec![(DESTINATION, 256)])
        );
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
        let first = pipeline.draw_buffer_formats(address, (&words, &[]), &Nothing);
        assert_eq!(first.map(|f| f.get(0)), Some(Some(0x1111_1111)));
        assert_eq!(
            pipeline
                .draw_buffer_formats(address, (&words, &[]), &Nothing)
                .map(|f| f.get(0)),
            Some(Some(0x1111_1111)),
            "served again"
        );
        pipeline.decoded.insert(address, program(24));
        assert_eq!(
            pipeline
                .draw_buffer_formats(address, (&words, &[]), &Nothing)
                .map(|f| f.get(0)),
            Some(Some(0x2222_2222)),
            "the rewritten program's descriptor"
        );
    }

    /// A pair of programs' flat twins are worked out once for their bytes and kept across
    /// submissions, as 2S2H draws the same pairs in submission after submission; a program written
    /// over another at the same address is its own pair, answered for itself at once.
    #[test]
    fn a_program_pair_s_flat_twins_are_kept_by_their_bytes() {
        use super::{Candidate, Pipeline, ShaderStage};
        use orbistoun_translate::{Fidelity, Strategy, Width};
        let bytes =
            |words: &[u32]| -> Vec<u8> { words.iter().flat_map(|w| w.to_le_bytes()).collect() };
        // attr0.x interpolated and attr0.y flat (D742), then attr0.x interpolated alone.
        let mixed = bytes(&[0xC808_0000, 0xC809_0001, 0xC80E_0102, 0xBF81_0000]);
        let smooth = bytes(&[0xC808_0000, 0xC809_0001, 0xBF81_0000]);
        let mut pipeline = Pipeline::new(Strategy::Predicated {
            fidelity: Fidelity::Wavefront,
            width: Width::Wave64,
        })
        .expect("a pipeline");
        let fragment = 0x50_0000;
        let candidates = [Candidate {
            address: fragment,
            stage: ShaderStage::Fragment,
        }];
        pipeline.decoded.insert(fragment, mixed.clone());
        assert!(
            pipeline.draw_flat_twins(&candidates).is_some(),
            "attribute 0 is mixed"
        );
        pipeline.end_draws();
        assert_eq!(
            pipeline.flat_twins_kept.len(),
            1,
            "kept past the submission"
        );
        pipeline.decoded.insert(fragment, smooth);
        assert!(
            pipeline.draw_flat_twins(&candidates).is_none(),
            "the program written over it, answered for itself"
        );
        pipeline.decoded.insert(fragment, mixed);
        assert!(pipeline.draw_flat_twins(&candidates).is_some());
        assert_eq!(pipeline.flat_twins_kept.len(), 2, "each pair once");
    }

    /// While a submission's draws are bound, a draw's chunks asking for the same program's formats
    /// with the same words are served what was resolved for the first - here even after the
    /// program is replaced, which cannot happen within one submission - and outside that the
    /// program is traced as it now stands.
    #[test]
    fn a_draws_chunks_share_the_formats_resolved_for_it() {
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
        let format = |pipeline: &mut Pipeline, words: &[u32; USER_DATA_WORDS]| {
            pipeline
                .draw_buffer_formats(address, (words, &[]), &Nothing)
                .and_then(|f| f.get(0))
        };
        pipeline.decoded.insert(address, program(20));
        pipeline.formats_memo = super::FormatsMemo::Kept(None);
        assert_eq!(format(&mut pipeline, &words), Some(0x1111_1111));
        pipeline.decoded.insert(address, program(24));
        assert_eq!(
            format(&mut pipeline, &words),
            Some(0x1111_1111),
            "the draw's own"
        );
        words[0] = 1;
        assert_eq!(
            format(&mut pipeline, &words),
            Some(0x2222_2222),
            "other words are resolved again"
        );
        pipeline.formats_memo = super::FormatsMemo::Off;
        words[0] = 0;
        assert_eq!(
            format(&mut pipeline, &words),
            Some(0x2222_2222),
            "and traced as it stands"
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

    /// A `10_11_11_FLOAT` texture - PPSA02664's HDR target, sampled by the pass that follows it -
    /// binds its words detiled and untouched, for the host to sample as packed floats (D774).
    #[test]
    fn a_packed_float_texture_binds_its_words_as_they_are() {
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
            ((texels >> 40) as u32 & 0xff) | (36 << 20) | (((width - 1) & 3) << 30),
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
        let swapped = {
            let mut swapped = bytes.clone();
            // Red and green exchanged: not a read a packed float texel can give.
            let word3 = (9_u32 << 28) | (27 << 20) | 0xfa5;
            swapped[12..16].copy_from_slice(&word3.to_le_bytes());
            swapped
        };
        assert!(
            read_texture(table, 0, &Memory(swapped), &mut super::TexelCache::new()).is_none(),
            "channels moved within a packed texel are not read"
        );
        let Some(RenderCommand::BindTexture {
            texels,
            width: w,
            height: h,
            encoding,
            faces: 1,
            ..
        }) = read_texture(table, 0, &Memory(bytes), &mut cache)
        else {
            panic!("a tiled 10_11_11 float texture binds");
        };
        assert_eq!(encoding, crate::backend::TextureEncoding::Float11_11_10);
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

    /// An `8_8_8_8_SRGB` texture - PPSA28061's notice-screen image - binds its words detiled and
    /// untouched, for the host to decode from sRGB as it samples. The same bytes read as
    /// `8_8_8_8_UNORM` are another image, so their content hashes differ; and a channel moved from
    /// its place is not read, since the decode belongs to where it lies.
    #[test]
    fn an_srgb_texture_binds_its_words_for_the_host_to_decode() {
        use super::{RenderCommand, read_texture};
        struct Memory(Vec<u8>);
        impl super::GuestMemory for Memory {
            fn read(&self, address: u64, length: usize) -> Option<&[u8]> {
                let start = usize::try_from(address.checked_sub(0x1000)?).ok()?;
                self.0.get(start..start.checked_add(length)?)
            }
        }
        let (table, texels, width, height) = (0x1000_u64, 0x1_0000_u64, 8u32, 4u32);
        let image = |format: u32, word3: u32| {
            let mut bytes = vec![0u8; 0xF000 + 0x1_0000];
            let descriptor = [
                (texels >> 8) as u32,
                ((texels >> 40) as u32 & 0xff) | (format << 20) | (((width - 1) & 3) << 30),
                ((width - 1) >> 2) | ((height - 1) << 14) | (1 << 31),
                word3,
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
                        + crate::tiling::tiled_byte_offset_64kb_rx_bpp4_surface(x, y, width);
                    bytes[at..at + 4].copy_from_slice(&(0x8040_2010 + y * 100 + x).to_le_bytes());
                }
            }
            Memory(bytes)
        };
        let identity = (9_u32 << 28) | (27 << 20) | 0xfac;
        let bind = |format, word3| {
            read_texture(
                table,
                0,
                &image(format, word3),
                &mut super::TexelCache::new(),
            )
        };
        let Some(RenderCommand::BindTexture {
            texels: words,
            encoding,
            faces: 1,
            hash,
            ..
        }) = bind(130, identity)
        else {
            panic!("a tiled 8_8_8_8_SRGB texture binds");
        };
        assert_eq!(encoding, crate::backend::TextureEncoding::Rgba8Srgb);
        let expected: Vec<u32> = (0..height)
            .flat_map(|y| (0..width).map(move |x| 0x8040_2010 + y * 100 + x))
            .collect();
        assert_eq!(words.as_ref(), expected.as_slice(), "row-major, untouched");
        let Some(RenderCommand::BindTexture { hash: unorm, .. }) = bind(56, identity) else {
            panic!("the same bytes as 8_8_8_8_UNORM bind");
        };
        assert_ne!(hash, unorm, "another image to the backend");
        // Red and green exchanged.
        assert!(bind(130, (9_u32 << 28) | (27 << 20) | 0xfa5).is_none());
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

    /// A dispatch image detiled again from the same surface and bytes is the texels kept for them,
    /// as a fresh detile would make; new bytes, or another surface over the same bytes, are
    /// detiled anew.
    #[test]
    fn a_dispatch_image_is_detiled_again_only_for_new_bytes() {
        use super::{DetiledImages, DispatchSurface};
        let surface = DispatchSurface::Linear {
            base: 0x1000,
            width: 3,
            height: 2,
            pitch: 64,
            bytes_per_texel: 4,
        };
        let narrow = DispatchSurface::Linear {
            base: 0x1000,
            width: 2,
            height: 2,
            pitch: 64,
            bytes_per_texel: 4,
        };
        let mut spanned: Vec<u8> = (0..67_u32).flat_map(u32::to_le_bytes).collect();
        let mut detiled = DetiledImages::default();
        assert_eq!(detiled.detile(&surface, &spanned), surface.detile(&spanned));
        assert_eq!(
            detiled.detile(&surface, &spanned),
            surface.detile(&spanned),
            "kept"
        );
        assert_eq!(
            detiled.detile(&narrow, &spanned),
            narrow.detile(&spanned),
            "another surface"
        );
        spanned[4] = 0xff;
        assert_eq!(
            detiled.detile(&surface, &spanned),
            vec![0, 0xff, 2, 64, 65, 66],
            "new bytes"
        );
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

    /// The geometry program's local share is what its `RSRC2_GS` declares: `LDS_SIZE`, bits 19-26
    /// (`gfx103.json`), in 512-byte blocks (`ac_shader_encode_lds_size`); none declared is none.
    /// The pixel stage's field there is `EXTRA_LDS_SIZE`, not its own share.
    #[test]
    fn the_geometry_program_s_local_share_is_its_rsrc2_s() {
        let write = |register, value| crate::registers::RegisterWrite {
            packet_offset: 0,
            register,
            value,
        };
        let layouts = super::user_data_layouts(&[
            write(super::RSRC2_REGISTERS[0], 4 << 19 | 12 << 1),
            write(super::RSRC2_REGISTERS[1], 4 << 19),
        ]);
        assert_eq!(layouts[0].local_words, Some(512), "four 512-byte blocks");
        assert_eq!(layouts[0].count, 12, "the user registers beside it");
        assert_eq!(layouts[1].local_words, None);
        let undeclared = super::user_data_layouts(&[write(super::RSRC2_REGISTERS[0], 12 << 1)]);
        assert_eq!(undeclared[0].local_words, None);
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

    /// A submission's parts prepared as it is carried out run under the state the whole was
    /// prepared over, though the carried state has moved past it since: what it leaves is carried
    /// before it runs (D821). Here the second stream draws before it resizes the target, and its
    /// draw is prepared after its own resize has been carried; it still draws into the target the
    /// first stream sized. STKT00001 drew a one-byte target as the four-byte one its stream set
    /// next, refused the draw and aborted on the fence it never wrote.
    #[test]
    fn a_submission_runs_under_the_state_it_was_prepared_over() {
        use super::Pipeline;
        use crate::packet::build::{nop, set_context_register};
        use orbistoun_translate::{Fidelity, Strategy, Width};

        struct Nothing;
        impl super::GuestMemory for Nothing {
            fn read(&self, _address: u64, _length: usize) -> Option<&[u8]> {
                None
            }
        }
        let bytes =
            |words: &[u32]| -> Vec<u8> { words.iter().flat_map(|w| w.to_le_bytes()).collect() };
        let extents = |submission: &super::Submission| {
            submission
                .targets
                .values()
                .map(|e| (e.width, e.height))
                .collect::<Vec<_>>()
        };
        // `CB_COLOR0_ATTRIB2`: 256 x 128, then 64 x 32.
        let first = bytes(&set_context_register(0x3B0, (255 << 14) | 127));
        let before = bytes(&nop());
        let mut second = before.clone();
        second.extend(bytes(&set_context_register(0x3B0, (63 << 14) | 31)));

        let mut live = Pipeline::new(Strategy::Predicated {
            fidelity: Fidelity::Lane,
            width: Width::default(),
        })
        .expect("a pipeline")
        .carrying_register_state();
        live.submit(&first, super::Queue::Draw, &[], &Nothing);
        live.carry(&first, None);
        let whole = live.submit(&second, super::Queue::Draw, &[], &Nothing);
        live.carry(&second, None);

        let part = live.submit_over(
            whole.carried_in.clone(),
            &before,
            super::Queue::Draw,
            &[],
            &Nothing,
        );
        assert_eq!(
            extents(&part),
            vec![(256, 128)],
            "the state it was prepared over"
        );
        let walked = crate::packet::walk(&before);
        let (writes, own) = live.writes_as_submitted(&whole, &walked, &before, None);
        assert_eq!(own, 0);
        assert!(
            writes
                .iter()
                .any(|w| w.register == 0xA3B0 && w.value == (255 << 14) | 127),
            "the first stream's size, not the second's: {writes:?}"
        );
        assert_eq!(
            extents(&live.submit(&before, super::Queue::Draw, &[], &Nothing)),
            vec![(64, 32)],
            "a new submission starts from everything carried"
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
        let decoded = pipeline.decoded.get(AT).map_or(0, Vec::len);

        let mut rewritten = 0xbe82_03ffu32.to_le_bytes().to_vec();
        rewritten.extend(0x1234u32.to_le_bytes());
        rewritten.extend(&program);
        let (second, fresh) = prepare(&mut pipeline, &placed(&rewritten));
        assert!(
            fresh && second != first,
            "a rewritten shader translates again"
        );
        assert_eq!(
            pipeline.decoded.get(AT).map(Vec::as_slice),
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
                (&std::collections::BTreeMap::new(), false),
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

    /// Quads are seeded as triangles: a quad list two a whole quad (D798), a quad strip as the
    /// triangle strip over its whole quads (D797).
    #[test]
    fn quads_are_seeded_as_triangles() {
        use super::{VGT_PRIMITIVE_TYPE, draw_geometry};
        use crate::registers::{DrawCall, DrawKind};
        let draw = |vertices, instances| DrawCall {
            packet_offset: 0,
            instances,
            kind: DrawKind::Auto { vertices },
        };
        let registers = |topology: u32, _stages: u32, _offset: u32| {
            move |register| match register {
                VGT_PRIMITIVE_TYPE => Some(topology),
                _ => None,
            }
        };
        // A quad list is two triangles a whole quad (D798): ten vertices are two quads.
        assert_eq!(
            draw_geometry(&draw(10, 1), registers(super::DI_PT_QUADLIST, 0, 0))
                .map(|g| (g.primitives, g.assembly)),
            Ok((
                4,
                super::Assembly::Quads {
                    provoking_last: false
                }
            ))
        );
        // A quad strip is the triangle strip over its whole quads (D797): eight vertices are three
        // quads, six triangles, and a ninth closes none.
        for (vertices, triangles) in [(8, 6), (9, 6), (4, 2), (3, 0)] {
            assert_eq!(
                draw_geometry(&draw(vertices, 1), registers(super::DI_PT_QUADSTRIP, 0, 0))
                    .map(|g| (g.primitives, g.assembly)),
                Ok((
                    triangles,
                    super::Assembly::Strip {
                        provoking_last: false
                    }
                )),
                "{vertices} vertices"
            );
        }
    }

    /// An index offset moves an indexed draw's fetched indices and not an auto draw's ids (D843,
    /// measured by obSCEne ix01: offset 5 gave an auto draw ids 0, 1, 2 and an indexed one 5, 6,
    /// 7).
    #[test]
    fn an_index_offset_moves_fetched_indices_only() {
        use super::{DI_PT_RECTLIST, GE_INDX_OFFSET, VGT_PRIMITIVE_TYPE, draw_geometry};
        use crate::registers::{DrawCall, DrawKind};
        let auto = DrawCall {
            packet_offset: 0,
            instances: 1,
            kind: DrawKind::Auto { vertices: 3 },
        };
        let registers = |register| match register {
            VGT_PRIMITIVE_TYPE => Some(DI_PT_RECTLIST),
            GE_INDX_OFFSET => Some(5),
            _ => None,
        };
        assert_eq!(
            draw_geometry(&auto, registers).map(|g| (g.first_vertex, g.vertices)),
            Ok((0, 3)),
            "the ids are 0, 1 and 2"
        );
        let indexed = DrawCall {
            kind: DrawKind::Indexed {
                indices: 3,
                address: 0,
            },
            ..auto
        };
        assert_eq!(
            draw_geometry(&indexed, registers).map(|g| (g.first_vertex, g.indices.is_some())),
            Ok((5, true)),
            "each fetched index has 5 added"
        );
    }

    /// A draw's geometry-engine inputs are seeded for a non-indexed, single-instance list or strip
    /// of triangles, without passthrough or an index offset (D730); anything else says why not.
    /// A draw with no primitive shader runs a legacy vertex shader (D881): its geometry says so,
    /// and that its stores are streamout where its `RSRC2_VS` sets `SO_EN`. A primitive shader's
    /// draw is neither; nor is one whose stream never wrote the stage register.
    #[test]
    fn a_draw_with_no_primitive_shader_is_a_legacy_vertex_draw() {
        use super::{DI_PT_TRILIST, VGT_PRIMITIVE_TYPE, VGT_SHADER_STAGES_EN, draw_geometry};
        use crate::registers::{DrawCall, DrawKind};
        let draw = DrawCall {
            packet_offset: 0,
            instances: 1,
            kind: DrawKind::Auto { vertices: 3 },
        };
        let geometry = |stages: Option<u32>, rsrc2: Option<u32>| {
            let found = draw_geometry(&draw, |register| match register {
                VGT_PRIMITIVE_TYPE => Some(DI_PT_TRILIST),
                VGT_SHADER_STAGES_EN => stages,
                0x2C4B => rsrc2,
                _ => None,
            })
            .expect("a triangle list is seeded");
            (found.legacy, found.streamout)
        };
        // GCTS00001's streamout pass: VS_W32_EN and MAX_PRIMGRP_IN_WAVE, no PRIMGEN_EN.
        assert_eq!(geometry(Some(0x0081_0000), Some(0x1112)), (true, true));
        assert_eq!(geometry(Some(0x0081_0000), Some(0x0012)), (true, false));
        // A primitive shader (PRIMGEN_EN), and a stream that never wrote the register.
        assert_eq!(geometry(Some(0x0041_2010), Some(0x1112)), (false, false));
        assert_eq!(geometry(None, Some(0x1112)), (false, false));
    }

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
                legacy: false,
                streamout: false,
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
                draw_geometry(&draw(6, 1), strip(mode)).map(|g| (g.primitives, g.assembly)),
                Ok((4, super::Assembly::Strip { provoking_last }))
            );
        }
        assert_eq!(
            draw_geometry(&draw(30, 1), registers(DI_PT_TRILIST, 0, 0)).map(|g| g.primitives),
            Ok(10)
        );
        // Instances are each a workgroup of their own (D863), seeded alike.
        assert_eq!(
            draw_geometry(&draw(3, 2), registers(DI_PT_RECTLIST, 0, 0)),
            draw_geometry(&draw(3, 1), registers(DI_PT_RECTLIST, 0, 0))
        );
        let fan = draw_geometry(&draw(3, 1), registers(5, 0, 0));
        assert!(fan.is_err(), "{fan:?}");
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
    /// of six vertices is three lines, a line strip of four three too. A point list of three is
    /// three points, a vertex each.
    #[test]
    fn a_line_list_and_strip_are_seeded() {
        use super::{
            Assembly, DI_PT_LINELIST, DI_PT_LINESTRIP, DI_PT_POINTLIST, VGT_PRIMITIVE_TYPE,
            draw_geometry,
        };
        use crate::registers::{DrawCall, DrawKind};
        for (topology, vertices, assembly) in [
            (DI_PT_LINELIST, 6, Assembly::LineList),
            (DI_PT_LINESTRIP, 4, Assembly::LineStrip),
            (DI_PT_POINTLIST, 3, Assembly::Points),
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
            legacy: false,
            streamout: false,
        };
        assert!(chunk_plans(whole(63, 21, Assembly::List, None), None, 64).is_empty());
        // 96 vertices of a list at 64 lanes: 21 triangles, then 11.
        let list = chunk_plans(whole(96, 32, Assembly::List, None), None, 64);
        assert_eq!(
            list,
            [
                ChunkPlan {
                    geometry: whole(63, 21, Assembly::List, None),
                    index_buffer: IndexBuffer::Counting { first: 0 },
                },
                ChunkPlan {
                    geometry: whole(33, 11, Assembly::List, None),
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

    /// A non-indexed split draw travels as one command (D787) that names the chunks D741 drew
    /// separately, and expands back into exactly those draws; an indexed draw's chunks, each with
    /// its own index range, stay separate.
    #[test]
    fn a_split_draw_travels_as_one_command_and_expands_to_its_chunks() {
        use super::{Chunk, IndexBuffer, push_chunks};
        use crate::backend::{RenderCommand, ShaderStage};
        let module = ResourceId(7);
        let counting = |first, indices| Chunk {
            vertex: module,
            indices,
            index_buffer: IndexBuffer::Counting { first },
        };
        let chunks = [counting(10, 63), counting(73, 63), counting(136, 33)];
        let mut commands = Vec::new();
        push_chunks(&mut commands, &chunks, 2, &mut Vec::new());
        assert_eq!(
            commands,
            [
                RenderCommand::BindShader {
                    stage: ShaderStage::Vertex,
                    shader: module,
                },
                RenderCommand::DrawChunks {
                    chunks: 3,
                    vertices: 63,
                    last: 33,
                    instances: 2,
                    first_vertex: 10,
                    stride: 63,
                },
            ]
        );
        let draw = |vertices, first_vertex| RenderCommand::Draw {
            vertices,
            instances: 2,
            first_vertex,
        };
        assert_eq!(
            commands[1].chunk_draws().collect::<Vec<_>>(),
            [draw(63, 10), draw(63, 73), draw(33, 136)]
        );

        let guest = |address| Chunk {
            vertex: module,
            indices: 32,
            index_buffer: IndexBuffer::Guest { address, bytes: 2 },
        };
        let mut indexed = Vec::new();
        push_chunks(
            &mut indexed,
            &[guest(0x1000), guest(0x1040)],
            1,
            &mut Vec::new(),
        );
        assert_eq!(
            indexed
                .iter()
                .filter(|c| matches!(c, RenderCommand::DrawIndexed { .. }))
                .count(),
            2
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
