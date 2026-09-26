//! What the guest formatted, in the order it formatted it.
//!
//! A guest about to give up usually formats why first, often into a buffer it hands to its
//! own logger or to a service nothing implements, where the message is seen nowhere. When
//! `ORBISTOUN_TRACE_FORMAT` is set, every string a renderer produces is kept here; an ordinary
//! run pays one atomic load per format. Recording runs on the guest's stack, so it takes a lock
//! and pushes a string and nothing else; formatting for a person happens after the guest has
//! stopped (D381).

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Mutex, OnceLock};

/// Every string this run rendered, in order.
fn said() -> &'static Mutex<Vec<String>> {
    static SAID: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
    SAID.get_or_init(|| Mutex::new(Vec::new()))
}

/// How many strings to remember: the last ones, because a guest says the interesting thing
/// just before it stops.
const MOST_REMEMBERED: usize = 256;

/// How long a remembered string may be, so a guest rendering a megabyte does not take the
/// record with it.
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
    // The oldest goes; the useful end of this record is the recent one.
    if said.len() >= MOST_REMEMBERED {
        said.remove(0);
    }
    let text = String::from_utf8_lossy(&rendered[..rendered.len().min(LONGEST)]);
    let text = text.trim_end_matches(['\n', '\r', '\0']);
    // And where it was said from: the call site is what a watchpoint or a disassembly needs
    // next. The format function's own thunk already records the address it returns to.
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
/// Lets the reporting layer tell an empty list from a list nobody asked for.
#[must_use]
pub fn recording() -> bool {
    enabled()
}

#[cfg(test)]
mod tests {
    /// Off by default, and an ordinary run records nothing.
    #[test]
    fn nothing_is_recorded_unless_it_was_asked_for() {
        // The environment is process-wide and tests share it, so this asserts the decision rather
        // than setting the variable (D324).
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
