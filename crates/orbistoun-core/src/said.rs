//! What the guest put into words.
//!
//! A failing guest usually says why in text, and that text arrives already interpreted. The
//! capture point is the platform ABI - the C library's format family and the log call - so it
//! is engine-independent and serves a homebrew payload and a commercial engine alike (D658).
//! It keeps text the guest formatted or wrote, not only text it printed, so a message survives
//! a write path that is not implemented. Storage is a fixed byte ring written under a lock:
//! recording does not allocate, and a guest printing in a loop cannot grow it.

use std::sync::Mutex;

/// How much of what a guest said is kept: the most recent bytes, because a guest describes
/// its problem immediately before it stops.
pub const SAID_CAPACITY: usize = 64 * 1024;

/// The line separator, written as its code point: a byte literal for it needs an escape,
/// which some editing tools handle inconsistently.
const NEWLINE: u8 = 10;

/// The ring itself.
struct Ring {
    bytes: [u8; SAID_CAPACITY],
    /// Where the next byte goes.
    at: usize,
    /// Whether the ring has wrapped, so a reader knows the start is not the beginning.
    wrapped: bool,
}

/// Everything the guest has said this run.
static SAID: Mutex<Ring> = Mutex::new(Ring {
    bytes: [0; SAID_CAPACITY],
    at: 0,
    wrapped: false,
});

/// Records something the guest formatted or wrote.
///
/// A poisoned lock is ignored: losing a log line is not worth failing a run. One call is one
/// utterance, so an unterminated one is followed by a newline; the guest's bytes are unchanged
/// and a caller that ended its own line does not get a blank one (D658).
pub fn note(text: &[u8]) {
    let Ok(mut ring) = SAID.lock() else {
        return;
    };
    let ends_a_line = text.last() == Some(&NEWLINE);
    let terminator: &[u8] = if ends_a_line { &[] } else { &[NEWLINE] };
    for byte in text.iter().chain(terminator.iter()) {
        let at = ring.at;
        ring.bytes[at] = *byte;
        ring.at = (at + 1) % SAID_CAPACITY;
        if ring.at == 0 {
            ring.wrapped = true;
        }
    }
}

/// Everything kept, oldest first, split into lines.
///
/// When the ring has wrapped, the first line is a fragment and is dropped.
#[must_use]
pub fn lines() -> Vec<String> {
    let Ok(ring) = SAID.lock() else {
        return Vec::new();
    };
    let ordered: Vec<u8> = if ring.wrapped {
        ring.bytes[ring.at..]
            .iter()
            .chain(ring.bytes[..ring.at].iter())
            .copied()
            .collect()
    } else {
        ring.bytes[..ring.at].to_vec()
    };
    let wrapped = ring.wrapped;
    drop(ring);

    let text = String::from_utf8_lossy(&ordered);
    let mut out: Vec<String> = text
        .split('\n')
        .map(|line| line.trim_end_matches('\r').to_owned())
        .filter(|line| !line.is_empty())
        .collect();
    // The first line of a wrapped ring starts wherever the cursor landed, usually mid-word.
    if wrapped && !out.is_empty() {
        out.remove(0);
    }
    out
}

#[cfg(test)]
mod tests {
    use std::sync::{Mutex, PoisonError};

    /// Serialises the tests that write the process-wide ring and then assert on its tail.
    ///
    /// Each writer holds this from its `note` through its assertion so no other writer
    /// interleaves lines into its tail. A reader cannot corrupt a tail and takes no lock. Poisoning
    /// is recovered from so one test's panic does not strand the others.
    static RING_WRITERS: Mutex<()> = Mutex::new(());

    /// A guest that said nothing yields no lines rather than one empty line.
    #[test]
    fn a_silent_guest_produces_no_lines() {
        // Not asserting on a fresh ring: another test in this binary may have written to it. Nothing
        // empty is reported either way.
        assert!(
            super::lines().iter().all(|line| !line.is_empty()),
            "no empty line is ever reported"
        );
    }

    /// What goes in comes out, split on newlines.
    #[test]
    fn what_a_guest_says_comes_back_as_lines() {
        let _writers = RING_WRITERS.lock().unwrap_or_else(PoisonError::into_inner);
        super::note(b"alpha\nbeta\n");
        let lines = super::lines();
        let tail: Vec<&String> = lines.iter().rev().take(2).collect();
        assert_eq!(tail, vec![&"beta".to_owned(), &"alpha".to_owned()]);
    }

    /// A line with no terminator is kept, because a guest that dies mid-sentence may be naming
    /// its fault.
    #[test]
    fn an_unterminated_last_line_survives() {
        let _writers = RING_WRITERS.lock().unwrap_or_else(PoisonError::into_inner);
        super::note(b"finished\ncut off here");
        let lines = super::lines();
        assert_eq!(
            lines.last().map(String::as_str),
            Some("cut off here"),
            "the last thing a guest said matters most and is not waiting for a newline"
        );
    }
}
