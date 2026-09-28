//! Which clock a guest asked for, and what it reads.
//!
//! POSIX `clock_gettime` lives in `orbistoun-libc` and `sceKernelClockGettime` in
//! `orbistoun-kernel`, sibling subsystems that do not depend on each other. The clock
//! identifiers, the answerable families and the monotonic origin are platform facts, so they
//! live here once (D536), and a monotonic clock read through two names answers one elapsed
//! time. Per-process and per-thread CPU clocks, `CLOCK_UPTIME` and the second-resolution
//! variants are refused rather than answered with the nearest clock. Identifiers come from
//! `sys/sys/_clock_id.h` through the harvested table.

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
/// A monotonic clock's zero is unspecified; only that it never goes backwards is. Process start
/// is a stable origin for the life of the guest.
fn started() -> Instant {
    static START: OnceLock<Instant> = OnceLock::new();
    *START.get_or_init(Instant::now)
}

/// Where the wall clock starts when it is not the host's.
///
/// A fixed instant, so a guest formatting a date gets the same string every run.
/// `2026-01-01 00:00:00 UTC`, chosen to read as synthetic rather than as a real timestamp.
const FIXED_EPOCH_SECONDS: u64 = 1_767_225_600;

/// How far the logical clock moves each time a guest looks at it.
///
/// A clock that advances a fixed step per observation repeats across runs without stopping a
/// guest that waits for time to pass (D582). One microsecond: plausible for a guest timing its
/// work, and a spin-wait on a millisecond ends in a thousand reads. A sleep advances it by the
/// requested time through [`advance`].
const STEP_NANOS: u128 = 1_000;

thread_local! {
    /// This thread's logical time, in nanoseconds since the guest started (D735).
    ///
    /// Per thread, so a thread's readings depend only on what it did: one shared counter moved
    /// by every thread made the main thread's time depend on when the host scheduled the others,
    /// and two runs of one build diverged within seconds. A thread starts at its creator's time
    /// ([`begin_thread_at`]).
    static THREAD_NANOS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Whether the guest reads a repeating clock rather than the host's.
///
/// Cached, because this is read on every clock call a guest makes and `Var::get` allocates.
fn logical() -> bool {
    static LOGICAL: OnceLock<bool> = OnceLock::new();
    // Anything but an explicit `host` is the repeating clock, because a run is comparable only if
    // it repeats (D582).
    *LOGICAL.get_or_init(|| orbistoun_env::CLOCK.get().as_deref() != Some("host"))
}

/// Nanoseconds since the guest started, from whichever clock this run reads.
///
/// One source, so `GetProcessTime`, the tick counter and POSIX `clock_gettime` stay the same
/// span in different units.
#[must_use]
pub fn since_start_nanos() -> u128 {
    if logical() {
        let step = u64::try_from(STEP_NANOS).unwrap_or(1);
        let before = THREAD_NANOS.with(|now| {
            let before = now.get();
            now.set(before.saturating_add(step));
            before
        });
        return u128::from(before);
    }
    started().elapsed().as_nanos()
}

/// Moves this thread's logical clock forward by `nanos`, for a guest thread that asked to wait.
///
/// A guest that sleeps ten milliseconds and reads a clock that moved a microsecond concludes the
/// sleep did not happen. Only the waiting thread's clock moves (D735). Does nothing under the host
/// clock, where the sleep moved it already.
pub fn advance(nanos: u128) {
    if !logical() {
        return;
    }
    let nanos = u64::try_from(nanos).unwrap_or(u64::MAX);
    THREAD_NANOS.with(|now| now.set(now.get().saturating_add(nanos)));
}

/// This thread's logical time now, without reading it (a reading moves it): what a thread it
/// creates starts from.
#[must_use]
pub fn thread_time() -> u64 {
    THREAD_NANOS.with(std::cell::Cell::get)
}

/// Starts the calling thread's logical time at `nanos`: its creator's [`thread_time`] when it
/// created it, so a new thread never reads an earlier time than the one that made it (D735).
pub fn begin_thread_at(nanos: u64) {
    THREAD_NANOS.with(|now| now.set(nanos));
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
/// Identifiers come from the harvested table, because the numbers differ between platforms.
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
    /// Each family answers from its own source, and an unanswerable clock answers `None`.
    ///
    /// Values are not reproducible, so the check is which source answered: the monotonic reading
    /// is small because the process just started, and the real-time one is not.
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

    /// A repeating clock still advances on every reading, so a spin-wait terminates.
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
    #[test]
    fn a_sleep_moves_the_clock_by_what_it_asked_for() {
        if !super::repeats() {
            // Under the host clock the sleep moved it already and `advance` is a no-op.
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

    /// A thread's logical time moves only with its own readings and waits (D735): another thread
    /// waiting does not move it, and a thread begun at its creator's time reads from there.
    #[test]
    fn a_thread_s_logical_time_is_its_own_and_starts_at_its_creator_s() {
        if !super::repeats() {
            return;
        }
        let before = super::since_start_nanos();
        std::thread::spawn(|| super::advance(1_000_000_000))
            .join()
            .expect("joins");
        let after = super::since_start_nanos();
        assert!(
            after - before < 1_000_000,
            "another thread's second-long wait moved this thread's clock"
        );
        let born = super::thread_time();
        let child = std::thread::spawn(move || {
            super::begin_thread_at(born);
            super::since_start_nanos()
        })
        .join()
        .expect("joins");
        assert_eq!(
            u64::try_from(child).expect("fits"),
            born,
            "starts where its creator was"
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
        // Nowhere near a real "now", so a reader cannot mistake it for one.
        assert!(
            seconds < super::FIXED_EPOCH_SECONDS + 63_072_000,
            "the wall clock is far enough from its epoch to look like a real timestamp"
        );
    }
}
