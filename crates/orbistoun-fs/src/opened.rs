//! Paths a guest asked for and got.
//!
//! # The other half of the question `wanted` answers
//!
//! [`crate::wanted`] records what a guest asked for and did not get, and calls that the
//! filesystem's most useful output. It is - but it only names the paths that were *missing*,
//! and a run can fail for the opposite reason: the guest opened everything it asked for and
//! still read nothing worth having.
//!
//! PPSA03416 is exactly that. It performed **one file read of zero bytes** in a whole run,
//! against a title directory holding four hundred megabytes of assets, and the only visible
//! evidence was four paths it probed and did not find - all four of which are the archive
//! layout this title does not use, so all four are red herrings. What it *did* open was not
//! recorded anywhere (D578).
//!
//! # Off unless asked for, which `wanted` does not have to be
//!
//! Failures are rare, so recording all of them costs an ordinary run nothing. Successes are the
//! common case - a title streaming assets opens hundreds - and a lock and a string for each, on
//! the guest's own stack, is an observation heavy enough to change what it observes
//! (principle 9). So this is gated on `ORBISTOUN_TRACE_OPENS` and an ordinary run pays one
//! atomic load per open.
//!
//! # Recorded here, printed by the reporting layer
//!
//! The same rule as `wanted`, and for the same reason: this is reached from the guest's own
//! call, on the guest's own stack, so it takes a lock and allocates a string and nothing else.
//! Formatting and writing to a stream happen after the guest has stopped (D381).

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
/// The same ceiling `wanted` uses, for a stronger reason: a title that streams assets opens
/// files for as long as it runs, and a list with no bound is a leak that grows with the length
/// of the run rather than with what there is to say.
const MOST_REMEMBERED: usize = 256;

/// Whether recording is on: unread, on, off.
///
/// Cached rather than read from the environment each time. `Var::get` allocates a `String`, and
/// doing that per open - on the guest's stack, to decide whether to record - would cost more
/// than the recording it is guarding.
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
/// Called from the calls that open one, and only when they succeed - so what the list holds is
/// what the guest actually got, which is the question it exists to answer.
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
/// Read once the guest has stopped. Sorted, because a list that reorders between runs cannot be
/// diffed - and comparing two runs is most of what this is for.
#[must_use]
pub fn answered() -> Vec<String> {
    taken()
        .lock()
        .map(|taken| taken.iter().cloned().collect())
        .unwrap_or_default()
}

/// Whether this run was recording. For the reporting layer, which otherwise cannot tell an
/// empty list from a list nobody asked for - and those are opposite findings.
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
/// **The other half of `read_stats`.** That counts reads and bytes and whether any was cut
/// short, which says a run read almost nothing and not *which* read of *which* file returned
/// what. PPSA03416 performs one read of zero bytes and the whole question is which file it was
/// against and how much it asked for - a count cannot answer either (D595).
///
/// Under the same gate as the opens, because it is the same question one step later.
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
/// **A seek is how a guest asks how big a file is.** Seek to the end, read the position: the
/// oldest idiom there is, and a title that then calls the file corrupt has been told a size by
/// this call and by nothing else. Neither the opens record nor the read statistics could show
/// it (D596).
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
    /// Off by default, and an ordinary run records nothing.
    ///
    /// **The property that keeps this off the hot path.** A version that recorded regardless
    /// and filtered when reporting would pay the lock and the string on every open, which is
    /// the cost the gate exists to avoid.
    #[test]
    fn nothing_is_recorded_unless_it_was_asked_for() {
        // The environment is process-wide and tests share it, so this asserts the decision
        // rather than setting the variable - mutating it here is the flaky shape D569 names.
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
