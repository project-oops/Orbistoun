//! Driving a captured graphics submission to a headless backend.
//!
//! `libSceAgcDriver`'s submit handler (in `orbistoun-gpu`) captures a guest's command buffer as a
//! [`Submission`] but attaches no backend: `orbistoun-gpu` has no dependency on a graphics runtime
//! (principle 12), and the worker is where a device is opened (D695). This is that step - after a
//! run, the worker takes the last submission a guest made and drives its commands to a constructed
//! [`VulkanBackend`], so a real submission reaches a real backend rather than only a report
//! (`-36c0`).
//!
//! Headless: it renders where a device is present and says so where one is not, rather than
//! requiring one. The worker runs on machines with no GPU, and a run there still reports its reach
//! and imports - so the absence of a device is a fact to record, not a failure.

use orbistoun_gpu::drive;
use orbistoun_gpu::pipeline::Submission;
use orbistoun_gpu_vulkan::VulkanBackend;
use orbistoun_gpu_vulkan::compute::{Availability, probe};

/// What driving a real submission to a constructed backend produced.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RenderOutcome {
    /// The graphics device the submission reached, or `None` where this build has none.
    pub device: Option<String>,
    /// RenderCommands the backend carried out.
    pub executed: usize,
    /// RenderCommands the backend refused as unsupported - a named gap, but still reaching it.
    pub refused: usize,
    /// The presented frame's dimensions, when one was produced.
    pub frame: Option<(u32, u32)>,
}

impl RenderOutcome {
    /// Whether a command reached the backend - the seam `-36c0` exists to open. A refusal counts:
    /// an unsupported command arriving at a constructed backend is still the path being established,
    /// whether or not the backend can carry it out yet.
    #[must_use]
    pub const fn reached_backend(&self) -> bool {
        self.device.is_some() && self.executed + self.refused > 0
    }
}

/// A submission driven to a backend: what it did, and the frame it produced.
///
/// The outcome is the small summary; the bytes are the bulk, kept apart so a caller that only
/// wants to know a command reached the backend does not carry a frame it will not read. When a
/// frame is present its bytes are `Rgba8`, `outcome.frame`'s dimensions - what the frame route
/// (7f1b) carries to the shim.
#[derive(Debug, Clone, Default)]
pub struct Rendered {
    /// What driving the submission did.
    pub outcome: RenderOutcome,
    /// The presented frame's bytes, when one was produced.
    pub frame_bytes: Option<Vec<u8>>,
}

/// Drives a submission to a headless graphics backend, if this build has a device (D695, `-36c0`).
///
/// Returns the default (`device: None`) where no device is available, so a headless run is reported
/// rather than refused.
#[must_use]
pub fn render(submission: &Submission) -> Rendered {
    match probe() {
        Availability::Unavailable { .. } => Rendered::default(),
        Availability::Available { properties } => {
            let mut backend = VulkanBackend::new();
            // A `BackendError` is the whole drive failing, not a single command being refused
            // (that is `FrameOutcome::refused`); the default reads as "reached the device, carried
            // nothing", which is honest for the rare case a device opens but a frame cannot start.
            let outcome = drive(&mut backend, submission).unwrap_or_default();
            let frame = backend.last_frame();
            Rendered {
                outcome: RenderOutcome {
                    device: Some(properties.device),
                    executed: outcome.executed,
                    refused: outcome.refused,
                    frame: frame.map(|f| (f.width, f.height)),
                },
                frame_bytes: frame.map(|f| f.bytes.clone()),
            }
        }
    }
}

/// Takes the last submission a guest made, drives it to a backend, and logs the outcome - the point
/// on the run path at which a run constructs a backend and a real submission reaches it (`-36c0`).
///
/// Does nothing when no guest reached a submission, which is every corpus title today: they fault
/// before `sceAgcDriverSubmitDcb`. So this establishes the route rather than exercising it - the
/// moment a title submits, its command buffer reaches a real backend instead of a report.
pub fn render_and_log_last_submission() {
    let Some(submission) = orbistoun_gpu::agc_driver::take_last_submission() else {
        return;
    };
    // The frame bytes are dropped here for now: writing them to the frame route (7f1b) so the shim
    // can display them needs the run's frames directory threaded to this point, which is the next
    // step once a title actually submits. `render` already produces them (see the round-trip test).
    let outcome = render(&submission).outcome;
    match &outcome.device {
        None => eprintln!(
            "orbistoun: a submission was made, but this build has no graphics device to render it"
        ),
        Some(device) => {
            let frame = match outcome.frame {
                Some((w, h)) => format!(", frame {w}x{h}"),
                None => String::new(),
            };
            eprintln!(
                "orbistoun: a submission reached the {device} backend: {} command(s) driven, {} refused{frame}",
                outcome.executed, outcome.refused,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{RenderOutcome, render};
    use orbistoun_gpu::pipeline::{GuestMemory, Pipeline, Queue};
    use orbistoun_gpu_vulkan::compute::{Availability, probe};
    use orbistoun_translate::{Fidelity, Strategy, Width};

    /// A command reaches the backend only with both a device and a command - and a refused command
    /// counts, because the seam `-36c0` opens is the arrival, not the carrying-out.
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

    /// The two shader payloads at their console addresses; `VERTEX_ADDR` is where the triangle
    /// record's registers name the vertex shader (as `console_triangle.rs` lays it out).
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

    /// **A real captured submission reaches a constructed backend.** This is `-36c0` end to end:
    /// the console triangle's command buffer, the same one f50b renders, driven through `render`
    /// to a device the worker opens itself. Skips where no device is present, the way every device
    /// test does - a headless machine reports `device: None` rather than failing.
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

        // And the frame it produced crosses the 7f1b frame route intact: the worker writes its
        // bytes into a region, the shim reads them back by the descriptor. This is `render`
        // (-36c0) meeting the transport (7f1b) - the two halves of "the worker hands the shim the
        // bytes" (D695), end to end.
        let (width, height) = rendered
            .outcome
            .frame
            .expect("a drawn submission presents a frame");
        let bytes = rendered.frame_bytes.expect("the presented frame has bytes");
        let dir = tempfile::tempdir().expect("a temp frames directory");
        let event = crate::frame_region::write_frame(
            dir.path(),
            1,
            width,
            height,
            orbistoun_proto::FrameFormat::Rgba8,
            &bytes,
        )
        .expect("the frame region is written");
        let read = crate::frame_region::read_frame(dir.path(), &event)
            .expect("the frame region reads back");
        assert_eq!(read, bytes, "the rendered frame crossed the route intact");
    }
}
