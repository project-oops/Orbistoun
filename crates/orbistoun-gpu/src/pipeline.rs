//! From a submitted command buffer to shaders ready to run.
//!
//! # Why this is the shape a console emulator has
//!
//! Nothing here is ever called by the emulator on the guest's behalf. The guest builds
//! a command buffer, writes shader addresses into hardware registers, and submits. That
//! submission is the only input, and everything the frame will do is derivable from it
//! and from guest memory. There is no high-level graphics call to intercept - on this
//! generation the guest talks to the hardware, so a translator that waited for one
//! would wait forever.
//!
//! So the direction of control is: walk the packets, read the registers the guest set,
//! find the shader addresses among them, fetch those shaders *out of guest memory*,
//! translate them, and emit backend commands. Each step is driven by what the guest
//! did, not by what this crate expects.
//!
//! # What was missing before this module
//!
//! Every piece of that already existed and none of them touched. `walk` decoded packets,
//! `shader_candidates` found addresses, `orbistoun-translate` turned shader bytes into
//! SPIR-V, and `RenderCommand` described what a backend should do. A shader translator
//! nothing calls is a library, not a subsystem, and it cannot be wrong in any way a test
//! would notice - because the only shaders it ever saw were the ones its own tests
//! handed it.
//!
//! # The address in a register is a *GPU* address
//!
//! Everything here reads a shader at the address a hardware register named, through
//! [`GuestMemory`], which reads the guest's address space. Those are two different
//! address spaces and this treats them as one.
//!
//! The loader side has established that a guest virtual address is the host address -
//! an identity mapping, and load-bearing. Whether a *GPU* virtual address is also that
//! same number is **not** established. The console has one coherent memory pool shared
//! by both processors, which is the reason to expect it; expecting is not knowing.
//!
//! [`guest_address_of`] is the one place that assumption lives. It is the identity
//! function today and exists so that when the answer arrives it is a single edit rather
//! than a search. If the assumption is wrong the failure is loud - the read finds
//! nothing mapped, or finds bytes that do not decode - which is the right shape for a
//! guess to fail in.
//!
//! # Guest memory is a trait
//!
//! A shader lives at a guest virtual address, so this needs to read guest memory - and
//! must not depend on the address space to do it. [`GuestMemory`] is the whole contract:
//! one fallible read. That keeps this crate testable with a fake and keeps the address
//! space free to change.
//!
//! # Shaders are cached by content
//!
//! A guest rebinds the same shader every draw, often thousands of times a frame.
//! Translating it each time would dominate the cost of everything else here. The cache
//! is keyed on the **bytes**, not on the address, because a guest is entitled to move a
//! shader, to write a different one to the same address, and to have two addresses hold
//! the same shader. An address-keyed cache is wrong in all three cases and right in the
//! common one, which is the worst combination available.
//!
//! # Two ways a shader address arrives, and which one is believed
//!
//! The guest's graphics layer is a command-buffer *builder*: library calls append packets
//! to a buffer the guest owns, and a separate call submits it. So a shader can be learned
//! about twice - once because the guest asked the library to register it, and once
//! because a register write in the submitted packets points at it.
//!
//! A [`RegisteredShader`] is believed over the register writes where the two overlap. Not
//! because registration is more fundamental - the packets are what the hardware executes
//! and the guest can hand-roll or patch a buffer without the library - but because
//! registration is *stated* and the register path is *inferred*, and the table that
//! inference rests on is the least verified thing in this crate.
//!
//! Neither replaces the other. A submission with no registrations still finds its shaders
//! the hard way, and the report says which route found what - because the two disagreeing
//! is the most useful signal available about whether that table is right.
//!
//! # A shader that will not translate is reported, never skipped
//!
//! It does not become a no-op and it does not stop the submission. The command referring
//! to it is omitted, the failure is recorded with its address and its reason, and
//! [`Submission::report`] carries it out. A frame missing a draw is visible; a frame
//! where a draw silently drew nothing is a bug somebody chases for a week.

use std::collections::BTreeMap;

use orbistoun_shader::{
    Capture, EncodingTable, Operand, OperandTable, ShaderCorpus, ShaderError, decode_program,
};
use orbistoun_translate::wavefront::MeshPrimitive;
use orbistoun_translate::wavefront::Stage;
use orbistoun_translate::wavefront::Window;
use orbistoun_translate::wavefront::{TextureSource, USER_DATA_STAGE_WORDS, UserData};
use orbistoun_translate::{Strategy, Width, translate_with_user_data};

use crate::backend::{Rect, RenderCommand, ResourceId, ShaderStage, USER_DATA_WORDS};
use crate::packet::{PacketWalk, walk};
use crate::registers::{
    BlendControl, ColourTarget, ColourTargetExtent, ColourTargetFormat, DepthControl, DrawCall,
    DrawKind, ImageDescriptor, PrimitiveTopology, RegisterWrite, StencilControl, SwizzleMode,
    Vocabulary, WaveWidths, blend_control_at, colour_swizzle_mode_at, colour_target_at,
    colour_target_bases_in, colour_target_extent_at, colour_target_format_at, decode_blend_control,
    decode_image_descriptor, depth_control_at, dispatch_calls, draw_calls, primitive_topology_at,
    register_writes, scissor_at, shader_candidates, stencil_control_at, viewport_transform_from,
};

/// Which queue a command buffer was submitted to.
///
/// The guest has two, and they take different work: one draws and one dispatches compute.
/// Which one a buffer came from decides which shader stages can legitimately appear in
/// it, so a vertex shader in a compute submission is a decode that went wrong rather than
/// an unusual frame.
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
    /// Used to report a stage that cannot belong, rather than to silently drop it: a
    /// vertex shader named by a compute submission means the register vocabulary
    /// mis-identified something, and that is worth surfacing rather than filtering.
    pub const fn permits(self, stage: ShaderStage) -> bool {
        match self {
            Self::Draw => matches!(stage, ShaderStage::Vertex | ShaderStage::Fragment),
            Self::Compute => matches!(stage, ShaderStage::Compute),
        }
    }
}

/// The host stage a shader named by the register vocabulary is translated for.
///
/// # Vertex is the mesh stage
///
/// There is no host vertex stage for what the guest calls one: a shader bound to the vertex
/// slot on this hardware is an NGG primitive shader, which declares how much it will emit and
/// emits both the primitive and its vertices - a mesh shader (D688, worklog 558). It was
/// attempted as a compute dispatch until the mesh stage existed, which reported the first
/// instruction it could not translate rather than a policy.
///
/// The stage itself is the typed one the submission reconciled, not the vocabulary's string:
/// the table's spelling is data and this is a dispatch, and the two should not be the same
/// thing.
const fn host_stage(stage: ShaderStage) -> Stage {
    match stage {
        ShaderStage::Fragment => Stage::Fragment,
        ShaderStage::Vertex => Stage::Mesh,
        ShaderStage::Compute => Stage::Compute,
    }
}

/// Distinguishes the same shader translated for different stages, in the cache key.
///
/// An arbitrary constant per stage, mixed into the content hash. It has to be *stable* across
/// runs, because a report is diffed, and it has to differ per stage - nothing else about it
/// matters.
const fn stage_salt(stage: Stage) -> u64 {
    match stage {
        Stage::Compute => 0,
        Stage::Fragment => 0x5352_4746_0000_0001,
        Stage::Mesh => 0x4d45_5348_0000_0001,
    }
}

/// Distinguishes the same shader translated for different mesh primitives, in the cache key.
///
/// A mesh module's output shape is in the module (D688), so a point and a triangle built from
/// the same bytes are different modules and must not share a cache entry (`-0c58`).
const fn primitive_salt(primitive: MeshPrimitive) -> u64 {
    match primitive {
        MeshPrimitive::Points => 0x504f_494e_0000_0001,
        MeshPrimitive::Lines => 0x4c49_4e45_0000_0001,
        MeshPrimitive::Triangles => 0,
    }
}

/// The mesh primitive a decoded topology asks the translator to assemble, or a refusal naming a
/// topology that has no mesh-primitive shape (`-0c58`).
///
/// A point, line and triangle map onto the three the mesh extension emits. A rectangle list and
/// any unknown value are **refused by name** rather than drawn as a triangle: assembling a shape
/// the stream did not ask for is the plausible-output principle 3 forbids, at the last step
/// before a picture.
fn mesh_primitive_of(topology: PrimitiveTopology) -> Result<MeshPrimitive, String> {
    match topology {
        PrimitiveTopology::PointList => Ok(MeshPrimitive::Points),
        PrimitiveTopology::LineStrip => Ok(MeshPrimitive::Lines),
        PrimitiveTopology::TriangleStrip => Ok(MeshPrimitive::Triangles),
        other => Err(format!(
            concat!(
                "the draw's primitive topology is {}, which has no mesh-primitive shape to ",
                "assemble - a point, line or triangle is emitted, and this is refused rather ",
                "than drawn as one of them"
            ),
            other.label()
        )),
    }
}

/// Distinguishes the same shader translated against different memory windows, in the cache key.
///
/// **The same reason the stage is in there.** A window's base and length are compiled into the
/// module - the base is subtracted before an index is masked, and the length is the mask - so
/// two windows produce two different modules from one shader. A cache that ignored the window
/// would serve whichever was translated first, and the shader would read the wrong memory while
/// everything about the run looked fine.
///
/// Both halves, because a window that moved and a window that grew are both different windows.
/// The strategy a stage's shader is translated with, at the wave width the stream declared for that
/// stage (D141), and the cache-key salt that width contributes.
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

const fn window_salt(window: Window) -> u64 {
    // The full address, rotated so the length lands in bits a low-half base does not reach; two
    // windows that differ only in their high half (D711) are different windows.
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
/// The only thing this module needs from the address space, kept to one method so a test
/// can supply a `Vec<u8>` and a real emulator can supply the mapped guest pages.
pub trait GuestMemory {
    /// Bytes at a guest virtual address, or `None` if the range is not mapped.
    ///
    /// Returning `None` rather than a short read or zeros is deliberate: a shader read
    /// from unmapped memory is not a short shader, it is a wrong address, and zeros
    /// decode into a plausible instruction stream.
    fn read(&self, address: u64, length: usize) -> Option<&[u8]>;
}

/// The largest shader this will read out of guest memory.
///
/// A shader carries no length. The decoder finds the end by reaching the instruction
/// that stops the program, so this is the window it is allowed to look in - not a claim
/// about how big shaders are. Reading past the real end is harmless because decoding
/// stops at the terminator; reading past the end of a *mapping* is not, which is why the
/// read narrows until it succeeds rather than demanding the whole window.
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

/// What a submission turned out to contain.
///
/// Counts first, because counts are what decide where effort goes - the same argument
/// the import survey made for the operating system, one layer down.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SubmissionReport {
    /// Packets the walk recognised.
    pub packets: usize,
    /// Register writes extracted from them.
    pub register_writes: usize,
    /// Draws the stream asked for.
    ///
    /// Auto-indexed draws only. An indexed one takes its count from separate state and an index
    /// buffer in guest memory, and neither is read - so a stream full of indexed draws reports
    /// none here, which is the honest answer rather than a guess at how many.
    pub draws: usize,
    /// The primitive the draw's geometry produces, from `VGT_GS_OUT_PRIM_TYPE` (`-0c58`).
    ///
    /// `None` when the stream set it nowhere. Carried so a reader - and a backend - can tell a point
    /// draw from a triangle one, which the input-assembly register cannot: it reads `TRILIST` for
    /// both. The backend's mesh output still emits triangles unconditionally, so this is decoded and
    /// reported before it is drawn with; making the mesh output follow it is the rest of `-0c58`.
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
    /// Stages where both routes produced an address and the two matched.
    ///
    /// Evidence *for* the register vocabulary, and the only kind available.
    pub agreed: usize,
    /// Stages where both produced an address and they differed.
    ///
    /// Evidence against it, and the most useful line in this report: the registered
    /// address is what the guest said, so a mismatch means the register table found the
    /// wrong bits. Nothing else in this crate can tell you that.
    pub disagreed: Vec<Disagreement>,
    /// Stages named by a route that the queue does not permit.
    ///
    /// A vertex shader in a compute submission is a decode that went wrong. Reported
    /// rather than filtered, for the same reason.
    pub impossible_stages: Vec<String>,
    /// Every shader that did not, and why.
    pub failures: Vec<ShaderFailure>,
    /// Addresses a register named that guest memory recognised.
    ///
    /// # What this is evidence *for*
    ///
    /// An address in a command stream is a **GPU** virtual address. Guest memory is
    /// indexed by a guest one. Everything here reads the first as the second on the
    /// assumption they are the same number, and nobody has confirmed that - it is the
    /// open half of D101.
    ///
    /// Every address that resolves is a data point saying they coincided at least there,
    /// and every one that does not is a data point against. Counted apart from the shader
    /// outcome on purpose: an address can resolve perfectly and its shader still fail to
    /// translate, and folding the two together would lose exactly the signal this exists
    /// to collect.
    ///
    /// It is weak evidence per address and strong in aggregate. A run where every address
    /// resolves is a run where the assumption held every time it was tested.
    pub addresses_resolved: usize,
    /// Addresses a register named that guest memory did not recognise.
    ///
    /// Two causes, and they want different responses: the register decode found the wrong
    /// bits, or a GPU address is not a guest address. The failure message says which to
    /// suspect first, and this count says how often it happens.
    pub addresses_unresolved: usize,
    /// Shaders that translated, and something about them worth knowing.
    ///
    /// Separate from `failures` because these *worked*. The one that exists today is a
    /// shader that had to be translated at the slowest fidelity, which is correct and
    /// costs a factor of sixty four - a thing a caller should be told rather than left
    /// to find in a field.
    pub warnings: Vec<String>,
    /// Every distinct texture the draws' pixel shaders name, in first-use order (worklog 827).
    ///
    /// Read the way the shader reads it: the fragment stage's user data words 0 and 1 are its
    /// descriptor table, and the image descriptor is the table's first eight words (the GL context's
    /// textured shader loads them with `s_load_dwordx8 s[4:11], s[0:1], 0x00`). What a backend needs
    /// to bind a draw's real texture - size, format, tiling, where the texels are - measured before
    /// anything is bound, because which of these the detiler can already serve decides what comes
    /// next.
    pub textures: Vec<ImageDescriptor>,
}

impl SubmissionReport {
    /// This submission in the shader work's progress vocabulary.
    ///
    /// # Why not a second set of counters
    ///
    /// The shader corpus already reports `FURTHER` / `same` / `BACK` against the previous
    /// run (D148), and a submission is the same question asked of shaders that arrived
    /// from a guest rather than from a directory. Given its own progress format it would
    /// drift from that one immediately, and a reader would have to learn which numbers
    /// meant what depending on where the shaders came from.
    ///
    /// So it converts. A submission's "complete" is a shader that translated, because
    /// that is the same claim the corpus makes: every instruction in it is understood.
    ///
    /// Blockers are the failures' reasons rather than instruction names - a submission
    /// fails for reasons a corpus never sees, like an address that does not resolve, and
    /// flattening those into instruction names would hide the ones worth acting on.
    pub fn summary(&self) -> orbistoun_shader::coverage::Summary {
        orbistoun_shader::coverage::Summary {
            complete: self.shaders_translated,
            // A submission has always counted shaders a translator actually ran over -
            // that is what `shaders_translated` is - so it is comparable with a corpus run
            // that does the same and not with one that counted supported opcodes.
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
    /// names, to their dimensions. A target carries no bytes - only the size a backend allocates its
    /// attachment to - so it rides here rather than in [`Self::modules`], and the driver makes it
    /// resident the same way (D701).
    pub targets: BTreeMap<ResourceId, ColourTargetExtent>,
    /// Colour target zero's full record - base address and extent - decoded from the stream
    /// (`CB_COLOR0_BASE` + `ATTRIB2`, worklog 655). A backend needs the base to find the attachment in
    /// guest memory; the extent it also holds mirrors the entry in [`Self::targets`]. `None` when the
    /// stream set neither register.
    pub colour_target: Option<ColourTarget>,
    /// Colour target zero's tiling mode (`CB_COLOR0_ATTRIB3`'s `COLOR_SW_MODE`, worklog 657). A backend
    /// detiles a `Tiled64KbRX` attachment and reads a `Linear` one straight. `None` when the stream set
    /// no mode.
    pub colour_target_tiling: Option<SwizzleMode>,
    /// Colour target zero's element layout (`CB_COLOR0_INFO`, worklog 832) - the byte order a frame
    /// written back into it must use. `None` when the stream set none.
    pub colour_target_format: Option<ColourTargetFormat>,
    /// How many distinct colour target zero bases the stream wrote (worklog 832): one frame written
    /// back for a submission is honest only when its draws all went to one target.
    pub colour_target_bases: usize,
    /// The depth- and stencil-test state a draw runs under (`DB_DEPTH_CONTROL`, worklog 669). `None`
    /// when the stream set none - a draw with no depth control has no depth test, not an assumed one.
    pub depth_control: Option<DepthControl>,
    /// The stencil operations a draw applies on each test outcome (`DB_STENCIL_CONTROL`, worklog 670).
    /// `None` when the stream set none.
    pub stencil_control: Option<StencilControl>,
    /// Colour target zero's blend state (`CB_BLEND0_CONTROL`, worklog 671). `None` when the stream set
    /// none.
    pub blend_control: Option<BlendControl>,
    /// The guest-memory window this submission's shaders read, as words, read out of guest memory at
    /// the pipeline's window (D703). Frame-level rather than per-draw, because every module is
    /// compiled against one window (worklog 635). Empty when the window is not mapped - a stream
    /// against the default window at address zero reads nothing, and a real one reads its region.
    pub guest_memory: Vec<u32>,
    /// The guest address [`Self::guest_memory`]'s first word was read from (worklog 836) - what a
    /// diagnostic needs to find an address the shaders form inside the window.
    pub guest_memory_base: u64,
    /// What was seen and what failed.
    pub report: SubmissionReport,
}

/// Why a shader could not be prepared, and whether its *address* was the problem.
///
/// The split is the whole point. An address that guest memory does not recognise is
/// evidence about the address space; anything after that is evidence about the shader.
/// Reporting them as one string loses the first, which is the only measurement available
/// for the open half of D101.
#[derive(Debug)]
enum PrepareFailure {
    /// Guest memory has nothing at the address the register named.
    Unresolved(String),
    /// The address resolved and something after it did not.
    Resolved(String),
}

impl PrepareFailure {
    /// The reason, for a report.
    fn reason(self) -> String {
        match self {
            Self::Unresolved(reason) | Self::Resolved(reason) => reason,
        }
    }
}

/// A translated shader, and enough to notice if the key ever lied.
///
/// # Why anything beyond the resource is kept
///
/// The cache key is a sixty-four-bit hash of the shader's bytes. A *collision* is not the
/// worry - at this width, with a few thousand shaders, that probability is around one in
/// a million million. The worry is that the key stops meaning what it did.
///
/// It is computed over `decoded.consumed`, which is the *decoder's* idea of where the
/// shader ends, not a property of the guest's bytes alone. That number has already
/// changed once: the decoder used to stop at the first end-of-program instruction and now
/// stops at the padding past the end, so the same shader keys differently before and
/// after. A hit is served without ever looking at the bytes again, so a key that has
/// quietly changed meaning serves a stale module and the report says nothing at all.
///
/// Keeping the length and the first and last words turns that from silent into loud, for
/// a few comparisons per bind. It is not a full re-compare and is not meant to be: it is
/// there to catch the key meaning something different, not to catch an adversary.
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
    /// Padded rather than answered as zero. Reading past the end returned zero for *any*
    /// short shader, so two different ones of the same short length compared equal - the
    /// exact "two shaders satisfy one entry" fault this type exists to prevent, hiding at
    /// the one end nobody thinks about. A shader shorter than a word is not a real shader,
    /// but `read_window` narrows rather than refusing, so one can reach here.
    fn word(bytes: &[u8], at: usize) -> u32 {
        let mut packed = [0u8; 4];
        let available = bytes.get(at..).unwrap_or_default();
        let take = available.len().min(4);
        packed[..take].copy_from_slice(&available[..take]);
        u32::from_le_bytes(packed)
    }
}

/// Translates submissions, remembering shaders between them.
///
/// Long-lived on purpose: the cache is the point, and a frame's worth of submissions
/// binds the same handful of shaders over and over.
#[derive(Debug)]
pub struct Pipeline {
    encodings: EncodingTable,
    operands: OperandTable,
    vocabulary: Vocabulary,
    strategy: Strategy,
    /// The span of guest memory a translated shader may reach, and where it starts.
    ///
    /// **Named by the caller, not derived from the submission, and that is the open half.**
    /// Every memory access a translated shader makes is checked against this; anchored in the
    /// wrong place, every one is refused and the shader draws nothing (worklog 561). A window
    /// placed where a record says the buffers were is what got the console's own shaders
    /// fetching vertices and landing a store (worklog 570).
    ///
    /// Deriving one from the command stream needs a register vocabulary that says which
    /// register carries a buffer address, and nothing has measured that - so the default is
    /// the default, and a caller that knows can say. Inventing the mapping instead would
    /// produce a plausible frame drawn from the wrong memory, which is what the vocabulary
    /// exists to prevent (`REQ-20260914T2348Z-4e71`).
    window: Window,
    /// Whether each submission places the window itself, from the constant base its geometry
    /// shader forms (D711) - set by [`Self::placing_window_from_shaders`] for a live guest, whose
    /// shaders name their own buffers; off for a caller that places the window by hand.
    places_window: bool,
    /// Whether translated shaders read their stage's user data at entry (worklog 826) - set by
    /// [`Self::feeding_user_data`] for a backend that supplies it per draw; off for callers whose
    /// modules must stay as they were.
    feeds_user_data: bool,
    /// Each stage's user-data layout for the submission being prepared, vertex then fragment.
    user_data: [UserData; 2],
    /// Shader bytes, by content hash, to the resource holding the translation.
    cache: BTreeMap<u64, Cached>,
    /// The bytes each shader address decoded to, end-of-program included.
    decoded: BTreeMap<u64, Vec<u8>>,
    /// The constant base each vertex program address formed (D711), with the program bytes it was
    /// found in.
    bases: BTreeMap<u64, (Vec<u8>, Option<u64>)>,
    /// Each translated module's texture sources, by its resource (worklog 840).
    texture_sources: BTreeMap<ResourceId, Vec<TextureSource>>,
    /// Texels read from guest memory, shared across submissions while their bytes are unchanged
    /// (worklog 851).
    texels: TexelCache,
    next_resource: u64,
}

impl Pipeline {
    /// Builds a pipeline over the built-in tables.
    ///
    /// # Errors
    ///
    /// If a built-in table does not load, which is a build fault rather than anything
    /// about a guest.
    pub fn new(strategy: Strategy) -> Result<Self, PipelineError> {
        Ok(Self {
            encodings: EncodingTable::builtin().map_err(|e| PipelineError::Table(e.to_string()))?,
            operands: OperandTable::builtin().map_err(|e| PipelineError::Table(e.to_string()))?,
            vocabulary: Vocabulary::builtin().map_err(|e| PipelineError::Table(e.to_string()))?,
            strategy,
            window: Window::default(),
            places_window: false,
            feeds_user_data: false,
            user_data: [UserData::default(); 2],
            cache: BTreeMap::new(),
            decoded: BTreeMap::new(),
            bases: BTreeMap::new(),
            texture_sources: BTreeMap::new(),
            texels: TexelCache::new(),
            next_resource: 1,
        })
    }

    /// Has every submission place the window at the constant 64-bit base its geometry shader forms
    /// for its memory accesses, and leaves the window where it was when the shader forms none (D711).
    ///
    /// For a live guest, whose shaders name their own buffers in their own words. A caller that
    /// places the window by hand - every capture test, which relocates a vertex buffer - leaves this
    /// off and keeps the window it chose.
    #[must_use]
    pub const fn placing_window_from_shaders(mut self) -> Self {
        self.places_window = true;
        self
    }

    /// Has every shader read its stage's user data at entry, from the push-constant block the
    /// backend fills per draw from each [`RenderCommand::SetUserData`] (worklog 826).
    ///
    /// For the live path, whose backend supplies the block. A caller whose backend does not leaves
    /// this off, and its modules read no block and are word for word what they were.
    #[must_use]
    pub const fn feeding_user_data(mut self) -> Self {
        self.feeds_user_data = true;
        self
    }

    /// Places the guest-memory window every shader this pipeline translates is built against.
    ///
    /// The default is at address zero, which refuses every access a real guest shader makes -
    /// a guest's buffers are not there. A caller that knows where they are says so here.
    ///
    /// **It clears the cache**, because the cached modules were built against the old window
    /// and are not the modules this one would produce. The key carries the window too, so the
    /// clear is belt and braces rather than the mechanism - but a cache holding entries no key
    /// will ever match again is just memory nobody can reach.
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

    /// Submits a command buffer that lives in **guest memory**.
    ///
    /// # Why this exists as well as [`Pipeline::submit`]
    ///
    /// `submit` takes bytes, which is what a test has. A guest has an *address and a
    /// length*: it builds a command buffer somewhere in its own memory and then calls the
    /// vendor's submit function with a pointer to it. This is the shape that call site
    /// needs, and it is deliberately the whole of what the shim above has to know - read
    /// the arguments, hand them here.
    ///
    /// **Nothing calls this yet**, and that is a statement about the loader rather than
    /// about this crate: no guest has reached a submission. The entry point exists so that
    /// when one does, the work is wiring a shim to a function rather than designing an
    /// interface under time pressure - and so the address the guest passes is measured on
    /// its way in, the same way every shader address already is.
    ///
    /// Answers `None` when the command buffer itself is not readable. That is the same
    /// evidence a shader address gives about D101, one level earlier: the pointer came
    /// from the guest's own CPU-side code, so if *that* does not resolve, the fault is in
    /// the shim's arguments rather than in any assumption about GPU addresses.
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
    /// Never fails on the guest's account. A command buffer full of packets nobody
    /// understands produces an empty command list and a report saying so, which is the
    /// honest answer and the one a worklist can act on.
    ///
    /// `registered` is what the guest told the graphics library about, if anything. It is
    /// believed where it overlaps with what the register writes imply, and the report
    /// records where the two disagreed.
    pub fn submit(
        &mut self,
        stream: &[u8],
        queue: Queue,
        registered: &[RegisteredShader],
        memory: &impl GuestMemory,
    ) -> Submission {
        use crate::perf::{Span, span};
        let walked = span(Span::PrepareWalk, || walk(stream));
        let writes = span(Span::PrepareRegisters, || {
            register_writes(&walked, stream, &self.vocabulary)
        });
        let inferred = shader_candidates(&writes, &self.vocabulary);

        let mut submission = Submission {
            report: SubmissionReport {
                packets: walked.packets.len(),
                register_writes: writes.len(),
                ..SubmissionReport::default()
            },
            ..Submission::default()
        };

        // **The colour target, before anything that draws into it.** A stream sizes its target with
        // a register write; decoding that (worklog 637) lets a backend allocate a real attachment
        // rather than a guessed square. Emitted first, because it is state a draw reads. The `targets`
        // map is still keyed by extent - the size register is corroborated by both sources where the
        // base once was not (D702) - but the base *is* decoded now (`CB_COLOR0_BASE`, worklog 655) and
        // rides on `colour_target` below, so a backend can find the attachment in guest memory.
        if let Some(extent) = colour_target_extent_at(&writes) {
            let target = colour_target_id(extent);
            submission.targets.insert(target, extent);
            submission.commands.push(RenderCommand::SetRenderTargets {
                colour: vec![target],
                depth: None,
            });
        }

        // The pipeline state a draw runs under, decoded from the same stream and carried for a backend
        // to build its pipeline from: colour target zero's base and tiling (worklogs 655/657), and the
        // depth, stencil and blend state (worklogs 669/670/671). Each is `None` when the stream set its
        // register nowhere - the state is read, never assumed, so a draw with no depth control has no
        // depth test rather than a default one.
        submission.colour_target = colour_target_at(&writes);
        submission.colour_target_tiling = colour_swizzle_mode_at(&writes);
        submission.colour_target_format = colour_target_format_at(&writes);
        submission.colour_target_bases = colour_target_bases_in(&writes);
        submission.depth_control = depth_control_at(&writes);
        submission.stencil_control = stencil_control_at(&writes);
        submission.blend_control = blend_control_at(&writes);
        // The primitive the draw produces, read from `VGT_GS_OUT_PRIM_TYPE` - a point draw and a
        // triangle draw are told apart here, and `prepare` threads this into the mesh module's
        // output shape (points, lines or triangles), so the two no longer render alike (`-0c58`).
        submission.report.primitive_topology = primitive_topology_at(&writes);
        // And how wide each stage's wave is: the encodings are identical at either width, so the
        // stream's registers are the only place it is said (D141).
        submission.report.wave_widths = crate::registers::wave_widths_at(&writes);

        // The scissor a stream set, as a viewport the backend restricts a draw to (worklog 646). The
        // generic scissor's corners are decoded (register offsets and layout mined from a hardware
        // draw capture) into the command's rectangle; a stream that set no scissor emits none, so a
        // draw covers the whole target.
        if let Some(scissor) = scissor_at(&writes) {
            submission.commands.push(RenderCommand::SetViewport(Rect {
                x: i32::try_from(scissor.x).unwrap_or(0),
                y: i32::try_from(scissor.y).unwrap_or(0),
                width: scissor.width,
                height: scissor.height,
            }));
        }

        // The guest-memory window this frame's shaders read, out of guest memory at the pipeline's
        // window (D703). Read here, once, because every module shares the one window; a shader
        // fetches its vertices from it. The default window at address zero is unmapped and reads
        // nothing, so a stream that never placed its window carries an empty region rather than a
        // guessed one - the honest answer, and what a shader reading it would find.
        let candidates = Self::reconcile(queue, registered, &inferred, &mut submission.report);
        span(Span::PrepareEnvironment, || {
            self.set_environment(&candidates, &writes, memory);
        });
        submission.guest_memory = span(Span::PrepareWindow, || {
            read_guest_window(memory, self.window)
        });
        submission.guest_memory_base = self.window.address();
        submission.report.shaders_found = candidates.len();

        // The draws, found once for both passes that walk them.
        let draws = draw_calls(&walked, stream);
        let per_draw = span(Span::PrepareShaders, || {
            self.bind_shaders(
                &candidates,
                (queue, registered),
                (&draws, &writes),
                memory,
                &mut submission,
            )
        });

        // **The draws and dispatches, after the binds.** A stream sets its state and then asks for
        // geometry, so emitting them in that order is what a backend can act on - and the order is
        // the stream's own, from the packet walk rather than from anything this function decides.
        span(Span::PrepareGeometry, || {
            push_geometry_commands(
                &mut submission.commands,
                (&walked, stream, &draws),
                &writes,
                &per_draw,
            );
        });
        span(Span::PrepareTextures, || {
            submission.report.textures = texture_census(&submission.commands, memory);
            if self.feeds_user_data {
                bind_textures(
                    &mut submission.commands,
                    (&self.texture_sources, &mut self.texels),
                    memory,
                );
            }
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
    /// Prepares the reconciled candidates and binds them, then works out **the shader each draw ran**
    /// (worklog 835): the program addresses in force at its packet, translated where the first pass
    /// did not already, so a stream that swaps a stage's program between draws binds each draw's
    /// own. A stage the guest registered keeps its registration. Returns each draw's shaders, in draw
    /// order.
    fn bind_shaders(
        &mut self,
        candidates: &[Candidate],
        (queue, registered): (Queue, &[RegisteredShader]),
        (draws, writes): (&[DrawCall], &[RegisterWrite]),
        memory: &impl GuestMemory,
        submission: &mut Submission,
    ) -> Vec<Vec<(ShaderStage, ResourceId)>> {
        // Every (stage, address) prepared so far, and what it became - `None` for one that failed, so
        // it is tried and reported once however many draws name it.
        let mut prepared: BTreeMap<(u32, u64), Option<ResourceId>> = BTreeMap::new();
        let registered_stages: Vec<ShaderStage> = candidates
            .iter()
            .filter(|c| {
                registered
                    .iter()
                    .any(|r| r.address == c.address && r.stage == c.stage)
            })
            .map(|c| c.stage)
            .collect();
        for &candidate in candidates {
            let resource = self.prepare_candidate(candidate, memory, submission);
            prepared.insert((candidate.stage as u32, candidate.address), resource);
            if let Some(resource) = resource {
                submission.commands.push(RenderCommand::BindShader {
                    stage: candidate.stage,
                    shader: resource,
                });
            }
        }

        let mut per_draw = Vec::new();
        // One pass over the stream's writes for all its draws, not one per draw (worklog 844).
        let mut sweep = crate::registers::RegisterSweep::new(writes);
        let shader_registers: Vec<u32> = self.vocabulary.shader_register_ids().collect();
        // A draw whose shader registers hold the same values as the previous draw's runs the same
        // shaders - which stage runs what is those values and nothing else - and a GL frame
        // rewrites one pair's addresses unchanged before each of thousands of draws.
        let mut previous_writes: Option<Vec<Option<u32>>> = None;
        let mut previous_shaders = Vec::new();
        for draw in draws {
            let written: Vec<Option<u32>> = shader_registers
                .iter()
                .map(|&register| sweep.latest(draw.packet_offset, register))
                .collect();
            if previous_writes.as_ref() == Some(&written) {
                per_draw.push(Vec::clone(&previous_shaders));
                continue;
            }
            let mut shaders = Vec::new();
            let latest =
                sweep.before_among(draw.packet_offset, self.vocabulary.shader_register_ids());
            for inferred in shader_candidates(&latest, &self.vocabulary) {
                let Some(stage) = stage_of(&inferred.stage) else {
                    continue;
                };
                if !queue.permits(stage) || registered_stages.contains(&stage) {
                    continue;
                }
                let key = (stage as u32, inferred.address);
                let resource = if let Some(known) = prepared.get(&key) {
                    *known
                } else {
                    let candidate = Candidate {
                        address: inferred.address,
                        stage,
                    };
                    let made = self.prepare_candidate(candidate, memory, submission);
                    prepared.insert(key, made);
                    made
                };
                if let Some(resource) = resource {
                    shaders.push((stage, resource));
                }
            }
            per_draw.push(shaders.clone());
            previous_writes = Some(written);
            previous_shaders = shaders;
        }
        per_draw
    }

    /// Prepares one shader candidate and records the outcome in the submission's report: the module
    /// travels with the submission when the backend has not seen it, and a failure is counted and
    /// named. The resource it became, or `None` when it could not be prepared.
    fn prepare_candidate(
        &mut self,
        candidate: Candidate,
        memory: &impl GuestMemory,
        submission: &mut Submission,
    ) -> Option<ResourceId> {
        match self.prepare(
            candidate.address,
            candidate.stage,
            (
                submission.report.primitive_topology,
                submission.report.wave_widths,
            ),
            memory,
        ) {
            Ok(prepared) => {
                // The address resolved, whatever happened to the shader after that.
                submission.report.addresses_resolved += 1;
                submission.report.shaders_translated += 1;
                Some(match prepared {
                    // Only a module the backend has not seen travels with the
                    // submission. A cached one is already there, and re-sending it
                    // every draw would undo the point of caching it.
                    Prepared::Fresh {
                        resource,
                        module,
                        warnings,
                    } => {
                        submission.report.warnings.extend(warnings);
                        submission.modules.insert(resource, module);
                        resource
                    }
                    Prepared::Cached { resource } => {
                        submission.report.cache_hits += 1;
                        resource
                    }
                })
            }
            Err(failure) => {
                // Counted before the reason is consumed, and counted apart from the
                // shader outcome: whether the address resolved is evidence about the
                // address space, and the two questions have different answers.
                match failure {
                    PrepareFailure::Unresolved(_) => {
                        submission.report.addresses_unresolved += 1;
                    }
                    PrepareFailure::Resolved(_) => submission.report.addresses_resolved += 1,
                }
                submission.report.failures.push(ShaderFailure {
                    address: candidate.address,
                    stage: format!("{:?}", candidate.stage).to_lowercase(),
                    reason: failure.reason(),
                });
                None
            }
        }
    }

    /// Decides which shader addresses to prepare, from both routes.
    ///
    /// Registration wins where the two overlap, and every overlap is counted either way -
    /// agreement is the only evidence available that the register vocabulary is right,
    /// and disagreement is the only evidence that it is wrong. Neither is obtainable
    /// without both routes running, which is why the inferred path keeps running even
    /// when registration has already answered.
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
    /// # The order of operations is the point
    ///
    /// Decode, then hash, then check the cache, then translate. Decoding first is what
    /// makes the key the shader's *actual bytes* rather than the arbitrary window it was
    /// read from - two identical shaders followed by different data would otherwise miss
    /// the cache every time, and a shader whose trailing data changed would translate
    /// again on every frame.
    ///
    /// Decoding is a linear walk and translation expands every instruction across
    /// sixty-four lanes, so the expensive half is the one behind the cache.
    fn prepare(
        &mut self,
        address: u64,
        stage: ShaderStage,
        (topology, widths): (Option<PrimitiveTopology>, WaveWidths),
        memory: &impl GuestMemory,
    ) -> Result<Prepared, PrepareFailure> {
        // A shader already decoded at this address, and still byte for byte the same, is not decoded
        // again: a decode is a function of the bytes, and a GL frame names the same few shaders in
        // every submission.
        if let Some(known) = self.decoded.get(&address)
            && let Some(bytes) = memory.read(guest_address_of(address), known.len())
            && bytes == known.as_slice()
        {
            return self.prepare_decoded(address, stage, (topology, widths), (bytes, None));
        }
        // The register named a GPU address; guest memory is indexed by a guest one.
        // They are assumed to be the same number - see `guest_address_of`.
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
        self.prepare_decoded(address, stage, (topology, widths), (shader, Some(&decoded)))
    }

    /// Translates and caches a shader whose bytes, end-of-program included, are known - or finds
    /// its translation in the cache.
    ///
    /// `decoded` is the decode of exactly `shader` when the caller has one. When it does not (the
    /// bytes were matched against an earlier decode at the same address), the decode is done again,
    /// only if a translation is needed: it is a function of the bytes, so it is the same decode.
    fn prepare_decoded(
        &mut self,
        address: u64,
        stage: ShaderStage,
        (topology, widths): (Option<PrimitiveTopology>, WaveWidths),
        (shader, decoded): (&[u8], Option<&orbistoun_shader::Decode>),
    ) -> Result<Prepared, PrepareFailure> {
        let host_stage = host_stage(stage);
        // The primitive the mesh output assembles, from the stream's decoded topology. Only the
        // mesh stage emits primitives, so only there does the topology matter or a bad one refuse;
        // a stream that set no topology draws the measured triangle (`-0c58`).
        let primitive = if host_stage == Stage::Mesh {
            match topology {
                Some(t) => mesh_primitive_of(t).map_err(PrepareFailure::Resolved)?,
                None => MeshPrimitive::default(),
            }
        } else {
            MeshPrimitive::default()
        };
        // **Keyed by content, stage and primitive, not content alone.** The same instructions
        // translated for a fragment stage and for a compute dispatch are different modules - one
        // declares inputs and a colour output and the other publishes its registers - and a mesh
        // module's output shape is in the module too, so a point and a triangle from the same
        // bytes are different modules. A cache that ignored any of these would serve whichever was
        // translated first.
        let user_data = match stage {
            ShaderStage::Vertex => self.user_data[0],
            ShaderStage::Fragment => self.user_data[1],
            ShaderStage::Compute => UserData::default(),
        };
        let (strategy, width_salt) = stage_strategy(self.strategy, host_stage, widths);
        // The user-data layout is in the module too: the same bytes reading two words at entry and
        // reading none are different modules (worklog 826). So is the width: a mask is one register
        // or two.
        let key = content_hash(shader)
            ^ stage_salt(host_stage)
            ^ primitive_salt(primitive)
            ^ window_salt(self.window)
            ^ width_salt
            ^ (u64::from(user_data.count) << 56 | u64::from(user_data.first_register) << 48);
        if let Some(&cached) = self.cache.get(&key) {
            if cached.matches(shader) {
                return Ok(Prepared::Cached {
                    resource: cached.resource,
                });
            }
            // The key matched and the shader did not. Refused rather than re-translated,
            // because the two possible causes - a hash collision, or a key that has
            // stopped meaning what it did - are both faults in this crate, and quietly
            // recovering from either would leave the cache in a state nobody can reason
            // about.
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
            strategy,
            (host_stage, primitive),
            self.window,
            user_data,
        )
        .map_err(|e| {
            PrepareFailure::Resolved(format!(
                "the shader at {address:#x} could not be translated: {e}"
            ))
        })?;

        let resource = ResourceId(self.next_resource);
        self.next_resource += 1;
        self.cache.insert(key, Cached::of(resource, shader));
        // Where each texture the module samples comes from, for binding them per draw (worklog 840).
        self.texture_sources
            .insert(resource, translated.textures.clone());
        Ok(Prepared::Fresh {
            resource,
            module: translated.module,
            warnings: translated
                .warnings
                .iter()
                .map(|warning| format!("the shader at {address:#x}: {warning}"))
                .collect(),
        })
    }
}

/// Appends a submission's draws and dispatches to its command list, in the stream's order.
///
/// Split from `submit` for length, but it is a coherent step: the geometry a stream asks for, after
/// its state. An auto draw becomes a [`RenderCommand::Draw`] and an indexed one a
/// [`RenderCommand::DrawIndexed`] - the index buffer's address is decoded (`DrawKind::Indexed`) but
/// bound separately (worklog 628), so the count is emitted and the address waits. Each
/// `DISPATCH_DIRECT` becomes a [`RenderCommand::Dispatch`]; the guest memory it reads and writes is a
/// windowed buffer the backend binds on its own, so only the workgroup counts are carried.
///
/// A guest's frame is many small draws: the captured GL cube asks for three vertices at a time,
/// twelve times over, one triangle per draw (worklog 585).
fn push_geometry_commands(
    commands: &mut Vec<RenderCommand>,
    (walked, stream, draws): (&PacketWalk, &[u8], &[DrawCall]),
    writes: &[RegisterWrite],
    shaders: &[Vec<(ShaderStage, ResourceId)>],
) {
    let mut sent: [Option<[u32; USER_DATA_WORDS]>; 2] = [None, None];
    let mut blend_sent = None;
    let mut transform_sent = None;
    // What each stage has bound so far - the up-front binds, then each draw's own (worklog 835).
    let mut bound: Vec<(ShaderStage, ResourceId)> = commands
        .iter()
        .filter_map(|command| match command {
            RenderCommand::BindShader { stage, shader } => Some((*stage, *shader)),
            _ => None,
        })
        .collect();
    // Each draw's state from the latest writes before it, found in one pass, and read a register at
    // a time rather than gathered into a list per draw.
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
        // The blend state in force at this draw, when the stream set one and it changed
        // (`REQ-...2ea9`, worklog 829).
        if let Some(value) = sweep.latest(at, crate::registers::CB_BLEND0_CONTROL)
            && blend_sent != Some(value)
        {
            commands.push(RenderCommand::SetBlend(decode_blend_control(value)));
            blend_sent = Some(value);
        }
        // The clip-to-pixel transform in force at this draw, when the stream set one and it changed
        // (worklog 837) - a GL guest's flips y.
        if let Some(transform) = viewport_transform_from(|register| sweep.latest(at, register))
            && transform_sent != Some(transform)
        {
            commands.push(RenderCommand::SetViewportTransform(transform));
            transform_sent = Some(transform);
        }
        // Each stage's user data as it stands at this draw, emitted when it changed (worklog 825).
        // A word the stream never wrote reads zero.
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
        let command = match draw.kind {
            DrawKind::Auto { vertices } => RenderCommand::Draw {
                vertices,
                instances: draw.instances,
                first_vertex: 0,
            },
            DrawKind::Indexed { indices, .. } => RenderCommand::DrawIndexed {
                indices,
                instances: draw.instances,
                first_index: 0,
            },
        };
        commands.push(command);
    }
    for dispatch in dispatch_calls(walked, stream) {
        commands.push(RenderCommand::Dispatch {
            x: dispatch.groups[0],
            y: dispatch.groups[1],
            z: dispatch.groups[2],
        });
    }
}

/// Each stage's first user-data register, as an absolute register index (worklog 825).
///
/// `SPI_SHADER_USER_DATA_PS_0` is `0x2C0C` - measured, the GL cube capture showed the pixel shader's
/// descriptor-table pointer there (`data/packets.toml`) - and `SPI_SHADER_USER_DATA_GS_0` is
/// `0x2C8C`: `gfx103.json` maps it at byte `45616` (`0xB230`, register `0x2C8C`), and the
/// open-toolchain GL context writes its per-draw vertex offset there (oops-sdk `gl_draw.c`). The
/// vertex stage reads the `GS` set because on this generation the vertex program runs as the NGG
/// geometry stage, the shape every translation of it takes (D688).
const USER_DATA_REGISTERS: [(ShaderStage, u32); 2] = [
    (ShaderStage::Vertex, 0x2C8C),
    (ShaderStage::Fragment, 0x2C0C),
];

/// The distinct image descriptors the draws' pixel shaders name, in first-use order (worklog 827).
///
/// Each fragment [`RenderCommand::SetUserData`]'s words 0 and 1 are a descriptor table's address; its
/// first eight words are the image descriptor. A table that is not readable guest memory names
/// nothing - a draw with no texture passes zeros, and zero is not mapped.
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

/// `SQ_IMG_RSRC_WORD3.TYPE` for a 2D image: 9 (Mesa `ac_descriptors.c:372`, as the SDK's
/// `gl_pack_descriptors` cites it, and obSCEne `-6c80` measured `0x90000fac` for its 2D control).
const IMAGE_TYPE_2D: u32 = 9;

/// `GFX10_FORMAT_8_8_8_8_UNORM` (`gfx10-rsrc.json:61`).
const FORMAT_8_8_8_8_UNORM: u32 = 56;

/// Inserts a [`RenderCommand::BindTexture`] after each fragment `SetUserData` whose descriptor table
/// names a texture read exactly, so the draws that follow sample the guest's own texels rather than
/// the backend's default (worklog 828).
fn bind_textures(
    commands: &mut Vec<RenderCommand>,
    (sources, texels): (&BTreeMap<ResourceId, Vec<TextureSource>>, &mut TexelCache),
    memory: &impl GuestMemory,
) {
    // **Each draw's textures, where its pixel shader says they are** (worklog 840): for every slot
    // the bound fragment module samples, the descriptor at that slot's table offset - zero for a
    // module whose one texture's descriptor did not come from the table, the one place a texture
    // was ever read from. Re-emitted before a draw whenever the table or the module changed.
    let mut out = Vec::with_capacity(commands.len());
    let (mut table, mut fragment, mut stale) = (None, None, false);
    let mut read: BTreeMap<(u64, u32), Option<RenderCommand>> = BTreeMap::new();
    for command in commands.drain(..) {
        match &command {
            RenderCommand::SetUserData {
                stage: ShaderStage::Fragment,
                words,
            } => {
                table = Some(u64::from(words[0]) | u64::from(words[1]) << 32);
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
                // A module this pipeline did not translate (none in practice) still gets the one
                // texture it always did.
                let default = [TextureSource {
                    slot: 0,
                    table_offset: None,
                }];
                let slots = if fragment.is_some_and(|m| !sources.contains_key(&m)) {
                    &default[..]
                } else {
                    slots
                };
                for source in slots {
                    let at = table.map(|t| t + u64::from(source.table_offset.unwrap_or(0)));
                    // **Read once per descriptor per submission** (worklog 844): guest memory does
                    // not change while its submission is prepared, and a GL frame binds the same few
                    // textures hundreds of times - each read, copied and hashed again.
                    out.extend(at.and_then(|at| {
                        read.entry((at, source.slot))
                            .or_insert_with(|| read_texture(at, source.slot, memory, texels))
                            .clone()
                    }));
                }
            }
            _ => {}
        }
        out.push(command);
    }
    *commands = out;
}

/// The texture a descriptor table's first eight words name, as a [`RenderCommand::BindTexture`] - or
/// `None` for a table that is not readable, or a texture whose layout is not one read exactly: 2D,
/// linear, `8_8_8_8_UNORM`.
///
/// **The row pitch is the descriptor's, not the width.** `ADDR_SW_LINEAR` aligns a row to 256 bytes
/// (Mesa `gfx9addrlib.cpp:5117-5127`, as the SDK cites it), so a 16-texel-wide texture's rows are 64
/// texels apart; for a 2D image `SQ_IMG_RSRC_WORD4` carries `pitch - 1` in bits 0-13 when the pitch
/// exceeds the width and zero when it does not (`gfx10-rsrc.json:401-406`). Read at the width
/// instead, every row after the first would be read from the wrong place.
///
/// **Hashed where it lies, copied only when new** (worklog 851): the rows are hashed in guest memory,
/// and texels whose hash `texels` already holds at the same place and shape are shared rather than
/// gathered again - a GL frame binds the same few textures in every one of its fifty submissions.
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
    let descriptor = decode_image_descriptor(words);
    if words[3] >> 28 != IMAGE_TYPE_2D
        || descriptor.tiling != SwizzleMode::Linear
        || descriptor.format != FORMAT_8_8_8_8_UNORM
    {
        return None;
    }
    let pitch_field = words[4] & 0x3fff;
    let pitch = if pitch_field == 0 {
        descriptor.width
    } else {
        pitch_field + 1
    };
    let (width, height) = (descriptor.width as usize, descriptor.height as usize);
    let row_bytes = pitch as usize * 4;
    let span = row_bytes * (height - 1) + width * 4;
    let key = (descriptor.base, descriptor.width, descriptor.height, pitch);
    // **Nothing written since it was read: nothing to read** (worklog 851) - asked of the host, and
    // only where it says so.
    if let Some(cached) = texels.get(&key)
        && cached.since.and_then(|since| {
            orbistoun_mem::watch::written_since(descriptor.base, span as u64, since)
        }) == Some(false)
    {
        return Some(RenderCommand::BindTexture {
            slot,
            hash: cached.hash,
            texels: cached.texels.clone(),
            width: descriptor.width,
            height: descriptor.height,
        });
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
    Some(RenderCommand::BindTexture {
        slot,
        hash,
        texels: shared,
        width: descriptor.width,
        height: descriptor.height,
    })
}

/// A texture's texels as last read, with their content hash and the host's mark of when.
#[derive(Debug)]
struct CachedTexels {
    since: Option<u64>,
    hash: u64,
    texels: std::sync::Arc<[u32]>,
}

/// Texels read from guest memory, by where they lie and their shape (worklog 851).
type TexelCache = std::collections::HashMap<(u64, u32, u32, u32), CachedTexels>;

/// How many textures the cache holds before it starts again - enough for a frame's set, and a bound
/// on what a guest streaming new textures every frame can make it keep.
const TEXEL_CACHE_ENTRIES: usize = 64;

/// Each stage's `SPI_SHADER_PGM_RSRC2`, vertex (the NGG geometry set) then fragment: `gfx103.json`
/// maps `RSRC2_GS` at byte `45612` (register `0x2C8B`) and `RSRC2_PS` at `45100` (`0x2C0B`), and
/// both carry `USER_SGPR` in bits 1-5.
const RSRC2_REGISTERS: [u32; 2] = [0x2C8B, 0x2C0B];

/// Each stage's `SPI_SHADER_PGM_RSRC1`, in the same order: `gfx103.json` maps `RSRC1_GS` at byte
/// `45608` (`0x2C8A`) and `RSRC1_PS` at `45096` (`0x2C0A`), with `DX10_CLAMP` in bit 21 (worklog 834).
const RSRC1_REGISTERS: [u32; 2] = [0x2C8A, 0x2C0A];

/// `SPI_SHADER_PGM_RSRC1.DX10_CLAMP`, bit 21 (`gfx103.json`).
const DX10_CLAMP_BIT: u32 = 1 << 21;

/// Where each stage's user data lands, how much of it there is, and where it sits in the block
/// (worklog 826): the count from the stage's last `RSRC2` write, the first register `s8` for the
/// geometry program - the eight before it are the wave's own, and the cube's vertex program reads
/// its per-draw word from `s8` (`v_add_nc_u32 v14, s8, v14`) - and `s0` for the pixel shader, the
/// vertex stage's words first in the block and the fragment stage's after them.
fn user_data_layouts(writes: &[RegisterWrite]) -> [UserData; 2] {
    let last = |register: u32| {
        writes
            .iter()
            .rev()
            .find(|write| write.register == register)
            .map(|write| write.value)
    };
    let count = |register: u32| last(register).map_or(0, |value| (value >> 1) & 0x1f);
    // `DX10_CLAMP`, bit 21 of the stage's `RSRC1` (Mesa `S_00B848_DX10_CLAMP`; worklog 834): what an
    // output clamp does with a NaN. `None` when the stream set no `RSRC1`.
    let dx10_clamp = |register: u32| last(register).map(|value| value & DX10_CLAMP_BIT != 0);
    [
        UserData {
            first_register: 8,
            count: count(RSRC2_REGISTERS[0]),
            block_offset: 0,
            dx10_clamp: dx10_clamp(RSRC1_REGISTERS[0]),
        },
        UserData {
            first_register: 0,
            count: count(RSRC2_REGISTERS[1]),
            block_offset: USER_DATA_STAGE_WORDS,
            dx10_clamp: dx10_clamp(RSRC1_REGISTERS[1]),
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
        // **Where the geometry shader says its memory is** (D711): a live guest's vertex buffer is a
        // constant the shader forms in its own words, not something a register write carries.
        if self.places_window {
            self.place_window(candidates, memory);
        }
        // How many user-data words each stage's shader takes, from its `RSRC2` - what the hardware
        // loads, and so what the module reads at entry (worklog 826).
        self.user_data = if self.feeds_user_data {
            user_data_layouts(writes)
        } else {
            [UserData::default(); 2]
        };
    }

    /// Places the window at the constant base the vertex-stage candidate's shader forms (D711).
    ///
    /// The largest power-of-two span from that base, at most [`PLACED_WINDOW_MAX_WORDS`], that is
    /// wholly readable guest memory and does not cross four gigabytes - bounded by what the guest
    /// mapped, never by a guessed buffer size. No base, or no readable span, leaves the window as
    /// it was: the honest answer is the old one, not a window somewhere else.
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

/// The 64-bit address a shader forms from two constant scalar registers as the base of its memory
/// accesses, or `None` (D711).
///
/// **What counts as constant, and why so narrowly.** A scalar register is a constant here only if the
/// whole program writes it exactly once, with `s_mov_b32` of a literal or an inline integer, and no
/// other instruction's destination - a scalar load's whole range included - touches it. The base is
/// then the pair a `v_add_co_u32` takes as its scalar source for the low half and a
/// `v_add_co_ci_u32` takes, one register up, for the high half: the carry-chained 64-bit add every
/// global access in the open-toolchain GL context's geometry shaders forms its address with
/// (oops-sdk `tools/shader/vs-param3.s`, `vs-param4.s`; orbistoun `tools/shader-fixtures/primitive.s`).
/// A register the program also computes, loads or moves from elsewhere is not a constant and names
/// no base - so a shader whose addresses are computed yields nothing rather than a guess.
fn constant_address_base(
    decoded: &orbistoun_shader::Decode,
    encodings: &EncodingTable,
) -> Option<u64> {
    // Named the way the translator names them (`EncodingTable::mnemonic_for`), which covers the
    // long-form families - the carry-out add the base feeds is a VOP3.
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
/// Narrowing rather than demanding the whole window, because a shader near the end of a
/// mapping is an ordinary thing and refusing to read it would make its placement in
/// memory decide whether it works.
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
/// Exactly the window's span - `words()` words from `base` - because that length is the mask the
/// module applies (`Window::spanning` refuses a non-power-of-two for that reason), so a backend that
/// binds a region of a different length would fold an index to a different word than the shader
/// meant. An empty vector when the region is not fully mapped: a partial window is not one to bind,
/// and the default window at address zero is unmapped, so a stream that never placed its window
/// reads nothing rather than a page of whatever happens to be at zero.
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
/// The identity function, and deliberately a function rather than nothing at all.
///
/// The console shares one coherent memory pool between both processors, so the two
/// address spaces are expected to coincide - but that has not been confirmed against a
/// real submission, and it is the sort of assumption that is either exactly right or
/// wrong by a constant offset with nothing in between. Naming it here means checking it
/// later is a one-line change, and means a reader of a failed shader read knows which
/// assumption to suspect first.
///
/// Not a trait. There is no second implementation and inventing a seam for a
/// hypothetical one would be speculation; this is a known-uncertain constant with one
/// call site.
pub const fn guest_address_of(gpu_address: u64) -> u64 {
    gpu_address
}

/// A content-addressed id for a colour target of a given extent.
///
/// # Why the extent and not the address
///
/// A resource's id is meant to be its guest identity, and a render target's is really its guest
/// address. But the register carrying that address is the disputed one - the hardware capture and
/// obSCEne's constructed stream name different offsets for `CB_COLOR0_BASE`, while they agree on the
/// size register - so keying on the address would rest the id on the less certain of the two. The
/// extent is what is corroborated, and it is enough: the backend draws into an attachment it
/// allocates and reads back, storing nothing per-target, so two same-sized targets sharing an id is
/// harmless today. When a per-target host object exists, the address becomes the identity and this
/// changes (D702).
///
/// # Why the top bit
///
/// Shader ids are minted sequentially from one and stay small. Tagging a target id into the top of
/// the id space keeps the two disjoint without a shared counter, so a target and a shader can never
/// collide. Width and height are each at most fourteen bits (the register's fields), so the packed
/// extent uses thirty bits and the tag is free.
fn colour_target_id(extent: ColourTargetExtent) -> ResourceId {
    const TARGET_NAMESPACE: u64 = 1 << 63;
    ResourceId(TARGET_NAMESPACE | (u64::from(extent.width) << 16) | u64::from(extent.height))
}

/// The smallest window worth trying: one instruction that ends a program.
const MIN_SHADER_BYTES: usize = 4;

/// A cache key over a shader's bytes.
///
/// Keyed on content rather than address deliberately. A guest may move a shader, write a
/// different one to the same address, or have two addresses hold the same shader, and an
/// address-keyed cache is wrong in all three - silently reusing a translation of code
/// that is no longer there, which produces a frame drawn with the wrong shader and
/// nothing to indicate it.
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
    /// The pipeline stage the shader address was attributed to. Reported, never
    /// dispatched on - the register mapping it rests on is a hypothesis.
    pub stage: String,
    /// Whether the corpus had not seen it before. A title rebinds the same shaders
    /// constantly, so most captures in a real frame are already held.
    pub fresh: bool,
    /// Its length in bytes, up to and including the terminator that ended it.
    pub length: usize,
}

/// A shader address that yielded no shader, and why. Reported rather than dropped: an
/// address the register mapping produced but memory could not honour is the strongest
/// signal available about whether that mapping is right.
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
/// This is the census's input, gathered the way a frame gathers it: walk the packets,
/// read the shader-address registers, fetch each shader out of guest memory, and store it
/// by content. It is the connective step [`registers`](crate::registers) describes as the
/// one that lets the packet walker and the shader decoder finally meet.
///
/// Unlike [`Pipeline::submit`], it does **not** require a shader to *translate*. A shader
/// carrying an instruction the translator cannot handle yet is exactly what the census
/// exists to rank, so it is captured, not refused. The one thing required is a
/// terminator: without an end-of-program instruction the extent is unknown (D114), and
/// bytes of unknown length are not a shader but a guess about where one stops.
///
/// Storage is by content hash, so a guest that rebinds the same shader every draw
/// contributes it once. The report says what was stored and what each unresolved address
/// was, because an address that names nothing is evidence about the register mapping.
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

        // The extent, not trustworthiness, is what gates a capture. A desynchronised
        // decode still bounds the shader if it reached the terminator, and its blockers
        // are the whole point of capturing it.
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
}

/// The stage a register name refers to.
///
/// Unknown names produce `None` rather than a guess. A shader bound to the wrong stage
/// runs, and produces a frame that is wrong in a way nothing points at.
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

    /// The GL cube's vertex program, as the console ran it: the first 64 words of oracle record B's
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

    /// **A linear texture is read at its descriptor's pitch, not its width** (worklog 828).
    ///
    /// A 16x2 `8_8_8_8_UNORM` 2D texture whose rows are 64 texels apart - `ADDR_SW_LINEAR`'s 256-byte
    /// row alignment - with `WORD4` carrying `pitch - 1 = 63`. Row 1 must come from 64 texels in; read
    /// at the width it would come from texel 16, which holds a different value here. And a descriptor
    /// that is not 2D linear RGBA8 binds nothing.
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

        // **A cached texture is re-read when its bytes change** (worklog 851): texel (1, 1) edited
        // in guest memory binds the edited value, not the cached one - and the padding past the
        // width, which no texel reads, changes nothing.
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

    /// **The backend's user-data block and the translator's are one layout** (worklog 826): the size
    /// and each stage's start, stated in two crates that must not import each other, pinned equal.
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

    /// **Each draw carries its own user data** (worklog 825).
    ///
    /// The console-run cube stream (oracle record B) writes a vertex offset into
    /// `SPI_SHADER_USER_DATA_GS_0` before each of its twelve draws. The submission must hand the
    /// vertex stage that word before each draw, and the twelve must differ - one offset for all of
    /// them is exactly why the cube's first live frame showed one face drawn twelve times.
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
        let mut distinct = offsets.clone();
        distinct.sort_unstable();
        distinct.dedup();
        assert_eq!(distinct.len(), 12, "each draw its own offset: {offsets:?}");
    }

    /// **The base a live vertex program forms is read out of its own words** (D711).
    ///
    /// The console-run cube program moves `0x0090_0000` into `s2` and `0x2` into `s3` and adds the
    /// pair, carry-chained, to every vertex's offset - so its buffer is at `0x2_0090_0000`, the
    /// address worklog 561 recorded. The capture tests anchor their window at the low half by hand;
    /// this is the same number, found. And the negative: the same program with `s2` also written by
    /// a second move is no longer a constant, so it names no base rather than the first value.
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

        // `s_mov_b32 s2, 0x1234` put first (a decode stops at `s_endpgm`, so it has to be inside the
        // program): s2 is written twice now, so it is not a constant, and the vertex loads' pair
        // names nothing. The scan moves on to the next add whose pair *is* constant - the canary
        // store's `s6:s7`, `0x2_0092_0000` - and never to either value of the reassigned register.
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
            (None, WaveWidths::default()),
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

    /// **A topology maps to the mesh primitive its shape is, or is refused by the name of the
    /// shape it is not.** The mesh module carries the output topology (D688), so this is the seam
    /// where a stream's `VGT_GS_OUT_PRIM_TYPE` becomes the primitive the translator emits. The
    /// negative is the point: a rectangle list and an unmeasured value have no mesh-primitive
    /// shape, and drawing one as a triangle is the plausible output principle 3 forbids at the
    /// last step before a picture - so they are refused, and the refusal names the topology
    /// (`-0c58` acceptance 3, watched failing here).
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

        let rect = mesh_primitive_of(PrimitiveTopology::RectangleList).unwrap_err();
        assert!(
            rect.contains("rectangle list") && rect.contains("refused"),
            "a rectangle list is refused by name, not drawn as a triangle: {rect}"
        );
        let other = mesh_primitive_of(PrimitiveTopology::Other(9)).unwrap_err();
        assert!(
            other.contains("VGT_GS_OUTPRIM_TYPE 9"),
            "an unmeasured value is refused by its raw field: {other}"
        );
    }

    /// The index count is the whole difference between the three shapes, and the mesh output is
    /// built from it - one index for a point, two for a line, three for a triangle.
    #[test]
    fn a_primitive_carries_its_index_count_and_a_distinct_cache_salt() {
        use super::{MeshPrimitive, primitive_salt};

        assert_eq!(MeshPrimitive::Points.indices(), 1);
        assert_eq!(MeshPrimitive::Lines.indices(), 2);
        assert_eq!(MeshPrimitive::Triangles.indices(), 3);

        // The salts must differ, or the same bytes at two topologies would share a cache entry
        // and the second draw would be served the first's module.
        let salts = [
            primitive_salt(MeshPrimitive::Points),
            primitive_salt(MeshPrimitive::Lines),
            primitive_salt(MeshPrimitive::Triangles),
        ];
        assert!(
            salts[0] != salts[1] && salts[1] != salts[2] && salts[0] != salts[2],
            "each primitive salts the cache key differently: {salts:?}"
        );
    }

    #[test]
    fn a_cache_entry_recognises_the_shader_it_was_built_from() {
        // The entry exists to notice when a key stops identifying a shader, so what it
        // has to get right is the *negative*: two different shaders must not both satisfy
        // one entry. The positive is the easy half and is here to keep the negative
        // honest - a `matches` that always answered false would pass the interesting test
        // and fail this one.
        let bytes: Vec<u8> = (0..32u8).collect();
        let entry = Cached::of(ResourceId(1), &bytes);
        assert!(entry.matches(&bytes));
    }

    #[test]
    fn a_cache_entry_rejects_a_shader_that_only_looks_like_it() {
        // Each field on its own, because an entry that compared only the length would
        // accept an edit in the middle, and one that compared only the ends would accept
        // a shader of a different size that happened to start and finish the same way.
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

    #[test]
    fn a_shader_too_short_to_hold_a_word_does_not_panic() {
        // Reached through `read_window`, which narrows rather than demanding a full
        // window - so a shader near the end of a mapping can be very short. Indexing
        // without checking would turn that into a crash in the emulator rather than a
        // refusal from the decoder.
        let entry = Cached::of(ResourceId(1), &[1, 2]);
        assert!(entry.matches(&[1, 2]));
        assert!(!entry.matches(&[3, 4]));
    }
}
