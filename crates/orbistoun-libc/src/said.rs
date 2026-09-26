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
fn said() -> &'static Mutex<Said> {
    static SAID: OnceLock<Mutex<Said>> = OnceLock::new();
    SAID.get_or_init(|| Mutex::new(Said::default()))
}

/// What was rendered: the first strings, the last ones, and how many fell between.
#[derive(Default)]
struct Said {
    first: Vec<String>,
    last: std::collections::VecDeque<String>,
    dropped: usize,
}

/// How many of the first strings to keep: a title explains a setup failure early, and a flood
/// of later messages would otherwise push the explanation out.
const FIRST_REMEMBERED: usize = 64;

/// How many strings to remember from the end, because a guest says the interesting thing just
/// before it stops.
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
    let text = String::from_utf8_lossy(&rendered[..rendered.len().min(LONGEST)]);
    let text = text.trim_end_matches(['\n', '\r', '\0']);
    // And where it was said from: the call site is what a watchpoint or a disassembly needs
    // next. The format function's own thunk already records the address it returns to.
    let line = match orbistoun_thunk::last_call().map(|call| call.from) {
        Some(from) if from != 0 => format!("{text}   [from {from:#x}]"),
        _ => text.to_owned(),
    };
    if said.first.len() < FIRST_REMEMBERED {
        said.first.push(line);
        return;
    }
    // Past the first few, the oldest of the rest goes; the useful end is the recent one.
    if said.last.len() >= MOST_REMEMBERED {
        said.last.pop_front();
        said.dropped += 1;
    }
    said.last.push_back(line);
}

/// The strings this run rendered, oldest first: the first few, then a line saying how many were
/// not kept, then the last ones.
#[must_use]
pub fn rendered() -> Vec<String> {
    let Ok(said) = said().lock() else {
        return Vec::new();
    };
    let mut out = said.first.clone();
    if said.dropped > 0 {
        out.push(format!("... {} more not kept ...", said.dropped));
    }
    out.extend(said.last.iter().cloned());
    out
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
