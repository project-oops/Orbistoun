//! What the guest put into words.
//!
//! # Why this is a first-class part of a run, and not a Unity feature
//!
//! A guest that is failing usually says so. Engines log their own boot - version, subsystems,
//! device initialisation - and when something goes wrong they write a diagnosis in English before
//! they die. That text is the only oracle in this project that arrives already interpreted: every
//! other signal (a fault address, an import count, a stopped thread) has to be reasoned back to a
//! cause, and this one *is* the cause, in the guest's own words.
//!
//! **Nothing here knows what an engine is.** The capture point is the platform ABI - the format
//! family in the C library and the console's log call - which is the one thing every guest on this
//! target shares whatever it was built with. An engine-specific reader would have to be written
//! again for the next title; this is written once and works for a homebrew payload printing with
//! `printf` exactly as it works for a commercial engine.
//!
//! D186 already used this by hand: two titles printed their own diagnostics through an implemented
//! `printf`, and reading them named four functions. This makes that systematic rather than
//! something somebody has to think to look for.
//!
//! # What it captures, precisely
//!
//! Text the guest **formatted or wrote**, which is deliberately broader than text it printed. A
//! guest that formats a message into a buffer and then hands it to a write path this project does
//! not implement has still said the thing, and that is exactly the case worth seeing - the message
//! survives even when the channel it was meant for does not.
//!
//! # Bounded, and allocation-free on the call path
//!
//! A fixed byte ring, written under a lock and read once the guest has stopped. Recording must not
//! allocate (principle 9), and a log that grows without limit is a log that changes the program it
//! observes - a guest printing in a loop would otherwise take the process down before the run
//! ended.

use std::sync::Mutex;

/// How much of what a guest said is kept.
///
/// The **most recent** that much: the interesting lines are the last ones, because a guest
/// describes its problem immediately before it stops.
pub const SAID_CAPACITY: usize = 64 * 1024;

/// The line separator, written as its code point: a byte literal for it needs an escape, and
/// this file is edited by tools that treat one inconsistently.
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
/// Silent about a poisoned lock: losing a log line is not worth failing a run over, and the run
/// report says how much was kept.
///
/// **One call is one utterance, so an unterminated one gets a newline.** A `printf` without a
/// trailing newline is still a separate thing the guest said, and concatenating it with the next
/// one produced `Argument Count = 1Arg 0 = ...` - two statements read as one. The separator is a
/// display decision and is stated as one: the bytes the guest passed are unchanged, and a caller
/// that ended its own line does not get a blank one (D658).
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
/// Lossy by construction and says so: when the ring has wrapped the first line is whatever was
/// left of one, so it is dropped rather than reported as though it were whole.
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
    // The first line of a wrapped ring begins wherever the cursor landed, which is usually
    // mid-word. Reporting a fragment as a line invites somebody to read meaning into the cut.
    if wrapped && !out.is_empty() {
        out.remove(0);
    }
    out
}

#[cfg(test)]
mod tests {
    /// **A guest that said nothing has said nothing**, rather than one empty line.
    ///
    /// The negative case, because a report that always shows one blank entry teaches a reader
    /// that the section is decorative.
    #[test]
    fn a_silent_guest_produces_no_lines() {
        // Deliberately not asserting on a fresh ring: this is a process-wide static and another
        // test in this binary may have written to it. What must hold either way is that nothing
        // empty is reported.
        assert!(
            super::lines().iter().all(|line| !line.is_empty()),
            "no empty line is ever reported"
        );
    }

    /// What goes in comes out, split on newlines.
    #[test]
    fn what_a_guest_says_comes_back_as_lines() {
        super::note(b"alpha\nbeta\n");
        let lines = super::lines();
        let tail: Vec<&String> = lines.iter().rev().take(2).collect();
        assert_eq!(tail, vec![&"beta".to_owned(), &"alpha".to_owned()]);
    }

    /// A line with no terminator is still kept, because a guest that dies mid-sentence has said
    /// the most interesting thing it will ever say.
    #[test]
    fn an_unterminated_last_line_survives() {
        super::note(b"finished\ncut off here");
        let lines = super::lines();
        assert_eq!(
            lines.last().map(String::as_str),
            Some("cut off here"),
            "the last thing a guest said matters most and is not waiting for a newline"
        );
    }
}
