//! End-to-end worker tests: a real child process, driven over real pipes.
//!
//! The protocol loop itself is unit-tested in `orbistoun-worker` over in-memory pipes.
//! These tests cover what that cannot: that the binary actually re-invokes itself, that
//! the handshake survives a process boundary, and that shutdown reaps the child.
//!
//! Kept separate on purpose - a protocol bug and a process-spawning bug should be
//! distinguishable failures rather than one confusing one.

use orbistoun_proto::{Event, Request};
use orbistoun_worker::WorkerHandle;

/// The binary under test, built by cargo for this integration test.
fn exe() -> std::path::PathBuf {
    env!("CARGO_BIN_EXE_orbistoun-cli").into()
}

#[test]
fn the_binary_re_invokes_itself_and_completes_a_handshake() {
    // Self-reinvocation (D033) is what makes version skew impossible: the worker is
    // literally this build, not a separately shipped one.
    let worker = WorkerHandle::spawn(&exe()).expect("spawn worker");
    worker.shutdown().expect("clean shutdown");
}

#[test]
fn a_survey_crosses_the_process_boundary_intact() {
    let mut worker = WorkerHandle::spawn(&exe()).expect("spawn worker");

    // A path that cannot parse: the interesting assertion is that the *failure* comes
    // back as a message rather than as a dead child.
    let events = worker
        .request(&Request::Survey {
            path: "definitely/not/a/container".into(),
        })
        .expect("request");

    assert!(
        matches!(events.last(), Some(Event::Failed { .. })),
        "got {events:?}"
    );

    // And the worker is still alive afterwards.
    let again = worker
        .request(&Request::Survey {
            path: "also/not/a/container".into(),
        })
        .expect("second request");
    assert!(matches!(again.last(), Some(Event::Failed { .. })));

    worker.shutdown().expect("clean shutdown");
}

#[test]
fn a_missing_guest_crosses_the_boundary_as_a_request_failure() {
    // `Failed` means the request was wrong; `Terminated` means a guest was loaded and
    // then stopped. Collapsing them would make "the path was a typo" and "the emulator
    // cannot go further" look identical to anything reading the stream.
    let mut worker = WorkerHandle::spawn(&exe()).expect("spawn worker");
    let events = worker
        .request(&Request::Run {
            symbols_db: None,
            limit_seconds: Some(5),
            call_budget: None,
            path: "no/such/guest".into(),
        })
        .expect("request");
    assert!(
        matches!(events.last(), Some(Event::Failed { .. })),
        "got {events:?}"
    );
    worker.shutdown().expect("clean shutdown");
}

#[test]
fn several_workers_can_run_at_once() {
    // The shim will eventually drive one worker per title. Nothing about the design
    // should make that exclusive, so assert it before something accidentally does.
    let a = WorkerHandle::spawn(&exe()).expect("spawn a");
    let b = WorkerHandle::spawn(&exe()).expect("spawn b");
    a.shutdown().expect("shutdown a");
    b.shutdown().expect("shutdown b");
}
