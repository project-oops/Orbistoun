//! The kernel's own log, which this emulator writes.
//!
//! `/dev/klog` is where a FreeBSD kernel reports to the programs running on it, and orbistoun
//! is that kernel. Its lines about a guest - a system call it could not serve, a name nothing
//! implements, a path it does not hold, a fault - are published here (D389). Only
//! kernel-boundary events go in: not the guest's own stdout, and not the emulator's reporting
//! about itself, since a guest may read this back. It is a ring that drops the oldest line when
//! full, because a kernel log is a tail and the guest is never blocked to preserve a line.

use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};

/// How many lines the log keeps: enough for a client connecting after a payload started,
/// small enough that a guest in a loop cannot grow it without bound.
const KEPT_LINES: usize = 512;

/// Bytes one line may hold before it is cut; kernel log lines are short by convention.
const LONGEST_LINE: usize = 512;

/// The log, and what has not been read out of it yet.
#[derive(Debug, Default)]
struct Log {
    /// Lines waiting to be read, oldest first.
    lines: VecDeque<String>,
    /// Bytes of the front line already handed over, for a reader whose buffer was too small.
    consumed: usize,
    /// How many lines were dropped because the ring was full, so the gap can be reported.
    dropped: u64,
}

/// The one log this process keeps.
fn log() -> &'static Mutex<Log> {
    static LOG: OnceLock<Mutex<Log>> = OnceLock::new();
    LOG.get_or_init(|| Mutex::new(Log::default()))
}

/// Writes one line to the kernel log.
///
/// Never called from the syscall dispatcher or anything else on the guest's own stack: this
/// takes a lock and allocates (D381). The reporting layer is where it belongs.
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
/// Zero means nothing is waiting (not ready, rather than end of file). A line longer than the
/// space left is split across calls, which is why the log tracks how much of the front line
/// has gone.
pub fn read_into(into: &mut [u8]) -> usize {
    let Ok(mut log) = log().lock() else {
        return 0;
    };
    let mut written = 0_usize;
    while written < into.len() {
        let Some(front) = log.lines.front() else {
            break;
        };
        // The newline is appended here rather than stored, where `truncate` would lose it.
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
    /// The log is one per process, so these tests share it; each takes this and starts empty.
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

    /// A line longer than the reader's buffer is split across reads, not dropped.
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

    /// Nothing waiting reads zero, meaning not ready rather than end of file.
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
