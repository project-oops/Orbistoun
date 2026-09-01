//! Time, and waiting.
//!
//! # Why these are worth having early
//!
//! Between them, `sleep`, `gettimeofday`, `time`, `usleep` and `clock_gettime` are wanted by
//! more of the open-toolchain payloads than the whole socket set: a server's main loop is
//! *wait, poll, timestamp, log*, and it does the first of those before it does anything
//! interesting. A guest whose `sleep` answers an error code does not pause - it spins.
//!
//! # There is no oracle problem here at all
//!
//! POSIX says exactly what each of these does, the structures are two machine words each,
//! and both are in the FreeBSD checkout the ABI constants are harvested from:
//!
//! ```text
//! struct timeval  { time_t tv_sec; suseconds_t tv_usec; }   sys/sys/_timeval.h
//! struct timespec { time_t tv_sec; long        tv_nsec; }   sys/sys/timespec.h
//! ```
//!
//! On this data model every one of those types is 64 bits, so each structure is sixteen
//! bytes of two little-endian words. That is the only assumption here, it follows from the
//! architecture rather than from anything about this target, and it is stated in the
//! knowledge file.
//!
//! # The wall clock is the host's, and that is a real answer
//!
//! Nothing here invents a time. A guest asking what time it is gets what time it is, for the
//! same reason `getpid` answers the host's process id: it is true in every sense that can be
//! checked from inside the guest, and a made-up constant would be indistinguishable until
//! something compared it against something else.

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestFn};

/// Answered by every call here that succeeds.
const OK: u64 = 0;

/// Answered by a call that could not do what was asked.
///
/// Negative one, which is what every one of these interfaces documents as its failure - and
/// deliberately not an invented errno, which is a different question this cannot answer.
const FAILED: u64 = -1_i64 as u64;

/// Nanoseconds in a second, named because it appears in two conversions and a typo in
/// either would be a clock that is wrong by a factor of a thousand.
const NANOS_PER_SECOND: u64 = 1_000_000_000;

/// Microseconds in a second.
const MICROS_PER_SECOND: u64 = 1_000_000;

/// When this process started, for the monotonic clocks.
///
/// **A baseline rather than the host's own uptime.** A monotonic clock's zero is
/// unspecified; what is specified is that it never goes backwards. So process start is a
/// legitimate origin, and it is the one thing here that is certainly stable for the life of
/// the guest.
fn started() -> std::time::Instant {
    use std::sync::OnceLock;
    static START: OnceLock<std::time::Instant> = OnceLock::new();
    *START.get_or_init(std::time::Instant::now)
}

/// Seconds and nanoseconds since the epoch, as the host knows them.
fn wall_clock() -> (u64, u64) {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or((0, 0), |d| (d.as_secs(), u64::from(d.subsec_nanos())))
}

/// Seconds and nanoseconds since this process started.
fn since_start() -> (u64, u64) {
    let elapsed = started().elapsed();
    (elapsed.as_secs(), u64::from(elapsed.subsec_nanos()))
}

/// Writes two machine words where a guest expects a two-field time structure.
///
/// Answers whether it could. A null destination is refused rather than written to, which is
/// the one error case these calls really have.
fn write_pair(address: u64, first: u64, second: u64) -> bool {
    if address == 0 {
        return false;
    }
    let Ok(at) = usize::try_from(address) else {
        return false;
    };
    // Built from the address rather than cast down from a byte pointer, so nothing here
    // claims an alignment the guest never promised. Every access below is unaligned.
    let at = std::ptr::with_exposed_provenance_mut::<u64>(at);
    // SAFETY: a guest-supplied address under the identity mapping (D014), where the guest
    // itself asked for sixteen bytes of answer - the same contract the real call has.
    unsafe { std::ptr::write_unaligned(at, first) };
    // SAFETY: the second field of the same structure the guest provided.
    let next = unsafe { at.add(1) };
    // SAFETY: in bounds by the line above, inside the structure the caller described.
    unsafe { std::ptr::write_unaligned(next, second) };
    true
}

/// `time(tloc)` - seconds since the epoch, answered and optionally written.
///
/// Reference: POSIX.1-2008 `time(3)`. A null `tloc` is not an error: the standard says the
/// value is answered either way, and callers rely on `time(NULL)`.
fn time(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (seconds, _) = wall_clock();
    if let Ok(at) = usize::try_from(args[0])
        .map(|_| args[0])
        .and_then(usize::try_from)
        && at != 0
    {
        let at = std::ptr::with_exposed_provenance_mut::<u64>(at);
        // SAFETY: a guest-supplied address under the identity mapping (D014), where the
        // guest asked for a `time_t` to be written.
        unsafe { std::ptr::write_unaligned(at, seconds) };
    }
    seconds
}

/// `gettimeofday(tp, tzp)` - the wall clock, to the microsecond.
///
/// **The timezone argument is ignored, as it must be.** POSIX marks it obsolete and says
/// that if it is not null the behaviour is unspecified; FreeBSD fills in a structure whose
/// contents have been meaningless for decades. Writing something plausible there would be
/// inventing data; leaving it is what every modern caller expects.
///
/// Reference: POSIX.1-2008 `gettimeofday(3)`; `struct timeval` from `sys/sys/_timeval.h`.
fn gettimeofday(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (seconds, nanos) = wall_clock();
    // A null destination is success with nothing written: the call is then a no-op, which
    // is what a caller passing two nulls asked for.
    if args[0] == 0 {
        return OK;
    }
    if write_pair(
        args[0],
        seconds,
        nanos / (NANOS_PER_SECOND / MICROS_PER_SECOND),
    ) {
        OK
    } else {
        FAILED
    }
}

/// `clock_gettime(clock_id, tp)` - a named clock, to the nanosecond.
///
/// # Which clocks, and why the rest are refused
///
/// Two families are answerable here and are answered: the real-time clocks, which are the
/// host's wall clock, and the monotonic ones, which are time since this process started. The
/// identifiers come from the harvested table rather than from recall, because the numbers
/// differ between platforms and a wrong one is a guest silently reading the wrong clock.
///
/// Anything else - the per-process and per-thread CPU clocks, `CLOCK_UPTIME`, the second
/// variants - is **refused rather than answered with the nearest thing**. A guest measuring
/// its own CPU time and receiving wall time gets a number that looks right and is not, which
/// is the failure this project refuses on principle (principle 3).
///
/// Reference: POSIX.1-2008 `clock_gettime(3)`; identifiers from `sys/sys/_clock_id.h`.
fn clock_gettime(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (asked, destination) = (args[0] as i64, args[1]);
    let matches = |name: &str| orbistoun_hle::constants::abi_constant("clock", name) == Some(asked);

    let (seconds, nanos) = if [
        "CLOCK_REALTIME",
        "CLOCK_REALTIME_PRECISE",
        "CLOCK_REALTIME_FAST",
    ]
    .iter()
    .any(|n| matches(n))
    {
        wall_clock()
    } else if [
        "CLOCK_MONOTONIC",
        "CLOCK_MONOTONIC_PRECISE",
        "CLOCK_MONOTONIC_FAST",
    ]
    .iter()
    .any(|n| matches(n))
    {
        since_start()
    } else {
        return FAILED;
    };

    if write_pair(destination, seconds, nanos) {
        OK
    } else {
        FAILED
    }
}

/// `sleep(seconds)` - waits, and really waits.
///
/// # Why this is a real sleep
///
/// A `sleep` that returns immediately does not save a guest any time - it turns a paced loop
/// into a spin, and the run then measures how fast this emulator can answer the same call a
/// million times. Every payload's main loop is *wait, poll, act*.
///
/// A guest that asks to wait longer than the run has left simply reaches the run's own limit,
/// which exists for exactly this and reports itself honestly. Capping the wait here would be
/// a lie the guest could measure with the clock above.
///
/// Reference: POSIX.1-2008 `sleep(3)`. Answers the seconds *remaining*, which is zero
/// whenever the sleep was not cut short - and nothing here can cut one short, because nothing
/// here delivers a signal.
fn sleep(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    std::thread::sleep(std::time::Duration::from_secs(args[0]));
    0
}

/// `usleep(microseconds)` - the same, finer.
///
/// Reference: POSIX.1-2001 `usleep(3)`. Answers zero on success; the only documented failure
/// is an interruption, which cannot happen here for the reason [`sleep`] gives.
fn usleep(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    std::thread::sleep(std::time::Duration::from_micros(args[0]));
    OK
}

/// `nanosleep(requested, remaining)` - the same again, by structure.
///
/// The remaining-time structure is written as zero when it is asked for, because a sleep here
/// always completes: nothing delivers a signal, so nothing can cut one short. Leaving it
/// untouched would hand back whatever the guest had there.
///
/// Reference: POSIX.1-2008 `nanosleep(3)`; `struct timespec` from `sys/sys/timespec.h`.
fn nanosleep(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (requested, remaining) = (args[0], args[1]);
    if requested == 0 {
        return FAILED;
    }
    let Ok(at) = usize::try_from(requested) else {
        return FAILED;
    };
    let at = std::ptr::with_exposed_provenance::<u64>(at);
    // SAFETY: a guest-supplied `struct timespec` under the identity mapping (D014) - the
    // same contract the real call has.
    let seconds = unsafe { std::ptr::read_unaligned(at) };
    // SAFETY: the second field of the same structure the guest provided.
    let next = unsafe { at.add(1) };
    // SAFETY: in bounds by the line above.
    let nanos = unsafe { std::ptr::read_unaligned(next) };

    if nanos >= NANOS_PER_SECOND {
        // What the standard says is invalid, refused rather than normalised: a caller that
        // built the structure wrongly should be told, not quietly corrected.
        return FAILED;
    }
    std::thread::sleep(std::time::Duration::new(seconds, nanos as u32));
    if remaining != 0 {
        write_pair(remaining, 0, 0);
    }
    OK
}

/// FreeBSD `struct tm`, LP64: nine `int` fields, then `long tm_gmtoff` and `char *tm_zone`.
///
/// The nine ints sit at offsets 0, 4, ... 32; `long` then aligns to 40 and the pointer to 48,
/// for fifty-six bytes in all. `asctime` and a guest reading the struct both take the ints from
/// those offsets, so the layout is the ABI here - taken from FreeBSD `<time.h>`, the same
/// checkout the other constants are harvested from, rather than guessed.
const TM_BYTES: usize = 56;

/// The three-letter day and month names `asctime` prints, in `tm_wday` (Sunday = 0) and
/// `tm_mon` (January = 0) order. ISO C fixes both the names and their order.
const DAY_NAME: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const MONTH_NAME: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// The nine `int` fields of `struct tm`, in order, from a count of seconds since the epoch.
///
/// `[tm_sec, tm_min, tm_hour, tm_mday, tm_mon(0-11), tm_year(-1900), tm_wday(0=Sun), tm_yday,
/// tm_isdst]`. Computed as UTC: no timezone is modelled here, so "local" time is UTC, which is
/// the one honest answer available and is noted as such in the knowledge file.
fn broken_down(secs: i64) -> [i32; 9] {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let hour = (rem / 3_600) as i32;
    let minute = ((rem % 3_600) / 60) as i32;
    let second = (rem % 60) as i32;
    // 1970-01-01 was a Thursday, which is 4 with Sunday at 0. `rem_euclid` keeps it in range
    // for dates before the epoch as well as after.
    let wday = (days + 4).rem_euclid(7) as i32;
    let (year, month, day) = civil_from_days(days);
    [
        second,
        minute,
        hour,
        day,
        month - 1,
        (year - 1_900) as i32,
        wday,
        yday(year, month, day),
        0,
    ]
}

/// Gregorian year, month (1-12) and day (1-31) from a count of days since 1970-01-01.
///
/// Howard Hinnant's `civil_from_days` (public domain, and widely used precisely because it is
/// exact across the whole range of a signed day count rather than only for recent dates).
fn civil_from_days(days: i64) -> (i64, i32, i32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let day = (doy - (153 * mp + 2) / 5 + 1) as i32; // [1, 31]
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as i32; // [1, 12]
    (if month <= 2 { year + 1 } else { year }, month, day)
}

/// Day of the year, zero-based, for a Gregorian date.
fn yday(year: i64, month: i32, day: i32) -> i32 {
    /// Days before the first of each month in a non-leap year.
    const BEFORE: [i32; 12] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let mut count = BEFORE[(month - 1) as usize] + (day - 1);
    if leap && month > 2 {
        count += 1;
    }
    count
}

/// Fills this thread's `struct tm` and answers its address.
///
/// Per-thread, like `errno`: the C `localtime` returns a pointer to a static object a later
/// call may overwrite, and a shared one would let two guest threads corrupt each other's. A
/// guest thread is a host thread here, so `thread_local!` gives exactly the per-thread static
/// the standard allows.
fn fill_tm(fields: [i32; 9]) -> u64 {
    thread_local! {
        static TM: std::cell::UnsafeCell<[u8; TM_BYTES]> =
            const { std::cell::UnsafeCell::new([0; TM_BYTES]) };
    }
    TM.with(|cell| {
        let slot = cell.get();
        // SAFETY: this thread's own buffer under the identity mapping. Nothing else holds a
        // reference to it - it is thread-local and this is the only writer on this thread -
        // and the address stays valid for the life of the thread, which is what the caller
        // keeps.
        let buffer = unsafe { &mut *slot };
        *buffer = [0; TM_BYTES];
        for (index, value) in fields.iter().enumerate() {
            buffer[index * 4..index * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        slot as usize as u64
    })
}

/// `localtime(timer)` and `gmtime(timer)` - a `time_t` broken down into a `struct tm`.
///
/// The two coincide here: no timezone is modelled, so both answer UTC. The result points at
/// this thread's static `struct tm`, exactly as the C library's does.
///
/// Reference: ISO C 7.27.3.4 (`localtime`) / 7.27.3.3 (`gmtime`); `struct tm` from FreeBSD
/// `<time.h>`.
fn localtime(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let timer = args[0];
    if timer == 0 {
        // A null `time_t*` has no time to break down; the C library answers null.
        return 0;
    }
    let Ok(at) = usize::try_from(timer) else {
        return 0;
    };
    let at = std::ptr::with_exposed_provenance::<i64>(at);
    // SAFETY: a guest-supplied `time_t*` under the identity mapping (D014) - the same contract
    // the real call has. `time_t` is a signed 64-bit count of seconds on this data model.
    let seconds = unsafe { std::ptr::read_unaligned(at) };
    fill_tm(broken_down(seconds))
}

/// `asctime(tm)` - the fixed 26-character rendering of a `struct tm`.
///
/// The format is ISO C's exactly: `"Www Mmm%3d %02d:%02d:%02d %d\n"`, with the day and month
/// names above and the year as `1900 + tm_year`. The result points at this thread's static
/// buffer.
///
/// Out-of-range `tm_wday`/`tm_mon` are wrapped rather than allowed to index out of bounds - the
/// C library's behaviour there is undefined, and a fault inside a formatting helper would read
/// as anything but the malformed `struct tm` that caused it.
///
/// Reference: ISO C 7.27.3.1 (`asctime`).
fn asctime(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let tm = args[0];
    if tm == 0 {
        return 0;
    }
    let Ok(base) = usize::try_from(tm) else {
        return 0;
    };
    let base = std::ptr::with_exposed_provenance::<u8>(base);
    let field = |index: usize| {
        // SAFETY: the field's address, `index` ints in from the base at its FreeBSD offset,
        // inside the `struct tm` the guest supplied under the identity mapping (D014).
        let at = unsafe { base.add(index * 4) };
        // SAFETY: one `int` read unaligned from that in-bounds field address.
        unsafe { std::ptr::read_unaligned(at.cast::<i32>()) }
    };
    let (sec, min, hour, mday, mon, year, wday) = (
        field(0),
        field(1),
        field(2),
        field(3),
        field(4),
        field(5),
        field(6),
    );
    let day = DAY_NAME[wday.rem_euclid(7) as usize];
    let month = MONTH_NAME[mon.rem_euclid(12) as usize];
    let text = format!(
        "{day} {month}{mday:3} {hour:02}:{min:02}:{sec:02} {}\n",
        year + 1_900
    );

    thread_local! {
        static RENDERED: std::cell::UnsafeCell<[u8; 64]> =
            const { std::cell::UnsafeCell::new([0; 64]) };
    }
    RENDERED.with(|cell| {
        let slot = cell.get();
        // SAFETY: this thread's own buffer, the sole writer on this thread, valid for its life.
        let buffer = unsafe { &mut *slot };
        *buffer = [0; 64];
        let bytes = text.as_bytes();
        // One short of the buffer, so the terminator the C string needs always fits.
        let take = bytes.len().min(buffer.len() - 1);
        buffer[..take].copy_from_slice(&bytes[..take]);
        slot as usize as u64
    })
}

/// Implementations this module provides, by symbol name.
pub(crate) fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[
        ("time", time),
        ("gettimeofday", gettimeofday),
        ("clock_gettime", clock_gettime),
        ("sleep", sleep),
        ("usleep", usleep),
        ("nanosleep", nanosleep),
        ("localtime", localtime),
        // No timezone is modelled, so `gmtime` and `localtime` are the same UTC breakdown.
        ("gmtime", localtime),
        ("asctime", asctime),
    ]
}

#[cfg(test)]
mod tests {
    use orbistoun_core::GUEST_ARG_REGISTERS;

    /// Calls one of these with the given arguments.
    fn call(f: fn(&[u64; GUEST_ARG_REGISTERS]) -> u64, args: [u64; GUEST_ARG_REGISTERS]) -> u64 {
        f(&args)
    }

    #[test]
    fn the_wall_clock_is_the_host_clock_and_is_written_where_it_was_asked_for() {
        let mut tv = [0_u64; 2];
        let answered = call(
            super::gettimeofday,
            [std::ptr::addr_of_mut!(tv) as usize as u64, 0, 0, 0, 0, 0],
        );
        assert_eq!(answered, 0);
        // Later than the release of the console this emulates, and not a fabricated
        // constant: the point is that it is a real time rather than a plausible one.
        assert!(tv[0] > 1_600_000_000, "seconds since the epoch");
        assert!(
            tv[1] < super::MICROS_PER_SECOND,
            "microseconds, not nanoseconds"
        );
    }

    #[test]
    fn time_answers_and_writes_the_same_number() {
        let mut when = 0_u64;
        let answered = call(
            super::time,
            [std::ptr::addr_of_mut!(when) as usize as u64, 0, 0, 0, 0, 0],
        );
        assert_eq!(answered, when, "the answer and the write agree");
        assert!(when > 1_600_000_000);
    }

    /// A null `tloc` is not an error - callers rely on `time(NULL)`.
    #[test]
    fn time_with_nowhere_to_write_still_answers() {
        assert!(call(super::time, [0; GUEST_ARG_REGISTERS]) > 1_600_000_000);
    }

    /// **The identifiers come from the harvested table**, and the two families differ.
    #[test]
    fn the_two_clock_families_answer_different_things() {
        let realtime =
            orbistoun_hle::constants::abi_constant("clock", "CLOCK_REALTIME").expect("harvested");
        let monotonic =
            orbistoun_hle::constants::abi_constant("clock", "CLOCK_MONOTONIC").expect("harvested");

        let mut wall = [0_u64; 2];
        let mut up = [0_u64; 2];
        assert_eq!(
            call(
                super::clock_gettime,
                [
                    realtime as u64,
                    std::ptr::addr_of_mut!(wall) as usize as u64,
                    0,
                    0,
                    0,
                    0
                ]
            ),
            0
        );
        assert_eq!(
            call(
                super::clock_gettime,
                [
                    monotonic as u64,
                    std::ptr::addr_of_mut!(up) as usize as u64,
                    0,
                    0,
                    0,
                    0
                ]
            ),
            0
        );
        assert!(wall[0] > 1_600_000_000, "the wall clock is a real date");
        assert!(
            up[0] < 60 * 60,
            "and the monotonic one counts from process start"
        );
        assert!(wall[1] < super::NANOS_PER_SECOND);
    }

    /// A clock this cannot answer is refused rather than given the nearest thing.
    #[test]
    fn a_clock_nothing_here_tracks_is_refused_and_nothing_is_written() {
        let mut destination = [0xAA_u64; 2];
        let answered = call(
            super::clock_gettime,
            [
                0x7FFF,
                std::ptr::addr_of_mut!(destination) as usize as u64,
                0,
                0,
                0,
                0,
            ],
        );
        assert_ne!(answered, 0, "a failure, not the wrong clock");
        assert_eq!(
            destination, [0xAA; 2],
            "and the caller's structure is untouched"
        );
    }

    /// Nowhere to put the answer is a refusal rather than a write to null.
    #[test]
    fn a_null_destination_is_refused() {
        let realtime =
            orbistoun_hle::constants::abi_constant("clock", "CLOCK_REALTIME").expect("harvested");
        assert_ne!(
            call(super::clock_gettime, [realtime as u64, 0, 0, 0, 0, 0]),
            0
        );
    }

    /// A sleep really sleeps, which is the whole point of it.
    #[test]
    fn a_short_sleep_actually_waits() {
        let before = std::time::Instant::now();
        assert_eq!(call(super::usleep, [20_000, 0, 0, 0, 0, 0]), 0);
        assert!(
            before.elapsed() >= std::time::Duration::from_millis(15),
            "a sleep that returns immediately turns a paced loop into a spin"
        );
    }

    /// A nanosecond field outside its range is refused rather than normalised.
    #[test]
    fn a_malformed_interval_is_refused_rather_than_corrected() {
        let mut interval = [0_u64, super::NANOS_PER_SECOND + 1];
        assert_ne!(
            call(
                super::nanosleep,
                [
                    std::ptr::addr_of_mut!(interval) as usize as u64,
                    0,
                    0,
                    0,
                    0,
                    0
                ]
            ),
            0
        );
    }

    /// Reads a NUL-terminated string back from a host address one of these answered.
    fn read_c_string(address: u64) -> String {
        let mut bytes = Vec::new();
        let mut at = address as usize as *const u8;
        loop {
            // SAFETY: a pointer this module just returned into its own thread-local buffer,
            // read one byte at a time up to the terminator it wrote.
            let byte = unsafe { *at };
            if byte == 0 {
                break;
            }
            bytes.push(byte);
            // SAFETY: still inside the buffer, advancing toward the terminator.
            at = unsafe { at.add(1) };
        }
        String::from_utf8(bytes).expect("asctime writes ASCII")
    }

    /// Runs `asctime(localtime(&t))` and returns the rendered string.
    fn ctime_of(seconds: i64) -> String {
        let t = seconds;
        let tm = call(
            super::localtime,
            [std::ptr::addr_of!(t) as usize as u64, 0, 0, 0, 0, 0],
        );
        assert_ne!(tm, 0, "localtime answered null for a real time");
        let rendered = call(super::asctime, [tm, 0, 0, 0, 0, 0]);
        assert_ne!(rendered, 0, "asctime answered null for a real struct tm");
        read_c_string(rendered)
    }

    /// **The epoch renders exactly as the C library documents it.** This is the whole chain -
    /// `time_t` in, broken down, formatted - checked against a value with one right answer.
    #[test]
    fn the_epoch_renders_as_the_standard_says() {
        assert_eq!(ctime_of(0), "Thu Jan  1 00:00:00 1970\n");
    }

    /// A leap day exercises the civil-date maths where a naive one is wrong.
    #[test]
    fn a_leap_day_is_computed_correctly() {
        // 2000-02-29 00:00:00 UTC - 2000 is a leap year (divisible by 400).
        assert_eq!(ctime_of(951_782_400), "Tue Feb 29 00:00:00 2000\n");
    }

    /// A date well after the epoch, including the day of the week, with a two-digit day.
    #[test]
    fn a_recent_date_including_the_weekday_is_right() {
        // 2023-11-14 22:13:20 UTC, a Tuesday.
        assert_eq!(ctime_of(1_700_000_000), "Tue Nov 14 22:13:20 2023\n");
    }

    /// A time before the epoch breaks down without underflowing - `div_euclid` is why.
    #[test]
    fn a_time_before_the_epoch_still_breaks_down() {
        // -1 second is one second before 1970, i.e. 1969-12-31 23:59:59, a Wednesday.
        assert_eq!(ctime_of(-1), "Wed Dec 31 23:59:59 1969\n");
    }

    /// A null `time_t*` is answered with null rather than a read of address zero.
    #[test]
    fn localtime_of_null_is_null() {
        assert_eq!(call(super::localtime, [0; GUEST_ARG_REGISTERS]), 0);
    }

    /// The remaining-time structure is cleared rather than left as it was.
    #[test]
    fn a_completed_sleep_reports_no_time_remaining() {
        let interval = [0_u64, 1_000_000];
        let mut left = [0xAA_u64; 2];
        assert_eq!(
            call(
                super::nanosleep,
                [
                    std::ptr::addr_of!(interval) as usize as u64,
                    std::ptr::addr_of_mut!(left) as usize as u64,
                    0,
                    0,
                    0,
                    0
                ]
            ),
            0
        );
        assert_eq!(left, [0, 0], "nothing here can cut a sleep short");
    }
}
