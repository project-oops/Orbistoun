//! The kernel's own log, which this emulator is in a position to write.
//!
//! # Why there is something true to serve here
//!
//! `klogsrv` exists to forward `/dev/klog` to a socket. It runs under orbistoun, binds its
//! port, accepts a connection - and then has nothing to send, because there is no
//! `/dev/klog`. The payload works; the device does not exist.
//!
//! **It does not have to be invented.** `/dev/klog` is where a FreeBSD kernel says what it is
//! doing to the programs running on it, and orbistoun *is* the kernel those programs are
//! running on. Every line it already writes about a guest - a system call it could not serve,
//! a name nothing implements, a path it does not hold, a fault - is exactly that: the kernel
//! talking about the process. Publishing it is not a fabrication, it is the one thing here
//! that has a genuine claim to the name (D389).
//!
//! # What goes in, and what deliberately does not
//!
//! Kernel-boundary events only: what the guest asked the kernel for and could not have. Not
//! the guest's own `printf` output, which is its stdout and belongs there; not this project's
//! progress reporting about *itself*, which is a fact about the emulator rather than about
//! the process it is running.
//!
//! The distinction matters because a guest may read this back and act on it. A log that mixes
//! "your call failed" with "orbistoun loaded 30086 symbol names" is a log whose reader cannot
//! tell which lines are about it.
//!
//! # Bounded, and lossy at the front
//!
//! A ring: when it is full the oldest line goes. A kernel log is a *tail* - `dmesg` shows you
//! the end of it - and a reader that connects late has always missed the beginning. Blocking
//! the guest to preserve a line nobody has read yet would be the wrong trade in the other
//! direction.

use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};

/// How many lines the log keeps.
///
/// Enough that a client connecting after a payload has started still sees what it did; small
/// enough that a guest in a loop cannot grow this without bound.
const KEPT_LINES: usize = 512;

/// Bytes one line may hold before it is cut.
///
/// A kernel log line is short by convention, and a reader parsing them is entitled to assume
/// they stay that way.
const LONGEST_LINE: usize = 512;

/// The log, and what has not been read out of it yet.
#[derive(Debug, Default)]
struct Log {
    /// Lines waiting to be read, oldest first.
    lines: VecDeque<String>,
    /// Bytes of the front line already handed over, for a reader whose buffer was too small.
    consumed: usize,
    /// How many lines were dropped because the ring was full.
    ///
    /// Counted so the gap can be *said* rather than silently closed: a log that quietly skips
    /// is one whose reader draws conclusions from an order that never happened.
    dropped: u64,
}

/// The one log this process keeps.
fn log() -> &'static Mutex<Log> {
    static LOG: OnceLock<Mutex<Log>> = OnceLock::new();
    LOG.get_or_init(|| Mutex::new(Log::default()))
}

/// Writes one line to the kernel log.
///
/// # Where this must not be called from
///
/// **Not the syscall dispatcher**, and not anything else running on the guest's own stack:
/// this takes a lock and allocates a string, which is what put a fault in the middle of the
/// first syscall a guest ever made here (D381). The reporting layer reads records and writes
/// lines; that is where this belongs.
pub fn note(line: &str) {
    let Ok(mut log) = log().lock() else {
        return;
    };
    let mut line = line.trim_end().to_owned();
    line.truncate(LONGEST_LINE);
    if line.is_empty() {
        return;
    }
    log.lines.push_back(line);
    while log.lines.len() > KEPT_LINES {
        log.lines.pop_front();
        log.consumed = 0;
        log.dropped += 1;
    }
}

/// Reads waiting bytes into `into`, answering how many.
///
/// Zero means nothing is waiting, which a caller reads as *not ready yet* rather than as an
/// end of file - a kernel log has no end while the kernel is running.
///
/// A line longer than the space left is split across calls rather than dropped, which is why
/// the log remembers how much of the front line has gone.
pub fn read_into(into: &mut [u8]) -> usize {
    let Ok(mut log) = log().lock() else {
        return 0;
    };
    let mut written = 0_usize;
    while written < into.len() {
        let Some(front) = log.lines.front() else {
            break;
        };
        // The newline is part of the line as far as a reader is concerned, so it is appended
        // here rather than stored - a stored one would be lost to `truncate`.
        let whole = format!("{front}\n");
        let consumed = log.consumed.min(whole.len());
        let rest = &whole.as_bytes()[consumed..];
        let room = into.len() - written;
        let take = rest.len().min(room);
        into[written..written + take].copy_from_slice(&rest[..take]);
        written += take;
        if take == rest.len() {
            log.lines.pop_front();
            log.consumed = 0;
        } else {
            log.consumed = consumed + take;
            break;
        }
    }
    written
}

/// Whether a read would return without waiting.
#[must_use]
pub fn has_lines() -> bool {
    log().lock().is_ok_and(|log| !log.lines.is_empty())
}

/// How many lines were dropped because the ring filled.
#[must_use]
pub fn dropped() -> u64 {
    log().lock().map_or(0, |log| log.dropped)
}

#[cfg(test)]
mod tests {
    /// The log is one per process, so these tests share it.
    ///
    /// **They failed in parallel and passed alone**, which is the worst way for a test to be
    /// wrong: one test drained what another had just written, so the failure depended on
    /// scheduling. Each takes this and starts from empty, the way the mount tests already do.
    static EXCLUSIVE: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Takes the log for the length of a test, and empties it first.
    fn alone() -> std::sync::MutexGuard<'static, ()> {
        let guard = EXCLUSIVE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut into = [0_u8; 256];
        while super::read_into(&mut into) > 0 {}
        guard
    }

    /// Lines come back in order, with a newline, and the log empties.
    #[test]
    fn lines_read_back_in_order() {
        let _guard = alone();
        super::note("first");
        super::note("second");
        let mut into = [0_u8; 64];
        let n = super::read_into(&mut into);
        let text = std::str::from_utf8(&into[..n]).expect("text");
        assert!(text.starts_with("first\n"), "{text:?}");
        assert!(text.contains("second\n"), "{text:?}");
        assert_eq!(super::read_into(&mut into), 0, "and nothing is left");
    }

    /// **A line longer than the reader's buffer is split, not dropped.**
    ///
    /// The case a naive ring gets wrong: a reader with a small buffer would otherwise lose
    /// the tail of every long line and never know.
    #[test]
    fn a_line_longer_than_the_buffer_continues_next_read() {
        let _guard = alone();
        super::note("abcdefghij");
        let mut small = [0_u8; 4];
        let mut seen = Vec::new();
        for _ in 0..4 {
            let n = super::read_into(&mut small);
            if n == 0 {
                break;
            }
            seen.extend_from_slice(&small[..n]);
        }
        assert_eq!(std::str::from_utf8(&seen).expect("text"), "abcdefghij\n");
    }

    /// Nothing waiting is zero, which is *not ready* rather than an end of file.
    #[test]
    fn an_empty_log_reads_zero() {
        let _guard = alone();
        let mut into = [0_u8; 8];
        assert_eq!(super::read_into(&mut into), 0);
        assert!(!super::has_lines());
    }

    /// An empty line is not an event.
    #[test]
    fn an_empty_line_is_not_recorded() {
        let _guard = alone();
        super::note("   ");
        assert!(!super::has_lines());
    }
}
