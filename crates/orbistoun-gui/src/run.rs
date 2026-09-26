//! Drives a run on its own thread so the window stays responsive.
//!
//! A run lasts until the guest faults or hits the time limit, so it runs off the UI thread
//! and its result arrives through a channel. Spawning the worker and comparing traces live
//! in `orbistoun_worker` and `orbistoun_report::trace`; this file owns only the threading.

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
    /// Taken before the handle moves, because the thread owning the handle is blocked
    /// reading from it and cannot act on a stop request.
    stopper: Receiver<orbistoun_worker::Stopper>,
    /// Kept once received, so a second press does not wait on an empty channel.
    held: std::cell::Cell<Option<orbistoun_worker::Stopper>>,
    /// Carries a shell action into the running guest, from this thread.
    ///
    /// The same shape as the stopper, but it tells the title something and leaves it
    /// running, where the stopper ends the process.
    control: Receiver<orbistoun_worker::Control>,
    /// What was last sent to the guest, so an unchanged pad sends nothing.
    ///
    /// Held per run, because a new run starts knowing nothing and must be told again.
    last_sent: std::cell::RefCell<Option<Vec<orbistoun_input::PadState>>>,
    /// Kept once received, so repeated actions do not race an empty channel.
    control_held: std::cell::RefCell<Option<orbistoun_worker::Control>>,
    /// Each frame the guest presents, as it is presented.
    frames: Receiver<egui::ColorImage>,
    /// Each title the guest asked the system to start.
    launches: Receiver<String>,
    /// Where each second of the run went.
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
    /// `Err(())` means the run thread died without sending, reported rather than waited on.
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

    /// The newest frame the guest has presented since the last call, if any; older ones are
    /// skipped.
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
    /// The result still arrives through the channel as an ordinary failed run. A stopped
    /// run keeps the trace written so far, because the worker writes it as it goes.
    pub(crate) fn stop(&self) {
        if let Some(stopper) = self.held.take() {
            stopper.stop();
        }
    }

    /// Tells the running title something, and leaves it running.
    ///
    /// Answers whether it could be sent. `false` is the ordinary race with a run ending,
    /// returned so a caller that then terminates the worker knows the guest was never told.
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
    /// Answers whether anything was sent: `false` when the state has not changed.
    pub(crate) fn input(&self, pads: &[orbistoun_input::PadState]) -> bool {
        // Only when it changes: input is a level, so an unchanged pad needs no message (D345).
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

/// What a run does with pad input beyond the window's own (D721): a script to play and a
/// file to capture into, each from the run's entry and chosen on the toolbar before launch.
#[derive(Debug, Clone, Default)]
pub(crate) struct RunInput {
    /// The pad script to play.
    pub(crate) play: Option<std::path::PathBuf>,
    /// The script file to capture into.
    pub(crate) capture: Option<std::path::PathBuf>,
}

/// Starts a run on a thread of its own.
///
/// `limit` is in seconds; zero asks for no limit, the same explicit choice the CLI offers.
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
        // A failed send means the window closed during the run, which is normal.
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
    // Likewise, so a shell action sent after the run starts has somewhere to go.
    let _ = control_sender.send(worker.control());

    // Read before the run, because the run overwrites it.
    let before = orbistoun_report::trace::load_previous(traces_dir, module);

    // Each presented frame goes to the window as it arrives; a region that cannot be read is
    // skipped, since the next flip brings another.
    let events = worker.request_streaming(
        &orbistoun_proto::Request::Run {
            path: module.to_path_buf(),
            symbols_db: None,
            limit_seconds: (limit > 0).then_some(limit),
            call_budget: (budget > 0).then_some(budget),
            // The window's own pads drive a GUI run; a script or a capture only when the
            // toolbar asked for one before the launch (D721).
            input_script: input.play,
            capture_input: input.capture,
            // A library title's storage is known from where it lies (D722), and the window
            // launches only library titles.
            staged: false,
            relink: false,
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
/// The event types are `orbistoun-proto` data; how they are shown is this crate's job. A
/// `Frame` event carries only a descriptor, and its bytes are read back from a region in
/// `frames_dir` (D695) and rendered as a line.
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
