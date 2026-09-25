//! Driving a run without freezing the window.
//!
//! A guest runs until it faults or hits the time limit, which is up to twenty seconds. On
//! the UI thread that is an application that has hung, so the work happens on a thread of
//! its own and the result arrives through a channel.
//!
//! **The orchestration itself is not here.** Spawning a worker, reading the previous
//! trace, and comparing the two are all below this crate - see `orbistoun_report::trace`
//! and `orbistoun_worker`. What this file owns is "on another thread, and report back",
//! which is a property of having a window rather than a property of running a guest.

use std::sync::mpsc::{Receiver, TryRecvError, channel};

use orbistoun_report::trace::{CallTrace, Progress};

/// What a finished run produced.
pub(crate) struct Finished {
    /// Protocol events, already rendered - the shim's job.
    pub(crate) events: Vec<String>,
    /// The trace the run left behind, if it left one.
    pub(crate) trace: Option<CallTrace>,
    /// How it compares with the run before it.
    pub(crate) progress: Option<Progress>,
    /// Why it could not be run at all, if that is what happened.
    pub(crate) error: Option<String>,
}

/// A run in flight.
pub(crate) struct InFlight {
    /// The module being run, so the panel can say what it is waiting for.
    pub(crate) module: String,
    receiver: Receiver<Finished>,
    /// Terminates the worker from this thread.
    ///
    /// Taken before the handle was moved, because the thread that owns the handle is the
    /// one blocked reading from it - and that is exactly the thread that cannot act on a
    /// stop request.
    stopper: Receiver<orbistoun_worker::Stopper>,
    /// Kept once received, so a second press does not wait on an empty channel.
    held: std::cell::Cell<Option<orbistoun_worker::Stopper>>,
    /// Carries a shell action into the running guest, from this thread.
    ///
    /// The same shape as the stopper and for the same reason - but a different act. A
    /// stopper ends the process; this **tells the title something** and leaves it running,
    /// which is the difference between quitting a title and pulling its power out.
    control: Receiver<orbistoun_worker::Control>,
    /// What was last sent to the guest, so an unchanged pad sends nothing.
    ///
    /// Held here rather than in the window, because "what the worker already knows" is a
    /// fact about this run - a new run starts knowing nothing and must be told again.
    last_sent: std::cell::RefCell<Option<Vec<orbistoun_input::PadState>>>,
    /// Kept once received, so repeated actions do not race an empty channel.
    control_held: std::cell::RefCell<Option<orbistoun_worker::Control>>,
    /// Each frame the guest presents, as it is presented (worklog 841).
    frames: Receiver<egui::ColorImage>,
    /// Each title the guest asked the system to start (worklog 842).
    launches: Receiver<String>,
    /// Where each second of the run went (worklog 844).
    perf: Receiver<orbistoun_proto::PerfReport>,
}

/// Where a run's live events go as they arrive: presented frames, titles the guest asked to start,
/// and where its time went.
struct Live {
    frames: std::sync::mpsc::Sender<egui::ColorImage>,
    launches: std::sync::mpsc::Sender<String>,
    perf: std::sync::mpsc::Sender<orbistoun_proto::PerfReport>,
}

impl InFlight {
    /// The result, if it has arrived.
    ///
    /// `Err(())` means the worker thread died without sending - which should not happen,
    /// but a UI that waits forever on it would be indistinguishable from one that hung.
    pub(crate) fn poll(&self) -> Result<Option<Finished>, ()> {
        // Pick the stopper up as soon as the run thread publishes it, so a stop pressed
        // later does not race the spawn.
        if self.held.get().is_none() {
            if let Ok(stopper) = self.stopper.try_recv() {
                self.held.set(Some(stopper));
            }
        }
        if self.control_held.borrow().is_none() {
            if let Ok(control) = self.control.try_recv() {
                *self.control_held.borrow_mut() = Some(control);
            }
        }
        match self.receiver.try_recv() {
            Ok(finished) => Ok(Some(finished)),
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => Err(()),
        }
    }

    /// The newest frame the guest has presented since the last call, if any - older ones that
    /// arrived in between are skipped, because only the newest is worth showing.
    pub(crate) fn latest_frame(&self) -> Option<egui::ColorImage> {
        let mut latest = None;
        while let Ok(frame) = self.frames.try_recv() {
            latest = Some(frame);
        }
        latest
    }

    /// The newest time breakdown the worker has streamed since the last call, if any.
    pub(crate) fn latest_perf(&self) -> Option<orbistoun_proto::PerfReport> {
        self.perf.try_iter().last()
    }

    /// The title the guest last asked the system to start, if it asked since the last call.
    pub(crate) fn requested_launch(&self) -> Option<String> {
        let mut latest = None;
        while let Ok(title_id) = self.launches.try_recv() {
            latest = Some(title_id);
        }
        latest
    }

    /// Terminates the run.
    ///
    /// The result still arrives through the channel: the worker dies, the request fails,
    /// and the thread reports that as an ordinary failed run. A stopped run keeps whatever
    /// trace it had already written, because the worker writes it as it goes.
    pub(crate) fn stop(&self) {
        if let Some(stopper) = self.held.take() {
            stopper.stop();
        }
    }

    /// Tells the running title something, and leaves it running.
    ///
    /// Answers whether it could be sent. A `false` is the ordinary race between a run
    /// ending and somebody pressing a button, not a failure worth reporting - but it is
    /// **not** silently discarded either, because a caller that then terminates the worker
    /// needs to know the guest was never told.
    pub(crate) fn shell(&self, action: orbistoun_shell::Request) -> bool {
        self.control_held
            .borrow()
            .as_ref()
            .is_some_and(|control| control.shell(action).is_ok())
    }

    /// Starts capturing what the running title reads from its pad into `to`, or stops with
    /// `None` (D721). Answers whether it could be sent.
    pub(crate) fn capture_input(&self, to: Option<std::path::PathBuf>) -> bool {
        self.control_held
            .borrow()
            .as_ref()
            .is_some_and(|control| control.capture_input(to).is_ok())
    }

    /// Starts playing the pad script `script` on the running title from now, or stops with
    /// `None` (D721). Answers whether it could be sent.
    pub(crate) fn play_input(&self, script: Option<std::path::PathBuf>) -> bool {
        self.control_held
            .borrow()
            .as_ref()
            .is_some_and(|control| control.play_input(script).is_ok())
    }

    /// Tells the running title what the pads are doing.
    ///
    /// Answers whether anything was sent - `false` when the state has not changed, which is
    /// the ordinary case for most frames.
    pub(crate) fn input(&self, pads: &[orbistoun_input::PadState]) -> bool {
        // **Only when it changes.** Input is a level rather than a stream: a title asks what
        // the pad is doing now, so an unchanged pad needs no message. Sending one per frame
        // would put sixty JSON lines a second down a pipe to say nothing happened.
        if self.last_sent.borrow().as_deref() == Some(pads) {
            return false;
        }
        let sent = self
            .control_held
            .borrow()
            .as_ref()
            .is_some_and(|control| control.input(pads).is_ok());
        if sent {
            *self.last_sent.borrow_mut() = Some(pads.to_vec());
        }
        sent
    }
}

/// Starts a run on a thread of its own.
///
/// `limit` is in seconds; zero asks for no limit, which is the same explicit choice the
/// CLI offers rather than a magic sentinel.
/// What a run does with pad input beyond the window's own (D721): a script to play from its entry,
/// and a file to capture into from its entry - each chosen on the toolbar before the launch.
#[derive(Debug, Clone, Default)]
pub(crate) struct RunInput {
    /// The pad script to play.
    pub(crate) play: Option<std::path::PathBuf>,
    /// The script file to capture into.
    pub(crate) capture: Option<std::path::PathBuf>,
}

pub(crate) fn start(
    module: &std::path::Path,
    limit: u64,
    budget: u64,
    traces_dir: std::path::PathBuf,
    input: RunInput,
) -> InFlight {
    let (sender, receiver) = channel();
    let (stop_sender, stopper) = channel();
    let (control_sender, control) = channel();
    let (frame_sender, frames) = channel();
    let (launch_sender, launches) = channel();
    let (perf_sender, perf) = channel();
    let live = Live {
        frames: frame_sender,
        launches: launch_sender,
        perf: perf_sender,
    };
    let name = module.to_string_lossy().into_owned();
    let for_thread = module.to_path_buf();

    std::thread::spawn(move || {
        let finished = execute(
            &for_thread,
            (limit, budget, input),
            &traces_dir,
            (&stop_sender, &control_sender),
            &live,
        );
        // A failed send means the window closed while the guest was running, which is a
        // normal thing for a person to do and not worth reporting.
        let _ = sender.send(finished);
    });

    InFlight {
        module: name,
        receiver,
        stopper,
        held: std::cell::Cell::new(None),
        control,
        control_held: std::cell::RefCell::new(None),
        last_sent: std::cell::RefCell::new(None),
        frames,
        launches,
        perf,
    }
}

/// Runs one guest and gathers everything worth showing.
fn execute(
    module: &std::path::Path,
    (limit, budget, input): (u64, u64, RunInput),
    traces_dir: &std::path::Path,
    (stop_sender, control_sender): (
        &std::sync::mpsc::Sender<orbistoun_worker::Stopper>,
        &std::sync::mpsc::Sender<orbistoun_worker::Control>,
    ),
    live: &Live,
) -> Finished {
    let mut worker = match orbistoun_worker::WorkerHandle::spawn_self() {
        Ok(worker) => worker,
        Err(e) => return failed(format!("spawning a worker process: {e}")),
    };
    // Published before anything blocks, so a stop pressed immediately is still honoured.
    let _ = stop_sender.send(worker.stopper());
    // Same moment, same reason. From here this thread is inside the run, and a shell action
    // that arrived after it started would have nowhere to go without this.
    let _ = control_sender.send(worker.control());

    // Read before the run, because the run overwrites it. The comparison is the whole
    // reason traces are kept at all.
    let before = orbistoun_report::trace::load_previous(traces_dir, module);

    // Each presented frame goes to the window as it arrives (worklog 841); a region that cannot be
    // read is skipped - the next flip brings another.
    let events = worker.request_streaming(
        &orbistoun_proto::Request::Run {
            path: module.to_path_buf(),
            symbols_db: None,
            limit_seconds: (limit > 0).then_some(limit),
            call_budget: (budget > 0).then_some(budget),
            // The window's own pads drive a GUI run; a script or a capture only when the toolbar
            // asked for one before the launch (D721).
            input_script: input.play,
            capture_input: input.capture,
            // A library title's storage is known from where it lies (D722); the window launches
            // only library titles, so it never asks.
            staged: false,
        },
        |event| match event {
            orbistoun_proto::Event::Frame { .. } => {
                if let Ok(image) = crate::frame::frame_image(traces_dir, event) {
                    let _ = live.frames.send(image);
                }
            }
            orbistoun_proto::Event::LaunchApp { title_id } => {
                let _ = live.launches.send(title_id.clone());
            }
            orbistoun_proto::Event::Perf(report) => {
                let _ = live.perf.send(report.clone());
            }
            _ => {}
        },
    );
    let events = match events {
        Ok(events) => events.iter().map(|e| describe(e, traces_dir)).collect(),
        Err(e) => return failed(format!("driving the worker: {e}")),
    };

    // Shut down before reading the trace, so what is on disk is complete.
    if let Err(e) = worker.shutdown() {
        return failed(format!("shutting the worker down: {e}"));
    }

    let after = orbistoun_report::trace::load_previous(traces_dir, module);
    let progress = after
        .as_ref()
        .map(|after| orbistoun_report::trace::compare(before.as_ref(), after));

    Finished {
        events,
        trace: after,
        progress,
        error: None,
    }
}

impl Finished {
    /// What a stopped or lost run looks like.
    pub(crate) fn stopped() -> Self {
        Self {
            events: Vec::new(),
            trace: None,
            progress: None,
            error: Some("the run was stopped".to_owned()),
        }
    }
}

/// A run that never started.
fn failed(error: String) -> Finished {
    Finished {
        events: Vec::new(),
        trace: None,
        progress: None,
        error: Some(error),
    }
}

/// Renders one protocol event.
///
/// Presentation, which is this crate's whole remit - the event types themselves are
/// `orbistoun-proto` data and say nothing about how they are shown.
///
/// A `Frame` event carries only a descriptor; its bytes are in a region on the same shared route the
/// traces take (`frames_dir`), so this is where the shim reads them back (D695). Rendered as a line
/// today, and read whole - the image goes to a texture once a run produces one to send (36c0).
fn describe(event: &orbistoun_proto::Event, frames_dir: &std::path::Path) -> String {
    match event {
        orbistoun_proto::Event::Reached { phase } => format!("reached    {phase:?}"),
        orbistoun_proto::Event::Terminated { outcome, .. } => match outcome {
            orbistoun_proto::Outcome::Halted { reason } => format!("halted     {reason}"),
            other => format!("outcome    {other:?}"),
        },
        orbistoun_proto::Event::Failed { error } => format!("failed     {error}"),
        orbistoun_proto::Event::Frame {
            sequence,
            width,
            height,
            format,
            ..
        } => match crate::frame::frame_image(frames_dir, event) {
            Ok(image) => format!(
                "frame      #{sequence} {width}x{height} {format:?} ({} px)",
                image.pixels.len()
            ),
            Err(e) => format!("frame      #{sequence} unreadable: {e}"),
        },
        other => format!("event      {other:?}"),
    }
}
