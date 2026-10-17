//! Where a running title's time goes.
//!
//! Cumulative nanoseconds per phase and counts of events, since the last [`take`]. Each layer that
//! takes part in presenting a frame adds to it, and the worker reads and streams it once a second,
//! so the window shows the frame rate and which part of a frame costs the most.
//!
//! Atomics rather than a lock: phases are timed on guest threads and the draw thread, and a sink
//! that blocks a guest thread changes the program it observes.

use std::sync::atomic::{AtomicU64, Ordering};

/// Every phase's label, once: [`Phase::label`] and, in a `tracy` build, its profiler zone are both
/// made from it, so a phase added has both.
macro_rules! phase_labels {
    ($phase:expr, $as:ident) => {
        match $phase {
            Phase::Prepare => $as!("prepare"),
            Phase::ReadTarget => $as!("read target"),
            Phase::Build => $as!("build"),
            Phase::Seed => $as!("seed"),
            Phase::Execute => $as!("execute"),
            Phase::ReadBack => $as!("read back"),
            Phase::Release => $as!("release"),
            Phase::DrawOther => $as!("draw setup"),
            Phase::WriteTarget => $as!("write target"),
            Phase::Present => $as!("present"),
            Phase::Submit => $as!("submit (total)"),
            Phase::Gpu => $as!("gpu busy"),
        }
    };
}

/// Every span's label, once, as [`phase_labels`] is for phases.
macro_rules! span_labels {
    ($span:expr, $as:ident) => {
        match $span {
            Span::SubmitCall => $as!("submit call"),
            Span::QueueFull => $as!("queue full"),
            Span::QueuedWrites => $as!("queued writes"),
            Span::SettleOver => $as!("settle over"),
            Span::PrepareAgain => $as!("prepare again"),
            Span::CommandProcessor => $as!("command processor"),
            Span::RunDraws => $as!("run draws"),
            Span::ReadTargetBefore => $as!("run draws: read target"),
            Span::ReadTargetCompare => $as!("read target: compare"),
            Span::ReadTargetEarlier => $as!("read target: earlier"),
            Span::ProtectUnderCopy => $as!("protect target: under a copy"),
            Span::ProtectRefused => $as!("protect target: refused"),
            Span::ProtectGuestWrote => $as!("protect target: guest wrote"),
            Span::ProtectHostWrote => $as!("protect target: host wrote"),
            Span::ReadTargetRead => $as!("read target: read"),
            Span::ExecuteHandOff => $as!("execute hand-off"),
            Span::ExecuteOnDevice => $as!("execute on device"),
            Span::Copy => $as!("dma copy"),
            Span::CopyGuard => $as!("guard: deferred copy"),
            Span::KeepSnapshot => $as!("keep snapshot"),
            Span::SnapshotOnDevice => $as!("keep snapshot: on device"),
            Span::ClosePass => $as!("device: close pass"),
            Span::SnapshotBuffer => $as!("keep snapshot: new buffer"),
            Span::SnapshotCopy => $as!("keep snapshot: copy"),
            Span::DispatchNewBuffer => $as!("dispatch: new buffer"),
            Span::DispatchNewHostBuffer => $as!("dispatch: new host buffer"),
            Span::DispatchNewImage => $as!("dispatch: new image"),
            Span::DeferredSuperseded => $as!("deferred: superseded"),
            Span::DeferredCovered => $as!("deferred: covered"),
            Span::DeferredFillDropped => $as!("deferred: fill under a frame"),
            Span::ReadBand => $as!("read band"),
            Span::FoldBand => $as!("fold band"),
            Span::DiscardSnapshot => $as!("discard snapshot"),
            Span::OtherMemory => $as!("other memory work"),
            Span::FlipWriteBack => $as!("flip write-back"),
            Span::CarryOut => $as!("carry out"),
            Span::CarryOutTake => $as!("carry out: take"),
            Span::CarryOutSettle => $as!("carry out: settle"),
            Span::CarryOutTile => $as!("carry out: tile"),
            Span::CarryOutWrite => $as!("carry out: write"),
            Span::Drive => $as!("drive"),
            Span::SubmitDraws => $as!("submit draws"),
            Span::ExecutorLog => $as!("executor log"),
            Span::GuestWindow => $as!("guest window"),
            Span::DrawCommand => $as!("draw command"),
            Span::StateCommand => $as!("state command"),
            Span::DrawJoined => $as!("draw joined"),
            Span::DrawRoom => $as!("execute: draw room"),
            Span::JoinedBuffers => $as!("joined: buffers"),
            Span::DrawWhole => $as!("draw whole"),
            Span::WholeWindow => $as!("whole: window"),
            Span::WholePipeline => $as!("whole: pipeline"),
            Span::WholeRecord => $as!("whole: record"),
            Span::PrepareWalk => $as!("prepare: walk"),
            Span::PrepareRegisters => $as!("prepare: registers"),
            Span::PrepareEnvironment => $as!("prepare: environment"),
            Span::PrepareWindow => $as!("prepare: window"),
            Span::PrepareShaders => $as!("prepare: shaders"),
            Span::PrepareGeometry => $as!("prepare: geometry"),
            Span::PrepareTextures => $as!("prepare: textures"),
            Span::TextureCensus => $as!("textures: census"),
            Span::TextureBind => $as!("textures: bind"),
            Span::BufferBind => $as!("textures: buffers"),
            Span::BuildModules => $as!("build: modules"),
            Span::BuildResources => $as!("build: resources"),
            Span::TextureStaged => $as!("texture: staged"),
            Span::TextureImage => $as!("texture: image"),
            Span::TextureStaging => $as!("texture: staging"),
            Span::BuildPipeline => $as!("build: pipeline"),
            Span::KeepPipelineCache => $as!("build: keep cache"),
            Span::BufferResolve => $as!("buffers: resolve"),
            Span::BufferEvaluate => $as!("buffers: evaluate"),
            Span::BufferWatch => $as!("buffers: watch"),
            Span::BufferCompare => $as!("buffers: compare"),
            Span::BufferRead => $as!("buffers: read"),
            Span::TextureKept => $as!("textures: kept"),
            Span::TextureBase => $as!("textures: base level"),
            Span::BaseHash => $as!("base level: hash"),
            Span::BaseTexels => $as!("base level: texels"),
            Span::TextureLinear => $as!("textures: linear read"),
            Span::TextureBorders => $as!("textures: borders"),
            Span::TextureLevels => $as!("textures: levels"),
            Span::TextureBlocks => $as!("textures: blocks"),
            Span::TextureWatch => $as!("textures: watch"),
            Span::TextureUnwatched => $as!("textures: unwatched"),
            Span::TextureSwizzle => $as!("textures: swizzle"),
            Span::CpWalk => $as!("cp: walk"),
            Span::CpDispatch => $as!("cp: dispatch"),
            Span::DispatchPrepare => $as!("dispatch: prepare"),
            Span::DispatchRead => $as!("dispatch: read"),
            Span::DispatchExecute => $as!("dispatch: execute"),
            Span::DispatchOnDevice => $as!("dispatch on device"),
            Span::DispatchWriteBack => $as!("dispatch: write back"),
            Span::DeviceSetup => $as!("device: setup"),
            Span::DeviceSetupSettle => $as!("device: setup: settle"),
            Span::DevicePipeline => $as!("device: pipeline"),
            Span::DeviceRun => $as!("device: run"),
            Span::DeviceRunWait => $as!("device: run: wait"),
            Span::DeviceReadBack => $as!("device: read back window"),
            Span::ReadBackImage => $as!("device: read back image"),
            Span::ReadBackBuffers => $as!("device: read back buffers"),
            Span::DeviceRelease => $as!("device: release"),
            Span::WriteBackChanges => $as!("write back: changes"),
            Span::ChangesWindow => $as!("changes: window"),
            Span::ChangesBuffers => $as!("changes: buffers"),
            Span::ChangesImages => $as!("changes: images"),
            Span::WriteBackGuest => $as!("write back: guest"),
            Span::WriteBackRelease => $as!("write back: release guards"),
            Span::WriteBackCopy => $as!("write back: copy"),
            Span::WriteBackFills => $as!("write back: fills"),
            Span::FillDeferred => $as!("fill: deferred"),
            Span::FillWritten => $as!("fill: written"),
            Span::DeferredRefilled => $as!("deferred: refilled"),
            Span::BuffersListed => $as!("read back buffers: listed"),
            Span::BuffersOverflowed => $as!("read back buffers: overflowed"),
            Span::BuffersWhole => $as!("read back buffers: whole"),
            Span::BuffersGathered => $as!("read back buffers: gathered"),
            Span::SettlePending => $as!("settle pending frame"),
            Span::ShadersUpFront => $as!("shaders: up front"),
            Span::TexturesEmptied => $as!("emptied: texture caches"),
            Span::BuffersEmptied => $as!("emptied: buffer caches"),
            Span::TwinsEmptied => $as!("emptied: flat twins"),
        }
    };
}

/// A label as itself.
macro_rules! as_label {
    ($label:literal) => {
        $label
    };
}

/// A label as a Tracy zone of that name (D829): one static location per label.
#[cfg(feature = "tracy")]
macro_rules! as_zone {
    ($label:literal) => {
        tracy_client::span_location!($label)
    };
}

/// A Tracy zone for `location` on this thread until the answer is dropped, while a profiler build
/// runs (D829); nothing otherwise.
#[cfg(feature = "tracy")]
fn zone(location: &'static tracy_client::SpanLocation) -> Option<tracy_client::Span> {
    tracy_client::Client::running().map(|client| client.span(location, 0))
}

/// Starts the Tracy client in a `tracy` build (D829): a viewer or `tracy-capture` may then connect,
/// and zones are recorded while one is. Only the process that calls this runs one - the worker, not
/// the launcher that spawned it - and until one does, each span asks only whether a client runs.
/// Answers whether this build has the client to start.
pub fn start_profiler() -> bool {
    // The handle is a token: once started, the client runs for the rest of the process.
    #[cfg(feature = "tracy")]
    let _client = tracy_client::Client::start();
    cfg!(feature = "tracy")
}

/// A part of presenting a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// A submitted command buffer walked, and its shaders translated or taken from the cache.
    Prepare,
    /// The colour target read out of guest memory and detiled, before a submission's draws.
    ReadTarget,
    /// A resident draw's pipeline built (or taken from the cache).
    Build,
    /// Guest memory copied into a draw's window buffer.
    Seed,
    /// A draw recorded and submitted.
    Execute,
    /// Something a draw wrote read back to the host.
    ReadBack,
    /// Command pools and uncached pipelines released.
    Release,
    /// Everything else in carrying out a submission's draws: setting the backend up, uploading the
    /// target, reading the finished frame back.
    DrawOther,
    /// The finished frame tiled and written back where the guest reads it.
    WriteTarget,
    /// A flipped buffer detiled and handed to the window.
    Present,
    /// A whole graphics submit, from the guest's call to its return. It contains the phases from
    /// [`Self::Prepare`] to [`Self::WriteTarget`], so it is reported beside them rather than summed
    /// with them.
    Submit,
    /// The device's busy time, by its own clock. Reported as a share of the window on its own,
    /// never summed with the host phases.
    Gpu,
}

impl Phase {
    /// Every phase, in the order a frame passes through them.
    pub const ALL: [Self; 12] = [
        Self::Prepare,
        Self::ReadTarget,
        Self::Build,
        Self::Seed,
        Self::Execute,
        Self::ReadBack,
        Self::Release,
        Self::DrawOther,
        Self::WriteTarget,
        Self::Present,
        Self::Submit,
        Self::Gpu,
    ];

    /// What to call it on screen.
    pub const fn label(self) -> &'static str {
        phase_labels!(self, as_label)
    }

    /// Its zone in the Tracy profiler (D829), named as [`Self::label`] names it.
    #[cfg(feature = "tracy")]
    fn zone(self) -> &'static tracy_client::SpanLocation {
        phase_labels!(self, as_zone)
    }
}

/// Something counted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Count {
    /// Flips the guest submitted.
    Flips,
    /// Submissions whose draws were carried out.
    Submissions,
    /// Draws carried out.
    Draws,
}

const PHASES: usize = Phase::ALL.len();
const COUNTS: usize = 3;

static SPENT: [AtomicU64; PHASES] = [const { AtomicU64::new(0) }; PHASES];
static COUNTED: [AtomicU64; COUNTS] = [const { AtomicU64::new(0) }; COUNTS];

/// Runs `work`, adding its time to `phase`.
pub fn measure<T>(phase: Phase, work: impl FnOnce() -> T) -> T {
    #[cfg(feature = "tracy")]
    let _zone = zone(phase.zone());
    let started = std::time::Instant::now();
    let out = work();
    add(phase, started.elapsed());
    out
}

/// Adds `spent` to `phase`.
pub fn add(phase: Phase, spent: std::time::Duration) {
    // The device's own time is plotted, so a report says how much of the run the device was busy
    // as well as how long the host waited for it.
    #[cfg(feature = "tracy")]
    if phase == Phase::Gpu
        && let Some(client) = tracy_client::Client::running()
    {
        client.plot(
            tracy_client::plot_name!("gpu ms"),
            spent.as_secs_f64() * 1e3,
        );
    }
    let nanos = u64::try_from(spent.as_nanos()).unwrap_or(u64::MAX);
    SPENT[phase as usize].fetch_add(nanos, Ordering::Relaxed);
    if !FIRST_FLIPPED.load(Ordering::Relaxed) {
        FIRST_FRAME[phase as usize].fetch_add(nanos, Ordering::Relaxed);
    }
}

/// Nanoseconds per phase from the start of the run to its first flip, kept apart from the
/// windowed totals: the first frame pays for every translation and pipeline the title needs.
static FIRST_FRAME: [AtomicU64; PHASES] = [const { AtomicU64::new(0) }; PHASES];

/// Whether the first flip has been seen, which closes [`FIRST_FRAME`].
static FIRST_FLIPPED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Closes the first frame at a flip: the host time each phase took up to the first one, or `None`
/// at every later flip.
pub fn close_first_frame() -> Option<Snapshot> {
    if FIRST_FLIPPED.swap(true, Ordering::Relaxed) {
        return None;
    }
    Some(Snapshot {
        spent: std::array::from_fn(|i| FIRST_FRAME[i].load(Ordering::Relaxed)),
        counted: [0; COUNTS],
    })
}

/// Counts one of `what`.
pub fn count(what: Count) {
    COUNTED[what as usize].fetch_add(1, Ordering::Relaxed);
    // A guest flip ends a frame in the profiler's timeline (D829), and plots how long it took.
    #[cfg(feature = "tracy")]
    if what == Count::Flips
        && let Some(client) = tracy_client::Client::running()
    {
        client.frame_mark();
        if let Some(ms) = since_last_flip_ms() {
            client.plot(tracy_client::plot_name!("frame ms"), ms);
        }
    }
}

/// Milliseconds since the previous guest flip, `None` at the first: the frame time a profiler
/// plots (D829). A lock once a frame, taken only while a profiler client runs.
#[cfg(feature = "tracy")]
fn since_last_flip_ms() -> Option<f64> {
    static LAST: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);
    let now = std::time::Instant::now();
    let mut last = LAST
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let previous = last.replace(now)?;
    Some(now.duration_since(previous).as_secs_f64() * 1e3)
}

/// What was measured since the last [`take`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    /// Nanoseconds per phase, in [`Phase::ALL`] order.
    pub spent: [u64; PHASES],
    /// Flips, submissions and draws, in [`Count`] order.
    pub counted: [u64; COUNTS],
}

impl Snapshot {
    /// Nanoseconds spent in `phase`.
    pub const fn spent(&self, phase: Phase) -> u64 {
        self.spent[phase as usize]
    }

    /// How many of `what`.
    pub const fn counted(&self, what: Count) -> u64 {
        self.counted[what as usize]
    }
}

/// Everything measured since the last call, and resets it.
pub fn take() -> Snapshot {
    Snapshot {
        spent: std::array::from_fn(|i| SPENT[i].swap(0, Ordering::Relaxed)),
        counted: std::array::from_fn(|i| COUNTED[i].swap(0, Ordering::Relaxed)),
    }
}

/// A finer span of presenting a frame, measured only when [`set_detail`] asks for it.
///
/// The phases above are always on. These cut a submission finer, for finding which part of it to
/// make faster; when off, each costs one relaxed load.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Span {
    /// The guest's submit call, whole.
    SubmitCall,
    /// A guest waiting at its submit for room in the command processor's queue (D821).
    QueueFull,
    /// Working out what a submission writes as it is queued, besides its drawn targets (D876).
    QueuedWrites,
    /// A preparation read waiting for the queue to drain because a queued submission writes over
    /// it (D874, D876).
    SettleOver,
    /// A preparation made again because the one before read what a queued submission writes,
    /// with the drain before it (D876).
    PrepareAgain,
    /// The command processor's walk of a submission, draws included.
    CommandProcessor,
    /// Carrying out a submission's draws into its target.
    RunDraws,
    /// Within [`Self::RunDraws`]: what the colour target holds before a run of draws, read or
    /// found unchanged.
    ReadTargetBefore,
    /// Within [`Self::ReadTargetBefore`]: the target's memory compared with the bytes it held when
    /// last written or read, where it is the last target and was not kept read-only (D720).
    ReadTargetCompare,
    /// Within [`Self::ReadTargetBefore`]: the target's memory compared with what it held as one of
    /// the earlier targets kept (D801).
    ReadTargetEarlier,
    /// A target not made read-only (D720) because a deferred copy guards its pages, so the next
    /// "unchanged" is answered by comparing. Counted.
    ProtectUnderCopy,
    /// A target not made read-only (D720) because the host refused it - its pages differ in
    /// protection, or the change failed - so the next "unchanged" is answered by comparing.
    /// Counted.
    ProtectRefused,
    /// A read-only target (D720) given back because the guest wrote it. Counted.
    ProtectGuestWrote,
    /// A read-only target (D720) given back because this process wrote or guarded its pages.
    /// Counted.
    ProtectHostWrote,
    /// Within [`Self::ReadTargetBefore`]: the target read and detiled afresh, something else having
    /// written it.
    ReadTargetRead,
    /// Handing the draws to the device thread and getting its answer.
    ExecuteHandOff,
    /// The draws on the device thread.
    ExecuteOnDevice,
    /// A `DMA_DATA` copy, deferred or carried out.
    Copy,
    /// Making a deferred copy's destination inaccessible, so the first touch carries it out - a
    /// change of protection over every page of it: a DMA copy's destination, or a flipped frame's
    /// own target (D719), guarded again each flip after the next frame drawn over it let it go.
    CopyGuard,
    /// Keeping a frame snapshot for a deferred copy.
    KeepSnapshot,
    /// The snapshot [`Self::KeepSnapshot`] asks for, kept on the device thread: what it waits for there
    /// (D831) - behind that thread's queued work under async submission - stays the guest's own.
    SnapshotOnDevice,
    /// The draws recorded into the open render pass ended and submitted, so device work that is not
    /// a draw - a copy out of the target, a snapshot - follows them on the queue. The draws' own
    /// submission, paid wherever the pass first closes.
    ClosePass,
    /// Within [`Self::SnapshotOnDevice`]: a buffer made for a snapshot, none kept spare fitting it.
    SnapshotBuffer,
    /// Within [`Self::SnapshotOnDevice`]: the copy into the snapshot recorded and submitted.
    SnapshotCopy,
    /// A dispatch's device buffer made afresh, no spare of its size and use kept (D805).
    DispatchNewBuffer,
    /// A dispatch's host buffer - its observation or window - made afresh, no spare of its size
    /// kept.
    DispatchNewHostBuffer,
    /// A dispatch's image made afresh, no spare of its format and extent kept.
    DispatchNewImage,
    /// A deferred write-back replaced, unread, by a later one of the same target at a flip, its
    /// guard kept (D719). Counted.
    DeferredSuperseded,
    /// A deferred copy dropped, unread, by a fill or copy that overwrites it whole, its pages given
    /// back (D719). Counted.
    DeferredCovered,
    /// A deferred fill dropped at a flip because the frame's write-back writes every byte of it,
    /// its pages given back (D864). Counted.
    DeferredFillDropped,
    /// Releasing a superseded snapshot.
    DiscardSnapshot,
    /// Every other command-processor memory step: fills, fences, waits.
    OtherMemory,
    /// Writing the pending frame back at the flip.
    FlipWriteBack,
    /// Carrying out a deferred copy (D717).
    CarryOut,
    /// A deferred copy's frame taken off the device snapshot it was kept in.
    CarryOutTake,
    /// Within [`Self::CarryOutTake`]: waiting for the device to finish the copy into the kept
    /// frame, before its words are read.
    CarryOutSettle,
    /// That frame laid out in the target's tiling over what memory held.
    CarryOutTile,
    /// The laid-out bytes written to the copy's destination.
    CarryOutWrite,
    /// Driving a submission's commands into the backend, on the device thread.
    Drive,
    /// Sending the recorded draws to the device.
    SubmitDraws,
    /// What the executor says about each submission on the error stream.
    ExecutorLog,
    /// Handing the backend a submission's guest-memory window.
    GuestWindow,
    /// A draw command, carried out by the backend.
    DrawCommand,
    /// Any other command, carried out by the backend.
    StateCommand,
    /// A draw that joined the open batch without being worked out again (D718).
    DrawJoined,
    /// Working out whether a joining draw's re-bound buffers leave the batch's set unchanged.
    JoinedBuffers,
    /// Making room in the draw-data buffer for a draw: when it is full, closing the pass and
    /// waiting for the device mid-frame.
    DrawRoom,
    /// A draw that could not join the open batch, and took the whole path.
    DrawWhole,
    /// A whole-path draw's guest-memory window, uploaded when it changed.
    WholeWindow,
    /// A whole-path draw's pipeline, found in the cache or built.
    WholePipeline,
    /// A whole-path draw recorded into its attachment's open pass.
    WholeRecord,
    /// Preparing a submission: walking its packets.
    PrepareWalk,
    /// Preparing a submission: collecting its register writes.
    PrepareRegisters,
    /// Preparing a submission: what its shaders are translated against - the window's place and
    /// each stage's user-data layout.
    PrepareEnvironment,
    /// Preparing a submission: its guest-memory window, read.
    PrepareWindow,
    /// Preparing a submission: its draws' shaders, found and translated or taken from the cache.
    PrepareShaders,
    /// Preparing a submission: its draws' state and geometry commands.
    PrepareGeometry,
    /// Preparing a submission: its textures, read and bound.
    PrepareTextures,
    /// Within [`Self::PrepareTextures`]: every bound texture's descriptor read for the report.
    TextureCensus,
    /// Within [`Self::PrepareTextures`]: the textures each draw samples, read or kept.
    TextureBind,
    /// Within [`Self::PrepareTextures`]: the buffers each draw reads, read and bound.
    BufferBind,
    /// Within a draw's pipeline build: its shader modules made.
    BuildModules,
    /// Within a draw's pipeline build: its textures, storage image and buffers made.
    BuildResources,
    /// Within [`Self::BuildResources`]: a texture's texels laid out as its host format stages them.
    TextureStaged,
    /// Within [`Self::BuildResources`]: a texture's device image made, its memory bound and viewed.
    TextureImage,
    /// Within [`Self::BuildResources`]: a texture's staging buffer made and its texels copied in.
    TextureStaging,
    /// Within a draw's pipeline build: the device's pipeline made.
    BuildPipeline,
    /// The driver's pipeline cache written to disk, where it grew (D744).
    KeepPipelineCache,
    /// Within [`Self::BufferBind`]: a draw buffer's range resolved from its descriptor.
    BufferResolve,
    /// Within [`Self::BufferResolve`]: the program before an access replayed for the descriptor
    /// it computes (D753), where no kept answer still holds.
    BufferEvaluate,
    /// Within [`Self::BufferBind`]: the host asked whether a kept large range was written.
    BufferWatch,
    /// Within [`Self::BufferBind`]: a small range read and compared with what was kept.
    BufferCompare,
    /// Within [`Self::BufferBind`]: a range read and hashed afresh.
    BufferRead,
    /// A texture's texels kept from an earlier read, nothing having written them since. Counted.
    TextureKept,
    /// A tiled texture's base level, kept or read and detiled.
    TextureBase,
    /// Within [`Self::TextureBase`]: a level read afresh hashed, so the same contents are known.
    BaseHash,
    /// Within [`Self::TextureBase`]: a level read afresh made into texels - detiled or packed.
    BaseTexels,
    /// A linear texture read from guest memory and hashed, its kept texels written since.
    TextureLinear,
    /// The border colours a submission's samplers name, read from its border table.
    TextureBorders,
    /// A tiled texture's levels after its base, read and detiled (D833).
    TextureLevels,
    /// A block-compressed texture's blocks, read.
    TextureBlocks,
    /// Reading a band of a pending frame's rows off the device for a guest's write (D864).
    ReadBand,
    /// Patching a band the guest wrote back onto the device (D864).
    FoldBand,
    /// The host asked whether a kept texture's pages were written since it was kept.
    TextureWatch,
    /// A texture read whose memory the host does not write-watch, so the next binding reads it
    /// again. Counted.
    TextureUnwatched,
    /// A texture's channels re-picked for its view, not having been swizzled for it before.
    TextureSwizzle,
    /// Within [`Self::CommandProcessor`]: the stream walked into packets and its draw segments
    /// found.
    CpWalk,
    /// Within [`Self::CommandProcessor`]: a compute dispatch carried out into guest memory.
    CpDispatch,
    /// Within [`Self::CpDispatch`]: its registers found and its program translated or kept.
    DispatchPrepare,
    /// Within [`Self::CpDispatch`]: its window and buffers read out of guest memory.
    DispatchRead,
    /// Within [`Self::CpDispatch`]: the device's part, handed over and answered.
    DispatchExecute,
    /// A dispatch on the device thread, which [`Span::DispatchExecute`] waits on.
    DispatchOnDevice,
    /// Within [`Self::CpDispatch`]: what it changed written back into guest memory.
    DispatchWriteBack,
    /// Within [`Self::DispatchExecute`]: its buffers and images made and filled.
    DeviceSetup,
    /// Within [`Self::DeviceSetup`]: waiting for the draws already queued to finish, since the
    /// dispatch's window was read from memory they write.
    DeviceSetupSettle,
    /// Within [`Self::DispatchExecute`]: its pipeline built.
    DevicePipeline,
    /// Within [`Self::DispatchExecute`]: recorded, submitted and waited for.
    DeviceRun,
    /// Within [`Self::DeviceRun`]: the host waiting on the dispatch's fence, from its submission to
    /// the device finishing it - the device's time and the round trip to it.
    DeviceRunWait,
    /// Within [`Self::DispatchExecute`]: its observation and guest-memory window read off the
    /// device.
    DeviceReadBack,
    /// Within [`Self::DispatchExecute`]: the image it stored read off the device.
    ReadBackImage,
    /// Within [`Self::DispatchExecute`]: its own buffers read off the device.
    ReadBackBuffers,
    /// Within [`Self::DispatchExecute`]: its images, buffers and window released.
    DeviceRelease,
    /// Within [`Self::DispatchWriteBack`]: what changed worked out, a stored image tiled back.
    WriteBackChanges,
    /// Within [`Self::WriteBackChanges`]: the window's words compared, run by run.
    ChangesWindow,
    /// Within [`Self::WriteBackChanges`]: the buffers' writes worked out.
    ChangesBuffers,
    /// Within [`Self::WriteBackChanges`]: the stored images' texels tiled back into writes.
    ChangesImages,
    /// Within [`Self::DispatchWriteBack`]: the changed bytes and fills written into guest memory.
    WriteBackGuest,
    /// Within [`Self::WriteBackGuest`]: the guards over a stretch written whole given back - a
    /// protected target's and the write-watch's (D720, D824).
    WriteBackRelease,
    /// Within [`Self::WriteBackGuest`]: a stretch's runs copied into guest memory.
    WriteBackCopy,
    /// Within [`Self::WriteBackGuest`]: the buffer fills a dispatch's results are, written.
    WriteBackFills,
    /// A command-processor fill deferred (D719): its pages guarded, carried out at the first touch.
    FillDeferred,
    /// A command-processor fill written into guest memory now.
    FillWritten,
    /// A deferral a fill overwrites whole, over exactly its pages, turned into that fill with its
    /// guard kept rather than given back and taken again. Counted.
    DeferredRefilled,
    /// Within [`Self::ReadBackBuffers`]: a slot's list of stored words read back, which answered it.
    BuffersListed,
    /// Within [`Self::ReadBackBuffers`]: a slot whose stores overflowed its list, its buffer and mask
    /// copied back on the device and waited for.
    BuffersOverflowed,
    /// Within [`Self::ReadBackBuffers`]: a slot's buffer and mask read back whole.
    BuffersWhole,
    /// Within [`Self::BuffersListed`]: the list read back, sorted into the words stored.
    BuffersGathered,
    /// A frame still on the device written back because command-processor work touched its target
    /// before the flip (D714).
    SettlePending,
    /// Within [`Self::PrepareShaders`]: the reconciled shaders prepared and bound before any draw;
    /// the rest of it is each draw's own.
    ShadersUpFront,
    /// A kept-texels, swizzle or levels cache full, and let go whole to keep a new entry. Counted:
    /// a few a run is a title touching more than the cache holds once; one every frame or so is a
    /// working set larger than the cache, every entry read afresh each time it comes back.
    TexturesEmptied,
    /// As [`Self::TexturesEmptied`], for the draw-buffer caches: a descriptor's runs, a program's
    /// prefix, or the programs interned for computed descriptors (D753).
    BuffersEmptied,
    /// As [`Self::TexturesEmptied`], for the flat-twin answers a pipeline keeps.
    TwinsEmptied,
}

impl Span {
    /// Every span, in report order.
    pub const ALL: [Self; 122] = [
        Self::SubmitCall,
        Self::QueueFull,
        Self::QueuedWrites,
        Self::SettleOver,
        Self::PrepareAgain,
        Self::CommandProcessor,
        Self::RunDraws,
        Self::ReadTargetBefore,
        Self::ReadTargetCompare,
        Self::ReadTargetEarlier,
        Self::ProtectUnderCopy,
        Self::ProtectRefused,
        Self::ProtectGuestWrote,
        Self::ProtectHostWrote,
        Self::ReadTargetRead,
        Self::ExecuteHandOff,
        Self::ExecuteOnDevice,
        Self::Copy,
        Self::CopyGuard,
        Self::KeepSnapshot,
        Self::SnapshotOnDevice,
        Self::ClosePass,
        Self::SnapshotBuffer,
        Self::SnapshotCopy,
        Self::DispatchNewBuffer,
        Self::DispatchNewHostBuffer,
        Self::DispatchNewImage,
        Self::DeferredSuperseded,
        Self::DeferredCovered,
        Self::DeferredFillDropped,
        Self::DiscardSnapshot,
        Self::OtherMemory,
        Self::FlipWriteBack,
        Self::CarryOut,
        Self::CarryOutTake,
        Self::CarryOutSettle,
        Self::CarryOutTile,
        Self::CarryOutWrite,
        Self::Drive,
        Self::SubmitDraws,
        Self::ExecutorLog,
        Self::GuestWindow,
        Self::DrawCommand,
        Self::StateCommand,
        Self::DrawJoined,
        Self::DrawRoom,
        Self::JoinedBuffers,
        Self::DrawWhole,
        Self::WholeWindow,
        Self::WholePipeline,
        Self::WholeRecord,
        Self::PrepareWalk,
        Self::PrepareRegisters,
        Self::PrepareEnvironment,
        Self::PrepareWindow,
        Self::PrepareShaders,
        Self::PrepareGeometry,
        Self::PrepareTextures,
        Self::TextureCensus,
        Self::TextureBind,
        Self::BufferBind,
        Self::BuildModules,
        Self::BuildResources,
        Self::TextureStaged,
        Self::TextureImage,
        Self::TextureStaging,
        Self::BuildPipeline,
        Self::KeepPipelineCache,
        Self::BufferResolve,
        Self::BufferEvaluate,
        Self::BufferWatch,
        Self::BufferCompare,
        Self::BufferRead,
        Self::TextureKept,
        Self::TextureBase,
        Self::BaseHash,
        Self::BaseTexels,
        Self::TextureLinear,
        Self::TextureBorders,
        Self::TextureLevels,
        Self::TextureBlocks,
        Self::ReadBand,
        Self::FoldBand,
        Self::TextureWatch,
        Self::TextureUnwatched,
        Self::TextureSwizzle,
        Self::CpWalk,
        Self::CpDispatch,
        Self::DispatchPrepare,
        Self::DispatchRead,
        Self::DispatchExecute,
        Self::DispatchOnDevice,
        Self::DispatchWriteBack,
        Self::DeviceSetup,
        Self::DeviceSetupSettle,
        Self::DevicePipeline,
        Self::DeviceRun,
        Self::DeviceRunWait,
        Self::DeviceReadBack,
        Self::ReadBackImage,
        Self::ReadBackBuffers,
        Self::DeviceRelease,
        Self::WriteBackChanges,
        Self::ChangesWindow,
        Self::ChangesBuffers,
        Self::ChangesImages,
        Self::WriteBackGuest,
        Self::WriteBackRelease,
        Self::WriteBackCopy,
        Self::WriteBackFills,
        Self::FillDeferred,
        Self::FillWritten,
        Self::DeferredRefilled,
        Self::BuffersListed,
        Self::BuffersOverflowed,
        Self::BuffersWhole,
        Self::BuffersGathered,
        Self::SettlePending,
        Self::ShadersUpFront,
        Self::TexturesEmptied,
        Self::BuffersEmptied,
        Self::TwinsEmptied,
    ];

    /// What to call it in the report.
    pub const fn label(self) -> &'static str {
        span_labels!(self, as_label)
    }

    /// Its zone in the Tracy profiler (D829), named as [`Self::label`] names it.
    #[cfg(feature = "tracy")]
    fn zone(self) -> &'static tracy_client::SpanLocation {
        span_labels!(self, as_zone)
    }
}

/// Spans a thread spends waiting on another thread's span, each with the one it waits on: the
/// profiler counts the wait as the waiting span's own time, since what it waits on runs on another
/// thread, and a report subtracts it (D831).
pub const WAITS_ON: [(Span, Span); 3] = [
    (Span::ExecuteHandOff, Span::ExecuteOnDevice),
    (Span::DispatchExecute, Span::DispatchOnDevice),
    (Span::KeepSnapshot, Span::SnapshotOnDevice),
];

/// Spans that are the host waiting for the device to finish work it was given, which a report
/// sums: a run waiting on the device for much of its time is bound by the device's work, and a
/// wait taken out of one of these reappears in another rather than leaving the frame (CRFT00001:
/// a larger draw-data buffer took "execute: draw room" from 12% to nothing and "device: setup"
/// from 5% to 17%, the frame rate unmoved).
pub const DEVICE_WAITS: [Span; 3] = [
    Span::DrawRoom,
    Span::CarryOutSettle,
    Span::DeviceSetupSettle,
];

/// Spans that are a cache let go whole because it was full, which a report counts: a cache
/// emptied about once a frame holds less than the title comes back to, and every entry it kept is
/// worked out again (PPSA04263's computed descriptors, emptied five times a second, missed 70% of
/// questions that repeat 92% of the time).
pub const EMPTIED: [Span; 3] = [
    Span::TexturesEmptied,
    Span::BuffersEmptied,
    Span::TwinsEmptied,
];

/// Spans that are one thread waiting for another's work, which is counted where it runs: a report
/// marks them, so a ranking of costs does not send anyone to make a wait faster. What they wait on
/// is the cost; [`DEVICE_WAITS`] are the waits on the device.
pub const THREAD_WAITS: [Span; 2] = [Span::QueueFull, Span::SettleOver];

/// Spans that are a pool making something afresh, each beside the span that asks the pool, which a
/// report counts: a pool that makes afresh for most asks lets go of what it is asked for again
/// (PPSA04263's snapshot buffers, 6,251 made in 6,459 asks, 17% of its run).
pub const FRESH: [(Span, Span); 4] = [
    (Span::SnapshotBuffer, Span::SnapshotOnDevice),
    (Span::DispatchNewBuffer, Span::DeviceSetup),
    (Span::DispatchNewHostBuffer, Span::DeviceSetup),
    (Span::DispatchNewImage, Span::DeviceSetup),
];

const SPANS: usize = Span::ALL.len();
static DETAIL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static SPAN_SPENT: [AtomicU64; SPANS] = [const { AtomicU64::new(0) }; SPANS];
static SPAN_COUNTED: [AtomicU64; SPANS] = [const { AtomicU64::new(0) }; SPANS];

/// Turns the finer [`Span`] measurement on or off. Set by the worker from `ORBISTOUN_PERF_DETAIL`.
pub fn set_detail(on: bool) {
    DETAIL.store(on, Ordering::Relaxed);
}

/// Whether [`Span`]s are being measured.
pub fn detail() -> bool {
    DETAIL.load(Ordering::Relaxed)
}

/// A [`Span`] measured from [`SpanGuard::start`] until the guard is dropped, for a stretch with
/// several ways out.
pub struct SpanGuard {
    span: Span,
    started: Option<std::time::Instant>,
    /// Its Tracy zone, in a profiler build (D829), ended with the guard.
    #[cfg(feature = "tracy")]
    _zone: Option<tracy_client::Span>,
}

impl std::fmt::Debug for SpanGuard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SpanGuard")
            .field("span", &self.span)
            .field("started", &self.started)
            .finish_non_exhaustive()
    }
}

impl SpanGuard {
    /// Starts measuring `span`, when detail is on, and its Tracy zone in a profiler build.
    pub fn start(span: Span) -> Self {
        Self {
            span,
            started: detail().then(std::time::Instant::now),
            #[cfg(feature = "tracy")]
            _zone: zone(span.zone()),
        }
    }
}

impl Drop for SpanGuard {
    fn drop(&mut self) {
        if let Some(started) = self.started {
            let nanos = u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX);
            SPAN_SPENT[self.span as usize].fetch_add(nanos, Ordering::Relaxed);
            SPAN_COUNTED[self.span as usize].fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// Runs `work`, adding its time to `span` when detail is on.
pub fn span<T>(span: Span, work: impl FnOnce() -> T) -> T {
    #[cfg(feature = "tracy")]
    let _zone = zone(span.zone());
    if !detail() {
        return work();
    }
    let started = std::time::Instant::now();
    let out = work();
    let nanos = u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX);
    SPAN_SPENT[span as usize].fetch_add(nanos, Ordering::Relaxed);
    SPAN_COUNTED[span as usize].fetch_add(1, Ordering::Relaxed);
    out
}

/// Each span's nanoseconds and occurrences since the last call, in [`Span::ALL`] order, and resets
/// them.
pub fn take_spans() -> [(u64, u64); SPANS] {
    std::array::from_fn(|i| {
        (
            SPAN_SPENT[i].swap(0, Ordering::Relaxed),
            SPAN_COUNTED[i].swap(0, Ordering::Relaxed),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::{Count, Phase, Span};
    use std::sync::{Mutex, MutexGuard, PoisonError};

    /// The tests that [`super::take`] share its global totals, which a take resets, so they run
    /// one at a time.
    fn serial() -> MutexGuard<'static, ()> {
        static LOCK: Mutex<()> = Mutex::new(());
        LOCK.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// A span is measured only when detail is on, and taking resets it. `Drive` because nothing in
    /// this crate enters it: the counters are global, and a copy test beside this one would count.
    #[test]
    fn a_span_counts_only_with_detail_on() {
        super::set_detail(false);
        let _ = super::take_spans();
        super::span(Span::Drive, || ());
        assert_eq!(super::take_spans()[Span::Drive as usize], (0, 0));
        super::set_detail(true);
        super::span(Span::Drive, || {
            std::thread::sleep(std::time::Duration::from_millis(1));
        });
        let (spent, counted) = super::take_spans()[Span::Drive as usize];
        assert_eq!(counted, 1);
        assert!(spent >= 1_000_000);
        super::set_detail(false);
    }

    /// The first frame keeps what was added before the first flip, survives a take, and closes
    /// once.
    #[test]
    fn the_first_frame_closes_at_the_first_flip() {
        let _guard = serial();
        super::add(Phase::Build, std::time::Duration::from_millis(2));
        let _ = super::take();
        let first = super::close_first_frame().expect("the first flip closes it");
        assert!(first.spent(Phase::Build) >= 2_000_000);
        assert_eq!(super::close_first_frame(), None);
    }

    /// What is added is what is taken, and taking resets it.
    #[test]
    fn a_take_answers_what_was_added_and_starts_again() {
        let _guard = serial();
        let _ = super::take();
        super::add(Phase::Present, std::time::Duration::from_millis(3));
        super::count(Count::Flips);
        super::count(Count::Flips);
        let first = super::take();
        assert!(first.spent(Phase::Present) >= 3_000_000);
        assert_eq!(first.counted(Count::Flips), 2);
        let second = super::take();
        assert_eq!(second.counted(Count::Flips), 0);
        assert_eq!(second.spent(Phase::Present), 0);
    }
}
