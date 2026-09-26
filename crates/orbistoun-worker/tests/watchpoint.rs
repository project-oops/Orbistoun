//! Making a watchpoint fire, on purpose, against memory this test owns.
//!
//! A watchpoint that silently arms nothing reports "never touched", which reads as a finding,
//! so the machinery is proved against an address this test writes itself. One test, because
//! arming is per-process state and two tests would see each other's hits.
//! would each see the other's hits.

#![cfg(windows)]

use orbistoun_worker::{report, watchpoint};

/// The word the watchpoint is armed on.
///
/// A `static` rather than a local, so the address outlives the arming and is eight-byte
/// aligned as the hardware requires.
static TARGET: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

/// A write to a watched word is trapped and attributed to a site with the value it held.
#[test]
fn a_write_to_a_watched_word_is_trapped_and_attributed() {
    // The handler turns a debug exception into a recorded site; without it the trap is an
    // unhandled exception.
    assert!(
        report::install(),
        "the vectored handler is what makes a trap observable at all"
    );

    let address = core::ptr::from_ref(&TARGET) as u64;
    // Read-or-write, so the handler's own read of the watched word is itself a watched access
    // and the re-entrancy guard is exercised; a write-only watchpoint never re-enters.
    let request = watchpoint::Request {
        address,
        length: 8,
        kind: watchpoint::Kind::Access,
    };
    watchpoint::arm(std::slice::from_ref(&request)).expect("a static word is watchable");

    // Written through the atomic, so the compiler cannot remove the store as unobservable.
    TARGET.store(0xDEAD_BEEF, core::sync::atomic::Ordering::SeqCst);

    let sites = watchpoint::sites();
    let report = sites.join("\n");

    // "never touched" is what a watchpoint that armed nothing produces, so it is the string
    // refused.
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
