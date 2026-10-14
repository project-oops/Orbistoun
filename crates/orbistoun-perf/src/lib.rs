//! Profiling a run with Tracy (D829, D831).
//!
//! A worker started with `ORBISTOUN_TRACY=1` runs the Tracy client, which streams every perf phase
//! and span as a zone, a frame at each guest flip and each frame's time as a plot. This crate is
//! everything around that: finding the Tracy tools ([`Tools`]) or installing the pinned version
//! ([`install`]), capturing a run headless ([`Capture`]), and reading a capture back as a report
//! ranked by where the time went ([`Report`]). The command line, the window and the bulk benchmark
//! share it, so each says the same thing about the same capture.

pub mod baseline;
mod report;
mod tools;

pub use report::{FrameTimes, Report, ZoneStat};

/// A count or a span of nanoseconds as a float, for a mean, a share or a rate.
#[must_use]
#[expect(
    clippy::cast_precision_loss,
    reason = "counts and nanosecond totals of a run stay far below 2^52, where an f64 would round them"
)]
pub fn as_f64(n: u64) -> f64 {
    n as f64
}
pub use tools::{Capture, CaptureSummary, TRACY_VERSION, Tools, free_port, install, release_url};
