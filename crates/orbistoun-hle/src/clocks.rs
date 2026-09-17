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

/// Where the wall clock starts when it is not the host's.
///
/// A fixed instant, so a guest that formats a date gets the same string every run. `2026-01-01
/// 00:00:00 UTC`, chosen for being obviously synthetic rather than plausibly today - a
/// timestamp a reader might mistake for a real one is the failure principle 3 names.
const FIXED_EPOCH_SECONDS: u64 = 1_767_225_600;

/// How far the logical clock moves each time a guest looks at it.
///
/// # Why it moves at all, and why by this much
///
/// D256 refused to pin the clock, and was right about the reason: *"pinning it would stop any
/// title that waits for time to pass"*. A clock that repeats does not have to be a clock that
/// stands still - one that advances by a fixed step per observation does both, and that is the
/// option D256 did not have in front of it.
///
/// One microsecond because it is small enough that a guest timing its own work reads a
/// plausible number, and large enough that a spin-wait on a millisecond terminates in a
/// thousand reads rather than a million. A guest that sleeps advances it by what it asked for,
/// through [`advance`], so waiting for real durations still works.
const STEP_NANOS: u128 = 1_000;

/// The logical clock, in nanoseconds since the guest started.
static LOGICAL_NANOS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Whether the guest reads a repeating clock rather than the host's.
///
/// Cached, because this is read on every clock call a guest makes and `Var::get` allocates.
fn logical() -> bool {
    static LOGICAL: OnceLock<bool> = OnceLock::new();
    // Anything but an explicit `host` is the repeating clock: the default is the one that
    // makes a run comparable, because a measurement that cannot be repeated is not one
    // (D181, D238, D582).
    *LOGICAL.get_or_init(|| orbistoun_env::CLOCK.get().as_deref() != Some("host"))
}

/// Nanoseconds since the guest started, from whichever clock this run reads.
///
/// **One source, so two names cannot disagree.** The platform's `GetProcessTime`, its tick
/// counter and POSIX `clock_gettime` are the same span in different units, and a guest
/// converting between them lands somewhere else if they are read from different clocks.
#[must_use]
pub fn since_start_nanos() -> u128 {
    if logical() {
        let before = LOGICAL_NANOS.fetch_add(
            u64::try_from(STEP_NANOS).unwrap_or(1),
            std::sync::atomic::Ordering::Relaxed,
        );
        return u128::from(before);
    }
    started().elapsed().as_nanos()
}

/// Moves the logical clock forward by `nanos`, for a guest that asked to wait.
///
/// **A sleep is time passing, and the clock has to agree.** A guest that sleeps ten
/// milliseconds and then reads a clock that moved a microsecond concludes the sleep did not
/// happen - which is the failure D275 records for a tick counter that did not advance at all.
/// Does nothing under the host clock, where the sleep moved it already.
pub fn advance(nanos: u128) {
    if !logical() {
        return;
    }
    LOGICAL_NANOS.fetch_add(
        u64::try_from(nanos).unwrap_or(u64::MAX),
        std::sync::atomic::Ordering::Relaxed,
    );
}

/// Whether this run's clock repeats, for the report to say so.
#[must_use]
pub fn repeats() -> bool {
    logical()
}

/// Seconds and nanoseconds since the epoch, as this run's clock knows them.
#[must_use]
pub fn wall_clock() -> (u64, u64) {
    if logical() {
        let nanos = since_start_nanos();
        let seconds = u64::try_from(nanos / u128::from(NANOS_PER_SECOND)).unwrap_or(0);
        let rest = u64::try_from(nanos % u128::from(NANOS_PER_SECOND)).unwrap_or(0);
        return (FIXED_EPOCH_SECONDS.saturating_add(seconds), rest);
    }
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or((0, 0), |d| (d.as_secs(), u64::from(d.subsec_nanos())))
}

/// Nanoseconds in a second, named once because three conversions here need it.
const NANOS_PER_SECOND: u64 = 1_000_000_000;

/// Seconds and nanoseconds since this process started.
#[must_use]
pub fn since_start() -> (u64, u64) {
    let nanos = since_start_nanos();
    (
        u64::try_from(nanos / u128::from(NANOS_PER_SECOND)).unwrap_or(u64::MAX),
        u64::try_from(nanos % u128::from(NANOS_PER_SECOND)).unwrap_or(0),
    )
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
            concat!(
                "the monotonic clock counts from process start, so a test reads a small number, ",
                "got {} - a large one means it answered the wall clock"
            ),
            mono
        );

        for refused in [
            "CLOCK_PROCESS_CPUTIME_ID",
            "CLOCK_THREAD_CPUTIME_ID",
            "CLOCK_UPTIME",
            "CLOCK_SECOND",
        ] {
            assert!(
                super::reading(id(refused)).is_none(),
                concat!(
                    "{} must be refused rather than answered with the nearest thing - a ",
                    "guest measuring CPU time and receiving wall time cannot tell"
                ),
                refused
            );
        }
    }

    /// **A clock that repeats must still move**, or every spin-wait becomes an infinite loop.
    ///
    /// This is the whole of D256's objection to pinning the clock, and the property that makes
    /// the logical one an answer to it rather than an override of it. Watched failing: with
    /// `STEP_NANOS` set to zero, two readings are equal and this rejects it.
    #[test]
    fn the_clock_advances_on_every_reading() {
        let first = super::since_start_nanos();
        let second = super::since_start_nanos();
        assert!(
            second > first,
            "two readings gave the same time, so a guest waiting for time to pass never stops"
        );
    }

    /// A sleep moves the clock by what was asked for.
    ///
    /// A guest that sleeps ten milliseconds and reads a clock that moved a microsecond concludes
    /// the sleep did not happen - the failure D275 records for a counter that never advanced,
    /// arriving by a different route.
    #[test]
    fn a_sleep_moves_the_clock_by_what_it_asked_for() {
        if !super::repeats() {
            // Under the host clock the sleep moved it already and `advance` is a no-op, which
            // is the documented behaviour rather than something to assert against.
            return;
        }
        let before = super::since_start_nanos();
        super::advance(10_000_000);
        let after = super::since_start_nanos();
        assert!(
            after >= before + 10_000_000,
            "a ten-millisecond sleep did not move the clock ten milliseconds"
        );
    }

    /// The wall clock starts from a fixed instant, so a formatted date repeats.
    #[test]
    fn the_wall_clock_starts_somewhere_obviously_synthetic() {
        if !super::repeats() {
            return;
        }
        let (seconds, _) = super::wall_clock();
        assert!(
            seconds >= super::FIXED_EPOCH_SECONDS,
            "the wall clock ran before its own epoch"
        );
        // Two years of guest time would be a run nobody has had; the point is that this is
        // nowhere near a real 'now', so a reader cannot mistake it for one.
        assert!(
            seconds < super::FIXED_EPOCH_SECONDS + 63_072_000,
            "the wall clock is far enough from its epoch to look like a real timestamp"
        );
    }
}
