//! Where a running title's time goes (worklog 844).
//!
//! Cumulative nanoseconds per phase and counts of what happened, since the last [`take`]. Every layer
//! that does a part of presenting a frame adds to it - the command processor reading and writing the
//! target, the backend building and running draws, the worker presenting a flip - and the worker
//! reads it once a second and streams it, so the window can say what the frame rate is and which part
//! of a frame is the one to make faster. A frame rate with no breakdown says only that something is
//! slow; the breakdown says what.
//!
//! Atomics rather than a lock: the phases are timed on guest threads and the host thread that runs
//! the draws, and a sink that blocks a guest thread has changed the program it observes (principle 9).

use std::sync::atomic::{AtomicU64, Ordering};

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
    /// A whole graphics submit, from the guest's call to its return - **containing** the phases from
    /// [`Self::Prepare`] to [`Self::WriteTarget`], so it is reported beside them rather than summed
    /// with them: what it holds beyond them is the submit's own unmeasured work, and what the window
    /// holds beyond it and [`Self::Present`] is the guest's.
    Submit,
    /// The **device's** busy time, by its own clock (worklog 847) - alongside the host's, not part of
    /// it, so it is reported as a share of the window of its own and never summed with the rest.
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
        match self {
            Self::Prepare => "prepare",
            Self::ReadTarget => "read target",
            Self::Build => "build",
            Self::Seed => "seed",
            Self::Execute => "execute",
            Self::ReadBack => "read back",
            Self::Release => "release",
            Self::DrawOther => "draw setup",
            Self::WriteTarget => "write target",
            Self::Present => "present",
            Self::Submit => "submit (total)",
            Self::Gpu => "gpu busy",
        }
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
    let started = std::time::Instant::now();
    let out = work();
    add(phase, started.elapsed());
    out
}

/// Adds `spent` to `phase`.
pub fn add(phase: Phase, spent: std::time::Duration) {
    let nanos = u64::try_from(spent.as_nanos()).unwrap_or(u64::MAX);
    SPENT[phase as usize].fetch_add(nanos, Ordering::Relaxed);
}

/// Counts one of `what`.
pub fn count(what: Count) {
    COUNTED[what as usize].fetch_add(1, Ordering::Relaxed);
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

/// A finer span of presenting a frame, measured only when [`set_detail`] asked for it (worklog 852).
///
/// The phases above are always on, and cheap enough to be. These cut a submission finer - where it
/// overlaps the phases, across the thread hand-offs, around the command processor's own steps - for
/// when the question is which part of a submission to make faster. Kept rather than re-added each
/// time that question comes back; off, each costs one relaxed load.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Span {
    /// The guest's submit call, whole.
    SubmitCall,
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
    /// Carrying out a deferred copy (D717, D719).
    CarryOut,
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
}

impl Span {
    /// Every span, in report order.
    pub const ALL: [Self; 29] = [
        Self::SubmitCall,
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
    ];

    /// What to call it in the report.
    pub const fn label(self) -> &'static str {
        match self {
            Self::SubmitCall => "submit call",
            Self::CommandProcessor => "command processor",
            Self::RunDraws => "run draws",
            Self::ExecuteHandOff => "execute hand-off",
            Self::ExecuteOnDevice => "execute on device",
            Self::Copy => "dma copy",
            Self::KeepSnapshot => "keep snapshot",
            Self::DiscardSnapshot => "discard snapshot",
            Self::OtherMemory => "other memory work",
            Self::FlipWriteBack => "flip write-back",
            Self::CarryOut => "carry out",
            Self::Drive => "drive",
            Self::SubmitDraws => "submit draws",
            Self::ExecutorLog => "executor log",
            Self::GuestWindow => "guest window",
            Self::DrawCommand => "draw command",
            Self::StateCommand => "state command",
            Self::DrawJoined => "draw joined",
            Self::DrawWhole => "draw whole",
            Self::WholeWindow => "whole: window",
            Self::WholePipeline => "whole: pipeline",
            Self::WholeRecord => "whole: record",
            Self::PrepareWalk => "prepare: walk",
            Self::PrepareRegisters => "prepare: registers",
            Self::PrepareEnvironment => "prepare: environment",
            Self::PrepareWindow => "prepare: window",
            Self::PrepareShaders => "prepare: shaders",
            Self::PrepareGeometry => "prepare: geometry",
            Self::PrepareTextures => "prepare: textures",
        }
    }
}

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

/// Runs `work`, adding its time to `span` when detail is on.
pub fn span<T>(span: Span, work: impl FnOnce() -> T) -> T {
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

    /// **A span is measured only when detail is on** (worklog 852), and taking resets it.
    #[test]
    fn a_span_counts_only_with_detail_on() {
        super::set_detail(false);
        let _ = super::take_spans();
        super::span(Span::Copy, || ());
        assert_eq!(super::take_spans()[Span::Copy as usize], (0, 0));
        super::set_detail(true);
        super::span(Span::Copy, || {
            std::thread::sleep(std::time::Duration::from_millis(1));
        });
        let (spent, counted) = super::take_spans()[Span::Copy as usize];
        assert_eq!(counted, 1);
        assert!(spent >= 1_000_000);
        super::set_detail(false);
    }

    /// **What is added is what is taken, and taking resets it.**
    #[test]
    fn a_take_answers_what_was_added_and_starts_again() {
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
