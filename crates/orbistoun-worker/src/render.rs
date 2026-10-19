//! Driving captured graphics submissions to a headless backend.
//!
//! The submit handler in `orbistoun-gpu` captures a guest's command buffer as a [`Submission`]
//! but attaches no backend, since that crate has no graphics runtime dependency; the worker is
//! where a device is opened (D695). This module runs a submission's draws on a
//! [`VulkanBackend`], presents flipped frames, and at the end of a run drives the last
//! submission. Where no device is present it says so and the run still reports its reach.

use crate::device_thread::on_device;
use crate::frame_region::write_frame;
use orbistoun_gpu::agc_driver::Before;
use orbistoun_gpu::drive;
use orbistoun_gpu::perf;
use orbistoun_gpu::pipeline::Submission;
use orbistoun_gpu_vulkan::VulkanBackend;
use orbistoun_gpu_vulkan::compute::{Availability, probe};
use orbistoun_gpu_vulkan::framebuffer::Pixels;
use orbistoun_proto::{Event, FrameFormat};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

/// What driving a real submission to a constructed backend produced.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RenderOutcome {
    /// The graphics device the submission reached, or `None` where this build has none.
    pub device: Option<String>,
    /// RenderCommands the backend carried out.
    pub executed: usize,
    /// RenderCommands the backend refused as unsupported; each still reached it.
    pub refused: usize,
    /// The refusals by the name each carried, most frequent first.
    pub refusals: Vec<(&'static str, usize)>,
    /// The presented frame's dimensions, when one was produced.
    pub frame: Option<(u32, u32)>,
}

impl RenderOutcome {
    /// Whether a command reached the backend. A refused command counts: it arrived at a
    /// constructed backend.
    #[must_use]
    pub const fn reached_backend(&self) -> bool {
        self.device.is_some() && self.executed + self.refused > 0
    }
}

/// A submission driven to a backend: what it did, and the frame it produced.
///
/// The outcome is the small summary and the bytes the bulk, kept apart so a caller that only
/// wants the outcome does not carry a frame. Frame bytes are `Rgba8` at `outcome.frame`'s
/// dimensions.
#[derive(Debug, Clone, Default)]
pub struct Rendered {
    /// What driving the submission did.
    pub outcome: RenderOutcome,
    /// The presented frame's bytes, when one was produced.
    pub frame_bytes: Option<Vec<u8>>,
}

/// Drives a submission to a headless graphics backend, if this build has a device (D695).
///
/// Returns the default (`device: None`) where no device is available, so a headless run is reported
/// rather than refused.
#[must_use]
pub fn render(submission: &Submission) -> Rendered {
    match probe() {
        Availability::Unavailable { .. } => Rendered::default(),
        Availability::Available { properties } => {
            let mut backend = VulkanBackend::new();
            // A `BackendError` is the whole drive failing, not one command refused; the default
            // reads as "reached the device, carried nothing".
            let outcome = drive(&mut backend, submission).unwrap_or_default();
            let frame = backend.last_frame();
            Rendered {
                outcome: RenderOutcome {
                    device: Some(properties.device),
                    executed: outcome.executed,
                    refused: outcome.refused,
                    refusals: outcome.refusals.clone(),
                    frame: frame.map(|f| (f.width, f.height)),
                },
                frame_bytes: frame.map(|f| f.bytes.clone()),
            }
        }
    }
}

/// Where a live event goes as it happens: the worker process's own protocol stream, written a
/// whole message at a time so events from the presenting thread interleave only between lines.
/// Unset (a test, or the CLI) streams nothing.
static LIVE_EVENTS: OnceLock<fn(&Event)> = OnceLock::new();

/// Streams presented frames to `sink` as they are flipped. First install wins.
pub fn stream_events_to(sink: fn(&Event)) {
    let _ = LIVE_EVENTS.set(sink);
}

/// Streams a guest's request to start another title to the front end, which ends this run and
/// launches it. With no front end listening it is only logged.
pub fn request_launch(title_id: &str) {
    tracing::info!("[launch] the guest asked to start {title_id}");
    if let Some(sink) = LIVE_EVENTS.get() {
        sink(&Event::LaunchApp {
            title_id: title_id.to_owned(),
        });
    }
}

/// Streams the system keyboard's view to the front end, which draws it over the title, or `None`
/// once it has closed (D826). With no front end listening it is only logged.
pub fn keyboard_changed(view: Option<&orbistoun_shell::keyboard::View>) {
    if let Some(view) = view {
        tracing::info!("[keyboard] {:?}", view.text);
    } else {
        tracing::info!("[keyboard] closed");
    }
    if let Some(sink) = LIVE_EVENTS.get() {
        sink(&Event::Keyboard {
            view: view.cloned(),
        });
    }
}

/// The frame regions presented frames rotate through: a front end reads each as its event arrives,
/// so a few slots are enough, and a long run leaves a few frames on disk rather than one per flip.
const PRESENT_RING: u64 = 8;

/// The first sequence number of the ring, clear of the numbers the run's own ending writes.
const PRESENT_BASE: u64 = 1_000_000;

/// The shortest gap between two presented frames: a front end refreshes far slower than a title
/// that flips freely, and each frame is 8 MB detiled and written.
const PRESENT_INTERVAL: std::time::Duration = std::time::Duration::from_millis(50);

/// The scanout format the open-toolchain SDK registers (`agc_display.c`, "64-bit SDR format"),
/// whose pixels its GL context writes as B, G, R, A (`gl_draw.c`).
const SCANOUT_BGRA8: u64 = 0x8000_0000_0000_0000;

/// Whether any flip found a frame orbistoun drew waiting for write-back.
static FRAME_DRAWN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether a flip wrote a drawn frame back into the very buffer it flipped, and that frame held a
/// picture: a byte that is not zero, the test the guest-buffer branch applies (D694).
static DRAWN_FRAME_PRESENTED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// When a flipped drawn frame was last read back to look for a picture.
static PICTURE_LOOKED_FOR: Mutex<Option<std::time::Instant>> = Mutex::new(None);

/// How often a flipped drawn frame is read back while none has held a picture. A read-back is a
/// whole frame off the device, so a title that flips blank frames pays it once a second rather
/// than every flip, and nothing once a picture has been seen.
const PICTURE_LOOK_INTERVAL: std::time::Duration = std::time::Duration::from_secs(1);

/// Whether the guest flipped a buffer holding a frame orbistoun drew for it, with a picture in it:
/// the drawn half of `Reach::Presented`.
pub fn drawn_frame_presented() -> bool {
    DRAWN_FRAME_PRESENTED.load(Ordering::Relaxed)
}

/// Whether a flip found a picture in a buffer orbistoun did not draw into, put there by the guest's
/// own code - a GL context that draws offscreen and copies its frame into the scanout buffer with
/// the CPU. Looked for at the flip, where the buffer can be read, rather than at the report, where
/// a drawn run's guarded pages cannot be.
static MEMORY_FRAME_PRESENTED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// When a flipped buffer orbistoun did not draw into was last read to look for a picture.
static FLIPPED_LOOKED_AT: Mutex<Option<std::time::Instant>> = Mutex::new(None);

/// Whether a flip found the guest's own picture in a buffer orbistoun did not draw into: the
/// memory half of `Reach::Presented`, for a run that drew.
pub fn memory_frame_presented() -> bool {
    MEMORY_FRAME_PRESENTED.load(Ordering::Relaxed)
}

/// Looks for a picture in the buffer just flipped, which orbistoun did not draw into, if none has
/// been seen and a look is due: its `width * height` pixels at four bytes each, once what is
/// deferred into them has been carried out, any byte not zero (D694's test).
fn look_in_the_flipped_buffer(address: u64, shape: orbistoun_video::BufferShape) {
    if MEMORY_FRAME_PRESENTED.load(Ordering::Relaxed) {
        return;
    }
    let due = FLIPPED_LOOKED_AT.lock().is_ok_and(|mut last| {
        let due = last.is_none_or(|at| at.elapsed() >= PICTURE_LOOK_INTERVAL);
        if due {
            *last = Some(std::time::Instant::now());
        }
        due
    });
    let length = u64::from(shape.width) * u64::from(shape.height) * 4;
    if !due
        || length == 0
        || !orbistoun_kernel::is_guest_readable(address, length)
        || !orbistoun_gpu::agc_driver::carry_out_before_reading(address, length)
    {
        return;
    }
    let (Ok(at), Ok(length)) = (usize::try_from(address), usize::try_from(length)) else {
        return;
    };
    // SAFETY: `[address, address + length)` is readable guest memory (checked above) under the
    // identity mapping, and the guest mappings live for the process; anything deferred into it was
    // carried out above, so no page of it is guarded. The bytes are only read.
    let bytes =
        unsafe { std::slice::from_raw_parts(std::ptr::with_exposed_provenance::<u8>(at), length) };
    if holds_a_picture(bytes) {
        MEMORY_FRAME_PRESENTED.store(true, Ordering::Relaxed);
    }
}

/// Whether a frame's bytes hold a picture: any byte not zero. A frame cleared to zero and flipped
/// is a flip, not a presentation.
fn holds_a_picture(bytes: &[u8]) -> bool {
    bytes.iter().any(|&b| b != 0)
}

/// Looks for a picture in the drawn frame just flipped, if none has been seen and a look is due.
fn look_for_a_picture() {
    if DRAWN_FRAME_PRESENTED.load(Ordering::Relaxed) {
        return;
    }
    let due = PICTURE_LOOKED_FOR.lock().is_ok_and(|mut last| {
        let due = last.is_none_or(|at| at.elapsed() >= PICTURE_LOOK_INTERVAL);
        if due {
            *last = Some(std::time::Instant::now());
        }
        due
    });
    if due && executed_frame().is_some_and(|(_, _, bytes)| holds_a_picture(&bytes)) {
        DRAWN_FRAME_PRESENTED.store(true, Ordering::Relaxed);
    }
}

/// Whether any frame was drawn this run. Its targets are guarded for lazy write-back (D717), so a
/// report must not read them.
pub fn any_frame_drawn() -> bool {
    FRAME_DRAWN.load(Ordering::Relaxed)
}

/// Shows the frame the guest just flipped: the [`orbistoun_video::FlipObserver`] the worker
/// installs. The flipped buffer is read out of guest memory, detiled, put in `Rgba8` order,
/// written to a frame region and streamed as an [`Event::Frame`] as it is presented.
///
/// A buffer in a layout this does not know is reported once and not shown, rather than shown
/// wrong.
pub fn present_flip(address: u64, shape: orbistoun_video::BufferShape) {
    static LAST: Mutex<Option<std::time::Instant>> = Mutex::new(None);
    perf::count(perf::Count::Flips);
    // The user's mods' cheats, written at each flip (D836).
    crate::cheats::at_flip();
    if let Some(first) = perf::close_first_frame() {
        let ms = |phase| std::time::Duration::from_nanos(first.spent(phase)).as_secs_f64() * 1000.0;
        let parts: Vec<String> = perf::Phase::ALL
            .iter()
            .filter(|&&phase| phase != perf::Phase::Submit && first.spent(phase) > 0)
            .map(|&phase| format!("{} {:.1}", phase.label(), ms(phase)))
            .collect();
        tracing::info!(
            "first frame: {:.1} ms of host submit work; ms: {}",
            ms(perf::Phase::Submit),
            parts.join(", ")
        );
    }
    let listening = LIVE_EVENTS.get().copied().zip(frames_dir());
    let due = listening.is_some()
        && LAST.lock().is_ok_and(|mut last| {
            let due = last.is_none_or(|at| at.elapsed() >= PRESENT_INTERVAL);
            if due {
                *last = Some(std::time::Instant::now());
            }
            due
        });
    // The frame drawn since the last flip is written back to guest memory on every flip, shown or
    // not (D714), since the guest's own scanout is what is honoured. A frame due in the window
    // comes with a device copy of its own. One the guest read before flipping was written back
    // then, and is still the frame drawn into this buffer.
    let drawn_into = orbistoun_gpu::agc_driver::frame_drawn_into(address);
    if let Ok(mut seen) = TOUCHES.lock() {
        note_touches(&mut seen, orbistoun_gpu::agc_driver::take_early_touches());
    }
    crate::flip_origins::note_flip(address, drawn_into);
    ask_for_unfilled_texture(address);
    if drawn_into.is_some() {
        keep_frame_at_this_flip();
    }
    let (written, shown) = orbistoun_gpu::agc_driver::write_back_at_this_flip_showing(due);
    if !written {
        tracing::warn!("a drawn frame could not be written back at the flip");
    }
    if drawn_into.is_some() {
        FRAME_DRAWN.store(true, Ordering::Relaxed);
    }
    if written && drawn_into == Some(address) {
        look_for_a_picture();
    } else if drawn_into != Some(address) {
        look_in_the_flipped_buffer(address, shape);
    }
    // Decided on every flip, window or not, so a headless run's record can say whether the window
    // would have shown what was flipped.
    LAST_FLIP_SHOWABLE.store(
        window_can_show(address, shape, drawn_into),
        Ordering::Relaxed,
    );
    report_perf(LIVE_EVENTS.get().copied());
    let Some((sink, dir)) = listening.filter(|_| due) else {
        if let Some(shown) = shown {
            discard_snapshot(shown.snapshot);
        }
        return;
    };
    perf::measure(perf::Phase::Present, || match shown {
        Some(shown) if shows_directly(address, shape, shown) => {
            present_later(PresentJob {
                frame: Presented::Shown(shown),
                sink,
                dir,
            });
        }
        other => {
            if let Some(shown) = other {
                discard_snapshot(shown.snapshot);
            }
            present_now(address, shape, sink, dir);
        }
    });
}

/// Whether the window can show the buffer flipped at `address`: from the device copy when the frame
/// was drawn into exactly that buffer (D719, D784), or read from memory when its scanout format is
/// one [`scanout_decoder`] knows. Either way only video-out tiling 0, the one [`present_now`]
/// detiles. A buffer it cannot show leaves the window empty while the flip still counts.
fn window_can_show(
    address: u64,
    shape: orbistoun_video::BufferShape,
    drawn_into: Option<u64>,
) -> bool {
    shape.tiling == 0 && (drawn_into == Some(address) || scanout_decoder(shape.format).is_some())
}

/// Whether the last flip was of a buffer the window can show; see [`window_can_show`].
static LAST_FLIP_SHOWABLE: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Whether the window could show the buffer the guest last flipped, decided at that flip whether a
/// window was attached or not. False before any flip.
pub fn last_flip_showable() -> bool {
    LAST_FLIP_SHOWABLE.load(Ordering::Relaxed)
}

/// Whether the flipped buffer is exactly the frame's target, so the device copy is the frame with
/// no memory to read (D719). Its video-out format does not decide it: the device copy holds the
/// colours the title drew, which is what a working title shows (D784).
fn shows_directly(
    address: u64,
    shape: orbistoun_video::BufferShape,
    shown: orbistoun_gpu::agc_driver::ShownFrame,
) -> bool {
    shape.tiling == 0
        && address == shown.target.base
        && (shape.width, shape.height) == (shown.target.width, shown.target.height)
}

/// A frame the window will show, handed to the thread that shows it: its device copy, or a copy
/// of the flipped buffer's tiled words with how its pixels decode.
enum Presented {
    /// The device copy of the frame drawn into the flipped buffer (D719).
    Shown(orbistoun_gpu::agc_driver::ShownFrame),
    /// The flipped buffer's words as the flip left them, still tiled.
    Memory {
        tiled: Vec<u32>,
        shape: orbistoun_video::BufferShape,
        decode: fn(u32) -> u32,
    },
}

/// A frame to show, and where it goes.
struct PresentJob {
    frame: Presented,
    sink: fn(&Event),
    dir: &'static Path,
}

/// Whether the presenting thread is busy with a frame, so a flip can drop the next without
/// copying it first.
static PRESENTING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Shows a flipped frame on a thread of its own, so the guest does not wait for the window: it
/// waited 6 to 8 ms a frame for a buffer detiled from memory and written out, up to twenty times
/// a second. A frame arriving while another is being shown is dropped, as a slow display misses
/// frames; answers whether it was taken.
fn present_later(job: PresentJob) -> bool {
    static PRESENTER: OnceLock<Option<std::sync::mpsc::SyncSender<PresentJob>>> = OnceLock::new();
    let presenter = PRESENTER.get_or_init(|| {
        let (send, receive) = std::sync::mpsc::sync_channel::<PresentJob>(1);
        std::thread::Builder::new()
            .name("orbistoun-present".to_owned())
            .spawn(move || {
                for job in receive {
                    PRESENTING.store(true, Ordering::Release);
                    match job.frame {
                        Presented::Shown(shown) => present_shown(shown, job.sink, job.dir),
                        Presented::Memory {
                            tiled,
                            shape,
                            decode,
                        } => present_tiled(&tiled, shape, decode, (job.sink, job.dir)),
                    }
                    PRESENTING.store(false, Ordering::Release);
                }
            })
            .ok()
            .map(|_| send)
    });
    let Some(send) = presenter.as_ref() else {
        return false;
    };
    match send.try_send(job) {
        Ok(()) => true,
        Err(
            std::sync::mpsc::TrySendError::Full(job)
            | std::sync::mpsc::TrySendError::Disconnected(job),
        ) => {
            if let Presented::Shown(shown) = job.frame {
                discard_snapshot(shown.snapshot);
            }
            false
        }
    }
}

/// Reads a shown frame's device copy and streams it, converting each pixel from its memory order
/// to `Rgba8` as [`present_tiled`] does from memory (D719).
fn present_shown(shown: orbistoun_gpu::agc_driver::ShownFrame, sink: fn(&Event), dir: &Path) {
    let Some((extent, frame)) = take_sized_snapshot(shown.snapshot) else {
        return;
    };
    // The device copy is linear `Rgba8`, the colours the title drew: shown as-is (D784), which for
    // the SDK's alternate-order targets is exactly what their B, G, R, A scanout shows. At the
    // extent it was drawn at, which a render scale makes larger than the target's (D862).
    let bytes = zerocopy::IntoBytes::as_bytes(frame.as_slice());
    emit_presented(extent, bytes, sink, dir);
}

/// A scanout pixel, B, G, R, A in memory, as `Rgba8`: red and blue trade places.
const fn scanout_rgba(word: u32) -> u32 {
    (word & 0xff00_ff00) | ((word >> 16) & 0xff) | ((word & 0xff) << 16)
}

/// Writes a presented frame into the ring and streams its descriptor.
fn emit_presented((width, height): (u32, u32), bytes: &[u8], sink: fn(&Event), dir: &Path) {
    static SHOWN: AtomicU64 = AtomicU64::new(0);
    let sequence = PRESENT_BASE + SHOWN.fetch_add(1, Ordering::Relaxed) % PRESENT_RING;
    match write_frame(dir, sequence, width, height, FrameFormat::Rgba8, bytes) {
        Ok(event) => sink(&event),
        Err(e) => tracing::warn!("a presented frame could not be written: {e}"),
    }
}

/// How often the time breakdown is streamed to the window.
const PERF_INTERVAL: std::time::Duration = std::time::Duration::from_secs(1);

/// Streams where the last second went, once a second: the flips, submissions and draws in it and
/// each measured phase's share, for the window to draw over the picture. With no window
/// listening it is logged instead.
fn report_perf(sink: Option<fn(&Event)>) {
    static SINCE: Mutex<Option<std::time::Instant>> = Mutex::new(None);
    let window = {
        let Ok(mut since) = SINCE.lock() else {
            return;
        };
        let Some(started) = *since else {
            *since = Some(std::time::Instant::now());
            let _ = perf::take();
            return;
        };
        let window = started.elapsed();
        if window < PERF_INTERVAL {
            return;
        }
        *since = Some(std::time::Instant::now());
        window
    };
    let taken = perf::take();
    let ms = |nanos: u64| std::time::Duration::from_nanos(nanos).as_secs_f64() * 1000.0;
    // The finer spans, when `ORBISTOUN_PERF_DETAIL` asks for them: each one's time and count,
    // logged in report order and streamed most time first (D831).
    let spans = if perf::detail() {
        let mut spans: Vec<(String, f64, u64)> = perf::Span::ALL
            .iter()
            .zip(perf::take_spans())
            .filter(|(_, (_, counted))| *counted > 0)
            .map(|(span, (spent, counted))| (span.label().to_owned(), ms(spent), counted))
            .collect();
        let logged: Vec<String> = spans
            .iter()
            .map(|(name, spent, counted)| format!("{name} {spent:.1} ms/{counted}"))
            .collect();
        tracing::info!("perf detail: {}", logged.join(", "));
        spans.sort_by(|a, b| b.1.total_cmp(&a.1));
        spans
    } else {
        Vec::new()
    };
    let report = orbistoun_proto::PerfReport {
        window_ms: window.as_secs_f64() * 1000.0,
        flips: taken.counted(perf::Count::Flips),
        submissions: taken.counted(perf::Count::Submissions),
        draws: taken.counted(perf::Count::Draws),
        phases: perf::Phase::ALL
            .iter()
            .map(|&phase| {
                let name = match phase {
                    perf::Phase::Submit => orbistoun_proto::SUBMIT_TOTAL_PHASE,
                    perf::Phase::Gpu => orbistoun_proto::GPU_BUSY_PHASE,
                    _ => phase.label(),
                };
                (name.to_owned(), ms(taken.spent(phase)))
            })
            .collect(),
        spans,
    };
    if let Ok(mut run) = run_perf().lock() {
        run.add(&report);
    }
    if let Some(sink) = sink {
        sink(&Event::Perf(report));
    } else {
        let phases: Vec<String> = report
            .phases
            .iter()
            .filter(|(_, ms)| *ms > 0.0)
            .map(|(name, ms)| format!("{name} {ms:.0}"))
            .collect();
        tracing::info!(
            "perf over {:.0} ms: {} flips, {} submissions, {} draws; ms: {}",
            report.window_ms,
            report.flips,
            report.submissions,
            report.draws,
            phases.join(", ")
        );
    }
}

/// The run's perf windows that flipped, summed: their milliseconds, flips and each phase's time.
#[derive(Debug, Default)]
struct RunPerf {
    window_ms: f64,
    flips: u64,
    phases: Vec<(String, f64)>,
    /// The windows by their frames' shape - draws a flip - each shape's milliseconds, flips and
    /// phases summed.
    shapes: std::collections::BTreeMap<u64, Stretch>,
}

/// Windows summed: their milliseconds, flips and each phase's milliseconds.
type Stretch = (f64, u64, Vec<(String, f64)>);

/// How many frame shapes [`RunPerf::describe_shapes`] describes, those with the most flips.
const SHAPES_DESCRIBED: usize = 3;

/// Adds `phases` into `into`, phase by phase.
fn add_phases(into: &mut Vec<(String, f64)>, phases: &[(String, f64)]) {
    for (name, ms) in phases {
        match into.iter_mut().find(|(kept, _)| kept == name) {
            Some((_, total)) => *total += ms,
            None => into.push((name.clone(), *ms)),
        }
    }
}

impl RunPerf {
    /// Adds a window, when it flipped: a window with no flip is not part of any frame's time.
    fn add(&mut self, report: &orbistoun_proto::PerfReport) {
        if report.flips == 0 {
            return;
        }
        self.window_ms += report.window_ms;
        self.flips += report.flips;
        add_phases(&mut self.phases, &report.phases);
        let shape = self
            .shapes
            .entry(report.draws / report.flips)
            .or_insert_with(|| (0.0, 0, Vec::new()));
        shape.0 += report.window_ms;
        shape.1 += report.flips;
        add_phases(&mut shape.2, &report.phases);
    }

    /// The run's line: each phase's milliseconds a flip, costliest first, the flips and their rate,
    /// and the flip's whole length - what of a frame the host spends where. `None` before a flip.
    fn describe(&self) -> Option<String> {
        per_flip_line(
            "host time per flip over the windows that flipped",
            (self.window_ms, self.flips),
            &self.phases,
        )
    }

    /// The same for each frame shape - windows of the same draws a flip - of the
    /// [`SHAPES_DESCRIBED`] with the most flips, most first: a run whose fast frames drown the
    /// average still names what its other frames spend.
    fn describe_shapes(&self) -> Vec<String> {
        let mut shapes: Vec<_> = self.shapes.iter().collect();
        shapes.sort_by(|a, b| b.1.1.cmp(&a.1.1).then(a.0.cmp(b.0)));
        shapes
            .into_iter()
            .take(SHAPES_DESCRIBED)
            .filter_map(|(draws, (window_ms, flips, phases))| {
                per_flip_line(
                    &format!("frames of {draws} draws"),
                    (*window_ms, *flips),
                    phases,
                )
            })
            .collect()
    }
}

/// `lead`, then each of `phases`' milliseconds over `flips` flips in `window_ms`, costliest first,
/// with the flips' rate and length. `None` with no flip.
#[allow(clippy::cast_precision_loss)] // a run's flip count is far below 2^52
fn per_flip_line(
    lead: &str,
    (window_ms, flips): (f64, u64),
    phases: &[(String, f64)],
) -> Option<String> {
    if flips == 0 || window_ms <= 0.0 {
        return None;
    }
    let count = flips as f64;
    let mut per_flip: Vec<(&str, f64)> = phases
        .iter()
        .filter(|(_, ms)| *ms > 0.0)
        .map(|(name, ms)| (name.as_str(), ms / count))
        .collect();
    per_flip.sort_by(|a, b| b.1.total_cmp(&a.1));
    let parts: Vec<String> = per_flip
        .iter()
        .map(|(name, ms)| format!("{name} {ms:.2} ms"))
        .collect();
    Some(format!(
        concat!(
            "{}: {} flips at {:.1} a second; {} - ",
            "the rest of each {:.1} ms flip is the guest's own time and what nothing measures"
        ),
        lead,
        flips,
        count * 1000.0 / window_ms,
        parts.join(", "),
        window_ms / count
    ))
}

/// The run's perf windows so far.
fn run_perf() -> &'static Mutex<RunPerf> {
    static RUN: OnceLock<Mutex<RunPerf>> = OnceLock::new();
    RUN.get_or_init(Default::default)
}

/// The scanout format PPSA28061 registers, whose memory its draws write as R, G, B, A (`8_8_8_8`
/// sRGB, standard swap); the title shows correct colours on a console, so the display reads it in
/// that order (D784).
const SCANOUT_RGBA8: u64 = 0x8000_0000_2200_0000;

/// An R, G, B, A word, unchanged.
const fn as_rgba(word: u32) -> u32 {
    word
}

/// How a scanout format's memory decodes to R, G, B, A, or `None` for a format not known.
fn scanout_decoder(format: u64) -> Option<fn(u32) -> u32> {
    match format {
        SCANOUT_BGRA8 => Some(scanout_rgba),
        SCANOUT_RGBA8 => Some(as_rgba),
        _ => None,
    }
}

/// Copies a flipped buffer as the flip left it, for the presenting thread to detile and stream -
/// the guest goes on once its words are copied. Nothing is copied while a frame is still being
/// shown, since that frame would be dropped.
fn present_now(
    address: u64,
    shape: orbistoun_video::BufferShape,
    sink: fn(&Event),
    dir: &'static Path,
) {
    static REFUSED: std::sync::Once = std::sync::Once::new();
    if PRESENTING.load(Ordering::Acquire) {
        return;
    }
    let (width, height) = (shape.width, shape.height);
    let decode = scanout_decoder(shape.format);
    let Some((tiled, decode)) = decode
        .filter(|_| shape.tiling == 0)
        .and_then(|decode| Some((read_scanout(address, width, height)?, decode)))
    else {
        REFUSED.call_once(|| {
            tracing::warn!(
                "a flipped buffer at {address:#x} ({width}x{height}, format {:#x}, tiling {}) is not one this shows - no live frames",
                shape.format, shape.tiling
            );
        });
        return;
    };
    present_later(PresentJob {
        frame: Presented::Memory {
            tiled,
            shape,
            decode,
        },
        sink,
        dir,
    });
}

/// Detiles a flipped buffer's copied words and streams them as a frame, on the presenting thread.
fn present_tiled(
    tiled: &[u32],
    shape: orbistoun_video::BufferShape,
    decode: fn(u32) -> u32,
    (sink, dir): (fn(&Event), &Path),
) {
    let (width, height) = (shape.width, shape.height);
    if tiled.len() < orbistoun_gpu::tiling::surface_words_64kb_rx_bpp4(width, height) {
        return;
    }
    // The format's byte order to R, G, B, A, as each texel is detiled.
    let linear =
        orbistoun_gpu::tiling::detile_surface_64kb_rx_bpp4_mapped(tiled, width, height, 0, decode);
    let bytes = zerocopy::IntoBytes::as_bytes(linear.as_slice());
    emit_presented((width, height), bytes, sink, dir);
}

/// The words of a tiled 32-bpp scanout buffer, or `None` when guest memory does not cover it.
fn read_scanout(address: u64, width: u32, height: u32) -> Option<Vec<u32>> {
    let words = orbistoun_gpu::tiling::surface_words_64kb_rx_bpp4(width, height);
    let length = u64::try_from(words * 4).ok()?;
    if width == 0 || height == 0 || !orbistoun_kernel::is_guest_readable(address, length) {
        return None;
    }
    // A flipped frame reaches memory when something reads it (D719): this is something.
    if !orbistoun_gpu::agc_driver::carry_out_before_reading(address, length) {
        return None;
    }
    let at = usize::try_from(address).ok()?;
    // SAFETY: `[address, address + length)` is readable guest memory (checked above) under the
    // identity mapping, and the guest mappings live for the process; anything deferred into it
    // was carried out above, so no page of it is guarded. The bytes are copied out at once.
    let bytes = unsafe {
        std::slice::from_raw_parts(std::ptr::with_exposed_provenance::<u8>(at), words * 4)
    };
    // One copy of the bytes as they lie, little-endian words on this host: the flip waits for it.
    let mut copied = vec![0u32; words];
    zerocopy::IntoBytes::as_mut_bytes(copied.as_mut_slice()).copy_from_slice(bytes);
    Some(copied)
}

/// How many drawn submissions a run dumps, in order.
const DUMPED_SUBMISSIONS: u64 = 64;

/// The file in the run's frames directory holding the latest frame drawn at submit.
const LATEST_DRAWN_FRAME: &str = "latest-drawn-frame.rgba";

/// A frame drawn at submit: width, height, and its `Rgba8` bytes.
type ExecutedFrame = (u32, u32, Vec<u8>);

/// The frame the most recent submission left on its target when its draws ran at submit, as
/// `Rgba8` with its dimensions: what the run's ending presents. Asked of the live backend, on
/// the device thread.
fn executed_frame() -> Option<ExecutedFrame> {
    on_device(|| {
        let mut held = live_backend()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let frame = held.as_mut()?.last_frame()?;
        Some((frame.width, frame.height, frame.bytes.clone()))
    })
    .flatten()
}

/// Gives each colour attachment after the first what it holds before a submission's draws (D825):
/// the [`orbistoun_gpu::agc_driver::MoreTargetsSeeder`] the worker installs. On the device thread,
/// with the run's backend.
pub fn seed_more_targets(targets: &[(orbistoun_gpu::ResourceId, (u32, u32), &[u32])]) -> bool {
    on_device(|| {
        let Availability::Available { .. } = probe() else {
            return false;
        };
        let mut held = live_backend()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let backend = held.get_or_insert_with(VulkanBackend::new);
        for &(target, (width, height), words) in targets {
            backend.seed_target(
                target,
                Pixels {
                    width,
                    height,
                    bytes: zerocopy::IntoBytes::as_bytes(words).to_vec(),
                },
            );
        }
        true
    })
    .unwrap_or(false)
}

/// The samples every draw that counts them has passed so far (D827): the
/// [`orbistoun_gpu::agc_driver::SampleCounter`] the worker installs. On the device thread.
pub fn samples_passed() -> Option<u64> {
    on_device(|| {
        live_backend()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get_or_insert_with(VulkanBackend::new)
            .samples_passed()
    })
    .flatten()
}

/// What a colour attachment after the first holds after the draws, as linear `Rgba8` words (D825):
/// the [`orbistoun_gpu::agc_driver::TargetReader`] the worker installs.
pub fn read_target_frame(target: orbistoun_gpu::ResourceId) -> Option<Vec<u32>> {
    on_device(move || {
        let mut held = live_backend()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let frame = held.as_mut()?.target_frame(target)?;
        Some(
            frame
                .bytes
                .chunks_exact(4)
                .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                .collect(),
        )
    })
    .flatten()
}

/// Carries out a submission's draws at the moment it is submitted: the
/// [`orbistoun_gpu::agc_driver::DrawExecutor`] the worker installs.
///
/// The target starts from `before`, what the guest's own memory held. Every command must be
/// carried out: a single refusal means the frame is not the one the draws make, so it answers
/// `false` and the draws stay unexecuted (D705), as it does where the device is absent.
///
/// Runs on the device thread with the guest thread waiting: the submit that calls this runs on a
/// guest stack the host's exception dispatch cannot walk, and a graphics driver raises and
/// handles exceptions as ordinary business.
pub fn execute_draws(submission: &Submission, before: Before<'_>) -> bool {
    perf::span(perf::Span::ExecuteHandOff, || {
        on_device(|| {
            perf::span(perf::Span::ExecuteOnDevice, || {
                execute_draws_here(submission, before)
            })
        })
    })
    .flatten()
    .is_some()
}

/// Runs a guest compute dispatch on the device, on the device thread, for the command processor
/// (installed as its dispatch executor).
pub fn execute_dispatch(
    module: (&[u32], u64),
    memory: &[u32],
    (push, groups): (&[u32], [u32; 3]),
    (images, buffers): (
        &orbistoun_gpu::pipeline::DispatchImages,
        &[orbistoun_gpu::dispatch::BufferBefore],
    ),
) -> Result<orbistoun_gpu::agc_driver::DispatchDone, String> {
    on_device(|| execute_dispatch_here(module, memory, (push, groups), (images, buffers)))
        .unwrap_or_else(|| Err("the device thread did not answer".to_owned()))
}

/// [`execute_dispatch`] on the device thread.
fn execute_dispatch_here(
    module: (&[u32], u64),
    memory: &[u32],
    (push, groups): (&[u32], [u32; 3]),
    (images, buffers): (
        &orbistoun_gpu::pipeline::DispatchImages,
        &[orbistoun_gpu::dispatch::BufferBefore],
    ),
) -> Result<orbistoun_gpu::agc_driver::DispatchDone, String> {
    let (fetched, more_fetched, stored_before) = with_depths(images)?;
    let more: Vec<_> = more_fetched.iter().map(dispatch_image).collect();
    let stored: Vec<_> = stored_before.iter().map(dispatch_image).collect();
    let bound = orbistoun_gpu_vulkan::compute_images::DispatchImages {
        fetched: fetched.as_ref().map(dispatch_image),
        more_fetched: &more,
        stored: &stored,
    };
    // A buffer the program only stores to goes over unread, as its length alone (D809).
    let buffers: Vec<orbistoun_gpu_vulkan::BufferIn<'_>> = buffers
        .iter()
        .map(|buffer| match buffer {
            orbistoun_gpu::dispatch::BufferBefore::Words(words) => {
                orbistoun_gpu_vulkan::BufferIn::Words(words)
            }
            orbistoun_gpu::dispatch::BufferBefore::Unread(words) => {
                orbistoun_gpu_vulkan::BufferIn::Unread(*words)
            }
        })
        .collect();
    let done = perf::span(perf::Span::DispatchOnDevice, || {
        orbistoun_gpu_vulkan::dispatch_guest_hashed(
            module,
            memory,
            (push, groups),
            &bound,
            &buffers,
        )
    })
    .map_err(|e| format!("the device refused it: {e:?}"))?;
    write_depths(images, &done.stored)?;
    let buffers = done
        .buffers
        .into_iter()
        .map(|out| orbistoun_gpu::dispatch::BufferAfter {
            words: out.words,
            written: out.written,
            listed: out.stored,
            whole: out.whole,
        })
        .collect();
    Ok((done.memory, done.escaped, done.stored, buffers))
}

/// The images a dispatch binds, fetched and stored to: each that is a depth target the host holds
/// filled with its depths as the draws before left them (D894). Refused for a depth target the host
/// never drew, or one whose extent is not the image's.
type BoundTexels = (
    Option<orbistoun_gpu::pipeline::DispatchTexels>,
    Vec<orbistoun_gpu::pipeline::DispatchTexels>,
    Vec<orbistoun_gpu::pipeline::DispatchTexels>,
);

/// [`BoundTexels`] for `images`.
fn with_depths(images: &orbistoun_gpu::pipeline::DispatchImages) -> Result<BoundTexels, String> {
    let mut fetched = images.fetched.clone();
    let mut more: Vec<_> = images
        .more_fetched
        .iter()
        .map(|(texels, _)| texels.clone())
        .collect();
    let mut stored: Vec<_> = images
        .stored
        .iter()
        .map(|image| image.texels.clone())
        .collect();
    let stored_depth: Vec<(usize, orbistoun_gpu::ResourceId)> = images
        .stored
        .iter()
        .enumerate()
        .filter_map(|(n, image)| image.depth.map(|target| (n, target)))
        .collect();
    if images.fetched_depth.is_empty() && stored_depth.is_empty() {
        return Ok((fetched, more, stored));
    }
    let mut held = live_backend()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let backend = held.get_or_insert_with(VulkanBackend::new);
    let mut fill = |image: &mut orbistoun_gpu::pipeline::DispatchTexels, target| {
        let (depths, _, extent) = backend
            .depth_texels(target)
            .map_err(|e| format!("its depth target could not be read: {e:?}"))?
            .ok_or("it reaches a depth target no draw here drew")?;
        if extent != (image.width, image.height) {
            return Err(format!(
                "it reaches a {}x{} depth target as a {}x{} image",
                extent.0, extent.1, image.width, image.height
            ));
        }
        image.texels = depths;
        Ok::<(), String>(())
    };
    for &(slot, target) in &images.fetched_depth {
        let image = match slot {
            0 => fetched.as_mut(),
            n => more.get_mut(n as usize - 1),
        }
        .ok_or("a depth target's slot is past the images fetched")?;
        fill(image, target)?;
    }
    for (n, target) in stored_depth {
        fill(&mut stored[n], target)?;
    }
    Ok((fetched, more, stored))
}

/// Writes what a dispatch stored to each depth target the host holds back into that target
/// (D894): guest memory is not where the host keeps it, so nothing else would carry it there.
fn write_depths(
    images: &orbistoun_gpu::pipeline::DispatchImages,
    stored: &[Vec<u32>],
) -> Result<(), String> {
    let mut targets = images
        .stored
        .iter()
        .zip(stored)
        .filter_map(|(image, texels)| image.depth.map(|target| (target, texels)))
        .peekable();
    if targets.peek().is_none() {
        return Ok(());
    }
    let mut held = live_backend()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let backend = held.get_or_insert_with(VulkanBackend::new);
    for (target, texels) in targets {
        let written = backend
            .write_depth_texels(target, texels)
            .map_err(|e| format!("its depth target could not be written: {e:?}"))?;
        if !written {
            return Err("it stores to a depth target no draw here drew".to_owned());
        }
    }
    Ok(())
}

/// One dispatch image as the device takes it, borrowing its texels.
fn dispatch_image(
    image: &orbistoun_gpu::pipeline::DispatchTexels,
) -> orbistoun_gpu_vulkan::compute_images::DispatchImage<'_> {
    orbistoun_gpu_vulkan::compute_images::DispatchImage {
        texels: &image.texels,
        format: match image.format {
            // A dispatch image is never a packed-float or sRGB one: `dispatch_format` reads neither,
            // so their arm is the four-byte one's.
            orbistoun_gpu::pipeline::TexelFormat::Rgba8
            | orbistoun_gpu::pipeline::TexelFormat::Float11_11_10
            | orbistoun_gpu::pipeline::TexelFormat::Rgba8Srgb => {
                orbistoun_gpu_vulkan::compute_images::DispatchFormat::Rgba8
            }
            orbistoun_gpu::pipeline::TexelFormat::Rgba32Uint => {
                orbistoun_gpu_vulkan::compute_images::DispatchFormat::Rgba32Uint
            }
            orbistoun_gpu::pipeline::TexelFormat::R32Uint => {
                orbistoun_gpu_vulkan::compute_images::DispatchFormat::R32Uint
            }
            orbistoun_gpu::pipeline::TexelFormat::R16Uint => {
                orbistoun_gpu_vulkan::compute_images::DispatchFormat::R16Uint
            }
            orbistoun_gpu::pipeline::TexelFormat::Rgba16Float => {
                orbistoun_gpu_vulkan::compute_images::DispatchFormat::Rgba16Float
            }
            orbistoun_gpu::pipeline::TexelFormat::R32Float => {
                orbistoun_gpu_vulkan::compute_images::DispatchFormat::R32Float
            }
            orbistoun_gpu::pipeline::TexelFormat::R8Uint => {
                orbistoun_gpu_vulkan::compute_images::DispatchFormat::R8Uint
            }
            orbistoun_gpu::pipeline::TexelFormat::Rgba8Uint => {
                orbistoun_gpu_vulkan::compute_images::DispatchFormat::Rgba8Uint
            }
            orbistoun_gpu::pipeline::TexelFormat::R8 => {
                orbistoun_gpu_vulkan::compute_images::DispatchFormat::R8
            }
            orbistoun_gpu::pipeline::TexelFormat::Rgb10A2 => {
                orbistoun_gpu_vulkan::compute_images::DispatchFormat::Rgb10A2
            }
        },
        width: image.width,
        height: image.height,
        depth: image.depth,
        layers: image.layers,
    }
}

/// The backend a running guest's submissions are drawn on, kept for the run.
fn live_backend() -> &'static Mutex<Option<VulkanBackend>> {
    static LIVE: Mutex<Option<VulkanBackend>> = Mutex::new(None);
    &LIVE
}

/// When the latest drawn frame was last kept on disk.
static LATEST_KEPT: Mutex<Option<std::time::Instant>> = Mutex::new(None);

fn execute_draws_here(submission: &Submission, before: Before<'_>) -> Option<()> {
    static TRACE: OnceLock<bool> = OnceLock::new();
    // Every drawn submission, not only the last: a GL frame is several, since the context submits
    // whenever its vertex ring fills. Numbered in order and bounded.
    static DUMPED: AtomicU64 = AtomicU64::new(0);
    let Availability::Available { .. } = probe() else {
        return None;
    };
    let number = DUMPED.fetch_add(1, Ordering::Relaxed);
    if number < DUMPED_SUBMISSIONS {
        write_submission(submission, &format!("drawn-{number:02}"));
    }
    keep_submission_before_named_flip(submission);
    let (&target, extent) = submission.targets.iter().next()?;
    let started = std::time::Instant::now();
    // One backend for the run, because the pipeline names each shader and resource to the backend
    // once, on first sight, and a later submission refers to them. It also keeps shader modules and
    // the guest-memory window resident between submissions.
    let mut held = live_backend()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let backend = held.get_or_insert_with(VulkanBackend::new);
    match before {
        Before::Words(before) => perf::measure(perf::Phase::DrawOther, || {
            backend.seed_target(
                target,
                Pixels {
                    width: extent.width,
                    height: extent.height,
                    // The words' own bytes, little-endian as the host is, in one copy.
                    bytes: zerocopy::IntoBytes::as_bytes(before).to_vec(),
                },
            );
        }),
        // A clear, filled on the device rather than handed over as megabytes.
        Before::Uniform(word) => perf::measure(perf::Phase::DrawOther, || {
            backend.seed_target_uniform(target, (extent.width, extent.height), word);
        }),
        // The target is the frame this backend last produced; with nothing held for it the
        // draws are declined rather than drawn on a clear.
        Before::Held if backend.holds_target(target, (extent.width, extent.height)) => {}
        Before::Held => return None,
    }
    let outcome = match perf::span(perf::Span::Drive, || drive(backend, submission)) {
        Ok(outcome) => outcome,
        Err(why) => {
            tracing::warn!(
                "a submission's draws could not run at submit, in {} ms: {why}",
                started.elapsed().as_millis()
            );
            write_failed_submission(submission);
            return None;
        }
    };
    perf::count(perf::Count::Submissions);
    // Reported from submissions as well as flips, for titles that submit many times per frame.
    report_perf(LIVE_EVENTS.get().copied());
    // Every submission's line only when asked (`ORBISTOUN_TRACE_SUBMITS`); a refusal is
    // always said, below.
    let trace = *TRACE.get_or_init(|| orbistoun_env::TRACE_SUBMITS.get().as_deref() == Some("1"));
    perf::span(perf::Span::ExecutorLog, || {
        if !trace && outcome.refused == 0 {
            return;
        }
        tracing::info!(
            "a submission's draws ran at submit: {} command(s), {} refused, in {} ms{}{}",
            outcome.executed,
            outcome.refused,
            started.elapsed().as_millis(),
            if outcome.refused == 0 {
                ""
            } else {
                " - not written back"
            },
            refusal_lines(&outcome.refusals)
        );
    });
    if outcome.refused > 0 {
        count_refusals(&outcome.refusals);
        return None;
    }
    // A dumped submission's frame is read now, for its snapshot; every other frame stays on the
    // device until it is written back (D714).
    if number < DUMPED_SUBMISSIONS {
        let frame = perf::measure(perf::Phase::ReadBack, || backend.last_frame())?;
        snapshot_frame(number, frame);
    }
    if let Err(e) = perf::span(perf::Span::SubmitDraws, || backend.submit_draws()) {
        tracing::warn!("a submission's draws could not be sent to the device: {e:?}");
        return None;
    }
    Some(())
}

/// The frame the last submissions drew, read off the device as linear `Rgba8` words: the
/// [`orbistoun_gpu::agc_driver::FrameReader`] the worker installs, called when a drawn target is
/// written back (D714). On the device thread, since reading waits on the graphics driver.
pub fn read_frame() -> Option<Vec<u32>> {
    on_device(read_frame_here).flatten()
}

fn read_frame_here() -> Option<Vec<u32>> {
    let mut held = live_backend()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let backend = held.as_mut()?;
    let frame = perf::measure(perf::Phase::ReadBack, || backend.last_frame())?;
    keep_latest_frame(frame);
    // One copy where the bytes are word-aligned, as a large allocation is; word at a time
    // otherwise.
    Some(
        <[u32] as zerocopy::FromBytes>::ref_from_bytes(&frame.bytes).map_or_else(
            |_| {
                frame
                    .bytes
                    .chunks_exact(4)
                    .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                    .collect()
            },
            <[u32]>::to_vec,
        ),
    )
}

/// Rows `first` to `first + count` of the frame the last submissions drew, read off the device as
/// linear `Rgba8` words: the [`orbistoun_gpu::agc_driver::RowsReader`] the worker installs, for a
/// guest's write into a frame still on the device (D864).
pub fn read_frame_rows(first: u32, count: u32) -> Option<Vec<u32>> {
    on_device(move || {
        let mut held = live_backend()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let rows = held.as_mut()?.last_frame_rows((first, count))?;
        Some(
            rows.bytes
                .chunks_exact(4)
                .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                .collect(),
        )
    })
    .flatten()
}

/// Gives rows `first` to `first + count` of the frame the last submissions drew new texels, linear
/// `Rgba8` words: the [`orbistoun_gpu::agc_driver::RowsPatcher`] the worker installs (D864).
pub fn patch_frame_rows(first: u32, count: u32, texels: &[u32]) -> bool {
    let bytes = zerocopy::IntoBytes::as_bytes(texels).to_vec();
    on_device(move || {
        live_backend()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_mut()
            .is_some_and(|backend| backend.patch_last_frame_rows((first, count), &bytes))
    })
    .unwrap_or(false)
}

/// Keeps the frame the executor last drew as it stands now, on the device: the snapshot a
/// deferred copy of the colour target reads later.
pub fn keep_frame_on_device() -> Option<u64> {
    on_device(|| {
        live_backend()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_mut()?
            .snapshot_last_frame()
    })
    .flatten()
}

/// Keeps the frame the executor last drew for the display, at the extent it was drawn at (D862).
pub fn keep_frame_shown() -> Option<u64> {
    on_device(|| {
        live_backend()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_mut()?
            .snapshot_last_frame_shown()
    })
    .flatten()
}

/// Releases kept frame `old` unread and keeps the frame as it stands now, in one trip to the
/// device thread.
pub fn replace_snapshot(old: u64) -> Option<u64> {
    on_device(move || {
        let mut held = live_backend()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let backend = held.as_mut()?;
        backend.drop_snapshot(old);
        backend.snapshot_last_frame()
    })
    .flatten()
}

/// A kept frame's pixels as linear `Rgba8` words, releasing it: read into `into`'s storage,
/// whatever it held.
pub fn take_snapshot(id: u64, into: Vec<u32>) -> Option<Vec<u32>> {
    on_device(move || {
        live_backend()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_mut()?
            .take_snapshot_words_into(id, into)
    })
    .flatten()
    .map(|(_, words)| words)
}

/// [`take_snapshot`], with the extent the frame was kept at: a display copy's is the drawn extent
/// (D862).
fn take_sized_snapshot(id: u64) -> Option<((u32, u32), Vec<u32>)> {
    on_device(move || {
        live_backend()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_mut()?
            .take_snapshot_words(id)
    })
    .flatten()
}

/// Releases a kept frame unread.
pub fn discard_snapshot(id: u64) {
    on_device(move || {
        if let Some(backend) = live_backend()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_mut()
        {
            backend.drop_snapshot(id);
        }
    });
}

/// Which guest flips have the frame drawn before them kept: `ORBISTOUN_FRAME_EVERY` and
/// `ORBISTOUN_FRAME_AT`, by the guest's own flip index.
#[derive(Debug, Default, PartialEq, Eq)]
struct KeptFlips {
    /// Flips 1, 1+N, 1+2N ...
    every: Option<u64>,
    /// These flips exactly.
    at: Vec<u64>,
}

impl KeptFlips {
    /// From the two variables' values; anything that is not a positive number is ignored.
    fn parse(every: Option<&str>, at: Option<&str>) -> Self {
        let positive = |value: &str| value.trim().parse::<u64>().ok().filter(|&n| n > 0);
        Self {
            every: every.and_then(positive),
            at: at
                .map(|list| list.split(',').filter_map(positive).collect())
                .unwrap_or_default(),
        }
    }

    /// Whether the frame drawn before guest flip `flip` (counted from 1) is kept.
    fn wants(&self, flip: u64) -> bool {
        flip > 0
            && (self.every.is_some_and(|every| (flip - 1) % every == 0) || self.at.contains(&flip))
    }
}

/// The flips `ORBISTOUN_FRAME_EVERY` and `ORBISTOUN_FRAME_AT` ask for, read once.
fn kept_flips() -> &'static KeptFlips {
    static ASKED: OnceLock<KeptFlips> = OnceLock::new();
    ASKED.get_or_init(|| {
        KeptFlips::parse(
            orbistoun_env::FRAME_EVERY.get().as_deref(),
            orbistoun_env::FRAME_AT.get().as_deref(),
        )
    })
}

/// Where kept frames, and the submissions kept with them, go: `ORBISTOUN_FRAME_DIR` names a
/// directory, such as the CLI's sheet directory; without it they go to the run's own frames
/// directory (D865).
fn kept_frames_dir() -> Option<&'static Path> {
    static OWN: OnceLock<Option<PathBuf>> = OnceLock::new();
    OWN.get_or_init(|| orbistoun_env::FRAME_DIR.get().map(PathBuf::from))
        .as_deref()
        .or_else(frames_dir)
}

/// The most submissions kept before one flip: a frame is several, and a bound keeps a flip that
/// submits without end from filling the disk.
const MOST_KEPT_PER_FLIP: usize = 64;

/// The name a submission carried out after guest flip `last_flip` is kept under, when the next
/// flip is one `ORBISTOUN_FRAME_AT` names: `flip-<next>-submission-<k>`, `k` counting the
/// submissions before that flip in `seen` (the flip, and how many so far). Only the flips named
/// exactly keep submissions; `ORBISTOUN_FRAME_EVERY` keeps frames alone.
fn kept_submission_name(
    asked: &KeptFlips,
    last_flip: Option<u64>,
    seen: &mut (u64, usize),
) -> Option<String> {
    let next = last_flip.unwrap_or(0) + 1;
    if !asked.at.contains(&next) {
        return None;
    }
    if seen.0 != next {
        *seen = (next, 0);
    }
    if seen.1 >= MOST_KEPT_PER_FLIP {
        return None;
    }
    let name = format!("flip-{next:06}-submission-{:02}", seen.1);
    seen.1 += 1;
    Some(name)
}

/// Keeps `submission` beside the frame of the flip after it, when that flip is one
/// `ORBISTOUN_FRAME_AT` names: the per-draw lines see only the run's last submission, and this is
/// what drew a frame mid-run.
fn keep_submission_before_named_flip(submission: &Submission) {
    static SEEN: Mutex<(u64, usize)> = Mutex::new((0, 0));
    let asked = kept_flips();
    if asked.at.is_empty() {
        return;
    }
    let name = kept_submission_name(
        asked,
        orbistoun_video::last_flip_index(),
        &mut SEEN
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
    );
    if let (Some(name), Some(dir)) = (name, kept_frames_dir()) {
        if let Err(e) = std::fs::create_dir_all(dir) {
            tracing::warn!("a kept submission could not be written: {e}");
            return;
        }
        write_submission_to(dir, submission, &name);
    }
}

/// Keeps the frame drawn before this flip when `ORBISTOUN_FRAME_EVERY` or `ORBISTOUN_FRAME_AT`
/// asks, named by the guest's flip index: the index a console reports for the same flip, so a
/// kept frame can be compared with the console's frame of that index.
fn keep_frame_at_this_flip() {
    let Some(flip) = orbistoun_video::last_flip_index() else {
        return;
    };
    if !kept_flips().wants(flip) {
        return;
    }
    let Some(dir) = kept_frames_dir() else {
        return;
    };
    let Some((width, height, bytes)) = executed_frame() else {
        return;
    };
    let written = std::fs::create_dir_all(dir).and_then(|()| {
        std::fs::write(
            dir.join(format!("flip-{flip:06}-{width}x{height}.rgba")),
            bytes,
        )
    });
    if let Err(e) = written {
        tracing::warn!("a kept frame could not be written: {e}");
    }
}

/// Keeps a written-back frame on disk, at most once a second, for whoever reads it after the run.
fn keep_latest_frame(frame: &Pixels) {
    // Raw `Rgba8` at the target's extent, so a run that ends mid-frame still leaves the last
    // completed picture. At most once a second: writing 8 MB per submission is costly, and only a
    // run's ending reads the file.
    let due = LATEST_KEPT.lock().is_ok_and(|mut kept| {
        let due = kept.is_none_or(|at| at.elapsed() >= std::time::Duration::from_secs(1));
        if due {
            *kept = Some(std::time::Instant::now());
        }
        due
    });
    if due
        && let Some(dir) = frames_dir()
        && let Err(e) = std::fs::write(dir.join(LATEST_DRAWN_FRAME), &frame.bytes)
    {
        tracing::warn!("the drawn frame could not be kept: {e}");
    }
}

/// A dumped submission's frame as a quarter-scale `Rgba8` snapshot (every fourth pixel of every
/// fourth row), so a run's frames can be looked at in sequence cheaply.
fn snapshot_frame(number: u64, frame: &Pixels) {
    if let Some(dir) = frames_dir() {
        let (w, h) = (frame.width as usize, frame.height as usize);
        let mut small = Vec::with_capacity((w / 4) * (h / 4) * 4);
        for y in (0..h).step_by(4) {
            for x in (0..w).step_by(4) {
                let at = (y * w + x) * 4;
                small.extend_from_slice(&frame.bytes[at..at + 4]);
            }
        }
        let name = format!(
            "drawn-{number:02}-frame-{}x{}.rgba",
            w.div_ceil(4),
            h.div_ceil(4)
        );
        if let Err(e) = std::fs::write(dir.join(name), small) {
            tracing::warn!("the drawn frame snapshot could not be kept: {e}");
        }
    }
}

/// A texture the last flip's frame sampled, as a file: its name - order, slot and size - and its
/// texels as `Rgba8` bytes in memory order, as they were bound; a one-byte texture's bytes as its
/// lanes read them.
fn sampled_dump(
    number: usize,
    texture: &orbistoun_gpu::agc_driver::HeldTexture,
) -> (String, Vec<u8>) {
    let (slot, width, height, ..) = texture;
    let name = format!("sampled-{number:02}-slot{slot}-{width}x{height}.rgba");
    (
        name,
        as_rgba8(texture)
            .iter()
            .flat_map(|texel| texel.to_le_bytes())
            .collect(),
    )
}

/// A bound texture's texels as `Rgba8` words: as they are, or a one-byte texture's bytes as its
/// lanes read them.
fn as_rgba8(
    (_, width, height, texels, encoding): &orbistoun_gpu::agc_driver::HeldTexture,
) -> std::borrow::Cow<'_, [u32]> {
    match *encoding {
        orbistoun_gpu::TextureEncoding::R8 { lanes } => std::borrow::Cow::Owned(
            orbistoun_gpu::byte_lanes_as_rgba8(lanes, texels, *width as usize * *height as usize),
        ),
        _ => std::borrow::Cow::Borrowed(&**texels),
    }
}

/// Keeps every texture `by` sampled in the run's frames directory, so what a frame shows nothing of
/// can be looked at as orbistoun bound it.
fn keep_sampled(by: &orbistoun_gpu::agc_driver::DrawnBy) {
    let Some(dir) = frames_dir() else {
        return;
    };
    for (number, texture) in by.textures.iter().enumerate() {
        let (name, bytes) = sampled_dump(number, texture);
        if let Err(e) = std::fs::write(dir.join(name), bytes) {
            tracing::warn!("a sampled texture could not be kept: {e}");
        }
    }
}

/// Every buffer `commands` bind for their draws, whole: by command, stage, slot and guest address,
/// four little-endian words a line after their offset.
fn buffers_listing(commands: &[orbistoun_gpu::RenderCommand]) -> String {
    let mut listing = String::new();
    for (at, command) in commands.iter().enumerate() {
        let orbistoun_gpu::RenderCommand::BindDrawBuffers { stage, buffers } = command else {
            continue;
        };
        for (slot, buffer) in buffers.iter().enumerate() {
            let _ = writeln!(
                listing,
                "command {at}, {stage:?} slot {slot}, {} bytes at {:#x}",
                buffer.bytes.len(),
                buffer.base
            );
            for (row, chunk) in buffer.bytes.chunks(16).enumerate() {
                let words: Vec<String> = chunk
                    .chunks(4)
                    .map(|word| {
                        let mut padded = [0_u8; 4];
                        padded[..word.len()].copy_from_slice(word);
                        format!("{:08x}", u32::from_le_bytes(padded))
                    })
                    .collect();
                let _ = writeln!(listing, "  +{:#06x}  {}", row * 16, words.join(" "));
            }
        }
    }
    listing
}

/// What one draw did to a frame: the pixels that differ from the frame before it.
#[derive(Debug, Default, PartialEq, Eq)]
struct FrameChange {
    /// How many pixels differ.
    pixels: usize,
    /// The rectangle they lie in, `(left, top, right, bottom)`, inclusive.
    bounds: Option<(usize, usize, usize, usize)>,
    /// The first changed pixel's new `Rgba8` bytes.
    first: Option<[u8; 4]>,
}

/// The pixels of `after` that differ from `before`, two `Rgba8` frames `width` pixels wide.
fn frame_change(before: &[u8], after: &[u8], width: usize) -> FrameChange {
    let mut change = FrameChange::default();
    for (index, (old, new)) in before
        .chunks_exact(4)
        .zip(after.chunks_exact(4))
        .enumerate()
    {
        if old == new {
            continue;
        }
        let (x, y) = (index % width.max(1), index / width.max(1));
        change.pixels += 1;
        change.first.get_or_insert([new[0], new[1], new[2], new[3]]);
        change.bounds = Some(change.bounds.map_or((x, y, x, y), |(l, t, r, b)| {
            (l.min(x), t.min(y), r.max(x), b.max(y))
        }));
    }
    change
}

/// Where a capturing geometry stage writes the vertices and primitives it declared, in the
/// observation buffer: `orbistoun_translate::wavefront::CAPTURED_COUNTS`, after its first vertices'
/// positions.
const CAPTURED_COUNTS: usize = 12;

/// What a geometry stage captured in `words` (`ORBISTOUN_CAPTURE_POSITIONS`): the vertices and
/// primitives it declared, then its first vertices' clip positions, four words each, with where
/// each lands after the divide. A draw that changes nothing either rasterised no fragment, which
/// these say - no vertex ran, no primitive was declared, or every one lies off screen - or wrote
/// what was already there.
fn positions_line(words: &[u32]) -> String {
    // The buffer is zeroed before each drive, and no position a vertex writes is all zeros.
    if words.iter().all(|&word| word == 0) {
        return String::from("none - no vertex of it ran");
    }
    let (positions, counts) = words.split_at(CAPTURED_COUNTS.min(words.len()));
    let declared = match counts {
        [vertices, primitives, ..] => {
            format!("{vertices} vertices and {primitives} primitives declared; ")
        }
        _ => String::new(),
    };
    let landed = positions
        .chunks_exact(4)
        .map(|vertex| {
            let [x, y, z, w] = [0, 1, 2, 3].map(|at| f32::from_bits(vertex[at]));
            let clip = format!("clip ({x}, {y}, {z}, {w})");
            if w > 0.0 {
                format!("{clip} at ({}, {})", x / w, y / w)
            } else {
                format!("{clip} behind the eye")
            }
        })
        .collect::<Vec<_>>()
        .join("; ");
    declared + &landed
}

/// Whether `command` draws.
const fn is_draw(command: &orbistoun_gpu::RenderCommand) -> bool {
    matches!(
        command,
        orbistoun_gpu::RenderCommand::Draw { .. }
            | orbistoun_gpu::RenderCommand::DrawChunks { .. }
            | orbistoun_gpu::RenderCommand::DrawIndexed { .. }
    )
}

/// `submission` with blending off at every draw: what a draw's fragments write lands whatever
/// their alpha.
fn without_blending(submission: &Submission) -> Submission {
    let mut opaque = submission.clone();
    for command in &mut opaque.commands {
        if let orbistoun_gpu::RenderCommand::SetBlend(blend) = command {
            blend.enable = false;
        }
    }
    opaque
}

/// The lines saying what each of `draws` did to the frame with blending off: a draw that changes
/// nothing blended but something opaque wrote transparent fragments, and one that changes nothing
/// either way drew no fragment at all - culled, clipped or outside the target. GTA V's logo draws
/// change nothing either way.
fn opaque_draw_changes(submission: &Submission, draws: &[usize], width: u32) -> String {
    let opaque = without_blending(submission);
    let through = |end: usize| {
        let mut prefix = opaque.clone();
        prefix.commands.truncate(end);
        render(&prefix).frame_bytes
    };
    let mut said = String::from("\n  with blending off:");
    let mut previous: Option<Vec<u8>> = None;
    for (number, &at) in draws.iter().enumerate() {
        let Some(after) = through(at + 1) else {
            break;
        };
        let change = previous.as_ref().map_or_else(
            || frame_change(&vec![0; after.len()], &after, width as usize),
            |before| frame_change(before, &after, width as usize),
        );
        let _ = write!(said, "\n    draw {number}: ");
        match (change.bounds, change.first) {
            (Some((left, top, right, bottom)), Some(colour)) => {
                let _ = write!(
                    said,
                    "{} pixel(s) in ({left},{top})-({right},{bottom}), the first {:02x}{:02x}{:02x}{:02x}",
                    change.pixels, colour[0], colour[1], colour[2], colour[3]
                );
            }
            _ => said.push_str("changed nothing"),
        }
        previous = Some(after);
    }
    said
}

/// The most draws [`log_draw_changes`] replays: each is a drive of the submission so far.
const MOST_DRAWS_REPLAYED: usize = 32;

/// Says, for each draw of the last submission, what it did to the frame: the submission is driven
/// once to just before its first draw and again through each draw, and each frame compared with the
/// one before it. A draw that changes nothing drew nothing visible - culled, clipped, blended away
/// or written where nothing shows - which no count of draws or texels can tell apart from one that
/// drew (GTA V's title drew its "Continue" and not its logo).
fn log_draw_changes(submission: &Submission) {
    let draws: Vec<usize> = submission
        .commands
        .iter()
        .enumerate()
        .filter(|(_, command)| is_draw(command))
        .map(|(at, _)| at)
        .take(MOST_DRAWS_REPLAYED)
        .collect();
    let Some(&first) = draws.first() else {
        return;
    };
    // Driven to a backend of its own, the submission carries every module it binds: the run's
    // backend holds the ones it was given before, and this one holds none.
    let whole = orbistoun_gpu::agc_driver::with_every_bound_module(submission);
    let capturing = orbistoun_env::CAPTURE_POSITIONS.get().as_deref() == Some("1");
    // Each drive ends with the draw measured, so the shared observation buffer holds its captured
    // positions when the run asked for them.
    let positions = || {
        capturing
            .then(orbistoun_gpu_vulkan::framebuffer::shared_observation)
            .flatten()
            .map(|words| positions_line(&words))
    };
    let through = |end: usize| {
        let mut prefix = whole.clone();
        prefix.commands.truncate(end);
        if capturing {
            // Zero, so a draw that runs no vertex reads as zeros, not the draw before's.
            orbistoun_gpu_vulkan::framebuffer::clear_shared_observation();
        }
        let rendered = render(&prefix);
        rendered.frame_bytes.zip(rendered.outcome.frame)
    };
    // A frame exists only once a draw has drawn it, so the first draw is measured against black:
    // what it leaves lit.
    let Some((first_frame, (width, _))) = through(first + 1) else {
        return;
    };
    let first_positions = positions();
    let mut before = vec![0; first_frame.len()];
    let mut said = String::from(
        "what each draw of the last submission did to its frame (the first against black):",
    );
    for (number, &at) in draws.iter().enumerate() {
        let (after, landed) = if number == 0 {
            (first_frame.clone(), first_positions.clone())
        } else if let Some((after, _)) = through(at + 1) {
            (after, positions())
        } else {
            break;
        };
        let change = frame_change(&before, &after, width as usize);
        let _ = write!(said, "\n  draw {number} (command {at}): ");
        match (change.bounds, change.first) {
            (Some((left, top, right, bottom)), Some(colour)) => {
                let _ = write!(
                    said,
                    "{} pixel(s) in ({left},{top})-({right},{bottom}), the first now {:02x}{:02x}{:02x}{:02x}",
                    change.pixels, colour[0], colour[1], colour[2], colour[3]
                );
            }
            _ => said.push_str("changed nothing"),
        }
        if let Some(landed) = landed {
            let _ = write!(
                said,
                "
    its first vertices: {landed}"
            );
        }
        before = after;
    }
    said.push_str(&opaque_draw_changes(&whole, &draws, width));
    // A draw that changed nothing may never have run: what the backend refused of the submission,
    // driven whole the same way, says which.
    let whole_outcome = render(&whole).outcome;
    let _ = write!(
        said,
        "\n  driven whole: {} command(s) carried out, {} refused",
        whole_outcome.executed, whole_outcome.refused
    );
    for (why, count) in &whole_outcome.refusals {
        let _ = write!(said, "\n    refused {count:>4}  {why}");
    }
    tracing::info!("{said}");
}

/// Writes a submission whose drive failed on the device into the run's frames directory, one
/// command per numbered line, with each shader module's SPIR-V beside it for a disassembler.
fn write_failed_submission(submission: &Submission) {
    write_submission(submission, "failed");
}

/// What a bound texture holds, at a glance: its first texel, how many distinct texels it has - a
/// texture of one value, nothing written to it, reading as `1 distinct` - and how many are lit, a
/// colour channel set whatever the alpha: a mostly black picture reads as few lit. Counted over at
/// most 4096 texels spread evenly over the whole texture, so a large texture costs no more to list
/// than a small one and a picture below its first rows still counts.
fn texel_summary(texels: &[u32]) -> String {
    const COUNTED: usize = 4096;
    let step = texels.len().div_ceil(COUNTED).max(1);
    let counted: Vec<u32> = texels.iter().step_by(step).copied().collect();
    let distinct: std::collections::BTreeSet<u32> = counted.iter().copied().collect();
    // The low three bytes in memory order are the colour channels; the top one is the alpha.
    let lit = counted
        .iter()
        .filter(|&&texel| texel & 0x00ff_ffff != 0)
        .count();
    let first = texels.first().copied().unwrap_or(0);
    let bound = if step > 1 {
        " among 4096 spread over it"
    } else {
        ""
    };
    format!(
        "first {first:#010x}, {} distinct, {lit} of {} lit{bound}",
        distinct.len(),
        counted.len()
    )
}

/// Where the presented frames came from - a frame orbistoun drew, or another writer's - and what
/// drew the frame the last flip showed: its draws and what they sampled.
fn log_flip_origins() {
    let origins = crate::flip_origins::flip_origins();
    if let Some(line) = origins.describe() {
        tracing::info!("{line}");
    }
    if let Some(line) = orbistoun_gpu::detiled::describe(&orbistoun_gpu::detiled::by_layout()) {
        tracing::info!("{line}");
    }
    if let Ok(run) = run_perf().lock() {
        for line in run.describe().into_iter().chain(run.describe_shapes()) {
            tracing::info!("{line}");
        }
    }
    let Some(shown) = origins.last else {
        return;
    };
    let into = orbistoun_gpu::agc_driver::last_drawn_into;
    let census = orbistoun_gpu::agc_driver::dispatch_writes_by_module();
    let frame_into = |base: u64| filled_by(base, &census);
    let Some(by) = into(shown) else {
        tracing::info!("{}", undrawn_line(shown));
        return;
    };
    let lead = format!("the last flip showed {shown:#x}, drawn into as");
    tracing::info!("{}", drawn_by_line(&lead, &by, &frame_into));
    keep_sampled(&by);
    // Each render target it sampled, followed to the frame that drew it: what a black frame
    // composited from black targets is traced through.
    for (base, target) in sampled_targets(&by, MOST_TRACED_TARGETS, &into) {
        let lead = format!("  it sampled {base:#x}, drawn into as");
        tracing::info!("{}", drawn_by_line(&lead, &target, &frame_into));
    }
    // What the guest read or wrote of drawn frames before their flips.
    if let Some(line) = TOUCHES.lock().ok().and_then(|seen| touches_line(&seen)) {
        tracing::info!("{line}");
    }
    // Who wrote the texture a frame sampled that nothing tracked filled, where one was watched.
    if let Some((address, writers)) = crate::waits::texture_writers() {
        tracing::info!("{}", texture_writers_line(address, &writers));
    }
}

/// The most textures [`drawn_by_line`] lists.
const MOST_DRAWN_TEXTURES: usize = 8;

/// The last flip's line where no frame orbistoun drew went into the buffer it showed.
fn undrawn_line(shown: u64) -> String {
    format!("the last flip showed {shown:#x}, which no frame orbistoun drew went into")
}

/// A program's address as a report names it: with the region the run placed there and how far
/// in, where `locate` knows one.
fn program_named(at: Option<u64>, locate: &dyn Fn(u64) -> Option<(&'static str, u64)>) -> String {
    let Some(at) = at else {
        return "?".to_owned();
    };
    match locate(at) {
        Some((region, into)) => format!("{at:#x} ({region}+{into:#x})"),
        None => format!("{at:#x}"),
    }
}

/// Who read the `bytes` at `at` a program stored: a drawn frame that sampled a texture starting
/// inside them, or a program whose dispatches read a buffer or fetched an image overlapping them.
fn consumed_by(
    at: u64,
    bytes: u64,
    frames: &[orbistoun_gpu::agc_driver::DrawnBy],
    census: &[(u64, orbistoun_gpu::agc_driver::DispatchWrites)],
) -> String {
    let inside = |base: u64| (at..at.saturating_add(bytes)).contains(&base);
    let overlaps = |(from, length): (u64, u64)| {
        from < at.saturating_add(bytes) && at < from.saturating_add(length)
    };
    if let Some(frame) = frames
        .iter()
        .find(|by| by.sampled.iter().any(|&(base, _, _)| inside(base)))
    {
        return format!("sampled by frame {}", frame.frame);
    }
    if let Some(frame) = frames
        .iter()
        .find(|by| by.read.iter().any(|&range| overlaps(range)))
    {
        return format!("read as a buffer by the draws of frame {}", frame.frame);
    }
    let reader = census.iter().find(|(_, writes)| {
        writes.fetched.is_some_and(overlaps)
            || writes
                .buffers
                .iter()
                .any(|&(from, length, read)| read.is_some() && overlaps((from, length)))
    });
    match reader {
        Some((_, writes)) => format!(
            "read by the program at {}",
            writes
                .program
                .map_or_else(|| "an unknown address".to_owned(), |p| format!("{p:#x}"))
        ),
        None => "nothing tracked read it".to_owned(),
    }
}

/// How the memory at `base` was filled, where something orbistoun tracks filled it: a frame it
/// drew into it, a file read, a `DMA_DATA` the command processor carried out, or a compute
/// program's stores (`census`).
fn filled_by(
    base: u64,
    census: &[(u64, orbistoun_gpu::agc_driver::DispatchWrites)],
) -> Option<String> {
    orbistoun_gpu::agc_driver::last_drawn_into(base)
        .map(|by| format!("drawn into as frame {}", by.frame))
        .or_else(|| orbistoun_kernel::apr::read_landed_at(base))
        .or_else(|| orbistoun_gpu::cp::dma_landed_at(base))
        .or_else(|| stored_by(base, census))
}

/// The first texture `by`'s draws sampled that `filled` names no filler for.
fn first_unfilled(
    by: &orbistoun_gpu::agc_driver::DrawnBy,
    filled: &dyn Fn(u64) -> Option<String>,
) -> Option<u64> {
    by.sampled
        .iter()
        .map(|&(base, _, _)| base)
        .find(|&base| filled(base).is_none())
}

/// Asks the wait sub-loop to watch the first texture the frame flipped at `shown` sampled that
/// nothing tracked filled, while no texture has been asked for yet.
fn ask_for_unfilled_texture(shown: u64) {
    // A texture asked for that a frame was since drawn into was a render target not yet drawn when
    // it was asked for: another is looked for.
    if crate::waits::asked_texture()
        .is_some_and(|asked| orbistoun_gpu::agc_driver::last_drawn_into(asked).is_some())
    {
        crate::waits::forget_texture();
    }
    if !crate::waits::texture_wanted() {
        return;
    }
    let Some(by) = orbistoun_gpu::agc_driver::last_drawn_into(shown) else {
        return;
    };
    let census = orbistoun_gpu::agc_driver::dispatch_writes_by_module();
    // The frame's own textures, then those of the render targets it sampled, as the summary follows
    // them: a texture behind a target is as much the frame's as one it samples itself.
    let behind = sampled_targets(
        &by,
        MOST_TRACED_TARGETS,
        &orbistoun_gpu::agc_driver::last_drawn_into,
    );
    let unfilled = std::iter::once(&by)
        .chain(behind.iter().map(|(_, target)| target))
        .find_map(|frame| first_unfilled(frame, &|base| filled_by(base, &census)));
    if let Some(base) = unfilled {
        crate::waits::watch_texture(base);
    }
}

/// The guest accesses that brought drawn frames back before their flips, this run, each kind once.
static TOUCHES: Mutex<Vec<(u64, orbistoun_gpu::agc_driver::GuestTouch)>> = Mutex::new(Vec::new());

/// How many kinds of guest access to drawn frames a run keeps.
const MOST_TOUCHES: usize = 8;

/// Adds `touches` to `seen`, each target, instruction and kind of access once, up to
/// [`MOST_TOUCHES`].
fn note_touches(
    seen: &mut Vec<(u64, orbistoun_gpu::agc_driver::GuestTouch)>,
    touches: Vec<(u64, orbistoun_gpu::agc_driver::GuestTouch)>,
) {
    for (target, touch) in touches {
        let known = seen.iter().any(|(base, kept)| {
            *base == target && kept.rip == touch.rip && kept.write == touch.write
        });
        if !known && seen.len() < MOST_TOUCHES {
            seen.push((target, touch));
        }
    }
}

/// The run's line for the guest accesses that brought drawn frames back before their flips, where
/// there were any.
fn touches_line(seen: &[(u64, orbistoun_gpu::agc_driver::GuestTouch)]) -> Option<String> {
    if seen.is_empty() {
        return None;
    }
    let said: Vec<String> = seen
        .iter()
        .map(|(target, touch)| {
            format!(
                "a {} of {:#x} in {target:#x} from {:#x}",
                if touch.write { "write" } else { "read" },
                touch.address,
                touch.rip
            )
        })
        .collect();
    Some(format!(
        "the guest touched drawn frames before their flips: {}",
        said.join("; ")
    ))
}

/// How many distinct writers [`texture_writers_line`] names.
const MOST_TEXTURE_WRITERS: usize = 4;

/// What the watch of a texture nothing tracked filled found: how many writes, from where.
fn texture_writers_line(address: u64, writers: &[u64]) -> String {
    if writers.is_empty() {
        return format!(
            "the texture at {address:#x}, which nothing tracked filled, was not written while watched"
        );
    }
    let mut distinct: Vec<u64> = Vec::new();
    for &writer in writers {
        if !distinct.contains(&writer) {
            distinct.push(writer);
        }
    }
    let from: Vec<String> = distinct
        .iter()
        .take(MOST_TEXTURE_WRITERS)
        .map(|writer| format!("{writer:#x}"))
        .collect();
    format!(
        "the texture at {address:#x}, which nothing tracked filled, was written {} time(s) while watched, from {}",
        writers.len(),
        from.join(", ")
    )
}

/// The compute program whose dispatches stored over `address`, from the run's census: an image
/// the last of them stored, or a buffer they only stored to. Named by where the program lies, how
/// many dispatches ran it and whether what they wrote was all zero.
fn stored_by(
    address: u64,
    census: &[(u64, orbistoun_gpu::agc_driver::DispatchWrites)],
) -> Option<String> {
    let covers = |(at, bytes): (u64, u64)| (at..at.saturating_add(bytes)).contains(&address);
    let (_, writes) = census.iter().find(|(_, writes)| {
        writes.stored.is_some_and(covers)
            || writes
                .buffers
                .iter()
                .any(|&(at, bytes, read)| read.is_none() && covers((at, bytes)))
    })?;
    let program = writes
        .program
        .map_or_else(|| "an unknown address".to_owned(), |at| format!("{at:#x}"));
    Some(format!(
        "stored by the dispatch program at {program} ({} dispatch(es), {})",
        writes.dispatches,
        if writes.lit {
            "not all zero"
        } else {
            "all zero"
        }
    ))
}

/// How many sampled render targets [`log_flip_origins`] follows from the last flip's frame.
const MOST_TRACED_TARGETS: usize = 4;

/// The render targets `by` sampled, followed breadth first to the frames `into` says were drawn
/// into them and on to the targets those sampled, each once, at most `most`.
fn sampled_targets(
    by: &orbistoun_gpu::agc_driver::DrawnBy,
    most: usize,
    into: &dyn Fn(u64) -> Option<orbistoun_gpu::agc_driver::DrawnBy>,
) -> Vec<(u64, orbistoun_gpu::agc_driver::DrawnBy)> {
    let mut followed: Vec<(u64, orbistoun_gpu::agc_driver::DrawnBy)> = Vec::new();
    let mut queue: std::collections::VecDeque<u64> =
        by.sampled.iter().map(|&(base, _, _)| base).collect();
    while let Some(base) = queue.pop_front() {
        if followed.len() >= most {
            break;
        }
        if followed.iter().any(|(seen, _)| *seen == base) {
            continue;
        }
        if let Some(target) = into(base) {
            queue.extend(target.sampled.iter().map(|&(next, _, _)| next));
            followed.push((base, target));
        }
    }
    followed
}

/// What drew a frame, after `lead`: which frame drawn it was, its draws, each texture they bound
/// with what it holds ([`texel_summary`]), and where each texture they sampled lies, with what
/// `filled` says filled it - a frame orbistoun drew into it, or a file read - where it knows.
fn drawn_by_line(
    lead: &str,
    by: &orbistoun_gpu::agc_driver::DrawnBy,
    filled: &dyn Fn(u64) -> Option<String>,
) -> String {
    let mut textures: Vec<String> = by
        .textures
        .iter()
        .take(MOST_DRAWN_TEXTURES)
        .map(|texture| {
            let (slot, width, height, ..) = texture;
            format!(
                "slot {slot} {width}x{height} ({})",
                texel_summary(&as_rgba8(texture))
            )
        })
        .collect();
    if by.textures.len() > MOST_DRAWN_TEXTURES {
        textures.push(format!(
            "and {} more",
            by.textures.len() - MOST_DRAWN_TEXTURES
        ));
    }
    let binding = if textures.is_empty() {
        "no texture".to_owned()
    } else {
        textures.join("; ")
    };
    let sampled: Vec<String> = by
        .sampled
        .iter()
        .take(MOST_DRAWN_TEXTURES)
        .map(|&(base, width, height)| match filled(base) {
            Some(how) => format!("{base:#x} ({width}x{height}, {how})"),
            None => format!("{base:#x} ({width}x{height})"),
        })
        .collect();
    let sampling = if sampled.is_empty() {
        String::new()
    } else {
        format!("; sampling {}", sampled.join(", "))
    };
    format!(
        "{lead} frame {} by {} draw(s), binding {binding}{sampling}",
        by.frame, by.draws
    )
}

/// One command of a dumped submission. Addresses and resource ids read best in hexadecimal and
/// sizes in decimal, so the commands that carry a size name it in pixels.
fn command_line(command: &orbistoun_gpu::RenderCommand) -> String {
    match command {
        // Named by the guest shader it was made from, as its draw-program listing is, so a draw's
        // code is found from its bind.
        orbistoun_gpu::RenderCommand::BindShader { stage, shader } => {
            let line = orbistoun_gpu::pipeline::shader_name(*shader).map_or_else(
                || format!("BindShader {{ stage: {stage:?}, shader: {shader:x?} }}"),
                |name| {
                    format!(
                        "BindShader {{ stage: {stage:?}, shader: {shader:x?}, draw-program-{name} }}"
                    )
                },
            );
            // What a vertex shader's format loads converted by: where its vertices land rests on it.
            match orbistoun_gpu::pipeline::bound_vertex_formats(*shader) {
                Some(formats) => format!("{line}, formats {formats}"),
                None => line,
            }
        }
        orbistoun_gpu::RenderCommand::BindTexture {
            slot,
            width,
            height,
            texels,
            ..
        } => format!(
            "BindTexture {{ slot {slot}, {width}x{height}, {} }}",
            texel_summary(texels)
        ),
        orbistoun_gpu::RenderCommand::BindDrawBuffers { stage, buffers } => {
            draw_buffers_line(*stage, buffers)
        }
        orbistoun_gpu::RenderCommand::SetViewport(rect) => format!(
            "SetViewport {{ at ({}, {}), {}x{} }}",
            rect.x, rect.y, rect.width, rect.height
        ),
        other => format!("{other:x?}"),
    }
}

/// A dumped submission's colour target: its base in hexadecimal and its size in pixels, in
/// decimal. Printed whole as hexadecimal, a 1920 x 1080 target read as `780 x 438`.
fn colour_target_line(
    target: Option<&orbistoun_gpu::registers::ColourTarget>,
    tiling: Option<orbistoun_gpu::registers::SwizzleMode>,
    format: Option<orbistoun_gpu::registers::ColourTargetFormat>,
    bases: usize,
) -> String {
    let target = target.map_or_else(
        || "none".to_owned(),
        |t| {
            format!(
                "{:#x}, {}x{}, pipe-bank xor {:#x}, {:?}, {:?}",
                t.base, t.width, t.height, t.pipe_bank_xor, t.layout, t.place
            )
        },
    );
    format!("colour target {target}, tiling {tiling:?}, format {format:?}, {bases} base(s)")
}

/// Writes `submission` under `name`: `failed` for a drive the device refused, `last` for the
/// run's last submission, which is where a wrong picture is read from.
/// A stage's draw buffers in a listing: each slot's length and first words, not its bytes.
fn draw_buffers_line(
    stage: orbistoun_gpu::ShaderStage,
    buffers: &[orbistoun_gpu::DrawBuffer],
) -> String {
    let slots: Vec<String> = buffers
        .iter()
        .map(|buffer| {
            let first: Vec<String> = buffer
                .bytes
                .chunks_exact(4)
                .take(4)
                .map(|w| format!("{:#010x}", u32::from_le_bytes([w[0], w[1], w[2], w[3]])))
                .collect();
            format!("{} bytes [{}]", buffer.bytes.len(), first.join(" "))
        })
        .collect();
    format!("BindDrawBuffers {{ {stage:?}: {} }}", slots.join(", "))
}

fn write_submission(submission: &Submission, name: &str) {
    if let Some(dir) = frames_dir() {
        write_submission_to(dir, submission, name);
    }
}

/// Writes `submission` as `name`'s listing, guest memory window and shader modules into `dir`.
fn write_submission_to(dir: &Path, submission: &Submission, name: &str) {
    let mut text = String::new();
    for (index, command) in submission.commands.iter().enumerate() {
        let _ = writeln!(text, "{index:5}  {}", command_line(command));
    }
    let _ = writeln!(
        text,
        "window {:#x}, {} words, in {name}-window.bin",
        submission.guest_memory_base,
        submission.guest_memory.len()
    );
    for failure in &submission.report.failures {
        let _ = writeln!(
            text,
            "its {} shader at {:#x} was not prepared: {}",
            failure.stage, failure.address, failure.reason
        );
    }
    let _ = writeln!(
        text,
        "{}",
        colour_target_line(
            submission.colour_target.as_ref(),
            submission.colour_target_tiling,
            submission.colour_target_format,
            submission.colour_target_bases,
        )
    );
    // The primitive the mesh modules were made to emit: a draw of another shape draws nothing.
    let _ = writeln!(
        text,
        "primitive topology {:?}",
        submission.report.primitive_topology
    );
    // Each texture the draws bind, and where it lies: what a texture of one value is traced by.
    for texture in &submission.report.bound_textures {
        let _ = writeln!(
            text,
            "bound texture {}x{}, format {}, tiling {:?}, at {:#x}",
            texture.width, texture.height, texture.format, texture.tiling, texture.base
        );
    }
    let listing = dir.join(format!("{name}-submission.txt"));
    let window: Vec<u8> = submission
        .guest_memory
        .iter()
        .flat_map(|w| w.to_le_bytes())
        .collect();
    // Made here too, so a run whose directory another run pruned keeps its dumps.
    let written = std::fs::create_dir_all(dir).and_then(|()| {
        std::fs::write(&listing, text)?;
        std::fs::write(dir.join(format!("{name}-window.bin")), &window)?;
        std::fs::write(
            dir.join(format!("{name}-buffers.txt")),
            buffers_listing(&submission.commands),
        )?;
        for (id, words) in &submission.modules {
            let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
            std::fs::write(dir.join(format!("{name}-module-{}.spv", id.0)), bytes)?;
        }
        Ok(())
    });
    match written {
        Ok(()) => tracing::info!(
            "the {name} submission's commands and {} module(s) are in {name}-submission.txt and {name}-module-*.spv in {}",
            submission.modules.len(),
            dir.display()
        ),
        Err(e) => tracing::warn!("the {name} submission could not be written: {e}"),
    }
}

/// Takes the last submission a guest made, drives it to a backend, logs the outcome and its
/// time, and writes the frame it produced.
///
/// Does nothing when no guest reached a submission. Called from every ending (the ordinary
/// return, the time limit and the call budget), since a guest that submits and waits on a fence
/// ends on the clock or the budget. The frame goes to the run's frames directory through
/// [`crate::frame_region::write_frame`], and the returned [`Event::Frame`] is for a caller that
/// owns the protocol stream; the endings that stop the process from their own thread only log
/// the region's name.
pub fn render_and_log_last_submission() -> Option<Event> {
    // Submissions still queued are carried out first, so the record and the frame are the run's
    // whole (D821). Bounded: one blocked on a stopped guest thread would hold the ending forever.
    if !orbistoun_gpu::agc_driver::settle(std::time::Duration::from_secs(2)) {
        tracing::warn!(
            "submissions were still queued when the run ended; the report leaves them out"
        );
    }
    // The pipelines the run's last seconds compiled, kept for the next run (D744).
    on_device(orbistoun_gpu_vulkan::pipeline_cache::flush);
    log_execution();
    // How the asynchronous file reads fared: a read held by a buffer never submitted never lands.
    if let Some(line) = orbistoun_kernel::apr::read_summary().describe() {
        tracing::info!("{line}");
    }
    let submission = orbistoun_gpu::agc_driver::take_last_submission()?;
    write_submission(&submission, "last");
    // On the device thread, as one job: a guest still running draws there too, and between two of
    // the replay's drives its draws would write the observation buffer the replay reads.
    on_device(|| log_draw_changes(&submission));
    if let Some(refused) = orbistoun_gpu::agc_driver::take_refused_submission() {
        write_submission(&refused, "refused");
    }
    // A submission whose draws already ran at submit has its frame; drawing it again would
    // blend it over itself.
    let ran = orbistoun_gpu::agc_driver::execution()
        .last
        .is_some_and(|last| last.draws > 0);
    let executed = if ran { executed_frame() } else { None };
    if let (true, Some((width, height, bytes)), Some(dir)) = (ran, executed, frames_dir()) {
        let sequence = FRAME_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        return match write_frame(dir, sequence, width, height, FrameFormat::Rgba8, &bytes) {
            Ok(event) => {
                if let Event::Frame { region, .. } = &event {
                    tracing::info!("the last submission's frame, drawn at submit, is {region}");
                }
                Some(event)
            }
            Err(e) => {
                tracing::warn!("the executed frame could not be written: {e}");
                None
            }
        };
    }
    render_submission_to(&submission, frames_dir())
}

/// The sequence number of the next frame region written.
static FRAME_SEQUENCE: AtomicU64 = AtomicU64::new(1);

/// Where the run's frames and dumps go: a directory of the run's own under the traces directory,
/// where the shim reads a frame region from (D695), so another session's run cannot overwrite them
/// (D865). Unset in a run with no paths, which writes no frame.
fn frames_directory() -> &'static OnceLock<PathBuf> {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    &DIR
}

/// Sets the run's frames directory, and drops the frames and dumps of the title's older runs. First
/// set wins; a run has one.
pub fn frames_to(dir: PathBuf) {
    crate::frame_region::prune_runs(&dir);
    if let Err(e) = std::fs::create_dir_all(&dir) {
        tracing::warn!(
            "the run's frames directory {} was not made: {e}",
            dir.display()
        );
    }
    let _ = frames_directory().set(dir);
}

fn frames_dir() -> Option<&'static Path> {
    frames_directory().get().map(PathBuf::as_path)
}

/// Drives `submission` to a backend, logs what happened, and writes the frame it produced into
/// `frames_dir`, returning the descriptor that names it, or `None` when there is no directory,
/// no device or no frame. The directory is an argument so a test can use a temporary one.
pub fn render_submission_to(submission: &Submission, frames_dir: Option<&Path>) -> Option<Event> {
    let mut said = command_summary(&submission.commands)
        .iter()
        .map(|line| format!("  {line}"))
        .collect::<Vec<_>>()
        .join("\n");
    // The textures the draws name.
    let textures = &submission.report.textures;
    if !textures.is_empty() {
        let _ = write!(said, "\n  {} distinct texture(s) named:", textures.len());
        for texture in textures.iter().take(SUMMARY_LINES) {
            let _ = write!(
                said,
                "\n    {}x{} format {} {:?} at {:#x}",
                texture.width, texture.height, texture.format, texture.tiling, texture.base
            );
        }
    }
    if !said.is_empty() {
        tracing::info!("{}", said.trim_start_matches('\n'));
    }
    let started = std::time::Instant::now();
    let rendered = render(submission);
    let took = started.elapsed();
    let outcome = &rendered.outcome;
    let written = match (frames_dir, outcome.frame, rendered.frame_bytes.as_deref()) {
        (Some(dir), Some((width, height)), Some(bytes)) => {
            let sequence = FRAME_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            match write_frame(dir, sequence, width, height, FrameFormat::Rgba8, bytes) {
                Ok(event) => Some(event),
                Err(e) => {
                    tracing::warn!(
                        "the rendered frame could not be written to {}: {e}",
                        dir.display()
                    );
                    None
                }
            }
        }
        _ => None,
    };
    log_outcome(outcome, took, written.as_ref());
    written
}

fn log_outcome(outcome: &RenderOutcome, took: std::time::Duration, written: Option<&Event>) {
    match &outcome.device {
        None => tracing::warn!(
            "a submission was made, but this build has no graphics device to render it"
        ),
        Some(device) => {
            let frame = match outcome.frame {
                Some((w, h)) => format!(", frame {w}x{h}"),
                None => String::new(),
            };
            tracing::info!(
                "a submission reached the {device} backend: {} command(s) driven, {} refused{frame}, in {} ms{}",
                outcome.executed,
                outcome.refused,
                took.as_millis(),
                refusal_lines(&outcome.refusals)
            );
            if let Some(Event::Frame { region, .. }) = written {
                tracing::info!("the rendered frame is {region}");
            }
        }
    }
}

/// The lines [`render_submission_to`] prints for a submission's commands: each run of identical
/// consecutive commands once, with its count, and at most [`SUMMARY_LINES`] of them.
///
/// It shows which target a draw lands on, the rectangle it is restricted to and how many
/// vertices it asks for, which is what a frame that comes back black needs first.
fn command_summary(commands: &[orbistoun_gpu::RenderCommand]) -> Vec<String> {
    let mut lines = Vec::new();
    let mut runs = commands.iter().peekable();
    while let Some(command) = runs.next() {
        let mut count = 1usize;
        while runs.peek() == Some(&command) {
            runs.next();
            count += 1;
        }
        if lines.len() == SUMMARY_LINES {
            lines.push("... and more".to_owned());
            break;
        }
        // A texture's texels are its bulk; the summary names its size.
        let shown = match command {
            orbistoun_gpu::RenderCommand::BindTexture {
                slot,
                texels,
                width,
                height,
                ..
            } => {
                // How varied it is, and its two ends: enough to tell a one-colour texture from a
                // draw that samples one texel.
                let mut distinct = texels.to_vec();
                distinct.sort_unstable();
                distinct.dedup();
                format!(
                    "BindTexture {{ slot {slot}, {width}x{height}, {} distinct texels, first {:#010x}, last {:#010x} }}",
                    distinct.len(),
                    texels.first().copied().unwrap_or_default(),
                    texels.last().copied().unwrap_or_default()
                )
            }
            orbistoun_gpu::RenderCommand::BindDrawBuffers { stage, buffers } => {
                draw_buffers_line(*stage, buffers)
            }
            other => format!("{other:?}"),
        };
        lines.push(if count == 1 {
            shown
        } else {
            format!("{shown} x{count}")
        });
    }
    lines
}

/// Every refusal over the run, by reason, and how many submissions they kept from the guest.
static REFUSALS: Mutex<(std::collections::BTreeMap<&'static str, u64>, u64)> =
    Mutex::new((std::collections::BTreeMap::new(), 0));

/// Adds one refused submission's refusals to the run's.
fn count_refusals(refusals: &[(&'static str, usize)]) {
    let mut run = REFUSALS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    for &(why, count) in refusals {
        *run.0.entry(why).or_default() += count as u64;
    }
    run.1 += 1;
}

/// The run's refusals by reason, most first, and the submissions they kept from the guest: what
/// the run report names as a host gap.
pub fn refusals() -> (Vec<(String, u64)>, u64) {
    let run = REFUSALS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut by_reason: Vec<(String, u64)> = run
        .0
        .iter()
        .map(|(why, count)| ((*why).to_owned(), *count))
        .collect();
    by_reason.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    (by_reason, run.1)
}

/// Where the GPU queue stopped, if it did: the submission that stopped it, counted from the first
/// carried out, and the byte it stopped at (D849). Nothing after it retired (D710).
pub fn gpu_stop() -> Option<orbistoun_report::trace::GpuStop> {
    let record = orbistoun_gpu::agc_driver::execution();
    let byte = record.last?.stopped.offset()?;
    Some(orbistoun_report::trace::GpuStop {
        submission: record.submissions,
        byte,
    })
}

/// Each refusal reason and its count, one indented line apiece, for the end of an outcome line.
fn refusal_lines(refusals: &[(&'static str, usize)]) -> String {
    let mut lines = String::new();
    for (why, count) in refusals {
        let _ = write!(lines, "\n  refused {count:>5}  {why}");
    }
    lines
}

/// How many summary lines a submission's commands print before the rest is elided.
const SUMMARY_LINES: usize = 12;

/// Says what the command processor carried out at submit: how many submissions ran to
/// Writes each program the dispatches ran, and each shader the draws ran, listed, beside the
/// report: what a dispatch or a draw did is read against its code.
fn write_dispatch_listings() {
    let Some(dir) = frames_dir() else {
        return;
    };
    for (address, listing) in orbistoun_gpu::pipeline::dispatch_program_listings() {
        let path = dir.join(format!("dispatch-program-{address:x}.txt"));
        if let Err(e) = std::fs::write(&path, listing) {
            tracing::warn!("a dispatch program's listing could not be written: {e}");
        }
    }
    // And each shader the draws ran, so what a draw drew is read against its code too.
    for (name, listing) in orbistoun_gpu::pipeline::draw_program_listings() {
        let path = dir.join(format!("draw-program-{name}.txt"));
        if let Err(e) = std::fs::write(&path, listing) {
            tracing::warn!("a draw program's listing could not be written: {e}");
        }
    }
}

/// What each compute program's dispatches wrote, and who read what they stored.
fn log_dispatch_writes() {
    // What each program's dispatches wrote: one that writes only zeros produces nothing, and what
    // nothing reads is not seen.
    let census = orbistoun_gpu::agc_driver::dispatch_writes_by_module();
    let frames = orbistoun_gpu::agc_driver::drawn_frames();
    for (module, writes) in &census {
        let module = *module;
        let span = |span: Option<(u64, u64)>| {
            span.map_or_else(
                || "none".to_owned(),
                |(at, bytes)| format!("{at:#x} ({bytes} bytes)"),
            )
        };
        tracing::info!(
            "dispatch module {module:016x}, the program at {}: {} carried out, {} byte(s) written back, {}; fetches an image at {}, stores one at {}",
            program_named(writes.program, &crate::report::locate),
            writes.dispatches,
            writes.bytes,
            if writes.lit {
                "some of them not zero"
            } else {
                "every one zero"
            },
            span(writes.fetched),
            span(writes.stored)
        );
        if let Some((at, bytes)) = writes.stored {
            tracing::info!(
                "  the image it stores: {}",
                consumed_by(at, bytes, &frames, &census)
            );
        }
        for &(at, bytes, lit) in &writes.buffers {
            tracing::info!(
                "  it binds a buffer at {at:#x} ({bytes} bytes), {}",
                match lit {
                    Some(true) => "read, some of it not zero".to_owned(),
                    Some(false) => "read, every word zero".to_owned(),
                    None => format!(
                        "only stored to; {}",
                        consumed_by(at, bytes, &frames, &census)
                    ),
                }
            );
        }
    }
}

/// completion, and where the last one stopped when it did not (D705).
fn log_execution() {
    let record = orbistoun_gpu::agc_driver::execution();
    if record.submissions == 0 {
        return;
    }
    let body = record.last.map(|l| l.stopped_body).unwrap_or_default();
    let last = match record.last.map(|l| l.stopped) {
        Some(orbistoun_gpu::cp::Stopped::Completed) | None => "ran to completion".to_owned(),
        Some(orbistoun_gpu::cp::Stopped::NeedsGpu { offset, opcode }) => format!(
            "stopped at byte {offset:#x}: opcode {opcode:#04x} (body {body:08x?}) needs the GPU, so nothing after it retired"
        ),
        Some(other) => format!("stopped: {other:?}"),
    };
    let held = if record.held_back == 0 {
        String::new()
    } else {
        format!(
            "; {} later submission(s) wait behind it and were not carried out",
            record.held_back
        )
    };
    if let Some(why) = orbistoun_gpu::agc_driver::last_dispatch_refusal() {
        tracing::info!("a compute dispatch was not carried out - {why}");
    }
    report_commands(
        "the stream that stopped its queue",
        &orbistoun_gpu::agc_driver::stalled_commands(),
    );
    let writes = orbistoun_gpu::agc_driver::stalled_writes();
    if !writes.is_empty() {
        let listed: Vec<String> = writes
            .iter()
            .map(|(packet, address)| format!("{packet} {address:#x}"))
            .collect();
        tracing::info!(
            "the memory the stream that stopped its queue wrote before it stopped: {}",
            listed.join(", ")
        );
    }
    log_flip_origins();
    log_dispatch_writes();
    // The program a refused dispatch runs, to read the refusal against.
    if let (Some((address, program)), Some(dir)) = (
        orbistoun_gpu::pipeline::last_refused_program(),
        frames_dir(),
    ) {
        match std::fs::write(dir.join("refused-dispatch-program.bin"), &program) {
            Ok(()) => tracing::info!(
                "the refused dispatch's program, {} bytes at {address:#x}, is refused-dispatch-program.bin in {}",
                program.len(),
                dir.display()
            ),
            Err(e) => tracing::warn!("the refused dispatch's program could not be written: {e}"),
        }
    }
    write_dispatch_listings();
    // Each shader that could not be translated, likewise.
    let refused = orbistoun_gpu::pipeline::refused_shader_programs();
    if let (false, Some(dir)) = (refused.is_empty(), frames_dir()) {
        let written = refused.iter().try_for_each(|(address, program)| {
            std::fs::write(dir.join(format!("refused-shader-{address:x}.bin")), program)
        });
        match written {
            Ok(()) => tracing::info!(
                "{} shader(s) that could not be translated are refused-shader-<address>.bin in {}",
                refused.len(),
                dir.display()
            ),
            Err(e) => tracing::warn!("the refused shaders could not be written: {e}"),
        }
    }
    if let Some(why) = orbistoun_gpu::agc_driver::last_draw_refusal() {
        tracing::info!("a draw segment was not carried out - {why}");
    }
    tracing::info!(
        "the command processor carried out {} of {} submission(s) to completion, {} with their draws ({} bytes written); the last {last}{held}",
        record.completed,
        record.submissions,
        record.drawn,
        record.bytes_written,
    );
    report_released_labels();
    if let Some(orbistoun_gpu::cp::Stopped::WaitNeverSatisfied { address, .. }) =
        record.last.map(|last| last.stopped)
    {
        report_unsatisfied_wait(address);
    }
    report_stepped_aside();
    report_passed_refusals();
}

/// Lists the dispatches and draws the run passed over at the host's refusal (D896), each reason
/// once with how many times it was met: every wall the run met, not only the first.
fn report_passed_refusals() {
    let passed = orbistoun_gpu::agc_driver::passed_refusals();
    if passed.is_empty() {
        return;
    }
    tracing::info!(
        "{} distinct refusal(s) were passed over (ORBISTOUN_PASS_REFUSED, D896), what they would have written missing:",
        passed.len()
    );
    for (refusal, times) in passed {
        tracing::info!("  {times} x {refusal}");
    }
}

/// Says which asynchronous compute streams the run left stepped aside (D891), and what the first
/// waits on: what its ring was handed after it never ran.
fn report_stepped_aside() {
    let aside = orbistoun_gpu::agc_driver::stepped_aside();
    let Some(&first) = aside.first() else {
        return;
    };
    let waits = match first {
        Some(orbistoun_gpu::cp::Stopped::WaitNeverSatisfied {
            offset,
            address,
            reference,
            observed,
        }) => format!(
            "the first stopped at byte {offset:#x} waiting for {address:#x} to read {reference:#x}, and it reads {observed:#x}"
        ),
        other => format!("the first stopped at {other:?}"),
    };
    tracing::info!(
        "{} asynchronous compute stream(s) were left stepped aside at a wait; {waits}",
        aside.len()
    );
    if let Some(orbistoun_gpu::cp::Stopped::WaitNeverSatisfied { address, .. }) = first {
        report_unsatisfied_wait(address);
    }
    report_commands("the first", &orbistoun_gpu::agc_driver::aside_commands());
}

/// Lists the commands a stream that stopped ran before it stopped, named where the packet
/// vocabulary names them: a command carried out here without a memory write it makes on the
/// console is found beside the wait that waits for that write.
fn report_commands(stream: &str, commands: &[(u8, usize)]) {
    if commands.is_empty() {
        return;
    }
    let vocabulary = orbistoun_gpu::registers::Vocabulary::builtin().ok();
    let named: Vec<String> = commands
        .iter()
        .map(|&(opcode, times)| {
            let name = vocabulary
                .as_ref()
                .and_then(|vocabulary| vocabulary.opcode_name(opcode))
                .map_or_else(|| format!("{opcode:#04x}"), str::to_owned);
            format!("{name} x{times}")
        })
        .collect();
    tracing::info!(
        "the commands {stream} ran before it stopped: {}",
        named.join(", ")
    );
}

/// Says what last filled the word a queue stopped waiting on, where something orbistoun tracks
/// filled it: a wait that never held may hold later, written by work behind it - PPSA04263's
/// prologue waits for 0xc800040b0 to read 0, and something writes the 0 after.
fn report_unsatisfied_wait(address: u64) {
    let census = orbistoun_gpu::agc_driver::dispatch_writes_by_module();
    let filled = filled_by(address, &census).unwrap_or_else(|| {
        concat!(
            "nothing tracked filled it - an end-of-pipe release, a WRITE_DATA, or the title's own ",
            "stores"
        )
        .to_owned()
    });
    tracing::info!("the word the command processor waited on, {address:#x}: {filled}");
    for (packet, offset, held) in orbistoun_gpu::agc_driver::held_fillers(address) {
        tracing::info!(
            "  a submission held behind the stopped queue would fill it: {packet} at byte {offset:#x} of the {} held - its queue may run apart from the stopped one on the console (D710)",
            ordinal(held + 1)
        );
    }
}

/// `n` as an English ordinal: 1st, 2nd, 3rd, 4th, 11th, 21st.
fn ordinal(n: u64) -> String {
    let suffix = match (n % 10, n % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

/// How many released labels the report names, most released first.
const LABELS_NAMED: usize = 8;

/// Says where the end-of-pipe labels stood when the run ended: a thread waiting on a fence reads
/// one of them, and the value it asks for set beside the last one written says how many releases
/// never came (PPSA04263's prologue asked for 0x1be1 of a label left at 0x1bbb).
fn report_released_labels() {
    let mut labels = orbistoun_gpu::cp::released_labels();
    if labels.is_empty() {
        return;
    }
    labels.sort_by_key(|&(.., releases)| std::cmp::Reverse(releases));
    let named: Vec<String> = labels
        .iter()
        .take(LABELS_NAMED)
        .map(|(at, value, releases)| format!("{at:#x} = {value:#x} ({releases} release(s))"))
        .collect();
    tracing::info!(
        "the labels end-of-pipe releases wrote, as the run left them: {}",
        named.join(", ")
    );
}

#[cfg(test)]
mod tests {
    use super::{KeptFlips, RenderOutcome, holds_a_picture, render};

    /// The run's host time per flip is each phase's milliseconds over its flips, costliest first,
    /// with the flips and the rate they came at: what a frame-bound stretch spends where, read at the
    /// run's end. Windows with no flip add nothing, and phases that took no time are left out.
    #[test]
    fn the_run_s_host_time_per_flip_is_each_phase_over_its_flips() {
        let mut run = super::RunPerf::default();
        let window = |ms: f64, flips: u64, phases: &[(&str, f64)]| orbistoun_proto::PerfReport {
            window_ms: ms,
            flips,
            submissions: flips,
            draws: 2 * flips,
            phases: phases.iter().map(|&(n, v)| (n.to_owned(), v)).collect(),
            spans: Vec::new(),
        };
        run.add(&window(
            1000.0,
            20,
            &[("prepare", 120.0), ("build", 40.0), ("seed", 0.0)],
        ));
        run.add(&window(1000.0, 0, &[("prepare", 500.0)]));
        run.add(&window(
            1000.0,
            20,
            &[("prepare", 80.0), ("build", 40.0), ("seed", 0.0)],
        ));
        assert_eq!(
            run.describe().as_deref(),
            Some(concat!(
                "host time per flip over the windows that flipped: 40 flips at 20.0 a second; ",
                "prepare 5.00 ms, build 2.00 ms - the rest of each 50.0 ms flip is the guest's own ",
                "time and what nothing measures"
            ))
        );
        assert_eq!(super::RunPerf::default().describe(), None);
    }

    /// Windows are grouped by their frames' shape - draws a flip - and the shapes with the most
    /// flips each get their own per-flip phases, so a run whose fast frames drown the average still
    /// names what its other frames spend: GTA V's title screen flips a hundred times a second, its
    /// intro video's two-draw frames under twenty.
    #[test]
    fn each_frame_shape_is_described_on_its_own() {
        let mut run = super::RunPerf::default();
        let window = |ms: f64, flips: u64, draws: u64, prepare: f64| orbistoun_proto::PerfReport {
            window_ms: ms,
            flips,
            submissions: flips,
            draws,
            phases: vec![("prepare".to_owned(), prepare)],
            spans: Vec::new(),
        };
        run.add(&window(1000.0, 100, 900, 100.0));
        run.add(&window(1000.0, 10, 20, 300.0));
        run.add(&window(1000.0, 10, 20, 100.0));
        run.add(&window(1000.0, 1, 40, 900.0));
        assert_eq!(
            run.describe_shapes(),
            [
                concat!(
                    "frames of 9 draws: 100 flips at 100.0 a second; prepare 1.00 ms - ",
                    "the rest of each 10.0 ms flip is the guest's own time and what nothing measures"
                ),
                concat!(
                    "frames of 2 draws: 20 flips at 10.0 a second; prepare 20.00 ms - ",
                    "the rest of each 100.0 ms flip is the guest's own time and what nothing measures"
                ),
                concat!(
                    "frames of 40 draws: 1 flips at 1.0 a second; prepare 900.00 ms - ",
                    "the rest of each 1000.0 ms flip is the guest's own time and what nothing measures"
                ),
            ]
        );
        assert!(super::RunPerf::default().describe_shapes().is_empty());
    }

    /// A draw's capture reads as the counts it declared, then clip coordinates and where they land: a
    /// vertex outside -1..1 after the divide, or with w at or below zero, draws nothing there; a
    /// buffer still zero says no vertex ran.
    #[test]
    fn captured_positions_read_as_clip_and_screen() {
        let mut words: Vec<u32> = [
            [0.5_f32, -0.5, 0.0, 1.0],
            [2.0, 4.0, 0.5, 2.0],
            [1.0, 1.0, 0.0, 0.0],
        ]
        .iter()
        .flatten()
        .map(|component| component.to_bits())
        .collect();
        words.extend([30, 0, 0, 0]);
        assert_eq!(
            super::positions_line(&words),
            concat!(
                "30 vertices and 0 primitives declared; ",
                "clip (0.5, -0.5, 0, 1) at (0.5, -0.5); ",
                "clip (2, 4, 0.5, 2) at (1, 2); ",
                "clip (1, 1, 0, 0) behind the eye"
            )
        );
        assert_eq!(
            super::positions_line(&[0; 16]),
            "none - no vertex of it ran"
        );
        assert_eq!(
            super::CAPTURED_COUNTS,
            orbistoun_translate::wavefront::CAPTURED_COUNTS as usize,
            "the translator's layout"
        );
    }

    /// What one draw did to a frame is the pixels that differ from the frame before it: how many,
    /// the rectangle they lie in, and the colour of the first - GTA V's title showed its
    /// "Continue" and not its logo, and which draw drew what was not measured.
    #[test]
    fn a_draw_s_change_is_the_pixels_that_differ() {
        // A 4x2 frame; the second has pixel (1,0) and (3,1) changed.
        let before = vec![0_u8; 4 * 2 * 4];
        let mut after = before.clone();
        after[4..8].copy_from_slice(&[0xff, 0xff, 0xff, 0xff]);
        after[(4 + 3) * 4..(4 + 3) * 4 + 4].copy_from_slice(&[0x10, 0x20, 0x30, 0xff]);
        let change = super::frame_change(&before, &after, 4);
        assert_eq!(change.pixels, 2);
        assert_eq!(change.bounds, Some((1, 0, 3, 1)));
        assert_eq!(change.first, Some([0xff, 0xff, 0xff, 0xff]));
        let none = super::frame_change(&before, &before, 4);
        assert_eq!((none.pixels, none.bounds), (0, None));
    }

    /// Every buffer a submission's draws bind is listed whole, by command, stage, slot and guest
    /// address, four words a line with their offset: what a draw computed its colour from is read
    /// there, not guessed from the first four words.
    #[test]
    fn every_bound_buffer_is_listed_whole() {
        use orbistoun_gpu::{DrawBuffer, RenderCommand, ShaderStage};
        let words: Vec<u8> = (0_u32..5).flat_map(u32::to_le_bytes).collect();
        let commands = vec![
            RenderCommand::SetWriteMask(0xf),
            RenderCommand::BindDrawBuffers {
                stage: ShaderStage::Fragment,
                buffers: vec![DrawBuffer {
                    bytes: words.into(),
                    hash: 0,
                    base: 0x7400_1000,
                }],
            },
        ];
        let listing = super::buffers_listing(&commands);
        assert_eq!(
            listing,
            concat!(
                "command 1, Fragment slot 0, 20 bytes at 0x74001000\n",
                "  +0x0000  00000000 00000001 00000002 00000003\n",
                "  +0x0010  00000004\n",
            )
        );
    }

    /// A shader bind with no draw shader behind it is listed as it was; the listing's own form is
    /// unchanged for one (a test makes no draw shader).
    #[test]
    fn a_shader_bind_with_no_guest_shader_is_listed_as_it_was() {
        use orbistoun_gpu::{RenderCommand, ResourceId, ShaderStage};
        let line = super::command_line(&RenderCommand::BindShader {
            stage: ShaderStage::Fragment,
            shader: ResourceId(0xdead_0030),
        });
        assert_eq!(
            line,
            "BindShader { stage: Fragment, shader: ResourceId(dead0030) }"
        );
    }

    /// With blending off every blend command of the submission is turned off, and nothing else
    /// changes: what a draw writes then lands whatever its alpha.
    #[test]
    fn blending_is_turned_off_at_every_draw() {
        use orbistoun_gpu::RenderCommand;
        let blend = orbistoun_gpu::registers::target_blend_control(0x2006_1601 | 1 << 30, 0);
        let submission = super::Submission {
            commands: vec![
                RenderCommand::SetBlend(blend),
                RenderCommand::SetWriteMask(0xf),
            ],
            ..Default::default()
        };
        let opaque = super::without_blending(&submission);
        assert!(matches!(
            opaque.commands[0],
            RenderCommand::SetBlend(off) if !off.enable && off.color_src == blend.color_src
        ));
        assert!(matches!(
            opaque.commands[1],
            RenderCommand::SetWriteMask(0xf)
        ));
    }

    /// Only the three draw commands count as draws.
    #[test]
    fn the_draw_commands_are_the_draws() {
        use orbistoun_gpu::RenderCommand;
        assert!(super::is_draw(&RenderCommand::Draw {
            vertices: 3,
            instances: 1,
            first_vertex: 0,
        }));
        assert!(!super::is_draw(&RenderCommand::SetWriteMask(0xf)));
    }

    /// Each texture the last flip's frame sampled is kept as the texels orbistoun bound, named by
    /// its order, slot and size, so a texture a frame shows nothing of can be looked at: GTA V's
    /// title drew its "Continue" and not its logo.
    #[test]
    fn a_sampled_texture_is_kept_as_its_texels() {
        let texels: std::sync::Arc<[u32]> = vec![0xff00_00ff, 0x0000_ff00].into();
        let (name, bytes) =
            super::sampled_dump(3, &(1, 2, 1, texels, orbistoun_gpu::TextureEncoding::Rgba8));
        assert_eq!(name, "sampled-03-slot1-2x1.rgba");
        assert_eq!(bytes, [0xff, 0, 0, 0xff, 0, 0xff, 0, 0]);
        // A one-byte texture's packed bytes, kept as its lanes read them: here (X, X, X, 1).
        let packed: std::sync::Arc<[u32]> = vec![0x0000_8010].into();
        let lanes = [
            orbistoun_gpu::ByteLane::Byte,
            orbistoun_gpu::ByteLane::Byte,
            orbistoun_gpu::ByteLane::Byte,
            orbistoun_gpu::ByteLane::One,
        ];
        let (_, bytes) = super::sampled_dump(
            0,
            &(
                0,
                2,
                1,
                packed,
                orbistoun_gpu::TextureEncoding::R8 { lanes },
            ),
        );
        assert_eq!(bytes, [0x10, 0x10, 0x10, 0xff, 0x80, 0x80, 0x80, 0xff]);
    }

    /// The frame a flip showed is named by what drew it: which frame drawn it was, its draws, each
    /// texture they bound with what it holds - a texture of one value reads as one distinct - and
    /// where each lies, with the frame orbistoun drew into it where it drew one: a render target
    /// sampled is traced to the frame that filled it.
    #[test]
    fn the_last_flip_names_what_drew_it() {
        let by = orbistoun_gpu::agc_driver::DrawnBy {
            frame: 12,
            draws: 3,
            textures: vec![(
                0,
                2,
                1,
                std::sync::Arc::from(vec![5_u32, 5]),
                orbistoun_gpu::TextureEncoding::Rgba8,
            )],
            sampled: vec![(0x2000, 2, 1), (0x3000, 4, 4)],
            read: Vec::new(),
        };
        let drawn = |base: u64| (base == 0x2000).then(|| "drawn into as frame 9".to_owned());
        assert_eq!(
            super::drawn_by_line("the last flip showed 0x1000, drawn into as", &by, &drawn),
            concat!(
                "the last flip showed 0x1000, drawn into as frame 12 by 3 draw(s), binding ",
                "slot 0 2x1 (first 0x00000005, 1 distinct, 2 of 2 lit); sampling 0x2000 (2x1, drawn into as ",
                "frame 9), 0x3000 (4x4)"
            )
        );
        let bare = orbistoun_gpu::agc_driver::DrawnBy {
            textures: Vec::new(),
            sampled: Vec::new(),
            ..by
        };
        assert!(
            super::drawn_by_line("the last flip showed 0x1000, drawn into as", &bare, &drawn)
                .ends_with("binding no texture")
        );
        // A flip of a buffer no drawn frame went into says so.
        assert_eq!(
            super::undrawn_line(0x1000),
            "the last flip showed 0x1000, which no frame orbistoun drew went into"
        );
    }

    /// The guest accesses that brought drawn frames back before their flips are named, each kind
    /// once: what the guest read or wrote of which target, from where. A guest that reads back an
    /// earlier render target says so.
    #[test]
    fn the_guest_accesses_to_drawn_frames_are_named() {
        let touch = |rip, address, write| orbistoun_gpu::agc_driver::GuestTouch {
            rip,
            address,
            write,
        };
        let mut seen = Vec::new();
        super::note_touches(
            &mut seen,
            vec![
                (
                    0x7400_0000_0000,
                    touch(0x4000_1000, 0x7400_0000_0040, false),
                ),
                (
                    0x7400_0000_0000,
                    touch(0x4000_1000, 0x7400_0000_0080, false),
                ),
                (0x7400_1000_0000, touch(0x4000_2000, 0x7400_1000_0000, true)),
            ],
        );
        assert_eq!(seen.len(), 2, "one per target, instruction and kind");
        assert_eq!(
            super::touches_line(&seen).as_deref(),
            Some(concat!(
                "the guest touched drawn frames before their flips: a read of 0x740000000040 in ",
                "0x740000000000 from 0x40001000; a write of 0x740010000000 in 0x740010000000 from ",
                "0x40002000"
            ))
        );
        assert_eq!(super::touches_line(&[]), None);
    }

    /// The texture a frame sampled that nothing tracked filled is the first of its sampled ones no
    /// filler names, and its watch says who wrote it or that nothing did while watched.
    #[test]
    fn an_unfilled_texture_is_picked_and_its_writers_named() {
        let by = orbistoun_gpu::agc_driver::DrawnBy {
            frame: 3,
            draws: 1,
            textures: Vec::new(),
            sampled: vec![(0x2000, 4, 4), (0x3000, 4, 4), (0x4000, 4, 4)],
            read: Vec::new(),
        };
        let filled = |base: u64| (base == 0x2000).then(|| "drawn into as frame 1".to_owned());
        assert_eq!(super::first_unfilled(&by, &filled), Some(0x3000));
        assert_eq!(super::first_unfilled(&by, &|_| Some(String::new())), None);
        assert_eq!(
            super::texture_writers_line(0x3000, &[0x4000_1234, 0x4000_1234, 0x4000_5678]),
            concat!(
                "the texture at 0x3000, which nothing tracked filled, was written 3 time(s) while ",
                "watched, from 0x40001234, 0x40005678"
            )
        );
        assert_eq!(
            super::texture_writers_line(0x3000, &[]),
            "the texture at 0x3000, which nothing tracked filled, was not written while watched"
        );
    }

    /// A program is named by where it lies and, where the run placed that memory, which region and
    /// how far in: the title's own code reads apart from a library's.
    #[test]
    fn a_program_is_named_by_its_region() {
        let locate = |at: u64| (at >= 0x4000_0000).then(|| ("image", at - 0x4000_0000));
        assert_eq!(
            super::program_named(Some(0x4000_1200), &locate),
            "0x40001200 (image+0x1200)"
        );
        assert_eq!(super::program_named(Some(0x1000), &locate), "0x1000");
        assert_eq!(super::program_named(None, &locate), "?");
    }

    /// What a program stored is followed to who read it: a drawn frame that sampled a texture
    /// starting inside it, or another program that read or fetched from it - and when neither did,
    /// the report says so: output nothing consumes.
    #[test]
    fn what_a_program_stored_is_followed_to_its_readers() {
        let frame = orbistoun_gpu::agc_driver::DrawnBy {
            frame: 12,
            draws: 1,
            textures: Vec::new(),
            sampled: vec![(0x2400, 8, 8)],
            read: Vec::new(),
        };
        let reader = orbistoun_gpu::agc_driver::DispatchWrites {
            program: Some(0x7400_0000_9000),
            buffers: vec![(0x8100, 0x40, Some(true))],
            ..Default::default()
        };
        let census = [(2, reader)];
        let frames = [frame];
        assert_eq!(
            super::consumed_by(0x2000, 0x1000, &frames, &census),
            "sampled by frame 12"
        );
        assert_eq!(
            super::consumed_by(0x8000, 0x200, &frames, &census),
            "read by the program at 0x740000009000"
        );
        assert_eq!(
            super::consumed_by(0x5000, 0x100, &frames, &census),
            "nothing tracked read it"
        );
        // A draw that reads it as a buffer, not a texture, reads it too.
        let reading = [orbistoun_gpu::agc_driver::DrawnBy {
            frame: 14,
            draws: 1,
            textures: Vec::new(),
            sampled: Vec::new(),
            read: vec![(0x5080, 0x40)],
        }];
        assert_eq!(
            super::consumed_by(0x5000, 0x100, &reading, &census),
            "read as a buffer by the draws of frame 14"
        );
    }

    /// A sampled texture a compute program stored over is named by that program: where it lies,
    /// how many dispatches ran it, and whether what they wrote was all zero. An image it stored or
    /// a buffer it only stored to counts; one it read does not.
    #[test]
    fn a_texture_a_dispatch_stored_names_its_program() {
        let writes = orbistoun_gpu::agc_driver::DispatchWrites {
            dispatches: 7,
            lit: true,
            program: Some(0x7400_4314_7600),
            stored: Some((0x5000, 0x1000)),
            buffers: vec![(0x9000, 0x100, None), (0xa000, 0x100, Some(true))],
            ..Default::default()
        };
        let census = [(1, writes)];
        let named = super::stored_by(0x5800, &census);
        assert_eq!(
            named.as_deref(),
            Some("stored by the dispatch program at 0x740043147600 (7 dispatch(es), not all zero)")
        );
        assert!(
            super::stored_by(0x9010, &census).is_some(),
            "a buffer only stored to"
        );
        assert_eq!(super::stored_by(0xa010, &census), None, "a buffer it read");
        assert_eq!(super::stored_by(0x6000, &census), None, "past the image");
    }

    /// The render targets a frame sampled are followed to the frames drawn into them, and theirs
    /// in turn, each once and no deeper than the bound: a target drawn from itself does not loop.
    #[test]
    fn the_sampled_targets_are_followed_to_their_frames() {
        let frame =
            |frame: u64, sampled: Vec<(u64, u32, u32)>| orbistoun_gpu::agc_driver::DrawnBy {
                frame,
                draws: 1,
                textures: Vec::new(),
                sampled,
                read: Vec::new(),
            };
        let shown = frame(9, vec![(0x2000, 4, 4), (0x3000, 4, 4)]);
        let into = |base: u64| match base {
            0x2000 => Some(frame(5, vec![(0x4000, 4, 4), (0x2000, 4, 4)])),
            0x4000 => Some(frame(2, Vec::new())),
            _ => None,
        };
        let followed: Vec<(u64, u64)> = super::sampled_targets(&shown, 4, &into)
            .iter()
            .map(|(base, by)| (*base, by.frame))
            .collect();
        assert_eq!(followed, vec![(0x2000, 5), (0x4000, 2)]);
        assert_eq!(
            super::sampled_targets(&shown, 1, &into).len(),
            1,
            "no deeper than asked"
        );
    }

    /// A large texture's summary samples all of it, not its first row: one black above and grey
    /// below counts two values, where the first 4096 texels alone said one.
    #[test]
    fn a_textures_summary_samples_all_of_it() {
        let mut texels = vec![0xff00_0000_u32; 8192 * 4];
        texels[16384..].fill(0xff40_4040);
        let said = super::texel_summary(&texels);
        assert!(said.starts_with("first 0xff000000, 2 distinct"), "{said}");
        // Half of it grey below black: half the texels counted are lit.
        assert!(said.contains("2048 of 4096 lit"), "{said}");
        // Lit is any colour channel set, the alpha aside (the low three bytes in memory order).
        assert_eq!(
            super::texel_summary(&[7, 7, 0xff00_0000]),
            "first 0x00000007, 2 distinct, 2 of 3 lit"
        );
    }

    /// Two runs dumping a submission under the same name keep their own: each run's dumps go to
    /// its own directory, since every session's runs share the traces directory (D865).
    #[test]
    fn two_runs_submission_dumps_do_not_collide() {
        use orbistoun_report::RunId;
        let traces = tempfile::tempdir().expect("tempdir");
        let module = "/titles/PPSA04263/eboot.bin";
        let runs = [
            (RunId::new(1_760_000_000_000, 0x10), 0x1000_u64),
            (RunId::new(1_760_000_000_000, 0x20), 0x2000_u64),
        ];
        for (run, base) in &runs {
            let dir = crate::frame_region::run_dir(traces.path(), module, run);
            let submission = orbistoun_gpu::pipeline::Submission {
                guest_memory_base: *base,
                guest_memory: vec![0xdead_beef],
                ..Default::default()
            };
            super::write_submission_to(&dir, &submission, "last");
        }
        for (run, base) in &runs {
            let dir = crate::frame_region::run_dir(traces.path(), module, run);
            let listing = std::fs::read_to_string(dir.join("last-submission.txt"))
                .expect("the run's own listing");
            assert!(
                listing.contains(&format!("window {base:#x}")),
                "{run}'s listing is its own: {listing}"
            );
            assert_eq!(
                std::fs::read(dir.join("last-window.bin")).expect("the run's own window"),
                0xdead_beef_u32.to_le_bytes()
            );
        }
        assert!(
            !traces.path().join("last-submission.txt").exists(),
            "nothing is dumped into the shared traces directory itself"
        );
    }

    /// A drawn frame cleared to zero is no picture, whatever its size; one non-zero byte anywhere
    /// in it, alpha included, is.
    #[test]
    fn a_frame_cleared_to_zero_holds_no_picture() {
        let mut frame = vec![0_u8; 64 * 64 * 4];
        assert!(!holds_a_picture(&frame));
        assert!(!holds_a_picture(&[]));
        *frame.last_mut().expect("a frame") = 1;
        assert!(holds_a_picture(&frame));
    }

    /// The window shows a flip drawn into exactly its buffer whatever the format, a buffer read from
    /// memory only in a scanout format it decodes, and nothing tiled other than video-out tiling 0.
    #[test]
    fn the_window_shows_what_it_can_decode_and_nothing_else() {
        let shape = |format: u64, tiling: u32| orbistoun_video::BufferShape {
            tiling,
            width: 1920,
            height: 1080,
            format,
        };
        let unknown = 0x8000_0000_1100_0000;
        let sdk = 0x8000_0000_0000_0000;
        let at = 0x1000_0000;
        assert!(
            super::window_can_show(at, shape(unknown, 0), Some(at)),
            "drawn into it"
        );
        assert!(!super::window_can_show(
            at,
            shape(unknown, 0),
            Some(at + 0x10_0000)
        ));
        assert!(
            !super::window_can_show(at, shape(unknown, 0), None),
            "an unknown format"
        );
        assert!(
            super::window_can_show(at, shape(sdk, 0), None),
            "a format it decodes"
        );
        assert!(
            !super::window_can_show(at, shape(sdk, 1), None),
            "tiling it does not read"
        );
        assert!(!super::window_can_show(at, shape(unknown, 1), Some(at)));
    }

    /// The submissions carried out before a flip `ORBISTOUN_FRAME_AT` names are kept beside its
    /// frame, numbered from 0 and bounded: what drew a frame mid-run, where the per-draw lines see
    /// only the run's last submission (GTA V's opening, which drew nothing but its title's logo).
    #[test]
    fn submissions_before_a_named_flip_are_kept_by_number() {
        let asked = KeptFlips::parse(Some("2"), Some("3"));
        let mut seen = (0, 0);
        let mut name = |last_flip| super::kept_submission_name(&asked, last_flip, &mut seen);
        assert_eq!(
            name(Some(1)),
            None,
            "flip 2 is only every-N, which keeps no submission"
        );
        assert_eq!(name(Some(2)).as_deref(), Some("flip-000003-submission-00"));
        assert_eq!(name(Some(2)).as_deref(), Some("flip-000003-submission-01"));
        assert_eq!(name(Some(3)), None, "after the flip");
        let first = KeptFlips::parse(None, Some("1"));
        let mut seen = (0, 0);
        assert_eq!(
            super::kept_submission_name(&first, None, &mut seen).as_deref(),
            Some("flip-000001-submission-00"),
            "before any flip, the next is the first"
        );
        let mut seen = (3, super::MOST_KEPT_PER_FLIP);
        assert_eq!(
            super::kept_submission_name(&asked, Some(2), &mut seen),
            None,
            "bounded"
        );
    }

    /// Every N keeps guest flips 1, 1+N, 1+2N; a list keeps exactly the flips it names; the two
    /// combine, and flip 0, which no guest submits, is never kept.
    #[test]
    fn kept_flips_follow_the_guest_flip_index() {
        let every = KeptFlips::parse(Some("30"), None);
        let kept: Vec<u64> = (0..=91).filter(|&flip| every.wants(flip)).collect();
        assert_eq!(
            kept,
            [1, 31, 61, 91],
            "the first drawn flip and every 30th after it"
        );

        let at = KeptFlips::parse(None, Some("1, 30,60,x,0"));
        assert_eq!(at.at, [1, 30, 60], "not-a-number and zero are ignored");
        let kept: Vec<u64> = (0..=100).filter(|&flip| at.wants(flip)).collect();
        assert_eq!(kept, [1, 30, 60], "exactly the flips named");

        let both = KeptFlips::parse(Some("50"), Some("30"));
        let kept: Vec<u64> = (0..=101).filter(|&flip| both.wants(flip)).collect();
        assert_eq!(kept, [1, 30, 51, 101], "every N and the list together");

        assert_eq!(KeptFlips::parse(Some("0"), Some("")), KeptFlips::default());
        assert!(
            !KeptFlips::default().wants(1),
            "nothing asked, nothing kept"
        );
    }

    /// A flipped buffer that is exactly its frame's colour target is shown from the device copy
    /// whatever its video-out format: PPSA28061 flips 3840x2160 in `0x8000000022000000`, not the
    /// SDK's format, and its frames are what it drew (D784).
    #[test]
    fn a_flipped_target_is_shown_whatever_its_video_out_format() {
        let target = orbistoun_gpu::registers::ColourTarget {
            base: 0x7400_1001_0000,
            width: 3840,
            height: 2160,
            pipe_bank_xor: 0,
            layout: orbistoun_gpu::tiling::SurfaceLayout::Rx64Kb,
            place: orbistoun_gpu::registers::Place::Whole,
        };
        let shown = orbistoun_gpu::agc_driver::ShownFrame {
            snapshot: 0,
            target,
            swap: orbistoun_gpu::registers::ComponentSwap::Standard,
        };
        let shape = orbistoun_video::BufferShape {
            tiling: 0,
            width: 3840,
            height: 2160,
            format: 0x8000_0000_2200_0000,
        };
        assert!(super::shows_directly(0x7400_1001_0000, shape, shown));
        assert!(
            !super::shows_directly(0x7400_2001_0000, shape, shown),
            "another buffer"
        );
    }

    /// A scanout read from memory decodes by its format's byte order: the SDK's as B, G, R, A, and
    /// PPSA28061's `0x8000000022000000` as R, G, B, A, the order its draws write it in (D784); any
    /// other format is not decoded.
    #[test]
    fn a_scanout_decodes_by_its_format_s_byte_order() {
        let word = 0x8040_2010;
        let sdk = super::scanout_decoder(0x8000_0000_0000_0000).expect("the SDK's format");
        assert_eq!(sdk(word), 0x8010_2040, "red and blue trade places");
        let rgba = super::scanout_decoder(0x8000_0000_2200_0000).expect("PPSA28061's format");
        assert_eq!(rgba(word), word, "already R, G, B, A");
        assert!(super::scanout_decoder(0x8000_0000_1100_0000).is_none());
    }

    /// For the SDK's targets, written in the alternate order and scanned out as B, G, R, A, the
    /// device copy already is the shown pixel, so showing it as-is changes nothing for them.
    #[test]
    fn the_sdk_s_scanout_shows_the_device_copy_unchanged() {
        let word = 0x8040_2010;
        let alternate = orbistoun_gpu::registers::ComponentSwap::Alternate;
        assert_eq!(
            super::scanout_rgba(orbistoun_gpu::agc_driver::memory_order(word, alternate)),
            word
        );
    }

    /// A dumped scissor reads in pixels too: `SetViewport` printed in hexadecimal made a 1920 x 1080
    /// one read as `780 x 438`.
    #[test]
    fn a_dumped_scissor_is_in_pixels() {
        let line = super::command_line(&orbistoun_gpu::RenderCommand::SetViewport(
            orbistoun_gpu::Rect {
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
            },
        ));
        assert!(line.contains("1920x1080"), "{line}");
        assert!(!line.contains("780"), "{line}");
    }

    /// A dumped submission names its colour target's size in pixels, as decimal: printed with the
    /// rest of the target in hexadecimal, a 1920 x 1080 target read as `780 x 438`.
    #[test]
    fn a_colour_target_is_described_in_pixels() {
        use orbistoun_gpu::registers::{ColourTarget, Place};
        let target = ColourTarget {
            base: 0x40_0000_0000,
            width: 1920,
            height: 1080,
            pipe_bank_xor: 0,
            layout: orbistoun_gpu::tiling::SurfaceLayout::default(),
            place: Place::Whole,
        };
        let line = super::colour_target_line(Some(&target), None, None, 1);
        assert!(line.contains("1920x1080"), "{line}");
        assert!(line.contains("0x4000000000"), "{line}");
        assert!(!line.contains("780"), "{line}");
    }

    use orbistoun_gpu::pipeline::{GuestMemory, Pipeline, Queue};
    use orbistoun_gpu_vulkan::compute::{Availability, probe};
    use orbistoun_translate::{Fidelity, Strategy, Width};

    /// A command reaches the backend only with both a device and a command, and a refused command
    /// counts.
    #[test]
    fn reaching_the_backend_needs_a_device_and_a_command() {
        assert!(
            !RenderOutcome::default().reached_backend(),
            "no device, nothing reached"
        );
        let dev = |executed, refused| RenderOutcome {
            device: Some("test device".to_owned()),
            executed,
            refused,
            refusals: Vec::new(),
            frame: None,
        };
        assert!(
            !dev(0, 0).reached_backend(),
            "a device with no command reached nothing"
        );
        assert!(dev(1, 0).reached_backend(), "a carried command reached it");
        assert!(
            dev(0, 2).reached_backend(),
            "a refused command still reached it"
        );
        assert!(
            !RenderOutcome {
                device: None,
                ..dev(3, 0)
            }
            .reached_backend(),
            "commands with no device reached none"
        );
    }

    /// The two shader payloads at their guest addresses; `VERTEX_ADDR` is where the triangle
    /// record's registers name the vertex shader.
    struct Shaders {
        bytes: Vec<u8>,
    }
    const VERTEX_ADDR: u64 = 0x2_000c_0000;
    impl GuestMemory for Shaders {
        fn read(&self, address: u64, length: usize) -> Option<&[u8]> {
            let offset = usize::try_from(address.checked_sub(VERTEX_ADDR)?).ok()?;
            self.bytes.get(offset..offset.checked_add(length)?)
        }
    }
    fn capture(name: &str) -> Vec<u8> {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("orbistoun-gpu")
            .join("tests")
            .join("captures")
            .join(name);
        let text =
            std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let mut bytes = Vec::new();
        for line in text.lines() {
            for word in line
                .split('#')
                .next()
                .unwrap_or_default()
                .split_whitespace()
            {
                let value = u32::from_str_radix(word, 16)
                    .unwrap_or_else(|e| panic!("{}: {word:?}: {e}", path.display()));
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        bytes
    }

    /// A captured triangle submission reaches a constructed backend through `render`, and its frame
    /// crosses the frame-region route intact through the run path's own step. Skips where no
    /// device is present.
    #[test]
    fn a_captured_submission_reaches_a_constructed_backend() {
        if !matches!(probe(), Availability::Available { .. }) {
            eprintln!("[a_captured_submission_reaches_a_constructed_backend] SKIPPED - no device");
            return;
        }
        let stream = capture("agc-primitive-draw-triangle-fw1240.hex");
        let vertex = capture("agc-primitive-draw-triangle-fw1240.vertex.hex");
        let pixel = capture("agc-primitive-draw-triangle-fw1240.pixel.hex");
        let mut bytes = vec![0u8; 0x1000];
        bytes[..vertex.len()].copy_from_slice(&vertex);
        bytes[0x200..0x200 + pixel.len()].copy_from_slice(&pixel);
        let memory = Shaders { bytes };

        let mut pipeline = Pipeline::new(Strategy::Predicated {
            fidelity: Fidelity::Auto,
            width: Width::default(),
        })
        .expect("a pipeline over the built-in tables");
        let submission = pipeline.submit(&stream, Queue::Draw, &[], &memory);

        let rendered = render(&submission);
        assert!(
            rendered.outcome.reached_backend(),
            "a real submission did not reach a constructed backend: {:?}",
            rendered.outcome
        );

        let (width, height) = rendered
            .outcome
            .frame
            .expect("a drawn submission presents a frame");
        let bytes = rendered.frame_bytes.expect("the presented frame has bytes");

        // Through the run path's own step, not a hand-built write.
        let dir = tempfile::tempdir().expect("a temp frames directory");
        let event = super::render_submission_to(&submission, Some(dir.path()))
            .expect("the run path wrote the frame and returned its descriptor");
        let orbistoun_proto::Event::Frame {
            width: w,
            height: h,
            ..
        } = &event
        else {
            panic!("the run path returned {event:?}, not a frame");
        };
        assert_eq!(
            (*w, *h),
            (width, height),
            "the descriptor carries the frame's size"
        );
        let read = crate::frame_region::read_frame(dir.path(), &event)
            .expect("the frame region reads back");
        assert_eq!(read, bytes, "the rendered frame crossed the route intact");

        // With no frames directory nothing is written and nothing is claimed.
        assert!(
            super::render_submission_to(&submission, None).is_none(),
            "no directory, no descriptor"
        );
    }
}
