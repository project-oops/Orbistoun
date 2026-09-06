//! Which clock a guest asked for, and what it reads.
//!
//! # Why this is here and not in the library that implements `clock_gettime`
//!
//! Two surfaces ask the same question. POSIX `clock_gettime` is implemented in
//! `orbistoun-libc`; `sceKernelClockGettime` is the same call under the platform's own name and
//! belongs to `libkernel`, which `orbistoun-kernel` declares - and **`orbistoun-kernel` does not
//! depend on `orbistoun-libc`**, nor should it: they are sibling subsystems.
//!
//! Copying thirty lines into the second one would have worked and would have drifted. The clock
//! identifiers come from a harvested table, the families that can be answered are a judgement,
//! and the monotonic origin is process-wide - all three are facts about the *platform* rather
//! than about either library, which is what makes this the right floor for them (D536).
//!
//! # One origin, not one per crate
//!
//! [`since_start`] measures from the first call anywhere in the process. When the same code sat
//! in one library that was true by accident; here it is true by construction, and it matters:
//! a monotonic clock read through two names must not answer two different elapsed times.
//!
//! # What is deliberately refused
//!
//! The per-process and per-thread CPU clocks, `CLOCK_UPTIME`, and the second-resolution
//! variants. A guest measuring its own CPU time and receiving wall time gets a number that
//! looks right and is not - which is the failure principle 3 exists to prevent, and answering
//! "the nearest thing available" is how it happens.
//!
//! Reference: identifiers from `sys/sys/_clock_id.h`, via the harvested table.

use std::sync::OnceLock;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// The real-time family: the host's wall clock.
const REAL_TIME: &[&str] = &[
    "CLOCK_REALTIME",
    "CLOCK_REALTIME_PRECISE",
    "CLOCK_REALTIME_FAST",
];

/// The monotonic family: time since this process started.
const MONOTONIC: &[&str] = &[
    "CLOCK_MONOTONIC",
    "CLOCK_MONOTONIC_PRECISE",
    "CLOCK_MONOTONIC_FAST",
];

/// When this process started, for the monotonic clocks.
///
/// **A baseline rather than the host's own uptime.** A monotonic clock's zero is unspecified;
/// what is specified is that it never goes backwards. Process start is a legitimate origin and
/// the one thing here that is certainly stable for the life of the guest.
fn started() -> Instant {
    static START: OnceLock<Instant> = OnceLock::new();
    *START.get_or_init(Instant::now)
}

/// Seconds and nanoseconds since the epoch, as the host knows them.
#[must_use]
pub fn wall_clock() -> (u64, u64) {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or((0, 0), |d| (d.as_secs(), u64::from(d.subsec_nanos())))
}

/// Seconds and nanoseconds since this process started.
#[must_use]
pub fn since_start() -> (u64, u64) {
    let elapsed = started().elapsed();
    (elapsed.as_secs(), u64::from(elapsed.subsec_nanos()))
}

/// What the clock `asked` names reads, or [`None`] for one this cannot answer.
///
/// The identifiers come from the harvested table rather than from recall, because the numbers
/// differ between platforms and a wrong one is a guest silently reading the wrong clock.
#[must_use]
pub fn reading(asked: i64) -> Option<(u64, u64)> {
    let matches = |name: &&str| crate::constants::abi_constant("clock", name) == Some(asked);
    if REAL_TIME.iter().any(matches) {
        return Some(wall_clock());
    }
    if MONOTONIC.iter().any(matches) {
        return Some(since_start());
    }
    None
}

#[cfg(test)]
mod tests {
    /// **The families are told apart, and everything else is refused.**
    ///
    /// # What this asserts
    ///
    /// That the identifiers resolve through the harvested table and that an unanswerable clock
    /// answers `None` rather than the nearest thing. The second half is the point: a guest
    /// asking for its own CPU time and receiving wall time cannot tell.
    ///
    /// # What it cannot assert
    ///
    /// That the values are right. A wall clock and an elapsed time are not reproducible, so
    /// what is checked is *which* source answered - the monotonic reading is small because the
    /// process just started, and the real-time one is not. That distinguishes them without
    /// pinning either to a number no run can repeat.
    #[test]
    fn each_family_answers_from_its_own_source_and_the_rest_are_refused() {
        let id = |name: &str| {
            crate::constants::abi_constant("clock", name)
                .unwrap_or_else(|| panic!("{name} is in the harvested table"))
        };

        let (real, _) = super::reading(id("CLOCK_REALTIME")).expect("real time is answerable");
        let (mono, _) = super::reading(id("CLOCK_MONOTONIC")).expect("monotonic is answerable");
        assert!(
            real > 1_600_000_000,
            "the real-time clock is seconds since the epoch, got {real}"
        );
        assert!(
            mono < 60 * 60 * 24,
            "the monotonic clock counts from process start, so a test reads a small number, \
             got {mono} - a large one means it answered the wall clock"
        );

        for refused in [
            "CLOCK_PROCESS_CPUTIME_ID",
            "CLOCK_THREAD_CPUTIME_ID",
            "CLOCK_UPTIME",
            "CLOCK_SECOND",
        ] {
            assert!(
                super::reading(id(refused)).is_none(),
                "{refused} must be refused rather than answered with the nearest thing - a \
                 guest measuring CPU time and receiving wall time cannot tell"
            );
        }
    }
}
