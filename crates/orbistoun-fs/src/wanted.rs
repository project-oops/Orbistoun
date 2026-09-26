//! Paths a guest asked for and did not get.
//!
//! A directory tree is a fact about the platform, and the guest names the paths it needs:
//! an FTP server calls `stat` on a directory, a title calls `open` on a save file. Every
//! failed resolution is recorded once per distinct path and reported when the run ends, so a
//! mount added later answers a request that was made (D387). Recording happens on the
//! guest's own stack, so it takes a lock and allocates a string and nothing else; the
//! reporting layer formats it after the guest stops.

use std::collections::BTreeSet;
use std::sync::{Mutex, OnceLock};

/// Every path this run could not answer, in order.
fn asked() -> &'static Mutex<BTreeSet<String>> {
    static ASKED: OnceLock<Mutex<BTreeSet<String>>> = OnceLock::new();
    ASKED.get_or_init(|| Mutex::new(BTreeSet::new()))
}

/// How many distinct paths to remember.
///
/// A ceiling, because a guest walking a tree it does not have can ask forever.
const MOST_REMEMBERED: usize = 256;

/// Records a path the mount table could not answer.
///
/// Called only when a resolution fails, so an ordinary run records nothing.
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
        // Said now as well: a kernel log is read while the kernel runs, so an event reaching
        // it only after the guest stops never reaches the guest (D396).
        orbistoun_core::klog::note(&format!("orbistoun: no such path {guest_path}"));
    }
}

/// Every path this run was asked for and could not answer.
///
/// Read once the guest has stopped. Sorted, so two runs' lists can be diffed.
#[must_use]
pub fn unanswered() -> Vec<String> {
    asked()
        .lock()
        .map(|asked| asked.iter().cloned().collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    /// The same path twice is one work item.
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
