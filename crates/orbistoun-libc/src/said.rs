//! What the guest formatted, in the order it formatted it.
//!
//! # The messages a title writes about itself
//!
//! A guest that is about to give up usually says why first. PPSA03416 renders a hundred and
//! fifty-two bytes with `vsnprintf` immediately before calling
//! `sceSystemServiceReportAbnormalTermination`, and that string is the title's own account of
//! what went wrong - the single highest-signal thing in the run, produced by orbistoun itself
//! and then thrown away.
//!
//! Some of it reaches a stream and is visible; most does not. A message formatted into a buffer
//! the guest then hands to its own logger, or drops, or sends to a service nothing implements,
//! is rendered here and seen nowhere (D590).
//!
//! # Off unless asked for
//!
//! A title formats constantly - this one 125 times in a boot that walls early, and a running
//! game far more. Keeping every string is an observation heavy enough to change what it
//! observes, so it is gated on `ORBISTOUN_TRACE_FORMAT` and an ordinary run pays one atomic
//! load per format (principle 9).
//!
//! # Recorded here, printed by the reporting layer
//!
//! Reached from the guest's own call on the guest's own stack: it takes a lock and pushes a
//! string, and nothing else. Formatting for a person happens after the guest has stopped (D381).

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Mutex, OnceLock};

/// Every string this run rendered, in order.
fn said() -> &'static Mutex<Vec<String>> {
    static SAID: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
    SAID.get_or_init(|| Mutex::new(Vec::new()))
}

/// How many strings to remember.
///
/// **The last ones, not the first.** A guest says the interesting thing just before it stops, so
/// a list that filled up early and then refused would keep the least useful end of the run - the
/// opposite of `opened`, where a path is a path whenever it appears.
const MOST_REMEMBERED: usize = 256;

/// How long a remembered string may be.
///
/// Longer than the message that prompted this, and short enough that a guest rendering a
/// megabyte does not take the record with it.
const LONGEST: usize = 512;

/// Whether recording is on: unread, on, off.
static ENABLED: AtomicU8 = AtomicU8::new(UNREAD);

/// Not yet looked up.
const UNREAD: u8 = 0;
/// Looked up, and on.
const ON: u8 = 1;
/// Looked up, and off.
const OFF: u8 = 2;

/// Whether this run was asked to record what it renders.
fn enabled() -> bool {
    match ENABLED.load(Ordering::Relaxed) {
        ON => true,
        OFF => false,
        _ => {
            let on = orbistoun_env::TRACE_FORMAT.is_set();
            ENABLED.store(if on { ON } else { OFF }, Ordering::Relaxed);
            on
        }
    }
}

/// Records a string the guest rendered.
///
/// Called from every renderer, so a message reaches this whether it was printed, written to a
/// descriptor, or formatted into a buffer and never shown.
pub(crate) fn note(rendered: &[u8]) {
    if rendered.is_empty() || !enabled() {
        return;
    }
    let Ok(mut said) = said().lock() else {
        return;
    };
    // The oldest goes, because the useful end of this record is the recent one.
    if said.len() >= MOST_REMEMBERED {
        said.remove(0);
    }
    let text = String::from_utf8_lossy(&rendered[..rendered.len().min(LONGEST)]);
    let text = text.trim_end_matches(['\n', '\r', '\0']);
    // **And where it was said from.** A message names what the guest believes; the call site
    // names the code that believes it, which is what a watchpoint or a disassembly needs next.
    // Taken from the call this render is inside - the format function's own thunk already
    // records the address it returns to, so no new plumbing is needed (D596).
    match orbistoun_thunk::last_call().map(|call| call.from) {
        Some(from) if from != 0 => said.push(format!("{text}   [from {from:#x}]")),
        _ => said.push(text.to_owned()),
    }
}

/// Every string this run rendered, oldest first.
#[must_use]
pub fn rendered() -> Vec<String> {
    said().lock().map(|s| s.clone()).unwrap_or_default()
}

/// Whether this run was recording.
///
/// For the reporting layer, which otherwise cannot tell an empty list from a list nobody asked
/// for - opposite findings, and the distinction `opened` already draws.
#[must_use]
pub fn recording() -> bool {
    enabled()
}

#[cfg(test)]
mod tests {
    /// Off by default, and an ordinary run records nothing.
    #[test]
    fn nothing_is_recorded_unless_it_was_asked_for() {
        // The environment is process-wide and tests share it, so this asserts the decision
        // rather than setting the variable - mutating it here is the flaky shape D569 names.
        if orbistoun_env::TRACE_FORMAT.is_set() {
            return;
        }
        super::note(b"a message no run asked to keep");
        assert!(
            !super::rendered().iter().any(|s| s.contains("no run asked")),
            "a rendered string was kept on a run that never asked for it"
        );
        assert!(!super::recording());
    }

    /// An empty render is not a message.
    #[test]
    fn an_empty_render_is_not_recorded() {
        super::note(b"");
        assert!(!super::rendered().iter().any(String::is_empty));
    }
}
