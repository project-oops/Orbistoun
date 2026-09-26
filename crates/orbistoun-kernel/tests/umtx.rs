//! `_umtx_op`, the FreeBSD futex, driven through the table a resolved import reaches.
//!
//! The op numbers are FreeBSD `sys/sys/umtx.h`'s. The word is a heap cell this test owns: guest
//! memory is host memory.

use orbistoun_core::GUEST_ARG_REGISTERS;
use orbistoun_core::GuestFn;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::Duration;

const UMTX_OP_WAIT: u64 = 2;
const UMTX_OP_WAKE: u64 = 3;
const UMTX_OP_WAIT_UINT: u64 = 11;
const UMTX_OP_WAIT_UINT_PRIVATE: u64 = 15;
const UMTX_OP_WAKE_PRIVATE: u64 = 16;
/// Priority-inheritance mutexes: not modelled.
const UMTX_OP_MUTEX_LOCK: u64 = 5;

/// Long enough that a wake genuinely had to travel between threads, short enough that a broken
/// test fails rather than hangs.
const PATIENCE: Duration = Duration::from_secs(5);

fn umtx_op(args: [u64; 5]) -> u64 {
    let function: GuestFn = orbistoun_kernel::implementations()
        .iter()
        .find(|(n, _)| *n == "_umtx_op")
        .map(|(_, f)| *f)
        .expect("_umtx_op is not implemented, so no guest can reach it");
    let mut regs = [0xDEAD_BEEF_DEAD_BEEF_u64; GUEST_ARG_REGISTERS];
    regs[..5].copy_from_slice(&args);
    function(&regs)
}

fn address_of<T>(cell: &T) -> u64 {
    std::ptr::from_ref(cell).expose_provenance() as u64
}

/// A word that already differs answers at once: the wake came first.
#[test]
fn a_wait_on_a_word_that_differs_returns_at_once() {
    let word = Box::new(AtomicU32::new(1));
    for op in [UMTX_OP_WAIT_UINT, UMTX_OP_WAIT_UINT_PRIVATE] {
        assert_eq!(umtx_op([address_of(&*word), op, 2, 0, 0]), 0);
    }
    let long = Box::new(AtomicU64::new(1));
    assert_eq!(umtx_op([address_of(&*long), UMTX_OP_WAIT, 2, 0, 0]), 0);
}

/// `WAIT_UINT` compares 32 bits: the neighbouring word does not take part.
#[test]
fn a_uint_wait_compares_only_its_own_word() {
    // Low word 2 (equal), high word 7: a 64-bit compare would see a mismatch and return at once.
    let pair = Box::new(AtomicU64::new(0x0000_0007_0000_0002));
    let at = address_of(&*pair);
    let (done, finished) = mpsc::channel();
    let waiter = std::thread::spawn(move || {
        done.send(umtx_op([at, UMTX_OP_WAIT_UINT, 2, 0, 0]))
            .unwrap();
    });
    assert!(
        finished.recv_timeout(Duration::from_millis(200)).is_err(),
        "an equal 32-bit word sleeps"
    );
    pair.store(0x0000_0007_0000_0000, Ordering::SeqCst);
    assert_eq!(umtx_op([at, UMTX_OP_WAKE, 1, 0, 0]), 0);
    assert_eq!(finished.recv_timeout(PATIENCE), Ok(0));
    waiter.join().unwrap();
}

/// A sleeping waiter leaves on a wake from another thread, private or not.
#[test]
fn a_wake_releases_a_sleeping_waiter() {
    for (wait, wake) in [
        (UMTX_OP_WAIT_UINT, UMTX_OP_WAKE),
        (UMTX_OP_WAIT_UINT_PRIVATE, UMTX_OP_WAKE_PRIVATE),
    ] {
        let word = Box::new(AtomicU32::new(2));
        let at = address_of(&*word);
        let (done, finished) = mpsc::channel();
        let waiter = std::thread::spawn(move || {
            done.send(umtx_op([at, wait, 2, 0, 0])).unwrap();
        });
        assert!(finished.recv_timeout(Duration::from_millis(200)).is_err());
        word.store(0, Ordering::SeqCst);
        assert_eq!(umtx_op([at, wake, i32::MAX as u64, 0, 0]), 0);
        assert_eq!(finished.recv_timeout(PATIENCE), Ok(0));
        waiter.join().unwrap();
    }
}

/// A wake with nobody asleep succeeds.
#[test]
fn a_wake_with_nobody_waiting_succeeds() {
    let word = Box::new(AtomicU32::new(0));
    assert_eq!(umtx_op([address_of(&*word), UMTX_OP_WAKE, 1, 0, 0]), 0);
}

/// What is not modelled is refused, not waited on or answered with success: a timed wait and an
/// op outside the wait/wake family.
#[test]
fn a_timed_wait_and_an_unmodelled_op_are_refused() {
    let unimplemented = u64::from(orbistoun_core::GuestError::Unimplemented.as_raw());
    let word = Box::new(AtomicU32::new(2));
    let timeout = Box::new([0_u64; 2]);
    assert_eq!(
        umtx_op([
            address_of(&*word),
            UMTX_OP_WAIT_UINT,
            2,
            16,
            address_of(&*timeout)
        ]),
        unimplemented
    );
    assert_eq!(
        umtx_op([address_of(&*word), UMTX_OP_MUTEX_LOCK, 0, 0, 0]),
        unimplemented
    );
}
