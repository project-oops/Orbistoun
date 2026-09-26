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

/// Shows the frame the guest just flipped: the [`orbistoun_video::FlipObserver`] the worker
/// installs. The flipped buffer is read out of guest memory, detiled, put in `Rgba8` order,
/// written to a frame region and streamed as an [`Event::Frame`] as it is presented.
///
/// A buffer in a layout this does not know is reported once and not shown, rather than shown
/// wrong.
pub fn present_flip(address: u64, shape: orbistoun_video::BufferShape) {
    static LAST: Mutex<Option<std::time::Instant>> = Mutex::new(None);
    perf::count(perf::Count::Flips);
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
    // comes with a device copy of its own.
    let (written, shown) = orbistoun_gpu::agc_driver::write_back_at_this_flip_showing(due);
    if !written {
        tracing::warn!("a drawn frame could not be written back at the flip");
    }
    report_perf(LIVE_EVENTS.get().copied());
    let Some((sink, dir)) = listening.filter(|_| due) else {
        if let Some(shown) = shown {
            discard_snapshot(shown.snapshot);
        }
        return;
    };
    perf::measure(perf::Phase::Present, || match shown {
        Some(shown) if shows_directly(address, shape, shown) => {
            present_later(shown, sink, dir);
        }
        other => {
            if let Some(shown) = other {
                discard_snapshot(shown.snapshot);
            }
            present_now(address, shape, sink, dir);
        }
    });
}

/// Whether the flipped buffer is exactly the frame's target, in the one scanout layout this
/// shows, so the device copy is the frame with no memory to read (D719).
fn shows_directly(
    address: u64,
    shape: orbistoun_video::BufferShape,
    shown: orbistoun_gpu::agc_driver::ShownFrame,
) -> bool {
    shape.format == SCANOUT_BGRA8
        && shape.tiling == 0
        && address == shown.target.base
        && (shape.width, shape.height) == (shown.target.width, shown.target.height)
}

/// A frame the window will show, handed to the thread that shows it.
struct PresentJob {
    shown: orbistoun_gpu::agc_driver::ShownFrame,
    sink: fn(&Event),
    dir: &'static Path,
}

/// Shows a flipped frame from its device copy on a thread of its own, so the guest does not wait
/// for the window. A frame arriving while another is being shown is dropped, as a slow display
/// misses frames.
fn present_later(
    shown: orbistoun_gpu::agc_driver::ShownFrame,
    sink: fn(&Event),
    dir: &'static Path,
) {
    static PRESENTER: OnceLock<Option<std::sync::mpsc::SyncSender<PresentJob>>> = OnceLock::new();
    let presenter = PRESENTER.get_or_init(|| {
        let (send, receive) = std::sync::mpsc::sync_channel::<PresentJob>(1);
        std::thread::Builder::new()
            .name("orbistoun-present".to_owned())
            .spawn(move || {
                for job in receive {
                    present_shown(&job);
                }
            })
            .ok()
            .map(|_| send)
    });
    let sent = presenter
        .as_ref()
        .is_some_and(|send| send.try_send(PresentJob { shown, sink, dir }).is_ok());
    if !sent {
        discard_snapshot(shown.snapshot);
    }
}

/// Reads a shown frame's device copy and streams it, converting each pixel from its memory order
/// to `Rgba8` as [`present_now`] does from memory (D719).
fn present_shown(job: &PresentJob) {
    let shown = job.shown;
    let Some(frame) = take_snapshot(shown.snapshot) else {
        return;
    };
    let linear: Vec<u32> = frame
        .iter()
        .map(|&word| scanout_rgba(orbistoun_gpu::agc_driver::memory_order(word, shown.swap)))
        .collect();
    let bytes = zerocopy::IntoBytes::as_bytes(linear.as_slice());
    emit_presented(
        (shown.target.width, shown.target.height),
        bytes,
        job.sink,
        job.dir,
    );
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
    // The finer spans, when `ORBISTOUN_PERF_DETAIL` asks for them: each one's time and count.
    if perf::detail() {
        let spans: Vec<String> = perf::Span::ALL
            .iter()
            .zip(perf::take_spans())
            .filter(|(_, (_, counted))| *counted > 0)
            .map(|(span, (spent, counted))| {
                format!("{} {:.1} ms/{counted}", span.label(), ms(spent))
            })
            .collect();
        tracing::info!("perf detail: {}", spans.join(", "));
    }
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
    };
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

/// Detiles a flipped buffer and streams it as a frame.
fn present_now(address: u64, shape: orbistoun_video::BufferShape, sink: fn(&Event), dir: &Path) {
    static REFUSED: std::sync::Once = std::sync::Once::new();
    let (width, height) = (shape.width, shape.height);
    let Some(tiled) = (shape.format == SCANOUT_BGRA8 && shape.tiling == 0)
        .then(|| read_scanout(address, width, height))
        .flatten()
    else {
        REFUSED.call_once(|| {
            tracing::warn!(
                "a flipped buffer at {address:#x} ({width}x{height}, format {:#x}, tiling {}) is not one this shows - no live frames",
                shape.format, shape.tiling
            );
        });
        return;
    };
    if tiled.len() < orbistoun_gpu::tiling::surface_words_64kb_rx_bpp4(width, height) {
        return;
    }
    // B, G, R, A in memory to R, G, B, A, swapped as each texel is detiled.
    let linear = orbistoun_gpu::tiling::detile_surface_64kb_rx_bpp4_mapped(
        &tiled,
        width,
        height,
        scanout_rgba,
    );
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
    Some(
        bytes
            .chunks_exact(4)
            .map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]))
            .collect(),
    )
}

/// How many drawn submissions a run dumps, in order.
const DUMPED_SUBMISSIONS: u64 = 64;

/// The file in the traces directory holding the latest frame drawn at submit.
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

/// A kept frame's pixels as linear `Rgba8` words, releasing it.
pub fn take_snapshot(id: u64) -> Option<Vec<u32>> {
    on_device(move || {
        let pixels = live_backend()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_mut()?
            .take_snapshot(id)?;
        Some(
            <[u32] as zerocopy::FromBytes>::ref_from_bytes(&pixels.bytes).map_or_else(
                |_| {
                    pixels
                        .bytes
                        .chunks_exact(4)
                        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                        .collect()
                },
                <[u32]>::to_vec,
            ),
        )
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

/// Writes a submission whose drive failed on the device into the run's frames directory, one
/// command per numbered line, with each shader module's SPIR-V beside it for a disassembler.
fn write_failed_submission(submission: &Submission) {
    write_submission(submission, "failed");
}

/// Writes `submission` under `name`: `failed` for a drive the device refused, `last` for the
/// run's last submission, which is where a wrong picture is read from.
fn write_submission(submission: &Submission, name: &str) {
    let Some(dir) = frames_dir() else {
        return;
    };
    let mut text = String::new();
    for (index, command) in submission.commands.iter().enumerate() {
        let shown = match command {
            orbistoun_gpu::RenderCommand::BindTexture {
                slot,
                width,
                height,
                ..
            } => format!("BindTexture {{ slot {slot}, {width}x{height} }}"),
            other => format!("{other:x?}"),
        };
        let _ = writeln!(text, "{index:5}  {shown}");
    }
    let _ = writeln!(
        text,
        "window {:#x}, {} words, in {name}-window.bin",
        submission.guest_memory_base,
        submission.guest_memory.len()
    );
    let _ = writeln!(
        text,
        "colour target {:x?}, tiling {:?}, format {:?}, {} base(s)",
        submission.colour_target,
        submission.colour_target_tiling,
        submission.colour_target_format,
        submission.colour_target_bases
    );
    let listing = dir.join(format!("{name}-submission.txt"));
    let window: Vec<u8> = submission
        .guest_memory
        .iter()
        .flat_map(|w| w.to_le_bytes())
        .collect();
    let written = std::fs::write(&listing, text).and_then(|()| {
        std::fs::write(dir.join(format!("{name}-window.bin")), &window)?;
        for (id, words) in &submission.modules {
            let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
            std::fs::write(dir.join(format!("{name}-module-{}.spv", id.0)), bytes)?;
        }
        Ok(())
    });
    match written {
        Ok(()) => tracing::info!(
            "the {name} submission's commands and {} module(s) are in {name}-submission.txt and {name}-module-*.spv in the traces directory",
            submission.modules.len()
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
    log_execution();
    let submission = orbistoun_gpu::agc_driver::take_last_submission()?;
    write_submission(&submission, "last");
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
                    tracing::info!(
                        "the last submission's frame, drawn at submit, is {region} in the traces directory"
                    );
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

/// Where the run's frames go: the traces directory, where the shim reads a frame region from
/// (D695). Unset in a run with no paths, which writes no frame.
fn frames_directory() -> &'static OnceLock<PathBuf> {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    &DIR
}

/// Sets the run's frames directory. First set wins; a run has one.
pub fn frames_to(dir: PathBuf) {
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
                tracing::info!("the rendered frame is {region} in the traces directory");
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
/// completion, and where the last one stopped when it did not (D705).
fn log_execution() {
    let record = orbistoun_gpu::agc_driver::execution();
    if record.submissions == 0 {
        return;
    }
    let last = match record.last.map(|l| l.stopped) {
        Some(orbistoun_gpu::cp::Stopped::Completed) | None => "ran to completion".to_owned(),
        Some(orbistoun_gpu::cp::Stopped::NeedsGpu { offset, opcode }) => format!(
            "stopped at byte {offset:#x}: opcode {opcode:#04x} needs the GPU, so nothing after it retired"
        ),
        Some(orbistoun_gpu::cp::Stopped::DrawsInterleaved { offset, opcode }) => format!(
            "stopped at its first draw: opcode {opcode:#04x} at byte {offset:#x} sits between its draws, so they could not run as one"
        ),
        Some(other) => format!("stopped: {other:?}"),
    };
    tracing::info!(
        "the command processor carried out {} of {} submission(s) to completion, {} with their draws ({} bytes written); the last {last}",
        record.completed,
        record.submissions,
        record.drawn,
        record.bytes_written,
    );
}

#[cfg(test)]
mod tests {
    use super::{RenderOutcome, render};
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
