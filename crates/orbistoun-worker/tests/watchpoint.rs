//! Making a watchpoint fire, on purpose, against memory this test owns.
//!
//! **A guard nobody has watched trigger is a guard nobody knows anything about**, and that
//! applies twice over here: a watchpoint that silently arms nothing reports "never touched",
//! which is a *finding* rather than a failure - and a wrong one. So the machinery is proved
//! against an address this test writes to itself, where the answer is known in advance.
//!
//! One test rather than several, because arming is per-process state and two of them racing
//! would each see the other's hits.

#![cfg(windows)]

use orbistoun_worker::{report, watchpoint};

/// The word the watchpoint is armed on.
///
/// A `static` rather than a local: the address has to outlive the arming, and eight-byte
/// alignment is a property the hardware requires rather than one to hope for.
static TARGET: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

#[test]
fn a_write_to_a_watched_word_is_trapped_and_attributed() {
    // The handler that turns a debug exception into a recorded site. Without this the trap
    // is an unhandled exception rather than a finding, which is the ordering the arming
    // path in the worker exists to get right.
    assert!(
        report::install(),
        "the vectored handler is what makes a trap observable at all"
    );

    let address = core::ptr::from_ref(&TARGET) as u64;
    // **Read-or-write rather than write-only, deliberately.** The handler reads the watched
    // word to say what the instruction saw, and under this kind that read is itself a
    // watched access - which traps, and reads again, until the process dies reporting
    // nothing. A write-only watchpoint never re-enters, so it is the version of this test
    // that would have passed while the hazard was still there (D278).
    let request = watchpoint::Request {
        address,
        length: 8,
        kind: watchpoint::Kind::Access,
    };
    watchpoint::arm(std::slice::from_ref(&request)).expect("a static word is watchable");

    // The access under test. Written through the atomic so the compiler cannot decide the
    // store is unobservable and remove it - which would leave the test passing by luck on
    // one optimisation level and failing on another.
    TARGET.store(0xDEAD_BEEF, core::sync::atomic::Ordering::SeqCst);

    let sites = watchpoint::sites();
    let report = sites.join("\n");

    // **Asserted on the failure, not on a count.** "never touched" is what a watchpoint that
    // armed nothing produces, and it is indistinguishable from a real negative result - so
    // it is the specific string worth refusing.
    assert!(
        !report.contains("never touched"),
        "the store above happened, so anything else means nothing was armed:\n{report}"
    );
    assert_eq!(sites.len(), 1, "one store, one site:\n{report}");
    assert!(
        report.contains("after the access at"),
        "a site names where the access came from:\n{report}"
    );
    assert!(
        report.contains("0xdeadbeef"),
        "a site says what the word held when it was touched:\n{report}"
    );
    assert_eq!(
        watchpoint::dropped(),
        0,
        "one site cannot overflow a table of thirty-two"
    );
}
