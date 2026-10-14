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
            Span::CommandProcessor => $as!("command processor"),
            Span::RunDraws => $as!("run draws"),
            Span::ExecuteHandOff => $as!("execute hand-off"),
            Span::ExecuteOnDevice => $as!("execute on device"),
            Span::Copy => $as!("dma copy"),
            Span::KeepSnapshot => $as!("keep snapshot"),
            Span::ReadBand => $as!("read band"),
            Span::FoldBand => $as!("fold band"),
            Span::DiscardSnapshot => $as!("discard snapshot"),
            Span::OtherMemory => $as!("other memory work"),
            Span::FlipWriteBack => $as!("flip write-back"),
            Span::CarryOut => $as!("carry out"),
            Span::CarryOutTake => $as!("carry out: take"),
            Span::CarryOutTile => $as!("carry out: tile"),
            Span::CarryOutWrite => $as!("carry out: write"),
            Span::Drive => $as!("drive"),
            Span::SubmitDraws => $as!("submit draws"),
            Span::ExecutorLog => $as!("executor log"),
            Span::GuestWindow => $as!("guest window"),
            Span::DrawCommand => $as!("draw command"),
            Span::StateCommand => $as!("state command"),
            Span::DrawJoined => $as!("draw joined"),
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
            Span::BuildPipeline => $as!("build: pipeline"),
            Span::KeepPipelineCache => $as!("build: keep cache"),
            Span::BufferResolve => $as!("buffers: resolve"),
            Span::BufferWatch => $as!("buffers: watch"),
            Span::BufferCompare => $as!("buffers: compare"),
            Span::BufferRead => $as!("buffers: read"),
            Span::TextureKept => $as!("textures: kept"),
            Span::TextureBase => $as!("textures: base level"),
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
            Span::DispatchWriteBack => $as!("dispatch: write back"),
            Span::DeviceSetup => $as!("device: setup"),
            Span::DevicePipeline => $as!("device: pipeline"),
            Span::DeviceRun => $as!("device: run"),
            Span::DeviceReadBack => $as!("device: read back window"),
            Span::ReadBackImage => $as!("device: read back image"),
            Span::ReadBackBuffers => $as!("device: read back buffers"),
            Span::DeviceRelease => $as!("device: release"),
            Span::WriteBackChanges => $as!("write back: changes"),
            Span::WriteBackGuest => $as!("write back: guest"),
            Span::SettlePending => $as!("settle pending frame"),
            Span::ShadersUpFront => $as!("shaders: up front"),
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
    /// The command processor's walk of a submission, draws included.
    CommandProcessor,
    /// Carrying out a submission's draws into its target.
    RunDraws,
    /// Handing the draws to the device thread and getting its answer.
    ExecuteHandOff,
    /// The draws on the device thread.
    ExecuteOnDevice,
    /// A `DMA_DATA` copy, deferred or carried out.
    Copy,
    /// Keeping a frame snapshot for a deferred copy.
    KeepSnapshot,
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
    /// Within a draw's pipeline build: the device's pipeline made.
    BuildPipeline,
    /// The driver's pipeline cache written to disk, where it grew (D744).
    KeepPipelineCache,
    /// Within [`Self::BufferBind`]: a draw buffer's range resolved from its descriptor.
    BufferResolve,
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
    /// Within [`Self::CpDispatch`]: what it changed written back into guest memory.
    DispatchWriteBack,
    /// Within [`Self::DispatchExecute`]: its buffers and images made and filled.
    DeviceSetup,
    /// Within [`Self::DispatchExecute`]: its pipeline built.
    DevicePipeline,
    /// Within [`Self::DispatchExecute`]: recorded, submitted and waited for.
    DeviceRun,
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
    /// Within [`Self::DispatchWriteBack`]: the changed bytes and fills written into guest memory.
    WriteBackGuest,
    /// A frame still on the device written back because command-processor work touched its target
    /// before the flip (D714).
    SettlePending,
    /// Within [`Self::PrepareShaders`]: the reconciled shaders prepared and bound before any draw;
    /// the rest of it is each draw's own.
    ShadersUpFront,
}

impl Span {
    /// Every span, in report order.
    pub const ALL: [Self; 70] = [
        Self::SubmitCall,
        Self::QueueFull,
        Self::CommandProcessor,
        Self::RunDraws,
        Self::ExecuteHandOff,
        Self::ExecuteOnDevice,
        Self::Copy,
        Self::KeepSnapshot,
        Self::DiscardSnapshot,
        Self::OtherMemory,
        Self::FlipWriteBack,
        Self::CarryOut,
        Self::CarryOutTake,
        Self::CarryOutTile,
        Self::CarryOutWrite,
        Self::Drive,
        Self::SubmitDraws,
        Self::ExecutorLog,
        Self::GuestWindow,
        Self::DrawCommand,
        Self::StateCommand,
        Self::DrawJoined,
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
        Self::BuildPipeline,
        Self::KeepPipelineCache,
        Self::BufferResolve,
        Self::BufferWatch,
        Self::BufferCompare,
        Self::BufferRead,
        Self::TextureKept,
        Self::TextureBase,
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
        Self::DispatchWriteBack,
        Self::DeviceSetup,
        Self::DevicePipeline,
        Self::DeviceRun,
        Self::DeviceReadBack,
        Self::ReadBackImage,
        Self::ReadBackBuffers,
        Self::DeviceRelease,
        Self::WriteBackChanges,
        Self::WriteBackGuest,
        Self::SettlePending,
        Self::ShadersUpFront,
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
pub const WAITS_ON: [(Span, Span); 1] = [(Span::ExecuteHandOff, Span::ExecuteOnDevice)];

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
