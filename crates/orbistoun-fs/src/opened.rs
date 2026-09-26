//! Paths a guest asked for and got, and the reads and seeks made on them.
//!
//! The counterpart of [`crate::wanted`]: a run can open everything it asks for and still
//! read nothing useful, and this names what it opened. Successes are the common case, so
//! recording is gated on `ORBISTOUN_TRACE_OPENS` and an ordinary run pays one atomic load
//! per open. Recording happens on the guest's own stack, so it takes a lock and allocates a
//! string and nothing else; the reporting layer formats it after the guest stops (D381).

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Mutex, OnceLock};

/// Every path this run opened, in order.
fn taken() -> &'static Mutex<BTreeSet<String>> {
    static TAKEN: OnceLock<Mutex<BTreeSet<String>>> = OnceLock::new();
    TAKEN.get_or_init(|| Mutex::new(BTreeSet::new()))
}

/// How many distinct paths to remember.
///
/// A title that streams assets opens files for as long as it runs, so an unbounded list
/// grows with the run's length.
const MOST_REMEMBERED: usize = 256;

/// Whether recording is on: unread, on, off.
///
/// Cached, because `Var::get` allocates a `String`, which per open on the guest's stack would
/// cost more than the recording it guards.
static ENABLED: AtomicU8 = AtomicU8::new(UNREAD);

/// Not yet looked up.
const UNREAD: u8 = 0;
/// Looked up, and on.
const ON: u8 = 1;
/// Looked up, and off.
const OFF: u8 = 2;

/// Whether this run was asked to record what it opens.
fn enabled() -> bool {
    match ENABLED.load(Ordering::Relaxed) {
        ON => true,
        OFF => false,
        _ => {
            let on = orbistoun_env::TRACE_OPENS.is_set();
            ENABLED.store(if on { ON } else { OFF }, Ordering::Relaxed);
            on
        }
    }
}

/// Records a path a guest opened.
///
/// Called only when an open succeeds, so the list holds what the guest got.
pub(crate) fn note(guest_path: &str) {
    if guest_path.is_empty() || !enabled() {
        return;
    }
    let Ok(mut taken) = taken().lock() else {
        return;
    };
    if taken.len() >= MOST_REMEMBERED && !taken.contains(guest_path) {
        return;
    }
    taken.insert(guest_path.to_owned());
}

/// Every path this run opened successfully.
///
/// Read once the guest has stopped. Sorted, so two runs' lists can be diffed.
#[must_use]
pub fn answered() -> Vec<String> {
    taken()
        .lock()
        .map(|taken| taken.iter().cloned().collect())
        .unwrap_or_default()
}

/// Whether this run was recording. The reporting layer otherwise cannot tell an empty list
/// from one nobody asked for.
#[must_use]
pub fn recording() -> bool {
    enabled()
}

/// Every read a guest made, as a line a person reads.
fn reads() -> &'static Mutex<Vec<String>> {
    static READS: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
    READS.get_or_init(|| Mutex::new(Vec::new()))
}

/// Records one read: what was asked for, and what arrived.
///
/// `read_stats` counts reads and bytes; this records which read of which file asked for how
/// much and got how much. Under the same gate as the opens.
pub(crate) fn note_read(guest_path: &str, wanted: usize, got: usize) {
    if !enabled() {
        return;
    }
    let Ok(mut reads) = reads().lock() else {
        return;
    };
    if reads.len() >= MOST_REMEMBERED {
        return;
    }
    reads.push(format!("{guest_path}: asked {wanted}, got {got}"));
}

/// Every read this run made, in order.
#[must_use]
pub fn reads_made() -> Vec<String> {
    reads().lock().map(|r| r.clone()).unwrap_or_default()
}

/// Every seek a guest made, as a line a person reads.
fn seeks() -> &'static Mutex<Vec<String>> {
    static SEEKS: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
    SEEKS.get_or_init(|| Mutex::new(Vec::new()))
}

/// Records one seek: where from, how far, and where it landed.
///
/// Seeking to the end and reading the position is how a guest learns a file's size, so this
/// records the size each guest was told.
pub(crate) fn note_seek(guest_path: &str, from: &str, offset: i64, landed: u64) {
    if !enabled() {
        return;
    }
    let Ok(mut seeks) = seeks().lock() else {
        return;
    };
    if seeks.len() >= MOST_REMEMBERED {
        return;
    }
    seeks.push(format!("{guest_path}: from {from} by {offset} -> {landed}"));
}

/// Every seek this run made, in order.
#[must_use]
pub fn seeks_made() -> Vec<String> {
    seeks().lock().map(|s| s.clone()).unwrap_or_default()
}
#[cfg(test)]
mod tests {
    /// Off by default: an ordinary run records nothing and stays off the hot path.
    #[test]
    fn nothing_is_recorded_unless_it_was_asked_for() {
        // The environment is process-wide and shared by tests, so this asserts the decision
        // rather than setting the variable.
        if orbistoun_env::TRACE_OPENS.is_set() {
            return;
        }
        super::note("/app0/Media/globalgamemanagers");
        assert!(
            !super::answered()
                .iter()
                .any(|p| p == "/app0/Media/globalgamemanagers"),
            "a path was recorded on a run that never asked for it"
        );
        assert!(!super::recording());
    }

    /// An empty path is not an open.
    #[test]
    fn an_empty_path_is_not_recorded() {
        super::note("");
        assert!(!super::answered().iter().any(String::is_empty));
    }
}
