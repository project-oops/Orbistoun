//! Paths a guest asked for and did not get.
//!
//! # Why this is the filesystem's most useful output
//!
//! The mount table is two entries wide, and the question "what else should be in it" has no
//! answer that can be looked up: a directory tree is a fact about the platform, and this
//! project does not read the platform. So it has been an open research question.
//!
//! It is not a research question. **The guest names them.** An FTP server asked to list a
//! directory calls `stat` on it; a title asked for a save file calls `open`. Every one that
//! comes back empty is a path something real wanted, spelled by the thing that wanted it -
//! which is a measurement, and the only kind of evidence this project accepts.
//!
//! So every resolution that fails is recorded, once per distinct path, and the run says so
//! when it ends. A mount added afterwards is then answering a request that was actually made
//! rather than a guess about what a console holds (D387).
//!
//! # Recorded here, printed by the reporting layer
//!
//! Same rule as every other record: this is reached from the guest's own call, on the guest's
//! own stack, so it takes a lock and allocates a string and nothing else. Formatting and
//! writing to a stream happen after the guest has stopped (D381).

use std::collections::BTreeSet;
use std::sync::{Mutex, OnceLock};

/// Every path this run could not answer, in order.
fn asked() -> &'static Mutex<BTreeSet<String>> {
    static ASKED: OnceLock<Mutex<BTreeSet<String>>> = OnceLock::new();
    ASKED.get_or_init(|| Mutex::new(BTreeSet::new()))
}

/// How many distinct paths to remember.
///
/// **A ceiling, because a guest walking a tree it does not have can ask forever.** A title
/// scanning for a file in a hundred directories would otherwise fill memory with the same
/// finding said a hundred ways.
const MOST_REMEMBERED: usize = 256;

/// Records a path the mount table could not answer.
///
/// Called from the calls that resolve one - and only when they fail, so an ordinary run
/// records nothing and pays one comparison.
pub(crate) fn note(guest_path: &str) {
    if guest_path.is_empty() {
        return;
    }
    let Ok(mut asked) = asked().lock() else {
        return;
    };
    if asked.len() >= MOST_REMEMBERED && !asked.contains(guest_path) {
        return;
    }
    if asked.insert(guest_path.to_owned()) {
        // **Said now, not only in the summary.** A kernel log is read while the kernel is
        // running, so an event that only reaches it after the guest has stopped never reaches
        // the guest at all (D396).
        orbistoun_core::klog::note(&format!("orbistoun: no such path {guest_path}"));
    }
}

/// Every path this run was asked for and could not answer.
///
/// Read once the guest has stopped. Sorted, because it is a work item and a list that
/// reorders between runs cannot be diffed.
#[must_use]
pub fn unanswered() -> Vec<String> {
    asked()
        .lock()
        .map(|asked| asked.iter().cloned().collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    /// **The same path twice is one work item**, which is what makes the list readable.
    #[test]
    fn a_path_is_remembered_once() {
        super::note("/system/common/lib/libc.sprx");
        super::note("/system/common/lib/libc.sprx");
        let seen = super::unanswered();
        assert_eq!(
            seen.iter()
                .filter(|p| *p == "/system/common/lib/libc.sprx")
                .count(),
            1
        );
    }

    /// An empty path is not a request for anything.
    #[test]
    fn an_empty_path_is_not_recorded() {
        super::note("");
        assert!(!super::unanswered().iter().any(String::is_empty));
    }
}
