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
    Capture, EncodingTable, OperandTable, ShaderCorpus, ShaderError, decode_program,
};
use orbistoun_translate::wavefront::MeshPrimitive;
use orbistoun_translate::wavefront::Stage;
use orbistoun_translate::wavefront::Window;
use orbistoun_translate::{Strategy, translate_windowed_primitive};

use crate::backend::{Rect, RenderCommand, ResourceId, ShaderStage};
use crate::packet::{PacketWalk, walk};
use crate::registers::{
    BlendControl, ColourTarget, ColourTargetExtent, DepthControl, DrawKind, PrimitiveTopology,
    StencilControl, SwizzleMode, Vocabulary, blend_control_at, colour_swizzle_mode_at,
    colour_target_at, colour_target_extent_at, depth_control_at, dispatch_calls, draw_calls,
    primitive_topology_at, register_writes, scissor_at, shader_candidates, stencil_control_at,
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
const fn window_salt(window: Window) -> u64 {
    ((window.base as u64) << 32) | window.words() as u64
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
    /// What was seen and what failed.
    pub report: SubmissionReport,
}

/// Why a shader could not be prepared, and whether its *address* was the problem.
///
/// The split is the whole point. An address that guest memory does not recognise is
/// evidence about the address space; anything after that is evidence about the shader.
/// Reporting them as one string loses the first, which is the only measurement available
/// for the open half of D101.
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
    /// Shader bytes, by content hash, to the resource holding the translation.
    cache: BTreeMap<u64, Cached>,
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
            cache: BTreeMap::new(),
            next_resource: 1,
        })
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
        let walked = walk(stream);
        let writes = register_writes(&walked, stream, &self.vocabulary);
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
        submission.depth_control = depth_control_at(&writes);
        submission.stencil_control = stencil_control_at(&writes);
        submission.blend_control = blend_control_at(&writes);
        // The primitive the draw produces, read from `VGT_GS_OUT_PRIM_TYPE` - a point draw and a
        // triangle draw are told apart here, and `prepare` threads this into the mesh module's
        // output shape (points, lines or triangles), so the two no longer render alike (`-0c58`).
        submission.report.primitive_topology = primitive_topology_at(&writes);

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
        submission.guest_memory = read_guest_window(memory, self.window);

        let candidates = Self::reconcile(queue, registered, &inferred, &mut submission.report);
        submission.report.shaders_found = candidates.len();

        for candidate in candidates {
            match self.prepare(
                candidate.address,
                candidate.stage,
                submission.report.primitive_topology,
                memory,
            ) {
                Ok(prepared) => {
                    // The address resolved, whatever happened to the shader after that.
                    submission.report.addresses_resolved += 1;
                    submission.report.shaders_translated += 1;
                    let resource = match prepared {
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
                    };
                    submission.commands.push(RenderCommand::BindShader {
                        stage: candidate.stage,
                        shader: resource,
                    });
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
                }
            }
        }

        // **The draws and dispatches, after the binds.** A stream sets its state and then asks for
        // geometry, so emitting them in that order is what a backend can act on - and the order is
        // the stream's own, from the packet walk rather than from anything this function decides.
        push_geometry_commands(&mut submission.commands, &walked, stream);
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
        topology: Option<PrimitiveTopology>,
        memory: &impl GuestMemory,
    ) -> Result<Prepared, PrepareFailure> {
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
        let key = content_hash(shader)
            ^ stage_salt(host_stage)
            ^ primitive_salt(primitive)
            ^ window_salt(self.window);
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

        let translated = translate_windowed_primitive(
            &decoded,
            &self.encodings,
            self.strategy,
            host_stage,
            primitive,
            self.window,
        )
        .map_err(|e| {
            PrepareFailure::Resolved(format!(
                "the shader at {address:#x} could not be translated: {e}"
            ))
        })?;

        let resource = ResourceId(self.next_resource);
        self.next_resource += 1;
        self.cache.insert(key, Cached::of(resource, shader));
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
fn push_geometry_commands(commands: &mut Vec<RenderCommand>, walked: &PacketWalk, stream: &[u8]) {
    for draw in draw_calls(walked, stream) {
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
        .read(u64::from(window.base), byte_len)
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
