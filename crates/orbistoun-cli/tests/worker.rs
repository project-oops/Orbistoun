//! End-to-end worker tests: a real child process driven over real pipes.
//!
//! The protocol loop is unit-tested in `orbistoun-worker` over in-memory pipes. These tests cover
//! what that cannot: self-reinvocation, the handshake across a process boundary, and reaping the
//! child at shutdown.

use orbistoun_proto::{Event, Request};
use orbistoun_worker::WorkerHandle;

/// The binary under test, built by cargo for this integration test.
fn exe() -> std::path::PathBuf {
    env!("CARGO_BIN_EXE_orbistoun-cli").into()
}

/// The binary re-invokes itself as a worker and completes the handshake (D033).
#[test]
fn the_binary_re_invokes_itself_and_completes_a_handshake() {
    let worker = WorkerHandle::spawn(&exe()).expect("spawn worker");
    worker.shutdown().expect("clean shutdown");
}

/// A failed request comes back as a message, and the worker survives it.
#[test]
fn a_survey_crosses_the_process_boundary_intact() {
    let mut worker = WorkerHandle::spawn(&exe()).expect("spawn worker");

    // A path that cannot parse.
    let events = worker
        .request(&Request::Survey {
            path: "definitely/not/a/container".into(),
        })
        .expect("request");

    assert!(
        matches!(events.last(), Some(Event::Failed { .. })),
        "got {events:?}"
    );

    // The worker is still alive afterwards.
    let again = worker
        .request(&Request::Survey {
            path: "also/not/a/container".into(),
        })
        .expect("second request");
    assert!(matches!(again.last(), Some(Event::Failed { .. })));

    worker.shutdown().expect("clean shutdown");
}

/// A missing guest is reported as `Failed` (a bad request), not `Terminated` (a stopped guest).
#[test]
fn a_missing_guest_crosses_the_boundary_as_a_request_failure() {
    let mut worker = WorkerHandle::spawn(&exe()).expect("spawn worker");
    let events = worker
        .request(&Request::Run {
            symbols_db: None,
            limit_seconds: Some(5),
            call_budget: None,
            path: "no/such/guest".into(),
            input_script: None,
            capture_input: None,
            staged: false,
        })
        .expect("request");
    assert!(
        matches!(events.last(), Some(Event::Failed { .. })),
        "got {events:?}"
    );
    worker.shutdown().expect("clean shutdown");
}

/// Several workers run at once; nothing makes a worker exclusive.
#[test]
fn several_workers_can_run_at_once() {
    let a = WorkerHandle::spawn(&exe()).expect("spawn a");
    let b = WorkerHandle::spawn(&exe()).expect("spawn b");
    a.shutdown().expect("shutdown a");
    b.shutdown().expect("shutdown b");
}
